# Design: Obsidian Skill Migration (Phase 36-D)

**Date**: 2026-05-31  
**Phase**: 36-D

---

## Overview

Migrate `ObsidianSearchTool`, `ObsidianReadTool`, and `ObsidianWriteTool` (Rust-native, Local REST API) into a single shell-script skill at `production/workspace/skills/obsidian/`. One unified script handles all 4 operations (search, read, write, append) via subcommands. Then delete the Rust implementations.

---

## Change Scope

| Action | Target |
|---|---|
| Create | `production/workspace/skills/obsidian/SKILL.md` |
| Create | `production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh` |
| Delete | `crates/rustyclaw-tools/src/lib.rs` — `percent_encode()` (lines 138–143), `ObsidianSearchTool` (lines 145–219), `ObsidianReadTool` (lines 221–277), `ObsidianWriteTool` (lines 279–356), related tests (lines 1444–1487, 1830–1857) |
| Delete | `crates/rustyclaw-gateway/src/lib.rs` — Obsidian registration block (lines 704–718) |

---

## Script Specification

**File**: `production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh`

### Endpoint & Auth

- **Host**: `http://192.168.1.2:27123` (hardcoded)
- **Token**: injected via `OBSIDIAN_TOKEN` environment variable (`$vault:obsidian-api-key` in SKILL.md)

### Usage

```
507_obsidian-ops.sh search <query> [limit]
507_obsidian-ops.sh read   <vault-relative-path>
507_obsidian-ops.sh write  <vault-relative-path> <content>
507_obsidian-ops.sh append <vault-relative-path> <content>
```

### URL Encoding

Uses Python 3 for percent-encoding (RFC 3986 unreserved chars preserved):

```bash
url_encode() { python3 -c "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "$1"; }
```

### Operations

| Subcommand | HTTP | URL | jq / processing |
|---|---|---|---|
| `search` | POST | `/search/simple/?query=<encoded>&contextLength=100` | Trim to `limit` (default 10), output `[{path, excerpt}]` |
| `read` | GET | `/vault/<encoded-path>` | Raw text output (no jq) |
| `write` | PUT | `/vault/<encoded-path>` | Body = `$content`, Content-Type: text/markdown; 204 = success |
| `append` | GET→PUT | `/vault/<encoded-path>` | GET existing text, append `\n$content`, PUT result |

### Error Handling

- Missing `OBSIDIAN_TOKEN`: print error and `exit 1`
- Unknown subcommand: print usage and `exit 1`
- Non-2xx HTTP response: print `Obsidian API error: HTTP <status>` and `exit 1`

### jq filter for `search`

```bash
jq --argjson limit 10 '.[:$limit] | map({path: .filename, excerpt: (.matches[0].context // "")})'
```

---

## SKILL.md Specification

**Frontmatter:**
```yaml
name: obsidian
description: Use when the user asks to search, read, write, or append notes in their Obsidian vault.
```

**Workflow** — all operations use `run_workspace_script`:
- `script_name`: `skills/obsidian/scripts/507_obsidian-ops.sh`
- `env`: `{ "OBSIDIAN_TOKEN": "$vault:obsidian-api-key" }`
- `args`: subcommand + parameters

---

## Rust Cleanup

**`crates/rustyclaw-tools/src/lib.rs`** — delete:
- `fn percent_encode()` — lines 138–143 (only used by Obsidian tools)
- `ObsidianSearchTool` struct + `impl` — lines 145–219
- `ObsidianReadTool` struct + `impl` — lines 221–277
- `ObsidianWriteTool` struct + `impl` — lines 279–356
- Tests: `test_obsidian_search_tool_name_and_schema`, `test_obsidian_read_tool_name_and_schema`, `test_obsidian_read_tool_missing_path`, `test_obsidian_search_tool_missing_query` (~lines 1444–1487)
- Tests: `test_obsidian_write_tool_schema`, `test_obsidian_write_missing_path`, `test_obsidian_write_missing_content` (~lines 1830–1857)

**`crates/rustyclaw-gateway/src/lib.rs`** — delete:
- Obsidian registration block: `// Obsidian ネイティブツール登録` through closing `}` (lines 704–718)

Run `cargo test` — all remaining tests must pass.
