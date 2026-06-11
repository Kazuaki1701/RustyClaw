---
name: karakeep
description: Use when an AI agent needs to access, clean up, tag, or match user interests with bookmarks inside the KaraKeep server.
---

# KaraKeep Bookmark Management Skill

## Overview
Manages bookmarks inside the self-hosted KaraKeep server, executing periodic cleanup for stale RSS feeds and applying daily recommendations based on the user's profile interests.

---

## When to Use

### Triggering Symptoms / Scenarios:
- The daily `job-karakeep-daily-recommendation` cron triggers (4:45 AM).
- The daily `job-karakeep-auto-cleanup` cron triggers (4:00 AM).
- The user requests manual bookmark tagging, cleanup, or recommendation analysis.

### When NOT to use:
- Managing bookmarks on third-party public cloud services (e.g. Pocket, Raindrop).
- General web research or fetching links not related to the KaraKeep database.

---

## Prerequisites & Endpoints

To interact with KaraKeep, connection parameters are configured as follows:
*   **Server Address**: Configured directly in the environment variables as `http://192.168.1.2:33000`.
*   **Authentication**: Managed automatically via the `KARAKEEP_API_KEY` environment variable. The agent MUST NOT attempt to read `vault.json` or `vault.enc` directly.
*   **Bookmarks Fetch Endpoint**: `[server-address]/api/v1/bookmarks`

---

## The Core Safeguard Rules

### 1. Safety Guardrails for Auto-Cleanup
Before running the cleanup, you **MUST** ensure the deletion target selection criteria strictly complies with these safeguards:
*   **RSS Origin Only**: Only delete bookmarks whose `"source"` field is exactly `"rss"`.
*   **No Favourites**: Never delete bookmarks where `"favourited"` is `true`.
*   **Protected Tags**: Do not delete items carrying any of these tags: `_bookmarked`, `_star`, `_doitlater`, or `_recommended`.

### 2. Precise Interest Matching (Daily Recommendation)
*   **Source**: Load K's interests from `/workspace/USER.md` under the `## Interests` header.
*   **Retrieval Scope**: Fetch bookmarks from the Endpoint and filter for items created within the **last 3 days (72 hours)** by comparing `bookmarks[].createdAt` to the current system time.
*   **Rule**: Perform case-insensitive matching on bookmark titles and descriptions. If a bookmark mentions any extracted interest keyword (e.g., "AI", "Cloudflare", "Obsidian"), mark it as a MATCH.
*   **Action**: Apply the tag `_recommended` strictly to the matched IDs.

---

## Pattern Implementation

### Step 1: Execution (Level 3)
Do NOT run scripts via absolute shell paths. Use `ctx_execute` with `language: bash`. `KARAKEEP_SERVER_ADDR` はコードに直接埋め込む。`KARAKEEP_API_KEY` は Phase 49-2 の vault キャッシュ機構で systemd 環境変数として解決予定。
*   **Tool**: `ctx_execute`
*   **Scripts Available**:
    *   `503_karakeep-list.sh`: Retrieves recent bookmarks list.
        *   `language`: `bash`
        *   `code`: `KARAKEEP_SERVER_ADDR=http://192.168.1.2:33000 bash workspace/skills/karakeep/scripts/503_karakeep-list.sh <limit>`
    *   `502_karakeep-tag-items.sh`: Tags matching bookmark IDs.
        *   `language`: `bash`
        *   `code`: `KARAKEEP_SERVER_ADDR=http://192.168.1.2:33000 bash workspace/skills/karakeep/scripts/502_karakeep-tag-items.sh _recommended <id1> <id2> ...`
    *   `501_karakeep-cleanup.sh`: Automated daily stale RSS items purge.
        *   `language`: `bash`
        *   `code`: `KARAKEEP_SERVER_ADDR=http://192.168.1.2:33000 bash workspace/skills/karakeep/scripts/501_karakeep-cleanup.sh`

### Step 2: Standardized Logging Format (Level 2)
Append all execution summaries to `production/workspace/memory/logs/YYYY-MM-DD.md` in the following structured layout:

```markdown
- [HH:MM:SS] Cron Job: job-karakeep-[daily-recommendation / auto-cleanup] completed:
  | Metric | Result |
  | :--- | :--- |
  | **Matched Interests** | AI, Obsidian, Cloudflare (parsed from USER.md) |
  | **Processed Bookmarks** | N total items retrieved (last 3 days) |
  | **Recommended Actions** | Tagged IDs: `<id1>`, `<id2>` with `_recommended` |
  | **Deleted Items** | M stale RSS bookmarks purged (cleanup job only) |
  
  **Details:**
  - Recommended: "[Title 1]" (ID: `<id1>`) - Matched: `Obsidian`
  - Recommended: "[Title 2]" (ID: `<id2>`) - Matched: `Cloudflare`
```

---

## Common Mistakes & Antipatterns

*   **Missing Vault Keys**: Attempting to read `vault.json` or `vault.enc` directly. (Fix: Rely on the system-injected `KARAKEEP_API_KEY` environment variable and let the scripts handle authentication. If the environment variable is missing or scripts fail, fail gracefully).
*   **Absolute Path Execution**: Running `bash production/workspace/scripts/501_karakeep-cleanup.sh` directly. (Fix: Invoke through `ctx_execute` with `language: bash` and workspace-relative path).
*   **Accidental Purging**: Deleting non-RSS items or favorited bookmarks. (Fix: Verify script logic filters strictly on source="rss" and favourited=false).
*   **Unstructured Logs**: Dumping unstructured text or raw JSON into the daily log file. (Fix: Always use the standardized Markdown Table format).

---

## Red Flags - STOP and Check Context

- You did not check `USER.md` before compiling recommended bookmarks.
- You did not verify the `createdAt` timestamp is within the 72-hour retrieval window.
- You executed a shell script without using `ctx_execute`.
- You logged raw API payloads or unstructured console output to `memory/logs/`.

**All of these mean: Stop. Apply the KaraKeep Bookmark Management Skill rules immediately.**
