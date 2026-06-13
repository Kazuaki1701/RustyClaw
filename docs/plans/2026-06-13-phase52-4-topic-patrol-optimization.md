# Phase 52-4: Topic Patrol 最適化 実装計画書

> [!NOTE]
> **ステータス**: `[ACTIVE]` (実装準備中)
> **最終更新日**: 2026-06-13
> **対象コード**: `crates/rustyclaw-agent/src/lib.rs`, `crates/rustyclaw-gateway/src/lib.rs`
> **設計仕様書**: [`docs/specs/2026-06-13-phase52-context-optimization-design.md`](../specs/2026-06-13-phase52-context-optimization-design.md)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Topic Patrol 実行時のコンテキストを極小化し、SOUL.md/USER.md 全文の除去・固定 RSS フィードの `ctx_fetch_and_index` 事前取得・Interests のみの動的注入を実装する。

**Architecture:** Agent 側に `build_patrol_context()` を追加し、`execute_with_rig_agent` が `purpose == "patrol"` のとき SOUL.md/USER.md を読まない最小プロンプトを生成する。Gateway 側ではチャットハンドラを拡張し、patrol セッションに限り USER.md Interests を直接読んで extra_system_context に渡し、Interests の `sources:` 行から HTTP(S) URL を抽出して `ctx_fetch_and_index` で事前キャッシュする。

**Tech Stack:** Rust 2024 Edition, `rustyclaw-agent`, `rustyclaw-gateway`, context-mode MCP (ctx_fetch_and_index / ctx_search)

---

## 開発タスクチェックリスト

- [ ] **Task 1: run_purpose バグ修正 + build_patrol_context（Agent）**
- [ ] **Task 2: Gateway patrol 対応強化（Interests 注入 + ctx_fetch_and_index 事前フェッチ）**
- [ ] **Task 3: テスト・Clippy・コミット・ドキュメント更新**

---

## 前提知識: 現状と問題点

### Topic Patrol のセッション ID と purpose マッピング

Topic Patrol の実際のセッション ID は以下の 2 種類:
- `"cron:topic-patrol-explore"` — 探索モード
- `"cron:topic-patrol-deliver"` — 配信モード

**既存バグ（gateway/lib.rs ~line 758）:**

```rust
// ❌ 現状: 完全一致のため patrol セッションは常に "discord" になってしまう
let run_purpose = if session_id == "cron:topic-patrol" {
    "patrol"
} else {
    "discord"  // ← patrol が常にここに来てしまっている
};
```

Task 1 でこれを `session_id.contains("topic-patrol")` に修正する。

### execute_with_rig_agent のコンテキスト構築（agent/lib.rs ~line 1437）

```rust
let cw = self.config.get_model(purpose).context_window_tokens;
let mut system_context = self.build_system_context(workspace_dir, cw)?;
// ↑ 常に SOUL.md + USER.md を読んでいる（patrol でも同じ）
if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id) {
    system_context.push_str(&continuation);
}
let now = chrono::Local::now();
system_context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));
if let Some(extra) = extra_system_context {
    system_context.push_str(&extra);
}
```

Task 1 では、`purpose == "patrol"` のとき `build_patrol_context()` を使い SOUL.md/USER.md を読まない。

### Gateway チャットハンドラの patrol 分岐（gateway/lib.rs ~line 758-784）

```rust
// 現状: patrol は user_interests_extra = None（cron: プレフィックスで除外される）
let user_interests_extra: Option<String> = if !session_id.starts_with("cron:") {
    try_ctx_search(&tool_server_handle, &query)
        .await
        .filter(|r| r.contains("[user-interests]"))
        .map(|r| format!("\n\n# Relevant User Interests\n{}", r))
} else {
    None  // ← patrol もここに入る。USER.md Interests が注入されない
};
```

Task 2 でこれを patrol 専用ブランチに分割し、USER.md Interests と事前フェッチを追加する。

### 既存の関連関数

| 関数 | ファイル | 概要 |
|------|---------|------|
| `extract_interests_section(content)` | `gateway/lib.rs` | USER.md の Interests セクション抽出（Phase 52-3 追加） |
| `try_ctx_index(handle, content, source)` | `gateway/lib.rs` | ctx_index 呼び出しラッパー |
| `try_ctx_search(handle, query)` | `gateway/lib.rs` | ctx_search 呼び出しラッパー |
| `build_system_context(workspace_dir, cw)` | `agent/lib.rs:502` | SOUL.md+USER.md ロード |
| `execute_with_rig_agent(...)` | `agent/lib.rs:1421` | チャット/patrol 統合実行（extra_system_context 対応済み） |

---

## Task 1: run_purpose バグ修正 + build_patrol_context（Agent）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (~line 758)
- Modify: `crates/rustyclaw-agent/src/lib.rs` (~line 1430, ~line 1440)

### Step 1: run_purpose バグを修正する

- [ ] `gateway/lib.rs ~line 758` の `session_id == "cron:topic-patrol"` を以下に変更する

```rust
// Before
let run_purpose = if session_id == "cron:topic-patrol" {
    "patrol"
} else {
    "discord"
};

// After: topic-patrol-explore / topic-patrol-deliver の両方に対応
let run_purpose = if session_id.contains("topic-patrol") {
    "patrol"
} else {
    "discord"
};
```

### Step 2: build_patrol_context 関数を agent/lib.rs に追加する

- [ ] `build_system_context` 関数（~line 502）の近くに以下を追加する

```rust
/// Topic Patrol 専用の極小システムコンテキストを構築する。
/// SOUL.md や USER.md 全文は読まない。USER.md Interests は gateway から extra_system_context で渡される。
fn build_patrol_context() -> String {
    "You are a topic patrol agent. Find and summarize interesting news based on the user's interests provided below.\n".to_string()
}
```

### Step 3: execute_with_rig_agent で patrol のとき build_patrol_context を使うよう変更する

- [ ] `agent/lib.rs ~line 1437` の `build_system_context` 呼び出しを以下に変更する

```rust
// Before
let mut system_context = self.build_system_context(workspace_dir, cw)?;

// After: patrol は極小コンテキスト、それ以外は従来通り
let mut system_context = if purpose == "patrol" {
    build_patrol_context()
} else {
    self.build_system_context(workspace_dir, cw)?
};
```

### Step 4: テストを agent/lib.rs の既存テストモジュールに追加する

- [ ] 以下のテストをテストモジュールに追加する

```rust
#[test]
fn test_build_patrol_context_excludes_soul_and_user_md() {
    let ctx = build_patrol_context();
    assert!(
        !ctx.to_lowercase().contains("soul"),
        "SOUL.md は patrol コンテキストに含まれないこと"
    );
    assert!(
        !ctx.to_lowercase().contains("# user"),
        "USER.md 全文は patrol コンテキストに含まれないこと"
    );
    assert!(
        ctx.contains("patrol") || ctx.contains("agent"),
        "patrol の目的が含まれること: {:?}",
        ctx
    );
}
```

### Step 5: ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-agent -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし（エラーなし）

---

## Task 2: Gateway patrol 対応強化（Interests 注入 + ctx_fetch_and_index 事前フェッチ）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

### Step 1: try_ctx_fetch_and_index ヘルパーを追加する

- [ ] `try_ctx_search` 関数（~line 1037）の近くに以下を追加する

```rust
/// ctx_fetch_and_index を呼び出す。context-mode が未接続の場合は None を返す（fail-open）。
/// 指定 URL の HTML コンテンツを Markdown に変換し、SQLite FTS5 にインデックス登録する。
async fn try_ctx_fetch_and_index(
    handle: &rig_core::tool::server::ToolServerHandle,
    url: &str,
) -> Option<String> {
    let args = serde_json::json!({ "url": url }).to_string();
    match handle.call_tool("ctx_fetch_and_index", &args).await {
        Ok(result) if !result.trim().is_empty() => Some(result),
        Ok(_) => None,
        Err(e) => {
            tracing::debug!("ctx_fetch_and_index 呼び出し失敗 (url={}): {}", url, e);
            None
        }
    }
}
```

### Step 2: extract_patrol_feed_urls 関数を追加する

- [ ] `extract_interests_section` 関数（~line 1136）の近くに以下を追加する

```rust
/// USER.md Interests の sources: 行から HTTP/HTTPS URL のみを抽出する。
/// HN, Reddit, github: ショートカットは除外する（動的検索は事前フェッチ不可のため）。
fn extract_patrol_feed_urls(interests: &str) -> Vec<String> {
    interests
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let value = if let Some(v) = trimmed.strip_prefix("sources: ") {
                v
            } else if let Some(v) = trimmed.strip_prefix("sources:") {
                v.trim()
            } else {
                return None;
            };
            if value.starts_with("http://") || value.starts_with("https://") {
                Some(value.to_string())
            } else {
                None
            }
        })
        .collect()
}
```

### Step 3: チャットハンドラの user_interests_extra を patrol 対応に拡張する

- [ ] `gateway/lib.rs ~line 758-784` の `user_interests_extra` 計算ブロックを以下に置き換える

現在のコード（変更前）:
```rust
// USER.md Interests を ctx_search で動的取得（cron 以外のみ）
let user_interests_extra: Option<String> = if !session_id.starts_with("cron:") {
    let query = format!("{} user interests hobbies", content);
    try_ctx_search(&tool_server_handle, &query)
        .await
        .filter(|r| r.contains("[user-interests]"))
        .map(|r| format!("\n\n# Relevant User Interests\n{}", r))
} else {
    None
};
```

変更後:
```rust
// Interests 注入: patrol は USER.md 直読み + 固定フィード事前フェッチ、
//                 通常 chat は ctx_search で動的取得、その他 cron は None
let user_interests_extra: Option<String> = if session_id.contains("topic-patrol") {
    // USER.md から Interests を直接読み込み
    let interests_opt = std::fs::read_to_string(workspace_path.join("USER.md"))
        .ok()
        .map(|c| extract_interests_section(&c))
        .filter(|s| !s.is_empty());

    // 固定 RSS/Web フィードを ctx_fetch_and_index で事前インデックス登録
    if let Some(ref interests_text) = interests_opt {
        for url in extract_patrol_feed_urls(interests_text) {
            tracing::info!("Topic Patrol: ctx_fetch_and_index: {}", url);
            try_ctx_fetch_and_index(&tool_server_handle, &url).await;
        }
    }

    interests_opt.map(|i| format!("\n\n# User Interests\n{}", i))
} else if !session_id.starts_with("cron:") {
    // 通常 chat: ctx_search で動的取得
    let query = format!("{} user interests hobbies", content);
    try_ctx_search(&tool_server_handle, &query)
        .await
        .filter(|r| r.contains("[user-interests]"))
        .map(|r| format!("\n\n# Relevant User Interests\n{}", r))
} else {
    None
};
```

### Step 4: テストを gateway/lib.rs の既存テストモジュールに追加する

- [ ] 以下のテストを gateway/lib.rs のテストモジュールに追加する

```rust
#[test]
fn test_extract_patrol_feed_urls_http_only() {
    let interests = "- AI Agent\n  sources: HN\n- Cloudflare\n  sources: https://blog.cloudflare.com\n- GitHub Rust\n  sources: github:rust-lang/rust\n- RSS feed\n  sources: https://example.com/feed.xml\n- Reddit\n  sources: Reddit/r/rust";
    let urls = extract_patrol_feed_urls(interests);
    assert_eq!(urls.len(), 2, "HTTP(S) URL が 2 件抽出されること: {:?}", urls);
    assert!(urls.contains(&"https://blog.cloudflare.com".to_string()));
    assert!(urls.contains(&"https://example.com/feed.xml".to_string()));
}

#[test]
fn test_extract_patrol_feed_urls_empty_when_no_http() {
    let interests = "- AI Agent\n  sources: HN\n- GitHub\n  sources: github:rust-lang/rust";
    let urls = extract_patrol_feed_urls(interests);
    assert!(urls.is_empty(), "HTTP URL がない場合は空であること");
}
```

### Step 5: ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 3: テスト・Clippy・コミット・ドキュメント更新

**Files:**
- Modify: `docs/plans/2026-06-13-phase52-4-topic-patrol-optimization.md`
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
git checkout -b feat/phase52-4
git add crates/rustyclaw-agent/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(patrol): Phase 52-4 Topic Patrol 極小コンテキスト・run_purpose バグ修正・ctx_fetch_and_index 事前フェッチ"
```

### Step 4: 本計画書のチェックリストを更新する

- [ ] 本計画書のすべての `- [ ]` を `- [x]` に更新する

### Step 5: 実装計画書を更新する

`docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md` の Phase 52-4 チェックリストをすべて `[x]` に更新する。

### Step 6: task.md を更新する

`docs/task.md` の Phase 52-4 に完了日を追記する。

### Step 7: ドキュメントコミットして main にマージする

```bash
git add docs/
git commit -m "docs(phase52): Phase 52-4 完了チェックリスト更新"
git checkout main
git merge --no-ff feat/phase52-4 -m "feat(phase52-4): Topic Patrol 専用コンテキスト最適化（ctx_fetch_and_index 事前フェッチ）"
git branch -d feat/phase52-4
```
