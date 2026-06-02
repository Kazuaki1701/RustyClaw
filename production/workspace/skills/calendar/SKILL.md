---
name: calendar
description: Use when the user asks to check, list, or create Google Calendar events. Covers reading upcoming schedules and writing new events to allowed calendars.
---

# Calendar Skill

## Overview
Reads upcoming Google Calendar events and creates new events via the `gws` CLI. Write operations are guarded by a hardcoded allowlist — only the two permitted calendars can receive new events.

---

## When to Use

### Triggering Scenarios:
- The user asks about today's or this week's schedule.
- The user asks to add, create, or schedule a calendar event.
- Any scheduled calendar patrol cron triggers.

### When NOT to use:
- Calendars outside the permitted allowlist.

---

## Workflow

- **Tool**: `run_workspace_script`
- **Parameters**:
  - `script_name`: `skills/calendar/scripts/calendar-ops.sh`
  - `args`: `["<subcommand>", ...]`

### Operations

| args[0] | 説明 | 追加 args |
|---|---|---|
| list   | 今後7日の予定取得（event_id 含む） | なし |
| create | 予定作成 | calendar_id, summary, start, end, [description] |
| delete | 予定削除 | calendar_id, event_id |
| update | 予定更新（patch） | calendar_id, event_id, [--summary <val>] [--start <val>] [--end <val>] [--description <val>] |

`delete`/`update` の `event_id` は `list` の出力から取得します。
`start`/`end` は RFC3339 形式（例: `2026-06-01T10:00:00+09:00`）。

**Permitted calendar IDs:**
- `6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com` (AI AGENT)
- `d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com` (学習計画カレンダー)

Any other `calendar_id` will be blocked with `WRITE BLOCKED` and exit code 1.

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `run_workspace_script` を使うこと。
- **許可外カレンダー ID への書き込みは不可。** 常に上記2件の ID を使うこと。
- **start/end は RFC3339 形式**（例: `2026-06-01T10:00:00+09:00`）。
