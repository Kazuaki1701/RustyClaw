#!/bin/bash
# skills/karakeep/scripts/503_karakeep-list.sh
# Retrieves a list of bookmarks from the KaraKeep server

LIMIT=${1:-20}

if [ -z "$KARAKEEP_API_KEY" ] || [ -z "$KARAKEEP_SERVER_ADDR" ]; then
    echo "Error: KARAKEEP_API_KEY or KARAKEEP_SERVER_ADDR is not set."
    exit 1
fi

curl -s -H "Authorization: Bearer $KARAKEEP_API_KEY" \
     "$KARAKEEP_SERVER_ADDR/api/v1/bookmarks?limit=$LIMIT"
