# GeminiClaw — HEARTBEAT non-OK 時の配信フロー詳細分析

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の調査報告書 - 点検完了済み)  
> **完了日**: 2026-06-03  
> **備考**: 本件は、GeminiClawのHEARTBEAT失敗時の通知ロジックに関する詳細なコード分析です。

**調査日:** 2026-06-03  
**調査対象:** `src/agent/turn/finalize.ts`, `src/inngest/heartbeat.ts`, `src/agent/context-builder.ts`, `src/cli/commands/heartbeat.ts`, `src/mcp/admin-server.ts`  
**関連文書:** `2026-06-03-geminiclaw-heartbeat-gap-analysis.md`（HEARTBEAT_OK 検出ロジックのギャップ G1〜G5）

---

## 1. 配信フローの全体構造

HEARTBEAT_OK でない場合（Critical）、GeminiClaw は **2段階**で配信を行う。

```
Heartbeat 実行
  │
  ├─ [Agent 実行中]  geminiclaw_post_message ツール呼び出し
  │       → home channel に通知テキストを直接投稿
  │       → SessionStore に trigger:"proactive" として記録
  │
  └─ [deliver フェーズ]  notifyBackgroundJob ハンドラ（finalize.ts:58）
          → notifications チャンネル（未設定なら home）に
            "⚠️ Heartbeat Alert\n{responseText 先頭500文字}" を投稿
          → config.heartbeat.desktop=true なら OS デスクトップ通知も発火
```

---

## 2. Agent 実行中の能動的投稿（geminiclaw_post_message）

### コンテキスト注入（context-builder.ts:275–296）

Heartbeat トリガー時、以下がセッションコンテキストに注入される：

```typescript
// Heartbeat Mode ディレクティブ
'Post all notifications via `geminiclaw_post_message` to the home channel. ' +
'Always respond with `HEARTBEAT_OK` when done.'

// Home チャンネルの解決（config.home から取得）
`Home channel: ${homeChannel}`  // 例: "discord:1234567890"
```

エージェントはこの指示に従い、Critical な発見があった場合は **応答テキストに書くのではなく** `geminiclaw_post_message` ツールを呼び出して home channel に直接投稿する。

### ツール実行後の記録（admin-server.ts:382–398）

投稿成功後、該当チャンネルに対応するセッション JSONL に自動記録される：

```typescript
store.appendEntry(sessionId, {
    trigger: 'proactive',
    responseText: message,
    heartbeatOk: true,  // ← この投稿自体は "OK" として記録
    // ...
});
```

これにより次回セッション再開時、エージェントは「自分が以前このチャンネルに投稿した内容」を会話履歴外のプロアクティブ発言として認識できる（RustyClaw の `process_proactive_posts` 相当）。

---

## 3. deliver フェーズの notifyBackgroundJob（finalize.ts:58–97）

エージェント実行完了後、Inngest の deliver ステップで実行されるシステム側投稿。

```typescript
async function notifyBackgroundJob(ctx: DeliverContext): Promise<void> {
    const trigger = ctx.eventData.trigger;
    const promises: Promise<void>[] = [];

    let text: string;
    if (trigger === 'heartbeat') {
        const isAlert = !ctx.runResult.heartbeatOk;
        const filtered = filterResponseText(ctx.runResult.responseText);
        text = isAlert
            ? `⚠️ **Heartbeat Alert**\n${filtered.substring(0, 500)}`
            : '✅ **Heartbeat OK**';

        // デスクトップ通知: Alert 時のみ
        if (isAlert && ctx.config.heartbeat.desktop) {
            promises.push(sendDesktopNotification('GeminiClaw ⚠️', filtered.substring(0, 300)));
        }
    } else {
        // Cron ジョブの完了/失敗通知
        const jobId = ctx.eventData.sessionId.replace(/^cron:/, '');
        const hasError = !!ctx.runResult.error;
        text = hasError
            ? `⚠️ **Cron failed: ${jobId}**\n${ctx.runResult.error?.substring(0, 300)}`
            : `✅ **Cron done: ${jobId}**`;
    }

    // notifications チャンネル（未設定なら home にフォールバック）
    const notifTarget = ctx.config.notifications ?? ctx.config.home;
    if (notifTarget) {
        promises.push(
            postToChannel({
                channelType: notifTarget.channel,
                channelId: notifTarget.channelId,
                text,
                config: ctx.config,
            })
        );
    }

    await Promise.allSettled(promises);
}
```

### 投稿内容まとめ

| trigger | heartbeatOk | 投稿テキスト | デスクトップ通知 |
|---|---|---|---|
| heartbeat | false | `⚠️ **Heartbeat Alert**\n{先頭500文字}` | あり（`desktop=true` 時） |
| heartbeat | true | `✅ **Heartbeat OK**` | なし |
| cron | 成功 | `✅ **Cron done: {jobId}**` | なし |
| cron | 失敗 | `⚠️ **Cron failed: {jobId}**\n{error先頭300文字}` | なし |

### 発火条件（FINALIZE_HANDLERS）

```typescript
const FINALIZE_HANDLERS = [
    { id: 'generate-title',        condition: (ctx) => /* 最初のターン */ },
    { id: 'notify-background-job', condition: isBackgroundJob,  run: notifyBackgroundJob },
    { id: 'send-reply',            condition: hasReplyTarget,   run: sendReply },
];
```

- `isBackgroundJob`: trigger が `'heartbeat'` または `'cron'` の場合
- `sendReply`: `serializedThread` がある場合のみ実行。Heartbeat は `sessionId: 'cron:heartbeat'` でスレッドなし → **常にスキップ**

---

## 4. デスクトップ通知（notifier.ts）

macOS と Linux の両方に対応。失敗は非 fatal でサイレントに無視される。

```typescript
export async function sendDesktopNotification(title: string, body: string): Promise<void> {
    try {
        if (process.platform === 'darwin') {
            await execFileAsync('osascript', [
                '-e', `display notification "${body}" with title "${title}"`,
            ]);
        } else {
            // Linux / WSL
            await execFileAsync('notify-send', [title, body]);
        }
    } catch {
        // 非 fatal
    }
}
```

- タイトル: `GeminiClaw ⚠️`
- ボディ: `filterResponseText(responseText).substring(0, 300)`

---

## 5. CLI 環境での挙動

### 通常起動（Inngest 経由）

```
geminiclaw heartbeat
```

`inngest.send` で `geminiclaw/run` イベントを発火 → `agent-run.ts` が受け取り、上記の通常フロー（deliver フェーズ含む）を実行。

### `--sync` モード（直接実行）

```
geminiclaw heartbeat --sync
```

`runner.ts::runAgentTurn` を直接呼ぶ：

```typescript
export async function runAgentTurn(params: RunTurnParams): Promise<RunResult> {
    const resumeCheck = checkResumable(params);
    const { sessionContext } = await buildAgentContext(params);
    const result = await runGemini({ ...params, sessionContext, resumeCheck });
    await runPostRun({ params, runResult: result });
    return result;  // ← runDeliver は呼ばれない
}
```

`runDeliver`（`notifyBackgroundJob` を含む）が**呼ばれない**。

- `result.responseText` が `process.stdout` に書き出されるだけ
- エージェントが実行中に `geminiclaw_post_message` ツールを呼んだ場合は Discord に届く
- `notifyBackgroundJob` による `⚠️ Heartbeat Alert` 投稿は発生しない
- デスクトップ通知も発生しない

→ `--sync` はデバッグ・動作確認用途。本番運用は必ず Inngest 経由。

---

## 6. RustyClaw との設計差分

| 観点 | GeminiClaw | RustyClaw |
|---|---|---|
| 通知の発火構造 | **二重**: エージェントがツールで能動投稿 ＋ システムが `notifyBackgroundJob` でも投稿 | **単一**: `responseText` を `MessageBus` 経由で home_channel_id に配信 |
| OK 時の Discord 投稿 | `✅ Heartbeat OK` を notifications に送る | 完全無音（`memory/logs/` に記録のみ） |
| Cron 完了通知 | `✅ Cron done: {jobId}` を notifications に集約 | 実装なし |
| デスクトップ通知 | あり（`config.heartbeat.desktop`） | なし |
| CLI 直接実行 | `--sync` で動作確認可能（deliver スキップ） | `--no-agent` テストで動作確認 |
| Proactive 記録 | `trigger:"proactive"` で SessionStore に記録 → 次回差し戻し | 同等実装あり（`process_proactive_posts`） |

### RustyClaw の二重投稿リスクへの対処

GeminiClaw はエージェントがツールで投稿する設計のため、モデルが誤って `responseText` にも通知内容を書いた場合に二重投稿が発生しうる。これを防ぐためプロンプトで「通知はツール経由のみ」と明示している。

RustyClaw は逆に「エージェントは responseText に書くだけ」の単一構造なので、この問題は発生しない。ただし `geminiclaw_post_message` 相当のツールがなく、エージェントがチャンネルを能動的に選択する柔軟性もない。

---

## 7. Phase 39 実装時の参照ポイント

LINE 導入（Phase 39-1）と notifications チャンネル（Phase 39-2）の実装時：

- `heartbeat.rs::process_heartbeat_response` の配信先を `notifications_channel_id` 優先に切り替える際、**フォールバック（未設定 → home）** を必ず実装すること（GeminiClaw `finalize.ts:79` 参照）
- LINE を home にした場合、HEARTBEAT_OK のログが LINE に届き続けるノイズを防ぐのが notifications 分離の主目的
- Cron 完了通知の集約も notifications チャンネルの用途として Phase 39-2 の対象に含める
