# Context Window Phase 1 — 安定化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** TPM ベースの誤ったコンテキスト圧縮を `context_window` ベースの件数制限に置き換え、memory flush の 400 エラーを防ぎ、session-summary の重複実行を解消する。

**Architecture:** 3つの独立した外科的修正。(1) `parse_context_window()` で `context_window` 文字列をトークン数に変換し `get_history_message_limit()` を書き換え、`compact_if_needed_with_overhead()` を廃止。(2) `flush_memory()` にコンテキストサイズ安全チェックを追加し debug config を修正。(3) `find_next_session_needing_summary()` に 10 分以内の summary mtime ガードを追加。

**Tech Stack:** Rust, tokio, rustyclaw-agent, rustyclaw-gateway, rustyclaw-config

---

## File Map

| ファイル | 変更内容 |
|---------|---------|
| `crates/rustyclaw-agent/src/lib.rs` | `parse_context_window()` 新設、`get_history_message_limit()` 変更、`get_history_limit()` 削除、`compact_if_needed_with_overhead()` 3箇所削除、`flush_memory()` にコンテキストチェック追加 |
| `crates/rustyclaw-gateway/src/cron.rs` | `find_next_session_needing_summary()` に 10 分 mtime ガード追加 |
| `production/config/config.debug.json` | `agents."memory"` から LM Studio を削除 |

---

## Task 1: `parse_context_window()` ヘルパー関数

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-agent/src/lib.rs` の `#[cfg(test)] mod tests {` ブロック末尾（`}` の直前）に追加する。

```rust
    #[test]
    fn test_parse_context_window_k_suffix() {
        assert_eq!(parse_context_window(Some("8k")), 8_192);
        assert_eq!(parse_context_window(Some("16k")), 16_384);
        assert_eq!(parse_context_window(Some("32k")), 32_768);
        assert_eq!(parse_context_window(Some("131k")), 134_144);
        assert_eq!(parse_context_window(Some("256k")), 262_144);
    }

    #[test]
    fn test_parse_context_window_m_suffix() {
        assert_eq!(parse_context_window(Some("1M")), 1_048_576);
    }

    #[test]
    fn test_parse_context_window_none_or_empty() {
        assert_eq!(parse_context_window(None), 32_768);
        assert_eq!(parse_context_window(Some("")), 32_768);
    }
```

- [ ] **Step 2: テストの失敗を確認する**

```bash
cargo test -p rustyclaw-agent test_parse_context_window 2>&1 | grep -E "error|FAILED|test result"
```

Expected: `error[E0425]: cannot find function 'parse_context_window'`

- [ ] **Step 3: `parse_context_window()` を実装する**

`crates/rustyclaw-agent/src/lib.rs` で `fn truncate_70_20(content: &str, max_bytes: usize)` 関数（line 1321 付近）の**直前**に追加する。

```rust
/// config.json の context_window 文字列（"8k", "131k", "256k", "1M" 等）をトークン数に変換する。
/// 未設定または認識不能な場合は保守的なデフォルト 32,768 を返す。
fn parse_context_window(context_window: Option<&str>) -> usize {
    let s = match context_window {
        Some(s) if !s.is_empty() => s.trim().to_lowercase(),
        _ => return 32_768,
    };
    if let Some(num) = s.strip_suffix('m') {
        num.trim().parse::<usize>().unwrap_or(1) * 1_048_576
    } else if let Some(num) = s.strip_suffix('k') {
        num.trim().parse::<usize>().unwrap_or(32) * 1_024
    } else {
        s.parse::<usize>().unwrap_or(32_768)
    }
}
```

- [ ] **Step 4: テストの通過を確認する**

```bash
cargo test -p rustyclaw-agent test_parse_context_window 2>&1 | grep -E "PASSED|ok|test result"
```

Expected: `3 passed; 0 failed`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): add parse_context_window() helper"
```

---

## Task 2: `get_history_message_limit()` を context_window ベースに変更し、`compact_if_needed_with_overhead()` を廃止

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: テストを書く**

`#[cfg(test)] mod tests {` ブロック末尾に追加する。

```rust
    #[test]
    fn test_get_history_message_limit_uses_context_window() {
        use rustyclaw_config::{Config, ModelEntry, AgentsConfig, ModelNames};

        fn make_config(context_window: &str) -> Config {
            Config {
                model_list: vec![ModelEntry {
                    model_name: "test-model".to_string(),
                    provider: "openai".to_string(),
                    model: "test-model-api".to_string(),
                    api_base: "http://localhost".to_string(),
                    api_key: "key".to_string(),
                    max_tokens: Some(2048),
                    temperature: Some(0.7),
                    enabled: true,
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: Some(context_window.to_string()),
                    cf_aig_gateway_id: None,
                }],
                agents: AgentsConfig {
                    default: ModelNames::Single("test-model".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            }
        }

        let flush_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));

        let p16k  = Pipeline::new(make_config("16k"),  flush_sem.clone());
        let p32k  = Pipeline::new(make_config("32k"),  flush_sem.clone());
        let p64k  = Pipeline::new(make_config("64k"),  flush_sem.clone());
        let p131k = Pipeline::new(make_config("131k"), flush_sem.clone());
        let p256k = Pipeline::new(make_config("256k"), flush_sem.clone());

        assert_eq!(p16k.get_history_message_limit("default"),  10, "16k → 10件");
        assert_eq!(p32k.get_history_message_limit("default"),  20, "32k → 20件");
        assert_eq!(p64k.get_history_message_limit("default"),  40, "64k → 40件");
        assert_eq!(p131k.get_history_message_limit("default"), 60, "131k → 60件");
        assert_eq!(p256k.get_history_message_limit("default"), 80, "256k → 80件");
    }
```

- [ ] **Step 2: テストの失敗を確認する**

```bash
cargo test -p rustyclaw-agent test_get_history_message_limit_uses_context_window 2>&1 | grep -E "FAILED|test result|assert"
```

Expected: `FAILED` — 現行は TPM ベースなので `131k → 60` が通らない。

- [ ] **Step 3: `get_history_message_limit()` を書き換える**

`crates/rustyclaw-agent/src/lib.rs` の lines 118–134 を以下に置き換える。

```rust
    /// context_window ベースのメッセージ件数ハードキャップ。
    /// モデルの context_window 文字列を parse_context_window() で解釈し、
    /// 大きいモデルほど多くの履歴を保持できるようにする。
    fn get_history_message_limit(&self, purpose: &str) -> usize {
        let model_cfg = self.config.get_model(purpose);
        let cw = self.config.model_list.iter()
            .find(|m| m.model == model_cfg.model_name && m.enabled)
            .and_then(|m| m.context_window.as_deref());
        let ctx = parse_context_window(cw);
        match ctx {
            0..=16_383      => 10,
            16_384..=32_767  => 20,
            32_768..=65_535  => 40,
            65_536..=262_143 => 60,
            _                => 80,
        }
    }
```

- [ ] **Step 4: `get_history_limit()` を削除し、3箇所の `compact_if_needed_with_overhead()` 呼び出しを削除する**

lines 101–116 の `get_history_limit()` 関数全体を削除する。

次に以下の3箇所を変更する（行番号は削除後にずれるため内容で検索すること）:

**変更箇所 1** (`execute()` 内、`// ISSUE-02/FU-3` コメントの周辺):
```rust
// 削除する3行:
        let history_limit = self.get_history_limit("default");
        // ISSUE-02/FU-3: system プロンプト分のトークンを考慮して実効上限を下げる（ツール無し経路）
        let overhead_tokens = (system_context.chars().count() * 3) / 2;
        history.compact_if_needed_with_overhead(history_limit, overhead_tokens);
        history.trim_to_last(self.get_history_message_limit("default"));
// 残す1行:
        history.trim_to_last(self.get_history_message_limit("default"));
```

**変更箇所 2** (`execute_with_tools()` 内、`// ISSUE-02:` コメントの周辺):
```rust
// 削除する4行:
        let history_limit = self.get_history_limit(purpose);
        // ISSUE-02: system プロンプト＋ツール定義分のトークンを考慮して実効上限を下げる
        let overhead_chars = system_context.chars().count()
            + tool_registry.to_llm_schemas().iter().map(|s| s.to_string().chars().count()).sum::<usize>();
        let overhead_tokens = (overhead_chars * 3) / 2;
        history.compact_if_needed_with_overhead(history_limit, overhead_tokens);
        history.trim_to_last(self.get_history_message_limit(purpose));
// 残す1行:
        history.trim_to_last(self.get_history_message_limit(purpose));
```

**変更箇所 3** (`execute_stream()` 内、`// ISSUE-02/FU-3` コメントの周辺):
```rust
// 削除する3行:
        let history_limit = self.get_history_limit("default");
        // ISSUE-02/FU-3: system プロンプト分のトークンを考慮して実効上限を下げる（ツール無し経路）
        let overhead_tokens = (system_context.chars().count() * 3) / 2;
        history.compact_if_needed_with_overhead(history_limit, overhead_tokens);
        history.trim_to_last(self.get_history_message_limit("default"));
// 残す1行:
        history.trim_to_last(self.get_history_message_limit("default"));
```

- [ ] **Step 5: ビルドが通ることを確認する**

```bash
cargo check -p rustyclaw-agent 2>&1 | grep -E "error|warning.*unused" | head -20
```

Expected: エラーなし（`compact_if_needed_with_overhead` が unused になる場合は storage 側で参照があるため warning にならない）

- [ ] **Step 6: テストを実行して全通過を確認する**

```bash
cargo test -p rustyclaw-agent 2>&1 | grep -E "test result|FAILED"
```

Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "fix(agent): replace TPM-based context limit with context_window message count"
```

---

## Task 3: memory flush のコンテキストサイズ安全チェック + debug config 修正

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`
- Modify: `production/config/config.debug.json`

- [ ] **Step 1: `flush_memory()` にコンテキストチェックを追加する**

`flush_memory()` 内で `conversation_text` が組み立てられた直後（`let existing_memory = ...` の前）に以下を挿入する。

```rust
        // コンテキストサイズ安全チェック: flush プロンプトの推定トークン数がモデルの
        // context_window の 80% を超える場合はスキップして 400 エラーを防ぐ。
        let flush_text_tokens = (conversation_text.chars().count() * 3 / 2) + 2_000; // system overhead
        let flush_model_cw = config.model_list.iter()
            .find(|m| m.model == memory_model.model_name && m.enabled)
            .and_then(|m| m.context_window.as_deref());
        let flush_ctx_limit = (parse_context_window(flush_model_cw) * 4) / 5; // 80%
        if flush_text_tokens > flush_ctx_limit {
            tracing::warn!(
                session = %session_id,
                estimated_tokens = flush_text_tokens,
                ctx_limit = flush_ctx_limit,
                "memory flush: skipping — estimated tokens exceeds model context limit"
            );
            return Ok(());
        }
```

挿入位置の前後コンテキスト（`conversation_text` の組み立てはこの付近にある）:

```rust
        // ...conversation_text の組み立て...
        for msg in &history[start..] {
            conversation_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        // ← ここに挿入 ↑

        // 既存の MEMORY.md を読み込む（なければ空）
        let memory_path = workspace_dir.join("MEMORY.md");
```

- [ ] **Step 2: ビルドが通ることを確認する**

```bash
cargo check -p rustyclaw-agent 2>&1 | grep "error"
```

Expected: エラーなし

- [ ] **Step 3: debug config の `"memory"` purpose から LM Studio を削除する**

`production/config/config.debug.json` の `agents."memory"` を以下に変更する。

変更前:
```json
"memory": [
  "lms-gemma-4-26b",
  "groq-llama-8b"
]
```

変更後:
```json
"memory": [
  "groq-llama-8b"
]
```

理由: LM Studio（n_ctx=11,008）は memory flush に十分なコンテキストを持たない。コンテキストチェックでスキップされても、フォールバック先として残していると混乱を招く。

- [ ] **Step 4: テストを実行して全通過を確認する**

```bash
cargo test -p rustyclaw-agent 2>&1 | grep -E "test result|FAILED"
```

Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs production/config/config.debug.json
git commit -m "fix(agent): add context_window safety check to memory flush; fix debug config"
```

---

## Task 4: session-summary の重複トリガー防止

**Files:**
- Modify: `crates/rustyclaw-gateway/src/cron.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-gateway/src/cron.rs` の `#[cfg(test)] mod tests {` ブロック（line 422 付近）内で、`test_recent_sessions_are_excluded` テストの後に追加する。

```rust
    #[test]
    fn test_recently_summarized_session_is_excluded() {
        let ws = tempfile::tempdir().unwrap();
        let sessions_dir = ws.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let summaries_dir = ws.path().join("memory").join("summaries");
        std::fs::create_dir_all(&summaries_dir).unwrap();

        // アイドル10分のセッションを作成
        let session_path = sessions_dir.join("discord-test-20260603.jsonl");
        let mut f = std::fs::File::create(&session_path).unwrap();
        writeln!(f, r#"{{"role":"user","content":"hi"}}"#).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let ft = filetime::FileTime::from_system_time(old);
        filetime::set_file_mtime(&session_path, ft).unwrap();

        // サマリーファイルを 2 分前に作成（最近要約済み）
        let summary_path = summaries_dir.join("2026-06-03-discord-test-20260603.md");
        std::fs::write(&summary_path, "<!-- turns: 1 -->").unwrap();
        let recent = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
        let ft2 = filetime::FileTime::from_system_time(recent);
        filetime::set_file_mtime(&summary_path, ft2).unwrap();

        let result = find_next_session_needing_summary(&sessions_dir, ws.path());
        assert!(result.is_none(), "recently summarized session (< 10 min ago) must be excluded");
    }
```

- [ ] **Step 2: テストの失敗を確認する**

```bash
cargo test -p rustyclaw-gateway test_recently_summarized_session_is_excluded 2>&1 | grep -E "FAILED|test result"
```

Expected: `FAILED` — 現行コードは summary mtime の新しさを考慮しない。

- [ ] **Step 3: `find_next_session_needing_summary()` を修正する**

`crates/rustyclaw-gateway/src/cron.rs` の `find_next_session_needing_summary()` 関数（line 8 付近）で、`let needs_summary = ...` ブロックの**前**に以下を追加する。

```rust
        // サマリーファイルが過去 10 分以内に更新されていれば再トリガーしない。
        // summary 生成中（プレースホルダー書き込み後）および完了直後を対象とする。
        if summary_path.exists() {
            if let Some(summary_age_secs) = summary_path.metadata().ok()
                .and_then(|m| m.modified().ok())
                .and_then(|sm| now.duration_since(sm).ok())
                .map(|d| d.as_secs())
            {
                if summary_age_secs < 600 {
                    continue;
                }
            }
        }
```

追加後の関数の構造:
```rust
pub(crate) fn find_next_session_needing_summary(...) -> Option<String> {
    let now = std::time::SystemTime::now();
    ...
    for entry in entries.flatten() {
        ...
        if elapsed < 300 { continue; }  // 5分未満のアクティブセッション除外（既存）
        ...
        // ← ここに 10分ガードを挿入（上記コード）

        let needs_summary = if !summary_path.exists() {
            true
        } else {
            summary_path.metadata().ok()
                .and_then(|m| m.modified().ok())
                .map(|sm| sm < modified)
                .unwrap_or(false)
        };
        if needs_summary {
            return Some(safe_session_id);
        }
    }
    None
}
```

- [ ] **Step 4: テストの通過を確認する**

```bash
cargo test -p rustyclaw-gateway test_recently_summarized 2>&1 | grep -E "test result|FAILED"
```

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 5: 既存テストが全通過することを確認する**

```bash
cargo test -p rustyclaw-gateway 2>&1 | grep -E "test result|FAILED"
```

Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-gateway/src/cron.rs
git commit -m "fix(gateway): prevent duplicate session-summary triggers within 10 minutes"
```

---

## Task 5: ワークスペーステスト確認 + deploy

- [ ] **Step 1: ワークスペース全テストを確認する**

```bash
cargo test --workspace --quiet 2>&1 | grep -E "test result|FAILED|error\["
```

Expected: 全 crate テスト通過、`FAILED` なし

- [ ] **Step 2: release ビルドを確認する**

```bash
cargo build --release -p rustyclaw-cli 2>&1 | tail -5
```

Expected: `Finished \`release\` profile`

- [ ] **Step 3: デプロイ**

```bash
bash scripts/deploy.sh
```

Expected: 4ステップ全て ✓ で完了
