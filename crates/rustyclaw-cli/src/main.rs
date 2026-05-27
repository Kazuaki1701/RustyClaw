use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use rustyclaw_agent::Pipeline;
use rustyclaw_config::load_config;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing_subscriber::prelude::*;

#[derive(Parser)]
#[command(name = "rustyclaw")]
#[command(about = "RustyClaw - AI Agent Runtime in Rust", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 設定ファイルのパス
    #[arg(short, long, global = true, default_value = "config.json")]
    config: PathBuf,

    /// ワークスペースのパス
    #[arg(short, long, global = true, default_value = "./workspace")]
    workspace: PathBuf,
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
}

/// デバッグログ（tracing）のセットアップを行う
fn setup_logging() -> Result<()> {
    // ログ保存先ディレクトリの作成 (~/.rustyclaw/)
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let log_dir = home_dir.join(".rustyclaw");
    std::fs::create_dir_all(&log_dir)?;

    // ファイルローリングアペンダーの設定
    let file_appender = tracing_appender::rolling::daily(&log_dir, "rustyclaw.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // tracing-subscriber のレイヤー構成
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let local_time = tracing_subscriber::fmt::time::ChronoLocal::new("%Y-%m-%dT%H:%M:%S%.3f%z".to_string());

    // 標準出力へのレイヤー（デバッグ用に標準出力にも出す）
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_timer(local_time.clone())
        .with_target(false)
        .with_writer(std::io::stdout);

    // ファイル出力へのレイヤー
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

    // ログ初期化時のメッセージ（ファイル保存先を記録）
    tracing::info!("Logging initialized. Logs are saved to {:?}", log_dir.join("rustyclaw.log"));

    // ガードをスレッドローカルに保持（またはライフサイクル中にリークさせることでプログラム終了までログを維持する）
    Box::leak(Box::new(_guard));

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // ログのセットアップ
    if let Err(e) = setup_logging() {
        eprintln!("Warning: Failed to initialize logging: {}", e);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Agent { message } => {
            // 1. 設定ファイルのロード
            let config = load_config(&cli.config)
                .with_context(|| format!("Failed to load configuration from {:?}", cli.config))?;

            tracing::info!("Configuration loaded successfully: provider={}, model={}", config.model_provider, config.model_name);

            // 2. パイプライン（エージェント）の構築
            let flush_sem = Arc::new(Semaphore::new(1));
            let pipeline = Pipeline::new(config, flush_sem);

            // 3. ストリーミング対話の実行
            tracing::info!("Starting agent interaction in streaming mode...");
            let stream = pipeline.execute_stream(&cli.workspace, "cli-session", &message).await?;
            let mut pin_stream = stream;

            // コンソールへのストリーミング出力
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
            println!(); // 最後に改行を出力
        }
        Commands::Gateway => {
            // Gateway デーモンの起動
            let gateway = rustyclaw_gateway::Gateway::new(&cli.config, &cli.workspace);
            gateway.run().await?;
        }
    }

    Ok(())
}
