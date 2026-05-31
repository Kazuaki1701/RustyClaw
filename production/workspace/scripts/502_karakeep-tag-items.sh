#!/bin/bash
# scripts/karakeep_tag_items.sh
# 指定されたIDリストにタグを付与する

TAG_NAME=$1
shift
IDS=$@

# 優先順位: 環境変数 → vault.json
if [ -z "$KARAKEEP_API_KEY" ]; then
    VAULT="$HOME/.rustyclaw/vault.json"
    if [ -f "$VAULT" ]; then
        KARAKEEP_API_KEY=$(python3 -c "import json; d=json.load(open('$VAULT')); print(d.get('karakeep-api-key',''))" 2>/dev/null)
    fi
fi

if [ -z "$KARAKEEP_API_KEY" ] || [ -z "$KARAKEEP_SERVER_ADDR" ]; then
    echo "Error: KARAKEEP_API_KEY or KARAKEEP_SERVER_ADDR is not set."
    exit 1
fi

if [ -z "$TAG_NAME" ] || [ -z "$IDS" ]; then
    echo "Usage: $0 <tag_name> <id1> <id2> ..."
    exit 1
fi

for id in $IDS; do
    echo "Tagging $id with $TAG_NAME..."
    curl -s -X POST -H "Authorization: Bearer $KARAKEEP_API_KEY" -H "Content-Type: application/json" -d "{\"tags\": [{\"tagName\": \"$TAG_NAME\"}]}" "$KARAKEEP_SERVER_ADDR/api/v1/bookmarks/$id/tags"
done
