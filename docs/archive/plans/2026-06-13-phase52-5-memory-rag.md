# Phase 52-5: MEMORY.md セマンティック分割（Memory RAG）実装計画書

> [!NOTE]
> **ステータス**: `[DONE]` (完了: 2026-06-13)
> **最終更新日**: 2026-06-13
> **対象コード**: `crates/rustyclaw-agent/src/lib.rs`, `crates/rustyclaw-gateway/src/lib.rs`
> **設計仕様書**: [`docs/specs/2026-06-13-phase52-context-optimization-design.md`](../specs/2026-06-13-phase52-context-optimization-design.md)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** MEMORY.md をセクション単位でチャンク分割して context-mode SQLite FTS5 にインデックス登録し、チャット時は ctx_search で関連チャンクのみを動的注入、Memory Flush 後は自動再インデックスする。

**Architecture:** 起動時（5 秒後）に既存の `chunk_memory_md()` で MEMORY.md を分割し `[memory-chunk]` タグ付きで ctx_index 登録する。チャット受信時は `try_ctx_search` でヒットしたチャンクを `user_interests_extra` と合算して `extra_system_context` に渡す。`trigger_memory_flush_async` に `Option<ToolServerHandle>` を追加し、`flush_memory` 完了後に `reindex_memory_after_flush` で SQLite を更新する。

**Tech Stack:** Rust 2024 Edition, `rustyclaw-agent`, `rustyclaw-gateway`, context-mode MCP (ctx_index / ctx_search)

**除外範囲:** ctx_patch による部分メモリ書き換えは Phase 52-5b として別途計画する（Memory Flush の LLM 出力フォーマット変更を伴うため）。

---

## 開発タスクチェックリスト

- [x] **Task 1: chunk_memory_md 公開化 + Gateway 起動時 MEMORY.md インデックス登録**
- [x] **Task 2: チャットハンドラで関連メモリチャンクを動的注入**
- [x] **Task 3: Memory Flush 後の再インデックス**
- [x] **Task 4: テスト・Clippy・コミット・ドキュメント更新**

---

## 前提知識: 現状と変更点

### chunk_memory_md（agent/lib.rs:1817）

MEMORY.md をセクションごとにバレット行をグループ化し、最大 800 文字のチャンクに分割する関数。現在は `pub(crate)` + `#[allow(dead_code)]` で未使用。

```
入力: "## ユーザープリファレンス\n- 名前: GeminiClaw\n- 言語: 日本語"
出力: ["[ユーザープリファレンス] - 名前: GeminiClaw\n- 言語: 日本語"]
```

### Gateway 起動時インデックス（gateway/lib.rs:1317-1335）

現在 3s → スキル、4s → USER.md Interests の順で非同期登録。MEMORY.md を 5s 後に追加する。

### チャットハンドラ（gateway/lib.rs:~763-800）

現在: patrol / 通常 chat / cron の 3 分岐で `user_interests_extra: Option<String>` を計算し `execute_with_rig_agent` に渡す。
変更後: さらに `memory_extra: Option<String>` を計算し、2 つを結合した `extra_system_context` を渡す。

### trigger_memory_flush_async（agent/lib.rs:734）

非同期タスクを spawn して `flush_memory` を呼び出す。`flush_memory` の呼び出し元（line 778）の直後に再インデックス処理を追加する。

### execute_with_rig_agent シグネチャ（agent/lib.rs:1421）

```rust
pub async fn execute_with_rig_agent(
    &self,
    workspace_dir: &Path,
    session_id: &str,
    raw_user_message: &str,
    injected_user_message: &str,
    tool_handle: rig_core::tool::server::ToolServerHandle,  // ← この名前に注意
    purpose: &str,
    progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
    extra_system_context: Option<String>,
) -> Result<LlmResponse>
```

`trigger_memory_flush_async` の呼び出し元（line 1230, 1608）は両方 `execute_with_rig_agent` 内にあり、`tool_handle` が使用可能。

---

## Task 1: chunk_memory_md 公開化 + Gateway 起動時 MEMORY.md インデックス登録

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs` (~line 1816)
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (line 2、~line 1235、~line 1335)

### Step 1: chunk_memory_md を pub に変更する

`agent/lib.rs` line 1816-1817:

```rust
// Before
#[allow(dead_code)]
pub(crate) fn chunk_memory_md(content: &str) -> Vec<String> {

// After
pub fn chunk_memory_md(content: &str) -> Vec<String> {
```

### Step 2: gateway/lib.rs の import に chunk_memory_md を追加する

`gateway/lib.rs` の先頭付近（line 2）:

```rust
// Before
use rustyclaw_agent::{Pipeline, build_heartbeat_rag_query};

// After
use rustyclaw_agent::{Pipeline, build_heartbeat_rag_query, chunk_memory_md};
```

### Step 3: index_memory_to_context_mode 関数を追加する

`index_user_interests` 関数（~line 1216）の直後に追加する:

```rust
/// 起動時に MEMORY.md のチャンクを context-mode にインデックス登録する。
/// [memory-chunk] プレフィックスにより ctx_search 結果から識別できる。
async fn index_memory_to_context_mode(
    workspace_path: &Path,
    handle: &rig_core::tool::server::ToolServerHandle,
) {
    let memory_path = workspace_path.join("MEMORY.md");
    let content = match std::fs::read_to_string(&memory_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("MEMORY.md 読み込み失敗（インデックス登録スキップ）: {}", e);
            return;
        }
    };
    let chunks = chunk_memory_md(&content);
    if chunks.is_empty() {
        tracing::info!("MEMORY.md にチャンクが見つからないためスキップ");
        return;
    }
    for chunk in &chunks {
        let indexed = format!("[memory-chunk]\n{}", chunk);
        try_ctx_index(handle, &indexed, "memory-chunk").await;
    }
    tracing::info!("context-mode: MEMORY.md {} チャンク インデックス登録完了", chunks.len());
}
```

### Step 4: 起動時 spawn を追加する

`gateway/lib.rs` の USER.md Interests spawn（~line 1327-1335）の直後に追加する:

```rust
// MEMORY.md チャンクを context-mode に非同期インデックス登録（5 秒後）
{
    let ws = self.workspace_path.clone();
    let tsh = tool_server_handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        index_memory_to_context_mode(&ws, &tsh).await;
    });
}
```

### Step 5: ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-agent -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし（エラーなし）

---

## Task 2: チャットハンドラで関連メモリチャンクを動的注入

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (~line 790-800)

### Step 1: memory_extra を追加し user_interests_extra と結合する

`gateway/lib.rs` の `user_interests_extra` ブロック（~line 763-790）の直後、`pipeline.execute_with_rig_agent(` 呼び出しの直前に以下を挿入する。

変更前（`user_interests_extra` の後の呼び出し部分）:

```rust
pipeline
    .execute_with_rig_agent(
        &workspace_path,
        &session_id,
        &content,
        &injected_content,
        tool_server_handle.clone(),
        run_purpose,
        progress_tx_opt,
        user_interests_extra,
    )
    .await
```

変更後:

```rust
// 関連メモリを ctx_search で動的取得（cron 以外のみ）
let memory_extra: Option<String> = if !session_id.starts_with("cron:") {
    try_ctx_search(&tool_server_handle, &content)
        .await
        .filter(|r| r.contains("[memory-chunk]"))
        .map(|r| format!("\n\n# Relevant Memory\n{}", r))
} else {
    None
};

// user_interests_extra と memory_extra を結合して extra_system_context を構築
let extra_system_context: Option<String> = match (user_interests_extra, memory_extra) {
    (Some(u), Some(m)) => Some(format!("{}{}", u, m)),
    (Some(u), None) => Some(u),
    (None, Some(m)) => Some(m),
    (None, None) => None,
};

pipeline
    .execute_with_rig_agent(
        &workspace_path,
        &session_id,
        &content,
        &injected_content,
        tool_server_handle.clone(),
        run_purpose,
        progress_tx_opt,
        extra_system_context,
    )
    .await
```

### Step 2: ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 3: Memory Flush 後の再インデックス

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs` (~line 734, ~line 778, ~line 1230, ~line 1608, after line 1812)

### Step 1: reindex_memory_after_flush 関数を追加する

`agent/lib.rs` の `build_patrol_context` 関数（~line 1810-1812）の直後に追加する:

```rust
/// Memory Flush 後に MEMORY.md チャンクを context-mode に再インデックスする（fail-open）。
/// flush_memory が MEMORY.md を書き換えた直後に呼び出し、SQLite FTS5 を最新状態に保つ。
async fn reindex_memory_after_flush(
    workspace_dir: &Path,
    handle: &rig_core::tool::server::ToolServerHandle,
) {
    let content = match std::fs::read_to_string(workspace_dir.join("MEMORY.md")) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("reindex_memory: MEMORY.md 読み込み失敗: {}", e);
            return;
        }
    };
    let chunks = chunk_memory_md(&content);
    let count = chunks.len();
    for chunk in chunks {
        let args = serde_json::json!({
            "content": format!("[memory-chunk]\n{}", chunk),
            "source": "memory-chunk"
        })
        .to_string();
        if let Err(e) = handle.call_tool("ctx_index", &args).await {
            tracing::debug!("reindex_memory: ctx_index 失敗（fail-open）: {}", e);
        }
    }
    tracing::info!("memory flush: {} チャンク再インデックス完了", count);
}
```

### Step 2: trigger_memory_flush_async に reindex_handle パラメータを追加する

`agent/lib.rs` の `trigger_memory_flush_async` 関数シグネチャ（line 734）を変更する:

```rust
// Before
pub fn trigger_memory_flush_async(&self, workspace_dir: &Path, session_id: &str) {

// After
pub fn trigger_memory_flush_async(
    &self,
    workspace_dir: &Path,
    session_id: &str,
    reindex_handle: Option<rig_core::tool::server::ToolServerHandle>,
) {
```

### Step 3: spawn ブロック内に reindex_handle をキャプチャし再インデックスを追加する

同関数の `tokio::spawn(async move {` ブロック内、`Self::flush_memory(...)` 呼び出しの直後:

```rust
// Before
if let Err(e) = Self::flush_memory(&workspace_dir, &session_id, config).await {
    tracing::warn!("Failed to flush memory for session {}: {:#}", session_id, e);
}

if let Some(ref cb) = on_flush_done {
    cb(&flush_session_id);
}

// After
if let Err(e) = Self::flush_memory(&workspace_dir, &session_id, config).await {
    tracing::warn!("Failed to flush memory for session {}: {:#}", session_id, e);
}

// Memory Flush 後に SQLite チャンクを再インデックス（fail-open）
if let Some(ref handle) = reindex_handle {
    reindex_memory_after_flush(&workspace_dir, handle).await;
}

if let Some(ref cb) = on_flush_done {
    cb(&flush_session_id);
}
```

`reindex_handle` を spawn のキャプチャに含めるために、関数先頭部分（`let workspace_dir = ...` のクローン群の後）に追加する:

```rust
pub fn trigger_memory_flush_async(
    &self,
    workspace_dir: &Path,
    session_id: &str,
    reindex_handle: Option<rig_core::tool::server::ToolServerHandle>,
) {
    let workspace_dir = workspace_dir.to_path_buf();
    let session_id = session_id.to_string();
    let flush_session_id = format!("flush:{}", session_id);
    let config = self.config.clone();
    let flush_sem = self.flush_sem.clone();
    let on_flush_queued = self.on_flush_queued.clone();
    let on_flush_executing = self.on_flush_executing.clone();
    let on_flush_done = self.on_flush_done.clone();
    // reindex_handle はそのまま move キャプチャ（Option<ToolServerHandle> は Clone 不要）

    if let Some(ref cb) = on_flush_queued {
        cb(&flush_session_id);
    }

    tokio::spawn(async move {
        // ...既存のセマフォ取得ロジック（変更なし）...
```

### Step 4: 呼び出し元 2 箇所を更新する

`execute_with_rig_agent` 内の 2 箇所（line 1230 付近、line 1608 付近）:

```rust
// Before（両箇所共通）
self.trigger_memory_flush_async(workspace_dir, session_id);

// After（両箇所共通）
self.trigger_memory_flush_async(workspace_dir, session_id, Some(tool_handle.clone()));
```

注意: `execute_with_rig_agent` 内の ToolServerHandle の変数名は `tool_handle`（line 1427 参照）。

### Step 5: テストを追加する

`agent/lib.rs` のテストモジュールに追加する:

```rust
#[tokio::test]
async fn test_reindex_memory_after_flush_no_panic_on_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    // MEMORY.md が存在しない状態でもパニックしないこと（fail-open）
    let server = rig_core::tool::server::ToolServer::new();
    let handle = server.run();
    reindex_memory_after_flush(dir.path(), &handle).await;
    // パニックしなければ OK
}

#[tokio::test]
async fn test_reindex_memory_after_flush_with_content() {
    let dir = tempfile::tempdir().unwrap();
    let memory = "## Preferences\n- Language: Japanese\n- Name: GeminiClaw\n";
    fs::write(dir.path().join("MEMORY.md"), memory).unwrap();
    // ctx_index が未接続でも fail-open で完了すること
    let server = rig_core::tool::server::ToolServer::new();
    let handle = server.run();
    reindex_memory_after_flush(dir.path(), &handle).await;
    // パニックしなければ OK（ctx_index は fail-open）
}
```

### Step 6: ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-agent -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 4: テスト・Clippy・コミット・ドキュメント更新

**Files:**
- Modify: `docs/plans/2026-06-13-phase52-5-memory-rag.md`
- Modify: `docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md`
- Modify: `docs/task.md`

### Step 1: 全ワークスペーステストを実行する

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | grep -E "^test result|FAILED"
```

期待: 全 crate で `test result: ok`

### Step 2: Clippy を確認する

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error"
```

期待: 出力なし

### Step 3: 実装コミットする

```bash
git checkout -b feat/phase52-5
git add crates/rustyclaw-agent/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(memory): Phase 52-5 MEMORY.md セマンティック分割・ctx_search 動的注入・フラッシュ後再インデックス"
```

### Step 4: 本計画書のチェックリストを更新する

本計画書のすべての `- [ ]` を `- [x]` に更新し、ステータスを `[DONE]` に変更する。

### Step 5: 実装計画書を更新する

`docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md` の Phase 52-5 チェックリスト 3 項目をすべて `[x]` に更新する。

### Step 6: task.md を更新する

`docs/task.md` の Phase 52-5 エントリに完了日 `2026-06-13` を追記する。

### Step 7: ドキュメントコミットして main にマージする

```bash
git add docs/
git commit -m "docs(phase52): Phase 52-5 完了チェックリスト更新"
git checkout main
git merge --no-ff feat/phase52-5 -m "feat(phase52-5): MEMORY.md Memory RAG（セマンティック分割・動的注入・フラッシュ後再インデックス）"
git branch -d feat/phase52-5
```
