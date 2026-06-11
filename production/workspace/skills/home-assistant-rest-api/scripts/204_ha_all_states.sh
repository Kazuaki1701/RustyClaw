#!/bin/sh
# ---------------------------------------------------------
# 【機能説明】家中の全エンティティ状態をスリム化して一括取得 (204)
# ---------------------------------------------------------
. "$(dirname "$0")/__200_ha_common.sh"

# HA から全エンティティの状態を取得し、wget と jq で圧縮
wget -qO- --header "Authorization: Bearer $HOMEASSISTANT_TOKEN" "$HA_ENDPOINT/states" | \
    jq -r '.[] | "\(.attributes.friendly_name // .entity_id) (\(.entity_id)): \(.state)"'
