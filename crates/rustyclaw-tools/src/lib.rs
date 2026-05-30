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
}

/// Karakeep の最近のブックマーク一覧を取得するネイティブツール
pub struct KarakeepListTool {
    server_addr: String,
    api_key: String,
}

impl KarakeepListTool {
    pub fn new(server_addr: String, api_key: String) -> Self {
        Self { server_addr, api_key }
    }
}

#[async_trait]
impl Tool for KarakeepListTool {
    fn name(&self) -> &str {
        "karakeep_list_bookmarks"
    }

    fn description(&self) -> &str {
        "List recent bookmarks from Karakeep. Returns title, URL, tags, and ID for each bookmark. Use to find bookmarks for recommendation or tagging."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Number of bookmarks to return (default: 20, max: 50)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let limit = args["limit"].as_u64().unwrap_or(20).min(50);
        let client = reqwest::Client::new();
        let url = format!("{}/api/v1/bookmarks?limit={}", self.server_addr, limit);
        match client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.text().await {
                    Ok(body) => ToolResult {
                        content: body,
                        is_error: false,
                    },
                    Err(e) => ToolResult {
                        content: format!("Failed to read response: {}", e),
                        is_error: true,
                    },
                }
            }
            Ok(resp) => ToolResult {
                content: format!("Karakeep API error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult {
                content: format!("Karakeep request failed: {}", e),
                is_error: true,
            },
        }
    }
}

/// Karakeep のブックマークにタグを付与するネイティブツール
pub struct KarakeepTagTool {
    server_addr: String,
    api_key: String,
}

impl KarakeepTagTool {
    pub fn new(server_addr: String, api_key: String) -> Self {
        Self { server_addr, api_key }
    }
}

#[async_trait]
impl Tool for KarakeepTagTool {
    fn name(&self) -> &str {
        "karakeep_tag_bookmark"
    }

    fn description(&self) -> &str {
        "Add a tag to a Karakeep bookmark by its ID. Use bookmark IDs from karakeep_list_bookmarks."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "bookmark_id": {
                    "type": "string",
                    "description": "The bookmark ID to tag"
                },
                "tag_name": {
                    "type": "string",
                    "description": "The tag name to apply (e.g. '_recommended')"
                }
            },
            "required": ["bookmark_id", "tag_name"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let bookmark_id = match args["bookmark_id"].as_str() {
            Some(id) => id.to_string(),
            None => {
                return ToolResult {
                    content: "Missing bookmark_id".to_string(),
                    is_error: true,
                }
            }
        };
        let tag_name = match args["tag_name"].as_str() {
            Some(t) => t.to_string(),
            None => {
                return ToolResult {
                    content: "Missing tag_name".to_string(),
                    is_error: true,
                }
            }
        };
        let client = reqwest::Client::new();
        let url = format!("{}/api/v1/bookmarks/{}/tags", self.server_addr, bookmark_id);
        let body = serde_json::json!({"tags": [{"tagName": tag_name}]});
        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => ToolResult {
                content: format!("Tagged {} with '{}'", bookmark_id, tag_name),
                is_error: false,
            },
            Ok(resp) => ToolResult {
                content: format!("Karakeep tag error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult {
                content: format!("Karakeep tag request failed: {}", e),
                is_error: true,
            },
        }
    }
}

fn percent_encode(s: &str) -> String {
    s.chars().flat_map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => vec![c],
        _ => format!("%{:02X}", c as u32).chars().collect(),
    }).collect()
}

/// Obsidian Vault 内をテキスト検索するネイティブツール (Local REST API)
pub struct ObsidianSearchTool {
    host: String,
    api_key: String,
}

impl ObsidianSearchTool {
    pub fn new(host: String, api_key: String) -> Self {
        Self { host, api_key }
    }
}

#[async_trait]
impl Tool for ObsidianSearchTool {
    fn name(&self) -> &str { "obsidian_search" }

    fn description(&self) -> &str {
        "Search for notes in the Obsidian vault by keyword. Returns matching note paths and excerpts."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query string"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return (default: 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let query = match args["query"].as_str() {
            Some(q) => q.to_string(),
            None => return ToolResult { content: "Missing query".to_string(), is_error: true },
        };
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;
        let client = reqwest::Client::new();
        let url = format!(
            "http://{}:27123/search/simple/?query={}&contextLength=100",
            self.host,
            percent_encode(&query)
        );
        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.text().await {
                    Ok(body) => {
                        let trimmed = serde_json::from_str::<Vec<Value>>(&body)
                            .map(|arr| {
                                let limited: Vec<&Value> = arr.iter().take(limit).collect();
                                serde_json::to_string(&limited).unwrap_or(body.clone())
                            })
                            .unwrap_or(body);
                        ToolResult { content: trimmed, is_error: false }
                    }
                    Err(e) => ToolResult { content: format!("Failed to read response: {}", e), is_error: true },
                }
            }
            Ok(resp) => ToolResult {
                content: format!("Obsidian API error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult { content: format!("Obsidian search failed: {}", e), is_error: true },
        }
    }
}

/// Obsidian の特定ノートを読み込むネイティブツール (Local REST API)
pub struct ObsidianReadTool {
    host: String,
    api_key: String,
}

impl ObsidianReadTool {
    pub fn new(host: String, api_key: String) -> Self {
        Self { host, api_key }
    }
}

#[async_trait]
impl Tool for ObsidianReadTool {
    fn name(&self) -> &str { "obsidian_read_note" }

    fn description(&self) -> &str {
        "Read the full content of a specific note from the Obsidian vault by its path."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Vault-relative path to the note (e.g. 'Folder/Note.md')"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => p.to_string(),
            None => return ToolResult { content: "Missing path".to_string(), is_error: true },
        };
        let client = reqwest::Client::new();
        let url = format!("http://{}:27123/vault/{}", self.host, percent_encode(&path));
        match client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.text().await {
                    Ok(body) => ToolResult { content: body, is_error: false },
                    Err(e) => ToolResult { content: format!("Failed to read note: {}", e), is_error: true },
                }
            }
            Ok(resp) => ToolResult {
                content: format!("Obsidian API error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult { content: format!("Obsidian read failed: {}", e), is_error: true },
        }
    }
}

/// Google Calendar のイベント一覧を取得するツール（gws CLI subprocess 経由）
pub struct GwsCalendarTool {
    gws_path: String,
}

impl GwsCalendarTool {
    pub fn new(gws_path: String) -> Self {
        Self { gws_path }
    }
}

#[async_trait]
impl Tool for GwsCalendarTool {
    fn name(&self) -> &str { "gws_calendar_list_events" }

    fn description(&self) -> &str {
        "List upcoming events from Google Calendar. Returns event title, start/end time, attendees, and location. READ-ONLY for all calendars EXCEPT 'AI AGENT' (ID: 6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com) which is write-enabled. Do NOT write to any other calendar (かずあき/あゆみ/ゆうき/ファミリー etc)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "calendar_id": {
                    "type": "string",
                    "description": "Calendar ID (default: 'primary')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Max events to return (default: 10)"
                },
                "time_min": {
                    "type": "string",
                    "description": "Start of time range in RFC3339 format (default: now)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let calendar_id = args["calendar_id"].as_str().unwrap_or("primary");
        let max_results = args["max_results"].as_u64().unwrap_or(10);

        let mut params = serde_json::json!({
            "calendarId": calendar_id,
            "maxResults": max_results,
            "singleEvents": true,
            "orderBy": "startTime"
        });
        if let Some(t) = args["time_min"].as_str() {
            params["timeMin"] = Value::String(t.to_string());
        } else {
            params["timeMin"] = Value::String(chrono::Utc::now().to_rfc3339());
        }

        let output = tokio::process::Command::new(&self.gws_path)
            .args(["calendar", "events", "list",
                   "--params", &params.to_string(),
                   "--format", "json"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let body = String::from_utf8_lossy(&out.stdout).to_string();
                ToolResult { content: body, is_error: false }
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).to_string();
                ToolResult { content: format!("gws calendar error: {}", err), is_error: true }
            }
            Err(e) => ToolResult { content: format!("gws calendar exec failed: {}", e), is_error: true },
        }
    }
}

/// 書き込み許可カレンダー専用の書き込みツール（gws CLI subprocess 経由）
/// 許可カレンダーリストは config.json の GWS_WRITABLE_CALENDARS で設定
pub struct GwsCalendarWriteTool {
    gws_path: String,
    /// (calendar_id, calendar_name) の許可リスト
    allowed: Vec<(String, String)>,
    description_cache: String,
}

impl GwsCalendarWriteTool {
    pub fn new(gws_path: String, allowed: Vec<(String, String)>) -> Self {
        let names: Vec<String> = allowed.iter()
            .map(|(id, name)| format!("'{}' ({})", name, id))
            .collect();
        let description_cache = format!(
            "Create an event in one of the writable calendars: {}. \
             Do NOT use this for any other calendar.",
            names.join(", ")
        );
        Self { gws_path, allowed, description_cache }
    }
}

#[async_trait]
impl Tool for GwsCalendarWriteTool {
    fn name(&self) -> &str { "gws_writable_calendar_insert" }

    fn description(&self) -> &str {
        &self.description_cache
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "calendar_id": {
                    "type": "string",
                    "description": "Target calendar ID. Must match the configured writable calendar."
                },
                "summary": {
                    "type": "string",
                    "description": "Event title"
                },
                "start_datetime": {
                    "type": "string",
                    "description": "Start datetime in RFC3339 format (e.g. '2026-06-01T10:00:00+09:00')"
                },
                "end_datetime": {
                    "type": "string",
                    "description": "End datetime in RFC3339 format"
                },
                "description": {
                    "type": "string",
                    "description": "Event description (optional)"
                }
            },
            "required": ["calendar_id", "summary", "start_datetime", "end_datetime"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        // 書き込み先 ID を検証 — 許可リストに含まれない場合はブロック
        let requested_id = args["calendar_id"].as_str().unwrap_or("").to_string();
        let is_allowed = self.allowed.iter().any(|(id, _)| id == &requested_id);
        if !is_allowed {
            let allowed_list: Vec<String> = self.allowed.iter()
                .map(|(id, name)| format!("'{}' ({})", name, id))
                .collect();
            return ToolResult {
                content: format!(
                    "WRITE BLOCKED: calendar '{}' is not in the writable list. \
                     Allowed: {}",
                    requested_id,
                    allowed_list.join(", ")
                ),
                is_error: true,
            };
        }

        let summary = match args["summary"].as_str() {
            Some(s) => s.to_string(),
            None => return ToolResult { content: "Missing summary".to_string(), is_error: true },
        };
        let start = match args["start_datetime"].as_str() {
            Some(s) => s.to_string(),
            None => return ToolResult { content: "Missing start_datetime".to_string(), is_error: true },
        };
        let end = match args["end_datetime"].as_str() {
            Some(s) => s.to_string(),
            None => return ToolResult { content: "Missing end_datetime".to_string(), is_error: true },
        };
        let description = args["description"].as_str().unwrap_or("").to_string();

        let body = serde_json::json!({
            "summary": summary,
            "description": description,
            "start": { "dateTime": start },
            "end": { "dateTime": end }
        });
        let params = serde_json::json!({ "calendarId": requested_id });

        let output = tokio::process::Command::new(&self.gws_path)
            .args(["calendar", "events", "insert",
                   "--params", &params.to_string(),
                   "--json", &body.to_string(),
                   "--format", "json"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let body = String::from_utf8_lossy(&out.stdout).to_string();
                ToolResult { content: body, is_error: false }
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).to_string();
                ToolResult { content: format!("gws calendar insert error: {}", err), is_error: true }
            }
            Err(e) => ToolResult { content: format!("gws calendar insert exec failed: {}", e), is_error: true },
        }
    }
}

/// Gmail のメッセージ一覧を取得するツール（gws CLI subprocess 経由）
pub struct GwsGmailTool {
    gws_path: String,
}

impl GwsGmailTool {
    pub fn new(gws_path: String) -> Self {
        Self { gws_path }
    }
}

#[async_trait]
impl Tool for GwsGmailTool {
    fn name(&self) -> &str { "gws_gmail_list_messages" }

    fn description(&self) -> &str {
        "Search and list Gmail messages. Returns sender, subject, snippet, and date. Use query syntax like 'is:unread', 'from:someone@example.com', 'subject:hello'."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Gmail search query (e.g. 'is:unread', 'from:boss@example.com')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Max messages to return (default: 10)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("is:unread");
        let max_results = args["max_results"].as_u64().unwrap_or(10);

        let params = serde_json::json!({
            "userId": "me",
            "q": query,
            "maxResults": max_results
        });

        let output = tokio::process::Command::new(&self.gws_path)
            .args(["gmail", "users", "messages", "list",
                   "--params", &params.to_string(),
                   "--format", "json"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let body = String::from_utf8_lossy(&out.stdout).to_string();
                ToolResult { content: body, is_error: false }
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).to_string();
                ToolResult { content: format!("gws gmail error: {}", err), is_error: true }
            }
            Err(e) => ToolResult { content: format!("gws gmail exec failed: {}", e), is_error: true },
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
    async fn test_karakeep_list_tool_name_and_schema() {
        let tool = KarakeepListTool::new(
            "http://localhost:33000".to_string(),
            "dummy-key".to_string(),
        );
        assert_eq!(tool.name(), "karakeep_list_bookmarks");
        assert!(tool.description().len() > 10);
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["limit"].is_object());
    }

    #[tokio::test]
    async fn test_karakeep_tag_tool_name_and_schema() {
        let tool = KarakeepTagTool::new(
            "http://localhost:33000".to_string(),
            "dummy-key".to_string(),
        );
        assert_eq!(tool.name(), "karakeep_tag_bookmark");
        assert!(tool.description().len() > 10);
        let params = tool.parameters();
        assert!(params["properties"]["bookmark_id"].is_object());
        assert!(params["properties"]["tag_name"].is_object());
        assert!(params["required"].as_array().unwrap().contains(&serde_json::json!("bookmark_id")));
        assert!(params["required"].as_array().unwrap().contains(&serde_json::json!("tag_name")));
    }

    #[tokio::test]
    async fn test_karakeep_tag_tool_missing_args() {
        let tool = KarakeepTagTool::new("http://localhost:33000".to_string(), "key".to_string());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("bookmark_id") || result.content.contains("Missing"));
    }

    #[tokio::test]
    async fn test_obsidian_search_tool_name_and_schema() {
        let tool = ObsidianSearchTool::new(
            "192.168.1.2".to_string(),
            "dummy-key".to_string(),
        );
        assert_eq!(tool.name(), "obsidian_search");
        assert!(tool.description().len() > 10);
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["query"].is_object());
        assert!(params["required"].as_array().unwrap().contains(&serde_json::json!("query")));
    }

    #[tokio::test]
    async fn test_obsidian_read_tool_name_and_schema() {
        let tool = ObsidianReadTool::new(
            "192.168.1.2".to_string(),
            "dummy-key".to_string(),
        );
        assert_eq!(tool.name(), "obsidian_read_note");
        assert!(tool.description().len() > 10);
        let params = tool.parameters();
        assert!(params["properties"]["path"].is_object());
        assert!(params["required"].as_array().unwrap().contains(&serde_json::json!("path")));
    }

    #[tokio::test]
    async fn test_obsidian_read_tool_missing_path() {
        let tool = ObsidianReadTool::new("192.168.1.2".to_string(), "key".to_string());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("path") || result.content.contains("Missing"));
    }

    #[tokio::test]
    async fn test_obsidian_search_tool_missing_query() {
        let tool = ObsidianSearchTool::new("192.168.1.2".to_string(), "key".to_string());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("query") || result.content.contains("Missing"));
    }

    #[tokio::test]
    async fn test_gws_calendar_tool_name_and_schema() {
        let tool = GwsCalendarTool::new("/usr/local/bin/gws".to_string());
        assert_eq!(tool.name(), "gws_calendar_list_events");
        assert!(tool.description().len() > 10);
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["calendar_id"].is_object());
    }

    #[tokio::test]
    async fn test_gws_gmail_tool_name_and_schema() {
        let tool = GwsGmailTool::new("/usr/local/bin/gws".to_string());
        assert_eq!(tool.name(), "gws_gmail_list_messages");
        assert!(tool.description().len() > 10);
        let params = tool.parameters();
        assert!(params["properties"]["query"].is_object());
    }

    #[tokio::test]
    async fn test_gws_calendar_write_blocks_unlisted_calendar() {
        let tool = GwsCalendarWriteTool::new(
            "/usr/local/bin/gws".to_string(),
            vec![("allowed-cal-id@group.calendar.google.com".to_string(), "AI AGENT".to_string())],
        );
        // 許可 ID 以外への書き込みはブロックされる
        let result = tool.execute(serde_json::json!({
            "calendar_id": "ayabe.kazuaki@gmail.com",
            "summary": "test",
            "start_datetime": "2026-06-01T10:00:00+09:00",
            "end_datetime":   "2026-06-01T11:00:00+09:00"
        })).await;
        assert!(result.is_error);
        assert!(result.content.contains("WRITE BLOCKED"));
    }

    #[tokio::test]
    async fn test_gws_calendar_write_requires_calendar_id() {
        let tool = GwsCalendarWriteTool::new(
            "/usr/local/bin/gws".to_string(),
            vec![
                ("allowed-cal@group.calendar.google.com".to_string(), "AI AGENT".to_string()),
            ],
        );
        // calendar_id なし → ブロック（空文字列は許可 ID と一致しない）
        let result = tool.execute(serde_json::json!({
            "summary": "test",
            "start_datetime": "2026-06-01T10:00:00+09:00",
            "end_datetime":   "2026-06-01T11:00:00+09:00"
        })).await;
        assert!(result.is_error);
        assert!(result.content.contains("WRITE BLOCKED"));
    }

    #[tokio::test]
    async fn test_gws_calendar_write_allows_multiple_calendars() {
        let tool = GwsCalendarWriteTool::new(
            "/usr/local/bin/gws".to_string(),
            vec![
                ("cal-a@group.calendar.google.com".to_string(), "AI AGENT".to_string()),
                ("cal-b@group.calendar.google.com".to_string(), "学習計画カレンダー".to_string()),
            ],
        );
        // 非許可 ID はブロック
        let blocked = tool.execute(serde_json::json!({
            "calendar_id": "personal@gmail.com",
            "summary": "test",
            "start_datetime": "2026-06-01T10:00:00+09:00",
            "end_datetime":   "2026-06-01T11:00:00+09:00"
        })).await;
        assert!(blocked.is_error);
        assert!(blocked.content.contains("WRITE BLOCKED"));

        // description に両カレンダー名が含まれる
        assert!(tool.description().contains("AI AGENT"));
        assert!(tool.description().contains("学習計画カレンダー"));
    }

    #[tokio::test]
    async fn test_gws_calendar_write_blocks_personal_calendar() {
        // 本番設定と同等の許可リスト
        let tool = GwsCalendarWriteTool::new(
            "/usr/local/bin/gws".to_string(),
            vec![
                (
                    "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com".to_string(),
                    "AI AGENT".to_string(),
                ),
                (
                    "d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com".to_string(),
                    "学習計画カレンダー".to_string(),
                ),
            ],
        );

        // 個人カレンダーへの書き込みはブロックされる
        let result = tool.execute(serde_json::json!({
            "calendar_id": "ayabe.kazuaki@gmail.com",
            "summary": "不正書き込みテスト",
            "start_datetime": "2026-06-01T10:00:00+09:00",
            "end_datetime":   "2026-06-01T11:00:00+09:00"
        })).await;
        assert!(result.is_error, "個人カレンダーへの書き込みはブロックされるべき");
        assert!(result.content.contains("WRITE BLOCKED"), "エラー内容: {}", result.content);
    }

    #[tokio::test]
    async fn test_gws_calendar_write_blocks_yuki_shared_calendar() {
        // 本番設定と同等の許可リスト
        let tool = GwsCalendarWriteTool::new(
            "/usr/local/bin/gws".to_string(),
            vec![
                (
                    "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com".to_string(),
                    "AI AGENT".to_string(),
                ),
                (
                    "d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com".to_string(),
                    "学習計画カレンダー".to_string(),
                ),
            ],
        );

        // ゆうき様共有カレンダーへの書き込みはブロックされる
        let result = tool.execute(serde_json::json!({
            "calendar_id": "28hs0ibka0oa84810dupunrskk@group.calendar.google.com",
            "summary": "不正書き込みテスト",
            "start_datetime": "2026-06-01T10:00:00+09:00",
            "end_datetime":   "2026-06-01T11:00:00+09:00"
        })).await;
        assert!(result.is_error, "共有カレンダーへの書き込みはブロックされるべき");
        assert!(result.content.contains("WRITE BLOCKED"), "エラー内容: {}", result.content);
    }
}
