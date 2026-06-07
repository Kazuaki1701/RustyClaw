# 完了済みタスク — Phase 43 / ISSUE-34

> アーカイブ日: 2026-06-07

---

## 優先課題

- `[x]` **Phase 43: RAG 最適化（旧 Context 削減策の廃止）**
  - `[x]` **Phase 43-A: RAG 最適化 Heartbeat**
    - `[x]` chunk_memory_md: section prefix + 隣接バレット結合（800 chars）
    - `[x]` flush_memory: Δ 閾値 6→3、5000 byte 上限廃止、truncate_70_20 廃止
    - `[x]` heartbeat_top_k: 2→3（TPM 安全マージン確保）
    - `[x]` USER.md を ingest_static_documents の RAG コーパスに追加
    - 設計書: [docs/archive/plans/2026-06-07-phase43a-rag-optimized-heartbeat-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/plans/2026-06-07-phase43a-rag-optimized-heartbeat-design.md)
    - 計画書: [docs/archive/plans/2026-06-07-phase43a-rag-optimized-heartbeat.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/plans/2026-06-07-phase43a-rag-optimized-heartbeat.md)
  - `[x]` **Phase 43-B: RAG 最適化 Dashboard**
    - `execute_with_tools` dead code 削除、`dashboard_top_k` 設定フィールド除去
    - Gateway trigger ラベルバグ修正（`http-dashboard` セッションが `"unknown"` → `"dashboard"` に）
    - 設計書: [docs/plans/2026-06-07-phase43b-dashboard-unification.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-phase43b-dashboard-unification.md)
    - 計画書: [docs/plans/2026-06-07-phase43b-implementation.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-phase43b-implementation.md)
  - `[x]` **Phase 43-C: RAG 最適化 Discord**
    - `discord_top_k` → `channel_top_k` リネーム（LINE / Discord / Dashboard 共通設定として明示）
    - 設計書: [docs/plans/2026-06-07-phase43c-channel-top-k-unification.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-phase43c-channel-top-k-unification.md)
    - 計画書: [docs/plans/2026-06-07-phase43c-implementation.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-phase43c-implementation.md)

---

## バグ修正

- `[x]` **ISSUE-34: Discord RAG `history_for_rag` のエラーハンドリングを `history_messages` と統一**
  - `execute_with_rig_agent` 内で `history_for_rag` ロード時に `unwrap_or_default()` を使用しているが、直後の `history_messages` ロードは `.context("Failed to load session history")?` で明示エラーを返す
  - 対象: `crates/rustyclaw-agent/src/lib.rs` の `history_for_rag` 変数（旧コミット 442a941 由来）
  - 対処: 後続コミット（9500471）で `history_for_rag` 変数自体が除去済み。コードレベルでは解決済みとしてクローズ。
