# Daily Briefing

Generate a concise, prioritized daily briefing. Gracefully skips unavailable data sources.

## Execution Flow

```
1. Gather   → Pull data from available sources
2. Analyze  → Prioritize and detect conflicts/urgencies
3. Format   → Produce scannable briefing
4. Deliver  → Output as response text (auto-delivered to channel)
```

## Phase 1: Gather Data

Current date/time is in the `[now: ...]` prefix of the system prompt. Use it to compute "today" and "yesterday".

Execute all available data sources. Skip any source whose tool is unavailable — do not error.

### 1.1 Calendar (Today)

Tool: `gws_calendar_list_events`

Run for each calendar in sequence:
- `calendar_id`: `primary`, `time_min`: today start (YYYY-MM-DDT00:00:00+09:00), `max_results`: `20`
- `calendar_id`: `ayabe.ayumi@gmail.com` (あゆみ様), same time range
- `calendar_id`: `28hs0ibka0oa84810dupunrskk@group.calendar.google.com` (ゆうき様), same time range

Extract: event summary, time, location, attendees.

### 1.2 Email (Unread, Last 24h)

Tool: `gws_gmail_list_messages`
- `query`: `newer_than:1d is:unread`
- `max_results`: `15`

Classify: **Needs Response** (from a person, expects reply) vs **FYI** (notifications, newsletters).

### 1.3 Tasks & TODOs

- Read `MEMORY.md` via system context — look for TODO, task, or reminder sections.
- Read `cron.json` via `workspace_read` — list active cron jobs for awareness.
- Read `TODO.md` via `workspace_read` (if it exists) — check for high-priority items.

### 1.4 Yesterday's Activity & Summaries

For yesterday's date (YYYY-MM-DD):

1. **Daily summary**: `memory/summaries/YYYY-MM-DD-daily.md` — use as primary recap
2. **Session summaries**: `memory/summaries/YYYY-MM-DD-*.md` (excluding `-daily.md` and `-heartbeat-activity.md`)
3. **Daily log** (fallback): `memory/logs/YYYY-MM-DD.md`

Extract: key accomplishments, decisions made, open items carried over.

### 1.5 Weather (Optional)

Tool: `yolp_weather`
- Check `USER.md` (already in system context) for K様の location coordinates (latitude, longitude)
- Extracts rainfall prediction for next 60 minutes in 10-minute intervals

Skip silently if location unknown or tool unavailable.

## Phase 2: Analyze & Prioritize

Review gathered data and identify:

- **Conflicts**: overlapping calendar events
- **Urgencies**: overdue tasks, emails from important senders, meetings starting soon
- **Carryover**: unfinished items from yesterday
- **#1 Priority**: the single most important thing to focus on today

## Phase 3: Format Briefing

Produce a concise briefing. Target: **under 2 minutes to read**.

```markdown
# Daily Briefing — [YYYY-MM-DD] [Day of Week]

[Weather one-liner if available — e.g. "☔ 30分後に雨の可能性あり（0.8mm）"]

## #1 Priority
**[Most important action for today]**
[Why it matters — one sentence]

## Schedule ([N] events)
| Time | Event | Notes |
|------|-------|-------|
| 09:00 | ... | [attendees or prep note] |

[あゆみ様・ゆうき様の予定があれば別行で追記]
[Conflicts or gaps worth noting]

## Email ([N] unread)

**Needs Response:**
- [Sender] — [Subject] ([time ago])

**FYI:**
- [Sender] — [Subject]

## Tasks
- [ ] [Carried over from yesterday]
- [ ] [From MEMORY.md / TODO.md]

## Yesterday ([N] sessions)
**Highlights:**
- [Key accomplishment / decision]

**Carried Over:**
- [ ] [Unfinished item]

## Active Cron Jobs
- [job-id]: [schedule] — [prompt description]
```

### Formatting Rules

- Keep each section to 3–5 items max. If more, show top items and note "(+N more)".
- Use relative time for emails ("2h ago", "yesterday").
- Omit empty sections entirely — do not show "No items" placeholders.
- Use **bold** for key terms, bullet lists for enumeration.

## Phase 4: Deliver

Output the briefing as your response text — it will be auto-delivered to the target channel.
When triggered manually in conversation, reply directly without a formal header.

## Graceful Degradation

| Source | Tool Missing | Behavior |
|--------|-------------|----------|
| Calendar | `gws_calendar_list_events` unavailable | Skip "Schedule" section |
| Email | `gws_gmail_list_messages` unavailable | Skip "Email" section |
| Weather | `yolp_weather` unavailable / no coordinates in USER.md | Skip weather line |
| Yesterday | No summaries or log file for yesterday | Skip "Yesterday" section |
| Tasks | No TODO in MEMORY.md and no TODO.md | Skip "Tasks" section |
