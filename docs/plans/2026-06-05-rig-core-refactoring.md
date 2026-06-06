# Unified RAG & rig-core Refactoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:**  
`rig-core` 0.38 をフル活用し、手動のコサイン類似度ループ、手動のプロンプト結合、自前の ReAct ループ、および自前 MCP 通信コードを完全に廃止し、`rig::agent::Agent` と `dynamic_context`（InMemoryVectorStore）および `rmcp` クライアントに全面移行する。

**Architecture:**  
`CloudflareEmbeddingClient` に `EmbeddingModel` を実装。`rustyclaw-storage` を `rig::vector::VectorStore` インターフェースで抽象化。ツール群を `#[tool]` アトリビュートで定義し、`rig::agent::Agent` に `dynamic_context` と `Toolset`、`rmcp` 経由の外部ツールをバインドして、RAG とツール実行ライフサイクルをすべて `rig` に完全委譲する。

**Tech Stack:** Rust, rig-core 0.38（`InMemoryVectorStore`, `EmbeddingModel`, `Agent`, `rmcp`）, rusqlite, tokio async

---

## ファイルマップ

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-providers/Cargo.toml` | `rig-core` 依存関係の追加 |
| `crates/rustyclaw-providers/src/lib.rs` | `CloudflareEmbeddingModel` の実装（`rig_core::embeddings::EmbeddingModel` 適合） |
| `crates/rustyclaw-storage/src/lib.rs` | `load_all_embeddings_with_ids()` および `delete_old_session_embeddings()` の追加 |
| `crates/rustyclaw-tools/Cargo.toml` | `rig-core` および `schemars` の追加 |
| `crates/rustyclaw-tools/src/lib.rs` | `#[rig_core::tool]` を用いたツールのリファクタリング、`ToolRegistry` から `Toolset` への移行 |
| `crates/rustyclaw-agent/Cargo.toml` | `rig-core` 依存関係の追加 |
| `crates/rustyclaw-agent/src/lib.rs` | ・`UnifiedRagEngine` の実装（`InMemoryVectorStore` ラッパー）<br>・`Pipeline` / `execute_with_tools` を `rig::agent::Agent` に一本化し、自前 ReAct ループと手動 RAG 結合コードを削除 |
| `crates/rustyclaw-mcp` | クレート全体を削除（`rig-mcp` または `rig-core` の `rmcp` フィーチャーで代替） |
| `crates/rustyclaw-gateway/src/lib.rs` | 起動時・リロード時の `UnifiedRagEngine` 初期化、および `Pipeline` / `Agent` のワイヤリング更新 |

---

## Task 1: providers — CloudflareEmbeddingModel の実装

**Files:**
- Modify: `crates/rustyclaw-providers/Cargo.toml`
- Modify: `crates/rustyclaw-providers/src/lib.rs`

- [ ] **Step 1: Cargo.toml に rig-core 依存関係を追加**
  `crates/rustyclaw-providers/Cargo.toml` の `[dependencies]` に追加：
  ```toml
  rig-core = "0.38"
  ```

- [ ] **Step 2: 失敗するユニットテストの追加**
  `crates/rustyclaw-providers/src/lib.rs` のテストブロックに追加：
  ```rust
  #[tokio::test]
  async fn test_cloudflare_embedding_model_implements_embedding_model() {
      let client = CloudflareEmbeddingClient::new(
          "http://127.0.0.1:1",
          "dummy",
          Some("text-embedding-bge-m3".to_string()),
      );
      let model = CloudflareEmbeddingModel::new(client, 1024);
      assert_eq!(model.ndims(), 1024);
  }
  ```

- [ ] **Step 3: テストを実行してコンパイルエラー（失敗）を確認**
  Run: `cargo test -p rustyclaw-providers -- test_cloudflare_embedding_model`
  Expected: FAIL (CloudflareEmbeddingModel 未定義)

- [ ] **Step 4: `CloudflareEmbeddingModel` の実装**
  `crates/rustyclaw-providers/src/lib.rs` に `CloudflareEmbeddingModel` と `EmbeddingModel` トレイトを実装：
  ```rust
  #[derive(Clone)]
  pub struct CloudflareEmbeddingModel {
      client: CloudflareEmbeddingClient,
      dims: usize,
  }

  impl CloudflareEmbeddingModel {
      pub fn new(client: CloudflareEmbeddingClient, dims: usize) -> Self {
          Self { client, dims }
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
  ※ `CloudflareEmbeddingClient` に `#[derive(Clone)]` を付与。

- [ ] **Step 5: テストを実行して通過を確認**
  Run: `cargo test -p rustyclaw-providers -- test_cloudflare_embedding_model`
  Expected: PASS

- [ ] **Step 6: コミット**
  ```bash
  git add crates/rustyclaw-providers/
  git commit -m "feat(providers): implement CloudflareEmbeddingModel for rig-core integration"
  ```

---

## Task 2: storage — インメモリ RAG 用 DB ロードメソッドの追加

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`

- [ ] **Step 1: 失敗するユニットテストの追加**
  `crates/rustyclaw-storage/src/lib.rs` のテストブロックに追加：
  ```rust
  #[test]
  fn test_load_all_embeddings_with_ids() {
      let dir = tempfile::tempdir().unwrap();
      let db = DbManager::new(dir.path().join("t.db").to_str().unwrap()).unwrap();
      db.upsert_embedding("m0", "memory", None, "text A", &[1.0f32, 0.0]).unwrap();
      
      let rows = db.load_all_embeddings_with_ids().unwrap();
      assert_eq!(rows.len(), 1);
      assert_eq!(rows[0].0, "m0");
      assert_eq!(rows[0].1, "memory");
  }
  ```

- [ ] **Step 2: テストを実行してコンパイルエラー（失敗）を確認**
  Run: `cargo test -p rustyclaw-storage -- test_load_all_embeddings_with_ids`
  Expected: FAIL (load_all_embeddings_with_ids 未定義)

- [ ] **Step 3: `load_all_embeddings_with_ids` と `delete_old_session_embeddings` の実装**
  `crates/rustyclaw-storage/src/lib.rs` に実装：
  ```rust
  pub fn load_all_embeddings_with_ids(&self) -> Result<Vec<(String, String, String, Vec<f32>)>> {
      let mut stmt = self.conn.prepare(
          "SELECT id, source, text_content, embedding FROM memory_embeddings"
      )?;
      let rows = stmt.query_map([], |row| {
          let id: String = row.get(0)?;
          let source: String = row.get(1)?;
          let text: String = row.get(2)?;
          let blob: Vec<u8> = row.get(3)?;
          Ok((id, source, text, blob))
      })?;
      let mut out = Vec::new();
      for row in rows {
          let (id, source, text, blob) = row?;
          out.push((id, source, text, Self::deserialize_embedding(&blob)));
      }
      Ok(out)
  }

  pub fn delete_old_session_embeddings(&self, keep_days: u32) -> Result<()> {
      self.conn.execute(
          "DELETE FROM memory_embeddings WHERE source = 'session' AND created_at < datetime('now', ?1)",
          rusqlite::params![format!("-{} days", keep_days)],
      )?;
      Ok(())
  }
  ```

- [ ] **Step 4: テストを実行して通過を確認**
  Run: `cargo test -p rustyclaw-storage`
  Expected: PASS

- [ ] **Step 5: コミット**
  ```bash
  git add crates/rustyclaw-storage/
  git commit -m "feat(storage): add load_all_embeddings_with_ids and delete_old_session_embeddings"
  ```

---

## Task 3: tools — `#[tool]` アトリビュートマクロへの移行

**Files:**
- Modify: `crates/rustyclaw-tools/Cargo.toml`
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- [ ] **Step 1: Cargo.toml に rig-core と schemars 依存関係を追加**
  `crates/rustyclaw-tools/Cargo.toml` の `[dependencies]` に追加：
  ```toml
  rig-core = "0.38"
  schemars = "0.8"
  serde = { version = "1.0", features = ["derive"] }
  ```

- [ ] **Step 2: 既存のツールの `#[tool]` 化テストを追加**
  例として `CronScheduleTool` や新規のデミーツールを `#[tool]` 定義するテストを `crates/rustyclaw-tools/src/lib.rs` に追加し、スキーマ生成を確認。

- [ ] **Step 3: `#[tool]` マクロへのリファクタリング**
  手動の `schema()` および `call()` ロジックを廃止し、`rig_core::tool` マクロで記述。
  ```rust
  use schemars::JsonSchema;
  use serde::Deserialize;

  #[derive(Deserialize, JsonSchema)]
  pub struct CronScheduleArgs {
      pub job_name: String,
      pub cron_expr: String,
  }

  #[rig_core::tool(description = "Schedule a recurring cron job")]
  pub async fn schedule_cron_job(args: CronScheduleArgs) -> Result<String, String> {
      // 既存のスケジューリングロジックの呼び出し
      Ok(format!("Job '{}' scheduled successfully", args.job_name))
  }
  ```

- [ ] **Step 4: テスト通過とビルドの確認**
  Run: `cargo test -p rustyclaw-tools`
  Expected: PASS

- [ ] **Step 5: コミット**
  ```bash
  git add crates/rustyclaw-tools/
  git commit -m "refactor(tools): migrate tools to rig-core #[tool] macro and schemars"
  ```

---

## Task 4: mcp — rig-core `rmcp` クライアントへの移行

**Files:**
- Delete: `crates/rustyclaw-mcp/`
- Modify: `crates/rustyclaw-agent/Cargo.toml`

- [ ] **Step 1: `rustyclaw-mcp` クレートの削除と Cargo.toml のクリーンアップ**
  ルートの `Cargo.toml` の `members` から `"crates/rustyclaw-mcp"` を削除し、ディレクトリを削除。

- [ ] **Step 2: `rustyclaw-agent` に `rig-core` の `rmcp` フィーチャーを導入**
  `crates/rustyclaw-agent/Cargo.toml` に `rig-core` の `rmcp` フィーチャーを指定：
  ```toml
  rig-core = { version = "0.38", features = ["rmcp"] }
  ```

- [ ] **Step 3: 外部 MCP サーバー接続用のテストコード作成**
  `crates/rustyclaw-agent/src/lib.rs` にて、`McpClient::connect_stdio` を使って外部プロセスからツールを取得できるかテスト。

- [ ] **Step 4: コミット**
  ```bash
  git commit -a -m "feat(mcp): replace custom rustyclaw-mcp with rig-core rmcp client"
  ```

---

## Task 5: agent — `UnifiedRagEngine` (InMemoryVectorStore) の実装

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: 失敗するテストを追加**
  `crates/rustyclaw-agent/src/lib.rs` のテストブロックに `test_unified_rag_engine_rebuild` を追加。

- [ ] **Step 2: コンパイルエラー（失敗）を確認**
  Run: `cargo test -p rustyclaw-agent -- test_unified_rag_engine_rebuild`
  Expected: FAIL

- [ ] **Step 3: `UnifiedRagEngine` の実装**
  インメモリでの高速ベクトル検索エンジンを実装。ID プレフィックス (`memory::`, `session::`) でメタデータを管理。
  ```rust
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

      pub fn rebuild_from_db(&self, db: &rustyclaw_storage::DbManager) -> anyhow::Result<()> {
          let rows = db.load_all_embeddings_with_ids()?;
          let mut store = self.store.lock().unwrap();
          *store = InMemoryVectorStore::default();
          let documents = rows.into_iter().map(|(id, _, text, vec_f32)| {
              let emb = rig_core::embeddings::Embedding {
                  document: text.clone(),
                  vec: vec_f32.iter().map(|&x| x as f64).collect(),
              };
              (id, text, rig_core::OneOrMany::one(emb))
          }).collect();
          store.add_documents_with_ids(documents);
          Ok(())
      }

      pub fn add_one(&self, id: &str, text: &str, vec_f32: &[f32]) {
          let emb = rig_core::embeddings::Embedding {
              document: text.to_string(),
              vec: vec_f32.iter().map(|&x| x as f64).collect(),
          };
          let mut store = self.store.lock().unwrap();
          store.add_documents_with_ids([(id.to_string(), text.to_string(), rig_core::OneOrMany::one(emb))]);
      }

      pub async fn search(&self, query_text: &str, top_k: usize) -> anyhow::Result<Vec<(String, String, f64)>> {
          let req = rig_core::vector_store::request::VectorSearchRequest::builder()
              .query(query_text.to_string())
              .samples(top_k as u64)
              .build();
          let store_clone = self.store.lock().unwrap().clone();
          let index = store_clone.index(self.model.clone());
          let id_scores = index.top_n_ids(req).await?;

          let text_map: std::collections::HashMap<String, String> = {
              let store = self.store.lock().unwrap();
              store.iter().map(|(id, (text, _))| (id.clone(), text.clone())).collect()
          };

          Ok(id_scores.into_iter().filter_map(|(score, id)| {
              let source = if id.starts_with("memory::") { "memory".to_string() }
                  else if id.starts_with("session::") { "session".to_string() }
                  else { "unknown".to_string() };
              let text = text_map.get(&id)?.clone();
              Some((source, text, score))
          }).collect())
      }
  }
  ```

- [ ] **Step 4: テスト通過を確認**
  Run: `cargo test -p rustyclaw-agent`
  Expected: PASS

- [ ] **Step 5: コミット**
  ```bash
  git add crates/rustyclaw-agent/src/lib.rs
  git commit -m "feat(agent): implement UnifiedRagEngine using InMemoryVectorStore"
  ```

---

## Task 6: agent — `rig::agent::Agent` 移行と ReAct/RAG ループ一本化

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: 自前 ReAct ループ / RAG 結合ロジックの削除**
  `execute_with_tools` および `execute_stream` の内部から、手動の `complete` 呼び出し、`while tool_calls` の実行ループ、および `retrieve_rag_context` からのプロンプト結合コードを完全に削除。

- [ ] **Step 2: `rig::agent::Agent` のバインドと呼び出しの実装**
  `Pipeline` 内にエージェント生成処理を実装。
  ```rust
  // execute_with_tools 内
  let store_clone = self.rag.as_ref().unwrap().store.lock().unwrap().clone();
  let index = store_clone.index(self.embedding_model.clone());

  let agent = client.agent(&self.config.main_model)
      .preamble(&self.build_system_context(workspace_dir)?)
      .dynamic_context(3, index) // RAG インデックスのバインド
      .tools(self.toolset.clone()) // 移行した Toolset を直接登録
      .build();

  // 実行 (ReAct 処理を含めて完全に rig 内で処理される)
  let response = agent.chat(user_message).await?;
  ```

- [ ] **Step 3: ビルド確認**
  Run: `cargo build -p rustyclaw-agent`
  Expected: エラーなし

- [ ] **Step 4: コミット**
  ```bash
  git add crates/rustyclaw-agent/src/lib.rs
  git commit -m "refactor(agent): replace custom ReAct and RAG assembly loops with rig::agent::Agent"
  ```

---

## Task 7: config & gateway — 全体のワイヤリングとデプロイ検証

**Files:**
- Modify: `production/config/config.debug.json`
- Modify: `production/config/config.release.json`
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: 設定ファイルへの TTL 追加**
  `config.debug.json` および `config.release.json` の `embedding` に `"session_summary_ttl_days": 7` を追加。

- [ ] **Step 2: ゲートウェイでの UnifiedRagEngine 初期化**
  `crates/rustyclaw-gateway/src/lib.rs` の `active_config` 確定後に、`UnifiedRagEngine` を作成し、DBから初期ロードを行い、`Pipeline::set_rag` でインジェクション。

- [ ] **Step 3: 全テスト実行**
  Run: `cargo test`
  Expected: `test result: ok` (全テスト通過)

- [ ] **Step 4: deploy + 動作確認**
  ```bash
  bash scripts/deploy.sh
  ssh rp1 "curl -s http://localhost:8080/reload"
  ```
  Expected: `RELOADED` が返り、journalctl ログに `init_rag_engine: loaded N embeddings` が記録されること。

- [ ] **Step 5: コミット**
  ```bash
  git add production/config/ crates/rustyclaw-gateway/
  git commit -m "feat(gateway): wire UnifiedRagEngine initialization on startup and config reload"
  ```
