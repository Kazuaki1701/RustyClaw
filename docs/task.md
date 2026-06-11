# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-11  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md](archive/tasks/2026-06-11-completed-phases-45-1-28b3-47-1-48-1.md) (Phase 45-1, 28b-3, 47-1, 48-1)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

- [ ] **Phase 49-2: Vault キャッシュ機構**
  - karakeep (`KARAKEEP_API_KEY`)、obsidian (`OBSIDIAN_TOKEN`)、vitals-coach (`HOMEASSISTANT_TOKEN`) が vault から環境変数を取得できない
  - Rust サービス起動時に vault の復号値を systemd `EnvironmentFile` 等で bash スクリプトに渡す仕組みが必要
  - 対象: `rustyclaw-context-mode` 側の起動フック実装

---

## 将来課題（低優先度）

- [ ] **Dashboard SETTING タブ**: `GET/POST /api/config` + 2ステップ確定 UI
- [ ] **Dashboard RELOAD ボタン**: 既存 `GET /reload` エンドポイントをダッシュボードから呼び出す
- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み
