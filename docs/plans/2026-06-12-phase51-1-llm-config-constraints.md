# Phase 51-1: LLM Config 制限の適切な適用 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `LlmModelConfig` に制限情報（context_window / rpm / tpm 等）を乗せてパイプライン全体で正しく参照し、小コンテキストモデルでも安全に動作させる。

**Architecture:** `rustyclaw-config` で `LlmModelConfig` に 5 フィールドを追加し `resolve_model()` で確定。エージェント側では (1) トークン予算ベースの履歴件数制限・(2) per-model RateLimiter・(3) 小コンテキスト時のシステムプロンプト圧縮の 3 層で多段防御する。

**Tech Stack:** Rust 2024 Edition、`rustyclaw-config`、`rustyclaw-agent`、TDD (`cargo test`)

---

## 実装状況（2026-06-12 時点）

| サブタスク | 状態 |
|---|---|
| `LlmModelConfig` に 5 フィールド追加・`resolve_model()` / `get_model()` へ伝播 | ✅ 完了（コミット `ef69c8c`） |
| `parse_context_window()` を `rustyclaw-config` に移動・公開 | ✅ 完了 |
| `get_history_message_limit()` をトークン予算式に置き換え | ✅ 完了 |
| `RateLimiter`（rpm / tpm ソフトリミット）追加 | ✅ 完了 |
| **小コンテキストモデルの下限クランプ修正** | ❌ 未実装（本計画 Task 1） |
| **`build_system_context()` の小コンテキスト対応** | ❌ 未実装（本計画 Task 2） |

---

## ファイル構成

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | Task 1: `get_history_message_limit()` 修正・Task 2: `build_system_context()` 引数追加と注入ロジック変更・全呼び出し元更新・テスト追加 |

---

## Task 1: 小コンテキストモデルの下限クランプ修正

**問題**: `get_history_message_limit()` の下限クランプが固定値 20 のため、4096 tokens モデルでは `20 × 350 ≈ 7,000 tokens` を要求してしまう（モデル上限超過）。

**期待値（修正後）:**

| context_window | raw 計算 | 適用 min | 結果 |
|---|---|---|---|
| 4,096 | (4096×65/100)/350 = 7 | 2 | **7** |
| 8,192 | (8192×65/100)/350 = 15 | 2 | **15** |
| 16,384 | (16384×65/100)/350 = 30 | 20 | 30（変化なし） |
| 32,768 | 60 | 20 | 60（変化なし） |
| 131k | 150（上限） | 20 | 150（変化なし） |

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:394-398`（`get_history_message_limit`）

- [ ] **Step 1: 失敗するテストを追加**

`crates/rustyclaw-agent/src/lib.rs` の `test_get_history_message_limit_uses_context_window` テスト関数末尾に追加：

```rust
// 小コンテキストモデル向けの下限クランプ確認
let p4k = Pipeline::new(make_config("4096"), flush_sem.clone());
let p8k = Pipeline::new(make_config("8192"), flush_sem.clone());
assert_eq!(p4k.get_history_message_limit("default"), 7, "4096 → 7件（下限クランプ = 2 のため生の計算値が使われる）");
assert_eq!(p8k.get_history_message_limit("default"), 15, "8192 → 15件（下限クランプ = 2）");
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
TZ=UTC cargo test --all-features --workspace -- test_get_history_message_limit_uses_context_window 2>&1 | tail -20
```

期待: `left: 20` vs `right: 7` のような assertion error

- [ ] **Step 3: `get_history_message_limit()` を修正**

`crates/rustyclaw-agent/src/lib.rs:394-398` を以下に置き換える：

```rust
fn get_history_message_limit(&self, purpose: &str) -> usize {
    let cw = self.config.get_model(purpose).context_window_tokens;
    let raw = (cw * 65 / 100) / 350;
    let min = if cw <= 8_192 { 2 } else { 20 };
    raw.clamp(min, 150)
}
```

（コメント行 `// 65% of context window for history; average ~350 tokens per message` はその上に残す）

- [ ] **Step 4: テストが通ることを確認**

```bash
TZ=UTC cargo test --all-features --workspace -- test_get_history_message_limit_uses_context_window 2>&1 | tail -10
```

期待: `test ... ok`（全 7 アサーション pass）

- [ ] **Step 5: Clippy を通す**

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "error\|warning:" | head -20
```

期待: warning / error なし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "fix(agent): Phase 51-1 小コンテキストモデルの下限クランプを文脈依存に修正"
```

---

## Task 2: `build_system_context()` の小コンテキスト対応

**問題**: `build_system_context()` が常に `SOUL.md + USER.md + proactive-posts` を注入する。4096 tokens モデルでは system prompt だけで予算の大半を消費する。

**方針**: 引数 `context_window_tokens: usize` を追加し、`≤ 8_192` では `SOUL.md` のみ注入（`USER.md` と `proactive-posts` を省略）。

**トークン概算（修正前後）:**

| 注入内容 | 推計 tokens |
|---|---|
| SOUL.md（3000 chars）+ USER.md（3000 chars） | ~1,500 |
| SOUL.md のみ | ~750 |
| 節約分 | ~750 tokens → 履歴 2〜3 往復分を確保可能 |

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:422`（`build_system_context` シグネチャ変更）
- Modify: `crates/rustyclaw-agent/src/lib.rs:1007,1299,1482`（呼び出し元 3 箇所）
- Modify: `crates/rustyclaw-agent/src/lib.rs:2191,2522,3170`（テスト呼び出し 3 箇所）

- [ ] **Step 1: 失敗するテストを追加**

テストモジュール（`#[cfg(test)]` ブロック）内、`test_build_system_context_returns_static_content` の近くに追加：

```rust
#[test]
fn test_build_system_context_small_model_omits_user_md() {
    let dir = tempfile::TempDir::new().unwrap();
    let ws = dir.path();
    std::fs::write(ws.join("SOUL.md"), "soul content").unwrap();
    std::fs::write(ws.join("USER.md"), "user content").unwrap();

    let config = make_test_config_with_url("http://localhost");
    let sem = Arc::new(Semaphore::new(1));
    let pipeline = Pipeline::new(config, sem);

    // 4096 tokens モデル → SOUL.md のみ
    let ctx = pipeline.build_system_context(ws, 4_096).unwrap();
    assert!(ctx.contains("# SOUL.md"), "SOUL.md は常に注入される");
    assert!(!ctx.contains("# USER.md"), "4096 では USER.md を省略");

    // 32768 tokens モデル → 両方注入
    let ctx32k = pipeline.build_system_context(ws, 32_768).unwrap();
    assert!(ctx32k.contains("# SOUL.md"), "32k: SOUL.md 注入");
    assert!(ctx32k.contains("# USER.md"), "32k: USER.md 注入");
}
```

- [ ] **Step 2: テストが失敗することを確認**

（この時点では `build_system_context` がまだ引数 1 個のため、コンパイルエラーが出る）

```bash
TZ=UTC cargo test --all-features --workspace -- test_build_system_context_small_model_omits_user_md 2>&1 | head -30
```

期待: `error[E0061]: this function takes 1 argument but 2 arguments were supplied`

- [ ] **Step 3: `build_system_context()` シグネチャと実装を変更**

`crates/rustyclaw-agent/src/lib.rs:422` の `build_system_context` 関数全体を置き換える：

```rust
pub fn build_system_context(&self, workspace_dir: &Path, context_window_tokens: usize) -> Result<String> {
    // 静的ブロック（SOUL/USER）のみを返す。
    // 動的な [now:] は呼び出し元で追加する（build_heartbeat_context と同パターン）。
    const MAX_CONTEXT_CHARS_PER_FILE: usize = 3_000;
    let files: &[&str] = if context_window_tokens <= 8_192 {
        &["SOUL.md"]
    } else {
        &["SOUL.md", "USER.md"]
    };
    let mut context = String::new();

    for filename in files {
        let path = workspace_dir.join(filename);
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                tracing::warn!(
                    "Failed to read context file {:?}: {}. Using empty content.",
                    path,
                    e
                );
                String::new()
            }
        };

        let stripped = Self::strip_comments(&content);
        let truncated = Self::truncate_context_content(&stripped, MAX_CONTEXT_CHARS_PER_FILE);
        context.push_str(&format!("# {}\n\n{}\n\n", filename, truncated));
    }

    // proactive-posts.md を注入（最終1件のみ・大コンテキストモデルのみ）
    if context_window_tokens > 8_192 {
        let posts_path = workspace_dir.join("memory").join("proactive-posts.md");
        if let Ok(posts) = fs::read_to_string(&posts_path)
            && let Some(last_entry) = posts.lines().rfind(|l| !l.trim().is_empty())
        {
            context.push_str(&format!(
                "# Recent AI Proactive Posts\n\n{}\n\n",
                last_entry
            ));
        }
    }

    Ok(context)
}
```

- [ ] **Step 4: 呼び出し元 3 箇所を更新**

**呼び出し元 1 — `execute()` (line ~1007):**

```rust
// 変更前
let mut system_context = self.build_system_context(workspace_dir)?;

// 変更後
let cw = self.config.get_model("default").context_window_tokens;
let mut system_context = self.build_system_context(workspace_dir, cw)?;
```

**呼び出し元 2 — `execute_with_rig_agent()` (line ~1299):**

```rust
// 変更前
let mut system_context = self.build_system_context(workspace_dir)?;

// 変更後
let cw = self.config.get_model(purpose).context_window_tokens;
let mut system_context = self.build_system_context(workspace_dir, cw)?;
```

**呼び出し元 3 — `execute_stream()` (line ~1482):**

```rust
// 変更前
let mut system_context = self.build_system_context(workspace_dir)?;

// 変更後
let cw = self.config.get_model("default").context_window_tokens;
let mut system_context = self.build_system_context(workspace_dir, cw)?;
```

- [ ] **Step 5: 既存テスト 3 箇所を更新**

**`test_build_system_context_returns_static_content` (line ~2191):**

```rust
// 変更前
let context = pipeline.build_system_context(ws_dir.path()).unwrap();

// 変更後（4096 → SOUL.md のみ。テストは SOUL.md の存在と [now:] 非存在を確認しているので問題なし）
let context = pipeline.build_system_context(ws_dir.path(), 4_096).unwrap();
```

**`test_build_system_context_injects_proactive_posts` (line ~2522):**

```rust
// 変更前
let ctx = pipeline.build_system_context(ws).unwrap();

// 変更後（32768 → proactive-posts が注入されること確認）
let ctx = pipeline.build_system_context(ws, 32_768).unwrap();
```

**`test_build_system_context_truncates_large_files` (line ~3170):**

```rust
// 変更前
let result = pipeline.build_system_context(workspace).unwrap();

// 変更後（32768 → USER.md が注入されてテスト内の find("# USER.md") が動く）
let result = pipeline.build_system_context(workspace, 32_768).unwrap();
```

- [ ] **Step 6: 全テストが通ることを確認**

```bash
TZ=UTC cargo test --all-features --workspace -- build_system_context 2>&1 | tail -20
```

期待: 4 テストすべて ok
- `test_build_system_context_returns_static_content`
- `test_build_system_context_injects_proactive_posts`
- `test_build_system_context_truncates_large_files`
- `test_build_system_context_small_model_omits_user_md`

- [ ] **Step 7: ワークスペース全テスト + Clippy**

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error" | head -10
```

期待: テスト全 pass、Clippy error なし

- [ ] **Step 8: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 51-1 小コンテキストモデルでシステムプロンプトを SOUL.md のみに絞る"
```

---

## Task 3: docs/task.md 更新・Phase 51-1 完了マーク

**Files:**
- Modify: `docs/task.md`（`[ ]` → `[x]`）
- Modify: `docs/specs/v0.4/92_llm_config_constraints.md`（ステータス更新）

- [ ] **Step 1: `docs/task.md` の Phase 51-1 チェックボックスを完了に変更**

```
- [ ] **Phase 51-1: LLM config 制限の適切な適用**（最優先）
↓
- [x] **Phase 51-1: LLM config 制限の適切な適用**（完了 2026-06-12）
```

- [ ] **Step 2: `docs/specs/v0.4/92_llm_config_constraints.md` のステータスを更新**

ファイル冒頭の NOTE ブロックを以下に変更：

```markdown
> [!NOTE]
> **ステータス**: `[実装完了]`
> **バージョン**: v0.4
> **最終更新日**: 2026-06-12（Phase 51-1 全サブタスク完了）
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md) / [`docs/task.md`](../../task.md)
```

§6 の各残課題ヘッダーも更新：

```
### 6.1 小コンテキストモデル向け下限クランプの修正（高優先）
→ ### 6.1 ✅ 小コンテキストモデル向け下限クランプの修正（完了 Task 1）

### 6.2 システムプロンプトの context_window 対応（中優先）
→ ### 6.2 ✅ システムプロンプトの context_window 対応（完了 Task 2）
```

- [ ] **Step 3: コミット**

```bash
git add docs/task.md docs/specs/v0.4/92_llm_config_constraints.md
git commit -m "docs: Phase 51-1 完了マーク・仕様書ステータス更新"
```

---

## 実行手順サマリ

```
git checkout -b feat/phase-51-1-small-context-fixes
# Task 1 の Steps 1-6 を実行
# Task 2 の Steps 1-8 を実行
# Task 3 の Steps 1-3 を実行
git push -u origin feat/phase-51-1-small-context-fixes
# PR 作成 → CI green → --no-ff マージ
```

## PR 本文テンプレート

```
### 1. 意思決定の背景と選定理由 (Why)
- **課題**: 4096 tokens 制限モデル（lms-gemma-4-12b）で履歴下限クランプ 20 件が逆効果（20×350≈7000 > 4096）
- **選択アプローチ**: context_window_tokens ≤ 8192 を「小コンテキスト」と分類し、下限を 2、システムプロンプトを SOUL.md のみに縮小
- **不採用アプローチ**: config-driven な閾値設定（設定項目増加・運用コスト増のため見送り）

### 2. 主な変更内容 (What)
- `get_history_message_limit()`: 下限クランプを `if cw <= 8_192 { 2 } else { 20 }` に変更
- `build_system_context()`: `context_window_tokens: usize` 引数追加、≤8192 で SOUL.md のみ注入
- 呼び出し元 3 箇所・テスト 3 箇所を更新、新テスト 1 件追加

### 3. 関連ドキュメント (Traceability)
- 計画書: docs/plans/2026-06-12-phase51-1-llm-config-constraints.md
- 調査メモ: docs/specs/v0.4/92_llm_config_constraints.md §6.1・§6.2

Closes #（Issue番号）
```
