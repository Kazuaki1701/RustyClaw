# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-10 (Phase 44-2 リクエストサイズ削減・完了)  
> **アーカイブ**: 完了済みの過去タスク履歴は [archive/tasks/README.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/tasks/README.md) を参照してください。

---

## バグ修正

> 実運用ログから発見されたバグ・要改善項目。優先度とは独立して管理し、次スプリントの実施案件を選択する。発見次第追記する。

---

- なし (すべてのバグ修正が完了しました)

---

## 優先課題

> 実装状況により今後の計画に与える影響が大きい案件。

#### Phase 44: LLM I/O 最適化と Dashboard 遅延削減
> **目的**: Dashboard の LLM リクエスト/レスポンス遅延を削減し、ユーザー体感速度を向上させる。  
> **優先順位の根拠**: 改善効果が高く副作用・実装コストが低い順に番号を振っている。

- `[x]` **Phase 44-1. Dashboard のタイムアウト調整** ⚡ 即効・副作用ゼロ
  - タイムアウトを 300 s → 120 s に短縮し、遅延があれば再試行＋キャッシュ返却を実装。
- `[x]` **Phase 44-2. リクエストサイズ削減** 🏆 全リクエストに効く・最高費用対効果
  - 不要な長文（SOUL.md、AGENTS.md、MEMORY.md 全文）を要点のみ（数百バイト）に圧縮してダンプ。
  - `last_request.json` のサイズ目標: **< 5 KB**。
  - ※ 44-3 より先に実施することで、固定化すべきプロンプトの最小サイズを把握できる。
- `[ ]` **Phase 44-3. システムプロンプトの固定化** 💡 プロバイダの Prefix Caching 活用
  - エージェント起動時に基本プロンプトのみ設定し、動的情報は差分メッセージとして追加する。
  - `rustyclaw-agent/src/lib.rs` の `dump_request`/`dump_response` を NOP にし、コメントで搬送先はプロバイダ層と明示。
  - ※ 44-2 でサイズ感を確認した後に設計すること。動的情報の差分注入ロジックを誤ると挙動が変わるため慎重に。
- `[ ]` **Phase 44-4. ダンプロジックのプロバイダ層へ集約** 🔧 44-5 の前提作業
  - `crates/rustyclaw-providers/src/lib.rs` に `dump_llm_io` を実装し、リクエスト/レスポンス JSON を統一的に保存。
  - 保存先を `workspace/memory/debug/llm/<date>/` に整理し、過去ログ検索を容易にする。
- `[ ]` **Phase 44-5. エラーハンドリングとディレクトリ作成** 🛡️ 44-4 完了後に実施
  - `dump_llm_io` 内でディレクトリ作成失敗時は警告のみ出し、処理は続行。
  - 古いデバッグディレクトリは 5 日以上前のものを自動削除。
- `[ ]` **Phase 44-6. ストリーミングとキャッシュ** 🚀 体感改善最大・大規模改修
  - `complete_stream` を使用してリアルタイムにレスポンスを流す。
  - 定型レポートはローカルキャッシュ (`memory/debug/llm/cache`) に保存し、同一リクエスト時は再利用。
  - ※ 44-1〜44-5 で十分な遅延削減を達成してから着手する。キャッシュ無効化設計を慎重に行うこと。
- `[ ]` **Phase 44-7. テストとベンチマーク** ✅ 最終検証（各ステップ完了後に実施）
  - 改修前後で `dashboard_response_analysis.md` に記録されたタイムスタンプを比較し、目標 **50 % 以上の遅延削減** を検証。
  - CI にベンチマークスクリプトを追加し、毎回のプルリクでパフォーマンス回帰を検知。

---

## 一般課題

### GeminiClaw とのギャップ解消

#### Phase 37: GeminiClaw 高度先進機能の移植と統合
> 設定と実行環境のギャップ回収により、ラズパイ運用環境での安全性、表現力、利便性を極大化する。  
> 完了済み: 37-1（Autonomy Level）・37-2（Web Preview Server）・37-3（Bubblewrap サンドボックス）

- `[ ]` **4. プロンプト予算 (Prompt Budget) 設定によるコンテキスト配分管理**
  - 詳細設計: [`docs/plans/2026-06-08-phase37-4-prompt-budget.md`](plans/2026-06-08-phase37-4-prompt-budget.md)

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
  - 定期実行時に USB SSD の空き容量をチェックし、残り容量が 5% 以下になった際に Discord 等へ警告アラートを投げる保護タスクの実装。

- `[ ]` **2. Cron セッションおよびログの自動プルーニングの実装**
  - 古い `cron:` 実行ログやセッションファイルを自動消去するクリーンアップ機構の実装。
  - 対象: `crates/rustyclaw-gateway/src/cron.rs`

- `[ ]` **3. 1回限り (at / deleteAfterRun) job の自動削除サポート**
  - 実行完了後に `cron.json` から自身のジョブ定義を自動削除し、アトミックに更新保存。

#### Phase 28b 残: ダッシュボード精度・起動最適化のフォローアップ
> 出典: 2026-05-31 の Phase 28 実機検証（`gateway --no-agent` 起動ログ点検）で判明した改善候補。

- `[ ]` **2. Gateway 起動時の設定ロード遅延（約11秒）の短縮検討**
  - `Initializing daemon` から `loaded configuration` まで約11秒を要する（`--no-agent` でも発生）。遅延要素の遅延初期化（lazy）等で起動高速化を検討。
  - 対象: `crates/rustyclaw-gateway/src/lib.rs`（`Gateway::run` 初期化シーケンス）

#### Phase 30: Upstream 先進機能：Hook・Steering・Spawn タスクの統合
> ※ PicoClaw (Go Upstream) 参照。GeminiClaw ギャップではなく、RustyClaw の独自進化機能として位置付け。  
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

- `[ ]` **embedding 検索における時間情報の有効化**
  - ハイブリッド検索 / メタデータフィルタ：
    「今日」「今週」などの時間的キーワードに含まれるクエリに対して、Embedding 検索対象範囲を更新日時などのメタデータでフィルタリング
  - 再順位付け:
     検索結果に対して時間的近接度をスコアで加算し、新しい情報を上位に引き上げる仕組み。

