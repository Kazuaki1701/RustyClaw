# Dashboard チャット RAG 活用 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dashboard チャットが Heartbeat 結果・cron セッションサマリー・過去の Dashboard 会話を RAG 経由で参照できるようにし、実運用の報告に対して会話するユースケースを支援する。

**Architecture:** アプローチ C（ハイブリッド）— ① EmbeddingConfig に `dashboard_top_k` フィールドを追加 ② Dashboard session_id を日付ローテーション化 ③ heartbeat-digest.md を Dashboard 専用動的注入 ④ 対象 cron ジョブ完了後にセッションサマリー RAG 化イベントを publish の 4 施策を組み合わせる。

**Tech Stack:** Rust 2024 edition, chrono 0.4（既存依存）, serde_json, std::fs（同期読み込み）

**関連設計書:** `docs/plans/2026-06-07-dashboard-rag-design.md`  
**関連 ADR:** `docs/adr/001-dashboard-rag-approach-c-hybrid.md`

---

## ファイル構成

| ファイル | 変更種別 | 変更内容 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | 修正 | `EmbeddingConfig` に `dashboard_top_k: Option<usize>` 追加 |
| `production/config/config.debug.json` | 修正 | `embedding.dashboard_top_k: 8` 追加 |
| `production/config/config.release.json` | 修正 | `embedding.dashboard_top_k: 8` 追加 |
| `crates/rustyclaw-gateway/src/health.rs` | 修正 | session_id を `http-dashboard-YYYYMMDD` に変更 |
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | heartbeat-digest 注入 + `retrieve_rag_context*` に `top_k: usize` 引数追加 + dashboard effective_top_k 計算 |
| `crates/rustyclaw-gateway/src/lib.rs` | 修正 | cron 完了後 session-summary publish（ホワイトリスト判定） |
| `docs/specs/06_dashboard_spec.md` | 修正 | session_id ローテーション・RAG 構成の追記 |

---

## Task 1: EmbeddingConfig に `dashboard_top_k` フィールドを追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs:97-126`（`EmbeddingConfig` 構造体）

- [ ] **Step 1: 既存の失敗テストを確認**

  現時点で `test_embedding_config_defaults` は `dashboard_top_k` を知らないため、新フィールドを追加後も既存テストが通ることを確認するためにまず現状を記録する。
  
  ```bash
  cargo test -p rustyclaw-config -- test_embedding_config 2>&1 | head -30
  ```
  Expected: すべて PASS（変更前の状態確認）

- [ ] **Step 2: 失敗テストを先に書く**

  `crates/rustyclaw-config/src/lib.rs` のテストセクション（`#[cfg(test)]` ブロック）に追加:

  ```rust
  #[test]
  fn test_embedding_config_dashboard_top_k_default() {
      let cfg: EmbeddingConfig = serde_json::from_str(r#"{}"#).unwrap();
      assert!(cfg.dashboard_top_k.is_none(), "dashboard_top_k default should be None");
  }

  #[test]
  fn test_embedding_config_dashboard_top_k_value() {
      let cfg: EmbeddingConfig =
          serde_json::from_str(r#"{"dashboard_top_k": 8}"#).unwrap();
      assert_eq!(cfg.dashboard_top_k, Some(8));
  }
  ```

- [ ] **Step 3: テストが失敗することを確認**

  ```bash
  cargo test -p rustyclaw-config -- test_embedding_config_dashboard_top_k 2>&1
  ```
  Expected: `error[E0560]: struct ... has no field named dashboard_top_k`（フィールド未定義）

- [ ] **Step 4: `EmbeddingConfig` にフィールドを追加**

  `crates/rustyclaw-config/src/lib.rs` の `EmbeddingConfig` 構造体末尾（`use_local_embedding` の後）に追加:

  ```rust
  /// ダッシュボードチャット専用の RAG 検索上限件数（省略時は top_k を使用）
  #[serde(default)]
  pub dashboard_top_k: Option<usize>,
  ```

  変更後の構造体末尾:
  ```rust
  pub struct EmbeddingConfig {
      // ... 既存フィールド ...
      #[serde(default)]
      pub use_local_embedding: bool,
      #[serde(default)]
      pub dashboard_top_k: Option<usize>,
  }
  ```

- [ ] **Step 5: テストが通ることを確認してコミット**

  ```bash
  cargo test -p rustyclaw-config -- test_embedding_config 2>&1
  ```
  Expected: すべて PASS

  ```bash
  git add crates/rustyclaw-config/src/lib.rs
  git commit -m "feat(config): Phase 41-1 EmbeddingConfig に dashboard_top_k フィールドを追加"
  ```

---

## Task 2: config.debug.json / config.release.json に `dashboard_top_k` を追加

**Files:**
- Modify: `production/config/config.debug.json:291-300`（`embedding` オブジェクト）
- Modify: `production/config/config.release.json`（同箇所）

- [ ] **Step 1: config.debug.json の `embedding` セクションに追加**

  現在:
  ```json
  "embedding": {
    "enabled": true,
    "use_local_embedding": true,
    "api_endpoint": "http://192.168.1.110:1234/v1/embeddings",
    "api_key": "lm-studio",
    "model": "intfloat/multilingual-e5-small",
    "dimensions": 384,
    "top_k": 5,
    "similarity_threshold": 0.60,
    "session_summary_ttl_days": 7
  }
  ```

  変更後:
  ```json
  "embedding": {
    "enabled": true,
    "use_local_embedding": true,
    "api_endpoint": "http://192.168.1.110:1234/v1/embeddings",
    "api_key": "lm-studio",
    "model": "intfloat/multilingual-e5-small",
    "dimensions": 384,
    "top_k": 5,
    "dashboard_top_k": 8,
    "similarity_threshold": 0.60,
    "session_summary_ttl_days": 7
  }
  ```

- [ ] **Step 2: config.release.json にも同様に追加**

  `production/config/config.release.json` の `embedding` セクションに `"dashboard_top_k": 8,` を `top_k` の直後に追加する（debug.json と同じ手順）。

- [ ] **Step 3: JSON パース検証**

  ```bash
  python3 -m json.tool production/config/config.debug.json > /dev/null && echo OK
  python3 -m json.tool production/config/config.release.json > /dev/null && echo OK
  ```
  Expected: 両方とも `OK`

- [ ] **Step 4: コミット**

  ```bash
  git add production/config/config.debug.json production/config/config.release.json
  git commit -m "feat(config): Phase 41-1 config に dashboard_top_k: 8 を追加"
  ```

---

## Task 3: health.rs の session_id を日付ローテーション化

**Files:**
- Modify: `crates/rustyclaw-gateway/src/health.rs:432`（`/chat` ハンドラ）

- [ ] **Step 1: 変更箇所を確認**

  ```bash
  grep -n "http-dashboard" crates/rustyclaw-gateway/src/health.rs
  ```
  Expected: `432:  let session_id = "http-dashboard".to_string();` の行が表示される

- [ ] **Step 2: 修正を適用**

  変更前:
  ```rust
  let session_id = "http-dashboard".to_string();
  ```

  変更後:
  ```rust
  let today = chrono::Local::now().format("%Y%m%d").to_string();
  let session_id = format!("http-dashboard-{}", today);
  ```

  `chrono` はすでに `crates/rustyclaw-gateway/Cargo.toml` に依存関係として記載されているため、`Cargo.toml` の変更は不要。

- [ ] **Step 3: ビルドで確認してコミット**

  ```bash
  cargo build -p rustyclaw-gateway 2>&1 | tail -5
  ```
  Expected: `Finished` または変更ファイルのみのコンパイル完了

  ```bash
  git add crates/rustyclaw-gateway/src/health.rs
  git commit -m "feat(gateway): Phase 41-1 dashboard session_id を YYYYMMDD 日付ローテーション化"
  ```

---

## Task 4: heartbeat-digest.md の Dashboard 専用動的注入

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`（`execute_with_tools` 関数、`build_system_context` 呼び出し直後）

- [ ] **Step 1: 挿入位置を確認**

  ```bash
  grep -n "build_system_context\|get_session_continuation" crates/rustyclaw-agent/src/lib.rs | head -10
  ```
  Expected: `execute_with_tools` 内の `build_system_context` 呼び出し行（約 1156 行）が表示される

- [ ] **Step 2: heartbeat-digest 注入ブロックを追加**

  `execute_with_tools` 内の `build_system_context` 呼び出しブロック（`get_session_continuation_context` の直後、RAG 注入の前）に以下を追加:

  変更前（約 line 1156-1162）:
  ```rust
  let mut system_context = self.build_system_context(workspace_dir)?;
  if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
  {
      system_context.push_str(&continuation);
  }

  // RAG: ユーザーメッセージに関連する記憶を動的注入
  ```

  変更後:
  ```rust
  let mut system_context = self.build_system_context(workspace_dir)?;
  if let Some(continuation) = self.get_session_continuation_context(workspace_dir, session_id)
  {
      system_context.push_str(&continuation);
  }

  // Dashboard 専用: heartbeat-digest.md を動的注入（fail-open）
  if session_id.contains("http-dashboard") {
      let digest_path = workspace_dir.join("memory").join("heartbeat-digest.md");
      if let Ok(digest) = std::fs::read_to_string(&digest_path) {
          if !digest.trim().is_empty() {
              system_context.push_str("\n\n## Latest Heartbeat Digest\n");
              system_context.push_str(&digest);
          }
      }
  }

  // RAG: ユーザーメッセージに関連する記憶を動的注入
  ```

- [ ] **Step 3: ビルドで確認してコミット**

  ```bash
  cargo build -p rustyclaw-agent 2>&1 | tail -5
  ```
  Expected: コンパイル成功

  ```bash
  git add crates/rustyclaw-agent/src/lib.rs
  git commit -m "feat(agent): Phase 41-1 Dashboard チャットに heartbeat-digest.md を動的注入"
  ```

---

## Task 5: RAG top_k を session_id に応じて切り替え（dashboard_top_k 適用）

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`（`retrieve_rag_context`, `retrieve_rag_context_local`, `execute_with_tools`, `execute_heartbeat`）

- [ ] **Step 1: `retrieve_rag_context` に `top_k: usize` 引数を追加**

  現在（約 line 2231）:
  ```rust
  pub(crate) async fn retrieve_rag_context(
      query_text: &str,
      config: &Config,
      rag_engine: &UnifiedRagEngine,
  ) -> String {
      // ...
      let top_k = config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
  ```

  変更後:
  ```rust
  pub(crate) async fn retrieve_rag_context(
      query_text: &str,
      config: &Config,
      rag_engine: &UnifiedRagEngine,
      top_k: usize,
  ) -> String {
      // `let top_k = ...` の行を削除し、引数の top_k をそのまま使用
  ```

- [ ] **Step 2: `retrieve_rag_context_local` に `top_k: usize` 引数を追加**

  現在（約 line 2270）:
  ```rust
  pub(crate) async fn retrieve_rag_context_local(
      query_text: &str,
      config: &Config,
      embed_client: &EmbedClientKind,
      db_path: &Path,
  ) -> String {
      // ...
      let top_k = config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
  ```

  変更後:
  ```rust
  pub(crate) async fn retrieve_rag_context_local(
      query_text: &str,
      config: &Config,
      embed_client: &EmbedClientKind,
      db_path: &Path,
      top_k: usize,
  ) -> String {
      // `let top_k = ...` の行を削除し、引数の top_k をそのまま使用
  ```

- [ ] **Step 3: `execute_with_tools` の呼び出し元を更新（dashboard_top_k 適用）**

  `execute_with_tools` の RAG 注入ブロック直前（Task 4 で追加した digest 注入の後）に `effective_top_k` を計算し、各呼び出しに渡す:

  変更前（約 line 1162-1183）:
  ```rust
  // RAG: ユーザーメッセージに関連する記憶を動的注入（rag が初期化済みの場合のみ）
  if self
      .config
      .embedding
      .as_ref()
      .map(|e| e.use_local_embedding)
      .unwrap_or(false)
  {
      if let Some(client) = make_embed_client(&self.config) {
          let db_path = workspace_dir.join("memory.db");
          let rag_ctx =
              retrieve_rag_context_local(user_message, &self.config, &client, &db_path).await;
          if !rag_ctx.is_empty() {
              system_context.push_str(&rag_ctx);
          }
      }
  } else if let Some(ref rag) = self.rag {
      let rag_ctx = retrieve_rag_context(user_message, &self.config, rag).await;
      if !rag_ctx.is_empty() {
          system_context.push_str(&rag_ctx);
      }
  }
  ```

  変更後:
  ```rust
  // RAG: ユーザーメッセージに関連する記憶を動的注入（rag が初期化済みの場合のみ）
  let effective_top_k = if session_id.contains("http-dashboard") {
      self.config
          .embedding
          .as_ref()
          .and_then(|e| e.dashboard_top_k)
          .unwrap_or_else(|| {
              self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5)
          })
  } else {
      self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5)
  };
  if self
      .config
      .embedding
      .as_ref()
      .map(|e| e.use_local_embedding)
      .unwrap_or(false)
  {
      if let Some(client) = make_embed_client(&self.config) {
          let db_path = workspace_dir.join("memory.db");
          let rag_ctx =
              retrieve_rag_context_local(user_message, &self.config, &client, &db_path, effective_top_k).await;
          if !rag_ctx.is_empty() {
              system_context.push_str(&rag_ctx);
          }
      }
  } else if let Some(ref rag) = self.rag {
      let rag_ctx = retrieve_rag_context(user_message, &self.config, rag, effective_top_k).await;
      if !rag_ctx.is_empty() {
          system_context.push_str(&rag_ctx);
      }
  }
  ```

- [ ] **Step 4: `execute_heartbeat` の呼び出し元を更新（標準 top_k を使用）**

  `execute_heartbeat` 内（約 line 714-725）の呼び出しに `top_k` を追加:

  変更前:
  ```rust
  if let Some(client) = make_embed_client(&self.config) {
      let rag_ctx =
          retrieve_rag_context_local(user_message, &self.config, &client, db_path).await;
      // ...
  } else if let Some(ref rag) = self.rag {
      let rag_ctx = retrieve_rag_context(user_message, &self.config, rag).await;
  ```

  変更後:
  ```rust
  let hb_top_k = self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
  if let Some(client) = make_embed_client(&self.config) {
      let rag_ctx =
          retrieve_rag_context_local(user_message, &self.config, &client, db_path, hb_top_k).await;
      // ...
  } else if let Some(ref rag) = self.rag {
      let rag_ctx = retrieve_rag_context(user_message, &self.config, rag, hb_top_k).await;
  ```

- [ ] **Step 5: ビルドと clippy で確認**

  ```bash
  cargo build -p rustyclaw-agent 2>&1 | tail -10
  cargo clippy -p rustyclaw-agent -- -D warnings 2>&1 | tail -20
  ```
  Expected: コンパイル成功、clippy エラーなし

- [ ] **Step 6: コミット**

  ```bash
  git add crates/rustyclaw-agent/src/lib.rs
  git commit -m "feat(agent): Phase 41-1 Dashboard RAG top_k を dashboard_top_k=8 に切り替え"
  ```

---

## Task 6: cron 完了後の session-summary publish（ホワイトリスト）

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（`Ok(response)` ブロック、`bus.publish(AgentResponse)` の直後）

- [ ] **Step 1: 挿入位置を確認**

  ```bash
  grep -n "AgentResponse\|channel_id: channel_id" crates/rustyclaw-gateway/src/lib.rs | head -10
  ```
  Expected: 約 line 702 に `bus.publish(SystemEvent::AgentResponse {` が表示される

- [ ] **Step 2: ホワイトリスト定数と publish ブロックを追加**

  `crates/rustyclaw-gateway/src/lib.rs` の先頭付近の定数定義エリア（モジュールレベル）にホワイトリスト定数を追加:

  ```rust
  const SUMMARIZE_CRON_SESSIONS: &[&str] = &[
      "cron:karakeep-cleanup",
      "cron:karakeep-recommendation",
      "cron:topic-patrol-explore",
      "cron:topic-patrol-deliver",
      "cron:vitals-morning",
      "cron:vitals-night",
      "cron:daily-briefing",
  ];
  ```

  次に `Ok(response)` ブロック内の `bus.publish(SystemEvent::AgentResponse {...})` の直後（約 line 705 の後）に追加:

  変更前（約 line 702-706）:
  ```rust
  let _ = bus.publish(SystemEvent::AgentResponse {
      session_id: session_id.clone(),
      channel_id: channel_id.clone(),
      content: response.content,
  });
  ```

  変更後:
  ```rust
  let _ = bus.publish(SystemEvent::AgentResponse {
      session_id: session_id.clone(),
      channel_id: channel_id.clone(),
      content: response.content,
  });

  // セッションサマリー RAG 化: ホワイトリスト cron ジョブ完了後に summary イベントを発行
  if SUMMARIZE_CRON_SESSIONS.contains(&session_id.as_str()) {
      let summary_session_id = format!("cron:session-summary:{}", session_id);
      if let Err(e) = bus.publish(SystemEvent::IncomingMessage {
          session_id: summary_session_id,
          user_id: "cron".to_string(),
          channel_id: "cron".to_string(),
          content: String::new(),
          priority: Priority::Background,
      }) {
          tracing::warn!("Failed to publish session-summary event: {:#}", e);
      }
  }
  ```

- [ ] **Step 3: ビルドと clippy で確認**

  ```bash
  cargo build -p rustyclaw-gateway 2>&1 | tail -10
  cargo clippy -p rustyclaw-gateway -- -D warnings 2>&1 | tail -20
  ```
  Expected: コンパイル成功、clippy エラーなし

- [ ] **Step 4: コミット**

  ```bash
  git add crates/rustyclaw-gateway/src/lib.rs
  git commit -m "feat(gateway): Phase 41-1 cron 完了後に session-summary RAG 化イベントを publish"
  ```

---

## Task 7: ワークスペース全体のビルド検証

**Files:** なし（検証のみ）

- [ ] **Step 1: フル workspace ビルド**

  ```bash
  cargo build --workspace 2>&1 | tail -15
  ```
  Expected: `Finished dev [unoptimized + debuginfo] target(s) in ...s`（エラーなし）

- [ ] **Step 2: cargo clippy（全ターゲット）**

  ```bash
  cargo clippy --all-targets 2>&1 | grep -E "^error|^warning\[" | head -20
  ```
  Expected: エラーなし（未使用変数 warning があれば修正）

- [ ] **Step 3: テスト実行**

  ```bash
  cargo test --workspace 2>&1 | tail -20
  ```
  Expected: すべてのテストが PASS

---

## Task 8: rp1 デプロイと動作確認

**Files:** なし（デプロイ・検証のみ）

- [ ] **Step 1: aarch64 クロスビルド**

  ```bash
  cargo build --release --target aarch64-unknown-linux-gnu 2>&1 | tail -10
  ```
  Expected: `Finished release [optimized] target(s) in ...s`

- [ ] **Step 2: rp1 にデプロイ**

  ```bash
  ./deploy.sh
  ```
  （`deploy.sh` は `docs/README.md §4` 参照。`production/deploy.sh` を実行することで rp1 へ転送・再起動される）

- [ ] **Step 3: 起動ログ確認**

  ```bash
  ssh rp1 "journalctl -u rustyclaw -n 50 --no-pager"
  ```
  Expected:
  - `Initializing agent` ログが出力される
  - `ingest_static_documents` 完了ログ（doc: チャンク登録）
  - エラーなし

- [ ] **Step 4: Dashboard チャットで動作確認**

  ブラウザで Dashboard の CHAT パネルを開き（`http://rp1:8080`）、以下のメッセージを送信:
  - 「今日の KaraKeep 推薦は？」
  - 「最近の Heartbeat 結果を教えて」

  Expected:
  - ログに `http-dashboard-20260607` の session_id が記録される
  - RAG 検索で `session:` チャンク（cron サマリー）がヒットする
  - `## Latest Heartbeat Digest` セクションがシステムプロンプトに注入される（debug_dump: true でログ確認可能）

---

## Task 9: docs/specs/06_dashboard_spec.md の更新

**Files:**
- Modify: `docs/specs/06_dashboard_spec.md`（session_id セクション・RAG 構成の追記）

- [ ] **Step 1: 更新内容を適用**

  `06_dashboard_spec.md` に以下のセクションを追記・更新する:
  - **session_id**: `"http-dashboard"` （固定）→ `"http-dashboard-YYYYMMDD"` （日付ローテーション）
  - **RAG 注入**: heartbeat-digest.md の動的注入（Dashboard 専用）、cron セッションサマリー RAG 化
  - **top_k**: Dashboard は `dashboard_top_k: 8`（通常 `top_k: 5`）

- [ ] **Step 2: コミット**

  ```bash
  git add docs/specs/06_dashboard_spec.md
  git commit -m "docs(spec): Phase 41-1 Dashboard session_id ローテーションと RAG 構成を仕様書に追記"
  ```

---

## 補足: 実施順序

Tasks 1→2（config が基盤）→3→4→5（agent の変更は Task 1 の型定義に依存）→6→7→8→9 の順で実施する。Task 7 でコンパイルエラーが出た場合は前のタスクに戻って修正する。
