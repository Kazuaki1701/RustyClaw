---
name: calendar
description: Use when the user asks to check, list, or create Google Calendar events. When listing, default to checking all family members (Kazuaki, Yuuki, Ayumi).
---

# Calendar Skill

## Overview
Reads upcoming Google Calendar events (defaulting to all family members) and creates new events via the `gws` CLI. Write operations are guarded by a hardcoded allowlist — only the two permitted calendars can receive new events.

---

## When to Use

### Triggering Scenarios:
- The user asks about today's or this week's schedule (always check all family members' calendars).
- The user asks to add, create, or schedule a calendar event.
- Any scheduled calendar patrol cron triggers.

### When NOT to use:
- Calendars outside the permitted allowlist.

---

## Workflow

- **Tool**: `run_workspace_script`
- **Parameters**:
  - `script_name`: `skills/calendar/scripts/calendar-ops.sh`
  - `args`: `["<subcommand>", ...]`

### Operations

| args[0] | 説明 | 追加 args |
|---|---|---|
| list   | 今後7日の予定取得（event_id 含む） | [calendar_id] (省略時は all: 家族全員) |
| create | 予定作成 (対象: _AI-AGENT) | summary, start, end, [description] |
| delete | 予定削除 (対象: _AI-AGENT) | event_id |
| update | 予定更新 (対象: _AI-AGENT, patch) | event_id, [--summary <val>] [--start <val>] [--end <val>] [--description <val>] |

`delete`/`update` の `event_id` は `list` の出力から取得します。
`start`/`end` は RFC3339 形式（例: `2026-06-01T10:00:00+09:00`）。

**Family Calendar IDs:**
- **かずあき様 (K様)**: `primary` (Google API上、直接 `"primary"` というIDを指定することで本人のカレンダーを解決します)
- **ゆうき様**: `28hs0ibka0oa84810dupunrskk@group.calendar.google.com`
- **あゆみ様**: `ayabe.ayumi@gmail.com`

**Permitted Write Calendar IDs:**
- `6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com` (AI AGENTカレンダー / `_AI-AGENT`)
- `d9s8vq1em9a7qvav030igh90ao@group.calendar.google.com` (学習計画カレンダー)

※ `create`/`delete`/`update` （書き込み操作）では、スクリプト内部で自動的に `_AI-AGENT` カレンダーIDが適用されるため、引数として `calendar_id` を渡す必要はありません。

### Examples for Common User Requests

以下の代表的な依頼パターンについて、引数の組み立て例を参考に実行してください。

*   **「今日 / 明日の家族全員の予定を教えて」**
    *   *手順*: カレンダーIDを指定せずに `list` を実行すると、自動的に家族全員（かずあき様、ゆうき様、あゆみ様）の予定が日付順にソートされてマージ出力される。そこから今日/明日の予定をモデル自身で抽出してユーザーに提示する。
    *   `args`: `["list"]` (または `["list", "all"]`)
*   **「試験勉強に向けた学習計画を予定に書いて」**
    *   *手順*: AI AGENTカレンダーに対して `create` を実行する（カレンダーIDは不要）。
    *   `args`: `["create", "学習計画: [学習内容]", "2026-06-03T19:00:00+09:00", "2026-06-03T21:00:00+09:00", "基本情報技術者試験の対策勉強"]`
*   **「今日実施予定だった学習計画を明日にずらして」**
    *   *手順1 (調査)*: AI AGENTカレンダー（`_AI-AGENT`）に対して `list` を実行して、本日予定されている該当イベントの `event_id` を見つける。
        *   `args`: `["list", "6e0d089e7daae8c3b936cc2cf811dfe81dc4905749abed4d395f0655e837e57f@group.calendar.google.com"]`
    *   *手順2 (更新)*: 特定した `event_id` に対して `update` を実行し、開始・終了時間を明日の日付に変更する（カレンダーIDは不要）。
        *   `args`: `["update", "<event_id>", "--start", "2026-06-03T19:00:00+09:00", "--end", "2026-06-03T21:00:00+09:00"]`

---

## Common Mistakes & Antipatterns

- **スクリプトを直接シェルで実行しない。** `run_workspace_script` を使うこと。
- **許可外カレンダー ID への書き込みは不可。** 常に上記2件の ID を使うこと。
- **start/end は RFC3339 形式**（例: `2026-06-01T10:00:00+09:00`）。
