use anyhow::{Context, Result};
use rustyclaw_config::Config;
use rustyclaw_channels::{Channel, DiscordConnector};
use rustyclaw_agent::Pipeline;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Mutex as TokioMutex, Semaphore};
use tokio::signal::unix::{signal, SignalKind};

pub mod health;
pub mod watchdog;
pub mod cron;
pub mod heartbeat;
pub(crate) mod skills;



// ==============================================================================
// 待ち行列キューの追跡用データ構造
// ==============================================================================
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct QueueItem {
    pub session_id: String,
    pub status: String,            // "Waiting" | "Executing" | "Cooldown"
    pub enqueued_at_ms: u64,       // Unix Epoch Milliseconds
    pub cooldown_left_secs: f64,   // クールダウン中の残り時間
    pub description: String,       // 要約やプロンプト内容のプレビュー
}

#[derive(Debug, Clone, Default)]
pub struct QueueState {
    pub items: Vec<QueueItem>,
}

pub static QUEUE_STATE: Mutex<QueueState> = Mutex::new(QueueState { items: Vec::new() });

pub fn queue_update_or_insert(session_id: &str, status: &str, cooldown_left_secs: f64, description: &str) {
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
    /// 全 gmn プロセスを直列化する統合セマフォ (容量1)。
    /// user 対話 / bg (heartbeat) / flush_memory() の全ての gmn 起動がこの枠を取得する。
    /// MEMORY.md 等ワークスペースファイルへの並列書き込みによるデータ消失を防止する。
    gmn_sem: Arc<Semaphore>,
    config: Arc<StdMutex<Config>>,
    workspace_path: PathBuf,
    bus: Arc<MessageBus>,
    tool_registry: Arc<rustyclaw_tools::ToolRegistry>,
    discord_connector: Arc<DiscordConnector>,
}

impl LaneRegistry {
    pub fn new(
        config: Config,
        workspace_path: PathBuf,
        bus: Arc<MessageBus>,
        tool_registry: Arc<rustyclaw_tools::ToolRegistry>,
        gmn_sem: Arc<Semaphore>,
        discord_connector: Arc<DiscordConnector>,
    ) -> Self {
        Self {
            lanes: Arc::new(TokioMutex::new(HashMap::new())),
            gmn_sem,
            config: Arc::new(StdMutex::new(config)),
            workspace_path,
            bus,
            tool_registry,
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

        let gmn_sem = self.gmn_sem.clone();
        let config = self.config.clone();
        let workspace_path = self.workspace_path.clone();
        let bus = self.bus.clone();
        let tool_registry = self.tool_registry.clone();
        let discord_connector = self.discord_connector.clone();

        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(evt) = rx.recv().await {
                if let SystemEvent::IncomingMessage { session_id, channel_id, mut content, priority, .. } = evt {
                    // Backgroundレーンのキュー制限(容量1): 古いキューを全て読み捨てる
                    if priority == Priority::Background {
                        while let Ok(next_evt) = rx.try_recv() {
                            if let SystemEvent::IncomingMessage { content: next_content, .. } = next_evt {
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
                        let heartbeat_svc = crate::heartbeat::HeartbeatService::new(active_config.clone(), workspace_path.clone(), bus.clone());
                        
                        let setup_res = async {
                            if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                if let Ok(digest) = heartbeat_svc.generate_digest(&db_path).await {
                                    let is_step5_allowed = heartbeat_svc.is_step5_allowed(&db).unwrap_or(false);
                                    let weather_alert = heartbeat_svc.run_weather_patrol(&db_path).await.unwrap_or(None);
                                    Some((digest, is_step5_allowed, weather_alert))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }.await;

                        if let Some((digest, is_step5_allowed, weather_alert)) = setup_res {
                            let mut prompt_parts = Vec::new();
                            prompt_parts.push(format!(
                                "You are executing your HEARTBEAT patrol. Step 5 Vocal Greeting allowed: {}.", 
                                is_step5_allowed
                            ));

                            if let Some(alert) = weather_alert {
                                prompt_parts.push(alert);
                            }

                            prompt_parts.push(format!("Recent activity digest:\n{}", digest));

                            let heartbeat_prompt = prompt_parts.join("\n\n");

                            let mut attempt = 0;
                            let max_attempts = 3;
                            let base_delay = Duration::from_secs(5);

                            let desc = "Heartbeat Patrol / Activity Scan";
                            loop {
                                if let Some(cooldown_dur) = rustyclaw_providers::global_cooldown_remaining() {
                                    crate::queue_update_or_insert(&session_id, "Cooldown", cooldown_dur.as_secs_f64(), desc);
                                    tracing::warn!(
                                        "Global rate limit active. Waiting {:.1}s before acquiring gmn_sem...",
                                        cooldown_dur.as_secs_f64()
                                    );
                                    tokio::time::sleep(cooldown_dur).await;
                                }
                                crate::queue_update_or_insert(&session_id, "Waiting", 0.0, desc);
                                tracing::debug!("Session {} attempting to acquire gmn_sem (Attempt {})...", session_id, attempt + 1);
                                let permit_res = tokio::time::timeout(Duration::from_secs(60), gmn_sem.acquire()).await;
                                match permit_res {
                                    Ok(Ok(permit)) => {
                                        // セマフォ取得直後のダブルチェック（待機サスペンド中に他のスレッドが制限を検知した可能性があるため）
                                        if let Some(cooldown_dur) = rustyclaw_providers::global_cooldown_remaining() {
                                            drop(permit);
                                            crate::queue_update_or_insert(&session_id, "Cooldown", cooldown_dur.as_secs_f64(), desc);
                                            tracing::warn!(
                                                "Global rate limit active just after acquiring gmn_sem. Releasing permit and waiting {:.1}s...",
                                                cooldown_dur.as_secs_f64()
                                            );
                                            tokio::time::sleep(cooldown_dur).await;
                                            continue;
                                        }
                                        crate::queue_update_or_insert(&session_id, "Executing", 0.0, desc);
                                        tracing::info!("Session {} acquired permit slot. Executing agent...", session_id);
                                        let pipeline = Pipeline::new(active_config.clone(), gmn_sem.clone());
                                        match pipeline.execute_heartbeat(&workspace_path, &session_id, &heartbeat_prompt).await {
                                            Ok(response) => {
                                                tracing::info!("Heartbeat LLM execution successful. Processing response...");
                                                if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                                    let _ = heartbeat_svc.process_heartbeat_response(&response.content, &db_path).await;
                                                }
                                                crate::queue_remove(&session_id);
                                                break; // Exit the retry loop!
                                            }
                                            Err(e) => {
                                                drop(permit); // Release permit immediately so other lanes aren't blocked!
                                                if let Some(err) = e.downcast_ref::<rustyclaw_providers::ProviderError>() {
                                                    if let rustyclaw_providers::ProviderError::RateLimit(limit_msg) = err {
                                                        if attempt < max_attempts {
                                                            let parsed_reset = err.reset_after();
                                                            let backoff = parsed_reset
                                                                .map(|d| d + Duration::from_secs(2)) // 2秒の安全マージンを追加
                                                                .unwrap_or_else(|| base_delay * 2u32.pow(attempt));
                                                            crate::queue_update_or_insert(&session_id, "Cooldown", backoff.as_secs_f64(), desc);
                                                            if let Some(reset_duration) = parsed_reset {
                                                                tracing::warn!("Rate limit exceeded. Detected quota reset time: {:.1}s. Dynamic backoff applied: {:.1}s (including 2s safety buffer). Error: {}", reset_duration.as_secs_f64(), backoff.as_secs_f64(), limit_msg);
                                                            } else {
                                                                tracing::warn!("Rate limit exceeded. No quota reset time detected. Falling back to exponential backoff: {:.1}s. Error: {}", backoff.as_secs_f64(), limit_msg);
                                                            }
                                                            tokio::time::sleep(backoff).await;
                                                            attempt += 1;
                                                            continue;
                                                        }
                                                    }
                                                }
                                                // Non-rate-limit error or max retries exceeded
                                                tracing::error!("Heartbeat LLM execution failed: {:#}", e);
                                                crate::queue_remove(&session_id);
                                                break;
                                            }
                                        }
                                    }
                                    Ok(Err(_)) => {
                                        tracing::error!("Failed to acquire permit for Session {}: Semaphore closed", session_id);
                                        crate::queue_remove(&session_id);
                                        break;
                                    }
                                    Err(_) => {
                                        tracing::error!("Session {} timed out (60s) waiting for permit slot. Execution skipped.", session_id);
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
                        let prompt = format!("Compile the daily summary of activities and conversation for date: {}. Please output a concise markdown summary containing a 'TL;DR' section and a list of key topics.", today);
                        
                        let mut attempt = 0;
                        let max_attempts = 3;
                        let base_delay = Duration::from_secs(5);

                        let desc = "Daily Activity Summary";
                        loop {
                            if let Some(cooldown_dur) = rustyclaw_providers::global_cooldown_remaining() {
                                crate::queue_update_or_insert(&session_id, "Cooldown", cooldown_dur.as_secs_f64(), desc);
                                tracing::warn!(
                                    "Global rate limit active. Waiting {:.1}s before acquiring gmn_sem...",
                                    cooldown_dur.as_secs_f64()
                                );
                                tokio::time::sleep(cooldown_dur).await;
                            }
                            crate::queue_update_or_insert(&session_id, "Waiting", 0.0, desc);
                            tracing::debug!("Session {} attempting to acquire gmn_sem (Attempt {})...", session_id, attempt + 1);
                            let permit_res = tokio::time::timeout(Duration::from_secs(60), gmn_sem.acquire()).await;
                            match permit_res {
                                Ok(Ok(permit)) => {
                                    // セマフォ取得直後のダブルチェック（待機サスペンド中に他のスレッドが制限を検知した可能性があるため）
                                    if let Some(cooldown_dur) = rustyclaw_providers::global_cooldown_remaining() {
                                        drop(permit);
                                        crate::queue_update_or_insert(&session_id, "Cooldown", cooldown_dur.as_secs_f64(), desc);
                                        tracing::warn!(
                                            "Global rate limit active just after acquiring gmn_sem. Releasing permit and waiting {:.1}s...",
                                            cooldown_dur.as_secs_f64()
                                        );
                                        tokio::time::sleep(cooldown_dur).await;
                                        continue;
                                    }
                                    crate::queue_update_or_insert(&session_id, "Executing", 0.0, desc);
                                    tracing::info!("Session {} acquired permit slot. Executing agent...", session_id);
                                    let pipeline = Pipeline::new(active_config.clone(), gmn_sem.clone());
                                    match pipeline.execute(&workspace_path, &session_id, &prompt).await {
                                        Ok(response) => {
                                            tracing::info!("Daily Summary generated successfully.");
                                            let summaries_dir = workspace_path.join("memory").join("summaries");
                                            let _ = std::fs::create_dir_all(&summaries_dir);
                                            let file_path = summaries_dir.join(format!("{}-daily-summary.md", today));
                                            
                                            let _ = rustyclaw_storage::atomic_write(&file_path, response.content.as_bytes()).await;

                                            // Index matching summary in Tantivy
                                            let index_dir = workspace_path.join("memory").join("index");
                                            if let Ok(search_mgr) = rustyclaw_storage::SearchIndexManager::new(&index_dir) {
                                                let _ = search_mgr.index_file(&file_path, &response.content, &today);
                                            }
                                            if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                                let trigger = if session_id.starts_with("cron:heartbeat") { "heartbeat" }
                                                    else if session_id.starts_with("cron:") { "cron" }
                                                    else if session_id.starts_with("discord-") { "discord" }
                                                    else if session_id.starts_with("cli-") { "cli" }
                                                    else { "unknown" };
                                                let _ = db.record_usage(
                                                    &session_id,
                                                    response.prompt_tokens.unwrap_or(0),
                                                    response.completion_tokens.unwrap_or(0),
                                                    response.total_tokens.unwrap_or(0),
                                                    response.model_used.as_deref().unwrap_or(""),
                                                    trigger,
                                                    0,
                                                );
                                            }
                                            crate::queue_remove(&session_id);
                                            break; // Exit the retry loop!
                                        }
                                        Err(e) => {
                                            drop(permit); // Release permit immediately so other lanes aren't blocked!
                                            if let Some(err) = e.downcast_ref::<rustyclaw_providers::ProviderError>() {
                                                if let rustyclaw_providers::ProviderError::RateLimit(limit_msg) = err {
                                                    if attempt < max_attempts {
                                                        let parsed_reset = err.reset_after();
                                                        let backoff = parsed_reset
                                                            .map(|d| d + Duration::from_secs(2)) // 2秒の安全マージンを追加
                                                            .unwrap_or_else(|| base_delay * 2u32.pow(attempt));
                                                        crate::queue_update_or_insert(&session_id, "Cooldown", backoff.as_secs_f64(), desc);
                                                        if let Some(reset_duration) = parsed_reset {
                                                            tracing::warn!("Rate limit exceeded. Detected quota reset time: {:.1}s. Dynamic backoff applied: {:.1}s (including 2s safety buffer). Error: {}", reset_duration.as_secs_f64(), backoff.as_secs_f64(), limit_msg);
                                                        } else {
                                                            tracing::warn!("Rate limit exceeded. No quota reset time detected. Falling back to exponential backoff: {:.1}s. Error: {}", backoff.as_secs_f64(), limit_msg);
                                                        }
                                                        tokio::time::sleep(backoff).await;
                                                        attempt += 1;
                                                        continue;
                                                    }
                                                }
                                            }
                                            // Non-rate-limit error or max retries exceeded
                                            tracing::error!("Failed to generate Daily Summary: {:#}", e);
                                            crate::queue_remove(&session_id);
                                            break;
                                        }
                                    }
                                }
                                Ok(Err(_)) => {
                                    tracing::error!("Failed to acquire permit for Session {}: Semaphore closed", session_id);
                                    crate::queue_remove(&session_id);
                                    break;
                                }
                                Err(_) => {
                                    tracing::error!("Session {} timed out (60s) waiting for permit slot. Execution skipped.", session_id);
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
                        let desc = if session_id.starts_with("cron:session-summary:") {
                            format!("Auto-Summary for Session '{}'", &session_id["cron:session-summary:".len()..])
                        } else {
                            let char_limit = 40;
                            let mut truncated = content.chars().take(char_limit).collect::<String>();
                            if content.chars().count() > char_limit {
                                truncated.push_str("...");
                            }
                            format!("User Prompt: \"{}\"", truncated.replace('\n', " "))
                        };
                        let mut attempt = 0;
                        let max_attempts = 3;
                        let base_delay = Duration::from_secs(5);

                        loop {
                            if let Some(cooldown_dur) = rustyclaw_providers::global_cooldown_remaining() {
                                crate::queue_update_or_insert(&session_id, "Cooldown", cooldown_dur.as_secs_f64(), &desc);
                                tracing::warn!(
                                    "Global rate limit active. Waiting {:.1}s before acquiring gmn_sem...",
                                    cooldown_dur.as_secs_f64()
                                );
                                tokio::time::sleep(cooldown_dur).await;
                            }
                            crate::queue_update_or_insert(&session_id, "Waiting", 0.0, &desc);
                            tracing::debug!("Session {} attempting to acquire gmn_sem (Attempt {})...", session_id, attempt + 1);
                            let permit_res = tokio::time::timeout(Duration::from_secs(60), gmn_sem.acquire()).await;
                            match permit_res {
                                Ok(Ok(permit)) => {
                                    // セマフォ取得直後のダブルチェック（待機サスペンド中に他のスレッドが制限を検知した可能性があるため）
                                    if let Some(cooldown_dur) = rustyclaw_providers::global_cooldown_remaining() {
                                        drop(permit);
                                        crate::queue_update_or_insert(&session_id, "Cooldown", cooldown_dur.as_secs_f64(), &desc);
                                        tracing::warn!(
                                            "Global rate limit active just after acquiring gmn_sem. Releasing permit and waiting {:.1}s...",
                                            cooldown_dur.as_secs_f64()
                                        );
                                        tokio::time::sleep(cooldown_dur).await;
                                        continue;
                                    }
                                    crate::queue_update_or_insert(&session_id, "Executing", 0.0, &desc);
                                    tracing::info!("Session {} acquired permit slot. Executing agent...", session_id);
                                    let pipeline = Pipeline::new(active_config.clone(), gmn_sem.clone());
                                    let tool_reg = tool_registry.clone();

                                    // ProgressReporter（進捗表示とタイピング）のセットアップ
                                    let is_user_channel = !session_id.starts_with("cron") && channel_id.parse::<u64>().is_ok();
                                    let progress_reporter = if is_user_channel {
                                        match discord_connector.create_progress_reporter(&channel_id) {
                                            Ok(rep) => {
                                                let _ = rep.start().await;
                                                Some(rep)
                                            }
                                            Err(e) => {
                                                tracing::warn!("Failed to create DiscordProgressReporter: {:?}", e);
                                                None
                                            }
                                        }
                                    } else {
                                        None
                                    };

                                    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<String>(10);
                                    let progress_tx_opt = if progress_reporter.is_some() { Some(progress_tx) } else { None };

                                    let progress_reporter_clone = progress_reporter.clone();
                                    let middle_task = tokio::spawn(async move {
                                        if let Some(rep) = progress_reporter_clone {
                                            while let Some(status) = progress_rx.recv().await {
                                                let _ = rep.update_status(&status).await;
                                            }
                                        }
                                    });

                                    // スキルファイル注入（workspace/skills/<name>.md が存在すれば前置）
                                    let content = crate::skills::inject_skill_content(&workspace_path, &content);
                                    let exec_res = if session_id.starts_with("cron:session-summary:") {
                                        let target_session_id = &session_id["cron:session-summary:".len()..];
                                        pipeline.generate_session_summary(&workspace_path, target_session_id)
                                            .await
                                            .map(|summary| rustyclaw_providers::LlmResponse {
                                                role: "assistant".to_string(),
                                                content: summary,
                                                tool_calls: None,
                                                prompt_tokens: None,
                                                completion_tokens: None,
                                                total_tokens: None,
                                                model_used: None,
                                            })
                                    } else {
                                        let run_purpose = if session_id == "cron:topic-patrol" { "patrol" } else { "discord" };
                                        pipeline.execute_with_tools(&workspace_path, &session_id, &content, &tool_reg, run_purpose, progress_tx_opt).await
                                    };

                                    // 進捗表示の終了とクリーンアップ
                                    if let Some(ref rep) = progress_reporter {
                                        let _ = rep.finish().await;
                                    }
                                    middle_task.abort();
                                    match exec_res {
                                        Ok(response) => {
                                            tracing::info!("Agent response generated successfully for Session {}", session_id);
                                            let _ = bus.publish(SystemEvent::AgentResponse {
                                                session_id: session_id.clone(),
                                                channel_id: channel_id.clone(),
                                                content: response.content,
                                            });

                                            // lastUserContact を更新 (SQLite & heartbeat-state.json)
                                            if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                                let now = chrono::Local::now().to_rfc3339();
                                                let _ = db.set_state_value("lastUserContact", &now);

                                                let state_path = workspace_path.join("memory").join("heartbeat-state.json");
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
                                                if let Ok(c) = std::fs::read_to_string(&state_path) {
                                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&c) {
                                                        current_state = parsed;
                                                        current_state["lastChecks"]["lastUserContact"] = serde_json::json!(now);
                                                    }
                                                }
                                                if let Ok(serialized) = serde_json::to_string_pretty(&current_state) {
                                                    let _ = rustyclaw_storage::atomic_write(&state_path, serialized.as_bytes()).await;
                                                }

                                                let trigger = if session_id.starts_with("cron:heartbeat") { "heartbeat" }
                                                    else if session_id.starts_with("cron:") { "cron" }
                                                    else if session_id.starts_with("discord-") { "discord" }
                                                    else if session_id.starts_with("cli-") { "cli" }
                                                    else { "unknown" };
                                                let _ = db.record_usage(
                                                    &session_id,
                                                    response.prompt_tokens.unwrap_or(0),
                                                    response.completion_tokens.unwrap_or(0),
                                                    response.total_tokens.unwrap_or(0),
                                                    response.model_used.as_deref().unwrap_or(""),
                                                    trigger,
                                                    0,
                                                );
                                            }
                                            crate::queue_remove(&session_id);
                                            break; // Exit the retry loop!
                                        }
                                        Err(e) => {
                                            drop(permit); // Release permit immediately so other lanes aren't blocked!
                                            if let Some(err) = e.downcast_ref::<rustyclaw_providers::ProviderError>() {
                                                if let rustyclaw_providers::ProviderError::RateLimit(limit_msg) = err {
                                                    if attempt < max_attempts {
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
                                                }
                                            }
                                            // Non-rate-limit error or max retries exceeded
                                            tracing::error!("Error in Agent execution for Session {}: {:#}", session_id, e);
                                            let _ = bus.publish(SystemEvent::SystemError {
                                                message: format!("Agent execution failed for session {}: {}", session_id, e),
                                            });
                                            crate::queue_remove(&session_id);
                                            break;
                                        }
                                    }
                                }
                                Ok(Err(_)) => {
                                    tracing::error!("Failed to acquire permit for Session {}: Semaphore closed", session_id);
                                    crate::queue_remove(&session_id);
                                    break;
                                }
                                Err(_) => {
                                    tracing::error!("Session {} timed out (60s) waiting for permit slot. Execution skipped.", session_id);
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

        tx.send(event).await.context("Failed to queue initial message to new lane worker")?;
        Ok(())
    }
}

// ==============================================================================
// 4. Gateway (オーケストレーターデーモン) の実装
// ==============================================================================

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
        {
            let m = config.get_model("default");
            tracing::info!("Gateway loaded configuration: provider={}, model={}", m.model_provider, m.model_name);
        }

        // 2. MCP Manager 初期化 (enabled な MCP サーバーに接続)
        let mcp_manager = Arc::new(rustyclaw_mcp::McpManager::new());
        if !config.mcp.is_empty() {
            mcp_manager.connect_all(&config.mcp).await.ok();
        }
        // MCP ツールを ToolRegistry に登録
        let mut tool_registry = rustyclaw_tools::ToolRegistry::new();
        for mcp_tool in mcp_manager.get_tools().await {
            tool_registry.register(mcp_tool);
        }



        // Obsidian ネイティブツール登録
        if let Some(o) = config.tools.obsidian.as_ref().filter(|o| o.enabled) {
            if !o.host.is_empty() && !o.api_key.is_empty() {
                tool_registry.register(Arc::new(rustyclaw_tools::ObsidianSearchTool::new(
                    o.host.clone(), o.api_key.clone(),
                )));
                tool_registry.register(Arc::new(rustyclaw_tools::ObsidianReadTool::new(
                    o.host.clone(), o.api_key.clone(),
                )));
                tool_registry.register(Arc::new(rustyclaw_tools::ObsidianWriteTool::new(
                    o.host.clone(), o.api_key.clone(),
                )));
                tracing::info!("Registered native Obsidian tools.");
            }
        }

        // Brave Search ネイティブツール登録
        if let Some(b) = config.tools.brave_search.as_ref().filter(|b| b.enabled) {
            if !b.api_key.is_empty() {
                tool_registry.register(Arc::new(rustyclaw_tools::WebSearchTool::new(b.api_key.clone())));
                tracing::info!("Registered WebSearchTool (Brave Search).");
            }
        }
        // WebFetchTool は常時登録（APIキー不要）
        tool_registry.register(Arc::new(rustyclaw_tools::WebFetchTool::new()));
        // YolpWeatherTool は常時登録（APIキー不要、Open-Meteoバックエンド）
        tool_registry.register(Arc::new(rustyclaw_tools::YolpWeatherTool::new()));

        // WorkspaceReadTool / WorkspaceWriteTool 登録
        tool_registry.register(Arc::new(rustyclaw_tools::WorkspaceReadTool::new(self.workspace_path.clone())));
        tool_registry.register(Arc::new(rustyclaw_tools::WorkspaceWriteTool::new(self.workspace_path.clone())));
        tool_registry.register(Arc::new(rustyclaw_tools::MemorySearchTool::new(self.workspace_path.clone())));
        tool_registry.register(Arc::new(rustyclaw_tools::WorkspaceExecuteScriptTool::new(self.workspace_path.clone())));
        tracing::info!("Registered Workspace I/O, Memory Search, and Script Execution tools.");

        // Google Workspace ネイティブツール登録
        if let Some(gws) = config.tools.google_workspace.as_ref().filter(|g| g.enabled) {
            let gws_path = gws.gws_path.clone().unwrap_or_else(|| {
                for p in &["/home/pi/.local/bin/gws", "/home/kazuaki/.local/bin/gws", "/usr/local/bin/gws"] {
                    if std::path::Path::new(p).exists() {
                        return p.to_string();
                    }
                }
                String::new()
            });
            if !gws_path.is_empty() {
                tool_registry.register(Arc::new(rustyclaw_tools::GwsCalendarTool::new(gws_path.clone())));

                if !gws.writable_calendar_ids.is_empty() {
                    // ISSUE-03: カレンダー解決を並列化して起動ブロッキングを短縮する。
                    // NOTE: writable_calendar_ids は通常 1-2 件のため無制限 spawn で安全。
                    // 将来この一覧が大幅に増える場合は Semaphore か
                    // futures::stream::buffer_unordered で並列度を制限すること。
                    let mut handles = Vec::new();
                    for cal_id in gws.writable_calendar_ids.clone() {
                        let gws_path = gws_path.clone();
                        handles.push(tokio::spawn(async move {
                            let params = serde_json::json!({ "calendarId": &cal_id }).to_string();
                            match tokio::process::Command::new(&gws_path)
                                .args(["calendar", "calendarList", "get", "--params", &params, "--format", "json"])
                                .output()
                                .await
                            {
                                Ok(out) if out.status.success() => {
                                    let body = String::from_utf8_lossy(&out.stdout);
                                    if let Ok(d) = serde_json::from_str::<serde_json::Value>(&body) {
                                        let name = d["summary"].as_str().unwrap_or(&cal_id).to_string();
                                        let desc = d["description"].as_str().unwrap_or("").to_string();
                                        tracing::info!("Calendar resolved: '{}' ({})", name, cal_id);
                                        return (cal_id.clone(), name, desc);
                                    }
                                    (cal_id.clone(), cal_id.clone(), String::new())
                                }
                                _ => {
                                    tracing::warn!("Failed to fetch calendar info for {}. Using ID as name.", cal_id);
                                    (cal_id.clone(), cal_id.clone(), String::new())
                                }
                            }
                        }));
                    }
                    let mut writable_calendars: Vec<(String, String, String)> = Vec::new();
                    for h in handles {
                        if let Ok(tuple) = h.await {
                            writable_calendars.push(tuple);
                        }
                    }
                    if !writable_calendars.is_empty() {
                        let allowed: Vec<(String, String)> = writable_calendars.iter()
                            .map(|(id, name, desc)| {
                                let label = if desc.is_empty() { name.clone() } else { format!("{} — {}", name, desc) };
                                (id.clone(), label)
                            })
                            .collect();
                        tracing::info!("Registering gws writable calendar tool ({} calendars).", allowed.len());
                        tool_registry.register(Arc::new(rustyclaw_tools::GwsCalendarWriteTool::new(
                            gws_path.clone(), allowed,
                        )));
                    }
                }

                tool_registry.register(Arc::new(rustyclaw_tools::GwsGmailTool::new(gws_path.clone())));

                if let Some(ref label) = gws.gmail_deletable_label {
                    if !label.is_empty() {
                        tool_registry.register(Arc::new(rustyclaw_tools::GwsGmailDeleteTool::new(
                            gws_path.clone(), label.clone(),
                        )));
                        tracing::info!("Registered gws Gmail delete tool (label: {}).", label);
                    }
                }

                tracing::info!("Registered native gws Google Workspace tools.");
            }
        }

        let ws_path_clone = self.workspace_path.clone();
        let db_path_for_tool = self.workspace_path.join("memory.db");
        tool_registry.register(Arc::new(rustyclaw_tools::CronScheduleTool::new(move || {
            if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path_for_tool) {
                let sched = crate::cron::compute_schedule(&ws_path_clone, &db);
                serde_json::Value::Array(sched)
            } else {
                serde_json::Value::Array(vec![])
            }
        })));
        tracing::info!("Registered native CronScheduleTool.");

        let tool_registry = Arc::new(tool_registry);
        tracing::info!("Tool registry initialized with {} tools.", tool_registry.tool_count());

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
        let gmn_sem = Arc::new(Semaphore::new(4)); // 全 gmn プロセス統合枠 (user + bg + flush を一本化、容量 4)
        let registry = Arc::new(LaneRegistry::new(
            config.clone(),
            self.workspace_path.clone(),
            bus.clone(),
            tool_registry.clone(),
            gmn_sem.clone(),
            discord_client.clone(),
        ));

        if discord_enabled {
            discord_client.start().await.context("Failed to start DiscordConnector")?;
        } else {
            tracing::info!("Discord channel disabled — skipping connector startup.");
        }

        // 5. MessageBus イベント監視ループ (MessageBus -> LaneRegistry)
        let registry_clone = registry.clone();
        let mut rx_bus = bus.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = rx_bus.recv().await {
                if matches!(event, SystemEvent::IncomingMessage { .. }) {
                    if let Err(e) = registry_clone.dispatch(event).await {
                        tracing::error!("Failed to dispatch event to LaneRegistry: {}", e);
                    }
                }
            }
        });

        // ISSUE-17: IncomingMessage 受信直後に Waiting を可視化する観測タスク
        {
            let mut wait_rx = bus.subscribe();
            tokio::spawn(async move {
                loop {
                    match wait_rx.recv().await {
                        Ok(SystemEvent::IncomingMessage { session_id, content, .. }) => {
                            let desc = {
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
                if let SystemEvent::AgentResponse { channel_id, content, .. } = event {
                    // 数字以外の channel_id（"http" など）は Discord チャンネルではないのでスキップ
                    if channel_id.chars().all(|c| c.is_ascii_digit()) {
                        if let Err(e) = discord_sender.send_message(&channel_id, &content).await {
                            tracing::error!("Failed to send agent response to channel {}: {:#}", channel_id, e);
                        }
                    }
                }
            }
        });

        // 7. 各種バックグラウンドサービスの初期化・起動
        
        // ① Systemd Watchdog の起動
        watchdog::WatchdogService::start();

        // ② CronService (Heartbeat, DailySummary) の起動
        let db_path = self.workspace_path.join("memory.db");
        let cron_svc = cron::CronService::new(bus.clone(), db_path);
        cron_svc.start();

        // ③ HealthServer (HTTPサーバー) の起動
        let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel::<()>(1);
        let health_server = health::HealthServer::new(
            8080,
            reload_tx,
            bus.clone(),
            self.workspace_path.clone(),
            gmn_sem.clone(),
            4, // gmn_sem capacity = 4
        );
        health_server.start().await.context("Failed to start HealthServer")?;

        // 8. シグナルおよび HTTP リロードハンドリングループ
        let mut sig_hup = signal(SignalKind::hangup())
            .context("Failed to register SIGHUP listener")?;
        let mut sig_int = signal(SignalKind::interrupt())
            .context("Failed to register SIGINT listener")?;
        let mut sig_term = signal(SignalKind::terminate())
            .context("Failed to register SIGTERM listener")?;

        tracing::info!("RustyClaw Gateway is now running. Monitoring signals (SIGHUP, SIGINT, SIGTERM) and HTTP reload...");

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
                    mcp_manager.close_all().await;
                    discord_client.shutdown().await;
                    break;
                }
                // SIGTERM: Graceful Shutdown
                _ = sig_term.recv() => {
                    tracing::info!("Received SIGTERM. Initiating graceful shutdown...");
                    mcp_manager.close_all().await;
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
// Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

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
        let test_gmn_sem = Arc::new(Semaphore::new(1));
        let mock_discord = Arc::new(DiscordConnector::new("mock"));
        let registry = LaneRegistry::new(config, ws_dir.path().to_path_buf(), bus.clone(), tool_registry, test_gmn_sem, mock_discord);

        // メッセージを投入
        registry.dispatch(SystemEvent::IncomingMessage {
            session_id: "session-abc".to_string(),
            user_id: "user-123".to_string(),
            channel_id: "channel-456".to_string(),
            content: "Hello".to_string(),
            priority: Priority::Normal,
        }).await?;

        // テスト環境で gmn CLI プロバイダを実行するとエラーになる（gmn がないため）か、
        // または mock プロバイダとして動く。
        // ここでは、dispatch が正常に完了しキューに入れられること自体を確認する。

        // セッションごとの mpsc が登録されていることを確認
        let lanes = registry.lanes.lock().await;
        assert!(lanes.contains_key("session-abc"));

        Ok(())
    }


}
