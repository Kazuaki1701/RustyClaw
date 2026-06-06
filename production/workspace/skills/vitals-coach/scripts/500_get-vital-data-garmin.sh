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
  "Garmin Connect Body battery":             .["Garmin Connect Body battery"],
  "Garmin Connect Body battery highest":     .["Garmin Connect Body battery highest"],
  "Garmin Connect Body battery charged":     .["Garmin Connect Body battery charged"],
  "Garmin Connect Body battery drained":     .["Garmin Connect Body battery drained"],
  "Garmin Connect Body battery lowest":      .["Garmin Connect Body battery lowest"],
  "Garmin Connect Average stress level":     .["Garmin Connect Average stress level"],
  "Garmin Connect High stress duration":     .["Garmin Connect High stress duration"],
  "Garmin Connect Max stress level":         .["Garmin Connect Max stress level"],
  "Garmin Connect Low stress duration":      .["Garmin Connect Low stress duration"],
  "Garmin Connect Medium stress duration":   .["Garmin Connect Medium stress duration"],
  "Garmin Connect Activity stress duration": .["Garmin Connect Activity stress duration"],
  "Garmin Connect Resting heart rate":       .["Garmin Connect Resting heart rate"],
  "Garmin Connect Steps":                    .["Garmin Connect Steps"],
  "Garmin Connect Daily step goal":          .["Garmin Connect Daily step goal"],
  "Garmin Connect Sedentary time":           .["Garmin Connect Sedentary time"],
  "Garmin Connect Active time":              .["Garmin Connect Active time"],
  "Garmin Connect Intensity minutes":        .["Garmin Connect Intensity minutes"],
  "Garmin Connect Yesterday steps":          .["Garmin Connect Yesterday steps"],
  "Garmin Connect Weekly step average":      .["Garmin Connect Weekly step average"],
  "Garmin Connect Sleep duration":           .["Garmin Connect Sleep duration"],
  "Garmin Connect Deep sleep":               .["Garmin Connect Deep sleep"],
  "Garmin Connect REM sleep":                .["Garmin Connect REM sleep"],
  "Garmin Connect Light sleep":              .["Garmin Connect Light sleep"],
  "Garmin Connect Awake time":               .["Garmin Connect Awake time"],
  "Garmin Connect Bedtime":                  .["Garmin Connect Bedtime"],
  "Garmin Connect Wake time":                .["Garmin Connect Wake time"],
  "Garmin Connect Last synced":              .["Garmin Connect Last synced"]
}'
