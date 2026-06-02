# Design: Weather Skill — tsukumijima API 移行

**Date**: 2026-06-02

---

## Overview

既存の `504_get-weather.sh`（Open-Meteo API）を tsukumijima 天気予報 API に置き換える。
気象庁データに基づく天気説明文（telop）・降水確率・予報本文（bodyText）を活用し、
台風などの警戒情報と通勤概況コメントを追加する。

---

## 変更スコープ

| 対象 | 内容 |
|---|---|
| 修正 | `production/workspace/skills/weather/scripts/504_get-weather.sh` |
| 修正 | `production/workspace/skills/weather/SKILL.md` |

---

## スクリプト仕様

**ファイル**: `production/workspace/skills/weather/scripts/504_get-weather.sh`  
**API**: `https://weather.tsukumijima.net/api/forecast?city=<CITY_CODE>`  
**APIキー**: 不要（パブリック）

### 都市コード

| 拠点 | 都市コード | 対応エリア |
|---|---|---|
| OMORI | 130010 | 東京 |
| ATSUGI | 140010 | 横浜（神奈川） |

### 取得フィールド（`forecasts[0]` = 今日）

```json
{
  "location": "OMORI",
  "telop": "雨",
  "today_max_c": null,
  "today_min_c": null,
  "weather_detail": "雨",
  "wind": "南の風　やや強く",
  "chance_of_rain": {
    "T00_06": "--%",
    "T06_12": "--%",
    "T12_18": "--%",
    "T18_24": "70%"
  },
  "forecast_text": "台風第６号が種子島付近にあって北東へ進んでおり..."
}
```

**注意事項**:
- `temperature.min/max.celsius` は当日未発表の場合 `null` になる。null の場合は気温チェックをスキップ。
- `chanceOfRain` の値が `"--%"` は「対象外（過去の時間帯）」を意味する。傘判断から除外する。
- `forecast_text` は両拠点で同じ地域予報（関東甲信地方）になるため、OMORI 分のみ取得して共有する。

### エラー処理

curl 失敗または非2xx レスポンス時:
```json
{"location": "OMORI", "error": "API request failed"}
```

---

## SKILL.md コーチングロジック

### ステップ1: データ取得

- `run_workspace_script` で `skills/weather/scripts/504_get-weather.sh` を実行
- 2つの JSON オブジェクト（OMORI・ATSUGI）を受け取る
- `forecast_text` は OMORI オブジェクトにのみ含まれる（関東甲信地方の地域予報として共有）

### ステップ2: 警戒チェック（最優先）

`telop` / `weather_detail` / `forecast_text` に以下のキーワードが含まれる場合、**レスポンス冒頭に警戒メッセージを表示**:

```
台風 / 暴風 / 大雨 / 警報 / 注意報
```

### ステップ3: コーチング判定

| 条件 | アクション |
|---|---|
| `chance_of_rain` の `--%` 以外スロットに ≥30% が存在 | 傘を推奨（地点・時間帯を明示） |
| `today_min_c < 10`（null でなければ） | 防寒着を提案 |
| `today_max_c > 33`（null でなければ） | 熱中症対策（水分補給・帽子）を提案 |

### ステップ4: 通勤概況

`forecast_text`（bodyText）をもとに大森↔厚木通勤の概況を1〜2文で日本語解説する。

> 例: 「台風の影響で関東南部は湿った空気が流れ込んでいます。大森・厚木ともに夕方以降の雨に備えてください。」

### ステップ5: 出力

日本語で簡潔にレポートとアドバイスを出力。警戒情報がある場合は冒頭に目立つ形で表示。

---

## 旧仕様からの変更点

| 旧（Open-Meteo） | 新（tsukumijima） |
|---|---|
| 緯度経度でAPIリクエスト | 都市コードでAPIリクエスト |
| 現在気温・風速（リアルタイム） | 今日の最高/最低気温（気象庁予報値） |
| 15分ごとの降水量（mm） | 6時間帯ごとの降水確率（%） |
| 傘判断: mm > 0.5 | 傘判断: 有効スロットで ≥30% |
| 天気説明なし | telop + weather_detail（晴れ/曇り/雨など） |
| 台風情報なし | forecast_text から警戒キーワード検出 |
| 通勤差分比較 | forecast_text ベースの通勤概況コメント |
