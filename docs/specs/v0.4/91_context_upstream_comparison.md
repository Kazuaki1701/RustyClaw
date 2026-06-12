# Context 管理 Upstream 横断比較

> **目的**: RustyClaw v0.4 の context window 最適化実装に先立ち、4 upstream の context content 管理を整理・比較する  
> **作成日**: 2026-06-12  
> **関連**: [`v0.4/00_rustyclaw.md §4.2・§7 #26`](00_rustyclaw.md) / [`v0.3/02_memory.md §5.3`](../v0.3/02_memory.md)

---

## 1. 各プロジェクトの設計哲学

| プロジェクト | 言語 | 設計哲学 |
|---|---|---|
| **GeminiClaw** | TypeScript | 人間中心。Markdown/JSONL で可読性優先。Heartbeat Digest・Session Continuation・fail-open を徹底 |
| **gemini-cli**（GeminiClaw upstream） | TypeScript | LLM-centric。全ターンを ECG（Episodic Context Graph）でノード管理。Token budget を厳密計算、Distillation＋Truncation の 2 本柱 |
| **PicoClaw** | Go | 軽量・実用主義。turn boundary 尊重でツールコール保護、ContextManager interface で戦略を差し替え可能 |
| **context-mode**（mksglu） | Node.js | MCP サーバー専用。BM25/FTS5 エピソード記憶・bwrap 実行・パッチ適用に特化。context 管理ロジック自体は持たない |

---

## 2. 5 観点での横断比較

### 2.1 system prompt / static context の構成

| | GeminiClaw | gemini-cli | PicoClaw |
|---|---|---|---|
| 方式 | `@import` で SOUL.md / AGENTS.md / MEMORY.md / USER.md を静的合成。GEMINI.md に書き出し（init/sync 時のみ更新） | ContextProfile の retained budget 内で管理。Protected logical ID で削除防止 | MEMORY.md＋過去 3 日分 daily notes を毎回注入。Skills ツリーを prompt registry に登録 |
| 自律度 | `autonomous` / `supervised` / `read_only` を session context に埋め込み | budget を超えると trigger→compaction | ContextManager interface で戦略を plug-in 交換 |

### 2.2 会話履歴の管理

| | GeminiClaw | gemini-cli | PicoClaw |
|---|---|---|---|
| 形式 | 日付回転 JSONL（`channel-id-YYYYMMDD.jsonl`）、append-only | Node graph 化→token GC→oldest drop | per-session JSONL + turn boundary 尊重 truncation |
| 継続 | 日またぎで continuation inject（初回ターンのみ） | 要約ノードが抽象化し graph に残留 | Ingest→async Compact |
| トークン管理 | HEAD 70%＋TAIL 20%＋省略マーカー で Sliding Window | budget overflow trigger で自動 compaction | budget-aware assembly（contextWindow − tool defs − maxTokens） |

### 2.3 動的コンテキスト注入

| | GeminiClaw | gemini-cli | PicoClaw |
|---|---|---|---|
| 主な注入源 | Heartbeat Digest（増分）・Session Summary・Channel Context・Proactive Posts | event-driven Processor chain（新 message / retained_exceeded / nodes_aged_out） | BM25＋regex tool discovery＋PromptContributor plugin |
| 注入タイミング | Heartbeat 実行前・アイドル後・日またぎ初回ターン | event-based state machine で自動トリガー | 毎リクエスト（GetMemoryContext） |

### 2.4 コンテキスト圧縮・削減

| | GeminiClaw | gemini-cli | PicoClaw |
|---|---|---|---|
| 戦略 | HEAD 70%＋TAIL 20%＋省略マーカー 10% | RollingSummary（LLM 要約）で古い複数ノードを 1 ノードに consolidate＋oldest drop | Safe boundary truncation（nearest user message を検索）、turn boundary 尊重 |
| Digest 上限 | max 3000 chars / エントリ max 200 chars | Token cache で O(1) コスト lookup | Shared tokenizer で事前 budget overflow 検知 |
| LLM 圧縮 | Session Summary をアイドル後に LLM 生成 | Distillation service（FULL/PARTIAL/SUMMARY/EXCLUDED） | async Compact で proactive 圧縮 |

### 2.5 セッション継続

| | GeminiClaw | gemini-cli | PicoClaw |
|---|---|---|---|
| 方式 | 前日 TLDR＋Topics を初回ターンのみ inject。Idempotency: 既 entries あれば skip | Consolidated summary node が old nodes を abstract | Session clear/reset、AssembleResponse で summary 返却 |
| バックフィル | サーバー起動時に 7 日分を自動生成 | Pristine graph 保持で rollback 可能 | — |

---

## 3. コード引用（要所）

### Heartbeat Digest — 増分カットオフ計算（GeminiClaw）
```typescript
// heartbeat-digest.ts
const cutoffMs = !state.lastRunTimestamp
  ? now.getTime() - 60 * 60 * 1000          // 初回: 1h lookback
  : isDeepScan
  ? now.getTime() - 24 * 60 * 60 * 1000    // deep scan (6 回毎): 24h
  : new Date(state.lastRunTimestamp).getTime(); // 増分: 前回実行以降のみ
```

### Token budget 検査（PicoClaw）
```go
// context_budget.go
func isOverContextBudget(contextWindow, msgTokens, toolTokens, maxTokens int) bool {
    return msgTokens + toolTokens + maxTokens > contextWindow
}
```

### Rolling summary ノード生成（gemini-cli）
```typescript
// rollingSummaryProcessor.ts
const summaryNode: RollingSummary = {
  id: randomUUID(),
  type: 'ROLLING_SUMMARY',
  text: snapshotText,
  abstractsIds: nodesToSummarize.map(n => n.id),
};
```

---

## 4. RustyClaw v0.4 への取り込み計画

### 高優先度（v0.4 残課題）

| 施策 | 参照 upstream | 実装場所 | 概要 |
|---|---|---|---|
| **Heartbeat Digest 増分生成** | GeminiClaw `heartbeat-digest.ts` | `rustyclaw-gateway/src/heartbeat.rs` | `lastRunTimestamp` 以降のみスキャン。6 回毎 deep scan（24h）。max 3000 chars 制限 |
| **ContextBuilder context window 対応** | gemini-cli `contextTokenCalculator.ts`＋PicoClaw budget logic | `rustyclaw-agent/src/context.rs` | `LlmConfig` から window サイズ取得 → 70/20/10 予算計算 → turn boundary 尊重 truncation |
| **Session-level Summary** | GeminiClaw `summary.ts` / `daily-summary.ts` | `rustyclaw-gateway/src/lib.rs` | アイドル 5 分後に LLM で title＋tldr＋topics 生成 → `try_ctx_index` でエピソード記憶登録（fail-open） |

### 中優先度（v0.5 以降）

| 施策 | 参照 upstream | 概要 |
|---|---|---|
| **Session continuation 日付回転** | GeminiClaw `continuation.ts` | channel 毎の日付回転 session、前日 TLDR を初回ターン inject |
| **RollingSummary ノード統合** | gemini-cli `rollingSummaryProcessor.ts` | 古い複数 session を LLM で 1 ノードに consolidate |
| **Multi-strategy ContextManager** | PicoClaw interface | Registry pattern で複数実装を plug-in 可能に |

---

## 5. RustyClaw との設計上の最大の違い

| 観点 | GeminiClaw/gemini-cli | RustyClaw v0.4 |
|---|---|---|
| 会話状態の管理主体 | Gemini CLI native `always-resume`（履歴注入不要） | JSONL → `messages[]` 注入（gmn 依存の限り変更不可） |
| エピソード記憶 | Node graph ＋ SQLite（gemini-cli）、BM25 内製（GeminiClaw） | context-mode の BM25/FTS5 に完全委譲（`ctx_search` / `ctx_index`） |
| 圧縮の実行主体 | Rust 側で token 計算・truncation | Rust 側で budget 判断 → 超過分は context-mode 側のインデックス検索で補完 |

---

> **次のステップ**: このドキュメントを基に `docs/plans/` 配下に実装計画書を作成し、Heartbeat Digest → ContextBuilder window 対応 → Session Summary の順で段階実装する。
