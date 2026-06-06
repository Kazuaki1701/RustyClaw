# Git History Reconstruction Phase 4 Implementation Plan

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-06-06  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

**Goal:** `abe5855` から `cbcc8d6` までの直線的コミット履歴（計73件）を、論理的なトピックブランチに再構築して `--no-ff` でマージし、その後に続くすべてのマージコミットをその上に綺麗にリプレイ（再構成）する。

---

## 1. 再構築の全体像

### 1-1. 分割するトピックブランチと対象コミット

#### A. `feat/phase36-gws-skills` (Google Workspace関連)
- `dc6efb2` : feat(tools): add GwsCalendarTool and GwsGmailTool via gws subprocess
- `a5f6ff9` : docs(task): mark gws Google Workspace integration complete
- `051f7ed` : fix(tools,agents): enforce read-only constraint on Google Calendar
- `6f53958` : feat(tools): add GwsCalendarWriteTool for AI AGENT calendar only
- `abcca6`  : refactor(tools): move writable calendar config to config.json
- `c15bc3a` : feat(tools): add config-based write guard to GwsCalendarWriteTool
- `281ec4f` : feat(tools): support multiple writable calendars via GWS_WRITABLE_CALENDARS
- `ceae933` : refactor(gateway): auto-resolve calendar name/desc from API at startup
- `9fe9683` : test(tools): add explicit block test for ayabe.kazuaki@gmail.com
- `5b2eef6` : test(tools): add block test for ゆうき様 shared calendar
- `c71ab02` : test(tools): add guard-pass test for AI AGENT calendar
- `502d855` : feat(tools): add GwsGmailDeleteTool with label guard + document send prohibition
- `c756b93` : feat(calendar,gmail): remove gws gateway registration block
- `83399b4` : feat(calendar,gmail): remove 4 gws native tools from rustyclaw-tools
- `5684481` : feat(gmail): add gmail SKILL.md
- `63a6948` : feat(gmail): add 509_delete-gmail.sh with _ai-agent label guard
- `e40673e` : feat(gmail): add 506_get-gmail.sh
- `2ca375a` : feat(calendar): add calendar SKILL.md
- `72b7ef1` : feat(calendar): add 508_write-calendar.sh with allowlist guard
- `46e1c97` : feat(calendar): add 505_get-calendar.sh

#### B. `feat/phase36-obsidian-karakeep-skills` (Obsidian・Karakeep関連)
- `d540c4f` : docs(task): mark Phase 36-D Obsidian skill migration complete
- `2984d1a` : feat(obsidian): remove Obsidian gateway registration block
- `746a9d3` : feat(obsidian): remove ObsidianSearchTool... from rustyclaw-tools
- `13d51e8` : feat(obsidian): add obsidian SKILL.md
- `f3682d7` : feat(obsidian): add 507_obsidian-ops.sh unified script
- `b076d2b` : design(cron): update cron.json prompts to use localized karakeep script paths
- `fd7ecbc` : design(scripts): revert global copy and use localized paths in cron.json
- `410c991` : deploy(scripts): copy karakeep scripts to production workspace scripts directory

#### C. `feat/phase24-provider-cooldowns` (プロバイダー別クールダウン制限)
- `bfa690f` : feat: propagate provider_id through record_usage call sites...
- `2e4e65b` : feat(storage): add provider_id column and by_provider...
- `f78a142` : feat(providers): add provider_id to LlmResponse...
- `981d732` : feat: implement per-provider rate-limit cooldown system
- `7169704` : docs(task): add GLOBAL_COOLDOWN deletion to Phase 24...
- `f298f07` : docs(task): refine Phase 24...

#### D. `feat/phase28b-llm-dump-rotation` (LLMダンプローテーション)
- `44bbc91` : test(providers): add retention boundary assertion...
- `1d8889c` : feat(providers): rotate llm dumps by date/time with 5-day cleanup
- `9319037` : docs(plan): add dashboard upgrade implementation plan
- `378b352` : docs(spec): add dashboard upgrade... design spec

#### E. `feat/phase31-dashboard-upgrade` (ダッシュボード表示・レイアウト改善)
- `2c42989` : fix(dashboard): display time as HH:MM:SS in inspector dropdown...
- `94071d2` : fix(dashboard): set lane left pane to fixed 136px width
- `ea9eee3` : fix(dashboard): shrink lane left pane to min-content width
- `2d05d1c` : fix(dashboard): show cron job name in sched rows...
- `bf2d066` : feat(dashboard): restore description field...
- `b02eb99` : fix(dashboard): adjust lane queue split ratio...
- `51d03ba` : fix: correct by_provider variable name in renderSummary...
- `43c16ef` : feat(dashboard): add by-provider breakdown panel...
- `5321069` : feat(dashboard): add service-colored badges to app log lines
- `33f37a7` : feat(dashboard): add date/time dropdowns to llm inspector...
- `7461ee6` : feat(dashboard): replace concurrency panel with per-provider cooldown bars
- `1c37d17` : feat(dashboard): refactor lane queue to split badge layout...
- `f67d559` : feat(dashboard): add llm/dates, llm/times APIs...

#### F. `fix/hygiene-and-misc` (その他、細微な整理)
- `13a26c9` : fix(skills): show run_workspace_script path in discovery text...
- `ffd15a4` : feat(providers): classify provider by config_name prefix...
- `b53fab9` : feat(providers): classify local IP endpoints...
- `6ced73f` : perf(agent): move [now:] to end of system context...
- `11f5dc2` : fix: align HEARTBEAT.md proactivity rules...
- `a8c5326` : docs(task): mark Phase 36-5 gateway deregistration complete
- `e644682` : fix(weather): correct mm/h to mm/15min...
- `d40af59` : docs(task): mark Phase 36-A weather skill migration complete
- `8e5877d` : fix(cron): exclude http- sessions...
- `f000ee4` : fix(cron): stagger startup using interval_at...
- `06e5eb2` : config: reduce neuron consumption...
- `40b3ed2` : fix(scripts): load HOMEASSISTANT_TOKEN from vault.json...
- `f3f1a09` : perf(workspace): compress AGENTS.md 63%...
- `0fdb009` : fix(workspace): correct tool names...
- `ba23b59` : docs(task): add Phase 16...
- `68790ba` : docs: add GeminiClaw vs RustyClaw...

---

## 2. 実施タスク

### Task 1: 再構築スクリプト `scripts/reconstruct_phase4.sh` のひな形作成

- [x] **Step 1: ひな形ファイルを作成し、安全チェックとバックアップ処理を実装**

`/home/kazuaki/Projects/RustyClaw/scripts/reconstruct_phase4.sh` を作成し、スクリプト実行時に自動で現在の `master` ブランチを `backup-master-reconstructed-v4` として保存する処理、および `Cargo.lock` 競合時の自動チェックアウト処理を定義する。

```bash
cat << 'EOF' > scripts/reconstruct_phase4.sh
#!/bin/bash
set -euo pipefail

# 1. バックアップの作成
if ! git rev-parse --verify backup-master-reconstructed-v4 >/dev/null 2>&1; then
    echo "Creating backup branch: backup-master-reconstructed-v4"
    git branch backup-master-reconstructed-v4 master
else
    echo "Backup branch backup-master-reconstructed-v4 already exists."
fi

# 2. ヘルパー関数の定義
find_commit() {
    git log backup-master-reconstructed-v4 --grep="$1" --format="%H" -n 1
}

restore_cargo_lock() {
    if git status --porcelain | grep -q "Cargo.lock"; then
        echo "Resetting Cargo.lock to resolve conflict..."
        git checkout HEAD -- Cargo.lock
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
chmod +x scripts/reconstruct_phase4.sh
```

---

### Task 2: 新規トピックブランチ（A〜F）の作成とマージ処理の追加

- [x] **Step 1: `scripts/reconstruct_phase4.sh` に新規トピックブランチのマージ処理を追記**

`abe5855` から漸進的（プログレッシブ）にブランチを構築してマージするロジックを追記する。

```bash
cat << 'EOF' >> scripts/reconstruct_phase4.sh

# 3. master をベースコミット abe5855 にリセットして再構築開始
git checkout master
git reset --hard abe5855

# ── Topic A: feat/phase36-gws-skills ──
git checkout -b feat/phase36-gws-skills
safe_cherry_pick \
    $(find_commit "feat(tools): add GwsCalendarTool and GwsGmailTool") \
    $(find_commit "docs(task): mark gws Google Workspace integration complete") \
    $(find_commit "fix(tools,agents): enforce read-only constraint on Google Calendar") \
    $(find_commit "feat(tools): add GwsCalendarWriteTool for AI AGENT calendar only") \
    $(find_commit "refactor(tools): move writable calendar config to config.json") \
    $(find_commit "feat(tools): add config-based write guard to GwsCalendarWriteTool") \
    $(find_commit "feat(tools): support multiple writable calendars") \
    $(find_commit "refactor(gateway): auto-resolve calendar name/desc") \
    $(find_commit "test(tools): add explicit block test for ayabe.kazuaki") \
    $(find_commit "test(tools): add block test for ゆうき様") \
    $(find_commit "test(tools): add guard-pass test for AI AGENT calendar") \
    $(find_commit "feat(tools): add GwsGmailDeleteTool with label guard") \
    $(find_commit "feat(calendar,gmail): remove gws gateway registration block") \
    $(find_commit "feat(calendar,gmail): remove 4 gws native tools from rustyclaw-tools") \
    $(find_commit "feat(gmail): add gmail SKILL.md") \
    $(find_commit "feat(gmail): add 509_delete-gmail.sh") \
    $(find_commit "feat(gmail): add 506_get-gmail.sh") \
    $(find_commit "feat(calendar): add calendar SKILL.md") \
    $(find_commit "feat(calendar): add 508_write-calendar.sh") \
    $(find_commit "feat(calendar): add 505_get-calendar.sh")
git checkout master
git merge --no-ff feat/phase36-gws-skills -m "Merge branch 'feat/phase36-gws-skills' into master"
git branch -d feat/phase36-gws-skills

# ── Topic B: feat/phase36-obsidian-karakeep-skills ──
git checkout -b feat/phase36-obsidian-karakeep-skills
safe_cherry_pick \
    $(find_commit "docs(task): mark Phase 36-D Obsidian") \
    $(find_commit "feat(obsidian): remove Obsidian gateway registration block") \
    $(find_commit "feat(obsidian): remove ObsidianSearchTool") \
    $(find_commit "feat(obsidian): add obsidian SKILL.md") \
    $(find_commit "feat(obsidian): add 507_obsidian-ops.sh") \
    $(find_commit "design(cron): update cron.json prompts to use localized karakeep") \
    $(find_commit "design(scripts): revert global copy and use localized") \
    $(find_commit "deploy(scripts): copy karakeep scripts")
git checkout master
git merge --no-ff feat/phase36-obsidian-karakeep-skills -m "Merge branch 'feat/phase36-obsidian-karakeep-skills' into master"
git branch -d feat/phase36-obsidian-karakeep-skills

# ── Topic C: feat/phase24-provider-cooldowns ──
git checkout -b feat/phase24-provider-cooldowns
safe_cherry_pick \
    $(find_commit "feat: propagate provider_id through record_usage") \
    $(find_commit "feat(storage): add provider_id column") \
    $(find_commit "feat(providers): add provider_id to LlmResponse") \
    $(find_commit "feat: implement per-provider rate-limit cooldown system") \
    $(find_commit "docs(task): add GLOBAL_COOLDOWN deletion") \
    $(find_commit "docs(task): refine Phase 24")
git checkout master
git merge --no-ff feat/phase24-provider-cooldowns -m "Merge branch 'feat/phase24-provider-cooldowns' into master"
git branch -d feat/phase24-provider-cooldowns

# ── Topic D: feat/phase28b-llm-dump-rotation ──
git checkout -b feat/phase28b-llm-dump-rotation
safe_cherry_pick \
    $(find_commit "test(providers): add retention boundary") \
    $(find_commit "feat(providers): rotate llm dumps by date/time") \
    $(find_commit "docs(plan): add dashboard upgrade implementation plan") \
    $(find_commit "docs(spec): add dashboard upgrade")
git checkout master
git merge --no-ff feat/phase28b-llm-dump-rotation -m "Merge branch 'feat/phase28b-llm-dump-rotation' into master"
git branch -d feat/phase28b-llm-dump-rotation

# ── Topic E: feat/phase31-dashboard-upgrade ──
git checkout -b feat/phase31-dashboard-upgrade
safe_cherry_pick \
    $(find_commit "fix(dashboard): display time as HH:MM:SS") \
    $(find_commit "fix(dashboard): set lane left pane to fixed 136px") \
    $(find_commit "fix(dashboard): shrink lane left pane to min-content") \
    $(find_commit "fix(dashboard): show cron job name in sched rows") \
    $(find_commit "feat(dashboard): restore description field") \
    $(find_commit "fix(dashboard): adjust lane queue split ratio") \
    $(find_commit "fix: correct by_provider variable name in renderSummary") \
    $(find_commit "feat(dashboard): add by-provider breakdown panel") \
    $(find_commit "feat(dashboard): add service-colored badges to app log lines") \
    $(find_commit "feat(dashboard): add date/time dropdowns to llm inspector") \
    $(find_commit "feat(dashboard): replace concurrency panel with per-provider") \
    $(find_commit "feat(dashboard): refactor lane queue to split badge layout") \
    $(find_commit "feat(dashboard): add llm/dates, llm/times APIs")
git checkout master
git merge --no-ff feat/phase31-dashboard-upgrade -m "Merge branch 'feat/phase31-dashboard-upgrade' into master"
git branch -d feat/phase31-dashboard-upgrade

# ── Topic F: fix/hygiene-and-misc ──
git checkout -b fix/hygiene-and-misc
safe_cherry_pick \
    $(find_commit "fix(skills): show run_workspace_script path") \
    $(find_commit "feat(providers): classify provider by config_name") \
    $(find_commit "feat(providers): classify local IP endpoints") \
    $(find_commit "perf(agent): move \[now:\] to end") \
    $(find_commit "fix: align HEARTBEAT.md proactivity rules") \
    $(find_commit "docs(task): mark Phase 36-5 gateway deregistration") \
    $(find_commit "fix(weather): correct mm/h to mm/15min") \
    $(find_commit "docs(task): mark Phase 36-A weather") \
    $(find_commit "fix(cron): exclude http- sessions") \
    $(find_commit "fix(cron): stagger startup using interval_at") \
    $(find_commit "config: reduce neuron consumption") \
    $(find_commit "fix(scripts): load HOMEASSISTANT_TOKEN") \
    $(find_commit "perf(workspace): compress AGENTS.md") \
    $(find_commit "fix(workspace): correct tool names") \
    $(find_commit "docs(task): add Phase 16") \
    $(find_commit "docs: add GeminiClaw vs RustyClaw")
git checkout master
git merge --no-ff fix/hygiene-and-misc -m "Merge branch 'fix/hygiene-and-misc' into master"
git branch -d fix/hygiene-and-misc

EOF
```

---

### Task 3: 後続コミット（Phase 3 および Phase 1 & 2）のマージ・リプレイ追記

- [x] **Step 1: `scripts/reconstruct_phase4.sh` に `cbcc8d6` 以降の全トピックマージのリプレイを追記**

Phase 3およびそれ移行に master に統合されたすべてのトピックブランチのマージコミット順序を正確に再現するロジックを追記する。

```bash
cat << 'EOF' >> scripts/reconstruct_phase4.sh

# ── リプレイ: cbcc8d6 (base of Phase 3) ──
safe_cherry_pick $(find_commit "feat(agent): add hard message count cap")

# ── リプレイ: Phase 3 トピックブランチ ──
# 1. feat/cf-neurons-tracking
git checkout -b feat/cf-neurons-tracking
safe_cherry_pick \
    $(find_commit "debug(providers): log CF neurons header presence") \
    $(find_commit "debug(providers): elevate cf-ai-neurons missing log") \
    $(find_commit "fix(providers): track CF neurons via total_tokens") \
    $(find_commit "fix(config): add workers-ai/ prefix") \
    $(find_commit "perf(agent): reduce context size to stay") \
    $(find_commit "fix(providers): calculate CF neurons from model-specific")
git checkout master
git merge --no-ff feat/cf-neurons-tracking -m "Merge branch 'feat/cf-neurons-tracking' into master"
git branch -d feat/cf-neurons-tracking

# 2. feat/calendar-ops-unification
git checkout -b feat/calendar-ops-unification
safe_cherry_pick \
    $(find_commit "fix(skills): fix PATH and timezone issues") \
    $(find_commit "fix(calendar): convert exclusive end date") \
    $(find_commit "fix(calendar): add weekday field in Japanese") \
    $(find_commit "fix(calendar): add start_wday/end_wday fields") \
    $(find_commit "docs(plan): add calendar-ops.sh unification") \
    $(find_commit "feat(calendar): consolidate calendar scripts") \
    $(find_commit "cleanup(calendar): remove obsolete calendar") \
    $(find_commit "docs(calendar): add examples for common user requests") \
    $(find_commit "docs(calendar): add investigation report for chat") \
    $(find_commit "feat(calendar): support custom calendar list") \
    $(find_commit "feat(calendar): default to listing family schedules") \
    $(find_commit "feat(calendar): update list cmd to merge family") \
    $(find_commit "feat(calendar): hardcode _AI-AGENT calendar ID") \
    $(find_commit "feat(calendar): introduce target-specific subcommands") \
    $(find_commit "docs(calendar): update Common Mistakes in") \
    $(find_commit "docs(calendar): remove redundant raw calendar") \
    $(find_commit "refactor(calendar): use variable CAL_STUDY") \
    $(find_commit "feat(calendar): update search range from 7 days") \
    $(find_commit "feat(calendar): use explicit email for Kazuaki")
git checkout master
git merge --no-ff feat/calendar-ops-unification -m "Merge branch 'feat/calendar-ops-unification' into master"
git branch -d feat/calendar-ops-unification

# 3. feat/weather-tsukumijima-migration
git checkout -b feat/weather-tsukumijima-migration
safe_cherry_pick \
    $(find_commit "docs(weather): add tsukumijima API migration") \
    $(find_commit "docs(weather): add tsukumijima migration implementation plan") \
    $(find_commit "feat(weather): replace Open-Meteo with tsukumijima") \
    $(find_commit "fix(weather): improve script robustness") \
    $(find_commit "feat(weather): update SKILL.md for tsukumijima") \
    $(find_commit "fix(weather): clarify SKILL.md alert format")
git checkout master
git merge --no-ff feat/weather-tsukumijima-migration -m "Merge branch 'feat/weather-tsukumijima-migration' into master"
git branch -d feat/weather-tsukumijima-migration

# 4. feat/patrol-schedule-routing
git checkout -b feat/patrol-schedule-routing
safe_cherry_pick \
    $(find_commit "fix(tests): add missing cf_aig_gateway_id") \
    $(find_commit "feat(patrol): split into explore") \
    $(find_commit "feat(patrol): add deliver mode flow") \
    $(find_commit "feat(patrol): add github: and rss:") \
    $(find_commit "feat(patrol): add work-adjacent query") \
    $(find_commit "feat(patrol): add sources: annotations")
git checkout master
git merge --no-ff feat/patrol-schedule-routing -m "Merge branch 'feat/patrol-schedule-routing' into master"
git branch -d feat/patrol-schedule-routing

# 5. feat/async-summary-proto
git checkout -b feat/async-summary-proto
safe_cherry_pick \
    $(find_commit "docs(plan): write async-summary-proto") \
    $(find_commit "chore(proto): scaffold rustyclaw-summary-proto") \
    $(find_commit "chore(proto): use edition 2024") \
    $(find_commit "feat(proto): implement ChatSession") \
    $(find_commit "chore(proto): remove extra lib.rs") \
    $(find_commit "feat(proto): add SummaryProto scaffold") \
    $(find_commit "feat(proto): implement SummaryProto::chat") \
    $(find_commit "feat(proto): implement background summary task") \
    $(find_commit "feat(proto): implement interactive chat loop") \
    $(find_commit "chore(proto): create workspace/proto") \
    $(find_commit "test(proto): add integration test") \
    $(find_commit "fix(proto): remove unused ProviderClient") \
    $(find_commit "docs(spec): update model to gemma") \
    $(find_commit "docs(spec): add async rolling summary") \
    $(find_commit "docs(task): mark Phase 38 items")
git checkout master
git merge --no-ff feat/async-summary-proto -m "feat(proto): merge async rolling summary prototype"
git branch -d feat/async-summary-proto

# ── リプレイ: Phase 1 & 2 トピックブランチ ──
# 1. feat/context-window-stabilization
git checkout -b feat/context-window-stabilization
safe_cherry_pick \
    $(find_commit "chore: update gateway, agent, config") \
    $(find_commit "docs(spec): add context window Phase 1") \
    $(find_commit "docs(plan): add context window phase1") \
    $(find_commit "feat(agent): add parse_context_window") \
    $(find_commit "fix(agent): replace TPM-based context limit") \
    $(find_commit "fix(agent): add context_window safety check") \
    $(find_commit "fix(gateway): prevent duplicate session-summary") \
    $(find_commit "test(gateway): fix mtime guard test scenario") \
    $(find_commit "docs: archive context-window-phase1")
git checkout master
git merge --no-ff feat/context-window-stabilization -m "Merge branch 'feat/context-window-stabilization' into master"
git branch -d feat/context-window-stabilization

# 2. fix/patrol-and-diagnostics
git checkout -b fix/patrol-and-diagnostics
safe_cherry_pick \
    $(find_commit "fix(topic-patrol): fix SKILL.md inconsistencies") \
    $(find_commit "fix(heartbeat): improve Weather Patrol diagnostics")
git checkout master
git merge --no-ff fix/patrol-and-diagnostics -m "Merge branch 'fix/patrol-and-diagnostics' into master"
git branch -d fix/patrol-and-diagnostics

# 3. feat/rag-memory
git checkout -b feat/rag-memory
safe_cherry_pick \
    $(find_commit "feat(config): add EmbeddingConfig for RAG") \
    $(find_commit "fix(config): use bool_true default") \
    $(find_commit "feat(storage): add memory_embeddings table") \
    $(find_commit "feat(storage): add cosine similarity search") \
    $(find_commit "feat(providers): add CloudflareEmbeddingClient") \
    $(find_commit "feat(agent): add RAG ingestion pipeline") \
    $(find_commit "fix(agent): fix UTF-8 boundary panic") \
    $(find_commit "feat(agent): inject RAG context into") \
    $(find_commit "feat(config): add embedding section") \
    $(find_commit "fix(agent): add RAG injection to execute_stream") \
    $(find_commit "docs(task): mark Phase 40-3 RAG Memory") \
    $(find_commit "feat(config): add agents.embedding") \
    $(find_commit "feat(rag): lower similarity_threshold")
git checkout master
git merge --no-ff feat/rag-memory -m "Merge branch 'feat/rag-memory' into master"
git branch -d feat/rag-memory

# 4. feat/unified-rag
git checkout -b feat/unified-rag
safe_cherry_pick \
    $(find_commit "feat(providers): add CloudflareEmbeddingModel") \
    $(find_commit "fix(providers): derive Clone for CloudflareEmbeddingModel") \
    $(find_commit "feat(config): add session_summary_ttl_days to EmbeddingConfig") \
    $(find_commit "feat(storage): add load_all_embeddings_with_ids") \
    $(find_commit "feat(agent): add UnifiedRagEngine with") \
    $(find_commit "feat(agent): update ingest_memory_md with Option") \
    $(find_commit "feat(agent): use UnifiedRagEngine in retrieve_rag_context") \
    $(find_commit "docs(plan): add rig-core unified RAG") \
    $(find_commit "feat(gateway): initialize UnifiedRagEngine") \
    # 重複コミット a0f0507 (fix(config): add workers-ai/ prefix) はチェリーピックから除外
    $(find_commit "feat(config): add session_summary_ttl_days=7") \
    $(find_commit "fix(providers): use 'input' field for OpenAI-compat") \
    $(find_commit "docs(task): Phase 40-5 完了")
git checkout master
git merge --no-ff feat/unified-rag -m "Merge branch 'feat/unified-rag' into master"
git branch -d feat/unified-rag

# 5. feat/phase-40-6 (rmcp 移行の準備)
git checkout -b feat/phase-40-6
safe_cherry_pick \
    $(find_commit "chore: add .worktrees/ to .gitignore") \
    $(find_commit "feat(tools): add RigToolAdapter and ToolRegistry") \
    $(find_commit "feat(providers): implement RustyclawCompletionModel") \
    $(find_commit "refactor(providers): upgrade RustyclawCompletionModel") \
    $(find_commit "feat(agent): Task 6 — execute_with_rig_agent using") \
    $(find_commit "docs(task): Phase 40-6 進捗を") \
    $(find_commit "feat(gateway): wire execute_with_rig_agent") \
    $(find_commit "feat: Phase 40-6 — rig-core ReAct")
git checkout master
git merge --no-ff feat/phase-40-6 -m "Merge branch 'feat/phase-40-6' into master"
git branch -d feat/phase-40-6

# 6. feat/rmcp-migration
git checkout -b feat/rmcp-migration
safe_cherry_pick \
    $(find_commit "feat(agent): Task 4 — execute_with_rig_agent accepts") \
    $(find_commit "feat(gateway): Task 4 — replace McpManager") \
    $(find_commit "chore: remove rustyclaw-mcp") \
    $(find_commit "docs(task): Phase 40-6 Task 4") \
    $(find_commit "docs(task): Phase 26 参照を") \
    $(find_commit "docs(task): Phase 40 残タスクを最優先")
git checkout master
git merge --no-ff feat/rmcp-migration -m "Merge branch 'feat/rmcp-migration' into master"
git branch -d feat/rmcp-migration

# 7. feat/rig-tool-migration
git checkout -b feat/rig-tool-migration
safe_cherry_pick \
    $(find_commit "fix(agent): separate raw and injected") \
    $(find_commit "feat(tools): add ToolCallError \+ WebFetchTool") \
    $(find_commit "fix(tools): ToolCallError placement") \
    $(find_commit "feat(tools): WorkspaceReadTool \+ WorkspaceWriteTool") \
    $(find_commit "feat(tools): MemorySearch/WebSearch/CronSchedule") \
    $(find_commit "fix(tools): align WebSearch legacy schema") \
    $(find_commit "feat(tools): ToolRegistry migrated to") \
    $(find_commit "fix(tools): add deprecation note") \
    $(find_commit "feat(agent): migrate execute_heartbeat/execute_with_tools") \
    $(find_commit "fix(agent): remove unused ToolDyn") \
    $(find_commit "refactor(tools): remove custom Tool trait") \
    $(find_commit "docs(task): Phase 40-2 完了")
git checkout master
git merge --no-ff feat/rig-tool-migration -m "Merge branch 'feat/rig-tool-migration' into master"
git branch -d feat/rig-tool-migration

# 8. feat/seen-items-filtering
git checkout -b feat/seen-items-filtering
safe_cherry_pick \
    $(find_commit "feat(agent): add db_path param to execute_heartbeat") \
    $(find_commit "fix(agent): suppress unused db_path") \
    $(find_commit "feat(agent): add filter_seen_tool_result") \
    $(find_commit "fix(agent): use sync tests for filter_seen_tool_result") \
    $(find_commit "feat(agent): wire filter_seen_tool_result into") \
    $(find_commit "docs(task): seen_items フィルタリング完了")
git checkout master
git merge --no-ff feat/seen-items-filtering -m "Merge branch 'feat/seen-items-filtering' into master"
git branch -d feat/seen-items-filtering

# 9. docs/specs-update
git checkout -b docs/specs-update
safe_cherry_pick $(find_commit "docs: update spec files")
git checkout master
git merge --no-ff docs/specs-update -m "Merge branch 'docs/specs-update' into master"
git branch -d docs/specs-update

# 10. docs/git-branch-rules
git checkout -b docs/git-branch-rules
safe_cherry_pick $(find_commit "docs: add git branching and merging rules")
git checkout master
git merge --no-ff docs/git-branch-rules -m "Merge branch 'docs/git-branch-rules' into master"
git branch -d docs/git-branch-rules

# 11. feat/static-docs-rag
git checkout -b feat/static-docs-rag
safe_cherry_pick \
    $(find_commit "feat\(storage\): add document_states table") \
    $(find_commit "fix\(storage\): check_and_update_doc_state") \
    $(find_commit "feat\(agent\): add chunk_static_document") \
    $(find_commit "fix\(agent\): chunk_static_document skip empty-header") \
    $(find_commit "feat\(gateway\): trigger ingest_static_documents") \
    $(find_commit "feat\(agent\): extend format_rag_context") \
    $(find_commit "fix\(agent\): use stored source column") \
    $(find_commit "docs\(agent\): update search method comment") \
    $(find_commit "feat\(agent\): remove AGENTS.md from static prompt") \
    $(find_commit "docs\(agent\): fix stale comment in")
git checkout master
git merge --no-ff feat/static-docs-rag -m "Merge branch 'feat/static-docs-rag' into master"
git branch -d feat/static-docs-rag

# 12. docs/phase40-7-completion
git checkout -b docs/phase40-7-completion
safe_cherry_pick \
    $(find_commit "docs: update task.md, plan, README") \
    $(find_commit "chore: update Cargo.lock for sha2") \
    $(find_commit "docs: add ai-rules.md and ADR template") \
    $(find_commit "docs: add Phase 40 completed tasks") \
    $(find_commit "docs: Phase 40-7 完了処理")
git checkout master
git merge --no-ff docs/phase40-7-completion -m "Merge branch 'docs/phase40-7-completion' into master"
git branch -d docs/phase40-7-completion

# 13. docs/phase40-7-bug-record
git checkout -b docs/phase40-7-bug-record
safe_cherry_pick $(find_commit "docs\(task\): Phase 40-7 残存バグ")
git checkout master
git merge --no-ff docs/phase40-7-bug-record -m "Merge branch 'docs/phase40-7-bug-record' into master"
git branch -d docs/phase40-7-bug-record

# 14. feat/phase28b4-heartbeat-overflow
git checkout -b feat/phase28b4-heartbeat-overflow
safe_cherry_pick \
    $(find_commit "feat\(agent\): Phase 28b-4 add trim_heartbeat_messages") \
    $(find_commit "test\(agent\): Phase 28b-4 add tests for truncate") \
    $(find_commit "feat\(agent\): Phase 28b-4 cap tool results") \
    $(find_commit "docs: Phase 28b-4 仕様書更新")
git checkout master
git merge --no-ff feat/phase28b4-heartbeat-overflow -m "Merge branch 'feat/phase28b4-heartbeat-overflow' into master"
git branch -d feat/phase28b4-heartbeat-overflow

# 15. docs/cleanup-and-reorganization
git checkout -b docs/cleanup-and-reorganization
safe_cherry_pick $(find_commit "docs: comprehensive spec reorganization")
git checkout master
git merge --no-ff docs/cleanup-and-reorganization -m "Merge branch 'docs/cleanup-and-reorganization' into master"
git branch -d docs/cleanup-and-reorganization

# 16. docs/delete-old-index
git checkout -b docs/delete-old-index
safe_cherry_pick $(find_commit "docs: remove obsolete docs/00_rustyclaw.md copy")
git checkout master
git merge --no-ff docs/delete-old-index -m "Merge branch 'docs/delete-old-index' into master"
git branch -d docs/delete-old-index

# 4. 一時スクリプトの削除
git rm --cached scripts/reconstruct_phase4.sh 2>/dev/null || true

echo "=== SUCCESS ==="
EOF
```

---

### Task 4: テストとクリーンアップ

- [x] **Step 1: スクリプトの実行と再構築の完了検証**

`./scripts/reconstruct_phase4.sh` を実行し、マージグラフの再構築がエラーなく完了することを確認する。

- [x] **Step 2: 開発ブランチ `feat/phase40-8-local-embedding` の再適用**

開発ブランチを新しい master に基づいて再構築し、開発中のコードコミットを再チェリーピックして作業環境を復旧する。
