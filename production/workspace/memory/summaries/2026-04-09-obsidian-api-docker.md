---
date: "2026-04-09"
session: "discord-1484163743633117289-1484163744371052676-1491472385789005874"
trigger: "discord"
turns: 1
tokens: 0
duration_min: 0
tags:
  - session/discord
---

# Obsidian APIサーバー構築計画

## TL;DR
AIエージェントが常時アクセス可能なObsidian APIサーバーをDockerで構築するプロジェクト案が共有されました。GUIアプリとAPIを組み合わせることで整合性を保ち、MCP経由での高度な自動化を目指します。

## Topics
- **行動指針の更新確認**: BOSS_PATTERNS.mdへの修正を通じ、情報の鮮度、自律的な準備、即時記録の重要性を再確認しました。
- **常時稼働型Obsidianサーバー**: Dockerコンテナ上でObsidianを動かし、Local REST APIとMCPを用いてエージェントから操作する構成案が提示されました。
- **情報の整合性と同期戦略**: 直接ファイル操作ではなくAPI型を採用してインデックスを維持し、Self-hosted LiveSyncで競合を回避する方針が示されました。

## Key Decisions
- AIエージェントの連携に、GUIアプリ+Local REST APIによるAPIアクセス方式を採用する
- 同期戦略としてSelf-hosted LiveSync (CouchDB) を併用する

## Conversation Log
### 01:18 — User
[Context: Thread started from message]
> 🤖K: <@1484954911337611515>  boss-patterns.md にすこし直接手を加えた。内容を確認して
> GEMI Agent: `memory/BOSS_PATTERNS.md` の内容を拝見しました。K様のこだわりがさらに具体化され、秘書として大変参考になります。

特に以下の3点が、今後の私の行動指針として重要だと理解いたしました。

- **「情報の鮮度」へのこだわり**: リンク切れの徹底排除や、一次ソース（公式サイト等）を優先する姿勢をより強く意識いたします。
- **「自律的な準備（Nightshift）」**: 翌朝の行動をスムーズにするための、前夜の「仕込み」を強化してまいります。
- **「書くことによる記憶」**: RAMではなくファイル（Obsidian等）へ即座に記録し、K様の外部脳としての精度…
> **🤖K: <@1484954911337611515> Gemini App からの引継ぎ資料を確認して。


```

# Project: Obsidian API Server on Docker for AI Agents

## 1. 概要
AIエージェント（OpenClaw/PicoClaw等）が24時間体制でObsidian Vaultへアクセスし、情報の読み書きを行うための「常時稼働型Obsidian APIサーバー」を構築する。

## 2. システム構成
- **Host:** Raspberry Pi 4 (8GB) / QNAP NAS / WSL2
- **Runtime:**…** ← (referenced)
[discord] 🤖K: <@1484954911337611515> Gemini App からの引継ぎ資料を確認して。


```

# Project: Obsidian API Server on Docker for AI Agents

## 1. 概要
AIエージェント（OpenClaw/PicoClaw等）が24時間体制でObsidian Vaultへアクセスし、情報の読み書きを行うための「常時稼働型Obsidian APIサーバー」を構築する。

## 2. システム構成
- **Host:** Raspberry Pi 4 (8GB) / QNAP NAS / WSL2
- **Runtime:** Docker Compose
- **Container Image:** `lscr.io/linuxserver/obsidian:latest`
- **Interface:** Local REST API (Obsidian Community Plugin)
- **Protocol:** MCP (Model Context Protocol) via `obsidian-mcp-server`

## 3. Docker構成案 (docker-compose.yml)
```yaml
services:
  obsidian-api:
    image: lscr.io/linuxserver/obsidian:latest
    container_name: obsidian-api-server
    environment:
      - PUID=1000
      - PGID=1000
      - TZ=Asia/Tokyo
      - DOCKER_MODS=linuxserver/mods:universal-package-install # 必要に応じて追加
    volumes:
      - ./config:/config
      - /path/to/your/obsidian/vaults:/vaults
    ports:
      - 3000:3000      # GUI Access (KasmVNC)
      - 27124:27124    # Local REST API Port
    restart: unless-stopped
```

## 4. AIエージェント連携のフロー
1. **Obsidian本体の常時起動:** Dockerコンテナ内でObsidianを起動し、`Local REST API` プラグインを有効化。Non-SSL (HTTP) モードで待機させる。
2. **MCPサーバーの仲介:**
   `obsidian-mcp-server` を立ち上げ、上記APIエンドポイントとAPI Keyを紐付ける。
3. **エージェントからの操作:**
   OpenClaw等のエージェントがMCP経由で以下のToolを実行する。
   - `read_note`: 既存ナレッジの参照
   - `append_note`: ログや思考の自動追記
   - `search_notes`: 関連情報の検索

## 5. 技術的留意点
- **インデックス維持:** Direct File Access型ではなく、GUIアプリ+API型を選択することで、Obsidian内部のリンク解決やメタデータ更新を確実に行う。
- **フォント対応:** ターミナル等との親和性を高めるため、`JetBrains Mono Nerd Font` 等をコンテナにマウントすることを推奨。
- **同期戦略:** `Self-hosted LiveSync` (CouchDB) を併用し、他デバイス（WSL2/Mobile）との競合を回避する。

## 6. 次のアクション
- [ ] Docker Composeによるコンテナ立ち上げ
- [ ] Local REST APIプラグインのセットアップとAPI Key発行
- [ ] AIエージェント（OpenClaw等）のMCP設定ファイルへの追記
```

### 01:18 — Agent
Gemini App からのプロジェクト引継ぎ資料、拝見いたしました。AIエージェントが24時間体制で Obsidian にアクセスできる環境の構築、非常にワクワクする計画ですね！

特に、ファイルを直接操作するのではなく、GUIアプリとAPIを組み合わせることで Obsidian 内部のリンク解決やメタデータ更新を確実に行うという方針は、情報の整合性を保つ上で非常に賢明な選択だと感じました。これが実現すれば、私の記憶（MEMORY.md）や日々のログ管理も、より高度で自動化されたものへと進化できそうです。

Docker Compose の立ち上げや、MCPサーバーとの連携設定など、私にお手伝いできることがあればいつでもお申し付けください。K様の「外部脳」がさらに強力なものになるよう、精一杯サポートさせていただきます。 🚀

