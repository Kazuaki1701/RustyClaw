---
date: "2026-04-02"
session: "discord-1484163743633117289-1485590981251432560-1489165841902600212"
trigger: "discord"
turns: 1
tokens: 0
duration_min: 0
tags:
  - session/discord
  - topic/obsidian
  - topic/cli
  - topic/home-assistant
  - topic/sync
---

# obsidian-headless 調査

## TL;DR
新しく公開された `obsidian-headless` を使った同期の無料運用の可否を調査しました。本ツールは Obsidian Sync などの有料プラン契約が必須であり、無料でのヘッドレス同期には Syncthing 等の代替手段が引き続き必要であることが分かりました。

## Topics
- **Home Assistant アップデート**: 2026.4 リリースの新機能である赤外線プロキシやバックアップ強化について共有しました。
- **obsidian-headless の調査**: 公式 CLI クライアントの同期機能が、Obsidian Sync の有料サブスクリプションを必要とすることを確認しました。

## Key Decisions
- obsidian-headless による無料同期の断念

## Conversation Log
### 16:33 — User
[Context: Thread started from message]
> GEMI Agent: Kさん、おはようございます。☕️
今朝のトピック・パトロールで、興味深いアップデートをいくつか見つけました。

まず、Home Assistant 2026.4 がリリースされました！
ESPHomeなどを『赤外線プロキシ』としてネイティブに扱えるようになり、古い家電のスマート化がより公式に、簡単に行えるようになっています。バックアップも新方式のSecureTar v3になり、セキュリティが強化されていますよ。
Source: https://www.home-assistant.io/blog/2026/04/01/release-20264/

もう一つ、Kさんがお使いの CLI ツール（…
> GEMI Agent: Kさん、こんにちは！午後のパトロールでも面白いものを見つけました。

特に Obsidian をお使いの K さんには朗報かもしれません。
Obsidian 公式のヘッドレスクライアント（`obsidian-headless`）が GitHub で公開されています。デスクトップアプリを立ち上げなくても、コマンドラインから Sync や Publish が実行できるツールです。
これを使えば、Raspberry Pi や QNAP などのサーバー側で、バックグラウンドで常に保管庫を最新の状態に保つオートメーションが組めそうですね。
Source: https://github.com/obsid…
> **🤖K: <@1484954911337611515> 

https://github.com/obsidianmd/obsidian-headless

ヘッドレスシンクで sync する場合、無料運用が可能か、確認して。** ← (referenced)
[discord] 🤖K: <@1484954911337611515> 

https://github.com/obsidianmd/obsidian-headless

ヘッドレスシンクで sync する場合、無料運用が可能か、確認して。

### 16:33 — Agent
`obsidian-headless` について調査した結果、残念ながら **完全な無料運用は難しい** という結論になりました。

このツールは **Obsidian 公式の有料サービス（Obsidian Sync または Publish）を利用するための専用クライアント** です。ツール自体はオープンソースで無料ですが、同期機能（`ob sync`）を実行するには、有効な **Obsidian Sync のサブスクリプション（月額制）** を持ったアカウントでのログインが必須となります。

もし無料でヘッドレスな同期を実現したい場合は、従来通り **Syncthing** や **Git**、あるいは以前検討した **QNAP の同期機能** などを活用し、バックグラウンドでファイルをやり取りする構成が引き続き現実的な選択肢になりそうです。

公式ツールでスマートに同期できれば理想的でしたが、こちらはあくまで「公式 Sync ユーザー向けの CLI 版」という位置づけのようです。🌸

