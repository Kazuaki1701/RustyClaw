 > [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)
> **最終更新日**: 2026-05-31
> **対象コード**: `crates/rustyclaw-gateway/src/skills.rs`、`production/workspace/skills/`

# 11. Skills システム仕様 — GeminiClaw 比較・移植記録

---

## 1. Skills システム概要

### ロードエンジン

| 項目 | GeminiClaw | RustyClaw |
|---|---|---|
| 実装ファイル | Gemini CLI 組み込み（`@filename` 自動インポート） | `crates/rustyclaw-gateway/src/skills.rs` |
| 呼び出し箇所 | `GEMINI.md` 経由で cron 実行前に自動注入 | `lib.rs:530` — cron dispatch 前に `inject_skill_content()` を呼び出し |
| マッチング方式 | `skills/<name>/SKILL.md` を `@<name>` で参照 | プロンプト本文に skill 名（ファイル名）が含まれていれば前置注入 |
| ファイル構造 | `workspace/.gemini/skills/<name>/SKILL.md`（サブディレクトリ形式） | `workspace/skills/<name>.md`（フラット形式） |
| 参照ドキュメント | `references/` サブディレクトリで複数ファイル対応 | 単一 `.md` ファイルのみ（サブディレクトリ未対応） |

### 注入ロジック（`inject_skill_content`）

```rust
// crates/rustyclaw-gateway/src/skills.rs
pub fn inject_skill_content(workspace_path: &Path, content: &str) -> String {
    // 1. workspace/skills/ を読み込む
    // 2. プロンプト content を lowercase 化
    // 3. skill ファイル名（拡張子なし）が content に含まれていれば
    //    skill_md を content の前に前置して返す
}
```

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
| **daily-briefing** | ✅ | ✅ | `gog_calendar_events` → `gws_calendar_list_events`。`gog_gmail_search` → `gws_gmail_list_messages`。天気検索 → `yolp_weather`。あゆみ様・ゆうき様カレンダー追記 |
| **vitals-coach** | ✅ | ✅ | スクリプトパス絶対指定 → `run_workspace_script("500_get-vital-data-garmin.sh")`。配信 → 自動 |
| **deep-research** | ✅ | ✅ | `agent-browser` → `web_fetch`（フォールバック）。保存 → `workspace_write`。`geminiclaw_status` 削除 |
| **todo-tracker** | ✅ | ✅ | `geminiclaw_status` → `[now:]` 参照。`workspace_read`/`workspace_write` そのまま使用 |
| **coding-plan** | ✅ | ✅ | `skills/todo-tracker/SKILL.md` 参照 → `todo-tracker skill` に変更。それ以外はそのままコピー |
| **workspace** | ✅ | ✅ | `.gemini/skills/*/SKILL.md` → `skills/*.md`。`cron/jobs.json` → `cron.json` |
| **session-logs** | ✅ | ⚠️ | `run_shell_command`/`jq`/`rg` → `memory_search` + `workspace_read` + スクリプト経由に大幅書き換え。分析スクリプト（`session-stats.sh`・`session-search.sh`）が未作成のため高度クエリ不可（Phase 33） |

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

## 4. 未実装・残課題

### Phase 33: session-logs 向け分析スクリプト（🔴 優先）

session-logs skill は以下のスクリプトが `scripts/` に存在することを前提とする：

| スクリプト | 役割 |
|---|---|
| `scripts/session-stats.sh` | 直近セッション一覧・メッセージ数。SQLite `memory.db` から日次トークン使用量集計 |
| `scripts/session-search.sh` | `sessions/*.jsonl` の `content` フィールドを引数キーワードで grep 検索 |

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
