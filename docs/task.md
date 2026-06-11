# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-12（v0.4 残課題を将来課題に追加）  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md](archive/tasks/2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md) (Phase 45-1, 28b-3, 47-1, 48-1)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

（現在アクティブな優先課題なし）

---

## 将来課題（低優先度）

- [ ] **Dashboard SETTING タブ**: `GET/POST /api/config` + 2ステップ確定 UI
- [ ] **Dashboard RELOAD ボタン**: 既存 `GET /reload` エンドポイントをダッシュボードから呼び出す
- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
  - **再検討**: `SecureSandboxExecutor` 実装時に vault の per-call 解決（v0.3 相当）を導入。現在は起動時一括注入（Phase 49-2）だが、v0.5 では Rust が実行主体になるため `env: {"KEY": "$vault:key"}` 形式で最小限注入が自然に実現できる。
- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み

### v0.4 残課題（`docs/specs/v0.4/` 精査 — 2026-06-12）

- [ ] **Phase 28b-2: Gateway 起動遅延短縮**: `Gateway::run` 初期化シーケンスの約 11 秒起動遅延を lazy init で改善。対象: `crates/rustyclaw-gateway/src/lib.rs`
- [ ] **Phase 26-2: McpClientHandler Idle Eviction**: アイドル 30 分超の MCP 子プロセスを SIGTERM → 次回呼び出しで再スポーン。着手条件: 複数 MCP サーバー同居時。`10_mcp.md §2.2` 参照
- [ ] **本番自動バックアップ**: `workspace/`（`memory.db`・`sessions/*.jsonl`・`patrol/findings.md`）を定時 rsync で NAS（QNAP 等）へ退避。`08_deployment.md §将来拡張` 参照
