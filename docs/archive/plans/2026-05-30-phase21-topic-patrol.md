# Phase 21: Topic Patrol 完全移植 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** GeminiClaw の Topic Patrol 機能を RustyClaw に完全移植する。web_search / web_fetch / ローカルファイル I/O ツールを追加し、Skills ロード機構を実装して、cron.json から自律的に Web 巡回・フィルタ・Discord 投稿・state 更新が動作する状態にする。

**Architecture:** `rustyclaw-tools` に 4 本の新ツール（WebSearchTool・WebFetchTool・WorkspaceReadTool・WorkspaceWriteTool）を追加し、`rustyclaw-config` に `BraveSearchConfig` を追加して vault 経由で API キーを供給する。`rustyclaw-gateway` に `skills.rs` モジュールを新設し、cron ディスパッチ前にスキル定義ファイルをプロンプトへ自動注入する。

**Tech Stack:** Rust / reqwest（既存） / Brave Search REST API / tokio（既存）

---

## ファイルマップ

| ファイル | 変更種別 | 担当 |
|---|---|---|
| `crates/rustyclaw-tools/src/lib.rs` | 修正 | Task 1・2・3 |
| `crates/rustyclaw-config/src/lib.rs` | 修正 | Task 4 |
| `crates/rustyclaw-gateway/src/lib.rs` | 修正 | Task 4 |
| `crates/rustyclaw-gateway/src/skills.rs` | **新規** | Task 5 |
| `production/workspace/skills/topic-patrol.md` | **新規** | Task 6 |
| `workspace/skills/topic-patrol.md` | **新規** | Task 6 |

---

## Task 1: WorkspaceReadTool / WorkspaceWriteTool

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`（末尾 `#[cfg(test)]` の直前に追加）

> LLM が `patrol/state.json`・`patrol/findings.md` を読み書きするために必須。Obsidian REST API は workspace 外ファイルに触れないため独立実装が必要。

- [ ] **Step 1: 失敗テストを書く**

`crates/rustyclaw-tools/src/lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
#[tokio::test]
async fn test_workspace_read_write_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let read_tool  = WorkspaceReadTool::new(dir.path().to_path_buf());
    let write_tool = WorkspaceWriteTool::new(dir.path().to_path_buf());

    // write
    let res = write_tool.execute(serde_json::json!({
        "path": "patrol/state.json",
        "content": "{\"lastRun\":null,\"rotationIndex\":0}"
    })).await;
    assert!(!res.is_error);

    // read back
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
```

- [ ] **Step 2: テストが失敗することを確認**

```
cargo test -p rustyclaw-tools test_workspace 2>&1 | tail -10
```
Expected: `error[E0422]: cannot find struct WorkspaceReadTool`

- [ ] **Step 3: WorkspaceReadTool / WorkspaceWriteTool を実装**

`#[cfg(test)]` の直前（行頭を参考に末尾の `// ---` 付近）に追加:

```rust
// ─── WorkspaceReadTool ───────────────────────────────────────────────────────

pub struct WorkspaceReadTool {
    workspace_path: std::path::PathBuf,
}

impl WorkspaceReadTool {
    pub fn new(workspace_path: std::path::PathBuf) -> Self {
        Self { workspace_path }
    }
}

#[async_trait::async_trait]
impl Tool for WorkspaceReadTool {
    fn name(&self) -> &str { "workspace_read" }

    fn description(&self) -> &str {
        "Read a text file from the agent workspace (e.g. patrol/state.json, patrol/findings.md)."
    }

    fn parameters(&self) -> serde_json::Value {
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

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
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

#[async_trait::async_trait]
impl Tool for WorkspaceWriteTool {
    fn name(&self) -> &str { "workspace_write" }

    fn description(&self) -> &str {
        "Write or append to a text file in the agent workspace (patrol/, memory/logs/ etc.)."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string", "description": "Relative path within workspace" },
                "content": { "type": "string", "description": "Text to write" },
                "mode":    {
                    "anyOf": [{"type": "string"}, {"type": "string"}],
                    "description": "\"write\" (overwrite, default) or \"append\""
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
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
```

- [ ] **Step 4: テストが通ることを確認**

```
cargo test -p rustyclaw-tools test_workspace 2>&1 | tail -10
```
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(tools): add WorkspaceReadTool and WorkspaceWriteTool"
```

---

## Task 2: WebSearchTool（Brave Search API）

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

> Brave Search REST API: `GET https://api.search.brave.com/res/v1/web/search`  
> レスポンス: `json["web"]["results"][]` に `title`, `url`, `description` が入る。

- [ ] **Step 1: 失敗テストを書く**

テストブロックに追加:

```rust
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
```

- [ ] **Step 2: テストが失敗することを確認**

```
cargo test -p rustyclaw-tools test_web_search 2>&1 | tail -5
```
Expected: `error[E0422]: cannot find struct WebSearchTool`

- [ ] **Step 3: WebSearchTool を実装**

WorkspaceWriteTool の後に追加:

```rust
// ─── WebSearchTool ───────────────────────────────────────────────────────────

pub struct WebSearchTool {
    api_key: String,
}

impl WebSearchTool {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }

    fn description(&self) -> &str {
        "Search the web using Brave Search. Returns titles, URLs, and snippets. Use site: prefix for HN/Reddit (e.g. 'site:news.ycombinator.com rust')."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "count": {
                    "anyOf": [{"type": "integer"}, {"type": "string"}],
                    "description": "Number of results (default: 5, max: 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let query = match args["query"].as_str() {
            Some(q) => q.to_string(),
            None => return ToolResult { content: "Missing query".into(), is_error: true },
        };
        let count = match &args["count"] {
            serde_json::Value::Number(n) => n.as_u64().unwrap_or(5),
            serde_json::Value::String(s) => s.parse::<u64>().unwrap_or(5),
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
                match resp.json::<serde_json::Value>().await {
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
```

- [ ] **Step 4: テストが通ることを確認**

```
cargo test -p rustyclaw-tools test_web_search 2>&1 | tail -5
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(tools): add WebSearchTool (Brave Search API)"
```

---

## Task 3: WebFetchTool（URL フェッチ + HTML 除去）

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

> 新規 crate 依存なし。タグ除去はステートマシンで inline 実装。`<script>`・`<style>` タグの中身も除去する。

- [ ] **Step 1: 失敗テストを書く**

```rust
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
    assert!(plain.len() <= 110); // 多少の誤差を許容
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
```

- [ ] **Step 2: テストが失敗することを確認**

```
cargo test -p rustyclaw-tools test_strip_html test_web_fetch 2>&1 | tail -5
```

- [ ] **Step 3: `strip_html_to_text` と `WebFetchTool` を実装**

WebSearchTool の後に追加:

```rust
// ─── WebFetchTool ────────────────────────────────────────────────────────────

/// HTML タグ・script・style ブロックを除去してプレーンテキストを返す
fn strip_html_to_text(html: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag    = false;
    let mut skip_body = false; // script/style の中身を飛ばすフラグ
    let mut buf       = String::new(); // タグ名収集用

    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '<' => {
                in_tag = true;
                buf.clear();
            }
            '>' => {
                let tag = buf.trim().to_lowercase();
                let tag_name = tag.split_whitespace().next().unwrap_or("");
                // <script> / <style> 開始
                if tag_name == "script" || tag_name == "style" {
                    skip_body = true;
                }
                // </script> / </style> 終了
                if tag_name == "/script" || tag_name == "/style" {
                    skip_body = false;
                }
                in_tag = false;
                out.push(' '); // タグをスペースで区切る
            }
            _ if in_tag => {
                buf.push(c);
            }
            _ if !skip_body => {
                out.push(c);
            }
            _ => {}
        }
        i += 1;
        if out.len() >= max_chars {
            break;
        }
    }

    // 連続する空白・改行を圧縮
    out.split_whitespace().collect::<Vec<_>>().join(" ")
        .chars().take(max_chars).collect()
}

pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }

    fn description(&self) -> &str {
        "Fetch a web page and return its plain text content (HTML tags stripped). Use after web_search to read full article content."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" },
                "max_chars": {
                    "anyOf": [{"type": "integer"}, {"type": "string"}],
                    "description": "Max characters to return (default: 3000)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let url = match args["url"].as_str() {
            Some(u) => u.to_string(),
            None => return ToolResult { content: "Missing url".into(), is_error: true },
        };
        let max_chars = match &args["max_chars"] {
            serde_json::Value::Number(n) => n.as_u64().unwrap_or(3000) as usize,
            serde_json::Value::String(s) => s.parse::<usize>().unwrap_or(3000),
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
```

- [ ] **Step 4: テストが通ることを確認**

```
cargo test -p rustyclaw-tools 2>&1 | tail -10
```
Expected: `test result: ok. N passed`（既存含め全テスト通過）

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(tools): add WebFetchTool with inline HTML stripper"
```

---

## Task 4: Config 追加 + Gateway へのツール登録

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

### 4a. Config に BraveSearchConfig を追加

- [ ] **Step 1: `rustyclaw-config/src/lib.rs` を修正**

`ObsidianConfig` の後に追加:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BraveSearchConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
}
```

`ToolsConfig` に追加:

```rust
pub struct ToolsConfig {
    #[serde(default)]
    pub karakeep: Option<KarakeepConfig>,
    #[serde(default)]
    pub obsidian: Option<ObsidianConfig>,
    #[serde(default, rename = "google-workspace")]
    pub google_workspace: Option<GoogleWorkspaceConfig>,
    #[serde(default, rename = "brave-search")]         // ← 追加
    pub brave_search: Option<BraveSearchConfig>,       // ← 追加
}
```

`resolve_secrets()` に追加（`obsidian` の解決ブロックの後）:

```rust
if let Some(ref mut b) = self.tools.brave_search {
    b.api_key = resolve_value(&b.api_key);
}
```

- [ ] **Step 2: `cargo check` が通ることを確認**

```
cargo check -p rustyclaw-config 2>&1 | tail -5
```
Expected: `warning: ... (0 errors)`

### 4b. Gateway にツール登録を追加

- [ ] **Step 3: `rustyclaw-gateway/src/lib.rs` に登録コードを追加**

既存の Obsidian 登録ブロック（`tracing::info!("Registered native Obsidian tools.")`）の直後に追加:

```rust
        // Brave Search ネイティブツール登録
        if let Some(b) = config.tools.brave_search.as_ref().filter(|b| b.enabled) {
            if !b.api_key.is_empty() {
                tool_registry.register(Arc::new(rustyclaw_tools::WebSearchTool::new(b.api_key.clone())));
                tracing::info!("Registered WebSearchTool (Brave Search).");
            }
        }
        // WebFetchTool は常時登録（APIキー不要）
        tool_registry.register(Arc::new(rustyclaw_tools::WebFetchTool::new()));

        // WorkspaceReadTool / WorkspaceWriteTool 登録
        tool_registry.register(Arc::new(rustyclaw_tools::WorkspaceReadTool::new(self.workspace_path.clone())));
        tool_registry.register(Arc::new(rustyclaw_tools::WorkspaceWriteTool::new(self.workspace_path.clone())));
        tracing::info!("Registered Workspace I/O tools.");
```

- [ ] **Step 4: `cargo check` が通ることを確認**

```
cargo check -p rustyclaw-gateway 2>&1 | tail -5
```

### 4c. config.json に Brave Search 設定を追加

- [ ] **Step 5: `production/config.json` の `tools` セクションに追加**

既存の `"obsidian": {...}` の後に追加:

```json
"brave-search": {
  "enabled": true,
  "api_key": "$vault:BRAVE_SEARCH_API_KEY"
}
```

同様に開発用 `config.json`（プロジェクトルートの `config.json` や `workspace/config.json` があれば）にも追加。

- [ ] **Step 6: vault に API キーを登録**

```bash
rustyclaw vault set BRAVE_SEARCH_API_KEY
# → プロンプトが出たら Brave Search の API キーを入力
```

RPi4 にも転送が必要:

```bash
# 開発機で vault.enc を production/ 経由で RPi4 に同期（symlink 済みのため自動）
```

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(config,gateway): add BraveSearchConfig and register web/workspace tools"
```

---

## Task 5: Skills ロード機構（`gateway/src/skills.rs`）

**Files:**
- Create: `crates/rustyclaw-gateway/src/skills.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（`mod skills;` 追加 + dispatch フック）

> cron ディスパッチ時に `content` にスキル名が含まれていれば `workspace/skills/<name>.md` を読み込んでプロンプト先頭に前置する。既存ファイルがなければ `content` をそのまま返す。

- [ ] **Step 1: テストを書く（新規ファイル内）**

`crates/rustyclaw-gateway/src/skills.rs` を新規作成:

```rust
use std::path::Path;

/// workspace/skills/ 配下のスキルファイルを content に注入する。
/// ファイル名（拡張子なし）が content 中に現れたスキルを前置する。
/// スキルが見つからない場合は content をそのまま返す。
pub fn inject_skill_content(workspace_path: &Path, content: &str) -> String {
    let skills_dir = workspace_path.join("skills");
    if !skills_dir.exists() {
        return content.to_string();
    }
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return content.to_string();
    };
    let lower = content.to_lowercase();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let skill_name = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
        if lower.contains(skill_name.as_ref()) {
            if let Ok(skill_md) = std::fs::read_to_string(&path) {
                tracing::info!("Skills: injecting '{}' into prompt", skill_name);
                return format!("{}\n\n---\n\n{}", skill_md.trim(), content);
            }
        }
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_skill_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(skills_dir.join("topic-patrol.md"), "# Topic Patrol\nDo the patrol.").unwrap();

        let result = inject_skill_content(dir.path(), "Run the topic-patrol skill.");
        assert!(result.contains("# Topic Patrol"));
        assert!(result.contains("Run the topic-patrol skill."));
        // スキルが先頭に前置されること
        assert!(result.find("# Topic Patrol").unwrap() < result.find("Run the").unwrap());
    }

    #[test]
    fn test_no_inject_when_skill_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let content = "Run the unknown-skill.";
        let result = inject_skill_content(dir.path(), content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_no_inject_when_skills_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Run the topic-patrol skill.";
        let result = inject_skill_content(dir.path(), content);
        assert_eq!(result, content);
    }
}
```

- [ ] **Step 2: `mod skills;` を `lib.rs` に追加**

`crates/rustyclaw-gateway/src/lib.rs` の既存 `pub mod cron;` の下に追加:

```rust
pub(crate) mod skills;
```

- [ ] **Step 3: テストが失敗することを確認**

```
cargo test -p rustyclaw-gateway skills 2>&1 | tail -10
```
Expected: `error: cannot find crate...`（`tempfile` が dev-dep にない場合）

`crates/rustyclaw-gateway/Cargo.toml` の `[dev-dependencies]` に追加:

```toml
tempfile = "3"
```

再度テスト実行:
```
cargo test -p rustyclaw-gateway skills 2>&1 | tail -10
```
Expected: `test result: ok. 3 passed`

- [ ] **Step 4: dispatch フックに注入を追加**

`crates/rustyclaw-gateway/src/lib.rs` の `execute_with_tools` 呼び出し直前（`let exec_res = if session_id.starts_with("cron:session-summary:")` の前）を変更:

```rust
// スキルファイル注入（workspace/skills/<name>.md が存在すれば前置）
let content = crate::skills::inject_skill_content(&workspace_path, &content);

let exec_res = if session_id.starts_with("cron:session-summary:") {
```

- [ ] **Step 5: 全テストが通ることを確認**

```
cargo test 2>&1 | tail -10
```
Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-gateway/src/skills.rs crates/rustyclaw-gateway/src/lib.rs crates/rustyclaw-gateway/Cargo.toml
git commit -m "feat(gateway): add Skills injection mechanism (skills.rs)"
```

---

## Task 6: `workspace/skills/topic-patrol.md` 作成

**Files:**
- Create: `production/workspace/skills/topic-patrol.md`
- Create: `workspace/skills/topic-patrol.md`（開発環境用）

> GeminiClaw の SKILL.md をベースに RustyClaw 用に適合。ツール名を RustyClaw のものに変更し、geminiclaw_* の呼び出しを削除。

- [ ] **Step 1: スキルディレクトリを作成**

```bash
mkdir -p /mnt/Projects/RustyClaw/production/workspace/skills
mkdir -p /mnt/Projects/RustyClaw/workspace/skills
```

- [ ] **Step 2: スキルファイルを作成**

`production/workspace/skills/topic-patrol.md`（および `workspace/skills/topic-patrol.md` に同内容）:

```markdown
---
name: topic-patrol
description: Proactively explore the web for topics matching the user's interests and share findings conversationally. Runs as a cron job every 6 hours.
---

# Topic Patrol

Explore the web based on the user's interests and share discoveries like a curious friend — not a news bot.

## Available Tools

- `web_search` — Search the web (Brave Search). Use `site:` prefix for HN/Reddit routing.
- `web_fetch` — Fetch full article content from a URL.
- `workspace_read` — Read files like `patrol/state.json` or `patrol/findings.md`.
- `workspace_write` — Write/append to workspace files.
- `karakeep_list_bookmarks` — List recent Karakeep bookmarks.
- `obsidian_search` — Search Obsidian vault.

## Execution Flow

### Step 1: Load Context

1. Call `workspace_read` with `path: "USER.md"` → read Interests and Work Context sections.
2. Call `workspace_read` with `path: "patrol/state.json"` → get `lastRun` and `rotationIndex`. If file missing, use `{"lastRun": null, "rotationIndex": 0}`.
3. Call `workspace_read` with `path: "patrol/findings.md"` → get dedup history. If missing, treat as empty.
4. Check for `deferred` entries in findings.md. If any exist and are still relevant, share them first.

### Step 2: Explore

Run **2–3 queries**, rotating through the Interests list using `rotationIndex`. Wrap to 0 when exceeding the number of interests.

**Source Routing** — use `web_search` with these query patterns:

| Source | Query format |
|---|---|
| _(default)_ | `{interest} latest 2026` |
| HN | `site:news.ycombinator.com {interest}` |
| Reddit/{sub} | `site:reddit.com/r/{sub} {interest}` |
| Direct URL | Use `web_fetch` on the URL directly |

For promising search results, call `web_fetch` on the URL to read the full article. Do not judge on snippets alone.

Also check `karakeep_list_bookmarks` and `obsidian_search` for relevant recent material in the user's own knowledge base.

**Query categories** (rotate):
- **Interest-driven** — a topic from Interests
- **Work-adjacent** — something near the user's current Work Context
- **Serendipity** (1 in 3 runs) — cross-topic or tangential discovery

### Step 3: Filter — "Would I tell a friend?"

For each finding, check:
- **Novel?** — not already in `patrol/findings.md`
- **Interesting?** — not a generic press release or product announcement
- **Relevant?** — connects to user's work or interests
- **Worth sharing?** — would make someone say "oh cool"

If nothing clears the bar, share nothing. Silence is correct.

### Step 4: Share (only when worth it)

Check the current time (provided in the system context as `Current time:`).

**Quiet hours: 23:00–07:00** → Record finding as `deferred` in findings.md. Do not post.

Outside quiet hours → Reply with the finding. The reply goes directly to the Discord channel. Limit to **1–2 topics per message**.

**Tone**: natural, conversational. Explain WHY it's interesting, connect it to the user's current work. End with a question or action prompt.

**Good format:**
```
Hey — spotted something relevant to your {work_context} work.
{1–2 sentence summary and why it matters}.

Source: {URL}

Want me to dig into this further?
```

**Do NOT use:** numbered lists, emoji headers, "Topic Patrol Report" framing, 3+ topics at once.

### Step 5: Update State

1. Call `workspace_write` to append to `patrol/findings.md`:
   ```
   path: "patrol/findings.md"
   mode: "append"
   content: |
     ## YYYY-MM-DD
     - {topic}: {one-line summary} — shared / skipped ({reason}) / deferred (quiet hours)
   ```
   Prune entries older than 14 days: overwrite the file with `mode: "write"` keeping only the last 14 days.

2. Call `workspace_write` to overwrite `patrol/state.json`:
   ```
   path: "patrol/state.json"
   mode: "write"
   content: {"lastRun": "{ISO8601 timestamp}", "rotationIndex": {next_index}}
   ```
   Always update `lastRun` and increment `rotationIndex` even if nothing was found.

## Graceful Degradation

| Condition | Behavior |
|---|---|
| USER.md has no Interests or Work Context | Stay silent |
| `web_search` returns error | Fall back to `karakeep_list_bookmarks` + `obsidian_search` only |
| `patrol/state.json` missing | Start fresh: `{"lastRun": null, "rotationIndex": 0}` |
| `patrol/findings.md` missing | Treat as empty |
| Quiet hours (23:00–07:00) | Record as deferred, no Discord post |
| Nothing passes the filter | Stay silent — do not report "nothing found" |

## Prohibited Patterns

- Formatted news-briefing style (numbered lists, emoji headers, "report" framing)
- Posting 3+ topics in a single message
- Sharing something just because it's new — it must be genuinely interesting
- Reporting "nothing found" — silence is the correct response
- Sharing without checking `patrol/findings.md` for duplicates
```

- [ ] **Step 3: 両ファイルを作成したことを確認**

```bash
ls -la /mnt/Projects/RustyClaw/production/workspace/skills/
ls -la /mnt/Projects/RustyClaw/workspace/skills/
```

- [ ] **Step 4: コミット**

```bash
git add production/workspace/skills/topic-patrol.md workspace/skills/topic-patrol.md
git commit -m "feat(workspace): add topic-patrol skill definition"
```

---

## Task 7: E2E 動作確認

**Files:**
- Modify: `production/workspace/cron.json`（インターバルを一時的に短縮してテスト）

- [ ] **Step 1: cargo build でエラーがないことを確認**

```bash
cargo build 2>&1 | grep -E "^error" | head -10
```
Expected: エラーなし

- [ ] **Step 2: `--no-agent` で Skills 注入を確認**

```bash
RUSTYCLAW_NO_AGENT=1 cargo run --bin rustyclaw-cli -- agent \
  --session "cron:topic-patrol" \
  --message "Run the topic-patrol skill." \
  2>&1 | grep -i "skill\|inject\|topic-patrol"
```
Expected: `Skills: injecting 'topic-patrol' into prompt` のログが出る

- [ ] **Step 3: cron.json のインターバルを 2 分に変更してライブテスト**

`production/workspace/cron.json` の Topic Patrol エントリを一時変更:

```json
{
  "id": "topic-patrol",
  "trigger": { "type": "interval", "minutes": 2 },
  ...
}
```

Gateway を起動して 3 分待機:

```bash
cargo run --bin rustyclaw-cli -- gateway 2>&1 | grep -E "topic-patrol|web_search|workspace_write|patrol"
```

- [ ] **Step 4: 各ステップの成否を確認**

| 確認項目 | 期待値 |
|---|---|
| `Skills: injecting 'topic-patrol'` ログ | 出力される |
| `web_search` ツール呼び出し | ログに `tool_calls` / `web_search` が現れる |
| `patrol/state.json` の更新 | `lastRun` が更新されている |
| `patrol/findings.md` への追記 | 今日の日付エントリが追加されている |
| Discord への投稿（静寂時間外の場合） | チャンネルに届く |

```bash
cat production/workspace/patrol/state.json
tail -20 production/workspace/patrol/findings.md
```

- [ ] **Step 5: cron.json を元の 360 分間隔に戻す**

```json
"trigger": { "type": "interval", "minutes": 360 }
```

- [ ] **Step 6: task.md を更新**

`docs/task.md` の Phase 21 各タスクを `[x]` に更新する。

- [ ] **Step 7: 最終コミット**

```bash
git add production/workspace/cron.json docs/task.md
git commit -m "chore: restore topic-patrol interval to 360m, mark Phase 21 complete"
```

---

## Self-Review

### Spec Coverage チェック

| 要件 | 対応タスク |
|---|---|
| `web_search` ツール（Brave API） | Task 2 ✅ |
| `web_fetch` ツール（HTML 除去） | Task 3 ✅ |
| `workspace_read` / `workspace_write` | Task 1 ✅ |
| Config + vault（BRAVE_SEARCH_API_KEY） | Task 4 ✅ |
| Skills ロード機構 | Task 5 ✅ |
| スキル定義ファイル（5ステップ流れ） | Task 6 ✅ |
| Source Routing（HN・Reddit） | Task 6（SKILL.md 内） ✅ |
| Quiet Hours（23:00–07:00） | Task 6（SKILL.md 内） ✅ |
| state.json / findings.md 更新 | Task 6（SKILL.md 内） + Task 1（ツール） ✅ |
| 14日プルーニング | Task 6（SKILL.md 内）✅ |
| E2E 確認 | Task 7 ✅ |

### Placeholder チェック

- 「TBD」「TODO」なし ✅
- 全コードブロックに実装済みコードあり ✅
- テストに具体的なアサーションあり ✅
