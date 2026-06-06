# Phase 28b-4: Heartbeat コンテキストオーバーフロー対策 Implementation Plan

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-06-06  
> **備考**: 最新の動作仕様については、`docs/specs/04_heartbeat_spec.md` を参照してください。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `execute_heartbeat` のツールループでメッセージが無制限に蓄積する問題を解消し、Groq TPM 6,000 tokens/リクエスト上限内に収まるよう保証する。

**Architecture:** 2 段階で対処する。①各ツール結果を 3,000 bytes でキャップし単発の大きなレスポンス（Gmail 大量取得等）を抑制、②ループ先頭で古いツールペアを除去し直近 1 世代のみ保持することで複数ループにまたがる累積を防ぐ。どちらも fail-safe（ツール結果の截断・古いペアの削除は情報損失を伴うが LLM 失敗よりはマシ）。

**Tech Stack:** Rust 2024 edition, `crates/rustyclaw-agent/src/lib.rs` のみ変更。外部クレート追加なし。

---

## 背景と数値

```
Groq llama-3.1-8b-instant の上限: 6,000 tokens / リクエスト

現在の heartbeat リクエスト内訳（最悪ケース）:
  system (SOUL + MEMORY + HEARTBEAT):  ~3,550 tokens  ← 固定
  user message (digest):               ~  400 tokens  ← 固定
  assistant(tool_calls):               ~  150 tokens
  tool_result × N 件 (Gmail/Calendar): 無制限         ← 問題箇所
  ─────────────────────────────────────────────────
  2 ツール結果 × 2,000 chars 制限後:   ~  500 tokens
  合計 (上限適用後):                   ~4,600 tokens  ← 6k 以内 ✓
```

---

## ファイルマップ

| ファイル | 変更内容 |
|---------|---------|
| `crates/rustyclaw-agent/src/lib.rs` | `trim_heartbeat_messages` 関数を追加（行 ~1915 付近）、`execute_heartbeat` ループを修正（行 670〜731） |

---

## Task 1: `trim_heartbeat_messages` のテストを書く（TDD）

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`（テストブロック末尾に追加）

- [x] **Step 1-1: 失敗するテストを書く**

`crates/rustyclaw-agent/src/lib.rs` のテストブロック末尾（`}` の直前）に追加：

```rust
    // ── Phase 28b-4: trim_heartbeat_messages ──
    #[test]
    fn test_trim_heartbeat_messages_empty() {
        let mut msgs: Vec<Message> = vec![];
        trim_heartbeat_messages(&mut msgs);
        assert_eq!(msgs.len(), 0);
    }

    #[test]
    fn test_trim_heartbeat_messages_only_system_user() {
        let mut msgs = vec![
            Message { role: "system".to_string(), content: "sys".to_string(), ..Default::default() },
            Message { role: "user".to_string(),   content: "usr".to_string(), ..Default::default() },
        ];
        trim_heartbeat_messages(&mut msgs);
        assert_eq!(msgs.len(), 2, "system+user のみなら変化しない");
    }

    #[test]
    fn test_trim_heartbeat_messages_one_generation() {
        // [system, user, assistant(gen1), tool_result_1a, tool_result_1b]
        // trim 後も変化なし（1世代は保持）
        let mut msgs = vec![
            Message { role: "system".to_string(),    content: "sys".to_string(), ..Default::default() },
            Message { role: "user".to_string(),      content: "usr".to_string(), ..Default::default() },
            Message { role: "assistant".to_string(), content: "".to_string(),
                tool_calls: Some(vec![]), ..Default::default() },
            Message { role: "tool".to_string(), content: "result_a".to_string(), ..Default::default() },
            Message { role: "tool".to_string(), content: "result_b".to_string(), ..Default::default() },
        ];
        trim_heartbeat_messages(&mut msgs);
        assert_eq!(msgs.len(), 5, "1世代しかなければ削除しない");
    }

    #[test]
    fn test_trim_heartbeat_messages_two_generations() {
        // [system, user, assistant(gen1), tool_1, assistant(gen2), tool_2]
        // trim 後: [system, user, assistant(gen2), tool_2]
        let mut msgs = vec![
            Message { role: "system".to_string(),    content: "sys".to_string(), ..Default::default() },
            Message { role: "user".to_string(),      content: "usr".to_string(), ..Default::default() },
            Message { role: "assistant".to_string(), content: "gen1".to_string(), ..Default::default() },
            Message { role: "tool".to_string(),      content: "old_result".to_string(), ..Default::default() },
            Message { role: "assistant".to_string(), content: "gen2".to_string(), ..Default::default() },
            Message { role: "tool".to_string(),      content: "new_result".to_string(), ..Default::default() },
        ];
        trim_heartbeat_messages(&mut msgs);
        assert_eq!(msgs.len(), 4, "古い世代が削除され [system, user, gen2_assistant, gen2_tool] になる");
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[2].content, "gen2");
        assert_eq!(msgs[3].content, "new_result");
    }

    #[test]
    fn test_trim_heartbeat_messages_three_generations() {
        // 3世代ある場合も最新1世代のみ保持
        let mut msgs = vec![
            Message { role: "system".to_string(),    content: "sys".to_string(), ..Default::default() },
            Message { role: "user".to_string(),      content: "usr".to_string(), ..Default::default() },
            Message { role: "assistant".to_string(), content: "gen1".to_string(), ..Default::default() },
            Message { role: "tool".to_string(),      content: "r1".to_string(), ..Default::default() },
            Message { role: "assistant".to_string(), content: "gen2".to_string(), ..Default::default() },
            Message { role: "tool".to_string(),      content: "r2".to_string(), ..Default::default() },
            Message { role: "assistant".to_string(), content: "gen3".to_string(), ..Default::default() },
            Message { role: "tool".to_string(),      content: "r3".to_string(), ..Default::default() },
        ];
        trim_heartbeat_messages(&mut msgs);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[2].content, "gen3");
        assert_eq!(msgs[3].content, "r3");
    }
```

- [x] **Step 1-2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-agent test_trim_heartbeat_messages 2>&1 | grep -E "error|FAILED|cannot find"
```

期待出力: `error[E0425]: cannot find function` または同等のコンパイルエラー。

---

## Task 2: `trim_heartbeat_messages` を実装する

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`（`truncate_70_20` の直前、行 ~1900 付近に追加）

- [x] **Step 2-1: 関数を実装する**

`crates/rustyclaw-agent/src/lib.rs` の `truncate_70_20` 関数（行 ~1900）の直前に挿入：

```rust
/// Heartbeat メッセージ配列を [system, user, 直近1世代] に切り詰める。
/// messages[0]=system, messages[1]=user を保持しつつ、
/// それ以降の assistant+tool ペアは最後の1世代（最後の assistant メッセージ以降）のみ残す。
/// 世代が1つ以下の場合は何もしない。
fn trim_heartbeat_messages(messages: &mut Vec<Message>) {
    if messages.len() <= 2 {
        return;
    }
    // 末尾から assistant ロールのメッセージを探す
    let last_assistant_idx = messages
        .iter()
        .rposition(|m| m.role == "assistant");
    if let Some(idx) = last_assistant_idx {
        // idx >= 2 かつ前にもっと古い世代がある場合のみ削除
        if idx >= 2 && messages[..idx].iter().any(|m| m.role == "assistant") {
            let tail: Vec<Message> = messages.drain(idx..).collect();
            messages.truncate(2); // system + user のみ残す
            messages.extend(tail);
        }
    }
}
```

- [x] **Step 2-2: テストが通ることを確認**

```bash
cargo test -p rustyclaw-agent test_trim_heartbeat_messages 2>&1 | grep -E "test.*ok|FAILED|error"
```

期待出力: 5件すべて `test ... ok`。

- [x] **Step 2-3: Clippy を通す**

```bash
cargo clippy -p rustyclaw-agent 2>&1 | grep -E "error|warning.*trim_heartbeat"
```

期待出力: 警告・エラーなし。

- [x] **Step 2-4: コミット**

```bash
git checkout -b feat/phase28b4-heartbeat-overflow
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 28b-4 add trim_heartbeat_messages for loop context rotation"
```

---

## Task 3: ツール結果截断のテストを書く（TDD）

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`（テストブロック末尾）

- [x] **Step 3-1: 截断ヘルパーのテストを書く**

既存の `truncate_70_20` は正しく機能するが、Heartbeat 用の定数と適用を検証するテストを追加：

```rust
    // ── Phase 28b-4: ツール結果截断 ──
    #[test]
    fn test_truncate_70_20_within_limit() {
        let short = "abc".repeat(100); // 300 bytes < 3000
        let result = truncate_70_20(&short, 3_000);
        assert_eq!(result, short, "上限以内なら変化しない");
    }

    #[test]
    fn test_truncate_70_20_over_limit() {
        let long = "x".repeat(6_000); // 6000 bytes > 3000
        let result = truncate_70_20(&long, 3_000);
        assert!(result.len() < 6_000, "截断後は元より短い");
        assert!(result.contains("[..."), "省略マーカーが含まれる");
        // 先頭70% = 2100 bytes、末尾20% = 600 bytes が保持される
        assert!(result.starts_with(&"x".repeat(2_100)));
    }

    #[test]
    fn test_truncate_70_20_preserves_head_and_tail() {
        // head: "BEGIN...END" のような構造で先頭と末尾が保持されることを確認
        let mut content = "BEGIN_".to_string();
        content.push_str(&"m".repeat(5_000));
        content.push_str("_END");
        let result = truncate_70_20(&content, 3_000);
        assert!(result.starts_with("BEGIN_"), "先頭が保持される");
        assert!(result.ends_with("_END"), "末尾が保持される");
        assert!(result.contains("[..."), "省略マーカーが含まれる");
    }
```

- [x] **Step 3-2: テストが通ることを確認（`truncate_70_20` は既存実装）**

```bash
cargo test -p rustyclaw-agent test_truncate_70_20 2>&1 | grep -E "test.*ok|FAILED|error"
```

期待出力: 3件すべて `test ... ok`。

---

## Task 4: `execute_heartbeat` ループに截断とローテーションを組み込む

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:669〜731`（`execute_heartbeat` のループ部分）

- [x] **Step 4-1: ループを修正する**

現在のループ（行 669〜731）を以下に置き換える：

```rust
        let max_loops = 5;
        for loop_idx in 0..max_loops {
            // 2回目以降: 古いツールペアを捨てて直近1世代のみ保持
            if loop_idx > 0 {
                trim_heartbeat_messages(&mut messages);
            }

            self.dump_request(workspace_dir, &messages);

            let mut response = self.complete_with_fallback("heartbeat", session_id, &messages, &provider_tools, Duration::from_secs(900)).await?;
            response.content = filter_json_leaks(&response.content);

            let assistant_msg = Message {
                role: response.role.clone(),
                content: response.content.clone(),
                tool_calls: response.tool_calls.clone(),
                name: None,
                ..Default::default()
            };
            messages.push(assistant_msg.clone());
            logger.append_message(session_id, &assistant_msg)
                .context("Failed to save assistant response in session log (fail-closed)")?;
            self.dump_response(workspace_dir, &response.content, &response.role);

            if let Some(ref calls) = response.tool_calls {
                if !calls.is_empty() {
                    for call in calls {
                        tracing::info!("Agent executing tool call: {} (id: {})", call.function.name, call.id);
                        let (mut tool_content, tool_is_error) = if let Some(tool) = tool_registry.get(&call.function.name) {
                            match tool.call(call.function.arguments.clone()).await {
                                Ok(content) => (content, false),
                                Err(e) => (format!("Tool error: {}", e), true),
                            }
                        } else {
                            (format!("Error: Tool '{}' not found in registry", call.function.name), true)
                        };

                        // Filter already-seen Gmail/Calendar items (fail-open)
                        if !tool_is_error {
                            if let Some(ref db) = db_opt {
                                tool_content = filter_seen_tool_result(
                                    &call.function.name,
                                    &call.function.arguments,
                                    &tool_content,
                                    db,
                                );
                            }
                        }

                        // ツール結果を 3,000 bytes にキャップ（Groq TPM 上限対策）
                        tool_content = truncate_70_20(&tool_content, 3_000);

                        let tool_msg = Message {
                            role: "tool".to_string(),
                            content: tool_content,
                            tool_call_id: Some(call.id.clone()),
                            name: Some(call.function.name.clone()),
                            ..Default::default()
                        };
                        messages.push(tool_msg.clone());
                        logger.append_message(session_id, &tool_msg)
                            .context("Failed to save tool result message in session log (fail-closed)")?;
                    }
                    continue;
                }
            }

            return Ok(response);
        }

        Err(anyhow::anyhow!("Heartbeat agent loop exceeded maximum step limit of {}", max_loops))
```

**差分のポイント:**
1. `for _` → `for loop_idx` に変更
2. ループ先頭に `if loop_idx > 0 { trim_heartbeat_messages(&mut messages); }` を追加
3. `filter_seen_tool_result` の後に `tool_content = truncate_70_20(&tool_content, 3_000);` を追加

- [x] **Step 4-2: ビルドが通ることを確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep -E "error|warning"
```

期待出力: error なし（unused variable 等の警告があれば修正）。

- [x] **Step 4-3: 既存のテストが全件パスすることを確認**

```bash
cargo test -p rustyclaw-agent 2>&1 | tail -10
```

期待出力: `test result: ok. XX passed; 0 failed`。

- [x] **Step 4-4: Clippy を通す**

```bash
cargo clippy -p rustyclaw-agent 2>&1 | grep "error"
```

期待出力: error なし。

- [x] **Step 4-5: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 28b-4 cap tool results and rotate heartbeat message generations"
```

---

## Task 5: 仕様書更新・マージ・タスク完了

**Files:**
- Modify: `docs/specs/04_heartbeat_spec.md`（最終更新日 + 記述追加）
- Modify: `docs/task.md`（Phase 28b-4 を完了マーク）

- [x] **Step 5-1: `docs/specs/04_heartbeat_spec.md` の最終更新日を更新し、コンテキスト制御の記述を追記**

`docs/specs/04_heartbeat_spec.md` のヘッダー `**最終更新日**` を `2026-06-06` に更新し、Heartbeat ループのコンテキスト制御について記述されたセクションがあればその内容も同期する（なければ追記不要）。

- [x] **Step 5-2: `docs/task.md` の Phase 28b-4 を `[x]` に更新し、archive に記録**

`docs/task.md` の以下の行を変更：

```
- `[x]` **Phase 28b-4: Heartbeat コンテキストオーバーフロー対策** ⏰ 着手可能
```
→
```
- `[x]` **Phase 28b-4: Heartbeat コンテキストオーバーフロー対策** ✅ 完了（2026-06-06）
```

- [x] **Step 5-3: コミット**

```bash
git add docs/specs/04_heartbeat_spec.md docs/task.md
git commit -m "docs: Phase 28b-4 仕様書更新・タスク完了マーク"
```

- [x] **Step 5-4: master へマージ**

```bash
git checkout master
git merge --no-ff feat/phase28b4-heartbeat-overflow -m "Merge branch 'feat/phase28b4-heartbeat-overflow' into master"
git branch -d feat/phase28b4-heartbeat-overflow
```

---

## 完了条件チェック

- [x] `cargo test -p rustyclaw-agent` が全件パス
- [x] `cargo clippy --all-targets` に error なし
- [x] `trim_heartbeat_messages` の5テストが全件パス
- [x] `truncate_70_20` の3テストが全件パス
- [x] `execute_heartbeat` のループに `trim_heartbeat_messages` と `truncate_70_20` が適用されている
- [x] `docs/task.md` の Phase 28b-4 が `[x]` になっている
