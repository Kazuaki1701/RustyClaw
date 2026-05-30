use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use rustyclaw_agent::Pipeline;
use rustyclaw_config::{get_app_dir, load_config};
use rustyclaw_config::vault;
use std::collections::HashMap;
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
    #[arg(short, long, global = true, default_value = "config.json")]
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
    let log_dir = get_app_dir();
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
                let legacy = get_app_dir().join("vault.json");
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
            let json_path = path.unwrap_or_else(|| get_app_dir().join("vault.json"));
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

    // vault コマンドはログ初期化不要
    if let Commands::Vault { command } = cli.command {
        return run_vault(command, &cli.config);
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
            tracing::info!("Configuration loaded successfully: provider={}, model={}", config.model_provider, config.model_name);

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
        Commands::Vault { .. } => unreachable!(),
    }

    Ok(())
}
