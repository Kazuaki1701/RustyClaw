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

- **Tool**: `ctx_execute`
- **Parameters**:
  - `language`: `bash`
  - `code`: `bash workspace/skills/weather/scripts/504_get-weather.sh`
  - *(env 不要 — public API)*

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

警戒メッセージの形式例:
> ⚠️ **台風警戒**: 台風第6号が接近中です。外出には十分注意してください。

### Step 3: コーチング判定

両拠点を以下のルールで評価する:

| 条件 | アクション |
| :--- | :--- |
| `chance_of_rain` の `--%` 以外スロットに ≥30% が存在 | 傘を推奨（地点・時間帯を明示） |
| `today_min_c < 10`（null でなければ） | 防寒着を提案 |
| `today_max_c > 33`（null でなければ） | 熱中症対策（水分補給・帽子）を提案 |

### Step 4: 通勤概況

OMORI の `forecast_text`（気象庁予報本文）をもとに、大森↔厚木の通勤概況を1〜2文で日本語解説する。

出力例:
> 「台風の影響で関東南部は湿った空気が流れ込んでいます。大森・厚木ともに夕方以降の雨が予想されるため、傘をお忘れなく。」

### Step 5: Deliver

日本語で簡潔に天気レポートとアドバイスを出力。警戒情報がある場合は冒頭に表示。

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `ctx_execute` を使うこと。
- **`error` フィールドが JSON に含まれる場合はAPIエラー。** K様にネットワーク状態の確認を促すこと。エラー例: `{"location":"OMORI","error":"API request failed"}`
- **`chance_of_rain` が `--%` のスロットは傘判断に使わない。** 過去の時間帯を示す。
- **`today_max_c` / `today_min_c` が `null` の場合は気温アドバイスをスキップ。** 当日未発表の場合がある。
