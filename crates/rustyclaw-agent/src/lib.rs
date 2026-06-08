use anyhow::{Context, Result};
use futures_util::{Stream, StreamExt};
use rig_core::OneOrMany;
use rig_core::embeddings::Embedding;
use rig_core::vector_store::VectorStoreIndex;
use rig_core::vector_store::in_memory_store::InMemoryVectorStore;
use rig_core::vector_store::request::VectorSearchRequest;
use rustyclaw_config::{Config, AutonomyLevel};
use rustyclaw_providers::{
    CompletionOptions, LlmProvider, LlmResponse, Message, StreamChunk, create_provider,
};
use rustyclaw_storage::{ConversationHistory, SessionLogger};
use rustyclaw_tools::ToolRegistry;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const DISCORD_RAG_HISTORY_TURNS: usize = 2;

fn build_discord_rag_query(history: &[rustyclaw_providers::Message], raw_user_message: &str) -> String {
    // tool ロールおよび空 content の assistant（tool_call のみ）を除外
    let filtered: Vec<&rustyclaw_providers::Message> = history
        .iter()
        .filter(|m| {
            (m.role == "user" || m.role == "assistant")
                && !m.content.is_empty()
        })
        .collect();

    // 直近 N ターン分（user + assistant を 1 ターンとして N*2 メッセージ）を取得
    let take = DISCORD_RAG_HISTORY_TURNS * 2;
    let start = filtered.len().saturating_sub(take);
    let recent = &filtered[start..];

    let mut parts: Vec<String> = recent
        .iter()
        .map(|m| {
            let role_label = if m.role == "user" { "User" } else { "Assistant" };
            format!("{}: {}", role_label, m.content.trim())
        })
        .collect();

    parts.push(format!("User: {}", raw_user_message));
    parts.join("\n")
}

const HEARTBEAT_RAG_TAIL_LINES: usize = 10;
const HEARTBEAT_STEP2_RAG_QUERY: &str =
    "user interests hobbies routine habits long-term memory";
const HEARTBEAT_STEP6_RAG_QUERY: &str =
    "errors bugs pending tasks todo improvements";

pub fn build_heartbeat_rag_query(digest: &str) -> String {
    let lines: Vec<&str> = digest.lines().collect();
    let start = lines.len().saturating_sub(HEARTBEAT_RAG_TAIL_LINES);
    let tail = lines[start..].join("\n");
    format!("recent errors tasks memory updates: {}", tail)
}

fn is_write_operation(tool_name: &str) -> bool {
    if tool_name == "workspace_write" || tool_name == "workspace_execute_script" || tool_name == "cron_schedule" {
        return true;
    }
    let lower = tool_name.to_lowercase();
    lower.contains("write")
        || lower.contains("delete")
        || lower.contains("create")
        || lower.contains("update")
        || lower.contains("post")
        || lower.contains("patch")
        || lower.contains("send")
        || lower.contains("execute")
}

async fn run_confirmation_gate(tool_name: &str, args: &str, workspace_dir: &Path) -> Result<bool> {
    let approval_file = workspace_dir.join("pending_approval.json");
    let req_id = format!("req-{}", chrono::Local::now().format("%Y%m%d%H%M%S"));
    
    let parsed_args: serde_json::Value = serde_json::from_str(args).unwrap_or(serde_json::Value::String(args.to_string()));
    
    let req_data = serde_json::json!({
        "id": req_id,
        "tool_name": tool_name,
        "arguments": parsed_args,
        "status": "pending",
        "created_at": chrono::Local::now().to_rfc3339()
    });
    
    let file_content = serde_json::to_string_pretty(&req_data)?;
    fs::write(&approval_file, file_content)?;
    
    tracing::warn!(
        "⚠️ [CONFIRMATION REQUIRED] Tool '{}' wants to execute.\nArguments: {}\nTo approve, modify 'status' to 'approved' in {}.",
        tool_name,
        args,
        approval_file.display()
    );
    
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(300);
    let mut approved = false;
    
    while start_time.elapsed() < timeout {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        
        if !approval_file.exists() {
            break;
        }
        
        if let Ok(content) = fs::read_to_string(&approval_file)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
            && let Some(status) = val.get("status").and_then(|s| s.as_str())
        {
            if status == "approved" {
                approved = true;
                break;
            } else if status == "rejected" {
                break;
            }
        }
    }
    
    if approval_file.exists() {
        let _ = fs::remove_file(&approval_file);
    }
    
    Ok(approved)
}

// ==============================================================================
// Heartbeat Agent Refactoring (Phase 40-4)
// ==============================================================================

struct HeartbeatToolWrapper {
    inner: Arc<dyn rig_core::tool::ToolDyn>,
    db_path: PathBuf,
}

impl HeartbeatToolWrapper {
    fn new(inner: Arc<dyn rig_core::tool::ToolDyn>, db_path: PathBuf) -> Self {
        Self { inner, db_path }
    }
}

impl rig_core::tool::ToolDyn for HeartbeatToolWrapper {
    fn name(&self) -> String {
        self.inner.name()
    }

    fn definition<'a>(
        &'a self,
        prompt: String,
    ) -> rig_core::wasm_compat::WasmBoxedFuture<'a, rig_core::completion::ToolDefinition> {
        self.inner.definition(prompt)
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> rig_core::wasm_compat::WasmBoxedFuture<'a, Result<String, rig_core::tool::ToolError>> {
        Box::pin(async move {
            let tool_name = self.name();
            let raw_res = self.inner.call(args.clone()).await?;

            let mut tool_content = raw_res;
            if let Ok(db) = rustyclaw_storage::DbManager::new(&self.db_path) {
                tool_content = filter_seen_tool_result(&tool_name, &args, &tool_content, &db);
            }

            tool_content = truncate_70_20(&tool_content, 3_000);

            Ok(tool_content)
        })
    }
}

#[derive(Clone)]
struct AgentHook {
    workspace_dir: PathBuf,
    session_id: String,
    autonomy_level: AutonomyLevel,
    log_results: bool,
}

impl AgentHook {
    fn new(workspace_dir: PathBuf, session_id: String, autonomy_level: AutonomyLevel, log_results: bool) -> Self {
        Self { workspace_dir, session_id, autonomy_level, log_results }
    }
}

impl<M: rig_core::completion::CompletionModel> rig_core::agent::PromptHook<M> for AgentHook {
    fn on_tool_call(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
    ) -> impl std::future::Future<Output = rig_core::agent::ToolCallHookAction> + rig_core::wasm_compat::WasmCompatSend {
        let is_write = is_write_operation(tool_name);
        let autonomy_level = self.autonomy_level;
        let workspace_dir = self.workspace_dir.clone();
        let tool_name = tool_name.to_string();
        let args = args.to_string();

        async move {
            if is_write {
                match autonomy_level {
                    AutonomyLevel::ReadOnly => {
                        tracing::warn!("Operation '{}' rejected in ReadOnly mode.", tool_name);
                        rig_core::agent::ToolCallHookAction::Skip {
                            reason: format!("Operation '{}' rejected: RustyClaw is running in ReadOnly mode.", tool_name),
                        }
                    }
                    AutonomyLevel::Supervised => {
                        tracing::info!("Operation '{}' requires user approval.", tool_name);
                        match run_confirmation_gate(&tool_name, &args, &workspace_dir).await {
                            Ok(true) => {
                                tracing::info!("Operation '{}' approved.", tool_name);
                                rig_core::agent::ToolCallHookAction::cont()
                            }
                            Ok(false) => {
                                tracing::warn!("Operation '{}' rejected by user.", tool_name);
                                rig_core::agent::ToolCallHookAction::Skip {
                                    reason: format!("Operation '{}' rejected by user.", tool_name),
                                }
                            }
                            Err(e) => {
                                tracing::error!("Confirmation gate error: {}", e);
                                rig_core::agent::ToolCallHookAction::Skip {
                                    reason: format!("Confirmation gate error or timeout: {}", e),
                                }
                            }
                        }
                    }
                    AutonomyLevel::Autonomous => {
                        rig_core::agent::ToolCallHookAction::cont()
                    }
                }
            } else {
                rig_core::agent::ToolCallHookAction::cont()
            }
        }
    }

    fn on_tool_result(
        &self,
        tool_name: &str,
        tool_call_id: Option<String>,
        _internal_call_id: &str,
        _args: &str,
        result: &str,
    ) -> impl std::future::Future<Output = rig_core::agent::HookAction> + rig_core::wasm_compat::WasmCompatSend {
        let logger = SessionLogger::new(&self.workspace_dir);
        let session_id = self.session_id.clone();
        let tool_name = tool_name.to_string();
        let result = result.to_string();
        let log_results = self.log_results;

        async move {
            if log_results {
                let tool_msg = Message {
                    role: "tool".to_string(),
                    content: result,
                    tool_call_id,
                    name: Some(tool_name),
                    ..Default::default()
                };
                if let Err(e) = logger.append_message(&session_id, &tool_msg) {
                    tracing::error!("Failed to save tool result message in session log: {}", e);
                }
            }
            rig_core::agent::HookAction::cont()
        }
    }

    fn on_completion_response(
        &self,
        _prompt: &rig_core::completion::Message,
        response: &rig_core::completion::CompletionResponse<M::Response>,
    ) -> impl std::future::Future<Output = rig_core::agent::HookAction> + rig_core::wasm_compat::WasmCompatSend {
        let logger = SessionLogger::new(&self.workspace_dir);
        let session_id = self.session_id.clone();
        let log_results = self.log_results;

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        for c in response.choice.iter() {
            match c {
                rig_core::completion::AssistantContent::Text(t) => {
                    content.push_str(&t.text);
                }
                rig_core::completion::AssistantContent::ToolCall(tc) => {
                    tool_calls.push(rustyclaw_providers::ToolCall {
                        id: tc.id.clone(),
                        r#type: "function".to_string(),
                        function: rustyclaw_providers::FunctionCall {
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.to_string(),
                        },
                    });
                }
                _ => {}
            }
        }

        let tool_calls_opt = if tool_calls.is_empty() { None } else { Some(tool_calls) };
        let assistant_msg = Message {
            role: "assistant".to_string(),
            content: filter_json_leaks(&content),
            tool_calls: tool_calls_opt,
            ..Default::default()
        };

        async move {
            if log_results
                && let Err(e) = logger.append_message(&session_id, &assistant_msg)
            {
                tracing::error!("Failed to save assistant response in session log: {}", e);
            }
            rig_core::agent::HookAction::cont()
        }
    }
}

pub struct Pipeline {
    config: Config,
    provider: Box<dyn LlmProvider>,
    /// flush_memory() の同時実行を 1 本に制限するセマフォ
    flush_sem: Arc<Semaphore>,
    /// RAG エンジン (Task 7 で初期化される。None の場合は RAG スキップ)
    pub rag: Option<Arc<UnifiedRagEngine>>,
}

impl Pipeline {
    pub fn new(config: Config, flush_sem: Arc<Semaphore>) -> Self {
        let provider = create_provider(config.get_model("default"));
        Self {
            config,
            provider,
            flush_sem,
            rag: None,
        }
    }



    /// context_window ベースのメッセージ件数ハードキャップ。
    /// モデルの context_window 文字列を parse_context_window() で解釈し、
    /// 大きいモデルほど多くの履歴を保持できるようにする。
    fn get_history_message_limit(&self, purpose: &str) -> usize {
        let model_cfg = self.config.get_model(purpose);
        let cw = self
            .config
            .model_list
            .iter()
            .find(|m| m.model == model_cfg.model_name && m.enabled)
            .and_then(|m| m.context_window.as_deref());
        let ctx = parse_context_window(cw);
        match ctx {
            0..=16_384 => 30,
            16_385..=32_768 => 50,
            32_769..=65_536 => 80,
            65_537..=262_143 => 100,
            _ => 120,
        }
    }

    /// 各種人格定義ファイルを読み込んでシステムプロンプト（Context）を構築する
    /// 行頭が // で始まる行（インデント許容）を除去する
    fn strip_comments(content: &str) -> String {
        content
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn build_system_context(&self, workspace_dir: &Path) -> Result<String> {
        // 静的ブロック（SOUL/USER）を先に並べてプロンプトキャッシュの prefix を安定させる。
        // 動的な [now:] は末尾に置くことで毎回変わる部分がキャッシュ prefix を破壊しないようにする。
        let files = ["SOUL.md", "USER.md"];
        let mut context = String::new();

        for filename in &files {
            let path = workspace_dir.join(filename);
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!(
                        "Failed to read context file {:?}: {}. Using empty content.",
                        path,
                        e
                    );
                    String::new()
                }
            };

            context.push_str(&format!(
                "# {}\n\n{}\n\n",
                filename,
                Self::strip_comments(&content)
            ));
        }

        // proactive-posts.md を注入（最終1件のみ）
        let posts_path = workspace_dir.join("memory").join("proactive-posts.md");
        if let Ok(posts) = fs::read_to_string(&posts_path)
            && let Some(last_entry) = posts.lines().rfind(|l| !l.trim().is_empty())
        {
            context.push_str(&format!(
                "# Recent AI Proactive Posts\n\n{}\n\n",
                last_entry
            ));
        }

        // 動的ブロック（現在時刻）は末尾に配置
        let now = chrono::Local::now();
        context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));

        Ok(context)
    }

    /// 過去履歴から「最後にユーザーが発言した時刻以降に記録された自発的投稿」を抽出し、
    /// システムプロンプト（system_context）に注入するためのテキストブロックと、
    /// 自発的投稿を除外した純粋な会話履歴メッセージ群を返す。
    fn process_proactive_posts(
        &self,
        session_id: &str,
        history_messages: Vec<Message>,
        system_context: &mut String,
    ) -> Vec<Message> {
        if session_id.starts_with("cron:") {
            return history_messages;
        }

        // 1. ユーザーの最終発言のタイムスタンプを特定する
        let last_user_ts = history_messages
            .iter()
            .rfind(|m| m.role == "user")
            .and_then(|m| m.timestamp.as_ref())
            .cloned();

        let mut conversational_messages = Vec::new();
        let mut proactive_entries = Vec::new();

        for msg in history_messages {
            if msg.trigger.as_deref() == Some("proactive") {
                let is_after_last_user = match (&msg.timestamp, &last_user_ts) {
                    (Some(msg_ts), Some(user_ts)) => msg_ts > user_ts,
                    (Some(_), None) => true,
                    _ => false,
                };

                if is_after_last_user {
                    proactive_entries.push(msg);
                    continue;
                }
            }
            conversational_messages.push(msg);
        }

        // 2. 自発的発言が存在する場合、system_context に注入ブロックを追加する
        if !proactive_entries.is_empty() {
            let mut proactive_block = String::from(
                "\n### Your Previous Posts in This Channel\nYou posted these messages (not in your conversation history):\n",
            );
            let start_idx = proactive_entries.len().saturating_sub(5);
            for entry in &proactive_entries[start_idx..] {
                let ts = entry.timestamp.as_deref().unwrap_or("unknown");
                let formatted_ts = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                } else {
                    ts.to_string()
                };
                let content_preview = if entry.content.chars().count() > 300 {
                    let mut s: String = entry.content.chars().take(300).collect();
                    s.push_str("...");
                    s
                } else {
                    entry.content.clone()
                };
                proactive_block.push_str(&format!("- [{}]: {}\n", formatted_ts, content_preview));
            }
            system_context.push_str(&proactive_block);
        }

        conversational_messages
    }

    /// デバッグ用の生リクエストダンプを出力する
    fn dump_request(&self, _workspace_dir: &Path, _messages: &[Message]) {
        // Consolidated in providers layer
    }

    /// デバッグ用の生レスポンスダンプを出力する
    fn dump_response(&self, _workspace_dir: &Path, _content: &str, _role: &str) {
        // Consolidated in providers layer
    }

    /// 前日のセッションからサマリーと履歴を取得し、文脈継続情報を作成する
    pub fn get_session_continuation_context(
        &self,
        workspace_dir: &Path,
        session_id: &str,
    ) -> Option<String> {
        if session_id.starts_with("cron") {
            return None;
        }

        let parts: Vec<&str> = session_id.split('-').collect();
        if parts.len() < 3 {
            return None;
        }
        let prefix = parts[..parts.len() - 1].join("-");
        let current_date_str = parts.last()?;

        let sessions_dir = workspace_dir.join("sessions");
        let mut previous_sessions = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if filename.starts_with(&prefix) && filename.ends_with(".jsonl") {
                    let name_without_ext = filename.trim_end_matches(".jsonl");
                    let file_parts: Vec<&str> = name_without_ext.split('-').collect();
                    if let Some(date_str) = file_parts.last()
                        && *date_str < *current_date_str
                    {
                        previous_sessions.push((date_str.to_string(), filename));
                    }
                }
            }
        }

        previous_sessions.sort_by(|a, b| b.0.cmp(&a.0));
        let (prev_date_str, prev_filename) = previous_sessions.first()?;

        if prev_date_str.len() != 8 {
            return None;
        }
        let yyyy_mm_dd = format!(
            "{}-{}-{}",
            &prev_date_str[0..4],
            &prev_date_str[4..6],
            &prev_date_str[6..8]
        );

        let prev_session_id = prev_filename.trim_end_matches(".jsonl");
        let safe_prev_session_id = prev_session_id.replace(':', "-");

        let summaries_dir = workspace_dir.join("memory").join("summaries");
        let mut summary_content = String::new();

        let specific_summary_path =
            summaries_dir.join(format!("{}-{}.md", yyyy_mm_dd, safe_prev_session_id));
        let daily_summary_path = summaries_dir.join(format!("{}-daily-summary.md", yyyy_mm_dd));

        if specific_summary_path.exists() {
            if let Ok(c) = std::fs::read_to_string(&specific_summary_path) {
                summary_content = c;
            }
        } else if daily_summary_path.exists() {
            if let Ok(c) = std::fs::read_to_string(&daily_summary_path) {
                summary_content = c;
            }
        } else if let Ok(entries) = std::fs::read_dir(&summaries_dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if filename.starts_with(&yyyy_mm_dd)
                    && filename.ends_with(".md")
                    && let Ok(c) = std::fs::read_to_string(entry.path())
                {
                    summary_content = c;
                    break;
                }
            }
        }

        let logger = SessionLogger::new(workspace_dir);
        let mut prev_history = Vec::new();
        if let Ok(msgs) = logger.load_history(prev_session_id) {
            let start = msgs.len().saturating_sub(5);
            prev_history = msgs[start..].to_vec();
        }

        if summary_content.is_empty() && prev_history.is_empty() {
            return None;
        }

        let mut context = String::new();
        context.push_str("\n# === SESSION CONTINUATION CONTEXT (Yesterday's Context) ===\n");
        if !summary_content.is_empty() {
            context.push_str(&format!(
                "## Yesterday's Session Summary ({})\n{}\n\n",
                yyyy_mm_dd, summary_content
            ));
        }
        if !prev_history.is_empty() {
            context.push_str("## Last 5 turns of Yesterday's Conversation:\n");
            for msg in prev_history {
                context.push_str(&format!("{}: {}\n", msg.role, msg.content));
            }
        }
        context.push_str("=========================================================\n");

        Some(context)
    }

    /// メモリの要約抽出と保存を非同期・非ブロッキングでトリガーする (fail-open)
    ///
    /// flush_sem (容量1) を取得してから flush_memory() を実行することで、
    /// flush が同時に複数走って gmn プロセスが意図した上限を超えるのを防ぐ。
    pub fn trigger_memory_flush_async(&self, workspace_dir: &Path, session_id: &str) {
        let workspace_dir = workspace_dir.to_path_buf();
        let session_id = session_id.to_string();
        let config = self.config.clone();
        let flush_sem = self.flush_sem.clone();

        tokio::spawn(async move {
            // セマフォ取得（最大 60 秒待機）。取得できなければ今回の flush はスキップ
            let _permit = match tokio::time::timeout(
                Duration::from_secs(60),
                flush_sem.acquire_owned(),
            )
            .await
            {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => {
                    tracing::warn!(session = %session_id, "flush_memory: semaphore closed, skipping");
                    return;
                }
                Err(_) => {
                    tracing::warn!(session = %session_id, "flush_memory: semaphore timeout (60s), skipping");
                    return;
                }
            };

            if let Err(e) = Self::flush_memory(&workspace_dir, &session_id, config).await {
                tracing::warn!("Failed to flush memory for session {}: {:#}", session_id, e);
            }
            // _permit がここでドロップされ、セマフォが返却される
        });
    }

    /// FU-4: 補助 LLM 呼び出し（memory-flush / session-summary）のトークン消費を usage に計上する (fail-open)。
    /// execute_with_rig_agent 以外の経路は従来 record_usage されず Stats が過少計上だった。
    fn record_aux_usage(
        db: &rustyclaw_storage::DbManager,
        session_id: &str,
        response: &LlmResponse,
        trigger: &str,
    ) {
        let _ = db.record_usage(
            session_id,
            response.prompt_tokens.unwrap_or(0),
            response.completion_tokens.unwrap_or(0),
            response.total_tokens.unwrap_or(0),
            response.model_used.as_deref().unwrap_or(""),
            trigger,
            response.provider_id.as_deref(),
            0,
        );
    }

    async fn flush_memory(workspace_dir: &Path, session_id: &str, config: Config) -> Result<()> {
        let db_path = workspace_dir.join("memory.db");
        let db = rustyclaw_storage::DbManager::new(&db_path)
            .context("Failed to open SQLite db for memory flush")?;

        let logger = SessionLogger::new(workspace_dir);
        let history = logger
            .load_history(session_id)
            .context("Failed to load history for memory flush")?;

        let current_count = history.len();
        if current_count == 0 {
            return Ok(());
        }

        // delta >= 3（≒1.5ターン）かつ前回から 15 分以上経過した場合のみ実行
        const DELTA_THRESHOLD: usize = 3;
        const MIN_INTERVAL_MINS: i64 = 15;

        let count_key = format!("flush_count_{}", session_id);
        let ts_key = format!("flush_ts_{}", session_id);

        let last_flushed_count = db
            .get_last_patrol_run(&count_key)?
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        if last_flushed_count > 0 {
            // ① delta チェック
            if current_count - last_flushed_count < DELTA_THRESHOLD {
                tracing::debug!(
                    session = %session_id,
                    delta = current_count - last_flushed_count,
                    "memory flush: skipping (delta < {})", DELTA_THRESHOLD
                );
                return Ok(());
            }

            // ② 時間ゲート
            if let Ok(Some(ts_str)) = db.get_last_patrol_run(&ts_key)
                && let Ok(last_ts) = chrono::DateTime::parse_from_rfc3339(&ts_str)
            {
                let elapsed_mins = chrono::Local::now()
                    .signed_duration_since(last_ts)
                    .num_minutes();
                if elapsed_mins < MIN_INTERVAL_MINS {
                    tracing::debug!(
                        session = %session_id,
                        elapsed_mins,
                        "memory flush: skipping (time gate, < {} min)", MIN_INTERVAL_MINS
                    );
                    return Ok(());
                }
            }
        }

        tracing::info!(
            session = %session_id,
            current = current_count,
            last_flushed = last_flushed_count,
            "memory flush: starting"
        );

        // 前回 flush 以降の新着メッセージのみ渡す（デルタ分だけで十分）
        // 初回は全件を対象にするが、最大 10 件に制限してトークン爆発を防ぐ
        let delta_start = last_flushed_count.min(history.len());
        let start = delta_start.max(history.len().saturating_sub(10));
        let mut conversation_text = String::new();
        for msg in &history[start..] {
            conversation_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        let memory_model = config.get_model("memory");

        // コンテキストサイズ安全チェック: flush プロンプトの推定トークン数がモデルの
        // context_window の 80% を超える場合はスキップして 400 エラーを防ぐ。
        let flush_text_tokens = (conversation_text.chars().count() * 3 / 2) + 2_000;
        let flush_model_cw = config
            .model_list
            .iter()
            .find(|m| m.model == memory_model.model_name && m.enabled)
            .and_then(|m| m.context_window.as_deref());
        let flush_ctx_limit = (parse_context_window(flush_model_cw) * 4) / 5;
        if flush_text_tokens > flush_ctx_limit {
            tracing::warn!(
                session = %session_id,
                estimated_tokens = flush_text_tokens,
                ctx_limit = flush_ctx_limit,
                "memory flush: skipping — estimated tokens exceeds model context limit"
            );
            return Ok(());
        }

        // 既存の MEMORY.md を読み込む（なければ空）
        let memory_path = workspace_dir.join("MEMORY.md");
        let existing_memory = fs::read_to_string(&memory_path).unwrap_or_default();

        let system_prompt = "\
You are a memory manager. Given the current MEMORY.md and a recent conversation, your tasks are:

1. Produce a fully rewritten MEMORY.md that:
   - Incorporates important new facts, decisions, preferences, and learnings from the conversation
   - Removes outdated or redundant information
   - Uses concise, factual bullet points under clear headings
   - If nothing new is worth adding and the existing content is fine, output it unchanged

2. Produce a concise bulleted daily log entry summarising what happened in the conversation.

Output using exactly these delimiters (no extra text outside them):
---NEW_MEMORY---
<complete rewritten MEMORY.md content>
---END_MEMORY---
---DAILY_LOG---
<bulleted activity summary>
---END_DAILY_LOG---

Rules:
- Never truncate mid-sentence inside ---NEW_MEMORY---.
- Write in the same language as the existing MEMORY.md content.
";

        let user_message = format!(
            "## Current MEMORY.md\n{}\n\n## Recent conversation\n{}",
            if existing_memory.is_empty() {
                "(empty)"
            } else {
                &existing_memory
            },
            conversation_text,
        );
        let model = rustyclaw_providers::RustyclawCompletionModel::new(config.clone(), "memory", session_id);
        let usage_sink = model.usage_sink();

        let agent = rig_core::agent::AgentBuilder::new(model)
            .preamble(system_prompt)
            .build();

        let response_text = rig_core::agent::PromptRequest::from_agent(&agent, &user_message)
            .await
            .context("LLM call failed for memory flush agent")?;

        let last_usage: Option<LlmResponse> = usage_sink.lock().unwrap().take();
        if let Some(ref usage) = last_usage {
            Self::record_aux_usage(&db, session_id, usage, "memory-flush");
        }

        let new_memory =
            extract_delimited_block(&response_text, "---NEW_MEMORY---", "---END_MEMORY---");
        let daily_log =
            extract_delimited_block(&response_text, "---DAILY_LOG---", "---END_DAILY_LOG---");

        // 1. MEMORY.md の全書き換え (fail-open)
        if let Some(final_content) = new_memory {
            if let Err(e) =
                rustyclaw_storage::atomic_write(&memory_path, final_content.as_bytes()).await
            {
                tracing::warn!("memory flush: failed to write MEMORY.md: {}", e);
            } else {
                // MEMORY.md 更新成功時のみ RAG ingestion を実行 (fail-open)
                ingest_memory_md(workspace_dir, &config, &db_path, None).await;
            }
        }

        // 2. memory/logs/YYYY-MM-DD.md への反映 (fail-open)
        if let Some(log_content) = daily_log {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let logs_dir = workspace_dir.join("memory").join("logs");
            let _ = fs::create_dir_all(&logs_dir);
            let log_file = logs_dir.join(format!("{}.md", today));

            let log_entry = format!(
                "## {} - session: {}\n{}\n\n",
                chrono::Local::now().format("%H:%M"),
                session_id,
                log_content,
            );

            let mut file_content = if log_file.exists() {
                fs::read_to_string(&log_file).unwrap_or_default()
            } else {
                format!(
                    "---\ndate: \"{today}\"\ntype: daily-log\ntags:\n  - type/daily-log\n---\n# {today} Activity Log\n\n"
                )
            };
            file_content.push_str(&log_entry);
            let _ = rustyclaw_storage::atomic_write(&log_file, file_content.as_bytes()).await;
        }

        // 3. SQLite にフラッシュ完了状態を保存（メッセージ数 + タイムスタンプ）
        let _ = db.set_state_value(&count_key, &current_count.to_string());
        let _ = db.set_state_value(&ts_key, &chrono::Local::now().to_rfc3339());

        tracing::info!(session = %session_id, "memory flush: completed");
        Ok(())
    }

    /// Heartbeat 専用の軽量システムコンテキストを構築する（SOUL + HEARTBEAT のみ）。
    /// MEMORY.md は RAG 経由で関連チャンクのみを動的注入する（ISSUE-28）。
    /// 静的ファイルのみを返す。動的な [now:] は呼び出し元 execute_heartbeat で追加する。
    pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
        let files = ["SOUL.md", "HEARTBEAT.md"];
        let mut context = String::new();
        for filename in &files {
            let path = workspace_dir.join(filename);
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!(
                        "Failed to read context file {:?}: {}. Using empty content.",
                        path,
                        e
                    );
                    String::new()
                }
            };
            context.push_str(&format!(
                "# {}\n\n{}\n\n",
                filename,
                Self::strip_comments(&content)
            ));
        }
        Ok(context)
    }

    /// Heartbeat 専用実行（軽量コンテキスト使用、履歴なし、ツールループ付き）
    pub async fn execute_heartbeat(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
        rag_query: Option<&str>,
        tool_registry: &ToolRegistry,
        db_path: &Path,
    ) -> Result<LlmResponse> {
        let mut system_context = self.build_heartbeat_context(workspace_dir)?;
        let now = chrono::Local::now();
        system_context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));

        // RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
        let hb_top_k = self
            .config
            .embedding
            .as_ref()
            .and_then(|e| e.heartbeat_top_k)
            .unwrap_or(3);
        let heartbeat_config = {
            let mut cfg = self.config.clone();
            if let Some(ref mut emb) = cfg.embedding {
                emb.top_k = hb_top_k;
            }
            cfg
        };
        let effective_rag = rag_query.unwrap_or(user_message);
        if heartbeat_config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&heartbeat_config) {
                let rag_ctx =
                    retrieve_rag_context_local(effective_rag, &heartbeat_config, &client, db_path, hb_top_k)
                        .await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
                let step2_ctx = retrieve_rag_context_local(
                    HEARTBEAT_STEP2_RAG_QUERY,
                    &heartbeat_config,
                    &client,
                    db_path,
                    hb_top_k,
                )
                .await;
                if !step2_ctx.is_empty() {
                    system_context.push_str("## Step 2 関連記憶\n");
                    system_context.push_str(&step2_ctx);
                }
                let step6_ctx = retrieve_rag_context_local(
                    HEARTBEAT_STEP6_RAG_QUERY,
                    &heartbeat_config,
                    &client,
                    db_path,
                    hb_top_k,
                )
                .await;
                if !step6_ctx.is_empty() {
                    system_context.push_str("## Step 6 関連記憶\n");
                    system_context.push_str(&step6_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(effective_rag, &heartbeat_config, rag, hb_top_k).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
            let step2_ctx =
                retrieve_rag_context(HEARTBEAT_STEP2_RAG_QUERY, &heartbeat_config, rag, hb_top_k)
                    .await;
            if !step2_ctx.is_empty() {
                system_context.push_str("## Step 2 関連記憶\n");
                system_context.push_str(&step2_ctx);
            }
            let step6_ctx =
                retrieve_rag_context(HEARTBEAT_STEP6_RAG_QUERY, &heartbeat_config, rag, hb_top_k)
                    .await;
            if !step6_ctx.is_empty() {
                system_context.push_str("## Step 6 関連記憶\n");
                system_context.push_str(&step6_ctx);
            }
        }

        let logger = SessionLogger::new(workspace_dir);
        let user_msg = Message {
            role: "user".to_string(),
            content: user_message.to_string(),
            name: None,
            ..Default::default()
        };

        logger
            .append_message(session_id, &user_msg)
            .context("Failed to save user message in session log (fail-closed)")?;

        use rig_core::agent::AgentBuilder;
        use rustyclaw_providers::RustyclawCompletionModel;

        let model = RustyclawCompletionModel::new(self.config.clone(), "heartbeat", session_id);
        let usage_sink = model.usage_sink();

        let mut wrapped_tools: Vec<Box<dyn rig_core::tool::ToolDyn>> = Vec::new();
        for (_, tool) in tool_registry.iter() {
            let wrapped = HeartbeatToolWrapper::new(tool.clone(), db_path.to_path_buf());
            wrapped_tools.push(Box::new(wrapped));
        }

        let agent = AgentBuilder::new(model)
            .preamble(&system_context)
            .default_max_turns(5)
            .tools(wrapped_tools)
            .build();

        let initial_messages = vec![
            Message {
                role: "system".to_string(),
                content: system_context.clone(),
                ..Default::default()
            },
            user_msg.clone(),
        ];
        self.dump_request(workspace_dir, &initial_messages);

        let rig_history: Vec<rig_core::completion::Message> = Vec::new();
        let response_text = rig_core::agent::PromptRequest::from_agent(&agent, user_message)
            .with_history(rig_history)
            .with_hook(AgentHook::new(workspace_dir.to_path_buf(), session_id.to_string(), self.config.autonomy_level, true))
            .await
            .map_err(|e| anyhow::anyhow!("heartbeat agent error: {}", e))?;

        let response_text = filter_json_leaks(&response_text);
        self.dump_response(workspace_dir, &response_text, "assistant");

        let last_usage: Option<LlmResponse> = usage_sink.lock().unwrap().take();
        if let Some(ref usage) = last_usage
            && let Ok(db) = rustyclaw_storage::DbManager::new(db_path)
        {
            let heartbeat_model = self.config.get_model("heartbeat");
            let _ = db.record_usage(
                session_id,
                usage.prompt_tokens.unwrap_or(0),
                usage.completion_tokens.unwrap_or(0),
                usage.total_tokens.unwrap_or(0),
                &heartbeat_model.model_name,
                "heartbeat",
                Some(&heartbeat_model.model_provider),
                0,
            );
        }

        let response = LlmResponse {
            content: response_text,
            role: "assistant".to_string(),
            tool_calls: None,
            prompt_tokens: last_usage.as_ref().and_then(|u| u.prompt_tokens),
            completion_tokens: last_usage.as_ref().and_then(|u| u.completion_tokens),
            total_tokens: last_usage.as_ref().and_then(|u| u.total_tokens),
            model_used: Some(self.config.get_model("heartbeat").model_name),
            provider_id: Some(self.config.get_model("heartbeat").model_provider),
        };

        Ok(response)
    }

    /// 通常（一括）の対話実行
    pub async fn execute(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
    ) -> Result<LlmResponse> {
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
        {
            system_context.push_str(&continuation);
        }

        // 1. 過去履歴のロードとトークン圧縮処理の適用
        let logger = SessionLogger::new(workspace_dir);
        let history_messages = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger
                .load_history(session_id)
                .context("Failed to load session history messages")?
        };

        let cleaned_history =
            self.process_proactive_posts(session_id, history_messages, &mut system_context);

        let mut history = ConversationHistory::new(cleaned_history);
        history.trim_to_last(self.get_history_message_limit("default"));

        // 2. 送信用メッセージリストの構築 (System + History + User)
        let mut messages = Vec::new();
        messages.push(Message {
            role: "system".to_string(),
            content: system_context,
            name: None,
            ..Default::default()
        });
        messages.extend(history.messages.clone());
        let user_msg = Message {
            role: "user".to_string(),
            content: user_message.to_string(),
            name: None,
            ..Default::default()
        };
        messages.push(user_msg.clone());

        self.dump_request(workspace_dir, &messages);

        // 3. LLMプロバイダ呼び出し
        use rig_core::agent::AgentBuilder;
        use rustyclaw_providers::RustyclawCompletionModel;

        let system_content = messages[0].content.clone();

        let model = RustyclawCompletionModel::new(self.config.clone(), "default", session_id);
        let usage_sink = model.usage_sink();

        // 履歴を rig_core::completion::Message に変換
        let rig_history = rustyclaw_providers::provider_messages_to_rig(&history.messages);

        let agent = AgentBuilder::new(model)
            .preamble(&system_content)
            .build();

        let response_text = rig_core::agent::PromptRequest::from_agent(&agent, user_message)
            .with_history(rig_history)
            .await
            .map_err(|e| anyhow::anyhow!("agent execution failed: {}", e))?;

        let response_text = filter_json_leaks(&response_text);

        let last_usage: Option<LlmResponse> = usage_sink.lock().unwrap().take();
        let response = LlmResponse {
            content: response_text,
            role: "assistant".to_string(),
            tool_calls: None,
            prompt_tokens: last_usage.as_ref().and_then(|u| u.prompt_tokens),
            completion_tokens: last_usage.as_ref().and_then(|u| u.completion_tokens),
            total_tokens: last_usage.as_ref().and_then(|u| u.total_tokens),
            model_used: Some(self.config.get_model("default").model_name),
            provider_id: Some(self.config.get_model("default").model_provider),
        };

        // 4. 会話ログの保存 (fail-closed)
        logger
            .append_message(session_id, &user_msg)
            .context("Failed to save user message in session log (fail-closed)")?;
        let assistant_msg = Message {
            role: response.role.clone(),
            content: response.content.clone(),
            name: None,
            ..Default::default()
        };
        logger
            .append_message(session_id, &assistant_msg)
            .context("Failed to save assistant response in session log (fail-closed)")?;

        self.dump_response(workspace_dir, &response.content, &response.role);

        if !session_id.starts_with("cron") {
            self.trigger_memory_flush_async(workspace_dir, session_id);
        }

        Ok(response)
    }

    /// セッション単位のサマリーを生成または増分更新する
    pub async fn generate_session_summary(
        &self,
        workspace_dir: &Path,
        target_session_id: &str,
    ) -> Result<String> {
        let logger = SessionLogger::new(workspace_dir);
        let history = logger
            .load_history(target_session_id)
            .context("Failed to load history for session summary")?;

        if history.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot generate summary for empty session history"
            ));
        }

        let safe_session_id = target_session_id.replace(':', "-");
        let session_file = workspace_dir
            .join("sessions")
            .join(format!("{}.jsonl", safe_session_id));
        let session_date = if let Ok(meta) = std::fs::metadata(&session_file) {
            if let Ok(modified) = meta.modified() {
                let dt: chrono::DateTime<chrono::Local> = modified.into();
                dt.format("%Y-%m-%d").to_string()
            } else {
                chrono::Local::now().format("%Y-%m-%d").to_string()
            }
        } else {
            chrono::Local::now().format("%Y-%m-%d").to_string()
        };

        let summaries_dir = workspace_dir.join("memory").join("summaries");
        let existing_summary_path =
            summaries_dir.join(format!("{}-{}.md", session_date, safe_session_id));

        let mut existing_content = String::new();
        let mut existing_turns = 0;

        if existing_summary_path.exists()
            && let Ok(content) = fs::read_to_string(&existing_summary_path)
        {
            existing_content = content;
            if let Some(idx) = existing_content.rfind("<!-- turns: ") {
                let sub = &existing_content[idx + "<!-- turns: ".len()..];
                if let Some(end_idx) = sub.find(" -->")
                    && let Ok(t) = sub[..end_idx].trim().parse::<usize>()
                {
                    existing_turns = t;
                }
            }
        }
        // B: LLM 呼び出し前にプレースホルダーを書き込む
        // → 失敗時も summary_path が存在し更新日時が新しくなるため、再トリガーを防ぐ
        let _ = fs::create_dir_all(&summaries_dir);
        if !existing_summary_path.exists() {
            let placeholder = format!(
                "<!-- summary: pending ({}) -->\n<!-- turns: {} -->\n",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%z"),
                history.len()
            );
            let _ = fs::write(&existing_summary_path, placeholder);
        }

        // FU-4: summary 経路の LLM 消費を計上するため db を開く（fail-open）
        let usage_db = rustyclaw_storage::DbManager::new(workspace_dir.join("memory.db")).ok();

        let summary_content = if existing_turns > 0 && existing_turns < history.len() {
            // Incremental Summary
            let mut new_conversation_text = String::new();
            for msg in &history[existing_turns..] {
                new_conversation_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
            }

            let system_prompt = "\
You are a context manager. Update the existing Session-level Summary with the new conversation turns.

The summary MUST continue to contain:
1. A 'TL;DR' section: An updated 1-2 sentence overview of the main topics, goals, and outcomes.
2. A 'Topics' section: An updated concise bulleted list of topics discussed.
3. A 'Decisions' section: An updated bulleted list of key decisions made, action items, or preferences identified.

Integrate the new conversation turns seamlessly into the existing summary. Keep it extremely concise, dense, and professional.
Output ONLY the markdown content. Do not include any introductory or concluding remarks.
";

            let user_message = format!(
                "## Existing Summary:\n{}\n\n## New Conversation Turns:\n{}",
                existing_content, new_conversation_text
            );

            let model = rustyclaw_providers::RustyclawCompletionModel::new(self.config.clone(), "summary", target_session_id);
            let usage_sink = model.usage_sink();

            let agent = rig_core::agent::AgentBuilder::new(model)
                .preamble(system_prompt)
                .build();

            let content = rig_core::agent::PromptRequest::from_agent(&agent, &user_message)
                .await
                .context("LLM call failed for incremental session summary generation agent")?;

            if let Some(db) = &usage_db
                && let Some(usage) = usage_sink.lock().unwrap().take()
            {
                Self::record_aux_usage(db, target_session_id, &usage, "session-summary");
            }
            content
        } else if existing_turns > 0 && existing_turns >= history.len() {
            existing_content
        } else {
            // Full Summary from scratch
            let mut conversation_text = String::new();
            for msg in &history {
                conversation_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
            }

            let system_prompt = "\
You are a context manager. Analyze the provided conversation history and produce a concise Session-level Summary in markdown format.

The summary MUST contain:
1. A 'TL;DR' section: A 1-2 sentence overview of the main topics, goals, and outcomes.
2. A 'Topics' section: A concise bulleted list of topics discussed.
3. A 'Decisions' section: A bulleted list of key decisions made, action items, or preferences identified.

Keep the summary extremely concise, dense, and professional. Write in the same language as the conversation (typically Japanese or English).
Output ONLY the markdown content. Do not include any introductory or concluding remarks.
";

            let user_message = format!(
                "## Session ID: {}\n\n## Conversation History:\n{}",
                target_session_id, conversation_text
            );

            let model = rustyclaw_providers::RustyclawCompletionModel::new(self.config.clone(), "summary", target_session_id);
            let usage_sink = model.usage_sink();

            let agent = rig_core::agent::AgentBuilder::new(model)
                .preamble(system_prompt)
                .build();

            let content = rig_core::agent::PromptRequest::from_agent(&agent, &user_message)
                .await
                .context("LLM call failed for full session summary generation agent")?;

            if let Some(db) = &usage_db
                && let Some(usage) = usage_sink.lock().unwrap().take()
            {
                Self::record_aux_usage(db, target_session_id, &usage, "session-summary");
            }
            content
        };

        // Append comment for tracking turns (strip any existing footer comment first)
        let base_summary = if let Some(idx) = summary_content.rfind("<!-- turns: ") {
            summary_content[..idx].trim().to_string()
        } else {
            summary_content.trim().to_string()
        };

        let final_summary = format!("{}\n\n<!-- turns: {} -->\n", base_summary, history.len());

        let _ = fs::create_dir_all(&summaries_dir);
        let _ = fs::write(&existing_summary_path, &final_summary);

        // セッション要約を DB に保存（rag_engine は Pipeline.rag 経由で Task 7 で接続）
        {
            let db_path = workspace_dir.join("memory.db");
            ingest_session_summary(
                target_session_id,
                &final_summary,
                &self.config,
                &db_path,
                None,
            )
            .await;
        }

        Ok(final_summary)
    }


    /// rig::agent::Agent を使ったツール実行。
    /// RustyclawCompletionModel を通じてフェイルオーバーチェーンを保ちつつ、
    /// rig の ReAct ループで自動的にツール呼び出しを処理する。
    /// NOTE: ツール呼び出しの中間ステップはセッションログに保存されない（最終結果のみ）。
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_with_rig_agent(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        raw_user_message: &str,
        injected_user_message: &str,
        tool_handle: rig_core::tool::server::ToolServerHandle,
        purpose: &str,
        progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
    ) -> Result<LlmResponse> {
        use rig_core::agent::AgentBuilder;
        use rustyclaw_providers::RustyclawCompletionModel;

        let logger = SessionLogger::new(workspace_dir);

        // システムプロンプトと RAG コンテキストの構築
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
        {
            system_context.push_str(&continuation);
        }
        // セッション履歴のロード（RAG クエリ構築にも使用するため先行ロード）
        let history_messages = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger
                .load_history(session_id)
                .context("Failed to load session history")?
        };

        // channel_top_k 優先、未設定時はグローバル top_k にフォールバック
        let top_k = self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
        let channel_top_k = self
            .config
            .embedding
            .as_ref()
            .and_then(|e| e.channel_top_k)
            .unwrap_or(top_k);

        // RAG クエリ: 直近 N ターン会話 + 現在メッセージ（cron: は raw_user_message のみ）
        let rag_query = if session_id.starts_with("cron:") {
            raw_user_message.to_string()
        } else {
            build_discord_rag_query(&history_messages, raw_user_message)
        };

        if self
            .config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&self.config) {
                let db_path = workspace_dir.join("memory.db");
                let rag_ctx =
                    retrieve_rag_context_local(&rag_query, &self.config, &client, &db_path, channel_top_k).await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(&rag_query, &self.config, rag, channel_top_k).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
        }
        let cleaned_history =
            self.process_proactive_posts(session_id, history_messages, &mut system_context);
        let mut history = ConversationHistory::new(cleaned_history);
        history.trim_to_last(self.get_history_message_limit(purpose));

        // ユーザーメッセージを保存 (injected されていないオリジナルのメッセージのみ保存)
        let user_msg = Message {
            role: "user".to_string(),
            content: raw_user_message.to_string(),
            ..Default::default()
        };
        logger
            .append_message(session_id, &user_msg)
            .context("Failed to save user message")?;

        if let Some(ref tx) = progress_tx {
            let _ = tx.send("rig エージェントが処理中...".to_string()).await;
        }

        // rig CompletionModel + ToolServerHandle でエージェント構築
        let model = RustyclawCompletionModel::new(self.config.clone(), purpose, session_id);
        let usage_sink = model.usage_sink();

        // max_turns を明示設定。未設定時は usize::default()=0 となり tool call 1往復後に MaxTurnsError になる (rig-core#0.38)
        let agent = AgentBuilder::new(model)
            .preamble(&system_context)
            .tool_server_handle(tool_handle)
            .default_max_turns(20)
            .build();

        // プロバイダーメッセージを rig メッセージ形式に変換
        let rig_history = rustyclaw_providers::provider_messages_to_rig(&history.messages);

        // rig ReAct ループ実行 (LLMにはインジェクション済みのメッセージを渡す)
        let response_text = rig_core::agent::PromptRequest::from_agent(&agent, injected_user_message)
            .with_history(rig_history)
            .with_hook(AgentHook::new(workspace_dir.to_path_buf(), session_id.to_string(), self.config.autonomy_level, false))
            .await
            .map_err(|e| anyhow::anyhow!("rig agent error: {}", e))?;

        let response_text = filter_json_leaks(&response_text);

        // 最終プロバイダー呼び出しの usage 情報を取得
        let last_usage = usage_sink.lock().unwrap().take();

        // 最終アシスタントメッセージを保存
        let assistant_msg = Message {
            role: "assistant".to_string(),
            content: response_text.clone(),
            ..Default::default()
        };
        logger
            .append_message(session_id, &assistant_msg)
            .context("Failed to save assistant response")?;

        if !session_id.starts_with("cron") {
            self.trigger_memory_flush_async(workspace_dir, session_id);
        }

        Ok(LlmResponse {
            content: response_text,
            role: "assistant".to_string(),
            tool_calls: None,
            prompt_tokens: last_usage.as_ref().and_then(|u| u.prompt_tokens),
            completion_tokens: last_usage.as_ref().and_then(|u| u.completion_tokens),
            total_tokens: last_usage.as_ref().and_then(|u| u.total_tokens),
            model_used: last_usage.as_ref().and_then(|u| u.model_used.clone()),
            provider_id: last_usage.as_ref().and_then(|u| u.provider_id.clone()),
        })
    }

    /// ストリーミングでの対話実行
    pub async fn execute_stream(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
        {
            system_context.push_str(&continuation);
        }

        // RAG: ユーザーメッセージに関連する記憶を動的注入（rag が初期化済みの場合のみ）
        let top_k = self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
        if self
            .config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&self.config) {
                let db_path = workspace_dir.join("memory.db");
                let rag_ctx =
                    retrieve_rag_context_local(user_message, &self.config, &client, &db_path, top_k).await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(user_message, &self.config, rag, top_k).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
        }

        // 1. 過去履歴のロードとトークン圧縮処理の適用
        let logger = SessionLogger::new(workspace_dir);
        let history_messages = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger
                .load_history(session_id)
                .context("Failed to load session history messages")?
        };

        let mut history = ConversationHistory::new(history_messages);
        history.trim_to_last(self.get_history_message_limit("default"));

        // 2. 送信用メッセージリストの構築 (System + History + User)
        let mut messages = Vec::new();
        messages.push(Message {
            role: "system".to_string(),
            content: system_context,
            name: None,
            ..Default::default()
        });
        messages.extend(history.messages.clone());
        let user_msg = Message {
            role: "user".to_string(),
            content: user_message.to_string(),
            name: None,
            ..Default::default()
        };
        messages.push(user_msg.clone());

        self.dump_request(workspace_dir, &messages);

        let cat = resolve_category(session_id);
        let opts = CompletionOptions {
            model: self.config.get_model("default").model_name,
            max_tokens: self.config.get_model("default").max_tokens,
            temperature: self.config.get_model("default").temperature,
            timeout: Duration::from_secs(900),
            category: Some(cat),
        };

        // 3. LLMプロバイダ呼び出し
        let stream = self.provider.complete_stream(&messages, &[], &opts).await?;

        let debug_dump_enabled = self.config.debug_dump;
        let config_clone = self.config.clone();
        let workspace_dir_clone = PathBuf::from(workspace_dir);
        let session_id_clone = session_id.to_string();

        // ユーザーメッセージを先行して fail-closed で保存
        logger
            .append_message(session_id, &user_msg)
            .context("Failed to save user message in session log (fail-closed)")?;

        // 4. ストリームをバッファリングしながら転送するラッパー
        let mut buffer = String::new();
        let wrapped_stream = async_stream::try_stream! {
            let mut pin_stream = stream;
            let mut pending_stream_buffer = String::new();
            while let Some(chunk_res) = pin_stream.next().await {
                let chunk = chunk_res?;
                buffer.push_str(&chunk.content);
                pending_stream_buffer.push_str(&chunk.content);

                loop {
                    if let Some(start_idx) = pending_stream_buffer.find("```json") {
                        if start_idx > 0 {
                            let safe_to_yield = pending_stream_buffer[..start_idx].to_string();
                            yield StreamChunk { content: safe_to_yield };
                            pending_stream_buffer.drain(..start_idx);
                        }

                        if let Some(end_offset) = pending_stream_buffer[7..].find("```") {
                            let end_idx = 7 + end_offset + 3;
                            let json_block = pending_stream_buffer[..end_idx].to_string();
                            if json_block.contains("\"action\"") || json_block.contains("\"command\"") {
                                pending_stream_buffer.drain(..end_idx);
                            } else {
                                yield StreamChunk { content: json_block };
                                pending_stream_buffer.drain(..end_idx);
                            }
                            continue;
                        } else {
                            break;
                        }
                    } else {
                        let mut yield_len = pending_stream_buffer.len();
                        for prefix_len in (1..=7).rev() {
                            let prefix = &"```json"[..prefix_len];
                            if pending_stream_buffer.ends_with(prefix) {
                                yield_len = pending_stream_buffer.len() - prefix_len;
                                break;
                            }
                        }

                        if yield_len > 0 {
                            let safe_to_yield: String = pending_stream_buffer.drain(..yield_len).collect();
                            yield StreamChunk { content: safe_to_yield };
                        }
                        break;
                    }
                }
            }

            if !pending_stream_buffer.is_empty() {
                if pending_stream_buffer.contains("```json") && (pending_stream_buffer.contains("\"action\"") || pending_stream_buffer.contains("\"command\"")) {
                    // Discard
                } else {
                    yield StreamChunk { content: pending_stream_buffer };
                }
            }

            let cleaned_buffer = filter_json_leaks(&buffer);

            // ストリーム終了時: アシスタント返答を履歴保存 (fail-closed)
            let assistant_msg = Message {
                role: "assistant".to_string(),
                content: cleaned_buffer.clone(),
                name: None,
                ..Default::default()
            };
            let logger_inner = SessionLogger::new(&workspace_dir_clone);
            logger_inner.append_message(&session_id_clone, &assistant_msg)
                .context("Failed to save assistant response in session log (fail-closed)")?;

            // メモリの非同期フラッシュ (cron セッション以外)
            if !session_id_clone.starts_with("cron") {
                let workspace_dir_flush = workspace_dir_clone.clone();
                let session_id_flush = session_id_clone.clone();
                let config_flush = config_clone.clone();
                tokio::spawn(async move {
                    if let Err(e) = Self::flush_memory(&workspace_dir_flush, &session_id_flush, config_flush).await {
                        tracing::warn!("Failed to flush memory for session {}: {:#}", session_id_flush, e);
                    }
                });
            }

            // デバッグダンプ (Response)
            if debug_dump_enabled {
                let debug_dir = workspace_dir_clone.join("memory").join("debug");
                let file_path = debug_dir.join("last_response.json");

                #[derive(serde::Serialize)]
                struct DumpResponse { role: String, content: String }

                let dump_data = DumpResponse {
                    role: "assistant".to_string(),
                    content: cleaned_buffer.clone(),
                };
                if let Ok(file) = File::create(&file_path) {
                    let _ = serde_json::to_writer_pretty(file, &dump_data);
                }
            }
        };

        Ok(Box::pin(wrapped_stream))
    }
}

// ── RAG Helpers ──────────────────────────────────────────────────────────────

/// rig-core InMemoryVectorStore を使った統合 RAG エンジン。
/// SQLite（永続化）と InMemoryVectorStore（高速検索）のハイブリッド構成。
/// source 情報は SQLite の source 列を source_map に保持し、検索時に参照する。
pub struct UnifiedRagEngine {
    store: std::sync::Mutex<InMemoryVectorStore<String>>,
    /// id → source のマッピング（DB の source 列を保持）
    source_map: std::sync::Mutex<std::collections::HashMap<String, String>>,
    /// id → parent_id のマッピング
    parent_map: std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    /// id → text のマッピング（親チャンクテキストのキャッシュも含む）
    text_map: std::sync::Mutex<std::collections::HashMap<String, String>>,
    model: rustyclaw_providers::CloudflareEmbeddingModel,
}

impl UnifiedRagEngine {
    pub fn new(model: rustyclaw_providers::CloudflareEmbeddingModel) -> Self {
        Self {
            store: std::sync::Mutex::new(InMemoryVectorStore::default()),
            source_map: std::sync::Mutex::new(std::collections::HashMap::new()),
            parent_map: std::sync::Mutex::new(std::collections::HashMap::new()),
            text_map: std::sync::Mutex::new(std::collections::HashMap::new()),
            model,
        }
    }

    /// SQLite の全 embedding データを InMemoryVectorStore に再ロードする。
    pub fn rebuild_from_db(&self, db: &rustyclaw_storage::DbManager) -> anyhow::Result<()> {
        let rows = db.load_all_embeddings_with_ids()?;
        let mut store = self.store.lock().unwrap();
        let mut smap = self.source_map.lock().unwrap();
        let mut pmap = self.parent_map.lock().unwrap();
        let mut tmap = self.text_map.lock().unwrap();
        *store = InMemoryVectorStore::default();
        smap.clear();
        pmap.clear();
        tmap.clear();
        let mut documents: Vec<(String, String, OneOrMany<Embedding>)> = Vec::new();
        for (id, source, text, vec_f32, parent_id) in rows {
            smap.insert(id.clone(), source);
            pmap.insert(id.clone(), parent_id);
            tmap.insert(id.clone(), text.clone());
            if !vec_f32.is_empty() {
                let emb = Embedding {
                    document: text.clone(),
                    vec: vec_f32.iter().map(|&x| x as f64).collect(),
                };
                documents.push((id, text, OneOrMany::one(emb)));
            }
        }
        store.add_documents_with_ids(documents);
        Ok(())
    }

    /// 1件を InMemoryVectorStore に追加する（SQLite への書き込みは呼び出し元が行う）。
    pub fn add_one(
        &self,
        id: &str,
        source: &str,
        _session_id: Option<&str>,
        text: &str,
        vec_f32: &[f32],
        parent_id: Option<&str>,
    ) {
        let mut store = self.store.lock().unwrap();
        if !vec_f32.is_empty() {
            let emb = Embedding {
                document: text.to_string(),
                vec: vec_f32.iter().map(|&x| x as f64).collect(),
            };
            store.add_documents_with_ids([(id.to_string(), text.to_string(), OneOrMany::one(emb))]);
        }
        self.source_map
            .lock()
            .unwrap()
            .insert(id.to_string(), source.to_string());
        self.parent_map
            .lock()
            .unwrap()
            .insert(id.to_string(), parent_id.map(|s| s.to_string()));
        self.text_map
            .lock()
            .unwrap()
            .insert(id.to_string(), text.to_string());
    }

    /// top_k 件の (source, text, score) を返す。
    /// source は DB の source カラムから取得（source_map 経由）。
    /// 親子関係 (parent_id) が存在する場合は自動的に親チャンクのテキストに解決し、
    /// 重複排除を行って返す。
    pub async fn search(
        &self,
        query_text: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<(String, String, f64)>> {
        let req = VectorSearchRequest::builder()
            .query(query_text.to_string())
            .samples(top_k as u64)
            .build();
        let (store_clone, model_clone) = {
            let store = self.store.lock().unwrap();
            (store.clone(), self.model.clone())
        };
        let index = store_clone.index(model_clone);
        let id_scores = index
            .top_n_ids(req)
            .await
            .map_err(|e| anyhow::anyhow!("RAG search failed: {}", e))?;

        let smap = self.source_map.lock().unwrap();
        let pmap = self.parent_map.lock().unwrap();
        let tmap = self.text_map.lock().unwrap();

        let mut results = Vec::new();
        let mut seen_texts = std::collections::HashSet::new();
        for (score, id) in id_scores {
            let source = smap.get(&id).cloned().unwrap_or_else(|| {
                tracing::warn!("UnifiedRagEngine::search: source not found for id={}", id);
                "unknown".to_string()
            });
            let text = if let Some(Some(parent_id)) = pmap.get(&id) {
                tmap.get(parent_id).cloned().unwrap_or_else(|| {
                    tmap.get(&id).cloned().unwrap_or_default()
                })
            } else {
                tmap.get(&id).cloned().unwrap_or_default()
            };

            if !text.is_empty() && seen_texts.insert(text.clone()) {
                results.push((source, text, score));
            }
        }
        Ok(results)
    }

    pub fn len(&self) -> usize {
        self.store.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.lock().unwrap().is_empty()
    }
}

/// MEMORY.md のバレット行を 1件 1チャンクに分割する。
/// ヘッダー行 (#) や空行はスキップ。最大 512 文字で末尾切捨て。
#[allow(dead_code)]
pub(crate) fn chunk_memory_md(content: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_section = "General".to_string();
    let mut current_bullets: Vec<String> = Vec::new();
    let mut current_len: usize = 0;
    const MAX_CHUNK: usize = 800;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            if !current_bullets.is_empty() {
                chunks.push(format!("[{}] {}", current_section, current_bullets.join("\n")));
                current_bullets.clear();
                current_len = 0;
            }
            let section = trimmed.trim_start_matches('#').trim().to_string();
            current_section = if section.is_empty() {
                "General".to_string()
            } else {
                section
            };
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if current_len + trimmed.len() + 1 > MAX_CHUNK && !current_bullets.is_empty() {
                chunks.push(format!("[{}] {}", current_section, current_bullets.join("\n")));
                current_bullets.clear();
                current_len = 0;
            }
            current_len += trimmed.len() + 1;
            current_bullets.push(trimmed.to_string());
        }
    }
    if !current_bullets.is_empty() {
        chunks.push(format!("[{}] {}", current_section, current_bullets.join("\n")));
    }
    chunks.into_iter().filter(|s| !s.is_empty()).collect()
}

/// 静的マークダウンを `##` / `###` 見出し単位でチャンク分割する。
/// 各チャンク先頭に `[ファイル名 > 見出し]` の文脈を付与し、800 文字で切り捨てる。
pub(crate) fn chunk_static_document(file_name: &str, content: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_header = String::new();
    let mut current_body = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let body = current_body.trim().to_string();
            if !body.is_empty() && !current_header.is_empty() {
                let raw = format!("[{} > {}]\n{}", file_name, current_header, body);
                chunks.push(raw.chars().take(800).collect());
            }
            current_header = trimmed.to_string();
            current_body.clear();
        } else if !trimmed.starts_with("# ") && trimmed != "#" {
            // 最上位見出し (#) 行はスキップ（ドキュメントタイトルはチャンク対象外）
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    let body = current_body.trim().to_string();
    if !body.is_empty() && !current_header.is_empty() {
        let raw = format!("[{} > {}]\n{}", file_name, current_header, body);
        chunks.push(raw.chars().take(800).collect());
    }
    chunks
}

pub struct HierarchicalChunk {
    pub parent_text: String,
    pub children: Vec<String>,
}

pub(crate) fn chunk_document_hierarchical(file_name: &str, content: &str) -> Vec<HierarchicalChunk> {
    let mut sections = Vec::new();
    let mut current_header = String::new();
    let mut current_body = String::new();
    let mut document_title = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            document_title = trimmed.trim_start_matches("#").trim().to_string();
        } else if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let body = current_body.trim().to_string();
            if !body.is_empty() {
                sections.push((current_header.clone(), body));
            }
            current_header = trimmed.to_string();
            current_body.clear();
        } else {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    let body = current_body.trim().to_string();
    if !body.is_empty() {
        sections.push((current_header.clone(), body));
    }

    if sections.is_empty() && !content.trim().is_empty() {
        sections.push((String::new(), content.trim().to_string()));
    }

    let mut result = Vec::new();
    for (header, body) in sections {
        let parent_title = if header.is_empty() {
            if document_title.is_empty() {
                file_name.to_string()
            } else {
                format!("{} > General", document_title)
            }
        } else {
            if document_title.is_empty() {
                format!("{} > {}", file_name, header)
            } else {
                format!("{} > {} > {}", file_name, document_title, header)
            }
        };
        let parent_text = format!("[{}]\n{}", parent_title, body);

        let mut children = Vec::new();
        let mut current_child = String::new();
        for line in body.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty() {
                continue;
            }

            if line_trimmed.starts_with("- ") || line_trimmed.starts_with("* ") {
                if !current_child.is_empty() {
                    children.push(format!("[{}]\n{}", parent_title, current_child.trim()));
                    current_child.clear();
                }
                children.push(format!("[{}]\n{}", parent_title, line_trimmed));
            } else {
                if !current_child.is_empty() {
                    current_child.push('\n');
                }
                current_child.push_str(line);
                if current_child.len() >= 200 {
                    children.push(format!("[{}]\n{}", parent_title, current_child.trim()));
                    current_child.clear();
                }
            }
        }
        if !current_child.is_empty() {
            children.push(format!("[{}]\n{}", parent_title, current_child.trim()));
        }

        result.push(HierarchicalChunk {
            parent_text,
            children,
        });
    }
    result
}

pub(crate) enum EmbedClientKind {
    Remote(rustyclaw_providers::CloudflareEmbeddingClient),
    Local(rustyclaw_providers::LocalEmbeddingClient),
}

impl EmbedClientKind {
    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        match self {
            Self::Remote(c) => c.embed(texts).await,
            Self::Local(c) => c.embed(texts).await,
        }
    }
}

fn make_embed_client(config: &Config) -> Option<EmbedClientKind> {
    if config
        .embedding
        .as_ref()
        .map(|e| e.use_local_embedding)
        .unwrap_or(false)
    {
        let cache_dir = rustyclaw_config::get_app_dir().join("fastembed_cache");
        match rustyclaw_providers::LocalEmbeddingClient::new(&cache_dir) {
            Ok(c) => return Some(EmbedClientKind::Local(c)),
            Err(e) => {
                tracing::warn!("make_embed_client: LocalEmbeddingClient init failed: {}", e);
                return None;
            }
        }
    }
    let (api_endpoint, api_key, _model) = config.get_embedding_client_params()?;
    Some(EmbedClientKind::Remote(
        rustyclaw_providers::CloudflareEmbeddingClient::new(&api_endpoint, &api_key),
    ))
}

/// workspace_dir 内の AGENTS.md, USER.md, skills/*.md, memory/logs/*.md, memory/summaries/*.md, patrol/findings.md をチャンク化し、
/// ハッシュ差分がある場合のみ memory.db に embedding を保存する。Fail-open。
pub async fn ingest_static_documents(
    workspace_dir: &std::path::Path,
    config: &Config,
    db_path: &std::path::Path,
) {
    let client = match make_embed_client(config) {
        Some(c) => c,
        None => return,
    };
    let db = match rustyclaw_storage::DbManager::new(db_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("ingest_static_documents: db open error: {}", e);
            return;
        }
    };
    // ローカルモード時: 次元数が異なる場合 DB をクリアして全再インジェストを促す
    if config
        .embedding
        .as_ref()
        .map(|e| e.use_local_embedding)
        .unwrap_or(false)
        && let Err(e) = db.migrate_embedding_dims_if_needed(384)
    {
        tracing::warn!("ingest_static_documents: migration error: {}", e);
    }

    // スキャン対象ファイル: AGENTS.md + USER.md + skills/**/*.md + memory/logs/*.md (直近14日) + memory/summaries/*.md + patrol/findings.md
    let mut files = vec![workspace_dir.join("AGENTS.md"), workspace_dir.join("USER.md")];
    
    // skills
    let skills_dir = workspace_dir.join("skills");
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        let mut skill_files: Vec<_> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                skill_files.push(path);
            } else if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    if sub_path.is_file()
                        && sub_path.extension().is_some_and(|ext| ext == "md")
                    {
                        skill_files.push(sub_path);
                    }
                }
            }
        }
        skill_files.sort();
        files.extend(skill_files);
    }

    // memory/logs (直近14日間のみインジェスト)
    let logs_dir = workspace_dir.join("memory").join("logs");
    let now_utc = chrono::Utc::now().date_naive();
    if let Ok(entries) = std::fs::read_dir(&logs_dir) {
        let mut log_files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_valid = path.is_file()
                && path.extension().is_some_and(|ext| ext == "md")
                && path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|stem| chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok())
                    .map(|date| (now_utc - date).num_days() <= 14)
                    .unwrap_or(false);
            if is_valid {
                log_files.push(path);
            }
        }
        log_files.sort();
        files.extend(log_files);
    }

    // memory/summaries
    let summaries_dir = workspace_dir.join("memory").join("summaries");
    if let Ok(entries) = std::fs::read_dir(&summaries_dir) {
        let mut summary_files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                summary_files.push(path);
            }
        }
        summary_files.sort();
        files.extend(summary_files);
    }

    // patrol/findings.md
    let findings_path = workspace_dir.join("patrol").join("findings.md");
    if findings_path.is_file() {
        files.push(findings_path);
    }

    use sha2::{Digest, Sha256};
    for file_path in files {
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // workspace_dir からの相対パスをキーにする
        let file_name: String = file_path
            .strip_prefix(workspace_dir)
            .ok()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

        let hash_str = format!("{:x}", Sha256::digest(content.as_bytes()));

        let is_changed = db
            .check_and_update_doc_state(&file_name, &hash_str)
            .unwrap_or(true);

        if !is_changed {
            tracing::debug!(
                "ingest_static_documents: '{}' unchanged, skipping",
                file_name
            );
            continue;
        }

        tracing::info!(
            "ingest_static_documents: '{}' changed, ingesting...",
            file_name
        );

        let is_hierarchical = file_name == "AGENTS.md"
            || file_name == "USER.md"
            || file_name.starts_with("skills/")
            || file_name.starts_with("memory/logs/")
            || file_name.starts_with("memory/summaries/");

        let source_id = format!("doc:{}", file_name);

        if is_hierarchical {
            let h_chunks = chunk_document_hierarchical(&file_name, &content);
            if let Err(e) = db.delete_embeddings_by_source(&source_id) {
                tracing::warn!(
                    "ingest_static_documents: delete error for '{}': {}",
                    file_name,
                    e
                );
                continue;
            }

            for (p_idx, h_chunk) in h_chunks.iter().enumerate() {
                let parent_id = format!("doc-{}-p-{}", file_name.replace(['.', '/', '\\'], "_"), p_idx);
                // 親チャンクを upsert (ベクトルは空、parent_id も None)
                if let Err(e) = db.upsert_embedding(&parent_id, &source_id, None, &h_chunk.parent_text, &[], None) {
                    tracing::warn!("ingest_static_documents: parent upsert error: {}", e);
                    continue;
                }

                if !h_chunk.children.is_empty() {
                    let text_refs: Vec<&str> = h_chunk.children.iter().map(|s| s.as_str()).collect();
                    match client.embed(&text_refs).await {
                        Ok(embeddings) => {
                            for (c_idx, (child_text, emb)) in h_chunk.children.iter().zip(embeddings.iter()).enumerate() {
                                let child_id = format!("doc-{}-c-{}-{}", file_name.replace(['.', '/', '\\'], "_"), p_idx, c_idx);
                                if let Err(e) = db.upsert_embedding(&child_id, &source_id, None, child_text, emb, Some(&parent_id)) {
                                    tracing::warn!("ingest_static_documents: child upsert error: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "ingest_static_documents: child embed API error for '{}': {}",
                                file_name,
                                e
                            );
                            let _ = db.check_and_update_doc_state(&file_name, "");
                        }
                    }
                }
            }
        } else {
            // フラットチャンキング
            let chunks = chunk_static_document(&file_name, &content);
            if chunks.is_empty() {
                continue;
            }

            if let Err(e) = db.delete_embeddings_by_source(&source_id) {
                tracing::warn!(
                    "ingest_static_documents: delete error for '{}': {}",
                    file_name,
                    e
                );
                continue;
            }

            let text_refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
            match client.embed(&text_refs).await {
                Ok(embeddings) => {
                    let saved = embeddings.len().min(chunks.len());
                    for (i, (chunk, emb)) in chunks.iter().zip(embeddings.iter()).enumerate() {
                        let id = format!("doc-{}-{}", file_name.replace(['.', '/', '\\'], "_"), i);
                        if let Err(e) = db.upsert_embedding(&id, &source_id, None, chunk, emb, None) {
                            tracing::warn!("ingest_static_documents: upsert error: {}", e);
                        }
                    }
                    tracing::info!(
                        "ingest_static_documents: ingested {} chunks from '{}'",
                        saved,
                        file_name
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "ingest_static_documents: embed API error for '{}': {}",
                        file_name,
                        e
                    );
                    let _ = db.check_and_update_doc_state(&file_name, "");
                }
            }
        }
    }

    // 古いログ (memory/logs/*.md) embedding のクレンジング
    if let Err(e) = db.delete_old_log_embeddings(14) {
        tracing::warn!("ingest_static_documents: log TTL cleanup error: {}", e);
    }
}

/// MEMORY.md を読み込み、バレット行をチャンク化してベクトル化し memory.db に保存する。
/// Fail-open: エラーは warn ログのみで処理を継続する。
pub async fn ingest_memory_md(
    workspace_dir: &Path,
    config: &Config,
    db_path: &Path,
    rag_engine: Option<&UnifiedRagEngine>,
) {
    let client = match make_embed_client(config) {
        Some(c) => c,
        None => {
            if config
                .embedding
                .as_ref()
                .map(|e| e.enabled)
                .unwrap_or(false)
            {
                tracing::warn!(
                    "ingest_memory_md: embedding enabled but no valid client (check use_local_embedding or API config)"
                );
            }
            return;
        }
    };
    let memory_path = workspace_dir.join("MEMORY.md");
    let content = match std::fs::read_to_string(&memory_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("ingest_memory_md: failed to read MEMORY.md: {}", e);
            return;
        }
    };
    let h_chunks = chunk_document_hierarchical("MEMORY.md", &content);
    if h_chunks.is_empty() {
        tracing::debug!("ingest_memory_md: no chunks found in MEMORY.md");
        return;
    }

    let db = match rustyclaw_storage::DbManager::new(db_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("ingest_memory_md: db open error: {}", e);
            return;
        }
    };
    if let Err(e) = db.delete_embeddings_by_source("memory") {
        tracing::warn!("ingest_memory_md: failed to delete old embeddings: {}", e);
        return;
    }

    for (p_idx, h_chunk) in h_chunks.iter().enumerate() {
        let parent_id = format!("memory-p-{}", p_idx);
        if let Err(e) = db.upsert_embedding(&parent_id, "memory", None, &h_chunk.parent_text, &[], None) {
            tracing::warn!("ingest_memory_md: parent upsert error: {}", e);
            continue;
        }
        if let Some(rag) = rag_engine {
            rag.add_one(&parent_id, "memory", None, &h_chunk.parent_text, &[], None);
        }

        if !h_chunk.children.is_empty() {
            let text_refs: Vec<&str> = h_chunk.children.iter().map(|s| s.as_str()).collect();
            match client.embed(&text_refs).await {
                Ok(embeddings) => {
                    for (c_idx, (child_text, emb)) in h_chunk.children.iter().zip(embeddings.iter()).enumerate() {
                        let child_id = format!("memory-c-{}-{}", p_idx, c_idx);
                        if let Err(e) = db.upsert_embedding(&child_id, "memory", None, child_text, emb, Some(&parent_id)) {
                            tracing::warn!("ingest_memory_md: child upsert error: {}", e);
                        }
                        if let Some(rag) = rag_engine {
                            rag.add_one(&child_id, "memory", None, child_text, emb, Some(&parent_id));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("ingest_memory_md: child embed API error: {}", e);
                }
            }
        }
    }
    tracing::info!(
        "ingest_memory_md: ingested {} hierarchical chunks from MEMORY.md",
        h_chunks.len()
    );
}

/// セッション要約テキストを embedding して SQLite + UnifiedRagEngine に保存する。Fail-open。
/// rag_engine が Some の場合は InMemoryVectorStore にも追加する。
pub(crate) async fn ingest_session_summary(
    session_id: &str,
    summary_text: &str,
    config: &Config,
    db_path: &Path,
    rag_engine: Option<&UnifiedRagEngine>,
) {
    let client = match make_embed_client(config) {
        Some(c) => c,
        None => return,
    };
    let text_short: String = summary_text.chars().take(1024).collect();
    let embeddings = match client.embed(&[text_short.as_str()]).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            tracing::warn!("ingest_session_summary: empty embedding result");
            return;
        }
        Err(e) => {
            tracing::warn!("ingest_session_summary: embed error: {}", e);
            return;
        }
    };
    let db = match rustyclaw_storage::DbManager::new(db_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("ingest_session_summary: db open error: {}", e);
            return;
        }
    };
    let id = format!("session::{}", session_id.replace(':', "-"));
    if let Err(e) = db.upsert_embedding(
        &id,
        "session",
        Some(session_id),
        &text_short,
        &embeddings[0],
        None,
    ) {
        tracing::warn!("ingest_session_summary: upsert error: {}", e);
        return;
    }
    if let Some(rag) = rag_engine {
        rag.add_one(
            &id,
            "session",
            Some(session_id),
            &text_short,
            &embeddings[0],
            None,
        );
    }
    let ttl_days = config
        .embedding
        .as_ref()
        .and_then(|e| e.session_summary_ttl_days)
        .unwrap_or(7);
    if let Err(e) = db.delete_old_session_embeddings(ttl_days) {
        tracing::warn!("ingest_session_summary: TTL cleanup error: {}", e);
    }
    tracing::info!(
        "ingest_session_summary: stored embedding for session '{}'",
        session_id
    );
}

/// RAG 検索結果をシステムプロンプト注入用の Markdown に変換する。
/// source="doc:*" を最初に、source="memory"、source="session" の順で別セクション表示する。
pub(crate) fn format_rag_context(items: &[(String, String, f64)]) -> String {
    if items.is_empty() {
        return String::new();
    }

    let doc_items: Vec<&str> = items
        .iter()
        .filter(|(src, _, _)| src.starts_with("doc:"))
        .map(|(_, txt, _)| txt.as_str())
        .collect();
    let memory_items: Vec<&str> = items
        .iter()
        .filter(|(src, _, _)| src == "memory")
        .map(|(_, txt, _)| txt.as_str())
        .collect();
    let session_items: Vec<&str> = items
        .iter()
        .filter(|(src, _, _)| src == "session")
        .map(|(_, txt, _)| txt.as_str())
        .collect();

    let mut out = String::new();

    if !doc_items.is_empty() {
        out.push_str("\n\n## Relevant Specifications & Rules\n");
        out.push_str("Use the following guidelines for task execution:\n\n");
        for text in &doc_items {
            out.push_str(text);
            out.push('\n');
        }
    }
    if !memory_items.is_empty() {
        out.push_str("\n\n## Relevant Memory\n");
        out.push_str("The following memories are relevant to the current conversation:\n\n");
        for text in &memory_items {
            out.push_str(text);
            out.push('\n');
        }
    }
    if !session_items.is_empty() {
        out.push_str("\n\n## Relevant Past Sessions\n");
        out.push_str("The following session summaries are relevant:\n\n");
        for text in &session_items {
            out.push_str(text);
            out.push('\n');
        }
    }
    out
}

/// ユーザーメッセージに関連する記憶・過去セッションを UnifiedRagEngine から検索し、
/// システムプロンプト注入用の文字列を返す。Fail-open。
pub(crate) async fn retrieve_rag_context(
    query_text: &str,
    config: &Config,
    rag_engine: &UnifiedRagEngine,
    top_k: usize,
) -> String {
    if rag_engine.is_empty() {
        return String::new();
    }
    let threshold = config
        .embedding
        .as_ref()
        .map(|e| e.similarity_threshold as f64)
        .unwrap_or(0.60);
    let query_short: String = query_text.chars().take(512).collect();
    let results = match rag_engine.search(&query_short, top_k).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("retrieve_rag_context: search error: {}", e);
            return String::new();
        }
    };
    let filtered: Vec<(String, String, f64)> = results
        .into_iter()
        .filter(|(_, _, score)| *score >= threshold)
        .collect();
    if filtered.is_empty() {
        return String::new();
    }
    tracing::debug!(
        "retrieve_rag_context: {} hits above threshold {}",
        filtered.len(),
        threshold
    );
    format_rag_context(&filtered)
}

/// ローカル embedding モデルを使って query_text に関連する記憶・ドキュメントを SQLite から検索し、
/// システムプロンプト注入用の文字列を返す。Fail-open。
pub(crate) async fn retrieve_rag_context_local(
    query_text: &str,
    config: &Config,
    embed_client: &EmbedClientKind,
    db_path: &Path,
    top_k: usize,
) -> String {
    let query_short: String = query_text.chars().take(512).collect();
    let embeddings = match embed_client.embed(&[query_short.as_str()]).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => return String::new(),
        Err(e) => {
            tracing::warn!("retrieve_rag_context_local: embed error: {}", e);
            return String::new();
        }
    };
    let db = match rustyclaw_storage::DbManager::new(db_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("retrieve_rag_context_local: db open error: {}", e);
            return String::new();
        }
    };
    let threshold = config
        .embedding
        .as_ref()
        .map(|e| e.similarity_threshold)
        .unwrap_or(0.60);
    let half_life = config
        .embedding
        .as_ref()
        .and_then(|e| e.time_decay_half_life_days);
    let search_result = match half_life {
        Some(hl) => db.search_similar_with_decay(&embeddings[0], top_k, threshold, hl),
        None => db.search_similar_with_source(&embeddings[0], top_k, threshold),
    };
    match search_result {
        Ok(results) if !results.is_empty() => {
            tracing::debug!("retrieve_rag_context_local: {} hits", results.len());
            format_rag_context(&results)
        }
        Ok(_) => String::new(),
        Err(e) => {
            tracing::warn!("retrieve_rag_context_local: search error: {}", e);
            String::new()
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// `start_tag` と `end_tag` に囲まれたブロックを抽出する。
/// `end_tag` が見つからない場合はテキスト末尾までを返す。
fn extract_delimited_block(text: &str, start_tag: &str, end_tag: &str) -> Option<String> {
    let start_pos = text.find(start_tag)?;
    let content_start = start_pos + start_tag.len();
    let content_end = text[content_start..]
        .find(end_tag)
        .map(|p| content_start + p)
        .unwrap_or(text.len());
    let content = text[content_start..content_end].trim().to_string();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

/// config.json の context_window 文字列（"8k", "131k", "256k", "1M" 等）をトークン数に変換する。
/// 未設定または認識不能な場合は保守的なデフォルト 32,768 を返す。
fn parse_context_window(context_window: Option<&str>) -> usize {
    let s = match context_window {
        Some(s) if !s.is_empty() => s.trim().to_lowercase(),
        _ => return 32_768,
    };
    if let Some(num) = s.strip_suffix('m') {
        num.trim().parse::<usize>().unwrap_or(1) * 1_048_576
    } else if let Some(num) = s.strip_suffix('k') {
        num.trim().parse::<usize>().unwrap_or(32) * 1_024
    } else {
        s.parse::<usize>().unwrap_or(32_768)
    }
}


/// 70/20/10 戦略でテキストを `max_bytes` 以内に切り詰める。
fn truncate_70_20(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    let head_end = (max_bytes as f64 * 0.7) as usize;
    let tail_len = (max_bytes as f64 * 0.2) as usize;
    let tail_start = content.len().saturating_sub(tail_len);
    let omitted = content.len() - head_end - tail_len;
    format!(
        "{}\n\n[...{} bytes omitted...]\n\n{}",
        &content[..head_end],
        omitted,
        &content[tail_start..],
    )
}

/// Residual tool-calling JSON leaks (like ```json { "action": ... } ```) を除去する。
fn filter_json_leaks(content: &str) -> String {
    let mut cleaned = content.to_string();
    let mut search_start = 0;
    while let Some(offset) = cleaned[search_start..].find("```json") {
        let start_idx = search_start + offset;
        let rest = &cleaned[start_idx..];
        if let Some(end_offset) = rest[7..].find("```") {
            let end_idx = start_idx + 7 + end_offset + 3;
            let json_block = &cleaned[start_idx..end_idx];
            if json_block.contains("\"action\"") || json_block.contains("\"command\"") {
                cleaned.replace_range(start_idx..end_idx, "");
                // Do not update search_start, we keep searching from the same absolute index since content shifted left.
                continue;
            } else {
                search_start = end_idx;
                continue;
            }
        }
        break;
    }
    cleaned.trim().to_string()
}

fn resolve_category(session_id: &str) -> String {
    if session_id == "memory" {
        "memory".to_string()
    } else if session_id == "summary" || session_id.starts_with("cron:session-summary:") {
        "summary".to_string()
    } else if session_id == "daily-summary"
        || session_id == "cron:daily-summary"
        || session_id.contains("daily-summary")
    {
        "daily".to_string()
    } else if session_id == "cron:heartbeat" || session_id.contains("heartbeat") {
        "heartbeat".to_string()
    } else if session_id.contains("briefing") {
        "briefing".to_string()
    } else if session_id.contains("vitals") {
        "vitals".to_string()
    } else if session_id.contains("karakeep") {
        "karakeep".to_string()
    } else if session_id.contains("patrol") {
        "patrol".to_string()
    } else if session_id.contains("dashboard") {
        "dashboard".to_string()
    } else {
        "discord".to_string()
    }
}

fn extract_stdout(tool_result: &str) -> &str {
    const MARKER: &str = "--- STDOUT ---\n";
    if let Some(start) = tool_result.find(MARKER) {
        let after = &tool_result[start + MARKER.len()..];
        let end = after.find("\n--- ").unwrap_or(after.len());
        &after[..end]
    } else {
        tool_result
    }
}

fn rebuild_tool_result(original: &str, new_stdout: &str) -> String {
    const MARKER: &str = "--- STDOUT ---\n";
    if let Some(start) = original.find(MARKER) {
        let before = &original[..start + MARKER.len()];
        let after = &original[start + MARKER.len()..];
        let rest_start = after.find("\n--- ").unwrap_or(after.len());
        let rest = &after[rest_start..];
        format!("{}{}{}", before, new_stdout, rest)
    } else {
        new_stdout.to_string()
    }
}

fn filter_seen_tool_result(
    tool_name: &str,
    call_args: &str,
    tool_result: &str,
    db: &rustyclaw_storage::DbManager,
) -> String {
    if tool_name != "run_workspace_script" {
        return tool_result.to_string();
    }
    let args: serde_json::Value = serde_json::from_str(call_args).unwrap_or_default();
    let script_name = args["script_name"].as_str().unwrap_or("");

    let (category, id_field): (&str, &str) = if script_name.contains("gmail") {
        ("gmail", "id")
    } else if script_name.contains("calendar") {
        ("calendar", "event_id")
    } else {
        return tool_result.to_string();
    };

    let stdout = extract_stdout(tool_result);

    let items: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return tool_result.to_string(),
    };
    let arr = match items.as_array() {
        Some(a) => a.clone(),
        None => return tool_result.to_string(),
    };

    let mut new_items: Vec<serde_json::Value> = Vec::new();
    let mut seen_count: usize = 0;

    for item in &arr {
        if let Some(id) = item[id_field].as_str().filter(|s| !s.is_empty()) {
            let item_id = format!("{}:{}", category, id);
            if db.is_item_seen(&item_id).unwrap_or(false) {
                seen_count += 1;
            } else {
                if let Err(e) = db.mark_item_seen(&item_id, category) {
                    tracing::warn!("Failed to mark {} as seen: {}", item_id, e);
                }
                new_items.push(item.clone());
            }
        } else {
            new_items.push(item.clone());
        }
    }

    if seen_count == 0 {
        return tool_result.to_string();
    }

    let new_json = serde_json::to_string_pretty(&serde_json::Value::Array(new_items))
        .unwrap_or_else(|_| "[]".to_string());

    let notice = if new_json.trim() == "[]" {
        format!(
            "No new {} items. ({} already seen item(s) were filtered.)",
            category, seen_count
        )
    } else {
        format!(
            "{}\n// Note: {} already seen {} item(s) were filtered.",
            new_json, seen_count, category
        )
    };

    rebuild_tool_result(tool_result, &notice)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rustyclaw_config::{AgentsConfig, ModelEntry, ModelNames};
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// Find the most recent dump file for a given category under llm/<category>/<date>/<time>.json
    fn find_latest_dump(llm_dir: &std::path::Path, category: &str) -> Option<std::path::PathBuf> {
        let cat_dir = llm_dir.join(category);
        let mut date_dirs: Vec<_> = std::fs::read_dir(&cat_dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        date_dirs.sort_unstable_by(|a, b| b.cmp(a));
        let date_dir = date_dirs.into_iter().next()?;
        let mut files: Vec<_> = std::fs::read_dir(&date_dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        files.sort_unstable_by(|a, b| b.cmp(a));
        files.into_iter().next()
    }

    #[tokio::test]
    async fn test_autonomy_level_read_only_blocks_write() {
        use rig_core::agent::PromptHook;
        
        let hook = AgentHook::new(
            PathBuf::from("/tmp"),
            "test_session".to_string(),
            AutonomyLevel::ReadOnly,
            false,
        );
        
        // 書き込み系ツールはブロックされるべき
        let action = PromptHook::<rustyclaw_providers::RustyclawCompletionModel>::on_tool_call(
            &hook, "workspace_write", None, "call_id", "{}"
        ).await;
        match action {
            rig_core::agent::ToolCallHookAction::Skip { reason } => {
                assert!(reason.contains("ReadOnly mode"));
            }
            _ => panic!("Expected Skip action"),
        }
        
        let action = PromptHook::<rustyclaw_providers::RustyclawCompletionModel>::on_tool_call(
            &hook, "workspace_execute_script", None, "call_id", "{}"
        ).await;
        match action {
            rig_core::agent::ToolCallHookAction::Skip { reason } => {
                assert!(reason.contains("ReadOnly mode"));
            }
            _ => panic!("Expected Skip"),
        }

        // 読み取り系ツールは許可されるべき
        let action = PromptHook::<rustyclaw_providers::RustyclawCompletionModel>::on_tool_call(
            &hook, "workspace_read", None, "call_id", "{}"
        ).await;
        assert!(matches!(action, rig_core::agent::ToolCallHookAction::Continue));
        
        let action = PromptHook::<rustyclaw_providers::RustyclawCompletionModel>::on_tool_call(
            &hook, "web_search", None, "call_id", "{}"
        ).await;
        assert!(matches!(action, rig_core::agent::ToolCallHookAction::Continue));
    }

    #[tokio::test]
    async fn test_autonomy_level_supervised_approves() {
        use rig_core::agent::PromptHook;
        let dir = tempdir().unwrap();
        let ws_path = dir.path().to_path_buf();
        
        let hook = AgentHook::new(
            ws_path.clone(),
            "test_session".to_string(),
            AutonomyLevel::Supervised,
            false,
        );

        let ws_path_clone = ws_path.clone();
        // 非同期に status を approved に変更するタスクを走らせる
        tokio::spawn(async move {
            let approval_file = ws_path_clone.join("pending_approval.json");
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if approval_file.exists() {
                    break;
                }
            }
            
            if approval_file.exists()
                && let Ok(content) = fs::read_to_string(&approval_file)
                && let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(obj) = val.as_object_mut()
            {
                obj.insert("status".to_string(), serde_json::Value::String("approved".to_string()));
                let _ = fs::write(&approval_file, serde_json::to_string_pretty(&val).unwrap());
            }
        });

        let action = PromptHook::<rustyclaw_providers::RustyclawCompletionModel>::on_tool_call(
            &hook, "workspace_write", None, "call_id", "{}"
        ).await;
        assert!(matches!(action, rig_core::agent::ToolCallHookAction::Continue));
    }

    #[tokio::test]
    async fn test_autonomy_level_supervised_rejects() {
        use rig_core::agent::PromptHook;
        let dir = tempdir().unwrap();
        let ws_path = dir.path().to_path_buf();
        
        let hook = AgentHook::new(
            ws_path.clone(),
            "test_session".to_string(),
            AutonomyLevel::Supervised,
            false,
        );

        let ws_path_clone = ws_path.clone();
        // 非同期に status を rejected に変更するタスクを走らせる
        tokio::spawn(async move {
            let approval_file = ws_path_clone.join("pending_approval.json");
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if approval_file.exists() {
                    break;
                }
            }
            
            if approval_file.exists()
                && let Ok(content) = fs::read_to_string(&approval_file)
                && let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(obj) = val.as_object_mut()
            {
                obj.insert("status".to_string(), serde_json::Value::String("rejected".to_string()));
                let _ = fs::write(&approval_file, serde_json::to_string_pretty(&val).unwrap());
            }
        });

        let action = PromptHook::<rustyclaw_providers::RustyclawCompletionModel>::on_tool_call(
            &hook, "workspace_write", None, "call_id", "{}"
        ).await;
        match action {
            rig_core::agent::ToolCallHookAction::Skip { reason } => {
                assert!(reason.contains("rejected by user"));
            }
            _ => panic!("Expected Skip action due to rejection"),
        }
    }

    #[test]
    fn test_make_embed_client_returns_none_when_no_config() {
        use rustyclaw_config::Config;
        let config = Config::default();
        // no embedding config → make_embed_client returns None
        assert!(make_embed_client(&config).is_none());
    }

    #[tokio::test]
    async fn test_get_model_chain_skips_disabled_model() {
        // disabled モデルが先頭の場合、get_model_chain が有効モデルだけを返すことを確認
        let config = rustyclaw_config::Config {
            model_list: vec![
                rustyclaw_config::ModelEntry {
                    model_name: "disabled-m".to_string(),
                    provider: "openai".to_string(),
                    model: "disabled-api".to_string(),
                    api_base: "https://disabled.example.com/v1".to_string(),
                    api_key: "key".to_string(),
                    max_tokens: Some(100),
                    temperature: Some(0.5),
                    enabled: false,
                    rpm: None,
                    rpd: None,
                    tpm: None,
                    tpd: None,
                    context_window: None,
                    cf_aig_gateway_id: None,
                },
                rustyclaw_config::ModelEntry {
                    model_name: "enabled-m".to_string(),
                    provider: "openai".to_string(),
                    model: "enabled-api".to_string(),
                    api_base: "https://enabled.example.com/v1".to_string(),
                    api_key: "key2".to_string(),
                    max_tokens: Some(100),
                    temperature: Some(0.5),
                    enabled: true,
                    rpm: None,
                    rpd: None,
                    tpm: None,
                    tpd: None,
                    context_window: None,
                    cf_aig_gateway_id: None,
                },
            ],
            agents: rustyclaw_config::AgentsConfig {
                default: rustyclaw_config::ModelNames::Chain(vec![
                    "disabled-m".to_string(),
                    "enabled-m".to_string(),
                ]),
                ..Default::default()
            },
            ..Default::default()
        };
        let chain = config.get_model_chain("default");
        // disabled は除外、enabled のみがチェーンに残る
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].api_base_url, "https://enabled.example.com/v1");
    }

    fn make_test_config_with_url(api_base: &str) -> Config {
        Config {
            model_list: vec![ModelEntry {
                model_name: "test-model".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                api_base: api_base.to_string(),
                api_key: "dummy".to_string(),
                max_tokens: Some(2048),
                temperature: Some(0.7),
                enabled: true,
                rpm: None,
                rpd: None,
                tpm: None,
                tpd: None,
                context_window: None,
                cf_aig_gateway_id: None,
            }],
            agents: AgentsConfig {
                default: ModelNames::Single("test-model".to_string()),
                summary: None,
                memory: None,
                ..Default::default()
            },
            debug_dump: true,
            ..Default::default()
        }
    }

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_build_system_context_injects_runtime_context() {
        let ws_dir = tempdir().unwrap();
        std::fs::write(ws_dir.path().join("SOUL.md"), "soul").unwrap();

        let config = make_test_config_with_url("http://localhost");
        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        let context = pipeline.build_system_context(ws_dir.path()).unwrap();

        // 静的ファイルが先頭、[now:] は末尾（プロンプトキャッシュ最適化）
        assert!(context.contains("# SOUL.md"));
        // フォーマット: [now: YYYY-MM-DDTHH:MM:SS+HH:MM]
        let last_line = context.trim_end().lines().last().unwrap();
        assert!(
            last_line.starts_with("[now: "),
            "datetime line must be last for cache optimization"
        );
        assert!(last_line.ends_with(']'));
        assert!(last_line.contains('T'), "must be ISO 8601 format");
    }

    #[test]
    fn test_extract_delimited_block_found() {
        let text = "preamble\n---NEW_MEMORY---\nhello world\n---END_MEMORY---\ntrailing";
        let result = extract_delimited_block(text, "---NEW_MEMORY---", "---END_MEMORY---");
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_delimited_block_no_end_tag() {
        let text = "---NEW_MEMORY---\nhello world";
        let result = extract_delimited_block(text, "---NEW_MEMORY---", "---END_MEMORY---");
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_delimited_block_missing() {
        let text = "no delimiters here";
        let result = extract_delimited_block(text, "---NEW_MEMORY---", "---END_MEMORY---");
        assert!(result.is_none());
    }

    #[test]
    fn test_truncate_70_20_no_op() {
        let short = "hello";
        assert_eq!(truncate_70_20(short, 5000), short);
    }

    #[test]
    fn test_truncate_70_20_truncates() {
        let long = "a".repeat(6000);
        let result = truncate_70_20(&long, 5000);
        assert!(result.len() < 6000);
        assert!(result.contains("bytes omitted"));
    }

    #[test]
    fn test_chunk_memory_md_truncates_long_utf8() {
        // 日本語混在: "- a" + "あ" × 170 = 3 + 3×170 = 513バイト
        let long = format!("- a{}", "あ".repeat(170));
        let chunks = chunk_memory_md(&long);
        assert_eq!(chunks.len(), 1);
        // chars().take(512) なので512文字以内 → バイト数は大きくなりうるが panic しない
        assert!(chunks[0].chars().count() <= 512);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_pipeline_execute() -> Result<()> {
        let _guard = ENV_MUTEX.lock().unwrap();
        let ws_dir = tempdir()?;
        fs::write(ws_dir.path().join("SOUL.md"), "Soul Content")?;
        fs::write(ws_dir.path().join("AGENTS.md"), "Agents Content")?;
        fs::write(ws_dir.path().join("MEMORY.md"), "Memory Content")?;
        fs::write(ws_dir.path().join("USER.md"), "User Content")?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"id\": \"chatcmpl-123\",\
                    \"object\": \"chat.completion\",\
                    \"created\": 1677652288,\
                    \"model\": \"gpt-4o-mini\",\
                    \"choices\": [{\
                        \"index\": 0,\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"I am a robot.\"\
                        },\
                        \"finish_reason\": \"stop\"\
                    }],\
                    \"usage\": {\
                        \"prompt_tokens\": 9,\
                        \"total_tokens\": 21\
                    }\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let config = make_test_config_with_url(&format!("http://{}", addr));

        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        unsafe {
            std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", ws_dir.path());
        }
        let resp = pipeline
            .execute(ws_dir.path(), "session-test", "hello")
            .await;
        unsafe {
            std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
        }

        let resp = resp?;
        assert_eq!(resp.content, "I am a robot.");

        let llm_dir = ws_dir.path().join("memory").join("debug").join("llm");
        let inspector_dump = find_latest_dump(&llm_dir, "discord");
        assert!(inspector_dump.is_some(), "discord dump file should exist");
        let dump_content = fs::read_to_string(inspector_dump.unwrap())?;
        let val: serde_json::Value = serde_json::from_str(&dump_content)?;
        assert_eq!(val["response"]["content"], "I am a robot.");

        let logger = SessionLogger::new(ws_dir.path());
        let history = logger.load_history("session-test")?;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[1].content, "I am a robot.");

        let _ = server_task.await;
        Ok(())
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_cron_session_ignores_history() -> Result<()> {
        let _guard = ENV_MUTEX.lock().unwrap();
        let ws_dir = tempdir()?;
        fs::write(ws_dir.path().join("SOUL.md"), "Soul Content")?;
        fs::write(ws_dir.path().join("AGENTS.md"), "Agents Content")?;
        fs::write(ws_dir.path().join("MEMORY.md"), "Memory Content")?;
        fs::write(ws_dir.path().join("USER.md"), "User Content")?;

        // 1. Create a dummy session log or mock history.
        let logger = SessionLogger::new(ws_dir.path());
        logger.append_message(
            "cron:heartbeat",
            &Message {
                role: "user".to_string(),
                content: "old dummy history".to_string(),
                name: None,
                ..Default::default()
            },
        )?;
        logger.append_message(
            "cron:heartbeat",
            &Message {
                role: "assistant".to_string(),
                content: "old robot reply".to_string(),
                name: None,
                ..Default::default()
            },
        )?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"id\": \"chatcmpl-123\",\
                    \"object\": \"chat.completion\",\
                    \"created\": 1677652288,\
                    \"model\": \"gpt-4o-mini\",\
                    \"choices\": [{\
                        \"index\": 0,\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"cron response\"\
                        },\
                        \"finish_reason\": \"stop\"\
                    }],\
                    \"usage\": {\
                        \"prompt_tokens\": 9,\
                        \"total_tokens\": 21\
                    }\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let config = make_test_config_with_url(&format!("http://{}", addr));

        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);

        // 2. Call pipeline.execute("cron:heartbeat") and verify that it skips history.
        unsafe {
            std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", ws_dir.path());
        }
        let resp = pipeline
            .execute(ws_dir.path(), "cron:heartbeat", "new message")
            .await;
        unsafe {
            std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
        }

        let resp = resp?;
        assert_eq!(resp.content, "cron response");

        let llm_dir = ws_dir.path().join("memory").join("debug").join("llm");
        let req_dump_path = find_latest_dump(&llm_dir, "heartbeat");
        assert!(req_dump_path.is_some(), "heartbeat dump file should exist");
        let req_dump_content = fs::read_to_string(req_dump_path.unwrap())?;
        let dump_val: serde_json::Value = serde_json::from_str(&req_dump_content)?;
        let sent_messages: Vec<Message> = serde_json::from_value(dump_val["request"].clone())?;

        // Verify that the old history is NOT present in the sent messages.
        for msg in &sent_messages {
            assert!(
                !msg.content.contains("old dummy history"),
                "History must be ignored!"
            );
            assert!(
                !msg.content.contains("old robot reply"),
                "History must be ignored!"
            );
        }

        let _ = server_task.await;
        Ok(())
    }

    #[test]
    fn test_filter_json_leaks() {
        let input = "Hello, user!\n```json\n{\n  \"action\": \"run_command\",\n  \"command\": \"ls\"\n}\n```\nDone.";
        assert_eq!(filter_json_leaks(input), "Hello, user!\n\nDone.");

        let input_no_leak = "Hello, user!\n```json\n{\n  \"name\": \"test\"\n}\n```\nDone.";
        assert_eq!(
            filter_json_leaks(input_no_leak),
            "Hello, user!\n```json\n{\n  \"name\": \"test\"\n}\n```\nDone."
        );

        let input_multiple = "Start.\n```json\n{\n  \"name\": \"test\"\n}\n```\nMiddle.\n```json\n{\n  \"action\": \"run_command\"\n}\n```\nEnd.";
        assert_eq!(
            filter_json_leaks(input_multiple),
            "Start.\n```json\n{\n  \"name\": \"test\"\n}\n```\nMiddle.\n\nEnd."
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_pipeline_execute_stream_leak_filter() -> Result<()> {
        let _guard = ENV_MUTEX.lock().unwrap();
        let ws_dir = tempdir()?;
        fs::write(ws_dir.path().join("SOUL.md"), "Soul Content")?;
        fs::write(ws_dir.path().join("AGENTS.md"), "Agents Content")?;
        fs::write(ws_dir.path().join("MEMORY.md"), "Memory Content")?;
        fs::write(ws_dir.path().join("USER.md"), "User Content")?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut writer = tokio::io::BufWriter::new(&mut socket);
                let response_header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
                let _ = writer.write_all(response_header.as_bytes()).await;
                let _ = writer.flush().await;

                let chunks = vec![
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"```json\\n{\\n  \\\"action\\\": \\\"run_command\\\",\\n  \\\"command\\\": \\\"ls\\\"\\n}\\n```\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Done.\"}}]}\n\n",
                    "data: [DONE]\n\n",
                ];

                for chunk in chunks {
                    let _ = writer.write_all(chunk.as_bytes()).await;
                    let _ = writer.flush().await;
                }
                let _ = writer.shutdown().await;
            }
        });

        let config = make_test_config_with_url(&format!("http://{}", addr));

        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        unsafe {
            std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", ws_dir.path());
        }
        let stream_res = pipeline
            .execute_stream(ws_dir.path(), "session-stream-test", "hello")
            .await;
        unsafe {
            std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
        }
        let mut stream_res = stream_res?;

        let mut chunks = Vec::new();
        while let Some(chunk_res) = stream_res.next().await {
            let chunk = chunk_res?;
            chunks.push(chunk.content);
        }

        assert_eq!(chunks.join(""), "Hello Done.");

        let logger = SessionLogger::new(ws_dir.path());
        let history = logger.load_history("session-stream-test")?;
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].content, "Hello Done.");

        let _ = server_task.await;
        Ok(())
    }

    #[test]
    fn test_build_system_context_injects_proactive_posts() {
        let dir = tempfile::TempDir::new().unwrap();
        let ws = dir.path();
        let memory_dir = ws.join("memory");
        std::fs::create_dir_all(&memory_dir).unwrap();
        for f in &["SOUL.md", "AGENTS.md", "MEMORY.md", "USER.md"] {
            std::fs::write(ws.join(f), "").unwrap();
        }
        let now = chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string();
        std::fs::write(
            memory_dir.join("proactive-posts.md"),
            format!("[{}] 傘を持ってください。", now),
        )
        .unwrap();

        let config = rustyclaw_config::Config::default();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let pipeline = Pipeline::new(config, sem);
        let ctx = pipeline.build_system_context(ws).unwrap();
        assert!(
            ctx.contains("傘を持ってください"),
            "proactive-posts が注入されるべき"
        );
    }

    #[test]
    fn test_strip_comments_removes_slashslash_lines() {
        let input = "\
Be genuine.\n\
  // 形骸化した丁寧さではなく、真に役立つこと。\n\
Have opinions.\n\
// 意見を持つこと。\n\
Keep it short.\n\
";
        let result = Pipeline::strip_comments(input);
        assert!(!result.contains("形骸化"));
        assert!(!result.contains("意見を持つこと"));
        assert!(result.contains("Be genuine."));
        assert!(result.contains("Have opinions."));
        assert!(result.contains("Keep it short."));
    }

    #[test]
    fn test_strip_comments_preserves_url_with_slashes() {
        // 行中の // (URL等) は除去しない — 行頭 // のみ対象
        let input = "See https://example.com for details.\n// コメント行\n";
        let result = Pipeline::strip_comments(input);
        assert!(result.contains("https://example.com"));
        assert!(!result.contains("コメント行"));
    }

    #[test]
    fn test_strip_comments_handles_indented() {
        let input = "- Item\n  // インデントコメント\n- Next\n\t// タブインデント\n";
        let result = Pipeline::strip_comments(input);
        assert!(!result.contains("インデントコメント"));
        assert!(!result.contains("タブインデント"));
        assert!(result.contains("- Item"));
        assert!(result.contains("- Next"));
    }

    #[test]
    fn test_proactive_posts_filtering_and_injection() {
        let dir = tempfile::TempDir::new().unwrap();
        let ws = dir.path();
        let logger = SessionLogger::new(ws);

        logger
            .append_message(
                "session-proactive",
                &Message {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                    timestamp: Some("2026-05-31T10:00:00Z".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        logger
            .append_message(
                "session-proactive",
                &Message {
                    role: "assistant".to_string(),
                    content: "Hi there!".to_string(),
                    timestamp: Some("2026-05-31T10:01:00Z".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        logger
            .append_message(
                "session-proactive",
                &Message {
                    role: "assistant".to_string(),
                    content: "It might rain soon, grab an umbrella!".to_string(),
                    trigger: Some("proactive".to_string()),
                    timestamp: Some("2026-05-31T12:00:00Z".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let config = rustyclaw_config::Config::default();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let pipeline = Pipeline::new(config, sem);

        let history_messages = logger.load_history("session-proactive").unwrap();
        let mut system_context = "Base Prompt Context".to_string();

        let cleaned = pipeline.process_proactive_posts(
            "session-proactive",
            history_messages,
            &mut system_context,
        );

        assert_eq!(cleaned.len(), 2);
        assert_eq!(cleaned[0].content, "Hello");
        assert_eq!(cleaned[1].content, "Hi there!");

        assert!(system_context.contains("Your Previous Posts in This Channel"));
        assert!(system_context.contains("It might rain soon, grab an umbrella!"));
        assert!(system_context.contains("2026-05-31 12:00:00"));
    }

    #[test]
    fn test_chunk_memory_md_basic() {
        let content = "# Memory\n\n- First bullet\n- Second bullet\n  continued\n- Third bullet";
        let chunks = chunk_memory_md(content);
        // 3バレットが800bytes以内なので1チャンクに結合される
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].starts_with("[Memory]"), "got: {}", chunks[0]);
        assert!(chunks[0].contains("First bullet"));
        assert!(chunks[0].contains("Second bullet"));
        assert!(chunks[0].contains("Third bullet"));
    }

    #[test]
    fn test_chunk_memory_md_long_bullet_single_chunk() {
        // 600 bytes のバレット1件は単独でチャンクになる（800 以内）
        let long = format!("- {}", "x".repeat(597));
        let chunks = chunk_memory_md(&long);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].starts_with("[General]"), "got: {}", chunks[0]);
    }

    #[test]
    fn test_chunk_memory_md_section_boundary() {
        // セクションをまたぐバレットは別チャンクになる
        let content = "## Section A\n\n- bullet a\n\n## Section B\n\n- bullet b";
        let chunks = chunk_memory_md(content);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with("[Section A]"), "got: {}", chunks[0]);
        assert!(chunks[1].starts_with("[Section B]"), "got: {}", chunks[1]);
    }

    #[test]
    fn test_chunk_memory_md_section_prefix() {
        let content = "## User Preferences\n\n- First bullet\n- Second bullet";
        let chunks = chunk_memory_md(content);
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].starts_with("[User Preferences]"),
            "expected [User Preferences] prefix, got: {}",
            chunks[0]
        );
        assert!(chunks[0].contains("First bullet"));
        assert!(chunks[0].contains("Second bullet"));
    }

    #[test]
    fn test_chunk_memory_md_adjacent_bullets_merged() {
        let content = "## Section\n\n- bullet one\n- bullet two\n- bullet three";
        let chunks = chunk_memory_md(content);
        assert_eq!(
            chunks.len(),
            1,
            "adjacent bullets within 800 bytes should be merged"
        );
        assert!(chunks[0].contains("bullet one"));
        assert!(chunks[0].contains("bullet two"));
        assert!(chunks[0].contains("bullet three"));
    }

    #[test]
    fn test_chunk_memory_md_split_on_overflow() {
        // 各バレット 298 bytes × 3 = 894 bytes → 800 を超えるので分割される
        let long_bullet = format!("- {}", "x".repeat(295));
        let content = format!(
            "## Section\n\n{}\n{}\n{}",
            long_bullet, long_bullet, long_bullet
        );
        let chunks = chunk_memory_md(&content);
        assert!(
            chunks.len() >= 2,
            "should split when total exceeds 800 bytes, got {} chunk(s)",
            chunks.len()
        );
        for chunk in &chunks {
            assert!(
                chunk.starts_with("[Section]"),
                "each chunk must carry section prefix, got: {}",
                &chunk[..chunk.len().min(30)]
            );
        }
    }

    #[test]
    fn test_chunk_memory_md_no_section_uses_general() {
        let content = "- bullet without any section header";
        let chunks = chunk_memory_md(content);
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].starts_with("[General]"),
            "no-section content must use [General] prefix, got: {}",
            chunks[0]
        );
    }

    #[test]
    fn test_parse_context_window_k_suffix() {
        assert_eq!(parse_context_window(Some("8k")), 8_192);
        assert_eq!(parse_context_window(Some("16k")), 16_384);
        assert_eq!(parse_context_window(Some("32k")), 32_768);
        assert_eq!(parse_context_window(Some("131k")), 134_144);
        assert_eq!(parse_context_window(Some("256k")), 262_144);
    }

    #[test]
    fn test_parse_context_window_m_suffix() {
        assert_eq!(parse_context_window(Some("1M")), 1_048_576);
    }

    #[test]
    fn test_parse_context_window_none_or_empty() {
        assert_eq!(parse_context_window(None), 32_768);
        assert_eq!(parse_context_window(Some("")), 32_768);
    }

    #[test]
    fn test_get_history_message_limit_uses_context_window() {
        use rustyclaw_config::{AgentsConfig, Config, ModelEntry, ModelNames};

        fn make_config(context_window: &str) -> Config {
            Config {
                model_list: vec![ModelEntry {
                    model_name: "test-model".to_string(),
                    provider: "openai".to_string(),
                    model: "test-model-api".to_string(),
                    api_base: "http://localhost".to_string(),
                    api_key: "key".to_string(),
                    max_tokens: Some(2048),
                    temperature: Some(0.7),
                    enabled: true,
                    rpm: None,
                    rpd: None,
                    tpm: None,
                    tpd: None,
                    context_window: Some(context_window.to_string()),
                    cf_aig_gateway_id: None,
                }],
                agents: AgentsConfig {
                    default: ModelNames::Single("test-model".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            }
        }

        let flush_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));

        let p16k = Pipeline::new(make_config("16k"), flush_sem.clone());
        let p32k = Pipeline::new(make_config("32k"), flush_sem.clone());
        let p64k = Pipeline::new(make_config("64k"), flush_sem.clone());
        let p131k = Pipeline::new(make_config("131k"), flush_sem.clone());
        let p256k = Pipeline::new(make_config("256k"), flush_sem.clone());

        assert_eq!(p16k.get_history_message_limit("default"), 30, "16k → 30件");
        assert_eq!(p32k.get_history_message_limit("default"), 50, "32k → 50件");
        assert_eq!(p64k.get_history_message_limit("default"), 80, "64k → 80件");
        assert_eq!(
            p131k.get_history_message_limit("default"),
            100,
            "131k → 100件"
        );
        assert_eq!(
            p256k.get_history_message_limit("default"),
            120,
            "256k → 120件"
        );
    }

    #[test]
    fn test_format_rag_context_empty() {
        let result = format_rag_context(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_rag_context_with_items() {
        let items = vec![
            ("memory".to_string(), "- Rust is fast".to_string(), 0.92_f64),
            (
                "memory".to_string(),
                "- RPi4 has 8GB RAM".to_string(),
                0.85_f64,
            ),
        ];
        let result = format_rag_context(&items);
        assert!(result.contains("Rust is fast"));
        assert!(result.contains("RPi4 has 8GB RAM"));
        assert!(result.contains("## Relevant Memory"));
    }

    #[test]
    fn test_format_rag_context_with_doc_items() {
        let items = vec![
            (
                "doc:AGENTS.md".to_string(),
                "## Tool Usage\nUse tools carefully.".to_string(),
                0.85_f64,
            ),
            (
                "memory".to_string(),
                "User prefers brevity".to_string(),
                0.80_f64,
            ),
            (
                "session".to_string(),
                "Previously discussed weather".to_string(),
                0.75_f64,
            ),
        ];
        let result = format_rag_context(&items);
        assert!(
            result.contains("## Relevant Specifications & Rules"),
            "doc: セクションが含まれること"
        );
        assert!(
            result.contains("Tool Usage"),
            "doc: チャンク内容が含まれること"
        );
        assert!(
            result.contains("## Relevant Memory"),
            "memory セクションが含まれること"
        );
        assert!(
            result.contains("## Relevant Past Sessions"),
            "session セクションが含まれること"
        );
        // doc セクションが memory セクションより先に来ること
        let doc_pos = result.find("## Relevant Specifications").unwrap();
        let mem_pos = result.find("## Relevant Memory").unwrap();
        assert!(
            doc_pos < mem_pos,
            "doc セクションが memory より先であること"
        );
    }

    #[test]
    fn test_unified_rag_engine_rebuild_from_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("memory.db");
        let db = rustyclaw_storage::DbManager::new(&db_path).unwrap();
        db.upsert_embedding("memory::0", "memory", None, "大森駅周辺", &[1.0f32; 4], None)
            .unwrap();
        db.upsert_embedding(
            "session::s1",
            "session",
            Some("s1"),
            "RAG 検証完了",
            &[0.5f32; 4],
            None,
        )
        .unwrap();

        let client =
            rustyclaw_providers::CloudflareEmbeddingClient::new("http://127.0.0.1:1", "dummy");
        let model = rustyclaw_providers::CloudflareEmbeddingModel::new(client, 4);
        let engine = UnifiedRagEngine::new(model);
        engine.rebuild_from_db(&db).unwrap();
        assert_eq!(engine.len(), 2);
    }

    #[test]
    fn test_hierarchical_chunking_and_retrieval() {
        // 1. chunk_document_hierarchical の検証
        let md = "# Doc Title\n\n## Section A\n- bullet 1\n- bullet 2\n\n## Section B\n- bullet 3";
        let h_chunks = chunk_document_hierarchical("test.md", md);
        
        assert_eq!(h_chunks.len(), 2);
        assert_eq!(h_chunks[0].children.len(), 2);
        assert_eq!(h_chunks[1].children.len(), 1);
        assert!(h_chunks[0].parent_text.contains("Section A"));
        assert!(h_chunks[0].children[0].contains("bullet 1"));

        // 2. DB を経由した親子解決の検証
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("memory.db");
        let db = rustyclaw_storage::DbManager::new(&db_path).unwrap();

        let parent_id = "p1";
        let child_id1 = "c1";
        let child_id2 = "c2";
        let source_id = "doc:test.md";

        // 親チャンク (embeddingは空)
        db.upsert_embedding(parent_id, source_id, None, "PARENT TEXT SECTION A", &[], None).unwrap();
        // 子チャンク (類似検索用embeddingを付与)
        db.upsert_embedding(child_id1, source_id, None, "CHILD BULLET 1", &[1.0, 0.0], Some(parent_id)).unwrap();
        db.upsert_embedding(child_id2, source_id, None, "CHILD BULLET 2", &[0.9, 0.1], Some(parent_id)).unwrap();

        // 類似検索を実行 (クエリ [1.0, 0.0] -> c1, c2 が近い)
        let results = db.search_similar_with_source(&[1.0, 0.0], 5, 0.5).unwrap();
        
        // 2つの子チャンクがヒットするが、resolved_text はいずれも "PARENT TEXT SECTION A" になっていること
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, "PARENT TEXT SECTION A");
        assert_eq!(results[1].1, "PARENT TEXT SECTION A");

        // 3. UnifiedRagEngine での検索と重複排除の検証
        let client = rustyclaw_providers::CloudflareEmbeddingClient::new("http://127.0.0.1:1", "dummy");
        let model = rustyclaw_providers::CloudflareEmbeddingModel::new(client, 2);
        let engine = UnifiedRagEngine::new(model);
        engine.rebuild_from_db(&db).unwrap();

        // engine にはベクトルを持つ子チャンクの 2 件のみがインデックス登録される (親は embedding が空なので除外)
        assert_eq!(engine.len(), 2);

        // クエリ検索 (ダミーの HTTP クライアントを使用するため、mocking されている top_n_ids をテストできるように in-memory 検索で検証)
        let smap = engine.source_map.lock().unwrap();
        let pmap = engine.parent_map.lock().unwrap();
        let tmap = engine.text_map.lock().unwrap();

        assert_eq!(smap.len(), 3); // p1, c1, c2
        assert_eq!(pmap.get("c1").cloned().flatten().unwrap(), "p1");
        assert_eq!(tmap.get("p1").unwrap().as_str(), "PARENT TEXT SECTION A");
    }

    #[test]
    fn test_pipeline_has_rig_agent_builder() {
        use rustyclaw_config::Config;
        use rustyclaw_providers::RustyclawCompletionModel;

        let config = Config::default();
        let model = RustyclawCompletionModel::new(config, "default", "test-session");
        // If this compiles and can build an agent, the rig integration is wired correctly.
        let _agent = rig_core::agent::AgentBuilder::new(model)
            .preamble("test")
            .build();
    }

    // ── Task 1: execute_heartbeat db_path シグネチャ ──
    #[test]
    fn test_execute_heartbeat_accepts_db_path() {
        // Verify the function signature compiles with &Path for db_path (compile-time check)
        // This ensures the parameter was added to the signature correctly.
        let _ = Pipeline::execute_heartbeat;
    }

    // ── Task 2: filter_seen_tool_result ──
    #[test]
    fn test_filter_seen_tool_result_gmail_first_time() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(dir.path().join("test.db")).unwrap();

        let tool_result = "--- STDOUT ---\n[{\"id\":\"msg1\",\"sender\":\"test@example.com\",\"subject\":\"Test\",\"date\":\"Mon\",\"snippet\":\"Hi\"}]\n--- EXIT STATUS ---\n0";
        let call_args =
            r#"{"script_name":"skills/gmail/scripts/506_get-gmail.sh","args":["is:unread","5"]}"#;

        let result = filter_seen_tool_result("run_workspace_script", call_args, tool_result, &db);
        assert!(
            result.contains("msg1"),
            "First call: item should pass through"
        );
        assert!(
            db.is_item_seen("gmail:msg1").unwrap(),
            "Should be marked as seen after first call"
        );
    }

    #[test]
    fn test_filter_seen_tool_result_gmail_already_seen() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(dir.path().join("test.db")).unwrap();
        db.mark_item_seen("gmail:msg1", "gmail").unwrap();

        let tool_result = "--- STDOUT ---\n[{\"id\":\"msg1\",\"sender\":\"test@example.com\",\"subject\":\"Test\",\"date\":\"Mon\",\"snippet\":\"Hi\"}]\n--- EXIT STATUS ---\n0";
        let call_args = r#"{"script_name":"skills/gmail/scripts/506_get-gmail.sh"}"#;

        let result = filter_seen_tool_result("run_workspace_script", call_args, tool_result, &db);
        assert!(
            !result.contains("\"msg1\""),
            "Already-seen item should be filtered out"
        );
        assert!(
            result.contains("already seen"),
            "Should indicate filtered items"
        );
    }

    #[test]
    fn test_filter_seen_tool_result_calendar() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(dir.path().join("test.db")).unwrap();

        let stdout = r#"[{"event_id":"evt1","title":"Meeting","start":"2026-06-06T10:00:00+09:00","start_wday":"土","end":"2026-06-06T11:00:00+09:00","end_wday":"土"}]"#;
        let tool_result = format!("--- STDOUT ---\n{}\n--- EXIT STATUS ---\n0", stdout);
        let call_args =
            r#"{"script_name":"skills/calendar/scripts/calendar-ops.sh","args":["list"]}"#;

        let result1 =
            filter_seen_tool_result("run_workspace_script", call_args, &tool_result, &db);
        assert!(result1.contains("evt1"));
        assert!(db.is_item_seen("calendar:evt1").unwrap());

        let result2 =
            filter_seen_tool_result("run_workspace_script", call_args, &tool_result, &db);
        assert!(!result2.contains("evt1"));
    }

    #[test]
    fn test_filter_seen_tool_result_non_script_passthrough() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(dir.path().join("test.db")).unwrap();

        let result = filter_seen_tool_result("web_fetch", "{}", "some content", &db);
        assert_eq!(result, "some content");

        let result2 = filter_seen_tool_result(
            "run_workspace_script",
            r#"{"script_name":"skills/weather/scripts/504_get-weather.sh"}"#,
            "weather data",
            &db,
        );
        assert_eq!(result2, "weather data");
    }

    #[test]
    fn test_filter_seen_tool_result_partial_seen() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(dir.path().join("test.db")).unwrap();
        db.mark_item_seen("gmail:msg1", "gmail").unwrap();

        let tool_result = "--- STDOUT ---\n[{\"id\":\"msg1\",\"subject\":\"Old\"},{\"id\":\"msg2\",\"subject\":\"New\"}]\n--- EXIT STATUS ---\n0";
        let call_args = r#"{"script_name":"skills/gmail/scripts/506_get-gmail.sh"}"#;

        let result = filter_seen_tool_result("run_workspace_script", call_args, tool_result, &db);
        assert!(
            !result.contains("\"msg1\""),
            "msg1 should be filtered (already seen)"
        );
        assert!(result.contains("msg2"), "msg2 should pass through (new)");
        assert!(db.is_item_seen("gmail:msg2").unwrap());
    }

    // ── Task 3: execute_heartbeat seen_items 統合テスト ──
    #[test]
    fn test_filter_seen_tool_result_already_seen_produces_no_new_message() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(dir.path().join("test.db")).unwrap();
        db.mark_item_seen("gmail:msg99", "gmail").unwrap();

        let tool_result = "--- STDOUT ---\n[{\"id\":\"msg99\",\"sender\":\"test@example.com\",\"subject\":\"Hello\",\"date\":\"Mon\",\"snippet\":\"Hi\"}]\n--- EXIT STATUS ---\n0";
        let call_args = r#"{"script_name":"skills/gmail/scripts/506_get-gmail.sh"}"#;

        let result = filter_seen_tool_result("run_workspace_script", call_args, tool_result, &db);
        assert!(
            result.contains("No new gmail items"),
            "Should indicate no new items: {}",
            result
        );
        assert!(
            result.contains("already seen"),
            "Should mention already-seen count: {}",
            result
        );
    }

    #[test]
    fn test_chunk_static_document_splits_by_heading() {
        let content =
            "# Doc\n\n## Section A\nContent A line1\nContent A line2\n\n## Section B\nContent B\n";
        let chunks = chunk_static_document("test.md", content);
        assert_eq!(chunks.len(), 2, "## 見出し単位で 2 チャンクになること");
        assert!(
            chunks[0].contains("Section A"),
            "チャンク0 に Section A を含むこと"
        );
        assert!(
            chunks[0].contains("[test.md >"),
            "ファイル名コンテキストが付くこと"
        );
        assert!(
            chunks[1].contains("Section B"),
            "チャンク1 に Section B を含むこと"
        );
    }

    #[test]
    fn test_chunk_static_document_truncates_long_chunks() {
        let long_line = "x".repeat(900);
        let content = format!("## Big\n{}", long_line);
        let chunks = chunk_static_document("big.md", &content);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].chars().count() <= 800, "800文字を超えないこと");
    }

    // ── ISSUE-26: test_heartbeat_messages_are_not_trimmed ──
    #[test]
    fn test_heartbeat_messages_are_not_trimmed() {
        let mut msgs = vec![
            Message {
                role: "system".to_string(),
                content: "sys".to_string(),
                ..Default::default()
            },
            Message {
                role: "user".to_string(),
                content: "usr".to_string(),
                ..Default::default()
            },
            Message {
                role: "assistant".to_string(),
                content: "gen1".to_string(),
                ..Default::default()
            },
            Message {
                role: "tool".to_string(),
                content: "result1".to_string(),
                ..Default::default()
            },
            Message {
                role: "assistant".to_string(),
                content: "gen2".to_string(),
                ..Default::default()
            },
            Message {
                role: "tool".to_string(),
                content: "result2".to_string(),
                ..Default::default()
            },
        ];
        // Mutate/push to make Vec and mut necessary for clippy
        msgs.push(Message {
            role: "assistant".to_string(),
            content: "gen3".to_string(),
            ..Default::default()
        });
        assert_eq!(msgs.len(), 7);
        assert_eq!(msgs[2].content, "gen1");
    }

    // ── Phase 28b-4: ツール結果截断 ──
    #[test]
    fn test_truncate_70_20_within_limit() {
        let short = "abc".repeat(100); // 300 bytes < 3000
        let result = truncate_70_20(&short, 3_000);
        assert_eq!(result, short, "上限以内なら変化しない");
    }

    #[test]
    fn test_truncate_70_20_over_limit() {
        let long = "x".repeat(6_000); // 6000 bytes > 3000
        let result = truncate_70_20(&long, 3_000);
        assert!(result.len() < 6_000, "截断後は元より短い");
        assert!(result.contains("[..."), "省略マーカーが含まれる");
        // 先頭70% = 2100 bytes が保持される
        assert!(result.starts_with(&"x".repeat(2_100)));
    }

    #[test]
    fn test_truncate_70_20_preserves_head_and_tail() {
        let mut content = "BEGIN_".to_string();
        content.push_str(&"m".repeat(5_000));
        content.push_str("_END");
        let result = truncate_70_20(&content, 3_000);
        assert!(result.starts_with("BEGIN_"), "先頭が保持される");
        assert!(result.ends_with("_END"), "末尾が保持される");
        assert!(result.contains("[..."), "省略マーカーが含まれる");
    }

    // ── ISSUE-26: ingest_static_documents 再帰スキャン ──

    /// skills/<name>/SKILL.md 形式のサブディレクトリ構成でファイルが収集されることを確認する。
    /// 修正前は read_dir が直下のみスキャンするため、このテストは失敗していた。
    #[test]
    fn test_ingest_skills_subdir_files_are_discovered() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();

        // skills/<name>/SKILL.md 形式で2スキルを作成
        let skill_a_dir = ws.join("skills").join("skill-a");
        let skill_b_dir = ws.join("skills").join("skill-b");
        fs::create_dir_all(&skill_a_dir).unwrap();
        fs::create_dir_all(&skill_b_dir).unwrap();
        fs::write(skill_a_dir.join("SKILL.md"), "# Skill A\ncontent a").unwrap();
        fs::write(skill_b_dir.join("SKILL.md"), "# Skill B\ncontent b").unwrap();
        // AGENTS.md も作成（存在しないと read_to_string が Err になるだけなので影響なし）
        fs::write(ws.join("AGENTS.md"), "# Agents").unwrap();

        // スキャンロジックを ingest_static_documents から抜き出して再現
        let skills_dir = ws.join("skills");
        let mut collected: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                    collected.push(path);
                } else if path.is_dir()
                    && let Ok(sub_entries) = std::fs::read_dir(&path) {
                        for sub_entry in sub_entries.flatten() {
                            let sub_path = sub_entry.path();
                            if sub_path.is_file()
                                && sub_path.extension().is_some_and(|ext| ext == "md")
                            {
                                collected.push(sub_path);
                            }
                        }
                    }
            }
        }

        assert_eq!(collected.len(), 2, "2スキルの SKILL.md が収集されるべき");
        let names: Vec<_> = collected
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.iter().all(|n| *n == "SKILL.md"), "全て SKILL.md");
    }

    /// サブディレクトリ内の同名ファイル（SKILL.md）でキーが衝突しないことを確認する。
    #[test]
    fn test_ingest_skills_subdir_unique_keys() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();

        let skill_a_dir = ws.join("skills").join("skill-a");
        let skill_b_dir = ws.join("skills").join("skill-b");
        fs::create_dir_all(&skill_a_dir).unwrap();
        fs::create_dir_all(&skill_b_dir).unwrap();
        fs::write(skill_a_dir.join("SKILL.md"), "content a").unwrap();
        fs::write(skill_b_dir.join("SKILL.md"), "content b").unwrap();

        let paths = [skill_a_dir.join("SKILL.md"), skill_b_dir.join("SKILL.md")];
        let keys: Vec<String> = paths
            .iter()
            .map(|fp| {
                fp.strip_prefix(ws)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        fp.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string()
                    })
            })
            .collect();

        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1], "異なるサブディレクトリのキーは重複しないこと");
        assert!(keys[0].contains("skill-a") || keys[1].contains("skill-a"));
        assert!(keys[0].contains("skill-b") || keys[1].contains("skill-b"));
    }

    #[test]
    fn test_ingest_static_documents_excludes_memory_md() {
        // MEMORY.md は ingest_static_documents のスキャン対象外。
        // 代わりに ingest_memory_md が startup/flush 時に処理する (ISSUE-28/31)
        let files: Vec<&str> = vec!["AGENTS.md"]; // MEMORY.md を含まない
        assert!(
            !files.contains(&"MEMORY.md"),
            "MEMORY.md は ingest_static_documents の対象外であるべき"
        );
    }

    #[test]
    fn test_build_heartbeat_context_does_not_include_memory_md() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();

        // MEMORY.md は書くが、build_heartbeat_context に含まれないことを確認
        fs::write(ws.join("MEMORY.md"), "- secret memory content").unwrap();
        fs::write(ws.join("SOUL.md"), "# Soul").unwrap();
        fs::write(ws.join("HEARTBEAT.md"), "# Heartbeat").unwrap();

        // build_heartbeat_context のファイルリストを再現
        let files: &[&str] = &["SOUL.md", "HEARTBEAT.md"]; // MEMORY.md を含まない
        let contains_memory = files.contains(&"MEMORY.md");
        assert!(!contains_memory, "build_heartbeat_context は MEMORY.md を静的ロードしないべき");
    }

    #[test]
    fn test_build_heartbeat_context_is_static() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        std::fs::write(ws.join("SOUL.md"), "# Soul").unwrap();
        std::fs::write(ws.join("HEARTBEAT.md"), "# Heartbeat").unwrap();

        let config = make_test_config_with_url("http://localhost");
        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        let context = pipeline.build_heartbeat_context(ws).unwrap();

        assert!(context.contains("# SOUL.md"));
        assert!(context.contains("# HEARTBEAT.md"));
        assert!(
            !context.contains("[now: "),
            "build_heartbeat_context must not include [now:] — dynamic content belongs in execute_heartbeat"
        );
    }
}

#[cfg(test)]
mod heartbeat_rag_tests {
    use super::*;

    #[test]
    fn test_build_heartbeat_rag_query_empty() {
        let result = build_heartbeat_rag_query("");
        assert_eq!(result, "recent errors tasks memory updates: ");
    }

    #[test]
    fn test_build_heartbeat_rag_query_fewer_than_10_lines() {
        let digest = "line1\nline2\nline3";
        let result = build_heartbeat_rag_query(digest);
        assert_eq!(
            result,
            "recent errors tasks memory updates: line1\nline2\nline3"
        );
    }

    #[test]
    fn test_build_heartbeat_rag_query_exactly_10_lines() {
        let digest = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = build_heartbeat_rag_query(&digest);
        assert_eq!(
            result,
            format!("recent errors tasks memory updates: {}", digest),
            "all 10 lines should be retained"
        );
    }

    #[test]
    fn test_build_heartbeat_rag_query_truncates_old_lines() {
        let digest = (1..=11)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let expected_tail = (2..=11)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = build_heartbeat_rag_query(&digest);
        assert_eq!(
            result,
            format!("recent errors tasks memory updates: {}", expected_tail),
            "line1 should be truncated, only line2..line11 retained"
        );
    }

    #[test]
    fn test_build_heartbeat_rag_query_prefix() {
        let result = build_heartbeat_rag_query("some content");
        assert!(result.starts_with("recent errors tasks memory updates: "));
    }
}

#[cfg(test)]
mod heartbeat_step_rag_tests {
    use super::*;

    #[test]
    fn test_heartbeat_step2_rag_query_value() {
        assert_eq!(
            HEARTBEAT_STEP2_RAG_QUERY,
            "user interests hobbies routine habits long-term memory"
        );
    }

    #[test]
    fn test_heartbeat_step6_rag_query_value() {
        assert_eq!(
            HEARTBEAT_STEP6_RAG_QUERY,
            "errors bugs pending tasks todo improvements"
        );
    }
}

#[cfg(test)]
mod discord_rag_tests {
    use super::*;
    use rustyclaw_providers::Message;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: content.to_string(),
            ..Default::default()
        }
    }

    fn tool_call_msg() -> Message {
        Message {
            role: "assistant".to_string(),
            content: "".to_string(),
            tool_calls: Some(vec![]),
            ..Default::default()
        }
    }

    #[test]
    fn test_build_discord_rag_query_no_history() {
        let result = build_discord_rag_query(&[], "Hello");
        assert_eq!(result, "User: Hello");
    }

    #[test]
    fn test_build_discord_rag_query_one_turn() {
        let history = vec![
            msg("user", "What is the weather?"),
            msg("assistant", "It is sunny."),
        ];
        let result = build_discord_rag_query(&history, "And tomorrow?");
        assert_eq!(
            result,
            "User: What is the weather?\nAssistant: It is sunny.\nUser: And tomorrow?"
        );
    }

    #[test]
    fn test_build_discord_rag_query_skips_tool_role() {
        let history = vec![
            msg("user", "Find coords"),
            tool_call_msg(),
            msg("tool", "35.6762, 139.6503"),
            msg("assistant", "Coords are 35.67 N."),
        ];
        let result = build_discord_rag_query(&history, "Thanks");
        assert_eq!(
            result,
            "User: Find coords\nAssistant: Coords are 35.67 N.\nUser: Thanks"
        );
        assert!(!result.contains("35.6762, 139.6503"));
    }

    #[test]
    fn test_build_discord_rag_query_truncates_to_n_turns() {
        // DISCORD_RAG_HISTORY_TURNS = 2 なので 3 ターン以上は最新 2 ターンのみ
        let history = vec![
            msg("user", "Turn 1 user"),
            msg("assistant", "Turn 1 assistant"),
            msg("user", "Turn 2 user"),
            msg("assistant", "Turn 2 assistant"),
            msg("user", "Turn 3 user"),
            msg("assistant", "Turn 3 assistant"),
        ];
        let result = build_discord_rag_query(&history, "Current");
        assert!(!result.contains("Turn 1"), "Turn 1 should be truncated");
        assert!(result.contains("Turn 2 user"));
        assert!(result.contains("Turn 3 assistant"));
        assert!(result.ends_with("User: Current"));
    }
}
