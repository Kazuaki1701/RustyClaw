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
        Self {
            tools: std::collections::HashMap::new(),
        }
    }

    /// Registers a tool. tool.name() is used as the key.
    pub fn register(&mut self, tool: Arc<dyn rig_core::tool::ToolDyn>) {
        self.tools.insert(tool.name(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn rig_core::tool::ToolDyn>> {
        self.tools.get(name)
    }

    pub fn iter(
        &self,
    ) -> std::collections::hash_map::Iter<'_, String, Arc<dyn rig_core::tool::ToolDyn>> {
        self.tools.iter()
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
            description:
                "Get the upcoming scheduled cron tasks and their calculated next execution times."
                    .to_string(),
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
    preview_base_url: Option<String>,
}

impl WorkspaceWriteTool {
    pub fn new(workspace_path: std::path::PathBuf, preview_base_url: Option<String>) -> Self {
        Self {
            workspace_path,
            preview_base_url,
        }
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
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&full)
                .and_then(|mut f| f.write_all(args.content.as_bytes()))
        } else {
            std::fs::write(&full, args.content.as_bytes())
        };
        result
            .map(|_| {
                let preview_suffix = if let Some(ref base_url) = self.preview_base_url {
                    let path_str = rel.replace('\\', "/");
                    if let Some(sub_path) = path_str.strip_prefix("previews/") {
                        format!(" Preview URL: {}/{}", base_url, sub_path)
                    } else if path_str == "previews" {
                        format!(" Preview URL: {}/", base_url)
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                format!("OK: wrote to {}{}", rel, preview_suffix)
            })
            .map_err(|e| ToolCallError(format!("Write failed {}: {}", rel, e)))
    }
}

// ─── WebSearchTool ───────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
    #[serde(default = "web_search_default_count")]
    pub count: u64,
}

fn web_search_default_count() -> u64 {
    5
}

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
            .query(&[
                ("q", args.query.as_str()),
                ("count", count.to_string().as_str()),
            ])
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp
                .json::<serde_json::Value>()
                .await
                .map_err(|e| ToolCallError(format!("JSON parse error: {}", e)))
                .map(|json| {
                    json["web"]["results"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .map(|r| {
                                    let title = r["title"].as_str().unwrap_or("(no title)");
                                    let url = r["url"].as_str().unwrap_or("");
                                    let desc = r["description"].as_str().unwrap_or("");
                                    format!("**{}**\n{}\n{}", title, url, desc)
                                })
                                .collect::<Vec<_>>()
                                .join("\n\n")
                        })
                        .unwrap_or_else(|| "No results".to_string())
                }),
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Err(ToolCallError(format!(
                    "Brave Search error {}: {}",
                    status, body
                )))
            }
            Err(e) => Err(ToolCallError(format!("Request failed: {}", e))),
        }
    }
}

// ─── WebFetchTool ────────────────────────────────────────────────────────────

/// HTML タグ・script・style ブロックを除去してプレーンテキストを返す（新規 crate 依存なし）
fn strip_html_to_text(html: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_body = false;
    let mut buf = String::new();

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

    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

#[derive(Clone)]
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(serde::Deserialize)]
pub struct WebFetchArgs {
    pub url: String,
    #[serde(default = "web_fetch_default_max_chars")]
    pub max_chars: usize,
}

fn web_fetch_default_max_chars() -> usize {
    3000
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
            Ok(resp) if resp.status().is_success() => resp
                .text()
                .await
                .map(|html| strip_html_to_text(&html, max_chars))
                .map_err(|e| ToolCallError(format!("Read error: {}", e))),
            Ok(resp) => Err(ToolCallError(format!("HTTP {}", resp.status()))),
            Err(e) => Err(ToolCallError(format!("Fetch failed: {}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
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
        let read_tool = WorkspaceReadTool::new(dir.path().to_path_buf());
        let write_tool = WorkspaceWriteTool::new(dir.path().to_path_buf(), None);

        let res = write_tool.call(r#"{"path":"patrol/state.json","content":"{\"lastRun\":null,\"rotationIndex\":0}"}"#.to_string()).await;
        assert!(res.is_ok(), "write failed: {:?}", res);

        let res = read_tool
            .call(r#"{"path":"patrol/state.json"}"#.to_string())
            .await;
        assert!(res.is_ok(), "read failed: {:?}", res);
        assert!(res.unwrap().contains("rotationIndex"));
    }

    #[tokio::test]
    async fn test_workspace_write_append_mode() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceWriteTool::new(dir.path().to_path_buf(), None);

        tool.call(r#"{"path":"patrol/findings.md","content":"line1\n"}"#.to_string())
            .await
            .unwrap();
        tool.call(
            r#"{"path":"patrol/findings.md","content":"line2\n","mode":"append"}"#.to_string(),
        )
        .await
        .unwrap();

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
        assert!(
            plain.contains("こんにちは"),
            "日本語テキストが保持されること"
        );
        assert!(plain.contains("世界"), "日本語テキストが保持されること");
        assert!(!plain.contains("秘密"), "script内容が除去されること");
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
        let write_tool = WorkspaceWriteTool::new(dir.path().to_path_buf(), None);
        let read_tool = WorkspaceReadTool::new(dir.path().to_path_buf());

        let write_result = write_tool
            .call(r#"{"path":"notes.txt","content":"hello"}"#.to_string())
            .await;
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
        let tool = WorkspaceWriteTool::new(dir.path().to_path_buf(), None);
        tool.call(r#"{"path":"log.txt","content":"line1\n"}"#.to_string())
            .await
            .unwrap();
        tool.call(r#"{"path":"log.txt","content":"line2\n","mode":"append"}"#.to_string())
            .await
            .unwrap();
        let content = std::fs::read_to_string(dir.path().join("log.txt")).unwrap();
        assert!(content.contains("line1") && content.contains("line2"));
    }

    #[tokio::test]
    async fn test_workspace_write_preview_url() {
        use rig_core::tool::ToolDyn;
        let dir = tempfile::tempdir().unwrap();
        let tool = WorkspaceWriteTool::new(
            dir.path().to_path_buf(),
            Some("http://100.100.100.100:4000".to_string()),
        );
        let res = tool
            .call(r#"{"path":"previews/index.html","content":"<h1>hello</h1>"}"#.to_string())
            .await
            .unwrap();
        assert!(res.contains("Preview URL: http://100.100.100.100:4000/index.html"));
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
