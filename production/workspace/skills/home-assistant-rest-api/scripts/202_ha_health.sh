#!/bin/sh
# ---------------------------------------------------------
# 【機能説明】主要デバイス・センサーのヘルスチェック (202)
# ---------------------------------------------------------
. "$(dirname "$0")/__200_ha_common.sh"

# HA から全エンティティの状態を取得 (wget)
STATES_JSON=$(wget -qO- --header "Authorization: Bearer $HOMEASSISTANT_TOKEN" "$HA_ENDPOINT/states")

NOW_EPOCH=$(date +%s)
STALE_THRESHOLD=43200

echo "🛠️ [HEALTH] Device Health Check (Final Optimized):"

# 1. バッテリー
echo "🔋 バッテリー要確認:"
echo "$STATES_JSON" | jq -r --arg ex_bat "$HA_EXCLUDE_BATTERY" ".[] | select((.entity_id | test(\"battery|battery_level\")) and (.entity_id | test(\$ex_bat) | not) and (.state | tonumber? // 100) <= 50) | \"・\(.attributes.friendly_name) (\(.entity_id)): \(.state)%\"" | grep "." || echo "  - 警告対象のバッテリー低下はありません。"
echo
# 2. 通信異常 (OFFLINE)
echo "⚠️ 主要デバイスの異常 (OFFLINE):"
echo "$STATES_JSON" | jq -r --arg wl "$HA_WHITELIST" ".[] | select((.entity_id | test(\$wl)) and (.state == \"unavailable\" or .state == \"unknown\")) | \"・\(.attributes.friendly_name) (\(.entity_id)): \(.state)\"" | grep "." || echo "  - 全ての主要デバイスがオンラインです。"
echo

# 3. 停滞 (STALE / 正規化時刻で比較)
echo "❄️ 主要センサーの停滞 (STALE):"
echo "$STATES_JSON" | jq -r --arg wl "$HA_WHITELIST" --arg ex_stale "$HA_EXCLUDE_STALE" --arg NOW "$NOW_EPOCH" --arg THRESH "$STALE_THRESHOLD" '
  .[] | select(.entity_id | test($wl)) | 
  select(.state != "unavailable" and .state != "unknown") |
  select(.entity_id | test($ex_stale) | not) |
  { id: .entity_id, name: .attributes.friendly_name, state: .state, updated: .last_updated } |
  (.updated | sub("\\.[0-9]+\\+00:00$"; "Z")) as $norm_date |
  select(($NOW | tonumber) - ($norm_date | fromdateiso8601) > ($THRESH | tonumber)) |
  " - \(.name) (\(.id)): \(.state) (最終更新: \(.updated))"' | grep "." || echo "  - 全ての主要センサーが正常に更新されています。"
