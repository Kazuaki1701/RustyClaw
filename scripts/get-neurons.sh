#!/usr/bin/env bash
# Cloudflare Workers AI Neurons 使用状況を表示するスクリプト。
set -euo pipefail

# 基準となるディレクトリ設定
APP_DIR="${RUSTYCLAW_HOME:-$HOME/.rustyclaw}"
NEURON_FILE="$APP_DIR/neuron_usage.json"
API_URL="http://192.168.1.12:8080/api/neurons"

# デフォルト閾値
LIMIT=10000

# JSONの値をパースしてフォーマット表示する関数
print_usage() {
    local date="$1"
    local used="$2"
    
    # jqを使用して各種数値を算出
    local remaining
    remaining=$(jq -n "$LIMIT - $used")
    local percent
    percent=$(jq -n "($used / $LIMIT) * 100")
    
    echo "=========================================="
    echo "  Cloudflare Workers AI Neurons Usage"
    echo "=========================================="
    echo "Target Date  : $date"
    echo "Neurons Used : $used / $LIMIT"
    printf "Remaining    : %s\n" "$remaining"
    printf "Usage Rate   : %.2f%%\n" "$percent"
    echo "=========================================="
}

if [ -f "$NEURON_FILE" ]; then
    echo "Reading from local neuron usage file: $NEURON_FILE"
    DATE=$(jq -r '.last_reset_date' "$NEURON_FILE")
    USED=$(jq -r '.neurons_used' "$NEURON_FILE")
    print_usage "$DATE" "$USED"
elif curl -s -f -m 2 "$API_URL" > /dev/null; then
    echo "Querying from local gateway API: $API_URL"
    API_RESPONSE=$(curl -s "$API_URL")
    USED=$(echo "$API_RESPONSE" | jq -r '.neurons_used')
    RESETS=$(echo "$API_RESPONSE" | jq -r '.next_reset_jst // "unknown"')
    RESET_IN=$(echo "$API_RESPONSE" | jq -r '.reset_in // "unknown"')
    
    DATE=$(date +%Y-%m-%d)
    print_usage "$DATE" "$USED"
    echo "Next Reset   : $RESETS (in $RESET_IN)"
    echo "=========================================="
else
    echo "Error: Could not retrieve Neuron usage."
    echo "  - Local file not found: $NEURON_FILE"
    echo "  - Gateway API is not responding on: $API_URL"
    echo "  - (No Cloudflare inference has been executed yet today)"
fi
