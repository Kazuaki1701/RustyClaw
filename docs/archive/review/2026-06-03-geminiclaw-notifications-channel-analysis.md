# GeminiClaw — Notifications チャンネル機能分析と RustyClaw 導入判断

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の調査報告書 - 点検完了済み)  
> **完了日**: 2026-06-03  
> **備考**: 本件は、GeminiClawの完了通知チャンネル（notifications）の仕様分析とRustyClawへの導入評価です。

**調査日:** 2026-06-03  
**調査対象:** `src/config/schema.ts`, `src/agent/turn/finalize.ts`, `src/cli/commands/setup.ts`, `src/config/io.ts`  
**関連文書:** `2026-06-03-geminiclaw-nonok-delivery-analysis.md`（non-OK 配信フロー詳細）

---

## 1. 機能概要

`notifications` チャンネルは `home`（主チャンネル）とは独立した**バックグラウンドジョブ専用の完了通知チャンネル**。

```json
{
  "home": { "channel": "discord", "channelId": "AAA" },
  "notifications": { "channel": "discord", "channelId": "BBB" }
}
```

未設定時は `home` にフォールバック（`finalize.ts:79`）。

---

## 2. GeminiClaw の設定スキーマ（config/schema.ts）

```typescript
/** Primary channel for the agent. Bootstrap greetings, heartbeat and cron results are sent here. */
home: z.object({
    channel: z.enum(['discord', 'slack', 'telegram']),
    channelId: z.string(),
}).optional(),

/** Channel for background job notifications (heartbeat alerts, cron completion). */
notifications: z.object({
    channel: z.enum(['discord', 'slack', 'telegram']),
    channelId: z.string(),
}).optional(),
```

プラットフォームは `home` と `notifications` で**異なっても構わない**（例: home=Discord、notifications=Slack）。

---

## 3. setup wizard での設定フロー（cli/commands/setup.ts:743–833）

`geminiclaw setup notifications` で対話的に設定できる。

```
Job Notifications
─────────────────────────────────────────────────
Background jobs (heartbeat & cron) post brief completion notices here.
Examples:
  ✅ Heartbeat OK
  ⚠️ Heartbeat Alert — calendar conflict detected ...
  ✅ Cron done: daily-briefing
  ⚠️ Cron failed: market-analysis — rate limit exceeded ...

Full results are sent separately to each job's reply channel.
─────────────────────────────────────────────────

? Notification channel for background jobs (heartbeat & cron)?
  ○ Skip (No channel notifications)
  ○ Discord #general
  ● Discord #notifications   ← 専用チャンネルを選択
  ○ Slack #alerts

? Enable desktop notifications for heartbeat alerts? (Y/n)
```

コメントに「Full results are sent separately to each job's reply channel」とある通り、notifications はあくまで**ステータスサマリー**であり、アラートの本文は home（または能動投稿先）に別途届く設計。

---

## 4. 投稿内容（finalize.ts::notifyBackgroundJob）

| 条件 | 投稿テキスト |
|---|---|
| Heartbeat OK | `✅ **Heartbeat OK**` |
| Heartbeat non-OK | `⚠️ **Heartbeat Alert**\n{responseText 先頭500文字}` |
| Cron 成功 | `✅ **Cron done: {jobId}**` |
| Cron 失敗 | `⚠️ **Cron failed: {jobId}**\n{error 先頭300文字}` |

---

## 5. 運用上の意図

```
home チャンネル              notifications チャンネル
──────────────────────      ─────────────────────────────────
・ユーザーとの会話            ・✅ Heartbeat OK  (10:00)
・⚠️ Critical アラート本文    ・✅ Heartbeat OK  (10:30)
・エージェントの返答           ・⚠️ Heartbeat Alert (11:00)
                             ・✅ Cron done: daily-briefing
                             ・✅ Heartbeat OK  (11:30)
```

**home を会話用に保護し、heartbeat の稼働証跡と cron ログを別チャンネルに隔離する**設計。  
特に HEARTBEAT_OK の `✅` 連打が会話 home に流れ込まないようにするのが主目的。

---

## 6. config migration 履歴（config/io.ts）

notifications は設定フォーマットが変遷している。`io.ts` に migration コードが存在：

```typescript
// 1. Old `notifications: { enabled, method }` → deleted
// 2. `heartbeat.notifications.*.{ enabled, channelId }` → notifications: { channel, channelId }
```

初期は heartbeat 設定内にネストされていたが、cron 完了通知にも使うため top-level に昇格した経緯。  
RustyClaw 設計時は最初から top-level に置くべき。

---

## 7. RustyClaw の現状

- `DiscordConfig` に `home_channel_id: Option<String>` のみ
- notifications 相当の設定なし
- heartbeat non-OK 時: `responseText` をそのまま `home_channel_id` に配信
- heartbeat OK 時: 完全無音（`memory/logs/YYYY-MM-DD.md` に記録のみ）

---

## 8. 導入判断

### 現時点（Discord のみ）: 不要

- 単一チャンネルの single-user 運用では分離の価値が発生しない
- HEARTBEAT_OK の稼働証跡は Dashboard から確認可能
- GeminiClaw が notifications を分離した背景（多チャンネル・多ユーザー運用）が RustyClaw には該当しない

### LINE 導入後: **必要**（Phase 39-2 で実装）

LINE 追加でマルチチャンネル運用になると課題が顕在化する：

| ケース | notifications なしの問題 |
|---|---|
| home = LINE | HEARTBEAT_OK ログが LINE に届き続けるノイズ |
| home = Discord、LINE を通知専用に | heartbeat アラートを LINE に誘導する設定手段がない |
| Cron ジョブが増えた場合 | 完了ログが home の会話に混入する |

---

## 9. RustyClaw 実装方針（Phase 39-2）

### config 設計

GeminiClaw の top-level 設計に倣い、`Config` 直下にプラットフォーム横断で設定：

```json
"notifications": {
  "channel": "discord",
  "channel_id": "..."
}
```

Rust 側スキーマ案：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    pub channel: String,      // "discord" | "line"
    pub channel_id: String,
}

// Config 直下に追加
pub notifications: Option<NotificationsConfig>,
```

### heartbeat.rs の変更箇所

`process_heartbeat_response` の配信先決定ロジック（現状 `home_channel_id` 固定）を変更：

```rust
// 変更前
let (target_session_id, channel_id) = if let Some(ref ch_id) = self.home_channel_id {

// 変更後（notifications 優先、未設定なら home にフォールバック）
let notify_channel_id = self.notifications_channel_id
    .as_deref()
    .or(self.home_channel_id.as_deref());
```

### 注意点

- LINE 導入（Phase 39-1）と同時実装が最小コスト。後付けすると config + heartbeat.rs の2箇所を再度変更する手間が発生する
- Cron 完了通知の集約も Phase 39-2 のスコープに含める（GeminiClaw の `notifyBackgroundJob` 相当）
- notifications 未設定時は home フォールバックを必ず実装し、既存動作を維持すること
