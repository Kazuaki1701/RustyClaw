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

- **Tool**: `ctx_execute`
- **Parameters**:
  - `language`: `bash`
  - `code`: `bash workspace/skills/calendar/scripts/calendar-ops.sh <subcommand> [args...]`
  - ← **サブコマンドは必須。引数なし呼び出しはエラー。**

### Default Action

**予定確認（ユーザーからのリクエスト・パトロール cron トリガー問わず）は必ず `list_family` を使う。**
`code: "bash workspace/skills/calendar/scripts/calendar-ops.sh list_family"`

### Operations

| args[0] | 説明 | 追加 args |
|---|---|---|
| list_family | 家族全員の今後3日の予定取得 | なし |
| list_ai_agent | _AI-AGENT カレンダーの今後3日の予定取得 | なし |
| create_ai_agent | _AI-AGENT への予定作成 | summary, start, end, [description] |
| delete_ai_agent | _AI-AGENT の予定削除 | event_id |
| update_ai_agent | _AI-AGENT の予定更新 (patch) | event_id, [--summary <val>] [--start <val>] [--end <val>] [--description <val>] |

`delete_ai_agent`/`update_ai_agent` の `event_id` は `list_ai_agent` の出力から取得します。
`start`/`end` は RFC3339 形式（例: `2026-06-01T10:00:00+09:00`）。


### Examples for Common User Requests

以下の代表的な依頼パターンについて、引数の組み立て例を参考に実行してください。

*   **パトロール cron トリガー（定期チェック）**
    *   *手順*: `list_family` を実行して家族全員の予定を取得し、注目すべきイベントをサマリーとして出力する。
    *   `code`: `bash workspace/skills/calendar/scripts/calendar-ops.sh list_family`
*   **「今日 / 明日の家族全員の予定を教えて」**
    *   *手順*: `list_family` を実行すると、自動的に家族全員（かずあき様、ゆうき様、あゆみ様）の予定が日付順にソートされてマージ出力される。そこから今日/明日の予定をモデル自身で抽出してユーザーに提示する。
    *   `code`: `bash workspace/skills/calendar/scripts/calendar-ops.sh list_family`
*   **「_AI-AGENTの予定を教えて」 / 「学習計画の登録状況を教えて」**
    *   *手順*: `list_ai_agent` を実行して、AI AGENTカレンダーに登録されている予定を取得する。
    *   `code`: `bash workspace/skills/calendar/scripts/calendar-ops.sh list_ai_agent`
*   **「試験勉強に向けた学習計画を予定に書いて」**
    *   *手順*: `create_ai_agent` を実行して、AI AGENTカレンダーに予定を作成する。
    *   `code`: `bash workspace/skills/calendar/scripts/calendar-ops.sh create_ai_agent "学習計画: [学習内容]" "2026-06-03T19:00:00+09:00" "2026-06-03T21:00:00+09:00" "基本情報技術者試験の対策勉強"`
*   **「今日実施予定だった学習計画を明日にずらして」**
    *   *手順1 (調査)*: `list_ai_agent` を実行して、本日予定されている該当イベントの `event_id` を見つける。
        *   `code`: `bash workspace/skills/calendar/scripts/calendar-ops.sh list_ai_agent`
    *   *手順2 (更新)*: 特定した `event_id` に対して `update_ai_agent` を実行し、開始・終了時間を明日の日付に変更する。
        *   `code`: `bash workspace/skills/calendar/scripts/calendar-ops.sh update_ai_agent "<event_id>" --start "2026-06-03T19:00:00+09:00" --end "2026-06-03T21:00:00+09:00"`

---

## Common Mistakes & Antipatterns

- **`code` にサブコマンドを必ず含める。** 引数なしで呼ぶとスクリプトがエラー終了する。予定確認なら `code: "bash workspace/skills/calendar/scripts/calendar-ops.sh list_family"`。
- **スクリプトを直接シェルで実行しない。** `ctx_execute` を使うこと。
- **書き込み操作（create/delete/update）でカレンダーIDを指定しないこと。** スクリプト内部で自動的に `_AI-AGENT` カレンダーが対象になります。
- **start/end は RFC3339 形式**（例: `2026-06-01T10:00:00+09:00`）。
