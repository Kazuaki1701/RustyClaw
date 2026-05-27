use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use chrono::{Local, DateTime, Utc, Timelike};
use rustyclaw_config::Config;
use rustyclaw_providers::Message;
use rustyclaw_storage::{DbManager, SessionLogger};
use crate::{MessageBus, SystemEvent};

pub struct HeartbeatService {
    config: Config,
    workspace_path: PathBuf,
    bus: std::sync::Arc<MessageBus>,
}

impl HeartbeatService {
    pub fn new(config: Config, workspace_path: PathBuf, bus: std::sync::Arc<MessageBus>) -> Self {
        Self { config, workspace_path, bus }
    }

    /// Heartbeat pre-run: heartbeat-digest.md の自動生成
    pub fn generate_digest(&self, db: &DbManager) -> Result<String> {
        let now_dt = Local::now();
        let last_patrol_time_str = db.get_last_patrol_run("heartbeat_patrol")?.unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
        let last_patrol_dt = DateTime::parse_from_rfc3339(&last_patrol_time_str)
            .map(|dt| dt.with_timezone(&Local))
            .unwrap_or_else(|_| now_dt - chrono::Duration::hours(24));

        // 実行回数カウンタの取得・更新 (6回に1回 Deep Scan)
        let run_count_str = db.get_last_patrol_run("heartbeat_run_count")?.unwrap_or_else(|| "0".to_string());
        let run_count = run_count_str.parse::<usize>().unwrap_or(0) + 1;
        let _ = db.set_state_value("heartbeat_run_count", &run_count.to_string());

        let is_deep_scan = run_count % 6 == 0;
        tracing::info!("HeartbeatService: Generating digest. Run count: {} (Deep Scan: {})", run_count, is_deep_scan);

        let sessions_dir = self.workspace_path.join("sessions");
        let _ = fs::create_dir_all(&sessions_dir);

        let mut entries = Vec::new();

        // sessions/*.jsonl をスキャン
        if let Ok(dir_entries) = fs::read_dir(&sessions_dir) {
            for entry in dir_entries.flatten() {
                let path = entry.path();
                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                
                if !filename.ends_with(".jsonl") || filename.starts_with("cron") {
                    continue; // 自己参照 (cron-*) 除外
                }

                // ファイル変更時刻チェック
                let metadata = fs::metadata(&path)?;
                let modified: DateTime<Local> = metadata.modified()?.into();

                // incremental vs deep scan の判定条件
                let should_scan = if is_deep_scan {
                    now_dt.signed_duration_since(modified).num_hours() < 24
                } else {
                    modified > last_patrol_dt
                };

                if should_scan {
                    entries.push((modified, path, filename));
                }
            }
        }

        // 時間順（昇順）にソート
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut digest_lines = Vec::new();

        for (_mod_time, path, filename) in entries {
            let session_id = filename.trim_end_matches(".jsonl").to_string();
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            let mut messages = Vec::new();

            for line_res in reader.lines().flatten() {
                if let Ok(msg) = serde_json::from_str::<Message>(&line_res) {
                    messages.push(msg);
                }
            }

            // 最後の対話ペア（User -> Assistant）を取り出す
            let mut i = 0;
            while i < messages.len() {
                if messages[i].role == "user" {
                    if i + 1 < messages.len() && messages[i + 1].role == "assistant" {
                        let prompt = &messages[i].content;
                        let response = &messages[i + 1].content;
                        
                        // 1行に収めるため改行と空白を整理し、最大100文字にクリップ
                        let clean_prompt = prompt.replace('\n', " ").chars().take(100).collect::<String>();
                        let clean_response = response.replace('\n', " ").chars().take(100).collect::<String>();

                        digest_lines.push(format!(
                            "[--:--] {}: {} → {}",
                            session_id, clean_prompt, clean_response
                        ));
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }

        // 最新エントリを優先して最大3000文字に制限
        let mut final_digest = String::new();
        for line in digest_lines.iter().rev() {
            if final_digest.len() + line.len() + 1 > 3000 {
                break;
            }
            final_digest = format!("{}\n{}", line, final_digest);
        }

        let final_digest = final_digest.trim().to_string();
        
        // heartbeat-digest.md への原子性書き込み
        let digest_path = self.workspace_path.join("memory").join("heartbeat-digest.md");
        let _ = fs::create_dir_all(digest_path.parent().unwrap());
        let _ = rustyclaw_storage::atomic_write(&digest_path, final_digest.as_bytes());

        Ok(final_digest)
    }

    /// Heartbeat Step 5: Daytime & 8h 無通信声掛け判定
    pub fn is_step5_allowed(&self, db: &DbManager) -> Result<bool> {
        let now_dt = Local::now();
        let hour = now_dt.hour();
        
        // 1. Quiet Hours (23:00 〜 08:00) の除外
        if hour >= 23 || hour < 8 {
            tracing::info!("HeartbeatService: Quiet hours (23:00-08:00) active. Step 5 Vocal Greeting skipped.");
            return Ok(false);
        }

        // 2. 8時間無通信判定
        let last_user_contact_str = db.get_last_patrol_run("lastUserContact")?
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
        
        let last_user_contact_dt = DateTime::parse_from_rfc3339(&last_user_contact_str)
            .map(|dt| dt.with_timezone(&Local))
            .unwrap_or_else(|_| now_dt - chrono::Duration::hours(24));

        let elapsed_hours = now_dt.signed_duration_since(last_user_contact_dt).num_hours();
        
        if elapsed_hours >= 8 {
            tracing::info!("HeartbeatService: More than 8 hours since last user contact ({}h elapsed). Step 5 vocal greeting allowed.", elapsed_hours);
            Ok(true)
        } else {
            tracing::info!("HeartbeatService: Less than 8 hours since last user contact ({}h elapsed). Step 5 vocal greeting skipped.", elapsed_hours);
            Ok(false)
        }
    }

    /// Heartbeat 応答処理: HEARTBEAT_OK の有無による配信制御
    pub fn process_heartbeat_response(&self, response_content: &str, db: &DbManager) -> Result<()> {
        let is_silent = response_content.to_lowercase().contains("heartbeat_ok");

        if is_silent {
            // Mute / Silent (Nothing / Informational)
            tracing::info!("HeartbeatService: HEARTBEAT_OK detected. Silent mode active. Response is logged to memory/logs/ instead of channel.");
            
            // Obsidian日次ログ memory/logs/YYYY-MM-DD.md に静かに追記 (fail-open)
            let today = Local::now().format("%Y-%m-%d").to_string();
            let logs_dir = self.workspace_path.join("memory").join("logs");
            let _ = fs::create_dir_all(&logs_dir);
            let log_file = logs_dir.join(format!("{}.md", today));

            let log_entry = format!(
                "- [{}] Heartbeat Patrol completed silently:\n  {}\n",
                Local::now().format("%H:%M:%S"),
                response_content.replace('\n', "\n  ")
            );

            let mut file_content = String::new();
            if !log_file.exists() {
                file_content.push_str("---\ntype: daily-log\ncategory: heartbeat\n---\n# Daily Activity Log\n\n");
            } else if let Ok(c) = fs::read_to_string(&log_file) {
                file_content = c;
            }
            file_content.push_str(&log_entry);
            let _ = rustyclaw_storage::atomic_write(&log_file, file_content.as_bytes());

        } else {
            // Proactive speak / Critical
            tracing::info!("HeartbeatService: Proactive vocal speak triggered! Sending proactive message...");

            // Find last active user session to post response back to
            let sessions_dir = self.workspace_path.join("sessions");
            let mut last_active_session = None;
            let mut last_mod_time = SystemTime::UNIX_EPOCH;

            if let Ok(dir_entries) = fs::read_dir(&sessions_dir) {
                for entry in dir_entries.flatten() {
                    let path = entry.path();
                    let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if filename.ends_with(".jsonl") && !filename.starts_with("cron") {
                        if let Ok(metadata) = fs::metadata(&path) {
                            if let Ok(mod_time) = metadata.modified() {
                                if mod_time > last_mod_time {
                                    last_mod_time = mod_time;
                                    last_active_session = Some(filename.trim_end_matches(".jsonl").to_string());
                                }
                            }
                        }
                    }
                }
            }

            let target_session_id = last_active_session.unwrap_or_else(|| "discord-Cdefault-general".to_string());
            let channel_id = target_session_id.split('-').nth(1)
                .map(|s| s.trim_start_matches('C').to_string())
                .unwrap_or_else(|| "default".to_string());

            // 1. Proactive Post 注入 (ユーザーの会話履歴ファイルへ記録)
            let logger = SessionLogger::new(&self.workspace_path);
            let assistant_msg = Message {
                role: "assistant".to_string(),
                content: response_content.to_string(),
                name: None,
            };
            let _ = logger.append_message(&target_session_id, &assistant_msg);

            // 2. Discord/Telegram コネクタへの配信 (MessageBus経由)
            let event = SystemEvent::AgentResponse {
                session_id: target_session_id,
                channel_id,
                content: response_content.to_string(),
            };
            let _ = self.bus.publish(event);
        }

        // SQLiteの heartbeat_patrol 時刻を更新
        let _ = db.update_patrol_state("heartbeat_patrol");
        
        // heartbeat-state.json に状態を書き出し (状態管理の二重化)
        let state_path = self.workspace_path.join("memory").join("heartbeat-state.json");
        let last_patrol = Local::now().to_rfc3339();
        
        #[derive(serde::Serialize)]
        struct HeartbeatState {
            lastChecks: LastChecks,
        }

        #[derive(serde::Serialize)]
        struct LastChecks {
            activityReview: String,
            memoryMaintenance: String,
            calendar: String,
            email: String,
            weather: String,
            lastUserContact: String,
        }

        let state_val = HeartbeatState {
            lastChecks: LastChecks {
                activityReview: last_patrol.clone(),
                memoryMaintenance: last_patrol.clone(),
                calendar: last_patrol.clone(),
                email: last_patrol.clone(),
                weather: last_patrol.clone(),
                lastUserContact: db.get_last_patrol_run("lastUserContact")?.unwrap_or_else(|| last_patrol.clone()),
            }
        };

        if let Ok(serialized) = serde_json::to_string_pretty(&state_val) {
            let _ = rustyclaw_storage::atomic_write(&state_path, serialized.as_bytes());
        }

        Ok(())
    }
}
