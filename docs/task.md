# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-11  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md](archive/tasks/2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md) (Phase 45-1, 28b-3, 47-1, 48-1)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

- [ ] **Phase 49-1: SKILL.md run_workspace_script → ctx_execute 移行**
  - v0.4 で `workspace_execute_script`（旧 `run_workspace_script`）を削除したため、スクリプト実行スキルが全断線
  - 影響・残存ファイル：
    - **コア指示ファイル**: `SOUL.md`, `USER.md` (vault.json 記述注意), `AGENTS.md`, `HEARTBEAT.md`, `MEMORY.md`
    - **個別スキル指示書 (SKILL.md)**: `daily-briefing`, `deep-research`, `session-logs`, `todo-tracker`, `topic-patrol`, `workspace`
    - **スクリプトコメント内**: `session-logs/scripts/session-search.sh`, `session-stats.sh`
  - 手順: ① `ctx_execute` の実スキーマ確認（context-mode 側） ② 各ファイル内のツール名・パラメータを `ctx_execute` へ書き換え、`vault.json` の平文読み込み記述を排除
  - 対象: `production/workspace/` 配下の該当 Markdown およびスクリプトファイル

---

## 将来課題（低優先度）

- [ ] **Dashboard SETTING タブ**: `GET/POST /api/config` + 2ステップ確定 UI
- [ ] **Dashboard RELOAD ボタン**: 既存 `GET /reload` エンドポイントをダッシュボードから呼び出す
- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み
