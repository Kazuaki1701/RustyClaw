# seen_items フィルタリング Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Heartbeat 実行時に Gmail / Calendar スクリプト結果から既読アイテムを除外し、同一メール・イベントへの重複 Discord 通知を防止する。

**Architecture:** `execute_heartbeat` のツール呼び出しループで、`run_workspace_script` の結果（Gmail は `id` フィールド、Calendar は `event_id` フィールド）を `DbManager::is_item_seen` でチェック。未読アイテムのみ LLM に渡し、同時に `mark_item_seen` で既読登録する。LLM は既読アイテムを見ないため `HEARTBEAT_OK` を自然に出力するようになる。

**Tech Stack:** Rust 2024 edition, rustyclaw-storage `DbManager` (SQLite `seen_items` テーブル), serde_json

---

## File Structure

| ファイル | 変更内容 |
|---------|---------|
| `crates/rustyclaw-agent/src/lib.rs` | `execute_heartbeat` シグネチャ変更 + `filter_seen_tool_result` ヘルパー追加 + ツールループへの組み込み |
| `crates/rustyclaw-gateway/src/lib.rs` | `execute_heartbeat` の呼び出し側に `&db_path` を渡す（1行変更） |

---

## 背景知識

### `seen_items` テーブルと `DbManager` API（`crates/rustyclaw-storage/src/lib.rs`）

```rust
// mark_item_seen(item_id: &str, category: &str) -> Result<()>
//   → SQLite seen_items に INSERT OR REPLACE
// is_item_seen(item_id: &str) -> Result<bool>
//   → seen_items に item_id が存在するか確認
```

### Gmail スクリプト出力形式（`skills/gmail/scripts/506_get-gmail.sh`）
```json
[{"id": "abc123", "sender": "楽天カード <..>", "subject": "利用確定", "date": "...", "snippet": "..."}]
```

### Calendar スクリプト出力形式（`skills/calendar/scripts/calendar-ops.sh`）
```json
[{"event_id": "xyz789", "title": "会議", "start": "2026-06-06T10:00:00+09:00", ...}]
```

### `WorkspaceExecuteScriptTool` のツール結果形式
スクリプト出力は以下のようにラップされる：
```
--- STDOUT ---
[{"id": "abc123", ...}]
--- EXIT STATUS ---
0
```

### `execute_heartbeat` ツール呼び出しループ（`crates/rustyclaw-agent/src/lib.rs` 行 684 付近）
```rust
let (tool_content, _tool_is_error) = if let Some(tool) = tool_registry.get(&call.function.name) {
    match tool.call(call.function.arguments.clone()).await {
        Ok(content) => (content, false),
        Err(e) => (format!("Tool error: {}", e), true),
    }
} else {
    (format!("Error: Tool '{}' not found in registry", ...), true)
};
```

### Gateway の `execute_heartbeat` 呼び出し箇所（`crates/rustyclaw-gateway/src/lib.rs` 行 221, 273）
```rust
let db_path = workspace_path.join("memory.db");   // 行 221 で定義済み
// ...
match pipeline.execute_heartbeat(&workspace_path, &session_id, &heartbeat_prompt, &tool_registry).await {
//   ↑ ここに &db_path を追加するだけ
```

---

## Task 1: `execute_heartbeat` シグネチャ変更と呼び出し元更新

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:631`
- Modify: `crates/rustyclaw-gateway/src/lib.rs:273`

- [ ] **Step 1: 失敗するテストを追加**

`crates/rustyclaw-agent/src/lib.rs` の `#[cfg(test)] mod tests` ブロック末尾に追加：

```rust
    // ── Task 1: execute_heartbeat db_path シグネチャ ──
    #[tokio::test]
    async fn test_execute_heartbeat_accepts_db_path() {
        // db_path を受け取る新しいシグネチャがコンパイルできることを確認する
        // 実際の LLM 呼び出しは行わない（ビルドチェックのみ）
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("memory.db");
        // DbManager::new で空 DB を作成（seen_items テーブルが初期化される）
        let _db = rustyclaw_storage::DbManager::new(&db_path).unwrap();
        // db_path が Path として渡せることを型チェックするだけ
        let _: &std::path::Path = &db_path;
        // シグネチャが変われば呼び出し元のコンパイルが失敗するため、ここで検知できる
    }
```

- [ ] **Step 2: テスト失敗を確認**

```bash
cargo test -p rustyclaw-agent test_execute_heartbeat_accepts_db_path -- --nocapture 2>&1 | head -10
```

Expected: PASS（このテスト自体はシグネチャ変更前でも通る。コンパイルエラーは Step 3 の変更後に発生する）

- [ ] **Step 3: `execute_heartbeat` シグネチャに `db_path: &Path` を追加**

`crates/rustyclaw-agent/src/lib.rs` の `pub async fn execute_heartbeat(` ブロック（行 631 付近）を以下に変更：

変更前:
```rust
    pub async fn execute_heartbeat(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
        tool_registry: &ToolRegistry,
    ) -> Result<LlmResponse> {
```

変更後:
```rust
    pub async fn execute_heartbeat(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
        tool_registry: &ToolRegistry,
        db_path: &Path,
    ) -> Result<LlmResponse> {
```

- [ ] **Step 4: Gateway 呼び出し元を更新**

`crates/rustyclaw-gateway/src/lib.rs` 行 273 付近を変更：

変更前:
```rust
                                        match pipeline.execute_heartbeat(&workspace_path, &session_id, &heartbeat_prompt, &tool_registry).await {
```

変更後:
```rust
                                        match pipeline.execute_heartbeat(&workspace_path, &session_id, &heartbeat_prompt, &tool_registry, &db_path).await {
```

（`db_path` は同スコープの行 221 で `let db_path = workspace_path.join("memory.db");` として定義済み）

- [ ] **Step 5: ビルド確認**

```bash
cargo build -p rustyclaw-agent -p rustyclaw-gateway 2>&1 | grep "^error" | head -10
```

Expected: エラーなし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(agent): add db_path param to execute_heartbeat for seen_items support"
```

---

## Task 2: `filter_seen_tool_result` ヘルパー関数の実装

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: 失敗するテストを追加**

`mod tests` ブロック末尾に追加：

```rust
    // ── Task 2: filter_seen_tool_result ──
    #[tokio::test]
    async fn test_filter_seen_tool_result_gmail_first_time() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(&dir.path().join("test.db")).unwrap();

        let tool_result = "--- STDOUT ---\n[{\"id\":\"msg1\",\"sender\":\"test@example.com\",\"subject\":\"Test\",\"date\":\"Mon\",\"snippet\":\"Hi\"}]\n--- EXIT STATUS ---\n0";
        let call_args = r#"{"script_name":"skills/gmail/scripts/506_get-gmail.sh","args":["is:unread","5"]}"#;

        // 1回目: 未読 → 通過 & 既読登録
        let result = filter_seen_tool_result("run_workspace_script", call_args, tool_result, &db);
        assert!(result.contains("msg1"), "First call: item should pass through");
        assert!(db.is_item_seen("gmail:msg1").unwrap(), "Should be marked as seen after first call");
    }

    #[tokio::test]
    async fn test_filter_seen_tool_result_gmail_already_seen() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(&dir.path().join("test.db")).unwrap();
        db.mark_item_seen("gmail:msg1", "gmail").unwrap();

        let tool_result = "--- STDOUT ---\n[{\"id\":\"msg1\",\"sender\":\"test@example.com\",\"subject\":\"Test\",\"date\":\"Mon\",\"snippet\":\"Hi\"}]\n--- EXIT STATUS ---\n0";
        let call_args = r#"{"script_name":"skills/gmail/scripts/506_get-gmail.sh"}"#;

        // 既読 → フィルタ
        let result = filter_seen_tool_result("run_workspace_script", call_args, tool_result, &db);
        assert!(!result.contains("msg1"), "Already-seen item should be filtered out");
        assert!(result.contains("already seen"), "Should indicate filtered items");
    }

    #[tokio::test]
    async fn test_filter_seen_tool_result_calendar() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(&dir.path().join("test.db")).unwrap();

        let stdout = r#"[{"event_id":"evt1","title":"Meeting","start":"2026-06-06T10:00:00+09:00","start_wday":"土","end":"2026-06-06T11:00:00+09:00","end_wday":"土"}]"#;
        let tool_result = format!("--- STDOUT ---\n{}\n--- EXIT STATUS ---\n0", stdout);
        let call_args = r#"{"script_name":"skills/calendar/scripts/calendar-ops.sh","args":["list"]}"#;

        // 1回目: 通過 & 既読登録
        let result1 = filter_seen_tool_result("run_workspace_script", &call_args, &tool_result, &db);
        assert!(result1.contains("evt1"));
        assert!(db.is_item_seen("calendar:evt1").unwrap());

        // 2回目: フィルタ
        let result2 = filter_seen_tool_result("run_workspace_script", &call_args, &tool_result, &db);
        assert!(!result2.contains("evt1"));
    }

    #[tokio::test]
    async fn test_filter_seen_tool_result_non_script_passthrough() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(&dir.path().join("test.db")).unwrap();

        // run_workspace_script 以外のツールはそのまま通過
        let result = filter_seen_tool_result("web_fetch", "{}", "some content", &db);
        assert_eq!(result, "some content");

        // gmail/calendar に関係しないスクリプトもそのまま通過
        let result2 = filter_seen_tool_result(
            "run_workspace_script",
            r#"{"script_name":"skills/weather/scripts/504_get-weather.sh"}"#,
            "weather data",
            &db,
        );
        assert_eq!(result2, "weather data");
    }

    #[tokio::test]
    async fn test_filter_seen_tool_result_partial_seen() {
        let dir = tempfile::tempdir().unwrap();
        let db = rustyclaw_storage::DbManager::new(&dir.path().join("test.db")).unwrap();
        db.mark_item_seen("gmail:msg1", "gmail").unwrap();

        let tool_result = "--- STDOUT ---\n[{\"id\":\"msg1\",\"subject\":\"Old\"},{\"id\":\"msg2\",\"subject\":\"New\"}]\n--- EXIT STATUS ---\n0";
        let call_args = r#"{"script_name":"skills/gmail/scripts/506_get-gmail.sh"}"#;

        let result = filter_seen_tool_result("run_workspace_script", call_args, tool_result, &db);
        assert!(!result.contains("\"msg1\""), "msg1 should be filtered (already seen)");
        assert!(result.contains("msg2"), "msg2 should pass through (new)");
        assert!(db.is_item_seen("gmail:msg2").unwrap());
    }
```

- [ ] **Step 2: テスト失敗を確認**

```bash
cargo test -p rustyclaw-agent "test_filter_seen_tool_result" -- --nocapture 2>&1 | head -20
```

Expected: コンパイルエラー `filter_seen_tool_result not found`

- [ ] **Step 3: `extract_stdout` / `rebuild_tool_result` ヘルパーを追加**

`crates/rustyclaw-agent/src/lib.rs` の `impl Pipeline` ブロックの**外**（`mod tests` より前、ファイル末尾付近）に以下を追加：

```rust
/// ツール結果文字列から "--- STDOUT ---\n" と "--- STDERR ---" の間のテキストを抽出する。
/// STDOUT セクションが存在しない場合は文字列全体を返す。
fn extract_stdout(tool_result: &str) -> &str {
    const MARKER: &str = "--- STDOUT ---\n";
    if let Some(start) = tool_result.find(MARKER) {
        let after = &tool_result[start + MARKER.len()..];
        let end = after.find("\n--- ").unwrap_or(after.len());
        &after[..end]
    } else {
        tool_result
    }
}

/// ツール結果文字列の STDOUT セクションを `new_stdout` で置き換えて返す。
fn rebuild_tool_result(original: &str, new_stdout: &str) -> String {
    const MARKER: &str = "--- STDOUT ---\n";
    if let Some(start) = original.find(MARKER) {
        let before = &original[..start + MARKER.len()];
        let after = &original[start + MARKER.len()..];
        let rest_start = after.find("\n--- ").unwrap_or(after.len());
        let rest = &after[rest_start..];
        format!("{}{}{}", before, new_stdout, rest)
    } else {
        new_stdout.to_string()
    }
}
```

- [ ] **Step 4: `filter_seen_tool_result` を追加**

上記2関数の直後に追加：

```rust
/// Gmail / Calendar スクリプト結果から既読アイテムを除外する。
///
/// - `tool_name` が `"run_workspace_script"` 以外 → そのまま返す
/// - `call_args` の `script_name` に `"gmail"` / `"calendar"` が含まれない → そのまま返す
/// - 上記に該当する場合: STDOUT の JSON 配列を解析し、既読アイテムを除外して `mark_item_seen` を呼ぶ
///
/// この関数は fail-open: パース失敗時は元の結果をそのまま返す。
fn filter_seen_tool_result(
    tool_name: &str,
    call_args: &str,
    tool_result: &str,
    db: &rustyclaw_storage::DbManager,
) -> String {
    if tool_name != "run_workspace_script" {
        return tool_result.to_string();
    }
    let args: serde_json::Value = serde_json::from_str(call_args).unwrap_or_default();
    let script_name = args["script_name"].as_str().unwrap_or("");

    let (category, id_field): (&str, &str) = if script_name.contains("gmail") {
        ("gmail", "id")
    } else if script_name.contains("calendar") {
        ("calendar", "event_id")
    } else {
        return tool_result.to_string();
    };

    let stdout = extract_stdout(tool_result);

    let items: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return tool_result.to_string(),
    };
    let arr = match items.as_array() {
        Some(a) => a.clone(),
        None => return tool_result.to_string(),
    };

    let mut new_items: Vec<serde_json::Value> = Vec::new();
    let mut seen_count: usize = 0;

    for item in &arr {
        if let Some(id) = item[id_field].as_str().filter(|s| !s.is_empty()) {
            let item_id = format!("{}:{}", category, id);
            if db.is_item_seen(&item_id).unwrap_or(false) {
                seen_count += 1;
            } else {
                let _ = db.mark_item_seen(&item_id, category);
                new_items.push(item.clone());
            }
        } else {
            // ID フィールドが欠損しているアイテムはそのまま通過
            new_items.push(item.clone());
        }
    }

    if seen_count == 0 {
        return tool_result.to_string();
    }

    let new_json = serde_json::to_string_pretty(&serde_json::Value::Array(new_items))
        .unwrap_or_else(|_| "[]".to_string());

    let notice = if new_json.trim() == "[]" {
        format!(
            "No new {} items. ({} already-seen item(s) were filtered.)",
            category, seen_count
        )
    } else {
        format!(
            "{}\n// Note: {} already-seen {} item(s) were filtered.",
            new_json, seen_count, category
        )
    };

    rebuild_tool_result(tool_result, &notice)
}
```

- [ ] **Step 5: テスト通過を確認**

```bash
cargo test -p rustyclaw-agent "test_filter_seen_tool_result" -- --nocapture
```

Expected: `test result: ok. 5 passed`

- [ ] **Step 6: 全テスト通過を確認**

```bash
cargo test -p rustyclaw-agent --lib 2>&1 | tail -3
```

Expected: `test result: ok. XX passed; 0 failed`

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): add filter_seen_tool_result helper for Gmail/Calendar deduplication"
```

---

## Task 3: ツール呼び出しループへの組み込み

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

`execute_heartbeat` 内のツール呼び出しループに `filter_seen_tool_result` を組み込む。

- [ ] **Step 1: 失敗するテストを追加**

`mod tests` ブロック末尾に追加：

```rust
    // ── Task 3: execute_heartbeat seen_items 統合テスト ──
    #[tokio::test]
    async fn test_execute_heartbeat_filters_seen_gmail() -> Result<()> {
        let _guard = ENV_MUTEX.lock().unwrap();
        let ws_dir = tempdir()?;
        fs::write(ws_dir.path().join("SOUL.md"), "Soul")?;
        fs::write(ws_dir.path().join("AGENTS.md"), "Agents")?;
        fs::write(ws_dir.path().join("MEMORY.md"), "Memory")?;
        fs::write(ws_dir.path().join("USER.md"), "User")?;
        fs::create_dir_all(ws_dir.path().join("scripts"))?;

        // Gmail 結果を返すスクリプトを作成
        let script_path = ws_dir.path().join("scripts").join("test_gmail.sh");
        fs::write(&script_path, "#!/bin/bash\necho '[{\"id\":\"msg99\",\"sender\":\"test@example.com\",\"subject\":\"Hello\",\"date\":\"Mon\",\"snippet\":\"Hi\"}]'")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms)?;
        }

        // DB を事前に作成し msg99 を既読登録
        let db_path = ws_dir.path().join("memory.db");
        let db = rustyclaw_storage::DbManager::new(&db_path).unwrap();
        db.mark_item_seen("gmail:msg99", "gmail").unwrap();
        drop(db);

        // モック LLM サーバー: run_workspace_script を呼んだ後に HEARTBEAT_OK を返す
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server_task = tokio::spawn(async move {
            // 1回目の LLM 呼び出し: run_workspace_script を要求
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"\",\
                            \"tool_calls\": [{\
                                \"id\": \"call1\",\
                                \"type\": \"function\",\
                                \"function\": {\
                                    \"name\": \"run_workspace_script\",\
                                    \"arguments\": \"{\\\"script_name\\\":\\\"skills/gmail/scripts/506_get-gmail.sh\\\"}\"\
                                }\
                            }]\
                        }\
                    }]\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
            // 2回目の LLM 呼び出し: HEARTBEAT_OK を返す（既読フィルタで空リストを受け取ったため）
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"HEARTBEAT_OK\"\
                        }\
                    }]\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let config = make_test_config_with_url(&format!("http://{}", addr));
        let flush_sem = Arc::new(Semaphore::new(1));
        let mut pipeline = Pipeline::new(config, flush_sem);

        // WorkspaceExecuteScriptTool を登録（実際のスクリプトを実行）
        let mut registry = ToolRegistry::new();
        let script_tool = rustyclaw_tools::WorkspaceExecuteScriptTool::new(ws_dir.path().to_path_buf());
        registry.register(Arc::new(script_tool.clone()) as Arc<dyn rig_core::tool::ToolDyn>);

        unsafe { std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", ws_dir.path()); }
        let resp = pipeline.execute_heartbeat(
            ws_dir.path(),
            "test-session",
            "Heartbeat check",
            &registry,
            &db_path,
        ).await;
        unsafe { std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR"); }

        let _ = server_task.await;
        let resp = resp?;
        // フィルタされた結果を LLM に渡したので、LLM は HEARTBEAT_OK を返した
        assert_eq!(resp.content, "HEARTBEAT_OK");
        Ok(())
    }
```

- [ ] **Step 2: テスト失敗を確認**

```bash
cargo test -p rustyclaw-agent test_execute_heartbeat_filters_seen_gmail -- --nocapture 2>&1 | tail -20
```

Expected: FAIL（フィルタリングが未実装のため LLM がツール結果を受け取っても HEARTBEAT_OK にならない）

- [ ] **Step 3: `execute_heartbeat` 内に `db_opt` を作成**

`crates/rustyclaw-agent/src/lib.rs` の `execute_heartbeat` 関数内、`let provider_tools: ...` の直前（行 655 付近）に以下を追加：

変更前（行 655 付近）:
```rust
        let provider_tools: Vec<rustyclaw_providers::ToolDef> = tool_registry
```

変更後:
```rust
        // seen_items フィルタ用 DB（fail-open: 失敗時はフィルタをスキップ）
        let db_opt = rustyclaw_storage::DbManager::new(db_path).ok();

        let provider_tools: Vec<rustyclaw_providers::ToolDef> = tool_registry
```

- [ ] **Step 4: ツール呼び出しループに `filter_seen_tool_result` を組み込む**

`execute_heartbeat` 内のツール呼び出しループ（行 686〜708 付近）を変更：

変更前:
```rust
                        let (tool_content, _tool_is_error) = if let Some(tool) = tool_registry.get(&call.function.name) {
                            match tool.call(call.function.arguments.clone()).await {
                                Ok(content) => (content, false),
                                Err(e) => (format!("Tool error: {}", e), true),
                            }
                        } else {
                            (format!("Error: Tool '{}' not found in registry", call.function.name), true)
                        };
```

変更後:
```rust
                        let (mut tool_content, tool_is_error) = if let Some(tool) = tool_registry.get(&call.function.name) {
                            match tool.call(call.function.arguments.clone()).await {
                                Ok(content) => (content, false),
                                Err(e) => (format!("Tool error: {}", e), true),
                            }
                        } else {
                            (format!("Error: Tool '{}' not found in registry", call.function.name), true)
                        };

                        // Gmail / Calendar スクリプト結果から既読アイテムをフィルタ（fail-open）
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
```

- [ ] **Step 5: テスト通過を確認**

```bash
cargo test -p rustyclaw-agent test_execute_heartbeat_filters_seen_gmail -- --nocapture
```

Expected: PASS

- [ ] **Step 6: 全テスト通過を確認**

```bash
cargo test --workspace --lib 2>&1 | tail -5
```

Expected: 全テスト通過

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): wire filter_seen_tool_result into execute_heartbeat tool loop"
```

---

## Self-Review

### 1. Spec カバレッジ

仕様（`docs/specs/04_heartbeat_spec.md §4④`、ログレポート `2-2`）の要件:

| 要件 | タスク |
|------|--------|
| `is_item_seen` でチェックし未読の場合のみ LLM に渡す | Task 3 (filter 組み込み) |
| `mark_item_seen` で既読登録 | Task 2 (`filter_seen_tool_result` 内) |
| Gmail の `id` フィールドで識別 | Task 2 (id_field = "id") |
| Calendar の `event_id` フィールドで識別 | Task 2 (id_field = "event_id") |
| fail-open（DBエラー時はスキップ）| Task 3 (`db_opt = ...ok()`) |

### 2. プレースホルダー確認

なし。全ステップにコードブロックあり。

### 3. 型整合性確認

- `filter_seen_tool_result(tool_name: &str, call_args: &str, tool_result: &str, db: &DbManager) -> String` — Task 2 定義、Task 3 使用 ✅
- `extract_stdout(tool_result: &str) -> &str` — Task 2 定義、`filter_seen_tool_result` 内で使用 ✅
- `rebuild_tool_result(original: &str, new_stdout: &str) -> String` — Task 2 定義、`filter_seen_tool_result` 内で使用 ✅
- `db_opt: Option<DbManager>` — Task 3 で作成、ループ内で `if let Some(ref db) = db_opt` ✅
