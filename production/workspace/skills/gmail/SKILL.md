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
