# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-11  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-11-completed-bug-05.md](archive/tasks/2026-06-11-completed-bug-05.md) (BUG-05)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

- [ ] **Phase 45-1: v0.4 ctx_index/ctx_search エージェント統合** ← v0.4 の本丸
  - Phase 01 で `ingest_session_summary` を削除したまま代替未実装。記憶蓄積の穴が残っている。
  - セッション終了後に `ctx_index` でサマリを登録、Heartbeat・Discord で `ctx_search` を使う実装
  - 対象: `crates/rustyclaw-agent/src/lib.rs`（"Phase 02 で ctx_search へ移行予定" コメント箇所）

- [ ] **Phase 28b-3: Dashboard LANE QUEUE 表示名フォーマット変更**
  - `{cron title} ({HH:MM})` 形式に変更
  - 対象: `crates/rustyclaw-gateway/src/lib.rs`（cron キュー登録箇所）

---

## 一般課題

- [ ] **Phase 47-1: 非同期ローリング要約 (async-summary-proto) マージ**
  - worktree: `.claude/worktrees/feature+async-summary-proto`（branch: `worktree-feature+async-summary-proto`）
  - 設計完了済み・実装プラン作成 → バリデーション → main マージの順で着手

- [ ] **Phase 48-1: croner crate 置き換え**
  - `next_run_epoch()` / `compute_schedule()` を `crates/rustyclaw-gateway/src/cron.rs` で croner に移行
  - 計画書参照: `docs/2026-06-03-external-crate-replacement-analysis.md`（Phase B 小規模）

---

## 将来課題（低優先度）

- [ ] **Dashboard SETTING タブ**: `GET/POST /api/config` + 2ステップ確定 UI
- [ ] **Dashboard RELOAD ボタン**: 既存 `GET /reload` エンドポイントをダッシュボードから呼び出す
- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み
