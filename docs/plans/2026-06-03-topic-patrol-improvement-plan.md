# Topic Patrol 品質改善 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Topic Patrol の実施精度を改善し、深夜探索・日中配信の分離、Source Routing 拡張、クエリカテゴリ多様化を実現する。

**Architecture:** cron.json の `prompt` フィールドに配信フラグを埋め込むことで Rust 変更不要。SKILL.md に探索モード・配信モードの独立フローを追記。USER.md に sources: を整備。

**Tech Stack:** JSON（cron.json, config）、Markdown（SKILL.md, USER.md）、Bash（既存スクリプト活用）

**背景・調査結果:** `docs/2026-06-02-log-inspection-report.md` 参照。lms-gemma-4-e4b（4B）が SKILL.md の算術・条件分岐を正しく実行できないことが根本原因。方針: モデルに計算させず、Rust または設定側で情報を渡す。

**実施済み（本セッション）:**
- `skills/topic-patrol/scripts/510_prune-findings.sh` — findings.md プルーニング
- `skills/topic-patrol/scripts/511_karakeep-add-bookmark.sh` — KaraKeep 登録（`_ai-patrol` タグ）
- SKILL.md — web_fetch 必須化、Source URL 必須、トピック選択指示、`配信:` フラグ設計
- `patrol/state.json` — `rotationIndex` フィールド削除
- `config.debug.json` / `config.release.json` — `lms-gemma-4-26b` エントリ追加（context_window: 16k, enabled: false）

---

## ファイル変更マップ

| ファイル | 種別 | 内容 |
|---|---|---|
| `production/workspace/cron.json` | 変更 | topic-patrol を explore/deliver の2ジョブに分離 |
| `production/workspace/skills/topic-patrol/SKILL.md` | 変更 | 配信モード独立フロー・Source Routing 拡張・Work-adjacent 追加 |
| `production/workspace/USER.md` | 変更 | Interests に sources: 追記 |

---

## Task 1: cron.json — 探索ジョブ・配信ジョブの分離

**Files:**
- Modify: `production/workspace/cron.json`

**概要:** 現在の `topic-patrol`（interval:360）を廃止し、深夜の探索ジョブと朝の配信ジョブに分離する。`prompt` フィールドに `配信:` フラグを埋め込むだけで Rust 変更は不要。

- [ ] **Step 1: 現在の cron.json を確認**

```bash
cat production/workspace/cron.json | python3 -c "
import sys, json
jobs = json.load(sys.stdin)
for j in jobs:
    print(j['id'], '-', j['trigger'])
"
```

- [ ] **Step 2: topic-patrol エントリを2ジョブに置き換える**

`production/workspace/cron.json` の `topic-patrol` エントリを以下の2件に置き換える：

```json
{
  "id": "topic-patrol-explore",
  "name": "Topic Patrol (Explore / 深夜探索)",
  "enabled": true,
  "trigger": {
    "type": "cron",
    "expression": "02:00"
  },
  "prompt": "Run the topic-patrol skill.\n\n配信: スキップ",
  "channel_id": "1485590981251432560"
},
{
  "id": "topic-patrol-deliver",
  "name": "Topic Patrol (Deliver / 朝の配信)",
  "enabled": true,
  "trigger": {
    "type": "cron",
    "expression": "09:00"
  },
  "prompt": "Run the topic-patrol skill.\n\n配信: 許可",
  "channel_id": "1485590981251432560"
}
```

- [ ] **Step 3: JSON 構文を検証**

```bash
python3 -c "import json; json.load(open('production/workspace/cron.json')); print('OK')"
```

Expected: `OK`

- [ ] **Step 4: ダッシュボードで次回実行時刻を確認**

```bash
curl -s http://localhost:8080/api/schedule | python3 -c "
import sys, json
jobs = json.load(sys.stdin)
for j in jobs:
    if 'patrol' in j.get('id',''):
        import datetime
        t = datetime.datetime.fromtimestamp(j['next_run_epoch'])
        print(j['id'], '->', t.strftime('%Y-%m-%d %H:%M'))
"
```

Expected: `topic-patrol-explore` が翌日 02:00、`topic-patrol-deliver` が翌日 09:00 と表示されること。

- [ ] **Step 5: コミット**

```bash
git add production/workspace/cron.json
git commit -m "feat(patrol): split into explore(02:00) and deliver(09:00) cron jobs"
```

---

## Task 2: SKILL.md — 配信モード（Deliver Mode）の独立フロー追加

**Files:**
- Modify: `production/workspace/skills/topic-patrol/SKILL.md`

**概要:** 現在の SKILL.md は探索フローしか持たない。`配信: 許可` で呼ばれる配信ジョブ（09:00）は「昨夜 deferred した内容を届ける」フローを必要とする。このフローを追加する。

- [ ] **Step 1: 現在の SKILL.md の "What you receive" セクションを確認**

```bash
head -25 production/workspace/skills/topic-patrol/SKILL.md
```

- [ ] **Step 2: "What you receive" セクションに2モードの分岐を追記**

`## What you receive in the user message` セクションを以下に置き換える：

```markdown
## What you receive in the user message

The system provides the following in the user message:

- `配信:` — determines which mode to run:
  - `配信: スキップ` → **探索モード**: explore topics, record as deferred, no Discord output
  - `配信: 許可` → **配信モード**: deliver previously deferred findings to Discord, no new exploration

**Read this value first and choose the correct execution flow below.**
```

- [ ] **Step 3: 配信モード（Deliver Mode）専用フローを Execution Flow に追加**

`## Execution Flow` の先頭（`### Step 1` の前）に以下を挿入する：

```markdown
## Execution Flow

> **Before starting:** Check `配信:` in the user message.
> - `配信: スキップ` → follow **探索モード** (Steps 1–5 below)
> - `配信: 許可` → follow **配信モード** (Deliver Mode below), then skip to Step 5

---

### Deliver Mode（`配信: 許可` のとき）

1. Call `workspace_read` on `patrol/findings.md`.
2. Find all entries with status `deferred (quiet hours)` that have **not** been delivered yet.
3. From these, select the **1–2 most interesting** entries using the "Would I tell a friend?" criteria (Step 3 below).
4. If no deferred entries exist, respond with nothing and go to Step 5.
5. For each selected entry, post to Discord:
   - Write a short, natural explanation of WHY it is interesting.
   - End with the source as a Markdown link: `[タイトル](URL)`
6. Append a `delivered` record to `patrol/findings.md` using `workspace_write` with `mode: append`:
   ```
   - {topic}: {summary} — delivered (from deferred {YYYY-MM-DD})
     Source: {URL}
   ```
7. For each delivered URL, call:
   ```
   run_workspace_script skills/topic-patrol/scripts/511_karakeep-add-bookmark.sh
   args: ["{URL}"]
   ```
8. Go to **Step 5-3** (update state.json). Skip Steps 1–5.

---
```

- [ ] **Step 4: 探索モードであることを Step 1 の冒頭に明記**

`### Step 1: Read prior findings and select topics` の冒頭に以下を追加：

```markdown
### Step 1: Read prior findings and select topics
> _(探索モードのみ。配信モードは上の Deliver Mode セクションを参照)_
```

- [ ] **Step 5: SKILL.md の構文確認（Frontmatter が壊れていないこと）**

```bash
python3 -c "
text = open('production/workspace/skills/topic-patrol/SKILL.md').read()
assert '## Execution Flow' in text
assert 'Deliver Mode' in text
assert '配信: スキップ' in text
assert '配信: 許可' in text
print('OK')
"
```

Expected: `OK`

- [ ] **Step 6: コミット**

```bash
git add production/workspace/skills/topic-patrol/SKILL.md
git commit -m "feat(patrol): add deliver mode flow to SKILL.md"
```

---

## Task 3: SKILL.md — Source Routing 拡張（github: / rss:）

**Files:**
- Modify: `production/workspace/skills/topic-patrol/SKILL.md`

**概要:** GeminiClaw から移植。`github:{owner}/{repo}` と `rss:{url}` のルーティングを追加する。USER.md の `sources:` に指定すると自動的に使われる。

- [ ] **Step 1: Source Routing テーブルを拡張**

現在の Source Routing テーブル（`| Source prefix | Action |`）に2行を追加：

```markdown
| Source prefix | Action |
|---|---|
| _(none)_ | `web_search` → `web_fetch` top URL |
| `HN` | `web_search` with `site:news.ycombinator.com {topic}` |
| `Reddit/{sub}` | `web_search` with `site:reddit.com/r/{sub} {topic}` |
| `github:{owner}/{repo}` | `web_fetch https://github.com/{owner}/{repo}/releases` — scan latest release notes |
| `rss:{url}` | `web_fetch {url}` — parse feed entries and pick the newest item |
| URL | `web_fetch` the URL directly |
```

- [ ] **Step 2: 確認**

```bash
grep -A 10 "Source Routing" production/workspace/skills/topic-patrol/SKILL.md | grep "github\|rss"
```

Expected: 両行が表示されること。

- [ ] **Step 3: コミット**

```bash
git add production/workspace/skills/topic-patrol/SKILL.md
git commit -m "feat(patrol): add github: and rss: source routing to SKILL.md"
```

---

## Task 4: SKILL.md — Work-adjacent クエリの追加

**Files:**
- Modify: `production/workspace/skills/topic-patrol/SKILL.md`

**概要:** GeminiClaw から移植。Interest-driven クエリに加えて Work Context に隣接したクエリを1件追加する。探索の多様性を高める。

- [ ] **Step 1: Step 2 の末尾に Work-adjacent の指示を追加**

`### Step 2: Investigate each topic` の末尾（Source Routing テーブルの後）に以下を追加：

```markdown
#### Work-adjacent Query（任意 — 探索モードのみ）

After investigating the 2 selected topics, add **1 optional query** based on the user's current Work Context (`## Work Context` in `USER.md`):

- Pick a technology or tool mentioned in Work Context that was NOT one of the 2 selected topics.
- Search: `{tool} best practices 2026` or `{tool} tips 2026`.
- Apply the same web_fetch step and filter criteria.
- If Work Context is empty or matches the selected topics, skip this step.
```

- [ ] **Step 2: 確認**

```bash
grep "Work-adjacent" production/workspace/skills/topic-patrol/SKILL.md
```

Expected: 追加した行が表示されること。

- [ ] **Step 3: コミット**

```bash
git add production/workspace/skills/topic-patrol/SKILL.md
git commit -m "feat(patrol): add work-adjacent query category to SKILL.md"
```

---

## Task 5: USER.md — Interests に sources: を追記

**Files:**
- Modify: `production/workspace/USER.md`

**概要:** 各 Interest に適切な `sources:` を追記することで、モデルが適切なソースに絞って検索できるようになる。

- [ ] **Step 1: 現在の Interests セクションを確認**

```bash
grep -A 12 "^## Interests" production/workspace/USER.md
```

- [ ] **Step 2: sources: を追記**

`## Interests` セクションを以下に更新する：

```markdown
## Interests
- AI Agent / Autonomous Agent (Claude, Anthropic, MCP)
  sources: HN
- Cloudflare (Workers AI, AI Gateway, Vectorize, Wrangler)
  sources: https://blog.cloudflare.com
- Obsidian (Knowledge Management, Local REST API)
  sources: Reddit/ObsidianMD
- HomeAssistant
  sources: Reddit/homeassistant
- Karakeep (Self-hosted Bookmarks)
  sources: github:hoarder-app/hoarder
- Terminal tools (Fish shell, Ghostty, Zellij, tmux, yazi)
  sources: Reddit/unixporn
- Latest AI/ML tech (Local LLM, Gemma, Ollama)
  sources: Reddit/LocalLLaMA
- Evolutionary Memory / AI memory systems
  sources: HN
- Self-hosted infrastructure
  sources: Reddit/selfhosted
```

- [ ] **Step 3: 確認**

```bash
grep -A 20 "^## Interests" production/workspace/USER.md | grep "sources:"
```

Expected: 9件の `sources:` 行が表示されること。

- [ ] **Step 4: コミット**

```bash
git add production/workspace/USER.md
git commit -m "feat(patrol): add sources: annotations to USER.md Interests"
```

---

## 動作確認手順

全 Task 完了後、以下で動作を確認する。

- [ ] **探索モードの手動テスト（`--no-agent` なし）**

```bash
# cron.json を一時的に interval:1 に変更するか、dashboard から手動発火
# または直接 Discord に "Run the topic-patrol skill. 配信: スキップ" と送信
```

確認ポイント：
1. `patrol/findings.md` に `deferred (quiet hours)` エントリが追記されること
2. `Source:` 行が存在すること
3. Discord に何も投稿されないこと
4. `patrol/state.json` の `lastRun` が更新されること

- [ ] **配信モードの手動テスト**

Discord に以下を送信：
```
Run the topic-patrol skill.

配信: 許可
```

確認ポイント：
1. findings.md の deferred エントリが Discord に投稿されること
2. `[タイトル](URL)` 形式のリンクが含まれること
3. KaraKeep に `_ai-patrol` タグ付きでブックマークが登録されること
4. findings.md に `delivered` エントリが追記されること

---

## 参考：実施済み項目（本セッション）

- `skills/topic-patrol/scripts/510_prune-findings.sh` — findings.md プルーニング実装済み
- `skills/topic-patrol/scripts/511_karakeep-add-bookmark.sh` — KaraKeep 登録実装済み
- SKILL.md — web_fetch 必須化、Source URL 必須、トピック選択指示（直近セクション除外）実装済み
- `patrol/state.json` — rotationIndex フィールド削除済み
- `config.debug.json` / `config.release.json` — `lms-gemma-4-26b` エントリ追加済み（enabled: false）
- `crates/rustyclaw-agent/src/lib.rs` — `execute_heartbeat` に tool_registry 追加済み
- `crates/rustyclaw-gateway/src/heartbeat.rs` — http-dashboard digest 対応済み
- `production/workspace/HEARTBEAT.md` — Step 3 復活・Step 6 HEARTBEAT_OK 強化済み
