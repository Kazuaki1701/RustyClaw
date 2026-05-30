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
    "karakeepPatrol": "2026-03-09T06:00:00Z",
    "lastUserContact": "2026-03-09T12:00:00Z"
  }
}
```

## Quiet hours (0:00–4:59)

Check the current local time. During **0:00–4:59**, only act on truly urgent items
(critical emails, imminent deadlines). Do NOT send proactive check-ins or casual reminders.

## How to notify

**RustyClaw が agent の返答テキストを自動的に Discord の home channel へ投稿する。**
通知が必要な場合はレスポンステキストに含めるだけでよい。特別なツール呼び出しは不要。

## Step 1: Review recent activity (every run)

Read these sources to build a picture of what's been happening:

1. `memory/heartbeat-digest.md` — auto-generated session deltas since last run
2. Most recent files in `memory/summaries/` — session summaries (decisions, errors, pending work)
3. `memory/logs/YYYY-MM-DD.md` — daily activity log

As you review, look for:
- **Incomplete work** — tasks started but not finished, or explicitly "later" / "TODO"
- **Errors or failures** — sessions that ended with unresolved errors
- **New decisions or preferences** — things the user said worth remembering long-term

## Step 2: Memory maintenance (every few hours)

Periodically (2–4 times per day, not every run):

1. Review recent `memory/summaries/` and `memory/logs/` files
2. Update `MEMORY.md` with distilled learnings from recent sessions
3. Remove outdated info from `MEMORY.md`
4. If recent sessions reveal new interests, add them to `USER.md` Interests section

Check `lastChecks.memoryMaintenance` — if less than 3 hours ago, skip.

## Step 3: Calendar & Email check (every run)

### Calendar
- Use `gws_calendar_list_events` tool with `calendar_id: "primary"`, from: now, to: end of tomorrow
- If an event starts within 30 minutes and not yet notified, include a reminder in your response
- For tomorrow's events: mention once in the evening — don't repeat in subsequent runs
- Note any scheduling conflicts

### Email
- Use `gws_gmail_list_messages` tool with `query: "newer_than:1h is:unread"`, `max_results: 10`
- If urgent or important unread emails exist, summarize and include in your response
- Skip routine/automated emails (newsletters, CI notifications, etc.)

If the required tools are not available, skip silently.

## Step 4: Weather check

天気ツールは現在未実装のため、このステップは常にスキップする。

## Step 5: Check-in if silent too long

If it's been **8+ hours** since the last user interaction, send a lightweight check-in:
- "Anything you need?" / "Quiet day — let me know if anything comes up"
- Keep it short and natural
- Only during waking hours (respect quiet hours above)
- Do NOT check in if the user has been actively chatting

## Step 6: Proactive work (rotate)

- If a session had unresolved errors → notify the user with context
- If work was left incomplete and enough time has passed → send a reminder

### Background tasks (no permission needed)
- Read and organize memory files
- Update documentation that's gone stale
- Clean up old or redundant memory entries

## Step 7: Karakeep Patrol (periodic)

1. Use `karakeep_list_bookmarks` tool to fetch recent bookmarks
2. Filter items tagged `_bookmarked` or `_doitlater`
3. For `_doitlater`: extract actionable tasks and prepare reminders
4. For `_bookmarked`: evaluate applicability to K様の環境と Obsidian

Check `lastChecks.karakeepPatrol` — if less than 12 hours ago, skip.

## Step 8: Response

If no critical issues and no notifications sent:
- **Respond with `HEARTBEAT_OK`** (pipeline signal)
- **Do NOT post `HEARTBEAT_OK` to Discord.** （"Heartbeat OK の場合は、Discord への報告不要"）

If critical issues were found or notifications sent, provide a concise summary.

After responding, update `memory/heartbeat-state.json` with timestamps for checks you ran.
