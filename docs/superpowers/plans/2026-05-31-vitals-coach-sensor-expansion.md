# Vitals Coach Sensor Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the vitals-coach skill's Garmin data acquisition from 13 to 27 fields, and restructure SKILL.md Step 2 into alert-threshold and context-reference tables.

**Architecture:** Two files change: the jq filter in `500_get-vital-data-garmin.sh` gains 14 fields, and `SKILL.md` Step 2 is replaced with a two-table structure. No other files are touched.

**Tech Stack:** bash, jq, Home Assistant REST API, Markdown

---

## File Map

| Action | Path |
|---|---|
| Modify | `production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh` |
| Modify | `production/workspace/skills/vitals-coach/SKILL.md` |

---

## Task 1: Expand jq filter in the retrieval script

**Files:**
- Modify: `production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh`

- [ ] **Step 1: Verify current output has exactly 13 keys**

```bash
source /mnt/qnap/DESKTOP/Kazuaki/Documents/Projects/dotfiles/.shell.d/70_secrets.sh
bash production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh | jq 'keys | length'
```
Expected: `13`

- [ ] **Step 2: Replace the jq filter to include all 27 fields**

Replace the entire jq block at the end of `production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh` (lines 12–26):

```bash
     http://192.168.1.30:8123/api/template | jq '{
  "Garmin Connect Body battery":            .["Garmin Connect Body battery"],
  "Garmin Connect Body battery highest":    .["Garmin Connect Body battery highest"],
  "Garmin Connect Body battery charged":    .["Garmin Connect Body battery charged"],
  "Garmin Connect Body battery drained":    .["Garmin Connect Body battery drained"],
  "Garmin Connect Body battery lowest":     .["Garmin Connect Body battery lowest"],
  "Garmin Connect Average stress level":    .["Garmin Connect Average stress level"],
  "Garmin Connect High stress duration":    .["Garmin Connect High stress duration"],
  "Garmin Connect Max stress level":        .["Garmin Connect Max stress level"],
  "Garmin Connect Low stress duration":     .["Garmin Connect Low stress duration"],
  "Garmin Connect Medium stress duration":  .["Garmin Connect Medium stress duration"],
  "Garmin Connect Activity stress duration":.["Garmin Connect Activity stress duration"],
  "Garmin Connect Resting heart rate":      .["Garmin Connect Resting heart rate"],
  "Garmin Connect Steps":                   .["Garmin Connect Steps"],
  "Garmin Connect Daily step goal":         .["Garmin Connect Daily step goal"],
  "Garmin Connect Sedentary time":          .["Garmin Connect Sedentary time"],
  "Garmin Connect Active time":             .["Garmin Connect Active time"],
  "Garmin Connect Intensity minutes":       .["Garmin Connect Intensity minutes"],
  "Garmin Connect Yesterday steps":         .["Garmin Connect Yesterday steps"],
  "Garmin Connect Weekly step average":     .["Garmin Connect Weekly step average"],
  "Garmin Connect Sleep duration":          .["Garmin Connect Sleep duration"],
  "Garmin Connect Deep sleep":              .["Garmin Connect Deep sleep"],
  "Garmin Connect REM sleep":               .["Garmin Connect REM sleep"],
  "Garmin Connect Light sleep":             .["Garmin Connect Light sleep"],
  "Garmin Connect Awake time":              .["Garmin Connect Awake time"],
  "Garmin Connect Bedtime":                 .["Garmin Connect Bedtime"],
  "Garmin Connect Wake time":               .["Garmin Connect Wake time"],
  "Garmin Connect Last synced":             .["Garmin Connect Last synced"]
}'
```

- [ ] **Step 3: Verify output now has exactly 27 keys**

```bash
source /mnt/qnap/DESKTOP/Kazuaki/Documents/Projects/dotfiles/.shell.d/70_secrets.sh
bash production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh | jq 'keys | length'
```
Expected: `27`

- [ ] **Step 4: Verify all 14 new keys are present**

```bash
source /mnt/qnap/DESKTOP/Kazuaki/Documents/Projects/dotfiles/.shell.d/70_secrets.sh
bash production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh | jq '[
  "Garmin Connect Max stress level",
  "Garmin Connect Low stress duration",
  "Garmin Connect Medium stress duration",
  "Garmin Connect Activity stress duration",
  "Garmin Connect Body battery charged",
  "Garmin Connect Body battery drained",
  "Garmin Connect Body battery lowest",
  "Garmin Connect Light sleep",
  "Garmin Connect Awake time",
  "Garmin Connect Bedtime",
  "Garmin Connect Active time",
  "Garmin Connect Intensity minutes",
  "Garmin Connect Yesterday steps",
  "Garmin Connect Weekly step average"
] | map(. as $k | $k) | length'
```
Expected: `14`（すべてのキーが jq に通ったことの確認。値が null でも可）

- [ ] **Step 5: Commit**

```bash
git add production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh
git commit -m "feat(vitals-coach): expand sensor output from 13 to 27 fields"
```

---

## Task 2: Restructure SKILL.md Step 1 のフィールド数記述を修正

**Files:**
- Modify: `production/workspace/skills/vitals-coach/SKILL.md`

- [ ] **Step 1: Fix field count in Step 1 description (line 60)**

現在:
```
The script returns **only the 13 core fields** listed in the table above. Do not attempt to parse other fields — they are not returned.
```

変更後:
```
The script returns **only the 27 core fields** listed in the tables below. Do not attempt to parse other fields — they are not returned.
```

- [ ] **Step 2: Fix stale "7 core fields" in Common Mistakes (line 91)**

現在:
```
*   **Raw Data Dumping**: The script already filters to 7 core fields. Do not modify the script to output all sensors.
```

変更後:
```
*   **Raw Data Dumping**: The script already filters to 27 core fields. Do not modify the script to output all sensors.
```

- [ ] **Step 3: Commit**

```bash
git add production/workspace/skills/vitals-coach/SKILL.md
git commit -m "fix(vitals-coach): correct stale field count references (7/13 → 27)"
```

---

## Task 3: Restructure SKILL.md Step 2 into two-table layout

**Files:**
- Modify: `production/workspace/skills/vitals-coach/SKILL.md`

- [ ] **Step 1: Replace Step 2 content**

`SKILL.md` の `### Step 2: Filtration & Threshold Analysis (Level 2)` セクション（line 62〜77）全体を以下に置き換える：

```markdown
### Step 2: Filtration & Threshold Analysis (Level 2)

Evaluate the data in two passes.

#### Alert Evaluation — fields with explicit thresholds
Address **every** threshold violation in the coaching output.

| Metric | Alert Threshold | Coaching Strategy (Japanese) |
| :--- | :--- | :--- |
| **`Garmin Connect Body battery`** | < 20 % | 激しい活動を控え、早めの就寝や休息を優先するよう提案。 |
| **`Garmin Connect Body battery highest`** | < 40 % | 1日の最高値が低い場合、慢性的な疲労蓄積として言及。 |
| **`Garmin Connect Average stress level`** | > 50 | 深呼吸、スクリーンフリー時間、または軽い休憩を推奨。 |
| **`Garmin Connect High stress duration`** | > 90 min | 長時間の高ストレス状態を具体的に指摘し、こまめな休憩を促す。 |
| **`Garmin Connect Max stress level`** | > 80 | ピーク過負荷を指摘し、翌日の活動量を抑えるよう提案。 |
| **`Garmin Connect Low stress duration`** | < 120 min | リラックス時間の確保を促す。意識的な休息を提案。 |
| **`Garmin Connect Resting heart rate`** | > 70 bpm | 安静時心拍の上昇は疲労・体調不良のサイン。無理な活動を避けるよう提案。 |
| **`Garmin Connect Steps`** | Under `Daily step goal` | 軽いストレッチや散歩を提案。 |
| **`Garmin Connect Sedentary time`** | > 600 min | 長時間の座りっぱなしを指摘し、1時間ごとに立ち上がることを提案。 |
| **`Garmin Connect Active time`** | < 30 min | 最低限の活動量が不足している旨を指摘し、軽い運動を提案。 |
| **`Garmin Connect Intensity minutes`** | < 20 min | 有酸素活動の追加を提案（例：速歩き15分）。 |
| **`Garmin Connect Sleep duration`** | < 360 min | 睡眠不足を指摘し、短時間の昼寝や就寝環境の改善をアドバイス。 |
| **`Garmin Connect Deep sleep`** | < 60 min | 深睡眠の不足は身体回復の低下を意味する。早めの就寝・寝室環境の見直しを提案。 |
| **`Garmin Connect REM sleep`** | < 90 min | REM 不足は精神的疲労に直結。ストレス軽減・就寝前のリラックスを推奨。 |
| **`Garmin Connect Awake time`** | > 30 min | 睡眠中の覚醒が多い。就寝環境（温度・光・音）の見直しを提案。 |

#### Context Reference — fields without fixed thresholds
Use these as contextual signals combined with other data. No threshold-based alert required.

| Metric | Usage |
| :--- | :--- |
| **`Garmin Connect Body battery charged`** | `drained` / `lowest` との差分で睡眠中の回復効率を評価。 |
| **`Garmin Connect Body battery drained`** | 日中消費量のパターン把握。charged との差が大きい場合は過負荷を示唆。 |
| **`Garmin Connect Body battery lowest`** | 今日の最低到達点。current との差で回復傾向を確認。 |
| **`Garmin Connect Activity stress duration`** | 運動由来のストレス。High stress duration の内訳評価に使用（運動ならポジティブ）。 |
| **`Garmin Connect Medium stress duration`** | Low / High との比率でストレス構造を把握。 |
| **`Garmin Connect Light sleep`** | Deep / REM 比率との組み合わせで睡眠構造を評価。 |
| **`Garmin Connect Bedtime`** | `Wake time` との組み合わせで睡眠習慣を把握。値は **JST そのまま**（UTC 変換不要）。 |
| **`Garmin Connect Yesterday steps`** | 今日の Steps と比較してトレンドを判断。 |
| **`Garmin Connect Weekly step average`** | 活動習慣の長期トレンド把握。Daily step goal との乖離を確認。 |
| **`Garmin Connect Wake time`** | 起床時刻の把握。値は **JST そのまま**（`+00:00` サフィックスは誤表記、UTC 変換不要）。 |
| **`Garmin Connect Daily step goal`** | `Steps` のアラート基準値として参照。 |
| **`Garmin Connect Last synced`** | データ鮮度の検証。30分超で同期遅延警告を出す（真の UTC → JST に +9時間変換して計算）。 |
```

- [ ] **Step 2: Verify the file renders correctly (no broken markdown)**

```bash
grep -c "^|" production/workspace/skills/vitals-coach/SKILL.md
```
Expected: 30 以上（テーブル行が存在することを確認）

- [ ] **Step 3: Verify "Step 2" section heading is intact**

```bash
grep "Step 2" production/workspace/skills/vitals-coach/SKILL.md
```
Expected: `### Step 2: Filtration & Threshold Analysis (Level 2)` が1行含まれる

- [ ] **Step 4: Commit**

```bash
git add production/workspace/skills/vitals-coach/SKILL.md
git commit -m "feat(vitals-coach): restructure Step 2 into alert/context two-table layout"
```

---

## Task 4: 最終動作確認

- [ ] **Step 1: スクリプトを実行して全27フィールドが返ることを確認**

```bash
source /mnt/qnap/DESKTOP/Kazuaki/Documents/Projects/dotfiles/.shell.d/70_secrets.sh
bash production/workspace/skills/vitals-coach/scripts/500_get-vital-data-garmin.sh | jq 'keys'
```
Expected: 27 キーがアルファベット順に表示される。`null` 値のキーが含まれていても可（HA 未同期センサー）。

- [ ] **Step 2: SKILL.md に "7 core fields" または "13 core fields" が残っていないことを確認**

```bash
grep -n "7 core\|13 core" production/workspace/skills/vitals-coach/SKILL.md
```
Expected: 出力なし（0件）

- [ ] **Step 3: SKILL.md に "27 core fields" が2箇所あることを確認**

```bash
grep -n "27 core" production/workspace/skills/vitals-coach/SKILL.md
```
Expected: 2行ヒット（Step 1 の説明文 と Common Mistakes）
