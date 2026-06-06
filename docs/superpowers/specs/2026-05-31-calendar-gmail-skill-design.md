# Design: Calendar & Gmail Skill Migration (Phase 36-B/C)

**Date**: 2026-05-31  
**Phase**: 36-B (Calendar) + 36-C (Gmail)

---

## Overview

Migrate 4 native Rust gws tools into 2 pure shell-script skills:
- `skills/calendar/` — GwsCalendarTool + GwsCalendarWriteTool
- `skills/gmail/` — GwsGmailTool + GwsGmailDeleteTool

`gws` binary is resolved from `$PATH` (no vault injection needed).

---

## Change Scope

| Action | Target |
|---|---|
| Create | `production/workspace/skills/calendar/SKILL.md` |
| Create | `production/workspace/skills/calendar/scripts/505_get-calendar.sh` |
| Create | `production/workspace/skills/calendar/scripts/508_write-calendar.sh` |
| Create | `production/workspace/skills/gmail/SKILL.md` |
| Create | `production/workspace/skills/gmail/scripts/506_get-gmail.sh` |
| Create | `production/workspace/skills/gmail/scripts/509_delete-gmail.sh` |
| Delete | `crates/rustyclaw-tools/src/lib.rs` — `GwsCalendarTool` (line 358–), `GwsCalendarWriteTool` (line 432–), `GwsGmailTool` (line 538–), `GwsGmailDeleteTool` (line 604–) and all related tests |
| Delete | `crates/rustyclaw-gateway/src/lib.rs` — entire gws registration block (lines 739–817) |

---

## Calendar Skill

### `505_get-calendar.sh`

Calls `gws calendar events list` for the primary calendar, today through 7 days ahead. Filters via `jq` to output a JSON array of events.

**Output format:**
```json
[
  {"title": "チーム会議", "start": "2026-06-01T10:00:00+09:00", "end": "2026-06-01T11:00:00+09:00", "location": "会議室A"}
]
```

Fields: `title` (summary), `start`, `end`, `location` (empty string if absent). All other fields (id, reminders, attendees, etc.) are stripped.

**gws command:**
```bash
gws calendar events list \
  --params "{\"calendarId\":\"primary\",\"timeMin\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"timeMax\":\"$(date -u -d '+7 days' +%Y-%m-%dT%H:%M:%SZ)\",\"singleEvents\":true,\"orderBy\":\"startTime\"}" \
  --format json
```

Exits with code 1 and error message if `gws` is not found or returns non-zero.

---

### `508_write-calendar.sh <calendar_id> <summary> <start_datetime> <end_datetime> [description]`

**Allowed calendar IDs (hardcoded):**
```
6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com
d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com
```

Guard: if `$1` is not in the allowed list, print error to stderr and `exit 1` without calling gws.

**gws command (on guard pass):**
```bash
gws calendar events insert \
  --params "{\"calendarId\":\"${calendar_id}\"}" \
  --json "{\"summary\":\"${summary}\",\"description\":\"${description}\",\"start\":{\"dateTime\":\"${start}\"},\"end\":{\"dateTime\":\"${end}\"}}" \
  --format json
```

Outputs the created event JSON on success.

---

### `calendar/SKILL.md` frontmatter

```yaml
name: calendar
description: Use when the user asks to check, list, or create Google Calendar events. Covers reading upcoming schedules and writing new events to allowed calendars.
```

**Workflow**: read via `505_get-calendar.sh` (no env), write via `508_write-calendar.sh` (no env). SKILL.md documents the 2 allowed calendar IDs.

---

## Gmail Skill

### `506_get-gmail.sh [query] [max_results]`

- Default: `query="is:unread"`, `max_results=10`
- Calls `gws gmail users messages list` then fetches each message's metadata.

**Output format (JSON array):**
```json
[
  {
    "id": "18f3a2b1c4d5e6f7",
    "sender": "someone@example.com",
    "subject": "件名",
    "date": "2026-05-31T09:00:00+09:00",
    "snippet": "メッセージの冒頭..."
  }
]
```

Fields: `id`, `sender`, `subject`, `date`, `snippet`. All other metadata stripped via `jq`.

---

### `509_delete-gmail.sh <message_id>`

1. Fetch message metadata: `gws gmail users messages get --params '{"userId":"me","id":"<id>"}'`
2. Check `labelIds` array contains `_ai-agent` (case-insensitive). If not, print error and `exit 1`.
3. On guard pass: `gws gmail users messages trash --params '{"userId":"me","id":"<id>"}'`

Outputs trash confirmation JSON on success.

---

### `gmail/SKILL.md` frontmatter

```yaml
name: gmail
description: Use when the user asks to check unread emails, search Gmail, or trash AI-agent-labeled messages.
```

**Workflow**: list via `506_get-gmail.sh`, delete via `509_delete-gmail.sh`. SKILL.md explicitly states the `_ai-agent` label guard requirement.

---

## Rust Cleanup

**`crates/rustyclaw-tools/src/lib.rs`** — delete:
- `GwsCalendarTool` struct + `impl` (line ~358 to line ~431)
- `GwsCalendarWriteTool` struct + `impl` (line ~432 to line ~535)
- `GwsGmailTool` struct + `impl` (line ~538 to line ~600)
- `GwsGmailDeleteTool` struct + `impl` (line ~604 to end of impl)
- Test `test_gws_gmail_tool_name_and_schema` (~line 1497)
- Tests `test_gws_gmail_delete_*` (~lines 1654–1682)

**`crates/rustyclaw-gateway/src/lib.rs`** — delete:
- Entire gws block: `// Google Workspace ネイティブツール登録` through closing `}` (~lines 739–817)

Run `cargo test` — all remaining tests must pass.
