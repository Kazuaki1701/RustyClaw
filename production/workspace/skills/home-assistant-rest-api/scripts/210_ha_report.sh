#!/bin/sh
# ---------------------------------------------------------
# 【機能説明】HomeAssistant 統合ステータスレポート (Main)
# Usage: 210_ha_report.sh [--agent|--summary] [--discord]
# ---------------------------------------------------------
. "$(dirname "$0")/__200_ha_common.sh"

# --- Usage ---
usage() {
    echo "Usage: $(basename "$0") [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --summary        Output summarized text via GMN (Default)"
    echo "  --agent          Output raw-text for AI analysis"
    echo "  --discord        Send the resulting text to Discord"
    echo "  --silent         Silent output, but send to Discord"
    echo "  --help, -h       Show this help message"
    exit 0
}

MODE="summary"
SEND_DISCORD=false
SILENT=false

# --- Argument Parsing (Portable sh) ---
while [ "$#" -gt 0 ]; do
    case "$1" in
        --agent|--raw-text) MODE="agent" ;;
        --summary) MODE="summary" ;;
        --discord) SEND_DISCORD=true ;;
        --silent)  SILENT=true; SEND_DISCORD=true; MODE="summary" ;;
        --help|-h) usage ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
    shift
done

# --- Sub-module Execution ---
RES201=$("$HA_SCRIPT_DIR/201_ha_logs.sh")
RES202=$("$HA_SCRIPT_DIR/202_ha_health.sh")
RES203=$("$HA_SCRIPT_DIR/203_ha_summary.sh")

# POSIX printf for reliable multi-line
RAW_TEXT=$(printf "## Home Assistant Integrated Report: %s\n\n%s\n\n%s\n\n%s" "$(date +"%Y-%m-%d %H:%M:%S")" "$RES201" "$RES202" "$RES203")

if [ "$MODE" = "agent" ]; then
    FINAL_CONTENT="$RAW_TEXT"
else
    # GMN を介して要約
    GMN_BIN=$(command -v gmn 2>/dev/null || echo "/picoclaw_runtime/bin/gmn")
    
    if [ "$SILENT" = "true" ]; then
        FINAL_CONTENT=$(echo "$RAW_TEXT" | "$GMN_BIN" --no-agent --model flash)
    else
        # 進捗（ストリーミング）を画面に出しつつ、変数に格納する
        TMP_OUT=$(mktemp)
        echo "... 要約生成中 (GMN Streaming) ..."
        echo "$RAW_TEXT" | "$GMN_BIN" --no-agent --model flash --accept-raw-output-risk --raw-output | tee "$TMP_OUT"
        FINAL_CONTENT=$(cat "$TMP_OUT")
        rm -f "$TMP_OUT"
    fi
fi

# 表示制御 (SILENT の場合は上の tee で既に出ている可能性があるが一応)
if [ "$SILENT" = "false" ] && [ "$MODE" = "agent" ]; then
    echo "$FINAL_CONTENT"
fi

# Discord 連携 (wget)
if [ "$SEND_DISCORD" = "true" ]; then
    if [ -z "$DISCORD_REPORT_TOKEN" ] || [ -z "$DISCORD_REPORT_CHANNEL_ID" ]; then
        echo "Error: Discord credentials not found." >&2
        exit 1
    fi
    # jq で安全に JSON を構築
    PAYLOAD=$(jq -cn --arg c "$FINAL_CONTENT" '{content: $c}')
    wget -qO- --header="Authorization: Bot ${DISCORD_REPORT_TOKEN}" \
         --header="Content-Type: application/json" \
         --post-data="$PAYLOAD" \
         "https://discord.com/api/v10/channels/${DISCORD_REPORT_CHANNEL_ID}/messages" > /dev/null
fi
