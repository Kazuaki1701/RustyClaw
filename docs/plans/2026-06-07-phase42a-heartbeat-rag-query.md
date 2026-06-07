# Phase 42-A Heartbeat RAG クエリ最適化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Heartbeat patrol の RAG 検索クエリを `heartbeat_prompt` 全体から「digest 末尾 10 行 + 固定テンプレート」に置き換え、検索精度を向上させる。

**Architecture:** `crates/rustyclaw-agent/src/lib.rs` に `build_heartbeat_rag_query(digest: &str) -> String` を追加し（Task 1）、`execute_heartbeat` に `rag_query: Option<&str>` パラメータを追加して RAG 呼び出しを切り替える（Task 2）。最後に `crates/rustyclaw-gateway/src/lib.rs` の呼び出し側で `digest` から最適化クエリを生成して渡す（Task 3）。

**Tech Stack:** Rust 2024 Edition、`rustyclaw-agent`、`rustyclaw-gateway`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | `HEARTBEAT_RAG_TAIL_LINES` 定数・`build_heartbeat_rag_query` 関数追加、`execute_heartbeat` シグネチャ変更 |
| `crates/rustyclaw-gateway/src/lib.rs` | 修正 | `build_heartbeat_rag_query` 呼び出し・`execute_heartbeat` 引数追加 |

---

## Task 1: `build_heartbeat_rag_query` ヘルパー関数の追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-agent/src/lib.rs` のテストモジュール末尾（`discord_rag_tests` モジュールの直後）に追加する:

```rust
#[cfg(test)]
mod heartbeat_rag_tests {
    use super::*;

    #[test]
    fn test_build_heartbeat_rag_query_empty() {
        let result = build_heartbeat_rag_query("");
        assert_eq!(result, "recent errors tasks memory updates: ");
    }

    #[test]
    fn test_build_heartbeat_rag_query_fewer_than_10_lines() {
        let digest = "line1\nline2\nline3";
        let result = build_heartbeat_rag_query(digest);
        assert_eq!(
            result,
            "recent errors tasks memory updates: line1\nline2\nline3"
        );
    }

    #[test]
    fn test_build_heartbeat_rag_query_exactly_10_lines() {
        let digest = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = build_heartbeat_rag_query(&digest);
        assert!(result.starts_with("recent errors tasks memory updates: "));
        assert!(result.contains("line1"));
        assert!(result.contains("line10"));
    }

    #[test]
    fn test_build_heartbeat_rag_query_truncates_old_lines() {
        // 11行: 最古の line1 は除外され line2〜line11 のみ残る
        let digest = (1..=11)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = build_heartbeat_rag_query(&digest);
        assert!(result.starts_with("recent errors tasks memory updates: "));
        assert!(!result.contains("line1"), "line1 should be truncated");
        assert!(result.contains("line2"));
        assert!(result.contains("line11"));
    }

    #[test]
    fn test_build_heartbeat_rag_query_prefix() {
        let result = build_heartbeat_rag_query("some content");
        assert!(result.starts_with("recent errors tasks memory updates: "));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-agent -- heartbeat_rag_tests 2>&1 | tail -10
```

期待: コンパイルエラー（`build_heartbeat_rag_query` 未定義）

- [ ] **Step 3: 定数とヘルパー関数を実装する**

`crates/rustyclaw-agent/src/lib.rs` の `build_discord_rag_query` 関数（48行目付近）の直後に追加する:

```rust
const HEARTBEAT_RAG_TAIL_LINES: usize = 10;

pub fn build_heartbeat_rag_query(digest: &str) -> String {
    let lines: Vec<&str> = digest.lines().collect();
    let start = lines.len().saturating_sub(HEARTBEAT_RAG_TAIL_LINES);
    let tail = lines[start..].join("\n");
    format!("recent errors tasks memory updates: {}", tail)
}
```

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-agent -- heartbeat_rag_tests 2>&1 | tail -10
```

期待:
```
test heartbeat_rag_tests::test_build_heartbeat_rag_query_empty ... ok
test heartbeat_rag_tests::test_build_heartbeat_rag_query_exactly_10_lines ... ok
test heartbeat_rag_tests::test_build_heartbeat_rag_query_fewer_than_10_lines ... ok
test heartbeat_rag_tests::test_build_heartbeat_rag_query_prefix ... ok
test heartbeat_rag_tests::test_build_heartbeat_rag_query_truncates_old_lines ... ok
```

- [ ] **Step 5: Clippy を通す**

```bash
cargo clippy -p rustyclaw-agent --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 42-A build_heartbeat_rag_query ヘルパー関数を追加"
```

---

## Task 2: `execute_heartbeat` シグネチャ変更

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `execute_heartbeat` シグネチャに `rag_query: Option<&str>` を追加する**

`crates/rustyclaw-agent/src/lib.rs` の `execute_heartbeat` 関数シグネチャ（724行目付近）を以下に置き換える:

```rust
    pub async fn execute_heartbeat(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
        rag_query: Option<&str>,
        tool_registry: &ToolRegistry,
        db_path: &Path,
    ) -> Result<LlmResponse> {
        let mut system_context = self.build_heartbeat_context(workspace_dir)?;

        // RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
        // heartbeat_top_k が設定されている場合は config を clone して top_k を上書き (ISSUE-30)
        let hb_top_k = self
            .config
            .embedding
            .as_ref()
            .and_then(|e| e.heartbeat_top_k)
            .unwrap_or(2);
        let heartbeat_config = {
            let mut cfg = self.config.clone();
            if let Some(ref mut emb) = cfg.embedding {
                emb.top_k = hb_top_k;
            }
            cfg
        };
        // Phase 42-A: rag_query が指定された場合はそちらを使用、未指定時は user_message にフォールバック
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

（`user_message` は RAG クエリとしては使わなくなるが、LLM へ送るメッセージとしては引き続き使用するため残す。）

- [ ] **Step 2: コンパイルエラーを確認する**

この時点では gateway 側の呼び出しが古いシグネチャのままなのでコンパイルエラーが出るはず:

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error" | head -10
```

期待: `rustyclaw-agent` 自体はビルド通る（`rustyclaw-gateway` はまだ触れていないのでここでは確認不要）

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -10
```

期待: `execute_heartbeat` の引数が足りないというエラーが出る（Task 3 で修正する）

- [ ] **Step 3: Clippy を rustyclaw-agent で通す**

```bash
cargo clippy -p rustyclaw-agent --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 42-A execute_heartbeat に rag_query パラメータを追加"
```

---

## Task 3: `rustyclaw-gateway` の呼び出し側を更新

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: `use` 宣言に `build_heartbeat_rag_query` を追加する**

`crates/rustyclaw-gateway/src/lib.rs` の先頭付近（2行目付近）の import を更新する:

変更前:
```rust
use rustyclaw_agent::{Pipeline, UnifiedRagEngine};
```

変更後:
```rust
use rustyclaw_agent::{build_heartbeat_rag_query, Pipeline, UnifiedRagEngine};
```

- [ ] **Step 2: `execute_heartbeat` 呼び出しを更新する**

2箇所を個別に変更する。

**変更 A** — `heartbeat_prompt` 生成直後（`let heartbeat_prompt = ...` の次行）に1行追加:

変更前:
```rust
                            let heartbeat_prompt = prompt_parts.join("\n\n");

                            let mut attempt = 0;
```

変更後:
```rust
                            let heartbeat_prompt = prompt_parts.join("\n\n");
                            let heartbeat_rag_query = build_heartbeat_rag_query(&digest);

                            let mut attempt = 0;
```

**変更 B** — `execute_heartbeat` の引数リスト（`&heartbeat_prompt,` の直後）に1行追加:

変更前:
```rust
                                        match pipeline
                                            .execute_heartbeat(
                                                &workspace_path,
                                                &session_id,
                                                &heartbeat_prompt,
                                                &tool_registry,
                                                &db_path,
                                            )
                                            .await
```

変更後:
```rust
                                        match pipeline
                                            .execute_heartbeat(
                                                &workspace_path,
                                                &session_id,
                                                &heartbeat_prompt,
                                                Some(&heartbeat_rag_query),
                                                &tool_registry,
                                                &db_path,
                                            )
                                            .await
```

- [ ] **Step 3: ビルドが通ることを確認する**

```bash
cargo build --all 2>&1 | grep "^error" | head -20
```

期待: 出力なし

- [ ] **Step 4: 全テストを実行する**

```bash
cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```

期待: 全行が `test result: ok.`

- [ ] **Step 5: Clippy を全クレートで通す**

```bash
cargo clippy --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(gateway): Phase 42-A execute_heartbeat に heartbeat_rag_query を渡す"
```

---

## Task 4: task.md の更新と push

- [ ] **Step 1: `docs/task.md` の Phase 42-A を完了済みにする**

`docs/task.md` の Phase 42-A 行（クエリ最適化）を `[x]` に更新する。

- [ ] **Step 2: コミット＆push**

```bash
git add docs/task.md
git commit -m "chore(task): Phase 42-A 完了マーク"
git push origin main
```
