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
