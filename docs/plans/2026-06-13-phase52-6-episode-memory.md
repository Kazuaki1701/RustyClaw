# Phase 52-6: エピソード記憶連携とデイリーブリーフィング高度化 実装計画書

> [!NOTE]
> **ステータス**: `[DONE]` (完了: 2026-06-13)
> **最終更新日**: 2026-06-13
> **対象コード**: `crates/rustyclaw-gateway/src/lib.rs`
> **設計仕様書**: [`docs/specs/2026-06-13-phase52-context-optimization-design.md`](../specs/2026-06-13-phase52-context-optimization-design.md)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 毎日生成される daily-summary を SQLite FTS5 にエピソード記憶として蓄積し、Heartbeat 時に睡眠・疲労キーワードを検出した場合は過去の類似エピソードを `ctx_search` で引き出してアドバイザリーとして注入する。

**Architecture:** `cron:daily-summary` ハンドラの `atomic_write` 完了直後に `try_ctx_index` を呼び出し `[daily-summary:{date}]` タグ付きで SQLite に登録。Heartbeat ハンドラでは新規 `extract_vital_alert_query(digest)` 関数でキーワードを検出し、ヒット時のみ `try_ctx_search` を追加実行して "Past similar situation" セクションとして prompt に注入する。

**Tech Stack:** Rust 2024 Edition, `rustyclaw-gateway`, context-mode MCP (ctx_index / ctx_search)

---

## 開発タスクチェックリスト

- [x] **Task 1: daily-summary 結果の自動 ctx_index 登録**
- [x] **Task 2: Heartbeat バイタル相関検索アドバイザリー**
- [x] **Task 3: テスト・Clippy・コミット・ドキュメント更新**

---

## 前提知識: 現状と変更点

### daily-summary ハンドラ（gateway/lib.rs:435〜）

`cron:daily-summary` セッションが来ると:
1. LLM に当日のサマリー生成を依頼
2. `rustyclaw_storage::atomic_write` で `memory/summaries/{date}-daily-summary.md` に保存
3. `db.record_usage(...)` でトークン記録
4. **ctx_index 呼び出しは現状なし** ← ここに追加する

`tool_server_handle` は外側スコープ（~line 220）で `self.tool_server_handle.clone()` として取得済みで、このハンドラ内からも直接参照可能。

### Heartbeat ハンドラ（gateway/lib.rs:256〜）

現状 (~line 306-312):
```rust
let heartbeat_rag_query = build_heartbeat_rag_query(&digest);
if let Some(ctx) = try_ctx_search(&tool_server_handle, &heartbeat_rag_query).await {
    prompt_parts.push(format!("Past context (from episodic memory):\n{}", ctx));
}
```

Phase 52-6 では、この直後に「バイタルキーワードを検出した場合のみ追加 ctx_search」を追加する。

---

## Task 1: daily-summary 結果の自動 ctx_index 登録

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (~line 487-490)

### Step 1: atomic_write の直後に ctx_index 呼び出しを追加する

`cron:daily-summary` ハンドラ内の `atomic_write` 呼び出し（`let _ = rustyclaw_storage::atomic_write(&file_path, response.content.as_bytes()).await;`）の直後に追加する:

```rust
// エピソード記憶として SQLite FTS5 に自動登録（Heartbeat RAG で活用）
let indexed_summary =
    format!("[daily-summary:{}]\n{}", today, response.content);
try_ctx_index(
    &tool_server_handle,
    &indexed_summary,
    &format!("daily-summary:{}", today),
)
.await;
```

### Step 2: ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし（エラーなし）

---

## Task 2: Heartbeat バイタル相関検索アドバイザリー

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (~line 308-312、および末尾のユーティリティ関数群付近)

### Step 1: extract_vital_alert_query 関数を追加する

`extract_patrol_feed_urls` 関数（~line 1232付近）の直後に以下の関数を追加する:

```rust
/// Heartbeat digest 中に睡眠・疲労系のキーワードが含まれる場合、
/// 過去の類似エピソードを ctx_search するためのクエリを返す（なければ None）。
fn extract_vital_alert_query(digest: &str) -> Option<String> {
    let lower = digest.to_lowercase();
    let keywords = [
        "sleep", "tired", "fatigue", "exhausted", "sleepy",
        "睡眠", "疲れ", "疲労", "眠い", "不眠", "不足",
    ];
    if keywords.iter().any(|kw| lower.contains(kw)) {
        Some("daily-summary sleep deprivation fatigue advice past similar situation".to_string())
    } else {
        None
    }
}
```

### Step 2: Heartbeat ハンドラに追加 ctx_search を挿入する

heartbeat ハンドラ内の既存 `try_ctx_search` ブロック（~line 306-312）の直後に追加する:

変更前:
```rust
let heartbeat_rag_query = build_heartbeat_rag_query(&digest);
if let Some(ctx) =
    try_ctx_search(&tool_server_handle, &heartbeat_rag_query).await
{
    prompt_parts
        .push(format!("Past context (from episodic memory):\n{}", ctx));
}
```

変更後:
```rust
let heartbeat_rag_query = build_heartbeat_rag_query(&digest);
if let Some(ctx) =
    try_ctx_search(&tool_server_handle, &heartbeat_rag_query).await
{
    prompt_parts
        .push(format!("Past context (from episodic memory):\n{}", ctx));
}

// バイタルキーワードを検出した場合のみ過去の類似エピソードを追加検索
if let Some(vital_query) = extract_vital_alert_query(&digest) {
    if let Some(advisory) =
        try_ctx_search(&tool_server_handle, &vital_query).await
    {
        prompt_parts
            .push(format!("Past similar situation (advisory):\n{}", advisory));
    }
}
```

### Step 3: ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 3: テスト・Clippy・コミット・ドキュメント更新

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（テストモジュール末尾）
- Modify: `docs/plans/2026-06-13-phase52-6-episode-memory.md`
- Modify: `docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md`
- Modify: `docs/task.md`

### Step 1: extract_vital_alert_query のユニットテストを追加する

`crates/rustyclaw-gateway/src/lib.rs` の末尾の `#[cfg(test)]` モジュールに追加する（既存テストが存在する場合はその末尾に、なければ新しいモジュールを作成する）:

```rust
#[cfg(test)]
mod phase52_6_tests {
    use super::*;

    #[test]
    fn test_extract_vital_alert_query_no_keywords() {
        let digest = "User asked about the weather. Replied with sunny 22°C.";
        assert!(extract_vital_alert_query(digest).is_none());
    }

    #[test]
    fn test_extract_vital_alert_query_sleep_keyword_en() {
        let digest = "User mentioned they felt tired and had poor sleep last night.";
        let query = extract_vital_alert_query(digest);
        assert!(query.is_some());
        let q = query.unwrap();
        assert!(q.contains("sleep"));
    }

    #[test]
    fn test_extract_vital_alert_query_sleep_keyword_ja() {
        let digest = "ユーザーが睡眠不足だと報告しました。";
        let query = extract_vital_alert_query(digest);
        assert!(query.is_some());
    }

    #[test]
    fn test_extract_vital_alert_query_fatigue_keyword() {
        let digest = "User feels fatigue from long work sessions.";
        let query = extract_vital_alert_query(digest);
        assert!(query.is_some());
    }
}
```

### Step 2: 全ワークスペーステストを実行する

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | grep -E "^test result|FAILED"
```

期待: 全クレートで `test result: ok`

### Step 3: Clippy を確認する

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error"
```

期待: 出力なし

### Step 4: feat/phase52-6 ブランチを作成してコミットする

```bash
git checkout -b feat/phase52-6
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(episode): Phase 52-6 daily-summary エピソード記憶 ctx_index 登録 + バイタル相関検索アドバイザリー

- cron:daily-summary 完了後に [daily-summary:{date}] タグ付きで SQLite FTS5 に自動登録
- Heartbeat digest にバイタルキーワード（sleep/疲れ等）が含まれる場合のみ追加 ctx_search を実行
- 過去の類似エピソードを "Past similar situation (advisory)" として heartbeat prompt に注入
- extract_vital_alert_query 関数に 4 件のユニットテスト追加

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

### Step 5: 本計画書のチェックリストを更新する

本計画書（`docs/plans/2026-06-13-phase52-6-episode-memory.md`）の:
- ステータスを `[DONE]` に変更（完了: 2026-06-13）
- 全 `- [ ]` を `- [x]` に変更

### Step 6: 実装計画書を更新する

`docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md` の Phase 52-6 チェックリスト 2 項目を `[x]` に更新:
- `[ ] ブリーフィング結果の自動 ctx_index 登録処理の実装。`
- `[ ] ctx_search を用いた過去のバイタル・予定傾向の相関検索とアドバイス注入の実装。`

また、Phase 52 全体のステータス行を更新する（Phase 52-6 完了）。

### Step 7: task.md を更新する

`docs/task.md` の Phase 52-6 エントリを `[x]` に更新し、完了日 `2026-06-13` を追記する。

### Step 8: ドキュメントコミットして main にマージする

```bash
git add docs/
git commit -m "$(cat <<'EOF'
docs(phase52): Phase 52-6 完了チェックリスト更新

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
git checkout main
git merge --no-ff feat/phase52-6 -m "feat(phase52-6): エピソード記憶連携・デイリーブリーフィング高度化（daily-summary ctx_index + バイタル相関検索）"
git branch -d feat/phase52-6
```

**push は行わない。**
