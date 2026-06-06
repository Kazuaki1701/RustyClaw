# RAG Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** MEMORY.md のバレット項目を Cloudflare Workers AI でベクトル化し、会話ごとに最関連な記憶のみを動的注入することで、長期記憶の容量制限とコンテキスト肥大化を解消する。

**Architecture:** `flush_memory` が MEMORY.md を更新するたびに全バレットを再 Embed して `memory.db` に保存。`execute_with_tools` 実行時にユーザーメッセージを Embed し、コサイン類似度で上位 K 件を Rust 側で計算してシステムプロンプト末尾に注入する。CF API エラーは Fail-open（従来の MEMORY.md 全文注入にフォールバック）。rig-core 不要 — スタンドアロン HTTP クライアントとして実装。

**Tech Stack:** Rust, `reqwest` (既存), SQLite/rusqlite (既存), 追加依存なし。多言語モデル `@cf/baai/bge-m3` (1024次元)。

---

## 前提条件・設計上の注意事項

- **多言語**: `@cf/baai/bge-m3` を使用 (英語特化モデルは日本語精度が低いため不可)
- **パフォーマンス現実値**: RPi4 での 1000件コサイン類似度計算 ≈ 1ms、10,000件 ≈ 10ms (許容範囲)
- **チャンク戦略**: MEMORY.md の `- ` 行を 1チャンク (最大 512文字、それ以上は末尾切捨て)
- **Cloudflare Workers AI 無料枠**: Embeddings は 1日 10,000 Neurons (bge-m3 は 1リクエスト ≈ 1 Neuron + テキスト長分)。flush_memory は 1日 10〜30 回程度なので問題なし
- **CF API エンドポイント**: `https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/run/@cf/baai/bge-m3`
- **rig-core 前提なし**: Phase 40 TODO 1 完了前でも独立実装可能

---

## ファイル構成

| ファイル | 変更種別 | 担当責務 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | 修正 | `EmbeddingConfig` struct 追加、`Config` への組み込み、シークレット解決 |
| `crates/rustyclaw-storage/src/lib.rs` | 修正 | `memory_embeddings` テーブル、CRUD メソッド、コサイン類似度検索 |
| `crates/rustyclaw-providers/src/lib.rs` | 修正 | `CloudflareEmbeddingClient` (CF Workers AI HTTP クライアント) |
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | Ingestion pipeline (`ingest_memory_md`) + Retrieval injection (`retrieve_rag_context`) |
| `production/config/config.release.json` | 修正 | `embedding` セクション追加 |

---

## Task 1: EmbeddingConfig の追加 (rustyclaw-config)

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`

- [ ] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-config/src/lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
#[test]
fn test_embedding_config_defaults() {
    let json = r#"{ "enabled": true, "api_endpoint": "https://example.com", "api_key": "k" }"#;
    let cfg: EmbeddingConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.dimensions, 1024);
    assert_eq!(cfg.top_k, 5);
    assert!((cfg.similarity_threshold - 0.65).abs() < 1e-6);
}

#[test]
fn test_embedding_config_in_config() {
    let json = r#"{
        "model_list": [],
        "agents": { "default": "none" },
        "embedding": {
            "enabled": true,
            "api_endpoint": "$env:EMBED_ENDPOINT_TEST",
            "api_key": "$env:EMBED_KEY_TEST"
        }
    }"#;
    unsafe {
        std::env::set_var("EMBED_ENDPOINT_TEST", "https://resolved-endpoint.com");
        std::env::set_var("EMBED_KEY_TEST", "resolved-key");
    }
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(json.as_bytes()).unwrap();
    let config = load_config(f.path()).unwrap();
    let emb = config.embedding.unwrap();
    assert!(emb.enabled);
    assert_eq!(emb.api_endpoint, "https://resolved-endpoint.com");
    assert_eq!(emb.api_key, "resolved-key");
    unsafe {
        std::env::remove_var("EMBED_ENDPOINT_TEST");
        std::env::remove_var("EMBED_KEY_TEST");
    }
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
cargo test -p rustyclaw-config test_embedding_config 2>&1 | tail -10
```
Expected: `FAILED` — `EmbeddingConfig` not found

- [ ] **Step 3: `EmbeddingConfig` struct とデフォルト値関数を追加**

`crates/rustyclaw-config/src/lib.rs` の `fn bool_true()` の直後に追加:

```rust
fn default_embedding_dims() -> usize { 1024 }
fn default_top_k() -> usize { 5 }
fn default_similarity_threshold() -> f32 { 0.65 }

/// RAG ベクトルメモリの埋め込みモデル設定
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingConfig {
    #[serde(default)]
    pub enabled: bool,
    /// CF Workers AI embedding エンドポイント (account ID + モデルパス含む完全 URL)
    /// 例: "https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/run/@cf/baai/bge-m3"
    #[serde(default)]
    pub api_endpoint: String,
    /// CF API トークン ($vault:cf-api-key)
    #[serde(default)]
    pub api_key: String,
    /// ベクトル次元数 (bge-m3 = 1024)
    #[serde(default = "default_embedding_dims")]
    pub dimensions: usize,
    /// 検索時に返す上位 K 件
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// コサイン類似度の最低閾値 (0.0〜1.0)
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f32,
}
```

`Config` struct の `pub mcp` フィールドの直後に追加:

```rust
    /// RAG ベクトルメモリ設定
    #[serde(default)]
    pub embedding: Option<EmbeddingConfig>,
```

`resolve_secrets` メソッドの `for server in self.mcp.values_mut()` ブロックの直後に追加:

```rust
        if let Some(ref mut e) = self.embedding {
            e.api_endpoint = resolve_value(&e.api_endpoint);
            e.api_key = resolve_value(&e.api_key);
        }
```

- [ ] **Step 4: テストがパスすることを確認**

```bash
cargo test -p rustyclaw-config test_embedding_config 2>&1 | tail -5
```
Expected: `test test_embedding_config_defaults ... ok` / `test test_embedding_config_in_config ... ok`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): add EmbeddingConfig for RAG memory"
```

---

## Task 2: Storage — memory_embeddings テーブルと CRUD (rustyclaw-storage)

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`

- [ ] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-storage/src/lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
#[test]
fn test_serialize_deserialize_embedding() {
    let v: Vec<f32> = vec![1.0, -0.5, 0.0, 2.5];
    let bytes = DbManager::serialize_embedding(&v);
    assert_eq!(bytes.len(), 16); // 4 × 4 bytes
    let back = DbManager::deserialize_embedding(&bytes);
    for (a, b) in v.iter().zip(back.iter()) {
        assert!((a - b).abs() < 1e-7);
    }
}

#[test]
fn test_embedding_crud() {
    let tmp = tempfile::tempdir().unwrap();
    let db = DbManager::new(tmp.path().join("test.db")).unwrap();
    let emb: Vec<f32> = vec![0.1, 0.2, 0.3];
    db.upsert_embedding("id1", "memory", None, "hello world", &emb).unwrap();
    let rows = db.load_all_embeddings().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "hello world");
    assert_eq!(rows[0].1.len(), 3);
    db.delete_embeddings_by_source("memory").unwrap();
    assert!(db.load_all_embeddings().unwrap().is_empty());
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
cargo test -p rustyclaw-storage test_serialize_deserialize_embedding test_embedding_crud 2>&1 | tail -5
```
Expected: `FAILED` — methods not found

- [ ] **Step 3: テーブル定義を `create_tables` に追加**

`crates/rustyclaw-storage/src/lib.rs` の `create_tables` メソッド内の `execute_batch` の SQL の末尾（`seen_items` テーブル定義の後）に追加:

```sql
            CREATE TABLE IF NOT EXISTS memory_embeddings (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                session_id TEXT,
                text_content TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_embeddings_source
                ON memory_embeddings(source);
```

- [ ] **Step 4: CRUD メソッドと serialize ヘルパーを追加**

`crates/rustyclaw-storage/src/lib.rs` の `DbManager` impl ブロック内 `get_usage_summary` の前に追加:

```rust
    // --- Memory Embeddings (RAG) ---

    pub fn serialize_embedding(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    pub fn deserialize_embedding(bytes: &[u8]) -> Vec<f32> {
        bytes.chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    /// ベクトルを upsert する (同一 id は上書き)
    pub fn upsert_embedding(
        &self,
        id: &str,
        source: &str,
        session_id: Option<&str>,
        text_content: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let blob = Self::serialize_embedding(embedding);
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO memory_embeddings
             (id, source, session_id, text_content, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, source, session_id, text_content, blob, now],
        ).context("Failed to upsert embedding")?;
        Ok(())
    }

    /// 全ベクトルをロードする → (text_content, embedding)
    pub fn load_all_embeddings(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT text_content, embedding FROM memory_embeddings"
        ).context("Failed to prepare load_all_embeddings")?;
        let rows = stmt.query_map([], |row| {
            let text: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((text, blob))
        }).context("Failed to query embeddings")?;
        let mut out = Vec::new();
        for row in rows {
            let (text, blob) = row.context("Failed to read embedding row")?;
            out.push((text, Self::deserialize_embedding(&blob)));
        }
        Ok(out)
    }

    /// 指定 source の全エントリを削除する
    pub fn delete_embeddings_by_source(&self, source: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM memory_embeddings WHERE source = ?1",
            rusqlite::params![source],
        ).context("Failed to delete embeddings by source")?;
        Ok(())
    }
```

- [ ] **Step 5: テストがパスすることを確認**

```bash
cargo test -p rustyclaw-storage test_serialize_deserialize_embedding test_embedding_crud 2>&1 | tail -5
```
Expected: both `ok`

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-storage/src/lib.rs
git commit -m "feat(storage): add memory_embeddings table and CRUD for RAG"
```

---

## Task 3: Storage — コサイン類似度検索 (rustyclaw-storage)

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`

- [ ] **Step 1: 失敗するテストを書く**

`#[cfg(test)]` ブロックに追加:

```rust
#[test]
fn test_cosine_similarity_identical() {
    let v = vec![1.0_f32, 0.0, 0.0];
    assert!((DbManager::cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
}

#[test]
fn test_cosine_similarity_orthogonal() {
    let a = vec![1.0_f32, 0.0];
    let b = vec![0.0_f32, 1.0];
    assert!(DbManager::cosine_similarity(&a, &b).abs() < 1e-6);
}

#[test]
fn test_search_similar_memories() {
    let tmp = tempfile::tempdir().unwrap();
    let db = DbManager::new(tmp.path().join("test.db")).unwrap();
    // 「hello」に近いベクトル (1, 0, 0)
    db.upsert_embedding("a", "memory", None, "hello", &[1.0, 0.0, 0.0]).unwrap();
    // 「world」は直交 (0, 1, 0)
    db.upsert_embedding("b", "memory", None, "world", &[0.0, 1.0, 0.0]).unwrap();

    // クエリ: (1, 0, 0) → "hello" だけがヒットするはず
    let results = db.search_similar_memories(&[1.0, 0.0, 0.0], 5, 0.9).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "hello");
    assert!((results[0].1 - 1.0).abs() < 1e-6);
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
cargo test -p rustyclaw-storage test_cosine test_search_similar 2>&1 | tail -5
```
Expected: `FAILED`

- [ ] **Step 3: コサイン類似度と検索メソッドを追加**

`DbManager` impl ブロックの `delete_embeddings_by_source` の直後に追加:

```rust
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() { return 0.0; }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 { return 0.0; }
        dot / (norm_a * norm_b)
    }

    /// クエリベクトルに近い記憶を上位 top_k 件返す → Vec<(text_content, similarity)>
    /// threshold 未満のエントリは除外。全件ロード + Rust 側計算 (外部 dep 不要)。
    pub fn search_similar_memories(
        &self,
        query_vec: &[f32],
        top_k: usize,
        threshold: f32,
    ) -> Result<Vec<(String, f32)>> {
        let all = self.load_all_embeddings()?;
        let mut scored: Vec<(String, f32)> = all
            .into_iter()
            .map(|(text, emb)| {
                let sim = Self::cosine_similarity(query_vec, &emb);
                (text, sim)
            })
            .filter(|(_, sim)| *sim >= threshold)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }
```

- [ ] **Step 4: テストがパスすることを確認**

```bash
cargo test -p rustyclaw-storage test_cosine test_search_similar 2>&1 | tail -5
```
Expected: all `ok`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-storage/src/lib.rs
git commit -m "feat(storage): add cosine similarity search for RAG memory retrieval"
```

---

## Task 4: CloudflareEmbeddingClient (rustyclaw-providers)

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`

- [ ] **Step 1: CF Workers AI レスポンス型とクライアント struct を確認してテストを書く**

CF Workers AI embedding API:
```
POST {api_endpoint}
Authorization: Bearer {api_key}
Content-Type: application/json

{"text": ["text1", "text2"]}

Response (成功):
{"result": {"data": [[f32; 1024], ...]}, "success": true, "errors": []}

Response (失敗):
{"result": null, "success": false, "errors": [{"code": 123, "message": "..."}]}
```

`crates/rustyclaw-providers/src/lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
#[test]
fn test_cf_embedding_parse_response() {
    let json = r#"{
        "result": {"data": [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]},
        "success": true,
        "errors": []
    }"#;
    let parsed: CfEmbedResponse = serde_json::from_str(json).unwrap();
    assert!(parsed.success);
    assert_eq!(parsed.result.as_ref().unwrap().data.len(), 2);
    assert!((parsed.result.unwrap().data[0][0] - 0.1).abs() < 1e-6);
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
cargo test -p rustyclaw-providers test_cf_embedding_parse_response 2>&1 | tail -5
```
Expected: `FAILED`

- [ ] **Step 3: レスポンス型と `CloudflareEmbeddingClient` を追加**

`crates/rustyclaw-providers/src/lib.rs` のパブリック pub use / mod 宣言の後、`PROVIDER_COOLDOWNS` static の前あたりに追加:

```rust
// ==============================================================================
// Cloudflare Workers AI Embedding Client
// ==============================================================================

#[derive(Debug, serde::Deserialize)]
struct CfEmbedResult {
    data: Vec<Vec<f32>>,
}

#[derive(Debug, serde::Deserialize)]
struct CfEmbedResponse {
    result: Option<CfEmbedResult>,
    success: bool,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}

/// Cloudflare Workers AI の embedding エンドポイントに POST してベクトルを返すクライアント。
/// rig-core 不要。reqwest（既存依存）のみ使用。
pub struct CloudflareEmbeddingClient {
    client: reqwest::Client,
    api_endpoint: String,
    api_key: String,
}

impl CloudflareEmbeddingClient {
    pub fn new(api_endpoint: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_endpoint: api_endpoint.into(),
            api_key: api_key.into(),
        }
    }

    /// テキストのスライスをベクトル化して返す。
    /// 入力と同順・同数の Vec<Vec<f32>> を返す。
    /// CF API が返すエラーは anyhow::Error に変換する。
    pub async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let body = serde_json::json!({ "text": texts });
        let resp = self.client
            .post(&self.api_endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("CF embedding: HTTP request failed")?;

        let status = resp.status();
        let text = resp.text().await.context("CF embedding: failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!("CF embedding: HTTP {} — {}", status, &text[..text.len().min(200)]);
        }

        let parsed: CfEmbedResponse = serde_json::from_str(&text)
            .context("CF embedding: failed to parse response JSON")?;

        if !parsed.success {
            anyhow::bail!("CF embedding: API error — {:?}", parsed.errors);
        }

        parsed.result
            .map(|r| r.data)
            .ok_or_else(|| anyhow::anyhow!("CF embedding: result field is null"))
    }
}
```

- [ ] **Step 4: テストがパスすることを確認**

```bash
cargo test -p rustyclaw-providers test_cf_embedding_parse_response 2>&1 | tail -5
```
Expected: `ok`

- [ ] **Step 5: ビルドが通ることを確認**

```bash
cargo build -p rustyclaw-providers 2>&1 | grep -E "^error" | head -5
```
Expected: 出力なし (warning のみ許容)

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "feat(providers): add CloudflareEmbeddingClient for RAG ingestion"
```

---

## Task 5: Ingestion Pipeline (rustyclaw-agent)

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: チャンク分割関数のテストを書く**

`crates/rustyclaw-agent/src/lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
#[test]
fn test_chunk_memory_md_basic() {
    let content = "# Memory\n\n- First bullet\n- Second bullet\n  continued\n- Third bullet";
    let chunks = chunk_memory_md(content);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0], "- First bullet");
    assert!(chunks[1].contains("Second bullet"));
    assert_eq!(chunks[2], "- Third bullet");
}

#[test]
fn test_chunk_memory_md_truncates_long() {
    let long = format!("- {}", "x".repeat(600));
    let chunks = chunk_memory_md(&long);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].len() <= 512);
}

#[test]
fn test_chunk_memory_md_skips_headers() {
    let content = "# Title\n\n## Section\n\n- bullet one\n- bullet two";
    let chunks = chunk_memory_md(content);
    assert_eq!(chunks.len(), 2);
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
cargo test -p rustyclaw-agent test_chunk_memory_md 2>&1 | tail -5
```
Expected: `FAILED`

- [ ] **Step 3: `chunk_memory_md` 関数を追加**

`crates/rustyclaw-agent/src/lib.rs` の `extract_delimited_block` 関数の直前に追加:

```rust
/// MEMORY.md のバレット行を 1件 1チャンクに分割する。
/// ヘッダー行 (#) や空行はスキップ。最大 512 文字で末尾切捨て。
pub(crate) fn chunk_memory_md(content: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if let Some(prev) = current.take() {
                chunks.push(prev);
            }
            current = Some(trimmed.to_string());
        } else if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && current.is_some()
        {
            if let Some(ref mut cur) = current {
                cur.push(' ');
                cur.push_str(trimmed);
            }
        }
    }
    if let Some(last) = current {
        chunks.push(last);
    }
    chunks
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| if s.len() > 512 { s[..512].to_string() } else { s })
        .collect()
}
```

- [ ] **Step 4: テストがパスすることを確認**

```bash
cargo test -p rustyclaw-agent test_chunk_memory_md 2>&1 | tail -5
```
Expected: all `ok`

- [ ] **Step 5: `ingest_memory_md` 関数を追加**

`chunk_memory_md` の直後に追加:

```rust
/// MEMORY.md を読み込み、バレット行をチャンク化してベクトル化し memory.db に保存する。
/// Fail-open: エラーは warn ログのみで処理を継続する。
pub(crate) async fn ingest_memory_md(
    workspace_dir: &Path,
    config: &Config,
    db_path: &Path,
) {
    let emb_cfg = match config.embedding.as_ref().filter(|e| e.enabled) {
        Some(c) => c,
        None => return, // embedding 無効
    };
    if emb_cfg.api_endpoint.is_empty() || emb_cfg.api_key.is_empty() {
        tracing::warn!("ingest_memory_md: api_endpoint or api_key is empty, skipping");
        return;
    }
    let memory_path = workspace_dir.join("MEMORY.md");
    let content = match std::fs::read_to_string(&memory_path) {
        Ok(c) => c,
        Err(e) => { tracing::warn!("ingest_memory_md: failed to read MEMORY.md: {}", e); return; }
    };
    let chunks = chunk_memory_md(&content);
    if chunks.is_empty() {
        tracing::debug!("ingest_memory_md: no chunks found in MEMORY.md");
        return;
    }

    let client = rustyclaw_providers::CloudflareEmbeddingClient::new(
        &emb_cfg.api_endpoint,
        &emb_cfg.api_key,
    );
    let text_refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
    let embeddings = match client.embed(&text_refs).await {
        Ok(v) => v,
        Err(e) => { tracing::warn!("ingest_memory_md: embedding API error: {}", e); return; }
    };

    let db = match rustyclaw_storage::DbManager::new(db_path) {
        Ok(d) => d,
        Err(e) => { tracing::warn!("ingest_memory_md: db open error: {}", e); return; }
    };
    if let Err(e) = db.delete_embeddings_by_source("memory") {
        tracing::warn!("ingest_memory_md: failed to delete old embeddings: {}", e);
        return;
    }
    for (i, (chunk, emb)) in chunks.iter().zip(embeddings.iter()).enumerate() {
        let id = format!("memory-{}", i);
        if let Err(e) = db.upsert_embedding(&id, "memory", None, chunk, emb) {
            tracing::warn!("ingest_memory_md: failed to upsert embedding {}: {}", id, e);
        }
    }
    tracing::info!("ingest_memory_md: ingested {} chunks from MEMORY.md", chunks.len());
}
```

- [ ] **Step 6: `flush_memory` から `ingest_memory_md` を呼び出す**

`crates/rustyclaw-agent/src/lib.rs` の `flush_memory` 内、MEMORY.md 書き込みの `if let Some(content)` ブロック（行 ~548-563）を以下のように修正:

```rust
        // 1. MEMORY.md の全書き換え (fail-open)
        if let Some(content) = new_memory {
            let final_content = if content.len() > 5000 {
                tracing::warn!(
                    actual_bytes = content.len(),
                    "memory flush: LLM returned oversized MEMORY.md, truncating"
                );
                truncate_70_20(&content, 5000)
            } else {
                content
            };
            if let Err(e) = rustyclaw_storage::atomic_write(&memory_path, final_content.as_bytes()).await {
                tracing::warn!("memory flush: failed to write MEMORY.md: {}", e);
            } else {
                // MEMORY.md 更新成功時のみ RAG ingestion を実行 (fail-open)
                ingest_memory_md(workspace_dir, &config, &db_path).await;
            }
        }
```

- [ ] **Step 7: ビルドが通ることを確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error" | head -5
```
Expected: エラーなし

- [ ] **Step 8: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): add RAG ingestion pipeline (chunk_memory_md + ingest_memory_md)"
```

---

## Task 6: Retrieval と Context Injection (rustyclaw-agent)

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: 失敗するテストを書く**

`#[cfg(test)]` ブロック末尾に追加:

```rust
#[test]
fn test_format_rag_context_empty() {
    let result = format_rag_context(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_format_rag_context_with_items() {
    let items = vec![
        ("- Rust is fast".to_string(), 0.92_f32),
        ("- RPi4 has 8GB RAM".to_string(), 0.85),
    ];
    let result = format_rag_context(&items);
    assert!(result.contains("Rust is fast"));
    assert!(result.contains("RPi4 has 8GB RAM"));
    assert!(result.contains("## Relevant Memory"));
}
```

- [ ] **Step 2: テストを実行して失敗を確認**

```bash
cargo test -p rustyclaw-agent test_format_rag_context 2>&1 | tail -5
```
Expected: `FAILED`

- [ ] **Step 3: `format_rag_context` と `retrieve_rag_context` 関数を追加**

`ingest_memory_md` 関数の直後に追加:

```rust
/// RAG 検索結果をシステムプロンプト注入用の Markdown 文字列に変換する。
/// 結果が空の場合は空文字列を返す。
pub(crate) fn format_rag_context(items: &[(String, f32)]) -> String {
    if items.is_empty() { return String::new(); }
    let mut out = String::from("\n\n## Relevant Memory\n");
    out.push_str("The following memories are relevant to the current conversation:\n\n");
    for (text, _sim) in items {
        out.push_str(text);
        out.push('\n');
    }
    out
}

/// ユーザーメッセージをベクトル化して memory.db を検索し、
/// システムプロンプトに追記するコンテキスト文字列を返す (Fail-open)。
pub(crate) async fn retrieve_rag_context(
    query_text: &str,
    config: &Config,
    db_path: &Path,
) -> String {
    let emb_cfg = match config.embedding.as_ref().filter(|e| e.enabled) {
        Some(c) => c,
        None => return String::new(),
    };
    if emb_cfg.api_endpoint.is_empty() || emb_cfg.api_key.is_empty() {
        return String::new();
    }
    let client = rustyclaw_providers::CloudflareEmbeddingClient::new(
        &emb_cfg.api_endpoint,
        &emb_cfg.api_key,
    );
    let query_short: String = query_text.chars().take(512).collect();
    let embeddings = match client.embed(&[query_short.as_str()]).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => return String::new(),
        Err(e) => { tracing::warn!("retrieve_rag_context: embed error: {}", e); return String::new(); }
    };
    let db = match rustyclaw_storage::DbManager::new(db_path) {
        Ok(d) => d,
        Err(e) => { tracing::warn!("retrieve_rag_context: db error: {}", e); return String::new(); }
    };
    let results = match db.search_similar_memories(
        &embeddings[0],
        emb_cfg.top_k,
        emb_cfg.similarity_threshold,
    ) {
        Ok(r) => r,
        Err(e) => { tracing::warn!("retrieve_rag_context: search error: {}", e); return String::new(); }
    };
    if results.is_empty() {
        return String::new();
    }
    tracing::debug!("retrieve_rag_context: {} hits for query snippet", results.len());
    format_rag_context(&results)
}
```

- [ ] **Step 4: テストがパスすることを確認**

```bash
cargo test -p rustyclaw-agent test_format_rag_context 2>&1 | tail -5
```
Expected: both `ok`

- [ ] **Step 5: `execute_with_tools` に RAG retrieval を組み込む**

`crates/rustyclaw-agent/src/lib.rs` の `execute_with_tools` 内、行 ~980-983（`build_system_context` 〜 `get_session_continuation_context` の直後）を以下に変更:

```rust
        // 1. システムプロンプトとコンテキストの構築
        let mut system_context = self.build_system_context(workspace_dir)?;
        if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id) {
            system_context.push_str(&continuation);
        }

        // RAG: ユーザーメッセージに関連する記憶を動的注入 (fail-open)
        {
            let db_path = workspace_dir.join("memory.db");
            let rag = retrieve_rag_context(user_message, &self.config, &db_path).await;
            if !rag.is_empty() {
                system_context.push_str(&rag);
            }
        }
```

- [ ] **Step 6: ビルドが通ることを確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error" | head -5
```
Expected: エラーなし

- [ ] **Step 7: 全テストがパスすることを確認**

```bash
cargo test -p rustyclaw-agent 2>&1 | tail -10
```
Expected: `test result: ok`

- [ ] **Step 8: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): inject RAG context into system prompt via cosine similarity retrieval"
```

---

## Task 7: Config wiring (production/config/config.release.json)

**Files:**
- Modify: `production/config/config.release.json`

- [ ] **Step 1: アカウント ID の確認**

CF Workers AI の直接 API エンドポイントには Cloudflare Account ID が必要。以下のいずれかから確認する:
- Cloudflare Dashboard → ホーム → 右カラムの "Account ID"
- `curl -s -H "Authorization: Bearer $(rustyclaw vault get cf-api-key)" https://api.cloudflare.com/client/v4/accounts | jq '.result[0].id'`

確認したアカウント ID を `{ACCOUNT_ID}` として次ステップで使用。

- [ ] **Step 2: embedding セクションを config.release.json に追加**

`production/config/config.release.json` の `"debug_dump": true,` の直後（または末尾）に追加:

```json
  "embedding": {
    "enabled": true,
    "api_endpoint": "https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/run/@cf/baai/bge-m3",
    "api_key": "$vault:cf-api-key",
    "dimensions": 1024,
    "top_k": 5,
    "similarity_threshold": 0.65
  },
```

`{ACCOUNT_ID}` は Step 1 で確認した実際の値に置き換えること。

- [ ] **Step 3: 動作確認 (--no-agent)**

```bash
cd /home/kazuaki/Projects/RustyClaw
./production/bin/rustyclaw-x64 gateway --no-agent 2>&1 | grep -E "embedding|ingest|RAG" | head -10
```
Expected: 起動エラーなし。embedding 関連のログが出ることが望ましいが、初回は ingestion 未実行のため出力がなくても正常。

- [ ] **Step 4: 手動で初回 Ingestion をトリガー**

flush_memory は会話が発生してはじめて実行される。初回は手動で MEMORY.md を書き直すか、短い会話を行って flush を誘発する。あるいは今後の起動時に自動 ingestion を追加してもよい（optional improvement）。

- [ ] **Step 5: コミットと deploy**

```bash
git add production/config/config.release.json
git commit -m "feat(config): enable RAG memory with @cf/baai/bge-m3 embedding"
cd /home/kazuaki/Projects/RustyClaw && bash scripts/deploy.sh
```

---

## Self-Review

### Spec coverage チェック

| レビュー指摘点 | 対応 Task |
|---|---|
| 多言語モデル (`bge-m3`) | Task 4 で明記 |
| パフォーマンス見積もり修正 | 前提条件セクションに現実値を記載 |
| rig-core 依存なし | Task 4 のアーキテクチャ説明に明記 |
| チャンク粒度定義 | Task 5 `chunk_memory_md` で `- ` 行を 1 チャンク、512 文字上限 |
| CF API 無料枠の記載 | 前提条件セクションに記載 |
| 配信済み判定 | 該当なし (RAG 機能には無関係) |
| Fail-open 設計 | Task 5・6 で全 API エラーを `warn` ログ + return |

### Placeholder scan

- TBD / TODO なし
- 「{ACCOUNT_ID}」は Task 7 Step 1 で取得手順を明示済み

### 型一貫性チェック

| 型 / メソッド | 定義 Task | 使用 Task |
|---|---|---|
| `DbManager::upsert_embedding` | Task 2 | Task 5 |
| `DbManager::load_all_embeddings` | Task 2 | Task 3 |
| `DbManager::delete_embeddings_by_source` | Task 2 | Task 5 |
| `DbManager::cosine_similarity` | Task 3 | Task 3 |
| `DbManager::search_similar_memories` | Task 3 | Task 6 |
| `CloudflareEmbeddingClient::embed` | Task 4 | Task 5, 6 |
| `chunk_memory_md` | Task 5 | Task 5 |
| `ingest_memory_md` | Task 5 | Task 5 |
| `retrieve_rag_context` | Task 6 | Task 6 |
| `format_rag_context` | Task 6 | Task 6 |
| `EmbeddingConfig` | Task 1 | Task 5, 6 |
