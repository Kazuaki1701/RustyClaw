use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use rustyclaw_agent::Pipeline;
use rustyclaw_config::{get_app_dir, get_config_dir, load_config};
use rustyclaw_config::vault;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing_subscriber::prelude::*;

#[derive(Parser)]
#[command(name = "rustyclaw")]
#[command(about = "RustyClaw - AI Agent Runtime in Rust", long_about = None)]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), "\nbuilt at ", env!("BUILD_TIME")))]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 設定ファイルのパス
    #[arg(short, long, global = true, default_value = "config/config.json")]
    config: PathBuf,

    /// ワークスペースのパス
    #[arg(short, long, global = true, default_value = "./workspace")]
    workspace: PathBuf,

    /// デバッグモード: 実際の API 送信を行わず、リクエスト内容をログ出力する
    #[arg(long, global = true)]
    no_agent: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// エージェント対話実行（ストリーミング形式）
    Agent {
        /// 送信するメッセージ内容
        #[arg(short, long)]
        message: String,
    },
    /// Gateway バックグラウンドデーモンの起動
    Gateway,
    /// vault シークレット管理
    Vault {
        #[command(subcommand)]
        command: VaultCommands,
    },
    /// LLM への送受信データを参照する（debug_dump が有効な場合のみ）
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
    /// config.json の内容を点検し、エラー・警告を報告する
    CheckConfig,
}

#[derive(Subcommand)]
enum DebugCommands {
    /// 最後に送信したリクエスト（全メッセージ）を表示する
    Request {
        /// メッセージ本文を全文表示する（省略時は先頭 300 文字）
        #[arg(long)]
        full: bool,
    },
    /// 最後に受信したレスポンスを表示する
    Response,
    /// デバッグダンプファイルを削除する
    Clear,
}

#[derive(Subcommand)]
enum VaultCommands {
    /// シークレットを保存する（echo-off 入力）
    Set {
        /// シークレットのキー名
        key: String,
    },
    /// シークレットの値を stdout に出力する
    Get {
        /// シークレットのキー名
        key: String,
    },
    /// 保存済みキー一覧を表示する（値は非表示）
    List,
    /// シークレットを削除する
    Delete {
        /// シークレットのキー名
        key: String,
    },
    /// vault の状態（バックエンド・件数）を表示する
    Status,
    /// config.json の平文トークンを vault に移行する
    Migrate {
        /// config.json 内のドット記法パス（例: api_key）
        config_key: String,
        /// vault に保存するキー名
        vault_key: String,
    },
    /// 既存の平文 vault.json を vault.enc に一括移行する
    ImportJson {
        /// 平文 vault.json のパス（省略時: ~/.rustyclaw/vault.json）
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

/// デバッグログ（tracing）のセットアップを行う
fn setup_logging() -> Result<()> {
    let log_dir = get_app_dir().join("logs");
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "rustyclaw.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let local_time = tracing_subscriber::fmt::time::ChronoLocal::new("%Y-%m-%dT%H:%M:%S%.3f%z".to_string());

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_timer(local_time.clone())
        .with_target(false)
        .with_writer(std::io::stdout);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_timer(local_time)
        .with_ansi(false)
        .with_target(true)
        .with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    tracing::info!("Logging initialized. Logs are saved to {:?}", log_dir.join("rustyclaw.log"));
    Box::leak(Box::new(_guard));

    Ok(())
}

/// VAULT_PASSPHRASE 環境変数、または対話的入力でパスフレーズを取得する
fn prompt_passphrase() -> Result<String> {
    if let Ok(p) = vault::resolve_passphrase(None) {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    print!("Vault passphrase (Enter for none): ");
    io::stdout().flush()?;
    read_secret()
}

/// echo-off で1行入力する
fn read_secret() -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        unsafe { libc::tcgetattr(fd, &mut termios); }
        let termios_orig = termios;
        termios.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios); }
        let mut s = String::new();
        let result = io::stdin().read_line(&mut s);
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios_orig); }
        println!();
        result?;
        return Ok(s.trim().to_string());
    }
    #[cfg(not(unix))]
    {
        let mut s = String::new();
        io::stdin().read_line(&mut s)?;
        Ok(s.trim().to_string())
    }
}

fn add_vault_ref(map: &mut HashMap<String, (bool, Vec<String>)>, key: &str, source: String, critical: bool) {
    let entry = map.entry(key.to_string()).or_insert((false, Vec::new()));
    if critical { entry.0 = true; }
    entry.1.push(source);
}

async fn run_check_config(config_path: &PathBuf) -> Result<()> {
    let mut errors = 0usize;
    let mut warnings = 0usize;

    println!("=== Config Check: {} ===\n", config_path.display());

    // ── 1. ファイル読み込み ────────────────────────────────────────────
    let content = match std::fs::read_to_string(config_path) {
        Ok(c) => { println!("[OK]   file readable"); c }
        Err(e) => {
            println!("[ERR]  file: {} — {}", config_path.display(), e);
            println!("\nResult: 1 error(s)");
            return Ok(());
        }
    };

    // ── 2. JSON parse ─────────────────────────────────────────────────
    if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
        println!("[ERR]  JSON parse: {}", e);
        println!("\nResult: 1 error(s)");
        return Ok(());
    }
    println!("[OK]   JSON valid");

    // ── 3. Config schema ──────────────────────────────────────────────
    let config: rustyclaw_config::Config = match serde_json::from_str(&content) {
        Ok(c) => { println!("[OK]   schema valid"); c }
        Err(e) => {
            println!("[ERR]  schema: {}", e);
            println!("\nResult: 1 error(s)");
            return Ok(());
        }
    };

    // ── 4. model_list ────────────────────────────────────────────────
    println!();
    let total = config.model_list.len();
    let enabled_count = config.model_list.iter().filter(|m| m.enabled).count();

    if total == 0 {
        println!("[ERR]  model_list: empty");
        errors += 1;
    } else if enabled_count == 0 {
        println!("[ERR]  model_list: {} entries, 0 enabled — no usable model", total);
        errors += 1;
    } else {
        println!("[OK]   model_list: {} entries, {} enabled", total, enabled_count);
    }

    let mut seen_names: HashSet<&str> = HashSet::new();
    for m in &config.model_list {
        if !seen_names.insert(m.model_name.as_str()) {
            println!("[ERR]  model_name '{}': duplicate", m.model_name);
            errors += 1;
        }
        if m.model.is_empty() {
            println!("[ERR]  model_list[{}].model: empty string", m.model_name);
            errors += 1;
        }
        let base = &m.api_base;
        let is_ref = base.starts_with("$vault:") || base.starts_with("$env:");
        let is_url = base.starts_with("http://") || base.starts_with("https://");
        if !base.is_empty() && !is_ref && !is_url {
            println!("[ERR]  model_list[{}].api_base: not a URL or $ref: {}", m.model_name, base);
            errors += 1;
        }
    }

    // ── 5. agents ────────────────────────────────────────────────────
    println!();
    let all_names: HashSet<&str> = config.model_list.iter()
        .map(|m| m.model_name.as_str()).collect();
    let enabled_names: HashSet<&str> = config.model_list.iter()
        .filter(|m| m.enabled).map(|m| m.model_name.as_str()).collect();

    let check_agent_ref = |purpose: &str, name: &str, errors: &mut usize| {
        if !all_names.contains(name) {
            println!("[ERR]  agents.{:<8} → '{}': not found in model_list", purpose, name);
            *errors += 1;
        } else if !enabled_names.contains(name) {
            println!("[ERR]  agents.{:<8} → '{}': model is disabled", purpose, name);
            *errors += 1;
        } else {
            println!("[OK]   agents.{:<8} → '{}' (enabled)", purpose, name);
        }
    };

    check_agent_ref("default", &config.agents.default.model_name.clone(), &mut errors);
    if let Some(ref s) = config.agents.summary {
        check_agent_ref("summary", &s.model_name.clone(), &mut errors);
    }
    if let Some(ref m) = config.agents.memory {
        check_agent_ref("memory", &m.model_name.clone(), &mut errors);
    }

    // ── 6. vault references ──────────────────────────────────────────
    println!();
    let mut vault_map: HashMap<String, (bool, Vec<String>)> = HashMap::new();

    for m in &config.model_list {
        if let Some(k) = m.api_key.strip_prefix("$vault:") {
            add_vault_ref(&mut vault_map, k, format!("{}.api_key", m.model_name), m.enabled);
        }
        if let Some(k) = m.api_base.strip_prefix("$vault:") {
            add_vault_ref(&mut vault_map, k, format!("{}.api_base", m.model_name), m.enabled);
        }
    }
    if let Some(ref d) = config.channels.discord {
        if let Some(k) = d.token.strip_prefix("$vault:") {
            add_vault_ref(&mut vault_map, k, "channels.discord.token".to_string(), d.enabled);
        }
    }
    if let Some(ref l) = config.channels.line {
        if let Some(k) = l.channel_access_token.strip_prefix("$vault:") {
            add_vault_ref(&mut vault_map, k, "channels.line.channel_access_token".to_string(), l.enabled);
        }
        if let Some(k) = l.channel_secret.strip_prefix("$vault:") {
            add_vault_ref(&mut vault_map, k, "channels.line.channel_secret".to_string(), l.enabled);
        }
    }
    if let Some(ref k) = config.tools.karakeep {
        if let Some(key) = k.api_key.strip_prefix("$vault:") {
            add_vault_ref(&mut vault_map, key, "tools.karakeep.api_key".to_string(), k.enabled);
        }
    }
    if let Some(ref o) = config.tools.obsidian {
        if let Some(key) = o.api_key.strip_prefix("$vault:") {
            add_vault_ref(&mut vault_map, key, "tools.obsidian.api_key".to_string(), o.enabled);
        }
    }
    for (sname, srv) in &config.mcp {
        for (ekey, eval) in &srv.env {
            if let Some(k) = eval.strip_prefix("$vault:") {
                add_vault_ref(&mut vault_map, k, format!("mcp.{}.env.{}", sname, ekey), srv.enabled);
            }
        }
    }

    // vault を読み込み、後続の疎通確認でも使う
    let secrets: HashMap<String, String> = if vault_map.is_empty() {
        println!("[OK]   vault references: none");
        HashMap::new()
    } else {
        println!("vault references ({} unique key(s)):", vault_map.len());
        let passphrase = prompt_passphrase()?;
        let loaded = vault::load_vault(Some(&passphrase)).unwrap_or_default();

        let mut sorted: Vec<&String> = vault_map.keys().collect();
        sorted.sort();
        for key in sorted {
            let (critical, sources) = &vault_map[key];
            if loaded.contains_key(key.as_str()) {
                println!("[OK]   {} ({})", key, sources.join(", "));
            } else if *critical {
                println!("[ERR]  {} — not found in vault ({})", key, sources.join(", "));
                errors += 1;
            } else {
                println!("[WARN] {} — not found in vault; all referencing models disabled ({})", key, sources.join(", "));
                warnings += 1;
            }
        }
        loaded
    };

    // $vault:KEY / $env:KEY → 実値に解決するヘルパー
    let resolve = |val: &str| -> String {
        if let Some(k) = val.strip_prefix("$vault:") {
            secrets.get(k).cloned().unwrap_or_else(|| val.to_string())
        } else if let Some(k) = val.strip_prefix("$env:") {
            std::env::var(k).unwrap_or_else(|_| val.to_string())
        } else {
            val.to_string()
        }
    };

    // ── 7. api_base 疎通確認 ─────────────────────────────────────────
    println!();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // enabled モデルのユニーク (api_base_resolved, api_key_resolved) ペア
    let mut seen_bases: HashSet<String> = HashSet::new();
    let enabled_models: Vec<_> = config.model_list.iter().filter(|m| m.enabled).collect();

    if enabled_models.is_empty() {
        println!("api_base 疎通確認: (enabled model なし)");
    } else {
        println!("api_base 疎通確認:");
        for m in &enabled_models {
            let base = resolve(&m.api_base);
            if !seen_bases.insert(base.clone()) { continue; }
            let url = format!("{}/models", base.trim_end_matches('/'));
            let api_key = resolve(&m.api_key);
            match http.get(&url).bearer_auth(&api_key).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() || status.is_client_error() {
                        // 4xx はサーバー到達済み（認証エラー等は想定内）
                        println!("[OK]   {} → {}", base, status.as_u16());
                    } else {
                        println!("[WARN] {} → {} (unexpected status)", base, status.as_u16());
                        warnings += 1;
                    }
                }
                Err(e) if e.is_timeout() => {
                    println!("[ERR]  {} — timeout (5s)", base);
                    errors += 1;
                }
                Err(e) => {
                    println!("[ERR]  {} — {}", base, e);
                    errors += 1;
                }
            }
        }
    }

    // ── 8. tools 疎通確認 ────────────────────────────────────────────
    println!();
    println!("tools 疎通確認:");

    // karakeep
    match config.tools.karakeep.as_ref() {
        Some(k) if k.enabled => {
            let url = resolve(&k.server_addr);
            match http.get(&url).send().await {
                Ok(resp) if resp.status().as_u16() < 500 =>
                    println!("[OK]   karakeep: {} → {}", url, resp.status().as_u16()),
                Ok(resp) => {
                    println!("[WARN] karakeep: {} → {} (server error)", url, resp.status().as_u16());
                    warnings += 1;
                }
                Err(e) if e.is_timeout() => {
                    println!("[ERR]  karakeep: {} — timeout", url);
                    errors += 1;
                }
                Err(e) => {
                    println!("[ERR]  karakeep: {} — {}", url, e);
                    errors += 1;
                }
            }
        }
        Some(_) => println!("[SKIP] karakeep: disabled"),
        None    => println!("[SKIP] karakeep: not configured"),
    }

    // obsidian
    match config.tools.obsidian.as_ref() {
        Some(o) if o.enabled => {
            let url = format!("http://{}:27123", resolve(&o.host));
            match http.get(&url).send().await {
                Ok(resp) if resp.status().as_u16() < 500 =>
                    println!("[OK]   obsidian: {} → {}", url, resp.status().as_u16()),
                Ok(resp) => {
                    println!("[WARN] obsidian: {} → {} (server error)", url, resp.status().as_u16());
                    warnings += 1;
                }
                Err(e) if e.is_timeout() => {
                    println!("[ERR]  obsidian: {} — timeout", url);
                    errors += 1;
                }
                Err(e) => {
                    println!("[ERR]  obsidian: {} — {}", url, e);
                    errors += 1;
                }
            }
        }
        Some(_) => println!("[SKIP] obsidian: disabled"),
        None    => println!("[SKIP] obsidian: not configured"),
    }

    // google-workspace: gws コマンドの存在確認
    match config.tools.google_workspace.as_ref() {
        Some(g) if g.enabled => {
            let gws_path = g.gws_path.as_deref().unwrap_or("gws");
            let cmd_found = std::process::Command::new("which")
                .arg(gws_path)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if cmd_found {
                println!("[OK]   google-workspace: gws command found ({})", gws_path);
            } else {
                // デフォルト探索パスも試す
                let default_paths = ["/home/pi/.local/bin/gws", "/home/kazuaki/.local/bin/gws", "/usr/local/bin/gws"];
                let found = default_paths.iter().any(|p| std::path::Path::new(p).exists());
                if found {
                    println!("[OK]   google-workspace: gws command found (default path)");
                } else {
                    println!("[ERR]  google-workspace: gws command not found");
                    errors += 1;
                }
            }
        }
        Some(_) => println!("[SKIP] google-workspace: disabled"),
        None    => println!("[SKIP] google-workspace: not configured"),
    }

    // MCP stdio サーバー（enabled なものがあれば command 確認）
    let enabled_mcp: Vec<_> = config.mcp.iter().filter(|(_, s)| s.enabled).collect();
    if !enabled_mcp.is_empty() {
        println!("MCP stdio 疎通確認 ({} server(s)):", enabled_mcp.len());
        for (name, srv) in &enabled_mcp {
            let cmd_exists = std::process::Command::new("which")
                .arg(&srv.command)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if cmd_exists {
                println!("[OK]   {}: command '{}' found", name, srv.command);
            } else {
                println!("[ERR]  {}: command '{}' not found in PATH", name, srv.command);
                errors += 1;
            }
        }
    }

    // ── 9. API 疎通確認 ──────────────────────────────────────────────
    println!();
    if enabled_models.is_empty() {
        println!("API 疎通確認: (enabled model なし)");
    } else {
        println!("API 疎通確認 ({} model(s)):", enabled_models.len());
        let http_api = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        for m in &enabled_models {
            let base = resolve(&m.api_base);
            let api_key = resolve(&m.api_key);
            let url = format!("{}/chat/completions", base.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": m.model,
                "messages": [{"role": "user", "content": "Hi"}],
                "max_tokens": 1,
                "stream": false
            });
            let start = std::time::Instant::now();
            match http_api.post(&url).bearer_auth(&api_key).json(&body).send().await {
                Ok(resp) => {
                    let elapsed = start.elapsed().as_millis();
                    let status = resp.status();
                    if status.is_success() {
                        println!("[OK]   {} ({}) → {} ({}ms)", m.model_name, m.model, status.as_u16(), elapsed);
                    } else {
                        let body_text = resp.text().await.unwrap_or_default();
                        let hint = if status.as_u16() == 401 || status.as_u16() == 403 {
                            "auth error — API key invalid?"
                        } else if status.as_u16() == 404 {
                            "model not found"
                        } else if status.as_u16() == 429 {
                            "rate limit / quota exceeded"
                        } else {
                            "API error"
                        };
                        println!("[ERR]  {} ({}) → {} {} — {}", m.model_name, m.model, status.as_u16(), hint, body_text.chars().take(120).collect::<String>());
                        errors += 1;
                    }
                }
                Err(e) if e.is_timeout() => {
                    println!("[ERR]  {} — timeout (15s)", m.model_name);
                    errors += 1;
                }
                Err(e) => {
                    println!("[ERR]  {} — {}", m.model_name, e);
                    errors += 1;
                }
            }
        }
    }

    // ── Summary ──────────────────────────────────────────────────────
    println!();
    if errors == 0 && warnings == 0 {
        println!("Result: All checks passed.");
    } else {
        println!("Result: {} error(s), {} warning(s)", errors, warnings);
    }

    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn run_debug(cmd: DebugCommands, workspace: &PathBuf) -> Result<()> {
    let debug_dir = workspace.join("memory").join("debug");

    match cmd {
        DebugCommands::Request { full } => {
            let path = debug_dir.join("last_request.json");
            if !path.exists() {
                anyhow::bail!("No debug dump found at {}. Enable debug_dump in config.json.", path.display());
            }
            let content = std::fs::read_to_string(&path)?;
            let messages: Vec<serde_json::Value> = serde_json::from_str(&content)?;
            let meta = std::fs::metadata(&path)?;
            let modified: chrono::DateTime<chrono::Local> = meta.modified()?.into();

            println!("=== Last Request ({}) ===", modified.format("%Y-%m-%dT%H:%M:%S%z"));
            println!("Messages: {}", messages.len());
            println!();

            for (i, msg) in messages.iter().enumerate() {
                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let char_count = content.chars().count();

                if full {
                    println!("[{}] {} ({} chars)", i, role.to_uppercase(), char_count);
                    println!("{}", content);
                } else {
                    let preview: String = content.chars().take(300).collect();
                    let ellipsis = if char_count > 300 { format!("… (+{} chars)", char_count - 300) } else { String::new() };
                    println!("[{}] {} ({} chars)", i, role.to_uppercase(), char_count);
                    println!("{}{}", preview, ellipsis);
                }
                println!();
            }
        }

        DebugCommands::Response => {
            let path = debug_dir.join("last_response.json");
            if !path.exists() {
                anyhow::bail!("No debug dump found at {}. Enable debug_dump in config.json.", path.display());
            }
            let content = std::fs::read_to_string(&path)?;
            let json: serde_json::Value = serde_json::from_str(&content)?;
            let meta = std::fs::metadata(&path)?;
            let modified: chrono::DateTime<chrono::Local> = meta.modified()?.into();

            println!("=== Last Response ({}) ===", modified.format("%Y-%m-%dT%H:%M:%S%z"));
            if let Some(role) = json.get("role").and_then(|v| v.as_str()) {
                println!("Role: {}", role);
            }
            println!();
            if let Some(text) = json.get("content").and_then(|v| v.as_str()) {
                println!("{}", text);
            } else {
                println!("{}", serde_json::to_string_pretty(&json)?);
            }
        }

        DebugCommands::Clear => {
            let req = debug_dir.join("last_request.json");
            let res = debug_dir.join("last_response.json");
            let mut removed = 0;
            for p in [&req, &res] {
                if p.exists() {
                    std::fs::remove_file(p)?;
                    println!("Removed: {}", p.display());
                    removed += 1;
                }
            }
            if removed == 0 {
                println!("No debug dump files found.");
            }
        }
    }
    Ok(())
}

fn run_vault(cmd: VaultCommands, config_path: &PathBuf) -> Result<()> {
    match cmd {
        VaultCommands::Set { key } => {
            let passphrase = prompt_passphrase()?;
            let mut secrets = vault::load_vault(Some(&passphrase)).unwrap_or_default();
            print!("Enter value for '{}': ", key);
            io::stdout().flush()?;
            let value = read_secret()?;
            secrets.insert(key.clone(), value);
            vault::save_vault(&secrets, &passphrase)?;
            println!("Secret '{}' stored in {}.", key, vault::get_vault_path().display());
        }

        VaultCommands::Get { key } => {
            let passphrase = prompt_passphrase()?;
            let secrets = vault::load_vault(Some(&passphrase))?;
            match secrets.get(&key) {
                Some(v) => println!("{}", v),
                None => anyhow::bail!("Secret '{}' not found in vault", key),
            }
        }

        VaultCommands::List => {
            let passphrase = prompt_passphrase()?;
            let secrets = vault::load_vault(Some(&passphrase))?;
            if secrets.is_empty() {
                println!("(vault is empty)");
            } else {
                let mut keys: Vec<_> = secrets.keys().collect();
                keys.sort();
                for k in keys {
                    println!("{}", k);
                }
            }
        }

        VaultCommands::Delete { key } => {
            let passphrase = prompt_passphrase()?;
            let mut secrets = vault::load_vault(Some(&passphrase))?;
            if secrets.remove(&key).is_some() {
                vault::save_vault(&secrets, &passphrase)?;
                println!("Secret '{}' deleted.", key);
            } else {
                println!("Secret '{}' not found.", key);
            }
        }

        VaultCommands::Status => {
            let path = vault::get_vault_path();
            if !path.exists() {
                println!("Backend : File (AES-256-GCM)");
                println!("Path    : {}", path.display());
                println!("Status  : Not initialized");
                println!("Secrets : 0");

                // 平文 vault.json が同ディレクトリに残っている場合は移行を促す
                let legacy = get_config_dir().join("vault.json");
                if legacy.exists() {
                    println!();
                    println!("⚠  Legacy vault.json found at {}", legacy.display());
                    println!("   Run `rustyclaw vault import-json` to migrate to encrypted vault.");
                    println!("   Then remove: rm {}", legacy.display());
                }
            } else {
                let passphrase = prompt_passphrase()?;
                match vault::load_vault(Some(&passphrase)) {
                    Ok(secrets) => {
                        println!("Backend : File (AES-256-GCM)");
                        println!("Path    : {}", path.display());
                        println!("Status  : OK");
                        println!("Secrets : {}", secrets.len());
                    }
                    Err(e) => {
                        println!("Backend : File (AES-256-GCM)");
                        println!("Path    : {}", path.display());
                        println!("Status  : ERROR — {}", e);
                    }
                }
            }
        }

        VaultCommands::Migrate { config_key, vault_key } => {
            let content = std::fs::read_to_string(config_path)
                .with_context(|| format!("Failed to read {:?}", config_path))?;
            let mut json: serde_json::Value = serde_json::from_str(&content)?;

            // ドット記法でキーを辿る
            let parts: Vec<&str> = config_key.split('.').collect();
            let plain_val = {
                let mut cur = &json;
                let mut found = None;
                for (i, part) in parts.iter().enumerate() {
                    if i == parts.len() - 1 {
                        found = cur.get(part).and_then(|v| v.as_str()).map(|s| s.to_string());
                    } else {
                        match cur.get(part) {
                            Some(next) => cur = next,
                            None => break,
                        }
                    }
                }
                found
            };

            let plain_val = match plain_val {
                Some(v) if v.starts_with("$vault:") => {
                    println!("'{}' is already a vault reference: {}", config_key, v);
                    return Ok(());
                }
                Some(v) => v,
                None => anyhow::bail!("Key '{}' not found in config.json or is not a string", config_key),
            };

            let passphrase = prompt_passphrase()?;
            let mut secrets = vault::load_vault(Some(&passphrase)).unwrap_or_default();
            secrets.insert(vault_key.clone(), plain_val);
            vault::save_vault(&secrets, &passphrase)?;

            // config.json の値を $vault: 参照に書き換え
            {
                let mut cur = &mut json;
                for (i, part) in parts.iter().enumerate() {
                    if i == parts.len() - 1 {
                        cur[part] = serde_json::Value::String(format!("$vault:{}", vault_key));
                    } else {
                        cur = &mut cur[*part];
                    }
                }
            }
            let serialized = serde_json::to_string_pretty(&json)?;
            std::fs::write(config_path, serialized)?;
            println!("Migrated '{}' → vault key '{}' and updated config.json.", config_key, vault_key);
        }

        VaultCommands::ImportJson { path } => {
            let json_path = path.unwrap_or_else(|| get_config_dir().join("vault.json"));
            if !json_path.exists() {
                anyhow::bail!("vault.json not found: {}", json_path.display());
            }
            let passphrase = prompt_passphrase()?;
            let count = vault::import_from_json(&json_path, &passphrase)?;
            println!("Imported {} secret(s) from {} → {}.", count, json_path.display(), vault::get_vault_path().display());
            println!("You can now remove the plaintext file: rm {}", json_path.display());
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // vault / debug / check-config コマンドはログ初期化不要
    if let Commands::Vault { command } = cli.command {
        return run_vault(command, &cli.config);
    }
    if let Commands::Debug { command } = cli.command {
        return run_debug(command, &cli.workspace);
    }
    if let Commands::CheckConfig = cli.command {
        return run_check_config(&cli.config).await;
    }

    // --no-agent フラグを環境変数に伝播（Gateway 内の create_provider にも届く）
    if cli.no_agent {
        unsafe { std::env::set_var("RUSTYCLAW_NO_AGENT", "1"); }
        eprintln!("[NO-AGENT] Debug mode enabled — API calls suppressed");
    }

    if let Err(e) = setup_logging() {
        eprintln!("Warning: Failed to initialize logging: {}", e);
    }

    match cli.command {
        Commands::Agent { message } => {
            let config = load_config(&cli.config)
                .with_context(|| format!("Failed to load configuration from {:?}", cli.config))?;
            {
                let m = config.get_model("default");
                tracing::info!("Configuration loaded successfully: provider={}, model={}", m.model_provider, m.model_name);
            }

            let flush_sem = Arc::new(Semaphore::new(1));
            let pipeline = Pipeline::new(config, flush_sem);
            tracing::info!("Starting agent interaction in streaming mode...");
            let stream = pipeline.execute_stream(&cli.workspace, "cli-session", &message).await?;
            let mut pin_stream = stream;

            let mut stdout = std::io::stdout();
            while let Some(chunk_res) = pin_stream.next().await {
                match chunk_res {
                    Ok(chunk) => {
                        print!("{}", chunk.content);
                        let _ = std::io::Write::flush(&mut stdout);
                    }
                    Err(e) => {
                        tracing::error!("Error during LLM stream generation: {}", e);
                        eprintln!("\n[Error in LLM stream]: {}", e);
                        break;
                    }
                }
            }
            println!();
        }
        Commands::Gateway => {
            let gateway = rustyclaw_gateway::Gateway::new(&cli.config, &cli.workspace);
            gateway.run().await?;
        }
        Commands::Vault { .. } | Commands::Debug { .. } | Commands::CheckConfig => unreachable!(),
    }

    Ok(())
}
