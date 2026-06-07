# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-08 (ISSUE-26 完了)  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。

---

## バグ修正

> 実運用ログから発見されたバグ・要改善項目。優先度とは独立して管理し、次スプリントの実施案件を選択する。発見次第追記する。

---

- `[x]` **ISSUE-26: Heartbeat エージェントが 5ステップのループ上限に達して毎回クラッシュするバグの修正** (#20)
  - **症状**: 30分ごとの Heartbeat patrol 実行時、`list_family` や `get-gmail` などのツールを繰り返し呼び出し続け、最大ループ数（5回）を超過して `Heartbeat agent loop exceeded maximum step limit of 5` で終了する。
  - **原因**: `trim_heartbeat_messages` による対話履歴のトリミング（最新の1世代以外を捨てる）により、エージェントが「過去に確認済みであること」を忘れてしまい、無限にツールの再実行を繰り返している可能性が高い。
  - **対策案**: `trim_heartbeat_messages` の廃止または履歴保持件数の緩和、および新規データがない場合に速やかに `HEARTBEAT_OK` で終了させるプロンプト制御の強化。

---

## 優先課題

> 実装状況により今後の計画に与える影響が大きい案件。

- なし (すべての優先課題が完了しました)

---

## 一般課題

### リファクタリング

> Phase 40 完了済み: 40-2（rig-core Tool 直接実装）・40-3（RAG 長期記憶）・40-5（Unified RAG）・40-6（rmcp 移行・ReAct ループ一本化）・40-7（Static Docs RAG）。

- `[ ]` **40-1: `rustyclaw-providers` の rig-core Provider への置き換え**
  - Groq / Cloudflare などの自前 HTTP ペイロード構築を rig の共通 API にリファクタリング。

- `[ ]` **40-4: 宣言的 `AgentBuilder` の導入**
  - heartbeat / summary などのエージェント定義を AgentBuilder で再整理（現状は execute_heartbeat が独自ループ）。

---

### GeminiClaw とのギャップ解消

#### Phase 37: GeminiClaw 高度先進機能の移植と統合
> 設定と実行環境のギャップ回収により、ラズパイ運用環境での安全性、表現力、利便性を極大化する。

- `[ ]` **1. 自律性制御 (Autonomy Level) システムの導入**
  - `Config` に `autonomy_level` を追加し、`autonomous` / `supervised` / `read_only` の切り替えをサポート。
  - `supervised` 時に書き込み操作を一時中断し承認を待つゲートウェイインターセプション処理の実装。

- `[ ]` **2. Tailscale 連携 Web プレビューサーバーの実装**
  - `axum` または `warp` による非同期 HTTP サーバースレッドの実装。
  - `workspace/previews/` 配下の静的ファイルサービングと、安全な Tailscale アドレス経由でのプレビューURL提示。

- `[ ]` **3. Bubblewrap による実行スクリプトのサンドボックス化（ラズパイ環境保護）**
  - `bwrap` コマンドラインラッピングによる `WorkspaceExecuteScriptTool` の保護。
  - `/workspace` ディレクトリのみを書き込み可能バインドし、ホストOSやSSDの不用意な破壊を防ぐ。

- `[ ]` **4. プロンプト予算 (Prompt Budget) 設定によるコンテキスト配分管理**
  - `config.json` に `prompt_budget` の上限値を定義。
  - 会話圧縮（コンパクション）のトリガーしきい値と動的連動させるリファクタリング。

#### Phase 39: マルチチャンネル対応（LINE 導入 + Notifications チャンネル）
> GeminiClaw は Discord / Slack / Telegram のマルチチャンネルに対応しており、notifications チャンネル（home と独立したバックグラウンドジョブ通知先）を持つ。RustyClaw は Discord のみで、LINE 導入予定に伴いこのギャップを回収する。  
> 調査資料: [`docs/review/2026-06-03-geminiclaw-nonok-delivery-analysis.md`](review/2026-06-03-geminiclaw-nonok-delivery-analysis.md) / [`docs/review/2026-06-03-geminiclaw-notifications-channel-analysis.md`](review/2026-06-03-geminiclaw-notifications-channel-analysis.md)

- `[ ]` **1. LINE チャンネルコネクタの実装**
  - `rustyclaw-channels` に `LineConnector` を追加（`Channel` トレイト実装）。
  - LINE Messaging API（REST）による送信と、Webhook（HTTPS POST）受信エンドポイントの実装。
  - `channel_secret` を使った HMAC-SHA256 署名検証を必須実装。
  - session_id 命名規則: `line-U{userId}-{YYYYMMDD}` 形式。
  - gateway への `LineConnector` 初期化・起動組み込みと `MessageBus` 配信分岐の追加。
  - 対象: `crates/rustyclaw-channels/src/lib.rs`、`crates/rustyclaw-gateway/src/lib.rs`

- `[ ]` **2. Notifications チャンネル設定の導入**
  - GeminiClaw の `notifications: { channel, channelId }` 相当。home と独立したバックグラウンドジョブ通知先チャンネル（未設定時は home にフォールバック）。
  - `DiscordConfig`（および将来の LINE/Telegram 設定）に `notifications_channel_id` を追加、または `Config` 直下にプラットフォーム横断的 `notifications` 設定を追加。
  - `heartbeat.rs::process_heartbeat_response` の配信先を `notifications_channel_id` 優先に切り替え。
  - 背景: LINE を home にした場合、HEARTBEAT_OK の稼働ログが LINE に届き続けるノイズを防ぐための分離。
  - 対象: `crates/rustyclaw-config/src/lib.rs`、`crates/rustyclaw-gateway/src/heartbeat.rs`

---

### 機能追加

#### Phase 25 残: 並行制御の最適化とフリーズ防止（Lane Queue 改善）
> ※ 完了済み項目（1/3/4/6）はアーカイブ済み。残 2 件は独自安全改善。  
> ※ **item 5（Lossless Resume）は将来の優先課題昇格を検討する。**

- `[ ]` **2. 実行キュー取得の安全待機タイムアウトの実装**
  - `crates/rustyclaw-gateway/src/lib.rs` におけるセマフォ取得処理への `tokio::time::timeout` 導入。

- `[ ]` **5. 実行ステップのチェックポイント化と Lossless Resume の導入**（GeminiClaw ギャップB・昇格候補）
  - 途中でエラー（送信失敗など）が発生した際に、LLM API の再呼出を行わず失敗したフェーズから再試行できる中間状態の永続化と復旧機構。

#### Phase 26: 外部 MCP クライアントの堅牢化とトランスポート拡張
> **注記**: `rustyclaw-mcp` クレートは Phase 40-6 で削除済み。Phase 26 の実装対象は `crates/rustyclaw-gateway/src/lib.rs` 内の `McpClientHandler` / `ToolServerHandle` ブロック（`Gateway::run()` の MCP init 処理）に移行。

- `[ ]` **1. 子プロセスクラッシュ時の自動再接続・復旧 (Auto-Reconnect) の実装**
  - `crates/rustyclaw-gateway/src/lib.rs` `Gateway::run()` の MCP spawn ブロックに、`mcp_service_tasks` の終了監視と `McpClientHandler::connect()` 再試行ループを追加。

- `[ ]` **2. 外部 MCP サーバーの「メモリ回収（Idle Eviction）」機構の実装**
  - 一定時間 (例: 30分) 呼び出されていない MCP 子プロセスを一度安全にクローズしてメモリを回収、次回ツール呼び出し時にオンデマンドで自動再起動。

- `[ ]` **3. SSE (Server-Sent Events) トランスポートおよび Resources / Prompts 連携の追加**
  - rmcp の HTTP/SSE トランスポートを使った外部リモート MCP サーバー接続サポートの実装。
  - Tools 機能だけでなく、Resources や Prompts にもクエリ可能にするための I/O 拡張。

#### Phase 27: ハウスクリーニング、ディスク容量保護と Cron 拡張

- `[ ]` **1. ディスク空き容量監視と SSD 保護の導入**
  - 定期実行時に USB SSD の空き容量をチェックし、残り容  
> 対象: `crates/rustyclaw-agent/`, `crates/rustyclaw-gateway/`, `crates/rustyclaw-tools/`, `crates/rustyclaw-cli/`

- `[ ]` **1. イベント駆動 Hook システム (Hook Manager) の実装**
  - LLM 呼出前後、ツール実行前後、エラー発生時などに動作をアタッチできる `Hook`（オブザーバー、インターセプター、承認 Hook）機構の実装。
  - `Confirmation Gate` (Phase 23) などを Hook 側に移行・リファクタリング。

- `[ ]` **2. リアルタイム・ステアリング (Steering) 割り込み機構の実装**
  - `broadcast` または `mpsc` を監視し、長いツール実行の最中にユーザーが割り込み（Interruption）およびガイダンス（行動方向修正）を注入する仕組みの実装。

- `[ ]` **3. 長時間タスクの非同期 `spawn` ＆ サブエージェント機構の実装**
  - チャット応答をフリーズさせない長時間非同期ジョブ実行と、完了時の `MessageBus` アペンド通知。

- `[ ]` **4. ClawHub 互換の動的 Skill ダウンローダー・インストーラーの実装**
  - `rustyclaw skills install <skill-name>` サブコマンドの実装および `workspace/skills/` へのリモート展開ロジック。

---

### Security 対応

#### Phase 23: 安全ガードレールと構造化監査ログの構築
> ※ GeminiClaw に直接対応機能なし。RustyClaw 独自の安全機構として重要。

- `[ ]` **1. 自律レベル制御 (Autonomy Level) と承認ゲート (Confirmation Gate) の実装**
  - `AutonomyLevel` (`Autonomous` / `Supervised` / `ReadOnly`) の導入。
  - `supervised`（監視モード）時、書き込みや破壊的アクションに対して `ask-user` ファイル監視で実行を非同期ブロッキングする承認ゲートの実装。

- `[ ]` **2. 構造化ツール監査ログ (Audit Logger) の実装**
  - ツール実行結果をパラメータ切り詰めの上 `{workspace}/memory/audit.jsonl` に保存する仕組みの実装。

---

### RAG 機能の高度化



#### Phase 43-F: 自己進化型 RAG（RAG Flywheel）

- `[ ]` **1. 実運用インシデント＆ソリューション・ナレッジベース（RAGフライホイール）の導入**
  - 本番稼働時の外部コマンド実行エラーや外部 API（気象、Discord等）の呼び出しエラーが発生した際、その解決プロセス（自動リトライやフォールバック手順）を自動的に RAG へ登録。次回同様のエラーを検知した際、事前に解決策をロードしてシステムダウンを防止する。エラー発生・解決時のみ embedding を生成するため、定常負荷は増えない。

#### Phase 43-G: ユーザー知的支援（セカンドブレイン RAG）

- `[ ]` **1. KaraKeep ブックマークのインデックス化と API 同期**
  - KaraKeep サーバーから定時バッチでブックマーク（タイトル・タグ・URL）を取得してインジェスト。対話中に「保存した記事」を自然言語で検索・提示可能にする。テキスト量が少ないため RPi4 への負荷も極めて低い。
- `[ ]` **2. Obsidian 特定フォルダの増分インジェストとパーソナルナレッジ参照**
  - Obsidian の指定ディレクトリ（または `#rag` タグ付きノート）をスキャン。RPi4 の CPU 負荷集中を避けるため、深夜のアイドル時間帯に差分のみを増分 embedding してセカンドブレイン化する。

---

## 保留案件

### 条件付き待機（前提条件の解決後に着手）

- `[ ]` **ISSUE-22: `gmn_sem` capacity の config 化＋書き込み直列化の責務分離**（capacity 引き上げ検討時。**旧 Phase 25-1 を統合**。メモリ `project_user_sem_concurrency` 参照）
- `[ ]` **ISSUE-25: `●ACTIVE` → daemon STOP 制御**（無認証 LAN への破壊操作の露出・START 非対称性のセキュリティ前提を解決後）
- `[ ]` **ISSUE-09: rp1 の LM Studio 依存（単一障害点）のフェイルオーバ設計**
- 観察のみ: ISSUE-10（ローカル Gemma 品質）/ 13（一時 WS の context file WARN）/ 14（gws calendar WARN・現状解消）

### アイデアバックログ

- `[ ]` **LLM Provider 追加候補**
  - Cerebras `gpt-oss-120b`（14,400 RPD・60k TPM）
  - Google AI Studio（Gemma 3 27B）
  - OpenRouter 新モデル: `qwen3-coder:free`（1M ctx）・`qwen3-next-80b:free` ...等

- `[ ]` **本番環境の自動バックアップ体制**
  - `production/workspace/`（`memory.db`・`sessions/*.jsonl`・`patrol/findings.md` 等）を QNAP 等の NAS へ定時 rsync

- `[ ]` **MEMORY.md および知識構造のスリム化自動トリガー**
  - 稼働蓄積で肥大化するナレッジファイルの自動クリーンアップ検討

- `[ ]` **stn/rqmd によるローカル知識ベース RAG 構築**（Phase 13 積み残し）

- `[ ]` **Google Drive / Sheets / Docs ツール**
  - gws CLI 経由で実装可能。ユースケースが明確になった時点で追加




