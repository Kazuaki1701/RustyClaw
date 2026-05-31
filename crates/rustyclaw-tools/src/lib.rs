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
        "List Karakeep bookmarks (title, URL, tags, ID)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": { "type": "string", "description": "Max bookmarks as a string (default: '20')" }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let limit = match &args["limit"] {
            Value::Number(n) => n.as_u64().unwrap_or(20),
            Value::String(s) => s.parse::<u64>().unwrap_or(20),
            _ => 20,
        }.min(50);
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
        "Add a tag to a Karakeep bookmark by ID."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "bookmark_id": { "type": "string" },
                "tag_name": { "type": "string", "description": "e.g. '_recommended'" }
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

/// Karakeep のブックマークを削除するネイティブツール
pub struct KarakeepDeleteTool {
    server_addr: String,
    api_key: String,
}

impl KarakeepDeleteTool {
    pub fn new(server_addr: String, api_key: String) -> Self {
        Self { server_addr, api_key }
    }
}

#[async_trait]
impl Tool for KarakeepDeleteTool {
    fn name(&self) -> &str {
        "karakeep_delete_bookmark"
    }

    fn description(&self) -> &str {
        "Delete a Karakeep bookmark by ID (used for cleanup)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "bookmark_id": { "type": "string" }
            },
            "required": ["bookmark_id"]
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
        let client = reqwest::Client::new();
        let url = format!("{}/api/v1/bookmarks/{}", self.server_addr, bookmark_id);
        match client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => ToolResult {
                content: format!("Deleted bookmark {}", bookmark_id),
                is_error: false,
            },
            Ok(resp) => ToolResult {
                content: format!("Karakeep delete error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult {
                content: format!("Karakeep delete request failed: {}", e),
                is_error: true,
            },
        }
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
        "Search Obsidian vault by keyword. Returns note paths and excerpts."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "string", "description": "Max results as a string (default: '10')" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let query = match args["query"].as_str() {
            Some(q) => q.to_string(),
            None => return ToolResult { content: "Missing query".to_string(), is_error: true },
        };
        let limit = match &args["limit"] {
            Value::Number(n) => n.as_u64().unwrap_or(10),
            Value::String(s) => s.parse::<u64>().unwrap_or(10),
            _ => 10,
        } as usize;
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
        "Read a note from Obsidian vault by path."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "e.g. 'Folder/Note.md'" }
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

// ─── ObsidianWriteTool ───────────────────────────────────────────────────────

pub struct ObsidianWriteTool {
    host: String,
    api_key: String,
}

impl ObsidianWriteTool {
    pub fn new(host: String, api_key: String) -> Self {
        Self { host, api_key }
    }
}

#[async_trait]
impl Tool for ObsidianWriteTool {
    fn name(&self) -> &str { "obsidian_write_note" }

    fn description(&self) -> &str {
        "Create or update a note in Obsidian vault. Use mode 'append' to add content to an existing note, or 'write' to overwrite."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string", "description": "Vault-relative path, e.g. 'Folder/Note.md'" },
                "content": { "type": "string", "description": "Markdown content to write" },
                "mode":    { "type": "string", "description": "\"write\" (overwrite, default) or \"append\"" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => p.to_string(),
            None => return ToolResult { content: "Missing path".into(), is_error: true },
        };
        let new_content = match args["content"].as_str() {
            Some(c) => c.to_string(),
            None => return ToolResult { content: "Missing content".into(), is_error: true },
        };
        let mode = args["mode"].as_str().unwrap_or("write");
        let client = reqwest::Client::new();
        let url = format!("http://{}:27123/vault/{}", self.host, percent_encode(&path));
        let auth = format!("Bearer {}", self.api_key);

        let body = if mode == "append" {
            match client.get(&url).header("Authorization", &auth).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let existing = resp.text().await.unwrap_or_default();
                    format!("{}\n{}", existing.trim_end(), new_content)
                }
                _ => new_content.clone(),
            }
        } else {
            new_content
        };

        match client
            .put(&url)
            .header("Authorization", &auth)
            .header("Content-Type", "text/markdown")
            .body(body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 204 => {
                ToolResult { content: format!("Written to {}", path), is_error: false }
            }
            Ok(resp) => ToolResult {
                content: format!("Obsidian write error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult { content: format!("Obsidian write failed: {}", e), is_error: true },
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
        "List Google Calendar events (read-only). Use gws_writable_calendar_insert to create events."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "calendar_id": { "type": "string", "description": "default: 'primary'" },
                "max_results": { "type": "string", "description": "Max results as a string (default: '10')" },
                "time_min": { "type": "string", "description": "RFC3339 start (default: now)" }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let calendar_id = args["calendar_id"].as_str().unwrap_or("primary");
        let max_results = match &args["max_results"] {
            Value::Number(n) => n.as_u64().unwrap_or(10),
            Value::String(s) => s.parse::<u64>().unwrap_or(10),
            _ => 10,
        };

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
            "Create a Google Calendar event. Writable calendars: {}.",
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
                "calendar_id": { "type": "string" },
                "summary": { "type": "string", "description": "Event title" },
                "start_datetime": { "type": "string", "description": "RFC3339 e.g. '2026-06-01T10:00:00+09:00'" },
                "end_datetime": { "type": "string", "description": "RFC3339" },
                "description": { "type": "string" }
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
        "List Gmail messages. Query syntax: 'is:unread', 'from:x@y.com', 'subject:z'."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Gmail search query" },
                "max_results": { "type": "string", "description": "Max results as a string (default: '10')" }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("is:unread");
        let max_results = match &args["max_results"] {
            Value::Number(n) => n.as_u64().unwrap_or(10),
            Value::String(s) => s.parse::<u64>().unwrap_or(10),
            _ => 10,
        };

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

/// Gmail メール削除ツール（gws CLI subprocess 経由）
/// config の GWS_GMAIL_DELETABLE_LABEL に設定されたラベルを持つメールのみ削除可能
pub struct GwsGmailDeleteTool {
    gws_path: String,
    /// 削除を許可するラベル名（例: "_ai-agent"）
    deletable_label: String,
}

impl GwsGmailDeleteTool {
    pub fn new(gws_path: String, deletable_label: String) -> Self {
        Self { gws_path, deletable_label }
    }

    /// labelIds リストに削除許可ラベルが含まれるかチェック（テスト可能な純粋関数）
    pub fn has_deletable_label(&self, label_ids: &[&str], label_name_to_id: Option<&str>) -> bool {
        // labelIds に直接ラベル名が含まれる場合
        if label_ids.iter().any(|id| id.to_lowercase() == self.deletable_label.to_lowercase()) {
            return true;
        }
        // API から解決したラベル ID が含まれる場合
        if let Some(lid) = label_name_to_id {
            return label_ids.contains(&lid);
        }
        false
    }
}

#[async_trait]
impl Tool for GwsGmailDeleteTool {
    fn name(&self) -> &str { "gws_gmail_trash_message" }

    fn description(&self) -> &str {
        "Trash a Gmail message (only messages with the configured deletable label)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message_id": { "type": "string" }
            },
            "required": ["message_id"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let message_id = match args["message_id"].as_str() {
            Some(id) => id.to_string(),
            None => return ToolResult { content: "Missing message_id".to_string(), is_error: true },
        };

        // 削除前にメッセージのラベルを取得して確認
        let params = serde_json::json!({ "userId": "me", "id": &message_id }).to_string();
        let label_check = tokio::process::Command::new(&self.gws_path)
            .args(["gmail", "users", "messages", "get",
                   "--params", &params, "--format", "json"])
            .output()
            .await;

        match label_check {
            Ok(out) if out.status.success() => {
                let body = String::from_utf8_lossy(&out.stdout);
                let msg: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => return ToolResult {
                        content: format!("Failed to parse message: {}", e),
                        is_error: true,
                    },
                };
                // labelIds にラベルが含まれるか確認
                let label_ids = msg["labelIds"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                // ラベル名→ID の変換は省略。labelIds にラベル名そのものが含まれる場合もあるため
                // gws でラベル名検索して ID を解決する
                let has_label = label_ids.iter().any(|id| {
                    id.to_lowercase() == self.deletable_label.to_lowercase()
                });

                if !has_label {
                    // ラベル ID ではなくラベル名で照合するため gws labels list で確認
                    let labels_params = serde_json::json!({"userId": "me"}).to_string();
                    let labels_out = tokio::process::Command::new(&self.gws_path)
                        .args(["gmail", "users", "labels", "list",
                               "--params", &labels_params, "--format", "json"])
                        .output()
                        .await;

                    let label_id = labels_out.ok()
                        .filter(|o| o.status.success())
                        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
                        .and_then(|v| {
                            v["labels"].as_array()?.iter()
                                .find(|l| l["name"].as_str().unwrap_or("").to_lowercase()
                                    == self.deletable_label.to_lowercase())
                                .and_then(|l| l["id"].as_str().map(|s| s.to_string()))
                        });

                    let confirmed = label_id
                        .map(|lid| label_ids.contains(&lid.as_str()))
                        .unwrap_or(false);

                    if !confirmed {
                        return ToolResult {
                            content: format!(
                                "DELETE BLOCKED: message '{}' does not have the required label '{}'. \
                                 Only messages with this label can be trashed.",
                                message_id, self.deletable_label
                            ),
                            is_error: true,
                        };
                    }
                }

                // ラベル確認済み → trash に移動
                let trash_params = serde_json::json!({"userId": "me", "id": &message_id}).to_string();
                let trash_out = tokio::process::Command::new(&self.gws_path)
                    .args(["gmail", "users", "messages", "trash",
                           "--params", &trash_params, "--format", "json"])
                    .output()
                    .await;

                match trash_out {
                    Ok(o) if o.status.success() => ToolResult {
                        content: format!("Message '{}' moved to trash.", message_id),
                        is_error: false,
                    },
                    Ok(o) => ToolResult {
                        content: format!("Trash failed: {}", String::from_utf8_lossy(&o.stderr)),
                        is_error: true,
                    },
                    Err(e) => ToolResult {
                        content: format!("gws gmail trash exec failed: {}", e),
                        is_error: true,
                    },
                }
            }
            Ok(o) => ToolResult {
                content: format!("Failed to get message labels: {}", String::from_utf8_lossy(&o.stderr)),
                is_error: true,
            },
            Err(e) => ToolResult {
                content: format!("gws gmail get exec failed: {}", e),
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

pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self { Self }
}

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
    async fn test_karakeep_delete_tool_name_and_schema() {
        let tool = KarakeepDeleteTool::new("http://localhost:33000".to_string(), "key".to_string());
        assert_eq!(tool.name(), "karakeep_delete_bookmark");
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["bookmark_id"].is_object());
        assert!(params["required"].as_array().unwrap().contains(&serde_json::json!("bookmark_id")));
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

    #[tokio::test]
    async fn test_gws_calendar_write_guard_passes_for_ai_agent_calendar() {
        // AI AGENT カレンダーは許可リストに含まれるのでガードを通過する
        // (gws バイナリが存在しないためその後 exec エラーになるが WRITE BLOCKED にはならない)
        let ai_agent_id = "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com";
        let tool = GwsCalendarWriteTool::new(
            "/nonexistent/gws".to_string(),  // 存在しないパスで exec エラーを発生させる
            vec![
                (ai_agent_id.to_string(), "AI AGENT".to_string()),
                (
                    "d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com".to_string(),
                    "学習計画カレンダー".to_string(),
                ),
            ],
        );

        let result = tool.execute(serde_json::json!({
            "calendar_id": ai_agent_id,
            "summary": "テストイベント",
            "start_datetime": "2026-06-01T10:00:00+09:00",
            "end_datetime":   "2026-06-01T11:00:00+09:00"
        })).await;

        // ガードは通過している（WRITE BLOCKED でない）
        assert!(!result.content.contains("WRITE BLOCKED"),
            "AI AGENT カレンダーは WRITE BLOCKED されてはいけない。内容: {}", result.content);
        // gws バイナリが存在しないので exec エラー
        assert!(result.is_error);
        assert!(result.content.contains("exec") || result.content.contains("gws"),
            "exec エラーが期待される。内容: {}", result.content);
    }

    #[test]
    fn test_gmail_delete_label_check_blocks_without_label() {
        let tool = GwsGmailDeleteTool::new("/nonexistent/gws".to_string(), "_ai-agent".to_string());
        // ラベルなし → 削除不可
        assert!(!tool.has_deletable_label(&["INBOX", "UNREAD"], None));
    }

    #[test]
    fn test_gmail_delete_label_check_passes_with_label_name() {
        let tool = GwsGmailDeleteTool::new("/nonexistent/gws".to_string(), "_ai-agent".to_string());
        // labelIds にラベル名が直接含まれる → 削除可
        assert!(tool.has_deletable_label(&["INBOX", "_ai-agent"], None));
    }

    #[test]
    fn test_gmail_delete_label_check_passes_with_label_id() {
        let tool = GwsGmailDeleteTool::new("/nonexistent/gws".to_string(), "_ai-agent".to_string());
        // API が解決したラベル ID で照合 → 削除可
        assert!(tool.has_deletable_label(&["INBOX", "Label_12345"], Some("Label_12345")));
    }

    #[test]
    fn test_gmail_delete_label_check_case_insensitive() {
        let tool = GwsGmailDeleteTool::new("/nonexistent/gws".to_string(), "_AI-AGENT".to_string());
        // 大文字小文字を区別しない
        assert!(tool.has_deletable_label(&["_ai-agent"], None));
    }

    #[tokio::test]
    async fn test_gmail_delete_requires_message_id() {
        let tool = GwsGmailDeleteTool::new("/nonexistent/gws".to_string(), "_ai-agent".to_string());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("message_id") || result.content.contains("Missing"));
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
    async fn test_yolp_weather_tool_invalid_coords() {
        let tool = YolpWeatherTool::new();
        let res = tool.execute(serde_json::json!({ "coordinates": "invalid" })).await;
        assert!(res.is_error);
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
    async fn test_obsidian_write_tool_schema() {
        let tool = ObsidianWriteTool::new("192.168.1.2".to_string(), "key".to_string());
        assert_eq!(tool.name(), "obsidian_write_note");
        let params = tool.parameters();
        assert!(params["properties"]["path"].is_object());
        assert!(params["properties"]["content"].is_object());
    }

    #[tokio::test]
    async fn test_obsidian_write_missing_path() {
        let tool = ObsidianWriteTool::new("192.168.1.2".to_string(), "key".to_string());
        let res = tool.execute(serde_json::json!({"content": "hello"})).await;
        assert!(res.is_error);
        assert!(res.content.contains("Missing path"));
    }

    #[tokio::test]
    async fn test_obsidian_write_missing_content() {
        let tool = ObsidianWriteTool::new("192.168.1.2".to_string(), "key".to_string());
        let res = tool.execute(serde_json::json!({"path": "Test/Note.md"})).await;
        assert!(res.is_error);
        assert!(res.content.contains("Missing content"));
    }
}

// ─── YolpWeatherTool ─────────────────────────────────────────────────────────

pub struct YolpWeatherTool;

impl YolpWeatherTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for YolpWeatherTool {
    fn name(&self) -> &str { "yolp_weather" }

    fn description(&self) -> &str {
        "Get Yahoo YOLP-compatible weather forecast (rainfall prediction up to 60 mins in 10-min intervals) for given coordinates (longitude,latitude)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "coordinates": {
                    "type": "string",
                    "description": "Location coordinates formatted as 'longitude,latitude' (e.g. '139.732293,35.663613'). NOTE: Longitude comes first."
                },
                "output": {
                    "type": "string",
                    "description": "Output format, typically 'json'."
                }
            },
            "required": ["coordinates"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let coords = match args["coordinates"].as_str() {
            Some(c) => c.to_string(),
            None => return ToolResult { content: "Missing coordinates".into(), is_error: true },
        };

        let parts: Vec<&str> = coords.split(',').collect();
        if parts.len() != 2 {
            return ToolResult {
                content: format!("Invalid coordinates format: '{}'. Expected 'longitude,latitude'", coords),
                is_error: true,
            };
        }

        let lon: f64 = match parts[0].trim().parse() {
            Ok(v) => v,
            Err(e) => return ToolResult { content: format!("Invalid longitude: {}. Error: {}", parts[0], e), is_error: true },
        };
        let lat: f64 = match parts[1].trim().parse() {
            Ok(v) => v,
            Err(e) => return ToolResult { content: format!("Invalid latitude: {}. Error: {}", parts[1], e), is_error: true },
        };

        match fetch_open_meteo_weather(lat, lon).await {
            Ok(weather_list) => {
                let response_json = serde_json::json!({
                    "Feature": [
                        {
                            "Name": format!("地点({},{})の天気情報", lon, lat),
                            "Property": {
                                "WeatherList": {
                                    "Weather": weather_list
                                }
                            }
                        }
                    ]
                });
                ToolResult {
                    content: serde_json::to_string(&response_json).unwrap_or_default(),
                    is_error: false,
                }
            }
            Err(e) => ToolResult {
                content: format!("Failed to fetch weather: {}", e),
                is_error: true,
            }
        }
    }
}

async fn fetch_open_meteo_weather(lat: f64, lon: f64) -> Result<Vec<Value>, anyhow::Error> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&minutely_15=precipitation&timezone=Asia%2FTokyo",
        lat, lon
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("Open-Meteo API returned status {}", resp.status()));
    }

    let json: Value = resp.json().await?;
    let minutely_15 = json["minutely_15"].as_object()
        .ok_or_else(|| anyhow::anyhow!("Missing 'minutely_15' in Open-Meteo response"))?;

    let times = minutely_15["time"].as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing 'time' in minutely_15"))?;
    let precipitations = minutely_15["precipitation"].as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing 'precipitation' in minutely_15"))?;

    if times.len() != precipitations.len() {
        return Err(anyhow::anyhow!("Mismatched time and precipitation array lengths"));
    }

    use chrono::{DateTime, Local, TimeZone, Duration};

    let parse_time = |s: &str| -> Option<DateTime<Local>> {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
            return Local.from_local_datetime(&naive).single();
        }
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Local.from_local_datetime(&naive).single();
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.with_timezone(&Local));
        }
        None
    };

    let now = Local::now();
    let mut weather_entries = Vec::new();

    for offset_mins in [0, 10, 20, 30, 40, 50, 60] {
        let target_time = now + Duration::minutes(offset_mins);
        
        let mut before: Option<(DateTime<Local>, f64)> = None;
        let mut after: Option<(DateTime<Local>, f64)> = None;

        for (t_val, p_val) in times.iter().zip(precipitations.iter()) {
            let t_str = t_val.as_str().unwrap_or("");
            let p = p_val.as_f64().unwrap_or(0.0);

            if let Some(t) = parse_time(t_str) {
                if t <= target_time {
                    if before.is_none() || t > before.unwrap().0 {
                        before = Some((t, p));
                    }
                }
                if t >= target_time {
                    if after.is_none() || t < after.unwrap().0 {
                        after = Some((t, p));
                    }
                }
            }
        }

        let mut val = 0.0;
        if let (Some((t_b, p_b)), Some((t_a, p_a))) = (before, after) {
            if t_b == t_a {
                val = p_b;
            } else {
                let total_dur = (t_a - t_b).num_seconds() as f64;
                let elapsed = (target_time - t_b).num_seconds() as f64;
                val = p_b + (p_a - p_b) * (elapsed / total_dur);
            }
        } else if let Some((_, p_b)) = before {
            val = p_b;
        } else if let Some((_, p_a)) = after {
            val = p_a;
        }

        val = (val * 100.0).round() / 100.0;

        let entry_type = if offset_mins == 0 { "observation" } else { "forecast" };
        let date_str = target_time.format("%Y%m%d%H%M").to_string();

        weather_entries.push(serde_json::json!({
            "Type": entry_type,
            "Date": date_str,
            "Rainfall": val
        }));
    }

    Ok(weather_entries)
}
