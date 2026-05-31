---
name: vitals-coach
description: Use when an AI agent needs to perform body condition analysis, health coaching, daily health reports, or personalized physical wellness advice.
---

# Vitals Coach Skill

## Overview
Analyzes retrieved vital metrics to generate personalized, empathetic, and actionable physical wellness coaching and lifestyle advice.

---

## When to Use

### Triggering Symptoms / Scenarios:
- The user requests feedback, tips, or coaching based on their parsed physical data.
- Executing daily health summaries, vitality reports, or stress management advice.
- The user asks: "How should I structure my day based on my current vitals?"

### When NOT to use:
- You are retrieving raw data or validating device latency. (Use `get-vital-data-garmin` for retrieval and safety checks instead).
- General fitness chat not tied to specific user metrics.

---

## Core Workflow

### **REQUIRED SUB-SKILL:**
You **MUST** trigger the **`get-vital-data-garmin`** skill to safely retrieve raw metrics, calculate sync latency, and display critical medical safety warnings *before* formulating coaching feedback.

### Phase 1: Analysis (Level 2)
Evaluate the safely retrieved core vital metrics against the following thresholds:

| Metric | Alert Threshold | Coaching Strategy |
| :--- | :--- | :--- |
| **Stress Level** | Average > 50 | Recommend breathing exercises, immediate screen breaks, or light walks. |
| **Body Battery** | Current < 20 | Suggest reducing intensive tasks, prioritising rest, and going to bed early. |
| **Steps Taken** | Under 10,000 | Propose light physical movement (e.g. stretching) or a brief walk. |
| **Sleep Duration** | Under 6 hours | Inquire about sleep quality and propose nap strategies or bedtime hygiene. |
| **HRV Status** | "Unbalanced" | Suggest high physical fatigue; advise focusing on passive recovery. |

### Phase 2: Deliver (Level 2)
Generate a supportive, encouraging, yet professional coaching response in Japanese.
- Highlight achievements first (e.g. goal completion).
- Match the coaching recommendations exactly to the parsed thresholds.

---

## Common Mistakes & Antipatterns

*   **Hardcoded Scripts**: Invoking raw Garmin scripts directly inside this skill. (Fix: Delegate all script execution and raw timezone parsing to `get-vital-data-garmin`).
*   **Neglecting Safety**: Formulating health coaching without checking if the medical disclaimer or timezone lag warning was outputted. (Fix: Ensure `get-vital-data-garmin` output is processed first).

---

## Red Flags - STOP and Check Context

- You are executing `500_get-vital-data-garmin.sh` within this skill.
- You did not prompt or reference the `get-vital-data-garmin` sub-skill for data retrieval.

**All of these mean: Stop. Apply the Vitals Coach Skill rules and delegate retrieval.**
