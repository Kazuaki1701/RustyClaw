# Phase 34: session-logs Skill 向け分析スクリプト実装計画

> **ステータス**: `[PLAN]` 実装待ち
> **作成日**: 2026-05-31
> **関連タスク**: `docs/task.md` — Phase 34

---

## 背景と目的

`session-logs` skill（`production/workspace/skills/session-logs.md`）は、セッション履歴の検索・分析に `jq`・`rg`・`run_shell_command` を想定しているが、RustyClaw では `run_workspace_script`（`workspace/scripts/` 内の既存スクリプト実行のみ）しかない。

本 Phase では、LLM が `run_workspace_script` で呼び出せる分析スクリプト 2 本を `production/workspace/scripts/` に追加し、`session-logs` skill の核機能を有効化する。

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `production/workspace/scripts/session-stats.sh` | **新規** | セッション一覧・メッセージ数・SQLite トークン集計 |
| `production/workspace/scripts/session-search.sh` | **新規** | セッション JSONL を keyword で grep 検索 |

---

## SQLite スキーマ（参考）

`memory.db` の `usage` テーブル：

```sql
CREATE TABLE usage (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id   TEXT NOT NULL,
  prompt_tokens     INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens      INTEGER NOT NULL DEFAULT 0,
  model        TEXT,
  trigger_type TEXT,
  duration_ms  INTEGER,
  created_at   TEXT
);
```

---

## セッション JSONL 構造（参考）

`sessions/<id>.jsonl` の各行：

```json
{"role": "user",      "content": "..."}
{"role": "assistant", "content": "..."}
```

セッション ID 命名規則：`discord-C{channelId}-{YYYYMMDD}`、`cron-{jobId}`、`cli-session`、`http-dashboard`

---

## Task 1: `session-stats.sh`

**目的**: セッション一覧・メッセージ数と、SQLite からのトークン使用量集計を出力する。

**ファイル**: `production/workspace/scripts/session-stats.sh`

```bash
#!/bin/bash
# session-stats.sh [--date YYYY-MM-DD] [--days N]
# Show session list, message counts, and token usage from memory.db
#
# Usage:
#   session-stats.sh               # all sessions + last 14 days token stats
#   session-stats.sh --date 2026-05-31  # filter by date
#   session-stats.sh --days 7      # last 7 days token stats

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SESSIONS_DIR="$WORKSPACE_DIR/sessions"
DB="$WORKSPACE_DIR/memory.db"

DATE_FILTER=""
DAYS=14

while [[ $# -gt 0 ]]; do
  case "$1" in
    --date) DATE_FILTER="$2"; shift 2 ;;
    --days) DAYS="$2";        shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

echo "=== Session Files ==="
for f in "$SESSIONS_DIR"/*.jsonl; do
  [ -f "$f" ] || continue
  session=$(basename "$f" .jsonl)
  if [ -n "$DATE_FILTER" ] && [[ "$session" != *"$DATE_FILTER"* ]]; then
    continue
  fi
  count=$(wc -l < "$f")
  size=$(du -h "$f" | cut -f1)
  echo "  $session  ${count} msgs  $size"
done

echo ""
echo "=== Token Usage (last ${DAYS} days) ==="
if command -v sqlite3 >/dev/null 2>&1; then
  sqlite3 "$DB" "
    SELECT
      date(created_at)             AS date,
      COUNT(*)                     AS runs,
      SUM(total_tokens)            AS total_tokens,
      SUM(prompt_tokens)           AS input,
      SUM(completion_tokens)       AS output
    FROM usage
    WHERE date(created_at) >= date('now', '-${DAYS} days')
    GROUP BY date(created_at)
    ORDER BY date DESC;
  " | column -t -s '|'

  echo ""
  echo "=== By Model (last ${DAYS} days) ==="
  sqlite3 "$DB" "
    SELECT
      model,
      COUNT(*)          AS runs,
      SUM(total_tokens) AS tokens
    FROM usage
    WHERE date(created_at) >= date('now', '-${DAYS} days')
    GROUP BY model
    ORDER BY tokens DESC;
  " | column -t -s '|'

  echo ""
  echo "=== By Trigger Type (last ${DAYS} days) ==="
  sqlite3 "$DB" "
    SELECT
      COALESCE(trigger_type, 'unknown') AS type,
      COUNT(*)                          AS runs,
      SUM(total_tokens)                 AS tokens
    FROM usage
    WHERE date(created_at) >= date('now', '-${DAYS} days')
    GROUP BY trigger_type
    ORDER BY tokens DESC;
  " | column -t -s '|'
else
  echo "sqlite3 not available. Install with: sudo apt-get install sqlite3"
  echo "(Token stats require sqlite3 on rp1)"
fi
```

**実行例（LLM から）:**
```
run_workspace_script: { "script_name": "session-stats.sh" }
run_workspace_script: { "script_name": "session-stats.sh", "args": ["--date", "2026-05-31"] }
run_workspace_script: { "script_name": "session-stats.sh", "args": ["--days", "7"] }
```

---

## Task 2: `session-search.sh`

**目的**: セッション JSONL ファイルの `content` フィールドをキーワード検索し、マッチした会話断片を返す。

**ファイル**: `production/workspace/scripts/session-search.sh`

```bash
#!/bin/bash
# session-search.sh <keyword> [--date YYYY-MM-DD] [--context N]
# Search session JSONL files for keyword in message content
#
# Usage:
#   session-search.sh "Garmin"
#   session-search.sh "memory flush" --date 2026-05-31
#   session-search.sh "cron" --context 3

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SESSIONS_DIR="$WORKSPACE_DIR/sessions"

KEYWORD="${1:-}"
DATE_FILTER=""
CONTEXT_LINES=1

if [ -z "$KEYWORD" ]; then
  echo "Usage: session-search.sh <keyword> [--date YYYY-MM-DD] [--context N]"
  exit 1
fi
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --date)    DATE_FILTER="$2"; shift 2 ;;
    --context) CONTEXT_LINES="$2"; shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

echo "=== Searching: \"$KEYWORD\" ==="
FOUND=0

for f in "$SESSIONS_DIR"/*.jsonl; do
  [ -f "$f" ] || continue
  session=$(basename "$f" .jsonl)

  if [ -n "$DATE_FILTER" ] && [[ "$session" != *"$DATE_FILTER"* ]]; then
    continue
  fi

  # Count matches (case-insensitive)
  matches=$(grep -ic "$KEYWORD" "$f" 2>/dev/null || echo 0)
  [ "$matches" -eq 0 ] && continue

  echo ""
  echo "--- $session  ($matches match(es)) ---"
  FOUND=$((FOUND + 1))

  # Print matching lines with role prefix, truncated
  grep -in "$KEYWORD" "$f" | head -20 | while IFS=: read -r linenum line; do
    role=$(echo "$line" | grep -o '"role":"[^"]*"' | head -1 | cut -d'"' -f4)
    content=$(echo "$line" | grep -o '"content":"[^"]*"' | head -1 | cut -d'"' -f4)
    # Truncate long content
    if [ ${#content} -gt 200 ]; then
      content="${content:0:200}..."
    fi
    echo "  L${linenum} [${role:-?}]: ${content}"
  done
done

echo ""
if [ "$FOUND" -eq 0 ]; then
  echo "No matches found for \"$KEYWORD\"."
else
  echo "Found in $FOUND session(s)."
fi
```

**実行例（LLM から）:**
```
run_workspace_script: { "script_name": "session-search.sh", "args": ["Garmin"] }
run_workspace_script: { "script_name": "session-search.sh", "args": ["memory flush", "--date", "2026-05-31"] }
```

---

## Task 3: DoD — `docs/specs/09_geminiclaw_feature_comparison.md` の更新

session-logs skill の状態を ⚠️ → ✅ に更新する（`11_skills_spec.md` との整合確認）。

---

## テスト手順

スクリプト作成後、`--no-agent` 環境で動作確認：

```bash
# 開発機での直接実行テスト
bash production/workspace/scripts/session-stats.sh
bash production/workspace/scripts/session-stats.sh --date 2026-05-31
bash production/workspace/scripts/session-search.sh "heartbeat"
bash production/workspace/scripts/session-search.sh "memory" --date 2026-05-31

# rp1 では sqlite3 が利用可能のため、token stats も確認
ssh rp1 'bash ~/.rustyclaw/scripts/session-stats.sh --days 7'
```

---

## 注意事項

- `column -t -s '|'` は SQLite の `|` 区切り出力を整形する。`column` コマンドは rp1 の Raspberry Pi OS に標準で利用可能。
- `grep -i` は大文字小文字を区別しない検索。日本語は grep で問題なく動作する（UTF-8）。
- `memory_search`（tantivy BM25）はサマリーファイルのみ対象。本スクリプトは**セッション本文（JSONL）**を対象とするため役割が異なる。
