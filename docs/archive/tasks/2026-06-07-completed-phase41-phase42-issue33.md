# 完了済みタスク — Phase 41-1 / Phase 42 / ISSUE-33

> アーカイブ日: 2026-06-07

---

## 優先課題

- `[x]` **Phase 41-1: Dashboard チャット RAG 活用（アプローチ C ハイブリッド）** (#12)
  - 設計書: [2026-06-07-dashboard-rag-design.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/plans/2026-06-07-dashboard-rag-design.md)
  - 計画書: [2026-06-07-dashboard-rag-implementation.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/plans/2026-06-07-dashboard-rag-implementation.md)
  - ADR: [001-dashboard-rag-approach-c-hybrid.md](file:///home/kazuaki/Projects/RustyClaw/docs/adr/001-dashboard-rag-approach-c-hybrid.md)

- `[x]` **Phase 42: Heartbeat RAG 精度・効率の最適化**（アイデアバックログより昇格）
  - `[x]` **42-A 完了**: 検索クエリの最適化 — `build_heartbeat_rag_query` で digest 末尾 10 行 + 固定テンプレートに切り替え済み
  - `[x]` **42-B**: オンデマンド・ステップ別 RAG: Heartbeat の各 Step 実行時に必要な知識のみを動的・個別に検索注入
  - `[x]` **42-C**: プロンプトキャッシュの最適化: 静的指示と動的 RAG 結果の境界整理でキャッシュヒット率を最大化
  - `[x]` **42-D**: 時間減衰リランキング: 検索結果に経過時間ペナルティ/ブーストを付与し直近の重要情報を優先
  - `[x]` **42-E**: イベント駆動インデックス同期: flush_memory() → ingest_memory_md() の既存実装で対応済みのためクローズ

---

## バグ修正

- `[x]` **ISSUE-33: Discord チャット向け RAG 改善 — クエリ拡張 + discord_top_k** (#13)
  - 案A: `execute_with_rig_agent` のクエリを直近 2〜3 ターン会話 + ユーザーメッセージに拡張
  - 案B: `EmbeddingConfig` に `discord_top_k: Option<usize>` を追加（heartbeat_top_k と同パターン）
  - 対象: `crates/rustyclaw-config/src/lib.rs`、`crates/rustyclaw-agent/src/lib.rs`、`production/config/*.json`
