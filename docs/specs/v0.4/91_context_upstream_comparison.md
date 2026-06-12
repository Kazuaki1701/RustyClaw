# Context 管理 Upstream 横断比較

> **目的**: RustyClaw v0.4 の context window 最適化実装に先立ち、4 upstream の context content 管理を整理・比較する  
> **作成日**: 2026-06-12（Hermes Agent ソースコード反映: 2026-06-12）  
> **関連**: [`v0.4/00_rustyclaw.md §4.2・§7 #26`](00_rustyclaw.md) / [`v0.3/02_memory.md §5.3`](../v0.3/02_memory.md)  
> **Hermes Agent ソース**: https://github.com/nousresearch/hermes-agent（Python）

---

## 1. 各プロジェクトの設計哲学

| プロジェクト | 言語 | 設計哲学 |
|---|---|---|
| **GeminiClaw** | TypeScript | 人間中心。Markdown/JSONL で可読性優先。Heartbeat Digest・Session Continuation・fail-open を徹底 |
| **gemini-cli**（GeminiClaw upstream） | TypeScript | LLM-centric。全ターンを ECG（Episodic Context Graph）でノード管理。Token budget を厳密計算、Distillation＋Truncation の 2 本柱 |
| **PicoClaw** | Go | 軽量・実用主義。turn boundary 尊重でツールコール保護、ContextManager interface で戦略を差し替え可能 |
| **Hermes Agent** | Python | 二刀流。**セッション内圧縮**（ContextCompressor + セッション回転）と**Skill 結晶化**（AuditorWorker）を両立。メモリ注入をシステムプロンプトでなくユーザーメッセージに行い Anthropic prefix cache を最大化 |
| **context-mode**（mksglu） | Node.js | MCP サーバー専用。BM25/FTS5 エピソード記憶・bwrap 実行・パッチ適用に特化。context 管理ロジック自体は持たない |

---

## 2. 5 観点での横断比較

### 2.1 system prompt / static context の構成

| | GeminiClaw | gemini-cli | PicoClaw | Hermes Agent |
|---|---|---|---|---|
| 方式 | `@import` で SOUL.md / AGENTS.md / MEMORY.md / USER.md を静的合成。GEMINI.md に書き出し（init/sync 時のみ更新） | ContextProfile の retained budget 内で管理。Protected logical ID で削除防止 | MEMORY.md＋過去 3 日分 daily notes を毎回注入。Skills ツリーを prompt registry に登録 | `_restore_or_build_system_prompt()` が SQLite session DB から前回プロンプトを復元（新規時はフレッシュ構築→DB 永続化）。Anthropic prefix cache 再利用のためターン間でプロンプトを**バイト完全一致**に保つ |
| Skills 注入 | SOUL.md 等に埋め込み | Protected node | BM25 top-N | domain 別 skills/ サブディレクトリ（apple / devops / github / smart-home 等）から関連スキルを注入 |
| Ephemeral 追加 | なし | なし | なし | `agent.ephemeral_system_prompt` を API 呼び出し時のみ末尾追加（DB 永続化しない）。`prefill_messages` も同様 |

### 2.2 会話履歴の管理

| | GeminiClaw | gemini-cli | PicoClaw | Hermes Agent |
|---|---|---|---|---|
| 形式 | 日付回転 JSONL（`channel-id-YYYYMMDD.jsonl`）、append-only | Node graph 化→token GC→oldest drop | per-session JSONL + turn boundary 尊重 truncation | SQLite sessions テーブル。圧縮トリガー時に**新 session ID へ回転**（親 session を "compression" 理由で終了）。DAG 系統で lineage 保持 |
| 継続 | 日またぎで continuation inject（初回ターンのみ） | 要約ノードが抽象化し graph に残留 | Ingest→async Compact | FTS5 セッション検索 + LLM 要約で cross-session recall（Honcho user modeling）。圧縮後も lineage を辿れる |
| トークン管理 | HEAD 70%＋TAIL 20%＋省略マーカー で Sliding Window | budget overflow trigger で自動 compaction | budget-aware assembly（contextWindow − tool defs − maxTokens） | threshold = context length の 50%。到達時に ContextCompressor を起動 |

### 2.3 動的コンテキスト注入

| | GeminiClaw | gemini-cli | PicoClaw | Hermes Agent |
|---|---|---|---|---|
| 主な注入源 | Heartbeat Digest（増分）・Session Summary・Channel Context・Proactive Posts | event-driven Processor chain（新 message / retained_exceeded / nodes_aged_out） | BM25＋regex tool discovery＋PromptContributor plugin | memory prefetch 結果（`_ext_prefetch_cache`）＋ plugin context（`pre_llm_call` hook） |
| **注入先** | system prompt / 会話履歴 | Node として graph に追加 | system prompt | **ユーザーメッセージに注入**（system prompt 変更は prefix cache を破壊するため意図的に回避） |
| 注入タイミング | Heartbeat 実行前・アイドル後・日またぎ初回ターン | event-based state machine で自動トリガー | 毎リクエスト（GetMemoryContext） | `api_messages` 構築時、current_turn の user message に `build_memory_context_block()` で fence 付きテキスト化して注入 |

### 2.4 コンテキスト圧縮・削減

| | GeminiClaw | gemini-cli | PicoClaw | Hermes Agent |
|---|---|---|---|---|
| 戦略 | HEAD 70%＋TAIL 20%＋省略マーカー 10% | RollingSummary（LLM 要約）で古い複数ノードを 1 ノードに consolidate＋oldest drop | Safe boundary truncation（nearest user message を検索）、turn boundary 尊重 | 3 段階: ① tool output 1 行剪定 → ② head/tail 保護 → ③ 中盤を補助 LLM で要約＋セッション回転 |
| 保護範囲 | HEAD/TAIL 固定比率 | Protected node（削除禁止 ID） | turn boundary 尊重 | `protect_first_n=3`（system prompt＋最初の交換）＋ `protect_last_n=20`（または tail_token_budget: threshold の 20%） |
| 要約予算 | max 3000 chars | Token cache O(1) lookup | Shared tokenizer で事前 overflow 検知 | `max_summary_tokens = min(context_length × 5%, 12,000)` |
| 競合制御 | — | — | — | SQLite ベースのロック（`pid:tid:agent-instance:uuid`）。複数エージェント共有時の二重圧縮を防止 |
| Skill 剪定 | — | — | — | **Skill GC**（別レイヤー）: 類似 Skill をコンパクション・30 日ヒットなし Skill を削除（daily cron） |

### 2.5 セッション継続

| | GeminiClaw | gemini-cli | PicoClaw | Hermes Agent |
|---|---|---|---|---|
| 方式 | 前日 TLDR＋Topics を初回ターンのみ inject。Idempotency: 既 entries あれば skip | Consolidated summary node が old nodes を abstract | Session clear/reset、AssembleResponse で summary 返却 | FTS5 cross-session recall + Honcho user modeling。圧縮 DAG lineage で圧縮前後の文脈を辿れる |
| バックフィル | サーバー起動時に 7 日分を自動生成 | Pristine graph 保持で rollback 可能 | — | 圧縮時に補助モデルで要約生成 → 新セッションの system prompt に引き継ぎ |

---

## 2.6 Hermes Agent — context 管理の実装アーキテクチャ

### 二層構造

Hermes Agent は「セッション内圧縮」と「長期 Skill 結晶化」を**独立した二層**で実装する。

```
Layer A: セッション内コンテキスト圧縮（conversation_compression.py + context_compressor.py）
  ┌──────────────────────────────────────────────────────────┐
  │ [通常ターン] → API 呼び出し → token 使用量追跡            │
  │   ↓ threshold（context_length × 50%）到達                │
  │ [3 段階圧縮]                                              │
  │   ① tool output 剪定: 古い出力 → 1 行要約                │
  │   ② head 保護: system prompt + 最初の 3 交換             │
  │   ③ 中盤 LLM 要約: 補助モデルで構造化サマリー生成        │
  │   ↓ 圧縮完了                                             │
  │ [セッション回転] SQLite: 旧 session → "compression" で終了│
  │              新 session ID を発行・system prompt を再構築 │
  └──────────────────────────────────────────────────────────┘

Layer B: 長期 Skill 結晶化（AuditorWorker）
  ┌──────────────────────────────────────────────────────────┐
  │ [Post-run] tool 実行ログを 3 条件で審査                   │
  │   ① 成功か ② 汎用パターンか ③ 複数ステップか            │
  │   ↓ 条件を満たした場合                                   │
  │ [結晶化] CREATE（新規）または SEARCH/REPLACE パッチ       │
  │         で skills/ に Skill を永続化                     │
  │   ↓ 次回以降                                             │
  │ [注入] ContextBuilder が RAG top-N で関連 Skill を発見    │
  │       → system prompt に注入（= 長い試行錯誤不要）        │
  └──────────────────────────────────────────────────────────┘
```

### Prefix cache 最適化（Hermes 固有の工夫）

```python
# conversation_loop.py
# メモリをシステムプロンプトではなくユーザーメッセージに注入
if idx == current_turn_user_idx and msg.get("role") == "user":
    # memory prefetch 結果を fence 付きテキストとして user message に追加
    memory_block = build_memory_context_block(_ext_prefetch_cache)
    # → システムプロンプトのバイト完全性を保ち Anthropic prefix cache を再利用
```

**理由**: system prompt を変えると Anthropic の prefix cache がミスになり毎ターン full token 課金。ユーザーメッセージに注入することでキャッシュプリフィックスを維持しながら動的コンテキストを実現。

### ContextCompressor の予算計算

```python
# context_compressor.py
threshold_tokens = context_length * 0.50      # 圧縮開始閾値
tail_token_budget = threshold_tokens * 0.20   # tail 保護予算
max_summary_tokens = min(context_length * 0.05, 12_000)  # 要約上限
```

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

### ContextCompressor 予算配分（Hermes Agent）
```python
# context_compressor.py
target_tokens = int(self.threshold_tokens * self.summary_target_ratio)  # 圧縮後目標
self.max_summary_tokens = min(
    int(self.context_length * 0.05), _SUMMARY_TOKENS_CEILING,  # = 12,000
)
# protect_first_n=3: system prompt + 最初の 3 交換を保護
# protect_last_n=20: 最新 20 メッセージを保護（token 予算方式が優先）
```

### メモリのユーザーメッセージ注入（Hermes Agent）
```python
# conversation_loop.py
if idx == current_turn_user_idx and msg.get("role") == "user":
    # system prompt ではなく user message に注入 → prefix cache 維持
    memory_ctx = build_memory_context_block(agent._ext_prefetch_cache)
    plugin_ctx = agent._plugin_user_context
    # → api_messages に fence 付きブロックとして追加
```

---

## 4. RustyClaw v0.4 への取り込み計画

### 高優先度（v0.4 残課題）

| 施策 | 参照 upstream | 実装場所 | 概要 |
|---|---|---|---|
| **Heartbeat Digest 増分生成** | GeminiClaw `heartbeat-digest.ts` | `rustyclaw-gateway/src/heartbeat.rs` | `lastRunTimestamp` 以降のみスキャン。6 回毎 deep scan（24h）。max 3000 chars 制限 |
| **ContextBuilder context window 対応** | gemini-cli `contextTokenCalculator.ts`＋PicoClaw budget logic | `rustyclaw-agent/src/context.rs` | `LlmConfig` から window サイズ取得 → 70/20/10 予算計算 → turn boundary 尊重 truncation |
| **Session-level Summary** | GeminiClaw `summary.ts` / Hermes `conversation_compression.py` | `rustyclaw-gateway/src/lib.rs` | アイドル 5 分後に LLM で title＋tldr＋topics 生成 → `try_ctx_index` でエピソード記憶登録（fail-open） |

### 中優先度（v0.5 以降）

| 施策 | 参照 upstream | 概要 |
|---|---|---|
| **Session continuation 日付回転** | GeminiClaw `continuation.ts` | channel 毎の日付回転 session、前日 TLDR を初回ターン inject |
| **RollingSummary ノード統合** | gemini-cli `rollingSummaryProcessor.ts` | 古い複数 session を LLM で 1 ノードに consolidate |
| **Prefix cache 最適化** | Hermes Agent `conversation_loop.py` | メモリ・動的コンテキストをシステムプロンプトでなくユーザーメッセージに注入（Anthropic 利用時に有効） |
| **Multi-strategy ContextManager** | PicoClaw interface | Registry pattern で複数実装を plug-in 可能に |
| **Hermes Skill GC の本格稼働** | Hermes Agent AuditorWorker + `06_hermes_skills.md §12.7` | `daily-summary` cron に Skill コンパクション・30 日ヒットなし忘却を統合（`ctx_execute` + `ctx_patch` で実装可能） |

---

## 5. RustyClaw との設計上の最大の違い

| 観点 | GeminiClaw/gemini-cli | PicoClaw | Hermes Agent | RustyClaw v0.4 |
|---|---|---|---|---|
| 会話状態の管理主体 | Gemini CLI native `always-resume`（履歴注入不要） | per-session JSONL + ContextManager | SQLite sessions + DAG lineage（圧縮でセッション回転） | JSONL → `messages[]` 注入（gmn 依存の限り変更不可） |
| エピソード記憶 | Node graph ＋ SQLite（gemini-cli）、BM25 内製（GeminiClaw） | BM25＋PromptContributor | FTS5 cross-session recall + Honcho user modeling | context-mode の BM25/FTS5 に完全委譲 |
| 圧縮の実行主体 | Rust 側で token 計算・truncation | budget-aware truncation | ContextCompressor（セッション回転）＋ Skill GC（知識剪定）の二層 | Rust 側で budget 判断 → context-mode インデックスで補完 |
| 動的コンテキストの注入先 | system prompt / 会話履歴 | system prompt | **ユーザーメッセージ**（prefix cache 最適化） | system prompt（Anthropic prefix cache 未活用） |
| 長期知識の永続化 | MEMORY.md（人手管理）+ daily summary | MEMORY.md + daily notes | **Skill 結晶化（自律生成・GC）** | MEMORY.md（人手）+ `ctx_index` セッション要約 |

---

> **次のステップ**: このドキュメントを基に `docs/plans/` 配下に実装計画書を作成し、Heartbeat Digest → ContextBuilder window 対応 → Session Summary の順で段階実装する。
