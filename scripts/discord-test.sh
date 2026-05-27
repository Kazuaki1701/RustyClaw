#!/usr/bin/env bash
set -euo pipefail

CONFIG="${1:-config.json}"

if [[ ! -f "$CONFIG" ]]; then
    echo "ERROR: config file not found: $CONFIG" >&2
    exit 1
fi

TOKEN=$(jq -r '.discord_token // empty' "$CONFIG")
CHANNEL_ID=$(jq -r '.discord_home_channel_id // empty' "$CONFIG")
MESSAGE="${2:-RustyClaw test message 🦀 $(date '+%Y-%m-%dT%H:%M:%S')}"

if [[ -z "$TOKEN" ]]; then
    echo "ERROR: discord_token not set in $CONFIG" >&2
    exit 1
fi
if [[ -z "$CHANNEL_ID" ]]; then
    echo "ERROR: discord_home_channel_id not set in $CONFIG" >&2
    exit 1
fi

echo "Sending to channel $CHANNEL_ID ..."

HTTP_STATUS=$(curl -s -o /tmp/discord-test-response.json -w "%{http_code}" \
    -X POST "https://discord.com/api/v10/channels/${CHANNEL_ID}/messages" \
    -H "Authorization: Bot ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"content\": $(jq -Rn --arg m "$MESSAGE" '$m')}")

BODY=$(cat /tmp/discord-test-response.json)

if [[ "$HTTP_STATUS" == "200" ]]; then
    MSG_ID=$(echo "$BODY" | jq -r '.id')
    echo "OK  message_id=$MSG_ID  status=$HTTP_STATUS"
else
    echo "FAIL  status=$HTTP_STATUS"
    echo "$BODY" | jq . 2>/dev/null || echo "$BODY"
    exit 1
fi
