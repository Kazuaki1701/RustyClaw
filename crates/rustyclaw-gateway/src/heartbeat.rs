use anyhow::Result;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::SystemTime;
use chrono::{Local, DateTime, Timelike};
use rustyclaw_config::Config;
use rustyclaw_providers::Message;
use rustyclaw_storage::{DbManager, SessionLogger};
use rustyclaw_tools::Tool;
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
        let home_channel_id = config.channels.discord.as_ref()
            .and_then(|d| d.home_channel_id.clone());
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
        
        // 前回の実行時刻を取得
        let last_run_at_str = db.get_last_patrol_run("heartbeat_last_run_ts")?;
        let last_run_at: Option<DateTime<Local>> = last_run_at_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Local))
        });

        // 状態保存: 今回の実行時刻
        let _ = db.set_state_value("heartbeat_last_run_ts", &now_dt.to_rfc3339());

        tracing::info!("HeartbeatService: Generating digest. Run count: {} (Deep Scan: {}, Last Run: {:?})", run_count, is_deep_scan, last_run_at);

        let sessions_dir = self.workspace_path.join("sessions");
        let _ = fs::create_dir_all(&sessions_dir);

        let digest_path = self.workspace_path.join("memory").join("heartbeat-digest.md");

        // 既存のダイジェスト行をセッションIDごとにパースしてマップに格納
        let mut existing_digest_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        if !is_deep_scan && last_run_at.is_some() && digest_path.exists() {
            if let Ok(content) = fs::read_to_string(&digest_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed.starts_with('[') {
                        if let Some(close_bracket_idx) = trimmed.find("] ") {
                            let body = &trimmed[close_bracket_idx + 2..];
                            if let Some(colon_idx) = body.find(": ") {
                                let session_id = body[..colon_idx].to_string();
                                existing_digest_map.entry(session_id).or_default().push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }

        // sessions/*.jsonl をスキャンしてアクティブなセッションを収集
        let mut active_sessions = Vec::new();
        if let Ok(dir_entries) = fs::read_dir(&sessions_dir) {
            for entry in dir_entries.flatten() {
                let path = entry.path();
                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                
                if !filename.ends_with(".jsonl")
                    || filename.starts_with("cron")
                    || filename.starts_with("cli-")
                    || filename.starts_with("http-")
                {
                    continue; // cron-* / CLI テスト / HTTP ダッシュボード除外
                }

                // ファイル変更時刻チェック
                if let Ok(metadata) = fs::metadata(&path) {
                    if let Ok(modified_time) = metadata.modified() {
                        let modified: DateTime<Local> = modified_time.into();
                        // 過去24時間以内に更新されたアクティブな対話のみ対象
                        if now_dt.signed_duration_since(modified).num_hours() < 24 {
                            let session_id = filename.trim_end_matches(".jsonl").to_string();
                            active_sessions.push((modified, path, session_id));
                        }
                    }
                }
            }
        }

        let mut merged_lines: Vec<(DateTime<Local>, String)> = Vec::new();

        for (mod_time, path, session_id) in active_sessions {
            let is_modified_since_last = match last_run_at {
                Some(last_ts) => mod_time > last_ts,
                None => true,
            };

            if !is_deep_scan && !is_modified_since_last && existing_digest_map.contains_key(&session_id) {
                // 既存のダイジェスト行をそのまま利用（JSONL ファイルの再読み込みをスキップ）
                if let Some(lines) = existing_digest_map.get(&session_id) {
                    for line in lines {
                        merged_lines.push((mod_time, line.clone()));
                    }
                }
            } else {
                // 新規または更新されたセッション: ファイルを読み込んでダイジェスト行を生成
                if let Ok(file) = File::open(&path) {
                    let reader = BufReader::new(file);
                    let mut messages = Vec::new();

                    for line_res in reader.lines().flatten() {
                        if let Ok(msg) = serde_json::from_str::<Message>(&line_res) {
                            messages.push(msg);
                        }
                    }

                    // 最後の対話ペア（User -> Assistant）を取り出す
                    let mut i = 0;
                    let mut session_lines = Vec::new();
                    while i < messages.len() {
                        if messages[i].role == "user" {
                            let prompt = &messages[i].content;
                            let mut next_user_idx = messages.len();
                            for j in (i + 1)..messages.len() {
                                if messages[j].role == "user" {
                                    next_user_idx = j;
                                    break;
                                }
                            }
                            
                            let mut response = None;
                            for j in (i + 1)..next_user_idx {
                                if messages[j].role == "assistant" {
                                    if !messages[j].content.trim().is_empty() {
                                        response = Some(&messages[j].content);
                                    }
                                }
                            }

                            if let Some(resp_content) = response {
                                let clean_prompt = prompt.replace('\n', " ").chars().take(100).collect::<String>();
                                let clean_response = resp_content.replace('\n', " ").chars().take(100).collect::<String>();

                                let mut time_str = "--:--".to_string();
                                if let Some(ref ts) = messages[i].timestamp {
                                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                                        let local_dt = dt.with_timezone(&Local);
                                        time_str = local_dt.format("%H:%M").to_string();
                                    }
                                }

                                session_lines.push(format!(
                                    "[{}] {}: {} → {}",
                                    time_str, session_id, clean_prompt, clean_response
                                ));
                            }
                            i = next_user_idx;
                        } else {
                            i += 1;
                        }
                    }

                    for line in session_lines {
                        merged_lines.push((mod_time, line));
                    }
                }
            }
        }

        // 時間順（昇順）にソート
        merged_lines.sort_by(|a, b| a.0.cmp(&b.0));

        let digest_lines: Vec<String> = merged_lines.into_iter().map(|item| item.1).collect();

        let header = if is_deep_scan { "*(deep scan — 24h lookback)*\n\n" } else { "" };
        let body = if digest_lines.is_empty() {
            "*(No new activity since last heartbeat)*".to_string()
        } else {
            // 最新エントリを優先して最大3000文字に制限
            let mut final_digest = String::new();
            for line in digest_lines.iter().rev() {
                if final_digest.len() + line.len() + 1 > 3000 {
                    break;
                }
                final_digest = format!("{}\n{}", line, final_digest);
            }
            format!("{}{}", header, final_digest.trim())
        };

        let file_content = format!("# Heartbeat Digest\n\n{}\n", body);
        
        // heartbeat-digest.md への原子性書き込み
        let _ = fs::create_dir_all(digest_path.parent().unwrap());
        let _ = rustyclaw_storage::atomic_write(&digest_path, file_content.as_bytes());

        Ok(body)
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

            // proactive-posts.md への追記（最新 5 件・48h 以内を保持）
            {
                let posts_path = self.workspace_path.join("memory").join("proactive-posts.md");
                let now_label = Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
                let new_entry = format!("[{}] {}", now_label, response_content.replace('\n', " "));
                let cutoff = Local::now() - chrono::Duration::hours(48);
                let mut kept: Vec<String> = Vec::new();
                if let Ok(existing) = fs::read_to_string(&posts_path) {
                    for line in existing.lines() {
                        if line.starts_with('[') {
                            if let Some(end) = line.find(']') {
                                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&line[1..end]) {
                                    if dt.with_timezone(&Local) >= cutoff {
                                        kept.push(line.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                kept.push(new_entry);
                if kept.len() > 5 {
                    kept = kept.split_off(kept.len() - 5);
                }
                let _ = fs::create_dir_all(posts_path.parent().unwrap());
                let _ = rustyclaw_storage::atomic_write(&posts_path, kept.join("\n").as_bytes());
            }

            // 1. Proactive Post 注入 (ユーザーの会話履歴ファイルへ記録)
            let logger = SessionLogger::new(&self.workspace_path);
            let assistant_msg = Message {
                role: "assistant".to_string(),
                content: response_content.to_string(),
                name: None,
                trigger: Some("proactive".to_string()),
                timestamp: Some(chrono::Local::now().to_rfc3339()),
                ..Default::default()
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

    /// Heartbeat Step 4: Weather / Rain Patrol (雨雲パトロール)
    /// 指定された現在地の緯度経度から降水量をチェックし、必要に応じて雨雲接近アラートのプロンプトを返す。
    pub async fn run_weather_patrol(&self, db_path: &std::path::Path) -> Result<Option<String>> {
        // 1. USER.md から現在地の緯度経度を抽出
        let (lat, lon) = match extract_coordinates(&self.workspace_path) {
            Some(coords) => coords,
            None => {
                tracing::info!("HeartbeatService: Coordinates not found in USER.md. Weather Patrol skipped.");
                return Ok(None);
            }
        };

        // 2. 前回の雨アラート状態を SQLite から取得 (同期ブロック)
        let (last_alert_dt, last_alert_level) = {
            let db = DbManager::new(db_path.to_str().unwrap_or_default())?;
            let now_dt = Local::now();
            let last_alert_ts_str = db.get_last_patrol_run("last_rain_alert_ts")?.unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
            let last_alert_dt = DateTime::parse_from_rfc3339(&last_alert_ts_str)
                .map(|dt| dt.with_timezone(&Local))
                .unwrap_or_else(|_| now_dt - chrono::Duration::hours(24));

            let last_alert_level = db.get_last_patrol_run("last_rain_alert_level")?
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            (last_alert_dt, last_alert_level)
        };

        tracing::info!("HeartbeatService: Weather Patrol coordinates: lat={}, lon={}", lat, lon);

        // 3. YolpWeatherTool を用いて降水量をフェッチ (await が発生、この時点では db への参照はない)
        let tool = rustyclaw_tools::YolpWeatherTool::new();
        let tool_res = tool.execute(serde_json::json!({
            "coordinates": format!("{},{}", lon, lat)
        })).await;

        if tool_res.is_error {
            tracing::error!("HeartbeatService: Weather Patrol API fetch failed: {}", tool_res.content);
            return Ok(None);
        }

        // レスポンスのパース
        let parsed_val: serde_json::Value = match serde_json::from_str(&tool_res.content) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("HeartbeatService: Weather Patrol response parse failed: {}", e);
                return Ok(None);
            }
        };

        // WeatherList の Weather 配列を抽出
        let weather_list = parsed_val["Feature"][0]["Property"]["WeatherList"]["Weather"].as_array();
        let weather_array = match weather_list {
            Some(arr) if !arr.is_empty() => arr,
            _ => {
                tracing::warn!("HeartbeatService: Weather Patrol response is empty.");
                return Ok(None);
            }
        };

        // 降水強度の判定（最大降水量の算出）
        let mut max_rainfall = 0.0;
        let mut rain_timeline = Vec::new();
        for item in weather_array {
            let rainfall = item["Rainfall"].as_f64().unwrap_or(0.0);
            let time_str = item["Date"].as_str().unwrap_or("");
            let entry_type = item["Type"].as_str().unwrap_or("");
            
            if rainfall > max_rainfall {
                max_rainfall = rainfall;
            }

            // 時刻表示をHH:MMに整形
            let formatted_time = if time_str.len() == 12 {
                format!("{}:{}", &time_str[8..10], &time_str[10..12])
            } else {
                time_str.to_string()
            };

            rain_timeline.push(format!("- {} ({}): {} mm/h", formatted_time, entry_type, rainfall));
        }

        tracing::info!("HeartbeatService: Weather Patrol completed. Max rainfall forecast: {} mm/h", max_rainfall);

        // 雨が降らない場合は無音 (通知なし)
        if max_rainfall <= 0.0 {
            return Ok(None);
        }

        let now_dt = Local::now();
        let hour = now_dt.hour();

        // Quiet Hours (23:00 〜 08:00) の処理
        let is_quiet_hours = hour >= 23 || hour < 8;
        if is_quiet_hours && max_rainfall < 5.0 {
            // 豪雨（5.0mm/h以上）以外は Quiet Hours 中は Discord 通知をスキップ。
            // ただし findings.md に静かに記録する。
            let findings_log = format!(
                "\n- [{}] [Rain Patrol] Rain predicted (Max: {} mm/h) (skipped due to quiet hours)\n",
                now_dt.format("%H:%M"), max_rainfall
            );
            let findings_path = self.workspace_path.join("patrol").join("findings.md");
            if let Ok(mut current) = std::fs::read_to_string(&findings_path) {
                current.push_str(&findings_log);
                let _ = rustyclaw_storage::atomic_write(&findings_path, current.as_bytes());
            }
            tracing::info!("HeartbeatService: Rain predicted but skipped due to Quiet Hours ({} mm/h). Recorded to findings.md.", max_rainfall);
            return Ok(None);
        }

        let elapsed_mins = now_dt.signed_duration_since(last_alert_dt).num_minutes();
        let is_level_escalated = max_rainfall >= 1.0 && last_alert_level < 1.0; // 小雨から本降りの悪化

        if elapsed_mins < 180 && !is_level_escalated {
            // 3時間以内で、かつ著しい状況悪化でもない場合はサイレントスキップ
            tracing::info!("HeartbeatService: Rain predicted but skipped due to 3-hour duplicate guard (last alert: {} mins ago, last level: {} mm/h, current max: {} mm/h)", elapsed_mins, last_alert_level, max_rainfall);
            return Ok(None);
        }

        // 4. 警報を送信するため、状態を SQLite に保存 (同期ブロック)
        {
            let db = DbManager::new(db_path.to_str().unwrap_or_default())?;
            let _ = db.set_state_value("last_rain_alert_ts", &now_dt.to_rfc3339());
            let _ = db.set_state_value("last_rain_alert_level", &max_rainfall.to_string());
        }

        // findings.md に警報発令の旨を記録
        let findings_log = format!(
            "\n- [{}] [Rain Patrol] Rain alert triggered (Max: {} mm/h)\n",
            now_dt.format("%H:%M"), max_rainfall
        );
        let findings_path = self.workspace_path.join("patrol").join("findings.md");
        if let Ok(mut current) = std::fs::read_to_string(&findings_path) {
            current.push_str(&findings_log);
            let _ = rustyclaw_storage::atomic_write(&findings_path, current.as_bytes());
        }

        // プロンプト注入用警告テキストの生成
        let alert_prompt = format!(
            "⚠️ [RAIN PATROL ALERT] Rain is expected within the next hour at the user's location.\n\
             Max Rainfall Forecast: {} mm/h\n\
             Precipitation Timeline:\n{}\n\n\
             Action required: Since rain is approaching, do NOT output 'HEARTBEAT_OK'. Instead, proactively notify the user about the incoming rain (such as advising to carry an umbrella or take inside laundry) with a friendly, soft, personal secretary tone in Japanese. Keep the alert extremely concise and polite.",
            max_rainfall, rain_timeline.join("\n")
        );

        Ok(Some(alert_prompt))
    }
}

fn extract_coordinates(workspace_path: &std::path::Path) -> Option<(f64, f64)> {
    let user_md_path = workspace_path.join("USER.md");
    if let Ok(content) = std::fs::read_to_string(&user_md_path) {
        for line in content.lines() {
            if line.contains("Coordinates:") && !line.contains("Commute") {
                if let Some(pos) = line.find("Coordinates:") {
                    let s = &line[pos + "Coordinates:".len()..];
                    let end_pos = s.find('(').unwrap_or(s.len());
                    let clean_s = &s[..end_pos].trim();
                    let parts: Vec<&str> = clean_s.split(',').collect();
                    if parts.len() == 2 {
                        if let (Ok(lat), Ok(lon)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
                            return Some((lat, lon));
                        }
                    }
                }
            }
        }
    }
    None
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
            timestamp: Some("2026-05-31T10:15:30+09:00".to_string()),
            name: None,
            ..Default::default()
        };
        let msg_assistant = Message {
            role: "assistant".to_string(),
            content: "It is sunny and 22 degrees.".to_string(),
            timestamp: Some("2026-05-31T10:16:00+09:00".to_string()),
            name: None,
            ..Default::default()
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
        let config = Config::default();
        
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
        assert!(
            digest_1.contains("[10:15] telegram-U123-20260528"),
            "First digest should contain formatted local timestamp [10:15]"
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

    #[test]
    fn test_generate_digest_handles_tool_use() -> Result<()> {
        let ws_dir = tempdir()?;
        let ws_path = ws_dir.path().to_path_buf();
        let sessions_dir = ws_path.join("sessions");
        fs::create_dir_all(&sessions_dir)?;
        
        let session_file_path = sessions_dir.join("telegram-U456-20260528.jsonl");
        
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: "Find Tokyo coords".to_string(),
                timestamp: Some("2026-05-31T14:00:00+09:00".to_string()),
                ..Default::default()
            },
            Message {
                role: "assistant".to_string(),
                content: "".to_string(),
                tool_calls: Some(vec![]),
                timestamp: Some("2026-05-31T14:00:05+09:00".to_string()),
                ..Default::default()
            },
            Message {
                role: "tool".to_string(),
                content: "35.6762, 139.6503".to_string(),
                timestamp: Some("2026-05-31T14:00:10+09:00".to_string()),
                ..Default::default()
            },
            Message {
                role: "assistant".to_string(),
                content: "Coords are 35.6762 N.".to_string(),
                timestamp: Some("2026-05-31T14:00:15+09:00".to_string()),
                ..Default::default()
            },
        ];
        
        let mut file_content = String::new();
        for msg in &messages {
            file_content.push_str(&serde_json::to_string(msg)?);
            file_content.push('\n');
        }
        fs::write(&session_file_path, file_content)?;

        let db_path = ws_path.join("test.db");
        let db = DbManager::new(db_path.to_str().unwrap())?;
        let bus = std::sync::Arc::new(MessageBus::new());
        let config = Config::default();
        let service = HeartbeatService::new(config, ws_path.clone(), bus);

        let digest = service.generate_digest(&db)?;
        
        assert!(digest.contains("Find Tokyo coords"));
        assert!(digest.contains("Coords are 35.6762 N."));
        assert!(!digest.contains("35.6762, 139.6503"));
        assert!(digest.contains("[14:00]"));
        
        let digest_file = ws_path.join("memory").join("heartbeat-digest.md");
        assert!(digest_file.exists());
        let file_data = fs::read_to_string(&digest_file)?;
        assert!(file_data.starts_with("# Heartbeat Digest\n\n"));
        
        Ok(())
    }

    #[test]
    fn test_proactive_post_writes_to_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ws = dir.path();
        let memory_dir = ws.join("memory");
        std::fs::create_dir_all(ws.join("sessions"))?;
        std::fs::create_dir_all(&memory_dir)?;

        let db_path = memory_dir.join("memory.db");
        let db = rustyclaw_storage::DbManager::new(&db_path)?;
        let bus = std::sync::Arc::new(crate::MessageBus::new());
        let config = rustyclaw_config::Config::default();
        let svc = HeartbeatService::new(config, ws.to_path_buf(), bus);

        // proactive（非 HEARTBEAT_OK）応答を処理
        svc.process_heartbeat_response("雨が近づいています。傘をお持ちください。", &db)?;

        let posts_file = memory_dir.join("proactive-posts.md");
        assert!(posts_file.exists(), "proactive-posts.md が作成されるべき");
        let content = std::fs::read_to_string(&posts_file)?;
        assert!(content.contains("雨が近づいています"), "投稿内容が記録されるべき");
        Ok(())
    }

    #[test]
    fn test_generate_digest_excludes_cli_and_http_sessions() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ws = dir.path();
        let sessions_dir = ws.join("sessions");
        let memory_dir = ws.join("memory");
        std::fs::create_dir_all(&sessions_dir)?;
        std::fs::create_dir_all(&memory_dir)?;

        // CLI テストセッションと HTTP ダッシュボードセッションを作成
        std::fs::write(
            sessions_dir.join("cli-session.jsonl"),
            "{\"role\":\"user\",\"content\":\"test\"}\n{\"role\":\"assistant\",\"content\":\"[NO-AGENT] Debug mode\"}\n",
        )?;
        std::fs::write(
            sessions_dir.join("http-dashboard.jsonl"),
            "{\"role\":\"user\",\"content\":\"ping\"}\n{\"role\":\"assistant\",\"content\":\"pong\"}\n",
        )?;
        // 正規の Discord セッション（24h 以内の mtime にするため直接書いて OK）
        std::fs::write(
            sessions_dir.join("discord-C123-20260530.jsonl"),
            "{\"role\":\"user\",\"content\":\"こんにちは\"}\n{\"role\":\"assistant\",\"content\":\"やあ！\"}\n",
        )?;

        let db_path = memory_dir.join("memory.db");
        let db = rustyclaw_storage::DbManager::new(&db_path)?;
        let bus = std::sync::Arc::new(crate::MessageBus::new());
        let config = rustyclaw_config::Config::default();
        let svc = HeartbeatService::new(config, ws.to_path_buf(), bus);

        let digest = svc.generate_digest(&db)?;

        assert!(!digest.contains("NO-AGENT"), "cli-session エントリが除外されるべき");
        assert!(!digest.contains("ping"), "http-dashboard エントリが除外されるべき");
        Ok(())
    }
}

