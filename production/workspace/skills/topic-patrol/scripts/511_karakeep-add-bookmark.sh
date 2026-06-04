#!/bin/bash
# Topic Patrol で紹介した URL を KaraKeep にブックマーク登録し、_ai-patrol タグを付与する。
# Usage: 511_karakeep-add-bookmark.sh <url>

set -euo pipefail

URL="${1:-}"

if [ -z "$KARAKEEP_API_KEY" ] || [ -z "$KARAKEEP_SERVER_ADDR" ]; then
    echo "Error: KARAKEEP_API_KEY or KARAKEEP_SERVER_ADDR is not set." >&2
    exit 1
fi

if [ -z "$URL" ]; then
    echo "Usage: $0 <url>" >&2
    exit 1
fi

# ブックマーク作成
RESPONSE=$(curl -s -X POST \
    -H "Authorization: Bearer $KARAKEEP_API_KEY" \
    -H "Content-Type: application/json" \
    -d "{\"type\": \"link\", \"url\": $(echo "$URL" | jq -Rs .)}" \
    "$KARAKEEP_SERVER_ADDR/api/v1/bookmarks")

ID=$(echo "$RESPONSE" | jq -r '.id // empty')

if [ -z "$ID" ]; then
    echo "Error: failed to create bookmark. Response: $RESPONSE" >&2
    exit 1
fi

# _ai-patrol タグを付与
curl -s -X POST \
    -H "Authorization: Bearer $KARAKEEP_API_KEY" \
    -H "Content-Type: application/json" \
    -d '{"tags": [{"tagName": "_ai-patrol"}]}' \
    "$KARAKEEP_SERVER_ADDR/api/v1/bookmarks/$ID/tags" > /dev/null

echo "Bookmarked: $URL (id=$ID, tag=_ai-patrol)"
