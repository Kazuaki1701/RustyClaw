# Phase 40-2: rig-core Tool トレイト移行 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 独自の `rustyclaw_tools::Tool` トレイトと `RigToolAdapter` を削除し、すべてのツールが `rig_core::tool::Tool` トレイトを直接実装するよう移行する。

**Architecture:** 各ツール struct に `rig_core::tool::Tool` を直接実装し、typed `Args` struct で引数を受け取る。`rig_core` の blanket impl `impl<T: Tool> ToolDyn for T` により `RigToolAdapter` ラッパーが不要になる。`ToolRegistry` は `HashMap<String, Arc<dyn rig_core::tool::ToolDyn>>` に変更。Agent の `execute_heartbeat` / `execute_with_tools` の tool 呼び出しを `ToolDyn::call(json_str)` に統一する。

**Tech Stack:** Rust 2024 edition, rig-core 0.38, serde_json, tokio

---

## File Structure

| ファイル | 変更内容 |
|---------|---------|
| `crates/rustyclaw-tools/src/lib.rs` | 独自 `Tool` トレイト削除・各ツールに `rig_core::tool::Tool` 実装・`ToolRegistry` 型変更 |
| `crates/rustyclaw-tools/Cargo.toml` | `async-trait` dep 削除 |
| `crates/rustyclaw-agent/src/lib.rs` | `execute_heartbeat` / `execute_with_tools` のツール呼び出し書き換え |
| `crates/rustyclaw-gateway/src/lib.rs` | ツール登録ブロック書き換え（`RigToolAdapter` 除去） |

---

## Task 1: 共有インフラ + WebFetchTool テンプレート実装

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

### 背景知識

`rig_core::tool::Tool` トレイト（`~/.cargo/registry/…/rig-core-0.38.1/src/tool/mod.rs`）の定義：

```rust
pub trait Tool: Sized + WasmCompatSend + WasmCompatSync {
    const NAME: &'static str;
    type Error: std::error::Error + WasmCompatSend + WasmCompatSync + 'static;
    type Args: for<'a> Deserialize<'a> + WasmCompatSend + WasmCompatSync;
    type Output: Serialize;
    fn name(&self) -> String { Self::NAME.to_string() }
    async fn definition(&self, _prompt: String) -> rig_core::completion::ToolDefinition;
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>;
}
```

blanket impl `impl<T: Tool> ToolDyn for T` が `ToolDyn::call(args: String)` を自動実装する。
`call(args_str)` は内部で `serde_json::from_str::<T::Args>(&args_str)` してから `Tool::call()` を呼ぶ。

- `[ ]` **Step 1: 失敗するテストを書く**

`crates/rustyclaw-tools/src/lib.rs` の `mod tests` ブロック末尾（行 1206 の `}` 手前）に追加：

```rust
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
```

- `[ ]` **Step 2: テスト失敗を確認**

```bash
cargo test -p rustyclaw-tools test_web_fetch_rig_core -- --nocapture 2>&1 | head -20
```

Expected: コンパイルエラー `error[E0599]: no associated item named 'NAME'`

- `[ ]` **Step 3: 共有エラー型 + WebFetchArgs + WebFetchTool rig-core impl を追加**

`crates/rustyclaw-tools/src/lib.rs` の `pub struct WebFetchTool;`（行 524）より前（`// ─── WebFetchTool ─────` コメントの直前）に以下を挿入：

```rust
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
```

続いて `pub struct WebFetchTool;`（行 524）の直前に Args struct を追加し、既存の `impl Tool for WebFetchTool` ブロック（`#[async_trait]` で始まる）の **後に** `rig_core::tool::Tool` impl を追加：

```rust
// ─── WebFetchTool ────────────────────────────────────────────────────────────

/// HTML タグ・script・style ブロックを除去してプレーンテキストを返す（新規 crate 依存なし）
fn strip_html_to_text(html: &str, max_chars: usize) -> String {
    // ... 既存の実装はそのまま ...
}

#[derive(serde::Deserialize)]
pub struct WebFetchArgs {
    pub url: String,
    #[serde(default = "web_fetch_default_max_chars")]
    pub max_chars: usize,
}

fn web_fetch_default_max_chars() -> usize { 3000 }

#[derive(Clone)]
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self { Self }
}

// 既存の #[async_trait] impl Tool for WebFetchTool { ... } は一旦そのまま残す

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
```

- `[ ]` **Step 4: テスト通過を確認**

```bash
cargo test -p rustyclaw-tools test_web_fetch_rig_core -- --nocapture
```

Expected: `test result: ok. 3 passed`

- `[ ]` **Step 5: 全テストが引き続き通過することを確認**

```bash
cargo test -p rustyclaw-tools 2>&1 | tail -5
```

Expected: `test result: ok. XX passed; 0 failed`

- `[ ]` **Step 6: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(tools): add ToolCallError + WebFetchTool rig-core::Tool impl"
```

---

## Task 2: WorkspaceReadTool + WorkspaceWriteTool 移行

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- `[ ]` **Step 1: 失敗するテストを追加**

`mod tests` ブロック末尾（Task 1 で追加したテストの後）に追加：

```rust
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
```

- `[ ]` **Step 2: テスト失敗を確認**

```bash
cargo test -p rustyclaw-tools test_workspace_read_rig_core test_workspace_write_rig_core -- --nocapture 2>&1 | head -20
```

Expected: コンパイルエラー（`rig_core::tool::Tool` not implemented）

- `[ ]` **Step 3: WorkspaceReadArgs + rig-core impl を追加**

`// ─── WorkspaceReadTool ─────` コメントの直前（行 224 付近）に Args struct を追加し、既存 `impl Tool for WorkspaceReadTool` ブロックの後に rig-core impl を追加：

```rust
#[derive(serde::Deserialize)]
pub struct WorkspaceReadArgs {
    pub path: String,
}
```

既存 `impl Tool for WorkspaceReadTool { ... }` の **後**（行 270 付近）に追加：

```rust
#[derive(Clone)]  // WorkspaceReadTool struct 定義に追加（struct の行を置き換え）

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
```

- `[ ]` **Step 4: WorkspaceWriteArgs + rig-core impl を追加**

`// ─── WorkspaceWriteTool ─────` コメントの直前（行 272 付近）に追加：

```rust
#[derive(serde::Deserialize)]
pub struct WorkspaceWriteArgs {
    pub path: String,
    pub content: String,
    pub mode: Option<String>,
}
```

既存 `impl Tool for WorkspaceWriteTool { ... }` ブロックの後に追加：

```rust
#[derive(Clone)]  // WorkspaceWriteTool struct 定義に追加

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
```

- `[ ]` **Step 5: テスト通過を確認**

```bash
cargo test -p rustyclaw-tools test_workspace_read_rig_core test_workspace_write_rig_core -- --nocapture
```

Expected: `test result: ok. 3 passed`

- `[ ]` **Step 6: 全テスト通過を確認**

```bash
cargo test -p rustyclaw-tools 2>&1 | tail -3
```

- `[ ]` **Step 7: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(tools): WorkspaceReadTool + WorkspaceWriteTool rig-core::Tool impl"
```

---

## Task 3: 残りのツール（MemorySearch / WebSearch / CronSchedule / WorkspaceExecuteScript）

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- `[ ]` **Step 1: 失敗するテストを追加**

```rust
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
```

- `[ ]` **Step 2: テスト失敗を確認**

```bash
cargo test -p rustyclaw-tools "test_memory_search_rig_core|test_web_search_rig_core|test_cron_schedule_rig_core|test_workspace_execute_script_rig_core" -- --nocapture 2>&1 | head -20
```

Expected: コンパイルエラー

- `[ ]` **Step 3: MemorySearchArgs + rig-core impl を追加**

`// ─── MemorySearchTool ─────` コメントの直前に追加：

```rust
#[derive(serde::Deserialize)]
pub struct MemorySearchArgs {
    pub query: String,
}
```

既存 `impl Tool for MemorySearchTool { ... }` の後に追加：

```rust
#[derive(Clone)]  // MemorySearchTool struct 定義に追加

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
            Err(e) => Err(ToolCallError(format!("Memory index not available: {}", e))),
        }
    }
}
```

- `[ ]` **Step 4: WebSearchArgs + rig-core impl を追加**

`// ─── WebSearchTool ─────` コメントの直前に追加：

```rust
#[derive(serde::Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
    #[serde(default = "web_search_default_count")]
    pub count: u64,
}

fn web_search_default_count() -> u64 { 5 }
```

既存 `impl Tool for WebSearchTool { ... }` の後に追加：

```rust
#[derive(Clone)]  // WebSearchTool struct 定義に追加

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
                    "count": { "type": "string", "description": "Number of results as a string (default: '5', max: '10')" }
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
```

- `[ ]` **Step 5: CronScheduleArgs + rig-core impl を追加**

`/// Dynamic scheduled cron tasks scheduler retriever` コメントの直前に追加：

```rust
#[derive(serde::Deserialize)]
pub struct CronScheduleArgs {}
```

既存 `impl Tool for CronScheduleTool { ... }` の後に追加：

```rust
// CronScheduleTool struct 定義の derive に Clone を追加
// pub struct CronScheduleTool { schedule_fn: std::sync::Arc<dyn Fn() -> serde_json::Value + Send + Sync> }
// Arc<dyn Fn()> は Clone なので、derive(Clone) が機能する

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
```

- `[ ]` **Step 6: WorkspaceExecuteScriptArgs + rig-core impl を追加**

`/// workspace/scripts/ 配下のスクリプトを実行するツール` コメントの直前に追加：

```rust
#[derive(serde::Deserialize)]
pub struct WorkspaceExecuteScriptArgs {
    pub script_name: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}
```

既存 `impl Tool for WorkspaceExecuteScriptTool { ... }` の後に追加：

```rust
#[derive(Clone)]  // WorkspaceExecuteScriptTool struct 定義に追加

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
                        "description": "The script path relative to workspace. Examples: '500_get-vital-data-garmin.sh' (workspace/scripts/) or 'skills/vitals-coach/scripts/500_get-vital-data-garmin.sh' (localized). Path traversal sequences like '..' or absolute paths are forbidden."
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
            return Err(ToolCallError(format!("Error: Script '{}' does not exist at expected location: {:?}", script_name, script_path)));
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
                if !stderr.is_empty() { content.push_str(&format!("\n--- STDERR ---\n{}", stderr)); }
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
```

- `[ ]` **Step 7: テスト通過を確認**

```bash
cargo test -p rustyclaw-tools "test_memory_search_rig_core|test_web_search_rig_core|test_cron_schedule_rig_core|test_workspace_execute_script_rig_core" -- --nocapture
```

Expected: `test result: ok. 4 passed`

- `[ ]` **Step 8: 全テスト通過を確認**

```bash
cargo test -p rustyclaw-tools 2>&1 | tail -3
```

- `[ ]` **Step 9: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(tools): MemorySearch/WebSearch/CronSchedule/WorkspaceExecuteScript rig-core::Tool impl"
```

---

## Task 4: ToolRegistry 移行

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`
- Modify: `crates/rustyclaw-tools/Cargo.toml`

`ToolRegistry` が `Arc<dyn rig_core::tool::ToolDyn>` を保持するよう変更し、`to_llm_schemas()` を async `tool_definitions()` に置き換える。

- `[ ]` **Step 1: 失敗するテストを追加**

```rust
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
```

- `[ ]` **Step 2: テスト失敗を確認**

```bash
cargo test -p rustyclaw-tools test_tool_registry_rig_core -- --nocapture 2>&1 | head -20
```

Expected: コンパイルエラー（`register` 引数の型不一致、`tool_definitions` not found）

- `[ ]` **Step 3: ToolRegistry を書き換える**

`lib.rs` の `pub struct ToolRegistry` と `impl ToolRegistry` 全体を以下に置き換える：

```rust
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
```

注意: 旧 `to_llm_schemas()`, `to_toolset()`, `to_dyn_tools()` メソッドはまだこのタスクでは削除しない（Agent / Gateway が依存しているため Task 5/6 で削除する）。ただし旧メソッドが `Arc<dyn Tool>` を受け取る箇所はコンパイルエラーになるため、旧メソッドも削除または更新する必要がある。

旧 `to_llm_schemas()` を以下に変更（暫定ブリッジ — Task 5 完了後に削除）:
```rust
    /// Deprecated: use tool_definitions().await instead.
    /// Returns schemas as Vec<serde_json::Value> for compatibility.
    #[deprecated]
    pub fn to_llm_schemas_sync_compat(&self) -> Vec<serde_json::Value> {
        // この関数は使われなくなるが、旧テストが参照している間は残す
        Vec::new()
    }
```

旧 `to_toolset()` と `to_dyn_tools()` は削除する（Task 5/6 で不要になる）。

- `[ ]` **Step 4: Gateway の登録コードを暫定修正（コンパイル通過用）**

`crates/rustyclaw-gateway/src/lib.rs` の各 `tool_registry.register(t.clone())` 呼び出しを以下パターンに変更：

変更前（例）:
```rust
let t = Arc::new(rustyclaw_tools::WebFetchTool::new());
tool_registry.register(t.clone());
tool_server_handle.add_tool(rustyclaw_tools::RigToolAdapter::new(t)).await.ok();
```

変更後（全6箇所を同様に変更）:
```rust
let t = rustyclaw_tools::WebFetchTool::new();
tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
tool_server_handle.add_tool(t).await.ok();
```

WorkspaceReadTool の例:
```rust
let t = rustyclaw_tools::WorkspaceReadTool::new(self.workspace_path.clone());
tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
tool_server_handle.add_tool(t).await.ok();
```

WorkspaceWriteTool:
```rust
let t = rustyclaw_tools::WorkspaceWriteTool::new(self.workspace_path.clone());
tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
tool_server_handle.add_tool(t).await.ok();
```

MemorySearchTool:
```rust
let t = rustyclaw_tools::MemorySearchTool::new(self.workspace_path.clone());
tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
tool_server_handle.add_tool(t).await.ok();
```

WorkspaceExecuteScriptTool:
```rust
let t = rustyclaw_tools::WorkspaceExecuteScriptTool::new(self.workspace_path.clone());
tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
tool_server_handle.add_tool(t).await.ok();
```

WebSearchTool（api_key あり）:
```rust
if let Some(b) = config.tools.brave_search.as_ref().filter(|b| b.enabled) {
    if !b.api_key.is_empty() {
        let t = rustyclaw_tools::WebSearchTool::new(b.api_key.clone());
        tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
        tool_server_handle.add_tool(t).await.ok();
        tracing::info!("Registered WebSearchTool (Brave Search).");
    }
}
```

CronScheduleTool（クロージャ付き）:
```rust
let t = rustyclaw_tools::CronScheduleTool::new(move || { ... });
tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
tool_server_handle.add_tool(t).await.ok();
```

- `[ ]` **Step 5: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep -E "error|warning: unused" | head -20
```

Expected: エラーなし（`RigToolAdapter` 未使用の警告が出るかもしれないが、Task 6 で削除する）

- `[ ]` **Step 6: テスト通過を確認**

```bash
cargo test -p rustyclaw-tools test_tool_registry_rig_core -- --nocapture
cargo test -p rustyclaw-tools 2>&1 | tail -3
```

- `[ ]` **Step 7: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(tools): ToolRegistry migrated to Arc<dyn rig_core::tool::ToolDyn>"
```

---

## Task 5: Agent の execute_heartbeat / execute_with_tools 移行

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

`to_llm_schemas()` の呼び出しを `tool_definitions().await` に、`tool.execute(args: Value)` を `tool.call(args_str)` に置き換える。

- `[ ]` **Step 1: 失敗するテストを追加**

`crates/rustyclaw-agent/src/lib.rs` の `#[cfg(test)] mod tests` ブロックを確認し、既存の `test_pipeline_execute_with_tools` テスト内で `tool_registry.register` が旧 API を使っていれば更新する。また新規テストを追加：

```rust
    #[tokio::test]
    async fn test_execute_with_tools_rig_core_registry() {
        use std::sync::Arc;
        use rustyclaw_tools::{ToolRegistry, ToolCallError};

        struct EchoTool;
        impl rig_core::tool::Tool for EchoTool {
            const NAME: &'static str = "echo";
            type Error = ToolCallError;
            type Args = serde_json::Value;
            type Output = String;
            async fn definition(&self, _: String) -> rig_core::completion::ToolDefinition {
                rig_core::completion::ToolDefinition {
                    name: "echo".into(),
                    description: "echo".into(),
                    parameters: serde_json::json!({"type":"object","properties":{}}),
                }
            }
            async fn call(&self, _args: serde_json::Value) -> Result<String, ToolCallError> {
                Ok("echoed".into())
            }
        }

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool) as Arc<dyn rig_core::tool::ToolDyn>);
        let defs = registry.tool_definitions().await;
        assert_eq!(defs[0].name, "echo");
    }
```

- `[ ]` **Step 2: `execute_heartbeat` の tool schemas 取得を更新**

`crates/rustyclaw-agent/src/lib.rs` の `execute_heartbeat`（行 631）内、`let provider_tools:` で始まるブロック（行 659〜667 付近）を以下に置き換える：

変更前:
```rust
        let provider_tools: Vec<rustyclaw_providers::ToolDef> = tool_registry
            .to_llm_schemas()
            .iter()
            .map(|schema| rustyclaw_providers::ToolDef {
                name: schema["name"].as_str().unwrap_or("").to_string(),
                description: schema["description"].as_str().unwrap_or("").to_string(),
                parameters: schema["input_schema"].clone(),
            })
            .collect();
```

変更後:
```rust
        let provider_tools: Vec<rustyclaw_providers::ToolDef> = tool_registry
            .tool_definitions().await
            .into_iter()
            .map(|d| rustyclaw_providers::ToolDef {
                name: d.name,
                description: d.description,
                parameters: d.parameters,
            })
            .collect();
```

- `[ ]` **Step 3: `execute_heartbeat` のツール呼び出しを更新**

`execute_heartbeat` 内のツール実行ブロック（`let tool_result = if let Some(tool)...` 付近）を置き換える：

変更前:
```rust
                        let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                            .unwrap_or_else(|_| serde_json::json!({}));
                        let tool_result = if let Some(tool) = tool_registry.get(&call.function.name) {
                            tool.execute(args).await
                        } else {
                            rustyclaw_tools::ToolResult {
                                content: format!("Error: Tool '{}' not found in registry", call.function.name),
                                is_error: true,
                            }
                        };
                        let tool_msg = Message {
                            role: "tool".to_string(),
                            content: tool_result.content,
```

変更後:
```rust
                        let tool_content = if let Some(tool) = tool_registry.get(&call.function.name) {
                            match tool.call(call.function.arguments.clone()).await {
                                Ok(s) => s,
                                Err(e) => {
                                    tracing::warn!("Tool {} returned error: {}", call.function.name, e);
                                    format!("Tool error: {}", e)
                                }
                            }
                        } else {
                            format!("Error: Tool '{}' not found in registry", call.function.name)
                        };
                        let tool_msg = Message {
                            role: "tool".to_string(),
                            content: tool_content,
```

- `[ ]` **Step 4: `execute_with_tools` の同箇所を同様に更新**

`execute_with_tools`（行 984〜）内の `let provider_tools` 生成ブロックを同じパターンで更新：

変更前:
```rust
        let provider_tools: Vec<rustyclaw_providers::ToolDef> = tool_registry
            .to_llm_schemas()
            .iter()
            .map(|schema| rustyclaw_providers::ToolDef {
                name: schema["name"].as_str().unwrap_or("").to_string(),
                description: schema["description"].as_str().unwrap_or("").to_string(),
                parameters: schema["input_schema"].clone(),
            })
            .collect();
```

変更後:
```rust
        let provider_tools: Vec<rustyclaw_providers::ToolDef> = tool_registry
            .tool_definitions().await
            .into_iter()
            .map(|d| rustyclaw_providers::ToolDef {
                name: d.name,
                description: d.description,
                parameters: d.parameters,
            })
            .collect();
```

また `execute_with_tools` 内のツール実行ブロックも同様に更新（Step 3 と同じパターン）。

- `[ ]` **Step 5: 既存の `test_pipeline_execute_with_tools` テストを更新**

`lib.rs` の `test_pipeline_execute_with_tools` テスト（行 2190 付近）内で `ToolRegistry::register` を使っている箇所を `Arc<dyn rig_core::tool::ToolDyn>` 版に更新する。モック `AddTool` の impl も `rig_core::tool::Tool` に変更する（`async_trait` なし）：

変更前テスト内:
```rust
            struct MockTool; // ...
            #[async_trait]
            impl Tool for MockTool { ... fn execute(&self, args: Value) -> ... }
            let mut registry = ToolRegistry::new();
            registry.register(Arc::new(MockTool));
```

変更後:
```rust
            struct MockTool;
            impl rig_core::tool::Tool for MockTool {
                const NAME: &'static str = "calculate";
                type Error = rustyclaw_tools::ToolCallError;
                type Args = serde_json::Value;
                type Output = String;
                async fn definition(&self, _: String) -> rig_core::completion::ToolDefinition {
                    rig_core::completion::ToolDefinition {
                        name: "calculate".into(),
                        description: "Adds two numbers".into(),
                        parameters: serde_json::json!({"type":"object","properties":{"expression":{"type":"string"}},"required":["expression"]}),
                    }
                }
                async fn call(&self, _args: serde_json::Value) -> Result<String, rustyclaw_tools::ToolCallError> {
                    Ok("15".into())
                }
            }
            let mut registry = ToolRegistry::new();
            registry.register(Arc::new(MockTool) as Arc<dyn rig_core::tool::ToolDyn>);
```

- `[ ]` **Step 6: ビルド + テスト確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "error" | head -10
cargo test -p rustyclaw-agent 2>&1 | tail -5
```

Expected: エラーなし、全テスト通過

- `[ ]` **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): execute_heartbeat/execute_with_tools migrated to rig-core ToolDyn"
```

---

## Task 6: 旧コード削除（rustyclaw_tools::Tool + RigToolAdapter）

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`
- Modify: `crates/rustyclaw-tools/Cargo.toml`
- Verify: `crates/rustyclaw-gateway/src/lib.rs`（Task 4 で更新済み）

- `[ ]` **Step 1: 旧コードの参照がないことを確認**

```bash
grep -rn "RigToolAdapter\|\.execute(args\|to_llm_schemas\|to_dyn_tools\|to_toolset\|async_trait\|impl Tool for\b" \
  crates/rustyclaw-tools/src/lib.rs \
  crates/rustyclaw-agent/src/lib.rs \
  crates/rustyclaw-gateway/src/lib.rs \
  2>&1 | grep -v "test_rig_tool_adapter_wraps\|test_tool_registry_to_toolset\|test_tool_registry_to_dyn_tools\|#\[async_trait\].*AddTool"
```

既存の旧テスト（`test_rig_tool_adapter_wraps_custom_tool`、`test_tool_registry_to_toolset`、`test_tool_registry_to_dyn_tools`、`test_tool_execution_success` など）が `Tool::execute()` を直接呼んでいるものは更新または削除する。

- `[ ]` **Step 2: `rustyclaw_tools::Tool` カスタムトレイトを削除**

`lib.rs` から以下のブロックを削除：
1. `/// The trait that all native and MCP-proxied tools must implement.` から `}` まで（カスタム `Tool` トレイト定義、行 16〜30 付近）
2. `use async_trait::async_trait;` import（行 3）
3. `pub struct ToolResult { ... }` 型定義（行 8〜13）—— Agent が参照していないことを確認してから
4. 各ツールの `#[async_trait] impl Tool for XxxTool { ... }` 旧実装ブロック（全6箇所）
5. `RigToolAdapter` struct + impl ブロック全体（行 107〜172）
6. `ToolRegistry::to_toolset()` メソッド（行 87〜94）
7. `ToolRegistry::to_dyn_tools()` メソッド（行 96〜104）
8. 旧テスト: `AddTool` mock、`test_tool_registration_and_retrieval`、`test_tool_execution_success`、`test_tool_execution_error`、`test_to_llm_schemas`、`test_rig_tool_adapter_wraps_custom_tool`、`test_tool_registry_to_toolset`、`test_tool_registry_to_dyn_tools`

- `[ ]` **Step 3: `async-trait` dep を Cargo.toml から削除**

`crates/rustyclaw-tools/Cargo.toml` から `async-trait = "0.1"` 行を削除する。

- `[ ]` **Step 4: ビルド確認**

```bash
cargo build -p rustyclaw-tools 2>&1 | grep "error" | head -20
cargo build --workspace 2>&1 | grep "error" | head -20
```

Expected: エラーなし

- `[ ]` **Step 5: 全テスト実行**

```bash
cargo test --workspace 2>&1 | tail -10
```

Expected: 全テスト通過、`RigToolAdapter` や旧 `Tool::execute` を参照するテストがないこと

- `[ ]` **Step 6: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs crates/rustyclaw-tools/Cargo.toml
git commit -m "refactor(tools): remove custom Tool trait + RigToolAdapter; all tools use rig-core::Tool directly"
```

---

## Self-Review

**1. Spec coverage:**
- ✅ `#[tool]` マクロ相当の型安全な引数: `Args` struct + `Deserialize` で対応
- ✅ `RigToolAdapter` の削除: Task 6
- ✅ `rustyclaw_tools::Tool` カスタムトレイト削除: Task 6
- ✅ `ToolRegistry` の `rig_core::tool::ToolDyn` ベース化: Task 4
- ✅ Agent call site 更新: Task 5
- ✅ Gateway 登録更新: Task 4

**2. Placeholder scan:** なし

**3. Type consistency:**
- `ToolCallError` は Task 1 で定義し、全タスクで `type Error = ToolCallError` として使用 ✅
- `Arc<dyn rig_core::tool::ToolDyn>` は Task 4 以降一貫して使用 ✅
- `tool_definitions()` (async, returns `Vec<rig_core::completion::ToolDefinition>`) は Task 4 で定義、Task 5 で使用 ✅
- `tool.call(call.function.arguments.clone())` で `String` を直接渡す（`ToolDyn::call` の引数型） ✅
