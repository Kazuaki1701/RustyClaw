#!/bin/bash
# Google Calendar に予定を追加する（許可カレンダーのみ）
# Usage: 508_write-calendar.sh <calendar_id> <summary> <start_datetime> <end_datetime> [description]

set -euo pipefail

# systemd サービスは ~/.cargo/bin を PATH に含まないため補完する
export PATH="$HOME/.cargo/bin:$PATH"

CALENDAR_ID="${1:-}"
SUMMARY="${2:-}"
START="${3:-}"
END="${4:-}"
DESCRIPTION="${5:-}"

# 許可カレンダーリスト（ハードコード）
ALLOWED=(
    "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com"
    "d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com"
)

if [ -z "$CALENDAR_ID" ] || [ -z "$SUMMARY" ] || [ -z "$START" ] || [ -z "$END" ]; then
    echo "Usage: $0 <calendar_id> <summary> <start_datetime> <end_datetime> [description]" >&2
    exit 1
fi

allowed=false
for id in "${ALLOWED[@]}"; do
    if [ "$CALENDAR_ID" = "$id" ]; then
        allowed=true
        break
    fi
done

if [ "$allowed" = false ]; then
    echo "WRITE BLOCKED: calendar '${CALENDAR_ID}' is not in the writable list." >&2
    echo "Allowed: ${ALLOWED[*]}" >&2
    exit 1
fi

if ! command -v gws &>/dev/null; then
    echo "gws not found in PATH" >&2
    exit 1
fi

gws calendar events insert \
    --params "{\"calendarId\":\"${CALENDAR_ID}\"}" \
    --json "{\"summary\":\"${SUMMARY}\",\"description\":\"${DESCRIPTION}\",\"start\":{\"dateTime\":\"${START}\"},\"end\":{\"dateTime\":\"${END}\"}}" \
    --format json \
  | jq '
      def wday_ja: ["日","月","火","水","木","金","土"][(strptime("%Y-%m-%d"))[6]];
      def adj_end: if .end.dateTime then .end.dateTime
                   elif .end.date then (.end.date | strptime("%Y-%m-%d") | mktime - 86400 | strftime("%Y-%m-%d"))
                   else "" end;
      ((.start.date // (.start.dateTime | split("T")[0])) | wday_ja) as $start_wday |
      adj_end as $end |
      (($end | split("T")[0]) | wday_ja) as $end_wday |
      {
          status:      "created",
          title:       (.summary // ""),
          start:       (.start.dateTime // .start.date // ""),
          start_wday:  $start_wday,
          end:         $end,
          end_wday:    $end_wday,
          calendar_id: (.organizer.email // ""),
          event_id:    (.id // "")
      }'
