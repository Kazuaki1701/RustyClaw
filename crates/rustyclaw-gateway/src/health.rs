use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::io::BufRead;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct HealthServer {
    addr: SocketAddr,
    reload_tx: tokio::sync::mpsc::Sender<()>,
    bus: Arc<crate::MessageBus>,
    workspace_path: PathBuf,
}

impl HealthServer {
    pub fn new(port: u16, reload_tx: tokio::sync::mpsc::Sender<()>, bus: Arc<crate::MessageBus>, workspace_path: PathBuf) -> Self {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        Self { addr, reload_tx, bus, workspace_path }
    }

    pub async fn start(self) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await
            .with_context(|| format!("Failed to bind HealthServer to {}", self.addr))?;

        tracing::info!("HealthServer listening on http://{}", self.addr);
        let reload_tx = Arc::new(self.reload_tx);
        let bus = self.bus.clone();
        let workspace_path = Arc::new(self.workspace_path.clone());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut socket, client_addr)) => {
                        tracing::debug!("HealthServer: Accepted connection from {}", client_addr);
                        let reload_tx_clone = reload_tx.clone();
                        let bus_clone = bus.clone();
                        let workspace_path_clone = workspace_path.clone();

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
                                        "No application logs found under ~/.rustyclaw/".to_string()
                                    };
                                    ("200 OK".to_string(), logs, "text/plain; charset=utf-8")

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
    let log_dir = dirs::home_dir()?.join(".rustyclaw");
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

// ── ダッシュボード HTML ────────────────────────────────────────────────────────

fn get_dashboard_html() -> String {
    r#"<!DOCTYPE html>
<html lang="ja">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>RustyClaw Runtime Control Dashboard</title>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600;800&family=Fira+Code:wght@400;500&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg:           #0b0f19;
            --panel-bg:     rgba(15, 23, 42, 0.55);
            --border:       rgba(255, 255, 255, 0.08);
            --text:         #f8fafc;
            --muted:        #94a3b8;
            --purple:       #c084fc;
            --blue:         #60a5fa;
            --green:        #34d399;
            --cyan:         #22d3ee;
            --amber:        #fbbf24;
            --pink:         #f472b6;
            --term-bg:      #05070c;
        }
        * { box-sizing: border-box; margin: 0; padding: 0; }

        body {
            font-family: 'Outfit', sans-serif;
            background: var(--bg);
            background-image:
                radial-gradient(at 0% 0%,   rgba(168,85,247,.15) 0, transparent 50%),
                radial-gradient(at 100% 100%, rgba(59,130,246,.15) 0, transparent 50%);
            color: var(--text);
            height: 100vh;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }

        /* ── HEADER ─────────────────────────────────── */
        header {
            background: rgba(15,23,42,.8);
            backdrop-filter: blur(12px);
            border-bottom: 1px solid var(--border);
            padding: 14px 28px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            flex-shrink: 0;
        }
        header h1 {
            font-size: 20px; font-weight: 800;
            background: linear-gradient(135deg, var(--purple), var(--blue));
            -webkit-background-clip: text; -webkit-text-fill-color: transparent;
        }
        .status-badge {
            display: flex; align-items: center; gap: 8px;
            font-size: 13px; font-weight: 600; color: var(--green);
            background: rgba(52,211,153,.1); padding: 5px 12px;
            border-radius: 20px; border: 1px solid rgba(52,211,153,.25);
        }
        .status-dot {
            width: 8px; height: 8px; background: var(--green);
            border-radius: 50%; animation: pulse 1.5s infinite;
        }

        /* ── MAIN LAYOUT ────────────────────────────── */
        main {
            flex: 1;
            display: flex;
            padding: 16px;
            gap: 16px;
            overflow: hidden;
            min-height: 0;
        }

        /* ── CHAT PANE (left) ───────────────────────── */
        .chat-pane {
            flex: 4;
            background: var(--panel-bg);
            backdrop-filter: blur(16px);
            border: 1px solid var(--border);
            border-radius: 14px;
            display: flex; flex-direction: column;
            overflow: hidden;
            box-shadow: 0 10px 30px rgba(0,0,0,.3);
        }
        .chat-header {
            padding: 13px 18px; border-bottom: 1px solid var(--border);
            display: flex; align-items: center; gap: 10px; flex-shrink: 0;
        }
        .avatar {
            width: 30px; height: 30px; border-radius: 50%;
            background: linear-gradient(135deg, #a855f7, #3b82f6); flex-shrink: 0;
        }
        .chat-header-info h2 { font-size: 15px; font-weight: 600; }
        .chat-header-info p  { font-size: 11px; color: var(--muted); }

        .chat-messages {
            flex: 1; padding: 16px;
            overflow-y: auto;
            display: flex; flex-direction: column; gap: 12px;
            min-height: 0;
        }
        .message {
            max-width: 82%; padding: 10px 14px;
            border-radius: 12px; font-size: 14px; line-height: 1.55;
            animation: slideUp .25s ease forwards;
        }
        .message.user {
            align-self: flex-end;
            background: linear-gradient(135deg, rgba(168,85,247,.18), rgba(59,130,246,.18));
            border: 1px solid rgba(168,85,247,.2); border-bottom-right-radius: 3px;
        }
        .message.assistant {
            align-self: flex-start;
            background: rgba(255,255,255,.05);
            border: 1px solid var(--border); border-bottom-left-radius: 3px;
        }
        .chat-input-area {
            padding: 12px 16px; border-top: 1px solid var(--border);
            display: flex; gap: 8px; flex-shrink: 0;
        }
        .chat-input {
            flex: 1; background: rgba(0,0,0,.2); border: 1px solid var(--border);
            border-radius: 8px; padding: 10px 14px; color: var(--text);
            font-family: inherit; font-size: 14px; outline: none; transition: border-color .2s;
        }
        .chat-input:focus { border-color: var(--purple); }
        .send-btn {
            background: linear-gradient(135deg, #a855f7, #3b82f6);
            border: none; border-radius: 8px; color: #fff;
            padding: 0 18px; font-weight: 600; font-size: 14px;
            cursor: pointer; transition: opacity .2s;
        }
        .send-btn:hover { opacity: .88; }
        .loading-dots { display: flex; gap: 4px; padding: 10px 14px; align-self: flex-start; }
        .dot {
            width: 5px; height: 5px; background: var(--muted);
            border-radius: 50%; animation: blink 1.4s infinite both;
        }
        .dot:nth-child(2) { animation-delay: .2s; }
        .dot:nth-child(3) { animation-delay: .4s; }

        /* ── RIGHT COLUMN ───────────────────────────── */
        .right-col {
            flex: 6;
            display: flex; flex-direction: column;
            gap: 14px;
            min-height: 0; overflow: hidden;
        }

        /* ── TERMINAL PANEL (shared base) ───────────── */
        .panel {
            background: var(--term-bg);
            border: 1px solid var(--border);
            border-radius: 12px;
            display: flex; flex-direction: column;
            overflow: hidden;
            box-shadow: 0 6px 20px rgba(0,0,0,.35);
            min-height: 0;
        }
        .panel.memory         { border-color: rgba(192,132,252,.18); flex: 3; }
        .panel.heartbeat-wrap { flex: 3; display: flex; gap: 14px; background: transparent;
                                border: none; box-shadow: none; overflow: hidden; }
        .panel.digest         { border-color: rgba(52,211,153,.18); flex: 6; }
        .panel.hb-state       { border-color: rgba(251,191,36,.18);  flex: 4; }
        .panel.queue          { border-color: rgba(244,114,182,.18);  flex: 3; }
        .panel.applog         { border-color: rgba(34,211,238,.18);  flex: 4; }

        .panel-header {
            background: rgba(255,255,255,.02);
            padding: 7px 14px; border-bottom: 1px solid var(--border);
            display: flex; align-items: center; justify-content: space-between;
            flex-shrink: 0;
        }
        .panel-title {
            font-size: 11px; font-family: 'Fira Code', monospace; font-weight: 600;
            display: flex; align-items: center; gap: 7px;
        }
        .panel.memory   .panel-title { color: var(--purple); }
        .panel.digest   .panel-title { color: var(--green);  }
        .panel.hb-state .panel-title { color: var(--amber);  }
        .panel.queue    .panel-title { color: var(--pink);   }
        .panel.applog   .panel-title { color: var(--cyan);   }

        .panel-dots { display: flex; gap: 5px; }
        .pd { width: 9px; height: 9px; border-radius: 50%; background: rgba(255,255,255,.1); }

        .panel-body {
            flex: 1; padding: 12px 14px;
            overflow-y: auto; min-height: 0;
            font-family: 'Fira Code', monospace;
            font-size: 12px; line-height: 1.65;
            white-space: pre-wrap; word-break: break-word;
        }
        .panel.memory   .panel-body { color: #d8b4fe; }
        .panel.digest   .panel-body { color: #a7f3d0; }
        .panel.hb-state .panel-body { color: #fde68a; }
        .panel.queue    .panel-body { color: #fbcfe8; }
        .panel.applog   .panel-body { color: #bae6fd; }

        /* ── SCROLLBAR ──────────────────────────────── */
        ::-webkit-scrollbar { width: 5px; height: 5px; }
        ::-webkit-scrollbar-track { background: transparent; }
        ::-webkit-scrollbar-thumb { background: rgba(255,255,255,.1); border-radius: 3px; }
        ::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,.2); }

        /* ── ANIMATIONS ─────────────────────────────── */
        @keyframes pulse {
            0%   { transform: scale(.95); box-shadow: 0 0 0 0 rgba(52,211,153,.5); }
            70%  { transform: scale(1);   box-shadow: 0 0 0 6px rgba(52,211,153,0); }
            100% { transform: scale(.95); box-shadow: 0 0 0 0 rgba(52,211,153,0); }
        }
        @keyframes slideUp {
            from { opacity: 0; transform: translateY(8px); }
            to   { opacity: 1; transform: translateY(0); }
        }
        @keyframes blink {
            0%   { opacity: .2; }
            20%  { opacity: 1;  }
            100% { opacity: .2; }
        }
    </style>
</head>
<body>

<header>
    <h1>🐈‍⬛ RustyClaw Runtime Controller</h1>
    <div class="status-badge">
        <span class="status-dot"></span>ACTIVE GATEWAY RUNNING
    </div>
</header>

<main>
    <!-- ── LEFT: CHAT ── -->
    <div class="chat-pane">
        <div class="chat-header">
            <div class="avatar"></div>
            <div class="chat-header-info">
                <h2>GEMI アシスタント</h2>
                <p>RustyClaw Gateway 連携対話</p>
            </div>
        </div>
        <div class="chat-messages" id="chatMessages">
            <div class="message assistant">
                こんにちは。RustyClaw Dashboard です。何かお手伝いできますか？
            </div>
        </div>
        <div class="chat-input-area">
            <input class="chat-input" id="chatInput" type="text"
                   placeholder="メッセージを入力..." onkeydown="handleKey(event)">
            <button class="send-btn" id="sendBtn" onclick="sendMessage()">送信</button>
        </div>
    </div>

    <!-- ── RIGHT COLUMN ── -->
    <div class="right-col">

        <!-- MEMORY.md -->
        <div class="panel memory">
            <div class="panel-header">
                <div class="panel-title"><span>🟣</span> MEMORY.md</div>
                <div class="panel-dots"><span class="pd"></span><span class="pd"></span><span class="pd"></span></div>
            </div>
            <div class="panel-body" id="memoryLog">読み込み中...</div>
        </div>

        <!-- HEARTBEAT ROW: digest (left) + state (right) -->
        <div class="panel heartbeat-wrap">
            <div class="panel digest">
                <div class="panel-header">
                    <div class="panel-title"><span>🟢</span> heartbeat-digest.md</div>
                    <div class="panel-dots"><span class="pd"></span><span class="pd"></span><span class="pd"></span></div>
                </div>
                <div class="panel-body" id="hbDigest">読み込み中...</div>
            </div>
            <div class="panel hb-state">
                <div class="panel-header">
                    <div class="panel-title"><span>🟡</span> heartbeat-state.json</div>
                    <div class="panel-dots"><span class="pd"></span><span class="pd"></span><span class="pd"></span></div>
                </div>
                <div class="panel-body" id="hbState">読み込み中...</div>
            </div>
        </div>

        <!-- CLOUDFLARE NEURONS QUOTA -->
        <div class="panel neurons" style="background: rgba(251,191,36,0.02); border: 1px solid rgba(251,191,36,0.15); border-radius: 12px; margin-bottom: 16px; padding: 12px; box-shadow: 0 4px 20px rgba(0,0,0,0.15);">
            <div class="panel-header" style="border-bottom: 1px solid rgba(251,191,36,0.1); padding-bottom: 8px; margin-bottom: 10px; display: flex; justify-content: space-between; align-items: center;">
                <div class="panel-title" style="color: #fbbf24; font-weight: bold; font-family: Outfit, sans-serif; font-size: 14px; display: flex; align-items: center; gap: 6px;">
                    <span>⚡</span> Cloudflare Workers AI Quota
                </div>
                <div style="font-size: 10px; color: rgba(251,191,36,0.7); background: rgba(251,191,36,0.1); padding: 2px 6px; border-radius: 4px; font-weight: bold; border: 1px solid rgba(251,191,36,0.2);">
                    FREE TIER
                </div>
            </div>
            <div class="panel-body" id="neuronQuota" style="display: flex; flex-direction: column; gap: 10px; font-family: Outfit, sans-serif;">
                <div style="display: flex; justify-content: space-between; font-size: 13px; color: var(--text);">
                    <span>本日使用済み:</span>
                    <strong style="color: #fbbf24; font-size: 14px;" id="neuronsUsed">0.00 / 10,000.0 Neurons</strong>
                </div>
                <div style="background: rgba(255,255,255,0.05); border-radius: 6px; height: 10px; width: 100%; overflow: hidden; border: 1px solid rgba(251,191,36,0.1);">
                    <div id="neuronsProgress" style="background: linear-gradient(90deg, #fbbf24, #f59e0b); height: 100%; width: 0%; transition: width 0.8s cubic-bezier(0.4, 0, 0.2, 1);"></div>
                </div>
                <div style="display: flex; justify-content: space-between; font-size: 11px; color: var(--muted); margin-top: 2px;">
                    <span id="neuronsRemaining">残り: 10,000.0 Neurons</span>
                    <span id="neuronsReset">次回リセットまで: 0h 0m</span>
                </div>
            </div>
        </div>

        <!-- QUEUE STATE -->
        <div class="panel queue">
            <div class="panel-header">
                <div class="panel-title"><span>🔄</span> gmn API Pipeline Queue</div>
                <div class="panel-dots"><span class="pd"></span><span class="pd"></span><span class="pd"></span></div>
            </div>
            <div class="panel-body" id="queueState" style="overflow-y: auto;">読み込み中...</div>
        </div>

        <!-- APP LOG -->
        <div class="panel applog">
            <div class="panel-header">
                <div class="panel-title"><span>🔵</span> rustyclaw.log</div>
                <div class="panel-dots"><span class="pd"></span><span class="pd"></span><span class="pd"></span></div>
            </div>
            <div class="panel-body" id="appLog">読み込み中...</div>
        </div>

    </div><!-- /right-col -->
</main>

<script>
    const chatMessages = document.getElementById('chatMessages');
    const chatInput    = document.getElementById('chatInput');
    const sendBtn      = document.getElementById('sendBtn');

    // ── チャット ─────────────────────────────────────────────
    async function sendMessage() {
        const message = chatInput.value.trim();
        if (!message) return;

        appendMessage(message, 'user');
        chatInput.value = '';

        const loadingId = appendLoading();
        chatInput.disabled = true;
        sendBtn.disabled   = true;

        try {
            const res = await fetch('/chat', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ message })
            });
            removeLoading(loadingId);
            appendMessage(res.ok ? await res.text() : 'エラー: 返答の取得に失敗しました。', 'assistant');
        } catch {
            removeLoading(loadingId);
            appendMessage('通信エラー: Gateway と通信できません。', 'assistant');
        } finally {
            chatInput.disabled = false;
            sendBtn.disabled   = false;
            chatInput.focus();
        }
    }

    function handleKey(e) {
        if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
    }

    function appendMessage(text, role) {
        const d = document.createElement('div');
        d.className = `message ${role}`;
        d.innerHTML = text.replace(/\n/g, '<br>');
        chatMessages.appendChild(d);
        chatMessages.scrollTop = chatMessages.scrollHeight;
    }

    function appendLoading() {
        const id  = 'loading-' + Date.now();
        const el  = document.createElement('div');
        el.className = 'loading-dots'; el.id = id;
        el.innerHTML = '<span class="dot"></span><span class="dot"></span><span class="dot"></span>';
        chatMessages.appendChild(el);
        chatMessages.scrollTop = chatMessages.scrollHeight;
        return id;
    }

    function removeLoading(id) {
        const el = document.getElementById(id);
        if (el) el.remove();
    }

    // ── ログポーリング ────────────────────────────────────────
    function updatePanel(el, text) {
        const atBottom = el.scrollHeight - el.clientHeight <= el.scrollTop + 60;
        el.textContent = text;
        if (atBottom) el.scrollTop = el.scrollHeight;
    }

    const memoryLog = document.getElementById('memoryLog');
    const hbDigest  = document.getElementById('hbDigest');
    const hbState   = document.getElementById('hbState');
    const appLog    = document.getElementById('appLog');

    // MEMORY.md / heartbeat 系は変化が緩やかなので 5 秒間隔で十分
    async function fetchSlowLogs() {
        try {
            const [rMem, rDigest, rState] = await Promise.all([
                fetch('/logs/memory'),
                fetch('/logs/heartbeat-digest'),
                fetch('/logs/heartbeat-state'),
            ]);
            if (rMem.ok)    updatePanel(memoryLog, await rMem.text());
            if (rDigest.ok) updatePanel(hbDigest,  await rDigest.text());
            if (rState.ok)  updatePanel(hbState,   await rState.text());
        } catch { /* ネットワーク断は無視 */ }
    }

    // App ログは 2 秒間隔でポーリング
    async function fetchAppLog() {
        try {
            const r = await fetch('/logs/app');
            if (r.ok) updatePanel(appLog, await r.text());
        } catch { /* 無視 */ }
    }

    async function updateQueue() {
        try {
            const res = await fetch('/api/queue');
            if (!res.ok) return;
            const items = await res.json();
            
            const container = document.getElementById('queueState');
            if (items.length === 0) {
                container.innerHTML = '<div style="color: var(--muted); text-align: center; padding: 12px; font-family: Outfit, sans-serif;">キューは空です（稼働タスクなし）</div>';
                return;
            }
            
            let html = '<div style="display: flex; flex-direction: column; gap: 8px;">';
            items.forEach((item, index) => {
                let badgeColor = 'var(--muted)';
                let pulseStyle = '';
                let progressHtml = '';
                
                if (item.status === 'Executing') {
                    badgeColor = 'var(--green)';
                    pulseStyle = 'background: var(--green); box-shadow: 0 0 8px var(--green);';
                } else if (item.status === 'Waiting') {
                    badgeColor = 'var(--blue)';
                    pulseStyle = 'background: var(--blue); box-shadow: 0 0 8px var(--blue); animation: pulse 1.5s infinite;';
                } else if (item.status === 'Cooldown') {
                    badgeColor = 'var(--amber)';
                    pulseStyle = 'background: var(--amber); box-shadow: 0 0 8px var(--amber);';
                    
                    // プログレスバー
                    const totalDur = 47.0;
                    const pct = Math.min(100, Math.max(0, (item.cooldown_left_secs / totalDur) * 100));
                    progressHtml = `
                        <div style="background: rgba(255,255,255,0.05); border-radius: 4px; height: 5px; width: 100%; margin-top: 6px; overflow: hidden; border: 1px solid rgba(251,191,36,0.15);">
                            <div style="background: var(--amber); height: 100%; width: ${pct}%; transition: width 0.5s ease;"></div>
                        </div>
                    `;
                }
                
                const elapsedSecs = Math.floor((Date.now() - item.enqueued_at_ms) / 1000);
                const elapsedText = elapsedSecs >= 0 ? `${elapsedSecs}s` : '0s';
                
                const subText = item.status === 'Cooldown'
                    ? `解除まで: ${item.cooldown_left_secs.toFixed(1)}s`
                    : `待機時間: ${elapsedText}`;
                
                html += `
                    <div style="background: rgba(255,255,255,0.02); border: 1px solid var(--border); border-radius: 8px; padding: 10px; display: flex; flex-direction: column; gap: 4px;">
                        <div style="display: flex; align-items: center; justify-content: space-between;">
                            <div style="display: flex; align-items: center; font-weight: 600;">
                                <span style="width: 8px; height: 8px; border-radius: 50%; display: inline-block; margin-right: 8px; ${pulseStyle}"></span>
                                <span style="font-size: 13px; font-family: 'Fira Code', monospace; color: var(--text);">${item.session_id}</span>
                            </div>
                            <span style="font-size: 11px; font-weight: bold; text-transform: uppercase; color: ${badgeColor}; background: rgba(255,255,255,0.03); padding: 2px 8px; border-radius: 4px; border: 1px solid var(--border);">
                                ${item.status}
                            </span>
                        </div>
                        <div style="font-size: 11px; color: #fbbf24; background: rgba(251,191,36,0.03); padding: 4px 8px; border-radius: 4px; border: 1px solid rgba(251,191,36,0.1); margin-top: 4px; word-break: break-all; font-family: Outfit, sans-serif;">
                            ${item.description || '処理タスクを分析中...'}
                        </div>
                        <div style="display: flex; justify-content: space-between; font-size: 11px; color: var(--muted); margin-top: 2px;">
                            <span>順位: #${index + 1}</span>
                            <span>${subText}</span>
                        </div>
                        ${progressHtml}
                    </div>
                `;
            });
            html += '</div>';
            container.innerHTML = html;
        } catch { /* 無視 */ }
    }

    async function updateNeurons() {
        try {
            const res = await fetch('/api/neurons');
            if (!res.ok) return;
            const data = await res.json();
            
            const used = data.neurons_used || 0.0;
            const limit = data.quota_limit || 10000.0;
            const remaining = data.remaining || 10000.0;
            const resetIn = data.reset_in || '0h 0m';
            
            document.getElementById('neuronsUsed').innerText = `${used.toFixed(2)} / ${limit.toLocaleString()} Neurons`;
            document.getElementById('neuronsRemaining').innerText = `残り: ${remaining.toFixed(2)} Neurons`;
            document.getElementById('neuronsReset').innerText = `次回リセットまで: ${resetIn}`;
            
            const pct = Math.min(100, Math.max(0, (used / limit) * 100));
            document.getElementById('neuronsProgress').style.width = `${pct}%`;
        } catch { /* 無視 */ }
    }

    fetchSlowLogs();
    fetchAppLog();
    updateQueue();
    updateNeurons();
    setInterval(fetchSlowLogs, 5000);
    setInterval(fetchAppLog,   2000);
    setInterval(updateQueue,   2000);
    setInterval(updateNeurons, 5000);
</script>
</body>
</html>
"#.to_string()
}
