# GeminiClaw vs RustyClaw — HEARTBEAT_OK ギャップ分析

**調査日:** 2026-06-03  
**背景:** RustyClaw で気象警報（大雨・洪水警報）が HEARTBEAT_OK と混在し Discord に届かない事象が発生。GeminiClaw の実装を参照してギャップを特定。

---

## 発生した問題

本日 09:10〜11:38 の Heartbeat 5件で、モデルが以下のような出力を生成：

```
⚠️ **重要：気象警報**
神奈川県および東京都において、洪水警報や大雨警報が発表されています。

📅 基本情報技術者試験（6/13）まで残り10日となりました。

HEARTBEAT_OK
```

コードが `HEARTBEAT_OK` を検出して Silent 処理 → 気象警報が Discord に届かなかった。

---

## ギャップ一覧

| # | 項目 | GeminiClaw | RustyClaw（修正前） | Priority |
|---|---|---|---|---|
| G1 | **判定ロジック** | `/HEARTBEAT_OK\s*$/m`（行末マッチ） | `.contains("heartbeat_ok")`（単純一致） | High |
| G2 | **`<think>` ブロック除外** | 判定前に除去 | なし | High |
| G3 | **マークダウン正規化** | `**HEARTBEAT_OK**` も検出 | なし | Medium |
| G4 | **プロンプト指示の整合性** | AGENTS.md・HEARTBEAT.md 両方で一貫 | HEARTBEAT.md が混在を許容していた | → 修正済み |
| G5 | **Heartbeat 統計集計** | okCount / actions を日次集計 | なし | Low |

---

## GeminiClaw の実装詳細

### 判定ロジック（runner.ts）

```typescript
// 1. <think>/<thought> ブロックを除外
const responseWithoutThink = this.result.responseText
    .replace(/<(think|thought)>[\s\S]*?<\/\1>/g, '');

// 2. マークダウン記号を除去して正規化
const normalized = responseWithoutThink.replace(/[*`~]/g, '');

// 3. 行末に HEARTBEAT_OK があるかチェック
this.result.heartbeatOk = /HEARTBEAT_OK\s*$/m.test(normalized);
```

**テストケース（全 pass）:**
- ✅ `HEARTBEAT_OK` のみ
- ✅ `Updated heartbeat-state.json.\nHEARTBEAT_OK`
- ✅ `**HEARTBEAT_OK**`
- ❌（false）`<think>HEARTBEAT_OK</think>Something needs attention.`

### プロンプト設計（AGENTS.md）

```markdown
| Severity   | Action                          | HEARTBEAT_OK? |
|------------|---------------------------------|---------------|
| Critical   | Include as alert text           | No            |
| Informational | Log to memory/logs/ only     | Yes           |
| Nothing    | —                               | Yes           |
```

Critical → HEARTBEAT_OK なし（Discord 送信）  
Critical-free → HEARTBEAT_OK のみ（Silent）

### Context builder による追加指示

```typescript
if (options.trigger === 'heartbeat') {
    parts.push(
        'Post all notifications via `geminiclaw_post_message` to the home channel. ' +
        'Always respond with `HEARTBEAT_OK` when done.',
    );
}
// Interactive Mode では明示的に禁止
parts.push('Do NOT respond with HEARTBEAT_OK under any circumstances.');
```

---

## RustyClaw への適用方針

### 対応済み

- **G4（プロンプト整合）:** HEARTBEAT.md Step 6 を修正（2026-06-03）
  - Critical/Informational/Nothing の3分類テーブルを追加
  - Critical 時は HEARTBEAT_OK を一切含めないことを明記

### 実装対象（G1〜G3）

→ 2026-06-03 実装済み（`heartbeat.rs:process_heartbeat_response`）

### 将来課題（G5）— Heartbeat 統計集計

**GeminiClaw の実装（daily-summary.ts）:**

```typescript
lines.push(`- ${heartbeat.okCount}x HEARTBEAT_OK`);
if (heartbeat.actions.length > 0) {
    lines.push(`- ${heartbeat.actions.length}x actions executed:`);
    heartbeat.actions.forEach(a => lines.push(`  - ${a}`));
}
```

日次サマリーに以下を追加している：
- `HEARTBEAT_OK` 回数（Silent 実行回数）
- Proactive actions 件数と内容一覧

**RustyClaw でのギャップ:**
- Heartbeat の実行結果は `memory/logs/YYYY-MM-DD.md` に個別記録されるが、集計はない
- daily-briefing や session-summary から Heartbeat の全体傾向が把握できない
- 実装案: `memory/heartbeat-activity.md` に日次集計を append、または daily-summary セッション内で Heartbeat ログを集計して Discord に報告

**実装時の考慮点:**
- 集計タイミング: 日次 cron（daily-briefing 実行時）が自然
- 保存先: `memory/heartbeat-activity.md`（既存）または `memory/logs/YYYY-MM-DD.md` への統計行追記
- okCount は既存ログファイルから grep 可能
