# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-12（Phase 51-1 完了。v0.4 残課題: Context 最適化・Dashboard 改善）  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md](archive/tasks/2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md) (Phase 45-1, 28b-3, 47-1, 48-1)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

- [x] **Phase 51-1: LLM config 制限の適切な適用**（完了 2026-06-12）  
  `LlmConfig` に定義された各種制限をパイプライン全体で正しく参照・適用する。

  **現状の問題点（コード精査 2026-06-12）:**
  1. `LlmModelConfig`（解決済み設定）に `context_window` / `rpm` / `rpd` / `tpm` / `tpd` が含まれず、各利用箇所が `model_list` を再サーチしている
  2. `get_history_message_limit()` がメッセージ件数の粗いステップ制限（30/50/80/100/120）— トークン推計ベースでない
  3. `rpm` / `rpd` / `tpm` / `tpd` が config に定義済みだが enforce されていない
  4. system prompt + history + user message の合計トークン推計を事前に行う仕組みがない

  **対応スコープ:**
  - `LlmModelConfig` に `context_window_tokens: usize`・`rpm/rpd/tpm/tpd: Option<u64>` を追加し `resolve_model()` / `get_model()` で確定
  - `get_history_message_limit()` をトークン推計ベース予算管理に置き換え（chars × 1.5 で近似）
  - `rpm` / `tpm` のソフトリミット適用（超過時はスリープ or スキップ、`warn!` ログ）
  - 対象: `crates/rustyclaw-config/src/lib.rs`・`crates/rustyclaw-agent/src/lib.rs`

---

## Context Window 最適化

- [ ] **Memory Flush のコンテキスト最適化**:  
  `memory flush` 実行時における LLM リクエストおよびレスポンスのトークン数節約、およびコンテキスト窓（32k）の効率的な管理。
  - **内容**: XMLデリミタへの移行、システムプロンプトの圧縮、会話履歴のクレンジング、長期的なメモリセマンティック分割（RAG化）。
  - **詳細設計・改善提案**: [2026-06-13-memory-flush-context-improvement-proposal.md](file:///home/kazuaki/Projects/RustyClaw/docs/review/2026-06-13-memory-flush-context-improvement-proposal.md)

---

## 将来課題（低優先度）

- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
  - **再検討**: `SecureSandboxExecutor` 実装時に vault の per-call 解決（v0.3 相当）を導入。現在は起動時一括注入（Phase 49-2）だが、v0.5 では Rust が実行主体になるため `env: {"KEY": "$vault:key"}` 形式で最小限注入が自然に実現できる。

### v0.6 案件

- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み

### v0.4 残課題（`docs/specs/v0.4/` 精査 — 2026-06-12）

- [ ] **Context 最適化**: Heartbeat Digest・Session-level Summary・ContextBuilder 予算分割（70/20/10）の段階実装。Phase 51-1 で履歴件数トークン予算式・小コンテキスト時プロンプト圧縮を先行実装済み。詳細: `docs/specs/v0.4/92_llm_config_constraints.md` §6.3・`memory/project_context_management_plan.md`
- [ ] **Dashboard 改善**: SETTING タブ（`GET/POST /api/config` + 2ステップ確定 UI）・RELOAD ボタン（既存 `GET /reload` をダッシュボードから呼び出す）
- [x] **本番自動バックアップ**: `workspace/`（`memory.db`・`sessions/*.jsonl`・`patrol/findings.md`）を定時 rsync で NAS（QNAP 等）へ退避。`08_deployment.md §将来拡張` 参照
