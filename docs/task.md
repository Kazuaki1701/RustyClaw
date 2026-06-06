# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-06-05 (Phase 40-5 バグ修正完了・Phase 39 格下げ🟢・Phase 40 格上げ🔴)  
> **アーカイブ**: 完了済みフェーズ (Phase 2〜19) は `docs/archive/2026-05-30-completed-phases-2-to-19.md`、(Phase 20, 21, 28, 旧31) は `docs/archive/2026-05-31-completed-phases-20-21-28-31.md`、(Phase 29, 32, 34, 35, 35b) は `docs/archive/2026-06-02-completed-phases-29-32-34-35-35b.md`、(Phase 24, 36, 38) は `docs/archive/2026-06-04-completed-phases-24-36-38.md` に保存

> **優先方針（2026-05-31 更新）**: **GeminiClaw との機能ギャップ回収を最優先（🔴）とする。**  
> それ以外の独自機能・改善案件は一旦 🟢 に降格。GeminiClaw ギャップが解消され次第、改めて優先度を見直す。

## 🔴 GeminiClaw 機能ギャップ（最優先）

### ~~Phase 40-5 バグ修正: CF Embedding `'input' field is required`~~ ✅ 完了
> 2026-06-05 修正・デプロイ済み。commit `55f773f`。  
> 根本原因: LM Studio（OpenAI 互換）に `{"text":}` を送っていたが `{"input":}` が正しい。URL末尾 `/embeddings` 検出で分岐、レスポンスパースも OpenAI 形式対応。  
> 確認: 06:04 ログファイルで修正前エラー（`'input' field is required`）を 04:00〜06:01 に連続確認。06:49 デプロイ後は同エラー皆無。07:23 Heartbeat で `retrieve_rag_context` 成功ログを最終確認予定。

- `[x]` **1. `CloudflareEmbeddingClient.embed()` のリクエストボディを調査・修正**
  - `build_embed_body()` 追加：`api_endpoint.ends_with("/embeddings")` で `{"input":}` / `{"text":}` を分岐。
  - `OpenAiEmbedResponse` 構造体を追加しレスポンスパースを両形式対応。
  - 新規テスト 3本追加済み。対象: `crates/rustyclaw-providers/src/lib.rs`

- `[x]` **2. 修正後の動作確認 + deploy**
  - `deploy.sh` 実行済み（06:49 サービス再起動確認）。
  - 06:49 以降 embed エラー皆無（journalctl 確認）。07:23 wakeup で最終確認予定。

---

### Phase 37: GeminiClaw 高度先進機能の移植と統合 🔴
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

---

## 🟢 その他の改善案件（独自機能・将来対応）

### Phase 39: マルチチャンネル対応（LINE 導入 + Notifications チャンネル） 🟢
> GeminiClaw は Discord / Slack / Telegram のマルチチャンネルに対応しており、notifications チャンネル（home と独立したバックグラウンドジョブ通知先）を持つ。RustyClaw は Discord のみで、LINE 導入予定に伴いこのギャップを回収する。  
> 調査資料: [`docs/2026-06-03-geminiclaw-nonok-delivery-analysis.md`](2026-06-03-geminiclaw-nonok-delivery-analysis.md) / [`docs/2026-06-03-geminiclaw-notifications-channel-analysis.md`](2026-06-03-geminiclaw-notifications-channel-analysis.md)

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

### Phase 40: rig-core のフル活用による設計洗練とRAG拡張 🔴
> LLM 接続やツール管理を rig-core で統合し、ベクトル検索による長期記憶拡張を実現する。

- [ ] **1. rustyclaw-providers の rig-core Provider への置き換え**
  - Groq / Cloudflare などの自前 HTTP ペイロード構築を rig の共通 API にリファクタリング。
- [ ] **2. ツール定義と呼び出し処理の rig::tool へのリファクタリング**
  - `#[tool]` マクロによる JSON スキーマ自動生成と、呼び出し時引数の型安全パースを導入。
- `[x]` **3. ベクトル検索（RAG）による長期記憶の拡張**
  - MEMORY.md バレット行を CF AI Gateway `@cf/baai/bge-m3` (1024次元、多言語) でベクトル化し SQLite 保存。
  - `flush_memory` 後に自動 Ingestion、`execute_with_tools` / `execute_stream` 双方でコサイン類似度 top-K をシステムプロンプトに注入。
  - rig-core 不依存のスタンドアロン実装。Fail-open 設計。
  - 実装計画: `docs/plans/2026-06-04-rag-memory-implementation-plan.md`
  - 対象: `rustyclaw-config`, `rustyclaw-storage`, `rustyclaw-providers`, `rustyclaw-agent`, `production/config/config.release.json`
- [ ] **4. 宣言的 AgentBuilder の導入**
  - heartbeat / summary / memory などのエージェント定義を AgentBuilder で再整理。
- `[x]` **5. Unified RAG with rig-core InMemoryVectorStore**
  - `InMemoryVectorStore` の採用、`MEMORY.md` チャンクとセッション要約のインメモリ統合 RAG 化。
  - 実装済み・稼働中（commit `55f773f` で CF embedding バグ修正済み。06:49 デプロイ後エラー皆無）。
  - 実装計画: `docs/superpowers/plans/2026-06-05-rig-core-unified-rag.md`
- [ ] **6. rig-core 全面リファクタリング (Unified RAG & rig-core Refactoring)**
  - `#[tool]` アトリビュートマクロ、`rmcp` クライアントへの移行、`rig::agent::Agent` 移行による ReAct/RAG ループの一本化。
  - 前提: Phase 40-5 の CF embedding バグ修正完了後。
  - 実装計画: `docs/superpowers/plans/2026-06-05-rig-core-refactoring.md`
- [ ] **7. Static Docs RAG（AGENTS.md / skills/*.md の動的注入）**
  - 静的ドキュメントをチャンク化・差分インジェストし、ユーザー入力との類似度で動的にシステムプロンプトへ注入。固定プロンプト最小化・履歴上限緩和も含む。
  - 前提: Phase 40-5 の CF embedding バグ修正完了後。
  - 実装計画: `docs/superpowers/plans/2026-06-05-static-docs-rag.md`

---

## Phase 25: 並行制御の最適化とフリーズ防止（Lane Queue 改善・回収）残り 🟢
> ※ 完了済みの Phase 1〜2 はアーカイブ済み。残 2 件は独自安全改善。  
> ※ **item 5（Lossless Resume）は GeminiClaw ギャップB に相当するため、GeminiClaw ギャップ回収完了後に 🔴 昇格を検討する。**

- `[x]` **1. Lane Queue（Inngest 代替）の機能ギャップ分析とロードマップ策定**

- `[ ]` **2. 実行キュー取得の安全待機タイムアウトの実装**
  - `crates/rustyclaw-gateway/src/lib.rs` におけるセマフォ取得処理への `tokio::time::timeout` 導入。

- `[x]` **3. Chat Progress Reporter (Typing... 送信) の実装 (Phase 1)**

- `[x]` **4. 並行数 4 への拡張に向けたファイルロック機構の導入 (Phase 2)**

- `[ ]` **5. 実行ステップのチェックポイント化と Lossless Resume の導入 (Phase 3)**（GeminiClaw ギャップB・昇格候補）
  - 途中でエラー（送信失敗など）が発生した際に、LLM API の再呼出を行わず失敗したフェーズから再試行できる中間状態の永続化と復旧機構。

- `[x]` **6. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 28b: ダッシュボード精度・起動最適化のフォローアップ 🟢
> 出典: 2026-05-31 の Phase 28 実機検証（`gateway --no-agent` 起動ログ点検）で判明した改善候補。

- `[ ]` **2. Gateway 起動時の設定ロード遅延（約11秒）の短縮検討** 🟢 優先度低
  - `Initializing daemon` から `loaded configuration` まで約11秒を要する（`--no-agent` でも発生）。遅延要素の遅延初期化（lazy）等で起動高速化を検討。
  - 対象: `crates/rustyclaw-gateway/src/lib.rs`（`Gateway::run` 初期化シーケンス）

- `[x]` **3. LANE QUEUE 表示名を `{cron title} ({HH:MM})` 形式に変更**
  - 現状: キュー内のジョブ説明がハードコードまたはジョブ ID 等の内部名で表示されている。
  - 変更後: `cron.json` の `name` フィールドと `trigger.expression`（HH:MM）を組み合わせた形式で表示。
    - 例: `Topic Patrol Explore (02:00)` / `Daily Briefing (05:05)` / `Vital Check Morning (06:00)`
  - `queue_update_or_insert()` 呼び出し時に渡す `desc` 引数を `format!("{} ({})", job.name, job.trigger.expression)` 形式で生成するよう修正。
  - Heartbeat（`"Heartbeat Patrol / Activity Scan"`）はそのまま維持。
  - 対象: `crates/rustyclaw-gateway/src/lib.rs`（cron ジョブのキュー登録箇所）、`crates/rustyclaw-gateway/src/cron.rs`

- `[ ]` **4. Heartbeat コンテキストオーバーフロー対策** 🟡
  - Deep Scan 時（04:00 / 06:00 付近）にツール呼び出し後のコンテキストが肥大化し全モデル失敗 → Discord 通知欠落（2026-06-05 ログ確認: 9,812 tokens > Groq 6,000 上限）。
  - Heartbeat 専用のヒストリキャップ強化またはツール結果切り詰め処理を検討。
  - 対象: `crates/rustyclaw-gateway/src/heartbeat.rs`、`crates/rustyclaw-agent/src/lib.rs`（`get_history_message_limit`）

---

## Phase 26: 外部 MCP クライアントの堅牢化とトランスポート拡張 🟢

- `[ ]` **1. 子プロセスクラッシュ時の自動再接続・復旧 (Auto-Reconnect) の実装**
  - `crates/rustyclaw-mcp/src/lib.rs` の接続ライフサイクルに異常終了監視と `spawn` 再試行ループを追加。

- `[ ]` **2. 外部 MCP サーバーの「メモリ回収（Idle Eviction）」機構の実装**
  - 一定時間 (例: 30分) 呼び出されていない MCP 子プロセスを一度安全にクローズしてメモリを回収、次回ツール呼び出し時にオンデマンドで自動再起動。

- `[ ]` **3. SSE (Server-Sent Events) トランスポートおよび Resources / Prompts 連携の追加**
  - HTTP/SSE 経由の外部リモート MCP サーバー接続サポートの実装。
  - Tools（工具）機能だけでなく、Resources や Prompts にもクエリ可能にするための I/O 拡張。

- `[ ]` **4. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 27: ハウスクリーニング、ディスク容量保護と Cron 拡張 🟢

- `[ ]` **1. ディスク空き容量監視と SSD 保護の導入**
  - 定期実行時に USB SSD の空き容量をチェックし、残り容量が 5% 以下になった際に Discord 等へ警告アラートを投げる保護タスクの実装。

- `[ ]` **2. Cron セッションおよびログの自動プルーニングの実装**
  - 古い `cron:` 実行ログやセッションファイルを自動消去するクリーンアップ機構の実装。
  - 対象: `crates/rustyclaw-gateway/src/cron.rs`

- `[ ]` **3. 1回限り (at / deleteAfterRun) jobの自動削除サポート**
  - 実行完了後に `cron.json` から自身のジョブ定義を自動削除し、アトミックに更新保存。

- `[ ]` **4. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 23: 安全ガードレールと構造化監査ログの構築 🟢
> ※ GeminiClaw に直接対応機能なし。RustyClaw 独自の安全機構として重要だが、GeminiClaw ギャップ回収優先のため降格。

- `[ ]` **1. 自律レベル制御 (Autonomy Level) と承認ゲート (Confirmation Gate) の実装**
  - `AutonomyLevel` (`Autonomous` / `Supervised` / `ReadOnly`) の導入。
  - `supervised`（監視モード）時、書き込みや破壊的アクションに対して `ask-user` ファイル監視で実行を非同期ブロッキングする承認ゲートの実装。

- `[ ]` **2. 構造化ツール監査ログ (Audit Logger) の実装**
  - ツール実行結果をパラメータ切り詰めの上 `{workspace}/memory/audit.jsonl` に保存する仕組みの実装。

- `[ ]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 30: Upstream 先進機能：Hook・Steering・Spawn タスクの統合 🟢
> ※ PicoClaw (Go Upstream) 参照。GeminiClaw ギャップではなく、RustyClaw の独自進化機能として位置付け。  
> 対象: `crates/rustyclaw-agent/`, `crates/rustyclaw-gateway/`, `crates/rustyclaw-tools/`, `crates/rustyclaw-cli/`

- `[ ]` **1. イベント駆動 Hook システム (Hook Manager) の実装**
  - LLM 呼出前後、ツール実行前後、エラー発生時などに動作をアタッチできる `Hook`（オブザーバー、インターセプター、承認 Hook）機構の実装。
  - `Confirmation Gate` (Phase 23) などを Hook 側に移行・美しくリファクタリング。

- `[ ]` **2. リアルタイム・ステアリング (Steering) 割り込み機構の実装**
  - `broadcast` または `mpsc` を監視し、長いツール実行の最中にユーザーが割り込み（Interruption）およびガイダンス（行動方向修正）を注入する仕組みの実装。

- `[ ]` **3. 長時間タスクの非同期 `spawn` ＆ サブエージェント機構の実装**
  - チャット応答をフリーズさせない長時間非同期ジョブ実行と、完了時の `MessageBus` アペンド通知。

- `[ ]` **4. ClawHub 互換の動的 Skill ダウンローダー・インストーラーの実装**
  - `rustyclaw skills install <skill-name>` サブコマンドの実装および `workspace/skills/` へのリモート展開ロジック。

- `[ ]` **5. `docs/specs/PicoClaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 31 — 保留（前提条件の解決後に着手）
- `[ ]` **ISSUE-22: `gmn_sem` capacity の config 化＋書き込み直列化の責務分離**（capacity 引き上げ検討時。**旧 Phase 25-1 を統合**。メモリ `project_user_sem_concurrency` 参照）
- `[ ]` **ISSUE-25: `●ACTIVE` → daemon STOP 制御**（無認証 LAN への破壊操作の露出・START 非対称性のセキュリティ前提を解決後）
- `[ ]` **ISSUE-09: rp1 の LM Studio 依存（単一障害点）のフェイルオーバ設計**
- 観察のみ: ISSUE-10（ローカル Gemma 品質）/ 13（一時 WS の context file WARN）/ 14（gws calendar WARN・現状解消）

---

## 次期大型対応検討案件 🟢 優先度低

> 現時点では保留。前提条件の整理・設計検討が必要な案件。

---

## 将来の検討課題 🟢 優先度低

- `[ ]` **LLM Provider 追加候補**
  - Cerebras `gpt-oss-120b`（14,400 RPD・60k TPM）
  - Google AI Studio（Gemma 3 27B）
  - OpenRouter 新モデル: `qwen3-coder:free`（1M ctx）・`qwen3-next-80b:free` ...等

- `[ ]` **本番環境の自動バックアップ体制**
  - `production/workspace/`（`memory.db`• `sessions/*.jsonl`• `patrol/findings.md` 等）を QNAP 等の NAS へ定時 rsync

- `[ ]` **MEMORY.md および知識構造のスリム化自動トリガー**
  - 稼働蓄積で肥大化するナレッジファイルの自動クリーンアップ検討

- `[ ]` **stn/rqmd によるローカル知識ベース RAG 構築**（Phase 13 積み残し）

- `[ ]` **Google Drive / Sheets / Docs ツール**
  - gws CLI 経由で実装可能。ユースケースが明確になった時点で追加
