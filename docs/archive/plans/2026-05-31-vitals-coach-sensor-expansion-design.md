# Design: Vitals Coach Sensor Expansion

**Date**: 2026-05-31  
**Scope**: `production/workspace/skills/vitals-coach/`

---

## Overview

Expand the vitals-coach skill's data acquisition from 13 to 27 Garmin sensor fields, and restructure SKILL.md Step 2 into a two-table layout that explicitly separates alert-threshold metrics from context-reference metrics.

---

## Change Scope

| File | Change |
|---|---|
| `scripts/500_get-vital-data-garmin.sh` | Add 14 fields to jq filter (13 → 27 total) |
| `SKILL.md` | Restructure Step 2 into two tables; fix stale "7 core fields" mention |

`workspace/skills/vitals-coach.md` (legacy) is out of scope for this change.

---

## Script Changes

Add the following 14 fields to the jq filter in `500_get-vital-data-garmin.sh`:

**Stress detail**
- `Garmin Connect Max stress level`
- `Garmin Connect Low stress duration`
- `Garmin Connect Medium stress duration`
- `Garmin Connect Activity stress duration`

**Body Battery detail**
- `Garmin Connect Body battery charged`
- `Garmin Connect Body battery drained`
- `Garmin Connect Body battery lowest`

**Sleep detail**
- `Garmin Connect Light sleep`
- `Garmin Connect Awake time`
- `Garmin Connect Bedtime`

**Activity**
- `Garmin Connect Active time`
- `Garmin Connect Intensity minutes`
- `Garmin Connect Yesterday steps`
- `Garmin Connect Weekly step average`

---

## SKILL.md Step 2 Restructure

Replace the single metric table with two tables.

### Alert Evaluation Table
Fields with explicit thresholds. LLM must address every threshold violation in the coaching output.

| Metric | Alert Threshold | Coaching Strategy (Japanese) |
|---|---|---|
| Body battery | < 20 % | 休息優先、早めの就寝を提案 |
| Body battery highest | < 40 % | 慢性疲労の蓄積として言及 |
| Average stress level | > 50 | 深呼吸・休憩を推奨 |
| High stress duration | > 90 min | こまめな休憩を促す |
| Max stress level | > 80 | ピーク過負荷を指摘 |
| Low stress duration | < 120 min | リラックス時間の確保を促す |
| Resting heart rate | > 70 bpm | 疲労サインとして無理な活動を避けるよう提案 |
| Steps | < Daily step goal | 軽い散歩・ストレッチを提案 |
| Sedentary time | > 600 min | 1時間ごとに立ち上がることを提案 |
| Active time | < 30 min | 最低限の活動量確保を促す |
| Intensity minutes | < 20 min | 有酸素活動の追加を提案 |
| Sleep duration | < 360 min | 睡眠不足として昼寝・改善を提案 |
| Deep sleep | < 60 min | 身体回復の低下、早寝・環境見直しを提案 |
| REM sleep | < 90 min | 精神疲労に直結、ストレス軽減・就寝前リラックスを推奨 |
| Awake time | > 30 min | 睡眠中断が多い、就寝環境の見直しを提案 |

### Context Reference Table
Fields without fixed thresholds. LLM uses these as contextual signals combined with other data.

| Metric | Usage |
|---|---|
| Body battery charged | drained / lowest との差分で回復効率を評価 |
| Body battery drained | 日中消費量のパターン把握 |
| Body battery lowest | 今日の最低到達点の把握 |
| Activity stress duration | 運動由来ストレスの除外判断 |
| Medium stress duration | Low / High との比率でストレス構造を把握 |
| Light sleep | Deep / REM 比率との組み合わせで睡眠構造を評価 |
| Bedtime | Wake time との組み合わせで睡眠習慣を把握（値は JST、UTC 変換不要） |
| Yesterday steps | 今日との比較でトレンド判断 |
| Weekly step average | 活動習慣の長期トレンド把握 |
| Wake time | 起床時刻の把握（JST そのまま、UTC 変換不要） |
| Daily step goal | Steps のアラート基準値として参照 |
| Last synced | データ鮮度の検証（30分超で警告） |

---

## Fixes Included

- `SKILL.md` の "The script already filters to 7 core fields" → "27 core fields" に修正
- `SKILL.md` の Step 1 "13 core fields" → "27 core fields" に修正

---

## Out of Scope

- `workspace/skills/vitals-coach.md` の廃止・削除（別途対応）
- コーチング出力フォーマットの変更
- cron スケジュール・配信先の変更
