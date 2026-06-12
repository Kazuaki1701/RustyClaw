# RustyClaw v0.4 システム仕様書

> [!NOTE]
> **ステータス**: `[ACTIVE]`
> **バージョン**: v0.4
> **最終更新日**: 2026-06-12（Phase 01〜03, 28b-3, 45-1, 47-1, 48-1, 50 実装完了。Context 最適化を v0.4 残課題として追加）
> **対象コード**: `crates/` 全クレート + `context-mode` 外部 MCP サーバー
> **前バージョン比較**: v0.3 からの変更点は §4 参照
> **Upstream 比較**: [`../v0.3/91_upstream_comparison.md`](../v0.3/91_upstream_comparison.md) / [`91_context_upstream_comparison.md`](91_context_upstream_comparison.md)（context 管理特化）
> **LLM Config 制限**: [`92_llm_config_constraints.md`](92_llm_config_constraints.md)（Phase 51-1 実装・小コンテキスト制約・purpose 分析）

**プロジェクト名**: RustyClaw v0.4  
**設計方針**: **外部 MCP context-mode に委譲できるものはすべて委譲し、Rust 側は HA 連携・LLM 制御・レーン管理に集中する。**

---

## 1. プロジェクト概要

### 1.1 目的

v0.3 の設計資産（Pipeline・Lane Control・Heartbeat・メモリ管理）を維持しつつ、**`mksglu/context-mode`（Node.js ≥ 22.5）を外部 MCP サーバーとして RPi4 に同居（Colocation）**させる。

これにより：
- bwrap サンドボックス実行・SEARCH/REPLACE パッチ適用・BM25 エピソード記憶検索 の Rust 実装コードを **100% 削除**
- 自製コードの **約 45〜55% を削減**
- Rust 側は **HA 連携・レーン制御・LLM オーケストレーション** に専念

v0.5 では context-mode の全ロジックを純 Rust に移植し、Node.js 依存を完全排除する（単一バイナリ化）。

### 1.2 参照 Upstream

| プロジェクト | 役割 |
|---|---|
| PicoClaw (Go) | Gateway / Pipeline / CronService / Skills |
| GeminiClaw (TypeScript) | メモリ 3 層・Heartbeat・Session Continuation |
| Hermes Agent (Nous Research) | 自己改善 Skills・自己監査ループ |
| **context-mode (mksglu)** | **bwrap 実行・BM25 検索・パッチ適用の外部委譲先** |

### 1.3 実行環境

| 項目 | 値 |
|---|---|
| ハードウェア | Raspberry Pi 4 Model B (RAM 8GB) |
| OS | Raspberry Pi OS Lite (headless, aarch64) |
| Rust ターゲット | `aarch64-unknown-linux-gnu` |
| **Node.js（追加要件）** | **≥ 22.5（`node:sqlite` 内蔵、ネイティブコンパイル不要）** |
| 制約 | OpenSSL 依存禁止（`rustls` 統一）・クロスコンパイル対応必須 |

---

## 2. Cargo Workspace 構成

### 2.1 クレート構成（v0.4 変更点）

```
rustyclaw/
├── Cargo.toml
└── crates/
    ├── rustyclaw-cli/           # binary: main エントリポイント（変更なし）
    ├── rustyclaw-gateway/       # ★ ExternalMcpController 追加
    ├── rustyclaw-agent/         # Pipeline・AgentLoop（変更なし）
    ├── rustyclaw-providers/     # LLM HTTP クライアント群（変更なし）
    ├── rustyclaw-channels/      # Discord・LINE コネクタ（変更なし）
    ├── rustyclaw-tools/         # ★ bwrap・memory_search を削除（ctx_* は外部 MCP 経由で登録）
    ├── rustyclaw-config/        # 設定型定義（変更なし）
    └── rustyclaw-storage/       # ★ tantivy・memory_embeddings を削除
```

### 2.2 依存クレート変更

#### 削除（context-mode へ委譲）

| 削除クレート | 理由 |
|---|---|
| `tantivy` | BM25 検索 → context-mode `ctx_search` へ委譲 |
| `rig-fastembed` / `fastembed` | Embedding 生成 → context-mode へ委譲 |

#### 追加・変更なし

| 用途 | クレート | ステータス |
|---|---|---|
| 非同期ランタイム | `tokio` (full, multi-thread) | `[実装済]` |
| HTTP クライアント | `reqwest` + `rustls-tls` | `[実装済]` |
| シリアライズ | `serde` + `serde_json` | `[実装済]` |
| CLI | `clap` (derive) | `[実装済]` |
| エラー | `anyhow` + `thiserror` | `[実装済]` |
| ログ | `tracing` + `tracing-appender` | `[実装済]` |
| SQLite | `rusqlite` + `deadpool-sqlite` | `[実装済]` |
| MCP クライアント | `rmcp` | `[実装済]`（context-mode 通信にも使用） |
| atomic write | `tempfile` | `[実装済]` |
| systemd watchdog | `sd-notify` | `[実装済]` |
| 設定暗号化 | `age` | `[実装済]` |
| キャンセル | `tokio-util` (CancellationToken) | `[実装済]` |
| 日時 | `chrono` | `[実装済]` |
| Web UI | `axum` (0.7) | `[実装済]` |
| LLM 抽象 | `rig-core` (0.38, rmcp feature) | `[実装済]` |
| cron 次回時刻計算 | `croner` (3.0.1) | `[実装済]`（Phase 48-1） |
| async trait | `async-trait` (0.1) | `[実装済]` |
| 乱数 | `rand` (0.8) | `[実装済]` |

---

## 3. アーキテクチャ全体図

```
rustyclaw-cli (binary)
    ↓ コマンドディスパッチ
rustyclaw-gateway
    ├── MessageBus (tokio mpsc + broadcast)
    ├── AgentLoop → LaneRegistry
    ├── ChannelManager (Discord / LINE)
    ├── HeartbeatService          # ★ Phase 50: HA env context / spike alert を Heartbeat プロンプトに注入
    ├── CronService               # ★ Phase 50: 10 分毎に 220_ha_env_snapshot.sh を実行・exit 2 で緊急 Heartbeat
    ├── WatchdogService
    ├── HealthServer + WebDashboard
    └── ExternalMcpController  ◄── ★ NEW: context-mode 子プロセス管理
         │  tokio::process::Command でフォーク
         │  stdin/stdout MCP JSON-RPC 2.0
         ▼
    [ context-mode (Node.js ≥ 22.5) ]  ◄── 子プロセス（常駐）
         ├── ctx_execute   # bwrap サンドボックス実行
         ├── ctx_search    # BM25 / FTS5 エピソード記憶検索
         ├── ctx_index     # エピソード記憶インデックス登録
         └── ctx_patch     # SEARCH/REPLACE パッチ適用
              ストレージ: workspace/.context-mode/
        ↓
rustyclaw-agent (Pipeline: 変更なし)
    ├── ContextBuilder
    ├── CallLLM (FallbackChain + SSE)
    ├── ExecuteTools (ToolRegistry)
    │     ├── ctx_execute  ← context-mode MCP ツール
    │     ├── ctx_search   ← context-mode MCP ツール
    │     ├── ctx_index    ← context-mode MCP ツール
    │     ├── ctx_patch    ← context-mode MCP ツール
    │     ├── workspace_read / workspace_write  ← Rust 内製（維持）
    │     ├── web_fetch / web_search            ← Rust 内製（維持）
    │     └── cron_schedule                     ← Rust 内製（維持）
    └── PublishResponse
        ↓
rustyclaw-providers (変更なし)
    ├── OpenAiCompatProvider
    ├── AnthropicProvider
    ├── GeminiProvider
    └── OllamaProvider
        ↓
rustyclaw-storage (削減版)
    ├── SessionStore (JSONL append-only, fail-closed)   ← 維持
    ├── MemoryStore (MEMORY.md + logs/ + summaries/)    ← 維持
    └── SqliteStore
          ├── usage テーブル        ← 維持
          ├── patrol_state テーブル ← 維持
          └── seen_items テーブル   ← 維持
    ※ SearchIndex (tantivy)     → 削除（context-mode に委譲）
    ※ memory_embeddings テーブル → 削除（context-mode に委譲）
```

---

## 4. v0.3 からの変更サマリー

### 4.1 Rust 側から削除するコード

| 削除対象 | 所在 | 委譲先 | ステータス |
|---|---|---|---|
| `SearchIndexManager`（tantivy BM25） | `rustyclaw-storage/src/search.rs` | `ctx_search` | `[削除済]` |
| `memory_embeddings` テーブル全体 | `rustyclaw-storage/src/lib.rs` | `ctx_search` / `ctx_index` | `[削除済]` |
| `search_similar_with_source` / `search_similar_with_decay` | `rustyclaw-storage` | `ctx_search` | `[削除済]` |
| `ingest_memory_md` / `ingest_session_summary` / `ingest_static_documents` | `rustyclaw-agent` | `ctx_index` | `[削除済]` |
| `LocalEmbeddingClient` / `EmbeddingConfig.use_local_embedding` | `rustyclaw-providers` | context-mode 内部 | `[削除済]` |
| `workspace_execute_script`（bwrap） | `rustyclaw-tools` | `ctx_execute` | `[削除済]` |
| `memory_search` ツール | `rustyclaw-tools` | `ctx_search` | `[削除済]` |
| `rustyclaw-summary-proto` crate（PoC） | `crates/rustyclaw-summary-proto/` | Phase 45-1 実装に統合 | `[削除済]`（Phase 47-1） |

### 4.2 Rust 側に追加するコード

| 追加対象 | 所在 | 説明 |
|---|---|---|
| `ExternalMcpController` | `rustyclaw-gateway` | context-mode 子プロセスのライフサイクル管理 `[実装済]` |
| `ctx_execute` / `ctx_search` / `ctx_index` / `ctx_patch` | McpClientHandler 自動登録 | Rust ラッパー不要・context-mode が MCP ツールとして公開 `[実装済]` |
| `try_ctx_search` / `try_ctx_index` | `rustyclaw-gateway/src/lib.rs` | Gateway から ctx_search/ctx_index を呼ぶ fail-open ヘルパー `[実装済]`（Phase 45-1） |
| `HomeAssistantConfig` | `rustyclaw-config/src/lib.rs` | HA 接続設定（`enabled` / `endpoint` / `token`）。`ToolsConfig.home_assistant` に追加 `[実装済]`（Phase 50） |
| `HeartbeatService::get_ha_env_context()` / `check_ha_spike()` | `rustyclaw-gateway/src/heartbeat.rs` | `memory/ha-env-summary.txt` 読み取り・`memory/ha-state.json` の CO2 スパイク検知（fail-open）`[実装済]`（Phase 50） |
| CronService HA ポーリングループ | `rustyclaw-gateway/src/cron.rs` | 10 分毎に `220_ha_env_snapshot.sh --check-spike` を実行。exit 2 で `Priority::Normal` `cron:heartbeat` 発火（3 時間クールダウン付き）`[実装済]`（Phase 50） |
| `220_ha_env_snapshot.sh` | `workspace/skills/home-assistant-rest-api/scripts/` | HA REST API からセンサー取得・6 サンプルリングバッファ（`memory/ha-state.json`）・トレンド矢印算出・CO2 スパイク検知（exit 2）・1 行サマリー出力（`memory/ha-env-summary.txt`）`[実装済]`（Phase 50） |
| **Heartbeat Digest 生成** | `rustyclaw-gateway/src/heartbeat.rs` | Heartbeat 実行前に増分セッションダイジェストを生成し Heartbeat プロンプトに注入（GeminiClaw 参照実装あり）`[v0.4 残課題]` |
| **Session-level Summary** | `rustyclaw-gateway` / `rustyclaw-agent` | アイドル 5 分後にセッションサマリーを生成し `try_ctx_index` でエピソード記憶に登録（GeminiClaw 参照実装あり）`[v0.4 残課題]` |
| **ContextBuilder context window 対応** | `rustyclaw-agent/src/context.rs` | モデルの context window サイズに応じてセッション履歴・注入コンテキスト量を動的調整。70/20/10 予算分割（v0.3 §5.3 参照）`[v0.4 残課題]` |

#### try_ctx_search / try_ctx_index 概要

- **Heartbeat**: プロンプト生成前に `try_ctx_search(query, limit=3, sort="timeline")` を呼び、結果を "Past context (from episodic memory):" としてプロンプトに注入する
- **Session-summary**: `generate_session_summary()` 成功後に `try_ctx_index(content, source="session-summary:{id}")` でエピソード記憶に登録する
- どちらも失敗時は `warn!` ログのみ。Pipeline/Heartbeat を停止しない（fail-open）
- `tool_server_handle.call_tool(name, args_json)` を直接使用し、Agent Pipeline を経由しない

### 4.3 context-mode プロセス管理

```rust
pub struct ExternalMcpController {
    child: tokio::process::Child,
    workspace_root: PathBuf,
}

impl ExternalMcpController {
    // 戻り値: (Self, ChildStdin, ChildStdout) — stdin/stdout は rmcp transport に渡す
    pub fn spawn(workspace_root: &Path) -> Result<(Self, ChildStdin, ChildStdout)> {
        let storage_dir = workspace_root.join(".context-mode");
        let (cmd, args) = resolve_context_mode_command(); // bun 優先・context-mode にフォールバック
        let mut child = tokio::process::Command::new(&cmd)
            .args(&args)
            .env("CONTEXT_MODE_DIR", storage_dir.to_string_lossy().as_ref())
            .env("CONTEXT_MODE_PLATFORM", "custom-rustyclaw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        Ok((Self { child, workspace_root: workspace_root.to_path_buf() }, stdin, stdout))
    }
}
```

- **起動コマンド自動検出**: `bun`（`$HOME/.bun/bin/bun`）が見つかれば `bun cli.bundle.mjs` で起動（bun:sqlite 使用）、なければ `context-mode` コマンド（Node.js ≥ 22.5 必要）にフォールバック
- Gateway 起動時に `spawn()` → `Drop` 実装で Gateway シャットダウン時に `SIGTERM`
- context-mode がクラッシュした場合は `start_context_mode()` の指数バックオフループで自動再起動

---

## 5. ワークスペースファイル体系（v0.4 変更点）

```
~/.rustyclaw/workspace/
├── SOUL.md / AGENTS.md / MEMORY.md / USER.md / HEARTBEAT.md   ← 変更なし
├── memory/                                                      ← 変更なし（Phase 50 で HA ファイル追加）
│   ├── ha-env-summary.txt  ◄── ★ Phase 50: HA 環境 1 行サマリー（CronService が 10 分毎に更新）
│   └── ha-state.json       ◄── ★ Phase 50: HA センサー 6 サンプルリングバッファ + spike_detected フラグ
├── sessions/                                                    ← 変更なし
├── skills/                                                      ← 変更なし
└── .context-mode/          ◄── ★ NEW: context-mode 専用ストレージ
    ├── index/              # BM25 / FTS5 インデックス（SQLite）
    ├── sessions/           # エピソード記憶チャンク
    └── sandbox/            # bwrap 実行用一時領域
```

`.context-mode/` は context-mode プロセスが自律管理する。Rust 側から直接読み書きしない。

---

## 6. 詳細仕様ファイル

v0.3 仕様書（`docs/specs/v0.3/`）を基底として参照し、v0.4 差分を本ファイルに集約する。

| ファイル | 対応節 | ステータス | v0.4 変更 |
|---|---|---|---|
| [`../v0.3/01_pipeline.md`](../v0.3/01_pipeline.md) | §4, §7 | `[実装済 + 残課題]` | §将来拡張 ContextBuilder 節 → v0.4 残課題（context window 対応） |
| [`../v0.3/02_memory.md`](../v0.3/02_memory.md) | §5, §9 | `[実装済 + 残課題]` | §5.3 70/20/10 コンテキスト戦略 → v0.4 残課題（Heartbeat Digest・Session Summary・window 対応） |
| [`../v0.3/03_llm_provider.md`](../v0.3/03_llm_provider.md) | §8 | `[実装済]` | 変更なし |
| [`../v0.3/04_workspace.md`](../v0.3/04_workspace.md) | §6 | `[実装済]` | `.context-mode/` ディレクトリ追加 |
| [`../v0.3/05_heartbeat.md`](../v0.3/05_heartbeat.md) | §10 | `[実装済]` | Phase 50: HA 環境コンテキスト注入（§10.6 追加） |
| [`../v0.3/06_hermes_skills.md`](../v0.3/06_hermes_skills.md) | §12 | `[将来拡張]` | ctx_execute / ctx_patch を前提に再設計 |
| [`../v0.3/07_extensions.md`](../v0.3/07_extensions.md) | §13〜17 | `[一部完了]` | §13（context-mode 委譲）・§14（HA 統合 Phase 50）・§15（rig-core）完了。§16・§17 は将来拡張 |
| [`../v0.3/08_deployment.md`](../v0.3/08_deployment.md) | §16 | 運用ドキュメント | Node.js ≥ 22.5 + context-mode インストール手順追加済み（§16.7） |
| [`../v0.3/09_dashboard.md`](../v0.3/09_dashboard.md) | — | `[実装済]` | 変更なし |
| [`../v0.3/10_mcp.md`](../v0.3/10_mcp.md) | — | `[実装済]` | context-mode は stdio transport で接続 |
| [`../v0.3/11_operation.md`](../v0.3/11_operation.md) | — | `[実装済]` | Phase 50: HA 監視項目追加済み。context-mode プロセス監視項目追加済み（§2-6） |
| [`../v0.3/12_storage.md`](../v0.3/12_storage.md) | §11 | `[実装済]` | RAG / Embedding 節は context-mode に委譲 |

---

## 7. 重要設計決定事項（v0.4 追加・変更）

v0.3 不変ルール（[`../v0.3/00_rustyclaw.md` §重要設計決定事項](../v0.3/00_rustyclaw.md)）を継承した上で以下を追加。

17. **context-mode は Gateway 起動時に子プロセスとして spawn し、Gateway シャットダウンまで常駐させる**
18. **context-mode の stdin/stdout は `rmcp` stdio transport 経由で接続し、ポートを開けない**
19. **`.context-mode/` ディレクトリは context-mode プロセスが自律管理し、Rust 側から直接 read/write しない**
20. **context-mode がクラッシュした場合は fail-open とし、該当ツール呼び出しはエラーを返すが Pipeline は停止しない**
21. **`workspace_execute_script`（bwrap）と `memory_search` ツールを Rust 側から削除し、`ctx_execute` / `ctx_search` に一本化する**
22. **Node.js バージョンは ≥ 22.5 を必須とする**（`node:sqlite` 内蔵で C++ ネイティブビルド不要）
23. **Gateway は Heartbeat 直前に `try_ctx_search` を呼び、セッション終了後に `try_ctx_index` を呼ぶ**（Phase 45-1）。Agent Pipeline を経由せず `tool_server_handle.call_tool()` を直接使用し、両操作とも fail-open とする
24. **Cron の `"cron"` タイプは `croner` crate の `Cron::find_next_occurrence()` で次回時刻を計算する**（Phase 48-1）。サービス停止後の catch-up は `>=` 比較で一度だけ実行し、多重起動しない
26. **context window 最適化は「Heartbeat Digest → Session-level Summary → ContextBuilder window 予算」の順で段階実装する**（v0.4 残課題）。各モデルの context window サイズ（`max_tokens` 相当）を `LlmConfig` から取得し、セッション履歴・注入コンテキストを 70%/20%/10% 予算内に収める。失敗時は従来動作にフォールバック（fail-open）。
25. **HA 統合は `220_ha_env_snapshot.sh` を CronService から子プロセス実行するモデルを採用する**（Phase 50）。センサーデータは `memory/ha-env-summary.txt`（1 行サマリー）と `memory/ha-state.json`（6 サンプルリングバッファ）に書き出し、HeartbeatService が fail-open で読み取り Heartbeat プロンプトに注入する。`HOMEASSISTANT_TOKEN` は vault 経由・`HOMEASSISTANT_ENDPOINT` は config → 環境変数で bash スクリプトに継承させる
