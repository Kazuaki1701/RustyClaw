# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-13（Phase 51-2 完了。Phase 52 を優先課題に昇格）  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。  
> **最新アーカイブ**: [2026-06-13-completed-phases-51-1-51-2.md](archive/tasks/2026-06-13-completed-phases-51-1-51-2.md) (Phase 51-1, 51-2, v0.4 本番バックアップ)  
> **将来課題の管理**: 未着手の将来課題は [`docs/specs/v0.3/`](specs/v0.3/) 各仕様ファイルの「将来拡張」節で管理しています。

---

## 優先課題

- [ ] **Phase 52-1: Memory Flush のコンテキスト最適化**:  
  `memory flush` 実行時における LLM リクエストおよびレスポンスのトークン数節約、およびコンテキスト窓（32k）の効率的な管理。
  - **内容**: XMLデリミタへの移行、システムプロンプトの圧縮、会話履歴のクレンジング、長期的なメモリセマンティック分割（RAG化）。
  - **詳細設計・改善提案**: [2026-06-13-phase52-1-memory-flush-context-optimization.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-13-phase52-1-memory-flush-context-optimization.md)

- [ ] **Phase 52-2: リクエストプロンプト（指示文・スキル定義）の動的最適化**:  
  通常のチャットリクエストにおける入力トークンの肥大化（約10k）を防ぐための、プロンプト情報の動的読み込みとフィルタリング。
  - **内容**: ユーザー文脈に応じたスキルの動的選択（Dynamic Skill Selection）、USER.md の興味関心（Interests）等の動的注入、Home Assistant等のスクリプトインターフェース集約、外部スクリプトの MCP ツール化（context-modeネイティブ化）、ブリーフィング結果の相関検索および自動格納。
  - **詳細設計・改善提案**: [2026-06-13-request-prompt-optimization-report.md](file:///home/kazuaki/Projects/RustyClaw/docs/review/2026-06-13-request-prompt-optimization-report.md)

---

## 将来課題（低優先度）

- [ ] **v0.5: 純 Rust 単一バイナリ**: `rustyclaw-context-mode` crate に EmbeddedKnowledgeBase + InProcessPatchMerger + SecureSandboxExecutor を実装
  - **再検討**: `SecureSandboxExecutor` 実装時に vault の per-call 解決（v0.3 相当）を導入。現在は起動時一括注入（Phase 49-2）だが、v0.5 では Rust が実行主体になるため `env: {"KEY": "$vault:key"}` 形式で最小限注入が自然に実現できる。

### v0.6 案件

- [ ] **Phase 46-1: LINE チャンネル実装**: `LineConnector`（`Channel` トレイト）追加、config スキーマ実装済み

### v0.4 残課題（`docs/specs/v0.4/` 精査 — 2026-06-12）

- [ ] **Dashboard 改善**: SETTING タブ（`GET/POST /api/config` + 2ステップ確定 UI）・RELOAD ボタン（既存 `GET /reload` をダッシュボードから呼び出す）
