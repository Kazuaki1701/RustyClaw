#!/bin/bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

CMD="${1:-}"

# 各カレンダーIDのハードコード
CAL_KAZUAKI="ayabe.kazuaki@gmail.com" # かずあき様
CAL_YUKI="28hs0ibka0oa84810dupunrskk@group.calendar.google.com" # ゆうき
CAL_AYUMI="ayabe.ayumi@gmail.com"   # あゆみ
CAL_AI_AGENT="6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com" #_AI_AGENT
CAL_STUDY="d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com" # 学習計画カレンダー

# 許可カレンダーリストのハードコード（write系チェック用）
ALLOWED=(
    "$CAL_AI_AGENT"
    "$CAL_STUDY"   
)

if ! command -v gws &>/dev/null; then
    echo '{"error": "gws not found in PATH"}' >&2
    exit 1
fi

check_allowed() {
    local target="$1"
    for id in "${ALLOWED[@]}"; do
        [ "$target" = "$id" ] && return 0
    done
    echo "WRITE BLOCKED: calendar '${target}' is not in the writable allowlist." >&2
    echo "Allowed: ${ALLOWED[*]}" >&2
    exit 1
}

JQ_DEFS='
  def wday_ja: ["日","月","火","水","木","金","土"][(strptime("%Y-%m-%d"))[6]];
  def adj_end: if .end.dateTime then .end.dateTime
               elif .end.date then (.end.date | strptime("%Y-%m-%d") | mktime - 86400 | strftime("%Y-%m-%d"))
               else "" end;
'

# イベント一覧取得のヘルパー関数
fetch_events() {
    local cal_id="$1"
    local now="$2"
    local end="$3"
    gws calendar events list \
        --params "{\"calendarId\":\"${cal_id}\",\"timeMin\":\"${now}\",\"timeMax\":\"${end}\",\"singleEvents\":true,\"orderBy\":\"startTime\",\"maxResults\":50}" \
        --format json \
      | jq "${JQ_DEFS}"'
          [.items[]? |
            ((.start.date // (.start.dateTime | split("T")[0])) | wday_ja) as $start_wday |
            adj_end as $end |
            (($end | split("T")[0]) | wday_ja) as $end_wday |
            {
                event_id:    (.id // ""),
                title:       (.summary // ""),
                start:       (.start.dateTime // .start.date // ""),
                start_wday:  $start_wday,
                end:         $end,
                end_wday:    $end_wday,
                location:    (.location // "")
            }]'
}

now=$(date +%Y-%m-%dT%H:%M:%S%:z)
end=$(date -d '+3 days' +%Y-%m-%dT%H:%M:%S%:z)

case "$CMD" in
    list_family)
        res1=$(fetch_events "$CAL_KAZUAKI" "$now" "$end")
        res2=$(fetch_events "$CAL_YUKI" "$now" "$end")
        res3=$(fetch_events "$CAL_AYUMI" "$now" "$end")
        
        jq -n --argjson r1 "$res1" --argjson r2 "$res2" --argjson r3 "$res3" \
            '$r1 + $r2 + $r3 | sort_by(.start)'
        ;;

    list_ai_agent)
        fetch_events "$CAL_AI_AGENT" "$now" "$end"
        ;;

    create_ai_agent)
        SUMMARY="${2:-}"; START="${3:-}"; END="${4:-}"; DESCRIPTION="${5:-}"
        if [ -z "$SUMMARY" ] || [ -z "$START" ] || [ -z "$END" ]; then
            echo "Usage: $0 create_ai_agent <summary> <start> <end> [description]" >&2
            exit 1
        fi
        check_allowed "$CAL_AI_AGENT"
        gws calendar events insert \
            --params "{\"calendarId\":\"${CAL_AI_AGENT}\"}" \
            --json "{\"summary\":\"${SUMMARY}\",\"description\":\"${DESCRIPTION}\",\"start\":{\"dateTime\":\"${START}\"},\"end\":{\"dateTime\":\"${END}\"}}" \
            --format json \
          | jq "${JQ_DEFS}"'
              ((.start.date // (.start.dateTime | split("T")[0])) | wday_ja) as $start_wday |
              adj_end as $end |
              (($end | split("T")[0]) | wday_ja) as $end_wday |
              { status:"created", event_id:(.id//""), title:(.summary//""),
                start:(.start.dateTime//.start.date//""), start_wday:$start_wday,
                end:$end, end_wday:$end_wday, calendar_id:(.organizer.email//"") }'
        ;;

    delete_ai_agent)
        EVENT_ID="${2:-}"
        if [ -z "$EVENT_ID" ]; then
            echo "Usage: $0 delete_ai_agent <event_id>" >&2
            exit 1
        fi
        check_allowed "$CAL_AI_AGENT"
        gws calendar events delete \
            --params "{\"calendarId\":\"${CAL_AI_AGENT}\",\"eventId\":\"${EVENT_ID}\"}" \
            --format json
        echo "{\"status\":\"deleted\",\"event_id\":\"${EVENT_ID}\"}"
        ;;

    update_ai_agent)
        EVENT_ID="${2:-}"
        if [ -z "$EVENT_ID" ]; then
            echo "Usage: $0 update_ai_agent <event_id> [options]" >&2
            echo "Options: --summary <val> --start <val> --end <val> --description <val>" >&2
            exit 1
        fi
        check_allowed "$CAL_AI_AGENT"

        SUMMARY=""
        START=""
        END=""
        DESCRIPTION=""

        shift 2
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --summary)     SUMMARY="${2:-}";     shift 2 ;;
                --start)       START="${2:-}";       shift 2 ;;
                --end)         END="${2:-}";         shift 2 ;;
                --description) DESCRIPTION="${2:-}"; shift 2 ;;
                *) echo "Unknown option: $1" >&2; exit 1 ;;
            esac
        done

        body=$(jq -n \
            --arg s "$SUMMARY" --arg st "$START" --arg en "$END" --arg d "$DESCRIPTION" '
            {}
            + (if $s  != "" then {summary: $s} else {} end)
            + (if $st != "" then {start: {dateTime: $st}} else {} end)
            + (if $en != "" then {end:   {dateTime: $en}} else {} end)
            + (if $d  != "" then {description: $d} else {} end)')

        gws calendar events patch \
            --params "{\"calendarId\":\"${CAL_AI_AGENT}\",\"eventId\":\"${EVENT_ID}\"}" \
            --json "$body" \
            --format json \
          | jq "${JQ_DEFS}"'
              ((.start.date // (.start.dateTime | split("T")[0])) | wday_ja) as $start_wday |
              adj_end as $end |
              (($end | split("T")[0]) | wday_ja) as $end_wday |
              { status:"updated", event_id:(.id//""), title:(.summary//""),
                start:(.start.dateTime//.start.date//""), start_wday:$start_wday,
                end:$end, end_wday:$end_wday }'
        ;;

    *)
        echo "Usage: $0 {list_family|list_ai_agent|create_ai_agent|delete_ai_agent|update_ai_agent} ..." >&2
        exit 1
        ;;
esac
