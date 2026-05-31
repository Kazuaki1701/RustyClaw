#!/bin/bash
# session-stats.sh — セッション一覧・メッセージ数・SQLite トークン集計
#
# Usage:
#   session-stats.sh [--workspace DIR] [--date YYYY-MM-DD] [--days N]
#
# Options:
#   --workspace DIR    ワークスペースルート（省略時はスクリプトの親ディレクトリ）
#   --date YYYYMMDD    セッション名にこの文字列を含むものだけ表示
#   --days N           トークン集計の対象期間（デフォルト: 14日）
#
# LLM からの呼び出し例:
#   run_workspace_script: { "script_name": "session-stats.sh" }
#   run_workspace_script: { "script_name": "session-stats.sh", "args": ["--date", "20260531"] }
#   run_workspace_script: { "script_name": "session-stats.sh", "args": ["--days", "7"] }

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DATE_FILTER=""
DAYS=14

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace) WORKSPACE_DIR="$2"; shift 2 ;;
    --date)      DATE_FILTER="$2";   shift 2 ;;
    --days)      DAYS="$2";          shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

SESSIONS_DIR="$WORKSPACE_DIR/sessions"
DB="$WORKSPACE_DIR/memory.db"

echo "=== Session Files ==="
if [ -d "$SESSIONS_DIR" ]; then
  found=0
  for f in "$SESSIONS_DIR"/*.jsonl; do
    [ -f "$f" ] || continue
    session=$(basename "$f" .jsonl)
    if [ -n "$DATE_FILTER" ] && [[ "$session" != *"$DATE_FILTER"* ]]; then
      continue
    fi
    count=$(wc -l < "$f")
    size=$(du -h "$f" | cut -f1)
    echo "  $session  ${count} msgs  $size"
    found=$((found + 1))
  done
  [ "$found" -eq 0 ] && echo "  (no sessions found)"
else
  echo "  (sessions directory not found: $SESSIONS_DIR)"
fi

echo ""
echo "=== Token Usage (last ${DAYS} days) ==="
if command -v sqlite3 >/dev/null 2>&1 && [ -f "$DB" ]; then
  sqlite3 -separator '|' "$DB" "
    SELECT
      date(created_at)             AS date,
      COUNT(*)                     AS runs,
      SUM(total_tokens)            AS total_tokens,
      SUM(prompt_tokens)           AS input,
      SUM(completion_tokens)       AS output
    FROM usage
    WHERE date(created_at) >= date('now', '-${DAYS} days')
    GROUP BY date(created_at)
    ORDER BY date DESC;
  " | awk -F'|' 'BEGIN{printf "  %-12s %6s %12s %10s %10s\n","date","runs","total","input","output"}
    {printf "  %-12s %6s %12s %10s %10s\n",$1,$2,$3,$4,$5}'

  echo ""
  echo "=== By Model (last ${DAYS} days) ==="
  sqlite3 -separator '|' "$DB" "
    SELECT model, COUNT(*), SUM(total_tokens)
    FROM usage
    WHERE date(created_at) >= date('now', '-${DAYS} days')
    GROUP BY model ORDER BY SUM(total_tokens) DESC;
  " | awk -F'|' 'BEGIN{printf "  %-30s %6s %12s\n","model","runs","tokens"}
    {printf "  %-30s %6s %12s\n",$1,$2,$3}'

  echo ""
  echo "=== By Trigger Type (last ${DAYS} days) ==="
  sqlite3 -separator '|' "$DB" "
    SELECT COALESCE(trigger_type,'unknown'), COUNT(*), SUM(total_tokens)
    FROM usage
    WHERE date(created_at) >= date('now', '-${DAYS} days')
    GROUP BY trigger_type ORDER BY SUM(total_tokens) DESC;
  " | awk -F'|' 'BEGIN{printf "  %-20s %6s %12s\n","trigger","runs","tokens"}
    {printf "  %-20s %6s %12s\n",$1,$2,$3}'
else
  echo "  sqlite3 not available or memory.db not found."
  echo "  Install with: sudo apt-get install sqlite3"
  echo "  (Token stats are available on rp1 where sqlite3 is installed)"
fi
