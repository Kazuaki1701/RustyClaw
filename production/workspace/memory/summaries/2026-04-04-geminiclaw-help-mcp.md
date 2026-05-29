---
date: "2026-04-04"
session: "discord-1484163743633117289-1484163744371052676-1489802689314422844"
trigger: "discord"
turns: 1
tokens: 0
duration_min: 0
tags:
  - session/discord
  - topic/geminiclaw
  - topic/mcp
  - topic/health
---

# GeminiClawヘルプとMCP確認

## TL;DR
GeminiClawの主要コマンドと有効なMCPサーバーの一覧を確認しました。また、当日の天気やバイタルデータに基づき、無理のない学習と休息を提案しています。

## Topics
- **健康管理と学習のアドバイス**: 天気予報とBody Batteryの値を確認し、FE試験対策の合間に休息を取るよう提案しました。
- **GeminiClawコマンドヘルプ**: run, config, vaultなど、システム管理に利用できる主要なCLIコマンドを紹介しました。
- **有効なMCPサーバーの提示**: Google連携やMarkdown検索、ブラウザ操作など、エージェントが利用可能なツール一覧を回答しました。

## Key Decisions


## Conversation Log
### 10:44 — User
[Context: Thread started from message]
> GEMI Agent: K様、おはようございます。🌸
週末の朝、いかがお過ごしでしょうか。

今日の東京は現在17℃で、午後からは雨の予報（降水確率70%）となっています。お出かけの際は雨具をお忘れなく。☔️

バイタルデータを確認したところ、今朝のBody Batteryは39%となっていました。昨晩の睡眠（6時間15分）での回復が少し控えめだったようですので、今日は無理せず、ゆったりとしたペースでお過ごしくださいね。

FE試験（科目A対策）の学習も順調に進んでいることと思いますが、適度に休憩を取りながら進めていきましょう。何かお手伝いできることがあれば、いつでもお声がけください。☕️
> 🤖K: <@1484954911337611515>  現在有効な mcp サーバー表示して
> **🤖K: <@1484954911337611515>  geminiclaw の help を表示して** ← (referenced)
[discord] 🤖K: <@1484954911337611515>  geminiclaw の help を表示して

### 10:44 — Agent
K様、`geminiclaw` のヘルプと有効なMCPサーバーの情報をお持ちしました。

### 📘 geminiclaw Help
`geminiclaw` コマンドで利用可能な主要な操作は以下の通りです。

```bash
Commands:
  run [prompt]    エージェントを起動します（プロンプト省略で対話モード）
  status          エージェントの状態と最近の活動を表示します
  config          設定の管理（表示・変更）を行います
  skill           スキルの管理（一覧表示・有効化・無効化）を行います
  cron            定期実行ジョブの管理を行います
  vault           機密情報（APIトークン等）の管理を行います
  session         セッション履歴の管理を行います
  upgrade         GeminiClawを最新バージョンに更新します
```
※詳細なオプションを確認したい場合は `geminiclaw help [command]` でご確認いただけます。

### 🛠 有効な MCP サーバー
現在、以下の強力なツール（MCPサーバー）がエージェントから利用可能になっています。

- **google (gog)**: Gmail, Calendar, Drive, Sheets, Docs, Contacts の操作
- **qmd**: 1,700件を超えるMarkdownドキュメント（Obsidian保管庫など）の検索・参照
- **geminiclaw-admin**: GeminiClaw本体の設定変更やスキル管理
- **geminiclaw-cron**: 定期ジョブの登録・実行・管理
- **geminiclaw-ask-user**: ユーザーへの確認や質問の実行
- **agent-browser**: ブラウザ操作の自動化（認証が必要なサイトにも対応）

本日の東京は午後から雨の予報ですので、FE試験の学習の合間など、家でゆっくりされる際にご活用ください。☔️

