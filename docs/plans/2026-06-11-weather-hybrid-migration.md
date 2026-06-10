# 実装計画書: 気象庁・Open-Meteo ハイブリッド天気予報への移行

- **ステータス**: `[ACCEPTED]` (2026-06-11 実装完了)
- **起草日**: 2026-06-11
- **関連タスク**: BUG-02 / daily-briefing

---

## 1. 目的

現在、気象庁予報 API（`weather.tsukumijima.net`）のみから取得している天気予報に、ピンポイント座標での「直近60分間の15分刻みの降水量予測」（Open-Meteo API）をマージするハイブリッド化を行う。
また、プロンプト指示書（SKILL）からすでに削除されている `yolp_weather`（Yahoo YOLP API ツール）への参照を完全に消去し、移行後のスクリプトを呼び出すように統一する。

---

## 2. 設計仕様

### 2.1 スクリプト改修 (`504_get-weather.sh`)
*   **内部マッピングの追加**: 引数の拡張は行わず、スクリプト内部で `city` コードから Open-Meteo 用の緯度（`lat`）と経度（`lon`）へ変換するマッピング（case文等）を実装し、従来の引数形式を維持する。
*   **API の多重フェッチ**:
    1.  `weather.tsukumijima.net` から日本語の広域予報をフェッチ。
    2.  `api.open-meteo.com` から指定座標の15分刻み降雨予測をフェッチ。
*   **JSON マージ**:
    `jq` を用いて、気象庁の概況データに `rain_next_60min` 配列を合流させたマージ JSON を出力する。
*   **エラー耐性**: どちらか一方の API が一時的に接続エラーになってもスクリプト全体は落とさず、エラーがない側のデータを出力する。

### 2.2 プロンプト指示書 (SKILL) 修正
*   `yolp_weather` への参照をすべて削除。
*   `daily-briefing.md` において `run_workspace_script("skills/weather/scripts/504_get-weather.sh")` を実行するよう指示を書き換える。

---

## 3. 実行タスク

### Task 1: `504_get-weather.sh` の改修
**対象ファイル**: `production/workspace/skills/weather/scripts/504_get-weather.sh`

- [x] **Step 1**: スクリプト内部に `city` コードから緯度・経度へのマッピング処理を追加。
- [x] **Step 2**: Open-Meteo API からの `minutely_15` 降雨量取得と、`jq` による JSON マージ処理を追加。
- [x] **Step 3**: 実際に実行し、期待される JSON フォーマット（概況 + 直近雨量配列）が出力されることを検証。

### Task 2: 指示書（SKILL）の更新
**対象ファイル**:
*   `workspace/skills/daily-briefing.md`
*   `production/workspace/skills/daily-briefing/SKILL.md`

- [x] **Step 1**: `yolp_weather` ツールの使用指示を、`run_workspace_script("skills/weather/scripts/504_get-weather.sh")` の実行と出力パース指示へ変更する。

### Task 3: タスク管理の更新
**対象ファイル**: `docs/task.md`

- [x] **Step 1**: `task.md` にハイブリッド天気予報移行タスクを追加し、進捗を追跡する。

---

## 4. 期待される出力 JSON の形式

```json
{
  "location": "OMORI",
  "telop": "曇時々晴",
  "today_max_c": 25,
  "today_min_c": 18,
  "weather_detail": "くもり　昼過ぎから夕方晴れ",
  "wind": "南の風",
  "chance_of_rain": { "T00_06": "--%", "T06_12": "10%", ... },
  "rain_next_60min": [
    { "time": "06:45", "mm": 0.0 },
    { "time": "07:00", "mm": 0.4 },
    { "time": "07:15", "mm": 1.2 },
    { "time": "07:30", "mm": 0.0 }
  ],
  "forecast_text": "..." // (大森のみ)
}
```
