#!/bin/bash
# scripts/karakeep_cleanup.sh
# 2週間以上経過し、保護タグやお気に入りのないRSSアイテムを削除する

set -e

# Vault から設定を直接ロード
VAULT="$HOME/.rustyclaw/vault.json"
if [ ! -f "$VAULT" ]; then
    echo "Error: vault.json not found."
    exit 1
fi
KARAKEEP_API_KEY=$(python3 -c "import json; d=json.load(open('$VAULT')); print(d.get('karakeep-api-key',''))" 2>/dev/null)
KARAKEEP_SERVER_ADDR=$(python3 -c "import json; d=json.load(open('$VAULT')); print(d.get('karakeep-server-addr',''))" 2>/dev/null)

if [ -z "$KARAKEEP_API_KEY" ] || [ -z "$KARAKEEP_SERVER_ADDR" ]; then
    echo "Error: karakeep-api-key or karakeep-server-addr is not set in vault.json."
    exit 1
fi

# しきい値の計算 (14日前)
THRESHOLD=$(date -u -d "14 days ago" +"%Y-%m-%dT%H:%M:%SZ")
echo "Starting cleanup. Threshold: $THRESHOLD"

DELETED_COUNT=0
CURSOR=""

while :; do
    # APIリクエスト
    URL="$KARAKEEP_SERVER_ADDR/api/v1/bookmarks?limit=100"
    if [ -n "$CURSOR" ]; then URL="$URL&cursor=$CURSOR"; fi
    
    RESPONSE=$(curl -s -H "Authorization: Bearer $KARAKEEP_API_KEY" "$URL")
    
    # 削除対象の抽出
    # 条件: 14日以上前、favourited=false、source=rss、保護タグなし
    TARGETS=$(echo "$RESPONSE" | jq -r --arg threshold "$THRESHOLD" '
        .bookmarks[] | 
        select(.createdAt < $threshold) | 
        select(.favourited == false) |
        select(.source == "rss") |
        select(.tags | map(.name) | any(. == "_bookmarked" or . == "_star" or . == "_doitlater" or . == "_recommended") | not) |
        .id
    ')

    # 削除実行
    for id in $TARGETS; do
        echo "Deleting bookmark: $id"
        curl -s -X DELETE -H "Authorization: Bearer $KARAKEEP_API_KEY" "$KARAKEEP_SERVER_ADDR/api/v1/bookmarks/$id"
        DELETED_COUNT=$((DELETED_COUNT + 1))
    done

    # 次のページへ
    CURSOR=$(echo "$RESPONSE" | jq -r '.nextCursor')
    if [ "$CURSOR" == "null" ] || [ -z "$CURSOR" ]; then break; fi
    
    # ページ内の最後のアイテムがしきい値より新しければ、これ以上遡る必要はない
    LAST_CREATED=$(echo "$RESPONSE" | jq -r '.bookmarks[-1].createdAt')
    if [[ "$LAST_CREATED" > "$THRESHOLD" ]]; then
        # createdAtは降順なので、ここがしきい値より新しければ、次のページもしきい値より新しい可能性がある
        # ただしHoarderのAPIは順序が保証されない場合があるため、最後まで回すのが安全
        :
    fi
done

echo "Cleanup finished. Deleted $DELETED_COUNT items."
