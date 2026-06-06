# Design: Weather Skill Migration (Phase A)

**Date**: 2026-05-31  
**Phase**: 36-A

---

## Overview

Migrate `YolpWeatherTool` (Rust native) to a pure shell-script skill at `production/workspace/skills/weather/`. Adds temperature and wind data to the existing precipitation-only output, and covers 2 fixed locations (Omori + Atsugi).

---

## Change Scope

| Action | Target |
|---|---|
| Create | `production/workspace/skills/weather/SKILL.md` |
| Create | `production/workspace/skills/weather/scripts/504_get-weather.sh` |
| Delete | `crates/rustyclaw-tools/src/lib.rs` — `YolpWeatherTool` struct, `fetch_open_meteo_weather()`, related tests |
| Delete | `crates/rustyclaw-gateway/src/lib.rs` — `YolpWeatherTool` registration (~line 730) |

---

## Script Specification

**File**: `production/workspace/skills/weather/scripts/504_get-weather.sh`  
**API**: `https://api.open-meteo.com/v1/forecast` (no API key required)  
**Structure**: Shell function `fetch_weather <name> <lat> <lon>` called for each location sequentially.

### Locations (hardcoded)

| Name | Latitude | Longitude |
|---|---|---|
| OMORI | 35.5613 | 139.7241 |
| ATSUGI | 35.4432 | 139.3624 |

### API Request Parameters

```
minutely_15=precipitation
current=temperature_2m,wind_speed_10m
daily=temperature_2m_max,temperature_2m_min
timezone=Asia/Tokyo
forecast_days=1
```

### jq Output Format (per location)

```json
{
  "location": "OMORI",
  "current_temp_c": 23.1,
  "wind_speed_kmh": 12.4,
  "today_max_c": 26.0,
  "today_min_c": 18.5,
  "rain_next_60min": [
    {"time": "14:00", "mm": 0.0},
    {"time": "14:15", "mm": 0.3},
    {"time": "14:30", "mm": 0.0},
    {"time": "14:45", "mm": 0.0},
    {"time": "15:00", "mm": 0.0}
  ]
}
```

`rain_next_60min` contains the next 5 slots (0, 15, 30, 45, 60 min) from `minutely_15.time` / `minutely_15.precipitation`. Filter to the 5 entries closest to `[now, now+15, now+30, now+45, now+60]` using `jq` index lookup by time string match.

Script exits with code 1 and an error message if the API returns a non-2xx status.

---

## SKILL.md Specification

**Frontmatter**:
```yaml
name: weather
description: Use when the user asks about current weather, rain forecast, temperature, or what to wear. Also used for commute weather checks between Omori and Atsugi.
```

**Workflow**:
1. Run `run_workspace_script` with `script_name: skills/weather/scripts/504_get-weather.sh` (no `env` required).
2. Parse the JSON output for both locations.
3. Apply coaching logic:
   - Any `rain_next_60min` slot with `mm > 0.5` → recommend umbrella for that location.
   - `current_temp_c < 10` → suggest warm clothing.
   - `current_temp_c > 33` → suggest heat-stroke precautions.
   - If OMORI and ATSUGI differ significantly (rain at one but not the other) → call out the commute weather gap.
4. Deliver a concise Japanese summary.

---

## Rust Cleanup

Remove from `crates/rustyclaw-tools/src/lib.rs`:
- `YolpWeatherTool` struct and `impl Tool for YolpWeatherTool`
- `fetch_open_meteo_weather()` async function
- All unit tests referencing `YolpWeatherTool`

Remove from `crates/rustyclaw-gateway/src/lib.rs`:
- `tool_registry.register(Arc::new(rustyclaw_tools::YolpWeatherTool::new()))` and surrounding comment (~line 729–730)

Run `cargo test` — all remaining tests must pass.
