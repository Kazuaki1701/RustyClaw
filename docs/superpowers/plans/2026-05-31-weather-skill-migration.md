# Weather Skill Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate `YolpWeatherTool` (Rust native) to a pure shell-script skill at `production/workspace/skills/weather/`, adding temperature and wind data for 2 fixed locations (Omori + Atsugi), then delete the Rust implementation.

**Architecture:** Create bash script + SKILL.md, verify script output against live Open-Meteo API, then delete the Rust struct and its gateway registration. Each phase is independently committable.

**Tech Stack:** bash, curl, jq, Open-Meteo API (no auth), Rust (deletion only), cargo test

---

## File Map

| Action | Path |
|---|---|
| Create | `production/workspace/skills/weather/scripts/504_get-weather.sh` |
| Create | `production/workspace/skills/weather/SKILL.md` |
| Modify (delete lines) | `crates/rustyclaw-tools/src/lib.rs` — lines 1980–2163 + test at 1794–1799 |
| Modify (delete lines) | `crates/rustyclaw-gateway/src/lib.rs` — lines 729–730 |

---

## Task 1: Create `504_get-weather.sh`

**Files:**
- Create: `production/workspace/skills/weather/scripts/504_get-weather.sh`

- [ ] **Step 1: Create the skill directory**

```bash
mkdir -p production/workspace/skills/weather/scripts
```

- [ ] **Step 2: Write the script**

Create `production/workspace/skills/weather/scripts/504_get-weather.sh` with the following content:

```bash
#!/bin/bash
# Garmin 天気予報: 大森・厚木 の現在気温・風速・今日の最高/最低・60分降水量を取得

set -euo pipefail

fetch_weather() {
    local name="$1"
    local lat="$2"
    local lon="$3"

    local url="https://api.open-meteo.com/v1/forecast"
    url+="?latitude=${lat}&longitude=${lon}"
    url+="&minutely_15=precipitation"
    url+="&current=temperature_2m,wind_speed_10m"
    url+="&daily=temperature_2m_max,temperature_2m_min"
    url+="&timezone=Asia%2FTokyo"
    url+="&forecast_days=1"

    local raw
    raw=$(curl -sf "$url") || {
        echo "{\"location\":\"${name}\",\"error\":\"API request failed\"}"
        return
    }

    # 現在時刻から60分先までの minutely_15 スロット (5エントリ) を抽出
    local now_str
    now_str=$(date +"%Y-%m-%dT%H:%M")

    echo "$raw" | jq --arg name "$name" --arg now "$now_str" '
        # minutely_15 の time 配列から現在時刻以降の最初のインデックスを探す
        (.minutely_15.time | to_entries
          | map(select(.value >= $now))
          | .[0:5]
          | map({time: (.value | .[11:16]), mm: (.key as $i | .value | (input? // null) // 0)})
        ) as $slots |
        {
            location:        $name,
            current_temp_c:  .current.temperature_2m,
            wind_speed_kmh:  .current.wind_speed_10m,
            today_max_c:     (.daily.temperature_2m_max[0]),
            today_min_c:     (.daily.temperature_2m_min[0]),
            rain_next_60min: [
                range(5) as $i |
                {
                    time: (.minutely_15.time | map(select(. >= $now)) | .[$i] | .[11:16]),
                    mm:   (.minutely_15.precipitation | [
                            .[ (.minutely_15.time | to_entries | map(select(.value >= $now)) | .[$i].key) ]
                          ] | .[0] // 0)
                }
            ]
        }
    '
}

fetch_weather "OMORI"  "35.5613" "139.7241"
fetch_weather "ATSUGI" "35.4432" "139.3624"
```

- [ ] **Step 3: Make executable**

```bash
chmod +x production/workspace/skills/weather/scripts/504_get-weather.sh
```

- [ ] **Step 4: Run script and verify it returns 2 JSON objects**

> **Note**: jq の minutely_15 インデックス抽出は複雑なため、Step 4/5 で出力が期待通りでない場合は `rain_next_60min` の jq 式をデバッグすること。`echo "$raw" | jq '.minutely_15.time[:5]'` で時刻配列の先頭5件を確認するとデバッグしやすい。

```bash
bash production/workspace/skills/weather/scripts/504_get-weather.sh
```

Expected: 2 JSON objects printed sequentially, each containing:
- `location` ("OMORI" then "ATSUGI")
- `current_temp_c` (numeric)
- `wind_speed_kmh` (numeric)
- `today_max_c` (numeric)
- `today_min_c` (numeric)
- `rain_next_60min` (array of 5 objects with `time` and `mm`)

No `error` field should appear.

- [ ] **Step 5: Verify key presence with jq**

```bash
bash production/workspace/skills/weather/scripts/504_get-weather.sh | \
  jq -s '[.[].location] | sort'
```

Expected: `["ATSUGI", "OMORI"]`

- [ ] **Step 6: Commit**

```bash
git add production/workspace/skills/weather/scripts/504_get-weather.sh
git commit -m "feat(weather): add 504_get-weather.sh for Omori and Atsugi"
```

---

## Task 2: Create `weather/SKILL.md`

**Files:**
- Create: `production/workspace/skills/weather/SKILL.md`

- [ ] **Step 1: Write the SKILL.md**

Create `production/workspace/skills/weather/SKILL.md` with the following content:

```markdown
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
- `rain_next_60min`: array of 5 `{time, mm}` entries (next 60 minutes, 15-min intervals)

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
```

- [ ] **Step 2: Verify SKILL.md has valid YAML frontmatter**

```bash
head -5 production/workspace/skills/weather/SKILL.md
```

Expected output:
```
---
name: weather
description: Use when the user asks about current weather, rain forecast, temperature, or what to wear. Also used for commute weather checks between Omori and Atsugi.
---
```

- [ ] **Step 3: Commit**

```bash
git add production/workspace/skills/weather/SKILL.md
git commit -m "feat(weather): add weather skill SKILL.md"
```

---

## Task 3: Delete `YolpWeatherTool` from `rustyclaw-tools`

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- [ ] **Step 1: Verify current test count**

```bash
cargo test -p rustyclaw-tools 2>&1 | grep "^test result"
```

Note the number of passing tests (baseline before deletion).

- [ ] **Step 2: Delete the `YolpWeatherTool` test (lines 1794–1799)**

In `crates/rustyclaw-tools/src/lib.rs`, delete these lines:

```rust
    #[tokio::test]
    async fn test_yolp_weather_tool_invalid_coords() {
        let tool = YolpWeatherTool::new();
        let res = tool.execute(serde_json::json!({ "coordinates": "invalid" })).await;
        assert!(res.is_error);
    }
```

- [ ] **Step 3: Delete `YolpWeatherTool` struct and `fetch_open_meteo_weather` (lines 1980–2163)**

In `crates/rustyclaw-tools/src/lib.rs`, delete from the comment banner through the end of the file:

```rust
// ─── YolpWeatherTool ─────────────────────────────────────────────────────────

pub struct YolpWeatherTool;

impl YolpWeatherTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for YolpWeatherTool {
    // ... (entire impl block)
}

async fn fetch_open_meteo_weather(lat: f64, lon: f64) -> Result<Vec<Value>, anyhow::Error> {
    // ... (entire function)
}
```

*(This is lines 1980–2163 — the rest of the file after the test module closing brace.)*

- [ ] **Step 4: Verify `cargo check` passes**

```bash
cargo check -p rustyclaw-tools 2>&1 | grep -E "^error"
```

Expected: no output (zero errors).

- [ ] **Step 5: Run tests and confirm they still pass**

```bash
cargo test -p rustyclaw-tools 2>&1 | grep "^test result"
```

Expected: all results show `0 failed`. Count should be baseline minus 1 (the deleted test).

- [ ] **Step 6: Commit**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(weather): remove YolpWeatherTool from rustyclaw-tools"
```

---

## Task 4: Delete `YolpWeatherTool` registration from gateway

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: Delete the registration lines (729–730)**

In `crates/rustyclaw-gateway/src/lib.rs`, delete these 2 lines:

```rust
        // YolpWeatherTool は常時登録（APIキー不要、Open-Meteoバックエンド）
        tool_registry.register(Arc::new(rustyclaw_tools::YolpWeatherTool::new()));
```

- [ ] **Step 2: Verify `cargo check` passes**

```bash
cargo check -p rustyclaw-gateway 2>&1 | grep -E "^error"
```

Expected: no output.

- [ ] **Step 3: Run full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED"
```

Expected: all `test result: ok`, no `FAILED`.

- [ ] **Step 4: Commit**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(weather): remove YolpWeatherTool gateway registration"
```

---

## Task 5: Update `docs/task.md` — mark Phase 36 item 1 complete

**Files:**
- Modify: `docs/task.md`

- [ ] **Step 1: Mark Phase 36 item 1 as done**

In `docs/task.md`, change:

```markdown
- `[ ]` **1. 天気予報のスキル化（Phase A）**
```

to:

```markdown
- `[x]` **1. 天気予報のスキル化（Phase A）**
```

- [ ] **Step 2: Commit**

```bash
git add docs/task.md
git commit -m "docs(task): mark Phase 36-A weather skill migration complete"
```
