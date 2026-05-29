# 01. 全体アーキテクチャ・開発環境仕様

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: 2026-05-28  
> **対象コード**: 全クレートの最新実装

## 1. プロジェクト概要

### 目的
PicoClaw（Go製 AIエージェントランタイム）のRustクローンである「RustyClaw（仮）」を自作します。
GeminiClaw（TypeScript製）の優れた設計思想（メモリ管理・Heartbeatシステム）を取り込んで融合させた独自実装を目指します。

### 実行環境（確定）
本システムは以下のハードウェアおよびOS環境で動作することを前提として最適化します。

| 項目 | 値 |
|---|---|
| ハードウェア | Raspberry Pi 4 Model B |
| RAM | 4GB (実測 3.7GiB) |
| ストレージ | USB SSD（SDカードは使用しない） |
| OS | Raspberry Pi OS Lite (headless) |
| アーキテクチャ | aarch64 (ARMv8 Cortex-A72) |
| Rust ターゲット | `aarch64-unknown-linux-gnu` |

---

## 2. 全体アーキテクチャ構造

```
rustyclaw-cli (binary)
    ↓ コマンドディスパッチ: onboard / agent / gateway / cron / skills
rustyclaw-gateway (lib)
    ├── MessageBus (tokio mpsc + broadcast)
    ├── AgentLoop → LaneRegistry
    ├── ChannelManager (trait Channel)
    ├── HeartbeatService (HEARTBEAT.md ベース、GeminiClaw 原版)
    ├── CronService (内製スケジューラー)
    ├── WatchdogService (systemd watchdog)
    └── HealthServer (HTTP /health /ready /reload)
        ↓
rustyclaw-agent (lib) (Pipeline)
    ├── ContextBuilder
    │   ├── SystemContext (SOUL.md + AGENTS.md + MEMORY.md + USER.md)
    │   ├── ConversationHistory (Vec<Message> 蓄積)
    │   ├── SessionContinuation (日またぎ文脈)
    │   └── ProactivePosts 注入
    ├── CallLLM (FallbackChain + streaming SSE)
    ├── ExecuteTools (ToolRegistry in-process + MCP プロキシ)
    │   ├── rustyclaw-tools (Tool トレイト・ToolRegistry)
    │   └── rustyclaw-mcp (McpManager → 外部 MCP サーバー stdio 接続)
    └── PublishResponse
        ↓
rustyclaw-providers (lib) (LlmProvider trait)
    ├── OpenAiCompatProvider (reqwest + SSE、ToolCall 対応)
    └── GmnCliProvider (gmn CLI サブプロセス経由、ToolCall 非対応)
        ↓
rustyclaw-storage (lib)
    ├── SessionStore (JSONL append-only, fail-closed)
    ├── MemoryStore (MEMORY.md + logs/ + summaries/)
    ├── SearchIndex (tantivy BM25)
    └── SqliteStore
        ├── usage テーブル（トークン使用量）
        ├── patrol_state テーブル（heartbeat-state.json の Rust 管理）
        └── seen_items テーブル（Interest Patrol 既読管理）
```

---

## 3. Cargo Workspace 構成

プロジェクトは複数の機能ごとにクレートを分割した Cargo Workspace 構成を採用します。

```
rustyclaw/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── rustyclaw-cli/          # binary: main エントリポイント
│   ├── rustyclaw-gateway/      # lib: 起動・オーケストレーション
│   ├── rustyclaw-agent/        # lib: Pipeline・AgentLoop・AgentInstance
│   ├── rustyclaw-providers/    # lib: LLM HTTP クライアント群
│   ├── rustyclaw-channels/     # lib: Telegram・Discord 等の実装
│   ├── rustyclaw-tools/        # lib: Tool トレイト・ToolRegistry 定義
│   ├── rustyclaw-mcp/          # lib: MCP クライアント（JSON-RPC 2.0 over stdio）
│   ├── rustyclaw-config/       # lib: 設定ファイル型定義・migration
│   └── rustyclaw-storage/      # lib: SQLite・JSONL セッション永続化
└── workspace/                  # デフォルトワークスペース（開発用）
```

---

## 4. 依存クレート一覧

クロスコンパイル環境への適合、およびフットプリントの適正化のため、以下のクレートを標準依存として採用します。

| 用途 | クレート | 備考 |
|---|---|---|
| 非同期ランタイム | `tokio` (full, multi-thread) | 4GB RAM・4コアで十分効率的に動作可能 |
| HTTP クライアント | `reqwest` + `rustls-tls` | **OpenSSL 依存を排除（必須）** |
| SSE ストリーミング | `reqwest` bytes_stream | 手動パース |
| シリアライズ | `serde` + `serde_json` | 標準 |
| CLI | `clap` (derive) | サブコマンド型安全 |
| エラー処理 | `anyhow` + `thiserror` | アプリ境界 / ライブラリで使い分け |
| ログ・トレース | `tracing` + `tracing-appender` | rolling file、SSD 直書き |
| SQLite | `rusqlite` + `deadpool-sqlite` | 接続プール、WAL モード |
| 非同期トレイト | `async-trait` | trait に async fn を定義 |
| 全文検索 | `tantivy` | 純 Rust BM25、外部プロセス不要 |
| MCP クライアント | 自前実装（`rustyclaw-mcp`） | JSON-RPC 2.0 over stdio（`rmcp` クレートは不採用） |
| 安全な書き込み | `tempfile` | 原子性書き込み（電源断対策） |
| systemd連携 | `sd-notify` | WatchdogSec 連携用 |
| 設定暗号化 | `age` | `.security.yml` 暗号化 |
| ライフサイクル管理 | `tokio-util` (CancellationToken) | 処理キャンセル用 |
| 日時操作 | `chrono` | タイムゾーン付き日時管理 |

---

## 5. ビルド・プロファイル設定 (`Cargo.toml`)

Raspberry Pi 4 (Cortex-A72) での最適化および安全性のバランスを取るため、リリースビルド時に以下のプロファイルを指定します。

```toml
[profile.release]
opt-level     = 3           # 速度優先（パフォーマンス重視）
lto           = "thin"      # コンパイル時間と最適化のバランス
codegen-units = 4           # RPi4 の 4 コアに合わせる
strip         = "debuginfo" # パニック時スタックトレースは残す
panic         = "unwind"    # anyhow のエラー伝播に必要

[target.aarch64-unknown-linux-gnu]
rustflags = ["-C", "target-cpu=cortex-a72"]  # NEON SIMD 有効化
```

---

## 6. クロスコンパイル設定

開発環境（x86_64）から Raspberry Pi 4（aarch64）へビルドするための手順と制約事項です。

### ツールチェーン
`cross` コマンドを使用してコンパイルします。
```bash
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu
```

### ネットワークライブラリ制約
ターゲット環境での動的リンクや依存エラー（OpenSSL関連等）を防ぐため、`reqwest` には必ず `rustls-tls` フィーチャーを指定して **OpenSSL を完全に排除** します。

```toml
[dependencies]
reqwest = { version = "0.12", default-features = false,
            features = ["rustls-tls", "stream", "json"] }
```

---

## 7. USB SSD 運用上の留意点

Raspberry Pi 4 に接続する USB SSD の性能低下やトラブルを防止するための推奨設定です。

| 項目 | 対策 |
|---|---|
| UASP 相性確認 | `dmesg` でエラー確認。問題発生時のみ `/boot/cmdline.txt` に `usb-storage.quirks=XXXX:YYYY:u` を適用。 |
| fstrim (Trim) | 週次実行するため `systemctl enable fstrim.timer` を実行。 |
| noatime オプション | `/etc/fstab` に `noatime` を追加し、マウント時のファイル読み込みによる不要な書き込み（アクセス時刻更新）を防止。 |
| 電源断対策 | 突然の電源断でも破損を防ぐため、SQLiteのWALモード指定およびファイルの書き込み時の `atomic write` (一時ファイル作成 → rename) を必須とします。 |

---

## 8. Raspberry Pi 4 向け軽量化・外部プロセス排除決定事項 (2026-05-29 追加)

Raspberry Pi 4 (RAM 4GB) の制限されたリソース環境下で長期安定稼働を実現するため、以下の軽量化設計を採用し、外部プロセス (Node.js/Python) 依存を排除します。

### ① Google Workspace 連携の軽量化 (`gws`)
- **決定事項**: 従来の Node.js ベースの MCP サーバー (`gmail`, `calendar` などの複数常駐プロセス) を廃止。
- **代替案**: Go製シングルバイナリの `googleworkspace/cli` (`gws mcp`) に一本化。
- **効果**: 複数プロセス常駐による 200MB〜400MB のメモリ消費を、十数MB程度の Go 常駐プロセス1本に集約し、起動時負荷をゼロ化。

### ② Karakeep および Obsidian のインプロセス・Rust ネイティブ化
- **決定事項**: `npx` 経由の Karakeep MCP、および `uvx (Python)` 経由の Obsidian MCP の常駐プロセスを全廃。
- **代替案**: Obsidian Local REST API および Karakeep API は、どちらも単純な HTTP エンドポイントであるため、`rustyclaw-tools` クレート内に **Rust ネイティブツール (`reqwest` インプロセス通信)** として直接実装。
- **効果**: 外部常駐プロセスを 100% 排除し、ツール非実行時のメモリ消費をゼロ化。

### ③ 知識ベースの高度 RAG 拡張ロードマップ
- **方針**: 将来的に Obsidian Vault 内の高度な意味（セマンティック）検索を行う場合、Node.js ベースの RAG を避け、Rust 製の超軽量・ローカル Markdown 検索エンジン `stn/rqmd` を `rustyclaw-tools` の CLI 連携として追加する。

