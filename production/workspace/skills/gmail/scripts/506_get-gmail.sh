#!/bin/bash
# Gmail のメッセージ一覧を取得し、id/sender/subject/date/snippet のみ抽出する
# Usage: 506_get-gmail.sh [query] [max_results]

set -euo pipefail

# systemd サービスは ~/.cargo/bin を PATH に含まないため補完する
export PATH="$HOME/.cargo/bin:$PATH"

QUERY="${1:-is:unread}"
MAX="${2:-10}"

if ! command -v gws &>/dev/null; then
    echo '{"error": "gws not found in PATH"}' >&2
    exit 1
fi

# メッセージ ID 一覧を取得
ids=$(gws gmail users messages list \
    --params "{\"userId\":\"me\",\"q\":\"${QUERY}\",\"maxResults\":${MAX}}" \
    --format json \
  | jq -r '.messages[]?.id // empty')

if [ -z "$ids" ]; then
    echo "[]"
    exit 0
fi

# 各メッセージのヘッダーを取得して整形
result="["
first=true
while IFS= read -r id; do
    meta=$(gws gmail users messages get \
        --params "{\"userId\":\"me\",\"id\":\"${id}\",\"format\":\"metadata\",\"metadataHeaders\":[\"From\",\"Subject\",\"Date\"]}" \
        --format json)

    entry=$(echo "$meta" | jq --arg id "$id" '{
        id:      $id,
        sender:  ([.payload.headers[]? | select(.name == "From")  | .value][0] // ""),
        subject: ([.payload.headers[]? | select(.name == "Subject")| .value][0] // ""),
        date:    ([.payload.headers[]? | select(.name == "Date")  | .value][0] // ""),
        snippet: (.snippet // "")
    }')

    if [ "$first" = true ]; then
        result="${result}${entry}"
        first=false
    else
        result="${result},${entry}"
    fi
done <<< "$ids"

result="${result}]"
echo "$result" | jq .
