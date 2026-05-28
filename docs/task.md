# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-05-28  

---

## Phase 2 & 4: Gateway Services, Heartbeat System, Long-Term Memory ✅ 完了

- `[x]` **1. rustyclaw-storage の強化**
  - `[x]` `tantivy` 追加・`SearchIndexManager` 実装
  - `[x]` セッション ID のファイルシステムセーフなファイル名マッピング

- `[x]` **2. rustyclaw-agent の強化**
  - `[x]` Session Continuation（日またぎ文脈復元）実装
  - `[x]` Memory Flush 非同期トリガー実装（`execute` / `execute_stream`）

- `[x]` **3. rustyclaw-gateway の強化**
  - `[x]` `WatchdogService`（systemd watchdog）
  - `[x]` `HealthServer`（軽量 TCP HTTP サーバー）
  - `[x]` `CronService`（Heartbeat 10 分 / Daily Summary 毎時チェック）
  - `[x]` `HeartbeatService`（digest 生成・Quiet Hours・HEARTBEAT_OK 判定・Proactive Post）
  - `[x]` Background レーンキュー容量 1 制限
  - `[x]` `lastUserContact` 追跡

---

## Phase 5: Rate Limit 対策・Memory Flush 改善・運用品質向上 ✅ 完了

- `[x]` **gmn プロバイダ — `--no-agent` 必須化**
  - `complete()` / `complete_stream()` 両方に `--no-agent` 追加
  - rate limit 主因（エージェントモードで最大 25 API リクエスト/call）を除去

- `[x]` **gmn パッチビルド**
  - `GMN_MAX_RETRIES` 環境変数サポート（内部リトライ回数の外部制御）
  - `--version` に `+rustyclaw` サフィックス・`--help` にパッチ説明追加
  - `~/.local/share/go/bin/gmn` にインストール済み

- `[x]` **セマフォ値削減**（Antigravity 2.0 対応）
  - `user_sem`: 4 → 2、`bg_sem`: 2 → 1

- `[x]` **`user_sem` を 1 に削減**（共有ファイル競合防止）
  - `MEMORY.md` 等への並列書き込みによるデータ消失リスクへの対策（A案採用）
  - 詳細: `docs/specs/05_gateway_spec.md` の `[^gmn_sem]` 脚注を参照

- `[x]` **`gmn_sem` 統合セマフォへの一本化**（全 gmn プロセスを直列化）
  - `user_sem` / `bg_sem` / `flush_sem` の3セマフォを `gmn_sem(1)` に統合
  - user 対話 / bg heartbeat / flush_memory が同時に1つしか gmn を起動できない
  - Phase 5 MCP 統合時の影響は `[^mcp_heartbeat]` 脚注を参照

- `[x]` **`flush_sem` 専用セマフォ追加**
  - `flush_memory()` がセマフォ管理外で走っていた問題を修正
  - `flush_sem`（容量 1）を `Pipeline` / `LaneRegistry` に追加

- `[x]` **Memory Flush — GeminiClaw 方式全書き直し**
  - 既存 MEMORY.md をプロンプトに含め LLM に全書き直し版を返させる
  - MEMORY.md を**上書き**（追記 → サイズ上限超過永続スキップ問題を解消）
  - フェイルセーフ: Rust 側で 70/20 トランケート
  - `execute_stream()` の `|| true` バグ修正

- `[x]` **Memory Flush トークン最適化**
  - 会話履歴: 末尾 20 件固定 → **前回 Flush 以降のデルタ分のみ**（最大 10 件）
  - `max_tokens`: 2048 → **1500**

- `[x]` **Memory Flush 実行頻度最適化（時間ゲート + delta）**
  - delta 閾値: 3 → **6**（≒ 3 ターン）
  - 時間ゲート: 前回 Flush から **15 分以上**経過していない場合スキップ
  - `flush_count_{session}` + `flush_ts_{session}` を SQLite で管理

- `[x]` **ログタイムスタンプのローカルタイム化（JST）**
  - `ChronoLocal::new("%Y-%m-%dT%H:%M:%S%.3f%z")` を stdout / file レイヤーに適用

- `[x]` **Dashboard 更新**
  - セッションログ動的検索（`sessions/` 内最終更新 `.jsonl`、`cron-*` 除く）
  - `/chat` セッション ID を `"http-dashboard"` 固定に変更
  - 新エンドポイント: `/logs/memory`・`/logs/heartbeat-digest`・`/logs/heartbeat-state`
  - レイアウト: Chat / MEMORY.md / heartbeat-digest+state / App ログ

---

## Phase 6: 次回セッション向け Todo ✅ 完了

- `[x]` **gmn デバッグログの稼働確認**
  - `RUST_LOG=debug` で起動し `gmn spawn: args` / `gmn exit: response` が出力されるか確認
  - rate limit 時の stderr 内容が `WARN` として拾えているか確認

- `[x]` **Memory Flush の動作確認**
  - 実チャット 6 ターン後に flush が起動するか確認（`memory flush: starting` ログ）
  - 15 分ゲートのスキップ動作確認（`memory flush: skipping (time gate...)` ログ）
  - `MEMORY.md` が全書き直しされ 5KB 以内に収まっているか確認

- `[x]` **Dashboard の動作確認**
  - `MEMORY.md`・`heartbeat-digest.md`・`heartbeat-state.json` の表示確認
  - 5 秒ポーリングで内容が更新されるか確認

- `[x]` **rate limit 時のリトライ戦略検討**
  - 現状: `GMN_MAX_RETRIES=0` で即エラー返却（RustyClaw 側でリトライなし）
  - 検討: `LaneRegistry` レベルでの指数バックオフリトライ実装
  - 検討: rate limit エラー時のユーザー通知メッセージ整備

- `[x]` **Session Continuation の動作確認**
  - 日またぎセッションで前日サマリーが注入されるか確認
  - `memory/summaries/` の Daily Summary 生成確認

- `[x]` **コードレビュー指摘の対応 (Minor)**
  - `rustyclaw-gateway` クレートの 10 件のコンパイラ警告（未使用インポートや非 snake_case 命名）を解消する
  - 命名警告（`activityReview` 等）については、`#[serde(rename = "...")]` や `#[allow(non_snake_case)]` を用いて警告をクリーンにする

- `[x]` **仕様書へのフィードバック（DoD の適用）**
  - 各種動作確認で仕様との差異が判明した場合、`docs/specs/` 配下の基本仕様書（`01_`〜`06_`）を最新コードに同期させる
  - rate limit のリトライ戦略を検討・実装した際、`docs/specs/02_agent_pipeline.md` 等の関連仕様書をアップデートする

---

## Phase 7: 稼働観察で判明した要修正・要点検項目（2026-05-28）

> 5/28 00:00〜06:55 のセッションログ（`sessions/`・`memory/logs/`）分析で判明した問題。

- `[x]` **【バグ】Heartbeat セッション履歴の無制限肥大化**
  - **症状**: `cron-heartbeat.jsonl` に 182 ターン・179KB（6 時間稼働）。10 分毎に全履歴を LLM に流し込んでいる
  - **原因**: `cron:heartbeat` という固定セッション ID を使い続けるため、会話履歴が日をまたいで追記され続ける
  - **影響**: コンテキスト肥大 → レイテンシ増大・rate limit リスク増大
  - **対策**: Heartbeat などの `cron:` で始まるセッションIDの場合、Pipeline側で履歴の読み込みを完全にスキップしてステートレス化
  - **完了日**: 2026-05-28

- `[x]` **【バグ】Heartbeat の LLM 応答が空（セッション履歴に記録されない）**
  - **症状**: `cron-heartbeat.jsonl` の全 assistant エントリが空文字 `""`
  - **原因**: `--no-agent` モードでモデルがツール呼び出し JSON のみを生成しテキスト部分を生成しない場合、gmn がテキスト部分のみを返すため空になる
  - **影響**: セッション履歴が空で蓄積し続ける（メモリの無駄）・デバッグ困難
  - **対策**: `GmnCliProvider` にて `type == "content"` のテキストがない場合、標準出力の全生行をフォールバックとして採用して空のままになるのを防止
  - **完了日**: 2026-05-28

- `[x]` **【バグ】heartbeat-digest.md が 0 バイト**
  - **症状**: `workspace/memory/heartbeat-digest.md` が空ファイル（0 byte）のまま
  - **影響**: Dashboard の heartbeat-digest パネルに何も表示されない
  - **対策**: `HeartbeatService::generate_digest` において増分条件によるスキップを廃止し、過去24時間以内にアクティブであったログを常に網羅的にスキャンしてダイジェストを維持
  - **完了日**: 2026-05-28

- `[x]` **【既知問題・要対処】MCP ツール呼び出し JSON がチャットに漏出**
  - **症状**: Discord チャンネルに `{"action": "geminiclaw_status", ...}` 等の JSON ブロックが送信される
  - **原因**: `--no-agent` でツール実行ループなし。LLM がツール呼び出し JSON を生成しそのままテキスト出力として返る
  - **対策**: `AGENTS.md` のレガシーMCP指示を削除し、さらに `Pipeline` にてアシスタントの応答時に生のツール呼び出しJSONブロック（` ```json ... ``` `）をフィルタリング除去する機構（JSON Leak Filter）を追加
  - **完了日**: 2026-05-28

- `[x]` **【要確認】00:12〜01:25 の約 73 分間 Heartbeat が停止**
  - **症状**: 本来 10 分間隔のはずが 00:12 → 01:25 と 73 分の空白
  - **推定原因**: 00:43 の Daily Summary 実行が gmn_sem を長時間占有 + rate limit による待機
  - **対策**: `CronService` 内の Daily Summary トリガー処理に5分間（300秒）の tokio 待機オフセットを導入し、深夜の Heartbeat との同時起動による gmn_sem ロック・セマフォ競合を回避
  - **完了日**: 2026-05-28

---

## Phase 8: Context Management 改善 ✅ 完了

> GeminiClaw のコンテキスト管理アーキテクチャを分析し、RustyClaw への取り込みを完了した（2026-05-28）。

- `[x]` **A. Heartbeat Digest の真の実装（★★★ 最優先）**
  - **現状**: Phase 7 でステートレス化済みだが、Heartbeat がユーザー活動を全く把握できていない
  - **改善**: `HeartbeatService` 実行前に `generate_heartbeat_digest()` を呼び、`heartbeat-digest.md` を更新してから Heartbeat プロンプトに含める
  - **仕様**: 増分スキャン（`lastRunTimestamp` 管理）+ 6回毎 deep scan（24時間分）+ 最大 3000 文字
  - **影響範囲**: `rustyclaw-gateway/src/lib.rs`（HeartbeatService）+ 新規 `generate_digest()` 関数
  - **期待効果**: Heartbeat の proactive 投稿精度が向上
  - **参照**: GeminiClaw `src/agent/session/heartbeat-digest.ts`

- `[x]` **B. Session-level Summary の実装（★★★）**
  - **現状**: Daily Summary のみ。セッション単位のサマリーなし → Session Continuation の精度が低い
  - **改善**: 会話がアイドル（5分以上更新なし）になったときにセッション単位サマリーを生成
  - **出力**: `memory/summaries/<date>-<slug>.md`（TL;DR + topics + decisions）
  - **影響範囲**: `rustyclaw-gateway/src/lib.rs`（新規 `SummaryService` or `CronService` 拡張）
  - **期待効果**: 日またぎのコンテキスト継続品質向上
  - **参照**: GeminiClaw `src/agent/session/summary.ts`

- `[x]` **C. JSONL 削減（truncateBefore）（★★）**
  - **現状**: Discord セッション JSONL が無制限成長
  - **改善**: Daily Summary 生成後、30日以上前のエントリを削除
  - **影響範囲**: `rustyclaw-storage`（`SessionLogger` に削減メソッド追加）
  - **前提**: B の Session Summary 実装後に着手

- `[x]` **D. Session Summary の増分更新（★★）**
  - 既存サマリーの `turns` と JSONL 行数を比較し、差分エントリ + 既存 TL;DR のみで更新
  - **前提**: B の基本実装後に追加

- `[x]` **仕様書の更新**
  - 実装完了後、`docs/specs/04_heartbeat_spec.md` と `docs/specs/02_agent_pipeline.md` を最新コードに同期

---

---

## Phase 9: 自前 MCP クライアント実装 (rustyclaw-mcp) と外部サーバー統合 ✅ 完了

> 長期課題として残されていた PicoClaw 方式の自前 MCP クライアント (`rustyclaw-mcp`) を新設し、各外部サーバーへの接続とアジェンティックループへの統合を完全に完了した (2026-05-28)。

- `[x]` **1. rustyclaw-tools クレートの新設**
  - `[x]` 共通 `Tool` トレイト、`ToolResult`、および `ToolRegistry` の実装
- `[x]` **2. rustyclaw-mcp クレートの新設**
  - `[x]` JSON-RPC 2.0 に基づく stdio 接続モデル、初期化ハンドシェイク、ツール一覧取得 (`tools/list`)、ツール呼び出し (`tools/call`) を実装
  - `[x]` 接続診断テスト `test_real_mcp_servers_connectivity` を追加
- `[x]` **3. Gateway & Agent アジェンティックループへの統合**
  - `[x]` Gateway の `mcp_manager` 経由で起動時に MCP ツールをロードし、`tool_registry` に自動登録
  - `[x]` Agent の `execute_with_tools` ループにおいて、LLM からの `tool_calls` 要求をインターセプトし自律実行するマルチターンループを完成
- `[x]` **4. 外部 MCP サーバー接続設定の反映**
  - `[x]` **Google Calendar**: `@anthropic-ai/mcp-server-google-calendar` (npx)
  - `[x]` **Gmail**: `@anthropic-ai/mcp-server-gmail` (npx)
  - `[x]` **Karakeep**: `@karakeep/mcp` (npx, API キー & サーバーアドレス対応)
  - `[x]` **Obsidian**: `mcp-obsidian` (uvx, REST API ホスト 192.168.1.2 接続対応)
  - `[x]` 実機での接続点検テストを実行し、すべてのハンドシェイクと疎通が正常であることを実証
- `[x]` **5. Karakeep 運用スクリプトの点検と Agent 指示追加**
  - `[x]` `karakeep_cleanup.sh` / `karakeep_tag_items.sh` の動作点検
  - `[x]` エージェント行動規範 `AGENTS.md` にスクリプトの使用指示セクションを追記し、Agentが自律的に実行可能な状態に統合

---

## Phase 10: gmn (Gemini CLI) エラーヘルプ表示の抑制（SilenceUsage） ✅ 完了

> API レート制限（429）や不正なフラグ指定などのエラーによる異常終了の際、不要な Usage ヘルプメッセージやオプション説明がログ（`rustyclaw.log` 等）に大量出力されるのを防ぎ、エラー内容のみを出力させる品質改善（2026-05-28）。

- `[x]` **1. gmn のソースコード修正**
  - `/home/kazuaki/Projects/gmn/master/src/cmd/root.go` の `rootCmd` 定義ブロックに `SilenceUsage: true` を追記。
- `[x]` **2. gmn バイナリのビルド・デプロイ**
  - `/home/kazuaki/Projects/gmn/` のビルドスクリプトを実行し、WSL/Linux (x86_64) 向け `gmn` をリビルド。
  - 新しい `gmn` バイナリを `~/.local/share/go/bin/gmn` に上書き配置（デプロイ）。
- `[x]` **3. 動作検証**
  - `gmn --invalid-flag-abc` を実行し、`Usage:` の出力が完全に抑制され、`Error: unknown flag...` のみが出力されることを実証。

---

## Phase 11: 動的レートリミットバックオフ待機（Quota Reset 解析） ✅ 完了

> gmn (Gemini CLI) の 429 エラー発生時、エラーメッセージに含まれる `Your Quota will reset after XXs.` や `XXm YYs` などのリセットまでの待機時間を解析し、Rust 側のバックオフ待機時間として動的に適用する最適化改善（2026-05-28）。

- `[x]` **1. ProviderError の拡張と解析メソッド実装**
  - `rustyclaw-providers` の `ProviderError` enum に `reset_after(&self) -> Option<Duration>` を追加。
  - `"Your Quota will reset after XXs"`、`"XXm YYs"`、`"XXm"` などの多様な時間形式を頑健に分・秒単位で自動パースして合算秒数を算出するロジックを実装。
- `[x]` **2. ユニットテストの拡張と検証**
  - 単一の秒数表記に加え、分秒混在表記（`1m 30s` 等）、分のみ表記（`2m` 等）の様々なパターンのエラーメッセージから正しく秒数が解析できることを検証するテストケースを追加・実証。
- `[x]` **3. Gateway 側のリトライバックオフ処理への動的適用**
  - `rustyclaw-gateway` の3つのリトライループにおいて、解析された待機時間がある場合はそれに安全マージン（2秒）を加えた時間を `backoff` スリープ時間として動的に採用するよう修正。

---

## Phase 12: rustyclaw-cli --version およびビルド時刻表示の追加 ✅ 完了

> rustyclaw-cli に `--version`（および `-V`）フラグを追加し、パッケージバージョン情報と併せてビルド時刻（コンパイル完了時刻）を動的かつクリーンに表示するUX品質改善（2026-05-28）。

- `[x]` **1. ビルドスクリプト (build.rs) の新設**
  - `rustyclaw-cli` 内に `build.rs` を新設し、コンパイル時に OS の `date` コマンドから動的にビルドタイムスタンプを取得して環境変数 `BUILD_TIME` にインジェクションする仕組みを構築。
- `[x]` **2. clap の CLI メタデータ拡張**
  - `Cli` clap パーサーの `#[command(version)]` 属性を拡張し、`concat!` マクロにより Cargo バージョンとビルド時刻を綺麗に出力するように定義。
- `[x]` **3. ログ初期化順序の最適化による UX 改善**
  - `setup_logging` より前に `Cli::parse` を先行実行するようにリファクタリング。これにより `--version` や `--help` などのクエリ時に余計なログファイル初期化メッセージを出力させず、クリーンにメタデータのみを表示するプロ仕様の動作に改善。

---

## 継続検討課題

- `[ ]` **`gmn_sem > 1` の並列化復活（2026-05-28 積み残し）**
  - 現状 `gmn_sem=1` で全 gmn プロセスを直列化中（共有ファイル競合防止のため）
  - 並列化を再導入するには以下のいずれかが前提条件：
    - B案: `run-progress.json` によるソフト保護（TOCTOU 問題が残るため部分的対策）
    - C案: プロバイダー層でのファイルロック機構（Gemini CLI サブプロセス経由のため実装難度高）
  - 詳細設計は `docs/specs/05_gateway_spec.md` の `[^gmn_sem]` 脚注を参照

- `[ ]` **Phase 5 MCP 統合時の Heartbeat 所要時間増大への対応（2026-05-28 積み残し）**
  - Calendar / Gmail MCP ツール統合後、Heartbeat が gmn_sem を 1〜5 分占有する可能性
  - ユーザー対話が最大 5 分待機を強いられる場合がある
  - 詳細は `docs/specs/05_gateway_spec.md` の `[^mcp_heartbeat]` 脚注を参照
