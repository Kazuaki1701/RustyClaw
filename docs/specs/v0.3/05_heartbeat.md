# RustyClaw — Heartbeat システム仕様

> [!NOTE]
> **ステータス**: `[実装済]`
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw_hermes_featured.md`](00_rustyclaw_hermes_featured.md)

---

## 10. Heartbeat システム `[実装済]`

### 10.1 ファイルの役割分担

| ファイル / DB | 内容 | 変更者 |
|---|---|---|
| `HEARTBEAT.md` | Heartbeat 振る舞い指示（静的） | ユーザーのみ・**自己改変禁止** |
| `USER.md ## Interests` | 興味領域の定義（準静的） | ユーザー or エージェント |
| `memory/heartbeat-state.json` | 各チェックの最終実行時刻 | エージェント自己更新 |
| SQLite `patrol_state` | heartbeat-state.json の Rust 側管理 | システム自動 |
| SQLite `seen_items` | Interest Patrol 既読管理 | システム自動 |

### 10.2 HEARTBEAT.md 7 Step 構造

| Step | 頻度 | 内容 |
|---|---|---|
| Step 1 | 毎回 | heartbeat-digest.md + summaries/ + logs/ で活動レビュー |
| Step 2 | 数時間ごと | MEMORY.md 整理・USER.md Interests 更新 |
| Step 3 | 毎回 | Calendar / Email チェック（ツールがなければ skip silently） |
| Step 4 | 1 日 2〜3 回 | 天気チェック（4 時間インターバル、なければ skip silently） |
| Step 5 | 毎回 | 8h 以上無通信 → 昼間のみ軽く声掛け（Quiet hours 23:00〜08:00 除外） |
| Step 6 | ローテーション | 未完了タスク・失敗セッション対処・バックグラウンド作業 |
| Step 7 | 毎回 | **必ず HEARTBEAT_OK で応答** → 無音 or 通知配信 |

### 10.3 AGENTS.md における Heartbeat 応答規約

| 重要度 | 処理 | HEARTBEAT_OK? |
|---|---|---|
| **Critical**（緊急メール・直近 deadline・障害） | アラートテキストとして通知 | No |
| **Informational**（新着非緊急・定常カレンダー） | `logs/YYYY-MM-DD.md` にのみ記録 | Yes |
| **Nothing**（所見なし） | — | Yes |

### 10.4 heartbeat-digest.md 生成ルール

GeminiClaw `src/agent/session/heartbeat-digest.ts` の移植。

- **通常**: 前回 Heartbeat 以降の JSONL 差分のみスキャン（incremental）
- **6 回に 1 回**: 24 時間 deep scan
- `cron:heartbeat` セッション自身は除外
- 3000 文字以内に圧縮（最新エントリ優先）
- 各エントリを `[HH:MM] session: prompt → response` 形式に圧縮

### 10.5 重要設計ルール

1. **HEARTBEAT_OK で返したら無音**（ユーザーに通知しない）
2. **Heartbeat 実行は `last_user_interaction_at` を更新しない** → 声掛け判断が自分自身を「アクティブ」と誤判定するのを防ぐ
3. **HEARTBEAT.md はエージェントが絶対に自己改変しない**
4. **巡回済み管理は SQLite seen_items が担う**（ファイルに書かない）
5. **Heartbeat 実行自体は Lane B**。ただし HEARTBEAT_OK ではない Critical 判定時（緊急アラート・自発投稿）は Lane A で配信する
