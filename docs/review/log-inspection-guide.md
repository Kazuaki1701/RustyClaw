# RustyClaw 動作ログ & LLM 入出力 点検ガイド

本ドキュメントは、RustyClaw のデプロイ後や定期メンテナンス時に、動作ログおよび LLM のリクエスト/レスポンスを効率的かつ正確に点検するための手順と観点をまとめたガイドラインです。

---

## 1. 対象環境と基本構成

*   **開発・管理機**: `Ubuntu-NUCXI7` （本作業を実施するローカルマシン）
*   **本番・デプロイ先**: Raspberry Pi 4 (`rp1`)
    *   開発機から `ssh rp1` を介してリモート操作を行います。
    *   `production/` ディレクトリはNAS共有等により、開発機と同期または参照可能な構造になっています。

---

## 2. サービス・プロセス動作の確認 (システムログ点検)

デプロイ後、または稼働状況の確認時には、まず systemd サービスがエラーなく起動し、正常なサイクルで動いているかを確認します。

### 2.1. 実行コマンド

開発機側のターミナルから以下のコマンドを実行して、リモートの `rp1` から直接ログを取得します。

*   **サービスステータスの確認**:
    ```bash
    ssh rp1 "sudo systemctl status rustyclaw.service"
    ```
*   **直近のジャーナルログ（100行分）の確認**:
    ```bash
    ssh rp1 "sudo journalctl -u rustyclaw.service -n 100 --no-pager"
    ```
*   **特定のデプロイ日時以降の全ログ確認**:
    ```bash
    ssh rp1 "sudo journalctl -u rustyclaw.service --since '2026-06-13 06:38:00' --no-pager"
    ```
*   **ログが長くターミナルで切り捨てられる場合の回避策**（一時ファイルへ書き出して `view_file` で確認する）:
    ```bash
    ssh rp1 "sudo journalctl -u rustyclaw.service --since '2026-06-13 06:38:00' --no-pager" > ./production/logs/journal_temp.log
    # 点検後、必ず一時ファイルは削除してください: rm ./production/logs/journal_temp.log
    ```

### 2.2. システムログ点検 of チェックポイント
- [ ] **SIGTERM & Graceful Shutdown**: 再起動時に旧プロセスが正常に終了し、Discord Shard や Gateway の切断処理が完了しているか。
- [ ] **ポートのバインド**: Web Preview Server (`port 4000`)、HealthServer (`port 8080`) が正常起動しているか。
- [ ] **MCP サーバー (`context-mode`) の起動**: bun 経由で正常に起動し、`ctx_execute`, `search`, `index`, `patch` の登録が完了しているか。
- [ ] **Discord Bot の接続**: `bot_name=GEMI Agent` で接続に成功し、Ready 状態になっているか。
- [ ] **Cron サービスの開始**: 各種定期スケジューラ（Daily Summary, Heartbeat, Session Summary など）がエラーなく初期化されているか。

---

## 3. LLM リクエスト / レスポンスの点検

対話や定期ジョブの発生時、LLM とのやり取りの内容は JSON ダンプとして保存されます。これらを直接開いて精査します。

### 3.1. ダンプファイルの保存場所
ダンプは日付ごとのディレクトリに格納されています。
*   **ダッシュボード対話ログ**: `production/workspace/memory/debug/llm/dashboard/YYYY-MM-DD/HH-MM-SS.json`
*   **Discord 対話ログ**: `production/workspace/memory/debug/llm/discord/YYYY-MM-DD/HH-MM-SS.json`
*   **定期ジョブ（Heartbeat）ログ**: `production/workspace/memory/debug/llm/heartbeat/YYYY-MM-DD/HH-MM-SS.json`

### 3.2. JSON データの構造とチェックポイント

各 JSON ファイルを開き、以下のフィールドと中身を点検します。

```json
{
  "timestamp": 1781300428,
  "model": "google/gemma-4-12b-qat",
  "request": [
    { "role": "system", "content": "...(SOUL.md + USER.md)..." },
    { "role": "user", "content": "...(ユーザー発話 + スキル定義)..." }
  ],
  "response": {
    "content": "...(生成応答)...",
    "role": "assistant",
    "tool_calls": null,
    "prompt_tokens": 10025,
    "completion_tokens": 373,
    "total_tokens": 10398
  }
}
```

- [ ] **`model` (モデル選択)**: 指定したローカルモデル（例: `google/gemma-4-12b-qat`）が正しく使用されているか。
- [ ] **`request` (システムプロンプトの注入)**:
    *   `system` メッセージに `SOUL.md` と `USER.md` の内容が崩れずに注入されているか。
    *   `user` メッセージの末尾に、利用可能なスキル定義一覧（各スクリプトパス含む）が正しく整形されてマッピングされているか。
- [ ] **`response` (応答の品質)**:
    *   自然な日本語（です・ます調）で、文字数制限（Discordの場合は 2000文字以内、簡潔さ優先）を意識した内容になっているか。
    *   余分な JSON スニペットの漏れ出しや、未フォーマットのシステムテキストが含まれていないか。
- [ ] **`tool_calls` (ツール呼び出し)**:
    *   ツール使用が必要な発話の場合、適切な引数で呼び出せているか。
    *   ツール呼び出しが不要な場合（挨拶など）は、正しく `null` になっているか。
- [ ] **`prompt_tokens` (トークン消費量)**:
    *   トークン数がコンテキスト上限（32,768 トークン）に近すぎないか（10,000トークン前後が適正目安）。
    *   コンテキスト溢れによる対話の破綻がないか。

---

## 4. メモリ管理 & バックグラウンド処理の点検

会話完了後にトリガーされるメモリ自動要約（`memory flush`）についても動作を追跡します。

### 4.1. チェックポイント
- [ ] **Memory Flush の起動**: 会話直後に `INFO memory flush: starting session=...` のログが出力され、LLM リクエストが正常に送信されているか。
- [ ] **Heartbeat の実行**: 30分ごとの Heartbeat 実行時に、`ctx_execute` が `shell` 引数（`bash` はエラーとなるため不可）でエラーなく呼び出されているか。

---

## 5. トラブルシューティング（よくあるエラーと対応）

1.  **`ctx_execute` バリデーションエラー (`expected: 'shell', received: 'bash'`)**
    *   **原因**: プラットフォーム（context-mode）が期待する実行ツール引数名が `shell` であるのに対し、古い `HEARTBEAT.md` や設定で `bash` が使われている。
    *   **対策**: `production/workspace/HEARTBEAT.md` の中の記述を `shell` に統一し、サービスを再起動する。
2.  **`session-summary` でのコンテキスト溢れ**
    *   **原因**: 会話履歴が長くなり、システムプロンプト＋会話履歴＋スキル定義の総トークン数がモデルの限界（32k）を超えてバリデーションエラーが発生する。
    *   **対策**: エージェント（`rustyclaw-agent` 内など）に実装されている履歴トリミング機能が正しく作動しているか確認し、必要に応じてセッションの初期化や履歴の刈り込みを手動または自動で行う。
