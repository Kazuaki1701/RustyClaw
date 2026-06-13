# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-13（Phase 52-7 を優先課題に追加）  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-13-completed-phase52-all.md](archive/tasks/2026-06-13-completed-phase52-all.md) (Phase 52-1〜52-6 Context 最適化・Memory RAG・エピソード記憶連携)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

- [ ] **Phase 52-7: ctx_search sort 最適化・reindex ログ修正**:  
  `try_ctx_search` に sort 引数を追加して用途別（`timeline`/`relevance`）に最適化し、`reindex_memory_after_flush` の誤「完了」ログおよびデバッグレベル不一致を修正する。
  - **詳細設計・計画書**: [2026-06-13-phase52-7-search-log-improvements.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-7-search-log-improvements.md)

---

## 将来課題（低優先度）

- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
  - **再検討**: `SecureSandboxExecutor` 実装時に vault の per-call 解決（v0.3 相当）を導入。現在は起動時一括注入（Phase 49-2）だが、v0.5 では Rust が実行主体になるため `env: {"KEY": "$vault:key"}` 形式で最小限注入が自然に実現できる。

### v0.6 案件

- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み

### Phase 52 後続改善候補（2026-06-13 最終レビューで抽出）

- [ ] **ctx_search の sort 戦略を用途別に分離**: 現在 `try_ctx_search` は常に `sort=timeline`（時系列順）固定だが、バイタル相関検索（`extract_vital_alert_query`）では `sort=relevance` の方が過去の類似エピソードを正確に引けるため、sort を引数化して用途に応じて切り替えられるようにする。対象: `crates/rustyclaw-gateway/src/lib.rs` の `try_ctx_search` 関数。
- [ ] **reindex_memory_after_flush の誤「完了」ログ修正**: 全 `ctx_index` 呼び出しが失敗した場合でも「N チャンク再インデックス完了」と `info` ログが出る。成功カウンタを別途保持し、失敗件数を `warn` で出力するよう修正する。対象: `crates/rustyclaw-agent/src/lib.rs` の `reindex_memory_after_flush`。
- [ ] **agent 側 ctx_index 失敗ログを warn に統一**: `reindex_memory_after_flush` 内の `ctx_index` 失敗が `debug` レベルで記録されており、本番ログでは不可視。gateway 側の `try_ctx_index`（`warn` レベル）と統一する。対象: `crates/rustyclaw-agent/src/lib.rs:1856` 付近。
- [ ] **Phase 52-5b: ctx_patch 部分メモリ書き換え**: Memory Flush 時に LLM がメモリ全文を出力するのを禁止し、変更セクションのみを XML タグ形式で出力させて `ctx_patch` で部分書き換えする。Phase 52-5 で除外したスコープ。

### v0.4 残課題（`docs/specs/v0.4/` 精査 — 2026-06-12）

- [ ] **Dashboard 改善**: SETTING タブ（`GET/POST /api/config` + 2ステップ確定 UI）・RELOAD ボタン（既存 `GET /reload` をダッシュボードから呼び出す）
- [ ] **Dashboard: Rate Limit / Context Window リアルタイム表示**: `GET /api/rate-limits` エンドポイントを追加し、purpose ごとの `rpm/tpm/rpd/tpd`（設定値・分/日消費量）と `context_window_tokens` をダッシュボードに 10 秒ポーリング表示。実装時に `RateLimiter` のクレート間公開方式（スナップショット型）の ADR を起票する。関連: `docs/adr/005-rate-limiter-quota-enforcement.md`
