# Phase 44-3 システムプロンプトの固定化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `build_system_context()` から `[now: timestamp]` を除去し、呼び出し元（`execute()` / `execute_with_rig_agent()`）で追加する構造に変更することで、静的プレフィックス（SOUL + USER）を確定的にして Groq の Implicit Prefix Caching を最大限活用できるようにする。

**Architecture:** `build_heartbeat_context()` はすでに `[now:]` を返さず、呼び出し元 `execute_heartbeat()` が追加するパターンを確立している。本フェーズでは `build_system_context()` を同じパターンに統一する。変更は 1 ファイル（`rustyclaw-agent/src/lib.rs`）の 3 か所（関数本体・`execute`・`execute_with_rig_agent`）と、テスト修正 1 件、ドキュメント更新 2 件。

**Tech Stack:** Rust 2024 Edition、`crates/rustyclaw-agent`、`chrono::Local`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | `build_system_context()` から `[now:]` を除去、`execute()` と `execute_with_rig_agent()` に `[now:]` 追加 |
| `crates/rustyclaw-agent/src/lib.rs` | テスト修正 | `test_build_system_context_injects_runtime_context` → `[now:]` 不在を検証するテストに変更 |
| `docs/specs/02_agent_pipeline.md` | 更新 | `build_system_context()` の説明を「[now:] は呼び出し元で付与」に修正 |
| `docs/task.md` | 更新 | Phase 44-3 を `[x]` にクローズ |

---

### Task 1: `build_system_context()` からの `[now:]` 除去

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:3026-3046, 396-438`

- [ ] **Step 1: ベースラインを確認**

```bash
TZ=UTC cargo test -p rustyclaw-agent 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`

- [ ] **Step 2: テストを先に修正して失敗させる（TDD）**

`crates/rustyclaw-agent/src/lib.rs` の以下を:

```rust
    #[test]
    fn test_build_system_context_injects_runtime_context() {
        let ws_dir = tempdir().unwrap();
        std::fs::write(ws_dir.path().join("SOUL.md"), "soul").unwrap();

        let config = make_test_config_with_url("http://localhost");
        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        let context = pipeline.build_system_context(ws_dir.path()).unwrap();

        // 静的ファイルが先頭、[now:] は末尾（プロンプトキャッシュ最適化）
        assert!(context.contains("# SOUL.md"));
        // フォーマット: [now: YYYY-MM-DDTHH:MM:SS+HH:MM]
        let last_line = context.trim_end().lines().last().unwrap();
        assert!(
            last_line.starts_with("[now: "),
            "datetime line must be last for cache optimization"
        );
        assert!(last_line.ends_with(']'));
        assert!(last_line.contains('T'), "must be ISO 8601 format");
    }
```

以下に変更する（テスト名変更・`[now:]` 不在を検証）:

```rust
    #[test]
    fn test_build_system_context_returns_static_content() {
        let ws_dir = tempdir().unwrap();
        std::fs::write(ws_dir.path().join("SOUL.md"), "soul").unwrap();

        let config = make_test_config_with_url("http://localhost");
        let flush_sem = Arc::new(Semaphore::new(1));
        let pipeline = Pipeline::new(config, flush_sem);
        let context = pipeline.build_system_context(ws_dir.path()).unwrap();

        assert!(context.contains("# SOUL.md"));
        assert!(
            !context.contains("[now: "),
            "build_system_context must not include [now:] — dynamic content belongs in callers"
        );
    }
```

- [ ] **Step 3: テストが失敗することを確認**

```bash
TZ=UTC cargo test -p rustyclaw-agent test_build_system_context_returns_static_content 2>&1 | grep -E "FAILED|thread.*panicked"
```
Expected: テストが FAILED（`[now:]` がまだ存在するため assertion 失敗）

- [ ] **Step 4: `build_system_context()` から `[now:]` を除去**

`crates/rustyclaw-agent/src/lib.rs` の `build_system_context()` 内のコメントとコードを変更する。

コメントを更新（L397-398 付近）:

```rust
    pub fn build_system_context(&self, workspace_dir: &Path) -> Result<String> {
        // 静的ブロック（SOUL/USER）を先に並べてプロンプトキャッシュの prefix を安定させる。
        // 動的な [now:] は末尾に置くことで毎回変わる部分がキャッシュ prefix を破壊しないようにする。
```

以下に変更する:

```rust
    pub fn build_system_context(&self, workspace_dir: &Path) -> Result<String> {
        // 静的ブロック（SOUL/USER）のみを返す。
        // 動的な [now:] は呼び出し元で追加する（build_heartbeat_context と同パターン）。
```

続けて、末尾の `[now:]` 注入ブロックを削除する。以下を:

```rust
        // 動的ブロック（現在時刻）は末尾に配置
        let now = chrono::Local::now();
        context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));

        Ok(context)
    }
```

以下に変更する:

```rust
        Ok(context)
    }
```

- [ ] **Step 5: テストが通ることを確認**

```bash
TZ=UTC cargo test -p rustyclaw-agent test_build_system_context_returns_static_content 2>&1 | grep -E "^(test result|FAILED|ok)"
```
Expected: `test result: ok. 1 passed; 0 failed;`

- [ ] **Step 6: `rustyclaw-agent` 全テストで確認**

```bash
TZ=UTC cargo test -p rustyclaw-agent 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`（件数変化は -1 または 0。テスト名が変わるだけ）

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "refactor(agent): Phase 44-3 [now:] を build_system_context から除去"
```

---

### Task 2: `execute()` と `execute_with_rig_agent()` への `[now:]` 追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:1081-1085, 1383-1387`

Task 1 完了後に実施する。

- [ ] **Step 1: `execute()` に `[now:]` を追加（L1081-1085 付近）**

`crates/rustyclaw-agent/src/lib.rs` の `execute()` 内の以下を:

```rust
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
        {
            system_context.push_str(&continuation);
        }

        // 1. 過去履歴のロードとトークン圧縮処理の適用
```

以下に変更する:

```rust
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
        {
            system_context.push_str(&continuation);
        }
        let now = chrono::Local::now();
        system_context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));

        // 1. 過去履歴のロードとトークン圧縮処理の適用
```

- [ ] **Step 2: `execute_with_rig_agent()` に `[now:]` を追加（L1383-1387 付近）**

`execute_with_rig_agent()` 内の以下を:

```rust
        // システムプロンプトと RAG コンテキストの構築
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
        {
            system_context.push_str(&continuation);
        }
        // セッション履歴のロード（RAG クエリ構築にも使用するため先行ロード）
```

以下に変更する:

```rust
        // システムプロンプトと RAG コンテキストの構築
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
        {
            system_context.push_str(&continuation);
        }
        let now = chrono::Local::now();
        system_context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));
        // セッション履歴のロード（RAG クエリ構築にも使用するため先行ロード）
```

- [ ] **Step 3: ビルドとテストで確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error"
```
Expected: 出力なし

```bash
TZ=UTC cargo test -p rustyclaw-agent 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`

- [ ] **Step 4: `[now:]` の残存位置確認（`build_system_context` 内に戻り込んでいないこと）**

```bash
grep -n "\[now:" /mnt/Projects/RustyClaw/crates/rustyclaw-agent/src/lib.rs
```

Expected: 以下の 3 か所のみ（`build_system_context` の行は含まれない）:
- `execute_heartbeat()` 内（既存）
- `execute()` 内（今回追加）
- `execute_with_rig_agent()` 内（今回追加）

- [ ] **Step 5: 全体ビルド・テスト**

```bash
cargo build --all 2>&1 | grep "^error"
```
Expected: 出力なし

```bash
TZ=UTC cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: すべての crate で `test result: ok. N passed; 0 failed;`

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "refactor(agent): Phase 44-3 execute/execute_with_rig_agent に [now:] 注入を移動"
```

---

### Task 3: ドキュメント更新・最終検証

**Files:**
- Modify: `docs/specs/02_agent_pipeline.md`
- Modify: `docs/task.md`

- [ ] **Step 1: `02_agent_pipeline.md` の `build_system_context` 説明を更新**

`docs/specs/02_agent_pipeline.md` で `build_system_context` に関する記述を確認する:

```bash
grep -n "build_system_context\|now:\|prefix" docs/specs/02_agent_pipeline.md | head -20
```

見つかった記述のうち「`[now:]` を末尾に付与する」「`[now:]` を含む」という旨の説明があれば、「`[now:]` は呼び出し元（`execute` / `execute_with_rig_agent`）で付与する」に変更する。

変更前の表現例（実際の行を確認してから変更）:
```markdown
`[now: YYYY-MM-DDTHH:MM:SS+HH:MM]` を末尾に付与して返す
```

変更後の表現例:
```markdown
静的コンテンツ（SOUL.md / USER.md / proactive-posts）のみを返す。`[now:]` は呼び出し元で付与する
```

- [ ] **Step 2: `docs/task.md` の Phase 44-3 をクローズ**

`docs/task.md` の以下を:

```markdown
- `[ ]` **Phase 44-3. システムプロンプトの固定化** 💡 プロバイダの Prefix Caching 活用
```

以下に変更する:

```markdown
- `[x]` **Phase 44-3. システムプロンプトの固定化** 💡 プロバイダの Prefix Caching 活用
```

- [ ] **Step 3: 最終ビルド・テスト・clippy**

```bash
cargo build --all 2>&1 | grep "^error"
```
Expected: 出力なし

```bash
TZ=UTC cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: すべての crate で `test result: ok. N passed; 0 failed;`

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error"
```
Expected: 出力なし

- [ ] **Step 4: コミット**

```bash
git add docs/specs/02_agent_pipeline.md docs/task.md
git commit -m "docs(specs): Phase 44-3 build_system_context の [now:] 説明を更新・タスクをクローズ"
```
