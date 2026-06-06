mod proto;
mod session;

use anyhow::Result;
use proto::{Config, SummaryProto};
use session::ChatSession;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("summary_proto=info".parse().unwrap()),
        )
        .init();

    let workspace_dir = PathBuf::from(
        std::env::var("WORKSPACE_DIR")
            .unwrap_or_else(|_| "./production/workspace/proto".to_string()),
    );

    let config = Config::from_env()?;
    let session = ChatSession::load(&workspace_dir)?;

    tracing::info!(
        model = %config.main_model,
        base_url = %config.base_url,
        workspace = %workspace_dir.display(),
        summary_loaded = !session.current_summary.is_empty(),
        "SummaryProto starting"
    );

    let proto = SummaryProto::new(config, session);

    let stdin = io::stdin();
    print!("> ");
    io::stdout().flush()?;

    for line in stdin.lock().lines() {
        let input = line?;
        let trimmed = input.trim();

        if trimmed.is_empty() {
            print!("> ");
            io::stdout().flush()?;
            continue;
        }
        if trimmed == "/quit" || trimmed == "/exit" {
            println!("Goodbye!");
            break;
        }

        match proto.chat(trimmed).await {
            Ok(response) => println!("Assistant: {response}"),
            Err(e) => eprintln!("Error: {e:#}"),
        }

        print!("> ");
        io::stdout().flush()?;
    }

    Ok(())
}
