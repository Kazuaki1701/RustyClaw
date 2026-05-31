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
*   **Timezone Conversion**: The timestamp is in UTC (`+00:00`). You **MUST** convert it to JST (`+09:00`) before calculating the latency elapsed relative to current system time.
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

The script returns **only the 7 core fields** (Body battery, Average stress level, Steps, Daily step goal, Sleep duration, HRV status, Last synced). Do not attempt to parse other fields — they are not returned.

### Step 2: Filtration & Threshold Analysis (Level 2)
Extract only the **Core Health Metrics** and evaluate them against these coaching thresholds:

| Metric (HA sensor name) | Alert Threshold | Coaching Strategy (Japanese) |
| :--- | :--- | :--- |
| **`Garmin Connect Body battery`** | Current < 20 | 激しい活動を控え、早めの就寝や休息を優先するよう提案。 |
| **`Garmin Connect Average stress level`** | Average > 50 | 深呼吸、スクリーンフリー時間、または軽い休憩を推奨。 |
| **`Garmin Connect Steps`** | Under 10,000 (参照: `Garmin Connect Daily step goal`) | 軽いストレッチや散歩を提案。 |
| **`Garmin Connect Sleep duration`** | Under 360 min (6 hours) | 睡眠不足を指摘し、短時間の昼寝や就寝環境の改善をアドバイス。 |
| **`Garmin Connect HRV status`** | "Unbalanced" または "Unknown" | "Unbalanced": 疲労蓄積を指摘しパッシブリカバリーを推奨。"Unknown": データ未取得のため HRV 評価は省略し他メトリクスで判断。 |

### Step 3: Deliver (Concise & Empathetic Secretary Tone)
Formulate a supportive, professional, yet warm secretary-style response in Japanese (K-sama's preference).
*   Structure:
    1.  Emergency medical warning (if feeling unwell).
    2.  Data latency warning (if sync lag > 30 minutes).
    3.  Summary table of the Core Health Metrics.
    4.  Warm, actionable coaching advice.

---

## Common Mistakes & Antipatterns

*   **Raw Data Dumping**: The script already filters to 7 core fields. Do not modify the script to output all sensors.
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
