#!/bin/bash
# Gmail メッセージをゴミ箱に移動する（_ai-agent ラベル付きのみ許可）
# Usage: 509_delete-gmail.sh <message_id>

set -euo pipefail

# systemd サービスは ~/.cargo/bin を PATH に含まないため補完する
export PATH="$HOME/.cargo/bin:$PATH"

MESSAGE_ID="${1:-}"
REQUIRED_LABEL="_ai-agent"

if [ -z "$MESSAGE_ID" ]; then
    echo "Usage: $0 <message_id>" >&2
    exit 1
fi

if ! command -v gws &>/dev/null; then
    echo "gws not found in PATH" >&2
    exit 1
fi

# メッセージのラベルを取得
labels=$(gws gmail users messages get \
    --params "{\"userId\":\"me\",\"id\":\"${MESSAGE_ID}\",\"format\":\"metadata\"}" \
    --format json \
  | jq -r '.labelIds[]? // empty' 2>/dev/null)

# _ai-agent ラベルの存在確認（大文字小文字を区別しない）
has_label=false
while IFS= read -r label; do
    if [[ "${label,,}" == "${REQUIRED_LABEL,,}" ]]; then
        has_label=true
        break
    fi
done <<< "$labels"

if [ "$has_label" = false ]; then
    echo "DELETE BLOCKED: message '${MESSAGE_ID}' does not have the '${REQUIRED_LABEL}' label." >&2
    exit 1
fi

# ゴミ箱に移動
gws gmail users messages trash \
    --params "{\"userId\":\"me\",\"id\":\"${MESSAGE_ID}\"}" \
    --format json
