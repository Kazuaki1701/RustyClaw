# Phase 51-2: LANE QUEUE Memory Flush 可視化 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Memory flush と Session Summary の LLM 実行が Dashboard の LANE QUEUE に表示されるようにし、`UNKNOWN` バッジを解消する。

**Architecture:** `Pipeline` struct にオプションのコールバック（`on_flush_queued` / `on_flush_executing` / `on_flush_done`）を追加し、`trigger_memory_flush_async()` が呼び出すことで、`rustyclaw-agent` から `rustyclaw-gateway` へ依存関係を逆転させずにキュー状態を通知する（Option A: コールバック注入）。Service Badge は JavaScript 定数なので 1 行追加で対応。

**Tech Stack:** Rust, tokio, `Arc<dyn Fn(&str) + Send + Sync>`, JavaScript (health.rs インライン)

**設計判断（採用・不採用）:**

| 案 | 内容 | 採否 |
|---|---|---|
| **Option A（本計画）** | `Pipeline` にコールバック注入 | ✅ 採用 — 循環依存なし・fail-open |
| Option B | `flush_memory` を pub 化し gateway が spawn | ❌ gateway がセマフォを持つ必要があり侵略的 |
| Option C | `QUEUE_STATE` を `rustyclaw-storage` に移動 | ❌ storage に UI 依存が入る |

---

## ファイル構成

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | `Pipeline` struct にコールバックフィールド追加・`new()` 初期化・`with_flush_callbacks()` ビルダー・`trigger_memory_flush_async()` 変更 |
| `crates/rustyclaw-gateway/src/lib.rs` | 行 643 の `Pipeline::new()` 呼び出しにコールバックを連鎖 |
| `crates/rustyclaw-gateway/src/health.rs` | `SERVICE_BADGES` に `flush:` と `cron:session-summary` を追加 |

**変更しないファイル:**
- 行 350 (`Pipeline::new` for heartbeat) — `execute_heartbeat` は `trigger_memory_flush_async` を呼ばないため不要
- 行 475 (`Pipeline::new` for daily-summary) — session_id が `cron:` prefix なので flush は呼ばれないため不要
- `rustyclaw-cli/src/main.rs` — CLI は gateway の queue 表示を持たないため不要

---

## Task 1: FlushCallback 型と Pipeline struct フィールド追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

コールバック型エイリアスを定義し、`Pipeline` struct に 3 フィールドを追加して `Pipeline::new()` でそれらを `None` で初期化する。

現在の `Pipeline` struct（`lib.rs` 約 370–389 行）:
```rust
pub struct Pipeline {
    config: Config,
    provider: Box<dyn LlmProvider>,
    flush_sem: Arc<Semaphore>,
    rate_limiter: Arc<RateLimiter>,
}
```

現在の `Pipeline::new()`:
```rust
pub fn new(config: Config, flush_sem: Arc<Semaphore>) -> Self {
    let provider = create_provider(config.get_model("default"));
    Self {
        config,
        provider,
        flush_sem,
        rate_limiter: Arc::new(RateLimiter::new()),
    }
}
```

- [ ] **Step 1: 失敗テストを書く**

`lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
#[test]
fn test_flush_callback_queued_called_synchronously() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    use rustyclaw_config::{AgentsConfig, Config, ModelEntry, ModelNames};

    fn make_flush_config() -> Config {
        Config {
            model_list: vec![ModelEntry {
                model_name: "test-model".to_string(),
                provider: "openai".to_string(),
                model: "test-model-api".to_string(),
                api_base: "http://localhost".to_string(),
                api_key: "key".to_string(),
                max_tokens: Some(2048),
                temperature: Some(0.7),
                enabled: true,
                rpm: None,
                rpd: None,
                tpm: None,
                tpd: None,
                context_window: Some("32k".to_string()),
                cf_aig_gateway_id: None,
            }],
            agents: AgentsConfig {
                default: ModelNames::Single("test-model".to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    let received = Arc::new(std::sync::Mutex::new(String::new()));
    let received_clone = received.clone();

    let flush_sem = Arc::new(tokio::sync::Semaphore::new(1));
    let pipeline = Pipeline::new(make_flush_config(), flush_sem).with_flush_callbacks(
        Arc::new(move |id: &str| {
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            *received_clone.lock().unwrap() = id.to_string();
        }),
        Arc::new(|_: &str| {}),
        Arc::new(|_: &str| {}),
    );

    pipeline.trigger_memory_flush_async(std::path::Path::new("/tmp"), "test-session");

    assert!(called.load(Ordering::SeqCst), "on_flush_queued は spawn 前に同期呼び出しされる");
    assert_eq!(
        *received.lock().unwrap(),
        "flush:test-session",
        "flush_session_id は flush:<session_id> 形式"
    );
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
TZ=UTC cargo test --package rustyclaw-agent test_flush_callback_queued_called_synchronously 2>&1 | tail -20
```

Expected: `error[E0599]: no method named 'with_flush_callbacks'` などコンパイルエラーで FAIL。

- [ ] **Step 3: 型エイリアスと struct フィールドを追加**

`lib.rs` のファイル先頭付近（`use` 宣言の後）に型エイリアスを追加:
```rust
/// LANE QUEUE 可視化用コールバック型 (flush_session_id を引数に取る)
type FlushCallback = Arc<dyn Fn(&str) + Send + Sync>;
```

`Pipeline` struct を変更:
```rust
pub struct Pipeline {
    config: Config,
    provider: Box<dyn LlmProvider>,
    flush_sem: Arc<Semaphore>,
    rate_limiter: Arc<RateLimiter>,
    on_flush_queued: Option<FlushCallback>,
    on_flush_executing: Option<FlushCallback>,
    on_flush_done: Option<FlushCallback>,
}
```

`Pipeline::new()` を変更（3 フィールドを `None` で初期化）:
```rust
pub fn new(config: Config, flush_sem: Arc<Semaphore>) -> Self {
    let provider = create_provider(config.get_model("default"));
    Self {
        config,
        provider,
        flush_sem,
        rate_limiter: Arc::new(RateLimiter::new()),
        on_flush_queued: None,
        on_flush_executing: None,
        on_flush_done: None,
    }
}
```

`Pipeline::new()` の直後に `with_flush_callbacks()` ビルダーを追加:
```rust
/// LANE QUEUE 可視化のためのコールバックを注入する (gateway からのみ使用)
pub fn with_flush_callbacks(
    mut self,
    on_queued: FlushCallback,
    on_executing: FlushCallback,
    on_done: FlushCallback,
) -> Self {
    self.on_flush_queued = Some(on_queued);
    self.on_flush_executing = Some(on_executing);
    self.on_flush_done = Some(on_done);
    self
}
```

- [ ] **Step 4: テストが通ることを確認（まだ trigger_memory_flush_async は変更前）**

```bash
TZ=UTC cargo test --package rustyclaw-agent test_flush_callback_queued_called_synchronously 2>&1 | tail -20
```

Expected: コンパイルは通るが `assert!(called...)` で FAIL（`trigger_memory_flush_async` がコールバックを呼ばないため）。

---

## Task 2: trigger_memory_flush_async にコールバック呼び出しを追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

現在の `trigger_memory_flush_async()`（行 649–679）をコールバック対応に変更する。

変更後:
```rust
pub fn trigger_memory_flush_async(&self, workspace_dir: &Path, session_id: &str) {
    let workspace_dir = workspace_dir.to_path_buf();
    let session_id = session_id.to_string();
    let flush_session_id = format!("flush:{}", session_id);
    let config = self.config.clone();
    let flush_sem = self.flush_sem.clone();
    let on_flush_queued = self.on_flush_queued.clone();
    let on_flush_executing = self.on_flush_executing.clone();
    let on_flush_done = self.on_flush_done.clone();

    // キュー表示: spawn 直前に "Waiting" として登録（同期・fail-open）
    if let Some(ref cb) = on_flush_queued {
        cb(&flush_session_id);
    }

    tokio::spawn(async move {
        // セマフォ取得（最大 60 秒待機）。取得できなければ今回の flush はスキップ
        let _permit = match tokio::time::timeout(
            Duration::from_secs(60),
            flush_sem.acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                tracing::warn!(session = %session_id, "flush_memory: semaphore closed, skipping");
                if let Some(ref cb) = on_flush_done { cb(&flush_session_id); }
                return;
            }
            Err(_) => {
                tracing::warn!(session = %session_id, "flush_memory: semaphore timeout (60s), skipping");
                if let Some(ref cb) = on_flush_done { cb(&flush_session_id); }
                return;
            }
        };

        if let Some(ref cb) = on_flush_executing { cb(&flush_session_id); }

        if let Err(e) = Self::flush_memory(&workspace_dir, &session_id, config).await {
            tracing::warn!("Failed to flush memory for session {}: {:#}", session_id, e);
        }

        if let Some(ref cb) = on_flush_done { cb(&flush_session_id); }
        // _permit がここでドロップされ、セマフォが返却される
    });
}
```

**重要:** `flush_memory` には `session_id`（元のセッションID）を渡す。`flush_session_id` はキュー表示専用であり、ログや履歴ロードには使わない。

- [ ] **Step 1: 変更を適用する**

上記コードで `trigger_memory_flush_async` を置き換える（行 645–679 を完全に置換）。

- [ ] **Step 2: ビルドとテスト実行**

```bash
TZ=UTC cargo test --package rustyclaw-agent 2>&1 | tail -30
```

Expected: 全テスト PASS。`test_flush_callback_queued_called_synchronously` が GREEN になること。

- [ ] **Step 3: Clippy チェック**

```bash
cargo clippy --package rustyclaw-agent --all-features -- -D warnings 2>&1 | tail -20
```

Expected: エラーなし。

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(Phase 51-2): Pipeline に FlushCallback コールバック注入機構を追加

trigger_memory_flush_async() に on_flush_queued / on_flush_executing /
on_flush_done コールバックを追加。with_flush_callbacks() ビルダーで注入する。
flush_session_id = "flush:<session_id>" 形式で LANE QUEUE 可視化に使用。

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Gateway の Pipeline::new() にコールバックを渡す

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

行 643 の `Pipeline::new()` 呼び出し（Discord/HTTP セッションの通常実行パス）にコールバックを連鎖させる。

現在（行 642–643）:
```rust
let pipeline =
    Pipeline::new(active_config.clone(), lane_sem.clone());
```

変更後:
```rust
let flush_desc = format!("Memory Flush ({})", session_id);
let pipeline = Pipeline::new(active_config.clone(), lane_sem.clone())
    .with_flush_callbacks(
        Arc::new({
            let flush_desc = flush_desc.clone();
            move |flush_sid: &str| {
                crate::queue_update_or_insert(flush_sid, "Waiting", 0.0, &flush_desc);
            }
        }),
        Arc::new(|flush_sid: &str| {
            crate::queue_update_or_insert(flush_sid, "Executing", 0.0, "");
        }),
        Arc::new(|flush_sid: &str| {
            crate::queue_remove(flush_sid);
        }),
    );
```

**注意:** 行 350（heartbeat）・行 475（daily-summary）は変更しない。

- [ ] **Step 1: use 宣言に `Arc` が含まれることを確認**

```bash
grep -n "^use std::sync::Arc\|Arc," /mnt/Projects/RustyClaw/crates/rustyclaw-gateway/src/lib.rs | head -5
```

`Arc` がすでにインポートされていれば追加不要。ない場合は `use std::sync::Arc;` を追加する。

- [ ] **Step 2: 変更を適用する**

行 642–643 を上記コードで置き換える。

- [ ] **Step 3: ビルドチェック**

```bash
cargo build --package rustyclaw-gateway 2>&1 | tail -20
```

Expected: `Compiling rustyclaw-gateway` → `Finished`.

- [ ] **Step 4: 全テスト**

```bash
TZ=UTC cargo test --workspace 2>&1 | tail -20
```

Expected: 全テスト PASS。

- [ ] **Step 5: Clippy**

```bash
cargo clippy --package rustyclaw-gateway --all-features -- -D warnings 2>&1 | tail -20
```

Expected: エラーなし。

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(Phase 51-2): gateway の Pipeline 生成に flush コールバックを注入

Discord/HTTP セッションの Pipeline::new() に with_flush_callbacks() を連鎖させ、
memory flush が LANE QUEUE に Waiting→Executing→（消去）で表示されるようにする。
session_id = "flush:<original_session_id>" 形式。

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: SERVICE_BADGES に MEM-FLUSH と SES-SUM を追加

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs`

現在（行 1079–1089）:
```js
const SERVICE_BADGES = [
  { prefix: 'cron:heartbeat',      label: 'HEARTBEAT', color: '#bf00ff' },
  { prefix: 'cron:topic-patrol',   label: 'PATROL',    color: '#ff8c00' },
  { prefix: 'cron:daily-briefing', label: 'BRIEFING',  color: '#4488ff' },
  { prefix: 'cron:vitals',         label: 'VITALS',    color: '#00ff9f' },
  { prefix: 'cron:karakeep',       label: 'KARAKEEP',  color: '#ffe066' },
  { prefix: 'cron:daily-summary',  label: 'SUMMARY',   color: '#00e5ff' },
  { prefix: 'discord-',            label: 'DISCORD',   color: '#7b68ee' },
  { prefix: 'http-dashboard',      label: 'DASHBOARD', color: '#00d4ff' },
  { prefix: 'cli-',                label: 'CLI',       color: '#cccccc' },
];
```

変更後:
```js
const SERVICE_BADGES = [
  { prefix: 'cron:heartbeat',        label: 'HEARTBEAT', color: '#bf00ff' },
  { prefix: 'cron:topic-patrol',     label: 'PATROL',    color: '#ff8c00' },
  { prefix: 'cron:daily-briefing',   label: 'BRIEFING',  color: '#4488ff' },
  { prefix: 'cron:vitals',           label: 'VITALS',    color: '#00ff9f' },
  { prefix: 'cron:karakeep',         label: 'KARAKEEP',  color: '#ffe066' },
  { prefix: 'cron:daily-summary',    label: 'SUMMARY',   color: '#00e5ff' },
  { prefix: 'cron:session-summary',  label: 'SES-SUM',   color: '#00bfff' },
  { prefix: 'flush:',                label: 'MEM-FLUSH', color: '#ff6b6b' },
  { prefix: 'discord-',              label: 'DISCORD',   color: '#7b68ee' },
  { prefix: 'http-dashboard',        label: 'DASHBOARD', color: '#00d4ff' },
  { prefix: 'cli-',                  label: 'CLI',       color: '#cccccc' },
];
```

**色の選定:**
- `cron:session-summary` → `#00bfff` (deep sky blue: `cron:daily-summary` の `#00e5ff` と区別)
- `flush:` → `#ff6b6b` (salmon red: メモリ書き込みを示す暖色)

- [ ] **Step 1: health.rs の当該箇所を確認**

```bash
grep -n "SERVICE_BADGES\|prefix.*cron\|prefix.*flush" /mnt/Projects/RustyClaw/crates/rustyclaw-gateway/src/health.rs | head -20
```

行番号を確認する。

- [ ] **Step 2: 変更を適用する**

上記の変更後コードで `SERVICE_BADGES` 定義を置き換える。

- [ ] **Step 3: ビルドチェック**

```bash
cargo build --package rustyclaw-gateway 2>&1 | tail -10
```

Expected: `Finished` でエラーなし（JavaScript はコンパイル時チェックなし）。

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "$(cat <<'EOF'
feat(Phase 51-2): LANE QUEUE に MEM-FLUSH と SES-SUM バッジを追加

SERVICE_BADGES に flush: (MEM-FLUSH) と cron:session-summary (SES-SUM) を追加。
memory flush と session summary の LLM 呼び出しが UNKNOWN でなく名前付き
バッジで表示されるようになる。

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: 動作確認とドキュメント更新

**Files:**
- Modify: `docs/task.md`

- [ ] **Step 1: ローカルビルド最終確認**

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | tail -20
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -10
cargo fmt --all -- --check 2>&1
```

Expected: 全テスト PASS、Clippy エラーなし、フォーマット差分なし。

- [ ] **Step 2: docs/task.md 更新**

`docs/task.md` の「Context Window 最適化」セクション直上に Phase 51-2 を追記する:

```markdown
- [x] **Phase 51-2: LANE QUEUE Memory Flush 可視化**（完了 2026-06-13）  
  memory flush と Session Summary の LLM 実行を LANE QUEUE に表示。  
  コールバック注入（Option A）: Pipeline.with_flush_callbacks() + SERVICE_BADGES 追加。
```

- [ ] **Step 3: フォーマット適用・コミット**

```bash
cargo fmt --all
git add docs/task.md
git commit -m "$(cat <<'EOF'
docs(Phase 51-2): task.md に Phase 51-2 完了を記録

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**1. 仕様カバレッジ:**
- ✅ memory flush が LANE QUEUE に Waiting→Executing→消去 で表示される
- ✅ `cron:session-summary:*` が UNKNOWN でなく SES-SUM バッジになる
- ✅ `flush:<session_id>` 形式の session_id を使用
- ✅ 循環依存なし（agent → gateway の依存追加なし）
- ✅ fail-open: コールバック未設定（None）でも既存動作は変わらない
- ✅ `flush_memory()` には元の session_id を渡し続ける（ログ・履歴ロードに影響なし）

**2. プレースホルダスキャン:** なし

**3. 型整合性:**
- `FlushCallback = Arc<dyn Fn(&str) + Send + Sync>` — task 1〜3 で一貫して使用
- `with_flush_callbacks()` 引数と struct フィールドの型が一致
- `flush_session_id: String`、コールバック引数 `&str` — `cb(&flush_session_id)` で OK
