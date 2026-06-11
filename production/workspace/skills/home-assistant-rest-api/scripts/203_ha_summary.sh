#!/bin/sh
# ---------------------------------------------------------
# 【機能説明】主要センサーの状態サマリー取得 (203)
# ---------------------------------------------------------
. "$(dirname "$0")/__200_ha_common.sh"

# HA から全エンティティの状態を取得 (wget を使用)
STATES_JSON=$(wget -qO- --header "Authorization: Bearer $HOMEASSISTANT_TOKEN" "$HA_ENDPOINT/states")

# 整形して出力
echo "🛠️ [STATUS] Home Assistant Entity States:"
echo "$STATES_JSON" | jq -r --arg wl "$HA_WHITELIST" ".[]? | select(.entity_id | test(\$wl)) | \"・\(.attributes.friendly_name) (\(.entity_id)): \(.state)\""
