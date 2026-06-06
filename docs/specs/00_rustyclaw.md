# RustyClaw 総合システム仕様書 ＆ ドキュメントインデックス

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - ドキュメントインデックス)  
> **最終更新日**: 2026-05-30  
> **備考**: 本リポジトリのドキュメント構成は、`docs/specs/`（有効な基本仕様）および `docs/archive/`（完了した計画）に物理的整理され、運用ルール化されました。

**作成日**: 2026-05-26  
**更新日**: 2026-05-30  
**作成者**: Claude Sonnet 4.6（調査・設計セッション）  
**維持管理**: Gemini 3.5 Flash (Medium)  
**プロジェクト名**: RustyClaw  

---

## 1. プロジェクト概要

### 目的

PicoClaw（Go 製 AI エージェントランタイム）の Rust クローンを自作する。
GeminiClaw（TypeScript 製）の優れた設計思想（メモリ管理・Heartbeat システム）を取り込んで融合させた独自実装。

### 参照 Upstream

| プロジェクト | URL | 役割 |
|---|---|---|
| PicoClaw | https://github.com/sipeed/picoclaw | アーキテクチャの主参照 |
| GeminiClaw | https://github.com/e-mon/geminiclaw | メモリ・Heartbeat 設計の参照 |

### 実行環境（確定）

| 項目 | 値 |
|---|---|
| ハードウェア | Raspberry Pi 4 Model B |
| RAM | 8GB |
| ストレージ | USB SSD（SD カードではない） |
| OS | Raspberry Pi OS Lite（headless） |
| アーキテクチャ | aarch64 (ARMv8 Cortex-A72) |
| Rust ターゲット | `aarch64-unknown-linux-gnu` |

---

## 2. 設計方針（確定済み）

### PicoClaw から引き継ぐもの

- Workspace ファイル体系（SOUL.md / AGENTS.md / MEMORY.md / USER.md）
- Gateway + MessageBus + AgentLoop の 3 層構造
- Pipeline の 4 ステージ（ContextBuilder → CallLLM → ExecuteTools → Publish）
- CronService（内製スケジューラー）
- Skills システム（SKILL.md の階層ロード）
- PID ファイル + Health HTTP エンドポイント
- Hot Reload（SIGHUP で設定再読み込み）

### GeminiClaw から取り込むもの

- メモリ 3 層設計（短期 / 中期 / 長期）
- Post-run memory flush（セッション後 LLM 抽出 → MEMORY.md）
- Session Continuation（日またぎの文脈引き継ぎ）
- HEARTBEAT.md による Heartbeat システム（GeminiClaw 原版を流用）
- heartbeat-digest.md の自動生成（Heartbeat pre-run）
- heartbeat-state.json による各チェックの時刻管理
- memory/logs/YYYY-MM-DD.md（日次活動ログ）
- Daily summary cron
- Proactive posts 注入（自発投稿を「自分の投稿」として記録）
- 会話継続感を作る 6 技法（後述）
- truncateWithContext（70/20/10 戦略）

### GeminiClaw から採用しないもの

- ACP（子プロセス stdio JSON-RPC）→ LLM を直接 HTTP 呼び出しに変更
- Inngest → 内製 CronService で代替
- QMD 外部プロセス → tantivy（純 Rust BM25）で内製化
- ACP プロセスプール → LaneRegistry + Semaphore で代替
- Docker/Seatbelt サンドボックス → 別途検討

---

## 3. Cargo Workspace 構成

```
rustyclaw/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── rustyclaw-cli/          # binary: main エントリポイント
│   ├── rustyclaw-gateway/      # lib: 起動・オーケストレーション
│   ├── rustyclaw-agent/        # lib: Pipeline・AgentLoop・AgentInstance
│   ├── rustyclaw-providers/    # lib: LLM HTTP クライアント群
│   ├── rustyclaw-channels/     # lib: Telegram・Discord 等の実装
│   ├── rustyclaw-tools/        # lib: built-in tools・MCP クライアント
│   ├── rustyclaw-config/       # lib: 設定ファイル型定義・migration
│   └── rustyclaw-storage/      # lib: SQLite・JSONL セッション永続化
└── workspace/                  # デフォルトワークスペース（開発用）
```

---

## 4. 依存クレート（確定版）

| 用途 | クレート | 備考 |
|---|---|---|
| 非同期ランタイム | `tokio` (full, multi-thread) | 8GB・4コアで制限不要 |
| HTTP クライアント | `reqwest` + `rustls-tls` | OpenSSL 依存を排除（クロスコンパイル対策） |
| SSE ストリーミング | `reqwest` bytes_stream | 手動パース |
| シリアライズ | `serde` + `serde_json` | 標準 |
| CLI | `clap` (derive) | サブコマンド型安全 |
| エラー | `anyhow` + `thiserror` | 境界で使い分け |
| ログ | `tracing` + `tracing-appender` | rolling file、SSD 直書き |
| SQLite | `rusqlite` + `deadpool-sqlite` | 接続プール、WAL モード |
| async trait | `async-trait` | trait に async fn |
| 全文検索 | `tantivy` | 純 Rust BM25、外部プロセス不要 |
| MCP クライアント | `rmcp` | Rust 公式 MCP SDK |
| atomic write | `tempfile` | 電源断対策 |
| systemd watchdog | `sd-notify` | WatchdogSec 連携 |
| 設定暗号化 | `age` | .security.yml 相当 |
| キャンセル | `tokio-util` (CancellationToken) | turn キャンセル |
| 日時 | `chrono` | タイムゾーン付き日時 |

---

## 5. Cargo.toml プロファイル設定

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

---

## 6. クロスコンパイル設定

```bash
# ツールチェーン
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu

# OpenSSL を使わない（最重要）
# reqwest は必ず rustls-tls feature を指定する
```

```toml
[dependencies]
reqwest = { version = "0.12", default-features = false,
            features = ["rustls-tls", "stream", "json"] }
```

---

## 7. アーキテクチャ全体図

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
    └── HealthServer (HTTP /health /ready /reload)
        ↓
rustyclaw-agent (Pipeline)
    ├── ContextBuilder
    │   ├── SystemContext (SOUL.md + AGENTS.md + MEMORY.md + USER.md)
    │   ├── ConversationHistory (Vec<Message> 蓄積)
    │   ├── SessionContinuation (日またぎ文脈)
    │   └── ProactivePosts 注入
    ├── CallLLM (FallbackChain + streaming SSE)
    ├── ExecuteTools (ToolRegistry in-process)
    └── PublishResponse
        ↓
rustyclaw-providers (LlmProvider trait)
    ├── OpenAiCompatProvider (reqwest + SSE)
    ├── AnthropicProvider
    ├── GeminiProvider
    └── OllamaProvider (ローカル LLM)
        ↓
rustyclaw-storage
    ├── SessionStore (JSONL append-only, fail-closed)
    ├── MemoryStore (MEMORY.md + logs/ + summaries/)
    ├── SearchIndex (tantivy BM25)
    └── SqliteStore
        ├── usage テーブル（トークン使用量）
        ├── patrol_state テーブル（heartbeat-state.json 相当）
        └── seen_items テーブル（Interest Patrol 既読管理）
```

---

## 8. ワークスペースファイル体系（確定版）

```
~/.rustyclaw/workspace/
│
│  ╔══════════════════════════════════════════════════════╗
│  ║  人格定義 4 ファイル（毎ターン system prompt に常時注入）║
│  ║  GeminiClaw 原版テンプレートをそのまま流用            ║
│  ╚══════════════════════════════════════════════════════╝
│
├── SOUL.md
│   # アイデンティティ・価値観・人格の核
│   # Core Truths / Purpose / Personality / Values / Bounds
│   # エージェントが変更したらユーザーに報告する
│
├── AGENTS.md
│   # 行動ルール・Memory 書き先マトリクス・Tool 使用指針
│   # Heartbeat 応答規約（HEARTBEAT_OK の判定基準）
│   # エージェントは実質自己改変しない
│
├── MEMORY.md
│   # 長期記憶の核（5KB 以内厳守）
│   # 決定事項・学習内容・ユーザー嗜好・アーキテクチャメモ
│   # Writer: エージェント自発（重要発見時即時）+ system flush（session 後）
│
├── USER.md
│   # ユーザープロファイル
│   # Basics（Name / Timezone / 言語）
│   # Communication Preferences
│   # Work Context
│   # Interests  ← INTERESTS.md を統合済み。Heartbeat の Interest Patrol がここを読む
│   # Preferences & Habits / Notes
│   # Writer: エージェントが会話から学んだ情報を随時追記
│
│  ╔══════════════════════════════════════════════════════╗
│  ║  Heartbeat 制御ファイル                               ║
│  ╚══════════════════════════════════════════════════════╝
│
├── HEARTBEAT.md
│   # Heartbeat の振る舞い指示書（GeminiClaw 原版をそのまま流用）
│   # Step1〜7 の構造（下記参照）
│   # エージェントは絶対に自己改変しない
│   # ユーザーのみが編集する
│
│  ╔══════════════════════════════════════════════════════╗
│  ║  memory/（自動生成・エージェント記録）                 ║
│  ╚══════════════════════════════════════════════════════╝
│
├── memory/
│   │
│   ├── heartbeat-state.json
│   │   # 各チェックの最終実行時刻
│   │   # { "lastChecks": { "activityReview": "...", "memoryMaintenance": "...",
│   │   #   "calendar": "...", "email": "...", "weather": "...",
│   │   #   "lastUserContact": "..." } }
│   │   # Writer: エージェントが Heartbeat 後に自己更新
│   │   # Rust 内部: SQLite patrol_state テーブルで管理
│   │
│   ├── heartbeat-digest.md
│   │   # 前回 Heartbeat 以降のセッション差分ダイジェスト
│   │   # Writer: system（Heartbeat pre-run に自動生成）
│   │   # 通常: 前回以降の JSONL 差分のみ（incremental）
│   │   # 6 回に 1 回: 24 時間 deep scan
│   │   # cron:heartbeat セッション自身は除外
│   │   # 3000 文字以内に圧縮（最新エントリ優先）
│   │
│   ├── logs/
│   │   └── YYYY-MM-DD.md
│   │       # 日次活動ログ（Obsidian 互換 YAML frontmatter）
│   │       # Writer: エージェント任意 + system flush
│   │       # 信頼性: fail-open
│   │       # tantivy にインデックス
│   │
│   └── summaries/
│       └── YYYY-MM-DD-{slug}.md
│           # セッションサマリー（TL;DR + Topics）
│           # Writer: system（idle 時・daily cron）
│           # Session Continuation（日またぎ）に使用
│           # tantivy にインデックス
│
│  ╔══════════════════════════════════════════════════════╗
│  ║  sessions/（永続ログ）                                ║
│  ╚══════════════════════════════════════════════════════╝
│
└── sessions/
    ├── telegram-U12345678-20260525.jsonl   # ユーザー会話（日付ローテーション）
    ├── discord-C98765432-20260525.jsonl    # チャンネル会話
    ├── cron:heartbeat.jsonl               # Heartbeat 実行ログ
    ├── cron:flush.jsonl                   # Memory flush ログ
    ├── cron:daily-summary.jsonl           # 日次サマリーログ
    └── session-titles.json               # タイトル管理（atomic write、JSONL と分離）
```

```
~/.rustyclaw/
├── config.json           # 設定ファイル（atomic write）
├── .security.yml         # 暗号化シークレット（age）
├── workspace/            # 上記のワークスペース
└── memory.db             # SQLite WAL モード
                          #   usage テーブル（トークン使用量）
                          #   patrol_state テーブル（heartbeat-state.json の Rust 管理）
                          #   seen_items テーブル（Interest Patrol 既読管理）
```

### セッション ID 命名規則（GeminiClaw 踏襲）

```
telegram-U12345678-20260525    # ユーザー会話（日付でローテーション）
discord-C98765432-20260525     # チャンネル会話
cron:heartbeat                 # Heartbeat 実行（毎回新規セッション）
cron:flush                     # Memory flush
cron:daily-summary             # 日次サマリー
```

### ファイルの書き込み責任マトリクス

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

---

## 9. Lane Queue 設計（並列制御・混載防止）

GeminiClaw の Inngest Lane Queue + ACP プロセスプールを Rust で再実装。

### 確定数値（GeminiClaw コードから）

| パラメータ | GeminiClaw | RustyClaw 対応 |
|---|---|---|
| 全体同時実行上限 | maxSize=6 | user_sem(4) + bg_sem(2) = 6 |
| ユーザー枠 | Inngest limit=4 | `user_sem` Semaphore permits=4 |
| Background 予約枠 | reservedSlots=2 | `bg_sem` Semaphore permits=2 |
| 同一 session 直列 | Inngest scope=fn, limit=1 | Lane worker（1 worker/session） |
| 待機タイムアウト | waitTimeoutMs=60s | `timeout(60s, sem.acquire())` |

### 混載防止の 3 層

1. **Lane worker 直列保証**: 同一 sessionId → 同一 Lane の mpsc キュー → 1 worker が直列処理
2. **Semaphore**: user/background を独立した Semaphore で管理、枠の食い合いを防ぐ
3. **Priority 分類**: Heartbeat/Flush/DailySummary = Background、それ以外 = Normal

```rust
// Background Lane のキューは最大 1 件（積み上がり防止）
// Heartbeat が来たとき古い pending は破棄して最新だけ残す
let cap = match priority {
    Priority::Normal     => 0,   // 無制限
    Priority::Background => 1,   // 最新 1 件のみ
};
```

---

## 10. LlmProvider 設計（ステートレス HTTP）

### 重要な設計原則

**LlmProvider は完全ステートレス。** 毎回の API 呼び出しは「初対面」。
会話が続いている感覚はすべて Rust コード（ConversationHistory）が作り出す。

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

### ファクトリ

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

---

## 11. 会話継続感を作る 6 技法

ステートレス API で「会話が続いている」感覚を作るための技法。
GeminiClaw のコードから確認した実装パターン。

### ① 会話履歴の蓄積（最基本）

```rust
pub struct ConversationHistory {
    messages:         Vec<Message>,
    estimated_tokens: usize,
}

// turn ごとに push、毎回 API の messages[] に全部渡す
history.push_turn(user_input, &llm_response);
```

### ② コンテキスト圧縮（トークン上限対策）

GeminiClaw の `truncateWithContext` と同じ戦略。

```rust
pub fn compact_if_needed(&mut self, model_limit: usize) {
    if self.estimated_tokens < model_limit * 8 / 10 { return; }
    let n = self.messages.len();
    let keep_head = n * 4 / 10;  // 先頭 40%（導入・背景）
    let keep_tail = n * 4 / 10;  // 末尾 40%（直近の文脈）
    // 中間を [N messages omitted] で置換
}
```

### ③ Memory Flush（セッション後・非同期・fail-open）

```rust
// Pipeline 完了後に tokio::spawn で切り離す
tokio::spawn(async move {
    // 直近 20 エントリを LLM に渡し MEMORY.md + logs/ の更新を依頼
    // 失敗しても warn ログのみ（fail-open）
});
```

### ④ Session Continuation（日またぎ）

翌日の初回ターンのみ発動。前日の summary TL;DR + 直近 5 エントリを system に注入。

### ⑤ Proactive Posts 注入

Heartbeat が自発的に送ったメッセージを「会話履歴外の自分の投稿」として system に注入。
「自分が言ったこと」を忘れないための仕組み。（GeminiClaw context-builder.ts より）

### ⑥ System Prompt 常時注入

毎回の API 呼び出しの system に以下を含める:

```
SOUL.md / AGENTS.md / MEMORY.md / USER.md（Interests 含む）
```

---

## 12. Heartbeat システム設計（GeminiClaw 原版流用）

### ファイルの役割分担

| ファイル/DB | 内容 | 変更者 |
|---|---|---|
| `HEARTBEAT.md` | Heartbeat 振る舞い指示（静的） | ユーザーのみ・**自己改変禁止** |
| `USER.md ## Interests` | 興味領域の定義（準静的） | ユーザー or エージェント |
| `memory/heartbeat-state.json` | 各チェックの最終実行時刻 | エージェント自己更新 |
| SQLite `patrol_state` | heartbeat-state.json の Rust 側管理 | システム自動 |
| SQLite `seen_items` | Interest Patrol 既読管理 | システム自動 |

### HEARTBEAT.md の 7 Step 構造（GeminiClaw 原版そのまま）

| Step | 頻度 | 内容 |
|---|---|---|
| Step 1 | 毎回 | heartbeat-digest.md + summaries/ + logs/ で活動レビュー |
| Step 2 | 数時間ごと | MEMORY.md 整理・USER.md Interests 更新 |
| Step 3 | 毎回 | Calendar / Email チェック（ツールがなければ skip silently） |
| Step 4 | 1日2〜3回 | 天気チェック（4時間インターバル、なければ skip silently） |
| Step 5 | 毎回 | 8h 以上無通信 → 昼間のみ軽く声掛け（Quiet hours 23:00〜08:00 除外） |
| Step 6 | ローテーション | 未完了タスク・失敗セッション対処・バックグラウンド作業 |
| Step 7 | 毎回 | **必ず HEARTBEAT_OK で応答** → 無音 or 通知配信 |

### AGENTS.md における Heartbeat 応答規約（重要）

| 重要度 | 処理 | HEARTBEAT_OK? |
|---|---|---|
| **Critical**（緊急メール・直近 deadline・障害） | アラートテキストとして通知 | No |
| **Informational**（新着非緊急・定常カレンダー） | `logs/YYYY-MM-DD.md` にのみ記録 | Yes |
| **Nothing**（所見なし） | — | Yes |

### 重要な設計ルール

1. **HEARTBEAT_OK で返したら無音**（ユーザーに通知しない）
2. **Heartbeat 実行は `last_user_interaction_at` を更新しない**
   → 声掛け判断が自分自身を「アクティブ」と誤判定するのを防ぐ
3. **HEARTBEAT.md はエージェントが絶対に自己改変しない**
4. **巡回済み管理は SQLite seen_items が担う**（ファイルに書かない）
5. **Background priority**（bg_sem の 2 枠を使用）
6. **Phase 1 実装対象**: Step 1・2・5・7 のみ。Step 3・4 はツール整備後に追加

### heartbeat-digest.md の生成ルール

GeminiClaw `src/agent/session/heartbeat-digest.ts` の移植。

- **通常**: 前回 Heartbeat 以降の JSONL 差分のみスキャン（incremental）
- **6 回に 1 回**: 24 時間 deep scan
- `cron:heartbeat` セッション自身は除外
- 3000 文字以内に圧縮（最新エントリ優先）
- 各エントリを `[HH:MM] session: prompt → response` 形式に圧縮

---

## 13. Storage 設計

### 書き込み信頼性の分類（GeminiClaw docs/memory.md より）

```
System automatic (guaranteed)          Agent-initiated (best-effort)
◄──────────────────────────────────────────────────────────────────►

sessions/*.jsonl  memory.db  MEMORY.md(flush)  summaries/  │  MEMORY.md(agent)  logs/
  fail-closed     fail-closed   fail-open       on-idle     │    voluntary       voluntary
```

```rust
// fail-closed（必ず成功させる）
// → sessions/*.jsonl への append
// → 失敗したら pipeline を止める

// fail-open（失敗しても続行）
// → memory flush, summary 生成, heartbeat-digest 生成
// → tokio::spawn で切り離し、失敗は warn ログのみ

// on-idle（アイドル時に実行）
// → daily summary, search index reindex
// → HeartbeatService / CronService が担当
```

### SQLite 設定

```rust
conn.execute_batch("
    PRAGMA journal_mode=WAL;
    PRAGMA synchronous=NORMAL;
    PRAGMA cache_size=-32000;  -- 32MB（8GB あるので余裕）
    PRAGMA temp_store=MEMORY;
")?;
```

### atomic write（電源断対策）

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

## 14. systemd サービス設定

```ini
[Unit]
Description=RustyClaw AI Agent
After=network-online.target

[Service]
Type=simple
User=pi
ExecStart=/usr/local/bin/rustyclaw gateway
Restart=on-failure
RestartSec=5s
OOMScoreAdjust=-500
MemoryMax=2G
WatchdogSec=60s

[Install]
WantedBy=multi-user.target
```

```rust
// systemd watchdog 通知（main の起動後に spawn）
tokio::spawn(async {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]);
    }
});
```

---

## 15. USB SSD 運用上の留意点

SD カード問題はすべて解消済み。残る留意点のみ記載。

| 項目 | 対処 |
|---|---|
| UASP 相性 | `dmesg` でエラー確認。問題時のみ `usb-storage.quirks=XXXX:YYYY:u` |
| fstrim | `systemctl enable fstrim.timer`（週次） |
| noatime | `/etc/fstab` に `noatime` オプション追加 |
| 電源断 | atomic write 実装で対処（SQLite WAL で保護済み） |

---

## 16. 実装フェーズ計画

### Phase 1 — 動く最小構成（CLI で LLM と会話できる）

- `rustyclaw-config`: 設定ファイル読み書き（serde_json）
- `rustyclaw-providers`: OpenAI 互換 1 プロバイダー（reqwest + SSE）
- `rustyclaw-agent`: Pipeline 基本形（ContextBuilder → CallLLM → Publish）
- `rustyclaw-cli`: `rustyclaw agent -m "hello"` が動く

**完了条件**: CLI から LLM と会話できる

### Phase 2 — Gateway 化

- `rustyclaw-storage`: SQLite セッション + JSONL
- `rustyclaw-agent`: ConversationHistory + 会話継続 6 技法
- `rustyclaw-gateway`: 起動・シグナル処理・Hot Reload

**完了条件**: `rustyclaw gateway` でプロセスとして起動・停止できる

### Phase 3 — チャンネル接続

- `rustyclaw-channels`: Telegram 実装（まず 1 チャンネルのみ）
- `rustyclaw-agent`: LaneRegistry + Semaphore（Lane Queue）
- MessageBus: pub/sub 接続

**完了条件**: Telegram から話しかけられる

### Phase 4 — Heartbeat + Memory

- HeartbeatService（HEARTBEAT.md Step 1・2・5・7 を実装）
- heartbeat-digest.md 自動生成
- Memory Flush（post-run 非同期）
- Session Continuation（日またぎ）
- Daily Summary cron
- tantivy 全文検索インデックス

**完了条件**: 長期記憶・Heartbeat・声掛けが動く

### Phase 5 — 拡張

- Heartbeat Step 3・4（Calendar / Email / 天気ツール整備後）
- 追加プロバイダー（Anthropic / Gemini / Ollama）
- 追加チャンネル（Discord / Slack）— feature flag
- MCP クライアント（rmcp）
- Interest Patrol（USER.md の Interests 監視）

---

## 17. 重要な設計決定事項（変更不可）

1. **INTERESTS.md は USER.md に統合**（独立ファイルとしない）
2. **IDENTITY.md は使用しない**（SOUL.md で統合・GeminiClaw に存在しない）
3. **PATROL.md は使用しない**（HEARTBEAT.md で統合・GeminiClaw 原版を流用）
4. **LlmProvider は完全ステートレス HTTP**（子プロセス不要）
5. **Heartbeat 実行は `last_user_interaction_at` を更新しない**
6. **HEARTBEAT.md はエージェントが自己改変しない**
7. **sessions/*.jsonl は fail-closed**（書き込み失敗で pipeline 停止）
8. **memory flush は fail-open**（失敗しても続行）
9. **OpenSSL 依存を持ち込まない**（`rustls-tls` で統一）
10. **Background Lane のキューは最大 1 件**（Heartbeat 積み上がり防止）
11. **memory/logs/ と memory/summaries/ は別ディレクトリで管理**
12. **heartbeat-state.json はエージェントが自己更新し、Rust は SQLite patrol_state で管理**

---

## 18. 参照コード箇所（GeminiClaw）

調査時に clone したリポジトリのキーファイル。

| ファイル | 参照理由 |
|---|---|
| `templates/SOUL.md` | 人格定義ファイルの原版 |
| `templates/AGENTS.md` | 行動ルール・Heartbeat 応答規約の原版 |
| `templates/MEMORY.md` | 長期記憶ファイルの原版 |
| `templates/USER.md` | ユーザープロファイル（Interests セクション含む）の原版 |
| `templates/HEARTBEAT.md` | Heartbeat 全 7 Step の指示書（原版流用） |
| `docs/memory.md` | Write/Read タイムライン・信頼性スペクトラムの定義 |
| `src/agent/acp/client.ts` | ACP プロトコルの全容（Rust では不要だが設計参考） |
| `src/agent/acp/process-pool.ts` | Lane Queue の数値根拠（maxSize=6, reservedSlots=2） |
| `src/inngest/agent-run.ts` | Inngest concurrency 設定（limit=1/4 の根拠） |
| `src/agent/context-builder.ts` | ContextBuilder・Proactive Posts・truncateWithContext |
| `src/agent/session/store.ts` | SessionEntry 型・JSONL 管理・session-titles.json |
| `src/agent/session/continuation.ts` | 日またぎ Session Continuation の完全実装 |
| `src/agent/session/flush.ts` | Memory flush プロンプトの実装 |
| `src/agent/session/heartbeat-digest.ts` | digest 生成ロジック（incremental / deep scan） |
| `src/agent/turn/pre-execution.ts` | checkResumable（heartbeat は常に新規） |

---

## 19. Gemini への継続指示

### 引き継ぎのコンテキスト

この資料は Claude Sonnet 4.6 との調査・設計セッションの成果物です。
以下を実施済みです：

- PicoClaw（Go）と GeminiClaw（TypeScript）のソースコードを読んでアーキテクチャを把握
- GeminiClaw は `/home/claude/geminiclaw/` に clone 済み（調査環境のみ）
- Rust での実装方針・クレート構成・設計決定を全て確定

### Gemini に依頼する作業

Phase 1 から順に実装を開始してください。

**まず確認してほしいこと**:

1. `rustyclaw-config` クレートから始め、`config.json` の型定義（`Config` struct）を `serde_json` で実装
2. `rustyclaw-providers` の `LlmProvider` trait と `OpenAiCompatProvider` の実装
3. Phase 1 の完了条件（`rustyclaw agent -m "hello"` が動く）を達成

**開発環境**:
- 開発機: Ubuntu Desktop（x86_64）
- デプロイ先: Raspberry Pi 4（aarch64）
- クロスコンパイル: `cross` コマンドを使用
- エディタ: VS Code

**注意事項**:
- `reqwest` は必ず `rustls-tls` feature を使用（OpenSSL 排除）
- `tokio` は `rt-multi-thread` を使用（8GB あるので制限不要）
- エラーは `anyhow`（アプリ層）+ `thiserror`（ライブラリ層）で使い分け

---

*以上が RustyClaw 設計の引き継ぎ資料です。*
*調査期間中に確定した設計判断は「17. 重要な設計決定事項」を参照してください。*
