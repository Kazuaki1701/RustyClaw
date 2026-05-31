# Calendar & Gmail Skill Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate GwsCalendarTool, GwsCalendarWriteTool, GwsGmailTool, and GwsGmailDeleteTool (all Rust-native) into two pure shell-script skills (`skills/calendar/` and `skills/gmail/`), then delete the Rust implementations.

**Architecture:** Create bash scripts + SKILL.md files for each skill domain, verify script output against the live `gws` CLI, then delete the 4 Rust structs and their gateway registration block. Calendar and Gmail tasks are independently committable.

**Tech Stack:** bash, jq, gws CLI (in PATH), Rust (deletion only), cargo test

---

## File Map

| Action | Path |
|---|---|
| Create | `production/workspace/skills/calendar/scripts/505_get-calendar.sh` |
| Create | `production/workspace/skills/calendar/scripts/508_write-calendar.sh` |
| Create | `production/workspace/skills/calendar/SKILL.md` |
| Create | `production/workspace/skills/gmail/scripts/506_get-gmail.sh` |
| Create | `production/workspace/skills/gmail/scripts/509_delete-gmail.sh` |
| Create | `production/workspace/skills/gmail/SKILL.md` |
| Modify (delete) | `crates/rustyclaw-tools/src/lib.rs` — lines 358–600 (3 tools) + line 604– (GwsGmailDeleteTool) + tests 1487–1686 |
| Modify (delete) | `crates/rustyclaw-gateway/src/lib.rs` — lines 739–817 (gws registration block) |

---

## Task 1: Create `505_get-calendar.sh`

**Files:**
- Create: `production/workspace/skills/calendar/scripts/505_get-calendar.sh`

- [ ] **Step 1: Create directories**

```bash
mkdir -p production/workspace/skills/calendar/scripts
```

- [ ] **Step 2: Write the script**

Create `production/workspace/skills/calendar/scripts/505_get-calendar.sh`:

```bash
#!/bin/bash
# Google Calendar の今後7日間の予定を取得し、jq でタイトル・時刻・場所のみ抽出する

set -euo pipefail

if ! command -v gws &>/dev/null; then
    echo '{"error": "gws not found in PATH"}' >&2
    exit 1
fi

now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
end=$(date -u -d '+7 days' +%Y-%m-%dT%H:%M:%SZ)

gws calendar events list \
    --params "{\"calendarId\":\"primary\",\"timeMin\":\"${now}\",\"timeMax\":\"${end}\",\"singleEvents\":true,\"orderBy\":\"startTime\",\"maxResults\":50}" \
    --format json \
  | jq '[.items[]? | {
      title:    (.summary // ""),
      start:    (.start.dateTime // .start.date // ""),
      end:      (.end.dateTime   // .end.date   // ""),
      location: (.location // "")
  }]'
```

- [ ] **Step 3: Make executable**

```bash
chmod +x production/workspace/skills/calendar/scripts/505_get-calendar.sh
```

- [ ] **Step 4: Run and verify output structure**

```bash
bash production/workspace/skills/calendar/scripts/505_get-calendar.sh | jq 'type'
```

Expected: `"array"`

- [ ] **Step 5: Verify each element has the 4 required keys**

```bash
bash production/workspace/skills/calendar/scripts/505_get-calendar.sh \
  | jq '.[0] | keys | sort'
```

Expected: `["end", "location", "start", "title"]` (or `[]` if calendar is empty — both are acceptable)

- [ ] **Step 6: Commit**

```bash
git add production/workspace/skills/calendar/scripts/505_get-calendar.sh
git commit -m "feat(calendar): add 505_get-calendar.sh"
```

---

## Task 2: Create `508_write-calendar.sh`

**Files:**
- Create: `production/workspace/skills/calendar/scripts/508_write-calendar.sh`

- [ ] **Step 1: Write the script**

Create `production/workspace/skills/calendar/scripts/508_write-calendar.sh`:

```bash
#!/bin/bash
# Google Calendar に予定を追加する（許可カレンダーのみ）
# Usage: 508_write-calendar.sh <calendar_id> <summary> <start_datetime> <end_datetime> [description]

set -euo pipefail

CALENDAR_ID="${1:-}"
SUMMARY="${2:-}"
START="${3:-}"
END="${4:-}"
DESCRIPTION="${5:-}"

# 許可カレンダーリスト（ハードコード）
ALLOWED=(
    "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com"
    "d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com"
)

if [ -z "$CALENDAR_ID" ] || [ -z "$SUMMARY" ] || [ -z "$START" ] || [ -z "$END" ]; then
    echo "Usage: $0 <calendar_id> <summary> <start_datetime> <end_datetime> [description]" >&2
    exit 1
fi

allowed=false
for id in "${ALLOWED[@]}"; do
    if [ "$CALENDAR_ID" = "$id" ]; then
        allowed=true
        break
    fi
done

if [ "$allowed" = false ]; then
    echo "WRITE BLOCKED: calendar '${CALENDAR_ID}' is not in the writable list." >&2
    echo "Allowed: ${ALLOWED[*]}" >&2
    exit 1
fi

if ! command -v gws &>/dev/null; then
    echo "gws not found in PATH" >&2
    exit 1
fi

gws calendar events insert \
    --params "{\"calendarId\":\"${CALENDAR_ID}\"}" \
    --json "{\"summary\":\"${SUMMARY}\",\"description\":\"${DESCRIPTION}\",\"start\":{\"dateTime\":\"${START}\"},\"end\":{\"dateTime\":\"${END}\"}}" \
    --format json
```

- [ ] **Step 2: Make executable**

```bash
chmod +x production/workspace/skills/calendar/scripts/508_write-calendar.sh
```

- [ ] **Step 3: Verify guard blocks unlisted calendar**

```bash
bash production/workspace/skills/calendar/scripts/508_write-calendar.sh \
    "personal@gmail.com" "Test" "2026-06-01T10:00:00+09:00" "2026-06-01T11:00:00+09:00" 2>&1
echo "exit code: $?"
```

Expected: stderr contains `WRITE BLOCKED`, exit code `1`

- [ ] **Step 4: Verify guard passes for allowed calendar (no gws exec needed — just check exit code is NOT 1 due to guard)**

```bash
# 許可 ID でガードをすり抜けられるか確認（gws が存在すれば実行される。存在しなければ "gws not found" エラー）
bash production/workspace/skills/calendar/scripts/508_write-calendar.sh \
    "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com" \
    "Test" "2026-06-01T10:00:00+09:00" "2026-06-01T11:00:00+09:00" 2>&1 | head -1
```

Expected: does NOT contain `WRITE BLOCKED` (may show gws output or "gws not found")

- [ ] **Step 5: Commit**

```bash
git add production/workspace/skills/calendar/scripts/508_write-calendar.sh
git commit -m "feat(calendar): add 508_write-calendar.sh with allowlist guard"
```

---

## Task 3: Create `calendar/SKILL.md`

**Files:**
- Create: `production/workspace/skills/calendar/SKILL.md`

- [ ] **Step 1: Write the SKILL.md**

Create `production/workspace/skills/calendar/SKILL.md`:

```markdown
---
name: calendar
description: Use when the user asks to check, list, or create Google Calendar events. Covers reading upcoming schedules and writing new events to allowed calendars.
---

# Calendar Skill

## Overview
Reads upcoming Google Calendar events and creates new events via the `gws` CLI. Write operations are guarded by a hardcoded allowlist — only the two permitted calendars can receive new events.

---

## When to Use

### Triggering Scenarios:
- The user asks about today's or this week's schedule.
- The user asks to add, create, or schedule a calendar event.
- Any scheduled calendar patrol cron triggers.

### When NOT to use:
- Deleting or modifying existing events (not supported).
- Calendars outside the permitted allowlist.

---

## Workflow

### Read: list upcoming events

- **Tool**: `run_workspace_script`
- **Parameters**:
  - `script_name`: `skills/calendar/scripts/505_get-calendar.sh`
  - *(no `env` required)*

Returns a JSON array of events for the next 7 days. Each element: `{title, start, end, location}`.

### Write: create a new event

- **Tool**: `run_workspace_script`
- **Parameters**:
  - `script_name`: `skills/calendar/scripts/508_write-calendar.sh`
  - `args`: `["<calendar_id>", "<summary>", "<start_datetime_RFC3339>", "<end_datetime_RFC3339>", "<description>"]`
  - *(no `env` required)*

**Permitted calendar IDs:**
- `6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com` (AI AGENT)
- `d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com` (学習計画カレンダー)

Any other `calendar_id` will be blocked with `WRITE BLOCKED` and exit code 1.

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `run_workspace_script` を使うこと。
- **許可外カレンダー ID への書き込みは不可。** 常に上記2件の ID を使うこと。
- **start/end は RFC3339 形式**（例: `2026-06-01T10:00:00+09:00`）。
```

- [ ] **Step 2: Verify YAML frontmatter**

```bash
head -4 production/workspace/skills/calendar/SKILL.md
```

Expected:
```
---
name: calendar
description: Use when the user asks to check, list, or create Google Calendar events. Covers reading upcoming schedules and writing new events to allowed calendars.
---
```

- [ ] **Step 3: Commit**

```bash
git add production/workspace/skills/calendar/SKILL.md
git commit -m "feat(calendar): add calendar SKILL.md"
```

---

## Task 4: Create `506_get-gmail.sh`

**Files:**
- Create: `production/workspace/skills/gmail/scripts/506_get-gmail.sh`

- [ ] **Step 1: Create directories**

```bash
mkdir -p production/workspace/skills/gmail/scripts
```

- [ ] **Step 2: Write the script**

Create `production/workspace/skills/gmail/scripts/506_get-gmail.sh`:

```bash
#!/bin/bash
# Gmail のメッセージ一覧を取得し、id/sender/subject/date/snippet のみ抽出する
# Usage: 506_get-gmail.sh [query] [max_results]

set -euo pipefail

QUERY="${1:-is:unread}"
MAX="${2:-10}"

if ! command -v gws &>/dev/null; then
    echo '{"error": "gws not found in PATH"}' >&2
    exit 1
fi

# メッセージ ID 一覧を取得
ids=$(gws gmail users messages list \
    --params "{\"userId\":\"me\",\"q\":\"${QUERY}\",\"maxResults\":${MAX}}" \
    --format json \
  | jq -r '.messages[]?.id // empty')

if [ -z "$ids" ]; then
    echo "[]"
    exit 0
fi

# 各メッセージのヘッダーを取得して整形
result="["
first=true
while IFS= read -r id; do
    meta=$(gws gmail users messages get \
        --params "{\"userId\":\"me\",\"id\":\"${id}\",\"format\":\"metadata\",\"metadataHeaders\":[\"From\",\"Subject\",\"Date\"]}" \
        --format json)

    entry=$(echo "$meta" | jq --arg id "$id" '{
        id:      $id,
        sender:  ([.payload.headers[]? | select(.name == "From")  | .value][0] // ""),
        subject: ([.payload.headers[]? | select(.name == "Subject")| .value][0] // ""),
        date:    ([.payload.headers[]? | select(.name == "Date")  | .value][0] // ""),
        snippet: (.snippet // "")
    }')

    if [ "$first" = true ]; then
        result="${result}${entry}"
        first=false
    else
        result="${result},${entry}"
    fi
done <<< "$ids"

result="${result}]"
echo "$result" | jq .
```

- [ ] **Step 3: Make executable**

```bash
chmod +x production/workspace/skills/gmail/scripts/506_get-gmail.sh
```

- [ ] **Step 4: Run and verify output type**

```bash
bash production/workspace/skills/gmail/scripts/506_get-gmail.sh "is:unread" 3 | jq 'type'
```

Expected: `"array"`

- [ ] **Step 5: Verify each element has the 5 required keys**

```bash
bash production/workspace/skills/gmail/scripts/506_get-gmail.sh "is:unread" 1 \
  | jq '.[0] | keys | sort'
```

Expected: `["date", "id", "sender", "snippet", "subject"]` (or `[]` if no unread mail)

- [ ] **Step 6: Commit**

```bash
git add production/workspace/skills/gmail/scripts/506_get-gmail.sh
git commit -m "feat(gmail): add 506_get-gmail.sh"
```

---

## Task 5: Create `509_delete-gmail.sh`

**Files:**
- Create: `production/workspace/skills/gmail/scripts/509_delete-gmail.sh`

- [ ] **Step 1: Write the script**

Create `production/workspace/skills/gmail/scripts/509_delete-gmail.sh`:

```bash
#!/bin/bash
# Gmail メッセージをゴミ箱に移動する（_ai-agent ラベル付きのみ許可）
# Usage: 509_delete-gmail.sh <message_id>

set -euo pipefail

MESSAGE_ID="${1:-}"
REQUIRED_LABEL="_ai-agent"

if [ -z "$MESSAGE_ID" ]; then
    echo "Usage: $0 <message_id>" >&2
    exit 1
fi

if ! command -v gws &>/dev/null; then
    echo "gws not found in PATH" >&2
    exit 1
fi

# メッセージのラベルを取得
labels=$(gws gmail users messages get \
    --params "{\"userId\":\"me\",\"id\":\"${MESSAGE_ID}\",\"format\":\"metadata\"}" \
    --format json \
  | jq -r '.labelIds[]? // empty' 2>/dev/null)

# _ai-agent ラベルの存在確認（大文字小文字を区別しない）
has_label=false
while IFS= read -r label; do
    if [[ "${label,,}" == "${REQUIRED_LABEL,,}" ]]; then
        has_label=true
        break
    fi
done <<< "$labels"

if [ "$has_label" = false ]; then
    echo "DELETE BLOCKED: message '${MESSAGE_ID}' does not have the '${REQUIRED_LABEL}' label." >&2
    exit 1
fi

# ゴミ箱に移動
gws gmail users messages trash \
    --params "{\"userId\":\"me\",\"id\":\"${MESSAGE_ID}\"}" \
    --format json
```

- [ ] **Step 2: Make executable**

```bash
chmod +x production/workspace/skills/gmail/scripts/509_delete-gmail.sh
```

- [ ] **Step 3: Verify guard blocks message without label (using a dummy ID)**

```bash
# ダミー ID でラベル取得が空になる場合のガード確認
# 実際には gws が "message not found" を返すが、ラベルが空なのでガードが発動する
bash production/workspace/skills/gmail/scripts/509_delete-gmail.sh "dummy-id-000" 2>&1 | head -2
echo "exit code: $?"
```

Expected: stderr contains `DELETE BLOCKED` or gws API error; exit code non-zero

- [ ] **Step 4: Commit**

```bash
git add production/workspace/skills/gmail/scripts/509_delete-gmail.sh
git commit -m "feat(gmail): add 509_delete-gmail.sh with _ai-agent label guard"
```

---

## Task 6: Create `gmail/SKILL.md`

**Files:**
- Create: `production/workspace/skills/gmail/SKILL.md`

- [ ] **Step 1: Write the SKILL.md**

Create `production/workspace/skills/gmail/SKILL.md`:

```markdown
---
name: gmail
description: Use when the user asks to check unread emails, search Gmail messages, or trash AI-agent-labeled messages.
---

# Gmail Skill

## Overview
Lists Gmail messages filtered by a search query and extracts key fields (id, sender, subject, date, snippet). Can also trash messages, but only those carrying the `_ai-agent` label — a hard safety guard to prevent accidental deletion.

---

## When to Use

### Triggering Scenarios:
- The user asks to check unread email or search for messages.
- The user asks to delete or trash an email that was sent to the AI agent.
- Any scheduled Gmail patrol cron triggers.

### When NOT to use:
- Sending email (not supported by this skill).
- Trashing messages that do not carry the `_ai-agent` label.

---

## Workflow

### Read: list messages

- **Tool**: `run_workspace_script`
- **Parameters**:
  - `script_name`: `skills/gmail/scripts/506_get-gmail.sh`
  - `args`: `["<gmail_query>", "<max_results>"]`
  - *(no `env` required)*

Default query: `is:unread`. Default max: `10`.

Returns a JSON array. Each element: `{id, sender, subject, date, snippet}`.

### Delete: trash a message

- **Tool**: `run_workspace_script`
- **Parameters**:
  - `script_name`: `skills/gmail/scripts/509_delete-gmail.sh`
  - `args`: `["<message_id>"]`
  - *(no `env` required)*

**Guard**: Only messages with the `_ai-agent` label (case-insensitive) can be trashed. Any other message exits with `DELETE BLOCKED` and code 1.

Use the `id` field from `506_get-gmail.sh` output as the `<message_id>`.

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `run_workspace_script` を使うこと。
- **`_ai-agent` ラベルのないメッセージは削除不可。** ガードが強制的にブロックする。
- **message_id は `506_get-gmail.sh` の `id` フィールドから取得すること。**
```

- [ ] **Step 2: Verify YAML frontmatter**

```bash
head -4 production/workspace/skills/gmail/SKILL.md
```

Expected:
```
---
name: gmail
description: Use when the user asks to check unread emails, search Gmail messages, or trash AI-agent-labeled messages.
---
```

- [ ] **Step 3: Commit**

```bash
git add production/workspace/skills/gmail/SKILL.md
git commit -m "feat(gmail): add gmail SKILL.md"
```

---

## Task 7: Delete 3 gws tools from `rustyclaw-tools`

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- [ ] **Step 1: Record baseline test count**

```bash
cargo test -p rustyclaw-tools 2>&1 | grep "^test result"
```

Note the passing count. After Task 7 it will decrease by 13 (all gws calendar + gmail tests deleted).

- [ ] **Step 2: Delete `GwsCalendarTool` (line ~358–431)**

In `crates/rustyclaw-tools/src/lib.rs`, delete from the comment line before the struct through the end of its `impl Tool` block:

```rust
/// Google Calendar のイベント一覧を取得するツール（gws CLI subprocess 経由）
pub struct GwsCalendarTool {
    gws_path: String,
}
// ... (entire impl GwsCalendarTool and impl Tool for GwsCalendarTool)
```

*(Lines 358–431 inclusive)*

- [ ] **Step 3: Delete `GwsCalendarWriteTool` (line ~432–535)**

Delete from:
```rust
pub struct GwsCalendarWriteTool {
```
through the closing `}` of `impl Tool for GwsCalendarWriteTool`.

*(Lines 432–535 inclusive)*

- [ ] **Step 4: Delete `GwsGmailTool` (line ~537–600)**

Delete from:
```rust
/// Gmail のメッセージ一覧を取得するツール（gws CLI subprocess 経由）
pub struct GwsGmailTool {
```
through the closing `}` of `impl Tool for GwsGmailTool`.

*(Lines 537–600 inclusive)*

- [ ] **Step 5: Delete `GwsGmailDeleteTool` (line ~602–end of impl)**

Delete from:
```rust
/// Gmail メール削除ツール（gws CLI subprocess 経由）
pub struct GwsGmailDeleteTool {
```
through the closing `}` of `impl Tool for GwsGmailDeleteTool`.

- [ ] **Step 6: Delete all gws-related tests (lines ~1487–1686)**

Delete these test functions from the `mod tests` block:
- `test_gws_calendar_tool_name_and_schema`
- `test_gws_gmail_tool_name_and_schema`
- `test_gws_calendar_write_blocks_unlisted_calendar`
- `test_gws_calendar_write_requires_calendar_id`
- `test_gws_calendar_write_allows_multiple_calendars`
- `test_gws_calendar_write_blocks_personal_calendar`
- `test_gws_calendar_write_blocks_yuki_shared_calendar`
- `test_gws_calendar_write_guard_passes_for_ai_agent_calendar`
- `test_gmail_delete_label_check_blocks_without_label`
- `test_gmail_delete_label_check_passes_with_label_name`
- `test_gmail_delete_label_check_passes_with_label_id`
- `test_gmail_delete_label_check_case_insensitive`
- `test_gmail_delete_requires_message_id`

- [ ] **Step 7: Verify `cargo check` passes**

```bash
cargo check -p rustyclaw-tools 2>&1 | grep "^error"
```

Expected: no output.

- [ ] **Step 8: Run tests and confirm all pass**

```bash
cargo test -p rustyclaw-tools 2>&1 | grep "^test result"
```

Expected: all `ok`, count is baseline minus 13.

- [ ] **Step 9: Commit**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(calendar,gmail): remove 4 gws native tools from rustyclaw-tools"
```

---

## Task 8: Delete gws registration block from gateway

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: Delete the gws registration block (lines 739–817)**

In `crates/rustyclaw-gateway/src/lib.rs`, delete from:
```rust
        // Google Workspace ネイティブツール登録
        if let Some(gws) = config.tools.google_workspace.as_ref().filter(|g| g.enabled) {
```
through the closing `}` of that `if let Some(gws)` block (line ~817).

- [ ] **Step 2: Verify `cargo check` passes**

```bash
cargo check -p rustyclaw-gateway 2>&1 | grep "^error"
```

Expected: no output.

- [ ] **Step 3: Run full test suite**

```bash
cargo test 2>&1 | grep -E "^test result|FAILED"
```

Expected: all `test result: ok`, no `FAILED`.

- [ ] **Step 4: Commit**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(calendar,gmail): remove gws gateway registration block"
```

---

## Task 9: Update `docs/task.md` — mark Phase 36 items 2 and 3 complete

**Files:**
- Modify: `docs/task.md`

- [ ] **Step 1: Mark Phase 36 items 2 and 3 as done**

In `docs/task.md`, change:

```markdown
- `[ ]` **2. Googleカレンダーの予定管理スキル化（Phase B）**
```
to:
```markdown
- `[x]` **2. Googleカレンダーの予定管理スキル化（Phase B）**
```

And change:

```markdown
- `[ ]` **3. Gmailメッセージ取得・ゴミ箱化のスキル化（Phase C）**
```
to:
```markdown
- `[x]` **3. Gmailメッセージ取得・ゴミ箱化のスキル化（Phase C）**
```

- [ ] **Step 2: Commit**

```bash
git add docs/task.md
git commit -m "docs(task): mark Phase 36-B calendar and 36-C gmail skill migration complete"
```
