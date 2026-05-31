---
name: vitals-coach
description: Use when a user requests their current physical status, fatigue levels, Garmin heart rate, stress levels, sleep analytics, body battery, or personalized wellness coaching.
---

# Vitals Coach Skill

## Overview
Retrieves consumer-grade Garmin vital statistics via Home Assistant, enforces strict medical safety boundaries, validates data sync latency, and generates personalized, empathetic wellness coaching in Japanese.

---

## When to Use

### Triggering Symptoms / Scenarios:
- The user requests a health report, daily physical feedback, or sleep advice.
- The user reports feeling exhausted, physically fatigued, or unwell and mentions Garmin metrics.
- Daily scheduled vital patrols (e.g. `vitals-morning`, `vitals-night`) trigger (06:00 AM / 22:00 PM).

### When NOT to use:
- The user is experiencing an acute, severe medical emergency. (Prioritize emergency services immediately).
- General fitness chat not tied to specific Garmin metrics.

---

## Prerequisites & Endpoints

To retrieve vital statistics, all parameters are securely resolved from **RustyClaw's vault** (`~/.rustyclaw/vault.json`):
*   **Authentication**: HA Bearer Token resolved under key `homeassistant-token`.
*   **Endpoint Address**: `http://192.168.1.30:8123/api/template` (Home Assistant local API).

---

## The Core Safeguard Rules

### 1. Mandatory Medical Warning & Action
Garmin devices are consumer wearables, not clinical diagnostic tools. If the user reports feeling extremely unwell, you **MUST** prioritize the following warning *before* presenting any vital data:
*   **Seek Clinical Care Immediately**: Advise the user to contact emergency services (e.g. 119 in Japan) or visit the nearest hospital emergency room.
*   **Do Not Self-Diagnose**: Do not use consumer-grade metrics to make critical medical decisions.

### 2. Synchronization Latency Verification (Critical)
Always parse the `"Garmin Connect Last synced"` timestamp from the raw JSON payload.
*   **Timezone Note**: `Last synced` は **真の UTC**（`+00:00`）。JST に変換（+9時間）してから経過時間を計算すること。なお `Wake time` / `Bedtime` は `+00:00` サフィックスが付いているが **実態は JST**（UTC 変換不要）。
*   **Rule**: If the last synced time is older than **30 minutes**, append a prominent warning:
    > [!WARNING]
    > **データ同期の遅延があります**: このデータは **[経過時間]前**（[ローカル表記での同期時刻]）のものです。急激な体格・体調の変化は反映されていないため、現在の体調の判断材料にしないでください。

---

## Workflow & Implementation

### Step 1: Execution (Level 3)
Invoke the Garmin retrieval script located inside this skill's localized path, passing the decrypted Home Assistant token dynamically via the secure gateway tool:
*   **Tool**: `run_workspace_script`
*   **Parameters**:
    *   `script_name`: `skills/vitals-coach/scripts/500_get-vital-data-garmin.sh`
    *   `env`:
        *   `HOMEASSISTANT_TOKEN`: `$vault:homeassistant-token`

The script returns **only the 27 core fields** listed in the tables below. Do not attempt to parse other fields — they are not returned.

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

### Step 3: Deliver (Concise & Empathetic Secretary Tone)
Formulate a supportive, professional, yet warm secretary-style response in Japanese (K-sama's preference).
*   Structure:
    1.  Emergency medical warning (if feeling unwell).
    2.  Data latency warning (if sync lag > 30 minutes).
    3.  Summary table of the Core Health Metrics.
    4.  Warm, actionable coaching advice.

---

## Common Mistakes & Antipatterns

*   **Raw Data Dumping**: The script already filters to 27 core fields. Do not modify the script to output all sensors.
*   **Assuming Real-time Status**: Ignoring the sync time and declaring hours-old vitals as "fine" during an acute illness. (Fix: Always calculate and declare JST sync latency).
*   **Missing Vault Keys**: Running without verifying if `homeassistant-token` exists in `vault.json`. (Fix: Verify token presence first and fail gracefully).
*   **Absolute Script Execution**: Running shell scripts directly. (Fix: Use `run_workspace_script` for secure localized execution).

---

## Red Flags - STOP and Check Context

- You are executing `500_get-vital-data-garmin.sh` via a raw shell command or absolute path.
- You presented vital stats without calculating the sync latency in JST.
- K-sama feels ill, but you omitted the emergency clinical care warning.
- Unnecessary fitness parameters (e.g. trekking distance, VO2 max) are bloating the output.

**All of these mean: Stop. Apply the Vitals Coach Skill rules immediately.**
