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


// ==============================================================================
// 1. システムイベントと優先度定義
// ==============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Priority {
    Normal,     // ユーザー対話 (Semaphoreスロット数 4)
    Background, // 自発パトロール / Heartbeat (Semaphoreスロット数 2)
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
    user_sem: Arc<Semaphore>,
    bg_sem: Arc<Semaphore>,
    /// flush_memory() 専用セマフォ (容量1)。
    /// セマフォ管理外で走っていた flush gmn を制限し、意図した上限を超えないようにする。
    flush_sem: Arc<Semaphore>,
    config: Arc<StdMutex<Config>>,
    workspace_path: PathBuf,
    bus: Arc<MessageBus>,
}

impl LaneRegistry {
    pub fn new(config: Config, workspace_path: PathBuf, bus: Arc<MessageBus>) -> Self {
        Self {
            lanes: Arc::new(TokioMutex::new(HashMap::new())),
            user_sem: Arc::new(Semaphore::new(2)),  // ユーザー対話同時枠 (Antigravity 2.0 クォータ保護のため抑制)
            bg_sem: Arc::new(Semaphore::new(1)),    // バックグラウンド同時枠 (heartbeat / daily-summary)
            flush_sem: Arc::new(Semaphore::new(1)), // flush_memory() 専用枠
            config: Arc::new(StdMutex::new(config)),
            workspace_path,
            bus,
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

        let user_sem = self.user_sem.clone();
        let bg_sem = self.bg_sem.clone();
        let flush_sem = self.flush_sem.clone();
        let config = self.config.clone();
        let workspace_path = self.workspace_path.clone();
        let bus = self.bus.clone();

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

                    // レイヤー3：優先度に応じたセマフォの獲得 (待機タイムアウト 60秒)
                    let sem = match priority {
                        Priority::Normal => user_sem.clone(),
                        Priority::Background => bg_sem.clone(),
                    };

                    tracing::debug!("Session {} attempting to acquire {:?} slot permit...", session_id, priority);
                    
                    let permit_res = tokio::time::timeout(Duration::from_secs(60), sem.acquire()).await;
                    
                    match permit_res {
                        Ok(Ok(_permit)) => {
                            tracing::info!("Session {} acquired permit slot. Executing agent...", session_id);
                            
                            // 最新設定の取得
                            let active_config = {
                                let cfg = config.lock().unwrap();
                                cfg.clone()
                            };

                            let db_path = workspace_path.join("memory.db");

                            // 1. Heartbeat 実行処理
                            if session_id == "cron:heartbeat" {
                                let heartbeat_svc = crate::heartbeat::HeartbeatService::new(active_config.clone(), workspace_path.clone(), bus.clone());
                                
                                let setup_res = {
                                    if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        if let Ok(digest) = heartbeat_svc.generate_digest(&db) {
                                            let is_step5_allowed = heartbeat_svc.is_step5_allowed(&db).unwrap_or(false);
                                            Some((digest, is_step5_allowed))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                };

                                if let Some((digest, is_step5_allowed)) = setup_res {
                                    let heartbeat_prompt = format!(
                                        "You are executing your HEARTBEAT patrol. Step 5 Vocal Greeting allowed: {}.\n\nRecent activity digest:\n{}", 
                                        is_step5_allowed, digest
                                    );

                                    let pipeline = Pipeline::new(active_config, flush_sem.clone());
                                    match pipeline.execute(&workspace_path, &session_id, &heartbeat_prompt).await {
                                        Ok(response) => {
                                            tracing::info!("Heartbeat LLM execution successful. Processing response...");
                                            if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                                let _ = heartbeat_svc.process_heartbeat_response(&response.content, &db);
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Heartbeat LLM execution failed: {:#}", e);
                                        }
                                    }
                                }
                            }
                            // 2. Daily Summary 実行処理
                            else if session_id == "cron:daily-summary" {
                                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                                let prompt = format!("Compile the daily summary of activities and conversation for date: {}. Please output a concise markdown summary containing a 'TL;DR' section and a list of key topics.", today);
                                
                                let pipeline = Pipeline::new(active_config, flush_sem.clone());
                                match pipeline.execute(&workspace_path, &session_id, &prompt).await {
                                    Ok(response) => {
                                        tracing::info!("Daily Summary generated successfully.");
                                        let summaries_dir = workspace_path.join("memory").join("summaries");
                                        let _ = std::fs::create_dir_all(&summaries_dir);
                                        let file_path = summaries_dir.join(format!("{}-daily-summary.md", today));
                                        
                                        let _ = rustyclaw_storage::atomic_write(&file_path, response.content.as_bytes());
                                        
                                        // Index matching summary in Tantivy
                                        let index_dir = workspace_path.join("memory").join("index");
                                        if let Ok(search_mgr) = rustyclaw_storage::SearchIndexManager::new(&index_dir) {
                                            let _ = search_mgr.index_file(&file_path, &response.content, &today);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to generate Daily Summary: {:#}", e);
                                    }
                                }
                            }
                            // 3. 通常ユーザーセッション実行処理
                            else {
                                let pipeline = Pipeline::new(active_config, flush_sem.clone());
                                match pipeline.execute(&workspace_path, &session_id, &content).await {
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
                                                let _ = rustyclaw_storage::atomic_write(&state_path, serialized.as_bytes());
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Error in Agent execution for Session {}: {:#}", session_id, e);
                                        let _ = bus.publish(SystemEvent::SystemError {
                                            message: format!("Agent execution failed for session {}: {}", session_id, e),
                                        });
                                    }
                                }
                            }
                        }
                        Ok(Err(_)) => {
                            tracing::error!("Failed to acquire permit for Session {}: Semaphore closed", session_id);
                        }
                        Err(_) => {
                            tracing::error!("Session {} timed out (60s) waiting for permit slot. Execution skipped.", session_id);
                            let _ = bus.publish(SystemEvent::SystemError {
                                message: format!("Timeout (60s) waiting for execution slot in session {}", session_id),
                            });
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
        tracing::info!(
            "Gateway loaded configuration: provider={}, model={}",
            config.model_provider,
            config.model_name
        );

        // 2. MessageBus および LaneRegistry の初期化
        let bus = Arc::new(MessageBus::new());
        let registry = Arc::new(LaneRegistry::new(config.clone(), self.workspace_path.clone(), bus.clone()));

        // 3. DiscordConnector コネクタの起動
        // トークンは config.discord_token を優先、次に環境変数 DISCORD_TOKEN、なければ dummy
        let discord_token = config.discord_token.clone()
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
                priority: Priority::Normal, // 通常対話メッセージは Normal 枠
            });
        }));

        discord.set_respond_in_channels(config.discord_respond_in_channels.clone());
        discord.start().await.context("Failed to start DiscordConnector")?;
        let discord_client = Arc::new(discord);

        // 4. MessageBus イベント監視ループ (MessageBus -> LaneRegistry)
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

        // 5. Agent応答の配信監視ループ (LaneRegistry / MessageBus -> Discord)
        let mut rx_responses = bus.subscribe();
        let discord_sender = discord_client.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx_responses.recv().await {
                if let SystemEvent::AgentResponse { channel_id, content, .. } = event {
                    if let Err(e) = discord_sender.send_message(&channel_id, &content).await {
                        tracing::error!("Failed to send agent response to channel {}: {:#}", channel_id, e);
                    }
                }
            }
        });

        // 6. 各種バックグラウンドサービスの初期化・起動
        
        // ① Systemd Watchdog の起動
        watchdog::WatchdogService::start();

        // ② CronService (Heartbeat, DailySummary) の起動
        let db_path = self.workspace_path.join("memory.db");
        let cron_svc = cron::CronService::new(bus.clone(), db_path);
        cron_svc.start();

        // ③ HealthServer (HTTPサーバー) の起動
        let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel::<()>(1);
        let health_server = health::HealthServer::new(8080, reload_tx, bus.clone(), self.workspace_path.clone());
        health_server.start().await.context("Failed to start HealthServer")?;

        // 7. シグナルおよび HTTP リロードハンドリングループ
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
                            tracing::info!(
                                "Configuration reloaded successfully: provider={}, model={}",
                                new_config.model_provider,
                                new_config.model_name
                            );
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
                            tracing::info!(
                                "Configuration reloaded successfully via HTTP: provider={}, model={}",
                                new_config.model_provider,
                                new_config.model_name
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to reload configuration via HTTP: {:#}. Using previous configuration.", e);
                        }
                    }
                }
                // SIGINT: Graceful Shutdown
                _ = sig_int.recv() => {
                    tracing::info!("Received SIGINT. Initiating graceful shutdown...");
                    discord_client.shutdown().await;
                    break;
                }
                // SIGTERM: Graceful Shutdown
                _ = sig_term.recv() => {
                    tracing::info!("Received SIGTERM. Initiating graceful shutdown...");
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
        let config = Config {
            model_provider: "gmn".to_string(),
            model_name: "dummy".to_string(),
            api_key: "dummy".to_string(),
            api_base_url: "dummy".to_string(),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            timezone: None,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
        };

        let registry = LaneRegistry::new(config, ws_dir.path().to_path_buf(), bus.clone());

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
