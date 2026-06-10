#!/bin/bash
# 天気予報: 大森・厚木 の天気概況・降水確率を tsukumijima API（気象庁データ）から取得

set -euo pipefail

BASE_URL="https://weather.tsukumijima.net/api/forecast"

fetch_weather() {
    local name="$1"
    local city="$2"
    local include_forecast_text="${3:-0}"

    # 1. city コードから Open-Meteo 用の緯度・経度を内部マッピング
    local lat=""
    local lon=""
    case "$city" in
        "130010") # 大森 (東京)
            lat="35.5613"
            lon="139.7241"
            ;;
        "140010") # 厚木 (神奈川)
            lat="35.4432"
            lon="139.3624"
            ;;
    esac

    # 2. tsukumijima API から概況天気予報をフェッチ
    local raw
    raw=$(curl -sSf "${BASE_URL}?city=${city}") || {
        jq -n --arg loc "$name" --arg err "tsukumijima API request failed" '{"location":$loc,"error":$err}'
        return
    }

    # 3. Open-Meteo API から 15分刻み降水量（直近60分先まで）をフェッチ
    local open_meteo_raw=""
    if [ -n "$lat" ] && [ -n "$lon" ]; then
        local om_url="https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}&minutely_15=precipitation&timezone=Asia%2FTokyo&forecast_days=1"
        open_meteo_raw=$(curl -sSf "$om_url") || open_meteo_raw=""
    fi

    local now_str
    now_str=$(date +"%Y-%m-%dT%H:%M")
    local om_json="{}"
    if [ -n "$open_meteo_raw" ]; then
        om_json="$open_meteo_raw"
    fi

    local formatted_json
    formatted_json=$(echo "$raw" | jq \
        --arg name "$name" \
        --arg include_text "$include_forecast_text" \
        --arg now "$now_str" \
        --argjson om "$om_json" '
        . as $root |
        ($root.forecasts[0].temperature.max.celsius | if . == null then null else tonumber end) as $max_c |
        ($root.forecasts[0].temperature.min.celsius | if . == null then null else tonumber end) as $min_c |

        # Open-Meteo から直近60分の降雨予測を抽出
        (if $om.minutely_15 then
            ($om.minutely_15.time | to_entries | map(select(.value >= $now)) | .[0:5]) as $slots |
            [ $slots[] | {
                time: (.value | .[11:16]),
                mm: ($om.minutely_15.precipitation[.key] // 0)
            } ]
         else [] end) as $rain_next_60min |

        {
            location:       $name,
            telop:          $root.forecasts[0].telop,
            today_max_c:    $max_c,
            today_min_c:    $min_c,
            weather_detail: $root.forecasts[0].detail.weather,
            wind:           $root.forecasts[0].detail.wind,
            chance_of_rain: $root.forecasts[0].chanceOfRain,
            rain_next_60min: $rain_next_60min
        } |
        if $include_text == "1" then . + {forecast_text: $root.description.bodyText} else . end
    ') || {
        jq -n --arg loc "$name" --arg err "jq parse error" '{"location":$loc,"error":$err}'
        return
    }
    echo "$formatted_json"
}

fetch_weather "OMORI"  "130010" "1"
fetch_weather "ATSUGI" "140010" "0"
