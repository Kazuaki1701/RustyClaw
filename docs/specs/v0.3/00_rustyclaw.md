# RustyClaw ✕ Hermes Agent 統合システム仕様書

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)
> **バージョン**: v0.4（v0.3 開発スキップ・v0.4 へ直接移行）
> **最終更新日**: 2026-06-12（Phase 50: §14 HA 統合完了）
> **対象コード**: `crates/` 全クレート最新実装
> **備考**: インデックス + 概要。詳細は §4 以降の各仕様ファイルを参照。§13・§14 が v0.4 実装完了。
> **Upstream 比較**: [`91_upstream_comparison.md`](91_upstream_comparison.md)

**プロジェクト名**: RustyClaw
**更新日**: 2026-06-12

---

## 1. プロジェクト概要

### 1.1 目的

PicoClaw（Go 製 AI エージェントランタイム）の Rust クローンを自作する。
GeminiClaw（TypeScript 製）の優れた設計思想（メモリ管理・Heartbeat システム）と、
Nous Research の Hermes Agent が提唱する**「自己監査ループ」「永続的手続き知識の自動結晶化」「3 層メモリ制約」**を完全融合。
Raspberry Pi 4 向けに、Rust（Tokio async）のマルチクレート・ワークスペース構成で極限まで最適化した、セルフホスト型・自律改善型 AI エージェントランタイム。

### 1.2 参照 Upstream

> 各 Upstream の採用/不採用詳細は [`91_upstream_comparison.md`](91_upstream_comparison.md) を参照。

| プロジェクト | 役割 | 主な取り込み要素 |
|---|---|---|
| PicoClaw (Go) | アーキテクチャの主参照 | Gateway / Pipeline / CronService / Skills |
| GeminiClaw (TypeScript) | メモリ・Heartbeat 設計の参照 | メモリ 3 層・Heartbeat・Session Continuation・会話継続感 6 技法 |
| Hermes Agent (Nous Research) | 自己改善機構の参照 | 自己改善 Skills・自己監査ループ・3 層メモリ制約 |
| context-mode (mksglu) | v0.4 外部 MCP 同居 → v0.5 純 Rust 内製化の参照 | bwrap 実行・SEARCH/REPLACE パッチ・BM25 エピソード記憶 |

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
| [`04_workspace.md`](04_workspace.md) | §6 | `[実装済]` | ワークスペース体系 |
| [`12_storage.md`](12_storage.md) | §11 | `[実装済]` | データ永続化・RAG インデックス |
| [`05_heartbeat.md`](05_heartbeat.md) | §10 | `[実装済]` | Heartbeat システム |
| [`06_hermes_skills.md`](06_hermes_skills.md) | §12 | `[将来拡張]` | Hermes 自己改善 Skills システム |
| [`07_extensions.md`](07_extensions.md) | §13〜17 | `[将来拡張]` | §13（context-mode 委譲）・§14（HA 統合）は v0.4 完了済み。§15（rig-core）完了済み。§16・§17 は将来拡張 |
| [`08_deployment.md`](08_deployment.md) | §16 | 運用ドキュメント | 運用・デプロイ手順 |
| [`09_dashboard.md`](09_dashboard.md) | — | `[実装済]` | Web Dashboard・管理 API |
| [`10_mcp.md`](10_mcp.md) | — | `[実装済 + 将来拡張を含む]` | MCP クライアント（堅牢化・SSE は将来拡張） |
| [`11_operation.md`](11_operation.md) | — | `[実装済]` | 稼働点検ガイド（クイック/詳細点検・週次チェックリスト） |
| （本ファイル §13） | §13 | `[実装済 — v0.4]` | 外部 MCP サーバー統合（context-mode 同居）＆ 内製コード極小化 |
| （本ファイル §14） | §14 | `[将来拡張 v0.5]` | 純 Rust 完全内製化 ✕ インプロセス融合 |

---

##  重要設計決定事項（不変ルール）

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



---

## 13. [v0.4 仕様] 外部MCPサーバー統合 ＆ 内製コード極小化仕様

### 13.1 目的と設計方針
本バージョン（v0.4）では、初動の開発スピードと安定性を最優先するため、独立して動作するNode.js（またはBun）版の **`mksglu/context-mode` を外部MCPサーバーとしてラズパイ4に同居（Colocation）**させる [1, 2.1]。
これにより、実装およびデバッグの難易度が極めて高い「ファイルシステム隔離」「SEARCH/REPLACEパッチ当て」「ローカルBM25検索エンジン」のロジック開発を外部へ完全委譲し、自製コードの **約45%〜55%を削減** する。Rust側は「HomeAssistant連携」と「レーン制御」に100%集中する。

### 13.2 物理プロセス配置 ＆ Stdio インターフェース

RustyClaw（Rust）プロセスが主（親）となり、起動時に `tokio::process::Command` を用いて `context-mode`（Node.js）を従（子）としてバックグラウンドでフォーク駆動する。通信はポートを開けず、**標準入出力（stdin/stdout）を介した MCP JSON-RPC 2.0 規格**で高速往復させる。

```
┌──────────────────────────────────────┐
│  rustyclaw-cli / agent (Rust/Tokio)  │ ◄── 親プロセス（常時起動、環境・対話制御）
└──────────────────┬───────────────────┘
                   │
                   ▼ (tokio::process で子プロセスをフォーク)
┌──────────────────────────────────────┐
│    context-mode (Node.js ≥ 22.5)     │ ◄── 独立子プロセス（ストレージカプセル化）
└──────────────────────────────────────┘
```

#### Rust側でのプロセス起動・管理コード設計
```rust
use tokio::process::{Command, Child};
use std::process::Stdio;
use std::path::Path;

pub struct ExternalMcpController {
    pub child_process: Child,
}

impl ExternalMcpController {
    /// 稼働条件に基づき、環境変数とストレージパスを固定して子プロセスを起動する
    pub fn spawn_server(workspace_root: &Path) -> std::io::Result<Self> {
        // セッション・インデックスの保存先をプロジェクト領域内にカプセル化
        let storage_dir = workspace_root.join(".context-mode");
        let storage_str = storage_dir.to_string_lossy().into_owned();

        let child = Command::new("context-mode")
            .env("CONTEXT_MODE_DIR", &storage_str)
            .env("CONTEXT_MODE_PLATFORM", "custom-rustyclaw")
            .stdin(Stdio::piped())   // JSON-RPC 送信用土管
            .stdout(Stdio::piped())  // JSON-RPC 受信用土管
            .stderr(Stdio::inherit()) // サーバー側エラーはラズパイの stderr へ結合
            .spawn()?;

        println!("[v0.4 Infrastructure] External context-mode MCP server co-located successfully.");
        Ok(Self { child_process: child })
    }
}
```

### 13.3 外部委譲される機能と削減効果
*   **安全なコード実行 (`ctx_execute`)**: Rust側での複雑な `bwrap` コマンド組み立て、新規ファイルの一時生成、および symlink 実体の解決（`canonicalize`）コードを **100%削減**。
*   **パッチマージ (`context-mode` 内部置換)**: `SEARCH/REPLACE` の構文解析、アトミックな `.tmp` 置換、完全一致バリデーションの Rust コード（数画行）を **100%削減**。
*   **エピソード記憶 (`ctx_index` / `ctx_search`)**: 検索エンジンのスキーマ定義や、ポーター語幹処理（Stemming）、近接リランキングのチューニングコードを **100%削減**。

### 13.4 稼働条件（RPi4環境の前提）
1.  **Node.js ≥ 22.5（必須）**: 内蔵された `node:sqlite` を自動適用するため、C++ネイティブコンパイルに伴う Linux 上のメモリ管理バグ（`SIGSEGV` クラッシュ）を完全に回避する。
2.  **Lane B の車線規制**: 外部プロセスの呼び出しであっても、`rustyclaw-agent` 側が **`Lane B`（セマフォ制限数1）** の枠内で JSON-RPC を仲介するため、子プロセスがどれだけ重い演算を行ってもラズパイのCPUコアは最大1枚しか占有されず、対話の快適性（Lane A）は完全に死守される。

---

## 14. [v0.5 仕様] 純Rust完全内製化 ✕ インプロセス融合仕様

### 14.1 目的と設計方針
本バージョン（v0.5）は、RustyClawプロジェクトの究極のゴール（アイデンティティ）である。v0.4で外部委譲していた `context-mode` の全ロジックを **純Rust（インプロセス）へ完全移植** する。
これにより、ラズパイ4環境から Node.js ランタイム（常駐メモリ約100MB ＋ プロセス間通信のオーバーヘッド）を100%完全に排除し、**「`cargo build --release` で生成された、たった1つのバイナリを配置するだけで動作する、極限の省メモリ・超高速自律AIエージェントインフラ」** を完成させる。

### 14.2 ワークスペースの進化と関数インターフェース
外部の JSON-RPC 通信層を撤廃し、**`crates/rustyclaw-context-mode`** クレートを新設してインプロセスで結合する。

```
rustyclaw (Workspace Root)
├── rustyclaw-cli (バイナリエントリ)
└── crates/
    ├── rustyclaw-gateway       # 外部接続・イベント・CronService・HAサマリーマクロ
    ├── rustyclaw-agent         # 4ステージ・推論パイプラインコア (Hermes Core Loop)
    ├── rustyclaw-providers     # ステートレスHTTP / FallbackChain
    └── rustyclaw-context-mode  # [★v0.5完全移植] 純Rust製記憶・サンドボックス・パッチ当てコア
```

通信は Stdio のテキストパースから、メモリを直接共有する **Rust の型安全な関数呼び出し（`async fn`）** へと進化し、検索・実行レイテンシがマイクロ秒（μs）オーダーまで短縮される。

### 14.3 主要3大コンポーネントの純Rust実装仕様

#### ① 内製RAG / FTS5検索エンジン (`src/memory/search.rs`)
`rusqlite` の内蔵 FTS5（Porter Stemming 有効化）を用いて、過去ログの「濃縮コピー（構造化チャンク）」を爆速で引き出す。
```rust
pub struct EmbeddedKnowledgeBase { conn: rusqlite::Connection }

impl EmbeddedKnowledgeBase {
    pub fn search_snippets(&self, query: &str, top_n: usize) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context, dialogue FROM episodic_memory 
             WHERE episodic_memory MATCH ?1 ORDER BY bm25(episodic_memory) ASC LIMIT ?2"
        )?;
        // クエリから特殊文字を排除し、Markdownスニペットを動的整形して返す
        todo!("Tantivy または rusqlite(FTS5) によるBM25近接リランキング処理")
    }
}
```

#### ② 内製パッチマージエンジン (`src/patch/merger.rs`)
`SEARCH/REPLACE` ブロックの厳格な文字列バリデーション付き置換ロジック。
```rust
pub struct InProcessPatchMerger;
impl InProcessPatchMerger {
    pub fn apply_procedural_patch(file_path: &std::path::Path, patch_text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let original = std::fs::read_to_string(file_path)?;
        // <<<<<< SEARCH, =====, >>>>>> REPLACE マーカーの厳密インデックス検索と置換
        // 存在しない場合はマージを拒否（Skillファイル破損の完全防止）
        todo!("一時ファイル（.tmp）を生成してからのアトミックリネーム置換")
    }
}
```

#### ③ 5KB制限 ✕ 意図駆動型フィルター付き `bwrap` 実行器 (`src/sandbox/execute.rs`)
出力が5KBを超過した際、出力を内製RAG（FTS5）へ動的に退避させ、ユーザーの「意図」にマッチするスニペットだけをプロンプトへ還流させるサンドボックスの完全内製化。
```rust
pub struct SecureSandboxExecutor { max_raw_output_bytes: usize } // デフォルト 5120
impl SecureSandboxExecutor {
    pub async fn execute_sandboxed_code(&self, script: &str, intent: &str, kb: &EmbeddedKnowledgeBase) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Rust側で実体の絶対パスを解決し、bwrap を安全に Spawn
        // 出力が5KBを超えたら自動でインデックス化し、Smart Snippets のみをプロンプトに返す
        todo!("bwrap の --unshare-net / --tmpfs と FTS5 一時コミットの融合")
    }
}
```

### 14.4 v0.4 から v0.5 へのシームレスな移行戦略
`rustyclaw-context-mode` クレートは、外側（`rustyclaw-agent` の Pipeline）に対しては、v0.4の「外部MCPを叩くツールインターフェース」と**全く同じインターフェース（引数と出力型）を維持**して実装する。
これにより、インフラの実態を「外部Nodeプロセス（v0.4）」から「純Rust内製関数（v0.5）」へ切り替える際も、メインの `AgentLoop` やパイプラインコアのコードを1行も汚すことなく、依存の切り替え（カプセル化）だけで究極の単一バイナリへと進化させることができる。
