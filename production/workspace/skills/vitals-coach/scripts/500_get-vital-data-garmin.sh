#!/bin/bash
# Garmin コアメトリクスのみを JSON で取得する（全センサー出力は禁止）

if [ -z "$HOMEASSISTANT_TOKEN" ]; then
    echo "Error: HOMEASSISTANT_TOKEN is not set."
    exit 1
fi

curl -s -X POST -H "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"template": "{% set garmin = states.sensor | selectattr(\"entity_id\", \"search\", \"^sensor.garmin_\") %}\n{\n{% for s in garmin %}\n  \"{{ s.name }}\": \"{{ s.state }}{{ \" \" ~ s.attributes.unit_of_measurement if s.attributes.unit_of_measurement and s.state != \"unknown\" else \"\" }}\"{{ \",\" if not loop.last }}\n{% endfor %}\n}"}' \
     http://192.168.1.30:8123/api/template | jq '{
  "Garmin Connect Body battery":          .["Garmin Connect Body battery"],
  "Garmin Connect Average stress level":  .["Garmin Connect Average stress level"],
  "Garmin Connect Steps":                 .["Garmin Connect Steps"],
  "Garmin Connect Daily step goal":       .["Garmin Connect Daily step goal"],
  "Garmin Connect Sleep duration":        .["Garmin Connect Sleep duration"],
  "Garmin Connect HRV status":            .["Garmin Connect HRV status"],
  "Garmin Connect Last synced":           .["Garmin Connect Last synced"]
}'
