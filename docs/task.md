# Task List — RustyClaw

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

## Phase 6: 次回セッション向け Todo

- `[ ]` **gmn デバッグログの稼働確認**
  - `RUST_LOG=debug` で起動し `gmn spawn: args` / `gmn exit: response` が出力されるか確認
  - rate limit 時の stderr 内容が `WARN` として拾えているか確認

- `[ ]` **Memory Flush の動作確認**
  - 実チャット 6 ターン後に flush が起動するか確認（`memory flush: starting` ログ）
  - 15 分ゲートのスキップ動作確認（`memory flush: skipping (time gate...)` ログ）
  - `MEMORY.md` が全書き直しされ 5KB 以内に収まっているか確認

- `[ ]` **Dashboard の動作確認**
  - `MEMORY.md`・`heartbeat-digest.md`・`heartbeat-state.json` の表示確認
  - 5 秒ポーリングで内容が更新されるか確認

- `[ ]` **rate limit 時のリトライ戦略検討**
  - 現状: `GMN_MAX_RETRIES=0` で即エラー返却（RustyClaw 側でリトライなし）
  - 検討: `LaneRegistry` レベルでの指数バックオフリトライ実装
  - 検討: rate limit エラー時のユーザー通知メッセージ整備

- `[ ]` **Session Continuation の動作確認**
  - 日またぎセッションで前日サマリーが注入されるか確認
  - `memory/summaries/` の Daily Summary 生成確認
