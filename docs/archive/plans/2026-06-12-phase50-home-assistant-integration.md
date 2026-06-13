# Phase 50: HomeAssistant センサー統合 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** HA センサー（室温・CO2・湿度・人感・外気温）の時系列トレンドを Heartbeat コンテキストに注入し、CO2 スパイク時は即時 Proactive 通知を発する。

**Architecture:** 既存の `home-assistant-rest-api` SKILL スクリプトを基盤として `220_ha_env_snapshot.sh` を新設。Rust 側は CronService が 10 分おきに bash スクリプトを spawn してスナップショット + トレンド計算済みの `memory/ha-state.json` を更新する。HeartbeatService はこの state ファイルを読んで 1 行サマリーを Heartbeat プロンプトに注入し、スパイク検知時は緊急アラートとして扱う。

**Tech Stack:** Rust (tokio::process::Command, serde_json), sh/jq (220_ha_env_snapshot.sh), 既存 `__200_ha_common.sh` / `$HOMEASSISTANT_TOKEN` (vault 注入済み by Phase 49-2)

---

## ファイル構成

| 操作 | ファイル | 内容 |
|---|---|---|
| 新規 | `production/workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh` | センサー取得・リングバッファ更新・トレンド計算・スパイク検知 |
| 変更 | `crates/rustyclaw-config/src/lib.rs` | `HomeAssistantConfig` 追加 |
| 変更 | `crates/rustyclaw-gateway/src/cron.rs` | `workspace_path` フィールド追加・10 分 HA ポーリングループ追加 |
| 変更 | `crates/rustyclaw-gateway/src/heartbeat.rs` | `get_ha_env_context()` / `check_ha_spike()` 追加 |
| 変更 | `crates/rustyclaw-gateway/src/lib.rs` | HA context を heartbeat_prompt に注入・CronService 初期化に workspace_path 追加 |
| 変更 | `production/workspace/HEARTBEAT.md` | Step 2 に HA 環境コンテキスト指示追加 |
| 変更 | `production/workspace/skills/home-assistant-rest-api/SKILL.md` | 220 スクリプト行を表に追加 |

---

## Task 1: `220_ha_env_snapshot.sh` スクリプト作成

**Files:**
- Create: `production/workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh`

- [ ] **Step 1: スクリプトを作成する**

```sh
#!/bin/sh
# 220_ha_env_snapshot.sh — HA 環境スナップショット & トレンド計算
#
# 出力 (stdout):
#   [HA_ENV|HH:MM] [Room: XX.X°C↑ / XX%→] [CO2: XXXXppm↑] [Presence: Detected] [Outer: XX.X°C]
#
# 状態ファイル: memory/ha-state.json
#   .samples  — 最大 6 サンプルのリングバッファ [{ts, room_temp, room_humid, outer_temp, co2}, ...]
#   .latest   — 最新サンプル
#   .summary  — 1 行サマリー文字列
#   .spike_detected — CO2 > 1500 ppm の場合 true
#
# 終了コード:
#   0 — 正常
#   1 — HA 到達不能（token 未設定・タイムアウト等）
#   2 — スパイク検知 (--check-spike オプション指定時のみ)
#
# 使用方法:
#   bash workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh
#   bash workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh --check-spike
#
. "$(dirname "$0")/__200_ha_common.sh"

MEMORY_DIR="$HA_PROJECT_ROOT/memory"
STATE_FILE="$MEMORY_DIR/ha-state.json"
SUMMARY_FILE="$MEMORY_DIR/ha-env-summary.txt"
CHECK_SPIKE=false

for arg in "$@"; do
    [ "$arg" = "--check-spike" ] && CHECK_SPIKE=true
done

mkdir -p "$MEMORY_DIR"

# 1. HA REST API からセンサー一括取得
STATES=$(wget -qO- --timeout=10 --header "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
    "$HA_ENDPOINT/states" 2>/dev/null)
if [ -z "$STATES" ]; then
    echo "ERROR: HA unreachable or HOMEASSISTANT_TOKEN not set" >&2
    exit 1
fi

# 2. センサー値抽出 (jq)
ROOM_TEMP=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id == "sensor.livingroom_air_temperature")] | first | .state // "unknown"')
ROOM_HUMID=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id == "sensor.livingroom_air_humidity")] | first | .state // "unknown"')
OUTER_TEMP=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id == "sensor.outside1f_air_temperature")] | first | .state // "unknown"')
CO2=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id | test("sensor\\..*co2|sensor\\..*carbon_dioxide"))] | first | .state // "unknown"')
PRESENCE=$(printf '%s' "$STATES" | jq -r \
    '[.[] | select(.entity_id | test("binary_sensor\\..*motion|binary_sensor\\..*presence"))] | first | .state // "off"')

NOW=$(date +"%H:%M")
TS=$(date -Iseconds)

# 3. リングバッファ更新 (最大 6 サンプル)
if [ -f "$STATE_FILE" ]; then
    PREV_JSON=$(cat "$STATE_FILE")
else
    PREV_JSON='{"samples":[]}'
fi

NEW_SAMPLE=$(jq -cn \
    --arg ts "$TS" \
    --arg rt "$ROOM_TEMP" \
    --arg rh "$ROOM_HUMID" \
    --arg ot "$OUTER_TEMP" \
    --arg co2 "$CO2" \
    '{ts:$ts, room_temp:$rt, room_humid:$rh, outer_temp:$ot, co2:$co2}')

UPDATED=$(printf '%s' "$PREV_JSON" | jq \
    --argjson s "$NEW_SAMPLE" \
    '.samples += [$s] | .samples = (.samples | if length > 6 then .[-6:] else . end) | .latest = $s')

# 4. トレンド計算 (oldest と latest の差)
trend_arrow() {
    CURR="$1"; PREV="$2"; THRESH="$3"
    [ "$CURR" = "unknown" ] || [ "$PREV" = "unknown" ] && { echo "→"; return; }
    DIFF=$(awk -v c="$CURR" -v p="$PREV" -v t="$THRESH" \
        'BEGIN { d=c-p; if(d>t) print "up"; else if(d<-t) print "down"; else print "flat"}')
    case "$DIFF" in up) echo "↑" ;; down) echo "↓" ;; *) echo "→" ;; esac
}

OLDEST_TEMP=$(printf '%s' "$UPDATED" | jq -r '.samples[0].room_temp // "unknown"')
OLDEST_HUMID=$(printf '%s' "$UPDATED" | jq -r '.samples[0].room_humid // "unknown"')
OLDEST_CO2=$(printf '%s' "$UPDATED" | jq -r '.samples[0].co2 // "unknown"')

TEMP_ARROW=$(trend_arrow "$ROOM_TEMP" "$OLDEST_TEMP" "0.5")
HUMID_ARROW=$(trend_arrow "$ROOM_HUMID" "$OLDEST_HUMID" "3")
CO2_ARROW=$(trend_arrow "$CO2" "$OLDEST_CO2" "50")

# 5. スパイク検知 (CO2 > 1500 ppm)
SPIKE=false
if [ "$CO2" != "unknown" ]; then
    SPIKE=$(awk -v c="$CO2" 'BEGIN { if(c+0 > 1500) print "true"; else print "false"}')
fi

# 6. 状態ファイル書き込み
PRESENCE_STR="None"
{ [ "$PRESENCE" = "on" ] || [ "$PRESENCE" = "detected" ]; } && PRESENCE_STR="Detected"

SUMMARY="[HA_ENV|${NOW}] [Room: ${ROOM_TEMP}°C${TEMP_ARROW} / ${ROOM_HUMID}%${HUMID_ARROW}] [CO2: ${CO2}ppm${CO2_ARROW}] [Presence: ${PRESENCE_STR}] [Outer: ${OUTER_TEMP}°C]"

printf '%s' "$UPDATED" | jq \
    --arg summary "$SUMMARY" \
    --argjson spike "$SPIKE" \
    '.summary = $summary | .spike_detected = $spike' > "$STATE_FILE"

echo "$SUMMARY" > "$SUMMARY_FILE"

# 7. stdout 出力
echo "$SUMMARY"

# --check-spike: スパイク時に exit 2 (Rust CronService がこれを検出して即時 Heartbeat を発火)
if [ "$CHECK_SPIKE" = "true" ] && [ "$SPIKE" = "true" ]; then
    echo "SPIKE_DETECTED: CO2=${CO2}ppm" >&2
    exit 2
fi
```

- [ ] **Step 2: 実行権限を付与する**

```bash
chmod +x production/workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh
```

- [ ] **Step 3: 手動実行で動作確認する**

```bash
# HOMEASSISTANT_TOKEN が vault から注入されている前提
HOMEASSISTANT_TOKEN=$(cat ~/.rustyclaw/config/vault.json 2>/dev/null | jq -r '.HOMEASSISTANT_TOKEN // empty') \
  bash production/workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh
```

期待出力例: `[HA_ENV|14:30] [Room: 27.5°C→ / 62%→] [CO2: 850ppm→] [Presence: Detected] [Outer: 32.1°C]`

確認項目:
- `~/.rustyclaw/memory/ha-state.json` が生成されている
- `~/.rustyclaw/memory/ha-env-summary.txt` が生成されている
- `.samples` に 1 エントリが入っている

- [ ] **Step 4: コミット**

```bash
git add production/workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh
git commit -m "feat(ha): add 220_ha_env_snapshot.sh with ring-buffer trend tracking

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 2: `HomeAssistantConfig` を rustyclaw-config に追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`

対象箇所: `ToolsConfig` 構造体（現在 `google_workspace` と `brave_search` の 2 フィールドを持つ。行番号は grep で確認すること）。

- [ ] **Step 1: `HomeAssistantConfig` 構造体を追加するテストを書く**

`crates/rustyclaw-config/src/lib.rs` のテストモジュール末尾に追加:

```rust
#[test]
fn test_home_assistant_config_defaults() {
    let cfg: HomeAssistantConfig = serde_json::from_str(r#"{}"#).unwrap();
    assert!(!cfg.enabled);
    assert_eq!(cfg.endpoint, "http://192.168.1.30:8123");
    assert_eq!(cfg.poll_interval_secs, 600);
    assert!((cfg.spike_co2_ppm - 1500.0).abs() < 1e-9);
}

#[test]
fn test_home_assistant_config_in_tools() {
    let json = r#"{
        "model_list": [],
        "agents": {"default": "none"},
        "tools": {
            "home-assistant": {
                "enabled": true,
                "endpoint": "http://192.168.1.50:8123",
                "token": "test-token",
                "poll_interval_secs": 300,
                "spike_co2_ppm": 1200.0
            }
        }
    }"#;
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(json.as_bytes()).unwrap();
    let config = load_config(f.path()).unwrap();
    let ha = config.tools.home_assistant.unwrap();
    assert!(ha.enabled);
    assert_eq!(ha.endpoint, "http://192.168.1.50:8123");
    assert_eq!(ha.poll_interval_secs, 300);
    assert!((ha.spike_co2_ppm - 1200.0).abs() < 1e-9);
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
TZ=UTC cargo test -p rustyclaw-config test_home_assistant_config 2>&1 | tail -5
```

期待: `FAILED` (HomeAssistantConfig が未定義)

- [ ] **Step 3: `HomeAssistantConfig` と `ToolsConfig` 拡張を実装する**

`ToolsConfig` の直前に追加（`BraveSearchConfig` の後）:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeAssistantConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ha_endpoint")]
    pub endpoint: String,
    /// API トークン（$vault:HOMEASSISTANT_TOKEN で参照可）
    #[serde(default)]
    pub token: String,
    /// センサーポーリング間隔（秒、デフォルト 600 = 10 分）
    #[serde(default = "default_ha_poll_secs")]
    pub poll_interval_secs: u64,
    /// CO2 スパイク閾値（ppm、デフォルト 1500）
    #[serde(default = "default_co2_spike_ppm")]
    pub spike_co2_ppm: f64,
}

fn default_ha_endpoint() -> String {
    "http://192.168.1.30:8123".to_string()
}
fn default_ha_poll_secs() -> u64 {
    600
}
fn default_co2_spike_ppm() -> f64 {
    1500.0
}

impl Default for HomeAssistantConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_ha_endpoint(),
            token: String::new(),
            poll_interval_secs: default_ha_poll_secs(),
            spike_co2_ppm: default_co2_spike_ppm(),
        }
    }
}
```

`ToolsConfig` に `home_assistant` フィールドを追加:

```rust
pub struct ToolsConfig {
    #[serde(default, rename = "google-workspace")]
    pub google_workspace: Option<GoogleWorkspaceConfig>,
    #[serde(default, rename = "brave-search")]
    pub brave_search: Option<BraveSearchConfig>,
    #[serde(default, rename = "home-assistant")]
    pub home_assistant: Option<HomeAssistantConfig>,
}
```

`resolve_secrets()` に HA token の解決を追加（`brave_search` の resolve ブロックの後）:

```rust
if let Some(ref mut ha) = self.tools.home_assistant {
    ha.token = resolve_value(&ha.token);
}
```

- [ ] **Step 4: テストを通す**

```bash
TZ=UTC cargo test -p rustyclaw-config test_home_assistant_config 2>&1 | tail -5
```

期待: `test result: ok. 2 passed`

- [ ] **Step 5: ワークスペース全体のテストを通す**

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | tail -10
```

期待: `test result: ok.`

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): add HomeAssistantConfig with endpoint/token/poll/spike settings

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 3: HeartbeatService に HA 環境読み取りメソッドを追加

**Files:**
- Modify: `crates/rustyclaw-gateway/src/heartbeat.rs`

`HeartbeatService` の実装ブロック（`impl HeartbeatService`）内、`run_weather_patrol` の前後に追加する。

- [ ] **Step 1: テストを書く**

`heartbeat.rs` のテストモジュール（`#[cfg(test)] mod tests` 内）末尾に追加:

```rust
#[tokio::test]
async fn test_get_ha_env_context_reads_summary_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = dir.path().to_path_buf();
    let memory_dir = ws.join("memory");
    std::fs::create_dir_all(&memory_dir)?;

    // summary ファイルなし → None
    let bus = std::sync::Arc::new(MessageBus::new());
    let svc = HeartbeatService::new(Config::default(), ws.clone(), bus.clone());
    assert!(svc.get_ha_env_context().is_none());

    // summary ファイルあり → Some
    let summary = "[HA_ENV|14:30] [Room: 27.5°C→ / 62%→] [CO2: 850ppm→]";
    std::fs::write(memory_dir.join("ha-env-summary.txt"), summary)?;
    assert_eq!(svc.get_ha_env_context().as_deref(), Some(summary));
    Ok(())
}

#[tokio::test]
async fn test_check_ha_spike_returns_alert_when_spike() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = dir.path().to_path_buf();
    let memory_dir = ws.join("memory");
    std::fs::create_dir_all(&memory_dir)?;

    let bus = std::sync::Arc::new(MessageBus::new());
    let svc = HeartbeatService::new(Config::default(), ws.clone(), bus);

    // spike_detected = false → None
    let no_spike = r#"{"samples":[],"latest":{"co2":"900"},"spike_detected":false}"#;
    std::fs::write(memory_dir.join("ha-state.json"), no_spike)?;
    assert!(svc.check_ha_spike().is_none());

    // spike_detected = true → Some (アラートテキスト)
    let spike = r#"{"samples":[],"latest":{"co2":"1600"},"spike_detected":true}"#;
    std::fs::write(memory_dir.join("ha-state.json"), spike)?;
    let alert = svc.check_ha_spike();
    assert!(alert.is_some());
    assert!(alert.unwrap().contains("CO2"));
    Ok(())
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
TZ=UTC cargo test -p rustyclaw-gateway test_get_ha_env_context test_check_ha_spike 2>&1 | tail -5
```

期待: `FAILED`

- [ ] **Step 3: `get_ha_env_context()` と `check_ha_spike()` を実装する**

`HeartbeatService` の `run_weather_patrol` の直前に追加:

```rust
/// HA 環境スナップショットの 1 行サマリーを返す。
/// `memory/ha-env-summary.txt` が存在しない場合は `None`。
pub fn get_ha_env_context(&self) -> Option<String> {
    let path = self
        .workspace_path
        .join("memory")
        .join("ha-env-summary.txt");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// CO2 スパイクを検知した場合、緊急アラートプロンプトを返す。
/// `memory/ha-state.json` の `.spike_detected` が `true` の場合のみ `Some`。
pub fn check_ha_spike(&self) -> Option<String> {
    let path = self.workspace_path.join("memory").join("ha-state.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    if !json["spike_detected"].as_bool().unwrap_or(false) {
        return None;
    }
    let co2 = json["latest"]["co2"].as_str().unwrap_or("unknown");
    Some(format!(
        "⚠️ [HA SPIKE ALERT] CO2 レベルが危険域に達しています（{} ppm）。\
         換気を促すなど、ユーザーへ即座に通知してください。HEARTBEAT_OK を返してはいけません。",
        co2
    ))
}
```

- [ ] **Step 4: テストを通す**

```bash
TZ=UTC cargo test -p rustyclaw-gateway test_get_ha_env_context test_check_ha_spike 2>&1 | tail -5
```

期待: `test result: ok. 2 passed`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/heartbeat.rs
git commit -m "feat(heartbeat): add get_ha_env_context() and check_ha_spike() methods

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 4: CronService に 10 分 HA ポーリングループを追加

**Files:**
- Modify: `crates/rustyclaw-gateway/src/cron.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs`（`CronService::new()` 呼び出し箇所）

- [ ] **Step 1: `CronService` に `workspace_path` フィールドを追加する**

`crates/rustyclaw-gateway/src/cron.rs` の `CronService` 構造体:

```rust
pub struct CronService {
    bus: Arc<MessageBus>,
    db_path: std::path::PathBuf,
    workspace_path: std::path::PathBuf,
}

impl CronService {
    pub fn new(
        bus: Arc<MessageBus>,
        db_path: std::path::PathBuf,
        workspace_path: std::path::PathBuf,
    ) -> Self {
        Self {
            bus,
            db_path,
            workspace_path,
        }
    }
```

- [ ] **Step 2: `lib.rs` の `CronService::new()` 呼び出しに `workspace_path` を追加する**

`lib.rs` 該当箇所（`let cron_svc = cron::CronService::new(bus.clone(), db_path);`）を:

```rust
let cron_svc = cron::CronService::new(bus.clone(), db_path, self.workspace_path.clone());
```

- [ ] **Step 3: HA ポーリングループをビルドが通ることを確認する**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep -E "^error" | head -10
```

期待: エラーなし

- [ ] **Step 4: HA ポーリングループを `CronService::start()` に追加する**

`cron.rs` の `start()` メソッド末尾（最後の `tokio::spawn` ブロックの後）に追加:

```rust
// 4. HA snapshot loop (poll_interval_secs ごと、デフォルト 10 分)
let bus_ha = self.bus.clone();
let workspace_ha = self.workspace_path.clone();
tokio::spawn(async move {
    let script_path = workspace_ha
        .join("skills")
        .join("home-assistant-rest-api")
        .join("scripts")
        .join("220_ha_env_snapshot.sh");

    if !script_path.exists() {
        tracing::info!(
            "CronService: HA snapshot script not found at {:?}. HA polling disabled.",
            script_path
        );
        return;
    }

    tracing::info!("CronService: Starting HA snapshot polling (script: {:?})", script_path);

    // 起動直後の即時発火を避けるため 60s 待機してから開始
    tokio::time::sleep(Duration::from_secs(60)).await;

    let mut interval = time::interval(Duration::from_secs(600));

    loop {
        interval.tick().await;
        tracing::debug!("CronService: Running HA snapshot...");
        match tokio::process::Command::new("bash")
            .arg(&script_path)
            .arg("--check-spike")
            .current_dir(&workspace_ha)
            .output()
            .await
        {
            Ok(output) => {
                let summary = String::from_utf8_lossy(&output.stdout);
                let summary_trimmed = summary.trim();
                if !summary_trimmed.is_empty() {
                    tracing::info!("CronService: HA snapshot: {}", summary_trimmed);
                }
                if output.status.code() == Some(2) {
                    // exit 2 = CO2 スパイク検知 → 即時 Heartbeat を発火
                    tracing::warn!("CronService: HA CO2 spike detected! Triggering immediate Heartbeat...");
                    let event = SystemEvent::IncomingMessage {
                        session_id: "cron:heartbeat".to_string(),
                        user_id: "cron".to_string(),
                        channel_id: "cron".to_string(),
                        content: "heartbeat".to_string(),
                        priority: Priority::Background,
                    };
                    let _ = bus_ha.publish(event);
                }
            }
            Err(e) => {
                tracing::warn!("CronService: HA snapshot script error: {}", e);
            }
        }
    }
});
```

`cron.rs` のファイル先頭 `use` 句に `tokio::process` が必要:

```rust
use tokio::process;
```

注: `tokio::process::Command` は `tokio` の `process` feature が有効な場合のみ使用可能。`Cargo.toml` の確認が必要。

- [ ] **Step 5: `Cargo.toml` の tokio features に `process` が含まれているか確認**

```bash
grep -A5 'name = "tokio"' crates/rustyclaw-gateway/Cargo.toml
```

`features` に `"process"` が含まれていなければ追加する。

- [ ] **Step 6: ビルドを通す**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep -E "^error" | head -10
```

期待: エラーなし

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-gateway/src/cron.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(cron): add 10-minute HA snapshot polling with spike-triggered heartbeat

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 5: lib.rs の Heartbeat プロンプトに HA コンテキストを注入

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

対象箇所: `session_id == "cron:heartbeat"` の処理内、`prompt_parts.push(format!(...))` 群。

- [ ] **Step 1: HA context 注入コードを追加する**

`lib.rs` のハートビート処理（`if let Some((digest, is_step5_allowed, weather_alert)) = setup_res`）ブロック内:

`weather_alert` の注入の**直後**、`prompt_parts.push(format!("Recent activity digest:..."))` の**前**に追加:

```rust
// HA 環境コンテキスト注入（ha-env-summary.txt が存在する場合）
let ha_env = heartbeat_svc.get_ha_env_context();
let ha_spike = heartbeat_svc.check_ha_spike();

if let Some(ref ha_line) = ha_env {
    prompt_parts.push(format!("Home Environment: {}", ha_line));
}
if let Some(spike_alert) = ha_spike {
    prompt_parts.push(spike_alert);
}
```

- [ ] **Step 2: ビルドを通す**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep -E "^error" | head -10
```

期待: エラーなし

- [ ] **Step 3: 全テストを通す**

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | tail -10
```

期待: `test result: ok.`

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(gateway): inject HA env context and spike alert into heartbeat prompt

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 6: HEARTBEAT.md と SKILL.md を更新

**Files:**
- Modify: `production/workspace/HEARTBEAT.md`
- Modify: `production/workspace/skills/home-assistant-rest-api/SKILL.md`

- [ ] **Step 1: `HEARTBEAT.md` に HA 環境コンテキストの読み取り指示を追加する**

現在の `## Step 2: Weather alert` セクションを以下に置き換える:

```markdown
## Step 2: Weather & Home Environment alert

### Weather
If the user message contains a weather alert, include a concise notification. Do not fetch weather yourself.

### Home Environment (HA)
If the user message contains a `Home Environment:` line (e.g., `[HA_ENV|HH:MM] [Room: ...°C↑ / ...%] [CO2: ...ppm↑] ...`):
- 室温が **↑** トレンドかつ 30°C 超 → 熱中症リスクとして触れる（夏季のみ）
- CO2 が **↑** トレンドかつ 1000 ppm 超 → 換気を促すワンライナーを添える
- `[HA SPIKE ALERT]` が user message に含まれる場合 → **必ず** Important 扱いで通知。HEARTBEAT_OK を返してはいけない。

HA コンテキストが存在しない場合はこのステップを静かにスキップする。
```

- [ ] **Step 2: `SKILL.md` のスクリプト表に `220_ha_env_snapshot.sh` を追加する**

現在の表:

```markdown
| Script | Purpose | Output Format |
| :--- | :--- | :--- |
| `210_ha_report.sh` | ...
```

に行を追加:

```markdown
| `220_ha_env_snapshot.sh` | **環境スナップショット** (トレンド計算 + state 更新) | 1 行サマリー + `memory/ha-state.json` |
```

また `## 1. Using Standardized Internal Scripts` の ctx_execute 例を追加:

```markdown
```bash
# Example: Take HA environment snapshot (updates memory/ha-state.json)
ctx_execute: language=bash, code="bash workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh"
```
```

- [ ] **Step 3: コミット**

```bash
git add production/workspace/HEARTBEAT.md \
        production/workspace/skills/home-assistant-rest-api/SKILL.md
git commit -m "docs(workspace): integrate HA env context into HEARTBEAT step 2 and SKILL.md

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 7: docs/task.md と docs/specs 更新

**Files:**
- Modify: `docs/task.md`
- Modify: `docs/specs/v0.3/07_extensions.md`

- [ ] **Step 1: `docs/task.md` に Phase 50 エントリを追加する（作業開始時）**

`## 優先課題` セクションに追加:

```markdown
- [ ] **Phase 50-1**: HA snapshot スクリプト (`220_ha_env_snapshot.sh`) 作成
- [ ] **Phase 50-2**: Config + CronService + HeartbeatService 拡張
- [ ] **Phase 50-3**: lib.rs 注入 + HEARTBEAT.md + SKILL.md 更新
```

- [ ] **Step 2: 全 Phase 完了後、`07_extensions.md` §14 のステータスを更新する**

`## 14. HomeAssistant 統合 \`[将来拡張]\`` → `## 14. HomeAssistant 統合 \`[完了済 — v0.4 Phase 50]\``

ヘッダー直下に完了ノートを追加:

```markdown
> **v0.4 対応**: Phase 50 にて `220_ha_env_snapshot.sh`（HA skill スクリプト）+ CronService 10 分ポーリング + HeartbeatService スパイク検知を実装済み。TrendAnalyzer は bash リングバッファ（`memory/ha-state.json`）として実装。以下は設計背景の記録として残す。
```

- [ ] **Step 3: コミット**

```bash
git add docs/task.md docs/specs/v0.3/07_extensions.md
git commit -m "docs(task): add Phase 50 HA integration tasks and update spec status

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Self-Review

### Spec coverage (§14 対照)

| §14 要件 | 対応 Task |
|---|---|
| TrendAnalyzer (6 サンプル、傾き計算) | Task 1 — bash リングバッファで実装 |
| HA エンコーダ 1 行サマリー | Task 1 (スクリプト) + Task 5 (Rust 注入) |
| 10 分間引き (Throttling) | Task 4 (CronService 600 秒インターバル) |
| スパイク検知 → HeartbeatService 緊急フラグ | Task 4 (exit 2 → 即時 heartbeat) |
| Proactive Posts 強制キック | Task 4 (SystemEvent::IncomingMessage 発行) |
| コンテキスト圧縮（通常ターン） | Task 5 (heartbeat_prompt に注入) |

### Placeholder scan: なし

### Type consistency 確認
- `get_ha_env_context()` → `Option<String>`（Task 3 定義、Task 5 使用）✓
- `check_ha_spike()` → `Option<String>`（Task 3 定義、Task 5 使用）✓
- `CronService::new(bus, db_path, workspace_path)` — Task 4 で定義、lib.rs で Task 4 Step 2 に更新 ✓

### 注意事項
- `$HOMEASSISTANT_TOKEN` は Phase 49-2 により vault から Rust プロセス環境変数に注入済み。bash スクリプトはこれを継承する。
- センサー entity_id（`sensor.livingroom_air_temperature` 等）は `__200_ha_common.sh` の `HA_WHITELIST` に基づく。実環境によっては entity_id の調整が必要になる場合がある（Task 1 Step 3 の手動実行で確認する）。
- `tokio::process` feature: `rustyclaw-gateway/Cargo.toml` に `process` feature がない場合は Task 4 Step 5 で追加する。
