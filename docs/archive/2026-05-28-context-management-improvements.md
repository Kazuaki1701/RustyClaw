# Context Management 改善計画

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-05-28  
> **備考**: 最新の仕様は `docs/specs/02_agent_pipeline.md` および `docs/specs/04_heartbeat_spec.md` に反映されました。


---

## 1. 分析サマリー：GeminiClaw のコンテキスト設計

### 1-1. Static / Dynamic 2層構造

GeminiClaw は LLM に渡すコンテキストを2種類に分離している。

| 種別 | 内容 | 更新頻度 |
|------|------|----------|
| **Static（GEMINI.md）** | `@SOUL.md @AGENTS.md @USER.md @MEMORY.md` の `@import` | 起動時1回だけ書き込み |
| **Dynamic（`-p` 引数）** | セッション継続・チャンネル文脈・Runtime Directives | 毎呼び出し |

- Gemini CLI が `@import` を自身でファイル解決する → アプリ側でファイルを毎回読む必要なし
- 並列セッションが GEMINI.md を競合書き込みするリスクがない

**RustyClaw 現状**: `build_system_context()` が毎呼び出しで SOUL.md/AGENTS.md/USER.md/MEMORY.md を読み込み、1つのシステムプロンプトに連結している。アーキテクチャは異なるが機能は同等。

---

### 1-2. 会話状態の管理主体（最大の違い）

| | GeminiClaw | RustyClaw |
|---|---|---|
| **会話履歴の保持者** | Gemini CLI（`always-resume` ネイティブ） | RustyClaw（JSONL → `messages[]` に毎回注入） |
| **JSONL の役割** | 監査ログのみ | 履歴注入の源泉 |
| **コンテキスト肥大化リスク** | ない（CLI が圧縮責任を持つ） | ある（`compact_if_needed(4000)` で抑制中） |

---

### 1-3. 5つの圧縮・断絶防止レイヤー

```
時間軸
 ← 長期 ────────────────────────────── 直近 →

MEMORY.md           Session Summary    Heartbeat Digest
（永続的事実）       （セッション単位    （直近アクティビティ
                     TL;DR + topics）   ≤3000文字）
      ↑                   ↑                   ↑
 Memory Flush        Summary Generator  generate_digest()
（full agent、       （増分更新あり）   （増分スキャン、
 append-only）                          deep scan×6回毎）

                     Session JSONL（監査ログ）
                     └─ truncateBefore() で Daily Summary 後に削減
```

**多層設計の意義**: どれか一つが失敗しても他のレイヤーで補完できる。記録の「断絶」が起きにくい。

---

### 1-4. Heartbeat Digest の実装（GeminiClaw）

`heartbeat-digest.ts` の動作:
1. `heartbeat-state.json` から `lastRunTimestamp` と `runCount` を読む
2. `sessionsDir` 内の `.jsonl` ファイルのうち、`lastRunTimestamp` 以降に更新されたものだけを読む（増分）
3. 6回に1回は24時間分の deep scan を実施（取りこぼし防止）
4. 最大 3000 文字に収め、古いエントリを切り捨てる
5. `memory/heartbeat-digest.md` に書き込む
6. `heartbeat-state.json` の `lastRunTimestamp` と `runCount` を更新

これにより、Heartbeat LLM は「最近ユーザーが何をしていたか」を compact な形で把握できる。

---

### 1-5. Session Summary の増分更新（GeminiClaw）

```typescript
// 既存サマリーがある場合
if (existing) {
    const newEntries = entries.slice(existing.turns);
    callLlmForIncrementalSummary(existing.tldr, newEntries)
}
// なければフルサマリー生成
```

同日にセッションが再開された場合、既存 TL;DR + 新規エントリのみで差分更新。全履歴の再処理なし。

---

## 2. RustyClaw との比較表

| 機能 | GeminiClaw | RustyClaw（現状） | 差分 |
|------|-----------|-----------------|------|
| 会話状態 | Gemini CLI native `always-resume` | JSONL → messages[] 注入 | **根本差異**（gmn 依存限り変更不可） |
| Static/Dynamic 分離 | GEMINI.md `@import` | 毎回ファイル読み込み | アーキテクチャ差だが機能同等 |
| Session Continuation | 初回ターンのみ、サマリーベース | 初回ターンのみ、サマリーベース | ほぼ同等 ✓ |
| Memory Flush | full agent、append-only | `--no-agent`、全書き直し | 方式が異なる |
| Session Summary | セッション単位、増分更新あり | Daily Summary のみ | **RustyClaw 不足** |
| Heartbeat Digest | 増分スキャン・deep scan、≤3000文字 | ステートレス（コンテキストなし） | **RustyClaw 不足** |
| JSONL 削減 | `truncateBefore()` で Daily Summary 後削減 | 削減機構なし | **RustyClaw 不足** |
| 70/20 トランケート | `truncateWithContext()` あり | `truncate_70_20()` あり | 同等 ✓ |

---

## 3. 取り込み改善案（優先度順）

### ★★★ A. Heartbeat Digest の真の実装

**現状**: Phase 7 修正でステートレス化（history を渡さない）したが、heartbeat が最近のユーザー活動を全く把握できていない状態。

**改善案**:
- `HeartbeatService` の実行前に `generate_heartbeat_digest()` を呼ぶ
- `heartbeat-state.json` の `lastRunTimestamp` / `runCount` を Rust で管理
- 増分スキャン: 前回実行以降に更新された sessions/ の JSONL のみ読む
- 6回毎に deep scan（24時間分）
- 最大 3000 文字、Heartbeat プロンプトに追記

**影響範囲**: `rustyclaw-gateway/src/lib.rs`（`HeartbeatService`）+ 新規 `generate_digest()` 関数

**期待効果**: Heartbeat が「最近ユーザーが何をしていたか」を把握し、proactive 投稿の精度向上

---

### ★★★ B. Session-level Summary の実装

**現状**: Daily Summary のみ。Session Continuation は daily summary が存在しない場合に前日 JSONL の末尾5件しか使えない。

**改善案**:
- 会話がアイドル（5分以上更新なし）になったときにセッション単位のサマリーを生成
- 出力: `memory/summaries/<date>-<slug>.md`（TL;DR + topics + decisions）
- Session Continuation の精度向上（翌日の継続が構造化情報をもとに行える）
- `cron:` セッションはスキップ

**実装形態**: Gateway の CronService または独立した `SummaryService` として実装

**期待効果**: 日またぎのコンテキスト継続品質が向上

---

### ★★ C. JSONL 削減（truncateBefore）

**現状**: Discord セッションの JSONL が削減されない（無制限成長）。

**改善案**:
- Daily Summary 生成後、30日以上前のエントリを JSONL から削除
- または Session Summary 生成後、その会話分の古いエントリを削除

**影響範囲**: `rustyclaw-storage` の `SessionLogger`

---

### ★★ D. Session Summary の増分更新

**現状**: Session Summary を再生成する場合、全エントリを LLM に渡す。

**改善案**:
- 既存サマリーの `turns` カウントと JSONL の行数を比較
- 差分エントリ + 既存 TL;DR のみで更新

---

### ★（長期・gmn 脱却後）E. Static/Dynamic コンテキスト分離

GeminiClaw の `@import` 方式は gmn の Gemini CLI 機能に依存。`rustyclaw-mcp`（Phase 7）で直接 API 呼び出しに移行後に検討。

---

## 4. 実装優先度の根拠

| 案 | 実装コスト | 効果 | 依存 |
|---|---|---|---|
| A. Heartbeat Digest | 中（新規関数1つ + HeartbeatService 修正） | 高（proactive 精度向上） | なし |
| B. Session Summary | 中〜高（CronService 拡張 + summary 生成ロジック） | 高（継続品質向上） | なし |
| C. JSONL 削減 | 低（storage に1関数追加） | 中（ディスク節約） | B より後が望ましい |
| D. 増分 Summary 更新 | 低（B の延長） | 中（API コスト削減） | B が前提 |
| E. Static/Dynamic 分離 | 高（アーキテクチャ変更） | 低（現状でも機能） | Phase 7 前提 |

→ **次セッションでは A と B の実装方針を詳細検討する**

---

## 5. 参照ファイル（GeminiClaw）

| ファイル | 内容 |
|---------|------|
| `src/geminiclaw/src/agent/context-builder.ts` | Static/Dynamic 分離・`buildSessionContext()` |
| `src/geminiclaw/src/agent/session/continuation.ts` | Session Continuation 実装 |
| `src/geminiclaw/src/agent/session/store.ts` | JSONL store・`truncateBefore()` |
| `src/geminiclaw/src/agent/session/flush.ts` | Silent Memory Flush（full agent、append-only） |
| `src/geminiclaw/src/agent/session/summary.ts` | Session Summary・増分更新 |
| `src/geminiclaw/src/agent/session/heartbeat-digest.ts` | Heartbeat Digest・増分スキャン |

---

## 6. 関連ドキュメント

- `docs/specs/04_heartbeat_spec.md` — Heartbeat 設計仕様
- `docs/specs/02_agent_pipeline.md` — Pipeline・Memory Flush 仕様
- `docs/task.md` — Phase 8: 本計画の実装タスク
