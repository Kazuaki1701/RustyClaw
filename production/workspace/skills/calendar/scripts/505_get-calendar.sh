#!/bin/bash
# Google Calendar の今後7日間の予定を取得し、jq でタイトル・時刻・場所のみ抽出する

set -euo pipefail

if ! command -v gws &>/dev/null; then
    echo '{"error": "gws not found in PATH"}' >&2
    exit 1
fi

now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
end=$(date -u -d '+7 days' +%Y-%m-%dT%H:%M:%SZ)

gws calendar events list \
    --params "{\"calendarId\":\"primary\",\"timeMin\":\"${now}\",\"timeMax\":\"${end}\",\"singleEvents\":true,\"orderBy\":\"startTime\",\"maxResults\":50}" \
    --format json \
  | jq '[.items[]? | {
      title:    (.summary // ""),
      start:    (.start.dateTime // .start.date // ""),
      end:      (.end.dateTime   // .end.date   // ""),
      location: (.location // "")
  }]'
