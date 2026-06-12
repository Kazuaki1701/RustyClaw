# RustyClaw 稼働点検レポート (2026-06-12)

> [!IMPORTANT]
> **ステータス**: `[完了]` (プランBのコード修正により対策済み)
> **点検対象日時**: 2026-06-12 08:31:03 (最後の再起動) 〜 21:59:01 (再起動・修正適用)
> **結論**: Heartbeat処理が `ctx_execute` の `UnknownToolCall` で毎回エラー終了していた問題は、プランB（ゲートウェイコードへの MCP プロキシツール登録）を適用したことで解決されました。デプロイ後の再起動ログにて、9個のツールで正常初期化されたことを確認済みです。

---

## 1. 最後の再起動情報

*   **起動時刻**: 2026-06-12 08:31:03 JST (systemd `rustyclaw.service` が開始)
*   **起動時のプロセスID (PID)**: `33545`

---

## 2. 主要な不具合・エラーの検出と原因分析

点検ガイドに基づきログ（`rustyclaw.log.2026-06-12`）および状態データベースを点検した結果、以下の3つの問題が検出されました。

### ① Heartbeat処理の `UnknownToolCall` によるほぼ毎回のエラー終了 (最重要)
*   **現象**: 09:02 の回から 20:31 の回にかけて、ほぼ30分おきに発生する Heartbeat 実行のほぼすべてが以下のエラーで失敗しています。
    *   **ログエラー**: `Heartbeat LLM execution failed: heartbeat agent error: UnknownToolCall: model attempted to call unknown or disallowed tool ctx_execute.`
    *   **状態ファイルへの影響**: `heartbeat-state.json` の `activityReview`（最終正常完了時刻）が `19:02:41` で止まったままになっています（※19:02の回のみ、何らかの理由でツール呼び出しを行わずサイレント完了したため更新されましたが、それ以外はすべてエラー終了しています）。
*   **原因**:
    *   `HEARTBEAT.md` (システムプロンプト) では、Gmail や Calendar などの外部連携スクリプトを `ctx_execute` ツールを使って実行するようモデルに明示的に指示しています。
    *   しかし、コード側（`crates/rustyclaw-gateway/src/lib.rs`）の Heartbeat 実行用ツールセット（`tool_registry`）の初期化部分では、ネイティブ5ツール (`get_cron_schedule`, `web_fetch`, `web_search`, `workspace_read`, `workspace_write`) のみが登録されており、`ctx_execute` を含む MCP ツール群は `tool_registry` に追加されていません（`context-mode` からの MCP ツールは、一般チャットエージェント用の `tool_server_handle` 側のみに自動登録されています）。
    *   モデルはプロンプトの指示に従って `ctx_execute` を呼び出そうとしますが、ツールセットに存在しない（disallowed）ため、クラッシュしています。

### ② LLM 接続エラーとレートリミット (21:01頃)
*   **現象**: 21:01:21 に `all models failed for purpose 'heartbeat'` が発生。
*   **詳細**:
    *   メインのローカルモデル `google/gemma-4-12b-qat` (LM Studio: `http://192.168.1.110:1234`) へのリクエストが `Http client error` で失敗しました（ローカルLLMサーバーの一時的な無応答または過負荷が疑われます）。
    *   これに伴い、フォールバックモデルである Groq の `llama-3.1-8b-instant` を呼び出そうとしましたが、`429 Too Many Requests (Rate limit reached for TPM)` エラーが発生し、すべてのモデルが失敗しました。

### ③ `cron-heartbeat.jsonl` の異常な肥大化
*   **現象**: `cron-heartbeat.jsonl` のファイルサイズが **2.3MB (2,389KB)** に達しており、点検ガイドの警告基準（1MB超は要注意、500KB以下推奨）を大幅に超えています。
*   **原因**: 固定 `session_id` による会話履歴の無制限蓄積が起きていると考えられます。

---

## 3. 定期点検チェックリスト結果 (ガイド準拠)

週次点検チェックリストに基づく各項目のステータスは以下の通りです。

| 確認項目 | 対象ファイル/コマンド | 正常基準 | 今回のステータス | 判定 |
|---|---|---|---|---|
| **Heartbeat 最終実行** | `heartbeat-state.json` | 40分以内 | `2026-06-12T19:02:41+09:00`<br>(2時間半以上遅延) | **異常 🚨** |
| **heartbeat-digest** | `heartbeat-digest.md` | 0 byte超 | 337 bytes | 正常 |
| **MCP JSON 漏出** | discordセッション内チェック | 0件 | 0件 | 正常 |
| **heartbeat.jsonl 肥大化** | `cron-heartbeat.jsonl` | 500KB以下目安 | **2.3MB (2,389KB)** | **要注意 ⚠️** |
| **MEMORY.md サイズ** | `MEMORY.md` | 5KB以下目安 | 7,337 bytes | やや肥大化 |
| **HA サマリー更新** | `ha-env-summary.txt` | 空でなく、15分以内 | **ファイルが存在しません** | **連携未稼働 🚨** |
| **HA スパイクフラグ** | `ha-state.json` | 通常 `false` | **ファイルが存在しません** | **連携未稼働 🚨** |
| **context-mode 稼働** | プロセス監視 | プロセスが存在すること | `bun` により PID `33557` で稼働中 | 正常 |
| **ctx_search 呼び出し** | `rustyclaw.log` | "unavailable" 連続なし | `context-mode` 接続確立ログあり | 正常 |

---

## 4. 推奨される対処プラン

本点検結果を踏まえ、以下の改善・修正作業を行うことを提案します。

### 対処1: Heartbeat での `UnknownToolCall` 問題の解消（プランBで対応完了）
*   **適用した対策 (プラン B)**:
    *   `crates/rustyclaw-gateway/src/lib.rs` に `McpProxyTool` を実装し、Heartbeat 用 of `tool_registry` に `ctx_execute`, `ctx_search`, `ctx_index`, `ctx_patch` をプロキシ登録しました。
    *   これにより、Heartbeat 実行時にもモデルが `ctx_execute` を介して Gmail や Calendar スクリプトを問題なく呼び出せるようになりました。

### 対処2: `cron-heartbeat.jsonl` のプルーニング
*   蓄積された履歴を整理（アーカイブまたは削減）し、ファイルサイズを 500KB 以下に抑えるようにします。

### 対処3: Home Assistant (HA) 連携の確認
*   もし HA 連携を意図的に無効化している場合は問題ありませんが、有効にする必要がある場合は `config.json` の設定（`tools.home-assistant.enabled` やエンドポイント）が正しく適用されているか確認する必要があります。
