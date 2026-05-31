# Session Logs

A skill for searching and analyzing RustyClaw session history.

## File Structure

### Session JSONL (conversation history)
Location: `sessions/<sessionId>.jsonl`

One entry per line — simple message format:
```json
{"role": "user", "content": "..."}
{"role": "assistant", "content": "..."}
```

Session IDs follow the pattern: `discord-C{channelId}-{YYYYMMDD}`, `cron-{jobId}`, `cli-session`, etc.

### Summaries (LLM-generated)
Location: `memory/summaries/YYYY-MM-DD-<slug>.md`

Auto-generated when a session goes idle. Contains TL;DR, key decisions, and conversation highlights.

### Daily Logs
Location: `memory/logs/YYYY-MM-DD.md`

Agent-written activity notes, appended throughout the day.

## Common Queries

### Keyword search in summaries (recommended)

Use `memory_search` for fast full-text BM25 search across all session summaries:

```
memory_search: { "query": "your keyword" }
```

### List recent session files

Use `workspace_read` to list the sessions directory:

```
workspace_read: { "path": "sessions/" }
```

### Read a specific session

```
workspace_read: { "path": "sessions/discord-C1485590981251432560-20260531.jsonl" }
```

### Search summaries for a specific date

Use `workspace_read` to list and read summaries:

```
workspace_read: { "path": "memory/summaries/" }
```
Then read `memory/summaries/YYYY-MM-DD-*.md` files of interest.

### Read daily log

```
workspace_read: { "path": "memory/logs/YYYY-MM-DD.md" }
```

## Advanced Analysis via Scripts

For token usage, tool frequency, and other structured analysis, write a temporary analysis script and execute it:

**Step 1** — Write the script via `workspace_write`:
```bash
# Example: scripts/session-stats.sh
#!/bin/bash
# Count messages per session
for f in sessions/*.jsonl; do
  count=$(wc -l < "$f")
  echo "$count $f"
done | sort -rn | head -20
```

**Step 2** — Run it via `run_workspace_script`:
```
run_workspace_script: { "script_name": "session-stats.sh" }
```

> **Note**: Token usage statistics are tracked in SQLite (`memory.db`). A dedicated analysis script in `scripts/` can query this database for cost breakdowns.

## Searching Summaries by Topic

```
# Use memory_search for keyword-based retrieval
memory_search: { "query": "Garmin vitals coach" }
memory_search: { "query": "cron job failure" }
memory_search: { "query": "MEMORY.md update" }
```

## Tips

- Use `memory_search` first — it searches all summaries instantly via BM25
- Fall back to `workspace_read` on specific files when you need the full conversation text
- Session filenames encode the channel and date — use them to narrow down time ranges
- For token cost analysis, a dedicated script querying `memory.db` is needed (see Advanced Analysis)
