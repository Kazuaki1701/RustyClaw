use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use rustyclaw_tools::{Tool, ToolResult};
use rustyclaw_config::McpServerConfig;

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>, // Can be number or string or null
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[allow(dead_code)]
    data: Option<Value>,
}

/// Individual MCP server instance connection via standard I/O (stdio)
pub struct ServerConnection {
    name: String,
    #[allow(dead_code)]
    config: McpServerConfig,
    child: Arc<RwLock<Option<Child>>>,
    tx: mpsc::Sender<String>,
    pending_requests: Arc<RwLock<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, anyhow::Error>>>>>,
    next_id: AtomicU64,
}

impl ServerConnection {
    pub async fn new(name: &str, config: McpServerConfig) -> Result<Self, anyhow::Error> {
        let (tx, mut rx) = mpsc::channel::<String>(100);
        let pending_requests: Arc<RwLock<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, anyhow::Error>>>>> = Arc::new(RwLock::new(HashMap::new()));
        let pending_clone = pending_requests.clone();

        tracing::info!("Spawning MCP server process: {} (command: {})", name, config.command);
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // Capture stderr for diagnostic clarity
            .spawn()?;

        let mut stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to capture stdin of {}", name))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to capture stdout of {}", name))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow::anyhow!("Failed to capture stderr of {}", name))?;

        // 1. Task to write outgoing JSON-RPC requests to the subprocess's standard input
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let mut data = msg.as_bytes().to_vec();
                data.push(b'\n');
                if stdin.write_all(&data).await.is_err() || stdin.flush().await.is_err() {
                    tracing::error!("Failed to write message to MCP server's stdin");
                    break;
                }
            }
        });

        // 2. Task to read standard error (stderr) for diagnostic logging
        let name_clone = name.to_string();
        tokio::spawn(async move {
            let mut err_reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = err_reader.next_line().await {
                tracing::debug!("[MCP Server {} stderr] {}", name_clone, line);
            }
        });

        // 3. Task to read incoming JSON-RPC responses from the subprocess's standard output
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!("[MCP Server incoming] {}", line);
                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                    if let Some(id_val) = resp.id {
                        let id_u64 = id_val.as_u64();
                        if let Some(id) = id_u64 {
                            let mut pending = pending_clone.write().await;
                            if let Some(tx) = pending.remove(&id) {
                                if let Some(err) = resp.error {
                                    let _ = tx.send(Err(anyhow::anyhow!("JSON-RPC Error ({}): {}", err.code, err.message)));
                                } else {
                                    let _ = tx.send(Ok(resp.result.unwrap_or(Value::Null)));
                                }
                            }
                        }
                    }
                }
            }
        });

        let conn = Self {
            name: name.to_string(),
            config,
            child: Arc::new(RwLock::new(Some(child))),
            tx,
            pending_requests,
            next_id: AtomicU64::new(1),
        };

        // Perform standard MCP handshake
        conn.handshake().await?;

        Ok(conn)
    }

    async fn handshake(&self) -> Result<(), anyhow::Error> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "initialize".to_string(),
            params: serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "rustyclaw-mcp",
                    "version": "0.1.0"
                }
            }),
        };

        let (otx, orx) = tokio::sync::oneshot::channel();
        self.pending_requests.write().await.insert(id, otx);
        
        let json_str = serde_json::to_string(&req)?;
        tracing::debug!("[MCP Server outgoing] {}", json_str);
        self.tx.send(json_str).await?;

        // Await handshake result with a 10s timeout
        let _result = tokio::time::timeout(std::time::Duration::from_secs(10), orx).await???;

        // Send initialized notification
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        
        let notification_str = serde_json::to_string(&notification)?;
        tracing::debug!("[MCP Server outgoing] {}", notification_str);
        self.tx.send(notification_str).await?;
        
        tracing::info!("MCP server {} handshake successfully completed.", self.name);
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<Value>, anyhow::Error> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "tools/list".to_string(),
            params: serde_json::json!({}),
        };

        let (otx, orx) = tokio::sync::oneshot::channel();
        self.pending_requests.write().await.insert(id, otx);
        
        let json_str = serde_json::to_string(&req)?;
        tracing::debug!("[MCP Server outgoing] {}", json_str);
        self.tx.send(json_str).await?;

        let res = tokio::time::timeout(std::time::Duration::from_secs(10), orx).await???;
        let tools = res["tools"].as_array().cloned().unwrap_or_default();
        Ok(tools)
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, anyhow::Error> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": name,
                "arguments": arguments
            }),
        };

        let (otx, orx) = tokio::sync::oneshot::channel();
        self.pending_requests.write().await.insert(id, otx);
        
        let json_str = serde_json::to_string(&req)?;
        tracing::debug!("[MCP Server outgoing] {}", json_str);
        self.tx.send(json_str).await?;

        let res = tokio::time::timeout(std::time::Duration::from_secs(30), orx).await???;
        Ok(res)
    }

    pub async fn close(&self) {
        let mut guard = self.child.write().await;
        if let Some(mut child) = guard.take() {
            tracing::info!("Stopping MCP server process: {}", self.name);
            let _ = child.kill().await;
        }
    }
}

/// Dynamic proxy tool representing an external tool hosted by an MCP server
pub struct McpTool {
    #[allow(dead_code)]
    server_name: String,
    tool_name: String,
    description: String,
    input_schema: Value,
    connection: Arc<ServerConnection>,
}

impl McpTool {
    pub fn new(
        server_name: &str,
        tool_name: &str,
        description: &str,
        input_schema: Value,
        connection: Arc<ServerConnection>,
    ) -> Self {
        Self {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            input_schema,
            connection,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, args: Value) -> ToolResult {
        match self.connection.call_tool(&self.tool_name, args).await {
            Ok(res) => {
                let mut content_text = String::new();
                if let Some(arr) = res["content"].as_array() {
                    for block in arr {
                        if block["type"] == "text" {
                            if let Some(text) = block["text"].as_str() {
                                content_text.push_str(text);
                            }
                        }
                    }
                }
                let is_error = res["isError"].as_bool().unwrap_or(false);
                ToolResult {
                    content: content_text,
                    is_error,
                }
            }
            Err(e) => ToolResult {
                content: format!("Error calling tool: {:#}", e),
                is_error: true,
            },
        }
    }
}

/// Unified manager representing a set of connected MCP servers
pub struct McpManager {
    connections: RwLock<HashMap<String, Arc<ServerConnection>>>,
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
        }
    }

    /// Establish connections with all enabled MCP servers in parallel
    pub async fn connect_all(&self, servers: &HashMap<String, McpServerConfig>) -> Result<(), anyhow::Error> {
        for (name, conf) in servers {
            if conf.enabled {
                match ServerConnection::new(name, conf.clone()).await {
                    Ok(conn) => {
                        self.connections.write().await.insert(name.clone(), Arc::new(conn));
                        tracing::info!("Successfully established connection to MCP server: {}", name);
                    }
                    Err(e) => {
                        tracing::error!("Failed to establish connection to MCP server {}: {:#}", name, e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Retrieves all tools from all active server connections
    pub async fn get_tools(&self) -> Vec<Arc<McpTool>> {
        let mut list = Vec::new();
        let guard = self.connections.read().await;
        for (server_name, conn) in guard.iter() {
            match conn.list_tools().await {
                Ok(tools) => {
                    for t in tools {
                        if let Some(name) = t["name"].as_str() {
                            let desc = t["description"].as_str().unwrap_or("");
                            let schema = t["inputSchema"].clone();
                            list.push(Arc::new(McpTool::new(
                                server_name,
                                name,
                                desc,
                                schema,
                                conn.clone(),
                            )));
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to list tools from MCP server {}: {:#}", server_name, e);
                }
            }
        }
        list
    }

    /// Gracefully stops and shuts down all active MCP subprocesses
    pub async fn close_all(&self) {
        let guard = self.connections.read().await;
        for conn in guard.values() {
            conn.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn test_mcp_client_handshake_and_tool_call() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let mock_server_path = temp_dir.path().join("mock_mcp.sh");

        // Write a mock stdio MCP server script that implements newline-delimited JSON-RPC 2.0 handshake, listing, and execution
        let mut f = File::create(&mock_server_path)?;
        f.write_all(b"#!/bin/sh\n\
            while read -r line; do\n\
                if echo \"$line\" | grep -q '\"method\":\"initialize\"'; then\n\
                    id=$(echo \"$line\" | sed -n 's/.*\"id\":\\([0-9]*\\),.*/\\1/p')\n\
                    echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2024-11-05\\\",\\\"capabilities\\\":{},\\\"serverInfo\\\":{\\\"name\\\":\\\"mock\\\",\\\"version\\\":\\\"1.0\\\"}}}\"\n\
                elif echo \"$line\" | grep -q '\"method\":\"tools/list\"'; then\n\
                    id=$(echo \"$line\" | sed -n 's/.*\"id\":\\([0-9]*\\),.*/\\1/p')\n\
                    echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"tools\\\":[{\\\"name\\\":\\\"mock_echo\\\",\\\"description\\\":\\\"Echoes back\\\",\\\"inputSchema\\\":{\\\"type\\\":\\\"object\\\"}}]}}\"\n\
                elif echo \"$line\" | grep -q '\"method\":\"tools/call\"'; then\n\
                    id=$(echo \"$line\" | sed -n 's/.*\"id\":\\([0-9]*\\),.*/\\1/p')\n\
                    echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"content\\\":[{\\\"type\\\":\\\"text\\\",\\\"text\\\":\\\"mock_response_content\\\"}],\\\"isError\\\":false}}\"\n\
                fi\n\
            done\n\
        ")?;
        f.flush()?;
        drop(f);

        // Make executable
        let mut perms = std::fs::metadata(&mock_server_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&mock_server_path, perms)?;

        let config = McpServerConfig {
            enabled: true,
            command: mock_server_path.to_string_lossy().to_string(),
            args: vec![],
            env: HashMap::new(),
        };

        let conn = ServerConnection::new("mock-server", config).await?;
        
        // 1. Verify handshake completes successfully and we can list tools
        let tools = conn.list_tools().await?;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "mock_echo");
        assert_eq!(tools[0]["description"], "Echoes back");

        // 2. Wrap tool and verify execution
        let tool = McpTool::new(
            "mock-server",
            "mock_echo",
            "Echoes back",
            tools[0]["inputSchema"].clone(),
            Arc::new(conn),
        );

        let result = tool.execute(serde_json::json!({})).await;
        assert!(!result.is_error);
        assert_eq!(result.content, "mock_response_content");

        tool.connection.close().await;
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_real_mcp_servers_connectivity() -> anyhow::Result<()> {
        let mut config_path = std::path::PathBuf::from("config.json");
        if !config_path.exists() {
            config_path = std::path::PathBuf::from("../../config.json");
        }
        if !config_path.exists() {
            println!("config.json not found at workspace root or crate parent, skipping real MCP connectivity test.");
            return Ok(());
        }
        let config = rustyclaw_config::load_config(config_path)?;
        if config.mcp.is_empty() {
            println!("No MCP servers configured in config.json.");
            return Ok(());
        }
        let manager = McpManager::new();
        manager.connect_all(&config.mcp).await?;
        let tools = manager.get_tools().await;
        println!("\n==================================================");
        println!("Successfully connected! Found {} tools registered:", tools.len());
        for tool in tools {
            println!("  - [{}] {}: {}", tool.server_name, tool.name(), tool.description());
        }
        println!("==================================================\n");
        manager.close_all().await;
        Ok(())
    }
}
