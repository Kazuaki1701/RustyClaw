# Calendar スキル統合スクリプト実装計画

**Date:** 2026-06-02
**Goal:** 分散した calendar スクリプト（505/508）を単一の `calendar-ops.sh` に統合し、list/create/delete/update の4操作をサブコマンド方式で提供する。書き込み系3操作（create/delete/update）はハードコードされた許可カレンダー ID リストで保護を継続する。

---

## 背景・課題

### 現状の問題
1. **削除・更新スクリプトが存在しない** — チャットで「削除して」「時間変更して」がエラーになる（`509_delete-calendar.sh` / `510_update-calendar.sh` 未実装）。
2. **スクリプトが分散** — 操作ごとにファイルが増えると Discovery テキストの選択肢が増え、特に小型モデルがどれを呼ぶか迷う。
3. **許可リストの分散リスク** — write 操作が増えるたびに各ファイルへ許可リストをコピーすると、保守時に齟齬が生じやすい。

### 設計方針
- **単一ファイル `calendar-ops.sh`** に `case "$CMD"` 方式で統合（obsidian `507_obsidian-ops.sh` と同じパターン）。
- **許可カレンダー ID リストを1か所**で定義し、create/delete/update の全 write 操作で共通利用。
- list（読み取り）は許可リスト不要。
- 既存の曜日（`start_wday`/`end_wday`）・exclusive end 補正ロジックを維持。

---

## ファイル構成

| ファイル | 操作 |
|---|---|
| `[CREATE] skills/calendar/scripts/calendar-ops.sh` | list / create / delete / update を統合 |
| `[DELETE] skills/calendar/scripts/505_get-calendar.sh` | 統合後に削除 |
| `[DELETE] skills/calendar/scripts/508_write-calendar.sh` | 統合後に削除 |
| `[MODIFY] skills/calendar/SKILL.md` | 新スクリプトのサブコマンド仕様に更新 |

---

## calendar-ops.sh の仕様

```
Usage:
  calendar-ops.sh list
  calendar-ops.sh create <calendar_id> <summary> <start_rfc3339> <end_rfc3339> [description]
  calendar-ops.sh delete <calendar_id> <event_id>
  calendar-ops.sh update <calendar_id> <event_id> [--summary <summary>] [--start <start_rfc3339>] [--end <end_rfc3339>] [--description <description>]
```

### 共通ヘッダ
```bash
#!/bin/bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"   # systemd は ~/.cargo/bin を持たない

CMD="${1:-}"

ALLOWED=(
    "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com"
    "d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com"
)

if ! command -v gws &>/dev/null; then
    echo '{"error": "gws not found in PATH"}' >&2
    exit 1
fi

# 許可カレンダーチェック（write 系で共通利用）
check_allowed() {
    local target="$1"
    for id in "${ALLOWED[@]}"; do
        [ "$target" = "$id" ] && return 0
    done
    echo "WRITE BLOCKED: calendar '${target}' is not in the writable allowlist." >&2
    echo "Allowed: ${ALLOWED[*]}" >&2
    exit 1
}

# jq 共通定義（曜日・exclusive end 補正）
JQ_DEFS='
  def wday_ja: ["日","月","火","水","木","金","土"][(strptime("%Y-%m-%d"))[6]];
  def adj_end: if .end.dateTime then .end.dateTime
               elif .end.date then (.end.date | strptime("%Y-%m-%d") | mktime - 86400 | strftime("%Y-%m-%d"))
               else "" end;
'
```

### list（許可リスト不要）
```bash
list)
    now=$(date +%Y-%m-%dT%H:%M:%S%:z)
    end=$(date -d '+7 days' +%Y-%m-%dT%H:%M:%S%:z)
    gws calendar events list \
        --params "{\"calendarId\":\"primary\",\"timeMin\":\"${now}\",\"timeMax\":\"${end}\",\"singleEvents\":true,\"orderBy\":\"startTime\",\"maxResults\":50}" \
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
    ;;
```

**注:** list 出力に `event_id` を追加する（delete/update で必要になるため）。

### create
```bash
create)
    CALENDAR_ID="${2:-}"; SUMMARY="${3:-}"; START="${4:-}"; END="${5:-}"; DESCRIPTION="${6:-}"
    [ -z "$CALENDAR_ID" ] || [ -z "$SUMMARY" ] || [ -z "$START" ] || [ -z "$END" ] && {
        echo "Usage: $0 create <calendar_id> <summary> <start> <end> [description]" >&2; exit 1; }
    check_allowed "$CALENDAR_ID"
    gws calendar events insert \
        --params "{\"calendarId\":\"${CALENDAR_ID}\"}" \
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
```

### delete
```bash
delete)
    CALENDAR_ID="${2:-}"; EVENT_ID="${3:-}"
    [ -z "$CALENDAR_ID" ] || [ -z "$EVENT_ID" ] && {
        echo "Usage: $0 delete <calendar_id> <event_id>" >&2; exit 1; }
    check_allowed "$CALENDAR_ID"
    gws calendar events delete \
        --params "{\"calendarId\":\"${CALENDAR_ID}\",\"eventId\":\"${EVENT_ID}\"}" \
        --format json
    echo "{\"status\":\"deleted\",\"event_id\":\"${EVENT_ID}\"}"
    ;;
```

**注:** delete は成功時に空ボディを返すため、スクリプト側で `{"status":"deleted"}` を出力する。

### update（patch セマンティクス）
```bash
update)
    CALENDAR_ID="${2:-}"; EVENT_ID="${3:-}"
    if [ -z "$CALENDAR_ID" ] || [ -z "$EVENT_ID" ]; then
        echo "Usage: $0 update <calendar_id> <event_id> [options]" >&2
        echo "Options: --summary <val> --start <val> --end <val> --description <val>" >&2
        exit 1
    fi
    check_allowed "$CALENDAR_ID"

    SUMMARY=""
    START=""
    END=""
    DESCRIPTION=""

    shift 3
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --summary)     SUMMARY="${2:-}";     shift 2 ;;
            --start)       START="${2:-}";       shift 2 ;;
            --end)         END="${2:-}";         shift 2 ;;
            --description) DESCRIPTION="${2:-}"; shift 2 ;;
            *) echo "Unknown option: $1" >&2; exit 1 ;;
        esac
    done

    # 指定されたフィールドのみを含む JSON を構築（patch セマンティクス）
    body=$(jq -n \
        --arg s "$SUMMARY" --arg st "$START" --arg en "$END" --arg d "$DESCRIPTION" '
        {}
        + (if $s  != "" then {summary: $s} else {} end)
        + (if $st != "" then {start: {dateTime: $st}} else {} end)
        + (if $en != "" then {end:   {dateTime: $en}} else {} end)
        + (if $d  != "" then {description: $d} else {} end)')
    gws calendar events patch \
        --params "{\"calendarId\":\"${CALENDAR_ID}\",\"eventId\":\"${EVENT_ID}\"}" \
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
```

### フォールバック
```bash
*)
    echo "Usage: $0 {list|create|delete|update} ..." >&2
    exit 1
    ;;
```

---

## SKILL.md の更新内容

- `description` に delete/update を含める。
- Workflow を単一スクリプト `calendar-ops.sh` のサブコマンド表に書き換える。
- 許可カレンダー ID 2件を明記。
- 「delete/update には list で得た `event_id` が必要」と明示。

```markdown
## Operations

run_workspace_script: "skills/calendar/scripts/calendar-ops.sh"

| args[0] | 説明 | 追加 args |
|---|---|---|
| list   | 今後7日の予定取得（event_id 含む） | なし |
| create | 予定作成 | calendar_id, summary, start, end, [description] |
| delete | 予定削除 | calendar_id, event_id |
| update | 予定更新（patch） | calendar_id, event_id, [--summary <val>] [--start <val>] [--end <val>] [--description <val>] |

start/end は RFC3339（例: 2026-06-03T10:00:00+09:00）。
delete/update の event_id は list の出力から取得する。
write 系（create/delete/update）は許可カレンダー2件のみ。
```

---

## 検証計画

### 手動確認（rp1）
1. `list` → event_id 含む JSON が返る
2. `create` 許可カレンダー → `status:"created"` + event_id 返却
3. `delete` 許可カレンダー + 上記 event_id → `status:"deleted"`
4. `update` 許可カレンダー → `status:"updated"` + 変更後の値
5. `create`/`delete`/`update` に**許可外カレンダー ID** → `WRITE BLOCKED` で exit 1
6. 不正な `CMD` → Usage 表示で exit 1

### 後始末
- 統合動作確認後、`505_get-calendar.sh` と `508_write-calendar.sh` を削除。
- production workspace は NAS 共有のため rp1 へは即時反映（バイナリ変更なしのためサービス再起動不要、スクリプトのみ）。

---

## コミット方針
1. `calendar-ops.sh` 作成 + SKILL.md 更新（1コミット）
2. 旧 505/508 削除（1コミット）
