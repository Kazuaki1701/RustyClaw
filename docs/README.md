# ドキュメント運用ルール (Project Local)

> [!IMPORTANT]
> **AIコーディングアシスタントへの絶対指示**:
> 本プロジェクトの開発、Gitコミット、プルリクエスト、およびドキュメント管理は、以下の共通開発標準に完全に従う必要があります。
> 
> 実装を開始する前に、必ず最初に対象ファイルを読み込み、そのルール（フォルダ構成、コミット、PR、ADR、アーカイブ手順）を頭に焼き付けてください。
> 
> - **共通ルールファイルのパス**: `/mnt/Projects/RustyClaw/ai-rules.md`
>
> ※上記ルールを確認後、本リポジトリの `docs/task.md` または GitHub Issues のタスクに着手してください。



---

## 1. 物理的なフォルダ構成（役割分離）

ドキュメントは情報の性質に基づき、物理的に以下のフォルダに隔離して管理します。
各フォルダの役割定義・ステータスタグルールは **[ai-rules.md](/mnt/Projects/RustyClaw/ai-rules.md) Section 1** を参照してください。

```
docs/
├── specs/                     # 【常に最新】コードの最新実装と100%一致させる「基本仕様書」
│   ├── 00_rustyclaw.md        # 総合システム仕様書 ＆ ドキュメントインデックス
│   ├── 01_architecture.md     # 全体アーキテクチャ・開発環境仕様
│   ├── 02_agent_pipeline.md   # パイプライン・LLMプロバイダ仕様
│   ├── 03_workspace_spec.md   # ワークスペースファイル・ストレージ仕様
│   ├── 04_heartbeat_spec.md   # Heartbeat 自発行動システム仕様
│   ├── 05_gateway_spec.md     # Gateway・並列制御 Lane Queue 仕様
│   ├── 06_dashboard_spec.md   # Web Dashboard・管理用 API 仕様
│   ├── 08_operation_inspection.md  # 稼働点検ガイド（コマンド集・既知パターン）
│   ├── 10_git_collaboration_rules.md # GitHub 共同開発・運用ガイドライン仕様
│   ├── 12_weather_yolp_spec.md # YOLP気象情報APIリファレンス雨雲レーダー仕様
│   ├── 13_rag_file_operations.md # 実運用 RAG 運用マークダウン仕様
│   ├── 11_skills_spec.md      # Skills システム仕様・GeminiClaw 比較・移植記録
│   ├── 81_llm_provider_model_selection.md # プロバイダー・モデル選定指針仕様
│   ├── 91_geminiclaw_comparison.md # GeminiClaw との機能・コード比較移植仕様
│   └── 92_picoclaw_comparison.md  # PicoClaw とのアーキテクチャ・機能比較仕様
│
├── plans/                     # 【開発中】実装前または現在進行中の「個別実装計画書」
│   └── YYYY-MM-DD-<機能名>.md  # 現在進行中の計画書（ファイル名は Phase に応じて変動）
│
├── review/                    # 【点検・レビュー】コードレビュー、ログ点検、実行検証記録（現在は空）
│
├── adr/                       # 【永続保存】アーキテクチャ意思決定記録（Architecture Decision Records）
│   └── 001-xxx.md             # 連番ファイル。SUPERSEDED になっても削除しない
│
├── archive/                   # 【参照のみ・編集不可】すでに完了した「過去の計画書・報告書・タスクリスト」
│   ├── plans/                 # 過去の実装計画書（Historical Plans）
│   ├── review/                # 過去のログ検証・レビューレポート（Historical Reviews）
│   └── tasks/                 # 過去の完了済みタスクリスト（Historical Tasks）
│
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
   * **実装が完了した瞬間**、その計画書は `docs/archive/plans/` フォルダに移動し、ヘッダーを `[HISTORICAL]` に書き換えてください。そして、有効な仕様のみを `docs/specs/` に反映（同期）させてください。
4. **タスクリストのアーカイブ化とマスタインデックス更新**:
   * `task.md` のアクティブタスクや ISSUE が完了した際、完了した項目を `docs/archive/tasks/YYYY-MM-DD-completed-<対象フェーズまたはISSUE>.md` に切り出して新規アーカイブファイルを作成し、ヘッダーに `[HISTORICAL]` を付与します。
   * 同時に、`docs/archive/tasks/README.md` (マスタインデックス) の履歴テーブルの先頭（降順）に、完了日・完了対象・アーカイブファイル名（相対パス）を追加して更新します。
   * アクティブな `task.md` 自体はスリムに保ち、完了した項目は完全に消去してください。

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

## 5. コーディング規約 (Rust)

実装時に遵守すべき共通規約です。

*   **エラーハンドリング**:
    *   `unwrap()` や `expect()` による強制パニックは原則避け、エラー（`Result`）は `?` を使って適切に呼び出し元へ伝播させてください。
    *   アプリケーションのエントリポイントやテストコードでは `anyhow::Result` を使用し、ライブラリクレート（`crates/` 配下）ではドメイン固有エラーの定義に `thiserror` の使用を検討してください。
*   **ログ出力**:
    *   標準出力 (`println!`, `eprintln!`) は原則として使用せず、`tracing` クレート (`tracing::info!`, `tracing::warn!`, `tracing::error!`) を用いた構造化ログを出力してください。
*   **依存関係の制約 (OpenSSL 排除)**:
    *   Raspberry Pi 4 への安全なクロスコンパイル環境を維持するため、新しく HTTP クライアントなどの外部依存を追加する際は、必ず OpenSSL 依存を排除し、`reqwest` に `rustls-tls` フィーチャーを適用（かつ `default-features = false`）してください。

---

## 6. Git・ブランチ運用ルール (Git & Branching Rules)

AI エージェントは、本リポジトリでの変更管理において以下の Git 運用ルールを厳格に遵守してください。

*   **`main` ブランチへの直接コミット禁止 (No Direct Commits to `main`)**:
    *   `main` ブランチ上で直接コミットを作成したり、直接履歴変更を行ったりしないでください。
*   **要件ごとのトピックブランチ化の徹底 (Mandatory Topic Branches)**:
    *   機能実装 (`feat/`)、バグ修正 (`fix/`)、ドキュメント更新 (`docs/`)、リファクタリング (`refactor/`) など、すべての要件について必ず専用のトピックブランチ（例: `feat/xxx`）を切って変更を行ってください。
*   **マージコミットによる統合の義務 (Merge Commit Integration)**:
    *   `main` ブランチへの変更のマージは、トピックの境界と履歴の整合性を視覚的に明確にするため、必ず `--no-ff` (No Fast-Forward) オプションを使用してください。
    *   マージコマンドの例: `git merge --no-ff <branch_name> -m "Merge branch '<branch_name>' into main"`
*   **マージ後のブランチクリーンアップ (Cleanup Branch)**:
    *   `main` へのマージ完了後、不要となったトピックブランチはローカルから直ちに削除してください。

---

## 7. Git・ドキュメント管理・ADR 運用ルール

Git コミット・ドキュメント管理・ADR の運用ルールは共通ルールファイルに集約されています。実装・コミット・ADR 起票の前に必ず参照してください。

> 📄 **[ai-rules.md](/mnt/Projects/RustyClaw/ai-rules.md)**

**このプロジェクトの案件管理番号の形式**（ai-rules.md Step 2 で使用）:
- 機能実装: `Phase XX-Y`（例: `Phase 37-1`）
- バグ修正・保留課題: `ISSUE-XX`（例: `ISSUE-22`）

**コミット例**:
- `feat(gateway): Phase 37-1 Autonomy Level システムの導入`
- `fix(agent): Phase 28b-4 Groq のトークンオーバーフロー対策`
- `docs(plans): Phase 37-1 計画書チェックリストを完了に更新`

---

> [!TIP]
> AI エージェントは、本ルールを常に前提として解釈します。仕様変更を伴う提案や実装を行う際は、この `docs/README.md` を読み込み、対象ファイルを正しく分類・アップデートしてください。
