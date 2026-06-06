# Remaining Native Tools Decoupling & Skill Migration Plan

This plan details the roadmap and steps for migrating the remaining native Rust tools (Weather, Google Calendar, Gmail, and Obsidian) into pure shell-script skills inside localized directories. 

---

## 1. Core Goal
Establish 100% loose coupling by removing all API domain logic from the RustyClaw Rust binary, turning it into a pure, generic executor. In addition, implement bash-level filters (`jq`, `grep`) to strip unnecessary JSON metadata, slicing context token consumption by 80%+.

---

## 2. Migration Phases

### 🔴 Phase A: Weather & Yolp Skill Migration (`skills/weather/`)
Migrate pin-point weather tracking into a self-contained skill.
* **New Script**: `production/workspace/skills/weather/scripts/504_get-weather.sh`
  * Calls Open-Meteo API using latitude/longitude.
  * Uses `jq` to extract and print only high-density information (temp, wind, next 60m rain probability).
* **Rust Binary Cleanup**: Delete `YolpWeatherTool` and its unit tests.

### 🔴 Phase B: Google Calendar Skill Migration (`skills/calendar/`)
Replace native calendar operations with clean script execution.
* **New Scripts**: 
  * `505_get-calendar.sh`: Calls `gws calendar events`, filters out IDs and reminder configurations via `jq`, and prints a clean Markdown daily schedule table.
  * `508_write-calendar.sh <calendar_id> <title> <start_time> [location]`: Safely executes `gws calendar insert`, enforcing allowed calendar checks inside the script.
* **Rust Binary Cleanup**: Delete `GwsCalendarTool` and `GwsCalendarWriteTool` and their tests.

### 🔴 Phase C: Gmail Skill Migration (`skills/gmail/`)
Safely decouple email tracking and status purging.
* **New Scripts**:
  * `506_get-gmail.sh`: Calls `gws gmail messages`, extracting only Sender, Subject, Date, and Snippet.
  * `509_delete-gmail.sh <message_id>`: Safely moves a message to trash, enforcing that only messages carrying the `_ai-agent` label are allowed to be touched.
* **Rust Binary Cleanup**: Delete `GwsGmailTool` and `GwsGmailDeleteTool` and their tests.

### 🔴 Phase D: Obsidian Operations Skill Migration (`skills/obsidian/`)
Consolidate note manipulation into a single shell script.
* **New Script**: `507_obsidian-ops.sh {search|read|write|append} <param1> [param2]`
  * Unifies search, read, and write operations utilizing the Obsidian Local REST API.
  * Employs inline Bash percent encoding and `jq` search excerpt truncation.
* **Rust Binary Cleanup**: Delete `ObsidianSearchTool`, `ObsidianReadTool`, and `ObsidianWriteTool` and their tests.

---

## 3. Preserving Critical Guardrails inside Scripts

We will preserve all existing Rust-level safety rules by implementing them directly inside the Bash scripts:
1. **Calendar Allowed List Guard**: `508_write-calendar.sh` will check the requested `calendar_id` against a hardcoded array of allowed calendars.
2. **Gmail Trash Label Guard**: `509_delete-gmail.sh` will fetch the email metadata first via `gws` and check if it contains the label `_ai-agent` before initiating the trash payload.
3. **Decoupled Configuration**: All scripts will retrieve endpoints and API keys dynamically via `run_workspace_script`'s `env: { "KEY": "$vault:key" }` variables.

---

## 4. Testing & Verification

1. **Local Compile Checks**: Run `cargo check` and `cargo test` after each phase's code deletions to ensure all remaining tests pass.
2. **Interactive Run Verification**: Run local dry-runs on the sandbox environment to verify the exact format of the filtered data returned to the LLM.
3. **Real Device Deployment**: Sync the compiled binaries and skills directories to the Raspberry Pi 4 (`rp1`) using `./scripts/deploy.sh` and inspect systemd logs.
