# Phase 42-B Heartbeat ステップ別 RAG Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Heartbeat の RAG 注入を Step 2（記憶・興味整理）と Step 6（自発作業）専用クエリで拡張し、各ステップが必要とする知識を的確に検索注入する。

**Architecture:** `crates/rustyclaw-agent/src/lib.rs` に `HEARTBEAT_STEP2_RAG_QUERY` / `HEARTBEAT_STEP6_RAG_QUERY` 定数を追加し（Task 1）、`execute_heartbeat` の RAG ブロックを1クエリから3クエリに拡張する（Task 2）。ゲートウェイ側の変更はなし。

**Tech Stack:** Rust 2024 Edition、`rustyclaw-agent`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | 定数2件追加、`execute_heartbeat` RAG ブロック拡張、テスト追加 |

---

## Task 1: クエリ定数の追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-agent/src/lib.rs` のテストモジュール末尾（`heartbeat_rag_tests` モジュールの直後、`discord_rag_tests` の前）に追加する:

```rust
#[cfg(test)]
mod heartbeat_step_rag_tests {
    use super::*;

    #[test]
    fn test_heartbeat_step2_rag_query_value() {
        assert_eq!(
            HEARTBEAT_STEP2_RAG_QUERY,
            "user interests hobbies routine habits long-term memory"
        );
    }

    #[test]
    fn test_heartbeat_step6_rag_query_value() {
        assert_eq!(
            HEARTBEAT_STEP6_RAG_QUERY,
            "errors bugs pending tasks todo improvements"
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-agent -- heartbeat_step_rag_tests 2>&1 | tail -10
```

期待: コンパイルエラー（定数未定義）

- [ ] **Step 3: 定数を実装する**

`crates/rustyclaw-agent/src/lib.rs` の `HEARTBEAT_RAG_TAIL_LINES` 定数（50行目付近）の直後に追加する:

```rust
const HEARTBEAT_STEP2_RAG_QUERY: &str =
    "user interests hobbies routine habits long-term memory";
const HEARTBEAT_STEP6_RAG_QUERY: &str =
    "errors bugs pending tasks todo improvements";
```

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-agent -- heartbeat_step_rag_tests 2>&1 | tail -10
```

期待:
```
test heartbeat_step_rag_tests::test_heartbeat_step2_rag_query_value ... ok
test heartbeat_step_rag_tests::test_heartbeat_step6_rag_query_value ... ok
```

- [ ] **Step 5: Clippy を通す**

```bash
cargo clippy -p rustyclaw-agent --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 42-B HEARTBEAT_STEP2/6_RAG_QUERY 定数を追加"
```

---

## Task 2: `execute_heartbeat` の RAG ブロック拡張

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

現在の RAG ブロック（759〜779行目付近）を以下に置き換える。ローカル embed クライアントは1回生成して3クエリに再利用し、Step 2・Step 6 のコンテキストはラベル付きで追記する。

- [ ] **Step 1: RAG ブロックを置き換える**

現在のコード（`let effective_rag = ...` から `}` までの RAG ブロック全体）:

```rust
        let effective_rag = rag_query.unwrap_or(user_message);
        if heartbeat_config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&heartbeat_config) {
                let rag_ctx =
                    retrieve_rag_context_local(effective_rag, &heartbeat_config, &client, db_path, hb_top_k)
                        .await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(effective_rag, &heartbeat_config, rag, hb_top_k).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
        }
```

以下に置き換える:

```rust
        let effective_rag = rag_query.unwrap_or(user_message);
        if heartbeat_config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&heartbeat_config) {
                let rag_ctx =
                    retrieve_rag_context_local(effective_rag, &heartbeat_config, &client, db_path, hb_top_k)
                        .await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
                let step2_ctx = retrieve_rag_context_local(
                    HEARTBEAT_STEP2_RAG_QUERY,
                    &heartbeat_config,
                    &client,
                    db_path,
                    hb_top_k,
                )
                .await;
                if !step2_ctx.is_empty() {
                    system_context.push_str("## Step 2 関連記憶\n");
                    system_context.push_str(&step2_ctx);
                }
                let step6_ctx = retrieve_rag_context_local(
                    HEARTBEAT_STEP6_RAG_QUERY,
                    &heartbeat_config,
                    &client,
                    db_path,
                    hb_top_k,
                )
                .await;
                if !step6_ctx.is_empty() {
                    system_context.push_str("## Step 6 関連記憶\n");
                    system_context.push_str(&step6_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(effective_rag, &heartbeat_config, rag, hb_top_k).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
            let step2_ctx =
                retrieve_rag_context(HEARTBEAT_STEP2_RAG_QUERY, &heartbeat_config, rag, hb_top_k)
                    .await;
            if !step2_ctx.is_empty() {
                system_context.push_str("## Step 2 関連記憶\n");
                system_context.push_str(&step2_ctx);
            }
            let step6_ctx =
                retrieve_rag_context(HEARTBEAT_STEP6_RAG_QUERY, &heartbeat_config, rag, hb_top_k)
                    .await;
            if !step6_ctx.is_empty() {
                system_context.push_str("## Step 6 関連記憶\n");
                system_context.push_str(&step6_ctx);
            }
        }
```

- [ ] **Step 2: ビルドが通ることを確認する**

```bash
cargo build --all 2>&1 | grep "^error" | head -20
```

期待: 出力なし

- [ ] **Step 3: 全テストを実行する**

```bash
cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```

期待: 全行が `test result: ok.`

- [ ] **Step 4: Clippy を全クレートで通す**

```bash
cargo clippy --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 42-B execute_heartbeat に Step 2/6 専用 RAG 注入を追加"
```

---

## Task 3: task.md の更新と push

- [ ] **Step 1: `docs/task.md` の Phase 42-B を完了済みにする**

`docs/task.md` の `42-B` 行を `[x]` に更新する。

- [ ] **Step 2: コミット＆push**

```bash
git add docs/task.md
git commit -m "chore(task): Phase 42-B 完了マーク"
git push origin main
```
