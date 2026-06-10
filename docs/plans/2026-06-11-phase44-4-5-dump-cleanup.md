# Phase 44-4/44-5 ダンプロジック完全整理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** エージェント層に残る NOP の `dump_request`/`dump_response` 関数とその5つの呼び出し箇所を除去し（Phase 44-4）、`dump_llm_io` でディレクトリ作成失敗時に warn + スキップして `last_request.json` は書き続けるよう修正する（Phase 44-5）。

**Architecture:** Phase 28B と Phase 44-2 でプロバイダ層の `dump_llm_io` は完全実装済み。エージェント層の `dump_request`/`dump_response` はすでに NOP（空実装）になっているため、関数定義と呼び出し箇所を丸ごと除去するだけでよい。Phase 44-5 は `dump_llm_io` 内の `create_dir_all` 失敗パスを「error + return」→「warn + 続行（dated ファイルはスキップ、last_request.json は書く）」に変更する小修正。

**Tech Stack:** Rust 2024 Edition、`crates/rustyclaw-agent`、`crates/rustyclaw-providers`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 削除 | `dump_request`/`dump_response` 関数定義 + 5 つの呼び出し箇所 |
| `crates/rustyclaw-providers/src/lib.rs` | 修正 | `create_dir_all` 失敗時: error+return → warn+continue（dated ファイルスキップ）|
| `crates/rustyclaw-providers/src/lib.rs` | テスト追加 | dated dir 作成失敗時も `last_request.json` が書かれることを検証 |
| `docs/task.md` | 更新 | Phase 44-4 と 44-5 を `[x]` にクローズ |

---

### Task 1: エージェント層の NOP ダンプ関数を除去

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: ベースラインを確認**

```bash
TZ=UTC cargo test -p rustyclaw-agent 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`

- [ ] **Step 2: `dump_request`/`dump_response` 関数定義を削除**

`crates/rustyclaw-agent/src/lib.rs` の以下のブロックを丸ごと削除する:

```rust
    fn dump_request(&self, _workspace_dir: &Path, _messages: &[Message]) {
        // Consolidated in providers layer
    }

    /// デバッグ用の生レスポンスダンプを出力する
    fn dump_response(&self, _workspace_dir: &Path, _content: &str, _role: &str) {
        // Consolidated in providers layer
    }
```

（前の `fn` の末尾 `}` から次の `fn get_session_continuation_context` の `///` 直前まで）

- [ ] **Step 3: `execute_heartbeat()` の呼び出しを削除（2 箇所）**

`execute_heartbeat()` 内の以下 2 行を削除する:

1つ目（rig agent 呼び出し前）:
```rust
        self.dump_request(workspace_dir, &initial_messages);
```

2つ目（rig agent 呼び出し後）:
```rust
        self.dump_response(workspace_dir, &response_text, "assistant");
```

- [ ] **Step 4: `execute()` の呼び出しを削除（2 箇所）**

`execute()` 内の以下 2 行を削除する:

1つ目（LLM 呼び出し前）:
```rust
        self.dump_request(workspace_dir, &messages);
```

2つ目（LLM 呼び出し後）:
```rust
        self.dump_response(workspace_dir, &response.content, &response.role);
```

- [ ] **Step 5: `execute_stream()` の呼び出しを削除（1 箇所）**

`execute_stream()` 内の以下 1 行を削除する:

```rust
        self.dump_request(workspace_dir, &messages);
```

- [ ] **Step 6: コンパイルとテストで確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error"
```
Expected: 出力なし

```bash
TZ=UTC cargo test -p rustyclaw-agent 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`

呼び出しが全て除去されたことを確認:
```bash
grep -n "dump_request\|dump_response" /mnt/Projects/RustyClaw/crates/rustyclaw-agent/src/lib.rs
```
Expected: 出力なし

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "refactor(agent): Phase 44-4 NOP dump_request/dump_response を除去"
```

---

### Task 2: `dump_llm_io` のエラーハンドリング修正（Phase 44-5）

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`

- [ ] **Step 1: 失敗時に `last_request.json` が書かれることを検証するテストを追加（TDD）**

`crates/rustyclaw-providers/src/lib.rs` のテストモジュールに以下を追加する（`test_dump_llm_io_writes_last_request_json` の直後）:

```rust
    #[test]
    fn test_dump_llm_io_writes_last_request_even_when_dated_dir_fails() {
        let _lock = workspace_dir_lock().lock().unwrap();
        let tmp = tempdir().unwrap();
        let workspace = tmp.path();

        // category と同名のファイルを置いて create_dir_all を意図的に失敗させる
        let llm_dir = workspace.join("memory").join("debug").join("llm");
        std::fs::create_dir_all(&llm_dir).unwrap();
        std::fs::write(llm_dir.join("forcefail-cat"), b"I am a file").unwrap();

        unsafe {
            std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", workspace.to_str().unwrap());
        }

        let messages = vec![Message {
            role: "user".to_string(),
            content: "hello".to_string(),
            ..Default::default()
        }];
        let response = LlmResponse {
            content: "world".to_string(),
            role: "assistant".to_string(),
            ..Default::default()
        };

        // category="forcefail-cat" は既にファイルなので create_dir_all が失敗する
        dump_llm_io("forcefail-cat", "test-model", &messages, &response);

        // last_request.json は失敗時でも書かれるべき
        let last_req = llm_dir.join("last_request.json");
        assert!(
            last_req.exists(),
            "last_request.json は dated dir 作成失敗時でも書かれるべき"
        );

        unsafe {
            std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
        }
    }
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
TZ=UTC cargo test -p rustyclaw-providers test_dump_llm_io_writes_last_request_even_when_dated_dir_fails 2>&1 | grep -E "FAILED|panicked"
```
Expected: テストが FAILED（現在は `return` で early exit するため `last_request.json` が書かれない）

- [ ] **Step 3: `dump_llm_io` の失敗パスを修正**

`crates/rustyclaw-providers/src/lib.rs` の `dump_llm_io` 内の以下を:

```rust
    if let Err(e) = std::fs::create_dir_all(&date_dir) {
        tracing::error!("Failed to create llm dump directory {:?}: {}", date_dir, e);
        return;
    }

    // Delete date folders older than 5 days
```

以下に変更する:

```rust
    let dated_ok = if let Err(e) = std::fs::create_dir_all(&date_dir) {
        tracing::warn!("Failed to create llm dump directory {:?}: {}", date_dir, e);
        false
    } else {
        true
    };

    // Delete date folders older than 5 days
```

続けて、dated file の書き込みブロックを `dated_ok` で条件付きにする。以下の `file_path` から `match std::fs::File::create` ブロック（現在無条件で実行されている部分）を:

```rust
    let file_path = date_dir.join(format!("{}.json", time_str));

    #[derive(serde::Serialize)]
    struct LlmIoDump<'a> {
        timestamp: i64,
        model: &'a str,
        request: &'a [Message],
        response: &'a LlmResponse,
    }

    let dump = LlmIoDump {
        timestamp: now.timestamp(),
        model,
        request: messages,
        response,
    };

    match std::fs::File::create(&file_path) {
        Ok(file) => {
            if let Err(e) = serde_json::to_writer_pretty(file, &dump) {
                tracing::error!("Failed to write llm io dump to {:?}: {}", file_path, e);
            }
        }
        Err(e) => tracing::error!("Failed to create llm io dump file {:?}: {}", file_path, e),
    }
```

以下に変更する:

```rust
    if dated_ok {
        let file_path = date_dir.join(format!("{}.json", time_str));

        #[derive(serde::Serialize)]
        struct LlmIoDump<'a> {
            timestamp: i64,
            model: &'a str,
            request: &'a [Message],
            response: &'a LlmResponse,
        }

        let dump = LlmIoDump {
            timestamp: now.timestamp(),
            model,
            request: messages,
            response,
        };

        match std::fs::File::create(&file_path) {
            Ok(file) => {
                if let Err(e) = serde_json::to_writer_pretty(file, &dump) {
                    tracing::error!("Failed to write llm io dump to {:?}: {}", file_path, e);
                }
            }
            Err(e) => tracing::error!("Failed to create llm io dump file {:?}: {}", file_path, e),
        }
    }
```

- [ ] **Step 4: テストが通ることを確認**

```bash
TZ=UTC cargo test -p rustyclaw-providers test_dump_llm_io_writes_last_request_even_when_dated_dir_fails 2>&1 | grep -E "^(test result|FAILED|ok)"
```
Expected: `test result: ok. 1 passed; 0 failed;`

- [ ] **Step 5: rustyclaw-providers 全テストで確認**

```bash
TZ=UTC cargo test -p rustyclaw-providers 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`（N は 1 増加）

- [ ] **Step 6: 全体ビルド・テスト・clippy**

```bash
cargo build --all 2>&1 | grep "^error"
TZ=UTC cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error"
```
Expected: すべて出力なし / 全テスト ok

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "fix(providers): Phase 44-5 dump_llm_io の dir 作成失敗時を warn+continue に修正"
```

---

### Task 3: ドキュメント更新・Phase 44-4/44-5 クローズ

**Files:**
- Modify: `docs/task.md`

- [ ] **Step 1: Phase 44-4 と 44-5 を `[x]` にクローズ**

`docs/task.md` の以下を:

```markdown
- `[ ]` **Phase 44-4. ダンプロジックのプロバイダ層へ集約** 🔧 44-5 の前提作業
```

以下に変更する:

```markdown
- `[x]` **Phase 44-4. ダンプロジックのプロバイダ層へ集約** 🔧 44-5 の前提作業
```

さらに以下を:

```markdown
- `[ ]` **Phase 44-5. エラーハンドリングとディレクトリ作成** 🛡️ 44-4 完了後に実施
```

以下に変更する:

```markdown
- `[x]` **Phase 44-5. エラーハンドリングとディレクトリ作成** 🛡️ 44-4 完了後に実施
```

- [ ] **Step 2: 最終確認**

```bash
grep "44-4\|44-5" /mnt/Projects/RustyClaw/docs/task.md
```
Expected: 両方 `[x]`

```bash
TZ=UTC cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: 全 crate で `test result: ok. N passed; 0 failed;`

- [ ] **Step 3: コミット**

```bash
git add docs/task.md
git commit -m "docs: Phase 44-4/44-5 タスクをクローズ"
```
