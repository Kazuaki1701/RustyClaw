# Topic Patrol

Explore the web based on the user's interests and share discoveries like a curious friend — not a news bot.

```
1. Read     → USER.md (Interests + Work Context) + prior state
2. Explore  → Route to the right source tool per topic
3. Filter   → "Would I tell a friend about this?"
4. Share    → Conversational message as your direct response (or stay silent)
5. Record   → Update state + findings log via workspace tools
```

## Trigger Patterns

- **Cron job** — scheduled every 4-8 hours.
- **Manual** — user says "patrol", "track this topic", "what's new in {X}", or "anything interesting lately?"

When triggered manually, reply directly in the conversation. When triggered by cron, your text response is sent to the target channel.

## Execution Flow

### Step 1: Understand the User

1. Read `USER.md` inside your system prompt context:
   - **Interests** section — primary exploration source. Each topic may have an optional `sources:` line (see Source Routing below).
   - **Work Context** section — anchor findings to what the user is currently working on.
   - If Interests is empty, fall back to Work Context topics. If both are empty, skip and stay silent.
2. Read `patrol/state.json` using `workspace_read` (default if missing/error: `{ "lastRun": null, "rotationIndex": 0 }`).
3. Read `patrol/findings.md` using `workspace_read` (default if missing/error: empty) — for deduplication.

### Step 2: Explore

Run **2-3 queries**, rotating through Interests across runs via `rotationIndex`. Wrap around to 0 when it exceeds the number of Interest topics.

#### Source Routing

Each Interest topic in `USER.md` may have an optional `sources:` line. Route queries to the appropriate tool based on source type:

| Source prefix | Tool | Example query |
|---|---|---|
| _(no sources specified)_ | `web_search` + `web_fetch` | `{interest} latest news 2026` |
| `HN` | `web_search` with `site:news.ycombinator.com` | `site:news.ycombinator.com {interest}` |
| `Reddit/{subreddit}` | `web_search` with `site:reddit.com/r/{subreddit}` | `site:reddit.com/r/ {interest}` |
| URL (e.g. `https://blog.nodejs.org`) | `web_fetch` directly | Read the page and look for new content |

**Fallback rule**: If a specific tool is not available, fall back to `web_search` with a `site:` filter or topic keywords. Never error on a missing tool — degrade gracefully.

For promising results, **read the actual page** with `web_fetch` to get substance beyond snippets. Do not curate based on search snippets alone.

### Step 3: Filter — "Would I tell a friend?"

For each finding, consider:
- **Novel?** — not already in `patrol/findings.md`.
- **Interesting?** — not a generic press release or product announcement.
- **Relevant?** — connects to the user's work or stated interests.
- **Worth sharing?** — would make someone say "oh cool, I didn't know that".

If nothing clears the bar, share nothing — silence is better than noise.

### Step 4: Share (only when worth it)

Check the current time via the `[now: ...]` runtime prefix in your system prompt. Respect **quiet hours (23:00–08:00)** based on the user's timezone.
- **During Quiet Hours**: Defer delivery. Do NOT output the findings as your response text. Simply record them in `patrol/findings.md` as `deferred (quiet hours)` and reply with an empty response or absolute silence.
- **Outside Quiet Hours**: Explain WHY it's interesting in a natural, conversational tone. Connect it to the user's current work. Limit to **1-2 topics** per message.

### Step 5: Update State

1. Append to **`patrol/findings.md`** using `workspace_write` (whether shared, skipped, or deferred):
   ```markdown
   ## YYYY-MM-DD
   - {topic}: {one-line summary} — shared / skipped ({reason}) / deferred (quiet hours)
   ```
   **Important**: Parse and prune entries older than 14 days to keep the file small, then write the updated content back.
   
2. Update **`patrol/state.json`** using `workspace_write`:
   ```json
   { "lastRun": "YYYY-MM-DDTHH:MM:SS+09:00", "rotationIndex": 2 }
   ```
   If nothing was found, still update `lastRun` and increment `rotationIndex`.

## Prohibited Patterns

- Formatted news-briefing style (numbered lists, emoji headers, "report" framing)
- Cramming 3+ topics into a single message
- Sharing something just because it's new — it must be genuinely interesting
- Reporting "nothing found" — silence is the correct response
- Sending duplicates without checking `patrol/findings.md`
