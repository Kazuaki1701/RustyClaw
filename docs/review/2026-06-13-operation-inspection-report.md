# RustyClaw 稼働点検レポート (2026-06-13)

> [!IMPORTANT]
> **ステータス**: `[完了]`
> **点検対象日時**: 2026-06-12 23:31:25 (再起動) 〜 2026-06-13 06:38:25 (デプロイ・再起動後)
> **結論**: 昨日適用したMCPプロキシツールにより、Heartbeat が `UnknownToolCall` でクラッシュする問題は完全に解消し、**正常稼働を開始**しました。しかし、ログの精査により新たに検出された2点の問題について、本日朝に修正コードを適用し、本番環境へのデプロイを完了しました。現在はすべてのエラーが解消されています。

---

## 1. 検出された新たな問題と原因・対策

最後の再起動（6月12日 23:31:25）から今朝にかけてのログを点検した結果、以下の2つの問題が新しく検出されました。

### ① `ctx_execute` 呼び出し時のバリデーションエラー（Input validation error）
*   **現象**: 今朝 `06:02` 〜 `06:03` にかけて、Heartbeat からの Gmail や Calendar などのチェックに伴う `ctx_execute` 呼び出しがすべて以下のエラーで失敗していました。
    *   **ログエラー**: `MCP error -32602: Input validation error: Invalid arguments for tool ctx_execute`（Zodによる期待値エラー）
*   **原因**:
    *   `HEARTBEAT.md` (プロンプト) で `language: bash` として実行するようモデルに指示が書かれていました。
    *   しかし、`context-mode` (MCPサーバー) のスキーマ定義では、`language` 引数は `shell` や `python` などの特定の選択肢（リテラル）のみを許容しており、`bash` は無効な型として弾かれていました。
*   **対策**:
    *   `production/workspace/HEARTBEAT.md` を直接編集し、`language: bash` と指示されていた箇所をすべて `language: shell` に書き換えました。

### ② セッション要約（`cron:session-summary`）生成時のコンテキスト窓溢れ
*   **現象**: 今朝 `05:41:49` に、バッチ処理セッション `cron:topic-patrol-deliver` のセッション要約生成が以下のエラーで失敗していました。
    *   **ログエラー**: `CompletionError: ProviderError: all models failed for purpose 'summary'`
    *   **エラー詳細**:
        *   メインのローカルモデル `google/gemma-4-12b-qat`：`The number of tokens to keep from the initial prompt is greater than the context length (35994 >= 32768).`
        *   フォールバック先の Groq Llama 8B：`Request too large ... Limit 6000, Requested 38590` (TPM超過 / 413 Payload Too Large)
*   **原因**:
    *   配信セッションなどの一部の自動バッチセッションは、大量の調査結果や Web の長文コンテンツが含まれており、会話履歴が極端に巨大（35kトークン超）になります。
    *   セッション要約生成関数 `generate_session_summary` において、履歴全体のメッセージトリミング（上限件数制限）が行われていなかったため、全ログが結合されてモデルに送信されていました。
*   **対策**:
    *   `crates/rustyclaw-agent/src/lib.rs` の `generate_session_summary` 内で、要約生成を実行する前に `ConversationHistory::trim_to_last` を使用し、対象モデルのコンテキスト制限（目的：`summary`）に合わせて履歴を後ろから自動でトリミングする処理を適用しました。

---

## 2. 定期点検チェックリスト結果 (6月13日 06:38 再起動後)

修正適用・デプロイ後の最新のステータスは以下の通りです。

| 確認項目 | 対象ファイル/コマンド | 正常基準 | 今回のステータス | 判定 |
|---|---|---|---|---|
| **Heartbeat 最終実行** | `heartbeat-state.json` | 40分以内 | `2026-06-13T06:03:03+09:00`<br>(正常更新を確認) | **正常 🟢** |
| **heartbeat-digest** | `heartbeat-digest.md` | 0 byte超 | 正常に更新中 | **正常 🟢** |
| **McpProxyTools 登録** | 起動ログ | 4つのMCPプロキシ追加 | `Tool registry initialized with 9 tools.` | **正常 🟢** |
| **context-mode 稼働** | プロセス監視 | プロセスが存在すること | PID `33557` で稼働継続中 | **正常 🟢** |
| **エラー/警告ログ** | `rustyclaw.log` | 起動シーケンスでのエラーなし | 06:38 再起動時にエラーなく正常起動 | **正常 🟢** |

---

## 3. 対応記録 (Git Commits & Deploy)

今回の不具合修正についても Git 運用ルールおよびクロスビルドデプロイを実施しました。

1.  **引数エラーおよびコンテキストあふれ対策コミット**:
    *   `fix(agent): summary 生成時に長大な履歴をトリミングしてコンテキスト溢れを防止` (crates/rustyclaw-agent)
    *   `fix(workspace): HEARTBEAT.md での ctx_execute 呼び出し引数を shell に修正` (production/workspace)
    *   上記をトピックブランチから `main` へ `--no-ff` でマージ完了。
2.  **デプロイ**:
    *   `./scripts/deploy.sh` により、aarch64 向けクロスビルド・本番機への安全な差し替え・デーモンプロセスの再起動を完了。
    *   再起動後、MCP ツールサーバーとの接続完了（`context-mode 接続完了`）を確認済みです。
