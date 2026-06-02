use anyhow::{Context, Result};
use futures_util::{Stream, StreamExt};
use rustyclaw_config::Config;
use rustyclaw_providers::{LlmProvider, Message, StreamChunk, LlmResponse, CompletionOptions, create_provider};
use rustyclaw_storage::{SessionLogger, ConversationHistory};
use rustyclaw_tools::ToolRegistry;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

pub struct Pipeline {
    config: Config,
    provider: Box<dyn LlmProvider>,
    /// flush_memory() の同時実行を 1 本に制限するセマフォ
    flush_sem: Arc<Semaphore>,
}

impl Pipeline {
    pub fn new(config: Config, flush_sem: Arc<Semaphore>) -> Self {
        let provider = create_provider(config.get_model("default"));
        Self { config, provider, flush_sem }
    }

    /// モデルチェーンを走査し、最初に成功したレスポンスを返す。
    /// チェーン内の全モデルが失敗した場合はエラーを返す。
    /// フォールバックモデルが使用された場合は warn! ログを出力する。
    async fn complete_with_fallback(
        &self,
        purpose: &str,
        session_id: &str,
        messages: &[Message],
        tools: &[rustyclaw_providers::ToolDef],
        timeout: Duration,
    ) -> Result<LlmResponse> {
        let chain = self.config.get_model_chain(purpose);
        if chain.is_empty() {
            return Err(anyhow::anyhow!("no available models for purpose: {}", purpose));
        }

        let category = resolve_category(session_id);
        let mut last_err: Option<anyhow::Error> = None;

        for (idx, model_cfg) in chain.iter().enumerate() {
            let provider_id = if model_cfg.model_provider == "gmn" || model_cfg.model_provider == "gemini" {
                "gmn".to_string()
            } else {
                rustyclaw_providers::resolve_provider_id(model_cfg)
            };
            if let Some(remaining) = rustyclaw_providers::provider_cooldown_remaining(&provider_id) {
                tracing::info!(
                    model = %model_cfg.model_name,
                    provider = %provider_id,
                    remaining_secs = remaining.as_secs(),
                    "Provider is cooled down. Skipping in fallback chain..."
                );
                continue;
            }

            let opts = CompletionOptions {
                model: model_cfg.model_name.clone(),
                max_tokens: model_cfg.max_tokens,
                temperature: model_cfg.temperature,
                timeout,
                category: Some(category.clone()),
            };
            let provider = create_provider(model_cfg.clone());
            match provider.complete(messages, tools, &opts).await {
                Ok(response) => {
                    if idx > 0 {
                        tracing::warn!(
                            purpose = purpose,
                            used_model = %model_cfg.model_name,
                            primary_model = %chain[0].model_name,
                            fallback_index = idx,
                            "fallback model used"
                        );
                    }
                    return Ok(response);
                }
                Err(e) => {
                    tracing::warn!(
                        model = %model_cfg.model_name,
                        error = %e,
                        "model failed, trying next in chain"
                    );
                    last_err = Some(e.into());
                }
            }
        }

        Err(anyhow::anyhow!(
            "all models failed for purpose '{}': {}",
            purpose,
            last_err.map(|e| e.to_string()).unwrap_or_default()
        ))
    }

    /// 用途ごとのモデルの TPM 制限に基づいて、会話履歴の圧縮しきい値を動的に決定する。
    fn get_history_limit(&self, purpose: &str) -> usize {
        let model_cfg = self.config.get_model(purpose);
        let tpm = self.config.model_list.iter()
            .find(|m| m.model == model_cfg.model_name && m.enabled)
            .and_then(|m| m.tpm)
            .unwrap_or(40000); // 未設定の場合は十分大きな値をデフォルトとする

        if tpm <= 6000 {
            800
        } else if tpm <= 12000 {
            1500
        } else {
            3000
        }
    }

    /// TPM 上限に基づくメッセージ件数ハードキャップ。
    /// トークン推定のアンダーフロー問題の補完として、compact_if_needed_with_overhead 後に適用する。
    fn get_history_message_limit(&self, purpose: &str) -> usize {
        let model_cfg = self.config.get_model(purpose);
        let tpm = self.config.model_list.iter()
            .find(|m| m.model == model_cfg.model_name && m.enabled)
            .and_then(|m| m.tpm)
            .unwrap_or(40000);

        if tpm <= 6000 {
            10   // groq-llama-8b 等: 末尾 10 件 (~3K トークン)
        } else if tpm <= 12000 {
            20   // groq-llama-70b 等: 末尾 20 件 (~6K トークン)
        } else {
            10   // CF / LMS: neurons 節約のため末尾 10 件
        }
    }

    /// 各種人格定義ファイルを読み込んでシステムプロンプト（Context）を構築する
    /// 行頭が // で始まる行（インデント許容）を除去する
    fn strip_comments(content: &str) -> String {
        content.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn build_system_context(&self, workspace_dir: &Path) -> Result<String> {
        // 静的ブロック（SOUL/AGENTS/MEMORY/USER）を先に並べてプロンプトキャッシュの prefix を安定させる。
        // 動的な [now:] は末尾に置くことで毎回変わる部分がキャッシュ prefix を破壊しないようにする。
        let files = ["SOUL.md", "AGENTS.md", "MEMORY.md", "USER.md"];
        let mut context = String::new();

        for filename in &files {
            let path = workspace_dir.join(filename);
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!("Failed to read context file {:?}: {}. Using empty content.", path, e);
                    String::new()
                }
            };

            context.push_str(&format!("# {}\n\n{}\n\n", filename, Self::strip_comments(&content)));
        }

        // proactive-posts.md を注入（最終1件のみ）
        let posts_path = workspace_dir.join("memory").join("proactive-posts.md");
        if let Ok(posts) = fs::read_to_string(&posts_path) {
            if let Some(last_entry) = posts.lines().filter(|l| !l.trim().is_empty()).last() {
                context.push_str(&format!(
                    "# Recent AI Proactive Posts\n\n{}\n\n",
                    last_entry
                ));
            }
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
        let last_user_ts = history_messages.iter()
            .filter(|m| m.role == "user")
            .last()
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
            let mut proactive_block = String::from("\n### Your Previous Posts in This Channel\nYou posted these messages (not in your conversation history):\n");
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
    pub fn get_session_continuation_context(&self, workspace_dir: &Path, session_id: &str) -> Option<String> {
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
                    if let Some(date_str) = file_parts.last() {
                        if *date_str < *current_date_str {
                            previous_sessions.push((date_str.to_string(), filename));
                        }
                    }
                }
            }
        }

        previous_sessions.sort_by(|a, b| b.0.cmp(&a.0));
        let (prev_date_str, prev_filename) = previous_sessions.first()?;

        if prev_date_str.len() != 8 {
            return None;
        }
        let yyyy_mm_dd = format!("{}-{}-{}", &prev_date_str[0..4], &prev_date_str[4..6], &prev_date_str[6..8]);

        let prev_session_id = prev_filename.trim_end_matches(".jsonl");
        let safe_prev_session_id = prev_session_id.replace(':', "-");

        let summaries_dir = workspace_dir.join("memory").join("summaries");
        let mut summary_content = String::new();
        
        let specific_summary_path = summaries_dir.join(format!("{}-{}.md", yyyy_mm_dd, safe_prev_session_id));
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
                if filename.starts_with(&yyyy_mm_dd) && filename.ends_with(".md") {
                    if let Ok(c) = std::fs::read_to_string(entry.path()) {
                        summary_content = c;
                        break;
                    }
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
            context.push_str(&format!("## Yesterday's Session Summary ({})\n{}\n\n", yyyy_mm_dd, summary_content));
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
            ).await {
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
    /// execute / execute_with_tools 以外の経路は従来 record_usage されず Stats が過少計上だった。
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
        let history = logger.load_history(session_id)
            .context("Failed to load history for memory flush")?;

        let current_count = history.len();
        if current_count == 0 {
            return Ok(());
        }

        // delta >= 6（≒3ターン）かつ前回から 15 分以上経過した場合のみ実行
        const DELTA_THRESHOLD: usize = 6;
        const MIN_INTERVAL_MINS: i64 = 15;

        let count_key = format!("flush_count_{}", session_id);
        let ts_key    = format!("flush_ts_{}", session_id);

        let last_flushed_count = db.get_last_patrol_run(&count_key)?
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
            if let Ok(Some(ts_str)) = db.get_last_patrol_run(&ts_key) {
                if let Ok(last_ts) = chrono::DateTime::parse_from_rfc3339(&ts_str) {
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

        // 既存の MEMORY.md を読み込む（なければ空）
        let memory_path = workspace_dir.join("MEMORY.md");
        let existing_memory = fs::read_to_string(&memory_path).unwrap_or_default();

        let system_prompt = "\
You are a memory manager. Given the current MEMORY.md and a recent conversation, your tasks are:

1. Produce a fully rewritten MEMORY.md that:
   - Incorporates important new facts, decisions, preferences, and learnings from the conversation
   - Removes outdated or redundant information
   - Stays strictly under 5KB (≤ 5000 characters)
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
- Keep MEMORY.md total under 5000 characters.
- Write in the same language as the existing MEMORY.md content.
";

        let user_message = format!(
            "## Current MEMORY.md\n{}\n\n## Recent conversation\n{}",
            if existing_memory.is_empty() { "(empty)" } else { &existing_memory },
            conversation_text,
        );

        let memory_model = config.get_model("memory");
        let provider = create_provider(memory_model.clone());
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: system_prompt.to_string(),
                name: None,
                ..Default::default()
            },
            Message {
                role: "user".to_string(),
                content: user_message,
                name: None,
                ..Default::default()
            },
        ];

        let opts = CompletionOptions {
            model: memory_model.model_name,
            max_tokens: memory_model.max_tokens.or(Some(1500)), // MEMORY.md ≦ 1250 tok + daily log ≒ 100 tok
            temperature: memory_model.temperature.or(Some(0.3)),
            timeout: Duration::from_secs(300),
            category: Some("memory".to_string()),
        };

        let response = provider.complete(&messages, &[], &opts).await
            .context("LLM call failed for memory flush")?;

        // FU-4: memory flush の LLM 消費を usage テーブルへ計上（Stats 過少計上の解消）
        Self::record_aux_usage(&db, session_id, &response, "memory-flush");

        let response_text = response.content;

        let new_memory = extract_delimited_block(&response_text, "---NEW_MEMORY---", "---END_MEMORY---");
        let daily_log  = extract_delimited_block(&response_text, "---DAILY_LOG---",  "---END_DAILY_LOG---");

        // 1. MEMORY.md の全書き換え (fail-open)
        if let Some(content) = new_memory {
            // LLM が 5KB を超えて返した場合のフェイルセーフとして 70/20 トランケート
            let final_content = if content.len() > 5000 {
                tracing::warn!(
                    actual_bytes = content.len(),
                    "memory flush: LLM returned oversized MEMORY.md, truncating"
                );
                truncate_70_20(&content, 5000)
            } else {
                content
            };
            if let Err(e) = rustyclaw_storage::atomic_write(&memory_path, final_content.as_bytes()).await {
                tracing::warn!("memory flush: failed to write MEMORY.md: {}", e);
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

    /// Heartbeat 専用の軽量システムコンテキストを構築する（SOUL + MEMORY + HEARTBEAT のみ）
    /// 静的ファイルを先頭に、動的な [now:] を末尾に置くことでプロンプトキャッシュ prefix を安定させる。
    pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
        let files = ["SOUL.md", "MEMORY.md", "HEARTBEAT.md"];
        let mut context = String::new();
        for filename in &files {
            let path = workspace_dir.join(filename);
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!("Failed to read context file {:?}: {}. Using empty content.", path, e);
                    String::new()
                }
            };
            context.push_str(&format!("# {}\n\n{}\n\n", filename, Self::strip_comments(&content)));
        }
        // 動的ブロック（現在時刻）は末尾に配置
        let now = chrono::Local::now();
        context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));
        Ok(context)
    }

    /// Heartbeat 専用実行（軽量コンテキスト使用、履歴なし）
    pub async fn execute_heartbeat(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
    ) -> Result<LlmResponse> {
        let system_context = self.build_heartbeat_context(workspace_dir)?;

        let logger = SessionLogger::new(workspace_dir);
        let mut messages = Vec::new();
        messages.push(Message {
            role: "system".to_string(),
            content: system_context,
            name: None,
            ..Default::default()
        });
        let user_msg = Message {
            role: "user".to_string(),
            content: user_message.to_string(),
            name: None,
            ..Default::default()
        };
        messages.push(user_msg.clone());

        self.dump_request(workspace_dir, &messages);

        let mut response = self.complete_with_fallback("heartbeat", session_id, &messages, &[], Duration::from_secs(900)).await?;
        response.content = filter_json_leaks(&response.content);

        logger.append_message(session_id, &user_msg)
            .context("Failed to save user message in session log (fail-closed)")?;
        let assistant_msg = Message {
            role: response.role.clone(),
            content: response.content.clone(),
            name: None,
            ..Default::default()
        };
        logger.append_message(session_id, &assistant_msg)
            .context("Failed to save assistant response in session log (fail-closed)")?;

        self.dump_response(workspace_dir, &response.content, &response.role);

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
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id) {
            system_context.push_str(&continuation);
        }

        // 1. 過去履歴のロードとトークン圧縮処理の適用
        let logger = SessionLogger::new(workspace_dir);
        let history_messages = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger.load_history(session_id)
                .context("Failed to load session history messages")?
        };

        let cleaned_history = self.process_proactive_posts(session_id, history_messages, &mut system_context);

        let mut history = ConversationHistory::new(cleaned_history);
        let history_limit = self.get_history_limit("default");
        // ISSUE-02/FU-3: system プロンプト分のトークンを考慮して実効上限を下げる（ツール無し経路）
        let overhead_tokens = (system_context.chars().count() * 3) / 2;
        history.compact_if_needed_with_overhead(history_limit, overhead_tokens);
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
        let mut response = self.provider.complete(&messages, &[], &opts).await?;
        response.content = filter_json_leaks(&response.content);

        // 4. 会話ログの保存 (fail-closed)
        logger.append_message(session_id, &user_msg)
            .context("Failed to save user message in session log (fail-closed)")?;
        let assistant_msg = Message {
            role: response.role.clone(),
            content: response.content.clone(),
            name: None,
            ..Default::default()
        };
        logger.append_message(session_id, &assistant_msg)
            .context("Failed to save assistant response in session log (fail-closed)")?;

        self.dump_response(workspace_dir, &response.content, &response.role);

        if !session_id.starts_with("cron") {
            self.trigger_memory_flush_async(workspace_dir, session_id);
        }

        Ok(response)
    }

    /// セッション単位のサマリーを生成または増分更新する
    pub async fn generate_session_summary(&self, workspace_dir: &Path, target_session_id: &str) -> Result<String> {
        let logger = SessionLogger::new(workspace_dir);
        let history = logger.load_history(target_session_id)
            .context("Failed to load history for session summary")?;

        if history.is_empty() {
            return Err(anyhow::anyhow!("Cannot generate summary for empty session history"));
        }

        let safe_session_id = target_session_id.replace(':', "-");
        let session_file = workspace_dir.join("sessions").join(format!("{}.jsonl", safe_session_id));
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
        let existing_summary_path = summaries_dir.join(format!("{}-{}.md", session_date, safe_session_id));

        let mut existing_content = String::new();
        let mut existing_turns = 0;

        if existing_summary_path.exists() {
            if let Ok(content) = fs::read_to_string(&existing_summary_path) {
                existing_content = content;
                if let Some(idx) = existing_content.rfind("<!-- turns: ") {
                    let sub = &existing_content[idx + "<!-- turns: ".len()..];
                    if let Some(end_idx) = sub.find(" -->") {
                        if let Ok(t) = sub[..end_idx].trim().parse::<usize>() {
                            existing_turns = t;
                        }
                    }
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

        let summary_model = self.config.get_model("summary");
        let provider = create_provider(summary_model.clone());
        let opts = CompletionOptions {
            model: summary_model.model_name,
            max_tokens: summary_model.max_tokens.or(Some(1500)),
            temperature: summary_model.temperature.or(Some(0.3)),
            timeout: Duration::from_secs(300),
            category: Some("summary".to_string()),
        };

        // FU-4: summary 経路の LLM 消費を計上するため db を開く（fail-open）
        let usage_db = rustyclaw_storage::DbManager::new(&workspace_dir.join("memory.db")).ok();

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
                existing_content,
                new_conversation_text
            );

            let messages = vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                    name: None,
                    ..Default::default()
                },
                Message {
                    role: "user".to_string(),
                    content: user_message,
                    name: None,
                    ..Default::default()
                },
            ];

            let response = provider.complete(&messages, &[], &opts).await
                .context("LLM call failed for incremental session summary generation")?;
            if let Some(db) = &usage_db {
                Self::record_aux_usage(db, target_session_id, &response, "session-summary");
            }
            response.content
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
                target_session_id,
                conversation_text
            );

            let messages = vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                    name: None,
                    ..Default::default()
                },
                Message {
                    role: "user".to_string(),
                    content: user_message,
                    name: None,
                    ..Default::default()
                },
            ];

            let response = provider.complete(&messages, &[], &opts).await
                .context("LLM call failed for full session summary generation")?;
            if let Some(db) = &usage_db {
                Self::record_aux_usage(db, target_session_id, &response, "session-summary");
            }
            response.content
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

        Ok(final_summary)
    }


    /// 対話実行（ツール対応のマルチターンアジェンティックループ）
    pub async fn execute_with_tools(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
        tool_registry: &ToolRegistry,
        purpose: &str,
        progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
    ) -> Result<LlmResponse> {
        let logger = SessionLogger::new(workspace_dir);

        // 1. システムプロンプトとコンテキストの構築
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id) {
            system_context.push_str(&continuation);
        }

        // 過去履歴のロードとトークン圧縮処理の適用
        let history_messages = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger.load_history(session_id)
                .context("Failed to load session history messages")?
        };

        let cleaned_history = self.process_proactive_posts(session_id, history_messages, &mut system_context);

        let mut history = ConversationHistory::new(cleaned_history);
        let history_limit = self.get_history_limit(purpose);
        // ISSUE-02: system プロンプト＋ツール定義分のトークンを考慮して実効上限を下げる
        let overhead_chars = system_context.chars().count()
            + tool_registry.to_llm_schemas().iter().map(|s| s.to_string().chars().count()).sum::<usize>();
        let overhead_tokens = (overhead_chars * 3) / 2;
        history.compact_if_needed_with_overhead(history_limit, overhead_tokens);
        history.trim_to_last(self.get_history_message_limit(purpose));

        // 送信用メッセージリストの構築 (System + History)
        let mut active_messages = Vec::new();
        active_messages.push(Message {
            role: "system".to_string(),
            content: system_context,
            name: None,
            ..Default::default()
        });
        active_messages.extend(history.messages.clone());

        // ユーザー入力をメッセージに追加
        let user_msg = Message {
            role: "user".to_string(),
            content: user_message.to_string(),
            name: None,
            ..Default::default()
        };
        active_messages.push(user_msg.clone());

        // ユーザーメッセージを fail-closed で保存
        logger.append_message(session_id, &user_msg)
            .context("Failed to save user message in session log (fail-closed)")?;

        // ツール定義の変換 (rustyclaw-tools::Tool -> rustyclaw-providers::ToolDef)
        let provider_tools: Vec<rustyclaw_providers::ToolDef> = tool_registry
            .to_llm_schemas()
            .iter()
            .map(|schema| rustyclaw_providers::ToolDef {
                name: schema["name"].as_str().unwrap_or("").to_string(),
                description: schema["description"].as_str().unwrap_or("").to_string(),
                parameters: schema["input_schema"].clone(),
            })
            .collect();

        let mut loop_count = 0;
        let max_loops = 10;

        loop {
            loop_count += 1;
            if loop_count > max_loops {
                return Err(anyhow::anyhow!("Agent loop exceeded maximum step limit of {}", max_loops));
            }

            self.dump_request(workspace_dir, &active_messages);

            // 3. LLMプロバイダ呼び出し
            if let Some(ref tx) = progress_tx {
                let _ = tx.send("LLMの応答を待っています...".to_string()).await;
            }
            let mut response = self.complete_with_fallback(
                purpose,
                session_id,
                &active_messages,
                &provider_tools,
                Duration::from_secs(300),
            ).await?;
            response.content = filter_json_leaks(&response.content);

            // アシスタント返答をMessage形式にして active_messages / logger に保存
            let assistant_msg = Message {
                role: response.role.clone(),
                content: response.content.clone(),
                tool_calls: response.tool_calls.clone(),
                name: None,
                ..Default::default()
            };
            active_messages.push(assistant_msg.clone());
            logger.append_message(session_id, &assistant_msg)
                .context("Failed to save assistant response in session log (fail-closed)")?;

            self.dump_response(workspace_dir, &response.content, &response.role);

            // 4. ツール要求があるかどうか確認
            if let Some(ref calls) = response.tool_calls {
                if !calls.is_empty() {
                    // LLMがツール実行を要求したため、ツールを実行する
                    for call in calls {
                        tracing::info!("Agent executing tool call: {} (id: {})", call.function.name, call.id);
                        if let Some(ref tx) = progress_tx {
                            let _ = tx.send(format!("ツール '{}' を実行しています...", call.function.name)).await;
                        }

                        // 引数を Value にパース
                        let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                            .unwrap_or_else(|_| serde_json::json!({}));

                        let tool_result = if let Some(tool) = tool_registry.get(&call.function.name) {
                            tool.execute(args).await
                        } else {
                            rustyclaw_tools::ToolResult {
                                content: format!("Error: Tool '{}' not found in registry", call.function.name),
                                is_error: true,
                            }
                        };

                        // ツール実行結果のメッセージ作成
                        let tool_msg = Message {
                            role: "tool".to_string(),
                            content: tool_result.content,
                            tool_call_id: Some(call.id.clone()),
                            name: Some(call.function.name.clone()),
                            ..Default::default()
                        };

                        // 会話履歴に追記して保存
                        active_messages.push(tool_msg.clone());
                        logger.append_message(session_id, &tool_msg)
                            .context("Failed to save tool result message in session log (fail-closed)")?;
                    }

                    // ツール実行後にループを続行して、LLMに結果を提示する
                    continue;
                }
            }

            // ツール要求がなければループ終了
            if !session_id.starts_with("cron") {
                self.trigger_memory_flush_async(workspace_dir, session_id);
            }

            return Ok(response);
        }
    }

    /// ストリーミングでの対話実行
    pub async fn execute_stream(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id) {
            system_context.push_str(&continuation);
        }

        // 1. 過去履歴のロードとトークン圧縮処理の適用
        let logger = SessionLogger::new(workspace_dir);
        let history_messages = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger.load_history(session_id)
                .context("Failed to load session history messages")?
        };

        let mut history = ConversationHistory::new(history_messages);
        let history_limit = self.get_history_limit("default");
        // ISSUE-02/FU-3: system プロンプト分のトークンを考慮して実効上限を下げる（ツール無し経路）
        let overhead_tokens = (system_context.chars().count() * 3) / 2;
        history.compact_if_needed_with_overhead(history_limit, overhead_tokens);
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
        logger.append_message(session_id, &user_msg)
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
    if content.is_empty() { None } else { Some(content) }
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
    } else if session_id == "daily-summary" || session_id == "cron:daily-summary" || session_id.contains("daily-summary") {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rustyclaw_config::{ModelEntry, AgentsConfig, ModelNames};
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use tokio::io::AsyncWriteExt;

    /// Find the most recent dump file for a given category under llm/<category>/<date>/<time>.json
    fn find_latest_dump(llm_dir: &std::path::Path, category: &str) -> Option<std::path::PathBuf> {
        let cat_dir = llm_dir.join(category);
        let mut date_dirs: Vec<_> = std::fs::read_dir(&cat_dir).ok()?.flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        date_dirs.sort_unstable_by(|a, b| b.cmp(a));
        let date_dir = date_dirs.into_iter().next()?;
        let mut files: Vec<_> = std::fs::read_dir(&date_dir).ok()?.flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        files.sort_unstable_by(|a, b| b.cmp(a));
        files.into_iter().next()
    }

    #[tokio::test]
    async fn test_complete_with_fallback_skips_disabled_model() {
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
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: None,
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
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: None,
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
                rpm: None, rpd: None, tpm: None, tpd: None,
                context_window: None,
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
        assert!(last_line.starts_with("[now: "), "datetime line must be last for cache optimization");
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

    #[tokio::test]
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
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"I am a robot.\"\
                        }\
                    }]\
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
        let resp = pipeline.execute(ws_dir.path(), "session-test", "hello").await;
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
    async fn test_cron_session_ignores_history() -> Result<()> {
        let _guard = ENV_MUTEX.lock().unwrap();
        let ws_dir = tempdir()?;
        fs::write(ws_dir.path().join("SOUL.md"), "Soul Content")?;
        fs::write(ws_dir.path().join("AGENTS.md"), "Agents Content")?;
        fs::write(ws_dir.path().join("MEMORY.md"), "Memory Content")?;
        fs::write(ws_dir.path().join("USER.md"), "User Content")?;

        // 1. Create a dummy session log or mock history.
        let logger = SessionLogger::new(ws_dir.path());
        logger.append_message("cron:heartbeat", &Message {
            role: "user".to_string(),
            content: "old dummy history".to_string(),
            name: None,
            ..Default::default()
        })?;
        logger.append_message("cron:heartbeat", &Message {
            role: "assistant".to_string(),
            content: "old robot reply".to_string(),
            name: None,
            ..Default::default()
        })?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"cron response\"\
                        }\
                    }]\
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
        let resp = pipeline.execute(ws_dir.path(), "cron:heartbeat", "new message").await;
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
            assert!(!msg.content.contains("old dummy history"), "History must be ignored!");
            assert!(!msg.content.contains("old robot reply"), "History must be ignored!");
        }

        let _ = server_task.await;
        Ok(())
    }

    #[test]
    fn test_filter_json_leaks() {
        let input = "Hello, user!\n```json\n{\n  \"action\": \"run_command\",\n  \"command\": \"ls\"\n}\n```\nDone.";
        assert_eq!(filter_json_leaks(input), "Hello, user!\n\nDone.");

        let input_no_leak = "Hello, user!\n```json\n{\n  \"name\": \"test\"\n}\n```\nDone.";
        assert_eq!(filter_json_leaks(input_no_leak), "Hello, user!\n```json\n{\n  \"name\": \"test\"\n}\n```\nDone.");

        let input_multiple = "Start.\n```json\n{\n  \"name\": \"test\"\n}\n```\nMiddle.\n```json\n{\n  \"action\": \"run_command\"\n}\n```\nEnd.";
        assert_eq!(filter_json_leaks(input_multiple), "Start.\n```json\n{\n  \"name\": \"test\"\n}\n```\nMiddle.\n\nEnd.");
    }

    #[tokio::test]
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
        let mut stream_res = pipeline.execute_stream(ws_dir.path(), "session-stream-test", "hello").await;
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

    #[tokio::test]
    async fn test_pipeline_execute_with_tools() -> Result<()> {
        let _guard = ENV_MUTEX.lock().unwrap();
        let ws_dir = tempdir()?;
        fs::write(ws_dir.path().join("SOUL.md"), "Soul Content")?;
        fs::write(ws_dir.path().join("AGENTS.md"), "Agents Content")?;
        fs::write(ws_dir.path().join("MEMORY.md"), "Memory Content")?;
        fs::write(ws_dir.path().join("USER.md"), "User Content")?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server_task = tokio::spawn(async move {
            // First LLM call: requests tool execution
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"I need to calculate 5 + 10 first.\",\
                            \"tool_calls\": [{\
                                \"id\": \"call_calc\",\
                                \"type\": \"function\",\
                                \"function\": {\
                                    \"name\": \"add\",\
                                    \"arguments\": \"{\\\"a\\\": 5, \\\"b\\\": 10}\"\
                                }\
                            }]\
                        }\
                    }]\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
            // Second LLM call: receives tool output and returns final answer
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"The calculation result is 15.\"\
                        }\
                    }]\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let config = make_test_config_with_url(&format!("http://{}", addr));

        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);

        use async_trait::async_trait;

        struct MockAddTool;
        #[async_trait]
        impl rustyclaw_tools::Tool for MockAddTool {
            fn name(&self) -> &str {
                "add"
            }
            fn description(&self) -> &str {
                "Adds two numbers"
            }
            fn parameters(&self) -> serde_json::Value {
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "a": { "type": "number" },
                        "b": { "type": "number" }
                    }
                })
            }
            async fn execute(&self, args: serde_json::Value) -> rustyclaw_tools::ToolResult {
                let a = args["a"].as_f64().unwrap_or(0.0);
                let b = args["b"].as_f64().unwrap_or(0.0);
                rustyclaw_tools::ToolResult {
                    content: format!("{}", a + b),
                    is_error: false,
                }
            }
        }

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockAddTool));

        unsafe {
            std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", ws_dir.path());
        }
        let resp = pipeline.execute_with_tools(ws_dir.path(), "session-tool-test", "calculate 5+10", &registry, "default", None).await;
        unsafe {
            std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
        }
        let resp = resp?;
        assert_eq!(resp.content, "The calculation result is 15.");

        let logger = SessionLogger::new(ws_dir.path());
        let history = logger.load_history("session-tool-test")?;
        
        // Let's verify history contains 4 entries:
        // 0: User input
        // 1: Assistant tool call
        // 2: Tool result
        // 3: Assistant final answer
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].content, "calculate 5+10");
        
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[1].content, "I need to calculate 5 + 10 first.");
        assert_eq!(history[1].tool_calls.as_ref().unwrap()[0].id, "call_calc");
        
        assert_eq!(history[2].role, "tool");
        assert_eq!(history[2].content, "15");
        assert_eq!(history[2].tool_call_id.as_ref().unwrap(), "call_calc");
        assert_eq!(history[2].name.as_ref().unwrap(), "add");
        
        assert_eq!(history[3].role, "assistant");
        assert_eq!(history[3].content, "The calculation result is 15.");

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
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        std::fs::write(
            memory_dir.join("proactive-posts.md"),
            format!("[{}] 傘を持ってください。", now),
        ).unwrap();

        let config = rustyclaw_config::Config::default();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let pipeline = Pipeline::new(config, sem);
        let ctx = pipeline.build_system_context(ws).unwrap();
        assert!(ctx.contains("傘を持ってください"), "proactive-posts が注入されるべき");
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
        
        logger.append_message("session-proactive", &Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: Some("2026-05-31T10:00:00Z".to_string()),
            ..Default::default()
        }).unwrap();
        
        logger.append_message("session-proactive", &Message {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            timestamp: Some("2026-05-31T10:01:00Z".to_string()),
            ..Default::default()
        }).unwrap();
        
        logger.append_message("session-proactive", &Message {
            role: "assistant".to_string(),
            content: "It might rain soon, grab an umbrella!".to_string(),
            trigger: Some("proactive".to_string()),
            timestamp: Some("2026-05-31T12:00:00Z".to_string()),
            ..Default::default()
        }).unwrap();

        let config = rustyclaw_config::Config::default();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let pipeline = Pipeline::new(config, sem);

        let history_messages = logger.load_history("session-proactive").unwrap();
        let mut system_context = "Base Prompt Context".to_string();
        
        let cleaned = pipeline.process_proactive_posts("session-proactive", history_messages, &mut system_context);
        
        assert_eq!(cleaned.len(), 2);
        assert_eq!(cleaned[0].content, "Hello");
        assert_eq!(cleaned[1].content, "Hi there!");
        
        assert!(system_context.contains("Your Previous Posts in This Channel"));
        assert!(system_context.contains("It might rain soon, grab an umbrella!"));
        assert!(system_context.contains("2026-05-31 12:00:00"));
    }
}
