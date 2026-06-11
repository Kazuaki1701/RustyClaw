---
name: session-logs
description: 過去の会話セッション履歴（sessions/*.jsonl）の分析やキーワード検索を高速に行うためのスキル。
allowed-tools:
  - ctx_execute
  - ctx_search
---
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

Use `ctx_search` for fast full-text BM25 search across all session summaries:

```
ctx_search: { "query": "your keyword" }
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

For token usage, tool frequency, and other structured analysis, use the secure pre-packaged scripts within the skill directory:

### Count messages & analyze tokens
Use `ctx_execute` to get session message counts and SQLite-based token usage statistics:
- **Tool**: `ctx_execute`
- **Parameters**:
  - `language`: `bash`
  - `code`: `bash workspace/skills/session-logs/scripts/session-stats.sh` (optional: append `--days 7` or `--date YYYYMMDD`)

### Search full session history
Use `ctx_execute` to search raw `.jsonl` files for deep keyword matching:
- **Tool**: `ctx_execute`
- **Parameters**:
  - `language`: `bash`
  - `code`: `bash workspace/skills/session-logs/scripts/session-search.sh "your keyword"` (optional: append `--date 20260531`)

## Searching Summaries by Topic

Use `ctx_search` for keyword-based summary retrieval:
```json
ctx_search: { "query": "Garmin vitals coach" }
ctx_search: { "query": "cron job failure" }
ctx_search: { "query": "MEMORY.md update" }
```

## Tips

- Use `ctx_search` first — it searches all summaries instantly via BM25.
- Fall back to `workspace_read` on specific files when you need the full conversation text.
- Session filenames encode the channel and date — use them to narrow down time ranges.
- For token cost analysis, use `skills/session-logs/scripts/session-stats.sh` (see Advanced Analysis).
