# 外部 Crate 活用による自前実装置き換え分析レポート

**作成日**: 2026-06-03  
**対象**: RustyClaw — `rustyclaw-providers` / `rustyclaw-mcp` / `rustyclaw-tools` / `rustyclaw-gateway/cron.rs`  
**目的**: 既存外部 crate を積極活用し自前実装を削減するための調査・決定記録

---

## 1. 調査対象 Crate

| Crate | 役割 | 検討状況 |
|---|---|---|
| `rig-core` | LLM プロバイダ抽象・Tool トレイト・Agent ループ・MCP | 導入検討中 |
| `agent-skills` / `agent-skill-rs` | スキル管理 | **存在しない**（後述） |
| `agent-skills-rs` | スキルパッケージ管理（発見・インストール） | 用途が異なるため非採用 |
| `croner` | Cron 式パーサー・次回実行時刻計算 | **採用決定** |
| `tokio-cron-scheduler` | Tokio ネイティブスケジューラ | 候補として分析、非採用 |
| `apalis` + `apalis-sqlite` | バックグラウンドジョブキュー | 候補として分析、非採用（過剰） |

---

## 2. `rustyclaw-providers` の機能分析

### 現在提供する機能

| 機能 | 概要 |
|---|---|
| `LlmProvider` トレイト | `complete()` / `complete_stream()` の抽象インターフェース |
| `OpenAiCompatProvider` | OpenAI 互換 API への HTTP クライアント（Groq / OpenRouter / LM Studio / CF AI）|
| `GmnCliProvider` | `gmn` CLI をサブプロセス起動する Gemini 専用プロバイダ |
| `NoopProvider` | `RUSTYCLAW_NO_AGENT=1` 時に API 送信なし、ログのみ |
| `resolve_provider_id()` | `config_name` プレフィックス（`lms-` / `cf-` 等）でプロバイダ自動識別 |
| レートリミット管理 | `reset_after()` パーサー（`56s` / `1m30s` / CF daily quota 等）、プロバイダ別クールダウン HashMap |
| CF Neurons トラッキング | `cf-ai-neurons` ヘッダ読取・`calc_cf_neurons()` 計算・`neuron_usage.json` 日次永続化 |
| LLM I/O デバッグダンプ | `dump_llm_io()` → `memory/debug/llm/<category>/YYYY-MM-DD/HH-MM-SS.json`、5 日超自動削除 |
| Cloudflare AIG ヘッダ付与 | `cf-aig-gateway-id` / `cf-aig-authorization` |

---

## 3. `rig-core` の適用範囲

### 3-1. `rustyclaw-providers` への適用

| 現在の実装 | rig-core で置き換え可能 | 置き換え不可（RustyClaw 固有） |
|---|---|---|
| `LlmProvider` トレイト | `CompletionModel` / `CompletionClient` | — |
| `OpenAiCompatProvider` | `providers::openai::Client` + `base_url()` ビルダー | — |
| `Message` / `ToolCall` 型 | `rig_core::completion::Message` 等 | — |
| ストリーミング | `StreamingPrompt` / `StreamingChat` | — |
| レートリミット管理 | **なし** | プロバイダ別クールダウン HashMap |
| CF Neurons トラッキング | **なし** | Cloudflare 固有ビジネスロジック |
| LLM I/O デバッグダンプ | **なし** | `dump_llm_io()` 全体 |
| `GmnCliProvider` | **なし** | gmn CLI 固有アダプタ |
| `NoopProvider` | **なし** | デバッグ専用 |
| CF AIG ヘッダ付与 | **なし** | Cloudflare Gateway 固有 |

### 3-2. `rustyclaw-mcp` への適用

| 現在の実装 | rig-core で置き換え可能 |
|---|---|
| JSON-RPC 2.0 stdio プロトコル実装 | `rig` の MCP クライアント（`rmcp` feature） |
| MCP ハンドシェイク (`initialize` → `notifications/initialized`) | rig-mcp が担当 |
| `list_tools` / `call_tool` RPC | rig の MCP クライアントが標準提供 |
| `McpTool` → `Tool` トレイトアダプト | rig-mcp が自動変換 |
| `McpManager`（複数サーバー管理） | `Toolset` + MCP 接続管理で代替可能 |

→ **`rustyclaw-mcp` 全体を rig-core (rig-mcp) で置き換え可能。**

### 3-3. `rustyclaw-tools` への適用

| 現在の実装 | rig-core で置き換え可能 | 置き換え不可 |
|---|---|---|
| `Tool` トレイト | `rig_core::tool::Tool`（`#[tool]` マクロで JSON スキーマ自動生成） | — |
| `ToolRegistry` / `to_llm_schemas()` | `rig_core::tool::Toolset` | — |
| エージェントループ | `rig::agent::Agent` | — |
| `CronScheduleTool` | **なし** | ドメイン固有 |
| `WorkspaceReadTool` / `WorkspaceWriteTool` | **なし** | ドメイン固有 |
| `MemorySearchTool`（BM25） | **なし** | `rustyclaw-storage` 依存 |
| `WebSearchTool`（Brave Search） | **なし** | ドメイン固有 |
| `WebFetchTool`（HTML タグ除去） | **なし** | ドメイン固有 |
| `WorkspaceExecuteScriptTool` + Vault 注入 | **なし** | RustyClaw 固有 |

### 3-4. SKILL エンジン（`gateway/skills.rs`）への適用

rig-core にも `agent-skills-rs` にも相当機能なし。**全て独自実装として維持必要。**

| 機能 | 性質 |
|---|---|
| `SKILL.md` YAML フロントマター解析・バリデーション | RustyClaw 固有 |
| `load_skills()` — `workspace/skills/` 自動スキャン | RustyClaw 固有 |
| `generate_skills_directory()` — Discovery ディレクトリ生成 | RustyClaw 固有 |
| `inject_skill_content()` — Discovery + Activation 2段階インジェクション | RustyClaw 固有 |
| スクリプトパス列挙 → `run_workspace_script` ルーティング | RustyClaw 固有 |
| 相対リンクのスキルパス変換 | RustyClaw 固有 |

> **補足**: `agent-skill` / `agent-skill-rs` という名前の crate は crates.io に存在しない。
> 存在する `agent-skills-rs` は LLM 実行と無関係なスキルパッケージ管理ツール（GitHub/GitLab からスキルを発見・インストール・ロックファイル管理）であり、RustyClaw のスキルインジェクション機能とは目的が異なるため非採用。

---

## 4. スケジューリング分析と `croner` 採用決定

### 4-1. 現在の `CronService` の構造（4ループ）

| ループ | 間隔 | トリガー評価 | 二重実行防止 | 問題点 |
|---|---|---|---|---|
| ① Heartbeat | 30分固定 | なし | なし | — |
| ② Daily Summary | 1時間ポーリング | 日付文字列比較 | SQLite に当日日付 | — |
| ③ Session Summary | 60秒固定 | mtime で 5分 idle 判定 | なし | — |
| ④ Dynamic cron.json | 60秒ポーリング | `"HH:MM" == now_time` 文字列比較 | SQLite に日付/unixtime | **分をまたぐと取りこぼし** |

#### ④ の問題
`cron` トリガーは 60秒ごとに `now_time == "HH:MM"` を文字列比較するため、サービスが当該分にダウンしていると **その日のジョブが永久にスキップ**される。`interval` トリガーも手動の elapsed 計算に依存。

### 4-2. スケジューリング候補比較

| Crate | tokio 相性 | cron 式 | 動的追加/削除 | SQLite 永続化 | RPi4 重量 | 状態 |
|---|---|---|---|---|---|---|
| `tokio-cron-scheduler` | ネイティブ | あり | あり | **なし**（PG/Nats のみ） | 中 | 活発 |
| `apalis` + `apalis-sqlite` | 対応 | あり | あり | **あり** | **重い** | 活発 |
| `croner` | 組み合わせ自由 | **パーサーのみ** | — | — | **最軽量** | 活発 |
| `delay_timer` | 対応 | あり | あり | なし | 軽 | 活発 |
| `clokwerk` | 対応（AsyncScheduler） | なし（DSL のみ） | あり | なし | 軽 | 維持 |

### 4-3. `croner` 採用理由

1. **最小依存**: パーサー 1 クレートのみ追加。RPi4 のリソース制約に最適
2. **SQLite dedup 継続**: 現在の `rustyclaw-storage` による二重実行防止設計を変えない
3. **アーキテクチャ変更不要**: 4ループ構造を維持したまま `HH:MM` 文字列比較の欠陥だけを修正
4. **tokio との相性**: `croner` の `find_next_occurrence()` + `tokio::time::sleep_until()` で自然に組み合わせ可能
5. **`CronScheduleTool` の改善**: `next_run_epoch()` / `compute_schedule()` を croner で再実装することで、正確な次回実行時刻をエージェントに提供可能

### 4-4. `croner` 導入後の変更スコープ

**置き換えられる部分**:
- `next_run_epoch()` の `"HH:MM"` 手動パース → `croner::Cron::find_next_occurrence()`
- ④ Dynamic cron.json ループの `"HH:MM" == now_time` 文字列比較 → croner による正確な次回実行時刻計算
- `compute_schedule()` の実装を croner ベースに更新

**変わらない部分**:
- 4ループの基本構造（`tokio::time::interval` / `interval_at`）
- SQLite を使った二重実行防止（日付比較・unixtime 比較）
- `find_next_session_needing_summary()` のセッションスキャンロジック
- `SystemEvent::IncomingMessage` の `bus.publish()` 呼び出し
- cron.json のファイル形式と差分検出ポーリング

---

## 5. 置き換えロードマップまとめ

### Phase A: rig-core 移行（大規模・高優先）

```
削除可能なクレート/モジュール:
  rustyclaw-mcp (全体)        → rig-core rmcp feature
  rustyclaw-providers (HTTP層) → rig-core providers::openai

残存する RustyClaw 固有:
  GmnCliProvider / NoopProvider
  CF Neurons トラッキング
  レートリミット管理（クールダウン HashMap）
  LLM I/O デバッグダンプ
  CF AIG ヘッダ付与

Tool インフラ:
  Tool トレイト / ToolRegistry → rig-core Tool / Toolset
  組み込みツール 6 種           → 独自実装のまま維持
```

### Phase B: croner 導入（小規模・低リスク）

```
変更ファイル: crates/rustyclaw-gateway/src/cron.rs
追加依存: croner = "3"

変更内容:
  next_run_epoch()    → croner::Cron::find_next_occurrence() ベースに置き換え
  compute_schedule()  → croner ベースに更新
  ④ ループの文字列比較 → croner による正確なスケジューリングに置き換え
```

### 各 Phase の影響範囲

| Phase | 変更規模 | リスク | 期待効果 |
|---|---|---|---|
| A: rig-core | 大（`rustyclaw-mcp` 削除・`rustyclaw-providers` 大幅縮小） | 中 | コード削減・型安全性向上 |
| B: croner | 小（`cron.rs` 内のみ） | 低 | cron 取りこぼしバグ修正・精度向上 |

---

## 6. 決定事項

| 項目 | 決定 |
|---|---|
| スケジューリング crate | **`croner` 採用** |
| LLM フレームワーク | **`rig-core` 移行検討（Phase A として計画化）** |
| MCP クライアント | **`rig-core rmcp` feature で置き換え（Phase A に含む）** |
| SKILL エンジン | **独自実装維持**（相当 crate なし） |
| `agent-skills-rs` | **非採用**（目的が異なる） |
| `apalis` | **非採用**（RPi4 には過剰） |
| `tokio-cron-scheduler` | **非採用**（SQLite 永続化非対応、croner で十分） |
