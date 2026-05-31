# Workspace — Session Working Directory

When file creation or saving is needed, output to a session-specific directory under `runs/` instead of cluttering the workspace root.

## Trigger Patterns

Use this skill **autonomously** when the following apply:
- File creation or saving is needed (HTML, CSV, reports, images, etc.)
- User requests "write to a file", "create a report", etc.

**Exceptions — edit directly in the workspace root:**
- `MEMORY.md`, `memory/*.md` — Long-term memory / daily logs
- `cron.json` — Scheduled jobs
- `skills/*.md` — Skill definitions
- `TODO.md` — Task list

## Procedure

1. Determine the session ID from the current session context (e.g., the channel/session name visible in the runtime directives, or use the date portion of `[now: ...]`)
2. Create the `runs/{sessionId}/` directory by writing the first file there via `workspace_write`
3. All subsequent file output goes inside this directory
4. When referencing files for the user, use the relative path from the workspace root

## Path Examples

```
runs/discord-general-123456/report.csv
runs/discord-general-123456/chart.html
runs/manual/analysis.md
```

## Notes

- Use the session ID as-is for the directory name (no date needed)
- When creating multiple files in the same session, place them all in the same directory
- Do not change the working root — always use paths relative to the workspace root
