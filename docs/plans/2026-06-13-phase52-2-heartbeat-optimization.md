# Phase 52-2: 用途別最適化 - Heartbeat 実装計画書

> [!NOTE]
> **ステータス**: `[ACTIVE]` (実装準備中)
> **最終更新日**: 2026-06-13
> **対象コード**: `crates/rustyclaw-agent/src/lib.rs`, `crates/rustyclaw-gateway/src/lib.rs`
> **設計仕様書**: [`docs/specs/2026-06-13-phase52-context-optimization-design.md`](../specs/2026-06-13-phase52-context-optimization-design.md)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Heartbeat 実行時のコンテキストとツールを監視専用に絞り込み、不要なトークン消費と操作リスクを排除する。

**Architecture:** `build_heartbeat_context` から SOUL.md を除去して HEARTBEAT.md のみに限定。Heartbeat 専用 `ToolRegistry` から書き込みツール（WorkspaceWriteTool）を除外する。

**Tech Stack:** Rust 2024 Edition, `rustyclaw-agent`, `rustyclaw-gateway`

---

## 開発タスクチェックリスト

- [x] **Task 1: build_heartbeat_context から SOUL.md を除去**
- [x] **Task 2: Heartbeat ToolRegistry から WorkspaceWriteTool を除外**
- [x] **Task 3: テスト更新・全テスト PASS 確認**
- [x] **Task 4: ドキュメント整理（計画書チェック・task.md 更新）**

---

## Task 1: build_heartbeat_context から SOUL.md を除去

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:993`

**現状**: `let files = ["SOUL.md", "HEARTBEAT.md"];`
→ SOUL.md（キャラクター設定、~3000 chars）が毎 Heartbeat リクエストで送信されている。

- [ ] `lib.rs:993` のファイルリストを変更する

```rust
// Before
let files = ["SOUL.md", "HEARTBEAT.md"];

// After
let files = ["HEARTBEAT.md"];
```

- [ ] ビルドが通ることを確認

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error"
```
期待: 出力なし（エラーなし）

- [ ] コミット（Task 2 と合わせて 1 コミット）

---

## Task 2: Heartbeat ToolRegistry から WorkspaceWriteTool を除外

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs:~1219-1226`

**現状**: WorkspaceWriteTool が `tool_registry`（Heartbeat 専用）と `tool_server_handle`（Chat 用）の両方に登録されている。
→ Heartbeat が誤って workspace ファイルを書き換えるリスクがある。
→ `tool_server_handle` への登録は維持し、`tool_registry` への登録のみ削除する。

- [ ] `gateway/src/lib.rs` の WorkspaceWriteTool ブロックを修正する

```rust
// Before
{
    let t = rustyclaw_tools::WorkspaceWriteTool::new(
        self.workspace_path.clone(),
        preview_base_url.clone(),
    );
    tool_registry.register(Arc::new(t.clone()) as Arc<dyn rig_core::tool::ToolDyn>);
    tool_server_handle.add_tool(t).await.ok();
}

// After: tool_registry への登録を削除（Chat 用 tool_server_handle は維持）
{
    let t = rustyclaw_tools::WorkspaceWriteTool::new(
        self.workspace_path.clone(),
        preview_base_url.clone(),
    );
    // Heartbeat には書き込みツールを公開しない（読み取りと通知のみ許可）
    tool_server_handle.add_tool(t).await.ok();
}
```

- [ ] ビルドが通ることを確認

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```
期待: 出力なし

- [ ] Task 1 と合わせてコミット

```bash
git add crates/rustyclaw-agent/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(heartbeat): Phase 52-2 SOUL.md 除去・WorkspaceWriteTool をヘルスベート registry から除外"
```

---

## Task 3: テスト更新・全テスト PASS 確認

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:3405,3425`

Task 1 の変更により、以下の 2 つのテストが更新必要:

**test_build_heartbeat_context_does_not_include_memory_md** (lib.rs ~3404):
```rust
// Before
let files: &[&str] = &["SOUL.md", "HEARTBEAT.md"]; // MEMORY.md を含まない

// After
let files: &[&str] = &["HEARTBEAT.md"]; // SOUL.md も MEMORY.md も含まない
```

**test_build_heartbeat_context_is_static** (lib.rs ~3420):
```rust
// Before
assert!(context.contains("# SOUL.md"));
assert!(context.contains("# HEARTBEAT.md"));

// After: SOUL.md は除外されたので assertion を削除・HEARTBEAT.md のみ確認
assert!(!context.contains("# SOUL.md"), "SOUL.md は Heartbeat コンテキストから除外されるべき");
assert!(context.contains("# HEARTBEAT.md"));
```

- [ ] テストを修正する

- [ ] 全ワークスペーステストを実行する

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | grep -E "^test result|FAILED"
```
期待: 全 crate で `test result: ok`

- [ ] Clippy を確認する

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error"
```
期待: 出力なし

- [ ] テスト修正をコミット

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "test(heartbeat): Phase 52-2 build_heartbeat_context のテストを SOUL.md 除外後に更新"
```

---

## Task 4: ドキュメント整理

- [ ] 本計画書のチェックリストをすべて `[x]` に更新する

- [ ] `docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md` の Phase 52-2 チェックリストを `[x]` に更新する

- [ ] `docs/task.md` の Phase 52-2 に完了日を追記する（Phase 52-3 が次の優先に）

- [ ] ドキュメント更新をコミットして main にマージ

```bash
git add docs/
git commit -m "docs(phase52): Phase 52-2 完了チェックリスト更新"
git checkout main
git merge --no-ff feat/phase52-2 -m "feat(phase52-2): Heartbeat 専用コンテキスト最適化"
git branch -d feat/phase52-2
```
