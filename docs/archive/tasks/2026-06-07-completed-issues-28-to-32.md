> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-06-07  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-07 (ISSUE-28〜32)

## バグ修正

### ISSUE-28: Heartbeat — MEMORY.md を静的注入から RAG 注入へ切り替え
- **完了日**: 2026-06-07
- **概要**: `build_heartbeat_context` が MEMORY.md（4,529 B ≈ 1,130 tok）を毎回静的注入していた（最大の圧迫要因）のを、RAG 経由での注入へ切り替え。
- **対象**: `crates/rustyclaw-agent/src/lib.rs`（`build_heartbeat_context`, `ingest_static_documents`）
- **関連計画書**: [2026-06-07-heartbeat-context-optimization.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-heartbeat-context-optimization.md)

### ISSUE-29: Heartbeat — HEARTBEAT.md の圧縮
- **完了日**: 2026-06-07
- **概要**: HEARTBEAT.md（4,384 B ≈ 1,096 tok）のプロンプト説明文やコメントなど冗長な記述を削除し、エージェントへの指示のみに絞り込むことで、約650 tokensへ圧縮。
- **対象**: `workspace/HEARTBEAT.md` (実動作環境上は `production/workspace/HEARTBEAT.md`)
- **関連計画書**: [2026-06-07-heartbeat-context-optimization.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-heartbeat-context-optimization.md)

### ISSUE-30: Heartbeat — RAG top_k を 5→2 に引き下げ
- **完了日**: 2026-06-07
- **概要**: Heartbeat は固定ステップを実行するだけで top_k=5 は過剰なため、Heartbeat 専用の RAG top_k を 2 に引き下げ（config 上に `heartbeat_top_k` フィールドを導入）。
- **対象**: `crates/rustyclaw-config/src/lib.rs`, `crates/rustyclaw-agent/src/lib.rs`, `production/config/*.json`
- **関連計画書**: [2026-06-07-heartbeat-context-optimization.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-heartbeat-context-optimization.md)

### ISSUE-31: 通常チャット — MEMORY.md を RAG 登録して動的注入へ切り替え
- **完了日**: 2026-06-07
- **概要**: `execute_with_tools` では MEMORY.md が静的注入されず RAG にも登録されていなかったため、MEMORY.md を `ingest_static_documents` の対象に追加し、動的に注入可能に変更。
- **対象**: `crates/rustyclaw-agent/src/lib.rs`（`ingest_static_documents`）
- **関連計画書**: [2026-06-07-heartbeat-context-optimization.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-heartbeat-context-optimization.md)

### ISSUE-32: config — embedding model 名を実態に合わせて修正
- **完了日**: 2026-06-07
- **概要**: `config.json` の `embedding.model` が `"text-embedding-bge-m3"` のままだった不整合を、ローカルでロードしているモデル `"intfloat/multilingual-e5-small"` に合わせて修正。
- **対象**: `production/config/config.debug.json`, `config.release.json`
- **関連計画書**: [2026-06-07-heartbeat-context-optimization.md](file:///home/kazuaki/Projects/RustyClaw/docs/plans/2026-06-07-heartbeat-context-optimization.md)
