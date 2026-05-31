use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::io::BufRead;
use rustyclaw_config::get_app_dir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct HealthServer {
    addr: SocketAddr,
    reload_tx: tokio::sync::mpsc::Sender<()>,
    bus: Arc<crate::MessageBus>,
    workspace_path: PathBuf,
    gmn_sem: Arc<tokio::sync::Semaphore>,
    gmn_capacity: usize,
}

impl HealthServer {
    pub fn new(
        port: u16,
        reload_tx: tokio::sync::mpsc::Sender<()>,
        bus: Arc<crate::MessageBus>,
        workspace_path: PathBuf,
        gmn_sem: Arc<tokio::sync::Semaphore>,
        gmn_capacity: usize,
    ) -> Self {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        Self { addr, reload_tx, bus, workspace_path, gmn_sem, gmn_capacity }
    }

    pub async fn start(self) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await
            .with_context(|| format!("Failed to bind HealthServer to {}", self.addr))?;

        tracing::info!("HealthServer listening on http://{}", self.addr);
        let reload_tx = Arc::new(self.reload_tx);
        let bus = self.bus.clone();
        let workspace_path = Arc::new(self.workspace_path.clone());
        let gmn_sem_arc = self.gmn_sem.clone();
        let gmn_capacity = self.gmn_capacity;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut socket, client_addr)) => {
                        tracing::debug!("HealthServer: Accepted connection from {}", client_addr);
                        let reload_tx_clone = reload_tx.clone();
                        let bus_clone = bus.clone();
                        let workspace_path_clone = workspace_path.clone();
                        let gmn_sem_clone = gmn_sem_arc.clone();
                        let gmn_cap_clone = gmn_capacity;

                        tokio::spawn(async move {
                            let mut buffer = [0u8; 8192];
                            if let Ok(n) = socket.read(&mut buffer).await {
                                let request = String::from_utf8_lossy(&buffer[..n]);

                                let (status, body, content_type) = if request.starts_with("GET /health") {
                                    ("200 OK".to_string(), "OK".to_string(), "text/plain")
                                } else if request.starts_with("GET /ready") {
                                    ("200 OK".to_string(), "READY".to_string(), "text/plain")
                                } else if request.starts_with("GET /reload") {
                                    let _ = reload_tx_clone.send(()).await;
                                    ("200 OK".to_string(), "RELOADED".to_string(), "text/plain")

                                // ── ログ系エンドポイント ─────────────────────────
                                } else if request.starts_with("GET /logs/memory") {
                                    let path = workspace_path_clone.join("MEMORY.md");
                                    let content = std::fs::read_to_string(&path)
                                        .unwrap_or_else(|_| "(MEMORY.md が見つかりません)".to_string());
                                    ("200 OK".to_string(), content, "text/plain; charset=utf-8")

                                } else if request.starts_with("GET /logs/heartbeat-digest") {
                                    let path = workspace_path_clone.join("memory").join("heartbeat-digest.md");
                                    let content = std::fs::read_to_string(&path)
                                        .unwrap_or_else(|_| "(heartbeat-digest.md が見つかりません)".to_string());
                                    ("200 OK".to_string(), content, "text/plain; charset=utf-8")

                                } else if request.starts_with("GET /logs/heartbeat-state") {
                                    let path = workspace_path_clone.join("memory").join("heartbeat-state.json");
                                    let raw = std::fs::read_to_string(&path)
                                        .unwrap_or_else(|_| "{}".to_string());
                                    // JSON をパースして pretty-print し直す（失敗時はそのまま）
                                    let pretty = serde_json::from_str::<serde_json::Value>(&raw)
                                        .ok()
                                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                                        .unwrap_or(raw);
                                    ("200 OK".to_string(), pretty, "text/plain; charset=utf-8")

                                } else if request.starts_with("GET /logs/app") {
                                    let logs = if let Some(log_path) = get_latest_app_log() {
                                        read_last_lines(&log_path, 100)
                                    } else {
                                        format!("No application logs found under {}/logs/", get_app_dir().display())
                                    };
                                    ("200 OK".to_string(), logs, "text/plain; charset=utf-8")

                                // ── デバッグダンプ エンドポイント ──────────────
                                } else if request.starts_with("GET /debug/request") {
                                    let path = workspace_path_clone.join("memory").join("debug").join("last_request.json");
                                    let raw = std::fs::read_to_string(&path)
                                        .unwrap_or_else(|_| "null".to_string());
                                    let pretty = serde_json::from_str::<serde_json::Value>(&raw)
                                        .ok()
                                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                                        .unwrap_or(raw);
                                    ("200 OK".to_string(), pretty, "application/json; charset=utf-8")

                                } else if request.starts_with("GET /debug/response") {
                                    let path = workspace_path_clone.join("memory").join("debug").join("last_response.json");
                                    let raw = std::fs::read_to_string(&path)
                                        .unwrap_or_else(|_| "null".to_string());
                                    let pretty = serde_json::from_str::<serde_json::Value>(&raw)
                                        .ok()
                                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                                        .unwrap_or(raw);
                                    ("200 OK".to_string(), pretty, "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/queue") {
                                    let mut state = crate::QUEUE_STATE.lock().unwrap().clone();
                                    
                                    // 最新のクールダウン情報も動的に更新する
                                    if let Some(cooldown_dur) = rustyclaw_providers::global_cooldown_remaining() {
                                        for item in state.items.iter_mut() {
                                            if item.status == "Cooldown" {
                                                item.cooldown_left_secs = cooldown_dur.as_secs_f64();
                                            }
                                        }
                                    } else {
                                        // 全体のグローバルクールダウンが無い場合、Cooldown状態のアイテムはWaiting等に修正する
                                        for item in state.items.iter_mut() {
                                            if item.status == "Cooldown" {
                                                item.status = "Waiting".to_string();
                                                item.cooldown_left_secs = 0.0;
                                            }
                                        }
                                    }
                                    
                                    let json = serde_json::to_string(&state.items).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/neurons") {
                                    let stats = rustyclaw_providers::get_neuron_stats();
                                    let json = serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/concurrency") {
                                    let available = gmn_sem_clone.available_permits();
                                    let active = gmn_cap_clone.saturating_sub(available);
                                    let queue_state = crate::QUEUE_STATE.lock().unwrap();
                                    let queue_depth = queue_state.items.iter()
                                        .filter(|i| i.status == "Waiting")
                                        .count();
                                    drop(queue_state);
                                    let cooldown_secs = rustyclaw_providers::global_cooldown_remaining()
                                        .map(|d| d.as_secs_f64())
                                        .unwrap_or(0.0);
                                    let json = serde_json::json!({
                                        "active": active,
                                        "available": available,
                                        "capacity": gmn_cap_clone,
                                        "queue_depth": queue_depth,
                                        "cooldown_secs": cooldown_secs,
                                        "global_cooldown": if cooldown_secs > 0.0 { Some(cooldown_secs) } else { None::<f64> }
                                    });
                                    ("200 OK".to_string(), json.to_string(), "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/usage/summary") {
                                    let since = extract_since_param(&request);
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let json = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_summary(since.as_deref())
                                    } else {
                                        serde_json::json!({ "total_runs": 0, "total_tokens": 0, "by_model": {} })
                                    };
                                    ("200 OK".to_string(), json.to_string(), "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/usage/timeline") {
                                    let gran = extract_query_i64(&request, "gran").unwrap_or(86400).max(1) as u64;
                                    let from = extract_query_i64(&request, "from");
                                    let now = chrono::Utc::now().timestamp();
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let rows = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_timeline(from, now, gran)
                                    } else { vec![] };
                                    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")

                                } else if request.starts_with("GET /api/usage/by-trigger") {
                                    let since = extract_since_param(&request);
                                    let db_path = workspace_path_clone.join("memory.db");
                                    let rows = if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                                        db.get_usage_by_trigger(since.as_deref())
                                    } else { vec![] };
                                    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                                    ("200 OK".to_string(), json, "application/json; charset=utf-8")

                                // ── ダッシュボード & チャット ────────────────────
                                } else if request.starts_with("GET /dashboard")
                                    || request.starts_with("GET / ")
                                    || request.starts_with("GET /\r\n")
                                {
                                    let html = get_dashboard_html();
                                    ("200 OK".to_string(), html, "text/html; charset=utf-8")

                                } else if request.starts_with("POST /chat") {
                                    let mut chat_resp = "Error: Invalid request body".to_string();

                                    if let Some(body_start) = request.find("\r\n\r\n") {
                                        let json_body = request[body_start + 4..].trim();
                                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_body) {
                                            if let Some(msg) = val["message"].as_str() {
                                                let session_id = "http-dashboard".to_string();
                                                let mut rx = bus_clone.subscribe();

                                                let event = crate::SystemEvent::IncomingMessage {
                                                    session_id: session_id.clone(),
                                                    user_id: "http-user".to_string(),
                                                    channel_id: "http".to_string(),
                                                    content: msg.to_string(),
                                                    priority: crate::Priority::Normal,
                                                };

                                                if bus_clone.publish(event).is_ok() {
                                                    tokio::select! {
                                                        res = async {
                                                            loop {
                                                                if let Ok(crate::SystemEvent::AgentResponse { session_id: resp_session, content, .. }) = rx.recv().await {
                                                                    if resp_session == session_id {
                                                                        return content;
                                                                    }
                                                                }
                                                            }
                                                        } => { chat_resp = res; }
                                                        _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {
                                                            chat_resp = "Error: Request timeout".to_string();
                                                        }
                                                    }
                                                } else {
                                                    chat_resp = "Error: Failed to publish chat event to bus".to_string();
                                                }
                                            }
                                        }
                                    }
                                    ("200 OK".to_string(), chat_resp, "text/plain; charset=utf-8")

                                } else {
                                    ("404 NOT FOUND".to_string(), "NOT FOUND".to_string(), "text/plain")
                                };

                                let response = format!(
                                    "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    status, content_type, body.len(), body
                                );
                                let _ = socket.write_all(response.as_bytes()).await;
                                let _ = socket.flush().await;
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("HealthServer: Error accepting connection: {:#}", e);
                    }
                }
            }
        });

        Ok(())
    }
}

// ── ヘルパー関数 ──────────────────────────────────────────────────────────────

fn read_last_lines(path: &Path, limit: usize) -> String {
    if !path.exists() {
        return format!("Log file not found: {:?}", path);
    }
    if let Ok(file) = std::fs::File::open(path) {
        let reader = std::io::BufReader::new(file);
        let lines: Vec<String> = reader.lines().flatten().collect();
        let start = if lines.len() > limit { lines.len() - limit } else { 0 };
        return lines[start..].join("\n");
    }
    "Failed to open log file.".to_string()
}

fn get_latest_app_log() -> Option<PathBuf> {
    let log_dir = get_app_dir().join("logs");
    if !log_dir.exists() {
        return None;
    }
    let mut latest_file = None;
    let mut latest_time = std::time::SystemTime::UNIX_EPOCH;
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) {
                if name.starts_with("rustyclaw.log") {
                    if let Ok(mt) = std::fs::metadata(&path).and_then(|m| m.modified()) {
                        if mt > latest_time {
                            latest_time = mt;
                            latest_file = Some(path);
                        }
                    }
                }
            }
        }
    }
    latest_file
}

/// GET /api/usage/summary?since=2026-05-01 の since パラメータを抽出する。
/// 形式が `YYYY-MM-DD...`（先頭が数字・長さ10以上）の場合のみ採用する。
fn extract_since_param(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let query_start = first_line.find('?')?;
    let query = &first_line[query_start + 1..];
    let end = query.find(' ').unwrap_or(query.len());
    for pair in query[..end].split('&') {
        if let Some(val) = pair.strip_prefix("since=") {
            if val.len() >= 10 && val.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// GET /path?key=value のクエリから整数値を取り出す（無ければ None）。
fn extract_query_i64(request: &str, key: &str) -> Option<i64> {
    let first_line = request.lines().next()?;
    let query_start = first_line.find('?')?;
    let query = &first_line[query_start + 1..];
    let end = query.find(' ').unwrap_or(query.len());
    for pair in query[..end].split('&') {
        if let Some(val) = pair.strip_prefix(&format!("{}=", key)) {
            return val.parse::<i64>().ok();
        }
    }
    None
}

// ── ダッシュボード HTML ────────────────────────────────────────────────────────


fn get_dashboard_html() -> String {
    r##"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>RustyClaw — Runtime Controller</title>
<link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600;800&family=Fira+Code:wght@400;500&display=swap" rel="stylesheet">
<style>
:root {
  --bg:       #000810;
  --panel-bg: rgba(0,12,28,.88);
  --border:   rgba(0,200,255,.12);
  --text:     #e2f4ff;
  --muted:    #4a7a9b;
  --purple:   #bf00ff;
  --blue:     #00d4ff;
  --green:    #00ff9f;
  --cyan:     #00e5ff;
  --amber:    #ffaa00;
  --pink:     #ff006e;
  --red:      #ff2244;
  --term-bg:  #000510;
  --radius:   6px;
}
*{box-sizing:border-box;margin:0;padding:0}
body{
  font-family:'Outfit',sans-serif;
  background:var(--bg);
  background-image:radial-gradient(ellipse at 0% 0%,rgba(0,80,180,.18) 0,transparent 55%),radial-gradient(ellipse at 100% 100%,rgba(120,0,255,.14) 0,transparent 55%);
  color:var(--text);height:100vh;display:flex;flex-direction:column;overflow:hidden;position:relative;
}
body::after{
  content:'';position:fixed;inset:0;
  background:repeating-linear-gradient(0deg,transparent 0px,transparent 3px,rgba(0,0,0,.04) 3px,rgba(0,0,0,.04) 4px);
  pointer-events:none;z-index:9999;
}
header{
  background:rgba(0,8,20,.92);backdrop-filter:blur(16px);
  border-bottom:1px solid rgba(0,212,255,.15);
  box-shadow:0 1px 20px rgba(0,212,255,.06);
  padding:9px 18px;display:flex;align-items:center;gap:18px;flex-shrink:0;
}
.logo{
  font-size:17px;font-weight:800;letter-spacing:.02em;color:var(--blue);
  text-shadow:0 0 12px rgba(0,212,255,.6),0 0 30px rgba(0,212,255,.3);
  white-space:nowrap;position:relative;
}
.logo::before{
  content:attr(data-text);position:absolute;inset:0;color:var(--pink);
  text-shadow:0 0 8px var(--pink);clip-path:inset(100% 0 0 0);
  animation:glitch 6s infinite;
}
@keyframes glitch{
  0%,89%,100%{clip-path:inset(100% 0 0 0);transform:translate(0)}
  90%{clip-path:inset(30% 0 60% 0);transform:translate(-2px,0);opacity:.7}
  92%{clip-path:inset(60% 0 20% 0);transform:translate(2px,0);opacity:.9}
  94%{clip-path:inset(10% 0 80% 0);transform:translate(-1px,0);opacity:.6}
}
.tabs{display:flex;gap:2px;background:rgba(0,212,255,.04);padding:3px;border-radius:var(--radius);border:1px solid rgba(0,212,255,.1)}
.tab{
  padding:5px 18px;border-radius:4px;font-size:12px;font-weight:700;letter-spacing:.05em;
  cursor:pointer;border:none;background:transparent;color:var(--muted);
  font-family:'Fira Code',monospace;transition:all .15s;
}
.tab.active{background:rgba(0,212,255,.12);color:var(--cyan);border:1px solid rgba(0,212,255,.25);box-shadow:0 0 10px rgba(0,212,255,.15)}
.header-right{margin-left:auto;display:flex;align-items:center;gap:12px}
.status-badge{
  display:flex;align-items:center;gap:6px;font-size:11px;font-weight:700;letter-spacing:.06em;
  color:var(--green);background:rgba(0,255,159,.07);
  padding:4px 12px;border-radius:20px;border:1px solid rgba(0,255,159,.2);
  box-shadow:0 0 10px rgba(0,255,159,.1);font-family:'Fira Code',monospace;
}
.status-dot{width:6px;height:6px;background:var(--green);border-radius:50%;box-shadow:0 0 6px var(--green);animation:pulse 1.5s infinite}
@keyframes pulse{0%{transform:scale(.9);box-shadow:0 0 0 0 rgba(0,255,159,.6)}70%{transform:scale(1);box-shadow:0 0 0 6px rgba(0,255,159,0)}100%{transform:scale(.9);box-shadow:0 0 0 0 rgba(0,255,159,0)}}
.page{display:none;flex:1;overflow:hidden;min-height:0}
.page.active{display:flex;flex-direction:column}
/* MONITOR */
.monitor-grid{flex:1;display:grid;grid-template-rows:130px 1fr 1fr;gap:8px;padding:8px 10px;overflow:hidden;min-height:0}
.row1{display:grid;grid-template-columns:2.2fr 1fr 1fr;gap:8px}
.row2{display:grid;grid-template-columns:1fr 1fr;gap:8px;min-height:0}
.row3{display:grid;grid-template-columns:3fr 2fr;gap:8px;min-height:0}
.panel{background:var(--term-bg);border:1px solid var(--border);border-radius:var(--radius);display:flex;flex-direction:column;overflow:hidden;min-height:0}
.panel.queue   {border-color:rgba(255,0,110,.25);box-shadow:0 0 12px rgba(255,0,110,.06),inset 0 0 20px rgba(255,0,110,.02)}
.panel.concur  {border-color:rgba(0,212,255,.25);box-shadow:0 0 12px rgba(0,212,255,.06),inset 0 0 20px rgba(0,212,255,.02)}
.panel.neurons {border-color:rgba(0,229,255,.25);box-shadow:0 0 12px rgba(0,229,255,.06),inset 0 0 20px rgba(0,229,255,.02)}
.panel.request {border-color:rgba(191,0,255,.25);box-shadow:0 0 12px rgba(191,0,255,.06),inset 0 0 20px rgba(191,0,255,.02)}
.panel.response{border-color:rgba(0,255,159,.25);box-shadow:0 0 12px rgba(0,255,159,.06),inset 0 0 20px rgba(0,255,159,.02)}
.panel.applog  {border-color:rgba(0,212,255,.2);box-shadow:0 0 12px rgba(0,212,255,.04)}
.panel.chat-p  {border-color:rgba(191,0,255,.2);box-shadow:0 0 12px rgba(191,0,255,.06);background:rgba(0,12,28,.9)}
.panel-head{
  padding:5px 12px;border-bottom:1px solid rgba(255,255,255,.05);
  display:flex;align-items:center;justify-content:space-between;flex-shrink:0;background:rgba(0,0,0,.3);
}
.panel-label{font-size:10px;font-weight:700;letter-spacing:.1em;font-family:'Fira Code',monospace}
.panel.queue    .panel-label{color:var(--pink);  text-shadow:0 0 8px rgba(255,0,110,.5)}
.panel.concur   .panel-label{color:var(--blue);  text-shadow:0 0 8px rgba(0,212,255,.5)}
.panel.neurons  .panel-label{color:var(--cyan);  text-shadow:0 0 8px rgba(0,229,255,.5)}
.panel.request  .panel-label{color:var(--purple);text-shadow:0 0 8px rgba(191,0,255,.5)}
.panel.response .panel-label{color:var(--green); text-shadow:0 0 8px rgba(0,255,159,.5)}
.panel.applog   .panel-label{color:var(--cyan);  text-shadow:0 0 8px rgba(0,212,255,.4)}
.panel.chat-p   .panel-label{color:var(--purple);text-shadow:0 0 8px rgba(191,0,255,.5)}
.panel-body{flex:1;padding:8px 10px;overflow-y:auto;min-height:0;font-family:'Fira Code',monospace;font-size:11px;line-height:1.7}
.rts{font-size:9px;color:var(--muted);font-family:'Fira Code',monospace}
.q-item{display:flex;align-items:center;gap:7px;padding:4px 0;font-size:11.5px;border-bottom:1px solid rgba(255,255,255,.03)}
.q-item:last-child{border-bottom:none}
.q-pill{font-size:9px;font-weight:700;padding:2px 6px;border-radius:3px;letter-spacing:.06em;flex-shrink:0}
.pill-exec{background:rgba(0,255,159,.12);color:var(--green);border:1px solid rgba(0,255,159,.3);box-shadow:0 0 6px rgba(0,255,159,.2)}
.pill-wait{background:rgba(0,212,255,.10);color:var(--blue); border:1px solid rgba(0,212,255,.3);box-shadow:0 0 6px rgba(0,212,255,.15)}
.pill-cool{background:rgba(255,170,0,.10);color:var(--amber);border:1px solid rgba(255,170,0,.3);box-shadow:0 0 6px rgba(255,170,0,.15)}
.q-sid {color:var(--text);font-size:11px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:200px}
.q-desc{color:var(--muted);font-size:10px;margin-left:auto;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:160px}
.q-time{color:var(--muted);font-size:10px;flex-shrink:0}
.cool-bar{height:3px;background:rgba(255,255,255,.05);border-radius:2px;margin-top:2px;overflow:hidden}
.cool-fill{height:100%;border-radius:2px;background:linear-gradient(90deg,var(--amber),#ff6600);box-shadow:0 0 4px var(--amber);transition:width .5s}
.slot-row{display:flex;gap:4px;margin:6px 0 8px}
.slot{flex:1;height:16px;border-radius:3px;border:1px solid rgba(0,212,255,.15);background:rgba(255,255,255,.03)}
.slot.active{background:rgba(0,212,255,.25);border-color:var(--blue);box-shadow:0 0 6px rgba(0,212,255,.4)}
.stat-line{display:flex;justify-content:space-between;align-items:center;padding:3px 0;border-bottom:1px solid rgba(255,255,255,.03);font-size:11.5px}
.stat-line:last-child{border-bottom:none}
.sk{color:var(--muted)}.sv{color:var(--text);font-weight:600}
.sv.ok  {color:var(--green);text-shadow:0 0 8px rgba(0,255,159,.4)}
.sv.busy{color:var(--amber);text-shadow:0 0 8px rgba(255,170,0,.4)}
.sv.warn{color:var(--pink); text-shadow:0 0 8px rgba(255,0,110,.4)}
.n-gauge{margin:6px 0 8px;height:6px;background:rgba(255,255,255,.06);border-radius:3px;overflow:hidden}
.n-fill {height:100%;border-radius:3px;background:linear-gradient(90deg,var(--cyan),var(--blue));box-shadow:0 0 6px var(--cyan);transition:width .5s}
.log-line{display:flex;gap:8px;align-items:baseline;padding:1px 0}
.log-ts{color:var(--muted);font-size:10px;flex-shrink:0}
.log-lv{font-size:9px;font-weight:700;flex-shrink:0;letter-spacing:.04em}
.log-lv.info {color:var(--cyan)}
.log-lv.warn {color:var(--amber);text-shadow:0 0 6px rgba(255,170,0,.5)}
.log-lv.error{color:var(--red);  text-shadow:0 0 6px rgba(255,34,68,.5)}
.log-msg{color:var(--text);font-size:10.5px;word-break:break-all}
.chat-msgs{flex:1;overflow-y:auto;padding:10px;display:flex;flex-direction:column;gap:8px;min-height:0}
.bubble{max-width:86%;padding:8px 12px;border-radius:var(--radius);font-size:13px;line-height:1.55;font-family:'Outfit',sans-serif}
.bubble.user{align-self:flex-end;background:rgba(191,0,255,.12);border:1px solid rgba(191,0,255,.2);border-bottom-right-radius:1px;box-shadow:0 0 10px rgba(191,0,255,.08)}
.bubble.ai  {align-self:flex-start;background:rgba(0,212,255,.06);border:1px solid rgba(0,212,255,.12);border-bottom-left-radius:1px}
.chat-in-area{padding:8px 10px;border-top:1px solid rgba(255,255,255,.05);display:flex;gap:6px;flex-shrink:0}
.chat-in{flex:1;background:rgba(0,0,0,.4);border:1px solid rgba(0,212,255,.15);border-radius:var(--radius);padding:7px 12px;color:var(--text);font-size:13px;font-family:'Outfit',sans-serif;outline:none;transition:border-color .2s,box-shadow .2s}
.chat-in:focus{border-color:rgba(0,212,255,.45);box-shadow:0 0 8px rgba(0,212,255,.15)}
.send-btn{background:linear-gradient(135deg,rgba(191,0,255,.7),rgba(0,212,255,.7));border:none;border-radius:var(--radius);color:#fff;padding:0 16px;font-size:13px;font-weight:700;cursor:pointer;box-shadow:0 0 12px rgba(0,212,255,.2);transition:opacity .2s,box-shadow .2s;font-family:'Fira Code',monospace;letter-spacing:.04em}
.send-btn:hover{opacity:.88;box-shadow:0 0 20px rgba(0,212,255,.35)}
.loading-dots{display:flex;gap:4px;padding:10px 14px;align-self:flex-start}
.dot{width:5px;height:5px;background:var(--muted);border-radius:50%;animation:blink 1.4s infinite both}
.dot:nth-child(2){animation-delay:.2s}.dot:nth-child(3){animation-delay:.4s}
@keyframes blink{0%,80%,100%{opacity:.2}40%{opacity:1}}
/* STATS */
.stats-layout{flex:1;display:flex;flex-direction:column;gap:8px;padding:10px;overflow-y:auto;min-height:0}
.stats-top{display:flex;align-items:center;justify-content:space-between}
.stats-title{font-size:11px;color:var(--muted);font-family:'Fira Code',monospace;letter-spacing:.08em}
.period-bar{display:flex;gap:3px}
.period-btn{padding:4px 14px;border-radius:4px;font-size:11px;font-weight:700;cursor:pointer;border:1px solid rgba(255,255,255,.08);background:transparent;color:var(--muted);font-family:'Fira Code',monospace;transition:all .15s}
.period-btn.active{background:rgba(0,212,255,.12);color:var(--blue);border-color:rgba(0,212,255,.3);box-shadow:0 0 8px rgba(0,212,255,.12)}
.kpi-row{display:grid;grid-template-columns:repeat(4,1fr);gap:8px}
.kpi{background:var(--panel-bg);border:1px solid var(--border);border-radius:var(--radius);padding:12px 14px}
.kpi.runs   {border-color:rgba(0,212,255,.25);box-shadow:0 0 10px rgba(0,212,255,.06)}
.kpi.tokens {border-color:rgba(191,0,255,.25);box-shadow:0 0 10px rgba(191,0,255,.06)}
.kpi.avg    {border-color:rgba(0,255,159,.25);box-shadow:0 0 10px rgba(0,255,159,.06)}
.kpi.cf     {border-color:rgba(0,229,255,.25);box-shadow:0 0 10px rgba(0,229,255,.06)}
.kpi-lbl{font-size:10px;color:var(--muted);font-weight:700;letter-spacing:.06em;font-family:'Fira Code',monospace}
.kpi-val{font-size:24px;font-weight:800;margin:4px 0 2px}
.kpi.runs   .kpi-val{color:var(--blue);  text-shadow:0 0 12px rgba(0,212,255,.4)}
.kpi.tokens .kpi-val{color:var(--purple);text-shadow:0 0 12px rgba(191,0,255,.4)}
.kpi.avg    .kpi-val{color:var(--green); text-shadow:0 0 12px rgba(0,255,159,.4)}
.kpi.cf     .kpi-val{color:var(--cyan);  text-shadow:0 0 12px rgba(0,229,255,.4)}
.kpi-sub{font-size:10.5px;color:var(--muted)}
.chart-wrap{background:var(--term-bg);border:1px solid rgba(191,0,255,.2);border-radius:var(--radius);padding:12px 14px;box-shadow:0 0 12px rgba(191,0,255,.05)}
.chart-lbl{font-size:10px;font-weight:700;color:var(--purple);letter-spacing:.08em;font-family:'Fira Code',monospace;margin-bottom:8px}
.x-axis{display:flex;justify-content:space-between;margin-top:4px}
.x-axis span{font-size:9px;color:var(--muted);font-family:'Fira Code',monospace}
.chart-legend{display:flex;gap:14px;margin-top:6px}
.leg-item{display:flex;align-items:center;gap:5px;font-size:10px;color:var(--muted);font-family:'Fira Code',monospace}
.leg-dot{width:8px;height:3px;border-radius:1px}
.stats-bottom{display:grid;grid-template-columns:1fr 1fr;gap:8px}
.breakdown{background:var(--term-bg);border:1px solid var(--border);border-radius:var(--radius);padding:12px 14px}
.bd-title{font-size:10px;font-weight:700;letter-spacing:.08em;font-family:'Fira Code',monospace;margin-bottom:8px}
.bd-row{display:flex;align-items:center;gap:8px;padding:4px 0;border-bottom:1px solid rgba(255,255,255,.03);font-size:11px}
.bd-row:last-child{border-bottom:none}
.bd-name{width:150px;font-family:'Fira Code',monospace;font-size:10.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.bd-bar-bg{flex:1;height:5px;background:rgba(255,255,255,.06);border-radius:2px;overflow:hidden}
.bd-bar{height:100%;border-radius:2px}
.bd-cnt{font-size:10px;color:var(--muted);text-align:right;min-width:55px}
::-webkit-scrollbar{width:4px;height:4px}
::-webkit-scrollbar-track{background:transparent}
::-webkit-scrollbar-thumb{background:rgba(0,212,255,.15);border-radius:2px}
::-webkit-scrollbar-thumb:hover{background:rgba(0,212,255,.3)}
</style>
</head>
<body>
<header>
  <span class="logo" data-text="🐈‍⬛ RustyClaw">🐈‍⬛ RustyClaw</span>
  <nav class="tabs">
    <button class="tab active" onclick="switchTab('monitor',this)">MONITOR</button>
    <button class="tab"        onclick="switchTab('stats',  this)">STATS</button>
  </nav>
  <div class="header-right">
    <span id="hostLabel" style="font-size:10px;color:var(--muted);font-family:'Fira Code',monospace">:8080</span>
    <div class="status-badge"><span class="status-dot"></span>ACTIVE</div>
  </div>
</header>
<div id="page-monitor" class="page active">
<div class="monitor-grid">
  <div class="row1">
    <div class="panel queue">
      <div class="panel-head"><span class="panel-label">◈ LANE QUEUE</span><span class="rts" id="queue-ts">—</span></div>
      <div class="panel-body" id="queuePanel" style="padding:6px 8px;"><div style="color:var(--muted);text-align:center;padding:10px;font-family:'Fira Code',monospace;font-size:11px;">稼働タスクなし（待機中・正常）</div></div>
    </div>
    <div class="panel concur">
      <div class="panel-head"><span class="panel-label">◈ CONCURRENCY</span><span class="rts" id="concur-ts">—</span></div>
      <div class="panel-body">
        <div class="slot-row" id="slotRow"></div>
        <div class="stat-line"><span class="sk">Active</span><span class="sv" id="cActive">—</span></div>
        <div class="stat-line"><span class="sk">Queue depth</span><span class="sv" id="cDepth">—</span></div>
        <div class="stat-line"><span class="sk">Cooldown</span><span class="sv" id="cCool">—</span></div>
        <div class="stat-line"><span class="sk">Global limit</span><span class="sv" id="cGlobal">—</span></div>
      </div>
    </div>
    <div class="panel neurons">
      <div class="panel-head"><span class="panel-label">◈ CF NEURONS</span><span class="rts" id="neuron-ts">—</span></div>
      <div class="panel-body">
        <div class="n-gauge"><div class="n-fill" id="nFill" style="width:0%"></div></div>
        <div class="stat-line"><span class="sk">Today</span><span class="sv" id="nToday" style="color:var(--cyan)">—</span></div>
        <div class="stat-line"><span class="sk">Limit</span><span class="sv">10,000</span></div>
        <div class="stat-line"><span class="sk">Remaining</span><span class="sv ok" id="nRem">—</span></div>
        <div class="stat-line"><span class="sk">Usage</span><span class="sv" id="nPct">—</span></div>
      </div>
    </div>
  </div>
  <div class="row2">
    <div class="panel request">
      <div class="panel-head"><span class="panel-label">◈ LLM REQUEST</span><span class="rts" id="req-ts">—</span></div>
      <div class="panel-body" id="reqPanel" style="white-space:pre-wrap;word-break:break-all;font-size:10.5px;color:rgba(180,210,230,.65);">読み込み中...</div>
    </div>
    <div class="panel response">
      <div class="panel-head"><span class="panel-label">◈ LLM RESPONSE</span><span class="rts" id="res-ts">—</span></div>
      <div class="panel-body" id="resPanel" style="white-space:pre-wrap;word-break:break-all;font-size:10.5px;color:rgba(0,255,159,.7);">読み込み中...</div>
    </div>
  </div>
  <div class="row3">
    <div class="panel applog">
      <div class="panel-head"><span class="panel-label">◈ APP LOG</span><span class="rts">↻ 2s</span></div>
      <div class="panel-body" id="appLog">読み込み中...</div>
    </div>
    <div class="panel chat-p">
      <div class="panel-head"><span class="panel-label">◈ CHAT</span><span style="font-size:9px;color:var(--muted);font-family:'Fira Code',monospace">http-dashboard</span></div>
      <div class="chat-msgs" id="chatMessages"><div class="bubble ai">こんにちは。RustyClaw Dashboard です。何かお手伝いできますか？</div></div>
      <div class="chat-in-area">
        <input class="chat-in" id="chatInput" type="text" placeholder="メッセージを入力..." onkeydown="handleKey(event)">
        <button class="send-btn" id="sendBtn" onclick="sendMessage()">SEND</button>
      </div>
    </div>
  </div>
</div>
</div>
<div id="page-stats" class="page">
<div class="stats-layout">
  <div class="stats-top">
    <span class="stats-title">TOKEN USAGE STATISTICS</span>
    <div class="period-bar">
      <button class="period-btn" onclick="setPeriod(7,this)">7D</button>
      <button class="period-btn active" onclick="setPeriod(30,this)">30D</button>
      <button class="period-btn" onclick="setPeriod(0,this)">ALL</button>
    </div>
  </div>
  <div class="kpi-row">
    <div class="kpi runs"><div class="kpi-lbl">TOTAL RUNS</div><div class="kpi-val" id="kpiRuns">—</div><div class="kpi-sub" id="kpiRunsSub">loading...</div></div>
    <div class="kpi tokens"><div class="kpi-lbl">TOTAL TOKENS</div><div class="kpi-val" id="kpiTokens">—</div><div class="kpi-sub" id="kpiTokensSub">loading...</div></div>
    <div class="kpi avg"><div class="kpi-lbl">AVG / RUN</div><div class="kpi-val" id="kpiAvg">—</div><div class="kpi-sub">tokens per execution</div></div>
    <div class="kpi cf"><div class="kpi-lbl">CF NEURONS TODAY</div><div class="kpi-val" id="kpiCf">—</div><div class="kpi-sub" id="kpiCfSub">loading...</div></div>
  </div>
  <div class="chart-wrap">
    <div class="chart-lbl">DAILY TOKEN USAGE</div>
    <svg id="timelineChart" viewBox="0 0 900 130" width="100%" height="130" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="none">
      <text x="4" y="20" fill="#1e3a5f" font-size="8" font-family="Fira Code">No data yet</text>
    </svg>
    <div class="x-axis" id="chartXAxis"></div>
    <div class="chart-legend">
      <div class="leg-item"><div class="leg-dot" style="background:#bf00ff"></div>Input</div>
      <div class="leg-item"><div class="leg-dot" style="background:#00d4ff"></div>Output</div>
    </div>
  </div>
  <div class="stats-bottom">
    <div class="breakdown" style="border-color:rgba(191,0,255,.2)"><div class="bd-title" style="color:var(--purple)">BY MODEL</div><div id="modelBreakdown"><div style="color:var(--muted);font-size:11px;padding:8px 0">No data yet</div></div></div>
    <div class="breakdown" style="border-color:rgba(0,229,255,.2)"><div class="bd-title" style="color:var(--cyan)">BY TRIGGER</div><div id="triggerBreakdown"><div style="color:var(--muted);font-size:11px;padding:8px 0">No data yet</div></div></div>
  </div>
</div>
</div>
<script>
function switchTab(id,btn){
  document.querySelectorAll('.page').forEach(p=>p.classList.remove('active'));
  document.querySelectorAll('.tab').forEach(t=>t.classList.remove('active'));
  document.getElementById('page-'+id).classList.add('active');
  btn.classList.add('active');
  if(id==='stats')loadStats();
}
function fmtK(n){if(n>=1e6)return(n/1e6).toFixed(2)+'M';if(n>=1e3)return(n/1e3).toFixed(1)+'k';return n.toString()}
function now(){return new Date().toLocaleTimeString('ja-JP',{hour:'2-digit',minute:'2-digit',second:'2-digit'})}
async function updateQueue(){
  try{
    const r=await fetch('/api/queue');if(!r.ok)return;
    const items=await r.json();
    document.getElementById('queue-ts').textContent='↻ '+now();
    const panel=document.getElementById('queuePanel');
    if(items.length===0){panel.innerHTML='<div style="color:var(--muted);text-align:center;padding:10px;font-family:\'Fira Code\',monospace;font-size:11px;">稼働タスクなし（待機中・正常）</div>';return}
    let html='';
    items.forEach((item,i)=>{
      const cls=item.status==='Executing'?'pill-exec':item.status==='Waiting'?'pill-wait':'pill-cool';
      const lbl=item.status==='Executing'?'EXEC':item.status==='Waiting'?'WAIT':'COOL';
      const elapsed=Math.floor((Date.now()-item.enqueued_at_ms)/1000);
      html+=`<div class="q-item"><span class="q-pill ${cls}">${lbl}</span><span class="q-sid">${item.session_id}</span><span class="q-desc">${item.description||''}</span><span class="q-time">${elapsed}s</span></div>`;
      if(item.status==='Cooldown'&&item.cooldown_left_secs>0){const pct=Math.min(100,(item.cooldown_left_secs/60)*100);html+=`<div class="cool-bar"><div class="cool-fill" style="width:${pct}%"></div></div>`}
    });
    panel.innerHTML=html;
  }catch{}
}
async function updateConcurrency(){
  try{
    const r=await fetch('/api/concurrency');if(!r.ok)return;
    const d=await r.json();
    document.getElementById('concur-ts').textContent='↻ '+now();
    const slots=document.getElementById('slotRow');slots.innerHTML='';
    for(let i=0;i<d.capacity;i++){const s=document.createElement('div');s.className='slot'+(i<d.active?' active':'');slots.appendChild(s)}
    document.getElementById('cActive').textContent=d.active+' / '+d.capacity;
    document.getElementById('cActive').className='sv '+(d.active>=d.capacity?'busy':'ok');
    document.getElementById('cDepth').textContent=d.queue_depth;
    document.getElementById('cCool').textContent=d.cooldown_secs>0?d.cooldown_secs.toFixed(1)+'s':'none';
    document.getElementById('cCool').className='sv '+(d.cooldown_secs>0?'warn':'ok');
    document.getElementById('cGlobal').textContent=d.global_cooldown?d.global_cooldown.toFixed(1)+'s':'none';
    document.getElementById('cGlobal').className='sv '+(d.global_cooldown?'warn':'ok');
  }catch{}
}
async function updateNeurons(){
  try{
    const r=await fetch('/api/neurons');if(!r.ok)return;
    const d=await r.json();
    document.getElementById('neuron-ts').textContent='↻ '+now();
    const today=d.today_used??0;const limit=10000;const pct=Math.min(100,(today/limit)*100).toFixed(1);
    document.getElementById('nFill').style.width=pct+'%';
    document.getElementById('nToday').textContent=today.toLocaleString();
    document.getElementById('nRem').textContent=(limit-today).toLocaleString();
    document.getElementById('nPct').textContent=pct+'%';
  }catch{}
}
async function updateInspector(){
  try{
    const[rq,rs]=await Promise.all([fetch('/debug/request'),fetch('/debug/response')]);
    const ts=now();
    if(rq.ok){const txt=await rq.text();document.getElementById('req-ts').textContent=ts;document.getElementById('reqPanel').textContent=txt.length>4000?'...(truncated head)\n'+txt.slice(-4000):txt}
    if(rs.ok){const txt=await rs.text();document.getElementById('res-ts').textContent=ts;document.getElementById('resPanel').textContent=txt.length>3000?txt.substring(0,3000)+'\n...(truncated)':txt}
  }catch{}
}
async function updateLog(){
  try{
    const r=await fetch('/logs/app');if(!r.ok)return;const txt=await r.text();
    const el=document.getElementById('appLog');
    const atBottom=el.scrollHeight-el.clientHeight<=el.scrollTop+60;
    const lines=txt.trim().split('\n').slice(-100);
    el.innerHTML=lines.map(line=>{
      const lvl=line.includes(' INFO ')?'info':line.includes(' WARN ')?'warn':line.includes(' ERROR ')?'error':'info';
      const tsM=line.match(/\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2})/);
      const ts=tsM?tsM[1]:'';
      const msg=line.replace(/^\S+\s+(INFO|WARN|ERROR)\s+\S+:\s*/,'').trim();
      return`<div class="log-line"><span class="log-ts">${ts}</span><span class="log-lv ${lvl}">${lvl.toUpperCase()}</span><span class="log-msg">${msg}</span></div>`;
    }).join('');
    if(atBottom)el.scrollTop=el.scrollHeight;
  }catch{}
}
function handleKey(e){if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();sendMessage()}}
async function sendMessage(){
  const inp=document.getElementById('chatInput');const msg=inp.value.trim();if(!msg)return;
  addBubble(msg,'user');inp.value='';
  const lid=addLoading();inp.disabled=true;document.getElementById('sendBtn').disabled=true;
  try{const r=await fetch('/chat',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({message:msg})});removeLoading(lid);addBubble(r.ok?await r.text():'エラー: 返答の取得に失敗しました。','ai')}
  catch{removeLoading(lid);addBubble('通信エラー','ai')}
  finally{inp.disabled=false;document.getElementById('sendBtn').disabled=false;inp.focus()}
}
function addBubble(text,role){const d=document.createElement('div');d.className='bubble '+role;d.innerHTML=text.replace(/\n/g,'<br>');const m=document.getElementById('chatMessages');m.appendChild(d);m.scrollTop=m.scrollHeight}
function addLoading(){const id='ld-'+Date.now();const el=document.createElement('div');el.className='loading-dots';el.id=id;el.innerHTML='<span class="dot"></span><span class="dot"></span><span class="dot"></span>';const m=document.getElementById('chatMessages');m.appendChild(el);m.scrollTop=m.scrollHeight;return id}
function removeLoading(id){const el=document.getElementById(id);if(el)el.remove()}
let currentPeriodDays=30;
function setPeriod(days,btn){currentPeriodDays=days;document.querySelectorAll('.period-btn').forEach(b=>b.classList.remove('active'));btn.classList.add('active');loadStats()}
async function loadStats(){
  const since=currentPeriodDays>0?new Date(Date.now()-currentPeriodDays*86400000).toISOString().slice(0,10):undefined;
  const qs=since?'?since='+since:'';
  try{
    const[rSum,rTl,rTr,rN]=await Promise.all([fetch('/api/usage/summary'+qs),fetch('/api/usage/timeline'+qs),fetch('/api/usage/by-trigger'+qs),fetch('/api/neurons')]);
    if(rSum.ok)renderSummary(await rSum.json());
    if(rTl.ok) renderTimeline(await rTl.json());
    if(rTr.ok) renderTriggers(await rTr.json());
    if(rN.ok)  renderNeuronsKpi(await rN.json());
  }catch{}
}
function renderSummary(d){
  document.getElementById('kpiRuns').textContent=(d.total_runs??0).toLocaleString();
  document.getElementById('kpiRunsSub').textContent='total executions';
  document.getElementById('kpiTokens').textContent=fmtK(d.total_tokens??0);
  document.getElementById('kpiTokensSub').textContent='input '+fmtK(d.total_input_tokens??0)+' / output '+fmtK(d.total_completion_tokens??0);
  const avg=d.total_runs>0?Math.round((d.total_tokens??0)/d.total_runs):0;
  document.getElementById('kpiAvg').textContent=avg.toLocaleString();
  const models=Object.entries(d.by_model??{}).sort((a,b)=>b[1].tokens-a[1].tokens);
  const totalTok=d.total_tokens||1;
  const colors=['#bf00ff','#00d4ff','#00ff9f','#ffaa00','#ff006e'];
  document.getElementById('modelBreakdown').innerHTML=models.slice(0,6).map(([m,v],i)=>{
    const pct=Math.round((v.tokens/totalTok)*100);
    return`<div class="bd-row"><span class="bd-name" style="color:${colors[i]||'#aaa'}">${m}</span><div class="bd-bar-bg"><div class="bd-bar" style="width:${pct}%;background:${colors[i]||'#aaa'}"></div></div><span class="bd-cnt">${fmtK(v.tokens)}</span></div>`;
  }).join('')||'<div style="color:var(--muted);font-size:11px;padding:8px 0">No data yet</div>';
}
function renderNeuronsKpi(d){
  const today=d.today_used??0;
  document.getElementById('kpiCf').textContent=today.toLocaleString();
  document.getElementById('kpiCfSub').textContent=((today/10000)*100).toFixed(1)+'% of 10,000 limit';
}
function renderTimeline(rows){
  if(!rows.length){document.getElementById('timelineChart').innerHTML='<text x="4" y="20" fill="#1e3a5f" font-size="8" font-family="Fira Code">No data yet</text>';return}
  const maxT=Math.max(...rows.map(r=>r.total_tokens??r.tokens??0));
  if(maxT===0)return;
  const W=900,H=120,PAD=20;
  const xStep=(W-PAD*2)/Math.max(rows.length-1,1);
  const scale=v=>PAD+((H-PAD*2)*(1-v/maxT));
  const inputPts=rows.map((r,i)=>`${PAD+i*xStep},${scale(r.input_tokens??0)}`).join(' ');
  const outPts  =rows.map((r,i)=>`${PAD+i*xStep},${scale(r.completion_tokens??0)}`).join(' ');
  const last=PAD+(rows.length-1)*xStep;
  const ia='M'+inputPts.replace(/ /g,' L')+` L${last},${H} L${PAD},${H} Z`;
  const oa='M'+outPts.replace(/ /g,' L')  +` L${last},${H} L${PAD},${H} Z`;
  document.getElementById('timelineChart').innerHTML=`
    <defs>
      <linearGradient id="ig" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#bf00ff" stop-opacity=".35"/><stop offset="100%" stop-color="#bf00ff" stop-opacity=".02"/></linearGradient>
      <linearGradient id="og" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#00d4ff" stop-opacity=".25"/><stop offset="100%" stop-color="#00d4ff" stop-opacity=".02"/></linearGradient>
    </defs>
    <line x1="${PAD}" y1="${PAD}" x2="${PAD}" y2="${H}" stroke="rgba(0,212,255,.1)" stroke-width="1"/>
    <line x1="${PAD}" y1="${H}" x2="${W-PAD}" y2="${H}" stroke="rgba(0,212,255,.1)" stroke-width="1"/>
    <path d="${ia}" fill="url(#ig)"/><polyline points="${inputPts}" fill="none" stroke="#bf00ff" stroke-width="1.5" stroke-linejoin="round"/>
    <path d="${oa}" fill="url(#og)"/><polyline points="${outPts}"   fill="none" stroke="#00d4ff" stroke-width="1.5" stroke-linejoin="round"/>`;
  const step=Math.max(1,Math.floor(rows.length/7));
  document.getElementById('chartXAxis').innerHTML=rows.filter((_,i)=>i%step===0||i===rows.length-1).map(r=>`<span>${r.date}</span>`).join('');
}
function renderTriggers(rows){
  const maxT=Math.max(...rows.map(r=>r.tokens??0),1);
  const colors=['#bf00ff','#00ff9f','#00d4ff','#ffaa00','#ff006e'];
  document.getElementById('triggerBreakdown').innerHTML=rows.slice(0,6).map((r,i)=>{
    const pct=Math.round((r.tokens/maxT)*100);
    return`<div class="bd-row"><span class="bd-name" style="color:${colors[i]||'#aaa'}">${r.trigger}</span><div class="bd-bar-bg"><div class="bd-bar" style="width:${pct}%;background:${colors[i]||'#aaa'}"></div></div><span class="bd-cnt">${r.runs} runs</span></div>`;
  }).join('')||'<div style="color:var(--muted);font-size:11px;padding:8px 0">No data yet</div>';
}
document.getElementById('hostLabel').textContent=location.host;updateQueue();updateConcurrency();updateNeurons();updateInspector();updateLog();
setInterval(updateQueue,1000);setInterval(updateConcurrency,1000);setInterval(updateNeurons,5000);setInterval(updateInspector,2000);setInterval(updateLog,2000);
</script>
</body>
</html>
"##.to_string()
}
