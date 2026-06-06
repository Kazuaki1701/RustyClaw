# Static Documentation RAG Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:**  
`AGENT.md`（システム指示書）や `skills/*.md`（スキル定義）などの静的なドメイン知識ドキュメントを RPi4 上で RAG 化（インジェスト）し、`memory.db` に永続化する。  
会話時に「ユーザーの入力」と類似度の高いドキュメントのみを動的にシステムプロンプトへ注入することで、外部 LLM への送信コンテキストサイズを大幅に削減（圧縮）し、不要な情報の送信を防ぐ。  
また、このコンテキスト削減効果に伴い、**従来実施していた厳しいメッセージ履歴の強制カット（ハードキャップ）および静的プロンプトの連結処理を廃止・緩和**し、会話の文脈追従能力を最大化する。

**Prerequisites:**  
この計画は、先行して実行予定の `docs/superpowers/plans/2026-06-04-session-summary-rag.md` （セッション要約 RAG）が完了していることを前提とする（`search_similar_memories_with_source` の導入やデータベーススキーマが整備されていること）。

**Tech Stack:** Rust, rusqlite (rustyclaw-storage), CloudflareEmbeddingClient (rustyclaw-providers), tokio async

---

## ファイルマップ

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-storage/src/lib.rs` | ドキュメント更新用（ファイルパス別のハッシュ保存・差分判定用）のメタデータ管理機能の追加 |
| `crates/rustyclaw-agent/src/lib.rs` | ・静的ドキュメントのチャンク分割ロジック `chunk_static_document`、インジェスト処理 `ingest_static_documents` の追加<br>・RAG 検索・フォーマット更新<br>・システムプロンプト固定構築部分の最小化（`AGENTS.md` / `MEMORY.md` の除外）<br>・履歴メッセージ制限 `get_history_message_limit` の上限緩和 |
| `crates/rustyclaw-gateway/src/lib.rs` | ゲートウェイ起動時およびリロード時（SIGHUP、HTTP `/reload`）にインジェスト処理を非同期トリガーする処理の追加 |
| `production/config/config.debug.json` / `config.release.json` | 検索対象リトリーブ設定 of EmbeddingConfig （TTLなどの追加） |

---

## Task 1: storage — 差分インジェスト用ファイル変更検知スキーマの追加

RPi4 での無駄な Embedding API コール（およびローカル推論の CPU 負荷）を防ぐため、ドキュメントのハッシュ値を管理するテーブルを用意し、変更があったファイルのみを再インジェストする。

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`

- [ ] **Step 1: データベース初期化時に `document_states` テーブルを追加**
  `DbManager::new` 内のテーブル作成 SQL 群に, 以下を追加：
  ```sql
  CREATE TABLE IF NOT EXISTS document_states (
      file_path TEXT PRIMARY KEY,
      last_hash TEXT NOT NULL,
      updated_at TEXT NOT NULL
  );
  ```

- [ ] **Step 2: ドキュメントの変更検知メソッドを実装**
  `DbManager` に以下のメソッドを追加：
  ```rust
  /// ファイルのハッシュ値を検証し、前回から変更されているかを判定する。
  /// 変更されている（または未登録）の場合は true を返し、ハッシュ値を更新する。
  pub fn check_and_update_doc_state(&self, file_path: &str, current_hash: &str) -> Result<bool> {
      let mut stmt = self.conn.prepare(
          "SELECT last_hash FROM document_states WHERE file_path = ?1"
      )?;
      let mut rows = stmt.query([file_path])?;
      
      if let Some(row) = rows.next()? {
          let last_hash: String = row.get(0)?;
          if last_hash == current_hash {
              return Ok(false); // 変更なし
          }
      }

      // 新規登録または更新
      self.conn.execute(
          "INSERT OR REPLACE INTO document_states (file_path, last_hash, updated_at)
           VALUES (?1, ?2, datetime('now'))",
          rusqlite::params![file_path, current_hash],
      )?;
      Ok(true) // 変更あり
  }
  ```

- [ ] **Step 3: 単体テストの追加と確認**
  `check_and_update_doc_state` の動作テストを `crates/rustyclaw-storage/src/lib.rs` のテストブロックに追加して実行。

---

## Task 2: agent — マークダウンのチャンク分割とインジェストロジックの実装

静的マークダウンファイル（`AGENT.md` や `skills/*.md`）を見出し（`#`, `##`, `###`）単位で適切にチャンク分割する。

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: マークダウンのチャンク分割ロジック `chunk_static_document` を実装**
  `crates/rustyclaw-agent/src/lib.rs` の `// ── RAG Helpers ──` セクションに追加。
  ```rust
  /// マークダウンコンテンツを ## または ### の見出し単位で分割する。
  /// 各チャンクにはファイル名や大見出しの文脈（タイトル）を付与して検索精度を高める。
  pub(crate) fn chunk_static_document(file_name: &str, content: &str) -> Vec<String> {
      let mut chunks = Vec::new();
      let mut current_chunk = String::new();
      let mut current_header = String::new();

      for line in content.lines() {
          let trimmed = line.trim();
          if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
              // これまでのチャンクを保存
              if !current_chunk.is_empty() {
                  chunks.push(format!("[{file_name} > {current_header}]\n{current_chunk}"));
              }
              current_header = trimmed.to_string();
              current_chunk = trimmed.to_string();
          } else {
              if !current_chunk.is_empty() {
                  current_chunk.push('\n');
              }
              current_chunk.push_str(line);
          }
      }
      if !current_chunk.is_empty() {
          chunks.push(format!("[{file_name} > {current_header}]\n{current_chunk}"));
      }
      
      // 800文字を超える場合は切り捨てる
      chunks.into_iter()
          .filter(|s| !s.is_empty())
          .map(|s| if s.len() > 800 { s.chars().take(800).collect::<String>() } else { s })
          .collect()
  }
  ```

- [ ] **Step 2: 静的ドキュメント一括インジェスト `ingest_static_documents` の実装**
  ファイル一覧（`workspace/AGENT.md` および `workspace/skills/*.md`）を読み込んでチャンク化し、ハッシュ値に変更がある場合のみ embedding を再生成して `memory_embeddings` に登録する。
  ```rust
  pub async fn ingest_static_documents(
      workspace_dir: &std::path::Path,
      config: &Config,
      db_path: &std::path::Path,
  ) {
      let (api_endpoint, api_key, model) = match config.get_embedding_client_params() {
          Some(p) => p,
          None => return,
      };
      let client = rustyclaw_providers::CloudflareEmbeddingClient::new(&api_endpoint, &api_key, model);
      let db = match rustyclaw_storage::DbManager::new(db_path) {
          Ok(d) => d,
          Err(e) => { tracing::warn!("ingest_static_documents: db open error: {}", e); return; }
      };

      // スキャンするファイル一覧
      let mut files = vec![workspace_dir.join("AGENT.md")];
      let skills_dir = workspace_dir.join("skills");
      if let Ok(entries) = std::fs::read_dir(skills_dir) {
          for entry in entries.flatten() {
              let path = entry.path();
              if path.is_file() && path.extension().map_or(false, |ext| ext == "md") {
                  files.push(path);
              }
          }
      }

      for file_path in files {
          let Ok(content) = std::fs::read_to_string(&file_path) else { continue; };
          let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
          
          // ハッシュ値の計算 (SHA-256)
          use sha2::{Sha256, Digest};
          let mut hasher = Sha256::new();
          hasher.update(content.as_bytes());
          let hash_str = format!("{:x}", hasher.finalize());

          // 変更検知
          let is_changed = match db.check_and_update_doc_state(file_name, &hash_str) {
              Ok(changed) => changed,
              Err(e) => { tracing::warn!("check_and_update_doc_state failed: {}", e); true }
          };

          if !is_changed {
              tracing::debug!("Document '{}' is unchanged. Skipping ingestion.", file_name);
              continue;
          }

          tracing::info!("Document '{}' changed. Starting RAG ingestion...", file_name);
          let chunks = chunk_static_document(file_name, &content);
          if chunks.is_empty() { continue; }

          // 古い該当ファイルの Embedding を削除
          let source_id = format!("doc:{}", file_name);
          let _ = db.delete_embeddings_by_source(&source_id);

          // チャンク群を embedding 化して保存
          let text_refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
          if let Ok(embeddings) = client.embed(&text_refs).await {
              for (i, (chunk, emb)) in chunks.iter().zip(embeddings.iter()).enumerate() {
                  let id = format!("doc-{}-{}", file_name.replace('.', "_"), i);
                  let _ = db.upsert_embedding(&id, &source_id, None, chunk, emb);
              }
              tracing::info!("Ingested {} chunks from '{}'", chunks.len(), file_name);
          } else {
              tracing::warn!("Failed to embed chunks for '{}'", file_name);
          }
      }
  }
  ```

- [ ] **Step 3: 単体テストの実装とビルドテスト**
  ダミーのマークダウンファイルを作成し、`chunk_static_document` が正常にセクション分割されること、および `ingest_static_documents` が変更時のみ動作することをテストコードで検証。

---

## Task 3: gateway — 起動時およびリロード時のインジェスト呼び出し

ゲートウェイが動き始めた瞬間、および設定がリロード（SIGHUP、HTTP `/reload`）されたタイミングで、静的ドキュメントをバックグラウンドで非同期インジェストする。

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: 起動時のインジェスト追加**
  `crates/rustyclaw-gateway/src/lib.rs` 内で、ゲートウェイの各種サービスが開始される直前（`tracing::info!("RustyClaw Gateway is now running...")` の手前など）に、以下を追加：
  ```rust
  {
      let ws_path = PathBuf::from(&workspace_path);
      let cfg = config.clone();
      let db_path = ws_path.join("memory.db");
      tokio::spawn(async move {
          rustyclaw_agent::ingest_static_documents(&ws_path, &cfg, &db_path).await;
      });
  }
  ```

- [ ] **Step 2: リロードイベント処理内へのインジェスト追加**
  SIGHUP シグナル処理、および HTTP `/reload` 処理（`reload_rx.recv()` の後）の内部で、設定リロード成功後に以下を呼び出す：
  ```rust
  let ws_path = PathBuf::from(&workspace_path);
  let cfg = config.clone();
  let db_path = ws_path.join("memory.db");
  tokio::spawn(async move {
      rustyclaw_agent::ingest_static_documents(&ws_path, &cfg, &db_path).await;
  });
  ```

---

## Task 4: agent — 検索とプロンプト構築の拡張

`retrieve_rag_context` が `source` ごと（`memory`, `session`, `doc:*`）の混在結果を受け取れるようにし、システムプロンプト用の Markdown 生成部を拡張する。

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `format_rag_context` の更新**
  `source` が `doc:` で始まるドキュメント用チャンクを検出し、`## Relevant Specifications & Rules` としてフォーマットする。
  ```rust
  pub(crate) fn format_rag_context(items: &[(String, String, f32)]) -> String {
      if items.is_empty() { return String::new(); }

      let memory_items: Vec<&str> = items.iter()
          .filter(|(src, _, _)| src == "memory")
          .map(|(_, txt, _)| txt.as_str())
          .collect();
      let session_items: Vec<&str> = items.iter()
          .filter(|(src, _, _)| src == "session")
          .map(|(_, txt, _)| txt.as_str())
          .collect();
      let doc_items: Vec<&str> = items.iter()
          .filter(|(src, _, _)| src.starts_with("doc:"))
          .map(|(_, txt, _)| txt.as_str())
          .collect();

      let mut out = String::new();

      if !doc_items.is_empty() {
          out.push_str("\n\n## Relevant Specifications & Rules\n");
          out.push_str("Use the following guidelines and parameters for task execution:\n\n");
          for text in &doc_items {
              out.push_str(text);
              out.push('\n');
          }
      }

      if !memory_items.is_empty() {
          out.push_str("\n\n## Relevant Memory\n");
          out.push_str("The following memories are relevant to the current conversation:\n\n");
          for text in &memory_items {
              out.push_str(text);
              out.push('\n');
          }
      }

      if !session_items.is_empty() {
          out.push_str("\n\n## Relevant Past Sessions\n");
          out.push_str("The following session summaries are relevant to the current conversation:\n\n");
          for text in &session_items {
              out.push_str(text);
              out.push('\n');
          }
      }

      out
   }
  ```

---

## Task 5: agent — 固定プロンプトの最小化と履歴制限の緩和（見直し対応）

ドキュメント RAG 化によって生じるコンテキスト窓の空きを利用して、静的システムプロンプトの無駄な連結を廃止し、メッセージ履歴の上限を緩和する。

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `build_system_context` における `AGENTS.md` と `MEMORY.md` の読み込みを廃止**
  `crates/rustyclaw-agent/src/lib.rs` の `build_system_context` 内で、固定で読み込むファイルを変更する：
  ```rust
  // 変更前: let files = ["SOUL.md", "AGENTS.md", "MEMORY.md", "USER.md"];
  // 変更後:
  let files = ["SOUL.md", "USER.md"];
  ```
  ※ `AGENTS.md` (スキル一覧) と `MEMORY.md` (長期記憶) は Task 2 の RAG 化によって動的注入されるため、固定連結からは除外する。

- [ ] **Step 2: `get_history_message_limit` のハードキャップを緩和**
  `crates/rustyclaw-agent/src/lib.rs` の `get_history_message_limit` 内で、モデルの `context_window` に応じたメッセージ切り捨ての上限を大幅に拡張する：
  ```rust
  fn get_history_message_limit(&self, purpose: &str) -> usize {
      let model_cfg = self.config.get_model(purpose);
      let cw = self.config.model_list.iter()
          .find(|m| m.model == model_cfg.model_name && m.enabled)
          .and_then(|m| m.context_window.as_deref());
      let ctx = parse_context_window(cw);
      match ctx {
          0..=16_384       => 30,  // 従来: 10
          16_385..=32_768  => 50,  // 従来: 20
          32_769..=65_536  => 80,  // 従来: 40
          _                => 100, // 従来: 60〜80
      }
  }
  ```

- [ ] **Step 3: テストコードのアサーションを修正**
  `crates/rustyclaw-agent/src/lib.rs` の `test_get_history_message_limit_uses_context_window` 内のアサーション数値を、上記 Step 2 の変更に合わせて更新する：
  ```rust
  assert_eq!(p16k.get_history_message_limit("default"),  30);
  assert_eq!(p32k.get_history_message_limit("default"),  50);
  assert_eq!(p64k.get_history_message_limit("default"),  80);
  assert_eq!(p131k.get_history_message_limit("default"), 100);
  ```

---

## Task 6: 動作検証と確認

- [ ] **Step 1: アプリケーションのビルドとテストの実行**
  ```bash
  cargo build
  cargo test --all
  ```

- [ ] **Step 2: mtime またはハッシュ値ベース of 差分インジェスト動作検証**
  1. ゲートウェイを起動する。初回はログに `Ingested X chunks from 'AGENT.md'` が出力されることを確認。
  2. その後、リロード（`curl http://localhost:8080/reload`）を実行。今度は `Document 'AGENT.md' is unchanged. Skipping ingestion.` が出力され、Embedding API 呼び出しが発生しないことを確認。
  3. `AGENT.md` に適当な空行などを追記して再度リロード。今度は検知してインジェストが走ることを確認。

- [ ] **Step 3: 外部 LLM への送信プロンプトの圧縮効果確認**
  `memory/debug/llm` 下にダンプされる LLM I/O デバッグ JSON を確認し、プロンプトのシステムコンテキスト内に `## Relevant Specifications & Rules` として RAG 抽出された部分のみが注入され、`AGENT.md` の全文が含まれていない（大幅にトークンが節約されている）ことを実データで確認する。

- [ ] **Step 4: 履歴メッセージ保持件数の増加確認**
  対話を繰り返した際、デバッグログやダンプにおいて、会話履歴が以前より多く（かつコンテキスト制限に引っかからずに）LLM へ送信されていることを確認する。
