# Plan: Tailscale-Integrated Web Preview Server (Phase 37-2)

This plan outlines the design and implementation for the web preview server in RustyClaw.

## Objectives
1. Implement a non-blocking background HTTP server thread in the gateway daemon (`rustyclaw-gateway`).
2. Serve static files from the `{workspace}/previews/` directory.
3. Automatically determine the base URL using the following fallback order:
   - Configured `base_url` (from config).
   - Tailscale IPv4 address (via `tailscale ip -4`).
   - Local network IP (via `hostname -I`).
   - Loopback address `127.0.0.1`.
4. When writing files under `previews/` via `WorkspaceWriteTool`, append a friendly preview URL to the output message.

## Dependencies to Add
In `crates/rustyclaw-gateway/Cargo.toml`:
* `axum = "0.7"` (for web server routing and request handling)
* `tower-http = { version = "0.5", features = ["fs"] }` (for serving directory static files)

## Configuration Changes
In `crates/rustyclaw-config/src/lib.rs`, add a new configuration struct `WebPreviewConfig`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebPreviewConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    #[serde(default = "default_preview_host")]
    pub host: String,
    #[serde(default = "default_preview_port")]
    pub port: u16,
    #[serde(default)]
    pub base_url: Option<String>,
}
```

## Web Preview Module (`crates/rustyclaw-gateway/src/web_preview.rs`)
Implement the background task that spawns the Axum server.

## IP/Base URL Resolution Helper
Implement `get_tailscale_ip()`, `get_local_ip()`, and `get_preview_base_url()` in `crates/rustyclaw-gateway/src/web_preview.rs`.

## Integrating WorkspaceWriteTool
Modify `WorkspaceWriteTool` in `crates/rustyclaw-tools` to accept `preview_base_url` parameter and format preview URLs on write under `previews/`.

## Testing Plan
1. Add tests in `crates/rustyclaw-tools` verifying that writing to `previews/` returns the expected URL message.
2. Add tests in `crates/rustyclaw-gateway` verifying that the web preview server spawns and serves a test static file correctly.
