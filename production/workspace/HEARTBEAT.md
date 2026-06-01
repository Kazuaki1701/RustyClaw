# Heartbeat — Memory & Awareness

You are running a periodic background check (every ~30 min). Your goal is to
maintain awareness of recent activity and act on anything that needs attention.

Not every check needs to run every time. Use `memory/heartbeat-state.json` to
track when you last ran each check, and rotate through them intelligently:

```json
{
  "lastChecks": {
    "activityReview": "2026-03-09T12:00:00Z",
    "memoryMaintenance": "2026-03-09T09:00:00Z",
    "calendar": "2026-03-09T12:00:00Z",
    "email": "2026-03-09T12:00:00Z",
    "weather": "2026-03-09T06:00:00Z",
    "karakeepPatrol": "2026-03-09T06:00:00Z",
    "lastUserContact": "2026-03-09T12:00:00Z"
  }
}
```

## Quiet hours (0:00–4:59)

Check the current local time (use `config.timezone`).
During **0:00–4:59**, only act on truly urgent items (critical emails, imminent deadlines).
Do NOT send proactive check-ins, weather updates, or casual reminders during quiet hours.

## How to notify

RustyClaw automatically posts the agent's text response directly to the Discord home channel. If any notifications or alerts are required, simply include the notification text as part of your final response content. No special tool calls are required to post messages to Discord.

## Step 1: Review recent activity (every run)

Read these sources to build a picture of what's been happening:

1. `memory/heartbeat-digest.md` — auto-generated session deltas since last run
2. Most recent files in `memory/summaries/` — session summaries (decisions, errors, pending work)
3. `memory/logs/YYYY-MM-DD.md` — daily activity log

As you review, look for:
- **Incomplete work** — tasks started but not finished, or explicitly "later" / "TODO"
- **Errors or failures** — sessions that ended with unresolved errors, failed builds, broken tests
- **New decisions or preferences** — things the user said worth remembering long-term
- **Anything unusual** — patterns that seem off, or context you think the user would want to know about

## Step 2: Memory maintenance (every few hours)

Periodically (2–4 times per day, not every run):

1. Review recent `memory/summaries/` and `memory/logs/` files
2. Update `MEMORY.md` with distilled learnings from recent sessions
3. Remove outdated info from `MEMORY.md`
4. If recent sessions reveal new interests, add them to `USER.md` Interests section

Think of it like reviewing your journal and updating your mental model.
Session summaries are raw notes; MEMORY.md is curated wisdom.

Check `lastChecks.memoryMaintenance` — if less than 3 hours ago, skip.

## Step 3: Calendar & Email check (every run)

Check these on **every heartbeat** — they change frequently and the user needs timely awareness.

### Calendar
- Activate the `calendar` skill (`[use-skill: calendar]`).
- Execute the script `skills/calendar/scripts/505_get-calendar.sh` via the `run_workspace_script` tool (no arguments required).
- If an event starts within 30 minutes and not yet notified, include a reminder in your response.
- For tomorrow's events: mention once in the evening — don't repeat in subsequent runs.
- Note any scheduling conflicts.

### Email
- Activate the `gmail` skill (`[use-skill: gmail]`).
- Execute the script `skills/gmail/scripts/506_get-gmail.sh` via the `run_workspace_script` tool (no arguments required).
- If urgent or important unread emails exist, summarize and include in your response.
- Skip routine/automated emails (newsletters, CI notifications, etc.).

If the required skills or scripts are not available, skip silently.

## Step 4: Weather check (2–3 times per day)

- Activate the `weather` skill (`[use-skill: weather]`).
- Execute the script `skills/weather/scripts/504_get-weather.sh` via the `run_workspace_script` tool (no arguments required).
- Morning, midday, and evening are good times to check.
- Notify if: rain/snow is expected within 60 minutes, extreme temperatures are forecasted, or significant changes occur.
- Check `lastChecks.weather` — if it has been less than 4 hours, skip.

## Step 5: Check-in if silent too long

If it's been **8+ hours** since the last user interaction (check `lastChecks.lastUserContact` and recent session timestamps), send a lightweight check-in:
- "Anything you need?" / "Quiet day — let me know if anything comes up"
- Keep it short and natural, not robotic
- Only during waking hours (respect quiet hours above)
- Do NOT check in if the user has been actively chatting in other sessions

## Step 6: Proactive work (rotate)

- If a session had unresolved errors or failed tasks → notify the user with context
- If work was left incomplete and enough time has passed → send a reminder
- If you spotted something the user should know about → tell them

### Background tasks (no permission needed)
- Read and organize memory files
- Check on projects (git status, pending PRs, etc.)
- Update documentation that's gone stale
- Clean up old or redundant memory entries

### Use your judgment
You have full context of the user's recent activity. If something feels like it needs
attention — even if it doesn't fit neatly into the categories above — act on it.
The user trusts you to be a proactive assistant, not a passive checklist runner.

## Step 7: Karakeep Patrol (periodic)

1. Activate the `karakeep` skill (`[use-skill: karakeep]`).
2. Execute the script `skills/karakeep/scripts/503_karakeep-list.sh` via the `run_workspace_script` tool with arguments `["20"]` to fetch recent bookmarks.
3. Filter items tagged `_bookmarked` or `_doitlater`.
4. For `_doitlater`: extract actionable tasks and prepare reminders.
5. For `_bookmarked`: evaluate applicability to K-sama's environment and Obsidian.
6. If a valuable use case or configuration is identified, prepare a summary for the next briefing or notify immediately if it's a critical tool.

Check `lastChecks.karakeepPatrol` — if less than 12 hours ago, skip.

## Step 8: Response

If no critical issues and no notifications sent:
- **Respond with `HEARTBEAT_OK`** (pipeline signal)
- **Do NOT post `HEARTBEAT_OK` to Discord.** (Per directive: "Heartbeat OK の場合は、Discord への報告不要")

If critical issues were found or notifications sent, provide a concise summary.

After responding, update `memory/heartbeat-state.json` with timestamps for checks you ran.
