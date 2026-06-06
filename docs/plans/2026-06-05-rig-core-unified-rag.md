# Unified RAG with rig-core InMemoryVectorStore Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** rig-core 0.38 の `InMemoryVectorStore` を採用し、MEMORY.md チャンクとセッション要約を統合した RAG 検索空間を構築する。起動時に SQLite から InMemoryVectorStore を再構築し、検索は `InMemoryVectorIndex::top_n_ids()` 経由で行う。

**Architecture:** `CloudflareEmbeddingClient` に rig-core の `EmbeddingModel` トレイトを実装した `CloudflareEmbeddingModel` を追加する。`UnifiedRagEngine` が `InMemoryVectorStore<String>` と `CloudflareEmbeddingModel` を保持し、インジェスト（SQLite + InMemory 同時追加）と検索（`top_n_ids`）を一元管理する。source 情報は ID プレフィックス（`memory::N`, `session::SESSION_ID`）で管理する。MEMORY.md は固定システムプロンプトから除外し RAG で代替。

**Tech Stack:** Rust, rig-core 0.38.1（`InMemoryVectorStore`, `EmbeddingModel`）, rusqlite, tokio async, reqwest

---

## ファイルマップ

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-providers/Cargo.toml` | `rig-core = "0.38"` 追加 |
| `crates/rustyclaw-providers/src/lib.rs` | `CloudflareEmbeddingClient` に `Clone` 追加・`CloudflareEmbeddingModel` 追加（`EmbeddingModel` 実装） |
| `crates/rustyclaw-config/src/lib.rs` | `EmbeddingConfig` に `session_summary_ttl_days: Option<u32>` 追加 |
| `crates/rustyclaw-storage/src/lib.rs` | `load_all_embeddings_with_ids()` + `delete_old_session_embeddings()` 追加 |
| `crates/rustyclaw-agent/Cargo.toml` | `rig-core = "0.38"` 追加 |
| `crates/rustyclaw-agent/src/lib.rs` | `UnifiedRagEngine` 実装、`ingest_memory_md` / `ingest_session_summary` / `retrieve_rag_context` / `build_system_context` 更新、`Pipeline` に `rag` フィールド追加 |
| `crates/rustyclaw-gateway/src/lib.rs` | 起動・リロード時の `UnifiedRagEngine::rebuild_from_db()` 呼び出し追加 |
| `production/config/config.debug.json` / `config.release.json` | `session_summary_ttl_days: 7` 追加 |

---

## Task 1: providers — CloudflareEmbeddingModel の実装

**Files:**
- Modify: `crates/rustyclaw-providers/Cargo.toml`
- Modify: `crates/rustyclaw-providers/src/lib.rs`

- [ ] **Step 1: Cargo.toml に rig-core を追加**

`crates/rustyclaw-providers/Cargo.toml` の `[dependencies]` に追加:

```toml
rig-core = "0.38"
```

- [ ] **Step 2: 失敗テストを書く**

`crates/rustyclaw-providers/src/lib.rs` の `#[cfg(test)]` ブロックに追加:

```rust
#[tokio::test]
async fn test_cloudflare_embedding_model_implements_embedding_model() {
    use rig_core::embeddings::EmbeddingModel;
    let client = CloudflareEmbeddingClient::new(
        "http://127.0.0.1:1", // 到達不能 → embed は失敗するが型チェックのみ
        "dummy",
        Some("text-embedding-bge-m3".to_string()),
    );
    let model = CloudflareEmbeddingModel::new(client, 1024);
    assert_eq!(model.ndims(), 1024);
}
```

- [ ] **Step 3: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-providers -- test_cloudflare_embedding_model 2>&1 | tail -5
```

Expected: FAIL（`CloudflareEmbeddingModel` 未定義）

- [ ] **Step 4: `CloudflareEmbeddingClient` に `Clone` を追加**

`crates/rustyclaw-providers/src/lib.rs` の `CloudflareEmbeddingClient` 定義を修正:

```rust
#[derive(Clone)]
pub struct CloudflareEmbeddingClient {
    client: reqwest::Client,
    api_endpoint: String,
    api_key: String,
    model: Option<String>,
}
```

- [ ] **Step 5: `CloudflareEmbeddingModel` を実装**

`CloudflareEmbeddingClient` の `impl CloudflareEmbeddingClient` ブロックの直後に追加:

```rust
/// rig-core の EmbeddingModel トレイトを実装したラッパー。
/// CloudflareEmbeddingClient（OpenAI互換 / CF Workers AI）を rig の VectorStore と統合する。
pub struct CloudflareEmbeddingModel {
    client: CloudflareEmbeddingClient,
    dims: usize,
}

impl CloudflareEmbeddingModel {
    pub fn new(client: CloudflareEmbeddingClient, dims: usize) -> Self {
        Self { client, dims }
    }
}

impl Clone for CloudflareEmbeddingModel {
    fn clone(&self) -> Self {
        Self { client: self.client.clone(), dims: self.dims }
    }
}

impl rig_core::embeddings::EmbeddingModel for CloudflareEmbeddingModel {
    const MAX_DOCUMENTS: usize = 100;
    type Client = CloudflareEmbeddingClient;

    fn make(client: &Self::Client, _model: impl Into<String>, dims: Option<usize>) -> Self {
        Self::new(client.clone(), dims.unwrap_or(1024))
    }

    fn ndims(&self) -> usize {
        self.dims
    }

    async fn embed_texts(
        &self,
        texts: impl IntoIterator<Item = String> + rig_core::wasm_compat::WasmCompatSend,
    ) -> Result<Vec<rig_core::embeddings::Embedding>, rig_core::embeddings::EmbeddingError> {
        let texts: Vec<String> = texts.into_iter().collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let vecs = self.client.embed(&text_refs).await
            .map_err(|e| rig_core::embeddings::EmbeddingError::ProviderError(e.to_string()))?;
        Ok(texts.into_iter().zip(vecs).map(|(doc, vec)| {
            rig_core::embeddings::Embedding {
                document: doc,
                vec: vec.iter().map(|&x| x as f64).collect(),
            }
        }).collect())
    }
}
```

- [ ] **Step 6: テストを実行して通過確認**

```bash
cargo test -p rustyclaw-providers -- test_cloudflare_embedding_model 2>&1 | tail -5
```

Expected: `test result: ok. 1 passed`

全テスト確認:
```bash
cargo test -p rustyclaw-providers 2>&1 | tail -5
```

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-providers/Cargo.toml crates/rustyclaw-providers/src/lib.rs
git commit -m "feat(providers): add CloudflareEmbeddingModel implementing rig-core EmbeddingModel"
```

---

## Task 2: config — session_summary_ttl_days 追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`

- [ ] **Step 1: 失敗テストを追加**

`#[cfg(test)]` ブロックに追加:

```rust
#[test]
fn test_embedding_config_ttl_default() {
    let cfg: EmbeddingConfig = serde_json::from_str(r#"{}"#).unwrap();
    assert!(cfg.session_summary_ttl_days.is_none());

    let cfg2: EmbeddingConfig = serde_json::from_str(r#"{"session_summary_ttl_days": 14}"#).unwrap();
    assert_eq!(cfg2.session_summary_ttl_days, Some(14));
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-config -- test_embedding_config_ttl 2>&1 | tail -5
```

Expected: FAIL

- [ ] **Step 3: フィールドを追加**

`EmbeddingConfig` の `similarity_threshold` フィールドの直後に追加:

```rust
/// セッション要約 embedding の保持日数（省略時は 7 日）
#[serde(default)]
pub session_summary_ttl_days: Option<u32>,
```

- [ ] **Step 4: テスト通過確認**

```bash
cargo test -p rustyclaw-config 2>&1 | tail -5
```

Expected: `test result: ok`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): add session_summary_ttl_days to EmbeddingConfig"
```

---

## Task 3: storage — UnifiedRagEngine 用 DB メソッド追加

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`

- [ ] **Step 1: 失敗テストを追加**

`#[cfg(test)]` ブロックに追加:

```rust
#[test]
fn test_load_all_embeddings_with_ids() {
    let dir = tempfile::tempdir().unwrap();
    let db = DbManager::new(dir.path().join("t.db").to_str().unwrap()).unwrap();
    db.upsert_embedding("m0", "memory",  None,          "text A", &[1.0f32, 0.0]).unwrap();
    db.upsert_embedding("s0", "session", Some("ses-1"), "text B", &[0.0f32, 1.0]).unwrap();

    let rows = db.load_all_embeddings_with_ids().unwrap();
    assert_eq!(rows.len(), 2);
    let ids: Vec<&str> = rows.iter().map(|(id, _, _, _)| id.as_str()).collect();
    assert!(ids.contains(&"m0"));
    assert!(ids.contains(&"s0"));
    let (_, src, _, _) = rows.iter().find(|(id, _, _, _)| id == "m0").unwrap();
    assert_eq!(src, "memory");
}

#[test]
fn test_delete_old_session_embeddings() {
    let dir = tempfile::tempdir().unwrap();
    let db = DbManager::new(dir.path().join("t.db").to_str().unwrap()).unwrap();
    db.conn.execute(
        "INSERT INTO memory_embeddings(id,source,session_id,text_content,embedding,created_at)
         VALUES('old','session','s-old','old',X'00000000','2020-01-01T00:00:00Z')",
        [],
    ).unwrap();
    db.upsert_embedding("new", "session", Some("s-new"), "new", &[0.0f32]).unwrap();
    db.upsert_embedding("mem", "memory",  None,          "keep", &[0.0f32]).unwrap();

    db.delete_old_session_embeddings(30).unwrap();

    let n_session: i64 = db.conn.query_row(
        "SELECT count(*) FROM memory_embeddings WHERE source='session'", [], |r| r.get(0)
    ).unwrap();
    let n_memory: i64 = db.conn.query_row(
        "SELECT count(*) FROM memory_embeddings WHERE source='memory'", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(n_session, 1);
    assert_eq!(n_memory,  1);
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-storage -- test_load_all_embeddings_with_ids test_delete_old_session 2>&1 | tail -5
```

Expected: FAIL

- [ ] **Step 3: `load_all_embeddings_with_ids` を実装**

`load_all_embeddings` の直後に追加:

```rust
/// (id, source, text_content, embedding) の全行を返す。UnifiedRagEngine の rebuild に使用。
pub fn load_all_embeddings_with_ids(&self) -> Result<Vec<(String, String, String, Vec<f32>)>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, source, text_content, embedding FROM memory_embeddings"
    ).context("Failed to prepare load_all_embeddings_with_ids")?;
    let rows = stmt.query_map([], |row| {
        let id:     String   = row.get(0)?;
        let source: String   = row.get(1)?;
        let text:   String   = row.get(2)?;
        let blob:   Vec<u8>  = row.get(3)?;
        Ok((id, source, text, blob))
    }).context("Failed to query embeddings")?;
    let mut out = Vec::new();
    for row in rows {
        let (id, source, text, blob) = row.context("Failed to read row")?;
        out.push((id, source, text, Self::deserialize_embedding(&blob)));
    }
    Ok(out)
}
```

- [ ] **Step 4: `delete_old_session_embeddings` を実装**

`delete_embeddings_by_source` の直後に追加:

```rust
/// source="session" の embedding のうち keep_days 日より古いものを削除する。
pub fn delete_old_session_embeddings(&self, keep_days: u32) -> Result<()> {
    self.conn.execute(
        "DELETE FROM memory_embeddings
         WHERE source = 'session'
           AND created_at < datetime('now', ?1)",
        rusqlite::params![format!("-{} days", keep_days)],
    ).context("Failed to delete old session embeddings")?;
    Ok(())
}
```

- [ ] **Step 5: テスト通過確認**

```bash
cargo test -p rustyclaw-storage 2>&1 | tail -5
```

Expected: `test result: ok`

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-storage/src/lib.rs
git commit -m "feat(storage): add load_all_embeddings_with_ids and delete_old_session_embeddings"
```

---

## Task 4: agent — UnifiedRagEngine 実装

**Files:**
- Modify: `crates/rustyclaw-agent/Cargo.toml`
- Modify: `crates/rustyclaw-agent/src/lib.rs`（`// ── RAG Helpers ──` セクション）

- [ ] **Step 1: Cargo.toml に rig-core を追加**

`crates/rustyclaw-agent/Cargo.toml` の `[dependencies]` に追加:

```toml
rig-core = "0.38"
```

- [ ] **Step 2: 失敗テストを書く**

`#[cfg(test)]` ブロックに追加:

```rust
#[test]
fn test_unified_rag_engine_rebuild_from_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("memory.db");
    let db = rustyclaw_storage::DbManager::new(&db_path).unwrap();
    db.upsert_embedding("memory::0", "memory", None, "大森駅周辺", &vec![1.0f32; 1024]).unwrap();
    db.upsert_embedding("session::s1", "session", Some("s1"), "RAG 検証完了", &vec![0.5f32; 1024]).unwrap();

    let client = rustyclaw_providers::CloudflareEmbeddingClient::new(
        "http://127.0.0.1:1", "dummy", Some("text-embedding-bge-m3".to_string()),
    );
    let model = rustyclaw_providers::CloudflareEmbeddingModel::new(client, 1024);
    let engine = UnifiedRagEngine::new(model);
    engine.rebuild_from_db(&db).unwrap();
    assert_eq!(engine.len(), 2);
}
```

- [ ] **Step 3: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-agent -- test_unified_rag_engine_rebuild 2>&1 | tail -5
```

Expected: FAIL

- [ ] **Step 4: `UnifiedRagEngine` を実装**

`crates/rustyclaw-agent/src/lib.rs` のファイル先頭 `use` ブロックに追加:

```rust
use rig_core::vector_store::in_memory_store::InMemoryVectorStore;
use rig_core::embeddings::{Embedding, OneOrMany};
use rig_core::vector_store::request::VectorSearchRequest;
use rig_core::vector_store::VectorStoreIndex;
```

`// ── RAG Helpers ──` セクションの先頭（既存の `chunk_memory_md` より前）に追加:

```rust
/// rig-core InMemoryVectorStore を使った統合 RAG エンジン。
/// SQLite（永続化）と InMemoryVectorStore（高速検索）のハイブリッド構成。
/// source 情報は ID プレフィックス（"memory::N", "session::SESSION_ID"）で管理する。
pub struct UnifiedRagEngine {
    store: std::sync::Mutex<InMemoryVectorStore<String>>,
    model: rustyclaw_providers::CloudflareEmbeddingModel,
}

impl UnifiedRagEngine {
    pub fn new(model: rustyclaw_providers::CloudflareEmbeddingModel) -> Self {
        Self {
            store: std::sync::Mutex::new(InMemoryVectorStore::default()),
            model,
        }
    }

    /// SQLite の全 embedding データを InMemoryVectorStore に再ロードする。
    /// アプリ起動時・リロード時に呼ぶ。
    pub fn rebuild_from_db(&self, db: &rustyclaw_storage::DbManager) -> anyhow::Result<()> {
        let rows = db.load_all_embeddings_with_ids()?;
        let mut store = self.store.lock().unwrap();
        *store = InMemoryVectorStore::default();
        let documents: Vec<(String, String, OneOrMany<Embedding>)> = rows
            .into_iter()
            .map(|(id, _source, text, vec_f32)| {
                let emb = Embedding {
                    document: text.clone(),
                    vec: vec_f32.iter().map(|&x| x as f64).collect(),
                };
                (id, text, OneOrMany::one(emb))
            })
            .collect();
        store.add_documents_with_ids(documents);
        Ok(())
    }

    /// 1件を SQLite + InMemoryVectorStore に同時追加する。
    /// id は呼び出し元が `"memory::N"` / `"session::SESSION_ID"` 形式で渡す。
    pub fn add_one(
        &self,
        id: &str,
        source: &str,
        session_id: Option<&str>,
        text: &str,
        vec_f32: &[f32],
    ) {
        let emb = Embedding {
            document: text.to_string(),
            vec: vec_f32.iter().map(|&x| x as f64).collect(),
        };
        let mut store = self.store.lock().unwrap();
        store.add_documents_with_ids([(id.to_string(), text.to_string(), OneOrMany::one(emb))]);
        let _ = session_id; // SQLite 側の upsert は呼び出し元が行う
    }

    /// top_k 件の (source, text, score) を返す。
    /// source は ID プレフィックスから抽出（"memory::" → "memory", "session::" → "session"）。
    pub async fn search(
        &self,
        query_text: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<(String, String, f64)>> {
        let req = VectorSearchRequest::builder()
            .query(query_text.to_string())
            .samples(top_k as u64)
            .build();
        let store_clone = {
            let store = self.store.lock().unwrap();
            store.clone()
        };
        let index = store_clone.index(self.model.clone());
        let results = index.top_n_ids(req).await
            .map_err(|e| anyhow::anyhow!("RAG search failed: {}", e))?;
        Ok(results.into_iter().map(|(score, id)| {
            let source = if id.starts_with("memory::") { "memory".to_string() }
                else if id.starts_with("session::") { "session".to_string() }
                else { "unknown".to_string() };
            // text は InMemoryVectorStore の D フィールドから取得できないため
            // ここでは id を text として返す（呼び出し元で DB 参照するか、id から推定）
            // 実用上は store の get_document を使う
            (source, id, score)
        }).collect())
    }

    pub fn len(&self) -> usize {
        self.store.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.lock().unwrap().is_empty()
    }
}
```

- [ ] **Step 5: テスト通過確認**

```bash
cargo test -p rustyclaw-agent -- test_unified_rag_engine_rebuild 2>&1 | tail -5
```

Expected: `test result: ok. 1 passed`

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error" | head -5
```

Expected: エラーなし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/Cargo.toml crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): add UnifiedRagEngine with rig-core InMemoryVectorStore"
```

---

## Task 5: agent — ingest_memory_md / ingest_session_summary を UnifiedRagEngine ベースに更新

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

現在の `ingest_memory_md` と `retrieve_rag_context` を読んでから作業すること（1349〜1460行付近）。

- [ ] **Step 1: `ingest_memory_md` のシグネチャと実装を更新**

現在の `ingest_memory_md(workspace_dir, config, db_path)` を以下に置き換える。
`rag_engine` は `Option<&UnifiedRagEngine>` にすることで `flush_memory`（rag 未初期化）でも呼べるようにする:

```rust
/// MEMORY.md を chunk 分割して embedding し、SQLite + UnifiedRagEngine に保存する。Fail-open。
/// rag_engine が Some の場合は InMemoryVectorStore にも追加する。
pub(crate) async fn ingest_memory_md(
    workspace_dir: &Path,
    config: &Config,
    db_path: &Path,
    rag_engine: Option<&UnifiedRagEngine>,
) {
    let (api_endpoint, api_key, model) = match config.get_embedding_client_params() {
        Some(p) => p,
        None => {
            if config.embedding.as_ref().map(|e| e.enabled).unwrap_or(false) {
                tracing::warn!("ingest_memory_md: embedding enabled but no valid model config");
            }
            return;
        }
    };
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
        &api_endpoint, &api_key, model,
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
    if embeddings.len() != chunks.len() {
        tracing::warn!(
            "ingest_memory_md: chunk/embedding mismatch ({} vs {}), proceeding with zip",
            chunks.len(), embeddings.len()
        );
    }
    if let Err(e) = db.delete_embeddings_by_source("memory") {
        tracing::warn!("ingest_memory_md: failed to delete old embeddings: {}", e);
        return;
    }
    for (i, (chunk, emb)) in chunks.iter().zip(embeddings.iter()).enumerate() {
        let id = format!("memory::{}", i);
        if let Err(e) = db.upsert_embedding(&id, "memory", None, chunk, emb) {
            tracing::warn!("ingest_memory_md: failed to upsert {}: {}", id, e);
        }
        if let Some(rag) = rag_engine {
            rag.add_one(&id, "memory", None, chunk, emb);
        }
    }
    tracing::info!("ingest_memory_md: ingested {} chunks from MEMORY.md", chunks.len());
}
```

- [ ] **Step 2: `ingest_session_summary` を追加**

`ingest_memory_md` の直後に追加:

```rust
/// セッション要約テキストを embedding して SQLite + UnifiedRagEngine に保存する。Fail-open。
pub(crate) async fn ingest_session_summary(
    session_id: &str,
    summary_text: &str,
    config: &Config,
    db_path: &Path,
    rag_engine: &UnifiedRagEngine,
) {
    let (api_endpoint, api_key, model) = match config.get_embedding_client_params() {
        Some(p) => p,
        None => return,
    };
    let client = rustyclaw_providers::CloudflareEmbeddingClient::new(
        &api_endpoint, &api_key, model,
    );
    let text_short: String = summary_text.chars().take(1024).collect();
    let embeddings = match client.embed(&[text_short.as_str()]).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => { tracing::warn!("ingest_session_summary: empty embedding result"); return; }
        Err(e) => { tracing::warn!("ingest_session_summary: embed error: {}", e); return; }
    };
    let db = match rustyclaw_storage::DbManager::new(db_path) {
        Ok(d) => d,
        Err(e) => { tracing::warn!("ingest_session_summary: db open error: {}", e); return; }
    };
    let id = format!("session::{}", session_id.replace(':', "-"));
    if let Err(e) = db.upsert_embedding(&id, "session", Some(session_id), &text_short, &embeddings[0]) {
        tracing::warn!("ingest_session_summary: upsert error: {}", e);
        return;
    }
    rag_engine.add_one(&id, "session", Some(session_id), &text_short, &embeddings[0]);
    let ttl_days = config.embedding.as_ref()
        .and_then(|e| e.session_summary_ttl_days)
        .unwrap_or(7);
    if let Err(e) = db.delete_old_session_embeddings(ttl_days) {
        tracing::warn!("ingest_session_summary: TTL cleanup error: {}", e);
    }
    tracing::info!("ingest_session_summary: stored embedding for session '{}'", session_id);
}
```

- [ ] **Step 3: `flush_memory` 内の `ingest_memory_md` 呼び出しを更新**

`flush_memory` は static fn で `Pipeline.rag` にアクセスできないため、`rag_engine: None` を渡す（DB のみ更新）。
Task 7 の gateway 起動・リロード時に `rebuild_from_db()` で InMemoryVectorStore を同期する:

```rust
ingest_memory_md(workspace_dir, &config, &db_path, None).await;
```

- [ ] **Step 4: `generate_session_summary` の末尾に `ingest_session_summary` 呼び出しを追加**

`generate_session_summary` 関数の `Ok(final_summary)` 直前:

```rust
// セッション要約を RAG に登録（Fail-open）— rag_engine は Task 7 で Pipeline.rag 経由に切り替える
// {
//     let db_path = workspace_dir.join("memory.db");
//     ingest_session_summary(target_session_id, &final_summary, &self.config, &db_path, &rag).await;
// }
```

- [ ] **Step 5: ビルド確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error" | head -10
```

Expected: エラーなし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): add ingest_session_summary; update ingest_memory_md for UnifiedRagEngine"
```

---

## Task 6: agent — retrieve_rag_context の更新と MEMORY.md 除外

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `format_rag_context` の signature と実装を更新**

現在の `format_rag_context(items: &[(String, f32)])` を以下に置き換える:

```rust
/// RAG 検索結果をシステムプロンプト注入用の Markdown に変換する。
/// source="memory" と source="session" を別セクションで表示する。
pub(crate) fn format_rag_context(items: &[(String, String, f64)]) -> String {
    if items.is_empty() { return String::new(); }
    let memory_items: Vec<&str> = items.iter()
        .filter(|(src, _, _)| src == "memory")
        .map(|(_, txt, _)| txt.as_str())
        .collect();
    let session_items: Vec<&str> = items.iter()
        .filter(|(src, _, _)| src == "session")
        .map(|(_, txt, _)| txt.as_str())
        .collect();
    let mut out = String::new();
    if !memory_items.is_empty() {
        out.push_str("\n\n## Relevant Memory\n");
        out.push_str("The following memories are relevant to the current conversation:\n\n");
        for text in &memory_items { out.push_str(text); out.push('\n'); }
    }
    if !session_items.is_empty() {
        out.push_str("\n\n## Relevant Past Sessions\n");
        out.push_str("The following session summaries are relevant:\n\n");
        for text in &session_items { out.push_str(text); out.push('\n'); }
    }
    out
}
```

- [ ] **Step 2: `retrieve_rag_context` のシグネチャと実装を更新**

現在の `retrieve_rag_context(query_text, config, db_path)` を以下に置き換える。
`UnifiedRagEngine.search()` は `embed_text()` を内部で呼ぶため、threshold フィルタは `search()` 結果から手動で適用する:

```rust
/// ユーザーメッセージに関連する記憶・過去セッションを UnifiedRagEngine から検索し、
/// システムプロンプト注入用の文字列を返す。Fail-open。
pub(crate) async fn retrieve_rag_context(
    query_text: &str,
    config: &Config,
    rag_engine: &UnifiedRagEngine,
) -> String {
    if rag_engine.is_empty() {
        return String::new();
    }
    let top_k   = config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
    let threshold = config.embedding.as_ref().map(|e| e.similarity_threshold as f64).unwrap_or(0.60);
    let query_short: String = query_text.chars().take(512).collect();
    let results = match rag_engine.search(&query_short, top_k).await {
        Ok(r) => r,
        Err(e) => { tracing::warn!("retrieve_rag_context: search error: {}", e); return String::new(); }
    };
    // top_n_ids は distance（コサイン距離）を返す。score >= threshold でフィルタ。
    let filtered: Vec<(String, String, f64)> = results.into_iter()
        .filter(|(_, _, score)| *score >= threshold)
        .collect();
    if filtered.is_empty() { return String::new(); }
    tracing::debug!("retrieve_rag_context: {} hits above threshold {}", filtered.len(), threshold);

    // text を DB から取得する代わりに、UnifiedRagEngine の store から get_document を使う
    // ただし現時点では id を text として返しているため、DB から引き直す必要がある
    // Task 4 の search() では id を返しているため、ここで db から text を取得する
    // TODO: UnifiedRagEngine.search() が text も返すよう Task 4 を後で改良
    format_rag_context(&filtered)
}
```

**注意**: Task 4 の `search()` は現状 ID を text として返しているので、実際のテキストが注入されない。
Step 3 で `search()` を修正して実テキストを返すようにする。

- [ ] **Step 3: `UnifiedRagEngine::search()` を修正してテキストも返すようにする**

Task 4 で追加した `search()` メソッドを以下に置き換える（`InMemoryVectorStore::iter()` でテキストを取得）:

```rust
pub async fn search(
    &self,
    query_text: &str,
    top_k: usize,
) -> anyhow::Result<Vec<(String, String, f64)>> {
    let req = VectorSearchRequest::builder()
        .query(query_text.to_string())
        .samples(top_k as u64)
        .build();
    let store_clone = {
        let store = self.store.lock().unwrap();
        store.clone()
    };
    let index = store_clone.index(self.model.clone());
    let id_scores = index.top_n_ids(req).await
        .map_err(|e| anyhow::anyhow!("RAG search failed: {}", e))?;

    // InMemoryVectorStore の store から id → text のマッピングを作成
    let text_map: std::collections::HashMap<String, String> = {
        let store = self.store.lock().unwrap();
        store.iter()
            .map(|(id, (text, _))| (id.clone(), text.clone()))
            .collect()
    };

    Ok(id_scores.into_iter().filter_map(|(score, id)| {
        let source = if id.starts_with("memory::") { "memory".to_string() }
            else if id.starts_with("session::") { "session".to_string() }
            else { "unknown".to_string() };
        let text = text_map.get(&id)?.clone();
        Some((source, text, score))
    }).collect())
}
```

- [ ] **Step 4: `build_system_context` から MEMORY.md を除外**

`build_system_context` 内の `files` 配列を修正（MEMORY.md を除外し、RAG で代替）:

```rust
// MEMORY.md は RAG（UnifiedRagEngine）経由で動的に注入するため除外。
let files = ["SOUL.md", "AGENTS.md", "USER.md"];
```

- [ ] **Step 5: `execute_with_tools` / `execute_stream` の `retrieve_rag_context` 呼び出しを更新**

`execute_with_tools` 内の既存の RAG ブロック（`let db_path = workspace_dir.join("memory.db");` の3行）を以下に置き換える:

```rust
// RAG: ユーザーメッセージに関連する記憶を動的注入（rag が初期化済みの場合のみ）
if let Some(ref rag) = self.rag {
    let rag_ctx = retrieve_rag_context(user_message, &self.config, rag).await;
    if !rag_ctx.is_empty() { system_context.push_str(&rag_ctx); }
}
```

`execute_stream` 内にも同じブロックがある（`// RAG: ユーザーメッセージ` のコメントで検索）。
同一のコードに置き換える:

```rust
// RAG: ユーザーメッセージに関連する記憶を動的注入（rag が初期化済みの場合のみ）
if let Some(ref rag) = self.rag {
    let rag_ctx = retrieve_rag_context(user_message, &self.config, rag).await;
    if !rag_ctx.is_empty() { system_context.push_str(&rag_ctx); }
}
```

- [ ] **Step 6: ビルド確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error" | head -10
```

Expected: エラーなし

既存テスト `test_build_system_context_injects_runtime_context` 等が MEMORY.md を期待している場合は修正:
```bash
cargo test -p rustyclaw-agent -- test_build_system_context 2>&1 | tail -10
```

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): use UnifiedRagEngine in retrieve_rag_context; remove MEMORY.md from system prompt"
```

---

## Task 7: Pipeline — rag フィールド追加と gateway 初期化

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

このタスクを実施する前に `crates/rustyclaw-gateway/src/lib.rs` の以下を読むこと:
- `Pipeline::new()` 呼び出し箇所（3箇所、265行 / 338行 / 444行付近）
- `reload` 処理（835行付近）
- ファイル冒頭の `use` 宣言と構造体定義

- [ ] **Step 1: `Pipeline` 構造体に `rag` フィールドを追加**

`crates/rustyclaw-agent/src/lib.rs` の `Pipeline` 構造体を修正:

```rust
pub struct Pipeline {
    config: Config,
    provider: Box<dyn LlmProvider>,
    flush_sem: Arc<Semaphore>,
    /// UnifiedRagEngine（Arc で共有、起動後に set_rag で設定）
    pub rag: Option<Arc<UnifiedRagEngine>>,
}
```

`Pipeline::new()` に `rag: None` を追加:

```rust
pub fn new(config: Config, flush_sem: Arc<Semaphore>) -> Self {
    let provider = create_provider(config.get_model("default"));
    Self { config, provider, flush_sem, rag: None }
}
```

`set_rag()` メソッドを追加:

```rust
pub fn set_rag(&mut self, rag: Arc<UnifiedRagEngine>) {
    self.rag = Some(rag);
}
```

- [ ] **Step 2: `flush_memory` 内の ingest 呼び出しを有効化**

Task 5 でコメントアウトした箇所を有効化。`flush_memory` は `Pipeline` のメソッドではなく static fn のため、`rag` を渡せない。
代わりに `flush_memory` の末尾で `rebuild_from_db` を呼ぶ方式に変更:

`flush_memory` 関数の末尾（`atomic_write` 成功後）:

```rust
} else {
    ingest_memory_md(workspace_dir, &config, &db_path, /* rag は使わず DB のみ更新 */).await;
    // NOTE: InMemoryVectorStore の再構築は gateway の reload フックで行う
}
```

Task 5 で `ingest_memory_md` の `rag_engine` 引数を `Option<&UnifiedRagEngine>` にしているため、
`flush_memory` からは `None` を渡すだけでよい（Task 5 Step 3 で対応済み）。
InMemoryVectorStore の同期は gateway のリロード時に `rebuild_from_db()` が行う。

- [ ] **Step 3: `generate_session_summary` 末尾の呼び出しを有効化**

`generate_session_summary` 末尾のコメントを外し、`Pipeline` の `self.rag` を使って呼び出す:

```rust
// セッション要約を RAG に登録（Fail-open）
{
    let db_path = workspace_dir.join("memory.db");
    if let Some(ref rag) = self.rag {
        ingest_session_summary(
            target_session_id,
            &final_summary,
            &self.config,
            &db_path,
            rag,
        ).await;
    } else {
        // rag 未初期化の場合は DB のみに保存
        let (api_endpoint, api_key, model) = match self.config.get_embedding_client_params() {
            Some(p) => p,
            None => { return Ok(final_summary); }
        };
        let client = rustyclaw_providers::CloudflareEmbeddingClient::new(&api_endpoint, &api_key, model);
        let text_short: String = final_summary.chars().take(1024).collect();
        if let Ok(embeddings) = client.embed(&[text_short.as_str()]).await {
            if !embeddings.is_empty() {
                if let Ok(db) = rustyclaw_storage::DbManager::new(&db_path) {
                    let id = format!("session::{}", target_session_id.replace(':', "-"));
                    let _ = db.upsert_embedding(&id, "session", Some(target_session_id), &text_short, &embeddings[0]);
                    let ttl = self.config.embedding.as_ref().and_then(|e| e.session_summary_ttl_days).unwrap_or(7);
                    let _ = db.delete_old_session_embeddings(ttl);
                    tracing::info!("ingest_session_summary (db-only): stored '{}'", target_session_id);
                }
            }
        }
    }
}
```

- [ ] **Step 4: gateway で UnifiedRagEngine を初期化・共有**

`crates/rustyclaw-gateway/src/lib.rs` の起動処理（`Pipeline::new()` 呼び出し付近）で `UnifiedRagEngine` を作成し、DB から再ロードして `Pipeline::set_rag()` で設定する。

まず `use` を追加:

```rust
use rustyclaw_agent::UnifiedRagEngine;
use std::sync::Arc;
```

gateway のメイン起動処理に追加（`active_config` が確定した直後）。
`Pipeline::new()` が呼ばれる箇所（265行 / 338行 / 444行）のパターンは全て同様なので、
共通の `init_rag` ヘルパーを追加:

```rust
fn init_rag_engine(
    config: &rustyclaw_config::Config,
    workspace_path: &std::path::Path,
) -> Option<Arc<UnifiedRagEngine>> {
    let emb_cfg = config.embedding.as_ref().filter(|e| e.enabled)?;
    let (api_endpoint, api_key, model_name) = config.get_embedding_client_params()?;
    let dims = emb_cfg.dimensions;
    let client = rustyclaw_providers::CloudflareEmbeddingClient::new(
        &api_endpoint, &api_key, Some(model_name.unwrap_or_else(|| "text-embedding-bge-m3".to_string())),
    );
    let model = rustyclaw_providers::CloudflareEmbeddingModel::new(client, dims);
    let engine = UnifiedRagEngine::new(model);
    let db_path = workspace_path.join("memory.db");
    match rustyclaw_storage::DbManager::new(&db_path) {
        Ok(db) => {
            if let Err(e) = engine.rebuild_from_db(&db) {
                tracing::warn!("init_rag_engine: rebuild failed: {}", e);
            } else {
                tracing::info!("init_rag_engine: loaded {} embeddings", engine.len());
            }
        }
        Err(e) => tracing::warn!("init_rag_engine: db open failed: {}", e),
    }
    Some(Arc::new(engine))
}
```

`Pipeline::new()` の直後に `set_rag` を呼ぶよう変更（3箇所とも）:

```rust
let mut pipeline = Pipeline::new(active_config.clone(), gmn_sem.clone());
if let Some(rag) = init_rag_engine(&active_config, &workspace_path) {
    pipeline.set_rag(rag);
}
```

リロード処理（835行付近）にも同様に `rebuild_from_db` トリガーを追加。

- [ ] **Step 5: ビルド確認**

```bash
cargo build 2>&1 | grep "^error" | head -10
```

Expected: エラーなし

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(gateway): initialize UnifiedRagEngine at startup and reload; wire into Pipeline"
```

---

## Task 8: config 更新 + deploy + 動作確認

**Files:**
- Modify: `production/config/config.debug.json`
- Modify: `production/config/config.release.json`

- [ ] **Step 1: `config.debug.json` の `embedding` セクションに TTL 追加**

`embedding` オブジェクトに `"session_summary_ttl_days": 7` を追加する。

- [ ] **Step 2: `config.release.json` にも同様に追加**

- [ ] **Step 3: 全テスト実行**

```bash
cargo test 2>&1 | tail -15
```

Expected: `test result: ok`

- [ ] **Step 4: deploy + reload**

```bash
bash scripts/deploy.sh 2>&1 | tail -5
scp production/config/config.debug.json rp1:/home/kazuaki/.rustyclaw/config/config.debug.json
ssh rp1 "curl -s http://localhost:8080/reload"
```

Expected: `RELOADED`

- [ ] **Step 5: ログで初期化確認**

```bash
ssh rp1 "journalctl -u rustyclaw --no-pager -n 30 | grep -E 'init_rag|rebuild|UnifiedRag|embeddings'"
```

Expected: `init_rag_engine: loaded N embeddings`

- [ ] **Step 6: 動作確認 — RAG が system prompt に注入されているか**

```bash
ssh rp1 "journalctl -u rustyclaw --no-pager -n 50 | grep 'retrieve_rag_context'"
```

Expected: `retrieve_rag_context: N hits above threshold` のログが次回リクエスト時に出ること

- [ ] **Step 7: コミット**

```bash
git add production/config/config.debug.json production/config/config.release.json
git commit -m "feat(config): add session_summary_ttl_days=7 to embedding config"
```

---

## 実装メモ

**rig-core の `top_n_ids` はコサイン類似度ではなく距離を返す**
`rig-core` の `vector_search_brute_force` は内部でコサイン類似度を計算し、値をそのまま score として返す（distance ≈ cosine similarity）。0〜1 の範囲で 1 に近いほど類似。threshold フィルタは `retrieve_rag_context` で `score >= threshold` として適用する。

**`InMemoryVectorStore` の `Clone` について**
`InMemoryVectorStore<D>` は `#[derive(Clone, Default)]` を持つため、`search()` 内での `store.clone().index(model)` は安全に動作する。

**MEMORY.md 固定連結の廃止について**
`build_system_context` から `MEMORY.md` を除外することで、毎回数百トークンのシステムプロンプト削減が期待できる。ただし RAG の検索空間に MEMORY.md のチャンクが含まれていることが前提（`ingest_memory_md` が正常に動作していること）。

**静的ドキュメント（AGENTS.md 等）のインジェスト**
本計画では `AGENTS.md` は固定システムプロンプトに残す（除外しない）。将来的に `ingest_static_documents()` を実装して RAG 化することは可能だが、スコープ外とする。

**TTL=7 日の根拠**
セッション要約は 1 日複数件生成される可能性があるため、7 日間で週次の文脈を保持する設計。
