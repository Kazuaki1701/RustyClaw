# 07. rustyclaw-mcp 実装計画

> [!NOTE]
> **ステータス**: `[HISTORICAL]` (Phase 9 で実装完了済み・設計記録として保存)  
> **作成日**: 2026-05-28  
> **完了日**: 2026-05-28（Phase 9 にて全項目実装済み）  
> **参照 upstream**: PicoClaw `pkg/mcp`, `pkg/tools` (`/home/kazuaki/Projects/PicoClaw/master/upstream/`)

> [!IMPORTANT]
> このドキュメントは実装時の設計計画書です。最新の実装状態は `docs/specs/01_architecture.md` および `docs/specs/02_agent_pipeline.md` を参照してください。
>
> **計画と実際の主な差異**:
> - `rmcp` クレートは採用せず、自前の JSON-RPC 2.0 over stdio を実装（`rustyclaw-mcp/src/lib.rs`）
> - `LlmResponse` は enum ではなくフラットな struct `{ content: String, tool_calls: Option<Vec<ToolCall>> }`
> - `rustyclaw-mcp` は `rustyclaw-tools` と `rustyclaw-config` のみに依存（rmcp 不使用）
> - `McpManager::load_from_config()` ではなく `connect_all(&servers)` インターフェースで実装
> - Phase 7-6（ツール検索 Discovery）は未実装のまま

---

## 1. 背景と目的

### 現状の制約

RustyClaw は LLM バックエンドとして `gmn` CLI を `--no-agent` モードで起動し、
プロンプトを stdin で渡してテキスト応答を受け取るだけの構成になっている。

```
RustyClaw Pipeline
  └─ gmn --no-agent  (LLM呼び出しのみ)
       └─ MCP ツール実行ループ ← 存在しない
```

この構成では LLM が MCP ツール呼び出し JSON を生成しても実行されず、
そのまま Discord チャットに出力されてしまう（既知の問題）。

### 目指す姿

PicoClaw の `pkg/mcp` を参考に、RustyClaw 自身が MCP クライアントを持ち、
LLM とのアジェンティックループを内製する。

```
RustyClaw Pipeline (アジェンティックループ)
  ├─ LLM Provider (Anthropic API / OpenAI Compat / gmn)
  │    └─ ツールスキーマをシステムプロンプトに付与
  ├─ ToolRegistry
  │    ├─ NativeTool（Rust ネイティブ実装）
  │    └─ McpTool（外部 MCP サーバーへのプロキシ）
  └─ McpManager
       ├─ Server A (stdio: npx @anthropic-ai/mcp-server-google-calendar)
       └─ Server B (sse: https://mcp.example.com)
```

---

## 2. アーキテクチャ概観

### 現状のクレート依存関係

```
rustyclaw-cli
  └─ rustyclaw-gateway
       └─ rustyclaw-agent
            ├─ rustyclaw-providers   (LLM API / gmn CLI)
            ├─ rustyclaw-storage     (SQLite / Tantivy)
            └─ rustyclaw-config
```

### 変更後のクレート依存関係

```
rustyclaw-cli
  └─ rustyclaw-gateway
       └─ rustyclaw-agent            ← アジェンティックループ追加
            ├─ rustyclaw-providers   ← ToolCall レスポンス対応追加
            ├─ rustyclaw-tools       ← Tool トレイト + ToolRegistry 実装 (現在空)
            ├─ rustyclaw-mcp         ← 新規: MCP クライアント
            ├─ rustyclaw-storage
            └─ rustyclaw-config      ← MCP 設定スキーマ追加
```

---

## 3. 新設クレート: `rustyclaw-mcp`

### 役割

PicoClaw の `pkg/mcp` に相当。MCP サーバーへの接続・ツール定義取得・ツール実行を担当。

### 参照実装

| PicoClaw ファイル | 役割 |
|---|---|
| `pkg/mcp/manager.go` | `McpManager` の Rust 版 |
| `pkg/mcp/isolated_command_transport.go` | stdio トランスポートの Rust 版 |
| `pkg/agent/agent_mcp.go` | `Pipeline` への統合パターン |

### 主要型・関数設計

```rust
// crates/rustyclaw-mcp/src/lib.rs

pub struct McpManager {
    servers: Arc<RwLock<HashMap<String, ServerConnection>>>,
    closed:  AtomicBool,
}

pub struct ServerConnection {
    pub name:    String,
    pub config:  McpServerConfig,
    pub tools:   Vec<ToolDef>,  // tools/call 後のスキーマ
    session:     McpSession,    // rmcp クレートの型
}

/// ツール定義（LLM へ渡すスキーマ）
pub struct ToolDef {
    pub name:        String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema
}

impl McpManager {
    pub async fn new() -> Self;
    /// 設定に基づき全サーバーへ並列接続
    pub async fn load_from_config(
        &self,
        ctx:            CancellationToken,
        servers:        &HashMap<String, McpServerConfig>,
        workspace_path: &Path,
    ) -> Result<()>;
    /// ツール呼び出し (サーバー名 + ツール名 + 引数)
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name:   &str,
        arguments:   serde_json::Value,
    ) -> Result<serde_json::Value>;
    /// 全サーバーの全ツール一覧を返す
    pub fn all_tools(&self) -> Vec<(String, ToolDef)>;  // (server_name, tool_def)
    pub async fn close(&self);
}
```

### トランスポート実装

PicoClaw は `github.com/modelcontextprotocol/go-sdk` を使用。
Rust では公式 SDK `rmcp`（`modelcontextprotocol/rust-sdk`）を使用する。

```toml
# crates/rustyclaw-mcp/Cargo.toml
[dependencies]
rmcp = { version = "0.1", features = ["client", "transport-child-process", "transport-sse-client"] }
tokio        = { version = "1.0", features = ["full"] }
tokio-util   = "0.7"
serde        = { version = "1.0", features = ["derive"] }
serde_json   = "1.0"
anyhow       = "1.0"
tracing      = "0.1"
tokio-util   = "0.7"
```

サポートするトランスポート:

| 種別 | 設定 | 実装 |
|---|---|---|
| `stdio` | `command` + `args` + `env` | `rmcp::transport::TokioChildProcess` |
| `sse` / `http` | `url` + `headers` | `rmcp::transport::SseClientTransport` |

---

## 4. 拡張クレート: `rustyclaw-tools`

### 役割

PicoClaw の `pkg/tools` に相当。`Tool` トレイトと `ToolRegistry` を定義。
現在は空クレート（stub）のみ存在するため、ここに本実装を置く。

### 主要型設計

```rust
// crates/rustyclaw-tools/src/lib.rs

use async_trait::async_trait;

/// 全ツールが実装すべきトレイト (PicoClaw の Tool interface に相当)
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON Schema ({"type":"object","properties":{...}})
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> ToolResult;
}

pub struct ToolResult {
    pub content:  String,   // LLM に返すテキスト
    pub is_error: bool,
}

/// MCP サーバーのツール呼び出しをプロキシする Tool 実装
pub struct McpTool {
    server_name: String,
    tool_def:    rustyclaw_mcp::ToolDef,
    manager:     Arc<rustyclaw_mcp::McpManager>,
}

/// 全ツールのレジストリ
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>);
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>>;
    /// LLM へ渡す JSON Schema 形式のツール一覧
    pub fn to_llm_schemas(&self) -> Vec<serde_json::Value>;
    /// MCP サーバーのツールを一括登録
    pub fn register_mcp_tools(
        &mut self,
        manager: Arc<rustyclaw_mcp::McpManager>,
    );
}
```

---

## 5. 変更クレート: `rustyclaw-providers`

### 変更概要

現在の `LlmProvider` トレイトは `complete(prompt: &str)` / `complete_stream(prompt: &str)` のみ。
ツール対応のために「ツールスキーマの受け取り」と「ToolCall レスポンスの返却」を追加する。

### 追加型

```rust
// crates/rustyclaw-providers/src/lib.rs

/// LLM レスポンスに含まれるツール呼び出し要求
pub struct ToolCall {
    pub id:        String,
    pub name:      String,
    pub arguments: serde_json::Value,
}

/// LLM レスポンス (テキスト or ツール呼び出し)
pub enum LlmResponse {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

/// ツール実行結果をフィードバックするメッセージ
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub is_error: bool,
}
```

### `LlmProvider` トレイトの変更

```rust
pub trait LlmProvider: Send + Sync {
    // 既存（ツールなし・後方互換）
    async fn complete(&self, prompt: &str, opts: &CompletionOptions) -> Result<String>;
    async fn complete_stream(&self, prompt: &str, opts: &CompletionOptions)
        -> Result<BoxStream<'static, Result<String>>>;

    // 新規追加: ツール付き会話（アジェンティックループ用）
    async fn complete_with_tools(
        &self,
        messages: &[Message],
        tools:    &[serde_json::Value],  // JSON Schema 形式
        opts:     &CompletionOptions,
    ) -> Result<LlmResponse>;
}
```

### プロバイダー別対応方針

| プロバイダー | 実装方針 |
|---|---|
| `OpenAiCompatProvider` | `tools` フィールドをリクエストに追加。レスポンス `tool_calls` をパース |
| `GmnCliProvider` | `--no-agent` を外し、MCP ツールスキーマを `--tools-json` 等で渡す**か**、または gmn を廃止して Anthropic API への直接接続に移行する（要検討） |

> **注意**: `GmnCliProvider` は gmn CLI の仕様に依存するため、  
> ツール付きアジェンティックループの実現には `OpenAiCompatProvider` (Anthropic API 直接) への移行が現実的。

---

## 6. 変更クレート: `rustyclaw-agent`

### 変更概要

現在の `Pipeline::execute()` は単一の LLM 呼び出しで完結する。
MCP 対応後は「LLM → ツール実行 → LLM へフィードバック」のループに変える。

### アジェンティックループ設計

```rust
// crates/rustyclaw-agent/src/lib.rs（概念コード）

impl Pipeline {
    pub async fn execute_with_tools(
        &self,
        workspace:   &Path,
        session_id:  &str,
        user_input:  &str,
        tool_registry: &ToolRegistry,
    ) -> Result<AgentResponse> {
        let mut messages = self.build_context(workspace, session_id, user_input)?;
        let tool_schemas = tool_registry.to_llm_schemas();

        loop {
            let response = self.provider
                .complete_with_tools(&messages, &tool_schemas, &opts)
                .await?;

            match response {
                LlmResponse::Text(text) => {
                    // 最終回答 → ループ終了
                    return Ok(AgentResponse { content: text });
                }
                LlmResponse::ToolCalls(calls) => {
                    // ツール実行 → 結果を会話に追加して再ループ
                    let results = execute_tool_calls(&calls, tool_registry).await?;
                    messages.push_tool_results(calls, results);
                    // 無限ループ防止: 最大ステップ数チェック
                }
            }
        }
    }
}
```

### gmn_sem との関係

アジェンティックループ導入後は gmn プロセスを 1 回しか起動しないのではなく、
**ループ 1 反復 = 1 LLM 呼び出し** になる。
LLM が Anthropic API への直接呼び出しになれば `gmn_sem` は不要になる。
移行期は以下のいずれかで対応:

- `gmn_sem` を LLM API 呼び出し単位のレート制限として流用する
- または gmn を廃止し `gmn_sem` 自体を削除する

---

## 7. 変更クレート: `rustyclaw-config`

### 追加フィールド

```json5
// config.json (追加部分のみ)
{
  "mcp": {
    "enabled": true,
    "servers": {
      "google-calendar": {
        "enabled": true,
        "type": "stdio",
        "command": "npx",
        "args": ["-y", "@anthropic-ai/mcp-server-google-calendar"],
        "env": { "GOOGLE_OAUTH_TOKEN": "$vault:gog-token" }
      },
      "gmail": {
        "enabled": true,
        "type": "stdio",
        "command": "npx",
        "args": ["-y", "@anthropic-ai/mcp-server-gmail"],
        "env_file": ".env.gmail"
      },
      "remote-tool": {
        "enabled": false,
        "type": "sse",
        "url": "https://mcp.example.com/sse",
        "headers": { "Authorization": "Bearer $vault:remote-token" }
      }
    }
  }
}
```

```rust
// crates/rustyclaw-config/src/lib.rs

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    pub enabled: bool,
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub enabled: bool,
    #[serde(default)]
    pub r#type: String,          // "stdio" | "sse" | "http"  (省略時は自動判定)
    pub command: Option<String>, // stdio 用
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,     // sse/http 用
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub env_file: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}
```

---

## 8. 実装ロードマップ

### Phase 7-1: Foundation（ToolRegistry + Tool トレイト）

**目標**: `rustyclaw-tools` クレートを実装し、ネイティブツールを登録できるようにする  
**工数見積**: 1〜2 セッション  
**完了基準**: `Tool` トレイトを実装したダミーツールが `ToolRegistry` に登録・実行できる

タスク:
- `rustyclaw-tools` の `Tool` トレイト定義
- `ToolRegistry` の実装（登録・取得・スキーマ生成）
- `ToolResult` 型の定義
- ユニットテスト追加

---

### Phase 7-2: Provider 拡張（ToolCall 対応）

**目標**: `OpenAiCompatProvider` でツールスキーマ付きリクエストと ToolCall レスポンスを処理できるようにする  
**工数見積**: 1〜2 セッション  
**完了基準**: Anthropic API 直接呼び出しで `tool_use` ブロックをパースできる

タスク:
- `LlmResponse` 列挙型の追加
- `ToolCall` 型の追加
- `OpenAiCompatProvider::complete_with_tools()` の実装
- Anthropic API の `tool_use` / `tool_result` 形式に対応
- `GmnCliProvider` は `complete_with_tools()` を `Err(unsupported)` で返す（将来廃止予定）

---

### Phase 7-3: Agent アジェンティックループ

**目標**: `Pipeline::execute_with_tools()` でツール呼び出しループを動作させる  
**前提**: Phase 7-1, 7-2 完了  
**工数見積**: 1〜2 セッション  
**完了基準**: ダミーツールを呼び出すプロンプトでループが 1 往復動作する

タスク:
- `Pipeline::execute_with_tools()` の実装
- 会話履歴への `tool_result` 追加
- 最大ループ回数制限（デフォルト 10 回）
- `gmn_sem` との整理（LLM API 直接呼び出し時は不要）

---

### Phase 7-4: MCP クライアント（rustyclaw-mcp）

**目標**: 外部 MCP サーバーへ接続してツール一覧を取得・実行できる  
**前提**: Phase 7-1 完了  
**工数見積**: 2〜3 セッション  
**完了基準**: `npx @modelcontextprotocol/server-filesystem` に stdio 接続してファイル読み取りを実行できる

タスク:
- `rustyclaw-mcp` クレート新設
- `McpManager` の実装（並列接続・ツール一覧取得）
- stdio トランスポート対応（`rmcp::transport::TokioChildProcess`）
- SSE トランスポート対応（`rmcp::transport::SseClientTransport`）
- 切断時の自動再接続（PicoClaw の `reconnectServer` 相当）
- `McpTool` 実装（`Tool` トレイト経由で `McpManager::call_tool()` を呼び出す）
- ユニットテスト + 統合テスト

---

### Phase 7-5: 設定統合 + LaneRegistry への組み込み

**目標**: `config.json` の `mcp` セクションを読み込み、Gateway 起動時に全サーバーへ接続する  
**前提**: Phase 7-1〜7-4 完了  
**工数見積**: 1 セッション  
**完了基準**: 実際の MCP サーバー（例: `google-calendar`）のツールが Discord 経由で動作する

タスク:
- `rustyclaw-config` への `McpConfig` 追加
- `Gateway::run()` での `McpManager` 初期化
- `LaneRegistry` への `McpManager` + `ToolRegistry` の注入
- `Pipeline::execute()` → `execute_with_tools()` への切り替え
- `AGENTS.md` から GeminiClaw 固有ツール指示の削除

---

### Phase 7-6: ツール検索（Discovery、オプション）

**目標**: ツール数が多い場合に BM25 / Regex でツールを絞り込む  
**前提**: Phase 7-1〜7-5 完了  
**工数見積**: 1〜2 セッション  
**備考**: PicoClaw の `search_tool.go` が参照実装。`rustyclaw-storage` の Tantivy を流用可能

---

## 9. 依存クレート候補

| クレート | 用途 | 参照 |
|---|---|---|
| `rmcp` | MCP 公式 Rust SDK（クライアント実装） | `modelcontextprotocol/rust-sdk` |
| `tokio-util` | CancellationToken, codec | 既存依存で追加なし |
| `async-trait` | `Tool` トレイトの async メソッド | 既存依存 |

> **注意**: `rmcp` は 2025 年時点でまだ API が安定していない可能性がある。  
> バージョン固定（`=0.x.y`）して使用し、アップグレードは計画的に行うこと。

---

## 10. 移行戦略（gmn からの脱却）

gmn CLI に依存したまま MCP を追加することは困難。段階的な移行を推奨する。

```
現状:    gmn --no-agent (LLM として使用)
↓
Step 1: OpenAiCompatProvider + Anthropic API 直接呼び出しを並走させる
         - config.model_provider = "anthropic" で切り替え可能にする
Step 2: ToolCall 対応の完成後、gmn なしでアジェンティックループを動作させる
Step 3: gmn_sem を API レート制限セマフォとして流用または廃止
Step 4: GmnCliProvider を deprecated に、最終的に削除
```

---

## 11. 関連ドキュメント

- `docs/specs/02_agent_pipeline.md` — 現状の Pipeline 設計（Phase 7-3 完了時に更新）
- `docs/specs/05_gateway_spec.md` — `gmn_sem` と MCP 統合時の再検討事項 (`[^mcp_heartbeat]`)
- `docs/task.md` — 継続検討課題セクション
- PicoClaw 参照コード: `/home/kazuaki/Projects/PicoClaw/master/upstream/pkg/`
