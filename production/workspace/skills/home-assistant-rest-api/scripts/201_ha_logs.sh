#!/bin/sh
# ---------------------------------------------------------
# 【機能説明】Home Assistant 異常ログ「超・凝縮」エンジン
# ---------------------------------------------------------
. "$(dirname "$0")/__200_ha_common.sh"

# ログ取得 (wget) & パイプライン処理
wget -qO- --header "Authorization: Bearer $HOMEASSISTANT_TOKEN" "$HA_ENDPOINT/error_log" | \
    grep -E "(ERROR|WARNING)" | \
    sed -E "s/^.*(ERROR|WARNING)/\1/" | \
    sort | uniq -c | sort -nr | head -n 12
