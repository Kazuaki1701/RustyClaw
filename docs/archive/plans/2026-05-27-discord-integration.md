# Discord Integration Implementation Plan

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の実装計画書 - 開発完了済み)  
> **完了日**: 2026-05-27  
> **備考**: Discord ボット連携機能（Phase 3）の導入計画書です。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Discord ボットとして完全に動作させ、Heartbeat の proactive 通知まで含む本番品質の Discord 接続を完成させる。

**Architecture:** 既存の `serenity` 0.12 スケルトン（`DiscordConnector` + `DiscordHandler`）を拡張する。設定値を `config.json` に移し、GeminiClaw の `reply.ts` を参考にしたコードブロック対応の長文チャンク・`respondInChannels` ホワイトリスト・Discord システムメッセージフィルタ・typing indicator・ready イベント・ShardManager グレースフルシャットダウンを追加する。

**Tech Stack:** `serenity 0.12` (rustls_backend), `tokio`, `anyhow`, `chrono`

**GeminiClaw 参照元：**
- `channels/reply.ts` — `splitMessage()` のコードブロック対応アルゴリズム
- `channels/chat-handlers.ts` — `respondInChannels` ホワイトリスト・`isUserMessage()` 系統メッセージフィルタ

---

## ファイル変更マップ

| ファイル | 変更種別 | 責務 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | 修正 | `discord_token`, `discord_home_channel_id`, `discord_respond_in_channels` フィールド追加 |
| `crates/rustyclaw-channels/src/lib.rs` | 修正 | 長文チャンク・typing indicator・ready・ShardManager・システムメッセージフィルタ |
| `crates/rustyclaw-gateway/src/lib.rs` | 修正 | Config からトークン取得・graceful shutdown・テスト修正 |
| `crates/rustyclaw-gateway/src/heartbeat.rs` | 修正 | home_channel_id を使った proactive 通知先の確定 |

---

## Task 1: Discord 設定フィールドを Config に追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (テスト内 Config リテラル修正)

- [ ] **Step 1: Config に 2フィールド追加**

`crates/rustyclaw-config/src/lib.rs` の `Config` struct を以下に変更する：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_provider: String,
    pub model_name: String,
    pub api_key: String,
    pub api_base_url: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    #[serde(default)]
    pub debug_dump: bool,
    #[serde(default)]
    pub timezone: Option<String>,
    /// Discord Bot トークン。未設定時は環境変数 DISCORD_TOKEN にフォールバック。
    #[serde(default)]
    pub discord_token: Option<String>,
    /// Heartbeat proactive 通知の送信先 Discord チャンネル ID（数字文字列）。
    #[serde(default)]
    pub discord_home_channel_id: Option<String>,
    /// @mention なしでも応答するチャンネル ID のホワイトリスト（GeminiClaw respondInChannels 相当）。
    /// 空リストの場合は @mention のみ応答。
    #[serde(default)]
    pub discord_respond_in_channels: Vec<String>,
}
```

- [ ] **Step 2: gateway テストの Config リテラルを修正**

`crates/rustyclaw-gateway/src/lib.rs` の `test_lane_registry_serialization_and_semaphore` テスト内を以下に変更する：

```rust
let config = Config {
    model_provider: "gmn".to_string(),
    model_name: "dummy".to_string(),
    api_key: "dummy".to_string(),
    api_base_url: "dummy".to_string(),
    max_tokens: None,
    temperature: None,
    debug_dump: false,
    timezone: None,
    discord_token: None,
    discord_home_channel_id: None,
    discord_respond_in_channels: vec![],
};
```

- [ ] **Step 3: Gateway の Discord トークン取得を Config 優先に変更**

`crates/rustyclaw-gateway/src/lib.rs` の `Gateway::run()` 内、Discord トークン取得部分（現在の `std::env::var("DISCORD_TOKEN")` 行）を以下に置き換える：

```rust
let discord_token = config.discord_token.clone()
    .or_else(|| std::env::var("DISCORD_TOKEN").ok())
    .unwrap_or_else(|| "dummy".to_string());
```

- [ ] **Step 4: ビルドとテストの確認**

```bash
cargo test -p rustyclaw-config -p rustyclaw-gateway 2>&1
```

期待結果: `test result: ok. N passed`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(config): add discord_token and discord_home_channel_id fields"
```

---

## Task 2: Discord 長文チャンク分割（コードブロック対応）

Discord は 1メッセージあたり 2000文字制限がある。単純な文字数切断ではコードブロックが途中で壊れる。
GeminiClaw `channels/reply.ts` の `splitMessage()` と同じ戦略：行境界優先・コードフェンス補完。

**Files:**
- Modify: `crates/rustyclaw-channels/src/lib.rs`

- [ ] **Step 1: 失敗テストを書く**

`crates/rustyclaw-channels/src/lib.rs` のテストモジュール末尾に追加する：

```rust
#[test]
fn test_split_message_short() {
    let chunks = split_message("hello", 2000);
    assert_eq!(chunks, vec!["hello"]);
}

#[test]
fn test_split_message_empty() {
    let chunks = split_message("", 2000);
    assert!(chunks.is_empty());
}

#[test]
fn test_split_message_line_boundary() {
    // 3行、各行700文字。1チャンク = 最大2000文字なので 2行+1行 に分割される
    let line = "a".repeat(700);
    let text = format!("{}\n{}\n{}", line, line, line);
    let chunks = split_message(&text, 2000);
    assert_eq!(chunks.len(), 2, "should split into 2 chunks at line boundary");
    assert!(chunks[0].len() <= 2000);
    assert!(chunks[1].len() <= 2000);
}

#[test]
fn test_split_message_preserves_code_fence() {
    // コードブロックをまたいで分割される場合、フェンスが補完されること
    let code_line = "x".repeat(500);
    let text = format!("```rust\n{}\n{}\n{}\n```", code_line, code_line, code_line);
    let chunks = split_message(&text, 2000);
    if chunks.len() > 1 {
        // 途中チャンクは ``` で閉じられる
        assert!(chunks[0].ends_with("```"), "first chunk must close fence: {:?}", &chunks[0][chunks[0].len().saturating_sub(10)..]);
        // 次チャンクは ``` で再開される
        assert!(chunks[1].starts_with("```"), "next chunk must reopen fence");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-channels 2>&1
```

期待結果: `error[E0425]: cannot find function 'split_message'`

- [ ] **Step 3: `split_message` 関数を実装（GeminiClaw reply.ts 移植）**

`crates/rustyclaw-channels/src/lib.rs` に以下を追加する（モジュールトップレベル、`DiscordConnector` 定義より前）：

```rust
const DISCORD_MAX_LENGTH: usize = 2000;

/// Discord 2000文字制限に合わせてテキストを分割する。
///
/// 分割優先順:
///   1. 行境界（改行）
///   2. 単一行が制限超の場合はハードカット
///
/// コードブロック（```）をまたいで分割される場合は、
/// 閉じフェンスと開きフェンスを自動挿入して各チャンクを有効な Markdown にする。
/// (GeminiClaw channels/reply.ts の splitMessage() 移植)
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let lines: Vec<&str> = text.split('\n').collect();
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_code_block = false;
    let mut code_fence = String::new();

    for line in lines {
        // コードフェンス検出（``` または ````）
        let is_fence = line.trim_start().starts_with("```");
        let would_be_len = if current.is_empty() {
            line.len()
        } else {
            current.len() + 1 + line.len()
        };

        if would_be_len > max_len && !current.is_empty() {
            // コードブロック内で分割する場合はフェンスを閉じる
            if in_code_block {
                current.push_str("\n```");
            }
            chunks.push(current.clone());
            // 次チャンクの先頭: コードブロック継続中なら再開フェンスを追加
            current = if in_code_block {
                format!("{}\n", code_fence)
            } else {
                String::new()
            };
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);

        // フェンスのトグル
        if is_fence {
            if !in_code_block {
                in_code_block = true;
                code_fence = line.trim_start().to_string();
            } else {
                in_code_block = false;
                code_fence.clear();
            }
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
```

`Channel for DiscordConnector` の `send_message` 実装を以下に置き換える：

```rust
#[async_trait]
impl Channel for DiscordConnector {
    async fn send_message(&self, channel_id: &str, content: &str) -> Result<()> {
        let http = self.http.as_ref()
            .context("DiscordConnector is in MOCK mode. Cannot send real message.")?;

        let cid: u64 = channel_id.parse()
            .context("Failed to parse channel_id as u64")?;
        let serenity_channel_id = serenity::model::id::ChannelId::new(cid);

        for chunk in split_message(content, DISCORD_MAX_LENGTH) {
            tracing::debug!("Sending chunk ({} chars) to channel {}", chunk.len(), channel_id);
            serenity_channel_id.say(http, &chunk).await
                .context("Failed to send message chunk via serenity")?;
        }
        Ok(())
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

```bash
cargo test -p rustyclaw-channels 2>&1
```

期待結果: `test result: ok. N passed`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-channels/src/lib.rs
git commit -m "feat(channels): add line-aware Discord message chunking with code fence preservation"
```

---

## Task 3: Discord メッセージフィルタ・respond_in_channels・タイピングインジケーター

Discord システムメッセージ（pin 通知・スレッド作成通知など）の誤処理を防ぎ、
指定チャンネル以外では応答しないホワイトリスト制御を追加する。
ボット応答中は「入力中...」インジケーターを送出する。

GeminiClaw 参照: `channels/chat-handlers.ts` `isUserMessage()` + `shouldRespondInChannel()`

**Files:**
- Modify: `crates/rustyclaw-channels/src/lib.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (respond_in_channels を渡す)

前提: Task 1 完了済み（Config に `discord_respond_in_channels: Vec<String>` あり）

- [ ] **Step 1: 失敗テストを書く**

`crates/rustyclaw-channels/src/lib.rs` のテストモジュール末尾に追加する：

```rust
#[test]
fn test_is_allowed_message_type_regular() {
    use serenity::model::channel::MessageType;
    assert!(is_allowed_message_type(&MessageType::Regular));
}

#[test]
fn test_is_allowed_message_type_inline_reply() {
    use serenity::model::channel::MessageType;
    assert!(is_allowed_message_type(&MessageType::InlineReply));
}

#[test]
fn test_is_allowed_message_type_pins_add() {
    use serenity::model::channel::MessageType;
    assert!(!is_allowed_message_type(&MessageType::PinsAdd));
}

#[test]
fn test_is_allowed_message_type_member_join() {
    use serenity::model::channel::MessageType;
    assert!(!is_allowed_message_type(&MessageType::MemberJoin));
}

#[test]
fn test_should_respond_in_channel_empty_list() {
    // 空リスト → 全チャンネル対象
    assert!(should_respond_in_channel("123456789", &[]));
}

#[test]
fn test_should_respond_in_channel_listed() {
    let list = vec!["111".to_string(), "222".to_string()];
    assert!(should_respond_in_channel("111", &list));
    assert!(should_respond_in_channel("222", &list));
}

#[test]
fn test_should_respond_in_channel_not_listed() {
    let list = vec!["111".to_string()];
    assert!(!should_respond_in_channel("999", &list));
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-channels 2>&1
```

期待結果: `error[E0425]: cannot find function 'is_allowed_message_type'`

- [ ] **Step 3: フィルタヘルパー関数を追加**

`crates/rustyclaw-channels/src/lib.rs` の `split_message` 定義の直後（`DiscordConnector` 定義より前）に追加する：

```rust
/// Discord メッセージタイプフィルタ。
/// DEFAULT (0) と REPLY (19) のみ処理する（システムイベントを除外）。
/// (GeminiClaw channels/chat-handlers.ts isUserMessage() 相当)
fn is_allowed_message_type(kind: &serenity::model::channel::MessageType) -> bool {
    matches!(
        kind,
        serenity::model::channel::MessageType::Regular
            | serenity::model::channel::MessageType::InlineReply
    )
}

/// respond_in_channels ホワイトリストチェック。
/// リストが空の場合は全チャンネル対象。リストが非空の場合は一致するチャンネルのみ。
/// (GeminiClaw channels/chat-handlers.ts shouldRespondInChannel() 相当)
fn should_respond_in_channel(channel_id: &str, respond_in_channels: &[String]) -> bool {
    respond_in_channels.is_empty() || respond_in_channels.iter().any(|c| c == channel_id)
}
```

- [ ] **Step 4: テストが通ることを確認**

```bash
cargo test -p rustyclaw-channels 2>&1
```

期待結果: `test result: ok. N passed`

- [ ] **Step 5: `DiscordConnector` と `DiscordHandler` にフィールドを追加**

`DiscordConnector` 構造体を以下に変更する：

```rust
pub struct DiscordConnector {
    token: String,
    callback: Option<MessageCallback>,
    http: Option<Arc<serenity::http::Http>>,
    respond_in_channels: Vec<String>,
}
```

`DiscordConnector::new()` の `Self { ... }` を以下に変更する：

```rust
Self {
    token: token.to_string(),
    callback: None,
    http,
    respond_in_channels: Vec::new(),
}
```

`DiscordConnector` の `impl` ブロックに setter を追加する：

```rust
pub fn set_respond_in_channels(&mut self, channels: Vec<String>) {
    self.respond_in_channels = channels;
}
```

`DiscordHandler` 構造体を以下に変更する：

```rust
struct DiscordHandler {
    callback: Option<MessageCallback>,
    respond_in_channels: Vec<String>,
}
```

- [ ] **Step 6: `start()` でフィールドを handler に渡す**

`start()` 内の `DiscordHandler { ... }` 初期化を以下に変更する：

```rust
let handler = DiscordHandler {
    callback: self.callback.clone(),
    respond_in_channels: self.respond_in_channels.clone(),
};
```

- [ ] **Step 7: `message()` ハンドラを更新**

`DiscordHandler` の `impl EventHandler` ブロック内の `message()` を以下に置き換える：

```rust
async fn message(&self, ctx: serenity::prelude::Context, msg: SerenityMessage) {
    // ボット自身の発言は無視（自己ループ防止）
    if msg.author.bot {
        return;
    }

    // Discord システムメッセージをフィルタ
    // DEFAULT (0) と REPLY (19) のみ処理（GeminiClaw isUserMessage() 相当）
    if !is_allowed_message_type(&msg.kind) {
        return;
    }

    // respond_in_channels ホワイトリストチェック（空リストは全チャンネル対象）
    if !should_respond_in_channel(&msg.channel_id.to_string(), &self.respond_in_channels) {
        return;
    }

    tracing::info!(
        user = %msg.author.name,
        channel = %msg.channel_id,
        content = %msg.content,
        "Discord message received"
    );

    // "入力中..." インジケーター送信（失敗しても続行）
    let _ = ctx.http.broadcast_typing(msg.channel_id.get()).await;

    if let Some(ref cb) = self.callback {
        let today = chrono::Local::now().format("%Y%m%d").to_string();
        let session_id = format!("discord-C{}-{}", msg.channel_id, today);
        cb(IncomingMessage {
            session_id,
            user_id: msg.author.id.to_string(),
            channel_id: msg.channel_id.to_string(),
            content: msg.content.clone(),
        });
    }
}
```

- [ ] **Step 8: Gateway で respond_in_channels を渡す**

`crates/rustyclaw-gateway/src/lib.rs` の `Gateway::run()` 内、`discord_client.start().await?` の直前に追加する：

```rust
discord_client.set_respond_in_channels(config.discord_respond_in_channels.clone());
```

- [ ] **Step 9: コンパイルとテスト確認**

```bash
cargo test -p rustyclaw-channels -p rustyclaw-gateway 2>&1
```

期待結果: `test result: ok. N passed`

- [ ] **Step 10: コミット**

```bash
git add crates/rustyclaw-channels/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(channels): add message type filter, respond_in_channels whitelist, and typing indicator"
```

---

## Task 4: `ready` イベントと ShardManager グレースフルシャットダウン

`ShardManager` を保存しておくことで、SIGTERM/SIGINT 時に Discord WebSocket 接続をクリーンに切断できる。`ready` イベントでボット名とギルド数をログに残す。

**Files:**
- Modify: `crates/rustyclaw-channels/src/lib.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: `DiscordConnector` に ShardManager 保存フィールドを追加**

`DiscordConnector` 構造体を以下に変更する（Task 3 で追加した `respond_in_channels` を維持）：

```rust
pub struct DiscordConnector {
    token: String,
    callback: Option<MessageCallback>,
    http: Option<Arc<serenity::http::Http>>,
    respond_in_channels: Vec<String>,
    shard_manager: Arc<tokio::sync::Mutex<Option<Arc<serenity::gateway::ShardManager>>>>,
}
```

`DiscordConnector::new()` を以下に変更する：

```rust
pub fn new(token: &str) -> Self {
    let http = if token == "mock" || token == "dummy" || token.is_empty() {
        None
    } else {
        Some(Arc::new(serenity::http::Http::new(token)))
    };
    Self {
        token: token.to_string(),
        callback: None,
        http,
        respond_in_channels: Vec::new(),
        shard_manager: Arc::new(tokio::sync::Mutex::new(None)),
    }
}
```

- [ ] **Step 2: `start()` で ShardManager を保存する**

`start()` メソッドの serenity client 起動部分を以下に置き換える（`tokio::spawn` の直前に追記）：

```rust
// ShardManager を保存（graceful shutdown 用）
{
    let mut sm_lock = self.shard_manager.lock().await;
    *sm_lock = Some(client.shard_manager.clone());
}

tokio::spawn(async move {
    if let Err(why) = client.start().await {
        tracing::error!("Serenity client error: {:?}", why);
    }
});
```

- [ ] **Step 3: `shutdown()` メソッドを追加**

`DiscordConnector` の `impl` ブロックに以下を追加する：

```rust
/// Discord WebSocket 接続をグレースフルに切断する。
pub async fn shutdown(&self) {
    let lock = self.shard_manager.lock().await;
    if let Some(sm) = lock.as_ref() {
        sm.shutdown_all().await;
        tracing::info!("Discord ShardManager shutdown complete.");
    }
}
```

- [ ] **Step 4: `DiscordHandler` に `ready` イベントを追加**

`impl EventHandler for DiscordHandler` ブロックに以下を追加する（`message()` の前）：

```rust
async fn ready(&self, _ctx: serenity::prelude::Context, ready: serenity::model::gateway::Ready) {
    tracing::info!(
        bot_name = %ready.user.name,
        guild_count = ready.guilds.len(),
        "Discord bot connected and ready"
    );
}
```

- [ ] **Step 5: Gateway の shutdown 呼び出し**

`crates/rustyclaw-gateway/src/lib.rs` で `discord_client` を `Arc` 化する前後を修正し、SIGINT/SIGTERM ハンドラで shutdown を呼ぶ。

`Gateway::run()` 内、既存の `break;` を以下に置き換える（SIGINT と SIGTERM の両方）：

```rust
// SIGINT ブランチ
_ = sig_int.recv() => {
    tracing::info!("Received SIGINT. Initiating graceful shutdown...");
    discord_client.shutdown().await;
    break;
}
// SIGTERM ブランチ
_ = sig_term.recv() => {
    tracing::info!("Received SIGTERM. Initiating graceful shutdown...");
    discord_client.shutdown().await;
    break;
}
```

- [ ] **Step 6: コンパイルとテスト確認**

```bash
cargo test -p rustyclaw-channels -p rustyclaw-gateway 2>&1
```

期待結果: `test result: ok. N passed`

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-channels/src/lib.rs crates/rustyclaw-gateway/src/lib.rs
git commit -m "feat(channels): add ready event, ShardManager graceful shutdown"
```

---

## Task 5: Heartbeat home_channel_id による proactive 通知先の確定

現在の実装は「最後にアクティブだったセッション」を動的に探しているため、Heartbeat が誤ったチャンネルに投稿するリスクがある。`config.discord_home_channel_id` を使い、通知先を静的に確定する。

**Files:**
- Modify: `crates/rustyclaw-gateway/src/heartbeat.rs`

- [ ] **Step 1: `HeartbeatService` に home_channel_id を持たせる**

`HeartbeatService` 構造体を以下に変更する：

```rust
pub struct HeartbeatService {
    config: Config,
    workspace_path: PathBuf,
    bus: std::sync::Arc<MessageBus>,
    /// proactive 通知の送信先チャンネル ID。None の場合はセッション検索にフォールバック。
    home_channel_id: Option<String>,
}
```

`HeartbeatService::new()` を以下に変更する：

```rust
pub fn new(config: Config, workspace_path: PathBuf, bus: std::sync::Arc<MessageBus>) -> Self {
    let home_channel_id = config.discord_home_channel_id.clone();
    Self { config, workspace_path, bus, home_channel_id }
}
```

- [ ] **Step 2: `process_heartbeat_response()` の通知先決定ロジックを修正**

`process_heartbeat_response()` 内の `else` ブロック（Proactive speak / Critical）の先頭にある `target_session_id` 決定部分を以下に置き換える：

```rust
// home_channel_id が設定されていればそれを優先、なければ旧来のセッション検索
let (target_session_id, channel_id) = if let Some(ref ch_id) = self.home_channel_id {
    let today = chrono::Local::now().format("%Y%m%d").to_string();
    let session_id = format!("discord-C{}-{}", ch_id, today);
    (session_id, ch_id.clone())
} else {
    // フォールバック: 最後にアクティブだったセッションを探す
    let sessions_dir = self.workspace_path.join("sessions");
    let mut last_active_session: Option<String> = None;
    let mut last_mod_time = SystemTime::UNIX_EPOCH;
    if let Ok(dir_entries) = fs::read_dir(&sessions_dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            if filename.ends_with(".jsonl") && !filename.starts_with("cron") {
                if let Ok(metadata) = fs::metadata(&path) {
                    if let Ok(mod_time) = metadata.modified() {
                        if mod_time > last_mod_time {
                            last_mod_time = mod_time;
                            last_active_session = Some(filename.trim_end_matches(".jsonl").to_string());
                        }
                    }
                }
            }
        }
    }
    let sid = last_active_session.unwrap_or_else(|| "discord-Cunknown-00000000".to_string());
    let ch = sid.split('-').nth(1)
        .map(|s| s.trim_start_matches('C').to_string())
        .unwrap_or_else(|| "unknown".to_string());
    (sid, ch)
};
```

以降の `target_session_id` と `channel_id` は上記で定義済みなので、元あった同名変数定義部分（約15行）は削除する。

- [ ] **Step 3: コンパイル確認**

```bash
cargo check -p rustyclaw-gateway 2>&1
```

期待結果: `Finished` (error なし)

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-gateway/src/heartbeat.rs
git commit -m "feat(heartbeat): use discord_home_channel_id for proactive notification routing"
```

---

## 動作確認手順（全タスク完了後）

### config.json の設定例

```json
{
  "model_provider": "gmn",
  "model_name": "flash",
  "api_key": "not_required",
  "api_base_url": "not_required",
  "discord_token": "YOUR_BOT_TOKEN_HERE",
  "discord_home_channel_id": "123456789012345678",
  "timezone": "Asia/Tokyo",
  "debug_dump": true
}
```

### 起動と確認

```bash
# Gateway 起動
cargo run --bin rustyclaw -- gateway --config config.json --workspace ./workspace

# 期待されるログ
# INFO Discord bot connected and ready bot_name="YourBot" guild_count=1
# INFO RustyClaw Gateway is now running.

# Discord で Bot に話しかける → "入力中..." が表示 → 返答が届く
# 2000文字を超える返答は自動的に複数メッセージに分割される
```

### グレースフルシャットダウン確認

```bash
# Ctrl+C or kill -SIGTERM <pid>
# 期待されるログ
# INFO Received SIGINT. Initiating graceful shutdown...
# INFO Discord ShardManager shutdown complete.
# INFO RustyClaw Gateway shutdown complete.
```
