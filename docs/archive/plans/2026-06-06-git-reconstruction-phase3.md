# Git History Reconstruction Phase 3 Implementation Plan

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-06-06  
> **備考**: 本再構築タスクは実行完了し、コミット履歴の整理および master ブランチへの統合が完了しています。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reconstruct the linear history prior to `641fe3e` (base `cbcc8d6` up to `6682af2`) into logical topic branches and merge them with `--no-ff` to create a beautiful, structured commit graph, then replay all subsequent history cleanly.

**Architecture:** Reset `master` to base `cbcc8d6`, create and merge topic branches for Cloudflare Neurons tracking, Calendar ops, weather API migration, and patrol schedule. Replay the async-summary-proto branch, and then replay all Phase 1 & 2 topics on top.

**Tech Stack:** Git, Bash scripting for automation.

---

## 1. File Structure and Target Commits
We will modify no codebase files, only the Git repository structure. 
Target commits to be cherry-picked and branch mappings:

### Topic A: `feat/cf-neurons-tracking` (Cloudflare Neurons calculation)
- `f996bf5` : debug(providers): log CF neurons header presence to diagnose 0% display
- `8cec553` : debug(providers): elevate cf-ai-neurons missing log to INFO level
- `624cbfe` : fix(providers): track CF neurons via total_tokens fallback; thread-safe write
- `b0d423c` : fix(config): add workers-ai/ prefix to CF Gateway model names
- `955d123` : perf(agent): reduce context size to stay within CF neurons budget
- `64ef980` : fix(providers): calculate CF neurons from model-specific rates

### Topic B: `feat/calendar-ops-unification` (Calendar consolidation & validation)
- `2b3b820` : fix(skills): fix PATH and timezone issues in calendar/gmail scripts
- `f980352` : fix(calendar): convert exclusive end date to inclusive for all-day events
- `3242bc1` : fix(calendar): add weekday field in Japanese to prevent LLM day-of-week miscalculation
- `c3ab32b` : fix(calendar): add start_wday/end_wday fields and fix exclusive end in write script
- `8a7071a` : docs(plan): add calendar-ops.sh unification implementation plan
- `b7fd930` : feat(calendar): consolidate calendar scripts into calendar-ops.sh & update SKILL.md
- `d54b363` : cleanup(calendar): remove obsolete calendar scripts
- `358ecc7` : docs(calendar): add investigation report for chat truncation bug
- `f7d5cc4` : docs(calendar): add examples for common user requests in SKILL.md
- `2361871` : feat(calendar): support custom calendar list & add explicit IDs to SKILL.md
- `f57a8a8` : feat(calendar): default to listing family schedules when requested
- `1927810` : feat(calendar): update list cmd to merge family schedules by default & clarify primary ID
- `695b37f` : feat(calendar): hardcode _AI-AGENT calendar ID for write operations to simplify arguments
- `41bf5bd` : feat(calendar): introduce target-specific subcommands to fully simplify parameters
- `ef5bc9c` : docs(calendar): update Common Mistakes in SKILL.md to match new subcommand structure
- `bf5aa81` : feat(calendar): use explicit email for Kazuaki-sama calendar ID
- `04424d6` : docs(calendar): remove redundant raw calendar IDs from SKILL.md
- `064c967` : refactor(calendar): use variable CAL_STUDY and add name comments
- `e778109` : feat(calendar): update search range from 7 days to 3 days

### Topic C: `feat/weather-tsukumijima-migration` (Tsukumijima API migration)
- `a355fe8` : docs(weather): add tsukumijima API migration design spec
- `9328659` : docs(weather): add tsukumijima migration implementation plan
- `10e2d7b` : feat(weather): replace Open-Meteo with tsukumijima API
- `3790113` : fix(weather): improve script robustness (jq error JSON, curl -S, --arg type safety)
- `e5846fb` : feat(weather): update SKILL.md for tsukumijima API coaching logic
- `63cef04` : fix(weather): clarify SKILL.md alert format, commute example, error JSON

### Topic D: `feat/patrol-schedule-routing` (Patrol schedule split & source routing)
- `199bbd2` : fix(tests): add missing cf_aig_gateway_id: None to ModelEntry/LlmModelConfig test initializers
- `39f8753` : feat(patrol): split into explore(02:00) and deliver(09:00) cron jobs
- `f1d63ee` : feat(patrol): add deliver mode flow to SKILL.md
- `49fa656` : feat(patrol): add github: and rss: source routing to SKILL.md
- `fe17809` : feat(patrol): add work-adjacent query category to SKILL.md
- `71ee77c` : feat(patrol): add sources: annotations to USER.md Interests

### docs-update (Direct commits on master after Topic D)
- `e288757` : docs(task): mark Phase 38 items 2,4,6,7,8 as completed
- `4ca5f3a` : docs(spec): add async rolling summary prototype design spec
- `6682af2` : docs(spec): update model to gemma-4-e4b and execution env to rp1

### Topic E: `feat/async-summary-proto` (Reconstruct original 641fe3e merge)
- `fa143aa` : docs(plan): write async-summary-proto implementation plan
- `ef61518` : chore(proto): scaffold rustyclaw-summary-proto crate
- `48d3147` : chore(proto): use edition 2024 to match workspace convention
- `d92da14` : feat(proto): implement ChatSession with file persistence
- `750542b` : chore(proto): remove extra lib.rs from summary-proto
- `fb6549f` : feat(proto): add SummaryProto scaffold and helper functions
- `966d442` : feat(proto): implement SummaryProto::chat() with session state management
- `56a922e` : feat(proto): implement background summary task with semaphore guard
- `c26a2c1` : feat(proto): implement interactive chat loop in main.rs
- `930f9b1` : chore(proto): create workspace/proto directory for summary persistence
- `957bda3` : test(proto): add integration test (ignored) and verify workspace tests pass
- `29c2bbe` : fix(proto): remove unused ProviderClient import

---

## 2. Bite-Sized Tasks

### Task 1: Backup Current State and Setup Script

**Files:**
- Create: `scripts/reconstruct_phase3.sh`

- [x] **Step 1: Create the script template with safety checks and backup**

Write a bash script that creates a backup of the current master and sets up a clean working tree.

```bash
cat << 'EOF' > scripts/reconstruct_phase3.sh
#!/bin/bash
set -e

REPO_DIR="/home/kazuaki/Projects/RustyClaw"
cd "$REPO_DIR"

echo "=== Step 0: Cleaning up previous branches =="
git branch -D feat/cf-neurons-tracking feat/calendar-ops-unification feat/weather-tsukumijima-migration feat/patrol-schedule-routing feat/async-summary-proto 2>/dev/null || true
git branch -D feat/context-window-stabilization fix/patrol-and-diagnostics feat/rag-memory feat/unified-rag feat/phase-40-6 feat/rmcp-migration feat/rig-tool-migration feat/seen-items-filtering docs/specs-update docs/git-branch-rules 2>/dev/null || true

echo "=== Step 1: Backup current master ==="
if git rev-parse --verify backup-master-reconstructed-v3 >/dev/null 2>&1; then
    git branch -D backup-master-reconstructed-v3
fi
git branch backup-master-reconstructed-v3 master

restore_cargo_lock() {
    if git status --porcelain | grep -q "Cargo.lock"; then
        git restore Cargo.lock || git checkout HEAD -- Cargo.lock
    fi
}

safe_cherry_pick() {
    for commit in "$@"; do
        restore_cargo_lock
        echo "Cherry-picking $commit..."
        if ! git cherry-pick "$commit"; then
            if git status --porcelain | grep -q "Cargo.lock"; then
                git checkout HEAD -- Cargo.lock
                git add Cargo.lock
                git -c core.editor=cat cherry-pick --continue
            else
                echo "Cherry-pick failed on $commit. Aborting."
                exit 1
            fi
        fi
        restore_cargo_lock
    done
}
EOF
chmod +x scripts/reconstruct_phase3.sh
```

- [x] **Step 2: Commit scripts/reconstruct_phase3.sh template**

Run:
```bash
git add scripts/reconstruct_phase3.sh
git commit -m "chore: add reconstruct_phase3.sh script template"
```
Expected: Template script committed to master.

---

### Task 2: Append Reconstruct Base Line (Topic A to E) to Script

**Files:**
- Modify: `scripts/reconstruct_phase3.sh`

- [x] **Step 1: Append Topic A, B, C, D, E and docs commits logic to the script**

Write the commands in `scripts/reconstruct_phase3.sh` to reset master, cherry-pick and merge topics.

```bash
cat << 'EOF' >> scripts/reconstruct_phase3.sh

echo "=== Step 2: Reset master to cbcc8d6 ==="
git checkout master
git reset --hard cbcc8d6
restore_cargo_lock

echo "=== Step 3: Topic A (feat/cf-neurons-tracking) ==="
git checkout -b feat/cf-neurons-tracking
safe_cherry_pick f996bf5 8cec553 624cbfe b0d423c 955d123 64ef980
git checkout master
restore_cargo_lock
git merge --no-ff feat/cf-neurons-tracking -m "Merge branch 'feat/cf-neurons-tracking' into master"
git branch -d feat/cf-neurons-tracking
restore_cargo_lock

echo "=== Step 4: Topic B (feat/calendar-ops-unification) ==="
git checkout -b feat/calendar-ops-unification
safe_cherry_pick 2b3b820 f980352 3242bc1 c3ab32b 8a7071a b7fd930 d54b363 358ecc7 f75dcc4 2361871 f57a8a8 1927810 695b37f 41bf5bd ef5bc9c bf5aa81 04424d6 064c967 e778109
git checkout master
restore_cargo_lock
git merge --no-ff feat/calendar-ops-unification -m "Merge branch 'feat/calendar-ops-unification' into master"
git branch -d feat/calendar-ops-unification
restore_cargo_lock

echo "=== Step 5: Topic C (feat/weather-tsukumijima-migration) ==="
git checkout -b feat/weather-tsukumijima-migration
safe_cherry_pick a355fe8 9328659 10e2d7b 3790113 e5846fb 63cef04
git checkout master
restore_cargo_lock
git merge --no-ff feat/weather-tsukumijima-migration -m "Merge branch 'feat/weather-tsukumijima-migration' into master"
git branch -d feat/weather-tsukumijima-migration
restore_cargo_lock

echo "=== Step 6: Topic D (feat/patrol-schedule-routing) ==="
git checkout -b feat/patrol-schedule-routing
safe_cherry_pick 199bbd2 39f8753 f1d63ee 49fa656 fe17809 71ee77c
git checkout master
restore_cargo_lock
git merge --no-ff feat/patrol-schedule-routing -m "Merge branch 'feat/patrol-schedule-routing' into master"
git branch -d feat/patrol-schedule-routing
restore_cargo_lock

echo "=== Step 7: Apply docs updates to master ==="
safe_cherry_pick e288757 4ca5f3a 6682af2

echo "=== Step 8: Topic E (feat/async-summary-proto) ==="
git checkout -b feat/async-summary-proto
safe_cherry_pick fa143aa ef61518 48d3147 d92da14 750542b fb6549f 966d442 56a922e c26a2c1 930f9b1 957bda3 29c2bbe
git checkout master
restore_cargo_lock
git merge --no-ff feat/async-summary-proto -m "feat(proto): merge async rolling summary prototype"
git branch -d feat/async-summary-proto
restore_cargo_lock
EOF
```

---

### Task 3: Append Phase 1 & 2 Topics Replay to Script

**Files:**
- Modify: `scripts/reconstruct_phase3.sh`

- [x] **Step 1: Append Phase 1 & 2 topic replays to the script**

Write the commands in `scripts/reconstruct_phase3.sh` to replay all the topics we reconstructed in previous phases. We target the commits by finding their corresponding messages in the backup branch.

```bash
cat << 'EOF' >> scripts/reconstruct_phase3.sh

# We must resolve commit hashes from backup-master-reconstructed-v3
# for Phase 1 & 2 topics.
find_commit() {
    git log backup-master-reconstructed-v3 --grep="$1" --format="%H" -n 1
}

# Find all commits dynamically
commit_stabilization_plan=$(find_commit "docs(plan): add context window phase1")
commit_stabilization_spec=$(find_commit "docs(spec): add context window Phase 1")
commit_stabilization_bump=$(find_commit "chore: bump all crates to v0.20.0")
commit_stabilization_helper=$(find_commit "feat(agent): add parse_context_window")
commit_stabilization_cap=$(find_commit "fix(agent): replace TPM-based context limit")
commit_stabilization_check=$(find_commit "fix(agent): add context_window safety check")
commit_stabilization_dup=$(find_commit "fix(gateway): prevent duplicate session-summary")
commit_stabilization_test=$(find_commit "test(gateway): fix mtime guard test scenario")
commit_stabilization_archive=$(find_commit "docs: archive context-window-phase1")

echo "=== Step 9: Replay feat/context-window-stabilization ==="
git checkout -b feat/context-window-stabilization
# Also include update settings/gateway/agent commit (originally 9377b87)
commit_stabilization_settings=$(git log backup-master-reconstructed-v3 --grep="chore: update gateway, agent, config and settings" --format="%H" -n 1)
safe_cherry_pick $commit_stabilization_settings $commit_stabilization_spec $commit_stabilization_plan $commit_stabilization_bump $commit_stabilization_helper $commit_stabilization_cap $commit_stabilization_check $commit_stabilization_dup $commit_stabilization_test $commit_stabilization_archive
git checkout master
restore_cargo_lock
git merge --no-ff feat/context-window-stabilization -m "Merge branch 'feat/context-window-stabilization' into master"
git branch -d feat/context-window-stabilization
restore_cargo_lock

echo "=== Step 10: Replay fix/patrol-and-diagnostics ==="
commit_patrol_inconsistencies=$(find_commit "fix(topic-patrol): fix SKILL.md inconsistencies")
commit_patrol_diagnostics=$(find_commit "fix(heartbeat): improve Weather Patrol diagnostics")
git checkout -b fix/patrol-and-diagnostics
safe_cherry_pick $commit_patrol_inconsistencies $commit_patrol_diagnostics
git checkout master
restore_cargo_lock
git merge --no-ff fix/patrol-and-diagnostics -m "Merge branch 'fix/patrol-and-diagnostics' into master"
git branch -d fix/patrol-and-diagnostics
restore_cargo_lock

echo "=== Step 11: Replay feat/rag-memory ==="
commit_rag_config=$(find_commit "feat(config): add EmbeddingConfig for RAG memory")
commit_rag_consistency=$(find_commit "fix(config): use bool_true default for EmbeddingConfig")
commit_rag_table=$(find_commit "feat(storage): add memory_embeddings table")
commit_rag_search=$(find_commit "feat(storage): add cosine similarity search")
commit_rag_client=$(find_commit "feat(providers): add CloudflareEmbeddingClient")
commit_rag_pipeline=$(find_commit "feat(agent): add RAG ingestion pipeline")
commit_rag_boundary=$(find_commit "fix(agent): fix UTF-8 boundary panic")
commit_rag_inject=$(find_commit "feat(agent): inject RAG context into system prompt")
commit_rag_gateway=$(find_commit "feat(config): add embedding section for RAG memory")
commit_rag_stream=$(find_commit "fix(agent): add RAG injection to execute_stream")
commit_rag_completed=$(find_commit "docs(task): mark Phase 40-3 RAG Memory")
commit_rag_resolution=$(find_commit "feat(config): add agents.embedding for model_list")
commit_rag_threshold=$(find_commit "feat(rag): lower similarity_threshold")

git checkout -b feat/rag-memory
safe_cherry_pick $commit_rag_config $commit_rag_consistency $commit_rag_table $commit_rag_search $commit_rag_client $commit_rag_pipeline $commit_rag_boundary $commit_rag_inject $commit_rag_gateway $commit_rag_stream $commit_rag_completed $commit_rag_resolution $commit_rag_threshold
git checkout master
restore_cargo_lock
git merge --no-ff feat/rag-memory -m "Merge branch 'feat/rag-memory' into master"
git branch -d feat/rag-memory
restore_cargo_lock

echo "=== Step 12: Replay feat/unified-rag ==="
commit_urag_model=$(find_commit "feat(providers): add CloudflareEmbeddingModel")
commit_urag_clone=$(find_commit "fix(providers): derive Clone for CloudflareEmbeddingModel")
commit_urag_ttl=$(find_commit "feat(config): add session_summary_ttl_days to EmbeddingConfig")
commit_urag_load=$(find_commit "feat(storage): add load_all_embeddings_with_ids")
commit_urag_engine=$(find_commit "feat(agent): add UnifiedRagEngine")
commit_urag_ingest=$(find_commit "feat(agent): update ingest_memory_md")
commit_urag_retrieve=$(find_commit "feat(agent): use UnifiedRagEngine in retrieve_rag_context")
commit_urag_plans=$(find_commit "docs(plan): add rig-core unified RAG")
commit_urag_startup=$(find_commit "feat(gateway): initialize UnifiedRagEngine")
commit_urag_ttl_val=$(find_commit "feat(config): add session_summary_ttl_days=7")
commit_urag_input=$(find_commit "fix(providers): use 'input' field")
commit_urag_tasks=$(find_commit "docs(task): Phase 40-5 完了")

git checkout -b feat/unified-rag
safe_cherry_pick $commit_urag_model $commit_urag_clone $commit_urag_ttl $commit_urag_load $commit_urag_engine $commit_urag_ingest $commit_urag_retrieve $commit_urag_plans $commit_urag_startup $commit_urag_ttl_val $commit_urag_input $commit_urag_tasks
git checkout master
restore_cargo_lock
git merge --no-ff feat/unified-rag -m "Merge branch 'feat/unified-rag' into master"
git branch -d feat/unified-rag
restore_cargo_lock

echo "=== Step 13: Replay worktrees ignore (1cd85ff/8ba9e9b) ==="
commit_worktrees_ignore=$(find_commit "chore: add .worktrees/ to .gitignore")
safe_cherry_pick $commit_worktrees_ignore

echo "=== Step 14: Replay feat/phase-40-6 ==="
commit_p40_adapter=$(find_commit "feat(tools): add RigToolAdapter")
commit_p40_model=$(find_commit "feat(providers): implement RustyclawCompletionModel")
commit_p40_failover=$(find_commit "refactor(providers): upgrade RustyclawCompletionModel")
commit_p40_loop=$(find_commit "feat(agent): Task 6 — execute_with_rig_agent")
commit_p40_task=$(find_commit "docs(task): Phase 40-6 進捗を")
commit_p40_tokens=$(find_commit "feat(gateway): wire execute_with_rig_agent")

git checkout -b feat/phase-40-6
safe_cherry_pick $commit_p40_adapter $commit_p40_model $commit_p40_failover $commit_p40_loop $commit_p40_task $commit_p40_tokens
git checkout master
restore_cargo_lock
git merge --no-ff feat/phase-40-6 -m "feat: Phase 40-6 — rig-core ReAct ループ統合 (Tasks 3 & 6)"
git branch -d feat/phase-40-6
restore_cargo_lock

echo "=== Step 15: Replay feat/rmcp-migration ==="
commit_rmcp_handler=$(find_commit "feat(agent): Task 4 — execute_with_rig_agent")
commit_rmcp_client=$(find_commit "feat(gateway): Task 4 — replace McpManager")
commit_rmcp_remove=$(find_commit "chore: remove rustyclaw-mcp crate")
commit_rmcp_done=$(find_commit "docs(task): Phase 40-6 Task 4 完了")
commit_rmcp_ref=$(find_commit "docs(task): Phase 26 参照を")
commit_rmcp_prio=$(find_commit "docs(task): Phase 40 残タスクを")

git checkout -b feat/rmcp-migration
safe_cherry_pick $commit_rmcp_handler $commit_rmcp_client $commit_rmcp_remove $commit_rmcp_done $commit_rmcp_ref $commit_rmcp_prio
git checkout master
restore_cargo_lock
git merge --no-ff feat/rmcp-migration -m "Merge branch 'feat/rmcp-migration' into master"
git branch -d feat/rmcp-migration
restore_cargo_lock

echo "=== Step 16: Replay feat/rig-tool-migration ==="
commit_rtm_msg=$(find_commit "fix(agent): separate raw and injected user messages")
commit_rtm_error=$(find_commit "feat(tools): add ToolCallError + WebFetchTool")
commit_rtm_placement=$(find_commit "fix(tools): ToolCallError placement")
commit_rtm_readwrite=$(find_commit "feat(tools): WorkspaceReadTool + WorkspaceWriteTool")
commit_rtm_impl=$(find_commit "feat(tools): MemorySearch/WebSearch/CronSchedule")
commit_rtm_align=$(find_commit "fix(tools): align WebSearch legacy schema")
commit_rtm_arc=$(find_commit "feat(tools): ToolRegistry migrated")
commit_rtm_dep=$(find_commit "fix(tools): add deprecation note")
commit_rtm_dyn=$(find_commit "feat(agent): migrate execute_heartbeat/execute_with_tools")
commit_rtm_import=$(find_commit "fix(agent): remove unused ToolDyn import")
commit_rtm_remove_adapter=$(find_commit "refactor(tools): remove custom Tool trait")
commit_rtm_done=$(find_commit "docs(task): Phase 40-2 完了マーキング")

git checkout -b feat/rig-tool-migration
safe_cherry_pick $commit_rtm_msg $commit_rtm_error $commit_rtm_placement $commit_rtm_readwrite $commit_rtm_impl $commit_rtm_align $commit_rtm_arc $commit_rtm_dep $commit_rtm_dyn $commit_rtm_import $commit_rtm_remove_adapter $commit_rtm_done
git checkout master
restore_cargo_lock
git merge --no-ff feat/rig-tool-migration -m "Merge branch 'feat/rig-tool-migration' into master"
git branch -d feat/rig-tool-migration
restore_cargo_lock

echo "=== Step 17: Replay feat/seen-items-filtering ==="
commit_seen_db=$(find_commit "feat(agent): add db_path param")
commit_seen_warning=$(find_commit "fix(agent): suppress unused db_path warning")
commit_seen_helper=$(find_commit "feat(agent): add filter_seen_tool_result helper")
commit_seen_tests=$(find_commit "fix(agent): use sync tests for filter_seen_tool_result")
commit_seen_wire=$(find_commit "feat(agent): wire filter_seen_tool_result")
commit_seen_marking=$(find_commit "docs(task): seen_items フィルタリング完了マーキング")

git checkout -b feat/seen-items-filtering
safe_cherry_pick $commit_seen_db $commit_seen_warning $commit_seen_helper $commit_seen_tests $commit_seen_wire $commit_seen_marking
git checkout master
restore_cargo_lock
git merge --no-ff feat/seen-items-filtering -m "Merge branch 'feat/seen-items-filtering' into master"
git branch -d feat/seen-items-filtering
restore_cargo_lock

echo "=== Step 18: Replay deploy, specs-update, ignore, and new rules ==="
# Deploy script
commit_deploy_script=$(find_commit "chore(deploy): skip x64 build by default")
safe_cherry_pick $commit_deploy_script

# docs/specs-update
commit_specs_update=$(find_commit "docs: update spec files")
git checkout -b docs/specs-update
safe_cherry_pick $commit_specs_update
git checkout master
restore_cargo_lock
git merge --no-ff docs/specs-update -m "Merge branch 'docs/specs-update' into master"
git branch -d docs/specs-update
restore_cargo_lock

# ignore production/neuron_usage.json
commit_ignore_neuron=$(find_commit "chore: ignore production/neuron_usage.json")
safe_cherry_pick $commit_ignore_neuron

# docs/git-branch-rules
commit_git_rules_doc=$(find_commit "docs: add git branching and merging rules")
git checkout -b docs/git-branch-rules
safe_cherry_pick $commit_git_rules_doc
git checkout master
restore_cargo_lock
git merge --no-ff docs/git-branch-rules -m "Merge branch 'docs/git-branch-rules' into master"
git branch -d docs/git-branch-rules
restore_cargo_lock

# Clean up reconstruct_phase3.sh script itself from index if untracked
git rm --cached scripts/reconstruct_phase3.sh 2>/dev/null || true

echo "=== Step 19: Verification ==="
git log --graph --oneline -n 120

echo "=== SUCCESS ==="
EOF
