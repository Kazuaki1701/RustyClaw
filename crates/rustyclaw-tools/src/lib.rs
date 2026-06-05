use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;

/// Represents the output of a tool execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ToolResult {
    /// The text content returned to the LLM.
    pub content: String,
    /// Indicates whether the tool execution encountered an error.
    pub is_error: bool,
}

/// The trait that all native and MCP-proxied tools must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name of the tool (e.g., "read_file", "calculate").
    fn name(&self) -> &str;

    /// A clear description of what the tool does and when to use it.
    fn description(&self) -> &str;

    /// The JSON Schema defining the expected parameters.
    /// Typically formatted as `{"type": "object", "properties": {...}, "required": [...]}`.
    fn parameters(&self) -> Value;

    /// Executes the tool asynchronously with the given JSON arguments.
    async fn execute(&self, args: Value) -> ToolResult;
}

/// A registry that manages active tools and compiles their schemas for LLM ingestion.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Creates a new, empty ToolRegistry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Registers a tool in the registry. Overwrites if a tool with the same name exists.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Retrieves a tool by its unique name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Returns the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Compiles all registered tools into LLM-compatible schemas.
    /// Returns a list of tool definitions compatible with the Anthropic/OpenAI tools parameter structure.
    pub fn to_llm_schemas(&self) -> Vec<Value> {
        let mut schemas: Vec<Value> = self
            .tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.parameters()
                })
            })
            .collect();

        // Sort by name to guarantee deterministic schema ordering in tests and requests.
        schemas.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });

        schemas
    }

    /// Converts all registered tools into a `rig_core::tool::ToolSet` for use with rig agents.
    pub fn to_toolset(&self) -> rig_core::tool::ToolSet {
        let mut toolset = rig_core::tool::ToolSet::default();
        for tool in self.tools.values() {
            toolset.add_tool_boxed(Box::new(RigToolAdapter::new(tool.clone())));
        }
        toolset
    }

    /// Returns all registered tools as a `Vec<Box<dyn ToolDyn>>` for use with `AgentBuilder::tools()`.
    pub fn to_dyn_tools(&self) -> Vec<Box<dyn rig_core::tool::ToolDyn>> {
        self.tools
            .values()
            .map(|tool| -> Box<dyn rig_core::tool::ToolDyn> {
                Box::new(RigToolAdapter::new(tool.clone()))
            })
            .collect()
    }
}

/// Adapts a RustyClaw `Tool` to the `rig_core::tool::ToolDyn` interface.
/// Allows existing stateful tools to be used with `rig::agent::Agent`.
pub struct RigToolAdapter {
    inner: Arc<dyn Tool>,
}

impl RigToolAdapter {
    pub fn new(tool: Arc<dyn Tool>) -> Self {
        Self { inner: tool }
    }
}

#[derive(Debug)]
struct ToolCallFailed(String);

impl std::fmt::Display for ToolCallFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ToolCallFailed {}

impl rig_core::tool::ToolDyn for RigToolAdapter {
    fn name(&self) -> String {
        self.inner.name().to_string()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> rig_core::wasm_compat::WasmBoxedFuture<'a, rig_core::completion::ToolDefinition> {
        let name = self.inner.name().to_string();
        let description = self.inner.description().to_string();
        let parameters = self.inner.parameters();
        Box::pin(async move {
            rig_core::completion::ToolDefinition {
                name,
                description,
                parameters,
            }
        })
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> rig_core::wasm_compat::WasmBoxedFuture<
        'a,
        Result<String, rig_core::tool::ToolError>,
    > {
        let inner = self.inner.clone();
        Box::pin(async move {
            let parsed: Value = serde_json::from_str(&args)
                .unwrap_or(Value::Object(serde_json::Map::new()));
            let result = inner.execute(parsed).await;
            if result.is_error {
                Err(rig_core::tool::ToolError::ToolCallError(Box::new(
                    ToolCallFailed(result.content),
                )))
            } else {
                Ok(result.content)
            }
        })
    }
}



/// Dynamic scheduled cron tasks scheduler retriever
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

#[async_trait]
impl Tool for CronScheduleTool {
    fn name(&self) -> &str {
        "get_cron_schedule"
    }

    fn description(&self) -> &str {
        "Get the upcoming scheduled cron tasks and their calculated next execution times."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: Value) -> ToolResult {
        let val = (self.schedule_fn)();
        match serde_json::to_string_pretty(&val) {
            Ok(json) => ToolResult {
                content: json,
                is_error: false,
            },
            Err(e) => ToolResult {
                content: format!("Failed to serialize schedule: {}", e),
                is_error: true,
            },
        }
    }
}

// ─── WorkspaceReadTool ───────────────────────────────────────────────────────

pub struct WorkspaceReadTool {
    workspace_path: std::path::PathBuf,
}

impl WorkspaceReadTool {
    pub fn new(workspace_path: std::path::PathBuf) -> Self {
        Self { workspace_path }
    }
}

#[async_trait]
impl Tool for WorkspaceReadTool {
    fn name(&self) -> &str { "workspace_read" }

    fn description(&self) -> &str {
        "Read a text file from the agent workspace (e.g. patrol/state.json, patrol/findings.md)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path within workspace (e.g. patrol/state.json)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let rel = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolResult { content: "Missing path".into(), is_error: true },
        };
        if rel.contains("..") || rel.starts_with('/') {
            return ToolResult { content: "Invalid path".into(), is_error: true };
        }
        match std::fs::read_to_string(self.workspace_path.join(rel)) {
            Ok(c)  => ToolResult { content: c, is_error: false },
            Err(e) => ToolResult { content: format!("Cannot read {}: {}", rel, e), is_error: true },
        }
    }
}

// ─── WorkspaceWriteTool ──────────────────────────────────────────────────────

pub struct WorkspaceWriteTool {
    workspace_path: std::path::PathBuf,
}

impl WorkspaceWriteTool {
    pub fn new(workspace_path: std::path::PathBuf) -> Self {
        Self { workspace_path }
    }
}

#[async_trait]
impl Tool for WorkspaceWriteTool {
    fn name(&self) -> &str { "workspace_write" }

    fn description(&self) -> &str {
        "Write or append to a text file in the agent workspace (patrol/, memory/logs/ etc.)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string", "description": "Relative path within workspace" },
                "content": { "type": "string", "description": "Text to write" },
                "mode":    {
                    "type": "string",
                    "description": "\"write\" (overwrite, default) or \"append\""
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let rel = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolResult { content: "Missing path".into(), is_error: true },
        };
        let body = match args["content"].as_str() {
            Some(c) => c,
            None => return ToolResult { content: "Missing content".into(), is_error: true },
        };
        if rel.contains("..") || rel.starts_with('/') {
            return ToolResult { content: "Invalid path: traversal not allowed".into(), is_error: true };
        }
        let full = self.workspace_path.join(rel);
        if let Some(parent) = full.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return ToolResult { content: format!("mkdir failed: {}", e), is_error: true };
            }
        }
        let result = if args["mode"].as_str() == Some("append") {
            use std::io::Write;
            std::fs::OpenOptions::new().append(true).create(true).open(&full)
                .and_then(|mut f| f.write_all(body.as_bytes()))
        } else {
            std::fs::write(&full, body.as_bytes())
        };
        match result {
            Ok(_)  => ToolResult { content: format!("OK: wrote to {}", rel), is_error: false },
            Err(e) => ToolResult { content: format!("Write failed {}: {}", rel, e), is_error: true },
        }
    }
}

// ─── MemorySearchTool ────────────────────────────────────────────────────────

pub struct MemorySearchTool {
    workspace_path: std::path::PathBuf,
}

impl MemorySearchTool {
    pub fn new(workspace_path: std::path::PathBuf) -> Self {
        Self { workspace_path }
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str { "memory_search" }

    fn description(&self) -> &str {
        "Search the agent's long-term memory (session summaries) by keyword. Returns file paths of matching summaries."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let query = match args["query"].as_str() {
            Some(q) => q.to_string(),
            None => return ToolResult { content: "Missing query".into(), is_error: true },
        };
        let index_dir = self.workspace_path.join("memory").join("index");
        match rustyclaw_storage::SearchIndexManager::new(&index_dir) {
            Ok(mgr) => match mgr.search(&query) {
                Ok(paths) if paths.is_empty() => {
                    ToolResult { content: "No results found.".into(), is_error: false }
                }
                Ok(paths) => {
                    let result = paths.iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ToolResult { content: result, is_error: false }
                }
                Err(e) => ToolResult { content: format!("Search failed: {}", e), is_error: true },
            },
            Err(e) => ToolResult {
                content: format!("Memory index not available (may not have summaries yet): {}", e),
                is_error: true,
            },
        }
    }
}

// ─── WebSearchTool ───────────────────────────────────────────────────────────

pub struct WebSearchTool {
    api_key: String,
}

impl WebSearchTool {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }

    fn description(&self) -> &str {
        "Search the web using Brave Search. Returns titles, URLs, and snippets. Use site: prefix for HN/Reddit (e.g. 'site:news.ycombinator.com rust')."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "count": {
                    "type": "string",
                    "description": "Number of results as a string (default: '5', max: '10')"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let query = match args["query"].as_str() {
            Some(q) => q.to_string(),
            None => return ToolResult { content: "Missing query".into(), is_error: true },
        };
        let count = match &args["count"] {
            Value::Number(n) => n.as_u64().unwrap_or(5),
            Value::String(s) => s.parse::<u64>().unwrap_or(5),
            _ => 5,
        }.min(10);

        let client = reqwest::Client::new();
        match client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .query(&[("q", &query), ("count", &count.to_string())])
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Value>().await {
                    Ok(json) => {
                        let results = json["web"]["results"]
                            .as_array()
                            .map(|arr| {
                                arr.iter().map(|r| {
                                    let title = r["title"].as_str().unwrap_or("(no title)");
                                    let url   = r["url"].as_str().unwrap_or("");
                                    let desc  = r["description"].as_str().unwrap_or("");
                                    format!("**{}**\n{}\n{}", title, url, desc)
                                }).collect::<Vec<_>>().join("\n\n")
                            })
                            .unwrap_or_else(|| "No results".to_string());
                        ToolResult { content: results, is_error: false }
                    }
                    Err(e) => ToolResult { content: format!("JSON parse error: {}", e), is_error: true },
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                ToolResult { content: format!("Brave Search error {}: {}", status, body), is_error: true }
            }
            Err(e) => ToolResult { content: format!("Request failed: {}", e), is_error: true },
        }
    }
}

// ─── WebFetchTool ────────────────────────────────────────────────────────────

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

#[derive(serde::Deserialize)]
pub struct WebFetchArgs {
    pub url: String,
    #[serde(default = "web_fetch_default_max_chars")]
    pub max_chars: usize,
}

fn web_fetch_default_max_chars() -> usize { 3000 }

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }

    fn description(&self) -> &str {
        "Fetch a web page and return its plain text content (HTML tags stripped). Use after web_search to read full article content."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" },
                "max_chars": {
                    "type": "string",
                    "description": "Max characters to return as a string (default: '3000')"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let url = match args["url"].as_str() {
            Some(u) => u.to_string(),
            None => return ToolResult { content: "Missing url".into(), is_error: true },
        };
        let max_chars = match &args["max_chars"] {
            Value::Number(n) => n.as_u64().unwrap_or(3000) as usize,
            Value::String(s) => s.parse::<usize>().unwrap_or(3000),
            _ => 3000,
        };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (compatible; RustyClaw/1.0)")
            .build()
            .unwrap_or_default();
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.text().await {
                    Ok(html) => ToolResult { content: strip_html_to_text(&html, max_chars), is_error: false },
                    Err(e)   => ToolResult { content: format!("Read error: {}", e), is_error: true },
                }
            }
            Ok(resp) => ToolResult {
                content: format!("HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult { content: format!("Fetch failed: {}", e), is_error: true },
        }
    }
}

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
                        "type": "string",
                        "description": "Max characters to return as a string (default: '3000')"
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

/// workspace/scripts/ 配下のスクリプトを実行するツール
pub struct WorkspaceExecuteScriptTool {
    workspace_dir: std::path::PathBuf,
}

impl WorkspaceExecuteScriptTool {
    pub fn new(workspace_dir: std::path::PathBuf) -> Self {
        Self { workspace_dir }
    }
}

#[async_trait]
impl Tool for WorkspaceExecuteScriptTool {
    fn name(&self) -> &str {
        "run_workspace_script"
    }

    fn description(&self) -> &str {
        "Executes a custom script located strictly within the `workspace/scripts/` or a localized skill directory (e.g. `skills/[skill-name]/scripts/`) to run local computations, API interactions, or data operations."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "script_name": {
                    "type": "string",
                    "description": "The script path relative to workspace. Examples: '500_get-vital-data-garmin.sh' (for legacy scripts inside workspace/scripts/) or 'skills/vitals-coach/scripts/500_get-vital-data-garmin.sh' (for localized skill scripts). Path traversal sequences like '..' or absolute paths are strictly forbidden."
                },
                "args": {
                    "type": "array",
                    "items": {
                        "type": "string"
                    },
                    "description": "Optional list of arguments to pass to the script."
                },
                "env": {
                    "type": "object",
                    "description": "Optional environment variables to inject into the script process. Supports '$vault:key_name' value syntax to dynamically resolve values from vault.enc / vault.json.",
                    "additionalProperties": {
                        "type": "string"
                    }
                }
            },
            "required": ["script_name"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let script_name = match args["script_name"].as_str() {
            Some(s) => s,
            None => return ToolResult {
                content: "Error: Missing or invalid 'script_name' argument.".to_string(),
                is_error: true,
            },
        };

        // セキュリティチェック：絶対パスおよびパス走査 (..) の防御
        if script_name.contains("..") || script_name.starts_with('/') || script_name.contains('\\') {
            return ToolResult {
                content: "Error: Security violation. Path traversal ('..'), absolute paths, or backward slashes ('\\') are strictly prohibited.".to_string(),
                is_error: true,
            };
        }

        // パス解決：
        // パターンA (局所化): skills/ で始まる場合は workspace 直下からの相対パス
        // パターンB (従来フォールバック): それ以外は workspace/scripts/ 配下
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
            return ToolResult {
                content: format!("Error: Script '{}' does not exist at expected location: {:?}", script_name, script_path),
                is_error: true,
            };
        }

        // 引数のパース
        let mut cmd_args = Vec::new();
        if let Some(args_array) = args["args"].as_array() {
            for val in args_array {
                if let Some(arg_str) = val.as_str() {
                    cmd_args.push(arg_str.to_string());
                }
            }
        }

        // 拡張子から実行コマンドを判別
        let ext = script_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let mut cmd = match ext {
            "sh" => {
                let mut c = tokio::process::Command::new("bash");
                c.arg(&script_path);
                c
            }
            "py" => {
                let mut c = tokio::process::Command::new("python3");
                c.arg(&script_path);
                c
            }
            "js" => {
                let mut c = tokio::process::Command::new("node");
                c.arg(&script_path);
                c
            }
            "ts" => {
                let mut c = tokio::process::Command::new("bun"); // bun 優先
                c.arg("run");
                c.arg(&script_path);
                c
            }
            _ => {
                // その他の場合は直接実行（実行可能属性があると仮定）
                tokio::process::Command::new(&script_path)
            }
        };

        // 引数を追加
        cmd.args(cmd_args);

        // 環境変数の追加と Vault 解決
        if let Some(env_obj) = args["env"].as_object() {
            for (key, val) in env_obj {
                if let Some(val_str) = val.as_str() {
                    let resolved_value = if let Some(vault_key) = val_str.strip_prefix("$vault:") {
                        // 1. Vault (vault.enc) からの解決を試みる
                        let mut val_opt = match rustyclaw_config::vault::load_vault(None) {
                            Ok(secrets) => secrets.get(vault_key).cloned(),
                            Err(_) => None,
                        };
                        
                        // 2. なければ平文の vault.json からの解決を試みる (フォールバック)
                        if val_opt.is_none() {
                            let json_path = rustyclaw_config::get_config_dir().join("vault.json");
                            if let Ok(file) = std::fs::File::open(json_path) {
                                let reader = std::io::BufReader::new(file);
                                if let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(reader) {
                                    if let Some(v) = json.get(vault_key).and_then(|v| v.as_str()) {
                                        val_opt = Some(v.to_string());
                                    }
                                }
                            }
                        }

                        // 3. なければ UNIX環境変数からのインテリジェント・フォールバックを試みる
                        let env_var_name = vault_key.replace("-", "_").to_uppercase();
                        if val_opt.is_none() {
                            if let Ok(env_val) = std::env::var(&env_var_name) {
                                val_opt = Some(env_val);
                            }
                        }

                        // 4. どちらも解決できない場合は、方式A（安全にエラーを返して即時中断）
                        match val_opt {
                            Some(val) => val,
                            None => {
                                return ToolResult {
                                    content: format!(
                                        "Error: Failed to resolve vault reference '$vault:{}'. \n\
                                         Key '{}' was not found in ~/.rustyclaw/vault.enc (passphrase not configured?), \n\
                                         nor in fallback vault.json, nor is the environment variable '{}' set. \n\
                                         To configure this key in the vault, run: `rustyclaw vault set {} <value>`", 
                                        vault_key, vault_key, env_var_name, vault_key
                                    ),
                                    is_error: true,
                                };
                            }
                        }
                    } else {
                        // $vault: プレフィックスがない場合は普通の文字列値として採用
                        val_str.to_string()
                    };

                    cmd.env(key, resolved_value);
                }
            }
        }

        cmd.current_dir(&scripts_dir);

        // 実行
        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let is_success = output.status.success();

                let mut content = format!("--- STDOUT ---\n{}", stdout);
                if !stderr.is_empty() {
                    content.push_str(&format!("\n--- STDERR ---\n{}", stderr));
                }
                content.push_str(&format!("\n--- EXIT STATUS ---\n{}", output.status));

                ToolResult {
                    content,
                    is_error: !is_success,
                }
            }
            Err(e) => ToolResult {
                content: format!("Error: Failed to spawn script process: {:#}", e),
                is_error: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // A mock tool that performs basic math addition
    struct AddTool;

    #[async_trait]
    impl Tool for AddTool {
        fn name(&self) -> &str {
            "add"
        }

        fn description(&self) -> &str {
            "Adds two numbers together."
        }

        fn parameters(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["a", "b"]
            })
        }

        async fn execute(&self, args: Value) -> ToolResult {
            let a = match args["a"].as_f64() {
                Some(val) => val,
                None => return ToolResult {
                    content: "Missing or invalid parameter 'a'".to_string(),
                    is_error: true,
                },
            };
            let b = match args["b"].as_f64() {
                Some(val) => val,
                None => return ToolResult {
                    content: "Missing or invalid parameter 'b'".to_string(),
                    is_error: true,
                },
            };
            ToolResult {
                content: format!("{}", a + b),
                is_error: false,
            }
        }
    }

    #[tokio::test]
    async fn test_tool_registration_and_retrieval() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(AddTool);
        registry.register(tool.clone());

        let retrieved = registry.get("add");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "add");
        assert_eq!(retrieved.unwrap().description(), "Adds two numbers together.");

        let non_existent = registry.get("subtract");
        assert!(non_existent.is_none());
    }

    #[tokio::test]
    async fn test_tool_execution_success() {
        let tool = AddTool;
        let args = json!({ "a": 10.5, "b": 2.5 });
        let result = tool.execute(args).await;
        assert!(!result.is_error);
        assert_eq!(result.content, "13");
    }

    #[tokio::test]
    async fn test_tool_execution_error() {
        let tool = AddTool;
        let args = json!({ "a": 10.5 });
        let result = tool.execute(args).await;
        assert!(result.is_error);
        assert!(result.content.contains("parameter 'b'"));
    }

    #[tokio::test]
    async fn test_to_llm_schemas() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(AddTool));

        let schemas = registry.to_llm_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["name"], "add");
        assert_eq!(schemas[0]["description"], "Adds two numbers together.");
        assert_eq!(schemas[0]["input_schema"]["type"], "object");
    }



    #[tokio::test]
    async fn test_cron_schedule_tool() {
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
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        let result = tool.execute(serde_json::json!({})).await;
        assert!(!result.is_error);
        assert!(result.content.contains("test-job"));
    }

    #[tokio::test]
    async fn test_workspace_read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let read_tool  = WorkspaceReadTool::new(dir.path().to_path_buf());
        let write_tool = WorkspaceWriteTool::new(dir.path().to_path_buf());

        let res = write_tool.execute(serde_json::json!({
            "path": "patrol/state.json",
            "content": "{\"lastRun\":null,\"rotationIndex\":0}"
        })).await;
        assert!(!res.is_error);

        let res = read_tool.execute(serde_json::json!({
            "path": "patrol/state.json"
        })).await;
        assert!(!res.is_error);
        assert!(res.content.contains("rotationIndex"));
    }

    #[tokio::test]
    async fn test_workspace_write_append_mode() {
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceWriteTool::new(dir.path().to_path_buf());

        tool.execute(serde_json::json!({"path": "patrol/findings.md", "content": "line1\n"})).await;
        tool.execute(serde_json::json!({"path": "patrol/findings.md", "content": "line2\n", "mode": "append"})).await;

        let content = std::fs::read_to_string(dir.path().join("patrol/findings.md")).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
    }

    #[tokio::test]
    async fn test_workspace_read_blocks_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceReadTool::new(dir.path().to_path_buf());
        let res = tool.execute(serde_json::json!({"path": "../etc/passwd"})).await;
        assert!(res.is_error);
    }

    #[tokio::test]
    async fn test_web_search_tool_schema() {
        let tool = WebSearchTool::new("dummy-key".to_string());
        assert_eq!(tool.name(), "web_search");
        let params = tool.parameters();
        assert!(params["properties"]["query"].is_object());
        assert_eq!(params["required"][0], "query");
    }

    #[tokio::test]
    async fn test_web_search_missing_query() {
        let tool = WebSearchTool::new("dummy-key".to_string());
        let res = tool.execute(serde_json::json!({})).await;
        assert!(res.is_error);
        assert!(res.content.contains("Missing"));
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
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "web_fetch");
        let params = tool.parameters();
        assert!(params["properties"]["url"].is_object());
    }

    #[tokio::test]
    async fn test_web_fetch_missing_url() {
        let tool = WebFetchTool::new();
        let res = tool.execute(serde_json::json!({})).await;
        assert!(res.is_error);
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
        let dir = tempfile::tempdir().unwrap();
        let tool = MemorySearchTool::new(dir.path().to_path_buf());
        assert_eq!(tool.name(), "memory_search");
        let params = tool.parameters();
        assert!(params["properties"]["query"].is_object());
        assert_eq!(params["required"][0], "query");
    }

    #[tokio::test]
    async fn test_memory_search_missing_query() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MemorySearchTool::new(dir.path().to_path_buf());
        let res = tool.execute(serde_json::json!({})).await;
        assert!(res.is_error);
        assert!(res.content.contains("Missing"));
    }

    #[tokio::test]
    async fn test_memory_search_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MemorySearchTool::new(dir.path().to_path_buf());
        // インデックスが存在しない場合は graceful に処理（クラッシュしないこと）
        let res = tool.execute(serde_json::json!({"query": "rust"})).await;
        assert!(!res.content.is_empty());
    }

    #[tokio::test]
    async fn test_workspace_execute_script_tool_schema() {
        let tool = WorkspaceExecuteScriptTool::new(std::path::PathBuf::from("/tmp"));
        assert_eq!(tool.name(), "run_workspace_script");
        let params = tool.parameters();
        assert!(params["properties"]["script_name"].is_object());
    }

    #[tokio::test]
    async fn test_workspace_execute_script_path_traversal() {
        let tool = WorkspaceExecuteScriptTool::new(std::path::PathBuf::from("/tmp"));
        let res = tool.execute(serde_json::json!({
            "script_name": "../etc/passwd"
        })).await;
        assert!(res.is_error);
        assert!(res.content.contains("Security violation"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_absolute_path() {
        let tool = WorkspaceExecuteScriptTool::new(std::path::PathBuf::from("/tmp"));
        let res = tool.execute(serde_json::json!({
            "script_name": "/etc/passwd"
        })).await;
        assert!(res.is_error);
        assert!(res.content.contains("Security violation"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_localized_success() {
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

        let res = tool.execute(serde_json::json!({
            "script_name": "skills/vitals-coach/scripts/test-run.sh"
        })).await;

        assert!(!res.is_error, "Tool reported error: {}", res.content);
        assert!(res.content.contains("hello vitals"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_vault_injection_success() {
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

        let res = tool.execute(serde_json::json!({
            "script_name": "skills/vitals-coach/scripts/test-env.sh",
            "env": {
                "TEST_VAL": "$vault:test-key"
            }
        })).await;

        unsafe { std::env::remove_var("TEST_KEY"); }

        assert!(!res.is_error, "Tool reported error: {}", res.content);
        assert!(res.content.contains("VAL is resolved-secret-from-env"));
    }

    #[tokio::test]
    async fn test_workspace_execute_script_vault_injection_fail_fast() {
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
        let res = tool.execute(serde_json::json!({
            "script_name": "skills/vitals-coach/scripts/test-env.sh",
            "env": {
                "TEST_VAL": "$vault:non-existent-secret-key-xyz"
            }
        })).await;

        // 方式A: 即座にフェイルファスト（エラー）になり、スクリプト実行は行われない
        assert!(res.is_error);
        assert!(res.content.contains("Failed to resolve vault reference"));
        assert!(res.content.contains("rustyclaw vault set non-existent-secret-key-xyz"));
    }

    #[tokio::test]
    async fn test_rig_tool_adapter_wraps_custom_tool() {
        use rig_core::tool::ToolDyn as _;
        use std::sync::Arc;

        let tool = Arc::new(CronScheduleTool::new(|| serde_json::json!([])));
        let adapter = RigToolAdapter::new(tool);

        assert_eq!(adapter.name(), "get_cron_schedule");
        let def = adapter.definition("".to_string()).await;
        assert_eq!(def.name, "get_cron_schedule");
        assert!(!def.description.is_empty());
    }

    #[tokio::test]
    async fn test_tool_registry_to_toolset() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(CronScheduleTool::new(|| serde_json::json!([]))));

        let toolset = registry.to_toolset();
        assert!(toolset.contains("get_cron_schedule"));
    }

    #[tokio::test]
    async fn test_tool_registry_to_dyn_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(CronScheduleTool::new(|| serde_json::json!([]))));

        let tools = registry.to_dyn_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "get_cron_schedule");
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
}
