# Implementation Plan — Phase 2 & 4: Gateway Services, Heartbeat System, and Long-Term Memory

This plan outlines the design and implementation details for completing the remaining core architectural pieces of RustyClaw (Phase 2 & 4). This will turn RustyClaw from a single-turn CLI agent into a fully autonomous, long-running, self-active gateway runtime.

---

## Goal Description

We will implement the following services and systems to achieve a fully autonomous agentic runtime:
1. **Gateway Services**:
   - **`CronService`**: Internal scheduler for periodically triggering Heartbeat patrols (every 10 minutes) and Daily Summaries (daily at midnight).
   - **`WatchdogService`**: Integration with `systemd` watchdog (`sd-notify`) to send lifespans every 30 seconds.
   - **`HealthServer`**: Super lightweight HTTP server built on `tokio::net::TcpListener` supporting `/health`, `/ready`, and `/reload` (hot-reloads configuration).
2. **Heartbeat System (自発行動)**:
   - **`heartbeat-digest.md` Generator**: Reads recent activity since the last run (incremental) or does a 24-hour Deep Scan (every 6th run), formats logs into an ultra-dense 1-line layout, and writes atomically.
   - **Heartbeat Executor**: Executes `HEARTBEAT.md` + personality files with `Background` priority lane.
   - **Mute / Silent Control (`HEARTBEAT_OK`)**: If output contains `HEARTBEAT_OK` (case-insensitive), runs silently. Otherwise, posts proactive message to the active Discord channel and appends it to conversation history (Proactive Post).
   - **Patrol State & Seen Items**: Stores patrol timestamps (`lastUserContact` and Step-specific check-times) and dedupes seen items in SQLite `seen_items` and `patrol_state` tables, syncing them to `heartbeat-state.json`.
3. **Long-Term Memory & Conversation Continuity**:
   - **Memory Flush**: Post-turn non-blocking `tokio::spawn` task that triggers every 3 user messages (or 1st turn) to extract new learnings, update `MEMORY.md`, and log to `memory/logs/YYYY-MM-DD.md`.
   - **Session Continuation**: Restores context when the last message was from a previous day by fetching yesterday's daily summary and injecting it.
   - **Tantivy Search Index**: Integrates `tantivy` for pure-Rust BM25 indexing and querying of logs and summaries.
4. **Integration Specs alignment**:
   - **Background Queue Capacity**: Enforce a queue capacity limit of exactly 1 for the background lane to prevent heartbeat pile-up. Discard older pending background runs.
   - **Filename Mapping**: Map colon-containing session IDs (`cron:heartbeat`, `cron:flush`, `cron:daily-summary`) to clean, filesystem-safe filenames (`cron-heartbeat.jsonl`, `cron-flush.jsonl`, `cron-daily-summary.jsonl`).
   - **`lastUserContact` Tracking**: Dynamically update `lastUserContact` in SQLite and `heartbeat-state.json` during user sessions, and check it in Heartbeat Step 5 (triggering only when `now - lastUserContact >= 8 hours` and local time is outside Quiet Hours of 23:00 - 08:00).

---

## User Review Required

> [!IMPORTANT]
> **systemd Watchdog Integration**
> The systemd watchdog integration uses `sd-notify` crate. If the gateway is run outside a systemd service (e.g., standard development CLI), `sd-notify` will gracefully log a warning and do nothing, ensuring seamless local development.
>
> **Lightweight Health TCP Server**
> Rather than adding heavy web-framework dependencies (like `axum` or `hyper`) which increase compile times and memory footprint on Raspberry Pi 4, we will implement a super-clean, low-overhead HTTP parser directly using `tokio::net::TcpListener`. It will listen on port `8080` (customizable in `config.json` if needed).
>
> **Tantivy Integration**
> We will add `tantivy` to `rustyclaw-storage/Cargo.toml`. To fit the limited 4GB RAM environment of RPi4, we will use a single search thread and limit indexing buffer sizes.

---

## Proposed Changes

### 1. Component: `rustyclaw-storage` (Tantivy & Database upgrades)

#### [MODIFY] [Cargo.toml](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-storage/Cargo.toml)
- Add `tantivy = "0.22"` to dependencies.

#### [NEW] [search.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-storage/src/search.rs)
- Implement `SearchIndexManager` to manage BM25 full-text indexing.
- Schema:
  - `path`: String (stored, unique identifier)
  - `content`: Text (indexed, tokenized for search)
  - `date`: String (indexed, keyword for filtering)
- Methods:
  - `index_file(path: &Path, content: &str, date: &str) -> Result<()>`
  - `search(query: &str) -> Result<Vec<PathBuf>>`

#### [MODIFY] [lib.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-storage/src/lib.rs)
- Expose `SearchIndexManager` and integrate it with `DbManager`.

---

### 2. Component: `rustyclaw-agent` (Memory Flush & Continuation)

#### [MODIFY] [lib.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-agent/src/lib.rs)
- Implement `SessionContinuation`:
  - Check if the last log entry is from a different day.
  - If so, read `memory/summaries/YYYY-MM-DD-{slug}.md` and inject a summary context header.
- Implement **Memory Flush**:
  - Create `flush_memory_async(workspace_dir: PathBuf, session_id: String) -> Result<()>` that triggers a background `tokio::spawn`.
  - It extracts key learnings using LLM, updates `MEMORY.md` (atomic & fail-open), and appends activity to `memory/logs/YYYY-MM-DD.md`.
  - Maintain a simple SQLite/memory state checking the count of user messages since the last flush (delta threshold: 3).

---

### 3. Component: `rustyclaw-gateway` (Daemon Services & Heartbeat)

#### [MODIFY] [Cargo.toml](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-gateway/Cargo.toml)
- Add `sd-notify = "0.4.1"` for systemd integration.

#### [NEW] [cron.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-gateway/src/cron.rs)
- Implement `CronService` running background tick loops:
  - **Heartbeat Tick**: Every 10 minutes, publishes `SystemEvent::IncomingMessage` with `session_id: "cron:heartbeat"` and `priority: Priority::Background`.
  - **Daily Summary Tick**: Triggers daily at midnight, generating the Daily Summary of all active sessions and re-indexing them.

#### [NEW] [watchdog.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-gateway/src/watchdog.rs)
- Implement `WatchdogService` using `sd-notify`.
- Loops every 30 seconds to notify systemd that the gateway is alive.

#### [NEW] [health.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-gateway/src/health.rs)
- Implement a super lightweight custom `HealthServer` listening on `0.0.0.0:8080`.
- Handlers:
  - `GET /health` -> `200 OK`
  - `GET /ready` -> `200 OK`
  - `GET /reload` -> Sends hot-reload signal to Gateway and returns `200 OK`.

#### [NEW] [heartbeat.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-gateway/src/heartbeat.rs)
- Implement **`heartbeat-digest.md` generation**:
  - Computes difference between now and last run.
  - Read active `sessions/*.jsonl` files (ignoring `cron-heartbeat.jsonl`).
  - Format to ultra-dense one-liners: `[HH:MM] {session}: {prompt} -> {response}`.
  - Every 6th run performs a full 24-hour Deep Scan.
  - Limit total size to 3000 chars.
- Implement **Heartbeat Execution**:
  - Read `HEARTBEAT.md` + `SOUL.md` + `AGENTS.md` + `MEMORY.md` + `USER.md`.
  - Execute LLM using `Background` semaphore lane.
  - Check **Quiet Hours** and **Last Interaction**: Verify `now - lastUserContact >= 8 hours` and local time is NOT within Quiet Hours (23:00 - 08:00) before triggering proactive Step 5 vocal greetings.
  - Parse response:
    - If contains `HEARTBEAT_OK` (case-insensitive): silent mode. Record to `memory/logs/YYYY-MM-DD.md`.
    - If no `HEARTBEAT_OK` (case-insensitive): proactive speak. Send message to Discord/Telegram via connector, and write back to the active user session log file (`sessions/{user_session_id}.jsonl`) as an assistant message (Proactive Post injection).
  - Update last patrol run time in SQLite database and `heartbeat-state.json`.

#### [MODIFY] [lib.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-gateway/src/lib.rs)
- Integrate `CronService`, `WatchdogService`, `HealthServer`, and `HeartbeatService` inside the main `Gateway::run` orchestrator loop.
- **LaneRegistry Upgrades**:
  - Enforce a queue capacity limit of exactly 1 for the background lane to prevent heartbeat pile-up. Discard older pending background runs when a new one is received.
  - Dynamically update `lastUserContact` in SQLite and `heartbeat-state.json` upon successful execution of any user session (session ID not starting with `cron`).
  - Support config updates (reloads) in all services when `SIGHUP` or `/reload` is received.
- **SessionLogger Upgrades**:
  - Map colon-containing session IDs (`cron:heartbeat`, `cron:flush`, `cron:daily-summary`) to clean, filesystem-safe filenames (`cron-heartbeat.jsonl`, `cron-flush.jsonl`, `cron-daily-summary.jsonl`).

---

## Phase 5: Rate Limit 対策・Memory Flush 改善・運用品質向上

### 背景

Antigravity 2.0 によるクォータ強化に対応するため、gmn プロバイダの API アクセス頻度を抑制する一連の改善を実施した。

---

### 5-1. gmn プロバイダ — `--no-agent` 必須化

#### 問題
gmn のデフォルト動作はエージェントモード（最大25ターン）。`complete()` 1回で最大 25 API リクエストが発生し、rate limit の主因になっていた。

#### 対処 [`rustyclaw-providers/src/lib.rs`]
- `complete()` / `complete_stream()` の gmn 起動引数に `--no-agent` を追加。
- RustyClaw 側がループ制御するため、gmn は single-turn のみ実行する。

---

### 5-2. gmn パッチビルド — `GMN_MAX_RETRIES` 環境変数

#### 問題
gmn 内部の 429/5xx リトライが `maxRetries = 5`（ハードコード定数）で外部制御不可。

#### 対処 [`/mnt/Projects/gmn/master/src/internal/api/client.go`]
- `maxRetries` を `GMN_MAX_RETRIES` 環境変数で上書き可能な変数に変更。
- `--version` に `+rustyclaw` サフィックス、`--help` にパッチ説明・環境変数一覧を追加。
- `~/.local/share/go/bin/gmn` にインストール済み。

---

### 5-3. セマフォ値の削減

#### 問題
Antigravity 2.0 のクォータに対して同時 gmn 数が多すぎた。

#### 対処 [`rustyclaw-gateway/src/lib.rs`]
| セマフォ | 変更前 | 変更後 |
|---------|--------|--------|
| `user_sem` | 4 | 2 |
| `bg_sem`   | 2 | 1 |

---

### 5-4. flush_memory() — セマフォ管理外問題の修正

#### 問題
`trigger_memory_flush_async()` は `tokio::spawn` で直接起動されており、`user_sem` / `bg_sem` を取得しなかった。並列チャット時に flush の gmn プロセスがセマフォ上限を超えて起動していた。

#### 対処
- `LaneRegistry` に `flush_sem: Arc<Semaphore>` (容量1) を追加。
- `Pipeline::new(config, flush_sem)` でセマフォを渡す。
- `trigger_memory_flush_async()` 内で `flush_sem.acquire_owned()`（最大60秒待機）を取得してから `flush_memory()` を実行。

#### セマフォ全体像（変更後）
```
user_sem  = 2  ← メインチャット
bg_sem    = 1  ← heartbeat / daily-summary
flush_sem = 1  ← flush_memory() 専用
           ──────────────────────────────
           意図した最大同時 gmn = 4
```

---

### 5-5. Memory Flush — GeminiClaw 方式全書き直し

#### 問題
旧実装は「新情報を抽出して追記」方式。MEMORY.md が 5KB を超えると永続スキップになり、以降メモリが更新されなくなっていた。

#### 対処 [`rustyclaw-agent/src/lib.rs`]
- GeminiClaw の手法を取り入れ、既存 MEMORY.md をプロンプトに含めて LLM に**全書き直し版**を返させる方式に変更。
- デリミタ: `---NEW_MEMORY--- / ---END_MEMORY---`、`---DAILY_LOG--- / ---END_DAILY_LOG---`
- MEMORY.md は追記ではなく**上書き**（`atomic_write`）。サイズ管理は LLM が担う。
- LLM が 5KB を超えて返した場合のフェイルセーフとして Rust 側で 70/20 トランケートを適用。
- `execute_stream()` の `debug_dump_enabled || true` バグ（常に true）を修正。

#### ヘルパー関数
- `extract_delimited_block()`: デリミタ間テキスト抽出
- `truncate_70_20()`: 70%先頭 / 20%末尾のフェイルセーフトランケート

---

### 5-6. ログタイムスタンプのローカルタイム化

#### 問題
`tracing_subscriber` のデフォルトタイマーが UTC のため、ログ時刻が JST と9時間ずれていた。

#### 対処 [`rustyclaw-cli/src/main.rs`、`Cargo.toml`]
- `tracing-subscriber` に `chrono` feature を追加。
- `ChronoLocal::new("%Y-%m-%dT%H:%M:%S%.3f%z")` タイマーを stdout・file 両レイヤーに適用。
- 出力例: `2026-05-26T22:42:04.850+0900`

---

### 5-7. Dashboard セッションログ修正

#### 問題
`/logs/session` が `cli-session.jsonl` をハードコード参照しており、HTTP ダッシュボードのログが更新されなかった。

#### 対処 [`rustyclaw-gateway/src/health.rs`]
- `get_latest_session_log()` を実装: `sessions/` 内で最新更新の `.jsonl` ファイルを動的に検索（`cron*` 除く）。
- `POST /chat` のセッション ID を毎回生成から `"http-dashboard"` 固定に変更（セッション履歴が蓄積されるように）。

---

## Verification Plan

### Automated Tests
We will build and run all unit tests:
```bash
cargo check
cargo test --all
```

### Manual Verification
1. **Lightweight Health Server Verification**:
   - Start gateway: `cargo run -- gateway`
   - Test endpoints:
     ```bash
     curl http://localhost:8080/health
     curl http://localhost:8080/ready
     curl http://localhost:8080/reload
     ```
2. **Heartbeat System Verification**:
   - Run a heartbeat patrol manually or trigger it by letting the internal scheduler fire.
   - Verify `workspace/memory/heartbeat-digest.md` is correctly generated.
   - Verify `workspace/memory/heartbeat-state.json` updates with correct timestamps.
   - Verify `memory.db` gets populated with `patrol_state` and `seen_items`.
   - Verify that silent runs outputting `HEARTBEAT_OK` do not post to channels, while proactive runs correctly post messages and inject them back into conversation history.
3. **Memory Flush & Continuation Verification**:
   - Chat with the agent via CLI (`cargo run -- agent -m "..."`) multiple times.
   - Verify that non-blocking Memory Flush updates `MEMORY.md` and generates daily Obsidian-style logs under `workspace/memory/logs/`.
   - Start a conversation on a new day and verify that Session Continuation restores yesterday's context.
