---
name: obsidian
description: Use when the user asks to search, read, write, or append notes in their Obsidian vault.
---

# Obsidian Skill

## Overview
Provides full CRUD access to the user's local Obsidian vault via the Obsidian Local REST API plugin (port 27123). All operations are handled by a single script with subcommands.

---

## When to Use

### Triggering Scenarios:
- The user asks to search their notes or vault.
- The user asks to read a specific note.
- The user asks to create, update, or append to a note.

### When NOT to use:
- Obsidian is not running on the local network (`192.168.1.2`).
- Managing files outside the Obsidian vault.

---

## Prerequisites

The Obsidian Local REST API plugin must be running on `192.168.1.2:27123`.

---

## Workflow

All operations use `ctx_execute` with `language: bash`. `OBSIDIAN_TOKEN` は Phase 49-2 の vault キャッシュ機構で解決予定。

- **Tool**: `ctx_execute`
- **`language`**: `bash`
- **スクリプト**: `workspace/skills/obsidian/scripts/507_obsidian-ops.sh`

### Search

- **`code`**: `bash workspace/skills/obsidian/scripts/507_obsidian-ops.sh search "<query>" <limit>`
- Returns `[{path, excerpt}]` array. Default limit: 10.

### Read

- **`code`**: `bash workspace/skills/obsidian/scripts/507_obsidian-ops.sh read "<vault-relative-path>"`
- Returns raw Markdown text of the note. Example path: `"Daily/2026-05-31.md"`

### Write (overwrite)

- **`code`**: `bash workspace/skills/obsidian/scripts/507_obsidian-ops.sh write "<vault-relative-path>" "<markdown-content>"`
- Creates or overwrites the note. Returns `Written to <path>` on success.

### Append

- **`code`**: `bash workspace/skills/obsidian/scripts/507_obsidian-ops.sh append "<vault-relative-path>" "<markdown-content>"`
- Reads existing content and appends `<markdown-content>` on a new line. Returns `Appended to <path>` on success.

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `ctx_execute` を使うこと。
- **`OBSIDIAN_TOKEN` は vault キャッシュから自動解決（Phase 49-2）。** 未設定の場合は `exit 1` になる。
- **パスは Vault ルートからの相対パス**（例: `"Projects/MyNote.md"`）。絶対パス不可。
