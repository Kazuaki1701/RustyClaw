use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use serde::Deserialize;
use crate::{MessageBus, SystemEvent, Priority};

/// sessions/ ディレクトリ内でサマリーが必要な最初の1セッションを返す（1tick につき最大1件）。
pub(crate) fn find_next_session_needing_summary(sessions_dir: &std::path::Path, ws_path: &std::path::Path) -> Option<String> {
    let now = std::time::SystemTime::now();
    let entries = std::fs::read_dir(sessions_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let filename = path.file_name()?.to_string_lossy().to_string();
        // cron セッションと http-dashboard（Dashboard /chat）は除外
        if !filename.ends_with(".jsonl") || filename.starts_with("cron") || filename.starts_with("http-") {
            continue;
        }
        let safe_session_id = filename.trim_end_matches(".jsonl").to_string();
        let metadata = std::fs::metadata(&path).ok()?;
        let modified = metadata.modified().ok()?;
        let elapsed = now.duration_since(modified).unwrap_or_default().as_secs();
        if elapsed < 300 {
            continue;
        }
        let local_modified: chrono::DateTime<chrono::Local> = modified.into();
        let session_date = local_modified.format("%Y-%m-%d").to_string();
        let summary_path = ws_path.join("memory").join("summaries").join(format!("{}-{}.md", session_date, safe_session_id));
        let needs_summary = if !summary_path.exists() {
            true
        } else {
            summary_path.metadata().ok()
                .and_then(|m| m.modified().ok())
                .map(|sm| sm < modified)
                .unwrap_or(false)
        };
        if needs_summary {
            return Some(safe_session_id);
        }
    }
    None
}

#[derive(Debug, Clone, Deserialize)]
struct Trigger {
    #[serde(rename = "type")]
    trigger_type: String,
    expression: Option<String>,
    minutes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Job {
    id: String,
    name: String,
    enabled: bool,
    trigger: Trigger,
    prompt: String,
    channel_id: String,
}

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

        // 1. Heartbeat patrol loop (every 30 minutes)
        tokio::spawn(async move {
            tracing::info!("CronService: Starting 30-minute Heartbeat scheduler...");
            let mut interval = time::interval(Duration::from_secs(1800)); // 30 minutes = 1800s
            
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
            // 起動直後の同時発火を避けるため 90s 遅延してから開始
            let mut interval = time::interval_at(
                tokio::time::Instant::now() + Duration::from_secs(90),
                Duration::from_secs(3600),
            );

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
            // 起動直後の同時発火を避けるため 30s 遅延してから開始
            let mut interval = time::interval_at(
                tokio::time::Instant::now() + Duration::from_secs(30),
                Duration::from_secs(60),
            );
            
            loop {
                interval.tick().await;
                
                let sessions_dir = ws_path.join("sessions");
                if !sessions_dir.exists() {
                    continue;
                }
                
                // C: グローバルレート制限中はスキップ（無駄なイベント発火を抑制）
                if let Some(remaining) = rustyclaw_providers::global_cooldown_remaining() {
                    tracing::debug!("CronService: Session Summary skipped — global rate limit active ({:.0}s remaining)", remaining.as_secs_f64());
                    continue;
                }

                // 1tick につき最大1セッションのみ発火（CF rate limit 連続突破を防ぐ）
                if let Some(safe_session_id) = find_next_session_needing_summary(&sessions_dir, &ws_path) {
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
        });

        // 4. Dynamic cron.json loader loop (every 60 seconds)
        let bus_dynamic = self.bus.clone();
        let db_path_dynamic = self.db_path.clone();
        let ws_path_dynamic = self.db_path.parent().unwrap().to_path_buf();

        tokio::spawn(async move {
            tracing::info!("CronService: Starting 60-second Dynamic Cron settings loader...");
            // 起動直後の同時発火を避けるため 60s 遅延してから開始
            let mut interval = time::interval_at(
                tokio::time::Instant::now() + Duration::from_secs(60),
                Duration::from_secs(60),
            );
            
            loop {
                interval.tick().await;
                
                let cron_json_path = ws_path_dynamic.join("cron.json");
                if !cron_json_path.exists() {
                    continue;
                }

                let content = match std::fs::read_to_string(&cron_json_path) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("CronService: Failed to read cron.json: {:#}", e);
                        continue;
                    }
                };

                let jobs: Vec<Job> = match serde_json::from_str(&content) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!("CronService: Failed to parse cron.json: {:#}", e);
                        continue;
                    }
                };

                let db = match rustyclaw_storage::DbManager::new(&db_path_dynamic) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("CronService: Failed to open db in dynamic cron loop: {:#}", e);
                        continue;
                    }
                };

                for job in jobs {
                    if !job.enabled {
                        continue;
                    }

                    match job.trigger.trigger_type.as_str() {
                        "cron" => {
                            if let Some(expr) = &job.trigger.expression {
                                let now_time = chrono::Local::now().format("%H:%M").to_string();
                                if now_time == *expr {
                                    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                                    let state_key = format!("cron_run_date:{}", job.id);
                                    
                                    let last_run = db.get_last_patrol_run(&state_key).unwrap_or(None);
                                    if last_run.is_none() || last_run.unwrap() != today {
                                        if let Err(e) = db.set_state_value(&state_key, &today) {
                                            tracing::error!("CronService: Failed to set cron date state for {}: {:#}", job.id, e);
                                        }

                                        tracing::info!("CronService: Triggering dynamic cron job ({})...", job.name);
                                        let event = SystemEvent::IncomingMessage {
                                            session_id: format!("cron:{}", job.id),
                                            user_id: "cron".to_string(),
                                            channel_id: job.channel_id.clone(),
                                            content: job.prompt.clone(),
                                            priority: Priority::Background,
                                        };
                                        if let Err(e) = bus_dynamic.publish(event) {
                                            tracing::error!("CronService: Failed to publish dynamic cron event for {}: {:#}", job.id, e);
                                        }
                                    }
                                }
                            }
                        }
                        "interval" => {
                            if let Some(minutes) = job.trigger.minutes {
                                let state_key = format!("cron_last_run:{}", job.id);
                                let now_sec = chrono::Local::now().timestamp();
                                
                                let last_run_str = db.get_last_patrol_run(&state_key).unwrap_or(None);
                                let mut should_run = false;
                                
                                if let Some(last_str) = last_run_str {
                                    if let Ok(last_sec) = last_str.parse::<i64>() {
                                        let elapsed_min = (now_sec - last_sec) / 60;
                                        if elapsed_min >= minutes as i64 {
                                            should_run = true;
                                        }
                                    } else {
                                        should_run = true;
                                    }
                                } else {
                                    should_run = true;
                                }

                                if should_run {
                                    if let Err(e) = db.set_state_value(&state_key, &now_sec.to_string()) {
                                        tracing::error!("CronService: Failed to set cron last run time for {}: {:#}", job.id, e);
                                    }

                                    tracing::info!("CronService: Triggering dynamic interval job ({})...", job.name);
                                    let event = SystemEvent::IncomingMessage {
                                        session_id: format!("cron:{}", job.id),
                                        user_id: "cron".to_string(),
                                        channel_id: job.channel_id.clone(),
                                        content: job.prompt.clone(),
                                        priority: Priority::Background,
                                    };
                                    if let Err(e) = bus_dynamic.publish(event) {
                                        tracing::error!("CronService: Failed to publish dynamic interval event for {}: {:#}", job.id, e);
                                    }
                                }
                            }
                        }
                        other => {
                            tracing::warn!("CronService: Unknown trigger type {} for job {}", other, job.id);
                        }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_old_jsonl(dir: &std::path::Path, name: &str) {
        let path = dir.join(format!("{}.jsonl", name));
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"role":"user","content":"hi"}}"#).unwrap();
        // mtime を 10 分前に設定
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let ft = filetime::FileTime::from_system_time(old);
        filetime::set_file_mtime(&path, ft).unwrap();
    }

    #[test]
    fn test_find_next_session_returns_at_most_one() {
        let ws = tempfile::tempdir().unwrap();
        let sessions_dir = ws.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::create_dir_all(ws.path().join("memory").join("summaries")).unwrap();

        // 複数のアイドルセッションを作成
        make_old_jsonl(&sessions_dir, "discord-A");
        make_old_jsonl(&sessions_dir, "discord-B");
        make_old_jsonl(&sessions_dir, "discord-C");

        let result = find_next_session_needing_summary(&sessions_dir, ws.path());
        assert!(result.is_some(), "should find at least one session");

        // 2回呼んでも同じセッションか別の1件が返る（複数まとめて返さない）
        let result2 = find_next_session_needing_summary(&sessions_dir, ws.path());
        assert!(result2.is_some());
        // 1回の呼び出しで返るのは必ず1件
        assert_eq!(result.unwrap().len() > 0, true);
    }

    #[test]
    fn test_cron_sessions_are_excluded() {
        let ws = tempfile::tempdir().unwrap();
        let sessions_dir = ws.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::create_dir_all(ws.path().join("memory").join("summaries")).unwrap();

        // cron セッションと http-dashboard は除外される
        make_old_jsonl(&sessions_dir, "cron-heartbeat");
        make_old_jsonl(&sessions_dir, "cron-daily-summary");
        make_old_jsonl(&sessions_dir, "http-dashboard");

        let result = find_next_session_needing_summary(&sessions_dir, ws.path());
        assert!(result.is_none(), "cron and http- sessions must be excluded");
    }

    #[test]
    fn test_recent_sessions_are_excluded() {
        let ws = tempfile::tempdir().unwrap();
        let sessions_dir = ws.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::create_dir_all(ws.path().join("memory").join("summaries")).unwrap();

        // 直近（アイドル5分未満）のセッションは対象外
        let path = sessions_dir.join("discord-recent.jsonl");
        std::fs::write(&path, r#"{"role":"user","content":"hi"}"#).unwrap();

        let result = find_next_session_needing_summary(&sessions_dir, ws.path());
        assert!(result.is_none(), "recent sessions (< 5 min idle) must be excluded");
    }
}
