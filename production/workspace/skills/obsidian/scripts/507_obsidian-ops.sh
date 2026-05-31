#!/bin/bash
# Obsidian Local REST API クライアント
# Usage:
#   507_obsidian-ops.sh search <query> [limit]
#   507_obsidian-ops.sh read   <vault-relative-path>
#   507_obsidian-ops.sh write  <vault-relative-path> <content>
#   507_obsidian-ops.sh append <vault-relative-path> <content>

set -euo pipefail

HOST="http://192.168.1.2:27123"
TOKEN="${OBSIDIAN_TOKEN:-}"
CMD="${1:-}"

if [ -z "$TOKEN" ]; then
    echo "Error: OBSIDIAN_TOKEN is not set." >&2
    exit 1
fi

url_encode() {
    python3 -c "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "$1"
}

auth_header() {
    echo "Authorization: Bearer ${TOKEN}"
}

case "$CMD" in
    search)
        QUERY="${2:-}"
        LIMIT="${3:-10}"
        if [ -z "$QUERY" ]; then
            echo "Usage: $0 search <query> [limit]" >&2
            exit 1
        fi
        encoded=$(url_encode "$QUERY")
        curl -sf -X POST \
            -H "$(auth_header)" \
            "${HOST}/search/simple/?query=${encoded}&contextLength=100" \
          | jq --argjson limit "$LIMIT" \
              '.[:$limit] | map({path: .filename, excerpt: (.matches[0].context // "")})'
        ;;

    read)
        PATH_ARG="${2:-}"
        if [ -z "$PATH_ARG" ]; then
            echo "Usage: $0 read <vault-relative-path>" >&2
            exit 1
        fi
        encoded=$(url_encode "$PATH_ARG")
        curl -sf \
            -H "$(auth_header)" \
            "${HOST}/vault/${encoded}"
        ;;

    write)
        PATH_ARG="${2:-}"
        CONTENT="${3:-}"
        if [ -z "$PATH_ARG" ] || [ -z "$CONTENT" ]; then
            echo "Usage: $0 write <vault-relative-path> <content>" >&2
            exit 1
        fi
        encoded=$(url_encode "$PATH_ARG")
        status=$(curl -sf -o /dev/null -w "%{http_code}" -X PUT \
            -H "$(auth_header)" \
            -H "Content-Type: text/markdown" \
            --data-raw "$CONTENT" \
            "${HOST}/vault/${encoded}")
        if [ "$status" = "200" ] || [ "$status" = "204" ]; then
            echo "Written to ${PATH_ARG}"
        else
            echo "Obsidian API error: HTTP ${status}" >&2
            exit 1
        fi
        ;;

    append)
        PATH_ARG="${2:-}"
        CONTENT="${3:-}"
        if [ -z "$PATH_ARG" ] || [ -z "$CONTENT" ]; then
            echo "Usage: $0 append <vault-relative-path> <content>" >&2
            exit 1
        fi
        encoded=$(url_encode "$PATH_ARG")
        existing=$(curl -sf \
            -H "$(auth_header)" \
            "${HOST}/vault/${encoded}" 2>/dev/null || echo "")
        combined="${existing%$'\n'}
${CONTENT}"
        status=$(curl -sf -o /dev/null -w "%{http_code}" -X PUT \
            -H "$(auth_header)" \
            -H "Content-Type: text/markdown" \
            --data-raw "$combined" \
            "${HOST}/vault/${encoded}")
        if [ "$status" = "200" ] || [ "$status" = "204" ]; then
            echo "Appended to ${PATH_ARG}"
        else
            echo "Obsidian API error: HTTP ${status}" >&2
            exit 1
        fi
        ;;

    *)
        echo "Usage: $0 {search|read|write|append} <args>" >&2
        echo "  search <query> [limit]" >&2
        echo "  read   <vault-relative-path>" >&2
        echo "  write  <vault-relative-path> <content>" >&2
        echo "  append <vault-relative-path> <content>" >&2
        exit 1
        ;;
esac
