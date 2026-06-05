use std::sync::Arc;

// ─── 共有エラー型 ────────────────────────────────────────────────────────────

/// rig_core::tool::Tool::Error の共通実装。すべてのツールが使用する。
#[derive(Debug)]
pub struct ToolCallError(pub String);

impl std::fmt::Display for ToolCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ToolCallError {}

/// A registry that manages active tools as rig-core ToolDyn objects.
/// Stored as Arc so ToolRegistry is Clone (each Arc clone shares the same tool instance).
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: std::collections::HashMap<String, Arc<dyn rig_core::tool::ToolDyn>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: std::collections::HashMap::new() }
    }

    /// Registers a tool. tool.name() is used as the key.
    pub fn register(&mut self, tool: Arc<dyn rig_core::tool::ToolDyn>) {
        self.tools.insert(tool.name(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn rig_core::tool::ToolDyn>> {
        self.tools.get(name)
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Returns tool definitions sorted by name (deterministic order).
    pub async fn tool_definitions(&self) -> Vec<rig_core::completion::ToolDefinition> {
        let mut defs = Vec::with_capacity(self.tools.len());
        for tool in self.tools.values() {
            defs.push(tool.definition(String::new()).await);
        }
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

}



#[derive(serde::Deserialize)]
pub struct CronScheduleArgs {}

/// Dynamic scheduled cron tasks scheduler retriever
#[derive(Clone)]
pub struct CronScheduleTool {
    schedule_fn: std::sync::Arc<dyn Fn() -> serde_json::Value + Send + Sync>,
}

impl CronScheduleTool {
    pub fn new<F>(schedule_fn: F) -> Self
    where
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        Self {
            schedule_fn: std::sync::Arc::new(schedule_fn),
        }
    }
}

impl rig_core::tool::Tool for CronScheduleTool {
    const NAME: &'static str = "get_cron_schedule";
    type Error = ToolCallError;
    type Args = CronScheduleArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition {
        rig_core::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get the upcoming scheduled cron tasks and their calculated next execution times.".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    async fn call(&self, _args: CronScheduleArgs) -> Result<String, ToolCallError> {
        let val = (self.schedule_fn)();
        serde_json::to_string_pretty(&val)
            .map_err(|e| ToolCallError(format!("Failed to serialize schedule: {}", e)))
    }
}

// ─── WorkspaceReadTool ───────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct WorkspaceReadArgs {
    pub path: String,
}

#[derive(Clone)]
pub struct WorkspaceReadTool {
    workspace_path: std::path::PathBuf,
}

impl WorkspaceReadTool {
    pub fn new(workspace_path: std::path::PathBuf) -> Self {
        Self { workspace_path }
    }
}

impl rig_core::tool::Tool for WorkspaceReadTool {
    const NAME: &'static str = "workspace_read";
    type Error = ToolCallError;
    type Args = WorkspaceReadArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition {
        rig_core::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read a text file from the agent workspace (e.g. patrol/state.json, patrol/findings.md).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within workspace (e.g. patrol/state.json)" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: WorkspaceReadArgs) -> Result<String, ToolCallError> {
        let rel = &args.path;
        if rel.contains("..") || rel.starts_with('/') {
            return Err(ToolCallError("Invalid path".into()));
        }
        std::fs::read_to_string(self.workspace_path.join(rel))
            .map_err(|e| ToolCallError(format!("Cannot read {}: {}", rel, e)))
    }
}

// ─── WorkspaceWriteTool ──────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct WorkspaceWriteArgs {
    pub path: String,
    pub content: String,
    pub mode: Option<String>,
}

#[derive(Clone)]
pub struct WorkspaceWriteTool {
    workspace_path: std::path::PathBuf,
}

impl WorkspaceWriteTool {
    pub fn new(workspace_path: std::path::PathBuf) -> Self {
        Self { workspace_path }
    }
}

impl rig_core::tool::Tool for WorkspaceWriteTool {
    const NAME: &'static str = "workspace_write";
    type Error = ToolCallError;
    type Args = WorkspaceWriteArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition {
        rig_core::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Write or append to a text file in the agent workspace (patrol/, memory/logs/ etc.).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path":    { "type": "string", "description": "Relative path within workspace" },
                    "content": { "type": "string", "description": "Text to write" },
                    "mode":    { "type": "string", "description": "\"write\" (overwrite, default) or \"append\"" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: WorkspaceWriteArgs) -> Result<String, ToolCallError> {
        let rel = &args.path;
        if rel.contains("..") || rel.starts_with('/') {
            return Err(ToolCallError("Invalid path: traversal not allowed".into()));
        }
        let full = self.workspace_path.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ToolCallError(format!("mkdir failed: {}", e)))?;
        }
        let result = if args.mode.as_deref() == Some("append") {
            use std::io::Write;
            std::fs::OpenOptions::new().append(true).create(true).open(&full)
                .and_then(|mut f| f.write_all(args.content.as_bytes()))
        } else {
            std::fs::write(&full, args.content.as_bytes())
        };
        result.map(|_| format!("OK: wrote to {}", rel))
              .map_err(|e| ToolCallError(format!("Write failed {}: {}", rel, e)))
    }
}

// ─── MemorySearchTool ────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct MemorySearchArgs {
    pub query: String,
}

#[derive(Clone)]
pub struct MemorySearchTool {
    workspace_path: std::path::PathBuf,
}

impl MemorySearchTool {
    pub fn new(workspace_path: std::path::PathBuf) -> Self {
        Self { workspace_path }
    }
}

impl rig_core::tool::Tool for MemorySearchTool {
    const NAME: &'static str = "memory_search";
    type Error = ToolCallError;
    type Args = MemorySearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition {
        rig_core::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the agent's long-term memory (session summaries) by keyword. Returns file paths of matching summaries.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: MemorySearchArgs) -> Result<String, ToolCallError> {
        let index_dir = self.workspace_path.join("memory").join("index");
        match rustyclaw_storage::SearchIndexManager::new(&index_dir) {
            Ok(mgr) => match mgr.search(&args.query) {
                Ok(paths) if paths.is_empty() => Ok("No results found.".into()),
                Ok(paths) => Ok(paths.iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")),
                Err(e) => Err(ToolCallError(format!("Search failed: {}", e))),
            },
            Err(e) => Err(ToolCallError(format!("Memory index not available (may not have summaries yet): {}", e))),
        }
    }
}

// ─── WebSearchTool ───────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
    #[serde(default = "web_search_default_count")]
    pub count: u64,
}

fn web_search_default_count() -> u64 { 5 }

#[derive(Clone)]
pub struct WebSearchTool {
    api_key: String,
}

impl WebSearchTool {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl rig_core::tool::Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = ToolCallError;
    type Args = WebSearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition {
        rig_core::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the web using Brave Search. Returns titles, URLs, and snippets. Use site: prefix for HN/Reddit (e.g. 'site:news.ycombinator.com rust').".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "count": { "type": "integer", "description": "Number of results (default: 5, max: 10)" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: WebSearchArgs) -> Result<String, ToolCallError> {
        let count = args.count.min(10);
        let client = reqwest::Client::new();
        match client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .query(&[("q", args.query.as_str()), ("count", count.to_string().as_str())])
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<serde_json::Value>().await
                    .map_err(|e| ToolCallError(format!("JSON parse error: {}", e)))
                    .map(|json| {
                        json["web"]["results"]
                            .as_array()
                            .map(|arr| arr.iter().map(|r| {
                                let title = r["title"].as_str().unwrap_or("(no title)");
                                let url   = r["url"].as_str().unwrap_or("");
                                let desc  = r["description"].as_str().unwrap_or("");
                                format!("**{}**\n{}\n{}", title, url, desc)
                            }).collect::<Vec<_>>().join("\n\n"))
                            .unwrap_or_else(|| "No results".to_string())
                    })
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Err(ToolCallError(format!("Brave Search error {}: {}", status, body)))
            }
            Err(e) => Err(ToolCallError(format!("Request failed: {}", e))),
        }
    }
}

// ─── WebFetchTool ────────────────────────────────────────────────────────────

/// HTML タグ・script・style ブロックを除去してプレーンテキストを返す（新規 crate 依存なし）
fn strip_html_to_text(html: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag    = false;
    let mut skip_body = false;
    let mut buf       = String::new();

    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                buf.clear();
            }
            '>' => {
                let tag = buf.trim().to_lowercase();
                let tag_name = tag.split_whitespace().next().unwrap_or("");
                if tag_name == "script" || tag_name == "style" {
                    skip_body = true;
                }
                if tag_name == "/script" || tag_name == "/style" {
                    skip_body = false;
                }
                in_tag = false;
                out.push(' ');
            }
            _ if in_tag => {
                buf.push(c);
            }
            _ if !skip_body => {
                out.push(c);
            }
            _ => {}
        }
        if out.len() >= max_chars {
            break;
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
        .chars().take(max_chars).collect()
}

#[derive(Clone)]
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self { Self }
}

impl Default for WebFetchTool {
    fn default() -> Self { Self::new() }
}

#[derive(serde::Deserialize)]
pub struct WebFetchArgs {
    pub url: String,
    #[serde(default = "web_fetch_default_max_chars")]
    pub max_chars: usize,
}

fn web_fetch_default_max_chars() -> usize { 3000 }

impl rig_core::tool::Tool for WebFetchTool {
    const NAME: &'static str = "web_fetch";
    type Error = ToolCallError;
    type Args = WebFetchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition {
        rig_core::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fetch a web page and return its plain text content (HTML tags stripped). Use after web_search to read full article content.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" },
                    "max_chars": {
                        "type": "integer",
                        "description": "Max characters to return (default: 3000)"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: WebFetchArgs) -> Result<String, ToolCallError> {
        if args.url.is_empty() {
            return Err(ToolCallError("Missing url".into()));
        }
        let max_chars = args.max_chars;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (compatible; RustyClaw/1.0)")
            .build()
            .unwrap_or_default();
        match client.get(&args.url).send().await {
            Ok(resp) if resp.status().is_success() => {
                resp.text().await
                    .map(|html| strip_html_to_text(&html, max_chars))
                    .map_err(|e| ToolCallError(format!("Read error: {}", e)))
            }
            Ok(resp) => Err(ToolCallError(format!("HTTP {}", resp.status()))),
            Err(e) => Err(ToolCallError(format!("Fetch failed: {}", e))),
        }
    }
}

#[derive(serde::Deserialize)]
pub struct WorkspaceExecuteScriptArgs {
    pub script_name: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

/// workspace/scripts/ 配下のスクリプトを実行するツール
#[derive(Clone)]
pub struct WorkspaceExecuteScriptTool {
    workspace_dir: std::path::PathBuf,
}

impl WorkspaceExecuteScriptTool {
    pub fn new(workspace_dir: std::path::PathBuf) -> Self {
        Self { workspace_dir }
    }
}

impl rig_core::tool::Tool for WorkspaceExecuteScriptTool {
    const NAME: &'static str = "run_workspace_script";
    type Error = ToolCallError;
    type Args = WorkspaceExecuteScriptArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition {
        rig_core::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Executes a custom script located strictly within the `workspace/scripts/` or a localized skill directory (e.g. `skills/[skill-name]/scripts/`) to run local computations, API interactions, or data operations.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "script_name": {
                        "type": "string",
                        "description": "The script path relative to workspace. Examples: '500_get-vital-data-garmin.sh' or 'skills/vitals-coach/scripts/500_get-vital-data-garmin.sh'. Path traversal sequences like '..' or absolute paths are forbidden."
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional list of arguments to pass to the script."
                    },
                    "env": {
                        "type": "object",
                        "description": "Optional environment variables. Supports '$vault:key_name' syntax to resolve from vault.",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["script_name"]
            }),
        }
    }

    async fn call(&self, args: WorkspaceExecuteScriptArgs) -> Result<String, ToolCallError> {
        let script_name = &args.script_name;
        if script_name.contains("..") || script_name.starts_with('/') || script_name.contains('\\') {
            return Err(ToolCallError("Error: Security violation. Path traversal ('..'), absolute paths, or backward slashes ('\\') are strictly prohibited.".into()));
        }
        let (script_path, scripts_dir) = if script_name.starts_with("skills/") {
            let path = self.workspace_dir.join(script_name);
            let dir = path.parent().unwrap_or(&self.workspace_dir).to_path_buf();
            (path, dir)
        } else {
            let dir = self.workspace_dir.join("scripts");
            let path = dir.join(script_name);
            (path, dir)
        };
        if !script_path.exists() {
            return Err(ToolCallError(format!(
                "Error: Script '{}' does not exist at expected location: {:?}",
                script_name, script_path
            )));
        }
        let ext = script_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let mut cmd = match ext {
            "sh" => { let mut c = tokio::process::Command::new("bash"); c.arg(&script_path); c }
            "py" => { let mut c = tokio::process::Command::new("python3"); c.arg(&script_path); c }
            "js" => { let mut c = tokio::process::Command::new("node"); c.arg(&script_path); c }
            "ts" => { let mut c = tokio::process::Command::new("bun"); c.arg("run").arg(&script_path); c }
            _    => tokio::process::Command::new(&script_path),
        };
        cmd.args(&args.args);
        for (key, val_str) in &args.env {
            let resolved = if let Some(vault_key) = val_str.strip_prefix("$vault:") {
                let mut resolved_opt = match rustyclaw_config::vault::load_vault(None) {
                    Ok(secrets) => secrets.get(vault_key).cloned(),
                    Err(_) => None,
                };
                if resolved_opt.is_none() {
                    let json_path = rustyclaw_config::get_config_dir().join("vault.json");
                    if let Ok(file) = std::fs::File::open(json_path) {
                        let reader = std::io::BufReader::new(file);
                        if let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(reader) {
                            if let Some(v) = json.get(vault_key).and_then(|v| v.as_str()) {
                                resolved_opt = Some(v.to_string());
                            }
                        }
                    }
                }
                let env_var_name = vault_key.replace('-', "_").to_uppercase();
                if resolved_opt.is_none() {
                    if let Ok(env_val) = std::env::var(&env_var_name) {
                        resolved_opt = Some(env_val);
                    }
                }
                match resolved_opt {
                    Some(val) => val,
                    None => return Err(ToolCallError(format!(
                        "Error: Failed to resolve vault reference '$vault:{}'. \n\
                         Key '{}' was not found in vault.enc, vault.json, or env var '{}'. \n\
                         To configure: `rustyclaw vault set {} <value>`",
                        vault_key, vault_key, env_var_name, vault_key
                    ))),
                }
            } else {
                val_str.clone()
            };
            cmd.env(key, resolved);
        }
        cmd.current_dir(&scripts_dir);
        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let mut content = format!("--- STDOUT ---\n{}", stdout);
                if !stderr.is_empty() {
                    content.push_str(&format!("\n--- STDERR ---\n{}", stderr));
                }
                content.push_str(&format!("\n--- EXIT STATUS ---\n{}", output.status));
                if output.status.success() {
                    Ok(content)
                } else {
                    Err(ToolCallError(content))
                }
            }
            Err(e) => Err(ToolCallError(format!("Error: Failed to spawn script process: {:#}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_tool_registration_and_retrieval() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(WebFetchTool::new()) as Arc<dyn rig_core::tool::ToolDyn>;
        registry.register(tool);

        let retrieved = registry.get("web_fetch");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "web_fetch");

        let non_existent = registry.get("subtract");
        assert!(non_existent.is_none());
    }

    #[tokio::test]
    async fn test_tool_definitions() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(WebFetchTool::new()) as Arc<dyn rig_core::tool::ToolDyn>);
        let defs = registry.tool_definitions().await;
        assert!(!defs.is_empty());
        assert_eq!(defs[0].name, "web_fetch");
    }



    #[tokio::test]
    async fn test_cron_schedule_tool() {
        use rig_core::tool::ToolDyn;
        let tool = CronScheduleTool::new(|| {
            serde_json::json!([
                {
                    "id": "test-job",
                    "name": "Test Job",
                    "next_run_epoch": 12345678,
                    "trigger_type": "cron"
                }
            ])
        });
        assert_eq!(tool.name(), "get_cron_schedule");
        let def = tool.definition("".to_string()).await;
        assert_eq!(def.parameters["type"], "object");
        let result = tool.call("{}".to_string()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("test-job"));
    }

    #[tokio::test]
    async fn test_workspace_read_write_roundtrip() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let read_tool  = WorkspaceReadTool::new(dir.path().to_path_buf());
        let write_tool = WorkspaceWriteTool::new(dir.path().to_path_buf());

        let res = write_tool.call(r#"{"path":"patrol/state.json","content":"{\"lastRun\":null,\"rotationIndex\":0}"}"#.to_string()).await;
        assert!(res.is_ok(), "write failed: {:?}", res);

        let res = read_tool.call(r#"{"path":"patrol/state.json"}"#.to_string()).await;
        assert!(res.is_ok(), "read failed: {:?}", res);
        assert!(res.unwrap().contains("rotationIndex"));
    }

    #[tokio::test]
    async fn test_workspace_write_append_mode() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceWriteTool::new(dir.path().to_path_buf());

        tool.call(r#"{"path":"patrol/findings.md","content":"line1\n"}"#.to_string()).await.unwrap();
        tool.call(r#"{"path":"patrol/findings.md","content":"line2\n","mode":"append"}"#.to_string()).await.unwrap();

        let content = std::fs::read_to_string(dir.path().join("patrol/findings.md")).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
    }

    #[tokio::test]
    async fn test_workspace_read_blocks_path_traversal() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceReadTool::new(dir.path().to_path_buf());
        let res = tool.call(r#"{"path":"../etc/passwd"}"#.to_string()).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_web_search_tool_schema() {
        use rig_core::tool::ToolDyn;
        let tool = WebSearchTool::new("dummy-key".to_string());
        assert_eq!(tool.name(), "web_search");
        let def = tool.definition("".to_string()).await;
        assert!(def.parameters["properties"]["query"].is_object());
        assert_eq!(def.parameters["required"][0], "query");
    }

    #[tokio::test]
    async fn test_web_search_missing_query() {
        use rig_core::tool::ToolDyn;
        let tool = WebSearchTool::new("dummy-key".to_string());
        let res = tool.call("{}".to_string()).await;
        assert!(res.is_err());
    }

    #[test]
    fn test_strip_html_removes_tags() {
        let html = "<h1>Title</h1><p>Hello <b>world</b></p>";
        let plain = strip_html_to_text(html, 1000);
        assert!(!plain.contains('<'));
        assert!(plain.contains("Title"));
        assert!(plain.contains("Hello"));
        assert!(plain.contains("world"));
    }

    #[test]
    fn test_strip_html_removes_script_content() {
        let html = "<p>visible</p><script>var x = secret();</script><p>also visible</p>";
        let plain = strip_html_to_text(html, 1000);
        assert!(!plain.contains("secret"));
        assert!(plain.contains("visible"));
    }

    #[test]
    fn test_strip_html_truncates_at_max_chars() {
        let html = format!("<p>{}</p>", "a".repeat(5000));
        let plain = strip_html_to_text(&html, 100);
        assert!(plain.len() <= 110);
    }

    #[tokio::test]
    async fn test_web_fetch_tool_schema() {
        use rig_core::tool::ToolDyn;
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "web_fetch");
        let def = tool.definition("".to_string()).await;
        assert!(def.parameters["properties"]["url"].is_object());
    }

    #[tokio::test]
    async fn test_web_fetch_missing_url() {
        use rig_core::tool::ToolDyn;
        let tool = WebFetchTool::new();
        let res = tool.call("{}".to_string()).await;
        assert!(res.is_err());
    }

    #[test]
    fn test_strip_html_handles_multibyte() {
        let html = "<p>こんにちは</p><script>秘密</script><p>世界</p>";
        let plain = strip_html_to_text(html, 1000);
        assert!(plain.contains("こんにちは"), "日本語テキストが保持されること");
        assert!(plain.contains("世界"), "日本語テキストが保持されること");
        assert!(!plain.contains("秘密"), "script内容が除去されること");
    }

    #[tokio::test]
    async fn test_memory_search_tool_schema() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = MemorySearchTool::new(dir.path().to_path_buf());
        assert_eq!(tool.name(), "memory_search");
        let def = tool.definition("".to_string()).await;
        assert!(def.parameters["properties"]["query"].is_object());
        assert_eq!(def.parameters["required"][0], "query");
    }

    #[tokio::test]
    async fn test_memory_search_missing_query() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = MemorySearchTool::new(dir.path().to_path_buf());
        let res = tool.call("{}".to_string()).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_memory_search_empty_index() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = MemorySearchTool::new(dir.path().to_path_buf());
        // インデックスが存在しない場合は graceful に処理（クラッシュしないこと）
        let res = tool.call(r#"{"query":"rust"}"#.to_string()).await;
        // エラーになっても文字列は返る（空でない）
        let text = match res {
            Ok(s) => s,
            Err(e) => e.to_string(),
        };
        assert!(!text.is_empty());
    }

    #[tokio::test]
    async fn test_workspace_execute_script_tool_schema() {
        use rig_core::tool::ToolDyn;
        let tool = WorkspaceExecuteScriptTool::new(std::path::PathBuf::from("/tmp"));
        assert_eq!(tool.name(), "run_workspace_script");
        let def = tool.definition("".to_string()).await;
        assert!(def.parameters["properties"]["script_name"].is_object());
    }

    #[tokio::test]
    async fn test_workspace_execute_script_path_traversal() {
        use rig_core::tool::ToolDyn;
        let tool = WorkspaceExecuteScriptTool::new(std::path::PathBuf::from("/tmp"));
        let res = tool.call(r#"{"script_name":"../etc/passwd"}"#.to_string()).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Security violation"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_absolute_path() {
        use rig_core::tool::ToolDyn;
        let tool = WorkspaceExecuteScriptTool::new(std::path::PathBuf::from("/tmp"));
        let res = tool.call(r#"{"script_name":"/etc/passwd"}"#.to_string()).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Security violation"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_localized_success() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceExecuteScriptTool::new(dir.path().to_path_buf());

        // 局所化したダミースクリプトを作成
        let localized_script_dir = dir.path().join("skills/vitals-coach/scripts");
        std::fs::create_dir_all(&localized_script_dir).unwrap();

        let script_path = localized_script_dir.join("test-run.sh");
        std::fs::write(&script_path, "#!/bin/bash\necho 'hello vitals'").unwrap();

        // 実行権限の付与（Unix系のみ）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).unwrap();
        }

        let res = tool.call(r#"{"script_name":"skills/vitals-coach/scripts/test-run.sh"}"#.to_string()).await;

        assert!(res.is_ok(), "Tool reported error: {:?}", res);
        assert!(res.unwrap().contains("hello vitals"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_vault_injection_success() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceExecuteScriptTool::new(dir.path().to_path_buf());

        let localized_script_dir = dir.path().join("skills/vitals-coach/scripts");
        std::fs::create_dir_all(&localized_script_dir).unwrap();

        let script_path = localized_script_dir.join("test-env.sh");
        std::fs::write(&script_path, "#!/bin/bash\necho \"VAL is $TEST_VAL\"").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).unwrap();
        }

        // 環境変数フォールバックが働くように親プロセスにセット
        unsafe { std::env::set_var("TEST_KEY", "resolved-secret-from-env"); }

        let res = tool.call(r#"{"script_name":"skills/vitals-coach/scripts/test-env.sh","env":{"TEST_VAL":"$vault:test-key"}}"#.to_string()).await;

        unsafe { std::env::remove_var("TEST_KEY"); }

        assert!(res.is_ok(), "Tool reported error: {:?}", res);
        assert!(res.unwrap().contains("VAL is resolved-secret-from-env"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_vault_injection_fail_fast() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceExecuteScriptTool::new(dir.path().to_path_buf());

        let localized_script_dir = dir.path().join("skills/vitals-coach/scripts");
        std::fs::create_dir_all(&localized_script_dir).unwrap();

        let script_path = localized_script_dir.join("test-env.sh");
        std::fs::write(&script_path, "#!/bin/bash\necho \"VAL is $TEST_VAL\"").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).unwrap();
        }

        // キーが一切解決できない状態
        let res = tool.call(r#"{"script_name":"skills/vitals-coach/scripts/test-env.sh","env":{"TEST_VAL":"$vault:non-existent-secret-key-xyz"}}"#.to_string()).await;

        // 方式A: 即座にフェイルファスト（エラー）になり、スクリプト実行は行われない
        assert!(res.is_err());
        let err = res.unwrap_err().to_string();
        assert!(err.contains("Failed to resolve vault reference"));
        assert!(err.contains("rustyclaw vault set non-existent-secret-key-xyz"));
    }

    // ── Task 1: rig_core::tool::Tool 直接実装テスト ──
    #[tokio::test]
    async fn test_web_fetch_rig_core_tool_name() {
        use rig_core::tool::Tool;
        assert_eq!(WebFetchTool::NAME, "web_fetch");
    }

    #[tokio::test]
    async fn test_web_fetch_rig_core_tool_definition() {
        use rig_core::tool::Tool;
        let tool = WebFetchTool::new();
        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "web_fetch");
        assert!(def.parameters.get("properties").is_some());
    }

    #[tokio::test]
    async fn test_web_fetch_rig_core_call_missing_url() {
        use rig_core::tool::ToolDyn;
        let tool = WebFetchTool::new();
        // "url" フィールドが必須なので空オブジェクトは JsonError になる
        let result = tool.call("{}".to_string()).await;
        assert!(result.is_err());
    }

    // ── Task 2: WorkspaceReadTool / WorkspaceWriteTool rig-core impl ──
    #[tokio::test]
    async fn test_workspace_read_rig_core_roundtrip() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let write_tool = WorkspaceWriteTool::new(dir.path().to_path_buf());
        let read_tool  = WorkspaceReadTool::new(dir.path().to_path_buf());

        let write_result = write_tool.call(r#"{"path":"notes.txt","content":"hello"}"#.to_string()).await;
        assert!(write_result.is_ok(), "write failed: {:?}", write_result);

        let read_result = read_tool.call(r#"{"path":"notes.txt"}"#.to_string()).await;
        assert!(read_result.is_ok(), "read failed: {:?}", read_result);
        assert!(read_result.unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_workspace_read_rig_core_path_traversal() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceReadTool::new(dir.path().to_path_buf());
        let result = tool.call(r#"{"path":"../etc/passwd"}"#.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workspace_write_rig_core_append() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceWriteTool::new(dir.path().to_path_buf());
        tool.call(r#"{"path":"log.txt","content":"line1\n"}"#.to_string()).await.unwrap();
        tool.call(r#"{"path":"log.txt","content":"line2\n","mode":"append"}"#.to_string()).await.unwrap();
        let content = std::fs::read_to_string(dir.path().join("log.txt")).unwrap();
        assert!(content.contains("line1") && content.contains("line2"));
    }

    // ── Task 3: 残りのツール rig-core impl ──
    #[tokio::test]
    async fn test_memory_search_rig_core_missing_query() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = MemorySearchTool::new(dir.path().to_path_buf());
        let result = tool.call("{}".to_string()).await;
        assert!(result.is_err()); // "query" is required
    }

    #[tokio::test]
    async fn test_web_search_rig_core_missing_query() {
        use rig_core::tool::ToolDyn;
        let tool = WebSearchTool::new("dummy-key".to_string());
        let result = tool.call("{}".to_string()).await;
        assert!(result.is_err()); // "query" is required
    }

    #[tokio::test]
    async fn test_cron_schedule_rig_core_call() {
        use rig_core::tool::ToolDyn;
        let tool = CronScheduleTool::new(|| serde_json::json!([{"id":"job1"}]));
        let result = tool.call("{}".to_string()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("job1"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_rig_core_path_traversal() {
        use rig_core::tool::ToolDyn;
        let tool = WorkspaceExecuteScriptTool::new(std::path::PathBuf::from("/tmp"));
        let result = tool.call(r#"{"script_name":"../etc/passwd"}"#.to_string()).await;
        assert!(result.is_err());
    }

    // ── Task 4: ToolRegistry rig-core ToolDyn ──
    #[tokio::test]
    async fn test_tool_registry_rig_core_registration_and_lookup() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(WebFetchTool::new()) as Arc<dyn rig_core::tool::ToolDyn>;
        registry.register(tool);
        assert_eq!(registry.tool_count(), 1);
        assert!(registry.get("web_fetch").is_some());
        assert!(registry.get("no_such_tool").is_none());
    }

    #[tokio::test]
    async fn test_tool_registry_rig_core_tool_definitions() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(WebFetchTool::new()) as Arc<dyn rig_core::tool::ToolDyn>);
        let defs = registry.tool_definitions().await;
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "web_fetch");
        assert!(!defs[0].description.is_empty());
        assert!(defs[0].parameters.get("properties").is_some());
    }
}
