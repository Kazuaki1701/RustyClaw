# Phase 31 STEP 4–6 実装計画書

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ダッシュボード刷新と自律動作の完全整合を果たすため、以下3つのクロスカッティングな改善項目を実装する。
1. **STEP 6: cron 信頼性回復（ISSUE-06 / 07）** — 動的 cron の無言失敗（RSSクリーンアップ、ブックマーク推薦）をネイティブツール化＋プロンプト書き換えで根本治療する。
2. **STEP 4: LLM I/O インスペクタ刷新（ISSUE-20 / 21）** — プロバイダ層での dump 一元化（リングバッファ/用途別）＋ダッシュボードの11カテゴリ・タブ式 UI への再設計。
3. **STEP 5: 自己認識・事実確認の是正（ISSUE-04 / 05）** — cron予定一覧ツール `get_cron_schedule` の実装と、`SOUL.md` による capability 自己認識の修正（ツールによる事実確認の強制）。

**Architecture:** 
- `KarakeepDeleteTool` / `CronScheduleTool` は純粋な native ツールとして `rustyclaw-tools` に追加され、`cargo test -p rustyclaw-tools` で検証する。
- LLM I/O カテゴリ判定・一元 dump は `rustyclaw-providers` の `complete`/`complete_stream` にて集約ハンドリングされ、`crates/rustyclaw-gateway/src/health.rs` の `/api/llm/io` で公開される。
- `cron.json` および `SOUL.md` は本番ワークスペースにあり、これらを改修することで動作を適用する。

---

## STEP 6: cron 信頼性回復（ISSUE-06 / 07）

シェルスクリプト実行（`bash scripts/501, 502...`）に依存していた cron ジョブを、セキュリティリスクの高い汎用シェル実行ツールの追加を避け、ネイティブツールとプロンプトの更新（A-案）によって安全かつ堅牢に置き換える。

### Task 6.1: `KarakeepDeleteTool` の実装（RSSクリーンアップ対応）

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`（`KarakeepDeleteTool` 定義）
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（`ToolRegistry` への登録）

- [ ] **Step 1: `KarakeepDeleteTool` を定義**

`crates/rustyclaw-tools/src/lib.rs` の `KarakeepTagTool` の直後に定義を追加:
```rust
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
```

- [ ] **Step 2: ユニットテストを追加**

`crates/rustyclaw-tools/src/lib.rs` の `mod tests` 内に追加:
```rust
    #[tokio::test]
    async fn test_karakeep_delete_tool_name_and_schema() {
        let tool = KarakeepDeleteTool::new("http://localhost:33000".to_string(), "key".to_string());
        assert_eq!(tool.name(), "karakeep_delete_bookmark");
    }
```

- [ ] **Step 3: `ToolRegistry` に登録**

`crates/rustyclaw-gateway/src/lib.rs` の `ToolRegistry` 登録処理（`karakeep_tag_bookmark` を登録している付近、750行近辺）に `KarakeepDeleteTool` を追加:
```rust
        let kk_addr = config.providers.karakeep.server_addr.clone();
        let kk_key = config.providers.karakeep.api_key.clone();
        tool_registry.register(rustyclaw_tools::KarakeepListTool::new(kk_addr.clone(), kk_key.clone()));
        tool_registry.register(rustyclaw_tools::KarakeepTagTool::new(kk_addr.clone(), kk_key.clone()));
        tool_registry.register(rustyclaw_tools::KarakeepDeleteTool::new(kk_addr, kk_key));
```

- [ ] **Step 4: ビルド ＆ テスト実行**

Run: `cargo test -p rustyclaw-tools test_karakeep_delete`
Expected: テスト PASS。

### Task 6.2: `cron.json` のプロンプト書き換え

**Files:**
- Modify: `production/workspace/cron.json`（各 cron の `prompt` フィールド）

- [ ] **Step 1: ブックマーク推薦の prompt 変更**

`production/workspace/cron.json` 内の `karakeep-recommendation` の `"prompt"` に記述されている shell script 実行の指示を、**ネイティブツール呼び出し**に変更:
```diff
-    "prompt": "Daily Karakeep recommendation:\n1. Fetch recent bookmarks (last 3 days) from $KARAKEEP_SERVER_ADDR/api/v1/bookmarks.\n2. Select items matching user interests (AI, LLM, CLI, Cloudflare, Obsidian, etc.) from USER.md.\n3. Apply '_recommended' tag to selected IDs using 'bash scripts/502_karakeep-tag-items.sh _recommended <ids...>'.\n4. Log results to memory/logs/YYYY-MM-DD.md.",
+    "prompt": "Daily Karakeep recommendation:\n1. Fetch recent bookmarks (last 3 days) using the `karakeep_list_bookmarks` tool.\n2. Select items matching user interests (AI, LLM, CLI, Cloudflare, Obsidian, etc.) from USER.md.\n3. Apply the '_recommended' tag to each selected ID using the `karakeep_tag_bookmark` tool sequentially.\n4. Log results to memory/logs/YYYY-MM-DD.md.",
```

- [ ] **Step 2: RSS クリーンアップの prompt 変更**

`production/workspace/cron.json` 内の `karakeep-cleanup` の `"prompt"` を、シェル実行からネイティブツール削除に変更:
```diff
-    "prompt": "Run Karakeep auto-cleanup:\n1. Execute 'bash scripts/501_karakeep-cleanup.sh'.\n2. Log the output and summary to memory/logs/YYYY-MM-DD.md.",
+    "prompt": "Run Karakeep auto-cleanup:\n1. Fetch recent bookmarks using the `karakeep_list_bookmarks` tool.\n2. Filter bookmarks that are older than 2 weeks (14 days) and have the 'rss' tag.\n3. Delete these stale bookmarks using the `karakeep_delete_bookmark` tool.\n4. Log the output and summary of deleted bookmark count/titles to memory/logs/YYYY-MM-DD.md.",
```

- [ ] **Step 3: コミット**
```bash
git add crates/rustyclaw-tools crates/rustyclaw-gateway production/workspace/cron.json
git commit -m "feat(cron): substitute shell scripts with native Karakeep delete tool and prompts (ISSUE-06)"
```

---

## STEP 4: LLM I/O インスペクタ刷新（ISSUE-20 / 21）

全 LLM 呼び出し（memory/summary 含む）をプロバイダ層の単一点で 100% 漏れなくペア捕捉し、ダッシュボード上に 11 の用途別（カテゴリ別）にタブ切替で表示できる単一ペインを構築する。

### Task 4.1: `CompletionOptions` への `category` 追加とエージェント伝搬

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`（`CompletionOptions` 構造体）
- Modify: `crates/rustyclaw-agent/src/lib.rs`（各 API 呼び出しにカテゴリを付与）

- [ ] **Step 1: options 構造体にカテゴリを追加**

`crates/rustyclaw-providers/src/lib.rs` の `CompletionOptions` 構造体にフィールド追加:
```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionOptions {
    pub temperature: f64,
    pub max_tokens: Option<u32>,
    pub category: Option<String>, // 追加: 用途タグ (briefing/vitals/patrol 等)
}
```

- [ ] **Step 2: 各エントリポイント（agent）で category オプションを設定**

`crates/rustyclaw-agent/src/lib.rs` の各 LLM 呼び出し（`execute`, `execute_with_tools`, `flush_memory`, `generate_session_summary` 等）の `options.category` を設定するように修正。
- `flush_memory` -> `Some("memory".to_string())`
- `generate_session_summary` -> `Some("summary".to_string())`
- その他セッション別の category 設定（Gateway から session_id に応じて category 判定し Options に乗せる）。

### Task 4.2: プロバイダ層での一元 dump ロジック（リングバッファ保存）

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`（`complete`/`complete_stream` の dump 処理）

- [ ] **Step 1: リクエスト・レスポンスペアの一元捕捉とファイル保存**

`complete`/`complete_stream` メソッド内（JSON メッセージをバックエンドプロバイダへ送受信する直前・直後）で以下を実装:
- 応答に `tool_calls` が含まれる場合はカテゴリを強制的に `"tools"` にアロケート。
- なければ `options.category`（未指定時は `"discord"` などをデフォルトフォールバック）を適用。
- `{workspace_dir}/memory/debug/llm/<category>.json` に原子性書き込みで最新1ペアを保存:
```json
{
  "timestamp": 1780189200,
  "model": "gemini-2.5-pro",
  "request": [...],
  "response": {...}
}
```
- ※ 既存のエージェント層の個別 dump 処理（4箇所）を撤去し、プロバイダ側へ完全集約する。

### Task 4.3: `/api/llm/io` エンドポイント追加とフロント刷新

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs`（エンドポイント ＆ SPA フロント）

- [ ] **Step 1: `/api/llm/io` エンドポイントを実装**

GET `/api/llm/io?cat=<category>` および クエリ無しで全カテゴリ最新一覧を返すルートを `health.rs` に実装。

- [ ] **Step 2: SPA 画面レイアウト刷新（タブ式 UI）**

- HTMLの既存 `reqPanel`/`resPanel`（上下並列）を廃止し、上部に 11カテゴリのタブバー（`tools`/`discord`/`dashboard`/`briefing`/`vitals`/`karakeep`/`patrol`/`heartbeat`/`summary`/`daily`/`memory`）を持つ単一ペインに統合。
- タブ切り替え時に `/api/llm/io?cat=xx` をフェッチし、`REQUEST` の下に `RESPONSE` を縦スタックで綺麗に表示。
- 文字数 truncate は末尾4000文字保持（`slice(-4000)`）を適用。

---

## STEP 5: エージェント自己認識・事実確認の是正（ISSUE-04 / 05）

### Task 5.1: `CronScheduleTool`（スケジュール取得）の追加

エージェントが自身の今後の実行スケジュールを "ツール経由で事実確認" して正答できるようにする。

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`（`CronScheduleTool` 定義）
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（`ToolRegistry` 登録）

- [ ] **Step 1: スケジュール取得ツール `get_cron_schedule` の実装**

`crates/rustyclaw-tools/src/lib.rs` に追加:
```rust
pub struct CronScheduleTool {
    workspace_dir: std::path::PathBuf,
    db_path: std::path::PathBuf,
}

impl CronScheduleTool {
    pub fn new(workspace_dir: std::path::PathBuf, db_path: std::path::PathBuf) -> Self {
        Self { workspace_dir, db_path }
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
        if let Ok(db) = rustyclaw_storage::DbManager::new(&self.db_path) {
            let sched = crate::cron::compute_schedule(&self.workspace_dir, &db);
            match serde_json::to_string_pretty(&sched) {
                Ok(json) => ToolResult { content: json, is_error: false },
                Err(_) => ToolResult { content: "[]".to_string(), is_error: true }
            }
        } else {
            ToolResult { content: "Failed to open database".to_string(), is_error: true }
        }
    }
}
```

- [ ] **Step 2: ツール登録**

`crates/rustyclaw-gateway/src/lib.rs` 内で `CronScheduleTool` を登録。

### Task 5.2: `SOUL.md` とシステムプロンプトの調整（自己認識修正）

**Files:**
- Modify: `production/workspace/SOUL.md`（またはシステムプロンプトテンプレート）

- [ ] **Step 1: プロンプト記述のアップデート**

`SOUL.md`（あるいはデフォルトプロンプト）の自己定義を修正:
- **自己能力の正しい認識**: 「私は shell コマンドを実行する能力は持ちません」といった事実誤認を削除し、「私は Gmail、Calendar、Obsidian、Karakeep、およびスケジュール取得など、17以上の強力なネイティブツールを直接実行できます」と言及できるように調整する。
- **事実確認の強制**: 「今後のスケジュールや予定タスクについて尋ねられた場合は、記憶から推測して答えるのを厳禁とし、必ず `get_cron_schedule` ツールを呼び出して事実確認を行ってください」と指示を明示。

---

## 完了確認

- [ ] `cargo build -p rustyclaw-gateway` のコンパイルが成功すること。
- [ ] すべての新規ユニットテストがパスすること。
- [ ] ダッシュボード画面（MONITOR タブ）で 11 カテゴリのタブ式 LLM I/O 表示が正しく機能し、用途ごとの req/res ペアが末尾保持で閲覧できること。
- [ ] 対話上で「あなたの次のタスク予定を教えて」と尋ねた際、エージェントが `get_cron_schedule` ツールを呼んで `karakeep-cleanup` や `daily-briefing` などの予定を正確に答えること。
