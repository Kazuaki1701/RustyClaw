# Phase 7: Bugfix and Stabilization Implementation Plan

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-05-28  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

**Goal:** Resolve 5 critical bugs discovered during overnight gateway operations: eliminate infinite context accumulation for Heartbeat logs, fix empty heartbeat LLM responses, resolve the 0-byte heartbeat-digest issue, filter out leaking raw MCP JSON in chats, and prevent scheduler semaphore collisions between Heartbeats and Daily Summaries.

**Architecture:**
1. **Stateless Heartbeat History (Task 1)**: Skip conversation history load inside `Pipeline::execute` for any `cron:` prefixed session. Heartbeats must remain completely stateless to prevent infinite memory build-up.
2. **Fallback LLM Output Parsing (Task 2)**: Update `GmnCliProvider` to capture raw tool-call outputs in `final_content` if no text chunks (`type == "content"`) are yielded, preventing empty assistant entries in logs.
3. **Persisted Heartbeat Digest (Task 3)**: Modify `HeartbeatService::generate_digest` to always capture the last 24 hours of active dialogue (instead of relying on strict incremental modification gates) so `heartbeat-digest.md` is never 0-bytes.
4. **JSON Leak Filtration (Task 4)**: Clean up legacy MCP instructions in `workspace/AGENTS.md` and add a programmatic JSON regex filter inside `Pipeline::execute` to strip raw tool-call JSON leaks before publishing to chat.
5. **Scheduler Offset (Task 5)**: Introduce a few minutes offset inside `CronService` for midnight Daily Summary runs to prevent locks/collisions on `gmn_sem(1)` with Heartbeat ticks.

**Tech Stack:** Rust (Tokio, reqwest, deadpool-sqlite, Tantivy)

---

## File Structure

- **Modify**: `crates/rustyclaw-agent/src/lib.rs` (Skip history for `cron:` sessions, apply output JSON filter)
- **Modify**: `crates/rustyclaw-providers/src/lib.rs` (Fallback to raw output in `GmnCliProvider` if no text is parsed)
- **Modify**: `crates/rustyclaw-gateway/src/heartbeat.rs` (Revamp `generate_digest` time constraints to always include last 24h of dialogue)
- **Modify**: `crates/rustyclaw-gateway/src/cron.rs` (Adjust scheduler tick offset for Daily Summary)
- **Modify**: `workspace/AGENTS.md` (Remove legacy MCP instructions)

---

## Tasks

### Task 1: Stateless Cron Session History (Prevent infinite context accumulation)

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:412-445`
- Modify: `crates/rustyclaw-agent/src/lib.rs:479-512`

- [x] **Step 1: Write a unit test verifying no history is loaded for `cron:` sessions**

Add a test in `crates/rustyclaw-agent/src/lib.rs` to verify that `cron:heartbeat` does not append historical messages to the built prompt.

```rust
// Add to crates/rustyclaw-agent/src/lib.rs tests:
#[tokio::test]
async fn test_cron_session_ignores_history() {
    // 1. Create a logger and save dummy historical messages in "cron:heartbeat" session.
    // 2. Call pipeline.execute("cron:heartbeat") and intercept built context.
    // 3. Verify history messages are NOT loaded or included in the final system prompt.
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p rustyclaw-agent test_cron_session_ignores_history`
Expected: FAIL (loaded history is still included).

- [x] **Step 3: Implement cron session history check in `Pipeline::execute` & `Pipeline::execute_stream`**

Modify `crates/rustyclaw-agent/src/lib.rs`. If `session_id.starts_with("cron:")`, initialize the history as empty instead of loading from the `SessionLogger`.

```rust
// In execute() around lines 423-430:
let history_messages = if session_id.starts_with("cron:") {
    Vec::new()
} else {
    logger.load_history(session_id)
        .context("Failed to load session history messages")?
};

let mut history = ConversationHistory::new(history_messages);
```

Apply the exact same modification to `execute_stream()` around lines 490-497.

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p rustyclaw-agent test_cron_session_ignores_history`
Expected: PASS

- [x] **Step 5: Commit cron statelessness**

Run:
```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "fix(agent): skip loading conversation history for cron background sessions"
```

---

### Task 2: Capture Fallback Raw Output in `GmnCliProvider` (Resolve empty responses)

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`

- [x] **Step 1: Write a unit test simulating a raw JSON tool call from `gmn` CLI**

Add a test in `crates/rustyclaw-providers/src/lib.rs` where the `gmn` CLI outputs raw JSON or tool-call formats, and verify that the provider captures it as fallback instead of returning an empty response.

```rust
// Add to crates/rustyclaw-providers/src/lib.rs tests:
#[tokio::test]
async fn test_gmn_cli_fallback_raw_output() {
    // Verify that if no type=="content" line is parsed, but raw json strings exist in output,
    // they are preserved and returned rather than yielding empty string.
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p rustyclaw-providers test_gmn_cli_fallback_raw_output`
Expected: FAIL (returns empty `""`).

- [x] **Step 3: Update `GmnCliProvider::complete` output parser**

Modify `complete` in `crates/rustyclaw-providers/src/lib.rs` to fall back to the raw output lines if `final_content` is empty after parsing.

```rust
// Inside GmnCliProvider::complete around line 348:
let mut final_content = String::new();
let mut raw_lines = Vec::new();
for line in content.lines() {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        continue;
    }
    raw_lines.push(line);
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if val["type"] == "content" {
            if let Some(text) = val["text"].as_str() {
                final_content.push_str(text);
            }
        }
    } else {
        final_content.push_str(line);
        final_content.push('\n');
    }
}

// Fallback if no content block was extracted (e.g. tool call raw JSON):
if final_content.trim().is_empty() && !raw_lines.is_empty() {
    final_content = raw_lines.join("\n");
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p rustyclaw-providers test_gmn_cli_fallback_raw_output`
Expected: PASS

- [x] **Step 5: Commit fallback parser**

Run:
```bash
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "fix(providers): fallback to raw stdout lines in GmnCliProvider if no content blocks are parsed"
```

---

### Task 3: Revamp `generate_digest` time constraints (Fix 0-byte digest)

**Files:**
- Modify: `crates/rustyclaw-gateway/src/heartbeat.rs`

- [x] **Step 1: Write a unit test for `generate_digest`**

Write a test in `crates/rustyclaw-gateway/src/heartbeat.rs` ensuring that `generate_digest` always populates `heartbeat-digest.md` with active dialogue from the last 24h, even if no new sessions have modified timestamps since the last check.

```rust
// Add test in crates/rustyclaw-gateway/src/heartbeat.rs tests:
#[tokio::test]
async fn test_generate_digest_persists_recent_dialogue() {
    // 1. Create a dummy session log telegram-U123-20260528.jsonl with a user-assistant pair.
    // 2. Call generate_digest() and verify the digest contains the conversation.
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p rustyclaw-gateway test_generate_digest_persists_recent_dialogue`
Expected: FAIL (digest remains empty if run immediately after another simulated run).

- [x] **Step 3: Modify `HeartbeatService::generate_digest` to query 24h window always**

Modify `crates/rustyclaw-gateway/src/heartbeat.rs` around lines 57-72. Instead of strict incremental gating `modified > last_patrol_dt`, **always query all files modified within the last 24 hours**. This ensures that the last 24 hours of conversation are always represented in `heartbeat-digest.md`, maintaining a steady context for the dashboard and Step 5 Vocal greetings.

```rust
// Replace lines 57-72 in heartbeat.rs:
let metadata = fs::metadata(&path)?;
let modified: DateTime<Local> = metadata.modified()?.into();

// Always capture sessions active within the last 24 hours:
let should_scan = now_dt.signed_duration_since(modified).num_hours() < 24;

if should_scan {
    entries.push((modified, path, filename));
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p rustyclaw-gateway test_generate_digest_persists_recent_dialogue`
Expected: PASS

- [x] **Step 5: Commit digest improvements**

Run:
```bash
git add crates/rustyclaw-gateway/src/heartbeat.rs
git commit -m "fix(gateway): always generate heartbeat-digest from last 24 hours of conversation"
```

---

### Task 4: Clean up legacy MCP instructions & Add JSON Filter

**Files:**
- Modify: `workspace/AGENTS.md`
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [x] **Step 1: Clean up legacy MCP tool descriptions in `AGENTS.md`**

Modify `workspace/AGENTS.md`. Remove any instructions guiding the LLM to write markdown tool blocks or JSON blocks targeted for Inngest or `geminiclaw_status`.

- [x] **Step 2: Implement post-execution JSON leak filter**

Add a function in `crates/rustyclaw-agent/src/lib.rs` that catches any residual tool-calling JSON leaks (such as ` ```json {"action": ...} ``` `) and filters them out of the assistant's final response before it is sent to the chat channels.

```rust
// In crates/rustyclaw-agent/src/lib.rs, implement a clean filter function:
fn filter_json_leaks(content: &str) -> String {
    let mut cleaned = content.to_string();
    // Regex or simple block removal to discard Markdown JSON blocks containing tool-calling properties:
    // e.g. if the response has ```json \n { "action": ... } \n ```, strip it.
    if cleaned.contains("\"action\"") && cleaned.contains("{") {
        // Strip the json block
        if let Some(start_idx) = cleaned.find("```json") {
            if let Some(end_idx) = cleaned[start_idx..].find("```") {
                let actual_end = start_idx + end_idx + 3;
                cleaned.replace_range(start_idx..actual_end, "");
            }
        }
    }
    cleaned.trim().to_string()
}
```

Apply `filter_json_leaks` to both `execute()` and `execute_stream()` outputs prior to publishing the event.

- [x] **Step 3: Run cargo test to verify**

Run: `cargo test --all`
Expected: PASS

- [x] **Step 4: Commit leak filtration**

Run:
```bash
git add workspace/AGENTS.md crates/rustyclaw-agent/src/lib.rs
git commit -m "fix(agent): clean up AGENTS.md legacy MCP references and filter out raw JSON leaks"
```

---

### Task 5: Offset Daily Summary Midnight Cron Trigger

**Files:**
- Modify: `crates/rustyclaw-gateway/src/cron.rs`

- [x] **Step 1: Adjust `Daily Summary` check interval in `cron.rs`**

Modify `crates/rustyclaw-gateway/src/cron.rs` to schedule the `Daily Summary` run at `00:05` (5 minutes past midnight) instead of exactly `00:00`, so it does not collide with the exact midnight Heartbeat execution slot.

```rust
// Modify crates/rustyclaw-gateway/src/cron.rs Daily Summary scheduling:
// Make sure the hour and minute comparisons check for 00:05.
```

- [x] **Step 2: Run all tests to verify**

Run: `cargo test --all`
Expected: PASS (All 32+ tests compile and pass).

- [x] **Step 3: Commit cron offset**

Run:
```bash
git add crates/rustyclaw-gateway/src/cron.rs
git commit -m "fix(gateway): offset Daily Summary cron by 5 minutes to prevent semaphore collisions"
```
