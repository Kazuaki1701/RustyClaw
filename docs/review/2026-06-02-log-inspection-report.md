# RustyClaw ログ点検レポート — 2026-06-02

> 作成日: 2026-06-02  
> 調査範囲: `production/logs/rustyclaw.log.2026-06-01`（JST 早朝分）/ `rustyclaw.log.2026-06-02`

---

## 1. ログ全体サマリー

| 区分 | 内容 |
|---|---|
| 🔴 要対応 | Groq 413 頻発・LM Studio memory flush 失敗・CF gemma-4-26b Invalid provider（一時）・hard-trim 急変 |
| 🟠 機能障害 | RPi4 上で `gws not found in PATH`（calendar/gmail 失敗） |
| 🟡 軽微 | MEMORY.md oversized truncate・Discord WebSocket Reset（自動回復） |

---

## 2. 個別問題詳細

### 2-1. Groq 413 Too Large（06:27〜07:49）

- `llama-3.3-70b-versatile`（TPM 12k）・`llama-3.1-8b-instant`（TPM 6k）に対して 33k〜34k tokens のリクエストが連続
- 07:49:14 に `all models failed` → Discord 返答失敗 1件
- 原因: http-dashboard セッションのコンテキスト肥大化。hard-trim で 172→40 に刈り込んでも、システムプロンプト + Skills インジェクトが単体で TPM を超えている

### 2-2. CF `@cf/google/gemma-4-26b-a4b-it` Invalid provider（21:25〜21:31）

- discord purpose primary が 6回連続 400 Bad Request
- 22:17 以降は正常回復 → CF 側の一時障害。コード変更不要

### 2-3. hard-trim が 40→10 に急変（22:17 以降）

- 22:16 の 2 回目再起動前: `171→40 messages`
- 22:16 再起動後の初回: `172→10 messages`（162 件削除）
- CF Gemma-4-26b の `tpm` 未設定が hard-trim のデフォルト値に影響している可能性

### 2-4. LM Studio memory flush 失敗（22:17）

- エラー: `n_keep: 20696 >= n_ctx: 16384`
- memory purpose が lms-gemma-4-e4b（16k context）を使用
- memory flush 用システムプロンプトが 20696 tokens → 16k に収まらない

### 2-5. RPi4 で `gws not found in PATH`（06:49〜07:10）

- calendar/gmail スキルが `gws` コマンド不在で失敗
- Phase 36 item 6（RPi4 実機検証）未完了の影響

---

## 3. 21:30 以降タイムライン詳細

| 時刻 | イベント |
|---|---|
| 21:30 | MEMORY.md oversized truncate → flush 完了。cf-gemma-4-26b Invalid provider → fallback lms-gemma-4-e4b で成功 |
| 21:40/41/46 | HTTP `/reload` が3回連続 |
| 21:41〜43 | weather スキル Activation、CF 26k〜27k tokens で正常動作 |
| 21:47 | hard-trim 171→40、memory flush 完了 |
| 21:54 | Heartbeat → Proactive speak 送信 |
| 22:00 | Vital Check Night → daily-briefing + vitals-coach スキル → CF 正常完了 |
| 22:01:52 | **1回目 SIGTERM（デプロイ）** → 即再起動 |
| 22:16:01 | **2回目 SIGTERM（デプロイ）** → 再起動 |
| 22:17 | hard-trim 190→10（急変）。CF gemma-4-26b 正常動作。memory flush 失敗（n_keep > n_ctx） |
| 22:46 | Heartbeat 正常 |
| 22:52 | session-summary 正常 |

---

## 4. HEARTBEAT 調査

### 4-1. ルール vs 実際の対照表

| ルール | 規定 | 実際 | 判定 |
|---|---|---|---|
| Step 3 calendar/email 実行 | 毎回 | `tool_calls: []`（全件未実行） | ❌ |
| HEARTBEAT_OK のみ応答 | 問題なし時 | 全件で余分テキスト追加 | ❌ |
| 言語：日本語 | 常時 | 08:36、09:06 で英語 | ❌ |
| Quiet hours 遵守 | 0:00〜4:59 | 04:45 で verbose 応答 | △ |
| Step 5 Vocal Greeting 制御 | Rust 側判定 | 正常（全件 `allowed: false`） | ✅ |

### 4-2. 根本原因

**`execute_heartbeat` が `tools: &[]`（ツールなし）で実行されていた。**  
HEARTBEAT.md は Step 3/4/7 で `run_workspace_script` を要求していたが、ツールが利用不可のため：
- モデルが `tool_code` コードブロックを書いて誤魔化す（04:45）
- 実行できないまま架空の結果を報告（hallucination）

**digest が常に `*(No new activity since last heartbeat)*`:**  
`heartbeat.rs:88` の除外フィルタが `http-dashboard.jsonl` をスキップしていた。  
実際のユーザー活動はすべて `http-dashboard` セッションに記録されており、heartbeat に情報が届いていなかった。これが hallucination の直接原因。

### 4-3. ファクト捏造の確認された事例

| 時刻 | 内容 |
|---|---|
| 05:26 | 「カレンダーとメールのチェックを行いました」（tool_calls: []） |
| 21:55 | 「明日は午前9時よりチームミーティングが入っています」（存在しない予定） |
| 22:46 | 「今夜のご予定に変更点はありません」（確認していない） |
| 23:16 | 「緊急性の高い未読メールや直近の予定変更は確認されませんでした」（確認していない） |

### 4-4. 対応済み修正

| 修正内容 | ファイル |
|---|---|
| `execute_heartbeat` に `tool_registry` を追加、max 5 ターンのツールループ実装 | `crates/rustyclaw-agent/src/lib.rs` |
| gateway の呼び出しに `&tool_registry` を渡す | `crates/rustyclaw-gateway/src/lib.rs` |
| HEARTBEAT.md: Step 3（calendar/email）復活 | `production/workspace/HEARTBEAT.md` |
| HEARTBEAT.md: Step 6「HEARTBEAT_OK のみ、追加テキスト禁止」を強化 | `production/workspace/HEARTBEAT.md` |
| `http-dashboard.jsonl` を digest スキャン対象に追加（直近24時間のみ） | `crates/rustyclaw-gateway/src/heartbeat.rs` |

---

## 5. Topic Patrol 調査

### 5-1. 本日の実施記録

| 時刻 | トピック | web_search | web_fetch | state 読取 | findings 読取 |
|---|---|---|---|---|---|
| 07:39 | Local LLM / Gemma / Ollama | 1回 | ❌ | ❌ | ❌ |
| 13:40 | AI Agent 最新動向 | 1回 | ❌ | ✅ | ✅ |
| 19:40 | AI Agent 自律型 | 1回 | ❌ | ❌ | ❌ |

### 5-2. ルール vs 実際の対照表

| ルール | 規定 | 実際 | 判定 |
|---|---|---|---|
| 1回の実行で 2〜3 クエリ | SKILL.md Step 2 | 全件 1クエリのみ | ❌ |
| rotationIndex をインクリメント | SKILL.md Step 5 | 3件すべて 1 のまま | ❌ |
| state.json / findings.md を毎回読む | SKILL.md Step 1 | 07:39、19:40 でスキップ | ❌ |
| 有望結果を web_fetch で深掘り | SKILL.md Step 2 | 全件なし | ❌ |
| リンク URL を付与・検証 | USER.md 設定 | Source/Verification なし | ❌ |
| findings.md に全件記録 | SKILL.md Step 5 | 書き込み自体は実施 | ✅ |
| state.json 更新 | SKILL.md Step 5 | 書き込み実施 | ✅ |
| Quiet hours (23:00〜08:00) は deferred | SKILL.md Step 4 | 07:39（quiet hours 内）で Discord 送信 | ❌ |

### 5-3. 最重要ギャップ

**rotationIndex が固定（常に 1）：**  
Interests は9件あるが、毎回「AI Agent」か「Local LLM」しかパトロールしていない。Cloudflare・Obsidian・HomeAssistant・Karakeep・Terminal tools・Self-hosted infrastructure が永続的にスキップされている。

**web_fetch・リンク検証なし：**  
過去（2026-05 以前）の findings には Source/Verification が記録されていたが、最近は欠落。USER.md の「送信前に必ず web_fetch で確認」が守られていない。

**Quiet hours 判定なし：**  
07:39 は quiet hours（23:00〜08:00）内だが Discord 通知を送信。本来は `deferred (quiet hours)` として findings に記録し無言終了すべきだった。

### 5-4. 対応候補

| 対応 | 優先度 |
|---|---|
| SKILL.md に rotationIndex の計算例を明示（`(currentIndex + 1) % len(Interests)`） | 🔴 |
| SKILL.md に「2〜3 クエリ必須」「各クエリは異なる Interest から」を強調 | 🔴 |
| SKILL.md に「リンクは web_fetch で検証してから findings に記録」を明示 | 🔴 |
| Quiet hours ガードを Rust 側 cron で制御（SKILL.md 判断に依存しない） | 🟡 |

---

## 6. 未対応の残課題

| 課題 | 関連 Phase |
|---|---|
| RPi4 で `gws` が PATH に入っていない（calendar/gmail 動作不可） | Phase 36 item 6 |
| hard-trim が 22:16 再起動後に 40→10 に変化した原因調査 | — |
| memory flush 用プロンプトが LM Studio 16k を超過 | — |
| Topic Patrol SKILL.md の rotationIndex / クエリ数 / リンク検証 強化 | — |
