#!/bin/bash
# Geminiclaw 用: Garmin センサーの状態を JSON で一括取得する

# Load secrets from common location or workspace link
SECRETS_MASTER="/mnt/qnap/DESKTOP/Kazuaki/Documents/Projects/dotfiles/.shell.d/70_secrets.sh"
SECRETS_LOCAL="$(dirname "$0")/../secret.sh"

if [ -f "$SECRETS_MASTER" ]; then
    source "$SECRETS_MASTER"
elif [ -f "$SECRETS_LOCAL" ]; then
    source "$SECRETS_LOCAL"
fi

# Check required environment variables
if [ -z "$HOMEASSISTANT_TOKEN" ]; then
    echo "Error: HOMEASSISTANT_TOKEN is not set."
    exit 1
fi

curl -s -X POST -H "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"template": "{% set garmin = states.sensor | selectattr(\"entity_id\", \"search\", \"^sensor.garmin_\") %}\n{\n{% for s in garmin %}\n  \"{{ s.name }}\": \"{{ s.state }}{{ \" \" ~ s.attributes.unit_of_measurement if s.attributes.unit_of_measurement and s.state != \"unknown\" else \"\" }}\"{{ \",\" if not loop.last }}\n{% endfor %}\n}"}' \
     http://192.168.1.30:8123/api/template | jq .
