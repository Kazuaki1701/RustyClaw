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
    }
}
