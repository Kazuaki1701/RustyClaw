# Phase 6: Verification, Code Refining, and Non-blocking Rate-Limit Backoff Implementation Plan

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の実装計画書 - 開発完了済み)  
> **完了日**: 2026-05-28  
> **備考**: この計画書に記載された機能はすべて実装・検証が完了しています。最新の基本仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

**Goal:** Verify existing systems (Logging, Flush, Dashboard, Continuation), clean up 10 compiler warnings, and implement a robust non-blocking exponential backoff retry mechanism for LLM rate limits without choking the global gateway semaphore.

**Architecture:** 
1. **Refining**: Eliminate all compilation warnings in `rustyclaw-gateway` by removing unused imports/variables and introducing serde attributes and `#[allow(non_snake_case)]` to suppress json casing warnings safely.
2. **Robust Backoff**: Move the sleep retry loop out of the stateless `GmnCliProvider` (which chokes the single global `gmn_sem` semaphore during sleep) and bubble up a designated `RateLimit` error. Implement a non-blocking retry with exponential backoff at the `rustyclaw-agent` / `rustyclaw-gateway` pipeline executor level, releasing the semaphore during backoff.
3. **Validation**: Execute automated checks and manual system-wide operational test scripts.

**Tech Stack:** Rust (Tokio, Serenity, Chrono, deadpool-sqlite, Tantivy)

---

## File Structure

- **Modify**: `crates/rustyclaw-providers/src/lib.rs` (Define `RateLimit` error type, return it immediately on 429 without sleeping inside the provider)
- **Modify**: `crates/rustyclaw-gateway/src/cron.rs` (Remove unused imports `Context` and `Result` and unused variable `db_path`)
- **Modify**: `crates/rustyclaw-gateway/src/heartbeat.rs` (Remove unused imports `Context`, `Path`, `Utc`, unused struct field `config`, and add `#[allow(non_snake_case)]` to camelCase json casing state structures)
- **Modify**: `crates/rustyclaw-agent/src/lib.rs` (Intercept `RateLimit` errors, release permits, apply exponential backoff, and retry gracefully)
- **Modify**: `docs/specs/02_agent_pipeline.md` (Update specifications with the new non-blocking backoff retry architecture)

---

## Tasks

### Task 1: Gateway Compiler Warnings Cleanup (Code Quality)

**Files:**
- Modify: `crates/rustyclaw-gateway/src/cron.rs`
- Modify: `crates/rustyclaw-gateway/src/heartbeat.rs`

- [x] **Step 1: Clean up unused imports and unused variables in `cron.rs`**

Modify `crates/rustyclaw-gateway/src/cron.rs` to remove the unused `anyhow::Context` and `Result` import from the top, and prefix/remove the unused `db_path` variable in `CronService::start`.

```rust
// Replace lines 1-20 in cron.rs with:
use anyhow::Result; // Only import Result if used, else remove anyhow entirely if unused.
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing;
// ... (rest of standard imports)

// In start function, prefix unused db_path with underscore or remove:
let _db_path = self.db_path.clone(); 
```

- [x] **Step 2: Clean up unused imports, struct fields, and json warnings in `heartbeat.rs`**

Modify `crates/rustyclaw-gateway/src/heartbeat.rs` to clean up `Context`, `Path`, and `Utc`. Prefix the unused `config` field in `HeartbeatService` with an underscore, and apply `#[allow(non_snake_case)]` to the `LastChecks` and `HeartbeatState` inner structures.

```rust
// In heartbeat.rs, modify the HeartbeatState structures to suppress casing warnings:
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct LastChecks {
    pub activityReview: String,
    pub memoryMaintenance: String,
    pub calendar: String,
    pub email: String,
    pub weather: String,
    pub lastUserContact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct HeartbeatState {
    pub lastChecks: LastChecks,
}
```

- [x] **Step 3: Run the compiler to verify warnings are cleared**

Run: `cargo check`
Expected: Passes with ZERO warnings in the `rustyclaw-gateway` crate.

- [x] **Step 4: Commit warnings cleanup**

Run:
```bash
git add crates/rustyclaw-gateway/src/cron.rs crates/rustyclaw-gateway/src/heartbeat.rs
git commit -m "style(gateway): clean up unused imports, unused fields, and casing compiler warnings"
```

---

### Task 2: Propagate rate limit error from GmnCliProvider (Stateless Decoupling)

**Files:**
- Modify: `crates/rustyclaw-providers/src/lib.rs`

- [x] **Step 1: Introduce custom ProviderError containing RateLimit in `crates/rustyclaw-providers/src/lib.rs`**

We will define an error enum to bubble up the `RateLimit` explicitly, so that the pipeline can capture and handle it without sleeping inside the provider.

```rust
// Add this near the top of crates/rustyclaw-providers/src/lib.rs:
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Rate limit or quota exceeded: {0}")]
    RateLimit(String),
    #[error("API or CLI execution failed: {0}")]
    ExecutionFailed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

Modify the return types of `LlmProvider::complete` and `LlmProvider::complete_stream` to return `Result<LlmResponse, ProviderError>` instead of `anyhow::Result<LlmResponse>`.

- [x] **Step 2: Modify `GmnCliProvider::complete` to return `ProviderError::RateLimit` immediately on 429 detection**

Modify `complete` in `crates/rustyclaw-providers/src/lib.rs` to stop sleeping inside the provider loop and instead return `ProviderError::RateLimit` immediately upon rate limit detection.

```rust
// Inside GmnCliProvider::complete:
if combined_err.contains("quota") || combined_err.contains("RESOURCE_EXHAUSTED") || combined_err.contains("429") || combined_err.contains("rate limited") {
    let wait_secs = parse_reset_seconds(&combined_err).unwrap_or(23);
    return Err(ProviderError::RateLimit(format!(
        "gmn rate-limit exceeded. reset_secs: {}", wait_secs
    )));
}
```

Do the same for `complete_stream` to abort immediately and return `ProviderError::RateLimit` if rate limits are encountered during stream parsing.

- [x] **Step 3: Run cargo check to verify types**

Run: `cargo check`
Expected: Compilation errors in callers (e.g., `rustyclaw-agent` where `LlmProvider` is called) because of the new `ProviderError` return type. This proves TDD propagation.

- [x] **Step 4: Commit stateless error propagation**

Run:
```bash
git add crates/rustyclaw-providers/src/lib.rs
git commit -m "refactor(providers): introduce ProviderError and propagate RateLimit immediately"
```

---

### Task 3: Implement Non-Blocking Exponential Backoff in Pipeline Executor

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [x] **Step 1: Write a unit test for rate limit retry exponential backoff**

Add a test in `crates/rustyclaw-agent/src/lib.rs` that mocks a failing `LlmProvider` returning `ProviderError::RateLimit`, and verify that the executor retries with increasing backoff delays.

```rust
// Add to crates/rustyclaw-agent/src/lib.rs tests module:
#[tokio::test]
async fn test_pipeline_exponential_backoff_retry() {
    // 1. Create a mock provider returning ProviderError::RateLimit twice, then success.
    // 2. Run pipeline.execute() and verify it retries twice and eventually succeeds.
    // 3. Verify that elapsed time demonstrates non-blocking retry pauses.
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p rustyclaw-agent`
Expected: FAIL due to missing handling of `ProviderError` and backoff logic.

- [x] **Step 3: Implement exponential backoff in `Pipeline::execute` and `Pipeline::execute_stream`**

Update `crates/rustyclaw-agent/src/lib.rs` to intercept `ProviderError::RateLimit`. When caught, the pipeline must release any semaphore permits (`gmn_sem`), wait for `base_delay * 2^attempt` (e.g., 5s, 10s, 20s) utilizing `tokio::time::sleep`, re-acquire the permit, and retry the LLM call.

```rust
// Implementation template in Pipeline::execute:
let mut attempt = 0;
let max_attempts = 3;
let mut base_delay = std::time::Duration::from_secs(5);

loop {
    // Acquire gmn_sem permit...
    let permit = gmn_sem.acquire().await?;
    
    match provider.complete(&messages, &tools, &opts).await {
        Ok(response) => return Ok(response),
        Err(ProviderError::RateLimit(err_msg)) => {
            drop(permit); // Release permit immediately so other lanes aren't blocked!
            if attempt >= max_attempts {
                return Err(anyhow::anyhow!("Max LLM rate-limit retries reached: {}", err_msg));
            }
            let backoff = base_delay * 2u32.pow(attempt);
            tracing::warn!("LLM Rate limited. Released semaphore slot. Sleeping for {:?} before retry...", backoff);
            tokio::time::sleep(backoff).await;
            attempt += 1;
        }
        Err(other_err) => return Err(other_err.into()),
    }
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p rustyclaw-agent`
Expected: PASS

- [x] **Step 5: Commit backoff executor**

Run:
```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): implement non-blocking exponential backoff for rate limits"
```

---

### Task 4: Complete System-wide Integration Verification (Manual & Logging Check)

**Files:**
- Modify: `docs/task.md` (Update status of verified items)

- [x] **Step 1: Verify `RUST_LOG=debug` spawning output**

Run gateway: `RUST_LOG=debug cargo run --bin rustyclaw-cli gateway`
Send a chat from the dashboard.
Expected: Log output shows `gmn spawn: stdin stream initialized` and `gmn exit: response` successfully without OS error 7.

- [x] **Step 2: Verify Memory Flush after 6 turns**

Send 6 messages sequentially to the HTTP chat endpoint (`/chat`).
Expected: 
- Dashboard logs output: `memory flush: starting`
- `workspace/MEMORY.md` updates and compiles to under 5KB.
- Check time-gate by chatting again immediately; logs must report: `memory flush: skipping (time gate...)`.

- [x] **Step 3: Verify Dashboard Hot Reload and Log Views**

Open `http://localhost:8080/` in browser.
Expected:
- Correct layouts for Chat, MEMORY.md, heartbeat digest, and app logs.
- Polling at 5s/2s is active and does not throw errors.
- Trigger reload: `curl http://localhost:8080/reload` returns `200 OK` and logs reload success.

- [x] **Step 4: Verify Session Continuation (Day Overwrite)**

Simulate a day boundary in summaries:
Create a file at `workspace/memory/summaries/2026-05-27-daily.md` with daily logs.
Start a new chat session.
Expected: Logs show yesterday's daily summary being successfully compiled and injected into the startup system context.

- [x] **Step 5: Apply DoD and Update Specification**

Update [docs/specs/02_agent_pipeline.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/02_agent_pipeline.md) with the new `ProviderError` enum, rate-limit propagation, and non-blocking exponential backoff semaphore release architecture.

Mark verified tasks as complete (`[x]`) in [docs/task.md](file:///home/kazuaki/Projects/RustyClaw/docs/task.md).

- [x] **Step 6: Commit final integration verification**

Run:
```bash
git add docs/specs/02_agent_pipeline.md docs/task.md
git commit -m "docs: finalize Phase 6 verification and sync specification documentation"
```
