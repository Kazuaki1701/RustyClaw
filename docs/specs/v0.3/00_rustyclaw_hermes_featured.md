# RustyClaw ✕ Hermes Agent 統合システム仕様書

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **対象コード**: `crates/` 全クレート最新実装
> **備考**: インデックス + 概要。詳細は §4 以降の各仕様ファイルを参照。
> **Upstream 比較**: [`00.1_upstream_comparison.md`](00.1_upstream_comparison.md)

**プロジェクト名**: RustyClaw
**更新日**: 2026-06-11

---

## 1. プロジェクト概要

### 1.1 目的

PicoClaw（Go 製 AI エージェントランタイム）の Rust クローンを自作する。
GeminiClaw（TypeScript 製）の優れた設計思想（メモリ管理・Heartbeat システム）と、
Nous Research の Hermes Agent が提唱する**「自己監査ループ」「永続的手続き知識の自動結晶化」「3 層メモリ制約」**を完全融合。
Raspberry Pi 4 向けに、Rust（Tokio async）のマルチクレート・ワークスペース構成で極限まで最適化した、セルフホスト型・自律改善型 AI エージェントランタイム。

### 1.2 参照 Upstream

> 各 Upstream の採用/不採用詳細は [`00.1_upstream_comparison.md`](00.1_upstream_comparison.md) を参照。

| プロジェクト | 役割 | 主な取り込み要素 |
|---|---|---|
| PicoClaw (Go) | アーキテクチャの主参照 | Gateway / Pipeline / CronService / Skills |
| GeminiClaw (TypeScript) | メモリ・Heartbeat 設計の参照 | メモリ 3 層・Heartbeat・Session Continuation・会話継続感 6 技法 |
| Hermes Agent (Nous Research) | 自己改善機構の参照 | 自己改善 Skills・自己監査ループ・3 層メモリ制約 |

### 1.3 実行環境

| 項目 | 値 |
|---|---|
| ハードウェア | Raspberry Pi 4 Model B (RAM 8GB) |
| ストレージ | USB SSD 接続（SD カード非推奨、I/O 速度および寿命対策） |
| OS | Raspberry Pi OS Lite (headless, aarch64 / ARMv8 Cortex-A72) |
| Rust ターゲット | `aarch64-unknown-linux-gnu` |
| 制約 | OpenSSL 依存禁止（`rustls` 統一）、クロスコンパイル対応必須 |

**USB SSD 運用留意点**

| 項目 | 対処 |
|---|---|
| UASP 相性 | `dmesg` でエラー確認。問題時のみ `usb-storage.quirks=XXXX:YYYY:u` |
| fstrim | `systemctl enable fstrim.timer`（週次） |
| noatime | `/etc/fstab` に `noatime` オプション追加 |
| 電源断 | atomic write 実装で対処（SQLite WAL + `tempfile → rename` パターン） |

---

## 2. Cargo Workspace 構成

### 2.1 クレート構成

```
rustyclaw/
├── Cargo.toml                   # workspace root
├── crates/
│   ├── rustyclaw-cli/           # binary: main エントリポイント
│   ├── rustyclaw-gateway/       # lib: 起動・オーケストレーション・スケジュール
│   ├── rustyclaw-agent/         # lib: Pipeline・AgentLoop・AgentInstance
│   ├── rustyclaw-providers/     # lib: LLM HTTP クライアント群
│   ├── rustyclaw-channels/      # lib: Telegram・Discord 等のコネクタ
│   ├── rustyclaw-tools/         # lib: built-in tools・MCP クライアント
│   ├── rustyclaw-config/        # lib: 設定ファイル型定義・migration
│   └── rustyclaw-storage/       # lib: SQLite・JSONL セッション永続化
└── workspace/                   # デフォルトワークスペース（開発用）
```

### 2.2 依存クレート

| 用途 | クレート | ステータス |
|---|---|---|
| 非同期ランタイム | `tokio` (full, multi-thread) | `[実装済]` |
| HTTP クライアント | `reqwest` + `rustls-tls` | `[実装済]` |
| SSE ストリーミング | `reqwest` bytes_stream | `[実装済]` |
| シリアライズ | `serde` + `serde_json` | `[実装済]` |
| CLI | `clap` (derive) | `[実装済]` |
| エラー | `anyhow` + `thiserror` | `[実装済]` |
| ログ | `tracing` + `tracing-appender` | `[実装済]` |
| SQLite | `rusqlite` + `deadpool-sqlite` | `[実装済]` |
| async trait | `async-trait` | `[実装済]` |
| 全文検索 | `tantivy` (純 Rust BM25) | `[実装済]` |
| MCP クライアント | `rmcp` | `[実装済]` |
| atomic write | `tempfile` | `[実装済]` |
| systemd watchdog | `sd-notify` | `[実装済]` |
| 設定暗号化 | `age` | `[実装済]` |
| キャンセル | `tokio-util` (CancellationToken) | `[実装済]` |
| 日時 | `chrono` | `[実装済]` |
| Web UI | `axum` (0.7, http1 + json) | `[実装済]` |
| 乱数 | `rand` | `[実装済]` |
| LLM 抽象フレームワーク | `rig-core` | `[将来拡張]` |
| ローカル Embedding | `rig-fastembed` + `fastembed` (onnxruntime) | `[将来拡張]` |

### 2.3 Cargo.toml プロファイル設定

```toml
[profile.release]
opt-level     = 3           # 速度優先（8GB あるのでサイズ不問）
lto           = "thin"      # コンパイル時間と最適化のバランス
codegen-units = 4           # RPi4 の 4 コアに合わせる
strip         = "debuginfo" # パニック時スタックトレースは残す
panic         = "unwind"    # anyhow のエラー伝播に必要

[target.aarch64-unknown-linux-gnu]
rustflags = ["-C", "target-cpu=cortex-a72"]  # NEON SIMD 有効化
```

### 2.4 クロスコンパイル設定

```toml
# reqwest は必ず rustls-tls feature を指定（OpenSSL 排除）
[dependencies]
reqwest = { version = "0.12", default-features = false,
            features = ["rustls-tls", "stream", "json"] }
```

```bash
# ツールチェーン（開発機に一度だけ）
rustup target add aarch64-unknown-linux-gnu
# aarch64-linux-gnu-gcc（gcc-aarch64-linux-gnu）と .cargo/config.toml のリンカー設定も必要

CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  cargo build --release --target aarch64-unknown-linux-gnu -p rustyclaw-cli
```

---

## 3. アーキテクチャ全体図

```
rustyclaw-cli (binary)
    ↓ コマンドディスパッチ: onboard / agent / gateway / cron / skills
rustyclaw-gateway
    ├── MessageBus (tokio mpsc + broadcast)
    ├── AgentLoop → LaneRegistry
    ├── ChannelManager (trait Channel)
    ├── HeartbeatService (HEARTBEAT.md ベース、GeminiClaw 原版)
    ├── CronService (内製スケジューラー)
    ├── WatchdogService (systemd watchdog)
    ├── HealthServer (HTTP /health /ready /reload)
    └── WebDashboard (HTTP /monitor /stats)
        ↓
rustyclaw-agent (Pipeline)
    ├── ContextBuilder
    │   ├── SystemContext (SOUL.md + AGENTS.md + MEMORY.md + USER.md)
    │   ├── ConversationHistory (Vec<Message> 蓄積)
    │   ├── SessionContinuation (日またぎ文脈)
    │   └── ProactivePosts 注入
    ├── CallLLM (FallbackChain + streaming SSE)
    ├── ExecuteTools (ToolRegistry in-process)  [将来拡張: bwrap 隔離]
    └── PublishResponse
        ↓
rustyclaw-providers (LlmProvider trait)
    ├── OpenAiCompatProvider (reqwest + SSE)
    ├── AnthropicProvider
    ├── GeminiProvider
    └── OllamaProvider (ローカル LLM)
    [将来拡張] └── rig-core ベース実装へ移行
        ↓
rustyclaw-storage
    ├── SessionStore (JSONL append-only, fail-closed)
    ├── MemoryStore (MEMORY.md + logs/ + summaries/)
    ├── SearchIndex (tantivy BM25)  [将来拡張: rig-fastembed Embedding 追加]
    └── SqliteStore
        ├── usage テーブル（トークン使用量）
        ├── patrol_state テーブル（heartbeat-state.json 相当）
        └── seen_items テーブル（Interest Patrol 既読管理）
```

---

## 詳細仕様ファイル

| ファイル | 対応節 | ステータス | 内容 |
|---|---|---|---|
| [`01_pipeline.md`](01_pipeline.md) | §4, §7 | `[実装済]` | 4ステージパイプライン・Lane Control |
| [`02_memory.md`](02_memory.md) | §5, §9 | `[実装済 + 将来拡張を含む]` | メモリ管理・会話継続感 6 技法 |
| [`03_llm_provider.md`](03_llm_provider.md) | §8 | `[実装済]` | LlmProvider 設計（rig-core 移行は将来拡張） |
| [`04_workspace_storage.md`](04_workspace_storage.md) | §6, §11 | `[実装済]` | ワークスペース体系・Storage 設計 |
| [`05_heartbeat.md`](05_heartbeat.md) | §10 | `[実装済]` | Heartbeat システム |
| [`06_hermes_skills.md`](06_hermes_skills.md) | §12 | `[将来拡張]` | Hermes 自己改善 Skills システム |
| [`07_extensions.md`](07_extensions.md) | §13, §14, §15 | `[将来拡張]` | bwrap・HomeAssistant 統合・rig-core 統合 |
| [`08_deployment.md`](08_deployment.md) | §16 | 運用ドキュメント | 運用・デプロイ手順 |
| [`09_dashboard.md`](09_dashboard.md) | — | `[実装済]` | Web Dashboard・管理 API |
| [`10_mcp.md`](10_mcp.md) | — | `[実装済 + 将来拡張を含む]` | MCP クライアント（堅牢化・SSE は将来拡張） |
| [`11_operation.md`](11_operation.md) | — | `[実装済]` | 稼働点検ガイド（クイック/詳細点検・週次チェックリスト） |

---

## 17. 重要設計決定事項（不変ルール）

1. **INTERESTS.md は USER.md に統合**（独立ファイルとしない）
2. **IDENTITY.md は使用しない**（SOUL.md で統合）
3. **PATROL.md は使用しない**（HEARTBEAT.md で統合）
4. **LlmProvider は完全ステートレス HTTP**（子プロセス不要）
5. **Heartbeat 実行は `last_user_interaction_at` を更新しない**
6. **HEARTBEAT.md はエージェントが自己改変しない**
7. **`sessions/*.jsonl` は fail-closed**（書き込み失敗で pipeline 停止）
8. **memory flush は fail-open**（失敗しても続行）
9. **OpenSSL 依存を持ち込まない**（`rustls-tls` で統一）
10. **Lane B のキューは最大 1 件（Heartbeat 積み上がり防止）**
11. **`memory/logs/` と `memory/summaries/` は別ディレクトリで管理**
12. **heartbeat-state.json はエージェントが自己更新し、Rust は SQLite patrol_state で管理**
13. **`self_improved/` Skill への書き込みは AuditorWorker（Lane B）経由のみ**（対話ターンからの直接書き込み禁止）
14. **bwrap 隔離時は `std::fs::canonicalize` で symlink 実体パスを解決してからバインドする**
15. **ONNX モデルインスタンスはプロセス内でキャッシュし、毎リクエスト初期化を禁止**
16. **Lane A・Lane B は各セマフォ limit=1 で厳格分離（RPi4 サーマルプロテクション）**
