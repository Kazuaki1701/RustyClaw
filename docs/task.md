# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-13（Phase 52-8〜11 完了・main にマージ済み）  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-13-completed-phase52-all.md](archive/tasks/2026-06-13-completed-phase52-all.md) (Phase 52-1〜52-7 Context 最適化・reindexログ・エピソード記憶連携)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.4/`](specs/v0.4/) 各仕様ファイルの「将来拡張」節で管理しています。

## 完了済み（2026-06-13 マージ）

- [x] **Phase 52-8: ctx_execute_file ストリーム抽出および自動圧縮の実装** — `AGENTS.md` に指針追加 + `HeartbeatToolWrapper` の `truncate_70_20` を `ctx_index`/`ctx_search` 意味濃縮に置換
- [x] **Phase 52-9: 日またぎウォームスタート注入** — `generate_session_guide_xml` 実装 + `memory/sessions/YYYY-MM-DD.xml` 永続化 + `build_system_context` 先頭へ自動注入
- [x] **Phase 52-10: build_system_context 並列ロード** — `async fn` 化 + `tokio::join!` で SOUL.md/USER.md 並列ロード
- [x] **Phase 52-11: HEARTBEAT.md 動的ステップ選択** — `select_heartbeat_steps` で lastChecks に基づく Step 2/3/4 スキップ

## 一般対応案件（優先度 中）

### Phase 53: Dashboard 管理機能の強化と統計表示の統合 (v0.4 残課題)

- [ ] **Phase 53-1: Dashboard 改善 (SETTING / RELOAD)**
  - **内容**: SETTING タブ（`GET/POST /api/config` + 2ステップ確定 UI）・RELOAD ボタン（既存 `GET /reload` をダッシュボードから呼び出す）の実装。
- [ ] **Phase 53-2: Dashboard: Rate Limit / Context Window リアルタイム表示**
  - **内容**: `GET /api/rate-limits` エンドポイントを追加し、purpose ごとの `rpm/tpm/rpd/tpd`（設定値・分/日消費量）と `context_window_tokens` をダッシュボードに 10 秒ポーリング表示。実装時に `RateLimiter` のクレート間公開方式（スナップショット型）の ADR を起票する。関連: [docs/adr/005-rate-limiter-quota-enforcement.md](file:///home/kazuaki/Projects/RustyClaw/docs/adr/005-rate-limiter-quota-enforcement.md)
- [ ] **Phase 53-3: Dashboard: `ctx_stats` によるコンテキスト削減量・トークン統計のライブ表示**
  - **内容**: `GET /api/context/stats` エンドポイントを追加し、`context-mode` から MCP `ctx_stats` 経由で取得したトークン消費量、累積削減率、ツール別統計をダッシュボードの STATS 画面にライブ表示する。

## 将来課題（低優先度）

- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
  - **再検討**: `SecureSandboxExecutor` 実装時に vault の per-call 解決（v0.3 相当）を導入。現在は起動時一括注入（Phase 49-2）だが、v0.5 では Rust が実行主体になるため `env: {"KEY": "$vault:key"}` 形式で最小限注入が自然に実現できる。

### v0.6 案件

- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み
