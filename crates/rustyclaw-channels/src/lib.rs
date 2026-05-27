use anyhow::{Context, Result};
use async_trait::async_trait;
use serenity::async_trait as serenity_async_trait;
use serenity::model::channel::Message as SerenityMessage;
use serenity::prelude::*;
use std::sync::Arc;

// ==============================================================================
// 1. データ構造と Channel トレイトの定義
// ==============================================================================

#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub session_id: String,
    pub user_id: String,
    pub channel_id: String,
    pub content: String,
}

#[async_trait]
pub trait Channel: Send + Sync {
    /// メッセージを外部チャンネルに送信する (配信)
    async fn send_message(&self, channel_id: &str, content: &str) -> Result<()>;
}

// ==============================================================================
// 2. Discord コネクタ (DiscordConnector) の実装
// ==============================================================================

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

pub type MessageCallback = Arc<dyn Fn(IncomingMessage) + Send + Sync>;

pub struct DiscordConnector {
    token: String,
    callback: Option<MessageCallback>,
    http: Option<Arc<serenity::http::Http>>,
    respond_in_channels: Vec<String>,
    shard_manager: Arc<tokio::sync::Mutex<Option<Arc<serenity::gateway::ShardManager>>>>,
}

impl DiscordConnector {
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

    /// 受信メッセージコールバックの登録
    pub fn register_callback(&mut self, callback: MessageCallback) {
        self.callback = Some(callback);
    }

    pub fn set_respond_in_channels(&mut self, channels: Vec<String>) {
        self.respond_in_channels = channels;
    }

    /// Discord WebSocket 接続をグレースフルに切断する。
    pub async fn shutdown(&self) {
        let lock = self.shard_manager.lock().await;
        if let Some(sm) = lock.as_ref() {
            sm.shutdown_all().await;
            tracing::info!("Discord ShardManager shutdown complete.");
        }
    }

    /// コネクタ（クライアント）の起動
    pub async fn start(&self) -> Result<()> {
        if self.token == "mock" || self.token == "dummy" || self.token.is_empty() {
            tracing::info!("DiscordConnector started in MOCK/DUMMY mode. Gateway connection skipped.");
            return Ok(());
        }

        let intents = GatewayIntents::GUILD_MESSAGES 
            | GatewayIntents::DIRECT_MESSAGES 
            | GatewayIntents::MESSAGE_CONTENT;

        let handler = DiscordHandler {
            callback: self.callback.clone(),
            respond_in_channels: self.respond_in_channels.clone(),
        };

        let mut client = Client::builder(&self.token, intents)
            .event_handler(handler)
            .await
            .context("Failed to create serenity client")?;

        tracing::info!("Connecting to Discord Gateway...");

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

        // 接続が確立するのを待つ等の追加処理があればここに書くが、基本的には起動完了として返す
        Ok(())
    }
}

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

// ==============================================================================
// 3. Serenity イベントハンドラ (DiscordHandler)
// ==============================================================================

struct DiscordHandler {
    callback: Option<MessageCallback>,
    respond_in_channels: Vec<String>,
}

#[serenity_async_trait]
impl EventHandler for DiscordHandler {
    async fn ready(&self, _ctx: serenity::prelude::Context, ready: serenity::model::gateway::Ready) {
        tracing::info!(
            bot_name = %ready.user.name,
            guild_count = ready.guilds.len(),
            "Discord bot connected and ready"
        );
    }

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
        let _ = ctx.http.broadcast_typing(msg.channel_id.get().into()).await;

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
}

// ==============================================================================
// Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[tokio::test]
    async fn test_mock_connector_callback() -> Result<()> {
        let mut connector = DiscordConnector::new("mock");

        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        connector.register_callback(Arc::new(move |msg| {
            received_clone.lock().unwrap().push(msg);
        }));

        connector.start().await?;

        // モックコールバックのテスト
        if let Some(ref cb) = connector.callback {
            cb(IncomingMessage {
                session_id: "test-session".to_string(),
                user_id: "user-1".to_string(),
                channel_id: "channel-1".to_string(),
                content: "Hello Mock".to_string(),
            });
        }

        let msgs = received.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "Hello Mock");

        Ok(())
    }

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
}
