use anyhow::{Context, Result};
use rustyclaw_agent::{Pipeline, build_heartbeat_rag_query, chunk_memory_md};
use rustyclaw_channels::{Channel, DiscordConnector};
use rustyclaw_config::Config;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{Mutex as TokioMutex, Semaphore, broadcast, mpsc};

pub mod cron;
mod external_mcp;
pub mod health;
pub mod heartbeat;
pub(crate) mod skills;
pub mod watchdog;
pub mod web_preview;

const SUMMARIZE_CRON_SESSIONS: &[&str] = &[
    "cron:karakeep-cleanup",
    "cron:karakeep-recommendation",
    "cron:topic-patrol-explore",
    "cron:topic-patrol-deliver",
    "cron:vitals-morning",
    "cron:vitals-night",
    "cron:daily-briefing",
];

// ==============================================================================
// 待ち行列キューの追跡用データ構造
// ==============================================================================
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct QueueItem {
    pub session_id: String,
    pub status: String,          // "Waiting" | "Executing" | "Cooldown"
    pub enqueued_at_ms: u64,     // Unix Epoch Milliseconds
    pub cooldown_left_secs: f64, // クールダウン中の残り時間
    pub description: String,     // 要約やプロンプト内容のプレビュー
}

#[derive(Debug, Clone, Default)]
pub struct QueueState {
    pub items: Vec<QueueItem>,
}

pub static QUEUE_STATE: Mutex<QueueState> = Mutex::new(QueueState { items: Vec::new() });

pub fn queue_update_or_insert(
    session_id: &str,
    status: &str,
    cooldown_left_secs: f64,
    description: &str,
) {
    let mut state = QUEUE_STATE.lock().unwrap();
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;

    if let Some(item) = state.items.iter_mut().find(|i| i.session_id == session_id) {
        item.status = status.to_string();
        item.cooldown_left_secs = cooldown_left_secs;
        if !description.is_empty() {
            item.description = description.to_string();
        }
        if status == "Waiting" {
            item.enqueued_at_ms = now_ms;
        }
    } else {
        state.items.push(QueueItem {
            session_id: session_id.to_string(),
            status: status.to_string(),
            enqueued_at_ms: now_ms,
            cooldown_left_secs,
            description: description.to_string(),
        });
    }
}

pub fn queue_remove(session_id: &str) {
    let mut state = QUEUE_STATE.lock().unwrap();
    state.items.retain(|i| i.session_id != session_id);
}

// ==============================================================================
// 1. システムイベントと優先度定義
// ==============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Priority {
    Normal,     // ユーザー対話
    Background, // 自発パトロール / Heartbeat
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SystemEvent {
    IncomingMessage {
        session_id: String,
        user_id: String,
        channel_id: String,
        content: String,
        priority: Priority,
    },
    AgentResponse {
        session_id: String,
        channel_id: String,
        content: String,
    },
    SystemError {
        message: String,
    },
}

// ==============================================================================
// 2. MessageBus (イベント pub/sub) の実装
// ==============================================================================

pub struct MessageBus {
    tx: broadcast::Sender<SystemEvent>,
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { tx }
    }

    pub fn publish(&self, event: SystemEvent) -> Result<()> {
        let _ = self.tx.send(event);
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent> {
        self.tx.subscribe()
    }
}

// ==============================================================================
// 3. LaneRegistry (同一セッション直列化 & リソース枠セマフォ制御)
// ==============================================================================

pub struct LaneRegistry {
    lanes: Arc<TokioMutex<HashMap<String, mpsc::Sender<SystemEvent>>>>,
    /// 全レーンの並行実行を制御する統合セマフォ (容量4)。
    /// user 対話 / bg (heartbeat) / flush_memory() の全ての起動がこの枠を取得する。
    /// MEMORY.md 等ワークスペースファイルへの並列書き込みによるデータ消失を防止する。
    lane_sem: Arc<Semaphore>,
    config: Arc<StdMutex<Config>>,
    workspace_path: PathBuf,
    bus: Arc<MessageBus>,
    tool_registry: Arc<rustyclaw_tools::ToolRegistry>,
    tool_server_handle: rig_core::tool::server::ToolServerHandle,
    discord_connector: Arc<DiscordConnector>,
}

impl LaneRegistry {
    pub fn new(
        config: Config,
        workspace_path: PathBuf,
        bus: Arc<MessageBus>,
        tool_registry: Arc<rustyclaw_tools::ToolRegistry>,
        tool_server_handle: rig_core::tool::server::ToolServerHandle,
        lane_sem: Arc<Semaphore>,
        discord_connector: Arc<DiscordConnector>,
    ) -> Self {
        Self {
            lanes: Arc::new(TokioMutex::new(HashMap::new())),
            lane_sem,
            config: Arc::new(StdMutex::new(config)),
            workspace_path,
            bus,
            tool_registry,
            tool_server_handle,
            discord_connector,
        }
    }

    /// 設定ファイルのホットリロード時に config を動的に入れ替える
    pub fn update_config(&self, new_config: Config) {
        if let Ok(mut cfg) = self.config.lock() {
            *cfg = new_config;
            tracing::info!("LaneRegistry config updated dynamically.");
        }
    }

    /// イベントを受信し、適切な Lane worker に直列にディスパッチする (レイヤー2)
    pub async fn dispatch(&self, event: SystemEvent) -> Result<()> {
        let session_id = match &event {
            SystemEvent::IncomingMessage { session_id, .. } => session_id.clone(),
            _ => return Ok(()), // 他のイベントは LaneRegistry では直接ディスパッチしない
        };

        let mut lanes = self.lanes.lock().await;

        if let Some(tx) = lanes.get(&session_id) {
            if tx.send(event.clone()).await.is_err() {
                // 送信失敗した場合はワーカースレッドが消滅しているため、削除して再作成
                lanes.remove(&session_id);
            } else {
                return Ok(());
            }
        }

        // 新しいレーン（直列ワーカー）の起動
        let (tx, rx) = mpsc::channel::<SystemEvent>(10);
        lanes.insert(session_id.clone(), tx.clone());

        let lane_sem = self.lane_sem.clone();
        let config = self.config.clone();
        let workspace_path = self.workspace_path.clone();
        let bus = self.bus.clone();
        let tool_registry = self.tool_registry.clone();
        let tool_server_handle = self.tool_server_handle.clone();
        let discord_connector = self.discord_connector.clone();

        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(evt) = rx.recv().await {
                if let SystemEvent::IncomingMessage {
                    session_id,
                    channel_id,
                    mut content,
                    priority,
                    ..
                } = evt
                {
                    // Backgroundレーンのキュー制限(容量1): 古いキューを全て読み捨てる
                    if priority == Priority::Background {
                        while let Ok(next_evt) = rx.try_recv() {
                            if let SystemEvent::IncomingMessage {
                                content: next_content,
                                ..
                            } = next_evt
                            {
                                content = next_content;
                            }
                        }
                    }

                    // 最新設定の取得
                    let active_config = {
                        let cfg = config.lock().unwrap();
                        cfg.clone()
                    };

                    let db_path = workspace_path.join("memory.db");

                    // 1. Heartbeat 実行処理
                    if session_id == "cron:heartbeat" {
                        let heartbeat_svc = crate::heartbeat::HeartbeatService::new(
                            active_config.clone(),
                            workspace_path.clone(),
                            bus.clone(),
                        );

                        let setup_res = async {
                            if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                if let Ok(digest) = heartbeat_svc.generate_digest(&db_path).await {
                                    let is_step5_allowed =
                                        heartbeat_svc.is_step5_allowed(&db).unwrap_or(false);
                                    let weather_alert = heartbeat_svc
                                        .run_weather_patrol(&db_path)
                                        .await
                                        .unwrap_or(None);
                                    Some((digest, is_step5_allowed, weather_alert))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        .await;

                        if let Some((digest, is_step5_allowed, weather_alert)) = setup_res {
                            let mut prompt_parts = Vec::new();
                            prompt_parts.push(format!(
                                "You are executing your HEARTBEAT patrol. Step 5 Vocal Greeting allowed: {}.", 
                                is_step5_allowed
                            ));

                            if let Some(alert) = weather_alert {
                                prompt_parts.push(alert);
                            }

                            // HA 環境コンテキスト注入（ha-env-summary.txt が存在する場合）
                            let ha_env = heartbeat_svc.get_ha_env_context();
                            let ha_spike = heartbeat_svc.check_ha_spike();

                            if let Some(ref ha_line) = ha_env {
                                prompt_parts.push(format!("Home Environment: {}", ha_line));
                            }
                            if let Some(spike_alert) = ha_spike {
                                prompt_parts.push(spike_alert);
                            }

                            prompt_parts.push(format!("Recent activity digest:\n{}", digest));

                            let heartbeat_rag_query = build_heartbeat_rag_query(&digest);
                            if let Some(ctx) =
                                try_ctx_search(&tool_server_handle, &heartbeat_rag_query, "timeline").await
                            {
                                prompt_parts
                                    .push(format!("Past context (from episodic memory):\n{}", ctx));
                            }
                            // バイタルキーワードを検出した場合のみ過去の類似エピソードを追加検索
                            if let Some(vital_query) = extract_vital_alert_query(&digest)
                                && let Some(advisory) =
                                    try_ctx_search(&tool_server_handle, &vital_query, "relevance").await
                            {
                                prompt_parts
                                    .push(format!("Past similar situation (advisory):\n{}", advisory));
                            }
                            let heartbeat_prompt = prompt_parts.join("\n\n");

                            let mut attempt = 0;
                            let max_attempts = 3;
                            let base_delay = Duration::from_secs(5);

                            let desc = format!(
                                "Heartbeat Patrol / Activity Scan ({})",
                                chrono::Local::now().format("%H:%M")
                            );
                            loop {
                                crate::queue_update_or_insert(&session_id, "Waiting", 0.0, &desc);
                                tracing::debug!(
                                    "Session {} attempting to acquire lane_sem (Attempt {})...",
                                    session_id,
                                    attempt + 1
                                );
                                let permit_res = tokio::time::timeout(
                                    Duration::from_secs(60),
                                    lane_sem.acquire(),
                                )
                                .await;
                                match permit_res {
                                    Ok(Ok(permit)) => {
                                        crate::queue_update_or_insert(
                                            &session_id,
                                            "Executing",
                                            0.0,
                                            &desc,
                                        );
                                        tracing::info!(
                                            "Session {} acquired permit slot. Executing agent...",
                                            session_id
                                        );
                                        let pipeline =
                                            Pipeline::new(active_config.clone(), lane_sem.clone());
                                        match pipeline
                                            .execute_heartbeat(
                                                &workspace_path,
                                                &session_id,
                                                &heartbeat_prompt,
                                                &tool_registry,
                                                &db_path,
                                            )
                                            .await
                                        {
                                            Ok(response) => {
                                                tracing::info!(
                                                    "Heartbeat LLM execution successful. Processing response..."
                                                );
                                                if let Ok(_db) =
                                                    rustyclaw_storage::DbManager::new(&db_path)
                                                {
                                                    let _ = heartbeat_svc
                                                        .process_heartbeat_response(
                                                            &response.content,
                                                            &db_path,
                                                        )
                                                        .await;
                                                }
                                                crate::queue_remove(&session_id);
                                                break; // Exit the retry loop!
                                            }
                                            Err(e) => {
                                                drop(permit); // Release permit immediately so other lanes aren't blocked!
                                                if let Some(err) = e.downcast_ref::<rustyclaw_providers::ProviderError>()
                                                    && let rustyclaw_providers::ProviderError::RateLimit(limit_msg) = err
                                                        && attempt < max_attempts {
                                                            let parsed_reset = err.reset_after();
                                                            let backoff = parsed_reset
                                                                .map(|d| d + Duration::from_secs(2)) // 2秒の安全マージンを追加
                                                                .unwrap_or_else(|| base_delay * 2u32.pow(attempt));
                                                            crate::queue_update_or_insert(&session_id, "Cooldown", backoff.as_secs_f64(), &desc);
                                                            if let Some(reset_duration) = parsed_reset {
                                                                tracing::warn!("Rate limit exceeded. Detected quota reset time: {:.1}s. Dynamic backoff applied: {:.1}s (including 2s safety buffer). Error: {}", reset_duration.as_secs_f64(), backoff.as_secs_f64(), limit_msg);
                                                            } else {
                                                                tracing::warn!("Rate limit exceeded. No quota reset time detected. Falling back to exponential backoff: {:.1}s. Error: {}", backoff.as_secs_f64(), limit_msg);
                                                            }
                                                            tokio::time::sleep(backoff).await;
                                                            attempt += 1;
                                                            continue;
                                                        }
                                                // Non-rate-limit error or max retries exceeded
                                                tracing::error!(
                                                    "Heartbeat LLM execution failed: {:#}",
                                                    e
                                                );
                                                let _ = bus.publish(SystemEvent::SystemError {
                                                    message: format!(
                                                        "Heartbeat LLM execution failed: {:#}",
                                                        e
                                                    ),
                                                });
                                                crate::queue_remove(&session_id);
                                                break;
                                            }
                                        }
                                    }
                                    Ok(Err(_)) => {
                                        tracing::error!(
                                            "Failed to acquire permit for Session {}: Semaphore closed",
                                            session_id
                                        );
                                        crate::queue_remove(&session_id);
                                        break;
                                    }
                                    Err(_) => {
                                        tracing::error!(
                                            "Session {} timed out (60s) waiting for permit slot. Execution skipped.",
                                            session_id
                                        );
                                        let _ = bus.publish(SystemEvent::SystemError {
                                            message: format!("Timeout (60s) waiting for execution slot in session {}", session_id),
                                        });
                                        crate::queue_remove(&session_id);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    // 2. Daily Summary 実行処理
                    else if session_id == "cron:daily-summary" {
                        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                        let prompt = format!(
                            "Compile the daily summary of activities and conversation for date: {}. Please output a concise markdown summary containing a 'TL;DR' section and a list of key topics.",
                            today
                        );

                        let mut attempt = 0;
                        let max_attempts = 3;
                        let base_delay = Duration::from_secs(5);

                        let desc = format!(
                            "Daily Activity Summary ({})",
                            chrono::Local::now().format("%H:%M")
                        );
                        loop {
                            crate::queue_update_or_insert(&session_id, "Waiting", 0.0, &desc);
                            tracing::debug!(
                                "Session {} attempting to acquire lane_sem (Attempt {})...",
                                session_id,
                                attempt + 1
                            );
                            let permit_res =
                                tokio::time::timeout(Duration::from_secs(60), lane_sem.acquire())
                                    .await;
                            match permit_res {
                                Ok(Ok(permit)) => {
                                    crate::queue_update_or_insert(
                                        &session_id,
                                        "Executing",
                                        0.0,
                                        &desc,
                                    );
                                    tracing::info!(
                                        "Session {} acquired permit slot. Executing agent...",
                                        session_id
                                    );
                                    let pipeline =
                                        Pipeline::new(active_config.clone(), lane_sem.clone());
                                    match pipeline
                                        .execute(&workspace_path, &session_id, &prompt)
                                        .await
                                    {
                                        Ok(response) => {
                                            tracing::info!("Daily Summary generated successfully.");
                                            let summaries_dir =
                                                workspace_path.join("memory").join("summaries");
                                            let _ = std::fs::create_dir_all(&summaries_dir);
                                            let file_path = summaries_dir
                                                .join(format!("{}-daily-summary.md", today));

                                            let _ = rustyclaw_storage::atomic_write(
                                                &file_path,
                                                response.content.as_bytes(),
                                            )
                                            .await;

                                            // エピソード記憶として SQLite FTS5 に自動登録（Heartbeat RAG で活用）
                                            let indexed_summary =
                                                format!("[daily-summary:{}]\n{}", today, response.content);
                                            try_ctx_index(
                                                &tool_server_handle,
                                                &indexed_summary,
                                                &format!("daily-summary:{}", today),
                                            )
                                            .await;

                                            if let Ok(db) =
                                                rustyclaw_storage::DbManager::new(&db_path)
                                            {
                                                let trigger = if session_id
                                                    .starts_with("cron:heartbeat")
                                                {
                                                    "heartbeat"
                                                } else if session_id.starts_with("cron:") {
                                                    "cron"
                                                } else if session_id.starts_with("discord-") {
                                                    "discord"
                                                } else if session_id.starts_with("http-dashboard") {
                                                    "dashboard"
                                                } else if session_id.starts_with("cli-") {
                                                    "cli"
                                                } else {
                                                    "unknown"
                                                };
                                                let _ = db.record_usage(
                                                    &session_id,
                                                    response.prompt_tokens.unwrap_or(0),
                                                    response.completion_tokens.unwrap_or(0),
                                                    response.total_tokens.unwrap_or(0),
                                                    response.model_used.as_deref().unwrap_or(""),
                                                    trigger,
                                                    response.provider_id.as_deref(),
                                                    0,
                                                );
                                            }
                                            crate::queue_remove(&session_id);
                                            break; // Exit the retry loop!
                                        }
                                        Err(e) => {
                                            drop(permit); // Release permit immediately so other lanes aren't blocked!
                                            if let Some(err) = e
                                                .downcast_ref::<rustyclaw_providers::ProviderError>(
                                                )
                                                && let rustyclaw_providers::ProviderError::RateLimit(
                                                    limit_msg,
                                                ) = err
                                                && attempt < max_attempts
                                            {
                                                let parsed_reset = err.reset_after();
                                                let backoff = parsed_reset
                                                    .map(|d| d + Duration::from_secs(2)) // 2秒の安全マージンを追加
                                                    .unwrap_or_else(|| {
                                                        base_delay * 2u32.pow(attempt)
                                                    });
                                                crate::queue_update_or_insert(
                                                    &session_id,
                                                    "Cooldown",
                                                    backoff.as_secs_f64(),
                                                    &desc,
                                                );
                                                if let Some(reset_duration) = parsed_reset {
                                                    tracing::warn!(
                                                        "Rate limit exceeded. Detected quota reset time: {:.1}s. Dynamic backoff applied: {:.1}s (including 2s safety buffer). Error: {}",
                                                        reset_duration.as_secs_f64(),
                                                        backoff.as_secs_f64(),
                                                        limit_msg
                                                    );
                                                } else {
                                                    tracing::warn!(
                                                        "Rate limit exceeded. No quota reset time detected. Falling back to exponential backoff: {:.1}s. Error: {}",
                                                        backoff.as_secs_f64(),
                                                        limit_msg
                                                    );
                                                }
                                                tokio::time::sleep(backoff).await;
                                                attempt += 1;
                                                continue;
                                            }
                                            // Non-rate-limit error or max retries exceeded
                                            tracing::error!(
                                                "Failed to generate Daily Summary: {:#}",
                                                e
                                            );
                                            crate::queue_remove(&session_id);
                                            break;
                                        }
                                    }
                                }
                                Ok(Err(_)) => {
                                    tracing::error!(
                                        "Failed to acquire permit for Session {}: Semaphore closed",
                                        session_id
                                    );
                                    crate::queue_remove(&session_id);
                                    break;
                                }
                                Err(_) => {
                                    tracing::error!(
                                        "Session {} timed out (60s) waiting for permit slot. Execution skipped.",
                                        session_id
                                    );
                                    let _ = bus.publish(SystemEvent::SystemError {
                                        message: format!("Timeout (60s) waiting for execution slot in session {}", session_id),
                                    });
                                    crate::queue_remove(&session_id);
                                    break;
                                }
                            }
                        }
                    }
                    // 3. 通常ユーザーセッション実行処理
                    else {
                        let desc = if let Some(suffix) =
                            session_id.strip_prefix("cron:session-summary:")
                        {
                            format!("Auto-Summary for Session '{}'", suffix)
                        } else if session_id.starts_with("cron:") {
                            // cron sessions pre-register display name in CronService; keep it
                            String::new()
                        } else {
                            let char_limit = 80;
                            let mut truncated =
                                content.chars().take(char_limit).collect::<String>();
                            if content.chars().count() > char_limit {
                                truncated.push_str("...");
                            }
                            format!("User Prompt: \"{}\"", truncated.replace('\n', " "))
                        };
                        let mut attempt = 0;
                        let max_attempts = 3;
                        let base_delay = Duration::from_secs(5);

                        loop {
                            crate::queue_update_or_insert(&session_id, "Waiting", 0.0, &desc);
                            tracing::debug!(
                                "Session {} attempting to acquire lane_sem (Attempt {})...",
                                session_id,
                                attempt + 1
                            );
                            let permit_res =
                                tokio::time::timeout(Duration::from_secs(60), lane_sem.acquire())
                                    .await;
                            match permit_res {
                                Ok(Ok(permit)) => {
                                    crate::queue_update_or_insert(
                                        &session_id,
                                        "Executing",
                                        0.0,
                                        &desc,
                                    );
                                    tracing::info!(
                                        "Session {} acquired permit slot. Executing agent...",
                                        session_id
                                    );
                                    let flush_desc = format!("Memory Flush ({})", session_id);
                                    let pipeline =
                                        Pipeline::new(active_config.clone(), lane_sem.clone())
                                            .with_flush_callbacks(
                                                Arc::new({
                                                    let flush_desc = flush_desc.clone();
                                                    move |flush_sid: &str| {
                                                        crate::queue_update_or_insert(
                                                            flush_sid,
                                                            "Waiting",
                                                            0.0,
                                                            &flush_desc,
                                                        );
                                                    }
                                                }),
                                                Arc::new(|flush_sid: &str| {
                                                    crate::queue_update_or_insert(
                                                        flush_sid,
                                                        "Executing",
                                                        0.0,
                                                        "",
                                                    );
                                                }),
                                                Arc::new(|flush_sid: &str| {
                                                    crate::queue_remove(flush_sid);
                                                }),
                                            );
                                    // ProgressReporter（進捗表示とタイピング）のセットアップ
                                    let is_user_channel = !session_id.starts_with("cron")
                                        && channel_id.parse::<u64>().is_ok();
                                    let progress_reporter = if is_user_channel {
                                        match discord_connector
                                            .create_progress_reporter(&channel_id)
                                        {
                                            Ok(rep) => {
                                                let _ = rep.start().await;
                                                Some(rep)
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Failed to create DiscordProgressReporter: {:?}",
                                                    e
                                                );
                                                None
                                            }
                                        }
                                    } else {
                                        None
                                    };

                                    let (progress_tx, mut progress_rx) =
                                        tokio::sync::mpsc::channel::<String>(10);
                                    let progress_tx_opt = if progress_reporter.is_some() {
                                        Some(progress_tx)
                                    } else {
                                        None
                                    };

                                    let progress_reporter_clone = progress_reporter.clone();
                                    let middle_task = tokio::spawn(async move {
                                        if let Some(rep) = progress_reporter_clone {
                                            while let Some(status) = progress_rx.recv().await {
                                                let _ = rep.update_status(&status).await;
                                            }
                                        }
                                    });

                                    // ctx_search でスキルを動的選択（cron セッション以外のみ）
                                    let ctx_skill_names: Option<Vec<String>> =
                                        if !session_id.starts_with("cron:") {
                                            try_ctx_search(&tool_server_handle, &content, "relevance")
                                                .await
                                                .map(|ctx| parse_skill_names_from_ctx(&ctx))
                                                .filter(|names| !names.is_empty())
                                        } else {
                                            None
                                        };

                                    // スキルファイル注入（ctx_search 結果のスキルのみ Activation、Discovery は全件）
                                    let injected_content =
                                        crate::skills::inject_skill_content_with_filter(
                                            &workspace_path,
                                            &content,
                                            ctx_skill_names.as_deref(),
                                        );
                                    let exec_res = if let Some(target_session_id) =
                                        session_id.strip_prefix("cron:session-summary:")
                                    {
                                        match pipeline
                                            .generate_session_summary(
                                                &workspace_path,
                                                target_session_id,
                                            )
                                            .await
                                        {
                                            Ok(summary) => {
                                                // ctx_index でエピソード記憶に登録
                                                try_ctx_index(
                                                    &tool_server_handle,
                                                    &summary,
                                                    &format!(
                                                        "session-summary:{}",
                                                        target_session_id
                                                    ),
                                                )
                                                .await;
                                                Ok(rustyclaw_providers::LlmResponse {
                                                    role: "assistant".to_string(),
                                                    content: summary,
                                                    tool_calls: None,
                                                    prompt_tokens: None,
                                                    completion_tokens: None,
                                                    total_tokens: None,
                                                    model_used: None,
                                                    provider_id: None,
                                                })
                                            }
                                            Err(e) => Err(e),
                                        }
                                    } else {
                                        let run_purpose = if session_id.contains("topic-patrol") {
                                            "patrol"
                                        } else {
                                            "discord"
                                        };
                                        // Interests 注入: patrol は USER.md 直読み + 固定フィード事前フェッチ、
                                        //                 通常 chat は ctx_search で動的取得、その他 cron は None
                                        let user_interests_extra: Option<String> = if session_id
                                            .contains("topic-patrol")
                                        {
                                            // USER.md から Interests を直接読み込み
                                            let interests_opt = std::fs::read_to_string(
                                                workspace_path.join("USER.md"),
                                            )
                                            .ok()
                                            .map(|c| extract_interests_section(&c))
                                            .filter(|s| !s.is_empty());

                                            // 固定 RSS/Web フィードを ctx_fetch_and_index で事前インデックス登録
                                            if let Some(ref interests_text) = interests_opt {
                                                for url in extract_patrol_feed_urls(interests_text)
                                                {
                                                    tracing::info!(
                                                        "Topic Patrol: ctx_fetch_and_index: {}",
                                                        url
                                                    );
                                                    try_ctx_fetch_and_index(
                                                        &tool_server_handle,
                                                        &url,
                                                    )
                                                    .await;
                                                }
                                            }

                                            interests_opt
                                                .map(|i| format!("\n\n# User Interests\n{}", i))
                                        } else if !session_id.starts_with("cron:") {
                                            // 通常 chat: ctx_search で動的取得
                                            let query =
                                                format!("{} user interests hobbies", content);
                                            try_ctx_search(&tool_server_handle, &query, "relevance")
                                                .await
                                                .filter(|r| r.contains("[user-interests]"))
                                                .map(|r| {
                                                    format!("\n\n# Relevant User Interests\n{}", r)
                                                })
                                        } else {
                                            None
                                        };

                                        // 関連メモリを ctx_search で動的取得（cron 以外のみ）
                                        let memory_extra: Option<String> = if !session_id
                                            .starts_with("cron:")
                                        {
                                            try_ctx_search(&tool_server_handle, &content, "relevance")
                                                .await
                                                .filter(|r| r.contains("[memory-chunk]"))
                                                .map(|r| format!("\n\n# Relevant Memory\n{}", r))
                                        } else {
                                            None
                                        };

                                        // user_interests_extra と memory_extra を結合して extra_system_context を構築
                                        let extra_system_context: Option<String> =
                                            match (user_interests_extra, memory_extra) {
                                                (Some(u), Some(m)) => Some(format!("{}{}", u, m)),
                                                (Some(u), None) => Some(u),
                                                (None, Some(m)) => Some(m),
                                                (None, None) => None,
                                            };

                                        pipeline
                                            .execute_with_rig_agent(
                                                &workspace_path,
                                                &session_id,
                                                &content,
                                                &injected_content,
                                                tool_server_handle.clone(),
                                                run_purpose,
                                                progress_tx_opt,
                                                extra_system_context,
                                            )
                                            .await
                                    };

                                    // 進捗表示の終了とクリーンアップ
                                    if let Some(ref rep) = progress_reporter {
                                        let _ = rep.finish().await;
                                    }
                                    middle_task.abort();
                                    match exec_res {
                                        Ok(response) => {
                                            tracing::info!(
                                                "Agent response generated successfully for Session {}",
                                                session_id
                                            );
                                            let _ = bus.publish(SystemEvent::AgentResponse {
                                                session_id: session_id.clone(),
                                                channel_id: channel_id.clone(),
                                                content: response.content,
                                            });

                                            // セッションサマリー RAG 化: ホワイトリスト cron ジョブ完了後に summary イベントを発行 (Phase 41-1)
                                            if SUMMARIZE_CRON_SESSIONS
                                                .contains(&session_id.as_str())
                                            {
                                                let summary_session_id =
                                                    format!("cron:session-summary:{}", session_id);
                                                if let Err(e) =
                                                    bus.publish(SystemEvent::IncomingMessage {
                                                        session_id: summary_session_id,
                                                        user_id: "cron".to_string(),
                                                        channel_id: "cron".to_string(),
                                                        content: String::new(),
                                                        priority: Priority::Background,
                                                    })
                                                {
                                                    tracing::warn!(
                                                        "Failed to publish session-summary event: {:#}",
                                                        e
                                                    );
                                                }
                                            }

                                            // lastUserContact を更新 (SQLite & heartbeat-state.json)
                                            if let Ok(db) =
                                                rustyclaw_storage::DbManager::new(&db_path)
                                            {
                                                let now = chrono::Local::now().to_rfc3339();
                                                let _ = db.set_state_value("lastUserContact", &now);

                                                let state_path = workspace_path
                                                    .join("memory")
                                                    .join("heartbeat-state.json");
                                                let mut current_state = serde_json::json!({
                                                    "lastChecks": {
                                                        "activityReview": "1970-01-01T00:00:00Z",
                                                        "memoryMaintenance": "1970-01-01T00:00:00Z",
                                                        "calendar": "1970-01-01T00:00:00Z",
                                                        "email": "1970-01-01T00:00:00Z",
                                                        "weather": "1970-01-01T00:00:00Z",
                                                        "lastUserContact": now
                                                    }
                                                });
                                                if let Ok(c) = std::fs::read_to_string(&state_path)
                                                    && let Ok(parsed) =
                                                        serde_json::from_str::<serde_json::Value>(
                                                            &c,
                                                        )
                                                {
                                                    current_state = parsed;
                                                    current_state["lastChecks"]["lastUserContact"] =
                                                        serde_json::json!(now);
                                                }
                                                if let Ok(serialized) =
                                                    serde_json::to_string_pretty(&current_state)
                                                {
                                                    let _ = rustyclaw_storage::atomic_write(
                                                        &state_path,
                                                        serialized.as_bytes(),
                                                    )
                                                    .await;
                                                }

                                                let trigger = if session_id
                                                    .starts_with("cron:heartbeat")
                                                {
                                                    "heartbeat"
                                                } else if session_id.starts_with("cron:") {
                                                    "cron"
                                                } else if session_id.starts_with("discord-") {
                                                    "discord"
                                                } else if session_id.starts_with("http-dashboard") {
                                                    "dashboard"
                                                } else if session_id.starts_with("cli-") {
                                                    "cli"
                                                } else {
                                                    "unknown"
                                                };
                                                let _ = db.record_usage(
                                                    &session_id,
                                                    response.prompt_tokens.unwrap_or(0),
                                                    response.completion_tokens.unwrap_or(0),
                                                    response.total_tokens.unwrap_or(0),
                                                    response.model_used.as_deref().unwrap_or(""),
                                                    trigger,
                                                    response.provider_id.as_deref(),
                                                    0,
                                                );
                                            }
                                            crate::queue_remove(&session_id);
                                            break; // Exit the retry loop!
                                        }
                                        Err(e) => {
                                            drop(permit); // Release permit immediately so other lanes aren't blocked!
                                            if let Some(err) = e
                                                .downcast_ref::<rustyclaw_providers::ProviderError>(
                                                )
                                                && let rustyclaw_providers::ProviderError::RateLimit(
                                                    limit_msg,
                                                ) = err
                                                && attempt < max_attempts
                                            {
                                                let parsed_reset = err.reset_after();
                                                let backoff = parsed_reset
                                                    .map(|d| d + Duration::from_secs(2)) // 2秒の安全マージンを追加
                                                    .unwrap_or_else(|| {
                                                        base_delay * 2u32.pow(attempt)
                                                    });
                                                crate::queue_update_or_insert(
                                                    &session_id,
                                                    "Cooldown",
                                                    backoff.as_secs_f64(),
                                                    &desc,
                                                );
                                                if let Some(reset_duration) = parsed_reset {
                                                    tracing::warn!(
                                                        "Rate limit exceeded. Detected quota reset time: {:.1}s. Dynamic backoff applied: {:.1}s (including 2s safety buffer). Error: {}",
                                                        reset_duration.as_secs_f64(),
                                                        backoff.as_secs_f64(),
                                                        limit_msg
                                                    );
                                                } else {
                                                    tracing::warn!(
                                                        "Rate limit exceeded. No quota reset time detected. Falling back to exponential backoff: {:.1}s. Error: {}",
                                                        backoff.as_secs_f64(),
                                                        limit_msg
                                                    );
                                                }
                                                tokio::time::sleep(backoff).await;
                                                attempt += 1;
                                                continue;
                                            }
                                            // Non-rate-limit error or max retries exceeded
                                            tracing::error!(
                                                "Error in Agent execution for Session {}: {:#}",
                                                session_id,
                                                e
                                            );
                                            let _ = bus.publish(SystemEvent::SystemError {
                                                message: format!(
                                                    "Agent execution failed for session {}: {}",
                                                    session_id, e
                                                ),
                                            });
                                            crate::queue_remove(&session_id);
                                            break;
                                        }
                                    }
                                }
                                Ok(Err(_)) => {
                                    tracing::error!(
                                        "Failed to acquire permit for Session {}: Semaphore closed",
                                        session_id
                                    );
                                    crate::queue_remove(&session_id);
                                    break;
                                }
                                Err(_) => {
                                    tracing::error!(
                                        "Session {} timed out (60s) waiting for permit slot. Execution skipped.",
                                        session_id
                                    );
                                    let _ = bus.publish(SystemEvent::SystemError {
                                        message: format!("Timeout (60s) waiting for execution slot in session {}", session_id),
                                    });
                                    crate::queue_remove(&session_id);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        });

        tx.send(event)
            .await
            .context("Failed to queue initial message to new lane worker")?;
        Ok(())
    }
}

// ==============================================================================
// 4. Gateway (オーケストレーターデーモン) の実装
// ==============================================================================

/// vault.enc の全キーをプロセス環境変数に注入する。
/// context-mode → ctx_execute → bash スクリプトへ継承させるために起動時一度だけ呼ぶ。
/// vault が存在しない場合や復号失敗は warn ログのみで続行（fail-open）。
fn inject_vault_to_env() {
    match rustyclaw_config::vault::load_vault(None) {
        Ok(secrets) => {
            let count = secrets.len();
            // SAFETY: Gateway 起動時、他スレッドが env を読む前に一度だけ呼ぶ。
            unsafe {
                for (key, value) in secrets {
                    std::env::set_var(&key, &value);
                }
            }
            tracing::info!("vault: {} キーを環境変数に注入済み", count);
        }
        Err(e) => tracing::warn!(
            "vault 読み込みスキップ (ctx_execute スクリプトは vault キー未解決): {e}"
        ),
    }
}

/// context-mode を起動し、クラッシュ時に指数バックオフで再接続するループを spawn する。
/// 接続確立後は tool_server_handle に ctx_execute / ctx_search / ctx_index / ctx_patch が自動登録される。
async fn start_context_mode(
    workspace_root: PathBuf,
    tool_server_handle: rig_core::tool::server::ToolServerHandle,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut delay = Duration::from_secs(1);
        loop {
            match external_mcp::ExternalMcpController::spawn(&workspace_root) {
                Err(e) => {
                    tracing::error!("context-mode 起動失敗: {e}");
                }
                Ok((mut ctrl, stdin, stdout)) => {
                    let handler = rig_core::tool::rmcp::McpClientHandler::new(
                        rmcp::model::ClientInfo::default(),
                        tool_server_handle.clone(),
                    );
                    match handler.connect((stdout, stdin)).await {
                        Ok(service) => {
                            tracing::info!(
                                "context-mode 接続完了 (ctx_execute/search/index/patch 登録)"
                            );
                            delay = Duration::from_secs(1);
                            tokio::select! {
                                _ = ctrl.wait() => {
                                    tracing::warn!("context-mode プロセスが終了。再起動します...");
                                }
                                result = service.waiting() => {
                                    tracing::warn!(
                                        "context-mode MCP セッション終了 ({result:?})。再起動します..."
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("context-mode MCP 接続失敗: {e}");
                        }
                    }
                }
            }
            tracing::info!("context-mode を {:?} 後に再起動...", delay);
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(60));
        }
    })
}

/// ctx_search を呼び出す。context-mode が未接続の場合は None を返す（fail-open）。
async fn try_ctx_search(
    handle: &rig_core::tool::server::ToolServerHandle,
    query: &str,
    sort: &str,
) -> Option<String> {
    let args = serde_json::json!({
        "queries": [query],
        "sort": sort,
        "limit": 3
    })
    .to_string();
    match handle.call_tool("ctx_search", &args).await {
        Ok(result) if !result.trim().is_empty() => Some(result),
        Ok(_) => None,
        Err(e) => {
            tracing::debug!("ctx_search unavailable (context-mode 未接続?): {}", e);
            None
        }
    }
}

/// ctx_index を呼び出す。失敗は warn ログのみで続行（fail-open）。
async fn try_ctx_index(
    handle: &rig_core::tool::server::ToolServerHandle,
    content: &str,
    source: &str,
) {
    let args = serde_json::json!({
        "content": content,
        "source": source
    })
    .to_string();
    match handle.call_tool("ctx_index", &args).await {
        Ok(_) => tracing::info!("ctx_index 完了: source={}", source),
        Err(e) => tracing::warn!("ctx_index 失敗 (source={}): {}", source, e),
    }
}

/// ctx_fetch_and_index を呼び出す。context-mode が未接続の場合は None を返す（fail-open）。
/// 指定 URL の HTML コンテンツを Markdown に変換し、SQLite FTS5 にインデックス登録する。
async fn try_ctx_fetch_and_index(
    handle: &rig_core::tool::server::ToolServerHandle,
    url: &str,
) -> Option<String> {
    let args = serde_json::json!({ "url": url }).to_string();
    match handle.call_tool("ctx_fetch_and_index", &args).await {
        Ok(result) if !result.trim().is_empty() => Some(result),
        Ok(_) => None,
        Err(e) => {
            tracing::debug!("ctx_fetch_and_index 呼び出し失敗 (url={}): {}", url, e);
            None
        }
    }
}

static SKILL_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"\[skill:([a-z][a-z0-9-]*)\]").unwrap());

/// ctx_search の返却テキストから [skill:NAME] パターンを抽出してスキル名リストに変換する。
fn parse_skill_names_from_ctx(ctx: &str) -> Vec<String> {
    let names: std::collections::HashSet<String> = SKILL_RE
        .captures_iter(ctx)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();
    names.into_iter().collect()
}

/// 起動時に全 SKILL.md を context-mode にインデックス登録する。
/// [skill:NAME] プレフィックスにより ctx_search でスキル名が特定できる。
async fn index_skills_to_context_mode(
    workspace_path: &Path,
    handle: &rig_core::tool::server::ToolServerHandle,
) {
    let skills = crate::skills::load_skills(workspace_path);
    if skills.is_empty() {
        return;
    }
    for skill in &skills {
        let content = format!(
            "[skill:{}]\n{}\n{}",
            skill.manifest.name,
            skill.manifest.description,
            skill.instructions.trim()
        );
        let source = format!("skill:{}", skill.manifest.name);
        try_ctx_index(handle, &content, &source).await;
    }
    tracing::info!(
        "context-mode: {} スキルをインデックス登録完了",
        skills.len()
    );
}

/// USER.md の "## Interests" セクション本文を抽出して返す。
fn extract_interests_section(content: &str) -> String {
    let mut in_section = false;
    let mut lines: Vec<&str> = Vec::new();
    for line in content.lines() {
        if line.trim_start().starts_with("## Interests") {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with("## ") {
                break;
            }
            if !line.trim().is_empty() {
                lines.push(line);
            }
        }
    }
    lines.join("\n")
}

/// USER.md Interests の sources: 行から HTTP/HTTPS URL のみを抽出する。
/// HN, Reddit, github: ショートカットは除外する（動的検索は事前フェッチ不可のため）。
fn extract_patrol_feed_urls(interests: &str) -> Vec<String> {
    interests
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let value = if let Some(v) = trimmed.strip_prefix("sources: ") {
                v
            } else if let Some(v) = trimmed.strip_prefix("sources:") {
                v.trim()
            } else {
                return None;
            };
            if value.starts_with("http://") || value.starts_with("https://") {
                Some(value.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Heartbeat digest 中に睡眠・疲労系のキーワードが含まれる場合、
/// 過去の類似エピソードを ctx_search するためのクエリを返す（なければ None）。
fn extract_vital_alert_query(digest: &str) -> Option<String> {
    let lower = digest.to_lowercase();
    let keywords = [
        "sleep", "tired", "fatigue", "exhausted", "sleepy",
        "睡眠", "疲れ", "疲労", "眠い", "不眠", "睡眠不足",
    ];
    if keywords.iter().any(|kw| lower.contains(kw)) {
        Some("daily-summary sleep deprivation fatigue advice past similar situation".to_string())
    } else {
        None
    }
}

/// 起動時に USER.md の Interests セクションを context-mode にインデックス登録する。
/// [user-interests] プレフィックスにより ctx_search 結果から識別できる。
async fn index_user_interests(
    workspace_path: &Path,
    handle: &rig_core::tool::server::ToolServerHandle,
) {
    let user_md_path = workspace_path.join("USER.md");
    let content = match std::fs::read_to_string(&user_md_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "USER.md 読み込み失敗（Interests インデックス登録スキップ）: {}",
                e
            );
            return;
        }
    };
    let interests = extract_interests_section(&content);
    if interests.is_empty() {
        tracing::info!("USER.md に Interests セクションが見つからないためスキップ");
        return;
    }
    let indexed = format!("[user-interests]\n{}", interests);
    try_ctx_index(handle, &indexed, "user-interests").await;
    tracing::info!("context-mode: USER.md Interests インデックス登録完了");
}

async fn index_memory_to_context_mode(
    workspace_path: &Path,
    handle: &rig_core::tool::server::ToolServerHandle,
) {
    let memory_path = workspace_path.join("MEMORY.md");
    let content = match std::fs::read_to_string(&memory_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("MEMORY.md 読み込み失敗（インデックス登録スキップ）: {}", e);
            return;
        }
    };
    let chunks = chunk_memory_md(&content);
    if chunks.is_empty() {
        tracing::info!("MEMORY.md にチャンクが見つからないためスキップ");
        return;
    }
    for (i, chunk) in chunks.iter().enumerate() {
        let indexed = format!("[memory-chunk]\n{}", chunk);
        try_ctx_index(handle, &indexed, &format!("memory-chunk:{}", i)).await;
    }
    tracing::info!(
        "context-mode: MEMORY.md {} チャンク インデックス登録完了",
        chunks.len()
    );
}

pub struct Gateway {
    config_path: PathBuf,
    workspace_path: PathBuf,
}

impl Gateway {
    pub fn new<P: AsRef<Path>, W: AsRef<Path>>(config_path: P, workspace_path: W) -> Self {
        Self {
            config_path: config_path.as_ref().to_path_buf(),
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    pub async fn run(&self) -> Result<()> {
        tracing::info!("Initializing RustyClaw Gateway daemon...");

        // 1. 設定のロード
        let config = rustyclaw_config::load_config(&self.config_path)
            .context("Failed to load initial configuration for Gateway")?;

        // Web Preview Server Startup
        let mut web_preview_task = None;
        let preview_base_url = if config.web_preview.enabled {
            match crate::web_preview::start_web_preview_server(&config, self.workspace_path.clone())
                .await
            {
                Ok((h, bound_addr)) => {
                    web_preview_task = Some(h);
                    let base = crate::web_preview::get_preview_base_url(
                        &config.web_preview.base_url,
                        bound_addr.port(),
                    );
                    tracing::info!(
                        "Web Preview Server started successfully. Base URL: {}",
                        base
                    );
                    Some(base)
                }
                Err(e) => {
                    tracing::warn!("Failed to start Web Preview Server: {:#}", e);
                    None
                }
            }
        } else {
            tracing::info!("Web Preview Server is disabled in config.");
            None
        };

        {
            let m = config.get_model("default");
            tracing::info!(
                "Gateway loaded configuration: provider={}, model={}",
                m.model_provider,
                m.model_name
            );
        }

        // 2. Vault キーをプロセス環境変数に注入（ctx_execute 経由の bash スクリプトに継承させる）
        inject_vault_to_env();

        // HA エンドポイントを環境変数に注入（bash スクリプトが $HOMEASSISTANT_ENDPOINT を参照）
        if let Some(ref ha) = config.tools.home_assistant
            && !ha.endpoint.is_empty()
        {
            // SAFETY: Gateway 起動時、他スレッドが env を読む前に一度だけ呼ぶ。
            unsafe {
                std::env::set_var("HOMEASSISTANT_ENDPOINT", &ha.endpoint);
            }
            tracing::debug!("Gateway: HOMEASSISTANT_ENDPOINT set to {}", ha.endpoint);
        }

        // 3. ToolServer (rig エージェント用) と ToolRegistry (heartbeat 用) の初期化
        let tool_server_handle = rig_core::tool::server::ToolServer::new().run();
        let mut mcp_service_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        // context-mode 子プロセスを起動（fail-open: エラーでも Gateway は続行）
        let _ctx_mode_handle =
            start_context_mode(self.workspace_path.clone(), tool_server_handle.clone()).await;

        // スキルを context-mode に非同期インデックス登録（context-mode 起動後 3 秒待ってから実行）
        {
            let ws = self.workspace_path.clone();
            let tsh = tool_server_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                index_skills_to_context_mode(&ws, &tsh).await;
            });
        }

        // USER.md Interests を context-mode に非同期インデックス登録（4 秒後）
        {
            let ws = self.workspace_path.clone();
            let tsh = tool_server_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                index_user_interests(&ws, &tsh).await;
            });
        }

        // MEMORY.md チャンクを context-mode に非同期インデックス登録（5 秒後）
        {
            let ws = self.workspace_path.clone();
            let tsh = tool_server_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                index_memory_to_context_mode(&ws, &tsh).await;
            });
        }

        // MCP サーバーへの接続 (rig-core rmcp 経由)
        for (name, conf) in &config.mcp {
            if conf.enabled {
                let handler = rig_core::tool::rmcp::McpClientHandler::new(
                    rmcp::model::ClientInfo::default(),
                    tool_server_handle.clone(),
                );
                let mut cmd = tokio::process::Command::new(&conf.command);
                cmd.args(&conf.args)
                    .envs(&conf.env)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped());
                match cmd.spawn() {
                    Ok(mut child) => {
                        if let (Some(stdin), Some(stdout)) =
                            (child.stdin.take(), child.stdout.take())
                        {
                            match handler.connect((stdout, stdin)).await {
                                Ok(service) => {
                                    let n = name.clone();
                                    mcp_service_tasks.push(tokio::spawn(async move {
                                        if let Err(e) = service.waiting().await {
                                            tracing::warn!("MCP service {} ended: {e}", n);
                                        }
                                    }));
                                    tokio::spawn(async move {
                                        child.wait().await.ok();
                                    });
                                    tracing::info!("Connected to MCP server: {}", name);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to connect MCP server {}: {e}", name)
                                }
                            }
                        }
                    }
                    Err(e) => tracing::error!("Failed to spawn MCP process {}: {e}", name),
                }
            }
        }

        let mut tool_registry = rustyclaw_tools::ToolRegistry::new();

        // Brave Search ネイティブツール登録
        if let Some(b) = config.tools.brave_search.as_ref().filter(|b| b.enabled)
            && !b.api_key.is_empty()
        {
            let t = rustyclaw_tools::WebSearchTool::new(b.api_key.clone());
            tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
            tool_server_handle.add_tool(t).await.ok();
            tracing::info!("Registered WebSearchTool (Brave Search).");
        }
        // WebFetchTool は常時登録（APIキー不要）
        {
            let t = rustyclaw_tools::WebFetchTool::new();
            tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
            tool_server_handle.add_tool(t).await.ok();
        }

        // WorkspaceReadTool / WorkspaceWriteTool 登録
        {
            let t = rustyclaw_tools::WorkspaceReadTool::new(self.workspace_path.clone());
            tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
            tool_server_handle.add_tool(t).await.ok();
        }
        {
            let t = rustyclaw_tools::WorkspaceWriteTool::new(
                self.workspace_path.clone(),
                preview_base_url.clone(),
            );
            // Heartbeat には書き込みツールを公開しない（読み取りと通知のみ許可）。
            // Chat 用 tool_server_handle には引き続き登録する。
            tool_server_handle.add_tool(t).await.ok();
        }
        tracing::info!("Registered Workspace I/O tools.");

        let ws_path_clone = self.workspace_path.clone();
        let db_path_for_tool = self.workspace_path.join("memory.db");
        {
            let t = rustyclaw_tools::CronScheduleTool::new(move || {
                if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path_for_tool) {
                    let sched = crate::cron::compute_schedule(&ws_path_clone, &db);
                    serde_json::Value::Array(sched)
                } else {
                    serde_json::Value::Array(vec![])
                }
            });
            tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
            tool_server_handle.add_tool(t).await.ok();
        }
        tracing::info!("Registered native CronScheduleTool.");

        // MCP ツール群 (ctx_execute 等) を heartbeat 用の tool_registry にプロキシとして登録
        for tool_name in &["ctx_execute", "ctx_search", "ctx_index", "ctx_patch"] {
            let proxy = McpProxyTool::new(tool_name, tool_server_handle.clone());
            tool_registry.register(Arc::new(proxy) as Arc<dyn rig_core::tool::ToolDyn>);
        }
        tracing::info!("Registered McpProxyTools for heartbeat.");

        let tool_registry = Arc::new(tool_registry);
        tracing::info!(
            "Tool registry initialized with {} tools.",
            tool_registry.tool_count()
        );

        // 3. MessageBus および DiscordConnector の初期化
        let bus = Arc::new(MessageBus::new());

        let discord_cfg = config.channels.discord.as_ref();
        let discord_enabled = discord_cfg.map(|d| d.enabled).unwrap_or(false);
        let discord_token = discord_cfg
            .map(|d| d.token.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| std::env::var("DISCORD_TOKEN").ok())
            .unwrap_or_else(|| "dummy".to_string());

        let mut discord = DiscordConnector::new(&discord_token);

        // メッセージ受信時に MessageBus へパブリッシュするコールバックを設定
        let bus_clone = bus.clone();
        discord.register_callback(Arc::new(move |msg| {
            let _ = bus_clone.publish(SystemEvent::IncomingMessage {
                session_id: msg.session_id,
                user_id: msg.user_id,
                channel_id: msg.channel_id,
                content: msg.content,
                priority: Priority::Normal,
            });
        }));

        let respond_in = discord_cfg
            .map(|d| d.respond_in_channels.clone())
            .unwrap_or_default();
        discord.set_respond_in_channels(respond_in);

        let discord_client = Arc::new(discord);

        // 4. LaneRegistry の初期化
        let lane_sem = Arc::new(Semaphore::new(4)); // 全レーン統合枠 (user + bg + flush を一本化、容量 4)
        let registry = Arc::new(LaneRegistry::new(
            config.clone(),
            self.workspace_path.clone(),
            bus.clone(),
            tool_registry.clone(),
            tool_server_handle.clone(),
            lane_sem.clone(),
            discord_client.clone(),
        ));

        if discord_enabled {
            discord_client
                .start()
                .await
                .context("Failed to start DiscordConnector")?;
        } else {
            tracing::info!("Discord channel disabled — skipping connector startup.");
        }

        // 5. MessageBus イベント監視ループ (MessageBus -> LaneRegistry)
        let registry_clone = registry.clone();
        let mut rx_bus = bus.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = rx_bus.recv().await {
                if matches!(event, SystemEvent::IncomingMessage { .. })
                    && let Err(e) = registry_clone.dispatch(event).await
                {
                    tracing::error!("Failed to dispatch event to LaneRegistry: {}", e);
                }
            }
        });

        // ISSUE-17: IncomingMessage 受信直後に Waiting を可視化する観測タスク
        {
            let mut wait_rx = bus.subscribe();
            tokio::spawn(async move {
                loop {
                    match wait_rx.recv().await {
                        Ok(SystemEvent::IncomingMessage {
                            session_id,
                            content,
                            ..
                        }) => {
                            // cron sessions pre-register their display name in CronService; skip override
                            let desc = if session_id.starts_with("cron:") {
                                String::new()
                            } else {
                                let t: String = content.chars().take(40).collect();
                                format!("User Prompt: \"{}\"", t.replace('\n', " "))
                            };
                            crate::queue_update_or_insert(&session_id, "Waiting", 0.0, &desc);
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(_) => {} // Lagged は無視
                    }
                }
            });
        }

        // 6. Agent応答の配信監視ループ (LaneRegistry / MessageBus -> Discord)
        let mut rx_responses = bus.subscribe();
        let discord_sender = discord_client.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx_responses.recv().await {
                if let SystemEvent::AgentResponse {
                    channel_id,
                    content,
                    ..
                } = event
                {
                    // 数字以外の channel_id（"http" など）は Discord チャンネルではないのでスキップ
                    if channel_id.chars().all(|c| c.is_ascii_digit())
                        && let Err(e) = discord_sender.send_message(&channel_id, &content).await
                    {
                        tracing::error!(
                            "Failed to send agent response to channel {}: {:#}",
                            channel_id,
                            e
                        );
                    }
                }
            }
        });

        // 6b. SystemError 監視ループ — "all models failed" を Discord home channel へ通知
        if discord_enabled {
            let home_channel_id = discord_cfg
                .and_then(|d| d.home_channel_id.clone())
                .unwrap_or_default();
            let mut rx_errors = bus.subscribe();
            let discord_alert = discord_client.clone();
            tokio::spawn(async move {
                while let Ok(SystemEvent::SystemError { message }) = rx_errors.recv().await {
                    if message.contains("all models failed") {
                        tracing::warn!("All models failed — sending Discord alert");
                        let alert = format!("⚠️ **LLM 全モデル失敗**\n```\n{}\n```", message);
                        if let Err(e) = discord_alert.send_message(&home_channel_id, &alert).await {
                            tracing::error!("Failed to send all-models-failed alert: {:#}", e);
                        }
                    }
                }
            });
        }

        // 7. 各種バックグラウンドサービスの初期化・起動

        // ① Systemd Watchdog の起動
        watchdog::WatchdogService::start();

        // ② CronService (Heartbeat, DailySummary) の起動
        let db_path = self.workspace_path.join("memory.db");
        let cron_svc = cron::CronService::new(bus.clone(), db_path, self.workspace_path.clone());
        cron_svc.start();

        // ④ 静的ドキュメント RAG インジェスト（バックグラウンド、起動時）
        // ③ HealthServer (HTTPサーバー) の起動
        let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel::<()>(1);
        let health_server = health::HealthServer::new(
            8080,
            reload_tx,
            bus.clone(),
            self.workspace_path.clone(),
            lane_sem.clone(),
            4, // lane_sem capacity = 4
        );
        health_server
            .start()
            .await
            .context("Failed to start HealthServer")?;

        // 8. シグナルおよび HTTP リロードハンドリングループ
        let mut sig_hup =
            signal(SignalKind::hangup()).context("Failed to register SIGHUP listener")?;
        let mut sig_int =
            signal(SignalKind::interrupt()).context("Failed to register SIGINT listener")?;
        let mut sig_term =
            signal(SignalKind::terminate()).context("Failed to register SIGTERM listener")?;

        tracing::info!(
            "RustyClaw Gateway is now running. Monitoring signals (SIGHUP, SIGINT, SIGTERM) and HTTP reload..."
        );

        loop {
            tokio::select! {
                // SIGHUP: ホットリロード
                _ = sig_hup.recv() => {
                    tracing::info!("Received SIGHUP. Reloading configuration...");
                    match rustyclaw_config::load_config(&self.config_path) {
                        Ok(new_config) => {
                            registry.update_config(new_config.clone());
                            let m = new_config.get_model("default");
                            tracing::info!("Configuration reloaded successfully: provider={}, model={}", m.model_provider, m.model_name);
                        }
                        Err(e) => {
                            tracing::error!("Failed to reload configuration: {:#}. Using previous configuration.", e);
                        }
                    }
                }
                // HTTP /reload: ホットリロード
                _ = reload_rx.recv() => {
                    tracing::info!("Received HTTP /reload request. Reloading configuration...");
                    match rustyclaw_config::load_config(&self.config_path) {
                        Ok(new_config) => {
                            registry.update_config(new_config.clone());
                            let m = new_config.get_model("default");
                            tracing::info!("Configuration reloaded successfully via HTTP: provider={}, model={}", m.model_provider, m.model_name);
                        }
                        Err(e) => {
                            tracing::error!("Failed to reload configuration via HTTP: {:#}. Using previous configuration.", e);
                        }
                    }
                }
                // SIGINT: Graceful Shutdown
                _ = sig_int.recv() => {
                    tracing::info!("Received SIGINT. Initiating graceful shutdown...");
                    for h in &mcp_service_tasks { h.abort(); }
                    if let Some(ref h) = web_preview_task { h.abort(); }
                    discord_client.shutdown().await;
                    break;
                }
                // SIGTERM: Graceful Shutdown
                _ = sig_term.recv() => {
                    tracing::info!("Received SIGTERM. Initiating graceful shutdown...");
                    for h in &mcp_service_tasks { h.abort(); }
                    if let Some(ref h) = web_preview_task { h.abort(); }
                    discord_client.shutdown().await;
                    break;
                }
            }
        }

        tracing::info!("RustyClaw Gateway shutdown complete.");
        Ok(())
    }
}

// ==============================================================================
// RAG Engine Initialization
// ==============================================================================

// ==============================================================================
// Tests
// ==============================================================================

#[derive(Clone)]
struct McpProxyTool {
    name: String,
    tool_server_handle: rig_core::tool::server::ToolServerHandle,
}

impl McpProxyTool {
    fn new(name: &str, handle: rig_core::tool::server::ToolServerHandle) -> Self {
        Self {
            name: name.to_string(),
            tool_server_handle: handle,
        }
    }
}

impl rig_core::tool::ToolDyn for McpProxyTool {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> rig_core::wasm_compat::WasmBoxedFuture<'a, rig_core::completion::ToolDefinition> {
        let handle = self.tool_server_handle.clone();
        let name = self.name.clone();
        Box::pin(async move {
            if let Ok(defs) = handle.get_tool_defs(None).await
                && let Some(def) = defs.into_iter().find(|d| d.name == name)
            {
                return def;
            }
            rig_core::completion::ToolDefinition {
                name: name.clone(),
                description: format!("MCP proxy tool for {}", name),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            }
        })
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> rig_core::wasm_compat::WasmBoxedFuture<'a, Result<String, rig_core::tool::ToolError>> {
        let handle = self.tool_server_handle.clone();
        let name = self.name.clone();
        Box::pin(async move {
            handle
                .call_tool(&name, &args)
                .await
                .map_err(|e| rig_core::tool::ToolError::ToolCallError(Box::new(e)))
        })
    }
}

// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_extract_interests_section_extracts_correctly() {
        let user_md = "# User Profile\n\n## Basics\n- Name: K\n\n## Interests\n- AI Agent\n  sources: HN\n- Cloudflare\n  sources: blog\n\n## Work Context\n- Workplace: Atsugi";
        let interests = extract_interests_section(user_md);
        assert!(
            interests.contains("AI Agent"),
            "Interests セクションが抽出されること"
        );
        assert!(interests.contains("Cloudflare"), "複数行が含まれること");
        assert!(
            !interests.contains("Basics"),
            "Basics セクションは除外されること"
        );
        assert!(
            !interests.contains("Work Context"),
            "Work Context セクションは除外されること"
        );
    }

    #[test]
    fn test_extract_interests_section_empty_when_missing() {
        let user_md = "# User Profile\n\n## Basics\n- Name: K\n";
        let interests = extract_interests_section(user_md);
        assert!(
            interests.is_empty(),
            "Interests セクションがない場合は空文字を返すこと"
        );
    }

    #[tokio::test]
    async fn test_message_bus_pub_sub() -> Result<()> {
        let bus = MessageBus::new();
        let mut sub = bus.subscribe();

        bus.publish(SystemEvent::SystemError {
            message: "Test Error".to_string(),
        })?;

        let event = sub.recv().await?;
        if let SystemEvent::SystemError { message } = event {
            assert_eq!(message, "Test Error");
        } else {
            panic!("Expected SystemError event");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_lane_registry_serialization_and_semaphore() -> Result<()> {
        let ws_dir = tempdir()?;
        fs::write(ws_dir.path().join("SOUL.md"), "Soul Content")?;
        fs::write(ws_dir.path().join("AGENTS.md"), "Agents Content")?;
        fs::write(ws_dir.path().join("MEMORY.md"), "Memory Content")?;
        fs::write(ws_dir.path().join("USER.md"), "User Content")?;

        let bus = Arc::new(MessageBus::new());

        // LLM 通信用に mock/dummy プロバイダを返す config
        let config = Config::default();

        let tool_registry = Arc::new(rustyclaw_tools::ToolRegistry::new());
        let tool_server_handle = rig_core::tool::server::ToolServer::new().run();
        let test_lane_sem = Arc::new(Semaphore::new(1));
        let mock_discord = Arc::new(DiscordConnector::new("mock"));
        let registry = LaneRegistry::new(
            config,
            ws_dir.path().to_path_buf(),
            bus.clone(),
            tool_registry,
            tool_server_handle,
            test_lane_sem,
            mock_discord,
        );

        // メッセージを投入
        registry
            .dispatch(SystemEvent::IncomingMessage {
                session_id: "session-abc".to_string(),
                user_id: "user-123".to_string(),
                channel_id: "channel-456".to_string(),
                content: "Hello".to_string(),
                priority: Priority::Normal,
            })
            .await?;

        // テスト環境で gmn CLI プロバイダを実行するとエラーになる（gmn がないため）か、
        // または mock プロバイダとして動く。
        // ここでは、dispatch が正常に完了しキューに入れられること自体を確認する。

        // セッションごとの mpsc が登録されていることを確認
        let lanes = registry.lanes.lock().await;
        assert!(lanes.contains_key("session-abc"));

        Ok(())
    }

    #[tokio::test]
    async fn test_web_preview_server_serves_static_file() -> Result<()> {
        let ws_dir = tempdir()?;
        let previews_dir = ws_dir.path().join("previews");
        fs::create_dir_all(&previews_dir)?;
        fs::write(previews_dir.join("hello.html"), "<h1>hello world</h1>")?;

        let mut config = Config::default();
        config.web_preview.enabled = true;
        config.web_preview.host = "127.0.0.1".to_string();
        config.web_preview.port = 0;

        let (handle, bound_addr) =
            crate::web_preview::start_web_preview_server(&config, ws_dir.path().to_path_buf())
                .await?;

        let url = format!("http://{}/hello.html", bound_addr);
        let resp = reqwest::get(&url).await?;
        assert!(resp.status().is_success());
        let text = resp.text().await?;
        assert!(text.contains("hello world"));

        handle.abort();
        Ok(())
    }

    #[test]
    fn test_extract_patrol_feed_urls_http_only() {
        let interests = "- AI Agent\n  sources: HN\n- Cloudflare\n  sources: https://blog.cloudflare.com\n- GitHub Rust\n  sources: github:rust-lang/rust\n- RSS feed\n  sources: https://example.com/feed.xml\n- Reddit\n  sources: Reddit/r/rust";
        let urls = extract_patrol_feed_urls(interests);
        assert_eq!(
            urls.len(),
            2,
            "HTTP(S) URL が 2 件抽出されること: {:?}",
            urls
        );
        assert!(urls.contains(&"https://blog.cloudflare.com".to_string()));
        assert!(urls.contains(&"https://example.com/feed.xml".to_string()));
    }

    #[test]
    fn test_extract_patrol_feed_urls_empty_when_no_http() {
        let interests = "- AI Agent\n  sources: HN\n- GitHub\n  sources: github:rust-lang/rust";
        let urls = extract_patrol_feed_urls(interests);
        assert!(urls.is_empty(), "HTTP URL がない場合は空であること");
    }

    #[test]
    fn test_parse_skill_names_from_ctx_finds_skills() {
        let ctx = "Some context\n[skill:home-assistant-rest-api]\nSome instructions\n[skill:gmail]\nMore text";
        let mut names = parse_skill_names_from_ctx(ctx);
        names.sort();
        assert_eq!(names, vec!["gmail", "home-assistant-rest-api"]);
    }

    #[test]
    fn test_parse_skill_names_from_ctx_empty() {
        let names = parse_skill_names_from_ctx("no skill markers here");
        assert!(names.is_empty());
    }

    // Phase 52-6 tests
    #[test]
    fn test_extract_vital_alert_query_no_keywords() {
        let digest = "User asked about the weather. Replied with sunny 22°C.";
        assert!(extract_vital_alert_query(digest).is_none());
    }

    #[test]
    fn test_extract_vital_alert_query_sleep_keyword_en() {
        let digest = "User mentioned they felt tired and had poor sleep last night.";
        let query = extract_vital_alert_query(digest);
        assert!(query.is_some());
        let q = query.unwrap();
        assert!(q.contains("sleep"));
    }

    #[test]
    fn test_extract_vital_alert_query_sleep_keyword_ja() {
        let digest = "ユーザーが睡眠不足だと報告しました。";
        let query = extract_vital_alert_query(digest);
        assert!(query.is_some());
    }

    #[test]
    fn test_extract_vital_alert_query_fatigue_keyword() {
        let digest = "User feels fatigue from long work sessions.";
        let query = extract_vital_alert_query(digest);
        assert!(query.is_some());
    }

    // Phase 52-7 tests
    #[test]
    fn test_extract_vital_alert_query_returns_some_on_sleep() {
        let digest = "User mentioned poor sleep and fatigue.";
        assert!(extract_vital_alert_query(digest).is_some());
    }

    #[test]
    fn test_extract_vital_alert_query_returns_none_on_normal() {
        let digest = "User asked about the calendar for tomorrow.";
        assert!(extract_vital_alert_query(digest).is_none());
    }
}
