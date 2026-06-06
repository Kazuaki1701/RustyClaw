# Obsidian Skill Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate ObsidianSearchTool, ObsidianReadTool, and ObsidianWriteTool (Rust-native) into a single shell-script skill at `production/workspace/skills/obsidian/`, then delete the Rust implementations.

**Architecture:** One unified script `507_obsidian-ops.sh` handles search/read/write/append via subcommands using curl + jq against the Obsidian Local REST API. Token is injected via `OBSIDIAN_TOKEN` env var from the vault. After scripts are verified, delete the 3 Rust structs, the shared `percent_encode()` function, and the gateway registration block.

**Tech Stack:** bash, curl, jq, python3 (URL encoding), Obsidian Local REST API (port 27123), Rust (deletion only), cargo test

---

## File Map

| Action | Path |
|---|---|
| Create | `production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh` |
| Create | `production/workspace/skills/obsidian/SKILL.md` |
| Modify (delete) | `crates/rustyclaw-tools/src/lib.rs` — lines 138–356 (percent_encode + 3 structs) + tests 1444–1484 + 1830–1852 |
| Modify (delete) | `crates/rustyclaw-gateway/src/lib.rs` — lines 704–718 (Obsidian registration block) |

---

## Task 1: Create `507_obsidian-ops.sh`

**Files:**
- Create: `production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh`

- [ ] **Step 1: Create directory**

```bash
mkdir -p production/workspace/skills/obsidian/scripts
```

- [ ] **Step 2: Write the script**

Create `production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh`:

```bash
#!/bin/bash
# Obsidian Local REST API クライアント
# Usage:
#   507_obsidian-ops.sh search <query> [limit]
#   507_obsidian-ops.sh read   <vault-relative-path>
#   507_obsidian-ops.sh write  <vault-relative-path> <content>
#   507_obsidian-ops.sh append <vault-relative-path> <content>

set -euo pipefail

HOST="http://192.168.1.2:27123"
TOKEN="${OBSIDIAN_TOKEN:-}"
CMD="${1:-}"

if [ -z "$TOKEN" ]; then
    echo "Error: OBSIDIAN_TOKEN is not set." >&2
    exit 1
fi

url_encode() {
    python3 -c "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "$1"
}

auth_header() {
    echo "Authorization: Bearer ${TOKEN}"
}

case "$CMD" in
    search)
        QUERY="${2:-}"
        LIMIT="${3:-10}"
        if [ -z "$QUERY" ]; then
            echo "Usage: $0 search <query> [limit]" >&2
            exit 1
        fi
        encoded=$(url_encode "$QUERY")
        curl -sf -X POST \
            -H "$(auth_header)" \
            "${HOST}/search/simple/?query=${encoded}&contextLength=100" \
          | jq --argjson limit "$LIMIT" \
              '.[:$limit] | map({path: .filename, excerpt: (.matches[0].context // "")})'
        ;;

    read)
        PATH_ARG="${2:-}"
        if [ -z "$PATH_ARG" ]; then
            echo "Usage: $0 read <vault-relative-path>" >&2
            exit 1
        fi
        encoded=$(url_encode "$PATH_ARG")
        curl -sf \
            -H "$(auth_header)" \
            "${HOST}/vault/${encoded}"
        ;;

    write)
        PATH_ARG="${2:-}"
        CONTENT="${3:-}"
        if [ -z "$PATH_ARG" ] || [ -z "$CONTENT" ]; then
            echo "Usage: $0 write <vault-relative-path> <content>" >&2
            exit 1
        fi
        encoded=$(url_encode "$PATH_ARG")
        status=$(curl -sf -o /dev/null -w "%{http_code}" -X PUT \
            -H "$(auth_header)" \
            -H "Content-Type: text/markdown" \
            --data-raw "$CONTENT" \
            "${HOST}/vault/${encoded}")
        if [ "$status" = "200" ] || [ "$status" = "204" ]; then
            echo "Written to ${PATH_ARG}"
        else
            echo "Obsidian API error: HTTP ${status}" >&2
            exit 1
        fi
        ;;

    append)
        PATH_ARG="${2:-}"
        CONTENT="${3:-}"
        if [ -z "$PATH_ARG" ] || [ -z "$CONTENT" ]; then
            echo "Usage: $0 append <vault-relative-path> <content>" >&2
            exit 1
        fi
        encoded=$(url_encode "$PATH_ARG")
        existing=$(curl -sf \
            -H "$(auth_header)" \
            "${HOST}/vault/${encoded}" 2>/dev/null || echo "")
        combined="${existing%$'\n'}
${CONTENT}"
        status=$(curl -sf -o /dev/null -w "%{http_code}" -X PUT \
            -H "$(auth_header)" \
            -H "Content-Type: text/markdown" \
            --data-raw "$combined" \
            "${HOST}/vault/${encoded}")
        if [ "$status" = "200" ] || [ "$status" = "204" ]; then
            echo "Appended to ${PATH_ARG}"
        else
            echo "Obsidian API error: HTTP ${status}" >&2
            exit 1
        fi
        ;;

    *)
        echo "Usage: $0 {search|read|write|append} <args>" >&2
        echo "  search <query> [limit]" >&2
        echo "  read   <vault-relative-path>" >&2
        echo "  write  <vault-relative-path> <content>" >&2
        echo "  append <vault-relative-path> <content>" >&2
        exit 1
        ;;
esac
```

- [ ] **Step 3: Make executable**

```bash
chmod +x production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh
```

- [ ] **Step 4: Verify guard — missing token exits with error**

```bash
OBSIDIAN_TOKEN="" bash production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh search "test" 2>&1
echo "exit: $?"
```

Expected: stderr contains `OBSIDIAN_TOKEN is not set`, exit code `1`

- [ ] **Step 5: Verify guard — unknown subcommand exits with error**

```bash
OBSIDIAN_TOKEN="dummy" \
  bash production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh unknown 2>&1
echo "exit: $?"
```

Expected: stderr contains `Usage:`, exit code `1`

- [ ] **Step 6: Verify search returns JSON array (live API)**

> **Note**: vault からトークンを取得するには RustyClaw バイナリ経由の `run_workspace_script` が必要。ここではローカルの secrets.sh にトークンが設定されている場合のみ実施。スキップして Task 2 へ進んでも可。

```bash
# OBSIDIAN_TOKEN が環境変数に設定されている場合のみ実行
bash production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh search "日記" 3 \
  | jq 'type'
```

Expected: `"array"` (empty array `[]` is also acceptable if no results)

- [ ] **Step 7: Commit**

```bash
git add production/workspace/skills/obsidian/scripts/507_obsidian-ops.sh
git commit -m "feat(obsidian): add 507_obsidian-ops.sh unified script"
```

---

## Task 2: Create `obsidian/SKILL.md`

**Files:**
- Create: `production/workspace/skills/obsidian/SKILL.md`

- [ ] **Step 1: Write the SKILL.md**

Create `production/workspace/skills/obsidian/SKILL.md`:

```markdown
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

All operations use `run_workspace_script`:

- **Tool**: `run_workspace_script`
- **`script_name`**: `skills/obsidian/scripts/507_obsidian-ops.sh`
- **`env`**: `{ "OBSIDIAN_TOKEN": "$vault:obsidian-api-key" }`

### Search

- **`args`**: `["search", "<query>", "<limit>"]`
- Returns `[{path, excerpt}]` array. Default limit: 10.

### Read

- **`args`**: `["read", "<vault-relative-path>"]`
- Returns raw Markdown text of the note. Example path: `"Daily/2026-05-31.md"`

### Write (overwrite)

- **`args`**: `["write", "<vault-relative-path>", "<markdown-content>"]`
- Creates or overwrites the note. Returns `Written to <path>` on success.

### Append

- **`args`**: `["append", "<vault-relative-path>", "<markdown-content>"]`
- Reads existing content and appends `<markdown-content>` on a new line. Returns `Appended to <path>` on success.

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `run_workspace_script` を使うこと。
- **`OBSIDIAN_TOKEN` を `env` で必ず渡すこと。** 省略すると `exit 1` になる。
- **パスは Vault ルートからの相対パス**（例: `"Projects/MyNote.md"`）。絶対パス不可。
```

- [ ] **Step 2: Verify YAML frontmatter**

```bash
head -4 production/workspace/skills/obsidian/SKILL.md
```

Expected:
```
---
name: obsidian
description: Use when the user asks to search, read, write, or append notes in their Obsidian vault.
---
```

- [ ] **Step 3: Commit**

```bash
git add production/workspace/skills/obsidian/SKILL.md
git commit -m "feat(obsidian): add obsidian SKILL.md"
```

---

## Task 3: Delete Obsidian tools from `rustyclaw-tools`

**Files:**
- Modify: `crates/rustyclaw-tools/src/lib.rs`

- [ ] **Step 1: Record baseline test count**

```bash
cargo test -p rustyclaw-tools 2>&1 | grep "^test result"
```

Note the passing count. After this task it will decrease by 7 (4 search/read tests + 3 write tests).

- [ ] **Step 2: Delete `percent_encode()` function (lines 138–143)**

In `crates/rustyclaw-tools/src/lib.rs`, delete:

```rust
fn percent_encode(s: &str) -> String {
    s.chars().flat_map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => vec![c],
        _ => format!("%{:02X}", c as u32).chars().collect(),
    }).collect()
}
```

- [ ] **Step 3: Delete `ObsidianSearchTool` struct + impl (lines 145–219)**

Delete from:
```rust
/// Obsidian Vault 内をテキスト検索するネイティブツール (Local REST API)
pub struct ObsidianSearchTool {
```
through the closing `}` of `impl Tool for ObsidianSearchTool`.

- [ ] **Step 4: Delete `ObsidianReadTool` struct + impl (lines 221–277)**

Delete from:
```rust
/// Obsidian の特定ノートを読み込むネイティブツール (Local REST API)
pub struct ObsidianReadTool {
```
through the closing `}` of `impl Tool for ObsidianReadTool`.

- [ ] **Step 5: Delete `ObsidianWriteTool` struct + impl (lines 279–356)**

Delete from:
```rust
// ─── ObsidianWriteTool ───────────────────────────────────────────────────────

pub struct ObsidianWriteTool {
```
through the closing `}` of `impl Tool for ObsidianWriteTool`.

- [ ] **Step 6: Delete Obsidian tests (lines ~1444–1484 and ~1830–1852)**

Delete these 7 test functions from the `mod tests` block:
- `test_obsidian_search_tool_name_and_schema` (~line 1444)
- `test_obsidian_read_tool_name_and_schema` (~line 1458)
- `test_obsidian_read_tool_missing_path` (~line 1471)
- `test_obsidian_search_tool_missing_query` (~line 1479)
- `test_obsidian_write_tool_schema` (~line 1830)
- `test_obsidian_write_missing_path` (~line 1839)
- `test_obsidian_write_missing_content` (~line 1847)

- [ ] **Step 7: Verify `cargo check` passes**

```bash
cargo check -p rustyclaw-tools 2>&1 | grep "^error"
```

Expected: no output.

- [ ] **Step 8: Run tests and confirm all pass**

```bash
cargo test -p rustyclaw-tools 2>&1 | grep "^test result"
```

Expected: all `ok`, count is baseline minus 7.

- [ ] **Step 9: Commit**

```bash
git add crates/rustyclaw-tools/src/lib.rs
git commit -m "feat(obsidian): remove ObsidianSearchTool, ObsidianReadTool, ObsidianWriteTool from rustyclaw-tools"
```

---

## Task 4: Delete Obsidian registration block from gateway

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: Delete the Obsidian registration block (lines 704–718)**

In `crates/rustyclaw-gateway/src/lib.rs`, delete:

```rust
        // Obsidian ネイティブツール登録
        if let Some(o) = config.tools.obsidian.as_ref().filter(|o| o.enabled) {
            if !o.host.is_empty() && !o.api_key.is_empty() {
                tool_registry.register(Arc::new(rustyclaw_tools::ObsidianSearchTool::new(
                    o.host.clone(), o.api_key.clone(),
                )));
                tool_registry.register(Arc::new(rustyclaw_tools::ObsidianReadTool::new(
                    o.host.clone(), o.api_key.clone(),
                )));
                tool_registry.register(Arc::new(rustyclaw_tools::ObsidianWriteTool::new(
                    o.host.clone(), o.api_key.clone(),
                )));
                tracing::info!("Registered native Obsidian tools.");
            }
        }
```

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
git commit -m "feat(obsidian): remove Obsidian gateway registration block"
```

---

## Task 5: Update `docs/task.md` — mark Phase 36 item 4 complete

**Files:**
- Modify: `docs/task.md`

- [ ] **Step 1: Mark Phase 36 item 4 as done**

In `docs/task.md`, change:

```markdown
- `[ ]` **4. Obsidian 操作の統一スキル化（Phase D）**
```

to:

```markdown
- `[x]` **4. Obsidian 操作の統一スキル化（Phase D）**
```

- [ ] **Step 2: Commit**

```bash
git add docs/task.md
git commit -m "docs(task): mark Phase 36-D Obsidian skill migration complete"
```
