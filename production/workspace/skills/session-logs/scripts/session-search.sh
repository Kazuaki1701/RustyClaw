#!/bin/bash
# session-search.sh — セッション JSONL をキーワード検索
#
# Usage:
#   session-search.sh <keyword> [--workspace DIR] [--date YYYYMMDD] [--context N]
#
# Options:
#   --workspace DIR    ワークスペースルート（省略時はスクリプトの親ディレクトリ）
#   --date YYYYMMDD    セッション名にこの文字列を含むものだけ対象にする
#   --context N        マッチ前後 N 行も表示（デフォルト: 0）
#
# LLM からの呼び出し例:
#   ctx_execute: { "language": "bash", "code": "bash workspace/skills/session-logs/scripts/session-search.sh \"Garmin\"" }
#   ctx_execute: { "language": "bash", "code": "bash workspace/skills/session-logs/scripts/session-search.sh \"memory flush\" --date 20260531" }

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DATE_FILTER=""
CONTEXT_LINES=0

# キーワードは最初の引数（オプションでない場合）
KEYWORD="${1:-}"
if [ -z "$KEYWORD" ]; then
  echo "Usage: session-search.sh <keyword> [--workspace DIR] [--date YYYYMMDD] [--context N]"
  exit 1
fi
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace) WORKSPACE_DIR="$2"; shift 2 ;;
    --date)      DATE_FILTER="$2";   shift 2 ;;
    --context)   CONTEXT_LINES="$2"; shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

SESSIONS_DIR="$WORKSPACE_DIR/sessions"

echo "=== Searching: \"$KEYWORD\" ==="

if [ ! -d "$SESSIONS_DIR" ]; then
  echo "No matches found for \"$KEYWORD\"."
  exit 0
fi

FOUND=0

for f in "$SESSIONS_DIR"/*.jsonl; do
  [ -f "$f" ] || continue
  session=$(basename "$f" .jsonl)

  if [ -n "$DATE_FILTER" ] && [[ "$session" != *"$DATE_FILTER"* ]]; then
    continue
  fi

  matches=$(grep -ic "$KEYWORD" "$f" 2>/dev/null || true)
  [ -z "$matches" ] || [ "$matches" -eq 0 ] && continue

  echo ""
  echo "--- $session  ($matches match(es)) ---"
  FOUND=$((FOUND + 1))

  # マッチ行を role + content の形式で表示（最大 20 件、200 文字で切り詰め）
  grep -in "$KEYWORD" "$f" | head -20 | while IFS=: read -r linenum line; do
    role=$(echo "$line" | grep -o '"role":"[^"]*"' | head -1 | sed 's/"role":"//;s/"//')
    content=$(echo "$line" | grep -o '"content":"[^"]*"' | head -1 | sed 's/"content":"//;s/"$//')
    if [ ${#content} -gt 200 ]; then
      content="${content:0:200}..."
    fi
    printf "  L%s [%s]: %s\n" "$linenum" "${role:-?}" "${content:-<parse error>}"
  done
done

echo ""
if [ "$FOUND" -eq 0 ]; then
  echo "No matches found for \"$KEYWORD\"."
else
  echo "Found in $FOUND session(s)."
fi
