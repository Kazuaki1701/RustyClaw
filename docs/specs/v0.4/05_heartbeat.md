# RustyClaw — Heartbeat システム仕様

> [!NOTE]
> **ステータス**: `[実装済]`
> **バージョン**: v0.3
> **最終更新日**: 2026-06-12（Phase 50: HA 環境コンテキスト注入・ステップ構造更新）
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md)

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

### 10.2 HEARTBEAT.md 6 Step 構造（v0.4 現行）

| Step | 頻度 | 内容 |
|---|---|---|
| Step 1 | 毎回 | `Recent activity digest` レビュー（未完了・エラー・異常検知） |
| Step 2 | 毎回 | 天気アラート + **HA 環境コンテキスト確認**（★ Phase 50 追加）。コンテキストがなければ skip silently |
| Step 3 | 毎回 | Calendar / Email チェック（`ctx_execute` 経由 bash スクリプト、スキル不在なら skip silently） |
| Step 4 | 毎回 | 8h 以上無通信 → Quiet hours（0:00〜4:59）除外で声掛け |
| Step 5 | ローテーション | 未完了タスク・失敗セッション対処・バックグラウンド作業（Topic Patrol は別スケジュール） |
| Step 6 | 毎回 | **Important あり** → Discord 通知 / **Informational 以下** → `HEARTBEAT_OK` のみ |

### 10.3 AGENTS.md における Heartbeat 応答規約

| 重要度 | 処理 | HEARTBEAT_OK? |
|---|---|---|
| **Important**（緊急メール・直近 deadline・障害・費用発生・**HA SPIKE ALERT**） | Discord 通知（2〜5 行、日本語） | No |
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
5. **Heartbeat 実行自体は Lane B**。ただし Important 判定時（緊急アラート・自発投稿）は Lane A で配信する

### 10.6 v0.4 Phase 50 — HA 環境コンテキスト注入（Rust 側）

`HeartbeatService` がプロンプト生成前に以下を fail-open で読み取り、system プロンプトに注入する。

| 注入元ファイル | 読み取りメソッド | 注入内容 |
|---|---|---|
| `memory/ha-env-summary.txt` | `get_ha_env_context()` | `Home Environment: [HA_ENV|HH:MM] [Room: ...°C↑ ...]` |
| `memory/ha-state.json` の `spike_detected: true` | `check_ha_spike()` | `⚠️ [HA SPIKE ALERT] CO2 レベルが危険域に達しています（NNN ppm）。...` |

どちらもファイルが存在しない・読み取り失敗の場合は注入せず、Heartbeat を継続する（fail-open）。  
CO2 スパイクは CronService が exit 2 を検知して `Priority::Normal` の `cron:heartbeat` を発火し、3 時間クールダウンで多重発火を防ぐ。
