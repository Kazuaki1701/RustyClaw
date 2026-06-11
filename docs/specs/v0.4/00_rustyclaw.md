# RustyClaw v0.4 システム仕様書

> [!NOTE]
> **ステータス**: `[ACTIVE]`
> **バージョン**: v0.4
> **最終更新日**: 2026-06-11（Phase 01〜03 実装完了）
> **対象コード**: `crates/` 全クレート + `context-mode` 外部 MCP サーバー
> **前バージョン比較**: v0.3 からの変更点は §4 参照
> **Upstream 比較**: [`../v0.3/91_upstream_comparison.md`](../v0.3/91_upstream_comparison.md)

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
    ├── rustyclaw-tools/         # ★ bwrap・memory_search を削除、ctx_* ツール追加
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
| LLM 抽象 | `rig-core` | `[将来拡張]` |

---

## 3. アーキテクチャ全体図

```
rustyclaw-cli (binary)
    ↓ コマンドディスパッチ
rustyclaw-gateway
    ├── MessageBus (tokio mpsc + broadcast)
    ├── AgentLoop → LaneRegistry
    ├── ChannelManager (Discord / LINE)
    ├── HeartbeatService
    ├── CronService
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

| 削除対象 | 所在 | 委譲先 |
|---|---|---|
| `SearchIndexManager`（tantivy BM25） | `rustyclaw-storage/src/search.rs` | `ctx_search` |
| `memory_embeddings` テーブル全体 | `rustyclaw-storage/src/lib.rs` | `ctx_search` / `ctx_index` |
| `search_similar_with_source` / `search_similar_with_decay` | `rustyclaw-storage` | `ctx_search` |
| `ingest_memory_md` / `ingest_session_summary` / `ingest_static_documents` | `rustyclaw-agent` | `ctx_index` |
| `LocalEmbeddingClient` / `EmbeddingConfig.use_local_embedding` | `rustyclaw-providers` | context-mode 内部 |
| `workspace_execute_script`（bwrap） | `rustyclaw-tools` | `ctx_execute` |
| `memory_search` ツール | `rustyclaw-tools` | `ctx_search` |

### 4.2 Rust 側に追加するコード

| 追加対象 | 所在 | 説明 |
|---|---|---|
| `ExternalMcpController` | `rustyclaw-gateway` | context-mode 子プロセスのライフサイクル管理 `[実装済]` |
| `ctx_execute` / `ctx_search` / `ctx_index` / `ctx_patch` | McpClientHandler 自動登録 | Rust ラッパー不要・context-mode が MCP ツールとして公開 `[実装済]` |

### 4.3 context-mode プロセス管理

```rust
pub struct ExternalMcpController {
    child: tokio::process::Child,
}

impl ExternalMcpController {
    pub fn spawn(workspace_root: &Path) -> std::io::Result<Self> {
        let storage_dir = workspace_root.join(".context-mode");
        let child = tokio::process::Command::new("context-mode")
            .env("CONTEXT_MODE_DIR", storage_dir.to_string_lossy().as_ref())
            .env("CONTEXT_MODE_PLATFORM", "custom-rustyclaw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child })
    }
}
```

- Gateway 起動時に `spawn()` → Gateway シャットダウン時に `SIGTERM`
- context-mode がクラッシュした場合は指数バックオフで再起動（Phase 26 Auto-Reconnect と同様）
- Lane B 経由で JSON-RPC を仲介し、CPU コアは最大 1 枚占有に制限

---

## 5. ワークスペースファイル体系（v0.4 変更点）

```
~/.rustyclaw/workspace/
├── SOUL.md / AGENTS.md / MEMORY.md / USER.md / HEARTBEAT.md   ← 変更なし
├── memory/                                                      ← 変更なし
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
| [`../v0.3/01_pipeline.md`](../v0.3/01_pipeline.md) | §4, §7 | `[実装済]` | 変更なし |
| [`../v0.3/02_memory.md`](../v0.3/02_memory.md) | §5, §9 | `[実装済]` | 変更なし |
| [`../v0.3/03_llm_provider.md`](../v0.3/03_llm_provider.md) | §8 | `[実装済]` | 変更なし |
| [`../v0.3/04_workspace.md`](../v0.3/04_workspace.md) | §6 | `[実装済]` | `.context-mode/` ディレクトリ追加 |
| [`../v0.3/05_heartbeat.md`](../v0.3/05_heartbeat.md) | §10 | `[実装済]` | 変更なし |
| [`../v0.3/06_hermes_skills.md`](../v0.3/06_hermes_skills.md) | §12 | `[将来拡張]` | ctx_execute / ctx_patch を前提に再設計 |
| [`../v0.3/07_extensions.md`](../v0.3/07_extensions.md) | §13〜17 | `[将来拡張]` | §13（本仕様）を実装中 |
| [`../v0.3/08_deployment.md`](../v0.3/08_deployment.md) | §16 | 運用ドキュメント | Node.js ≥ 22.5 + context-mode インストール手順追加済み（§16.7） |
| [`../v0.3/09_dashboard.md`](../v0.3/09_dashboard.md) | — | `[実装済]` | 変更なし |
| [`../v0.3/10_mcp.md`](../v0.3/10_mcp.md) | — | `[実装済]` | context-mode は stdio transport で接続 |
| [`../v0.3/11_operation.md`](../v0.3/11_operation.md) | — | `[実装済]` | context-mode プロセス監視項目追加予定 |
| [`../v0.3/12_storage.md`](../v0.3/12_storage.md) | §11 | `[実装済]` | RAG / Embedding 節は context-mode に委譲 |

---

## 7. 重要設計決定事項（v0.4 追加・変更）

v0.3 不変ルール（[`../v0.3/00_rustyclaw.md` §17](../v0.3/00_rustyclaw.md)）を継承した上で以下を追加。

17. **context-mode は Gateway 起動時に子プロセスとして spawn し、Gateway シャットダウンまで常駐させる**
18. **context-mode の stdin/stdout は `rmcp` stdio transport 経由で接続し、ポートを開けない**
19. **context-mode への JSON-RPC 呼び出しは必ず Lane B 経由でシリアライズする**（CPU コア占有 1 枚制限）
20. **`.context-mode/` ディレクトリは context-mode プロセスが自律管理し、Rust 側から直接 read/write しない**
21. **context-mode がクラッシュした場合は fail-open とし、該当ツール呼び出しはエラーを返すが Pipeline は停止しない**
22. **`workspace_execute_script`（bwrap）と `memory_search` ツールを Rust 側から削除し、`ctx_execute` / `ctx_search` に一本化する**
23. **Node.js バージョンは ≥ 22.5 を必須とする**（`node:sqlite` 内蔵で C++ ネイティブビルド不要）
