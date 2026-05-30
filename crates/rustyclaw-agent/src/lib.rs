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

    /// 各種人格定義ファイルを読み込んでシステムプロンプト（Context）を構築する
    /// 行頭が // で始まる行（インデント許容）を除去する
    fn strip_comments(content: &str) -> String {
        content.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn build_system_context(&self, workspace_dir: &Path) -> Result<String> {
        // ── 現在日時を1行で先頭に注入（rate limit 対策で最小文字数）──
        let now = chrono::Local::now();
        let runtime_block = format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z"));

        let files = ["SOUL.md", "AGENTS.md", "MEMORY.md", "USER.md"];
        let mut context = runtime_block;

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

        Ok(context)
    }

    /// デバッグ用の生リクエストダンプを出力する
    fn dump_request(&self, workspace_dir: &Path, messages: &[Message]) {
        if !self.config.debug_dump {
            return;
        }

        let debug_dir = workspace_dir.join("memory").join("debug");
        if let Err(e) = fs::create_dir_all(&debug_dir) {
            tracing::error!("Failed to create debug dump directory {:?}: {}", debug_dir, e);
            return;
        }

        let file_path = debug_dir.join("last_request.json");
        match File::create(&file_path) {
            Ok(file) => {
                if let Err(e) = serde_json::to_writer_pretty(file, messages) {
                    tracing::error!("Failed to serialize request dump to {:?}: {}", file_path, e);
                } else {
                    tracing::debug!("Successfully dumped raw request to {:?}", file_path);
                }
            }
            Err(e) => {
                tracing::error!("Failed to create request dump file {:?}: {}", file_path, e);
            }
        }
    }

    /// デバッグ用の生レスポンスダンプを出力する
    fn dump_response(&self, workspace_dir: &Path, content: &str, role: &str) {
        if !self.config.debug_dump {
            return;
        }

        let debug_dir = workspace_dir.join("memory").join("debug");
        let file_path = debug_dir.join("last_response.json");

        #[derive(serde::Serialize)]
        struct DumpResponse {
            role: String,
            content: String,
        }

        let dump_data = DumpResponse {
            role: role.to_string(),
            content: content.to_string(),
        };

        match File::create(&file_path) {
            Ok(file) => {
                if let Err(e) = serde_json::to_writer_pretty(file, &dump_data) {
                    tracing::error!("Failed to serialize response dump to {:?}: {}", file_path, e);
                } else {
                    tracing::debug!("Successfully dumped raw response to {:?}", file_path);
                }
            }
            Err(e) => {
                tracing::error!("Failed to create response dump file {:?}: {}", file_path, e);
            }
        }
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
        };

        let response = provider.complete(&messages, &[], &opts).await
            .context("LLM call failed for memory flush")?;

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
            if let Err(e) = rustyclaw_storage::atomic_write(&memory_path, final_content.as_bytes()) {
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
            let _ = rustyclaw_storage::atomic_write(&log_file, file_content.as_bytes());
        }

        // 3. SQLite にフラッシュ完了状態を保存（メッセージ数 + タイムスタンプ）
        let _ = db.set_state_value(&count_key, &current_count.to_string());
        let _ = db.set_state_value(&ts_key, &chrono::Local::now().to_rfc3339());

        tracing::info!(session = %session_id, "memory flush: completed");
        Ok(())
    }

    /// Heartbeat 専用の軽量システムコンテキストを構築する（SOUL + MEMORY + HEARTBEAT のみ）
    pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
        let now = chrono::Local::now();
        let runtime_block = format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z"));
        let files = ["SOUL.md", "MEMORY.md", "HEARTBEAT.md"];
        let mut context = runtime_block;
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

        let opts = CompletionOptions {
            model: self.config.get_model("default").model_name,
            max_tokens: self.config.get_model("default").max_tokens,
            temperature: self.config.get_model("default").temperature,
            timeout: Duration::from_secs(900),
        };

        let mut response = self.provider.complete(&messages, &[], &opts).await?;
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

        let mut history = ConversationHistory::new(history_messages);
        history.compact_if_needed(1500);

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

        let opts = CompletionOptions {
            model: self.config.get_model("default").model_name,
            max_tokens: self.config.get_model("default").max_tokens,
            temperature: self.config.get_model("default").temperature,
            timeout: Duration::from_secs(900),
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
        };

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

        let mut history = ConversationHistory::new(history_messages);
        history.compact_if_needed(1500);

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

        let opts = CompletionOptions {
            model: self.config.get_model("default").model_name,
            max_tokens: self.config.get_model("default").max_tokens,
            temperature: self.config.get_model("default").temperature,
            timeout: Duration::from_secs(900),
        };

        let mut loop_count = 0;
        let max_loops = 10;

        loop {
            loop_count += 1;
            if loop_count > max_loops {
                return Err(anyhow::anyhow!("Agent loop exceeded maximum step limit of {}", max_loops));
            }

            self.dump_request(workspace_dir, &active_messages);

            // 3. LLMプロバイダ呼び出し
            let mut response = self.provider.complete(&active_messages, &provider_tools, &opts).await?;
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
        history.compact_if_needed(1500);

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

        let opts = CompletionOptions {
            model: self.config.get_model("default").model_name,
            max_tokens: self.config.get_model("default").max_tokens,
            temperature: self.config.get_model("default").temperature,
            timeout: Duration::from_secs(900),
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rustyclaw_config::{ModelEntry, AgentsConfig, AgentPurposeConfig};
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use tokio::io::AsyncWriteExt;

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
            }],
            agents: AgentsConfig {
                default: AgentPurposeConfig { model_name: "test-model".to_string() },
                summary: None,
                memory: None,
            },
            debug_dump: true,
            ..Default::default()
        }
    }

    #[test]
    fn test_build_system_context_injects_runtime_context() {
        let ws_dir = tempdir().unwrap();
        std::fs::write(ws_dir.path().join("SOUL.md"), "soul").unwrap();

        let config = make_test_config_with_url("http://localhost");
        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        let context = pipeline.build_system_context(ws_dir.path()).unwrap();

        // フォーマット: [now: YYYY-MM-DDTHH:MM:SS+HH:MM]
        let first_line = context.lines().next().unwrap();
        assert!(first_line.starts_with("[now: "), "datetime line must be first");
        assert!(first_line.ends_with(']'));
        assert!(first_line.contains('T'), "must be ISO 8601 format");
        assert!(context.contains("# SOUL.md"));
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
        let resp = pipeline.execute(ws_dir.path(), "session-test", "hello").await?;

        assert_eq!(resp.content, "I am a robot.");

        let req_dump = ws_dir.path().join("memory").join("debug").join("last_request.json");
        let resp_dump = ws_dir.path().join("memory").join("debug").join("last_response.json");
        assert!(req_dump.exists());
        assert!(resp_dump.exists());

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
        let resp = pipeline.execute(ws_dir.path(), "cron:heartbeat", "new message").await?;
        assert_eq!(resp.content, "cron response");

        let req_dump_path = ws_dir.path().join("memory").join("debug").join("last_request.json");
        let req_dump_content = fs::read_to_string(req_dump_path)?;
        let sent_messages: Vec<Message> = serde_json::from_str(&req_dump_content)?;

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
        let mut stream_res = pipeline.execute_stream(ws_dir.path(), "session-stream-test", "hello").await?;

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

        let resp = pipeline.execute_with_tools(ws_dir.path(), "session-tool-test", "calculate 5+10", &registry).await?;
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
}
