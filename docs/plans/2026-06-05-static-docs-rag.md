# Static Docs RAG Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `AGENTS.md` / `skills/*.md` などの静的ドキュメントを差分インジェストで `memory.db` に永続化し、ユーザー入力との類似度で動的にシステムプロンプトへ注入することで、不要なコンテキスト送信を削減する。

**Architecture:** ①ファイルの SHA-256 ハッシュを `document_states` テーブルで管理し変更ファイルのみ再インジェスト、②起動時・リロード時に `tokio::spawn` でバックグラウンド実行、③`format_rag_context` が `doc:*` ソースを "Relevant Specifications & Rules" セクションとして整形して既存 RAG 注入パイプラインに乗せる。

**Tech Stack:** Rust 2024 edition, rusqlite (`rustyclaw-storage`), `CloudflareEmbeddingClient` (`rustyclaw-providers`), `sha2` クレート, tokio async

---

## ファイルマップ

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-storage/src/lib.rs` | `document_states` テーブル追加・`check_and_update_doc_state` メソッド追加 |
| `crates/rustyclaw-agent/Cargo.toml` | `sha2 = "0.10"` 追加 |
| `crates/rustyclaw-agent/src/lib.rs` | `chunk_static_document` + `ingest_static_documents` 追加、`format_rag_context` を `doc:*` 対応に拡張、`build_system_context` から `AGENTS.md` を除外、`get_history_message_limit` 上限を緩和 |
| `crates/rustyclaw-gateway/src/lib.rs` | 起動時・SIGHUP・HTTP `/reload` で `ingest_static_documents` を非同期トリガー |

---

## Task 1: storage — 差分インジェスト用ファイル変更検知スキーマの追加

RPi4 での無駄な Embedding API コールを防ぐため、ドキュメントのハッシュ値を管理するテーブルを用意し、変更があったファイルのみを再インジェストする。

**Files:**
- Modify: `crates/rustyclaw-storage/src/lib.rs`

- [ ] **Step 1: `check_and_update_doc_state` の失敗テストを書く**

  `crates/rustyclaw-storage/src/lib.rs` の `#[cfg(test)] mod tests` ブロック末尾に追加する：

  ```rust
  #[test]
  fn test_check_and_update_doc_state() {
      let dir = tempfile::tempdir().unwrap();
      let db_path = dir.path().join("test.db");
      let db = DbManager::new(&db_path).unwrap();

      // 新規ファイル → 変更あり (true)
      assert!(db.check_and_update_doc_state("AGENTS.md", "hash_v1").unwrap());
      // 同じハッシュ → 変更なし (false)
      assert!(!db.check_and_update_doc_state("AGENTS.md", "hash_v1").unwrap());
      // ハッシュ変更 → 変更あり (true)
      assert!(db.check_and_update_doc_state("AGENTS.md", "hash_v2").unwrap());
      // 別ファイル → 変更あり (true)
      assert!(db.check_and_update_doc_state("skills/test.md", "hash_v1").unwrap());
  }
  ```

- [ ] **Step 2: テストを実行してコンパイルエラー（未実装）を確認する**

  ```bash
  cargo test -p rustyclaw-storage test_check_and_update_doc_state 2>&1 | head -20
  ```

  Expected: `error[E0599]: no method named check_and_update_doc_state`

- [ ] **Step 3: `document_states` テーブルを DB 初期化に追加する**

  `crates/rustyclaw-storage/src/lib.rs` の `DbManager::new` 内にある `execute_batch` のテーブル定義 SQL 末尾（`CREATE INDEX IF NOT EXISTS idx_memory_embeddings_source` の直後）に追記する：

  ```sql
  CREATE TABLE IF NOT EXISTS document_states (
      file_path TEXT PRIMARY KEY,
      last_hash TEXT NOT NULL,
      updated_at TEXT NOT NULL
  );
  ```

- [ ] **Step 4: `check_and_update_doc_state` メソッドを実装する**

  `crates/rustyclaw-storage/src/lib.rs` の `delete_old_session_embeddings` の直後（`// ---- Seen Items ----` セクションの手前）に追加する：

  ```rust
  /// ファイルのハッシュ値を検証し、前回から変更されているかを判定する。
  /// 変更されている（または未登録）の場合は true を返し、ハッシュ値を更新する。
  /// Fail-closed: DB エラー時は true を返して再インジェストを促す。
  pub fn check_and_update_doc_state(&self, file_path: &str, current_hash: &str) -> Result<bool> {
      let mut stmt = self.conn.prepare(
          "SELECT last_hash FROM document_states WHERE file_path = ?1"
      )?;
      let mut rows = stmt.query([file_path])?;

      if let Some(row) = rows.next()? {
          let last_hash: String = row.get(0)?;
          if last_hash == current_hash {
              return Ok(false);
          }
      }

      self.conn.execute(
          "INSERT OR REPLACE INTO document_states (file_path, last_hash, updated_at)
           VALUES (?1, ?2, datetime('now'))",
          rusqlite::params![file_path, current_hash],
      )?;
      Ok(true)
  }
  ```

- [ ] **Step 5: テストを実行してパスすることを確認する**

  ```bash
  cargo test -p rustyclaw-storage test_check_and_update_doc_state -- --nocapture
  ```

  Expected: `test test_check_and_update_doc_state ... ok`

- [ ] **Step 6: 全 storage テストがパスすることを確認してコミットする**

  ```bash
  cargo test -p rustyclaw-storage
  git add crates/rustyclaw-storage/src/lib.rs
  git commit -m "feat(storage): add document_states table and check_and_update_doc_state"
  ```

---

## Task 2: agent — sha2 追加・チャンク分割・インジェストロジックの実装

**Files:**
- Modify: `crates/rustyclaw-agent/Cargo.toml`
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `chunk_static_document` の失敗テストを書く**

  `crates/rustyclaw-agent/src/lib.rs` のテストブロック末尾に追加する：

  ```rust
  #[test]
  fn test_chunk_static_document_splits_by_heading() {
      let content = "# Doc\n\n## Section A\nContent A line1\nContent A line2\n\n## Section B\nContent B\n";
      let chunks = chunk_static_document("test.md", content);
      assert_eq!(chunks.len(), 2, "## 見出し単位で 2 チャンクになること");
      assert!(chunks[0].contains("Section A"), "チャンク0 に Section A を含むこと");
      assert!(chunks[0].contains("[test.md >"), "ファイル名コンテキストが付くこと");
      assert!(chunks[1].contains("Section B"), "チャンク1 に Section B を含むこと");
  }

  #[test]
  fn test_chunk_static_document_truncates_long_chunks() {
      let long_line = "x".repeat(900);
      let content = format!("## Big\n{}", long_line);
      let chunks = chunk_static_document("big.md", &content);
      assert_eq!(chunks.len(), 1);
      assert!(chunks[0].chars().count() <= 800, "800文字を超えないこと");
  }
  ```

- [ ] **Step 2: テストを実行してコンパイルエラーを確認する**

  ```bash
  cargo test -p rustyclaw-agent test_chunk_static_document 2>&1 | head -10
  ```

  Expected: `error[E0425]: cannot find function chunk_static_document`

- [ ] **Step 3: `sha2` 依存関係を `Cargo.toml` に追加する**

  `crates/rustyclaw-agent/Cargo.toml` の `[dependencies]` 末尾に追記する：

  ```toml
  sha2 = "0.10"
  ```

- [ ] **Step 4: `chunk_static_document` を実装する**

  `crates/rustyclaw-agent/src/lib.rs` の `chunk_memory_md` 関数の直後（`ingest_memory_md` の手前）に追加する：

  ```rust
  /// 静的マークダウンを `##` / `###` 見出し単位でチャンク分割する。
  /// 各チャンク先頭に `[ファイル名 > 見出し]` の文脈を付与し、800 文字で切り捨てる。
  pub(crate) fn chunk_static_document(file_name: &str, content: &str) -> Vec<String> {
      let mut chunks: Vec<String> = Vec::new();
      let mut current_header = String::new();
      let mut current_body = String::new();

      for line in content.lines() {
          let trimmed = line.trim();
          if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
              let body = current_body.trim().to_string();
              if !body.is_empty() {
                  let raw = format!("[{} > {}]\n{}", file_name, current_header, body);
                  chunks.push(raw.chars().take(800).collect());
              }
              current_header = trimmed.to_string();
              current_body.clear();
          } else {
              current_body.push_str(line);
              current_body.push('\n');
          }
      }
      let body = current_body.trim().to_string();
      if !body.is_empty() {
          let raw = format!("[{} > {}]\n{}", file_name, current_header, body);
          chunks.push(raw.chars().take(800).collect());
      }
      chunks
  }
  ```

- [ ] **Step 5: テストを実行してパスすることを確認する**

  ```bash
  cargo test -p rustyclaw-agent test_chunk_static_document -- --nocapture
  ```

  Expected: 2 tests pass

- [ ] **Step 6: `ingest_static_documents` を実装する**

  `chunk_static_document` の直後、`ingest_memory_md` の手前に追加する。`CloudflareEmbeddingClient::new` は `(endpoint, api_key)` の 2 引数であることに注意（第3引数 model は endpoint に含まれる）。

  ```rust
  /// workspace_dir 内の AGENTS.md および skills/*.md をチャンク化し、
  /// ハッシュ差分がある場合のみ memory.db に embedding を保存する。Fail-open。
  pub async fn ingest_static_documents(
      workspace_dir: &std::path::Path,
      config: &Config,
      db_path: &std::path::Path,
  ) {
      let (api_endpoint, api_key, _model) = match config.get_embedding_client_params() {
          Some(p) => p,
          None => return,
      };
      let client = rustyclaw_providers::CloudflareEmbeddingClient::new(&api_endpoint, &api_key);
      let db = match rustyclaw_storage::DbManager::new(db_path) {
          Ok(d) => d,
          Err(e) => { tracing::warn!("ingest_static_documents: db open error: {}", e); return; }
      };

      // スキャン対象ファイル: AGENTS.md + skills/*.md
      let mut files = vec![workspace_dir.join("AGENTS.md")];
      let skills_dir = workspace_dir.join("skills");
      if let Ok(entries) = std::fs::read_dir(&skills_dir) {
          let mut skill_files: Vec<_> = entries
              .flatten()
              .map(|e| e.path())
              .filter(|p| p.is_file() && p.extension().map_or(false, |ext| ext == "md"))
              .collect();
          skill_files.sort(); // 順序を安定させる
          files.extend(skill_files);
      }

      for file_path in files {
          let content = match std::fs::read_to_string(&file_path) {
              Ok(c) => c,
              Err(_) => continue, // ファイルが存在しない場合はスキップ
          };
          let file_name = file_path.file_name()
              .and_then(|n| n.to_str())
              .unwrap_or("unknown");

          // SHA-256 ハッシュで差分検知
          use sha2::{Sha256, Digest};
          let hash_str = format!("{:x}", Sha256::digest(content.as_bytes()));

          let is_changed = db.check_and_update_doc_state(file_name, &hash_str)
              .unwrap_or(true); // エラー時は再インジェスト

          if !is_changed {
              tracing::debug!("ingest_static_documents: '{}' unchanged, skipping", file_name);
              continue;
          }

          tracing::info!("ingest_static_documents: '{}' changed, ingesting...", file_name);
          let chunks = chunk_static_document(file_name, &content);
          if chunks.is_empty() { continue; }

          // 旧 embedding を削除してから新規登録
          let source_id = format!("doc:{}", file_name);
          if let Err(e) = db.delete_embeddings_by_source(&source_id) {
              tracing::warn!("ingest_static_documents: delete error for '{}': {}", file_name, e);
              continue;
          }

          let text_refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
          match client.embed(&text_refs).await {
              Ok(embeddings) => {
                  for (i, (chunk, emb)) in chunks.iter().zip(embeddings.iter()).enumerate() {
                      let id = format!("doc-{}-{}", file_name.replace('.', "_"), i);
                      if let Err(e) = db.upsert_embedding(&id, &source_id, None, chunk, emb) {
                          tracing::warn!("ingest_static_documents: upsert error: {}", e);
                      }
                  }
                  tracing::info!("ingest_static_documents: ingested {} chunks from '{}'", chunks.len(), file_name);
              }
              Err(e) => {
                  tracing::warn!("ingest_static_documents: embed API error for '{}': {}", file_name, e);
                  // ハッシュ更新を取り消す（次回再試行させる）
                  let _ = db.check_and_update_doc_state(file_name, "");
              }
          }
      }
  }
  ```

- [ ] **Step 7: ビルドが通ることを確認してコミットする**

  ```bash
  cargo build -p rustyclaw-agent
  cargo test -p rustyclaw-agent
  git add crates/rustyclaw-agent/Cargo.toml crates/rustyclaw-agent/src/lib.rs
  git commit -m "feat(agent): add chunk_static_document and ingest_static_documents for static docs RAG"
  ```

---

## Task 3: gateway — 起動時・リロード時のインジェスト呼び出し

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: 起動時インジェストを追加する**

  `crates/rustyclaw-gateway/src/lib.rs` の `Gateway::run()` 内、`cron_svc.start();`（861行目付近）の直後に追記する。`db_path` は直前の行で定義済みであること、`config` は `let config = rustyclaw_config::load_config(...)` で取得済みであることに注意する：

  ```rust
  // ④ 静的ドキュメント RAG インジェスト（バックグラウンド、起動時）
  {
      let ws = self.workspace_path.clone();
      let cfg = config.clone();
      let db = db_path.clone();
      tokio::spawn(async move {
          rustyclaw_agent::ingest_static_documents(&ws, &cfg, &db).await;
      });
  }
  ```

- [ ] **Step 2: SIGHUP リロード後のインジェストを追加する**

  同ファイルの signal loop、SIGHUP アーム内の `registry.update_config(new_config.clone());` の直後（`// TODO: reload 時の RAG 再構築` コメントを置き換える）：

  ```rust
  // 静的ドキュメント RAG を再インジェスト（変更ファイルのみ）
  {
      let ws = self.workspace_path.clone();
      let cfg = new_config.clone();
      let db = self.workspace_path.join("memory.db");
      tokio::spawn(async move {
          rustyclaw_agent::ingest_static_documents(&ws, &cfg, &db).await;
      });
  }
  ```

- [ ] **Step 3: HTTP `/reload` アーム後のインジェストを追加する**

  同ファイルの `reload_rx.recv()` アーム内の `registry.update_config(new_config.clone());` の直後（`// TODO` コメントを置き換える）：

  ```rust
  // 静的ドキュメント RAG を再インジェスト（変更ファイルのみ）
  {
      let ws = self.workspace_path.clone();
      let cfg = new_config.clone();
      let db = self.workspace_path.join("memory.db");
      tokio::spawn(async move {
          rustyclaw_agent::ingest_static_documents(&ws, &cfg, &db).await;
      });
  }
  ```

- [ ] **Step 4: ビルドが通ることを確認してコミットする**

  ```bash
  cargo build -p rustyclaw-gateway
  git add crates/rustyclaw-gateway/src/lib.rs
  git commit -m "feat(gateway): trigger ingest_static_documents on startup and config reload"
  ```

---

## Task 4: agent — `format_rag_context` の `doc:*` ソース対応

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `doc:` ソースを含む `format_rag_context` の失敗テストを書く**

  既存テスト `test_format_rag_context_with_items`（2614行目付近）の直後に追加する：

  ```rust
  #[test]
  fn test_format_rag_context_with_doc_items() {
      let items = vec![
          ("doc:AGENTS.md".to_string(), "## Tool Usage\nUse tools carefully.".to_string(), 0.85_f64),
          ("memory".to_string(), "User prefers brevity".to_string(), 0.80_f64),
          ("session".to_string(), "Previously discussed weather".to_string(), 0.75_f64),
      ];
      let result = format_rag_context(&items);
      assert!(result.contains("## Relevant Specifications & Rules"), "doc: セクションが含まれること");
      assert!(result.contains("Tool Usage"), "doc: チャンク内容が含まれること");
      assert!(result.contains("## Relevant Memory"), "memory セクションが含まれること");
      assert!(result.contains("## Relevant Past Sessions"), "session セクションが含まれること");
      // doc セクションが memory セクションより先に来ること
      let doc_pos = result.find("## Relevant Specifications").unwrap();
      let mem_pos = result.find("## Relevant Memory").unwrap();
      assert!(doc_pos < mem_pos, "doc セクションが memory より先であること");
  }
  ```

- [ ] **Step 2: テストを実行して失敗することを確認する**

  ```bash
  cargo test -p rustyclaw-agent test_format_rag_context_with_doc_items -- --nocapture
  ```

  Expected: `FAILED` — `## Relevant Specifications & Rules` が出力されない

- [ ] **Step 3: `format_rag_context` を更新する**

  `crates/rustyclaw-agent/src/lib.rs` の既存 `format_rag_context` 関数（1681行目付近）を以下で置き換える。シグネチャ `items: &[(String, String, f64)]` は変わらない：

  ```rust
  /// RAG 検索結果をシステムプロンプト注入用の Markdown に変換する。
  /// source が "doc:*" → "Relevant Specifications & Rules"（先頭）
  /// source が "memory" → "Relevant Memory"
  /// source が "session" → "Relevant Past Sessions"
  pub(crate) fn format_rag_context(items: &[(String, String, f64)]) -> String {
      if items.is_empty() { return String::new(); }

      let doc_items: Vec<&str> = items.iter()
          .filter(|(src, _, _)| src.starts_with("doc:"))
          .map(|(_, txt, _)| txt.as_str())
          .collect();
      let memory_items: Vec<&str> = items.iter()
          .filter(|(src, _, _)| src == "memory")
          .map(|(_, txt, _)| txt.as_str())
          .collect();
      let session_items: Vec<&str> = items.iter()
          .filter(|(src, _, _)| src == "session")
          .map(|(_, txt, _)| txt.as_str())
          .collect();

      let mut out = String::new();

      if !doc_items.is_empty() {
          out.push_str("\n\n## Relevant Specifications & Rules\n");
          out.push_str("Use the following guidelines for task execution:\n\n");
          for text in &doc_items { out.push_str(text); out.push('\n'); }
      }
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

- [ ] **Step 4: テストを実行してパスすることを確認する**

  ```bash
  cargo test -p rustyclaw-agent test_format_rag_context -- --nocapture
  ```

  Expected: 全 `test_format_rag_context_*` テストが pass

- [ ] **Step 5: コミットする**

  ```bash
  git add crates/rustyclaw-agent/src/lib.rs
  git commit -m "feat(agent): extend format_rag_context to handle doc:* sources as Relevant Specifications"
  ```

---

## Task 5: agent — `build_system_context` から `AGENTS.md` 除外 + `get_history_message_limit` 緩和

AGENTS.md のスキル一覧は RAG で動的注入されるため、固定連結から外す。コンテキスト削減分を会話履歴の保持件数に回す。

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: `get_history_message_limit` のテストのアサーションを先に更新する**

  `crates/rustyclaw-agent/src/lib.rs` の `test_get_history_message_limit_uses_context_window`（2566行目付近）のアサーション 4 行を以下に書き換える：

  ```rust
  assert_eq!(p16k.get_history_message_limit("default"),  30, "16k → 30件");
  assert_eq!(p32k.get_history_message_limit("default"),  50, "32k → 50件");
  assert_eq!(p64k.get_history_message_limit("default"),  80, "64k → 80件");
  assert_eq!(p131k.get_history_message_limit("default"), 100, "131k → 100件");
  assert_eq!(p256k.get_history_message_limit("default"), 120, "256k → 120件");
  ```

- [ ] **Step 2: テストを実行して失敗することを確認する**

  ```bash
  cargo test -p rustyclaw-agent test_get_history_message_limit -- --nocapture
  ```

  Expected: `FAILED` — 現在の値（10/20/40/60/80）とアサーション（30/50/80/100/120）が合わない

- [ ] **Step 3: `get_history_message_limit` の実装を更新する**

  `crates/rustyclaw-agent/src/lib.rs` の `get_history_message_limit` 本体（111行目付近）の `match ctx` ブランチを書き換える：

  ```rust
  fn get_history_message_limit(&self, purpose: &str) -> usize {
      let model_cfg = self.config.get_model(purpose);
      let cw = self.config.model_list.iter()
          .find(|m| m.model == model_cfg.model_name && m.enabled)
          .and_then(|m| m.context_window.as_deref());
      let ctx = parse_context_window(cw);
      match ctx {
          0..=16_384       => 30,
          16_385..=32_768  => 50,
          32_769..=65_536  => 80,
          65_537..=262_143 => 100,
          _                => 120,
      }
  }
  ```

- [ ] **Step 4: テストを実行してパスすることを確認する**

  ```bash
  cargo test -p rustyclaw-agent test_get_history_message_limit -- --nocapture
  ```

  Expected: PASS

- [ ] **Step 5: `build_system_context` から `AGENTS.md` を除外する**

  `crates/rustyclaw-agent/src/lib.rs` の `build_system_context`（135行目付近）の files 配列を書き換える：

  ```rust
  // 変更前
  let files = ["SOUL.md", "AGENTS.md", "USER.md"];
  // 変更後
  let files = ["SOUL.md", "USER.md"];
  ```

  ※ AGENTS.md は Task 2 の `ingest_static_documents` → RAG 検索 → `format_rag_context` の "Relevant Specifications & Rules" として動的注入される。

- [ ] **Step 6: 全テストがパスすることを確認してコミットする**

  ```bash
  cargo test -p rustyclaw-agent
  git add crates/rustyclaw-agent/src/lib.rs
  git commit -m "feat(agent): remove AGENTS.md from static prompt; relax get_history_message_limit caps"
  ```

---

## Task 6: 統合ビルドと動作検証

- [ ] **Step 1: ワークスペース全体のビルドとテストを実行する**

  ```bash
  cargo build --workspace
  cargo test --workspace
  ```

  Expected: 全クレートがビルド成功、全テスト pass

- [ ] **Step 2: 差分インジェストの動作を確認する（`--no-agent` 起動）**

  ```bash
  # ゲートウェイを起動
  cargo run -p rustyclaw-cli -- gateway --no-agent
  ```

  起動ログに以下が出力されることを確認する：
  - `ingest_static_documents: 'AGENTS.md' changed, ingesting...`
  - `ingest_static_documents: ingested X chunks from 'AGENTS.md'`

  次に `/reload` を呼んで「変更なし」が検知されることを確認する：
  ```bash
  curl http://localhost:8080/reload
  # ログ: ingest_static_documents: 'AGENTS.md' unchanged, skipping
  ```

  `AGENTS.md` を 1 文字変更して再度 `/reload`：
  ```bash
  echo "" >> production/workspace/AGENTS.md
  curl http://localhost:8080/reload
  # ログ: ingest_static_documents: 'AGENTS.md' changed, ingesting...
  ```

- [ ] **Step 3: LLM プロンプトへの動的注入を確認する**

  `production/workspace/memory/debug/llm/` 以下に出力される LLM I/O ダンプを確認し、システムプロンプト内に `## Relevant Specifications & Rules` が現れ、`AGENTS.md` の全文が含まれていないことを確認する。

- [ ] **Step 4: 最終コミット**

  ```bash
  git add -p  # 未コミットの残りがあれば
  git commit -m "docs(plan): mark static-docs-rag implementation complete"
  ```
