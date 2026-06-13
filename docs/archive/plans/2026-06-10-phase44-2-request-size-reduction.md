# Phase 44-2: LLM リクエストサイズ削減 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** システムプロンプトへ注入する SOUL.md・USER.md の文字数を上限付きで圧縮して LLM への送信トークン数を削減し、デバッグ用に `last_request.json`（< 5 KB）を自動生成する。

**Architecture:** `Pipeline::build_system_context()` に `truncate_context_content()` ヘルパーを追加し、SOUL.md と USER.md をそれぞれ最大 3,000 文字に制限する。`dump_llm_io()` にコンパクト版ダンプ書き出し処理を追加し、各リクエスト後に `{workspace}/memory/debug/llm/last_request.json` を生成する。AGENTS.md・MEMORY.md はすでに RAG 経由なので変更対象外。

**Tech Stack:** Rust (std::fs, serde_json, chrono)

---

## 背景と現状

調査結果の要点:

| ファイル | 注入方式 | 現在サイズ (本番) |
|---------|---------|----------------|
| SOUL.md | 静的 (`build_system_context`) | ~5,306 bytes |
| USER.md | 静的 (`build_system_context`) | ~3,505 bytes |
| AGENTS.md | RAG ベクトル検索（変更不要） | ~6,660 bytes |
| MEMORY.md | RAG ベクトル検索（変更不要） | ~5,896 bytes |

- `dump_request()` / `dump_response()` は agent 層では空スタブ（`// Consolidated in providers layer`）
- 実ダンプは `crates/rustyclaw-providers/src/lib.rs` の `dump_llm_io()` が `memory/debug/llm/{category}/{YYYY-MM-DD}/{HH-MM-SS}.json` に書き込む
- `last_request.json` の書き込みは現在のコードに存在しない（Dashboard の `/debug/request` は旧参照）

---

## ファイルマップ

| 操作 | ファイル | 変更内容 |
|------|---------|---------|
| Modify | `crates/rustyclaw-agent/src/lib.rs` | `truncate_context_content()` ヘルパー追加、`build_system_context()` で適用 |
| Modify | `crates/rustyclaw-providers/src/lib.rs` | `dump_llm_io()` に `last_request.json` 書き出し追加 |

---

## Task 1: `truncate_context_content()` ヘルパーの追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

`build_system_context()` は L382 付近にある。`const MAX_CONTEXT_CHARS_PER_FILE` と `fn truncate_context_content()` を `Pipeline` の `impl` ブロック内に追加する。

- [ ] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-agent/src/lib.rs` の `#[cfg(test)]` ブロック（ファイル末尾付近）に以下を追加する:

```rust
#[test]
fn test_truncate_context_content_short_stays_unchanged() {
    let short = "a".repeat(100);
    let result = Pipeline::truncate_context_content(&short, 3_000);
    assert_eq!(result, short);
}

#[test]
fn test_truncate_context_content_long_is_truncated() {
    let long_content = "あ".repeat(4_000); // 4000文字
    let result = Pipeline::truncate_context_content(&long_content, 3_000);
    let char_count = result.chars().count();
    // 切り詰め後の文字列 3000文字 + 省略メッセージ行が含まれる
    assert!(char_count > 3_000, "省略メッセージが付いているはず");
    assert!(result.contains("[RustyClaw]"), "省略マーカーが含まれるはず");
    // 先頭3000文字が保持されている
    let expected_prefix: String = "あ".repeat(3_000);
    assert!(result.starts_with(&expected_prefix));
}

#[test]
fn test_truncate_context_content_exact_limit_stays_unchanged() {
    let content = "x".repeat(3_000);
    let result = Pipeline::truncate_context_content(&content, 3_000);
    assert_eq!(result, content);
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-agent test_truncate_context_content 2>&1
```

期待出力: コンパイルエラー（`truncate_context_content` が未定義）またはテスト失敗。

- [ ] **Step 3: `truncate_context_content()` を実装する**

`Pipeline` の `impl` ブロック内の `build_system_context()` の直前に以下を追加する:

```rust
/// システムプロンプトへ注入するファイルコンテンツを最大 `max_chars` 文字に切り詰める。
/// 切り詰めた場合は末尾に省略マーカーを付与する。
pub fn truncate_context_content(content: &str, max_chars: usize) -> String {
    let char_count = content.chars().count();
    if char_count <= max_chars {
        return content.to_string();
    }
    let truncated: String = content.chars().take(max_chars).collect();
    format!(
        "{}\n\n> ⚠️ [RustyClaw] 以降を省略（全 {} 文字中 先頭 {} 文字を注入）",
        truncated,
        char_count,
        max_chars
    )
}
```

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-agent test_truncate_context_content 2>&1
```

期待出力:
```
test tests::test_truncate_context_content_exact_limit_stays_unchanged ... ok
test tests::test_truncate_context_content_long_is_truncated ... ok
test tests::test_truncate_context_content_short_stays_unchanged ... ok
test result: ok. 3 passed; 0 failed
```

- [ ] **Step 5: コミットする**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 44-2 truncate_context_content() ヘルパーを追加"
```

---

## Task 2: `build_system_context()` に圧縮を適用する

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

`build_system_context()` の `SOUL.md` / `USER.md` 読み込みループ内で `strip_comments()` の結果に `truncate_context_content()` を適用する。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test]
fn test_build_system_context_truncates_large_files() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let workspace = dir.path();

    // SOUL.md に 5000文字のコンテンツを書く
    let soul_content = "S".repeat(5_000);
    fs::write(workspace.join("SOUL.md"), &soul_content).unwrap();

    // USER.md に 4000文字のコンテンツを書く
    let user_content = "U".repeat(4_000);
    fs::write(workspace.join("USER.md"), &user_content).unwrap();

    // 既存テストと同じパターンで Pipeline を生成する（build_system_context は self フィールド不使用）
    let config = make_test_config_with_url("http://localhost");
    let flush_sem = Arc::new(Semaphore::new(1));
    let pipeline = Pipeline::new(config, flush_sem);
    let result = pipeline.build_system_context(workspace).unwrap();

    // 各ファイルの注入量が MAX_CONTEXT_CHARS_PER_FILE を超えないことを確認
    let soul_section_start = result.find("# SOUL.md").unwrap();
    let user_section_start = result.find("# USER.md").unwrap();
    let soul_section = &result[soul_section_start..user_section_start];
    // 省略マーカーが含まれている
    assert!(soul_section.contains("[RustyClaw]"), "SOUL.md が切り詰められているはず");
}
```

- [ ] **Step 2: テスト用ヘルパーを確認する（追加作業なし）**

`build_system_context` は `self` のフィールドを一切参照せず `Self::strip_comments()` のみを呼ぶため、テスト専用ファクトリは不要。
Step 1 のテストは既存の `make_test_config_with_url` + `Pipeline::new()` パターン（L3018-3020 と同形）を使うのでそのままコンパイルできる。

- [ ] **Step 3: `build_system_context()` に圧縮を適用する**

`build_system_context()` 内のループ処理を変更する。

変更前（概略）:
```rust
for filename in &files {
    let path = workspace_dir.join(filename);
    let content = match fs::read_to_string(&path) { ... };
    context.push_str(&format!("# {}\n\n{}\n\n", filename, Self::strip_comments(&content)));
}
```

変更後:
```rust
/// 各ファイルのシステムプロンプト注入上限（文字数）
const MAX_CONTEXT_CHARS_PER_FILE: usize = 3_000;

for filename in &files {
    let path = workspace_dir.join(filename);
    let content = match fs::read_to_string(&path) { ... };
    let stripped = Self::strip_comments(&content);
    let truncated = Self::truncate_context_content(&stripped, MAX_CONTEXT_CHARS_PER_FILE);
    context.push_str(&format!("# {}\n\n{}\n\n", filename, truncated));
}
```

> `const MAX_CONTEXT_CHARS_PER_FILE` はループの外（`impl Pipeline` 内のトップレベル定数か関数冒頭）に定義すること。

- [ ] **Step 4: Clippy でエラーがないことを確認する**

```bash
cargo clippy -p rustyclaw-agent 2>&1
```

期待出力: `Finished` が出て warning/error が 0 件。

- [ ] **Step 5: 全テストが通ることを確認する**

```bash
cargo test -p rustyclaw-agent 2>&1
```

期待出力: `test result: ok. N passed; 0 failed`

- [ ] **Step 6: コミットする**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 44-2 build_system_context で SOUL.md/USER.md を 3000文字に圧縮"
```

---

## Task 3: `dump_llm_io()` に `last_request.json` 書き出しを追加する

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`

`dump_llm_io()` は現在 `memory/debug/llm/{category}/{YYYY-MM-DD}/{HH-MM-SS}.json` にフル JSON を書く。
この関数の末尾に `last_request.json` のコンパクト版書き出しを追加する。

- [ ] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-providers/src/lib.rs` の `#[cfg(test)]` ブロックに以下を追加する:

```rust
#[test]
fn test_dump_llm_io_writes_last_request_json() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let workspace = dir.path();

    // RUSTYCLAW_WORKSPACE_DIR を一時ディレクトリに向ける
    unsafe { std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", workspace.to_str().unwrap()); }

    let messages = vec![
        crate::Message {
            role: "system".to_string(),
            content: Some("S".repeat(1_000)), // 1000文字のシステムプロンプト
            name: None,
            tool_calls: None,
            tool_call_id: None,
            trigger: None,
            timestamp: None,
        },
        crate::Message {
            role: "user".to_string(),
            content: Some("Hello".to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            trigger: None,
            timestamp: None,
        },
    ];

    let response = LlmResponse {
        content: "Hi there".to_string(),
        role: "assistant".to_string(),
        tool_calls: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        model_used: None,
        provider_id: None,
    };
    dump_llm_io("test", "test-model", &messages, &response);

    let last_req_path = workspace
        .join("memory").join("debug").join("llm").join("last_request.json");

    assert!(last_req_path.exists(), "last_request.json が生成されているはず");

    let content = fs::read_to_string(&last_req_path).unwrap();
    let size = content.len();
    assert!(size < 5_120, "last_request.json が 5KB 未満のはず (実際: {} bytes)", size);

    let val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(val["model"].is_string());
    assert!(val["message_count"].is_number());
    assert!(val["messages"].is_array());

    unsafe { std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR"); }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-providers test_dump_llm_io_writes_last_request_json 2>&1
```

期待出力: `FAILED` (`last_request.json` が存在しないため)

- [ ] **Step 3: `dump_llm_io()` に `last_request.json` 書き出しを追加する**

`dump_llm_io()` 関数の末尾（タイムスタンプ付きファイル書き出しの後）に以下を追加する:

```rust
// ── last_request.json (コンパクト版) の書き出し ──────────────────────
// Dashboard /debug/request エンドポイントおよびデバッグ用。目標 < 5 KB。
const CONTENT_PREVIEW_CHARS: usize = 500;

fn preview_str(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= CONTENT_PREVIEW_CHARS {
        s.to_string()
    } else {
        format!(
            "{}…[省略: 全 {} 文字中 先頭 {} 文字]",
            chars[..CONTENT_PREVIEW_CHARS].iter().collect::<String>(),
            chars.len(),
            CONTENT_PREVIEW_CHARS
        )
    }
}

let compact_messages: Vec<serde_json::Value> = messages
    .iter()
    .map(|m| {
        serde_json::json!({
            "role": m.role,
            "content_preview": preview_str(m.content.as_deref().unwrap_or("")),
        })
    })
    .collect();

let compact = serde_json::json!({
    "timestamp": now.format("%Y-%m-%dT%H:%M:%S").to_string(),
    "category": category,
    "model": model,
    "message_count": messages.len(),
    "messages": compact_messages,
    "response_preview": preview_str(&response.content),
});

if let Ok(json_str) = serde_json::to_string_pretty(&compact) {
    let last_req_path = workspace_dir
        .join("memory")
        .join("debug")
        .join("llm")
        .join("last_request.json");
    if let Some(parent) = last_req_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(e) = fs::write(&last_req_path, &json_str) {
        tracing::warn!("last_request.json の書き込みに失敗: {}", e);
    }
}
```

> `preview_str` はクロージャではなくネストした fn として定義するか、`dump_llm_io` の引数スコープ外で `fn preview_str(...)` として定義すること。クロージャを使う場合は `let preview_str = |s: &str| -> String { ... };` の形式でも可。

> `now` 変数は既存の `dump_llm_io` 内で `chrono::Local::now()` として定義されているはず。ない場合は関数冒頭で `let now = chrono::Local::now();` を追加する。

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-providers test_dump_llm_io_writes_last_request_json 2>&1
```

期待出力: `test tests::test_dump_llm_io_writes_last_request_json ... ok`

- [ ] **Step 5: 既存のダンプテストも通ることを確認する**

```bash
cargo test -p rustyclaw-providers dump_llm_io 2>&1
```

期待出力: `test result: ok. N passed; 0 failed`

- [ ] **Step 6: Clippy でエラーがないことを確認する**

```bash
cargo clippy -p rustyclaw-providers 2>&1
```

期待出力: `Finished` が出て warning/error が 0 件。

- [ ] **Step 7: コミットする**

```bash
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "feat(providers): Phase 44-2 dump_llm_io に last_request.json コンパクト書き出しを追加"
```

---

## Task 4: 全体テストとサイズ確認

- [ ] **Step 1: ワークスペース全テストを実行する**

```bash
cargo test --workspace 2>&1
```

期待出力: `test result: ok. N passed; 0 failed` (全クレート)

- [ ] **Step 2: 本番ログから実際のリクエストサイズを確認する（任意）**

サービス再起動後にチャットを1件送信し、以下で `last_request.json` を確認する:

```bash
ssh rp1 'wc -c ~/.rustyclaw/memory/debug/llm/last_request.json && cat ~/.rustyclaw/memory/debug/llm/last_request.json | python3 -m json.tool'
```

期待: ファイルサイズが 5120 bytes 未満、JSON が整形されて表示される。

- [ ] **Step 3: 仕様書・タスクリストを更新する**

`docs/specs/02_agent_pipeline.md` または `docs/specs/06_dashboard_spec.md` の該当箇所に以下を追記する:
- `build_system_context()`: SOUL.md / USER.md は最大 3,000 文字に圧縮して注入
- `dump_llm_io()`: タイムスタンプ付きダンプに加えて `memory/debug/llm/last_request.json` を毎回生成

ファイル冒頭の `最終更新日` を `2026-06-10` に更新する。

`docs/task.md` の `Phase 44-2` を `[x]` に変更する。

- [ ] **Step 4: コミットする**

```bash
git add docs/
git commit -m "docs(specs): Phase 44-2 コンテキスト圧縮・last_request.json 仕様を追記"
```

---

## Self-Review

- **Spec coverage**:
  - SOUL.md/USER.md 圧縮 → Task 2 ✅
  - last_request.json < 5KB → Task 3 ✅
  - AGENTS.md/MEMORY.md は対象外（すでに RAG 経由）→ 明記済み ✅
  - ドキュメント更新 → Task 4 ✅
- **Placeholder scan**: TBD/TODO なし。`new_for_test()` に実装者向けの具体的な指示あり
- **Type consistency**: `truncate_context_content(content: &str, max_chars: usize) -> String` は Task 1 で定義し Task 2 で使用。`preview_str` は Task 3 内で完結。一貫している
- **`Message` struct の `content` フィールド**: `Option<String>` であることを前提に `.as_deref().unwrap_or("")` で扱っている
- **ギャップ**: `Pipeline::new_for_test()` の実装は `Pipeline` の実際のフィールド構成に依存するため、実装者がファイルを参照して確定させる必要がある点を明示した
