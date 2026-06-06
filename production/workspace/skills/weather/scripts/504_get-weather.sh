#!/bin/bash
# 天気予報: 大森・厚木 の現在気温・風速・今日の最高/最低・75分降水量を取得

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

    local now
    now=$(date +"%Y-%m-%dT%H:%M")

    echo "$raw" | jq --arg name "$name" --arg now "$now" '
        . as $root |
        ($root.minutely_15.time | to_entries) as $time_entries |
        ($time_entries | map(select(.value >= $now)) | .[0:5]) as $future_slots |
        {
            location:        $name,
            current_temp_c:  $root.current.temperature_2m,
            wind_speed_kmh:  $root.current.wind_speed_10m,
            today_max_c:     $root.daily.temperature_2m_max[0],
            today_min_c:     $root.daily.temperature_2m_min[0],
            rain_next_60min: [
                $future_slots[] |
                {
                    time: (.value[11:16]),
                    mm:   (.key as $i | $root.minutely_15.precipitation[$i] // 0)
                }
            ]
        }
    ' || {
        echo "{\"location\":\"${name}\",\"error\":\"jq parse error\"}"
        return
    }
}

fetch_weather "OMORI"  "35.5613" "139.7241"
fetch_weather "ATSUGI" "35.4432" "139.3624"
