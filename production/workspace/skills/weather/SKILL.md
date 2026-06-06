---
name: weather
description: Use when the user asks about current weather, rain forecast, temperature, or what to wear. Also used for commute weather checks between Omori and Atsugi.
---

# Weather Skill

## Overview
Fetches real-time weather data for two fixed locations (Omori and Atsugi) via the Open-Meteo API and delivers concise Japanese coaching on umbrella need, clothing, and commute weather differences.

---

## When to Use

### Triggering Scenarios:
- The user asks about current weather, rain, temperature, or what to wear.
- The user asks about commute conditions (大森 ↔ 厚木).
- Any scheduled weather patrol cron triggers.

### When NOT to use:
- Detailed multi-day forecasts (this skill covers today only).
- Locations other than Omori or Atsugi.

---

## Workflow

### Step 1: Fetch weather data

- **Tool**: `run_workspace_script`
- **Parameters**:
  - `script_name`: `skills/weather/scripts/504_get-weather.sh`
  - *(no `env` required — public API)*

The script returns **2 JSON objects** (OMORI then ATSUGI), each with:
- `current_temp_c`, `wind_speed_kmh`
- `today_max_c`, `today_min_c`
- `rain_next_60min`: array of 5 `{time, mm}` entries (next 75 minutes, 15-min intervals)

### Step 2: Coaching analysis

Evaluate both locations against these rules:

| Condition | Action |
| :--- | :--- |
| Any `rain_next_60min[*].mm > 0.5` | 傘を推奨（該当地点を明示） |
| `current_temp_c < 10` | 防寒着を提案 |
| `current_temp_c > 33` | 熱中症対策（水分補給・帽子）を提案 |
| OMORI と ATSUGI の降水有無が異なる | 通勤時の天候差を具体的に言及 |

### Step 3: Deliver

日本語で簡潔に天気レポートとアドバイスを出力。

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `run_workspace_script` を使うこと。
- **`error` フィールドが JSON に含まれる場合はAPIエラー。** K様にネットワーク状態の確認を促すこと。
