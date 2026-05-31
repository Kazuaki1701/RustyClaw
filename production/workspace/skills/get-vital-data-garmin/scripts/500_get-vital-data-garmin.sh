#!/bin/bash
# Garmin センサーの状態を JSON で一括取得する

# 優先順位: 環境変数 → vault.json → QNAP secrets
if [ -z "$HOMEASSISTANT_TOKEN" ]; then
    VAULT="$HOME/.rustyclaw/vault.json"
    if [ -f "$VAULT" ]; then
        HOMEASSISTANT_TOKEN=$(python3 -c "import json; d=json.load(open('$VAULT')); print(d.get('homeassistant-token',''))" 2>/dev/null)
    fi
fi
if [ -z "$HOMEASSISTANT_TOKEN" ]; then
    SECRETS_MASTER="/mnt/qnap/DESKTOP/Kazuaki/Documents/Projects/dotfiles/.shell.d/70_secrets.sh"
    [ -f "$SECRETS_MASTER" ] && source "$SECRETS_MASTER"
fi

if [ -z "$HOMEASSISTANT_TOKEN" ]; then
    echo "Error: HOMEASSISTANT_TOKEN is not set."
    exit 1
fi

curl -s -X POST -H "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"template": "{% set garmin = states.sensor | selectattr(\"entity_id\", \"search\", \"^sensor.garmin_\") %}\n{\n{% for s in garmin %}\n  \"{{ s.name }}\": \"{{ s.state }}{{ \" \" ~ s.attributes.unit_of_measurement if s.attributes.unit_of_measurement and s.state != \"unknown\" else \"\" }}\"{{ \",\" if not loop.last }}\n{% endfor %}\n}"}' \
     http://192.168.1.30:8123/api/template | jq .
