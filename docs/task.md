# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-13（Phase 51-2 完了。Phase 52 を優先課題に昇格）  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-13-completed-phases-51-1-51-2.md](archive/tasks/2026-06-13-completed-phases-51-1-51-2.md) (Phase 51-1, 51-2, v0.4 本番バックアップ)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

- [x] **Phase 52-1: 全体共通の静的・基礎最適化**（完了: 2026-06-13）:  
  全ての用途におけるLLMリクエストの静的トークンおよび外部ツール（Gmail/Calendar/Keep）出力の削減・整理。
  - **詳細設計・計画書**: [2026-06-13-phase52-context-optimization-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/2026-06-13-phase52-context-optimization-design.md) / [2026-06-13-phase52-context-optimization-implementation-plan.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md)

- [x] **Phase 52-2: 用途別最適化 - Heartbeat**（完了: 2026-06-13）:  
  バックグラウンド自動処理（自動巡回監視）におけるプロンプト・ツール情報の極限スリム化。
  - **詳細設計・計画書**: [2026-06-13-phase52-context-optimization-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/2026-06-13-phase52-context-optimization-design.md) / [2026-06-13-phase52-context-optimization-implementation-plan.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md)

- [x] **Phase 52-3: 用途別最適化 - Chat**（完了: 2026-06-13、PreCompact/SessionStart は Phase 52-3b に分割）:  
  通常対話における動的フィルタリング（動的スキル選択、興味情報の動的注入、適応的クォータガード）と `context-mode` の完全統合。
  - **詳細設計・計画書**: [2026-06-13-phase52-context-optimization-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/2026-06-13-phase52-context-optimization-design.md) / [2026-06-13-phase52-context-optimization-implementation-plan.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md)

- [ ] **Phase 52-4: 用途別最適化 - Topic Patrol**:  
  外部ニュース・メール・RSS 巡回タスクのコンテキスト極小化。`ctx_fetch_and_index` による HTML→Markdown キャッシュと `ctx_search` RAG 巡回、特化型プロンプト適用。
  - **詳細設計・計画書**: [2026-06-13-phase52-context-optimization-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/2026-06-13-phase52-context-optimization-design.md) / [2026-06-13-phase52-context-optimization-implementation-plan.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md)

- [ ] **Phase 52-5: 長期記憶（MEMORY.md）のセマンティック分割（Memory RAG）**:  
  肥大化する `MEMORY.md` を `ctx_index` で SQLite FTS5 に分割登録し、通常対話時は `ctx_search` で関連チャンクのみを動的ロード。Memory Flush も `ctx_patch` による部分書き換えに移行。
  - **詳細設計・計画書**: [2026-06-13-phase52-context-optimization-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/2026-06-13-phase52-context-optimization-design.md) / [2026-06-13-phase52-context-optimization-implementation-plan.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md)

- [ ] **Phase 52-6: エピソード記憶連携とデイリーブリーフィングの高度化**:  
  日次ブリーフィング結果の自動 `ctx_index` 登録と、`ctx_search` による相関検索アドバイザリー（過去の同様状況の引き出し）。
  - **詳細設計・計画書**: [2026-06-13-phase52-context-optimization-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/2026-06-13-phase52-context-optimization-design.md) / [2026-06-13-phase52-context-optimization-implementation-plan.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md)

---

## 将来課題（低優先度）

- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
  - **再検討**: `SecureSandboxExecutor` 実装時に vault の per-call 解決（v0.3 相当）を導入。現在は起動時一括注入（Phase 49-2）だが、v0.5 では Rust が実行主体になるため `env: {"KEY": "$vault:key"}` 形式で最小限注入が自然に実現できる。

### v0.6 案件

- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み

### v0.4 残課題（`docs/specs/v0.4/` 精査 — 2026-06-12）

- [ ] **Dashboard 改善**: SETTING タブ（`GET/POST /api/config` + 2ステップ確定 UI）・RELOAD ボタン（既存 `GET /reload` をダッシュボードから呼び出す）
- [ ] **Dashboard: Rate Limit / Context Window リアルタイム表示**: `GET /api/rate-limits` エンドポイントを追加し、purpose ごとの `rpm/tpm/rpd/tpd`（設定値・分/日消費量）と `context_window_tokens` をダッシュボードに 10 秒ポーリング表示。実装時に `RateLimiter` のクレート間公開方式（スナップショット型）の ADR を起票する。関連: `docs/adr/005-rate-limiter-quota-enforcement.md`
