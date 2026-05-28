#!/bin/bash
# scripts/karakeep_tag_items.sh
# 指定されたIDリストにタグを付与する

TAG_NAME=$1
shift
IDS=$@

if [ -z "$TAG_NAME" ] || [ -z "$IDS" ]; then
    echo "Usage: $0 <tag_name> <id1> <id2> ..."
    exit 1
fi

for id in $IDS; do
    echo "Tagging $id with $TAG_NAME..."
    curl -s -X POST -H "Authorization: Bearer $KARAKEEP_API_KEY" -H "Content-Type: application/json" -d "{\"tags\": [{\"tagName\": \"$TAG_NAME\"}]}" "$KARAKEEP_SERVER_ADDR/api/v1/bookmarks/$id/tags"
done
