# Phase 52-7: ctx_search sort 最適化・reindex ログ修正 実装計画書

> [!NOTE]
> **ステータス**: `[HISTORICAL]` (完了: 2026-06-13)
> **最終更新日**: 2026-06-13
> **対象コード**: `crates/rustyclaw-gateway/src/lib.rs`, `crates/rustyclaw-agent/src/lib.rs`
> **元課題**: `docs/task.md` § Phase 52 後続改善候補（2026-06-13 最終レビュー抽出）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `try_ctx_search` に `sort` 引数を追加して用途別に最適な検索モードを選択できるようにし、`reindex_memory_after_flush` の誤「完了」ログおよびデバッグレベル不一致を修正する。

**Architecture:** `try_ctx_search(handle, query, sort)` とシグネチャを変更し、5 箇所の呼び出しを用途に応じて `"timeline"` / `"relevance"` に振り分ける。`reindex_memory_after_flush` は成功カウンタを導入し、失敗時を `warn` に変更して全失敗でも誤情報ログが出ないようにする。

**Tech Stack:** Rust 2024 Edition, `rustyclaw-gateway`, `rustyclaw-agent`, context-mode MCP

---

## 妥当性検証サマリー

### ✅ Item 1: ctx_search sort 分離（採用）

context-mode v1.0.162 の `src/search/unified.ts` および `src/server.ts` で確認:

| sort 値 | 動作 | 適用用途 |
|---------|------|---------|
| `"relevance"` | ContentStore のみ BM25 ランク順 | スキル選択・interests・memory_extra・vital advisory |
| `"timeline"` | ContentStore + SessionDB + auto-memory を時系列順マージ | Heartbeat RAG（過去セッション横断が必要） |

現状は全 5 箇所が `"timeline"` 固定。Heartbeat RAG のみ `"timeline"` が適切で、残り 4 箇所は `"relevance"` が精度上の最適解。

### ✅ Item 2+3: reindex ログ修正（採用、1 関数で解決）

`reindex_memory_after_flush` の問題:
1. `let count = chunks.len()` で全チャンク数を先に確定してから `info!("… {} チャンク完了", count)` を無条件出力 → **全失敗時でも「完了」と表示**
2. ctx_index 失敗が `debug!` → 本番ログ（info 以上）で不可視 → gateway の `try_ctx_index`（`warn!`）と非対称

### ❌ Item 4: ctx_patch 部分メモリ書き換え（Phase 52-7 スコープ除外）

理由: (a) LLM プロンプト変更が Memory Flush 品質に直結するリスクが高い (b) `reindex_memory_after_flush` で flush 後全件再インデックスする仕組みが既存であり部分更新の優先度が低下 (c) 実装複雑度（LLM 出力フォーマット変更 + ctx_patch 呼び出し）に対して得られるトークン削減効果が限定的。引き続き将来課題として維持する。

---

## 開発タスクチェックリスト

- [x] **Task 1: try_ctx_search に sort 引数を追加し全呼び出し元を更新**
- [x] **Task 2: reindex_memory_after_flush のログを修正**
- [x] **Task 3: テスト・Clippy・コミット・ドキュメント更新**

---

## 前提知識

### try_ctx_search の現在の実装（gateway/lib.rs:1135）

```rust
async fn try_ctx_search(
    handle: &rig_core::tool::server::ToolServerHandle,
    query: &str,
) -> Option<String> {
    let args = serde_json::json!({
        "queries": [query],
        "sort": "timeline",
        "limit": 3
    })
    .to_string();
    ...
}
```

### try_ctx_search の呼び出し元（5 箇所）と最適 sort

| 行 | 用途 | 現状 | 最適 |
|----|------|------|------|
| ~308 | Heartbeat RAG（過去セッション横断） | `"timeline"` | **`"timeline"`（維持）** |
| ~316 | バイタル相関検索アドバイザリー | `"timeline"` | **`"relevance"`（変更）** |
| ~728 | スキル動的選択 | `"timeline"` | **`"relevance"`（変更）** |
| ~818 | USER.md interests 動的注入 | `"timeline"` | **`"relevance"`（変更）** |
| ~832 | memory_extra（MEMORY.md チャンク） | `"timeline"` | **`"relevance"`（変更）** |

---

## Task 1: try_ctx_search に sort 引数を追加し全呼び出し元を更新

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (line 1135, ~308, ~316, ~728, ~818, ~832)

### Step 1: try_ctx_search のシグネチャと実装を変更する

`gateway/lib.rs` line 1135 の `try_ctx_search` 関数を変更する:

```rust
// Before
async fn try_ctx_search(
    handle: &rig_core::tool::server::ToolServerHandle,
    query: &str,
) -> Option<String> {
    let args = serde_json::json!({
        "queries": [query],
        "sort": "timeline",
        "limit": 3
    })
    .to_string();

// After
async fn try_ctx_search(
    handle: &rig_core::tool::server::ToolServerHandle,
    query: &str,
    sort: &str,
) -> Option<String> {
    let args = serde_json::json!({
        "queries": [query],
        "sort": sort,
        "limit": 3
    })
    .to_string();
```

### Step 2: Heartbeat RAG（~line 308）を "timeline" に更新する

```rust
// Before
try_ctx_search(&tool_server_handle, &heartbeat_rag_query).await

// After
try_ctx_search(&tool_server_handle, &heartbeat_rag_query, "timeline").await
```

### Step 3: バイタル相関検索（~line 316）を "relevance" に更新する

```rust
// Before
try_ctx_search(&tool_server_handle, &vital_query).await

// After
try_ctx_search(&tool_server_handle, &vital_query, "relevance").await
```

### Step 4: スキル動的選択（~line 728）を "relevance" に更新する

```rust
// Before
try_ctx_search(&tool_server_handle, &content)
    .await
    .map(|ctx| parse_skill_names_from_ctx(&ctx))
    .filter(|names| !names.is_empty())

// After
try_ctx_search(&tool_server_handle, &content, "relevance")
    .await
    .map(|ctx| parse_skill_names_from_ctx(&ctx))
    .filter(|names| !names.is_empty())
```

### Step 5: interests 動的注入（~line 818）を "relevance" に更新する

```rust
// Before
try_ctx_search(&tool_server_handle, &query)
    .await
    .filter(|r| r.contains("[user-interests]"))
    ...

// After
try_ctx_search(&tool_server_handle, &query, "relevance")
    .await
    .filter(|r| r.contains("[user-interests]"))
    ...
```

### Step 6: memory_extra（~line 832）を "relevance" に更新する

```rust
// Before
try_ctx_search(&tool_server_handle, &content)
    .await
    .filter(|r| r.contains("[memory-chunk]"))
    ...

// After
try_ctx_search(&tool_server_handle, &content, "relevance")
    .await
    .filter(|r| r.contains("[memory-chunk]"))
    ...
```

### Step 7: ビルド確認

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 2: reindex_memory_after_flush のログを修正

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs` (~line 1835-1861)

### Step 1: reindex_memory_after_flush を修正する

`agent/lib.rs` の `reindex_memory_after_flush` 関数内のループ部分を変更する:

```rust
// Before
let count = chunks.len();
for (i, chunk) in chunks.iter().enumerate() {
    let args = serde_json::json!({
        "content": format!("[memory-chunk]\n{}", chunk),
        "source": format!("memory-chunk:{}", i)
    })
    .to_string();
    if let Err(e) = handle.call_tool("ctx_index", &args).await {
        tracing::debug!("reindex_memory: ctx_index 失敗（fail-open）: {}", e);
    }
}
tracing::info!("memory flush: {} チャンク再インデックス完了", count);

// After
let total = chunks.len();
let mut success: usize = 0;
for (i, chunk) in chunks.iter().enumerate() {
    let args = serde_json::json!({
        "content": format!("[memory-chunk]\n{}", chunk),
        "source": format!("memory-chunk:{}", i)
    })
    .to_string();
    match handle.call_tool("ctx_index", &args).await {
        Ok(_) => success += 1,
        Err(e) => tracing::warn!("reindex_memory: ctx_index 失敗（fail-open）: {}", e),
    }
}
if success == total {
    tracing::info!("memory flush: {}/{} チャンク再インデックス完了", success, total);
} else {
    tracing::warn!(
        "memory flush: 再インデックス {}/{} チャンク成功（{} 件失敗）",
        success,
        total,
        total - success,
    );
}
```

### Step 2: ビルド確認

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 3: テスト・Clippy・コミット・ドキュメント更新

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`（テストモジュール末尾）
- Modify: `docs/plans/2026-06-13-phase52-7-search-log-improvements.md`
- Modify: `docs/task.md`

### Step 1: try_ctx_search の sort 引数テストを追加する

`crates/rustyclaw-gateway/src/lib.rs` にテストモジュールが存在する場合はその末尾に追加する。**`phase52_6_tests` モジュールの直後**に追加するのが自然:

```rust
#[cfg(test)]
mod phase52_7_tests {
    use super::*;

    #[test]
    fn test_extract_vital_alert_query_returns_some_on_sleep() {
        // extract_vital_alert_query の回帰テスト（sort 変更前後で動作が変わらないことを確認）
        let digest = "User mentioned poor sleep and fatigue.";
        assert!(extract_vital_alert_query(digest).is_some());
    }

    #[test]
    fn test_extract_vital_alert_query_returns_none_on_normal() {
        let digest = "User asked about the calendar for tomorrow.";
        assert!(extract_vital_alert_query(digest).is_none());
    }
}
```

**注意**: `try_ctx_search` は MCP 接続が必要なため直接テストしない。sort 引数の正しさはコンパイル + Clippy で保証する。

### Step 2: reindex_memory_after_flush のテストを追加する

`crates/rustyclaw-agent/src/lib.rs` の既存テストモジュールに追加する:

```rust
#[tokio::test]
async fn test_reindex_memory_after_flush_no_panic_on_missing_file() {
    // MEMORY.md が存在しない状態でもパニックしないこと（fail-open）
    let dir = tempfile::tempdir().unwrap();
    let server = rig_core::tool::server::ToolServer::new();
    let handle = server.run();
    reindex_memory_after_flush(dir.path(), &handle).await;
    // パニックしなければ OK
}
```

**注意**: 既存の同名テスト（Phase 52-5 で追加済み）と重複していないか確認してから追加する。重複している場合はスキップ。

### Step 3: 全ワークスペーステストを実行する

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | grep -E "^test result|FAILED"
```

期待: 全クレートで `test result: ok`

### Step 4: Clippy を確認する

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error"
```

期待: 出力なし

### Step 5: feat/phase52-7 ブランチを作成してコミットする

```bash
git checkout -b feat/phase52-7
git add crates/rustyclaw-gateway/src/lib.rs crates/rustyclaw-agent/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(search): Phase 52-7 ctx_search sort 最適化・reindex ログ修正

- try_ctx_search に sort 引数を追加（"timeline" / "relevance"）
- Heartbeat RAG のみ sort=timeline を維持、他 4 箇所を sort=relevance に変更
  - バイタル相関検索・スキル選択・interests 注入・memory_extra が BM25 精度向上
- reindex_memory_after_flush: 成功カウンタ導入・失敗時を debug→warn に変更
  - 全失敗時でも「完了」info ログが出る誤情報を解消

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

### Step 6: 本計画書を更新する

`docs/plans/2026-06-13-phase52-7-search-log-improvements.md` の:
- ステータスを `[DONE]` に変更（完了: 2026-06-13）
- 全 `- [ ]` を `- [x]` に変更

### Step 7: task.md の後続改善候補を更新する

`docs/task.md` の「Phase 52 後続改善候補」から、完了した以下 3 項目を削除（または `[x]` 済みとしてコメントアウト）する:
- `ctx_search の sort 戦略を用途別に分離`
- `reindex_memory_after_flush の誤「完了」ログ修正`
- `agent 側 ctx_index 失敗ログを warn に統一`

Phase 52-5b（ctx_patch）は引き続き残す。

### Step 8: ドキュメントコミットして main にマージする

```bash
git add docs/
git commit -m "$(cat <<'EOF'
docs(phase52-7): Phase 52-7 完了チェックリスト更新・task.md 後続改善候補整理

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
git checkout main
git merge --no-ff feat/phase52-7 -m "feat(phase52-7): ctx_search sort 最適化・reindex ログ修正（後続改善候補 3 件解消）"
git branch -d feat/phase52-7
```

**push は行わない。**
