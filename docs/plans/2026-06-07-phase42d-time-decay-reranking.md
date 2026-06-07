# Phase 42-D 時間減衰リランキング Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ローカル embedding の RAG 検索結果に指数減衰係数を適用し、直近チャンクを優先するリランキングを全 RAG パス（Heartbeat・Chat・Discord）に導入する。

**Architecture:** `EmbeddingConfig` に `time_decay_half_life_days: Option<f64>` を追加（Task 1）。Storage 層に `search_similar_with_decay` 関数を追加（Task 2）。Agent の `retrieve_rag_context_local` で `time_decay_half_life_days` が設定されていれば新関数を使う分岐を追加（Task 3）。未設定時は既存挙動を維持（後方互換）。

**Tech Stack:** Rust 2024 Edition、`rustyclaw-config`・`rustyclaw-storage`・`rustyclaw-agent`、`chrono`・`rusqlite`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | 修正 | `EmbeddingConfig` に `time_decay_half_life_days: Option<f64>` を追加、テスト追加 |
| `crates/rustyclaw-storage/src/lib.rs` | 修正 | `search_similar_with_decay` 関数を追加、テスト追加 |
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | `retrieve_rag_context_local` に decay 分岐を追加 |

---

## Task 1: Config — `time_decay_half_life_days` フィールドの追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-config/src/lib.rs` の既存テスト末尾（`test_embedding_config_heartbeat_top_k` などの後）に追加する:

```rust
#[test]
fn test_embedding_config_time_decay_half_life_days_default_none() {
    let cfg: EmbeddingConfig = serde_json::from_str(r#"{}"#).unwrap();
    assert!(
        cfg.time_decay_half_life_days.is_none(),
        "time_decay_half_life_days default must be None"
    );
}

#[test]
fn test_embedding_config_time_decay_half_life_days_set() {
    let cfg: EmbeddingConfig =
        serde_json::from_str(r#"{"time_decay_half_life_days": 30.0}"#).unwrap();
    assert!(
        (cfg.time_decay_half_life_days.unwrap() - 30.0).abs() < 1e-9
    );
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-config -- test_embedding_config_time_decay 2>&1 | tail -10
```

期待: コンパイルエラー（フィールド未定義）

- [ ] **Step 3: フィールドを追加する**

`crates/rustyclaw-config/src/lib.rs` の `EmbeddingConfig` 構造体の `discord_top_k` フィールド（line 135付近）の直後に追加する:

変更前:
```rust
    /// Discord チャット専用の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub discord_top_k: Option<usize>,
}
```

変更後:
```rust
    /// Discord チャット専用の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub discord_top_k: Option<usize>,
    /// RAG 検索結果の時間減衰 half-life（日数）。
    /// 省略時は減衰なし（既存挙動を維持）。
    /// 例: 30.0 → 30日で combined_score が半減。
    #[serde(default)]
    pub time_decay_half_life_days: Option<f64>,
}
```

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-config -- test_embedding_config_time_decay 2>&1 | tail -10
```

期待:
```
test test_embedding_config_time_decay_half_life_days_default_none ... ok
test test_embedding_config_time_decay_half_life_days_set ... ok
```

- [ ] **Step 5: Clippy を通す**

```bash
cargo clippy -p rustyclaw-config --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): Phase 42-D EmbeddingConfig に time_decay_half_life_days を追加"
```

---

## Task 2: Storage — `search_similar_with_decay` 関数の追加

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`

- [ ] **Step 1: テストを書く**

`crates/rustyclaw-storage/src/lib.rs` の `mod tests` ブロック内、`test_search_similar_with_source_threshold_filter` テストの後に追加する:

```rust
#[test]
fn test_search_similar_with_decay_newer_ranks_higher() {
    let db = DbManager::new(":memory:").unwrap();
    let v = vec![1.0f32, 0.0, 0.0];
    let blob = DbManager::serialize_embedding(&v);

    // 新しいチャンク (recent)
    db.conn
        .execute(
            "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            rusqlite::params!["new", "memory", "new text", blob.clone(), "2026-06-06T00:00:00+00:00"],
        )
        .unwrap();

    // 古いチャンク (old)
    db.conn
        .execute(
            "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            rusqlite::params!["old", "memory", "old text", blob, "2020-01-01T00:00:00+00:00"],
        )
        .unwrap();

    // 同じクエリベクトルで検索。half_life = 30日 → 古い方は大幅ペナルティ
    let results = db
        .search_similar_with_decay(&v, 2, 0.5, 30.0)
        .unwrap();

    assert_eq!(results.len(), 2);
    // new text が先頭
    assert_eq!(results[0].1, "new text", "newer item should rank first");
    // combined_score は降順
    assert!(results[0].2 >= results[1].2, "scores should be descending");
}

#[test]
fn test_search_similar_with_decay_threshold_filters() {
    let db = DbManager::new(":memory:").unwrap();
    let v1 = vec![1.0f32, 0.0];
    let v2 = vec![0.0f32, 1.0]; // 直交 → cosine = 0.0 → threshold 未満
    let blob1 = DbManager::serialize_embedding(&v1);
    let blob2 = DbManager::serialize_embedding(&v2);

    db.conn
        .execute(
            "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            rusqlite::params!["r", "memory", "relevant", blob1, "2026-06-06T00:00:00+00:00"],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            rusqlite::params!["u", "memory", "unrelated", blob2, "2026-06-06T00:00:00+00:00"],
        )
        .unwrap();

    let results = db
        .search_similar_with_decay(&v1, 5, 0.5, 30.0)
        .unwrap();

    // 直交ベクトルは cosine=0.0 < threshold 0.5 → 除外
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, "relevant");
}

#[test]
fn test_search_similar_with_decay_invalid_created_at_does_not_panic() {
    let db = DbManager::new(":memory:").unwrap();
    let v = vec![1.0f32, 0.0];
    let blob = DbManager::serialize_embedding(&v);

    db.conn
        .execute(
            "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            rusqlite::params!["x", "memory", "text", blob, "not-a-valid-date"],
        )
        .unwrap();

    // parse 失敗 → fail-open（age_days = 0.0、decay なし）
    let results = db
        .search_similar_with_decay(&v, 5, 0.5, 30.0)
        .unwrap();

    assert_eq!(results.len(), 1, "item with invalid created_at still returned");
}
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-storage -- test_search_similar_with_decay 2>&1 | tail -10
```

期待: コンパイルエラー（関数未定義）

- [ ] **Step 3: `search_similar_with_decay` を実装する**

`crates/rustyclaw-storage/src/lib.rs` の `search_similar_with_source` 関数（line 319-352付近）の直後、`cosine_similarity` 関数の前に追加する:

```rust
/// コサイン類似度に時間減衰係数を乗じた combined_score でリランキングする。
/// threshold は cosine_sim に適用（relevance ゲート）。
/// combined_score = cosine_sim * 0.5^(age_days / half_life_days)
/// created_at の parse 失敗は fail-open（age_days = 0.0 → decay なし）。
pub fn search_similar_with_decay(
    &self,
    query_vec: &[f32],
    top_k: usize,
    threshold: f32,
    half_life_days: f64,
) -> Result<Vec<(String, String, f64)>> {
    let now_utc = chrono::Utc::now();
    let mut stmt = self
        .conn
        .prepare(
            "SELECT source, text_content, embedding, created_at FROM memory_embeddings",
        )
        .context("search_similar_with_decay: prepare failed")?;
    let rows = stmt
        .query_map([], |row| {
            let source: String = row.get(0)?;
            let text: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            let created_at: String = row.get(3)?;
            Ok((source, text, blob, created_at))
        })
        .context("search_similar_with_decay: query failed")?;

    let mut scored: Vec<(String, String, f64)> = rows
        .filter_map(|r| r.ok())
        .filter_map(|(source, text, blob, created_at_str)| {
            let emb = Self::deserialize_embedding(&blob);
            let sim = Self::cosine_similarity(query_vec, &emb) as f64;
            if sim < threshold as f64 {
                return None;
            }
            let age_days = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| {
                    let secs = (now_utc - dt.to_utc()).num_seconds().max(0) as f64;
                    secs / 86400.0
                })
                .unwrap_or(0.0);
            let factor = 0.5_f64.powf(age_days / half_life_days);
            let combined = sim * factor;
            Some((source, text, combined))
        })
        .collect();

    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    Ok(scored)
}
```

- [ ] **Step 4: テストが通ることを確認する**

```bash
cargo test -p rustyclaw-storage -- test_search_similar_with_decay 2>&1 | tail -10
```

期待:
```
test tests::test_search_similar_with_decay_invalid_created_at_does_not_panic ... ok
test tests::test_search_similar_with_decay_newer_ranks_higher ... ok
test tests::test_search_similar_with_decay_threshold_filters ... ok
```

- [ ] **Step 5: 全テストを実行する**

```bash
cargo test -p rustyclaw-storage 2>&1 | grep -E "^(test result|FAILED)"
```

期待: `test result: ok.`

- [ ] **Step 6: Clippy を通す**

```bash
cargo clippy -p rustyclaw-storage --all-targets 2>&1 | grep "^error"
```

期待: 出力なし

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-storage/src/lib.rs
git commit -m "feat(storage): Phase 42-D search_similar_with_decay 関数を追加"
```

---

## Task 3: Agent — `retrieve_rag_context_local` に decay 分岐を追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `retrieve_rag_context_local` の DB 検索部分を置き換える**

`crates/rustyclaw-agent/src/lib.rs` の `retrieve_rag_context_local` 関数（line 2404付近）の DB 検索部分を置き換える。

変更前（line 2427-2442付近）:
```rust
    let threshold = config
        .embedding
        .as_ref()
        .map(|e| e.similarity_threshold)
        .unwrap_or(0.60);
    match db.search_similar_with_source(&embeddings[0], top_k, threshold) {
        Ok(results) if !results.is_empty() => {
            tracing::debug!("retrieve_rag_context_local: {} hits", results.len());
            format_rag_context(&results)
        }
        Ok(_) => String::new(),
        Err(e) => {
            tracing::warn!("retrieve_rag_context_local: search error: {}", e);
            String::new()
        }
    }
```

変更後:
```rust
    let threshold = config
        .embedding
        .as_ref()
        .map(|e| e.similarity_threshold)
        .unwrap_or(0.60);
    let half_life = config
        .embedding
        .as_ref()
        .and_then(|e| e.time_decay_half_life_days);
    let search_result = match half_life {
        Some(hl) => db.search_similar_with_decay(&embeddings[0], top_k, threshold, hl),
        None => db.search_similar_with_source(&embeddings[0], top_k, threshold),
    };
    match search_result {
        Ok(results) if !results.is_empty() => {
            tracing::debug!("retrieve_rag_context_local: {} hits", results.len());
            format_rag_context(&results)
        }
        Ok(_) => String::new(),
        Err(e) => {
            tracing::warn!("retrieve_rag_context_local: search error: {}", e);
            String::new()
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
git commit -m "feat(agent): Phase 42-D retrieve_rag_context_local に時間減衰リランキングを追加"
```

---

## Task 4: task.md の更新と PR 作成

- [ ] **Step 1: `docs/task.md` の Phase 42-D を完了済みにする**

`docs/task.md` の `42-D` 行を `[x]` に更新する。

- [ ] **Step 2: コミット**

```bash
git add docs/task.md
git commit -m "chore(task): Phase 42-D 完了マーク"
```

- [ ] **Step 3: ブランチを push して PR を作成する**

```bash
git push -u origin feat/phase42d-time-decay-reranking
gh pr create \
  --title "feat(rag): Phase 42-D 時間減衰リランキング" \
  --body "$(cat <<'EOF'
## Summary
- `EmbeddingConfig` に `time_decay_half_life_days: Option<f64>` を追加（未設定時は既存挙動を維持）
- `search_similar_with_decay` を storage 層に追加（cosine threshold → decay re-rank）
- `retrieve_rag_context_local` でローカル RAG 全パス（Heartbeat/Chat/Discord）に適用

## Test Plan
- [ ] `cargo test --all` が全 PASS
- [ ] `cargo clippy --all-targets` エラーなし
- [ ] config で `time_decay_half_life_days: 30.0` を設定して Heartbeat が正常動作することを確認
EOF
)"
```
