---
name: home-assistant-rest-api
description: >
  Direct operation of Home Assistant using REST API (curl) and optimized Agnostic monitoring scripts (200-series).
  
  TRIGGER THIS SKILL WHEN:
  - Reading or writing entity states directly in Home Assistant
  - Requesting health checks, summary reports, or detailed sensor logs
  - Interacting with devices (locks, lights, climate) via REST API services
  - Proactively monitoring for stale sensors or low battery
---

# Home Assistant Operations (PicoClaw Edition)

**CRITICAL INSTRUCTION**: 
Do NOT try to call a tool named `home_assistant_rest_api`. You MUST use `run_command` with either `curl` or the specialized **Agnostic scripts** provided in the library.

## 1. Using Standardized Internal Scripts (RECOMMENDED)

`ha-control.sh <subcommand>` で全 HA 操作を統一呼び出し。

| Subcommand | Purpose |
| :--- | :--- |
| `report` | 統合レポート（Logs + Health + Summary）**← 通常はこれ** |
| `health` | バッテリー / Offline / Stale センサー確認 |
| `summary` | 主要センサー状態サマリー |
| `logs` | エラーログ取得（凝縮済み） |
| `all_states` | 全エンティティ一括取得（600+） |
| `env_snapshot` | 環境スナップショット（`memory/ha-state.json` 更新） |

```bash
# 統合レポート取得
skills/home-assistant-rest-api/scripts/ha-control.sh report --agent

# ヘルスチェックのみ
skills/home-assistant-rest-api/scripts/ha-control.sh health
```

## 2. Direct REST API Access (curl)

特定のエンティティへの個別操作や、スクリプトでカバーされていない情報の取得に使用します。

- **Endpoint**: `http://192.168.1.30:8123/api/`
- **Auth**: `Authorization: Bearer $HOMEASSISTANT_TOKEN`

### A. Get Specific State (SINGLE ENTITY)

```bash
curl -s -X GET \
  -H "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
  http://192.168.1.30:8123/api/states/sensor.livingroom_air_temperature
```

### B. Call Services (POST)

```bash
# Turn OFF a device
curl -s -X POST \
  -H "Authorization: Bearer $HOMEASSISTANT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"entity_id": "light.living_room"}' \
  http://192.168.1.30:8123/api/services/light/turn_off
```

## 3. Data Slimming Strategy (jq)

大規模なデータを扱う際は、必ず `jq` で不必要な属性（Attributes）を削ぎ落としてください。

```bash
# Get all states in 1 line per entity format (Pattern from SKILL.md)
curl -s -H "Authorization: Bearer $HOMEASSISTANT_TOKEN" http://192.168.1.30:8123/api/states | \
    jq -r '.[] | "\(.attributes.friendly_name // .entity_id): \(.state)"'
```

```bash
# Fetch exposed entities (WebSocket)
uv run --with websockets skills/home-assistant-rest-api/scripts/get_exposed_entities.py
```

## 4. Security & Best Practices

- **Token Safety**: トークンは直接記述せず、必ず環境変数 `$HOMEASSISTANT_TOKEN` を参照してください。
- **Rate Limiting**: 大量のセンサー情報を短時間に繰り返し取得しないように注意してください。
- **Entity Identification**: `device_id` ではなく `entity_id` を使用して操作を指示してください。
