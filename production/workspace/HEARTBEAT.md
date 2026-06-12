# Heartbeat — Memory & Awareness

You are running a periodic background check (every ~30 min). Review recent activity and act only when something genuinely needs attention.

## Quiet hours (0:00–4:59)

Check the current local time (`config.timezone`). During **0:00–4:59**, only act on truly urgent items (critical emails, imminent deadlines). Do NOT send proactive check-ins or casual reminders.

## Step 1: Review recent activity

Read the `Recent activity digest` in the user message. Look for:
- **Incomplete work** — tasks started but not finished, or explicitly "later" / "TODO"
- **Errors or failures** — unresolved errors or failed builds
- **New decisions or preferences** — things worth noting
- **Anything unusual** — patterns that seem off

## Step 2: Weather & Home Environment alert

### Weather
If the user message contains a weather alert, include a concise notification. Do not fetch weather yourself.

### Home Environment (HA)
If the user message contains a `Home Environment:` line (e.g., `[HA_ENV|HH:MM] [Room: ...°C↑ / ...%] [CO2: ...ppm↑] ...`):
- 室温が **↑** トレンドかつ 30°C 超 → 熱中症リスクとして触れる（夏季のみ）
- CO2 が **↑** トレンドかつ 1000 ppm 超 → 換気を促すワンライナーを添える
- `[HA SPIKE ALERT]` が user message に含まれる場合 → **必ず** Important 扱いで通知。HEARTBEAT_OK を返してはいけない。

HA コンテキストが存在しない場合はこのステップを静かにスキップする。

## Step 3: Calendar & Email check

### Calendar
- Activate `[use-skill: calendar]`.
- Run via `ctx_execute`: `language: shell`, `code: bash workspace/skills/calendar/scripts/calendar-ops.sh list_family`.
- If an event starts within 30 minutes and not yet notified, include a reminder.
- For tomorrow's events: mention once in the evening only.

### Email
- Activate `[use-skill: gmail]`.
- Run via `ctx_execute`: `language: shell`, `code: bash workspace/skills/gmail/scripts/506_get-gmail.sh`.
- If urgent or important unread emails exist, summarize and include.
- **費用発生の可能性がある案件は必ず Important として通知する（金額・サービス名・期日を添えること）。**
- Skip routine/automated emails.

If skills or scripts are unavailable, skip silently.

## Step 4: Check-in if silent too long

If 8+ hours have passed since last user interaction (`lastChecks.lastUserContact` in `memory/heartbeat-state.json`), send a short check-in — **during waking hours only, never during quiet hours (0:00–4:59)** — one sentence.

## Step 5: Proactive work

- If a session had unresolved errors → notify with context
- If work was left incomplete and enough time has passed → send a reminder
- If you spotted something the user should know → tell them

**Prohibited:** Do NOT run `topic-patrol`, web searches, or deliver `patrol/findings.md`. Topic Patrol runs as a separate scheduled job.

## Step 6: Response

**Severity guide:** Important = action needed (urgent email, imminent deadline, unresolved error, cost/service alert). Informational = worth noting but no action. Nothing = no findings.

**If all findings are Informational or Nothing → Silent run:**

Respond with exactly:

```
HEARTBEAT_OK
```

**Nothing else.**

---

**If any finding is Important → Discord notification:**

- Write a concise alert (2–5 lines, Japanese).
- **Do NOT include `HEARTBEAT_OK` anywhere in the response.**
