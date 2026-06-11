#!/bin/sh
# 220_ha_env_snapshot.sh — HA 環境スナップショット & トレンド計算
#
# 出力 (stdout):
#   [HA_ENV|HH:MM] [Room: XX.X°C↑ / XX%→] [CO2: XXXXppm↑] [Presence: Detected] [Outer: XX.X°C]
#
# 状態ファイル: memory/ha-state.json
#   .samples  — 最大 6 サンプルのリングバッファ
#   .latest   — 最新サンプル
#   .summary  — 1 行サマリー文字列
#   .spike_detected — CO2 > 1500 ppm の場合 true
#
# 終了コード:
#   0 — 正常
#   1 — HA 到達不能
#   2 — スパイク検知 (--check-spike 時のみ)
#
. "$(dirname "$0")/__200_ha_common.sh"

MEMORY_DIR="$HA_PROJECT_ROOT/memory"
STATE_FILE="$MEMORY_DIR/ha-state.json"
SUMMARY_FILE="$MEMORY_DIR/ha-env-summary.txt"
CHECK_SPIKE=false

for arg in "$@"; do
    [ "$arg" = "--check-spike" ] && CHECK_SPIKE=true
done

mkdir -p "$MEMORY_DIR"

# 1. HA REST API からセンサー一括取得
STATES=$(wget -qO- --timeout=10 --header "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
    "$HA_ENDPOINT/states" 2>/dev/null)
if [ -z "$STATES" ]; then
    echo "ERROR: HA unreachable or HOMEASSISTANT_TOKEN not set" >&2
    exit 1
fi

# 2. センサー値抽出
ROOM_TEMP=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id == "sensor.livingroom_air_temperature")] | first | .state // "unknown"')
ROOM_HUMID=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id == "sensor.livingroom_air_humidity")] | first | .state // "unknown"')
OUTER_TEMP=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id == "sensor.outside1f_air_temperature")] | first | .state // "unknown"')
CO2=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id | test("sensor\\..*co2|sensor\\..*carbon_dioxide"))] | first | .state // "unknown"')
PRESENCE=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id | test("binary_sensor\\..*motion|binary_sensor\\..*presence"))] | first | .state // "off"')

NOW=$(date +"%H:%M")
TS=$(date -Iseconds)

# 3. リングバッファ更新
if [ -f "$STATE_FILE" ]; then
    PREV_JSON=$(cat "$STATE_FILE")
else
    PREV_JSON='{"samples":[]}'
fi

NEW_SAMPLE=$(jq -cn \
    --arg ts "$TS" \
    --arg rt "$ROOM_TEMP" \
    --arg rh "$ROOM_HUMID" \
    --arg ot "$OUTER_TEMP" \
    --arg co2 "$CO2" \
    '{ts:$ts, room_temp:$rt, room_humid:$rh, outer_temp:$ot, co2:$co2}')

UPDATED=$(printf '%s' "$PREV_JSON" | jq \
    --argjson s "$NEW_SAMPLE" \
    '.samples += [$s] | .samples = (.samples | if length > 6 then .[-6:] else . end) | .latest = $s')

# 4. トレンド計算
trend_arrow() {
    CURR="$1"; PREV_VAL="$2"; THRESH="$3"
    [ "$CURR" = "unknown" ] || [ "$PREV_VAL" = "unknown" ] && { echo "→"; return; }
    DIFF=$(awk -v c="$CURR" -v p="$PREV_VAL" -v t="$THRESH" \
        'BEGIN { d=c-p; if(d>t) print "up"; else if(d<-t) print "down"; else print "flat"}')
    case "$DIFF" in up) echo "↑" ;; down) echo "↓" ;; *) echo "→" ;; esac
}

OLDEST_TEMP=$(printf '%s' "$UPDATED" | jq -r '.samples[0].room_temp // "unknown"')
OLDEST_HUMID=$(printf '%s' "$UPDATED" | jq -r '.samples[0].room_humid // "unknown"')
OLDEST_CO2=$(printf '%s' "$UPDATED" | jq -r '.samples[0].co2 // "unknown"')

TEMP_ARROW=$(trend_arrow "$ROOM_TEMP" "$OLDEST_TEMP" "0.5")
HUMID_ARROW=$(trend_arrow "$ROOM_HUMID" "$OLDEST_HUMID" "3")
CO2_ARROW=$(trend_arrow "$CO2" "$OLDEST_CO2" "50")

# 5. スパイク検知
SPIKE=false
if [ "$CO2" != "unknown" ]; then
    SPIKE=$(awk -v c="$CO2" 'BEGIN { if(c+0 > 1500) print "true"; else print "false"}')
fi

# 6. Presence 表示
PRESENCE_STR="None"
{ [ "$PRESENCE" = "on" ] || [ "$PRESENCE" = "detected" ]; } && PRESENCE_STR="Detected"

# 7. サマリー生成と状態書き込み
SUMMARY="[HA_ENV|${NOW}] [Room: ${ROOM_TEMP}°C${TEMP_ARROW} / ${ROOM_HUMID}%${HUMID_ARROW}] [CO2: ${CO2}ppm${CO2_ARROW}] [Presence: ${PRESENCE_STR}] [Outer: ${OUTER_TEMP}°C]"

printf '%s' "$UPDATED" | jq \
    --arg summary "$SUMMARY" \
    --argjson spike "$SPIKE" \
    '.summary = $summary | .spike_detected = $spike' > "$STATE_FILE"

echo "$SUMMARY" > "$SUMMARY_FILE"
echo "$SUMMARY"

# --check-spike: スパイク時に exit 2
if [ "$CHECK_SPIKE" = "true" ] && [ "$SPIKE" = "true" ]; then
    echo "SPIKE_DETECTED: CO2=${CO2}ppm" >&2
    exit 2
fi
