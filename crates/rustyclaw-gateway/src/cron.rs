use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use crate::{MessageBus, SystemEvent, Priority};

pub struct CronService {
    bus: Arc<MessageBus>,
    db_path: std::path::PathBuf,
}

impl CronService {
    pub fn new(bus: Arc<MessageBus>, db_path: std::path::PathBuf) -> Self {
        Self { bus, db_path }
    }

    pub fn start(self) {
        let bus = self.bus.clone();
        let _db_path = self.db_path.clone();

        // 1. Heartbeat patrol loop (every 10 minutes)
        tokio::spawn(async move {
            tracing::info!("CronService: Starting 10-minute Heartbeat scheduler...");
            let mut interval = time::interval(Duration::from_secs(600)); // 10 minutes = 600s
            
            // First tick fires immediately, let's skip or let it run
            interval.tick().await; 
            
            loop {
                interval.tick().await;
                tracing::info!("CronService: Triggering Heartbeat patrol event...");
                let event = SystemEvent::IncomingMessage {
                    session_id: "cron:heartbeat".to_string(),
                    user_id: "cron".to_string(),
                    channel_id: "cron".to_string(),
                    content: "heartbeat".to_string(),
                    priority: Priority::Background,
                };
                if let Err(e) = bus.publish(event) {
                    tracing::error!("CronService: Failed to publish Heartbeat event: {:#}", e);
                }
            }
        });

        // 2. Daily Summary loop (checks hourly if date changed)
        let bus_daily = self.bus.clone();
        let db_path_daily = self.db_path.clone();
        tokio::spawn(async move {
            tracing::info!("CronService: Starting Daily Summary checker...");
            let mut interval = time::interval(Duration::from_secs(3600)); // Every hour = 3600s
            
            loop {
                interval.tick().await;
                
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                
                // Read from SQLite last run date
                let last_run = match rustyclaw_storage::DbManager::new(&db_path_daily) {
                    Ok(db) => {
                        match db.get_last_patrol_run("daily_summary_date") {
                            Ok(res) => res,
                            Err(e) => {
                                tracing::error!("CronService: Failed to get daily_summary_date: {:#}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("CronService: Failed to open db in daily summary loop: {:#}", e);
                        None
                    }
                };

                if last_run.is_none() || last_run.unwrap() != today {
                    // Update state to today first to prevent double-runs
                    if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path_daily) {
                        let _ = db.set_state_value("daily_summary_date", &today);
                    }

                    // Offset triggering by 5 minutes to prevent locks/collisions with Heartbeat 10m ticks on gmn_sem
                    tracing::info!("CronService: Daily Summary date changed. Waiting 5 minutes offset before triggering...");
                    let bus_clone = bus_daily.clone();
                    let today_clone = today.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(300)).await;
                        tracing::info!("CronService: Triggering Daily Summary event for date: {}...", today_clone);
                        let event = SystemEvent::IncomingMessage {
                            session_id: "cron:daily-summary".to_string(),
                            user_id: "cron".to_string(),
                            channel_id: "cron".to_string(),
                            content: "daily-summary".to_string(),
                            priority: Priority::Background,
                        };
                        if let Err(e) = bus_clone.publish(event) {
                            tracing::error!("CronService: Failed to publish Daily Summary event: {:#}", e);
                        }
                    });
                }
            }
        });

        // 3. Session Summary loop (checks every 60 seconds for idle sessions)
        let bus_summary = self.bus.clone();
        let ws_path = self.db_path.parent().unwrap().to_path_buf(); // db_path is workspace/memory.db, parent is workspace
        tokio::spawn(async move {
            tracing::info!("CronService: Starting 60-second Session Summary scheduler...");
            let mut interval = time::interval(Duration::from_secs(60));
            
            loop {
                interval.tick().await;
                
                let sessions_dir = ws_path.join("sessions");
                if !sessions_dir.exists() {
                    continue;
                }
                
                let now = std::time::SystemTime::now();
                
                if let Ok(dir_entries) = std::fs::read_dir(&sessions_dir) {
                    for entry in dir_entries.flatten() {
                        let path = entry.path();
                        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        
                        if !filename.ends_with(".jsonl") || filename.starts_with("cron") {
                            continue; // Skip self/cron/daily-summary logs
                        }
                        
                        // Parse session ID from filename
                        let safe_session_id = filename.trim_end_matches(".jsonl").to_string();
                        
                        // Check file modification time
                        let metadata = match std::fs::metadata(&path) {
                            Ok(m) => m,
                            Err(_) => continue,
                        };
                        
                        let modified = match metadata.modified() {
                            Ok(t) => t,
                            Err(_) => continue,
                        };
                        
                        let elapsed = match now.duration_since(modified) {
                            Ok(d) => d.as_secs(),
                            Err(_) => 0,
                        };
                        
                        // Check if idle for at least 5 minutes (300 seconds)
                        if elapsed >= 300 {
                            // Determine session date
                            let local_modified: chrono::DateTime<chrono::Local> = modified.into();
                            let session_date = local_modified.format("%Y-%m-%d").to_string();
                            
                            let summary_path = ws_path.join("memory").join("summaries").join(format!("{}-{}.md", session_date, safe_session_id));
                            
                            let mut needs_summary = false;
                            if !summary_path.exists() {
                                needs_summary = true;
                            } else if let Ok(summary_meta) = std::fs::metadata(&summary_path) {
                                if let Ok(summary_modified) = summary_meta.modified() {
                                    if summary_modified < modified {
                                        needs_summary = true; // Session was updated after summary was generated
                                    }
                                }
                            }
                            
                            if needs_summary {
                                // Load history and ensure it has messages
                                let logger = rustyclaw_storage::SessionLogger::new(&ws_path);
                                if let Ok(history) = logger.load_history(&safe_session_id) {
                                    if !history.is_empty() {
                                        tracing::info!("CronService: Session {} has been idle for 5+ mins and needs summary. Triggering...", safe_session_id);
                                        let event = SystemEvent::IncomingMessage {
                                            session_id: format!("cron:session-summary:{}", safe_session_id),
                                            user_id: "cron".to_string(),
                                            channel_id: "cron".to_string(),
                                            content: "session-summary".to_string(),
                                            priority: Priority::Background,
                                        };
                                        if let Err(e) = bus_summary.publish(event) {
                                            tracing::error!("CronService: Failed to publish Session Summary event: {:#}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
