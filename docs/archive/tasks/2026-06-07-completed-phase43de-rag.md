# 完了済みタスク: Phase 43-D & Phase 43-E (RAG最適化)

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-06-07  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

---

## 完了タスク一覧

### Phase 43-D: 実運用ルールの動的適用（RAG-based Guardrails）
- `[x]` **1. 実運用ルールの動的適用（RAG-based Guardrailsによるシステムプロンプト最適化）**
  - 増加するスキル仕様（`skills/**/*.md`）やユーザー固有の対応ルール（`USER.md`）を RAG に置き、現在の会話に関連する特定の運用ルールのみを動的にシステムプロンプトに注入することで、トークン削減と指示追従性を両立する。

### Phase 43-E: 階層型 RAG（Parent-Child Chunking）
- `[x]` **1. 階層型 RAG（Parent-Child Chunking）の導入**
  - 検索用の細かい「子チャンク（100-300文字）」でベクトル検索し、ヒット時に文脈を保持した「親チャンク（1000-3000文字）」を引き出す。`skills/**/*.md` や長期記憶 (`MEMORY.md`) の検索において、メタデータと SQLite の親子リレーション拡張だけで完結するため、推論負荷を増やさずに実運用でのスキル実行成功率を高める。
  - SQLite テーブル `memory_embeddings` に `parent_id` カラムを追加し、インジェスト時に親チャンクと子チャンクの紐付けと ID 設計。
  - ベクトル類似度検索の LEFT JOIN による親テキスト解決、および UnifiedRagEngine の重複排除の設計・実装。
  - `memory/logs/*.md` に 14 日間の TTL を適用して古い embeddings をクレンジング。
