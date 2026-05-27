use anyhow::Result;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::SystemTime;
use chrono::{Local, DateTime, Timelike};
use rustyclaw_config::Config;
use rustyclaw_providers::Message;
use rustyclaw_storage::{DbManager, SessionLogger};
use crate::{MessageBus, SystemEvent};

pub struct HeartbeatService {
    _config: Config,
    workspace_path: PathBuf,
    bus: std::sync::Arc<MessageBus>,
    /// proactive 通知の送信先チャンネル ID。None の場合はセッション検索にフォールバック。
    home_channel_id: Option<String>,
}

impl HeartbeatService {
    pub fn new(config: Config, workspace_path: PathBuf, bus: std::sync::Arc<MessageBus>) -> Self {
        let home_channel_id = config.discord_home_channel_id.clone();
        Self { _config: config, workspace_path, bus, home_channel_id }
    }

    /// Heartbeat pre-run: heartbeat-digest.md の自動生成
    pub fn generate_digest(&self, db: &DbManager) -> Result<String> {
        let now_dt = Local::now();


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

                // Always capture sessions active within the last 24 hours:
                let should_scan = now_dt.signed_duration_since(modified).num_hours() < 24;

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

            // home_channel_id が設定されていればそれを優先、なければ旧来のセッション検索
            let (target_session_id, channel_id) = if let Some(ref ch_id) = self.home_channel_id {
                let today = chrono::Local::now().format("%Y%m%d").to_string();
                let session_id = format!("discord-C{}-{}", ch_id, today);
                (session_id, ch_id.clone())
            } else {
                // フォールバック: 最後にアクティブだったセッションを探す
                let sessions_dir = self.workspace_path.join("sessions");
                let mut last_active_session: Option<String> = None;
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
                let sid = last_active_session.unwrap_or_else(|| "discord-Cunknown-00000000".to_string());
                let ch = sid.split('-').nth(1)
                    .map(|s| s.trim_start_matches('C').to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                (sid, ch)
            };

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
        #[allow(non_snake_case)]
        struct HeartbeatState {
            lastChecks: LastChecks,
        }

        #[derive(serde::Serialize)]
        #[allow(non_snake_case)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_digest_persists_recent_dialogue() -> Result<()> {
        let ws_dir = tempdir()?;
        let ws_path = ws_dir.path().to_path_buf();

        // 1. Create a dummy session log telegram-U123-20260528.jsonl with a user-assistant pair.
        let sessions_dir = ws_path.join("sessions");
        fs::create_dir_all(&sessions_dir)?;
        
        let session_file_path = sessions_dir.join("telegram-U123-20260528.jsonl");
        
        let msg_user = Message {
            role: "user".to_string(),
            content: "What is the weather today?".to_string(),
            name: None,
        };
        let msg_assistant = Message {
            role: "assistant".to_string(),
            content: "It is sunny and 22 degrees.".to_string(),
            name: None,
        };
        
        fs::write(
            &session_file_path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&msg_user)?,
                serde_json::to_string(&msg_assistant)?
            ),
        )?;

        // 2. Setup DbManager and HeartbeatService
        let db_path = ws_path.join("test.db");
        let db = DbManager::new(db_path.to_str().unwrap())?;
        
        let bus = std::sync::Arc::new(MessageBus::new());
        let config = Config {
            model_provider: "gmn".to_string(),
            model_name: "dummy".to_string(),
            api_key: "dummy".to_string(),
            api_base_url: "dummy".to_string(),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
        };
        
        let service = HeartbeatService::new(config, ws_path.clone(), bus);

        // 3. Call generate_digest() multiple times sequentially and verify the digest contains the conversation.
        // First run
        let digest_1 = service.generate_digest(&db)?;
        assert!(
            digest_1.contains("What is the weather today?"),
            "First digest should contain prompt"
        );
        assert!(
            digest_1.contains("It is sunny and 22 degrees."),
            "First digest should contain response"
        );

        // Update patrol state as if it just finished
        db.update_patrol_state("heartbeat_patrol")?;

        // Second run - should still contain the conversation because it's within the 24 hour window
        let digest_2 = service.generate_digest(&db)?;
        assert!(
            digest_2.contains("What is the weather today?"),
            "Second digest should contain prompt"
        );
        assert!(
            digest_2.contains("It is sunny and 22 degrees."),
            "Second digest should contain response"
        );

        Ok(())
    }
}

