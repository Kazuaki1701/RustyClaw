> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の完了済みタスク)  
> **完了日**: 2026-06-06  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-06 (Phase 28b-4 / ISSUE-26 / ISSUE-27 / Phase 40-8)

## バグ修正

### Phase 28b-4: Heartbeat コンテキストオーバーフロー対策
- **完了日**: 2026-06-06
- **概要**: Deep Scan 時（04:00 / 06:00 付近）のツール呼び出し後コンテキスト肥大化による全モデル失敗・Discord 通知欠落を修正（9,812 tokens > Groq 6,000 上限）
- **関連計画書**: `docs/archive/plans/2026-06-06-phase28b4-heartbeat-context-overflow.md`

### ISSUE-26: `ingest_static_documents` の非再帰スキャンにより skills/*.md が未 ingest
- **完了日**: 2026-06-06
- **概要**: `read_dir`（非再帰）を再帰スキャンに変更し `skills/*/*.md` を対象に追加。約 48KB 分のスキル定義を RAG に登録
- **対象**: `crates/rustyclaw-agent/src/lib.rs`（`ingest_static_documents`）
- **関連計画書**: なし（単独修正）

### ISSUE-27: `execute_heartbeat` に RAG 注入がなく Heartbeat コンテキストに doc: チャンクが届かない
- **完了日**: 2026-06-06
- **概要**: `execute_heartbeat` 内で `retrieve_rag_context` を呼び出し heartbeat_prompt に関連チャンクを追記
- **対象**: `crates/rustyclaw-agent/src/lib.rs`（`execute_heartbeat`）
- **関連計画書**: なし（単独修正）

## 優先課題

### Phase 40-8: Local Embedding & Complete RAG Unification
- **完了日**: 2026-06-06
- **概要**: `fastembed-rs`（ONNX Runtime）による `intfloat/multilingual-e5-small`（384次元）ローカル Embedding 実装、SQLite マイグレーション（1024→384次元）完了。外部 API（Cloudflare）依存をゼロ化し RAG の完全ローカル完結を達成。
- **関連計画書**: `docs/archive/plans/2026-06-06-local-embedding-complete-rag-unification.md`
