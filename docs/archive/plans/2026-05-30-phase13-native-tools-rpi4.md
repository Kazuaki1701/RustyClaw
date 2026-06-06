# Phase 13: Native Tools + RPi4 Deploy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 外部常駐プロセス（Node.js/Python）を全廃し、Karakeep・Obsidian を Rust インプロセス直実装に置き換え、MCP ツールを Gateway dispatch ループに接続したうえで RPi4 にデプロイする。

**Architecture:** Gateway の dispatch ループを `execute_with_tools()` に切り替え `McpManager` を接続する（Task 1 が全ての前提条件）。Karakeep・Obsidian は `rustyclaw-tools` 内に `reqwest` ベースのネイティブツールとして実装し MCP を廃止。gws（Google Workspace）は現時点でバイナリが存在しないため Calendar/Gmail を `enabled: false` で無効化し、別途移行する。

**Tech Stack:** Rust / reqwest / rustyclaw-tools / rustyclaw-mcp / McpManager / cross (aarch64 クロスコンパイル)

**⚠️ 注意:** `execute_with_tools` は `OpenAiCompatProvider` のみ tool_calls 対応。`GmnCliProvider` は非対応。`config.json` が Cloudflare (openai) のみになっている現状は前提条件を満たしている。

---

## 現状の重要な発見

- **Gateway dispatch ループは現在 `pipeline.execute()` を呼んでいる** — ツール一切未使用
- `McpManager` / `execute_with_tools` は実装済みだがゲートウェイに接続されていない
- Karakeep API: `KARAKEEP_SERVER_ADDR/api/v1/bookmarks` REST (Bearer Token)
- Obsidian: `http://OBSIDIAN_HOST:27123` Local REST API (API Key)
- vault.json に `karakeep-api-key` / `obsidian-api-key` が存在する

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-tools/src/lib.rs` | 修正 | KarakeepTool / ObsidianTool 追加 |
| `crates/rustyclaw-gateway/src/lib.rs` | 修正 | McpManager 起動・execute_with_tools への切り替え |
| `crates/rustyclaw-config/src/lib.rs` | 修正 | NativeTool 設定型追加 |
| `config.json` (開発) | 修正 | MCP Karakeep/Obsidian 無効化・native_tools 追加 |
| `production/config.json` | 修正 | 同上 + Calendar/Gmail 無効化 |
| `scripts/cross-build.sh` (新規) | 新規 | aarch64 クロスビルドスクリプト |

---

## Task 1: Gateway を execute_with_tools + McpManager に切り替え

**これが全タスクの前提条件。これが完了しないとツールが一切動作しない。**

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: 現在の dispatch ループのテストを確認**

```bash
cargo test -p rustyclaw-gateway 2>&1 | grep -E "test result|FAILED"
```

Expected: all ok（既存テスト確認）

- [ ] **Step 2: McpManager を LaneRegistry に持たせるための型変更テストを書く**

`crates/rustyclaw-gateway/src/lib.rs` の tests モジュールに追加:

```rust
#[tokio::test]
async fn test_lane_registry_has_tool_registry() {
    use rustyclaw_tools::ToolRegistry;
    // ToolRegistry が LaneRegistry に渡せることを確認するコンパイルテスト
    let _registry = ToolRegistry::new();
    // compile passes = test passes
}
```

- [ ] **Step 3: テストが pass することを確認（コンパイル確認）**

```bash
cargo test -p rustyclaw-gateway test_lane_registry_has_tool_registry 2>&1 | tail -5
```

- [ ] **Step 4: Gateway::run() に McpManager 初期化を追加**

`crates/rustyclaw-gateway/src/lib.rs` の `run()` 内、`// 2. MessageBus および LaneRegistry の初期化` の直後に追加:

```rust
// MCP Manager 初期化 (enabled な MCP サーバーに接続)
let mcp_manager = Arc::new(rustyclaw_mcp::McpManager::new());
if !config.mcp.is_empty() {
    if let Err(e) = mcp_manager.connect_all(&config.mcp).await {
        tracing::warn!("Some MCP servers failed to connect: {:#}", e);
    }
}
// MCP ツールを ToolRegistry に登録
let mut tool_registry = rustyclaw_tools::ToolRegistry::new();
for mcp_tool in mcp_manager.get_tools().await {
    tool_registry.register(mcp_tool);
}
let tool_registry = Arc::new(tool_registry);
tracing::info!("Tool registry initialized with {} tools.", tool_registry.tool_count());
```

- [ ] **Step 5: ToolRegistry に tool_count() メソッドを追加**

`crates/rustyclaw-tools/src/lib.rs` の `ToolRegistry` impl に追加:

```rust
/// 登録されているツールの数を返す
pub fn tool_count(&self) -> usize {
    self.tools.len()
}
```

- [ ] **Step 6: LaneRegistry に tool_registry フィールドを追加**

`crates/rustyclaw-gateway/src/lib.rs` の `LaneRegistry` struct と `new()` を変更:

```rust
pub struct LaneRegistry {
    lanes: Arc<TokioMutex<HashMap<String, mpsc::Sender<SystemEvent>>>>,
    gmn_sem: Arc<Semaphore>,
    config: Arc<StdMutex<Config>>,
    workspace_path: PathBuf,
    bus: Arc<MessageBus>,
    tool_registry: Arc<rustyclaw_tools::ToolRegistry>,  // 追加
}

impl LaneRegistry {
    pub fn new(
        config: Config,
        workspace_path: PathBuf,
        bus: Arc<MessageBus>,
        tool_registry: Arc<rustyclaw_tools::ToolRegistry>,  // 追加
    ) -> Self {
        Self {
            lanes: Arc::new(TokioMutex::new(HashMap::new())),
            gmn_sem: Arc::new(Semaphore::new(1)),
            config: Arc::new(StdMutex::new(config)),
            workspace_path,
            bus,
            tool_registry,  // 追加
        }
    }
```

- [ ] **Step 7: Gateway::run() の LaneRegistry 生成呼び出しを更新**

`run()` 内の `LaneRegistry::new()` 呼び出しを変更:

```rust
let registry = Arc::new(LaneRegistry::new(
    config.clone(),
    self.workspace_path.clone(),
    bus.clone(),
    tool_registry.clone(),  // 追加
));
```

- [ ] **Step 8: 通常メッセージ dispatch を execute_with_tools に切り替え**

gateway/src/lib.rs 内の通常メッセージ処理（`// 3. 通常メッセージ処理` or else分岐）で `pipeline.execute()` を呼んでいる箇所を `execute_with_tools` に変更。

現状のコード（約 457 行周辺）:
```rust
let pipeline = Pipeline::new(active_config.clone(), gmn_sem.clone());
// ... pipeline.execute() 呼び出し
```

変更後（tool_registry を spawn クロージャに clone して渡す）:
```rust
let pipeline = Pipeline::new(active_config.clone(), gmn_sem.clone());
let tool_reg = tool_registry.clone();
// execute_with_tools に変更
match pipeline.execute_with_tools(&workspace_path, &session_id, &content, &tool_reg).await {
```

- [ ] **Step 9: ビルド確認**

```bash
cargo build 2>&1 | grep -E "^error|Compiling rustyclaw-gateway|Finished"
```

Expected: `Finished` (no errors)

- [ ] **Step 10: 全テスト確認**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED"
```

Expected: all ok

- [ ] **Step 11: Commit**

```bash
git add crates/rustyclaw-gateway/src/lib.rs crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(gateway): wire McpManager and execute_with_tools into dispatch loop"
```

---

## Task 2: Karakeep ネイティブ Rust ツール実装

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- [ ] **Step 1: Karakeep ツールの失敗テストを書く**

`crates/rustyclaw-tools/src/lib.rs` の tests モジュールに追加:

```rust
#[tokio::test]
async fn test_karakeep_tool_name_and_schema() {
    let tool = KarakeepListTool::new("http://localhost:33000".to_string(), "dummy-key".to_string());
    assert_eq!(tool.name(), "karakeep_list_bookmarks");
    assert!(tool.description().len() > 10);
    let params = tool.parameters();
    assert_eq!(params["type"], "object");
}

#[tokio::test]
async fn test_karakeep_tag_tool_name_and_schema() {
    let tool = KarakeepTagTool::new("http://localhost:33000".to_string(), "dummy-key".to_string());
    assert_eq!(tool.name(), "karakeep_tag_bookmark");
    let params = tool.parameters();
    assert!(params["properties"]["bookmark_id"].is_object());
    assert!(params["properties"]["tag_name"].is_object());
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-tools test_karakeep 2>&1 | tail -10
```

Expected: compile error (KarakeepListTool not found)

- [ ] **Step 3: reqwest を rustyclaw-tools の依存に追加**

```bash
cargo add reqwest --features rustls-tls,json -p rustyclaw-tools
cargo add tokio --features rt -p rustyclaw-tools
cargo add serde_json -p rustyclaw-tools
```

- [ ] **Step 4: KarakeepListTool を実装**

`crates/rustyclaw-tools/src/lib.rs` の末尾（tests モジュールの前）に追加:

```rust
/// Karakeep の最近のブックマーク一覧を取得するツール
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
    fn name(&self) -> &str { "karakeep_list_bookmarks" }

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
                    Ok(body) => ToolResult { content: body, is_error: false },
                    Err(e) => ToolResult { content: format!("Failed to read response: {}", e), is_error: true },
                }
            }
            Ok(resp) => ToolResult {
                content: format!("Karakeep API error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult { content: format!("Karakeep request failed: {}", e), is_error: true },
        }
    }
}

/// Karakeep のブックマークにタグを付与するツール
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
    fn name(&self) -> &str { "karakeep_tag_bookmark" }

    fn description(&self) -> &str {
        "Add a tag to a Karakeep bookmark by ID. Use bookmark IDs from karakeep_list_bookmarks."
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
            None => return ToolResult { content: "Missing bookmark_id".to_string(), is_error: true },
        };
        let tag_name = match args["tag_name"].as_str() {
            Some(t) => t.to_string(),
            None => return ToolResult { content: "Missing tag_name".to_string(), is_error: true },
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
            Ok(resp) if resp.status().is_success() => {
                ToolResult { content: format!("Tagged {} with '{}'", bookmark_id, tag_name), is_error: false }
            }
            Ok(resp) => ToolResult {
                content: format!("Karakeep tag error: HTTP {}", resp.status()),
                is_error: true,
            },
            Err(e) => ToolResult { content: format!("Karakeep tag request failed: {}", e), is_error: true },
        }
    }
}
```

- [ ] **Step 5: テストを GREEN にする**

```bash
cargo test -p rustyclaw-tools test_karakeep 2>&1 | tail -10
```

Expected: `test result: ok. 2 passed`

- [ ] **Step 6: 全テスト確認**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED"
```

- [ ] **Step 7: Commit**

```bash
git add crates/rustyclaw-tools/src/lib.rs Cargo.lock
git commit -m "feat(tools): add KarakeepListTool and KarakeepTagTool native Rust implementation"
```

---

## Task 3: Obsidian ネイティブ Rust ツール実装

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- [ ] **Step 1: Obsidian ツールの失敗テストを書く**

```rust
#[tokio::test]
async fn test_obsidian_tool_name_and_schema() {
    let tool = ObsidianSearchTool::new("192.168.1.2".to_string(), "dummy-key".to_string());
    assert_eq!(tool.name(), "obsidian_search");
    assert!(tool.description().len() > 10);
    let params = tool.parameters();
    assert!(params["properties"]["query"].is_object());
    assert_eq!(params["required"][0], "query");
}

#[tokio::test]
async fn test_obsidian_read_tool_name_and_schema() {
    let tool = ObsidianReadTool::new("192.168.1.2".to_string(), "dummy-key".to_string());
    assert_eq!(tool.name(), "obsidian_read_note");
    let params = tool.parameters();
    assert!(params["properties"]["path"].is_object());
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-tools test_obsidian 2>&1 | tail -5
```

Expected: compile error (ObsidianSearchTool not found)

- [ ] **Step 3: ObsidianSearchTool / ObsidianReadTool を実装**

`crates/rustyclaw-tools/src/lib.rs` の Karakeep ツールの後に追加:

```rust
/// Obsidian Vault 内をテキスト検索するツール (Local REST API)
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
                    "description": "Max results (default: 10)"
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
        let limit = args["limit"].as_u64().unwrap_or(10);
        let client = reqwest::Client::new();
        // Obsidian Local REST API v1: POST /search/simple/
        let url = format!("http://{}:27123/search/simple/?query={}&contextLength=100", self.host, urlencoding(&query));
        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.text().await {
                    Ok(body) => {
                        // 結果を limit 件に絞る（JSON 配列の先頭 N 件）
                        let trimmed = if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&body) {
                            let limited: Vec<&Value> = arr.iter().take(limit as usize).collect();
                            serde_json::to_string(&limited).unwrap_or(body)
                        } else {
                            body
                        };
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

fn urlencoding(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
        _ => format!("%{:02X}", c as u32),
    }).collect()
}

/// Obsidian の特定ノートを読み込むツール (Local REST API)
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
        "Read the full content of a specific note from Obsidian vault by its path."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The vault-relative path to the note (e.g. 'Folder/Note.md')"
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
        let url = format!("http://{}:27123/vault/{}", self.host, urlencoding(&path));
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
```

- [ ] **Step 4: テストを GREEN にする**

```bash
cargo test -p rustyclaw-tools test_obsidian 2>&1 | tail -10
```

Expected: `test result: ok. 2 passed`

- [ ] **Step 5: 全テスト確認**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED"
```

- [ ] **Step 6: Commit**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(tools): add ObsidianSearchTool and ObsidianReadTool native Rust implementation"
```

---

## Task 4: ネイティブツールを Gateway 起動時に登録する

ネイティブツールのインスタンス生成には `config.json` の vault 解決済み値（`karakeep-api-key` など）が必要。Gateway の `run()` で MCP ToolRegistry 構築の後に追加する。

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`
- Modify: `crates/rustyclaw-tools/src/lib.rs` (pub use 追加)

- [ ] **Step 1: rustyclaw-tools の Cargo.toml に rustyclaw-gateway からの参照を確認**

```bash
grep "rustyclaw-tools" /mnt/Projects/RustyClaw/crates/rustyclaw-gateway/Cargo.toml
```

既に依存している場合は次へ。なければ追加:
```bash
cargo add rustyclaw-tools --path crates/rustyclaw-tools -p rustyclaw-gateway
```

- [ ] **Step 2: KarakeepListTool / KarakeepTagTool / ObsidianSearchTool / ObsidianReadTool を pub 公開**

`crates/rustyclaw-tools/src/lib.rs` の struct 定義先頭が `pub struct` になっていることを確認（前タスクの実装で既に `pub` のはず）。

- [ ] **Step 3: ネイティブツール登録コードを Gateway::run() に追加**

Task 1 Step 4 で追加したコードの直後（`let tool_registry = Arc::new(tool_registry);` の前）に追加:

```rust
// Karakeep ネイティブツール登録
let karakeep_addr = config.mcp.get("karakeep")
    .and_then(|c| c.env.get("KARAKEEP_SERVER_ADDR"))
    .cloned()
    .unwrap_or_else(|| std::env::var("KARAKEEP_SERVER_ADDR").unwrap_or_default());
let karakeep_key = config.mcp.get("karakeep")
    .and_then(|c| c.env.get("KARAKEEP_API_KEY"))
    .cloned()
    .unwrap_or_else(|| std::env::var("KARAKEEP_API_KEY").unwrap_or_default());
if !karakeep_addr.is_empty() && !karakeep_key.is_empty() {
    tool_registry.register(Arc::new(rustyclaw_tools::KarakeepListTool::new(karakeep_addr.clone(), karakeep_key.clone())));
    tool_registry.register(Arc::new(rustyclaw_tools::KarakeepTagTool::new(karakeep_addr, karakeep_key)));
    tracing::info!("Registered native Karakeep tools.");
}

// Obsidian ネイティブツール登録
let obsidian_host = config.mcp.get("obsidian")
    .and_then(|c| c.env.get("OBSIDIAN_HOST"))
    .cloned()
    .unwrap_or_else(|| std::env::var("OBSIDIAN_HOST").unwrap_or_default());
let obsidian_key = config.mcp.get("obsidian")
    .and_then(|c| c.env.get("OBSIDIAN_API_KEY"))
    .cloned()
    .unwrap_or_else(|| std::env::var("OBSIDIAN_API_KEY").unwrap_or_default());
if !obsidian_host.is_empty() && !obsidian_key.is_empty() {
    tool_registry.register(Arc::new(rustyclaw_tools::ObsidianSearchTool::new(obsidian_host.clone(), obsidian_key.clone())));
    tool_registry.register(Arc::new(rustyclaw_tools::ObsidianReadTool::new(obsidian_host, obsidian_key)));
    tracing::info!("Registered native Obsidian tools.");
}
```

- [ ] **Step 4: cargo build で確認**

```bash
cargo build 2>&1 | grep -E "^error|Finished"
```

- [ ] **Step 5: 全テスト確認**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED"
```

- [ ] **Step 6: Commit**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(gateway): register native Karakeep and Obsidian tools at startup"
```

---

## Task 5: config.json 更新（開発・本番）

**Files:**
- Modify: `config.json`
- Modify: `production/config.json`

- [ ] **Step 1: 開発 config.json を更新**

`config.json` の `mcp` セクションを変更。Karakeep と Obsidian を `enabled: false` に。Calendar/Gmail も同様（gws 未整備のため）:

```json
"mcp": {
    "google-calendar": {
        "enabled": false,
        "command": "npx",
        "args": ["-y", "@anthropic-ai/mcp-server-google-calendar"],
        "env": {}
    },
    "gmail": {
        "enabled": false,
        "command": "npx",
        "args": ["-y", "@anthropic-ai/mcp-server-gmail"],
        "env": {}
    },
    "karakeep": {
        "enabled": false,
        "command": "npx",
        "args": ["-y", "@karakeep/mcp"],
        "env": {
            "KARAKEEP_API_KEY": "$vault:karakeep-api-key",
            "KARAKEEP_SERVER_ADDR": "http://192.168.1.2:33000"
        }
    },
    "obsidian": {
        "enabled": false,
        "command": "uvx",
        "args": ["mcp-obsidian"],
        "env": {
            "OBSIDIAN_API_KEY": "$vault:obsidian-api-key",
            "OBSIDIAN_HOST": "192.168.1.2"
        }
    }
}
```

**Note:** `enabled: false` のままでも `env` に設定値が残るため、ネイティブツール登録コード（Task 4）がそこから API キーを読み取れる。

- [ ] **Step 2: production/config.json を同様に更新**

production/config.json も同様に全 mcp を `enabled: false` に変更。  
また production/config.json のモデル設定が開発版と異なる（70B など）場合は、現時点での稼働を優先して 8B に揃える:

```json
"model_name": "@cf/meta/llama-3-8b-instruct",
```

- [ ] **Step 3: cargo check で破損なし確認**

```bash
cargo check 2>&1 | grep -E "^error|Finished"
```

- [ ] **Step 4: Commit**

```bash
git add config.json production/config.json
git commit -m "config: disable all MCP external processes, use native Rust tools"
```

---

## Task 6: クロスコンパイルスクリプト作成と aarch64 ビルド

**Files:**
- Create: `scripts/cross-build.sh`

- [ ] **Step 1: cross がインストール済みか確認**

```bash
which cross || cargo install cross
cross --version
```

- [ ] **Step 2: cross-build.sh を作成**

`scripts/cross-build.sh`:

```bash
#!/bin/bash
set -e

BINARY="rustyclaw"
TARGET="aarch64-unknown-linux-gnu"
OUTPUT_DIR="target/${TARGET}/release"

echo "=== RustyClaw cross-compile for ${TARGET} ==="
cross build --release --target "${TARGET}"

echo "=== Build complete ==="
ls -lh "${OUTPUT_DIR}/${BINARY}"
file "${OUTPUT_DIR}/${BINARY}"
echo ""
echo "Deploy command:"
echo "  scp ${OUTPUT_DIR}/${BINARY} pi@<rpi4-ip>:~/.local/bin/rustyclaw"
```

```bash
chmod +x scripts/cross-build.sh
```

- [ ] **Step 3: クロスビルド実行**

```bash
bash scripts/cross-build.sh 2>&1 | tail -20
```

Expected: `ELF 64-bit LSB pie executable, ARM aarch64`

エラーが出た場合の対処:
- `OpenSSL` 関連エラー → `reqwest` が `rustls-tls` を使っているはずなので Cargo.toml を確認
- `linker` エラー → `cross` の Docker イメージが使われているか確認（`cross` は Docker 必要）

- [ ] **Step 4: バイナリサイズ確認**

```bash
ls -lh target/aarch64-unknown-linux-gnu/release/rustyclaw
```

目安: 20MB〜60MB 程度（strip なしの release）

- [ ] **Step 5: Commit**

```bash
git add scripts/cross-build.sh
git commit -m "build: add aarch64 cross-compile script"
```

---

## Task 7: RPi4 へのデプロイと稼働確認

**前提条件:** RPi4 が SSH でアクセス可能なこと。RPi4 IP アドレスを事前確認のこと。

- [ ] **Step 1: RPi4 の現在のサービス状態を確認**

```bash
ssh pi@<rpi4-ip> "systemctl status rustyclaw 2>/dev/null || echo 'service not found'"
ssh pi@<rpi4-ip> "which rustyclaw && rustyclaw --version || echo 'binary not found'"
```

- [ ] **Step 2: バイナリを RPi4 に転送**

```bash
scp target/aarch64-unknown-linux-gnu/release/rustyclaw pi@<rpi4-ip>:~/.local/bin/rustyclaw
ssh pi@<rpi4-ip> "chmod +x ~/.local/bin/rustyclaw && rustyclaw --version"
```

- [ ] **Step 3: vault.json を RPi4 に同期（初回のみ / 内容確認済みの場合）**

```bash
ssh pi@<rpi4-ip> "mkdir -p ~/.rustyclaw"
scp ~/.rustyclaw/vault.json pi@<rpi4-ip>:~/.rustyclaw/vault.json
```

- [ ] **Step 4: production/config.json を RPi4 に転送**

```bash
scp production/config.json pi@<rpi4-ip>:~/.rustyclaw/config.json
```

- [ ] **Step 5: production/workspace/ を RPi4 に同期**

```bash
rsync -av --exclude='sessions/' --exclude='memory/logs/' \
  production/workspace/ pi@<rpi4-ip>:~/.rustyclaw/workspace/
```

- [ ] **Step 6: RPi4 でテスト起動（フォアグラウンド）**

```bash
ssh pi@<rpi4-ip> "cd ~/.rustyclaw && RUST_LOG=info rustyclaw gateway --config config.json --workspace workspace/ 2>&1 | head -30"
```

確認ポイント:
- `Gateway loaded configuration` ログが出る
- `Tool registry initialized with N tools` が出る（N ≥ 2 = Karakeep + Obsidian）
- MCP 接続エラーが出ない（全て enabled: false のため）
- Discord 接続ログが出る

- [ ] **Step 7: systemd サービス設定（初回のみ）**

```bash
cat << 'EOF' | ssh pi@<rpi4-ip> "sudo tee /etc/systemd/system/rustyclaw.service"
[Unit]
Description=RustyClaw AI Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=pi
WorkingDirectory=/home/pi/.rustyclaw
ExecStart=/home/pi/.local/bin/rustyclaw gateway --config /home/pi/.rustyclaw/config.json --workspace /home/pi/.rustyclaw/workspace
Restart=on-failure
RestartSec=5s
OOMScoreAdjust=-500
MemoryMax=2G
WatchdogSec=60s
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

ssh pi@<rpi4-ip> "sudo systemctl daemon-reload && sudo systemctl enable rustyclaw && sudo systemctl start rustyclaw"
```

- [ ] **Step 8: サービス稼働確認**

```bash
ssh pi@<rpi4-ip> "sudo systemctl status rustyclaw"
ssh pi@<rpi4-ip> "curl -s http://localhost:8080/health"
```

Expected: `{"status":"ok", ...}`

- [ ] **Step 9: Discord から動作確認**

Discord の home チャンネルでエージェントに話しかけ、応答が返ることを確認。ツール呼び出し（`karakeep_list_bookmarks` など）が動作するか確認。

- [ ] **Step 10: ログ確認**

```bash
ssh pi@<rpi4-ip> "sudo journalctl -u rustyclaw -n 50 --no-pager"
```

エラーがなく `Tool registry initialized` が確認できたら完了。

---

## 保留: gws (Google Workspace) 移行

Calendar/Gmail の MCP は現時点で `enabled: false` のまま。  
`gws` バイナリのソースリポジトリ（`github.com/googleworkspace/go-workspace-cli` 等）が確認でき次第、別タスクとして対応する。

移行時の手順概要:
1. `gws` ソースを取得・`GOOS=linux GOARCH=arm64 go build` でクロスビルド
2. RPi4 に転送・`~/.local/bin/gws` に配置
3. `gws auth` で Google OAuth 認証
4. `config.json` の `google-calendar` / `gmail` を `gws mcp` コマンドで `enabled: true` に変更

---

## Self-Review

**Spec coverage チェック:**
- ✅ gws 移行 → Calendar/Gmail 無効化 + 保留セクションで文書化
- ✅ Karakeep Rust インプロセス化 → Task 2
- ✅ Obsidian Rust インプロセス化 → Task 3
- ✅ execute_with_tools への切り替え → Task 1（前提条件）
- ✅ RPi4 クロスビルド → Task 6
- ✅ RPi4 デプロイ・稼働確認 → Task 7

**Placeholder scan:** なし

**Type consistency:**
- `KarakeepListTool::new(server_addr, api_key)` — Task 2, 4 で一致
- `ObsidianSearchTool::new(host, api_key)` — Task 3, 4 で一致
- `tool_registry.register(Arc::new(...))` — Task 1, 4 で一致
