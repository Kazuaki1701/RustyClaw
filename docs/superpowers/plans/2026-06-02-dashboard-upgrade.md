# [Dashboard Upgrade & Historical LLM Inspector] Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** LLM ダンプのローテーション保存・プロバイダ別統計・LANE QUEUE バッジ UI・per-provider クールダウン表示・LLM 履歴インスペクタを実装する。

**Architecture:** Rust バックエンド側では `LlmResponse` に `provider_id` を追加して全呼び出し元に伝搬させ、storage の `usage` テーブルに `provider_id` カラムを追加する。ダッシュボードは `health.rs` の埋め込み HTML/CSS/JS を段階的に書き換える。

**Tech Stack:** Rust / rusqlite / serde_json / chrono / vanilla JS (embedded in health.rs)

**Spec:** `docs/superpowers/specs/2026-06-02-dashboard-upgrade-design.md`

---

## File Map

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-providers/src/lib.rs` | `dump_llm_io` ローテーション・`resolve_provider_id` 拡張・`LlmResponse.provider_id` 追加 |
| `crates/rustyclaw-storage/src/lib.rs` | `provider_id` カラム migration・`record_usage` 引数追加・`get_usage_summary` 拡張 |
| `crates/rustyclaw-gateway/src/lib.rs` | `char_limit` 変更・`record_usage` 呼び出し更新 |
| `crates/rustyclaw-agent/src/lib.rs` | `record_usage` 呼び出し更新 |
| `crates/rustyclaw-gateway/src/health.rs` | 新 API エンドポイント・ダッシュボード UI 全面刷新 |

---

## Task 1: dump_llm_io — ローテーション保存と自動クリーンアップ

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs:146-183`

- [ ] **Step 1: テストを書く**

`rustyclaw-providers/src/lib.rs` の `mod tests` ブロックに追加：

```rust
#[test]
fn dump_llm_io_writes_dated_file_and_cleans_old_dirs() {
    use std::fs;
    use chrono::{Local, Duration};

    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", tmp.path().to_str().unwrap());

    // 6日前のダミーフォルダを作成（削除対象）
    let old_date = (Local::now() - Duration::days(6)).format("%Y-%m-%d").to_string();
    let old_dir = tmp.path().join("memory/debug/llm/tools").join(&old_date);
    fs::create_dir_all(&old_dir).unwrap();

    let messages = vec![];
    let response = LlmResponse {
        content: "test".into(),
        role: "assistant".into(),
        tool_calls: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        model_used: None,
        provider_id: None,
    };
    dump_llm_io("tools", "test-model", &messages, &response);

    // 今日の日付ディレクトリに JSON が生成されている
    let today = Local::now().format("%Y-%m-%d").to_string();
    let today_dir = tmp.path().join("memory/debug/llm/tools").join(&today);
    let files: Vec<_> = fs::read_dir(&today_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    let filename = files[0].as_ref().unwrap().file_name();
    assert!(filename.to_str().unwrap().ends_with(".json"));

    // 6日前のフォルダは削除されている
    assert!(!old_dir.exists());

    std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-providers dump_llm_io_writes_dated_file 2>&1 | tail -5
```
Expected: FAIL（現状はフラットな `<category>.json` に書くため）

- [ ] **Step 3: `dump_llm_io` を書き換える**

`lib.rs:146-183` を以下に置き換える：

```rust
fn dump_llm_io(
    category: &str,
    model: &str,
    messages: &[Message],
    response: &LlmResponse,
) {
    use chrono::Local;

    let ws_dir = get_workspace_dir();
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H-%M-%S").to_string();

    let category_dir = ws_dir.join("memory").join("debug").join("llm").join(category);
    let date_dir = category_dir.join(&date_str);

    if let Err(e) = std::fs::create_dir_all(&date_dir) {
        tracing::error!("Failed to create llm dump directory {:?}: {}", date_dir, e);
        return;
    }

    // 5日超の日付フォルダを削除
    if let Ok(entries) = std::fs::read_dir(&category_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == date_str {
                continue;
            }
            if let Ok(folder_date) = chrono::NaiveDate::parse_from_str(&name_str, "%Y-%m-%d") {
                let today = now.date_naive();
                if (today - folder_date).num_days() > 5 {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
    }

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

- [ ] **Step 4: テストを通す**

```bash
cargo test -p rustyclaw-providers dump_llm_io_writes_dated_file 2>&1 | tail -5
```
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "feat(providers): rotate llm dumps by date/time with 5-day cleanup"
```

---

## Task 2: LlmResponse に provider_id を追加・resolve_provider_id を拡張

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs:106-119` (resolve_provider_id)
- Modify: `crates/rustyclaw-providers/src/lib.rs:225-233` (LlmResponse struct)
- Modify: `crates/rustyclaw-providers/src/lib.rs:464-480` (complete 非ストリーム)
- Modify: `crates/rustyclaw-providers/src/lib.rs:587-596` (complete_stream 末尾)
- Modify: `crates/rustyclaw-providers/src/lib.rs:767` (GmnProvider::complete)

- [ ] **Step 1: テストを書く**

`mod tests` ブロックに追加：

```rust
#[test]
fn resolve_provider_id_detects_openrouter() {
    let model = LlmModelConfig {
        model_purpose: "default".into(),
        model_provider: "openai".into(),
        model_name: "google/gemma-4-31b-it:free".into(),
        api_key: "key".into(),
        api_base_url: "https://openrouter.ai/api/v1".into(),
        max_tokens: None,
        temperature: None,
    };
    assert_eq!(resolve_provider_id(&model), "openrouter");
}

#[test]
fn resolve_provider_id_detects_cloudflare() {
    let model = LlmModelConfig {
        model_purpose: "default".into(),
        model_provider: "openai".into(),
        model_name: "@cf/qwen/qwen3-30b-a3b-fp8".into(),
        api_key: "key".into(),
        api_base_url: "https://api.cloudflare.com/client/v4/accounts/xxx/ai/v1".into(),
        max_tokens: None,
        temperature: None,
    };
    assert_eq!(resolve_provider_id(&model), "cloudflare");
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-providers resolve_provider_id 2>&1 | tail -5
```
Expected: `detect_openrouter` が FAIL（openrouter 未対応のため）

- [ ] **Step 3: `resolve_provider_id` に openrouter を追加**

`lib.rs:106-119` を以下に置き換える：

```rust
pub fn resolve_provider_id(model: &LlmModelConfig) -> String {
    let base = model.api_base_url.to_lowercase();
    if base.contains("groq.com") {
        "groq".to_string()
    } else if base.contains("cloudflare.com") {
        "cloudflare".to_string()
    } else if base.contains("openrouter.ai") {
        "openrouter".to_string()
    } else if base.contains("openai.com") {
        "openai".to_string()
    } else if base.contains("huggingface.co") {
        "huggingface".to_string()
    } else {
        model.model_provider.clone()
    }
}
```

- [ ] **Step 4: `LlmResponse` に `provider_id` フィールドを追加**

`lib.rs:225-233` の `LlmResponse` 構造体を以下に置き換える：

```rust
pub struct LlmResponse {
    pub content: String,
    pub role: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub model_used: Option<String>,
    pub provider_id: Option<String>,
}
```

- [ ] **Step 5: 非ストリーム `complete()` の `LlmResponse` 構築に `provider_id` を追加**

`lib.rs:464-480` の `let res = LlmResponse { ... }` を以下に更新（`dump_llm_io` 呼び出しの直前）：

```rust
let res = LlmResponse {
    content: choice.message.content.clone().unwrap_or_default(),
    role: choice.message.role.clone(),
    tool_calls: choice.message.tool_calls.clone(),
    prompt_tokens: resp_data.usage.as_ref().map(|u| u.prompt_tokens),
    completion_tokens: resp_data.usage.as_ref().map(|u| u.completion_tokens),
    total_tokens: resp_data.usage.as_ref().map(|u| u.total_tokens),
    model_used: resp_data.model.clone(),
    provider_id: Some(resolve_provider_id(&self.model)),
};
```

- [ ] **Step 6: ストリーム末尾の `LlmResponse` 構築に `provider_id` を追加**

`complete_stream()` 内で `resolved_model_clone` をクローンしている箇所の近くに以下を追加して、closure 内で使用する：

`lib.rs:547` 付近（`let resolved_model_clone = resolved_model.clone();` の次の行）：
```rust
let provider_id_clone = resolve_provider_id(&self.model);
```

`lib.rs:587-595` の `let llm_res = LlmResponse { ... }` を以下に更新：

```rust
let llm_res = LlmResponse {
    content: full_response_content,
    role: "assistant".to_string(),
    tool_calls: None,
    prompt_tokens: None,
    completion_tokens: None,
    total_tokens: None,
    model_used: Some(resolved_model_clone.clone()),
    provider_id: Some(provider_id_clone.clone()),
};
```

- [ ] **Step 7: GmnProvider の `LlmResponse` 構築に `provider_id` を追加**

`lib.rs:767` 付近の `Ok(LlmResponse { ... })` を検索し、`provider_id: Some("gmn".to_string()),` を追加する。

全文検索で他に `LlmResponse {` が存在するか確認：
```bash
grep -n "LlmResponse {" crates/rustyclaw-providers/src/lib.rs
```

見つかった全ての `LlmResponse {}` 構築に `provider_id: None,`（またはプロバイダ固有の値）を追加してコンパイルを通す。

- [ ] **Step 8: コンパイルを確認**

```bash
cargo build -p rustyclaw-providers 2>&1 | grep "^error" | head -10
```
Expected: エラーなし

- [ ] **Step 9: テストを通す**

```bash
cargo test -p rustyclaw-providers resolve_provider_id 2>&1 | tail -5
```
Expected: PASS

- [ ] **Step 10: コミット**

```bash
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "feat(providers): add provider_id to LlmResponse and extend resolve_provider_id for openrouter"
```

---

## Task 3: storage — provider_id カラム追加と by_provider 集計

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs:110-117` (migration)
- Modify: `crates/rustyclaw-storage/src/lib.rs:123-140` (record_usage)
- Modify: `crates/rustyclaw-storage/src/lib.rs:142-176` (get_usage_summary)

- [ ] **Step 1: テストを書く**

`mod tests` の `test_usage_aggregation` テスト（`lib.rs:596`）の下に追加：

```rust
#[test]
fn test_by_provider_aggregation() -> Result<()> {
    let tmp_dir = tempfile::tempdir()?;
    let db = DbManager::new(&tmp_dir.path().join("prov.db"))?;
    db.record_usage("s1", 100, 50, 150, "model-cf", "heartbeat", Some("cloudflare"), 0)?;
    db.record_usage("s2", 200, 80, 280, "model-gr", "discord",   Some("groq"),       0)?;
    db.record_usage("s3",  50, 20,  70, "model-gr", "cli",       Some("groq"),       0)?;
    db.record_usage("s4",  30, 10,  40, "model-old","unknown",   None,               0)?;

    let summary = db.get_usage_summary(None);
    let by_provider = &summary["by_provider"];
    assert_eq!(by_provider["cloudflare"]["runs"], 1);
    assert_eq!(by_provider["cloudflare"]["tokens"], 150);
    assert_eq!(by_provider["groq"]["runs"], 2);
    assert_eq!(by_provider["groq"]["tokens"], 350);
    // None は集計対象外
    assert!(by_provider.get("").is_none());
    Ok(())
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-storage test_by_provider 2>&1 | tail -5
```
Expected: コンパイルエラー（`record_usage` のシグネチャ不一致）

- [ ] **Step 3: migration に `provider_id` カラムを追加**

`lib.rs:110-117` の migration ループを以下に更新：

```rust
for stmt in [
    "ALTER TABLE usage ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE usage ADD COLUMN model TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE usage ADD COLUMN trigger_type TEXT NOT NULL DEFAULT 'unknown'",
    "ALTER TABLE usage ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE usage ADD COLUMN provider_id TEXT",
] {
    let _ = self.conn.execute(stmt, []);
}
```

- [ ] **Step 4: `record_usage` に `provider_id` 引数を追加**

`lib.rs:123-140` を以下に置き換える：

```rust
pub fn record_usage(
    &self,
    session_id: &str,
    prompt: u32,
    completion: u32,
    total: u32,
    model: &str,
    trigger_type: &str,
    provider_id: Option<&str>,
    duration_ms: u64,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    self.conn.execute(
        "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, provider_id, duration_ms, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![session_id, prompt, completion, total, model, trigger_type, provider_id, duration_ms as i64, now],
    )
    .context("Failed to record usage in SQLite")?;
    Ok(())
}
```

- [ ] **Step 5: `get_usage_summary` に `by_provider` を追加**

`lib.rs:142-176` の `get_usage_summary` メソッドの `serde_json::json!({...})` 返却部分を以下に置き換える：

```rust
let mut by_provider = serde_json::Map::new();
if let Ok(mut stmt) = self.conn.prepare(
    &format!("SELECT provider_id, COUNT(*), COALESCE(SUM(total_tokens),0) FROM usage WHERE provider_id IS NOT NULL {} GROUP BY provider_id ORDER BY SUM(total_tokens) DESC",
        if since.is_some() { "AND created_at >= ?1" } else { "" })
) {
    if let Ok(rows) = stmt.query_map(params.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    }) {
        for row in rows.flatten() {
            by_provider.insert(row.0, serde_json::json!({ "runs": row.1, "tokens": row.2 }));
        }
    }
}

serde_json::json!({
    "total_runs": total.0,
    "total_input_tokens": total.1,
    "total_completion_tokens": total.2,
    "total_tokens": total.3,
    "by_model": by_model,
    "by_provider": by_provider,
})
```

- [ ] **Step 6: 既存テストの `record_usage` 呼び出しを修正**

`lib.rs` 内の既存テスト2か所：

```rust
// lib.rs:548
db.record_usage("session-1", 100, 50, 150, "test-model", "cli", None, 0)?;

// lib.rs:600-601
db.record_usage("cron:heartbeat", 100, 50, 150, "model-a", "heartbeat", Some("groq"), 0)?;
db.record_usage("discord-1",      200, 80, 280, "model-a", "discord",   Some("groq"), 0)?;
```

- [ ] **Step 7: テストを通す**

```bash
cargo test -p rustyclaw-storage 2>&1 | tail -10
```
Expected: 全テスト PASS

- [ ] **Step 8: コミット**

```bash
git add crates/rustyclaw-storage/src/lib.rs
git commit -m "feat(storage): add provider_id column and by_provider aggregation to usage stats"
```

---

## Task 4: record_usage 呼び出し元の更新

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs:354-367` (1か所目)
- Modify: `crates/rustyclaw-gateway/src/lib.rs:532-544` (2か所目)
- Modify: `crates/rustyclaw-agent/src/lib.rs:374-383`

- [ ] **Step 1: gateway/lib.rs の2か所を修正**

`lib.rs:359` の `record_usage` 呼び出しを以下に更新：

```rust
let _ = db.record_usage(
    &session_id,
    response.prompt_tokens.unwrap_or(0),
    response.completion_tokens.unwrap_or(0),
    response.total_tokens.unwrap_or(0),
    response.model_used.as_deref().unwrap_or(""),
    trigger,
    response.provider_id.as_deref(),
    0,
);
```

`lib.rs:537` の2か所目も同様に更新（同じ内容）。

- [ ] **Step 2: agent/lib.rs の `record_aux_usage` を修正**

`lib.rs:375` の `record_usage` 呼び出しを以下に更新：

```rust
let _ = db.record_usage(
    session_id,
    response.prompt_tokens.unwrap_or(0),
    response.completion_tokens.unwrap_or(0),
    response.total_tokens.unwrap_or(0),
    response.model_used.as_deref().unwrap_or(""),
    trigger,
    response.provider_id.as_deref(),
    0,
);
```

- [ ] **Step 3: ビルド確認**

```bash
cargo build 2>&1 | grep "^error" | head -10
```
Expected: エラーなし

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/lib.rs crates/rustyclaw-agent/src/lib.rs
git commit -m "feat: propagate provider_id through record_usage call sites"
```

---

## Task 5: Gateway char_limit 拡張

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs:421-425`

- [ ] **Step 1: `char_limit` を 40 → 80 に変更**

`lib.rs:421` の以下の行：
```rust
let char_limit = 40;
```
を以下に変更：
```rust
let char_limit = 80;
```

- [ ] **Step 2: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -5
```

- [ ] **Step 3: コミット**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(gateway): extend lane description preview from 40 to 80 chars"
```

---

## Task 6: 新規 API エンドポイント — llm/dates・llm/times・concurrency 刷新

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:120-202`

- [ ] **Step 1: `/api/llm/dates` と `/api/llm/times` を追加**

`health.rs:120` の `GET /api/llm/io` ハンドラの直前に以下を挿入：

```rust
} else if request.starts_with("GET /api/llm/dates") {
    let cat = extract_query_param(&request, "cat").unwrap_or_else(|| "tools".to_string());
    let llm_dir = workspace_path_clone.join("memory").join("debug").join("llm").join(&cat);
    let mut dates: Vec<String> = std::fs::read_dir(&llm_dir)
        .map(|rd| rd.flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| chrono::NaiveDate::parse_from_str(n, "%Y-%m-%d").is_ok())
            .collect())
        .unwrap_or_default();
    dates.sort_unstable_by(|a, b| b.cmp(a));
    ("200 OK".to_string(), serde_json::to_string(&dates).unwrap_or_else(|_| "[]".to_string()), "application/json; charset=utf-8")

} else if request.starts_with("GET /api/llm/times") {
    let cat  = extract_query_param(&request, "cat").unwrap_or_else(|| "tools".to_string());
    let date = extract_query_param(&request, "date").unwrap_or_default();
    let time_dir = workspace_path_clone.join("memory").join("debug").join("llm").join(&cat).join(&date);
    let mut times: Vec<String> = std::fs::read_dir(&time_dir)
        .map(|rd| rd.flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().replace(".json", ""))
            .collect())
        .unwrap_or_default();
    times.sort_unstable_by(|a, b| b.cmp(a));
    ("200 OK".to_string(), serde_json::to_string(&times).unwrap_or_else(|_| "[]".to_string()), "application/json; charset=utf-8")
```

- [ ] **Step 2: `extract_query_param` ヘルパーを追加**

`health.rs` の `extract_since_param` 関数の近くに追加（ファイル末尾付近の関数群）：

```rust
fn extract_query_param(request: &str, key: &str) -> Option<String> {
    let query_start = request.find('?')?;
    let query_end   = request[query_start..].find(' ').map(|i| query_start + i).unwrap_or(request.len());
    let query = &request[query_start + 1..query_end];
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}
```

- [ ] **Step 3: `/api/llm/io` を日時パラメータ対応に更新**

`health.rs:120` 付近の `GET /api/llm/io` ハンドラを以下に置き換える：

```rust
} else if request.starts_with("GET /api/llm/io") {
    let cat  = extract_query_param(&request, "cat").unwrap_or_else(|| "tools".to_string());
    let date = extract_query_param(&request, "date");
    let time = extract_query_param(&request, "time");

    let llm_cat_dir = workspace_path_clone.join("memory").join("debug").join("llm").join(&cat);

    let file_path = if let (Some(d), Some(t)) = (date, time) {
        Some(llm_cat_dir.join(&d).join(format!("{}.json", t)))
    } else {
        // latest: 最新の日付ディレクトリ → 最新の時刻ファイル
        let latest_date = std::fs::read_dir(&llm_cat_dir).ok()
            .and_then(|rd| {
                let mut dates: Vec<_> = rd.flatten()
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| chrono::NaiveDate::parse_from_str(n, "%Y-%m-%d").is_ok())
                    .collect();
                dates.sort_unstable_by(|a, b| b.cmp(a));
                dates.into_iter().next()
            });
        latest_date.and_then(|d| {
            let date_dir = llm_cat_dir.join(&d);
            let mut times: Vec<_> = std::fs::read_dir(&date_dir).ok()?.flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            times.sort_unstable_by(|a, b| b.cmp(a));
            times.into_iter().next().map(|t| date_dir.join(t))
        })
    };

    match file_path.and_then(|p| std::fs::read_to_string(&p).ok()) {
        Some(content) => ("200 OK".to_string(), content, "application/json; charset=utf-8"),
        None => ("404 Not Found".to_string(), "{}".to_string(), "application/json; charset=utf-8"),
    }
```

- [ ] **Step 4: `/api/concurrency` を per-provider 対応に更新**

`health.rs:183-202` の `/api/concurrency` ハンドラを以下に置き換える：

```rust
} else if request.starts_with("GET /api/concurrency") {
    let providers_map = {
        let mut m = serde_json::Map::new();
        for p in ["cloudflare", "groq", "openrouter", "gmn"] {
            let secs = rustyclaw_providers::provider_cooldown_remaining(p)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            m.insert(p.to_string(), serde_json::json!(secs));
        }
        m
    };
    let json = serde_json::json!({
        "capacity": gmn_cap_clone,
        "providers": providers_map,
    });
    ("200 OK".to_string(), json.to_string(), "application/json; charset=utf-8")
```

- [ ] **Step 5: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -10
```
Expected: エラーなし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): add llm/dates, llm/times APIs and refactor concurrency to per-provider"
```

---

## Task 7: LANE QUEUE UI — 左右分割バッジ表示

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs` (CSS + HTML + JS)

- [ ] **Step 1: CSS にレーンバッジスタイルを追加**

`health.rs` の CSS セクション（`.stats-bottom` などがある箇所）に以下を追加：

```css
.lanes-split{display:grid;grid-template-columns:1fr 1fr;gap:0;height:100%}
.lanes-left{display:flex;flex-direction:column;gap:3px;padding:6px 8px;border-right:1px solid rgba(255,255,255,0.07)}
.lanes-right{display:flex;flex-direction:column;gap:2px;padding:6px 8px;overflow-y:auto}
.lane-badge-row{display:flex;align-items:center;gap:6px;font-size:11px;font-family:'Fira Code',monospace;white-space:nowrap}
.lane-dot-run{width:6px;height:6px;border-radius:50%;background:var(--green);flex-shrink:0;box-shadow:0 0 4px var(--green)}
.lane-badge{display:inline-block;padding:1px 5px;border-radius:3px;font-size:10px;font-weight:700;font-family:'Fira Code',monospace}
.lane-badge-idle{color:var(--muted);font-family:'Fira Code',monospace;font-size:10px}
.lane-elapsed{color:var(--muted);font-size:10px;margin-left:auto}
```

- [ ] **Step 2: `updateQueue()` JS を書き換える**

`health.rs:749-828` の `updateQueue` 関数を以下に置き換える：

```javascript
const SERVICE_BADGES = [
  { prefix: 'cron:heartbeat',      label: 'HEARTBEAT', color: '#bf00ff' },
  { prefix: 'cron:topic-patrol',   label: 'PATROL',    color: '#ff8c00' },
  { prefix: 'cron:daily-briefing', label: 'BRIEFING',  color: '#4488ff' },
  { prefix: 'cron:vitals',         label: 'VITALS',    color: '#00ff9f' },
  { prefix: 'cron:karakeep',       label: 'KARAKEEP',  color: '#ffe066' },
  { prefix: 'cron:daily-summary',  label: 'SUMMARY',   color: '#00e5ff' },
  { prefix: 'discord-',            label: 'DISCORD',   color: '#7b68ee' },
  { prefix: 'http-dashboard',      label: 'DASHBOARD', color: '#00d4ff' },
  { prefix: 'cli-',                label: 'CLI',       color: '#cccccc' },
];
function serviceBadge(sessionId) {
  const s = SERVICE_BADGES.find(b => sessionId.startsWith(b.prefix));
  return s || { label: 'UNKNOWN', color: '#888888' };
}
function badgeHtml(s, opts={}) {
  const style = `background:${s.color}22;color:${s.color};border:1px solid ${s.color}55`;
  return `<span class="lane-badge" style="${style}">${escapeHtml(s.label)}</span>`;
}
async function updateQueue(){
  try{
    const[rq,rs]=await Promise.all([fetch('/api/queue'),fetch('/api/schedule')]);
    if(!rq.ok)return;
    const items=await rq.json();
    const sched=rs.ok?await rs.json():[];
    document.getElementById('queue-ts').textContent='↻ '+now();
    const panel=document.getElementById('queuePanel');

    const executing=items.filter(i=>i.status==='Executing');
    const waiting=items.filter(i=>i.status!=='Executing');

    // 左列: LANES（capacity は updateConcurrency がキャッシュした値を使用）
    let lanesHtml='';
    const capacity=cachedCapacity;
    for(let i=0;i<capacity;i++){
      if(i<executing.length){
        const task=executing[i];
        const elapsed=Math.floor((Date.now()-task.enqueued_at_ms)/1000);
        const s=serviceBadge(task.session_id);
        lanesHtml+=`<div class="lane-badge-row"><span class="lane-dot-run"></span>${badgeHtml(s)}<span class="lane-elapsed">${elapsed}s</span></div>`;
      }else{
        lanesHtml+=`<div class="lane-badge-row"><span class="lane-badge-idle">[  ────  ]</span><span class="lane-elapsed">--</span></div>`;
      }
    }

    // 右列: PENDING + SCHEDULED
    let qHtml='';
    waiting.forEach(item=>{
      const cls=item.status==='Waiting'?'pill-wait':'pill-cool';
      const lbl=item.status==='Waiting'?'WAIT':'COOL';
      const elapsed=Math.floor((Date.now()-item.enqueued_at_ms)/1000);
      const s=serviceBadge(item.session_id);
      qHtml+=`<div class="q-item"><span class="q-pill ${cls}">${lbl}</span>${badgeHtml(s)}<span class="q-time">${elapsed}s</span></div>`;
      if(item.status==='Cooldown'&&item.cooldown_left_secs>0){const pct=Math.min(100,(item.cooldown_left_secs/60)*100);qHtml+=`<div class="cool-bar"><div class="cool-fill" style="width:${pct}%"></div></div>`}
    });
    sched.forEach(s=>{
      const left=Math.max(0,s.next_run_epoch-Math.floor(Date.now()/1000));
      const h=Math.floor(left/3600),m=Math.floor((left%3600)/60);
      const eta=h>0?`${h}h${m}m`:m>0?`${m}m`:`<1m`;
      const svc=serviceBadge(s.name);
      qHtml+=`<div class="q-item"><span class="q-pill pill-wait">SCHED</span>${badgeHtml(svc)}<span class="q-time">in ${eta}</span></div>`;
    });
    if(!qHtml)qHtml='<div style="color:var(--muted);text-align:center;padding:10px;font-size:10px;">待機なし</div>';

    const scrollPos=panel.querySelector('.lanes-right')?.scrollTop??0;
    panel.innerHTML=`<div class="lanes-split"><div class="lanes-left">${lanesHtml}</div><div class="lanes-right">${qHtml}</div></div>`;
    const newRight=panel.querySelector('.lanes-right');
    if(newRight)newRight.scrollTop=scrollPos;
  }catch{}
}
```

- [ ] **Step 3: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -5
```

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): refactor lane queue to split badge layout with service colors"
```

---

## Task 8: CONCURRENCY パネル — per-provider クールダウン表示

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs` (HTML + CSS + JS)

- [ ] **Step 1: CONCURRENCY パネルの HTML を置き換える**

`health.rs:647-656` の `<div class="panel concur">` 内部を以下に置き換える：

```html
<div class="panel concur">
  <div class="panel-head"><span class="panel-label">◈ PROVIDER COOLDOWNS</span><span class="rts" id="concur-ts">—</span></div>
  <div class="panel-body" id="concurPanel" style="padding:6px 8px;display:flex;flex-direction:column;gap:4px;"></div>
</div>
```

- [ ] **Step 2: プロバイダカラー定義と `updateConcurrency` を書き換える**

CSS に追加：
```css
.prov-row{display:flex;align-items:center;gap:6px;font-size:10px;font-family:'Fira Code',monospace}
.prov-name{width:80px;color:rgba(180,210,230,0.7)}
.prov-bar-wrap{flex:1;height:4px;background:rgba(255,255,255,0.08);border-radius:2px;overflow:hidden}
.prov-bar-fill{height:100%;border-radius:2px;transition:width 0.3s}
.prov-secs{width:44px;text-align:right}
```

`updateConcurrency` 関数（`health.rs:830-845`）を以下に置き換える：

```javascript
const PROVIDER_COLORS = {
  cloudflare: '#f48120',
  groq:       '#f55036',
  openrouter: '#6e45e2',
  gmn:        '#4285f4',
};
let cachedCapacity = 4;
async function updateConcurrency(){
  try{
    const r=await fetch('/api/concurrency');if(!r.ok)return;
    const d=await r.json();
    document.getElementById('concur-ts').textContent='↻ '+now();
    cachedCapacity = d.capacity ?? 4;
    const panel=document.getElementById('concurPanel');
    const providers=d.providers??{};
    panel.innerHTML=Object.entries(providers).map(([name,secs])=>{
      const pct=Math.min(100,(secs/60)*100).toFixed(1);
      const color=PROVIDER_COLORS[name]??'#888';
      const secsLabel=secs>0?secs.toFixed(1)+'s':'<span style="color:var(--muted)">none</span>';
      return `<div class="prov-row">
        <span class="prov-name">${escapeHtml(name)}</span>
        <div class="prov-bar-wrap"><div class="prov-bar-fill" style="width:${pct}%;background:${color}"></div></div>
        <span class="prov-secs">${secsLabel}</span>
      </div>`;
    }).join('');
  }catch{}
}
```

- [ ] **Step 3: `slotRow`・旧テキスト要素の参照をHTMLから削除済みか確認**

```bash
grep -n "slotRow\|cActive\|cDepth\|cCool\|cGlobal" crates/rustyclaw-gateway/src/health.rs
```
残存していれば削除する。

- [ ] **Step 4: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -5
```

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): replace concurrency panel with per-provider cooldown bars"
```

---

## Task 9: LLM Inspector — ダブルドロップダウンと truncation 削除

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs` (HTML + CSS + JS)

- [ ] **Step 1: ドロップダウン HTML を llmTabs の下に追加**

`health.rs:675` の `<div ... id="llmTabs"></div>` の直後に追加：

```html
<div style="display:flex;gap:6px;padding:2px 0;align-items:center;">
  <select id="llmDateSelect" style="background:#0a1628;color:#7bafd4;border:1px solid rgba(0,212,255,.25);border-radius:3px;font-family:'Fira Code',monospace;font-size:10px;padding:2px 4px;" onchange="onLlmDateChange()">
    <option value="">-- date --</option>
  </select>
  <select id="llmTimeSelect" style="background:#0a1628;color:#7bafd4;border:1px solid rgba(0,212,255,.25);border-radius:3px;font-family:'Fira Code',monospace;font-size:10px;padding:2px 4px;" onchange="updateInspector()">
    <option value="">-- time --</option>
  </select>
</div>
```

- [ ] **Step 2: `setLlmCategory` と `updateInspector` を書き換える**

`setLlmCategory` 関数（`health.rs:867-872`）を以下に置き換える：

```javascript
async function setLlmCategory(cat, btn) {
  activeLlmCategory = cat;
  document.querySelectorAll('.llm-tab').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
  await populateLlmDates();
}
async function populateLlmDates() {
  const dateSelect = document.getElementById('llmDateSelect');
  const timeSelect = document.getElementById('llmTimeSelect');
  const r = await fetch('/api/llm/dates?cat=' + activeLlmCategory).catch(()=>null);
  const dates = r?.ok ? await r.json() : [];
  dateSelect.innerHTML = '<option value="">-- date --</option>' +
    dates.map(d=>`<option value="${escapeHtml(d)}">${escapeHtml(d)}</option>`).join('');
  if (dates.length > 0) {
    dateSelect.value = dates[0];
    await populateLlmTimes();
  } else {
    timeSelect.innerHTML = '<option value="">-- time --</option>';
    updateInspector();
  }
}
async function populateLlmTimes() {
  const dateSelect = document.getElementById('llmDateSelect');
  const timeSelect = document.getElementById('llmTimeSelect');
  const date = dateSelect.value;
  if (!date) { timeSelect.innerHTML = '<option value="">-- time --</option>'; return; }
  const r = await fetch(`/api/llm/times?cat=${activeLlmCategory}&date=${date}`).catch(()=>null);
  const times = r?.ok ? await r.json() : [];
  timeSelect.innerHTML = '<option value="">-- time --</option>' +
    times.map(t=>`<option value="${escapeHtml(t)}">${escapeHtml(t)}</option>`).join('');
  if (times.length > 0) { timeSelect.value = times[0]; }
  updateInspector();
}
async function onLlmDateChange() {
  await populateLlmTimes();
}
```

`updateInspector` 関数（`health.rs:873-901`）を以下に置き換える：

```javascript
async function updateInspector(){
  try{
    const date = document.getElementById('llmDateSelect')?.value ?? '';
    const time = document.getElementById('llmTimeSelect')?.value ?? '';
    let url = '/api/llm/io?cat=' + activeLlmCategory;
    if (date && time) url += `&date=${date}&time=${time}`;
    const r = await fetch(url);
    document.getElementById('inspector-ts').textContent = '↻ ' + now();
    const reqPanel = document.getElementById('reqPanel');
    const resPanel = document.getElementById('resPanel');
    if (!r.ok) {
      if (reqPanel) reqPanel.textContent = '(no logs yet)';
      if (resPanel) resPanel.textContent = '(no logs yet)';
      return;
    }
    const d = await r.json();
    if (reqPanel) reqPanel.textContent = d?.request ? JSON.stringify(d.request, null, 2) : '(no request logged)';
    if (resPanel) resPanel.textContent = d?.response ? JSON.stringify(d.response, null, 2) : '(no response logged)';
  } catch(e) {
    console.error("Inspector fetch error:", e);
  }
}
```

- [ ] **Step 3: 初期化で `populateLlmDates` を呼ぶように変更**

`health.rs:1004` の初期化行を探し、`initLlmTabs()` の後に `populateLlmDates();` を追加する（または `initLlmTabs` 内で呼ぶ）：

```javascript
function initLlmTabs() {
  const container = document.getElementById('llmTabs');
  if (!container) return;
  container.innerHTML = llmCategories.map(cat =>
    `<button class="llm-tab${cat===activeLlmCategory?' active':''}" onclick="setLlmCategory('${cat}', this)">${cat.toUpperCase()}</button>`
  ).join('');
  populateLlmDates();
}
```

- [ ] **Step 4: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -5
```

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): add date/time dropdowns to llm inspector, remove truncation"
```

---

## Task 10: APP LOG — サービス別着色バッジ

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:902-917`

- [ ] **Step 1: `updateLog` のログ行パーサを拡張する**

`health.rs:902-917` の `updateLog` 関数内、`el.innerHTML=lines.map(line=>{` から始まるマップ処理を以下に置き換える：

```javascript
const LOG_BADGES = [
  { pattern: /HeartBeatService/,         label: 'HEARTBEAT', color: '#bf00ff' },
  { pattern: /rustyclaw_gateway/,        label: 'GATEWAY',   color: '#00d4ff' },
  { pattern: /PatrolService/,            label: 'PATROL',    color: '#ff8c00' },
  { pattern: /BriefingService/,          label: 'BRIEFING',  color: '#4488ff' },
  { pattern: /VitalsService/,            label: 'VITALS',    color: '#00ff9f' },
  { pattern: /DiscordService/,           label: 'DISCORD',   color: '#7b68ee' },
];
el.innerHTML=lines.map(line=>{
  const lvl=line.includes(' INFO ')?'info':line.includes(' WARN ')?'warn':line.includes(' ERROR ')?'error':'info';
  const tsM=line.match(/\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2})/);
  const ts=tsM?tsM[1]:'';
  let msg=line.replace(/^\S+\s+(INFO|WARN|ERROR)\s+\S+:\s*/,'').trim();
  const svcBadge=LOG_BADGES.find(b=>b.pattern.test(line));
  const badgeSpan=svcBadge
    ?`<strong style="color:${svcBadge.color};background:${svcBadge.color}22;border:1px solid ${svcBadge.color}55;padding:0 4px;border-radius:3px;font-size:9px;margin-right:4px;">[${svcBadge.label}]</strong>`
    :'';
  return`<div class="log-line"><span class="log-ts">${ts}</span><span class="log-lv ${lvl}">${lvl.toUpperCase()}</span>${badgeSpan}<span class="log-msg">${escapeHtml(msg)}</span></div>`;
}).join('');
```

- [ ] **Step 2: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -5
```

- [ ] **Step 3: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): add service-colored badges to app log lines"
```

---

## Task 11: Stats — BY PROVIDER パネル追加

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs` (CSS + HTML + JS)

- [ ] **Step 1: `stats-bottom` を3列に変更**

`health.rs:594` の CSS セクション：
```css
.stats-bottom{display:grid;grid-template-columns:1fr 1fr;gap:8px}
```
を以下に変更：
```css
.stats-bottom{display:grid;grid-template-columns:1fr 1fr 1fr;gap:8px}
```

- [ ] **Step 2: BY PROVIDER パネルの HTML を追加**

`health.rs:734` の `stats-bottom` div 内に3列目を追加：

```html
<div class="breakdown" style="border-color:rgba(255,165,0,.2)">
  <div class="bd-title" style="color:#f48120">BY PROVIDER</div>
  <div id="providerBreakdown"><div style="color:var(--muted);font-size:11px;padding:8px 0">No data yet</div></div>
</div>
```

- [ ] **Step 3: `loadStats` の `/api/usage/summary` 処理に `by_provider` 描画を追加**

`health.rs` の `loadStats` 関数内、`by_model` の処理に続けて以下を追加：

```javascript
// BY PROVIDER
const byProvider = data.by_provider ?? {};
const providerEntries = Object.entries(byProvider);
const maxProvTokens = Math.max(1, ...providerEntries.map(([,v])=>v.tokens));
const PROV_COLORS = { cloudflare:'#f48120', groq:'#f55036', openrouter:'#6e45e2', gmn:'#4285f4' };
document.getElementById('providerBreakdown').innerHTML = providerEntries.length
  ? providerEntries.map(([name, v]) => {
      const pct = ((v.tokens / maxProvTokens) * 100).toFixed(1);
      const color = PROV_COLORS[name] ?? '#888';
      return `<div style="margin-bottom:6px">
        <div style="display:flex;justify-content:space-between;font-size:10px;color:rgba(180,210,230,0.7);margin-bottom:2px">
          <span>${escapeHtml(name)}</span>
          <span>${fmtK(v.tokens)} tok / ${v.runs} runs</span>
        </div>
        <div style="height:4px;background:rgba(255,255,255,0.08);border-radius:2px">
          <div style="height:100%;width:${pct}%;background:${color};border-radius:2px"></div>
        </div>
      </div>`;
    }).join('')
  : '<div style="color:var(--muted);font-size:11px;padding:8px 0">No data yet</div>';
```

- [ ] **Step 4: ビルド確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -5
```

- [ ] **Step 5: テスト全通し**

```bash
cargo test 2>&1 | tail -15
```
Expected: 全テスト PASS

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-gateway/src/health.rs
git commit -m "feat(dashboard): add by-provider breakdown panel to stats page"
```

---

## Task 12: rp1 デプロイ & 手動確認

- [ ] **Step 1: rp1 にデプロイ**

```bash
bash deploy.sh
```

- [ ] **Step 2: ダッシュボード動作確認チェックリスト**

| 確認項目 | 期待値 |
|---|---|
| LANE QUEUE に `[HEARTBEAT]` 等の着色バッジが表示 | サービスに対応した色 |
| PROVIDER COOLDOWNS に cloudflare/groq/openrouter/gmn が表示 | クールダウンなし → `none` |
| LLM INSPECTOR の Date/Time ドロップダウンが表示 | カテゴリ切替で再 populate |
| 過去時刻を選択するとそのペイロードが表示 | 日時に対応した JSON |
| APP LOG に `[HEARTBEAT]`・`[GATEWAY]` バッジが紫・シアンで表示 | 正しい着色 |
| Stats → BY PROVIDER パネルが表示 | プロバイダ別バー |

- [ ] **Step 3: Heartbeat を手動実行して by_provider が集計されることを確認**

heartbeat 実行後に Stats を開き、BY PROVIDER パネルに数値が現れることを確認。

---
