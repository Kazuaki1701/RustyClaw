# RustyClaw ✕ Hermes Agent 統合システム仕様書

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **対象コード**: `crates/` 全クレート最新実装
> **備考**: `docs/specs/v0.2/00_rustyclaw.md` の章立てに準拠。Hermes Agent 拡張を統合した完全仕様。
> **Upstream 比較**: [`00_upstream_comparison.md`](00_upstream_comparison.md)

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

> 各 Upstream の採用/不採用詳細は [`00_upstream_comparison.md`](00_upstream_comparison.md) を参照。

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

## 4. 4ステージ・パイプライン `[実装済]`

エージェントの 1 思考ターンは以下の 4 ステージを厳格に経由する。

| ステージ | 役割 |
|---|---|
| **ContextBuilder** | システムコンテキスト（人格 4 ファイル）+ 会話履歴 + RAG 記憶をブレンドし、プロンプトを構築 |
| **CallLLM** | `FallbackChain` と SSE ストリーミングによるステートレス LLM 呼び出し |
| **ExecuteTools** | インプロセス ToolRegistry を通じたツール実行。`[将来拡張]` bwrap 隔離空間対応 |
| **PublishResponse** | Discord / Telegram / LINE / Web ダッシュボードへの応答分配 |

### 4.1 LoggableTool による透過ログキャプチャ `[将来拡張]`

rig-core の Tool トレイトを透過ラッパーで拡張。ツール実行のたびに「ツール名」「引数 (JSON)」「出力（エラー含む）」「実行時間」を MessageBus へブロードキャストし、バックグラウンドの AuditorWorker（Lane B）へ蓄積する。Hermes 自己改善 Skills システム（§12）のデータソースとなる。

---

## 5. メモリ管理

### 5.1 3層メモリ設計

| レイヤー | 役割 | 格納場所・制限 | 動作特性 |
|---|---|---|---|
| **Layer 1** 永続的事実 | 人格・ユーザー嗜好・現在のプロジェクト事実 | `SOUL.md`・`MEMORY.md` (5KB 以内)・`USER.md` | 毎ターン system プロンプトへ直接注入。予算超過時は LLM が自律要約（GC）。`[実装済]` |
| **Layer 2** 手続き的スキル | 複雑タスクの再現手順・コマンド・検証チェックリスト | `workspace/skills/*.md`（PicoClaw 互換階層） | オンデマンドロード。`[将来拡張]` AuditorWorker による自動生成・更新。 |
| **Layer 3** エピソード記憶 | 過去の全セッション履歴・試行錯誤ログ | SQLite + tantivy BM25 / `[将来拡張]` rig-fastembed | `search_past_sessions` ツールを LLM が動的実行して過去ログを回収。`[実装済]` |

### 5.2 Sliding Window ローテーション `[実装済]`

1. 会話バッファが予算（設定可能）を超過したとき、最も古い「User 発言 ✕ Agent 応答」の 1 ペアをバッファ先頭からポップする。
2. ポップしたペアは消去前に Markdown チャンクへ整形し、バックグラウンドの MemoryWorker（Lane B）へ非同期コミット（RAG への退避）。
3. GeminiClaw の `truncateWithContext`（先頭 40%・末尾 40% 保持、中間を `[N messages omitted]` で置換）と同等の実装。

### 5.3 70/20/10 コンテキスト戦略 `[検討中]`

コンテキストウィンドウを「対話バッファ(70%)」「動的文脈(20%)」「人格(10%)」に厳格に切り分け、各枠に予算上限を設ける方式。
現状は Sliding Window ローテーションで代替。将来的に rig-core 統合（§15）と合わせて実装を検討。

---

## 6. ワークスペースファイル体系 `[実装済]`

### 6.1 ディレクトリ構造

```
~/.rustyclaw/workspace/
├── SOUL.md           # アイデンティティ・価値観・人格の核
├── AGENTS.md         # 行動ルール・Heartbeat 応答規約・Tool 使用指針
├── MEMORY.md         # 長期記憶の核（5KB 以内厳守）
├── USER.md           # ユーザープロファイル（Interests セクション含む）
├── HEARTBEAT.md      # Heartbeat 振る舞い指示書（自己改変禁止）
├── memory/
│   ├── heartbeat-state.json   # 各チェックの最終実行時刻
│   ├── heartbeat-digest.md    # 前回 Heartbeat 以降のセッション差分ダイジェスト
│   ├── logs/
│   │   └── YYYY-MM-DD.md     # 日次活動ログ（Obsidian 互換 YAML frontmatter）
│   └── summaries/
│       └── YYYY-MM-DD-{slug}.md  # セッションサマリー（Session Continuation に使用）
├── sessions/
│   ├── discord-C{id}-YYYYMMDD.jsonl
│   ├── cron:heartbeat.jsonl
│   ├── cron:flush.jsonl
│   ├── cron:daily-summary.jsonl
│   └── session-titles.json
└── skills/
    ├── standard/              # 人間が記述する静的 Skill  [実装済]
    │   ├── home_assistant.md  # HA デバイス操作プロンプト  [将来拡張]
    │   └── secure_bash.md     # bwrap 実行基本プロンプト   [将来拡張]
    └── self_improved/         # エージェントが自律生成する動的 Skill  [将来拡張]
        └── *.md
```

```
~/.rustyclaw/
├── config/
│   ├── config.json              # 設定ファイル（追跡対象外の symlink）
│   ├── config.local-llm.json    # ローカル LLM 主力構成
│   └── config.cloud-llm.json   # クラウド LLM 主力構成
├── workspace/                   # 上記のワークスペース
└── memory.db                    # SQLite WAL モード
    # usage テーブル・patrol_state テーブル・seen_items テーブル
```

### 6.2 ファイル書き込み責任マトリクス

| ファイル | ユーザー編集 | エージェント自発 | system 自動 | 自己改変禁止 |
|---|---|---|---|---|
| `SOUL.md` | ✓ | ✓（変更時はユーザーに報告） | — | — |
| `AGENTS.md` | ✓ | — | — | 実質禁止 |
| `MEMORY.md` | ✓ | ✓（重要発見時に即時） | ✓（session 後 flush） | — |
| `USER.md` | ✓ | ✓（新情報を学んだとき） | — | — |
| `HEARTBEAT.md` | ✓ | — | — | **禁止** |
| `heartbeat-state.json` | — | ✓（Heartbeat 後に更新） | — | — |
| `heartbeat-digest.md` | — | — | ✓（pre-run 自動生成） | — |
| `logs/YYYY-MM-DD.md` | — | ✓（任意） | ✓（flush） | — |
| `summaries/*.md` | — | — | ✓（on-idle） | — |
| `sessions/*.jsonl` | — | — | ✓（fail-closed） | — |
| `skills/self_improved/*.md` `[将来拡張]` | — | ✓（AuditorWorker 経由のみ） | — | — |

### 6.3 セッション ID 命名規則

```
discord-C98765432-20260525     # チャンネル会話（日付でローテーション）
cron:heartbeat                 # Heartbeat 実行（毎回新規セッション）
cron:flush                     # Memory flush
cron:daily-summary             # 日次サマリー
```

---

## 7. Lane Control `[実装済]`

### 7.1 思想と目的

RPi4 の 4 コア（Cortex-A72）において、マルチチャンネルからの同時リクエスト・HA イベントスパイク・定時 Cron が衝突しても**「ユーザーへの対話レスポンスを最高位で保護し、CPU ハング・熱暴走を完全に回避する」**ためのリソース分配インフラ。

### 7.2 レーン定義と厳格な分離

```
[ ユーザー発言 / センサー値変化 / 定期 Cron ]
│
▼
┌──────────────────────────────┐
│   MessageBus による交通整理  │
└──────┬────────────────┬──────┘
       │                │
       ▼                ▼
┌──────────────┐  ┌────────────────────────────────┐
│    Lane A    │  │             Lane B             │
│ (対話・応答) │  │ (記憶・Embedding・監査・バッチ) │
└──────┬───────┘  └────────────────┬───────────────┘
       │                           │
       ▼ 【Semaphore limit: 1】    ▼ 【Semaphore limit: 1 / 待ち行列】
  [即時非同期駆動]          [tokio::task::spawn_blocking]
                                   │
                                   ▼ (連続処理時)
                             [200ms 息抜きスリープ]
```

- **Lane A (Interactive Lane)**: セマフォ `limit=1`。ユーザーとのストリーミング対話および `Publish` 専用。対話レイテンシを最高位で保護。
- **Lane B (Background Lane)**: セマフォ `limit=1`。ローカル Embedding・LLM 自己監査・Memory Flush・夜間バッチ専用。`tokio::task::spawn_blocking` で Tokio スレッドプールから完全分離。

### 7.3 待ち行列処理メカニズム

1. **チャネルによる非同期化**: `Publish` 完了後の会話ログは `tokio::sync::mpsc::channel::<MemoryJob>(100)` へ投下。プッシュ自体は数マイクロ秒で完了し、Lane A はすぐに次の発言の待機に戻る。
2. **セマフォによる非同期待機**: キューからポップした Job は Lane B セマフォの Permit を得るまで非同期待機（`.acquire_owned().await`）。同時に CPU を動かす重いタスクは常に 1 つに制限。
3. **息抜きスリープ（サーマルプロテクション）**: MemoryWorker が連続タスクを処理するとき、1 タスク完了・セマフォ解放の直前に `tokio::time::sleep(Duration::from_millis(200))` を挿入。1 コアの 100% 長時間占有によるサーマルスロットリングを防ぐ。

### 7.4 Cron 定期予約の調停ルール

| タスク種別 | レーン | 理由 |
|---|---|---|
| Heartbeat（自発対話） | **Lane A** | ユーザーへの自発投稿はインタラクションと同等の最優先 |
| Daily Summary / 知識の剪定 | **Lane B** | 重いメタ処理は行列末尾にシリアライズ |
| Memory Flush・Skill Flush | **Lane B** | Publish 完了後に kick。対話ループはゼロレイテンシで解放 |
| HA データポーリング | **Lane B**（10 分 Throttling 後） | スパイク防止のため Gateway 層で間引いてから投入 |

---

## 8. LlmProvider 設計 `[実装済]`

### 8.1 重要な設計原則

**LlmProvider は完全ステートレス。** 毎回の API 呼び出しは「初対面」。
会話が続いている感覚はすべて Rust コード（ConversationHistory）が作り出す。

### 8.2 trait 定義

```rust
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools:    &[ToolDef],
        opts:     &CompletionOptions,
    ) -> Result<LlmResponse>;

    async fn complete_stream(
        &self,
        messages: &[Message],
        tools:    &[ToolDef],
        opts:     &CompletionOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>>;
}

pub struct CompletionOptions {
    pub model:        String,
    pub max_tokens:   u32,
    pub timeout:      Duration,           // デフォルト 15 分
    pub cancel_token: CancellationToken,  // turn キャンセル用
}
```

### 8.3 ファクトリ

```rust
pub fn create_provider(cfg: &ModelConfig) -> Box<dyn LlmProvider> {
    match cfg.protocol.as_str() {
        "openai"    => Box::new(OpenAiCompatProvider::new(cfg)),
        "anthropic" => Box::new(AnthropicProvider::new(cfg)),
        "gemini"    => Box::new(GeminiProvider::new(cfg)),
        "ollama"    => Box::new(OllamaProvider::new(cfg)),
        _           => panic!("unknown provider: {}", cfg.protocol),
    }
}
```

`[将来拡張]` rig-core 統合後は `rig::providers` ベースの実装へ段階移行（§15 参照）。

---

## 9. 会話継続感 6 技法 `[実装済]`

ステートレス API で「会話が続いている」感覚を作るための技法。GeminiClaw のコードから確認した実装パターン。

### ① 会話履歴の蓄積

毎ターン `user_input` と `llm_response` を `ConversationHistory` へ push し、全メッセージを API の `messages[]` に渡す。

### ② コンテキスト圧縮（Sliding Window）

§5.2 参照。80% 閾値超過時に先頭 40% + 末尾 40% 保持、中間を省略。

### ③ Memory Flush（セッション後・非同期・fail-open）

Pipeline 完了後に `tokio::spawn` で切り離す。直近 20 エントリを LLM に渡し `MEMORY.md` + `logs/` の更新を依頼。失敗しても `warn` ログのみ。

### ④ Session Continuation（日またぎ）

翌日の初回ターンのみ発動。前日の summary TL;DR + 直近 5 エントリを system に注入。

### ⑤ Proactive Posts 注入

Heartbeat が自発的に送ったメッセージを「会話履歴外の自分の投稿」として system に注入。「自分が言ったこと」を忘れないための仕組み。

### ⑥ System Prompt 常時注入

毎回の API 呼び出しの system に `SOUL.md` / `AGENTS.md` / `MEMORY.md` / `USER.md`（Interests 含む）を含める。

---

## 10. Heartbeat システム `[実装済]`

### 10.1 ファイルの役割分担

| ファイル / DB | 内容 | 変更者 |
|---|---|---|
| `HEARTBEAT.md` | Heartbeat 振る舞い指示（静的） | ユーザーのみ・**自己改変禁止** |
| `USER.md ## Interests` | 興味領域の定義（準静的） | ユーザー or エージェント |
| `memory/heartbeat-state.json` | 各チェックの最終実行時刻 | エージェント自己更新 |
| SQLite `patrol_state` | heartbeat-state.json の Rust 側管理 | システム自動 |
| SQLite `seen_items` | Interest Patrol 既読管理 | システム自動 |

### 10.2 HEARTBEAT.md 7 Step 構造

| Step | 頻度 | 内容 |
|---|---|---|
| Step 1 | 毎回 | heartbeat-digest.md + summaries/ + logs/ で活動レビュー |
| Step 2 | 数時間ごと | MEMORY.md 整理・USER.md Interests 更新 |
| Step 3 | 毎回 | Calendar / Email チェック（ツールがなければ skip silently） |
| Step 4 | 1 日 2〜3 回 | 天気チェック（4 時間インターバル、なければ skip silently） |
| Step 5 | 毎回 | 8h 以上無通信 → 昼間のみ軽く声掛け（Quiet hours 23:00〜08:00 除外） |
| Step 6 | ローテーション | 未完了タスク・失敗セッション対処・バックグラウンド作業 |
| Step 7 | 毎回 | **必ず HEARTBEAT_OK で応答** → 無音 or 通知配信 |

### 10.3 AGENTS.md における Heartbeat 応答規約

| 重要度 | 処理 | HEARTBEAT_OK? |
|---|---|---|
| **Critical**（緊急メール・直近 deadline・障害） | アラートテキストとして通知 | No |
| **Informational**（新着非緊急・定常カレンダー） | `logs/YYYY-MM-DD.md` にのみ記録 | Yes |
| **Nothing**（所見なし） | — | Yes |

### 10.4 heartbeat-digest.md 生成ルール

GeminiClaw `src/agent/session/heartbeat-digest.ts` の移植。

- **通常**: 前回 Heartbeat 以降の JSONL 差分のみスキャン（incremental）
- **6 回に 1 回**: 24 時間 deep scan
- `cron:heartbeat` セッション自身は除外
- 3000 文字以内に圧縮（最新エントリ優先）
- 各エントリを `[HH:MM] session: prompt → response` 形式に圧縮

### 10.5 重要設計ルール

1. **HEARTBEAT_OK で返したら無音**（ユーザーに通知しない）
2. **Heartbeat 実行は `last_user_interaction_at` を更新しない** → 声掛け判断が自分自身を「アクティブ」と誤判定するのを防ぐ
3. **HEARTBEAT.md はエージェントが絶対に自己改変しない**
4. **巡回済み管理は SQLite seen_items が担う**（ファイルに書かない）
5. **Heartbeat 実行自体は Lane B**。ただし HEARTBEAT_OK ではない Critical 判定時（緊急アラート・自発投稿）は Lane A で配信する

---

## 11. Storage 設計 `[実装済]`

### 11.1 書き込み信頼性の分類

```
System automatic (guaranteed)          Agent-initiated (best-effort)
◄──────────────────────────────────────────────────────────────────►

sessions/*.jsonl  memory.db  MEMORY.md(flush)  summaries/  │  MEMORY.md(agent)  logs/
  fail-closed     fail-closed   fail-open       on-idle     │    voluntary       voluntary
```

- **fail-closed**: 書き込み失敗で pipeline を停止（`sessions/*.jsonl`、`memory.db`）
- **fail-open**: 失敗しても `warn` ログのみで続行（memory flush、summary 生成）
- **on-idle**: アイドル時に実行（daily summary、search index reindex）

### 11.2 SQLite 設定

```rust
conn.execute_batch("
    PRAGMA journal_mode=WAL;
    PRAGMA synchronous=NORMAL;
    PRAGMA cache_size=-32000;  -- 32MB（8GB あるので余裕）
    PRAGMA temp_store=MEMORY;
")?;
```

### 11.3 atomic write（電源断対策）

重要ファイルへの書き込みは必ず tempfile → rename パターン。

```rust
async fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
```

---

## 12. Hermes 自己改善 Skills システム `[将来拡張]`

### 12.1 思想と位置づけ

通常の Skill が外部環境（HA・OS）を操作する道具であるのに対し、本拡張は**「エージェントが自分自身の手続き知識（`workspace/skills/self_improved/`）を直接書き換えるための高位の道具」**として定義する。
PicoClaw 標準仕様の静的 Skill 機構を内包しつつ、Hermes Agent 由来の「動的結晶化」へと独自拡張する。

### 12.2 ディレクトリ分離構造

```
workspace/skills/
├── standard/                  # 人間が記述・固定する静的 Skill
│   ├── home_assistant.md
│   └── secure_bash.md
└── self_improved/             # エージェントが自律生成・修正する動的 Skill
    ├── error_recovery_rpi4.md # ハングアップ復旧手順書（自動生成）
    └── ha_retry_handler.md    # HA 通信瞬断リトライ手順書（自動生成）
```

- **ロードフェーズ（ContextBuilder）**: `standard/` と `self_improved/` の双方から、RAG 経由で現在のタスクに適合する Markdown を Top-N 抽出し、プロンプトへ透過的にブレンド。
- **フラッシュフェーズ（Post-run）**: Publish 完了をトリガーに AuditorWorker（Lane B）が `self_improved/` のみを動的改変。

### 12.3 振り返り監査（Skill Generation）

`LoggableTool` が蓄積した実行ログを元に、AuditorWorker が以下の 3 条件を LLM に自律審査させる。

1. タスクが最終的に成功したか
2. 将来再利用できる汎用的なアプローチか
3. 複数ステップを要した複雑な知識か

条件を満たした場合、`CREATE`（新規）または `UPDATE`（差分更新）を指示する専用 JSON を出力させ、後続の PatchMerger へ渡す。

### 12.4 Search & Replace パッチマージ

LLM にファイル全体の再執筆をさせず、部分的な修正パッチ（`SEARCH/REPLACE` ブロック）を出力させ、Rust 側の `PatchMerger` でアトミックにマージする。

```text
<<<<<< SEARCH
## 陥りがちな罠と対策
- [エラー] APIトークンの期限切れによる401エラー。
======
## 陥りがちな罠と対策
- [エラー] APIトークンの期限切れによる401エラー。
- [新規追加] 接続が瞬断した場合は、10秒待って最大3回リトライすること。
>>>>>> REPLACE
```

**安全性**: 既存ファイル内に `SEARCH` ブロックの文字列が完全一致で存在しない場合、`PatchMerger` が処理を拒否する。これにより既存 Skill の破損・白紙化リスクを防ぐ。

### 12.5 隠しメタツール

通常の対話ターンからは厳格に隠蔽され、`Post-run` フェーズの AuditorWorker（Lane B）からのみ駆動される 2 つのツール。

1. **`create_new_skill(name, markdown_content)`**: `workspace/skills/self_improved/{name}.md` を新規作成。PicoClaw 互換の標準フォーマットを厳守。
2. **`patch_existing_skill(target_file, search_replace_patch)`**: PatchMerger を呼び出し、`SEARCH/REPLACE` ブロックによる安全な差分パッチマージを適用。

### 12.6 自動生成 Skill テンプレート規格

```markdown
---
id: skill_{{skill_name}}
type: self_improved
trigger_condition: "{{RAG 検索用キーワード（エラー・要求のキーワード）}}"
last_updated: {{YYYY-MM-DD}}
---
# Skill: {{スキルの明快なタイトル}}

## 1. 概要・発動シチュエーション
- この手順書は、{{エラーやハングアップ・要求}}の際に適用する。
- 目的: {{最終的に達成すべきゴール}}

## 2. 実践的実行手順 (Runbook / Recipes)
エージェントは、この状況に直面した際、以下のシーケンスを正確に再現すること。
1. `{{ツール名_1}}` を実行し、引数に `{{想定パラメータ}}` を渡す。
2. 出力に `{{特定のエラー文}}` が含まれる場合、`{{ツール名_2}}` でリカバリせよ。

## 3. 陥りがちな罠と検証チェックリスト
- [注意] {{過去の失敗ログから判明したやってはいけない操作}}
- [ ] チェック: 実行後、HAのステータスまたはログが `{{正常値}}` に戻っていることを確認する。
```

### 12.7 Skill GC（コンパクション・忘却）

内製 `CronService` の `daily-summary` cron 実行時に、LLM 自身に以下の「知識の剪定」を実行させる。

- **コンパクション**: `self_improved/` 内に類似目的の Skill が複数乱立した場合、1 つの高位 Skill へマージし古いファイルを削除。
- **忘却（デリート）**: 過去 30 日間のセッションで 1 度も RAG からヒットされなかった動的 Skill は一時的な揮発性知識とみなし、物理ファイルを自動削除してインデックスから抹消。

---

## 13. bwrap サンドボックス `[将来拡張]`

### 13.1 採用理由

LLM が自律生成したコードやプロンプトインジェクションからシステムを物理的に守るため、一般権限で動作する超軽量コンテナ化ツール **`bwrap` (Bubblewrap)** を採用。**Spawn-per-call（1 回きりの完全使い捨て環境）** で実行する。

### 13.2 bwrap マウント・隔離戦略

| 設定 | 効果 |
|---|---|
| `--unshare-net` | ネットワーク遮断。機密データ（HA トークン等）の外部流出を物理的に防止 |
| `--ro-bind /usr /usr` 等 | OS システム領域は読み込み専用で安全に共有 |
| `--tmpfs /tmp` | 書き込み領域はメモリ上の使い捨て `tmpfs`。プロセス終了と同時に完全消滅 |

### 13.3 symlink 対策

サンドボックス外を指す絶対パスの symlink は隔離空間内で「リンク切れ」を起こす。
そのため、Rust 側からツールへファイルをマウントする際は必ず **`std::fs::canonicalize` を実行して実体の絶対パスを解決してからバインドする** ことを鉄則とする。

---

## 14. HomeAssistant 統合 `[将来拡張]`

### 14.1 導入の目的

HA サーバーのセンサーデータ（室温・湿度・CO2・人感等）を「感覚器官」として統合する。
生データをそのまま LLM へ渡すとコンテキストを過度に圧迫し、かつ単一の現在値では「上昇しつつある」という時系列文脈を理解できない。
インメモリ・リングバッファで時系列の「兆候」を算出し、極小のフットプリントで LLM に先回り理解させる。

### 14.2 TrendAnalyzer

センサーデータのスパイクによる誤検知を防ぎ、過去 1 時間（10 分おきに 6 サンプル）の傾きを計算する固定長バッファ。

```rust
pub struct TrendAnalyzer {
    history: VecDeque<SensorPoint>,
    max_samples: usize,           // デフォルト 6（RPi4 メモリ保護）
    stability_threshold: f32,     // トレンド反転判定閾値（例: 0.5）
}

impl TrendAnalyzer {
    pub fn get_trend_arrow(&self) -> &'static str {
        if self.history.len() < 2 { return "→"; }
        let diff = self.history.back().unwrap().value
                 - self.history.front().unwrap().value;
        if diff > self.stability_threshold { "↑" }
        else if diff < -self.stability_threshold { "↓" }
        else { "→" }
    }
}
```

### 14.3 HA エンコーダによるコンテキスト圧縮

毎ターンの `ContextBuilder` で system 領域に動的に埋め込まれる環境サマリー（1 行・約数十トークン）。

```
[HA_ENV|21:05] [Room: 27.5°C↑ | CO2: 1250ppm↑] [Presence: Detected] [Outer: Rain]
```

### 14.4 データ取り込み流量制限 & ルーティング

```
[ HA サーバー ]
│ (State Changed イベント / REST API)
▼
rustyclaw-gateway (ha_client)
  └── 最低 10 分間の時間ベース間引き (Throttling)
       ↓
MessageBus
  ├── TrendAnalyzer へプッシュして傾きを計算
  ├── [通常ターン] ContextBuilder が 1 行サマリーを吸い出し (Lane A)
  └── [スパイク検知] 閾値突破時（例: CO2↑ 1,500ppm 超）
         → HeartbeatService へ緊急フラグ通知
         → 自発投稿（Proactive posts）を強制キック (Lane A)
```

---

## 15. rig-core / rig-fastembed 統合 `[将来拡張]`

### 15.1 採用理由

- **rig-core**: LLM プロバイダー抽象・ツール定義・エージェントループを統一的に扱うフレームワーク。現状の自製 `LlmProvider` trait を段階的に置き換え、プロバイダー追加コストを削減する。
- **rig-fastembed**: `fastembed`（ONNX Runtime ベース）のラッパー。`multilingual-e5-small` 等のローカル Embedding モデルをプロセス内で実行し、外部 API なしでセマンティック検索を実現する。

### 15.2 移行方針

- `rustyclaw-providers` の各プロバイダーを `rig::providers` ベースへ段階的に移行。
- `LlmProvider` trait は互換ラッパーとして当面維持し、移行期間中の既存コードへの影響を最小化する。

### 15.3 ローカル Embedding

現状の tantivy BM25 全文検索に加え、`rig-fastembed` によるベクトル検索レイヤーを追加。

```rust
// Lane B の spawn_blocking 内で実行（ONNX 演算の Tokio スレッドプール汚染を防ぐ）
tokio::task::spawn_blocking(|| {
    let model = EmbeddingModel::new("multilingual-e5-small")?;
    let embeddings = model.embed(chunks)?;
    // SQLite または インメモリベクトルストアへ保存
})
```

> **注意**: ONNX Runtime の初期化コストが高いため、モデルインスタンスは `once_cell::sync::Lazy` 等でプロセス内キャッシュする。毎リクエスト初期化は禁止。

---

## 16. 運用・デプロイ

### 16.1 デプロイ先への SSH 接続

```bash
ssh rp1
```

| 項目 | 値 |
|---|---|
| SSH エイリアス | `rp1` |
| Hostname | `RaspberryPi.local`（解決不可時は `192.168.1.12`） |
| ユーザー / Arch | `kazuaki` / `aarch64` |
| バイナリ配置先 | `~/.local/bin/rustyclaw` |
| 本番ルート | `~/.rustyclaw` → NAS 共有 `production/`（symlink） |

### 16.2 aarch64 クロスビルド

```bash
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  cargo build --release --target aarch64-unknown-linux-gnu -p rustyclaw-cli
# 成果物: target/aarch64-unknown-linux-gnu/release/rustyclaw-cli
```

### 16.3 デプロイ（推奨: 自動）

```bash
./scripts/deploy.sh
# x64/aarch64 ビルド → production/bin/ 配置 → rp1 へ転送 → サービス再起動まで実行
```

**config profile 切り替え**:

```bash
# 本番（クラウド LLM 主力）
cd production/config && ln -sfn config.cloud-llm.json config.json
# 開発（ローカル LLM 主力）
cd production/config && ln -sfn config.local-llm.json config.json
```

### 16.4 サービス管理

```bash
ssh rp1 'sudo systemctl status  rustyclaw'
ssh rp1 'sudo systemctl restart rustyclaw'
ssh rp1 'journalctl --user -u rustyclaw -f'
```

**デプロイ前検証（実 API 不要）**:

```bash
rustyclaw --config /tmp/verify/config.json --workspace /tmp/verify/workspace --no-agent gateway
curl -s http://127.0.0.1:8080/api/concurrency
```

### 16.5 systemd サービス設定

```ini
[Unit]
Description=RustyClaw AI Agent
After=network-online.target

[Service]
Type=simple
User=kazuaki
ExecStart=/home/kazuaki/.local/bin/rustyclaw gateway
Restart=on-failure
RestartSec=5s
OOMScoreAdjust=-500
MemoryMax=2G
WatchdogSec=60s

[Install]
WantedBy=multi-user.target
```

```rust
// systemd watchdog 通知（main 起動後に spawn）
tokio::spawn(async {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]);
    }
});
```

### 16.6 Hot Reload

`SIGHUP` シグナルを受信するとプロセスを再起動することなく `workspace/` の設定ファイルおよび各種 Markdown プロンプトを安全にリロードする。
ダッシュボードの `/reload` エンドポイントからも同等の操作が可能。

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
