 > [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)
> **最終更新日**: 2026-06-06
> **対象コード**: `crates/rustyclaw-gateway/src/skills.rs`、`workspace/skills/`

# 11. Skills システム仕様 — GeminiClaw 比較・移植記録

---

## 1. Skills システム概要

### ロードエンジン

| 項目 | GeminiClaw | RustyClaw |
|---|---|---|
| 実装ファイル | Gemini CLI 組み込み（`@filename` 自動インポート） | `crates/rustyclaw-gateway/src/skills.rs` |
| 呼び出し箇所 | `GEMINI.md` 経由で cron 実行前に自動注入 | `lib.rs:488` — セッションの実行（プロンプト構築）直前に `inject_skill_content()` を呼び出し |
| マッチング方式 | `skills/<name>/SKILL.md` を `@<name>` で参照 | Discovery（末尾に全スキル名/スクリプトを自動付加）と Activation（プロンプト本文に `use-skill: <name>`, `skill:<name>`, または単に `<name>` が含まれていれば instructions 本文を前置注入）の二段階開示 |
| ファイル構造 | `workspace/.gemini/skills/<name>/SKILL.md`（サブディレクトリ形式） | `workspace/skills/<name>/SKILL.md` (標準ディレクトリ形式。YAMLフロントマター検証有) および `workspace/skills/<name>.md` (従来互換フラット形式) を両方サポート |
| 参照ドキュメント | `references/` サブディレクトリで複数ファイル対応 | `rewrite_relative_links` により `skills/<name>/...` への相対リンク自動書き換えによる複数ファイル参照に対応 |

### 注入ロジック（`inject_skill_content`）

1. **Discovery (段階的開示レベル1)**:
   ロードされたすべてのスキル名、説明、および `skills/<name>/scripts/*.sh` の一覧から `## Available Skills` を自動生成し、プロンプトの末尾に付与。LLMには `run_workspace_script` を通じて実行可能なシェルスクリプトのフルパス（例: `skills/session-logs/scripts/session-stats.sh`）を提示する。
2. **Activation (動的ロードレベル2)**:
   ユーザープロンプト本文（小文字化されたもの）に、`use-skill: <name>`, `skill:<name>`, または単に `<name>` が含まれている場合、該当するスキルの Markdown 本文（`SKILL.md` の instructions またはフラットな md ファイル。かつ相対リンクは `skills/<name>/` 配下を指すように自動書き換えされたもの）をプロンプトの先頭にマージして注入。

---

## 2. Skills 一覧・移植状況

### 凡例

| 記号 | 意味 |
|---|---|
| ✅ | 実装済み・正常動作 |
| ⚠️ | 部分実装 / 動作不完全 |
| ❌ | 未実装 |
| N/A | 設計上不要（意図的に除外） |

### 2-1. 移植済み Skills（`production/workspace/skills/` に存在）

| Skill | GeminiClaw | RustyClaw | 主な適合変更 |
|---|---|---|---|
| **topic-patrol** | ✅ | ✅ | `geminiclaw_post_message` → 自動配信。`geminiclaw_status` → `[now:]`。Phase 21 で移植済み |
| **daily-briefing** | ✅ | ✅ | カレンダー取得 (`calendar-ops.sh`)、Gmail取得 (`506_get-gmail.sh`)、天気検索 (`504_get-weather.sh`)。あゆみ様・ゆうき様カレンダー追記 |
| **vitals-coach** | ✅ | ✅ | スクリプトパス絶対指定 → `run_workspace_script("500_get-vital-data-garmin.sh")`。配信 → 自動 |
| **deep-research** | ✅ | ✅ | `agent-browser` → `web_fetch`（フォールバック）。保存 → `workspace_write`。`geminiclaw_status` 削除 |
| **todo-tracker** | ✅ | ✅ | `geminiclaw_status` → `[now:]` 参照。`workspace_read`/`workspace_write` そのまま使用 |
| **coding-plan** | ✅ | ✅ | `skills/todo-tracker/SKILL.md` 参照 → `todo-tracker skill` に変更。それ以外はそのままコピー |
| **workspace** | ✅ | ✅ | `.gemini/skills/*/SKILL.md` → `skills/*.md`。`cron/jobs.json` → `cron.json` |
| **session-logs** | ✅ | ✅ | `run_shell_command`/`jq`/`rg` → `memory_search` + `workspace_read` + `run_workspace_script` 用の分析スクリプト（`session-stats.sh`, `session-search.sh`）経由に書き換え。 |

### 2-2. 移植対象外 Skills

| Skill | GeminiClaw | RustyClaw | 除外理由 |
|---|---|---|---|
| **agent-browser** | ✅ | ❌ | `npx agent-browser:*` コマンドに依存。Playwright ブラウザ自動化ツールが未実装 |
| **pdf** | ✅ | ❌ | `pdftotext`・`tesseract` バイナリに依存 |
| **self-manage** | ✅ | N/A | `geminiclaw_admin` MCP 依存。RustyClaw のデーモン管理は systemd で代替 |
| **translate-preview** | ✅ | ❌ | 複雑な HTML 生成・DOM 操作に依存 |
| **gog** | ✅ | N/A | Google Workspace ツールのリファレンス skill。RustyClaw は `gws_*` ネイティブ実装済みのため不要 |
| **cron** | ✅ | N/A | `geminiclaw_cron_*` MCP 依存。RustyClaw は `CronScheduleTool` + `cron.json` で代替 |
| **obsidian-rest-api** | ✅ | N/A | `curl`/`jq` で Obsidian REST API を直呼び。RustyClaw は `obsidian_*` ネイティブ実装済みのため不要 |

---

## 3. ツール名マッピング（GeminiClaw → RustyClaw）

| GeminiClaw ツール | RustyClaw ツール | 備考 |
|---|---|---|
| `gog_calendar_events` | `gws_calendar_list_events` | `calendar_id`, `time_min`, `max_results` |
| `gog_gmail_search` | `gws_gmail_list_messages` | `query`, `max_results` |
| `gog_calendar_create` | `gws_writable_calendar_insert` | 許可カレンダーのみ |
| `gog_gmail_trash` | `gws_gmail_trash_message` | `_ai-agent` ラベル必須ガード付き |
| `web_search` | `web_search` | 同一（Brave Search） |
| `web_fetch` | `web_fetch` | 同一 |
| `geminiclaw_post_message` | N/A | レスポンステキストを自動配信するため不要 |
| `geminiclaw_status` | N/A | `[now: YYYY-MM-DDTHH:MM:SS+TZ]` がシステムプロンプト先頭に自動注入されるため不要 |
| `run_shell_command` | `run_workspace_script` | `scripts/` 内スクリプトのみ実行可能（インラインコマンド不可） |

---

## 4. 実装済みの session-logs 向けスクリプト

`session-logs` スキルは、以下のスクリプトが `skills/session-logs/scripts/` に配置され、`run_workspace_script` を経由して呼び出し可能になっています：

| スクリプト | 役割 |
|---|---|
| `skills/session-logs/scripts/session-stats.sh` | 直近セッション一覧・メッセージ数。SQLite `memory.db` から日次トークン使用量集計 |
| `skills/session-logs/scripts/session-search.sh` | `sessions/*.jsonl` の `content` フィールドを引数キーワードで grep 検索 |

### session JSONL 構造の差異

| 項目 | GeminiClaw | RustyClaw |
|---|---|---|
| フォーマット | `{runId, timestamp, trigger, responseText, toolCalls, tokens}` (構造化) | `{"role":"...", "content":"..."}` (メッセージペア) |
| トークン追跡 | JSONL の `tokens` フィールド | SQLite `memory.db` に別途記録 |
| ツール呼び出し履歴 | JSONL の `toolCalls` 配列 | SQLite に記録 |
| セッションパス | `memory/sessions/<id>.jsonl` | `sessions/<id>.jsonl` |

### 将来対応候補

| Skill | 対応条件 |
|---|---|
| `agent-browser` | Playwright / headless browser ツールの実装後 |
| `pdf` | `pdftotext` ラッパーツールの実装後 |

---

## 5. cron.json との連携

cron.json の `prompt` フィールドにスキル名が含まれていると、`inject_skill_content()` がマッチして対応する `.md` を前置注入する。

```json
// cron.json 例
{ "id": "daily-briefing", "prompt": "Run the daily-briefing skill." }
// → skills/daily-briefing.md の内容がプロンプト先頭に注入される

{ "id": "topic-patrol", "prompt": "Run the topic-patrol skill." }
// → skills/topic-patrol.md の内容がプロンプト先頭に注入される
```

cron.json のプロンプトは手順の詳細を持たず、スキル名だけを含む短い形式にすることを推奨する。手順・定義はすべて `skills/*.md` に記載する。
