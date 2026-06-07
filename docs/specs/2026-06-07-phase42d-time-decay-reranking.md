# Phase 42-D 設計書: 時間減衰リランキング

**日付**: 2026-06-07  
**案件番号**: Phase 42-D  
**ステータス**: 設計承認済み

---

## 概要

ローカル embedding の RAG 検索結果に経過時間ペナルティを付与し、直近の重要情報を優先する。コサイン類似度スコアに指数減衰係数を乗じることで `combined_score = cosine_sim × 0.5^(age_days / half_life_days)` で re-rank する。設定値 `time_decay_half_life_days` が未設定の場合は既存挙動を維持（後方互換）。

---

## 背景

現在の `search_similar_with_source`（storage 層）は全 embedding のコサイン類似度のみでランキングしており、いつ記録されたチャンクかを考慮しない。MEMORY.md やセッション要約が蓄積するほど古い記憶が直近の情報を押しのける可能性がある。`memory_embeddings.created_at` は既に RFC 3339 テキストとして保存されており、decay 計算に必要な情報は DB に揃っている。

リモート RAG（`UnifiedRagEngine`/rig-core in-memory store）は `created_at` を持たないため、ローカル embedding のみが対象。

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-config/src/lib.rs` | `EmbeddingConfig` に `time_decay_half_life_days: Option<f64>` を追加 |
| `crates/rustyclaw-storage/src/lib.rs` | `search_similar_with_decay` 関数を追加 |
| `crates/rustyclaw-agent/src/lib.rs` | `retrieve_rag_context_local` で decay 分岐を追加 |

---

## 詳細設計

### 1. Config の変更

`EmbeddingConfig` に以下を追加する:

```rust
/// RAG 検索結果の時間減衰 half-life（日数）。
/// 省略時は減衰なし（既存挙動を維持）。
/// 例: 30.0 → 30日で combined_score が半減。
#[serde(default)]
pub time_decay_half_life_days: Option<f64>,
```

**`config.json` 設定例**:
```json
"embedding": {
    "use_local_embedding": true,
    "time_decay_half_life_days": 30.0
}
```

### 2. Storage の変更

`search_similar_with_source`（既存）は変更しない。新たに `search_similar_with_decay` を追加する。

```rust
/// コサイン類似度に時間減衰係数を乗じた combined_score でリランキングする。
/// threshold はコサイン類似度に対して適用（relevance ゲート）。
/// combined_score = cosine_sim * 0.5^(age_days / half_life_days)
pub fn search_similar_with_decay(
    &self,
    query_vec: &[f32],
    top_k: usize,
    threshold: f32,
    half_life_days: f64,
) -> Result<Vec<(String, String, f64)>>
```

**実装**:
```rust
let now_utc = chrono::Utc::now();
let mut stmt = self.conn.prepare(
    "SELECT source, text_content, embedding, created_at FROM memory_embeddings"
)?;
let rows = stmt.query_map([], |row| {
    let source: String = row.get(0)?;
    let text: String = row.get(1)?;
    let blob: Vec<u8> = row.get(2)?;
    let created_at: String = row.get(3)?;
    Ok((source, text, blob, created_at))
})?;

let mut scored: Vec<(String, String, f64)> = rows
    .filter_map(|r| r.ok())
    .filter_map(|(source, text, blob, created_at_str)| {
        let emb = Self::deserialize_embedding(&blob);
        let sim = Self::cosine_similarity(query_vec, &emb) as f64;
        if sim < threshold as f64 {
            return None;  // relevance ゲート（コサイン threshold）
        }
        let age_days = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| {
                let secs = (now_utc - dt.to_utc()).num_seconds().max(0) as f64;
                secs / 86400.0
            })
            .unwrap_or(0.0);  // parse 失敗時は decay なし（fail-open）
        let factor = 0.5_f64.powf(age_days / half_life_days);
        let combined = sim * factor;
        Some((source, text, combined))
    })
    .collect();

scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
scored.truncate(top_k);
Ok(scored)
```

**設計上の注意点**:
- Threshold は **cosine_sim** に適用（decay 後の combined_score には適用しない）
- `created_at` の parse 失敗は fail-open（`age_days = 0.0` → decay なし = cos sim そのまま）
- 返値の型は `Vec<(String, String, f64)>` で `search_similar_with_source` と同一

### 3. Agent の変更

`retrieve_rag_context_local` の DB 検索部分（`db.search_similar_with_source` 呼び出し）を以下に置き換える:

```rust
let half_life = config
    .embedding
    .as_ref()
    .and_then(|e| e.time_decay_half_life_days);

match half_life {
    Some(hl) => db.search_similar_with_decay(&embeddings[0], top_k, threshold, hl),
    None => db.search_similar_with_source(&embeddings[0], top_k, threshold),
}
```

---

## `system_context` への影響

`format_rag_context` は `(source, text, score)` を受け取るが、`score` フィールドは実装上完全に無視される（`source` と `text` のみを使用）。返値の型が `Vec<(String, String, f64)>` で一致するため変更不要。

---

## エラーハンドリング

- `created_at` の RFC 3339 parse 失敗 → `age_days = 0.0` として扱う（decay factor = 1.0。既存の cosine スコアを維持）
- `time_decay_half_life_days = 0.0` → `0.5^∞ = 0.0` でスコアが潰れる。計算上は問題ないが意味のない設定値。Config の validation は行わない（現在の Config 設計に validation 機構がないため）

---

## テスト方針

### `rustyclaw-config`
- `test_embedding_config_time_decay_half_life_days_default_none`: 未設定時に `None` になることを確認

### `rustyclaw-storage`
- `test_search_similar_with_decay_basic`: 同一ベクトル2件を挿入し、decay 適用で新しい方が上位に来ることを確認
- `test_search_similar_with_decay_threshold_filters`: cosine threshold 未満の item が除外されることを確認
- `test_search_similar_with_decay_invalid_created_at`: `created_at` が不正値の場合に fail-open（panic しない）であることを確認

### `rustyclaw-agent`
- 既存の `retrieve_rag_context_local` テストは変更なし（`time_decay_half_life_days = None` のデフォルトパスを通る）

### 統合確認
- `cargo build --all` / `cargo test --all` / `cargo clippy --all-targets`
