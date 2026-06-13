# 完了済みタスク — 2026-06-13

## Phase 51-1: LLM config 制限の適切な適用

- **完了日**: 2026-06-12
- **計画書**: `docs/plans/2026-06-12-phase51-1-llm-config-constraints.md`
- **概要**: `LlmConfig` に定義された各種制限をパイプライン全体で正しく参照・適用。`LlmModelConfig` に `context_window_tokens`・`rpm/rpd/tpm/tpd` を追加し `resolve_model()` で確定。`get_history_message_limit()` をトークン推計ベース予算管理（chars × 1.5）に置き換え。

## Phase 51-2: LANE QUEUE Memory Flush 可視化

- **完了日**: 2026-06-13
- **計画書**: `docs/plans/2026-06-13-phase51-2-lane-queue-flush-visibility.md`
- **概要**: `memory flush` と Session Summary の LLM 実行を LANE QUEUE に表示。コールバック注入（Option A）: `Pipeline.with_flush_callbacks()` + `SERVICE_BADGES` 追加。

## v0.4 残課題: 本番自動バックアップ

- **完了日**: 2026-06-13 以前（実装済み確認）
- **計画書**: なし（`docs/specs/v0.4/08_deployment.md §将来拡張` 参照）
- **概要**: `workspace/`（`memory.db`・`sessions/*.jsonl`・`patrol/findings.md`）を定時 rsync で NAS（QNAP 等）へ退避。
