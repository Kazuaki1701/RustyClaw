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

- `[ ]` **【バグ】Heartbeat セッション履歴の無制限肥大化**
  - **症状**: `cron-heartbeat.jsonl` に 182 ターン・179KB（6 時間稼働）。10 分毎に全履歴を LLM に流し込んでいる
  - **原因**: `cron:heartbeat` という固定セッション ID を使い続けるため、会話履歴が日をまたいで追記され続ける
  - **影響**: コンテキスト肥大 → レイテンシ増大・rate limit リスク増大
  - **対策候補**: Heartbeat セッションは毎回新規 ID（例: `cron:heartbeat-YYYYMMDD-HHMM`）を使うか、会話履歴を使わずプロンプトのみで完結させる

- `[ ]` **【バグ】Heartbeat の LLM 応答が空（セッション履歴に記録されない）**
  - **症状**: `cron-heartbeat.jsonl` の全 assistant エントリが空文字 `""`
  - **原因**: `--no-agent` モードでモデルがツール呼び出し JSON のみを生成しテキスト部分を生成しない場合、gmn がテキスト部分のみを返すため空になる
  - **影響**: セッション履歴が空で蓄積し続ける（メモリの無駄）・デバッグ困難
  - **確認事項**: `process_heartbeat_response()` は activity log に正しく記録できているか（ログ上は OK に見えるが要精査）

- `[ ]` **【バグ】heartbeat-digest.md が 0 バイト**
  - **症状**: `workspace/memory/heartbeat-digest.md` が空ファイル（0 byte）のまま
  - **影響**: Dashboard の heartbeat-digest パネルに何も表示されない
  - **調査箇所**: `HeartbeatService::generate_digest()` の戻り値が空か、書き込み先パスが異なるか確認

- `[ ]` **【既知問題・要対処】MCP ツール呼び出し JSON がチャットに漏出**
  - **症状**: Discord チャンネルに `{"action": "geminiclaw_status", ...}` 等の JSON ブロックが送信される
  - **原因**: `--no-agent` でツール実行ループなし。LLM がツール呼び出し JSON を生成しそのままテキスト出力として返る
  - **対処案 A（応急）**: `Pipeline` のレスポンス後処理で ` ```json ... ``` ` ブロック（tool call パターン）をフィルタリング
  - **対処案 B（根本）**: `workspace/AGENTS.md` 等から GeminiClaw 固有 MCP ツール指示を削除 → LLM がツール呼び出しを試みなくなる
  - **対処案 C（長期）**: MCP クライアント実装（Phase 7-4）

- `[ ]` **【要確認】00:12〜01:25 の約 73 分間 Heartbeat が停止**
  - **症状**: 本来 10 分間隔のはずが 00:12 → 01:25 と 73 分の空白
  - **推定原因**: 00:43 の Daily Summary 実行が gmn_sem を長時間占有 + rate limit による待機
  - **対策候補**: Daily Summary の実行タイミングを CronService 内で Heartbeat と重ならないよう調整

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

- `[ ]` **MCP クライアント自前実装（PicoClaw 方式、長期課題）**
  - 現状: gmn CLI の `--no-agent` で LLM 呼び出しのみ行い、MCP ツール実行能力を持たない
  - 目標: PicoClaw の `pkg/mcp` に相当する Rust クレート `rustyclaw-mcp` を実装し、AgentLoop 内で直接 MCP サーバーと JSON-RPC 通信する
  - 詳細実装計画: `docs/specs/07_mcp_plan.md` を参照
  - 実装フェーズ（計画書より抜粋）:
    - Phase 7-1: Tool トレイト + ToolRegistry（`rustyclaw-tools` 実装）
    - Phase 7-2: Provider 拡張（ToolCall レスポンス対応）
    - Phase 7-3: Agent アジェンティックループ
    - Phase 7-4: `rustyclaw-mcp` クレート新設（MCP クライアント）
    - Phase 7-5: 設定統合 + Gateway への組み込み
    - Phase 7-6: ツール検索 Discovery（オプション）
  - 前提条件: `gmn_sem > 1` の並列化（共有ファイル排他制御）と同時に検討すること
