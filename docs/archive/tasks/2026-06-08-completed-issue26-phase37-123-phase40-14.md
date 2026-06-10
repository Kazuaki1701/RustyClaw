> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の完了済みタスク)  
> **完了日**: 2026-06-08  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-08 (ISSUE-26 / Phase 37-1, 2, 3 / Phase 40-1, 40-4)

## バグ修正

### ISSUE-26: Heartbeat エージェントが 5ステップのループ上限に達して毎回クラッシュするバグの修正 (#20)
- **完了日**: 2026-06-08
- **概要**: `trim_heartbeat_messages` の廃止と `HEARTBEAT_OK` プロンプト制御強化により、Heartbeat エージェントのループ超過クラッシュを解消。エージェントが過去ツール結果を忘れる原因だった履歴トリミングを廃止し、新規データがない場合に即時終了するよう修正。
- **関連計画書**: `docs/plans/2026-06-08-issue-26-heartbeat-loop-fix.md`

## リファクタリング（Phase 40）

### Phase 40-1: `rustyclaw-providers` の rig-core Provider への置き換え (#22)
- **完了日**: 2026-06-08
- **概要**: Groq / Cloudflare などの自前 HTTP ペイロード構築を rig の共通 API にリファクタリング。
- **関連計画書**: `docs/plans/2026-06-08-phase40-1-rig-providers-migration.md`

### Phase 40-4: 宣言的 `AgentBuilder` の導入
- **完了日**: 2026-06-08
- **概要**: heartbeat / summary などのエージェント定義を AgentBuilder で再整理。`execute_heartbeat` 独自ループを廃止し rig-core の標準エージェントループへ統一。
- **関連計画書**: なし（単独実装）

## GeminiClaw とのギャップ解消（Phase 37）

### Phase 37-1: 自律性制御 (Autonomy Level) システムの導入
- **完了日**: 2026-06-08
- **概要**: `Config` に `autonomy_level` を追加し、`autonomous` / `supervised` / `read_only` の切り替えを実装。`supervised` 時に書き込み操作を一時中断し承認を待つゲートウェイインターセプション処理を実装。
- **関連計画書**: `docs/plans/2026-06-08-phase37-1-autonomy-control.md`

### Phase 37-2: Tailscale 連携 Web プレビューサーバーの実装
- **完了日**: 2026-06-08
- **概要**: axum による非同期 HTTP サーバースレッドを実装。`workspace/previews/` 配下の静的ファイルサービングと、安全な Tailscale アドレス経由でのプレビュー URL 提示を実現。
- **関連計画書**: `docs/plans/2026-06-08-phase37-2-web-preview-server.md`

### Phase 37-3: Bubblewrap による実行スクリプトのサンドボックス化（ラズパイ環境保護）
- **完了日**: 2026-06-08
- **概要**: `bwrap` コマンドラインラッピングによる `WorkspaceExecuteScriptTool` の保護。`/workspace` ディレクトリのみを書き込み可能バインドし、ホストOSやSSDの不用意な破壊を防止。
- **関連計画書**: `docs/plans/2026-06-08-phase37-3-bubblewrap-sandbox.md`
