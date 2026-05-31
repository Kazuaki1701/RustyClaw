# RustyClaw ドキュメント管理・運用ルール

> [!IMPORTANT]
> **対象読者**: 開発者および AI コーディングアシスタント（Antigravity、その他エージェント）  
> 本ディレクトリ（`docs/`）配下の仕様書や計画書を管理・維持するための公式ルールです。開発や仕様変更を行う際は、必ず本ルールに従ってください。

---

## 1. 物理的なフォルダ構成（役割分離）

ドキュメントは情報の性質に基づき、物理的に以下のフォルダに隔離して管理します。

```
docs/
├── specs/                     # 【常に最新】コードの最新実装と100%一致させる「基本仕様書」
│   ├── 01_architecture.md     # 全体アーキテクチャ・開発環境仕様
│   ├── 02_agent_pipeline.md   # パイプライン・LLMプロバイダ仕様
│   ├── 03_workspace_spec.md   # ワークスペースファイル・ストレージ仕様
│   ├── 04_heartbeat_spec.md   # Heartbeat 自発行動システム仕様
│   ├── 05_gateway_spec.md     # Gateway・並列制御 Lane Queue 仕様
│   ├── 06_roadmap_decisions.md # ロードマップ・重要設計決定事項
│   ├── 07_mcp_plan.md         # [PLAN] rustyclaw-mcp 実装計画（Phase 7）
│   ├── 08_operation_inspection.md  # 稼働点検ガイド（コマンド集・既知パターン）
│   ├── 09_geminiclaw_comparison.md # GeminiClaw とのコードレベル比較・移植進捗仕様
│   ├── 10_weather_yolp_spec.md # YOLP気象情報APIリファレンス雨雲レーダー仕様
│   └── 11_skills_spec.md      # Skills システム仕様・GeminiClaw 比較・移植記録
│
├── archive/                   # 【参照のみ・編集不可】すでに完了した「過去の計画書・報告書」
│   ├── implementation_plan.md # 過去の実装計画（Phase 2 & 4）
│   ├── walkthrough.md         # 過去の実装検証・ウォークスルー（Phase 2 & 4）
│   ├── llmprover_gmn_plan.md  # 過去のデバッグプロバイダ追加計画（Phase 1）
│   └── 2026-05-27-discord-integration.md # 過去の Discord 連携計画（Phase 3）
│
├── 00_rustyclaw.md            # 全体引継ぎ資料 ＆ ドキュメントインデックス
├── task.md                    # 直近の開発タスク管理リスト（完了マーク付き）
└── README.md                  # 本ドキュメント運用ルール（本書）
```

---

## 2. メタデータ（ステータスタグ）付与のルール

すべてのドキュメントの最上部に、そのドキュメントの状態を示すメタデータブロックを記述します。

### ① 最新の仕様書（`docs/specs/` 配下）
最新の動作仕様を表すファイルには、`[ACTIVE]` タグを付与します。
```markdown
> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: YYYY-MM-DD  
> **対象コード**: `crates/<対象クレート名>/` の最新実装
```

### ② アーカイブされた計画書・報告書（`docs/archive/` 配下）
実装が完了し、変更を加えない過去のログには、`[HISTORICAL]` タグを付与します。**原則として、このファイル群を直接書き換えて新しい仕様を定義することは禁止**します。
```markdown
> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: YYYY-MM-DD  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。
```

---

## 3. 開発運用ルール（完了定義：DoD）

コードの新規実装、リファクタリング、仕様変更を行う際は、以下のドキュメント更新を**開発完了の必須定義（DoD: Definition of Done）**とします。

1. **基本仕様書の更新義務**:
   * 実装完了時、コードの変更内容に応じて、必ず `docs/specs/` 配下の該当する仕様書（例：ストレージ仕様の変更であれば `03_workspace_spec.md`）を書き換えて、**最新コードの実装と100%一致**させてください。
   * 更新時は、ファイル頭の **「最終更新日」** を当日の日付にアップデートしてください。
2. **タスク定義時のルール**:
   * `task.md` や `implementation_plan.md` に新しい機能開発タスクを追加する際、最後のステップとして必ず **「`docs/specs/` 配下の関連仕様書のアップデート」** というタスクを組み込んでください。
3. **新規計画書のライフサイクル**:
   * 新しい機能を開発する際は、計画書を `docs/plans/` フォルダ（存在しない場合は作成）または `docs/` 直下に作成して作業を進めます。
   * **実装が完了した瞬間**、その計画書は `docs/archive/` フォルダに移動し、ヘッダーを `[HISTORICAL]` に書き換えてください。そして、有効な仕様のみを `docs/specs/` に反映（同期）させてください。

---

## 4. デプロイ・SSH 接続手順（RPi4 / `rp1`）

本番は Raspberry Pi 4（aarch64, ホスト名 `rp1`）上で systemd サービス `rustyclaw.service` として稼働する。開発機（x86_64）から aarch64 バイナリをクロスビルドして配置する。

### 4-1. デプロイ先への SSH 接続

`~/.ssh/config` のエイリアスで接続する（鍵認証・NOPASSWD sudo 設定済み）。

```bash
ssh rp1
```

| 項目 | 値 |
|---|---|
| SSH エイリアス | `rp1` |
| Hostname | `RaspberryPi.local`（mDNS。解決不可時は `ssh kazuaki@192.168.1.12`） |
| LAN IP（参考） | `192.168.1.12` |
| ユーザー / Arch | `kazuaki` / `aarch64` |
| sudo | NOPASSWD（`sudo systemctl …` 可） |

**`rp1` のディレクトリ構成**

| パス | 役割 |
|---|---|
| `~/.local/bin/rustyclaw` | 実行バイナリ（デプロイ先） |
| `~/.rustyclaw` → `~/Projects/RustyClaw/production`（symlink） | 本番ルート（NAS 共有。開発機の `production/` と同一） |
| `~/.rustyclaw/config/config.json`, `vault.enc` | 設定とシークレット vault |
| `~/.rustyclaw/workspace/` | `*.md` 人格定義・`cron.json`・`memory/`・`memory.db` |
| `~/.rustyclaw/logs/` | アプリログ |

> バイナリは symlink 先（共有 `production/`）と分離するため `~/.local/bin/` に置く。

### 4-2. aarch64 クロスビルド

Docker は不要。`cargo` にクロスリンカーを指定する。前提（開発機に一度だけ）: `rustup target add aarch64-unknown-linux-gnu`、`aarch64-linux-gnu-gcc`（`gcc-aarch64-linux-gnu`）、`.cargo/config.toml` のリンカー設定。

```bash
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  cargo build --release --target aarch64-unknown-linux-gnu -p rustyclaw-cli
# 成果物: target/aarch64-unknown-linux-gnu/release/rustyclaw-cli
```

### 4-3. デプロイ

**自動（推奨）** — `scripts/deploy.sh` が x64/aarch64 ビルド → `production/bin/` へリネーム配置 → `rp1` へ配置 → サービス再起動まで実行する。

```bash
./scripts/deploy.sh
```

> **config profile**: `production/config/config.json` は追跡対象外の symlink。本番は release、デバッグは debug を指すよう運用側で設定する（NAS 共有のため rp1 にも反映される）。
> - 本番: `cd production/config && ln -sfn config.release.json config.json`
> - デバッグ: `cd production/config && ln -sfn config.debug.json config.json`
> 前提パッケージ: rp1 に `sqlite3`（state DB 点検用）、`gws`（Google Workspace CLI）。LM Studio 利用時は埋め込みモデル（nomic-embed-text）を常駐ロードしておくと `memory_search` 初回が速い。

**手動** — 稼働中バイナリは置換不可（`ETXTBSY`）。別名転送 → 停止 → 原子的差し替え → 再起動。

```bash
scp target/aarch64-unknown-linux-gnu/release/rustyclaw-cli rp1:~/.local/bin/rustyclaw.new
ssh rp1 'sudo systemctl stop rustyclaw && \
         mv ~/.local/bin/rustyclaw.new ~/.local/bin/rustyclaw && \
         chmod +x ~/.local/bin/rustyclaw && \
         sudo systemctl start rustyclaw'
ssh rp1 '~/.local/bin/rustyclaw --version'   # 確認
```

### 4-4. サービス管理・ダッシュボード

```bash
ssh rp1 'sudo systemctl status  rustyclaw'
ssh rp1 'sudo systemctl restart rustyclaw'
ssh rp1 'journalctl --user -u rustyclaw -f'   # ログ追尾（または ~/.rustyclaw/logs/）
```

- vault パスフレーズは systemd クレデンシャル（`vault-key`）または環境変数 `VAULT_PASSPHRASE` で供給。
- ダッシュボード: `http://192.168.1.12:8080/`（`MONITOR`/`STATS` の単一 SPA）。

### 4-5. デプロイ前検証（実 API 不要）

`--no-agent` で起動するとプロバイダが Noop になり実 API を送らない。vault/Discord に依存させないよう、検証時は Discord・外部ツールを無効化した一時 config ＋一時 workspace でクリーンに起動できる（本番サービスと同一ポートで同時起動しないこと）。

```bash
rustyclaw --config /tmp/verify/config.json --workspace /tmp/verify/workspace --no-agent gateway
curl -s http://127.0.0.1:8080/api/concurrency
```

---

> [!TIP]
> AI エージェントは、本ルールを常に前提として解釈します。仕様変更を伴う提案や実装を行う際は、この `docs/README.md` を読み込み、対象ファイルを正しく分類・アップデートしてください。
