---
name: topic-patrol
description: 登録された技術情報源（RSS、Webサイト）を定期巡回し、新規トピックを要約・収集するパトロールスキル。
allowed-tools:
  - web_search
  - web_fetch
  - workspace_read
  - workspace_write
---
# Topic Patrol

Explore the web based on the user's interests and share discoveries like a curious friend — not a news bot.

## What you receive in the user message

The system provides the following in the user message:

- `配信:` — determines which mode to run:
  - `配信: スキップ` → **探索モード**: explore topics, record as deferred, no Discord output
  - `配信: 許可` → **配信モード**: deliver previously deferred findings to Discord, no new exploration

**Read this value first and choose the correct execution flow below.**

## Execution Flow

> **Before starting:** Check `配信:` in the user message.
> - `配信: スキップ` → follow **探索モード** (Steps 1–5 below)
> - `配信: 許可` → follow **配信モード** (Deliver Mode below), then go to **Step 5-3 only**

---

### Deliver Mode（`配信: 許可` のとき）

1. Call `workspace_read` on `patrol/findings.md`.
2. Find all entries with status `deferred (quiet hours)` that have **not** been delivered yet. An entry is considered **already delivered** if `patrol/findings.md` contains a line with `delivered` status that references the same topic or URL — skip such entries.
3. From these, select the **1–2 most interesting** entries. Pick entries that satisfy all four: **Novel** (not a duplicate of something already shared), **Interesting** (not a generic press release or product announcement), **Relevant** (connects to the user's work or stated interests), and **Worth sharing** (would make someone say "oh cool, I didn't know that"). If nothing clears all four, skip delivery.
4. If no deferred entries exist, respond with nothing and go to Step 5-3.
5. For each selected entry, post to Discord:
   - Write a short, natural explanation of WHY it is interesting.
   - End with the source as a Markdown link: `[タイトル](URL)`
6. Append a `delivered` record to `patrol/findings.md` using `workspace_write` with `mode: append`:
   ```
   - {topic}: {summary} — delivered (from deferred {YYYY-MM-DD})
     Source: {URL}
   ```
7. For each delivered URL, call `ctx_execute`:
   - `language`: `bash`
   - `code`: `bash workspace/skills/topic-patrol/scripts/511_karakeep-add-bookmark.sh "{URL}"`
8. Go to **Step 5-3** (update state.json). Skip Steps 1–5.

---

### Step 1: Read prior findings and select topics
> _(探索モードのみ。配信モードは上の Deliver Mode セクションを参照)_

1. Call `workspace_read` on `patrol/findings.md`. If the file is missing, treat it as empty.
2. From the `## Interests` section of `USER.md` (already in your system context), pick **3 topics** to investigate this run. Select topics that do **not** appear in the most recent `##` section of `patrol/findings.md`. This ensures natural rotation without repeating recent topics.
3. If all topics appear in the most recent section, pick any 3 freely.

### Step 2: Investigate each topic

For **each of the 3 selected topics**, do the following in order:

1. Call `web_search` with the query `{topic} latest 2026`.
2. Call `web_fetch` on the **top URL** from the search results.
   - This step is **mandatory**. Do not skip it.
   - If `web_fetch` fails, record the URL as `unverified` and continue.

#### Source Routing

If a topic in `USER.md` has a `sources:` annotation, route as follows:

| Source prefix | Action |
|---|---|
| _(none)_ | `web_search` → `web_fetch` top URL |
| `HN` | `web_search` with `site:news.ycombinator.com {topic}` |
| `Reddit/{sub}` | `web_search` with `site:reddit.com/r/{sub} {topic}` |
| `github:{owner}/{repo}` | `web_fetch https://github.com/{owner}/{repo}/releases` — scan latest release notes |
| `rss:{url}` | `web_fetch {url}` — parse feed entries and pick the newest item |
| URL | `web_fetch` the URL directly |

#### Work-adjacent Query（任意 — 探索モードのみ）

After investigating the 3 selected topics, add **1 optional query** based on the user's current Work Context (`## Work Context` in `USER.md`):

- Pick a technology or tool mentioned in Work Context that was NOT one of the 3 selected topics.
- Search: `{tool} best practices 2026` or `{tool} tips 2026`.
- Apply the same web_fetch step and filter criteria.
- If Work Context is empty or matches the selected topics, skip this step.

### Step 3: Filter — "Would I tell a friend?"

For each finding, check all four:
- **Novel?** — not already in `patrol/findings.md`
- **Interesting?** — not a generic press release or product announcement
- **Relevant?** — connects to the user's work or stated interests
- **Worth sharing?** — would make someone say "oh cool, I didn't know that"

If nothing passes all four, do not share anything. Silence is the correct response.

### Step 4: Deliver

Check the `配信:` value in the user message:

**`配信: スキップ`** — Do NOT output any findings as response text. Go directly to Step 5 and record as `deferred (quiet hours)`. Reply with nothing.

**`配信: 許可`** — Write a short, natural response (1–2 topics max). Explain WHY it is interesting. Connect it to the user's current work. End with the source as a Markdown link on its own line:

```
[記事タイトルまたはサイト名](https://verified-url-from-web-fetch)
```

Use the page title obtained from `web_fetch`. Never omit this line.

### Step 5: Record

**5-0. Prune `patrol/findings.md`** by calling `ctx_execute`:
- `language`: `bash`
- `code`: `bash workspace/skills/topic-patrol/scripts/510_prune-findings.sh`

This removes old sections automatically. Do not skip this step.

**5-1. Append to `patrol/findings.md`** using `workspace_write` with `mode: append`.

Write one entry per topic investigated (whether shared, skipped, or deferred):

```
## YYYY-MM-DD
- {topic}: {one-line summary} — shared / skipped ({reason}) / deferred (quiet hours)
  Source: {URL from web_fetch, or "unverified" if web_fetch failed}
```

Do not rewrite or delete existing content. Only append.

**5-2. Register the URL in KaraKeep** (探索モード `配信: スキップ` 時のみ。配信モードは Deliver Mode Step 7 で登録済みのため不要):

For each URL shared in Step 4, call `ctx_execute`:
- `language`: `bash`
- `code`: `bash workspace/skills/topic-patrol/scripts/511_karakeep-add-bookmark.sh "{shared URL}"`

This registers the URL with the `_ai-patrol` tag. Skip if the URL is "unverified".

**5-3. Write `patrol/state.json`** using `workspace_write` with `mode: write`:

```json
{ "lastRun": "{current datetime in ISO 8601}" }
```

## Prohibited Patterns

- Sharing a finding without a source link in the response
- Reporting "nothing found" — silence is the correct response
- Sending duplicates (check `patrol/findings.md` first)
- Formatted news-briefing style (numbered lists, emoji headers, "report" framing)
- Cramming 3+ topics into a single message
- Rewriting or pruning `patrol/findings.md` — only append
- Bookmarking an unverified URL (web_fetch failed) to KaraKeep
- Picking the same 3 topics as the most recent `##` section in `patrol/findings.md`
