---
name: get-vital-data-garmin
description: Use when a user requests their current physical status, fatigue levels, heart rate, stress levels, sleep analytics, or body battery from Garmin wearables.
---

# Get Vital Data Garmin Skill

## Overview
Retrieves and filters raw consumer-grade Garmin vital statistics via Home Assistant, enforcing strict medical safety guidelines and synchronizing latency checks.

---

## When to Use

### Triggering Symptoms / Scenarios:
- The user asks for their daily step count, sleep duration, heart rate, stress, or body battery.
- The user reports feeling physically fatigued, exhausted, or unwell, and references Garmin tracking.
- Automated daily briefing or vitals patrol tasks require physical health metrics.

### When NOT to use:
- The user requires diagnostic-grade clinical assessments or is experiencing an acute, severe medical emergency.
- The user is asking about general fitness advice or workouts without requesting specific Garmin vital data.

---

## The Core Safeguard Rules

### 1. Mandatory Medical Warning & Action
Garmin devices are consumer wearables, not clinical diagnostic tools. If the user reports feeling extremely unwell, you **MUST** prioritize the following warning before presenting any data:
*   **Seek Clinical Care Immediately**: Advise the user to contact emergency services (e.g., 119 in Japan) or visit the nearest hospital emergency room.
*   **Do Not Self-Diagnose**: Do not use consumer-grade metrics to make critical medical decisions.

### 2. Synchronization Latency Verification (Critical)
Always parse the `"Garmin Connect Last synced"` timestamp from the raw data and compare it to the current local time.
*   **Timezone Conversion**: The API timestamp is usually in UTC (`+00:00`). You **MUST** convert it to the user's local timezone (e.g., JST `+09:00`) before calculating the latency elapsed.
*   **Rule**: If the last synced time is older than **30 minutes**, append a prominent latency warning:
    > [!WARNING]
    > **データ同期の遅延があります**: このデータは **[経過時間（時間と分）]前**（[ローカル表記での同期時刻]）のものです。急激な体格・体調の変化は反映されていないため、現在の体調の判断材料にしないでください。

---

## Pattern Implementation

### Step 1: Execution (Level 3)
Invoke the Garmin retrieval script located inside this skill's localized path:
*   **Tool**: `run_workspace_script`
*   **Script Name**: `500_get-vital-data-garmin.sh`

### Step 2: Filtration & Summary (Level 2)
Do NOT dump the entire JSON block to the user. Extract and present only the **Core Health Metrics**:
1.  **Body Battery**: Current status (%), Daily Low (%).
2.  **Stress**: Average level, peak stress level, and duration of "High Stress".
3.  **Heart Rate**: Resting HR (bpm), Max HR (bpm), Min HR (bpm).
4.  **Sleep**: Sleep duration (hours/mins) and Sleep Need (if available).
5.  **Activity**: Steps taken vs Daily Goal.

---

## Common Mistakes & Antipatterns

*   **Raw Data Dumping**: Outputting 70+ lines of raw JSON, which wastes tokens and overwhelms the user. (Fix: Extract only the 5 core health metrics).
*   **Assuming Real-time Status**: Presenting hours-old data as "your current heart rate is fine" during an acute illness. (Fix: Always calculate and declare the sync time latency).
*   **UTC/JST Confusion**: Calculating latency without converting the UTC timestamp to local JST, leading to a false 9-hour latency gap. (Fix: Explicitly parse and align timezones).
*   **Clinical Diagnostics Simulation**: Diagnosing a user with a specific disease based on high stress or low HRV. (Fix: Maintain a strict lifestyle-only coaching tone and advise medical consultation).

---

## Red Flags - STOP and Check Context

- You did not check the `"Garmin Connect Last synced"` timestamp or forgot timezone offset calculations.
- The user feels ill, but you did not output the clinical care emergency warning first.
- Unnecessary fitness parameters (e.g. trekking distance, VO2 max) are bloating the response.

**All of these mean: Stop. Apply the Get Vital Data Garmin Skill rules immediately.**
