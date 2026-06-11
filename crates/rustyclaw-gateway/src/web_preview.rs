use anyhow::{Context, Result};
use axum::Router;
use rustyclaw_config::Config;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;

/// Spawns the web preview server as a background Tokio task.
pub async fn start_web_preview_server(
    config: &Config,
    workspace_path: PathBuf,
) -> Result<(tokio::task::JoinHandle<()>, SocketAddr)> {
    let preview_dir = workspace_path.join("previews");
    std::fs::create_dir_all(&preview_dir).context("Failed to create previews directory")?;

    let host = &config.web_preview.host;
    let port = config.web_preview.port;
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .with_context(|| format!("Failed to parse preview bind address {}:{}", host, port))?;

    let app = Router::new().nest_service("/", ServeDir::new(&preview_dir));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind Web Preview Server to {}", addr))?;

    let bound_addr = listener.local_addr()?;
    tracing::info!("Web Preview Server listening on http://{}", bound_addr);

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Web Preview Server error: {}", e);
        }
    });

    Ok((handle, bound_addr))
}

/// Executes `tailscale ip -4` to fetch the Tailscale IPv4 address.
pub fn get_tailscale_ip() -> Option<String> {
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if output.status.success() {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() {
            return Some(ip);
        }
    }
    None
}

/// Executes `hostname -I` to fetch the local primary IPv4 address.
pub fn get_local_ip() -> Option<String> {
    let output = std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()?;
    if output.status.success() {
        let ips = String::from_utf8_lossy(&output.stdout);
        let first_ip = ips.split_whitespace().next()?.to_string();
        if !first_ip.is_empty() {
            return Some(first_ip);
        }
    }
    None
}

/// Resolves the preview base URL using base_url from config, Tailscale IP, local IP, or localhost.
pub fn get_preview_base_url(base_url_opt: &Option<String>, port: u16) -> String {
    if let Some(base_url) = base_url_opt {
        return base_url.trim_end_matches('/').to_string();
    }
    let host = if let Some(ip) = get_tailscale_ip() {
        ip
    } else if let Some(ip) = get_local_ip() {
        ip
    } else {
        "127.0.0.1".to_string()
    };
    format!("http://{}:{}", host, port)
}
