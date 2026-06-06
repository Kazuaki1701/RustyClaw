# Weather Skill tsukumijima API 移行 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `504_get-weather.sh` を Open-Meteo API から tsukumijima API に置き換え、天気説明文・降水確率・台風警戒・通勤概況コメントを提供できるようにする。

**Architecture:** shell スクリプト（`fetch_weather` 関数）が tsukumijima API を都市コードで叩き、jq で JSON 整形して OMORI・ATSUGI 各1オブジェクトを出力。OMORI のみ `forecast_text`（気象庁予報本文）を付与し SKILL.md でエージェントが通勤概況生成に使う。

**Tech Stack:** bash, curl, jq, tsukumijima 天気予報 API（https://weather.tsukumijima.net）

---

## ファイルマップ

| ファイル | 変更内容 |
|---|---|
| `production/workspace/skills/weather/scripts/504_get-weather.sh` | 全面書き換え（Open-Meteo → tsukumijima） |
| `production/workspace/skills/weather/SKILL.md` | コーチングロジック・フィールド定義を更新 |

---

### Task 1: `504_get-weather.sh` を tsukumijima API に書き換える

**Files:**
- Modify: `production/workspace/skills/weather/scripts/504_get-weather.sh`

- [ ] **Step 1: 現在の出力を記録しておく（比較用）**

```bash
bash production/workspace/skills/weather/scripts/504_get-weather.sh | jq .
```

現在の出力（OMORI・ATSUGI の JSON 2つ）が表示されることを確認。ネットワーク未接続の場合は `error` フィールドが返る。

- [ ] **Step 2: スクリプトを tsukumijima 版に書き換える**

`production/workspace/skills/weather/scripts/504_get-weather.sh` を以下で上書きする:

```bash
#!/bin/bash
# 天気予報: 大森・厚木 の天気概況・降水確率を tsukumijima API（気象庁データ）から取得

set -euo pipefail

BASE_URL="https://weather.tsukumijima.net/api/forecast"

fetch_weather() {
    local name="$1"
    local city="$2"
    local include_forecast_text="${3:-0}"

    local raw
    raw=$(curl -sf "${BASE_URL}?city=${city}") || {
        echo "{\"location\":\"${name}\",\"error\":\"API request failed\"}"
        return
    }

    echo "$raw" | jq --arg name "$name" --argjson include_text "$include_forecast_text" '
        . as $root |
        ($root.forecasts[0].temperature.max.celsius | if . == null then null else tonumber end) as $max_c |
        ($root.forecasts[0].temperature.min.celsius | if . == null then null else tonumber end) as $min_c |
        {
            location:       $name,
            telop:          $root.forecasts[0].telop,
            today_max_c:    $max_c,
            today_min_c:    $min_c,
            weather_detail: $root.forecasts[0].detail.weather,
            wind:           $root.forecasts[0].detail.wind,
            chance_of_rain: $root.forecasts[0].chanceOfRain
        } |
        if $include_text == 1 then . + {forecast_text: $root.description.bodyText} else . end
    ' || {
        echo "{\"location\":\"${name}\",\"error\":\"jq parse error\"}"
        return
    }
}

fetch_weather "OMORI"  "130010" "1"
fetch_weather "ATSUGI" "140010" "0"
```

- [ ] **Step 3: スクリプトを実行して出力を検証する**

```bash
bash production/workspace/skills/weather/scripts/504_get-weather.sh | jq .
```

期待する出力（2つの JSON オブジェクトが改行区切りで出力）:

```json
{
  "location": "OMORI",
  "telop": "晴れ",
  "today_max_c": 27,
  "today_min_c": null,
  "weather_detail": "晴れ　昼過ぎ　から　くもり",
  "wind": "南の風",
  "chance_of_rain": {
    "T00_06": "--%",
    "T06_12": "0%",
    "T12_18": "10%",
    "T18_24": "20%"
  },
  "forecast_text": "東京地方は..."
}
{
  "location": "ATSUGI",
  "telop": "晴れ",
  "today_max_c": 26,
  "today_min_c": null,
  "weather_detail": "晴れ　時々　くもり",
  "wind": "南の風",
  "chance_of_rain": {
    "T00_06": "--%",
    "T06_12": "0%",
    "T12_18": "10%",
    "T18_24": "20%"
  }
}
```

チェック項目:
- OMORI に `forecast_text` フィールドがある
- ATSUGI に `forecast_text` フィールドがない
- `today_max_c` / `today_min_c` が数値または `null`（文字列でない）
- `chance_of_rain` に4つのキー（T00_06・T06_12・T12_18・T18_24）がある
- `error` フィールドが含まれない

- [ ] **Step 4: jq 整形でエラーがないことを確認**

```bash
bash production/workspace/skills/weather/scripts/504_get-weather.sh 2>&1 | grep -i error || echo "No errors"
```

期待出力: `No errors`

- [ ] **Step 5: コミット**

```bash
git add production/workspace/skills/weather/scripts/504_get-weather.sh
git commit -m "feat(weather): replace Open-Meteo with tsukumijima API"
```

---

### Task 2: `SKILL.md` をコーチングロジックに合わせて更新する

**Files:**
- Modify: `production/workspace/skills/weather/SKILL.md`

- [ ] **Step 1: SKILL.md を新しい内容で上書きする**

`production/workspace/skills/weather/SKILL.md` を以下で上書きする:

```markdown
---
name: weather
description: Use when the user asks about current weather, rain forecast, temperature, or what to wear. Also used for commute weather checks between Omori and Atsugi.
---

# Weather Skill

## Overview
Fetches today's weather forecast for Omori (Tokyo) and Atsugi (Kanagawa) via the tsukumijima API (気象庁データ) and delivers concise Japanese coaching on umbrella need, clothing, typhoon alerts, and commute overview.

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

The script returns **2 JSON objects** (OMORI then ATSUGI).

**OMORI fields:**
- `telop`: 天気（例: 晴れ、曇り、雨）
- `today_max_c`, `today_min_c`: 今日の最高/最低気温（当日未発表の場合 `null`）
- `weather_detail`: 詳細天気説明（例: 「台風第3号の影響で暴風雨」）
- `wind`: 風向き・強さ（例: 「南の風　やや強く」）
- `chance_of_rain`: 6時間帯別降水確率 `{T00_06, T06_12, T12_18, T18_24}`（`"--%" ` は対象外の時間帯）
- `forecast_text`: 気象庁予報本文（関東甲信地方）— 通勤概況生成に使用

**ATSUGI fields:** `forecast_text` 以外は同様（`forecast_text` キーは存在しない）。

### Step 2: 警戒チェック（最優先）

`telop` / `weather_detail` / `forecast_text`（OMORI）に以下のキーワードが含まれる場合、**レスポンス冒頭に警戒メッセージを目立つ形で表示**:

```
台風 / 暴風 / 大雨 / 警報 / 注意報
```

### Step 3: コーチング判定

両拠点を以下のルールで評価する:

| 条件 | アクション |
| :--- | :--- |
| `chance_of_rain` の `--%` 以外スロットに ≥30% が存在 | 傘を推奨（地点・時間帯を明示） |
| `today_min_c < 10`（null でなければ） | 防寒着を提案 |
| `today_max_c > 33`（null でなければ） | 熱中症対策（水分補給・帽子）を提案 |

### Step 4: 通勤概況

OMORI の `forecast_text`（気象庁予報本文）をもとに、大森↔厚木の通勤概況を1〜2文で日本語解説する。

### Step 5: Deliver

日本語で簡潔に天気レポートとアドバイスを出力。警戒情報がある場合は冒頭に表示。

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `run_workspace_script` を使うこと。
- **`error` フィールドが JSON に含まれる場合はAPIエラー。** K様にネットワーク状態の確認を促すこと。
- **`chance_of_rain` が `--%` のスロットは傘判断に使わない。** 過去の時間帯を示す。
- **`today_max_c` / `today_min_c` が `null` の場合は気温アドバイスをスキップ。** 当日未発表の場合がある。
```

- [ ] **Step 2: 内容を確認する**

```bash
head -5 production/workspace/skills/weather/SKILL.md
```

期待出力: `---` から始まるフロントマターが表示される。

- [ ] **Step 3: コミット**

```bash
git add production/workspace/skills/weather/SKILL.md
git commit -m "feat(weather): update SKILL.md for tsukumijima API coaching logic"
```
