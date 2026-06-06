# ISSUE-33 Discord RAG 改善 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Discord チャットの RAG 検索精度を上げるため、クエリを直近 N ターン会話に拡張し、`discord_top_k` 設定を追加する。

**Architecture:** `EmbeddingConfig` に `discord_top_k: Option<usize>` を追加（Task 1）。`execute_with_rig_agent` 内でヒストリをロードしてロール付きクエリを組み立てるヘルパー関数 `build_discord_rag_query` を追加し（Task 2）、実際の呼び出し箇所に統合する（Task 3）。

**Tech Stack:** Rust 2024 Edition, `rustyclaw-config`, `rustyclaw-agent`, `rustyclaw-providers::Message`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | 修正 | `EmbeddingConfig` に `discord_top_k` 追加、テスト追加 |
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | `build_discord_rag_query` 関数追加、`execute_with_rig_agent` 統合 |

---

## Task 1: `EmbeddingConfig` に `discord_top_k` を追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-config/src/lib.rs` のテストモジュール末尾（`test_embedding_config_dashboard_top_k_value` の直後）に追加する:

```rust
#[test]
fn test_embedding_config_discord_top_k_default() {
    let cfg: EmbeddingConfig = serde_json::from_str(r#"{}"#).unwrap();
    assert!(cfg.discord_top_k.is_none(), "discord_top_k default should be None");
}

#[test]
fn test_embedding_config_discord_top_k_value() {
    let cfg: EmbeddingConfig =
        serde_json::from_str(r#"{"discord_top_k": 3}"#).unwrap();
    assert_eq!(cfg.discord_top_k, Some(3));
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-config -- test_embedding_config_discord_top_k 2>&1 | tail -10
```

期待: コンパイルエラー（`discord_top_k` フィールド未定義）

- [ ] **Step 3: `discord_top_k` フィールドを追加する**

`crates/rustyclaw-config/src/lib.rs` の `dashboard_top_k` フィールド（132行目付近）の直後に追記する:

```rust
    /// ダッシュボードチャット専用の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub dashboard_top_k: Option<usize>,
    /// Discord チャット専用の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub discord_top_k: Option<usize>,
```

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-config -- test_embedding_config_discord_top_k 2>&1 | tail -10
```

期待:
```
test test_embedding_config_discord_top_k_default ... ok
test test_embedding_config_discord_top_k_value ... ok
```

- [ ] **Step 5: Clippy を通す**

```bash
cargo clippy -p rustyclaw-config --all-targets 2>&1 | grep "^error"
```

期待: 出力なし（エラーなし）

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): ISSUE-33 EmbeddingConfig に discord_top_k を追加"
```

---

## Task 2: `build_discord_rag_query` ヘルパー関数の追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-agent/src/lib.rs` のテストモジュール末尾に追加する:

```rust
#[cfg(test)]
mod discord_rag_tests {
    use super::*;
    use rustyclaw_providers::Message;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: content.to_string(),
            ..Default::default()
        }
    }

    fn tool_call_msg() -> Message {
        Message {
            role: "assistant".to_string(),
            content: "".to_string(),
            tool_calls: Some(vec![]),
            ..Default::default()
        }
    }

    #[test]
    fn test_build_discord_rag_query_no_history() {
        // 履歴なし → raw_user_message のみ
        let result = build_discord_rag_query(&[], "Hello");
        assert_eq!(result, "User: Hello");
    }

    #[test]
    fn test_build_discord_rag_query_one_turn() {
        let history = vec![
            msg("user", "What is the weather?"),
            msg("assistant", "It is sunny."),
        ];
        let result = build_discord_rag_query(&history, "And tomorrow?");
        assert_eq!(
            result,
            "User: What is the weather?\nAssistant: It is sunny.\nUser: And tomorrow?"
        );
    }

    #[test]
    fn test_build_discord_rag_query_skips_tool_role() {
        let history = vec![
            msg("user", "Find coords"),
            tool_call_msg(),
            msg("tool", "35.6762, 139.6503"),
            msg("assistant", "Coords are 35.67 N."),
        ];
        let result = build_discord_rag_query(&history, "Thanks");
        // tool ロールと空 content の assistant は除外
        assert_eq!(
            result,
            "User: Find coords\nAssistant: Coords are 35.67 N.\nUser: Thanks"
        );
        assert!(!result.contains("35.6762, 139.6503"));
    }

    #[test]
    fn test_build_discord_rag_query_truncates_to_n_turns() {
        // DISCORD_RAG_HISTORY_TURNS = 2 なので、3ターン以上あっても最新2ターンのみ
        let history = vec![
            msg("user", "Turn 1 user"),
            msg("assistant", "Turn 1 assistant"),
            msg("user", "Turn 2 user"),
            msg("assistant", "Turn 2 assistant"),
            msg("user", "Turn 3 user"),
            msg("assistant", "Turn 3 assistant"),
        ];
        let result = build_discord_rag_query(&history, "Current");
        assert!(!result.contains("Turn 1"), "Turn 1 should be truncated");
        assert!(result.contains("Turn 2 user"));
        assert!(result.contains("Turn 3 assistant"));
        assert!(result.ends_with("User: Current"));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-agent -- discord_rag_tests 2>&1 | tail -15
```

期待: コンパイルエラー（`build_discord_rag_query` 未定義）

- [ ] **Step 3: 定数とヘルパー関数を実装する**

`crates/rustyclaw-agent/src/lib.rs` の `impl AgentPipeline {` ブロックの直前（構造体定義の外）に追加する:

```rust
const DISCORD_RAG_HISTORY_TURNS: usize = 2;

fn build_discord_rag_query(history: &[rustyclaw_providers::Message], raw_user_message: &str) -> String {
    // tool ロールおよび空 content の assistant（tool_call のみ）を除外
    let filtered: Vec<&rustyclaw_providers::Message> = history
        .iter()
        .filter(|m| {
            (m.role == "user" || m.role == "assistant")
                && !m.content.is_empty()
        })
        .collect();

    // 直近 N ターン分（user + assistant を1ターンとして N*2 メッセージ）を取得
    let take = DISCORD_RAG_HISTORY_TURNS * 2;
    let start = filtered.len().saturating_sub(take);
    let recent = &filtered[start..];

    let mut parts: Vec<String> = recent
        .iter()
        .map(|m| {
            let role_label = if m.role == "user" { "User" } else { "Assistant" };
            format!("{}: {}", role_label, m.content.trim())
        })
        .collect();

    parts.push(format!("User: {}", raw_user_message));
    parts.join("\n")
}
```

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-agent -- discord_rag_tests 2>&1 | tail -15
```

期待:
```
test discord_rag_tests::test_build_discord_rag_query_no_history ... ok
test discord_rag_tests::test_build_discord_rag_query_one_turn ... ok
test discord_rag_tests::test_build_discord_rag_query_skips_tool_role ... ok
test discord_rag_tests::test_build_discord_rag_query_truncates_to_n_turns ... ok
```

- [ ] **Step 5: Clippy を通す**

```bash
cargo clippy -p rustyclaw-agent --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): ISSUE-33 build_discord_rag_query ヘルパー関数を追加"
```

---

## Task 3: `execute_with_rig_agent` への統合

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

RAG クエリ構築箇所（1400行付近）を修正する。ヒストリを RAG より先に読み込むよう順序を変え、`build_discord_rag_query` と `discord_top_k` を使うよう切り替える。

- [ ] **Step 1: `execute_with_rig_agent` の RAG ブロックを修正する**

現在の RAG ブロック（`let top_k = ...` から `}` まで）を以下に置き換える:

```rust
        // ヒストリを RAG クエリ構築に先立ってロード（cron: は空）
        let history_for_rag: Vec<rustyclaw_providers::Message> = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger.load_history(session_id).unwrap_or_default()
        };

        // discord_top_k 優先、未設定時はグローバル top_k にフォールバック
        let top_k = self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
        let discord_top_k = self
            .config
            .embedding
            .as_ref()
            .and_then(|e| e.discord_top_k)
            .unwrap_or(top_k);

        // RAG クエリ: 直近 N ターン会話 + 現在メッセージ（cron: は raw_user_message のみ）
        let rag_query = if session_id.starts_with("cron:") {
            raw_user_message.to_string()
        } else {
            build_discord_rag_query(&history_for_rag, raw_user_message)
        };

        if self
            .config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&self.config) {
                let db_path = workspace_dir.join("memory.db");
                let rag_ctx =
                    retrieve_rag_context_local(&rag_query, &self.config, &client, &db_path, discord_top_k).await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(&rag_query, &self.config, rag, discord_top_k).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
        }
```

次に、既存のヒストリロードブロック（`// セッション履歴のロード` コメントから始まる部分）を、`history_for_rag` を再利用するよう変更する:

```rust
        // セッション履歴のロード（RAG 用にロード済みのものを再利用）
        let history_messages = if session_id.starts_with("cron:") {
            Vec::new()
        } else {
            logger
                .load_history(session_id)
                .context("Failed to load session history")?
        };
```

（`history_for_rag` は `unwrap_or_default` でエラーを無視したが、ここでは元通り `?` でエラー伝播させる。二重ロードだが読み取り専用のファイル操作のため問題ない。）

- [ ] **Step 2: ビルドが通ることを確認する**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error" | head -20
```

期待: 出力なし

- [ ] **Step 3: 全テストを実行する**

```bash
cargo test -p rustyclaw-agent 2>&1 | tail -10
```

期待:
```
test result: ok. N passed; 0 failed; ...
```

- [ ] **Step 4: Clippy を全クレートで通す**

```bash
cargo clippy --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 5: 全テストを全クレートで通す**

```bash
cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```

期待: 全行が `test result: ok.`

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): ISSUE-33 execute_with_rig_agent に discord_top_k とクエリ拡張を統合"
```

---

## Task 4: task.md の更新と push

- [ ] **Step 1: `docs/task.md` の ISSUE-33 を完了済みにする**

`docs/task.md` の ISSUE-33 行を更新する:

```markdown
- `[x]` **ISSUE-33: Discord チャット向け RAG 改善 — クエリ拡張 + discord_top_k**
```

- [ ] **Step 2: コミット＆push**

```bash
git add docs/task.md
git commit -m "chore(task): ISSUE-33 完了マーク"
git push origin fix/clippy-warnings
```
