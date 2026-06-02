#!/bin/bash
# Google Calendar の今後7日間の予定を取得し、jq でタイトル・時刻・場所のみ抽出する

set -euo pipefail

# systemd サービスは ~/.cargo/bin を PATH に含まないため補完する
export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v gws &>/dev/null; then
    echo '{"error": "gws not found in PATH"}' >&2
    exit 1
fi

now=$(date +%Y-%m-%dT%H:%M:%S%:z)
end=$(date -d '+7 days' +%Y-%m-%dT%H:%M:%S%:z)

gws calendar events list \
    --params "{\"calendarId\":\"primary\",\"timeMin\":\"${now}\",\"timeMax\":\"${end}\",\"singleEvents\":true,\"orderBy\":\"startTime\",\"maxResults\":50}" \
    --format json \
  | jq '[.items[]? | {
      title:    (.summary // ""),
      start:    (.start.dateTime // .start.date // ""),
      end:      (
                  if .end.dateTime then .end.dateTime
                  elif .end.date then
                    # 全日イベントの end は翌日 (exclusive)。1日引いて実際の最終日にする
                    (.end.date | strptime("%Y-%m-%d") | mktime - 86400 | strftime("%Y-%m-%d"))
                  else "" end
                ),
      location: (.location // "")
  }]'
