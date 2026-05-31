#!/bin/bash
# Garmin センサーの状態を JSON で一括取得する

if [ -z "$HOMEASSISTANT_TOKEN" ]; then
    echo "Error: HOMEASSISTANT_TOKEN is not set."
    exit 1
fi

curl -s -X POST -H "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"template": "{% set garmin = states.sensor | selectattr(\"entity_id\", \"search\", \"^sensor.garmin_\") %}\n{\n{% for s in garmin %}\n  \"{{ s.name }}\": \"{{ s.state }}{{ \" \" ~ s.attributes.unit_of_measurement if s.attributes.unit_of_measurement and s.state != \"unknown\" else \"\" }}\"{{ \",\" if not loop.last }}\n{% endfor %}\n}"}' \
     http://192.168.1.30:8123/api/template | jq .
