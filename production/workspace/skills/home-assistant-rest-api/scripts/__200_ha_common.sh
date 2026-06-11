#!/bin/sh
# 🏠 Home Assistant 共通基盤 (sh/BusyBox Compatible)
# ---------------------------------------------------------

# 1. パス解決 (POSIX sh compatible)
# カレントディレクトリがどこであっても自身のパスを取得
HA_SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
export HA_SCRIPT_DIR
HA_PROJECT_ROOT=$(cd "$HA_SCRIPT_DIR/../.." && pwd)
export HA_PROJECT_ROOT

# 2. 環境変数の読み込み (.env 優先)
# bash の source は . に置き換え
if [ -f "$HA_PROJECT_ROOT/.env" ]; then
    . "$HA_PROJECT_ROOT/.env"
elif [ -f "$HA_PROJECT_ROOT/PicoClaw/.env" ]; then
    . "$HA_PROJECT_ROOT/PicoClaw/.env"
fi

# 3. 認証チェック
if [ -z "$HOMEASSISTANT_TOKEN" ]; then
    echo "Error: HOMEASSISTANT_TOKEN is not set." >&2
fi

# 4. 監視対象一軍ホワイトリスト (主要デバイス)
export HA_WHITELIST='(sensor.(outside|livingroom|pc_room|bedroom)_air_(temperature|humidity)|sensor.nature_remo_1_illuminance|sensor.braviatv_duration|lock.switchbot_entrance_lock|climate.living_room|sensor.air_conditioner_[12]_inside_temperature|sensor.outside1f_air_temperature)'

# 5. バッテリー除外リスト
export HA_EXCLUDE_BATTERY='(sensor.switchbot_(contact_[12]|motion_1)_battery|sensor.hotuto_de_battery(_plus)?|sensor.kai_bi_sensa_[0-9a-f]+_battery|sensor.ren_gan_sensa_ea_battery|sensor.garmin_connect_body_battery_.*)'

# 6. 停滞監視除外リスト
export HA_EXCLUDE_STALE='(sensor.braviatv_duration_(tvasahi|recorder)|sensor.hotuto_de_battery_plus|sensor.kai_bi_sensa_[0-9a-f]+_battery|sensor.ren_gan_sensa_ea_battery|sensor.garmin_connect_body_battery_.*)'

# 共通変数のエクスポート
export HA_ENDPOINT="http://192.168.1.30:8123/api"
