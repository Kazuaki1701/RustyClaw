# Topic Patrol Deliver — SKILL.md 精査レポート

**日付**: 2026-06-04  
**対象**: `production/workspace/skills/topic-patrol/SKILL.md`（配信モード関連）  
**契機**: Heartbeat/Topic Patrol 分離対応後の LLM リクエスト品質点検

---

## 問題一覧

### 🔴 P1-A: 探索件数の数値不整合（今日の変更による）

**概要**: Step 1 を「3件」に変更したが、以下3箇所が「2」のまま残存。

| 箇所 | 現状テキスト | あるべき値 |
|---|---|---|
| Step 2 ヘッダー | `each of the 2 selected topics` | `3` |
| Step 2 Work-adjacent | `After investigating the 2 selected topics` | `3` |
| Prohibited Patterns 末尾 | `Picking the same 2 topics` | `3` |

**影響**: 探索モード実行時に「3件選べ」と「2件ずつ処理せよ」が矛盾し、モデルが件数を誤って解釈する可能性がある。

---

### 🔴 P1-B: KaraKeep 二重登録リスク

**概要**: 配信モード（`配信: 許可`）において KaraKeep 登録が2箇所に定義されている。

- **Deliver Mode Step 7**: `511_karakeep-add-bookmark.sh` を明示呼び出し
- **Step 5-2**: `配信: 許可` かつ共有済みの場合に同スクリプトを呼び出す

Deliver Mode Step 8 に「Skip Steps 1–5」とあるが、モデルが Step 5-2 も実行した場合、同一 URL が二重登録される。

**改善案**:
- KaraKeep 登録は Deliver Mode Step 7 に一本化する
- Step 5-2 の適用条件を「探索モード（`配信: スキップ`）かつ共有済み」に限定する、または Step 5-2 を削除する

---

### 🟡 P2-A: Execution Flow と Deliver Mode の参照矛盾

**概要**: 配信モードの終了後の動作指示が2箇所で食い違っている。

- **Execution Flow**: 「`配信: 許可` → follow Deliver Mode, then **skip to Step 5**」
- **Deliver Mode Step 8**: 「Go to **Step 5-3**. **Skip Steps 1–5**」

「Step 5 に進め」と「Steps 1–5 をスキップ」が同時に記述されており、Step 5-0（prune）・Step 5-1（append）を実行するかどうかが不明確。

**配信モードでの正しい動作（意図）**:
- Step 5-0（prune）: 実行不要（新規エントリを追記しないため）
- Step 5-1（append）: 実行不要（`delivered` 記録は Deliver Mode Step 6 で行う）
- Step 5-2（KaraKeep）: Deliver Mode Step 7 に移管
- Step 5-3（state.json）: 実行が必要

**改善案**: Execution Flow と Deliver Mode Step 8 を「Step 5-3 のみ実行して終了」に統一する。

---

### 🟡 P2-B: `配信: スキップ (quiet hours)` の表記ズレ

**概要**: Step 4 に `配信: スキップ (quiet hours)` と記述されているが、cron.json の実際のプロンプトは `配信: スキップ`（括弧なし）。

```
# cron.json 実際の値
"prompt": "Run the topic-patrol skill.\n\n配信: スキップ"
```

モデルが文字列照合で分岐判断を行う場合、`(quiet hours)` の有無でマッチしない恐れがある。

**改善案**: Step 4 の表記を `配信: スキップ` に統一する。

---

### 🟡 P2-C: 「not delivered yet」の判定基準が未定義

**概要**: Deliver Mode Step 2 に「`deferred (quiet hours)` that have **not** been delivered yet」とあるが、「配信済み」の判断基準が明示されていない。

`patrol/findings.md` 内の `delivered` ステータス文字列で判断するのか、日付で判断するのかが不明確であり、モデルが過去の delivered エントリを誤って再配信する可能性がある。

**改善案**: 「同一 `findings.md` 内に `delivered` ステータスで記録されているエントリは配信済みとみなす」と明記する。

---

### 🟢 P3-A: Step 3（フィルター）への参照が配信モードで不適切

**概要**: Deliver Mode Step 3 が「"Would I tell a friend?" criteria (Step 3 below)」を参照しているが、Step 3 は探索モード（web_fetch 後の新規コンテンツ評価）用に書かれたセクション。配信モードでは web 探索を行わないため文脈がずれている。

**改善案**: 配信モード内に「deferred エントリから面白さで上位 1–2 件を選ぶ」という独立した選択基準を直接記述し、Step 3 への参照を廃止する。

---

## 修正優先度サマリ

| ID | 優先度 | 修正内容 | 影響範囲 |
|---|---|---|---|
| P1-A | 🔴 必須 | Step 2・Prohibited の件数を 2→3 に修正 | 探索モード |
| P1-B | 🔴 必須 | KaraKeep 登録を Deliver Mode に一本化 | 配信モード |
| P2-A | 🟡 推奨 | Execution Flow と Step 8 の参照を Step 5-3 に統一 | 配信モード |
| P2-B | 🟡 推奨 | Step 4 の `配信: スキップ (quiet hours)` → `配信: スキップ` | 探索モード |
| P2-C | 🟡 推奨 | 「配信済み」判定基準を Deliver Mode Step 2 に明記 | 配信モード |
| P3-A | 🟢 任意 | 配信モードの選択基準を独立記述し Step 3 参照を廃止 | 配信モード |
