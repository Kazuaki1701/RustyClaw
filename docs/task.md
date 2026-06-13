# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-13（Phase 52-7 をアーカイブへ退避完了）  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-13-completed-phase52-all.md](archive/tasks/2026-06-13-completed-phase52-all.md) (Phase 52-1〜52-7 Context 最適化・reindexログ・エピソード記憶連携)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

## 将来課題（低優先度）

- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
  - **再検討**: `SecureSandboxExecutor` 実装時に vault の per-call 解決（v0.3 相当）を導入。現在は起動時一括注入（Phase 49-2）だが、v0.5 では Rust が実行主体になるため `env: {"KEY": "$vault:key"}` 形式で最小限注入が自然に実現できる。

### v0.6 案件

- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み



### v0.4 残課題（`docs/specs/v0.4/` 精査 — 2026-06-12）

- [ ] **Dashboard 改善**: SETTING タブ（`GET/POST /api/config` + 2ステップ確定 UI）・RELOAD ボタン（既存 `GET /reload` をダッシュボードから呼び出す）
- [ ] **Dashboard: Rate Limit / Context Window リアルタイム表示**: `GET /api/rate-limits` エンドポイントを追加し、purpose ごとの `rpm/tpm/rpd/tpd`（設定値・分/日消費量）と `context_window_tokens` をダッシュボードに 10 秒ポーリング表示。実装時に `RateLimiter` のクレート間公開方式（スナップショット型）の ADR を起票する。関連: `docs/adr/005-rate-limiter-quota-enforcement.md`
