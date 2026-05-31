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

To interact with KaraKeep, all parameters are securely resolved from **RustyClaw's vault** (`~/.rustyclaw/vault.json`):
*   **Server Address**: Resolved under the key `karakeep-server-addr` (Default: `http://192.168.1.2:33000`).
*   **Authentication**: Bearer Token resolved under the key `karakeep-api-key`.
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
Do NOT run scripts via absolute shell paths. Invoke the script inside the skill's local directory using the secure gateway tool:
*   **Tool**: `run_workspace_script`
*   **Cleanup**: `501_karakeep-cleanup.sh`
*   **Tagging**: `502_karakeep-tag-items.sh <tag_name> <ids...>`

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

*   **Missing Vault Keys**: Attempting to run scripts or API requests without verifying if `karakeep-server-addr` or `karakeep-api-key` are configured in `vault.json`. (Fix: Verify vault variables exist and fail gracefully if missing).
*   **Absolute Path Execution**: Running `bash production/workspace/scripts/501_karakeep-cleanup.sh` directly. (Fix: Invoke through `run_workspace_script` with localized script names).
*   **Accidental Purging**: Deleting non-RSS items or favorited bookmarks. (Fix: Verify script logic filters strictly on source="rss" and favourited=false).
*   **Unstructured Logs**: Dumping unstructured text or raw JSON into the daily log file. (Fix: Always use the standardized Markdown Table format).

---

## Red Flags - STOP and Check Context

- You did not check `USER.md` before compiling recommended bookmarks.
- You did not verify the `createdAt` timestamp is within the 72-hour retrieval window.
- You executed a shell script without using the secure `run_workspace_script` gateway tool.
- You logged raw API payloads or unstructured console output to `memory/logs/`.

**All of these mean: Stop. Apply the KaraKeep Bookmark Management Skill rules immediately.**
