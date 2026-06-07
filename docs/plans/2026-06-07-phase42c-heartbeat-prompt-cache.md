# Phase 42-C Heartbeat プロンプトキャッシュ最適化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `build_heartbeat_context` から `[now: timestamp]` を取り除き、`execute_heartbeat` の RAG 注入直前に移動することで、関数を純粋な静的コンテンツ返却に整理する。

**Architecture:** `crates/rustyclaw-agent/src/lib.rs` の1ファイルのみ変更。`build_heartbeat_context`（line 708付近）から `[now:]` 行を削除し、docコメントも更新。`execute_heartbeat`（line 746付近）に `[now:]` を追加。テストモジュール `mod tests`（line 2668〜4039）末尾に `test_build_heartbeat_context_is_static` を追加。ゲートウェイ側・他クレートの変更なし。

**Tech Stack:** Rust 2024 Edition、`rustyclaw-agent`、`chrono`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | `build_heartbeat_context` から `[now:]` 削除、docコメント更新、`execute_heartbeat` に `[now:]` 追加、テスト追加 |

---

## Task 1: `[now:]` の移動とテスト追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-agent/src/lib.rs` の `mod tests` ブロック末尾（line 4038の `}` の直前、`test_build_heartbeat_context_does_not_include_memory_md` の直後）に追加する:

```rust
    #[test]
    fn test_build_heartbeat_context_is_static() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        std::fs::write(ws.join("SOUL.md"), "# Soul").unwrap();
        std::fs::write(ws.join("HEARTBEAT.md"), "# Heartbeat").unwrap();

        let config = make_test_config_with_url("http://localhost");
        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        let context = pipeline.build_heartbeat_context(ws).unwrap();

        assert!(context.contains("# SOUL.md"));
        assert!(context.contains("# HEARTBEAT.md"));
        assert!(
            !context.contains("[now: "),
            "build_heartbeat_context must not include [now:] — dynamic content belongs in execute_heartbeat"
        );
    }
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-agent -- test_build_heartbeat_context_is_static 2>&1 | tail -10
```

期待: テスト失敗（`[now:]` が含まれているためアサーション失敗）

- [ ] **Step 3: `build_heartbeat_context` から `[now:]` を削除する**

`crates/rustyclaw-agent/src/lib.rs` の `build_heartbeat_context` 関数（line 705-734付近）を以下に置き換える。

変更前:
```rust
    /// Heartbeat 専用の軽量システムコンテキストを構築する（SOUL + HEARTBEAT のみ）。
    /// MEMORY.md は RAG 経由で関連チャンクのみを動的注入する（ISSUE-28）。
    /// 静的ファイルを先頭に、動的な [now:] を末尾に置くことでプロンプトキャッシュ prefix を安定させる。
    pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
        let files = ["SOUL.md", "HEARTBEAT.md"];
        let mut context = String::new();
        for filename in &files {
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
            context.push_str(&format!(
                "# {}\n\n{}\n\n",
                filename,
                Self::strip_comments(&content)
            ));
        }
        // 動的ブロック（現在時刻）は末尾に配置
        let now = chrono::Local::now();
        context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));
        Ok(context)
    }
```

変更後:
```rust
    /// Heartbeat 専用の軽量システムコンテキストを構築する（SOUL + HEARTBEAT のみ）。
    /// MEMORY.md は RAG 経由で関連チャンクのみを動的注入する（ISSUE-28）。
    /// 静的ファイルのみを返す。動的な [now:] は呼び出し元 execute_heartbeat で追加する。
    pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
        let files = ["SOUL.md", "HEARTBEAT.md"];
        let mut context = String::new();
        for filename in &files {
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
            context.push_str(&format!(
                "# {}\n\n{}\n\n",
                filename,
                Self::strip_comments(&content)
            ));
        }
        Ok(context)
    }
```

- [ ] **Step 4: `execute_heartbeat` に `[now:]` を追加する**

`crates/rustyclaw-agent/src/lib.rs` の `execute_heartbeat` 内、`build_heartbeat_context` 呼び出しの直後（line 746付近）を以下に置き換える。

変更前:
```rust
        let mut system_context = self.build_heartbeat_context(workspace_dir)?;

        // RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
```

変更後:
```rust
        let mut system_context = self.build_heartbeat_context(workspace_dir)?;
        let now = chrono::Local::now();
        system_context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));

        // RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
```

- [ ] **Step 5: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-agent -- test_build_heartbeat_context_is_static 2>&1 | tail -10
```

期待:
```
test tests::test_build_heartbeat_context_is_static ... ok
```

- [ ] **Step 6: 全テストを実行する**

```bash
cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```

期待: 全行が `test result: ok.`

- [ ] **Step 7: Clippy を全クレートで通す**

```bash
cargo clippy --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 8: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 42-C build_heartbeat_context を純粋静的に変更・[now:] を execute_heartbeat に移動"
```

---

## Task 2: task.md の更新と push

- [ ] **Step 1: `docs/task.md` の Phase 42-C を完了済みにする**

`docs/task.md` の `42-C` 行を `[x]` に更新する。

- [ ] **Step 2: コミット＆push**

```bash
git add docs/task.md
git commit -m "chore(task): Phase 42-C 完了マーク"
git push origin main
```
