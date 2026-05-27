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

pub type MessageCallback = Arc<dyn Fn(IncomingMessage) + Send + Sync>;

pub struct DiscordConnector {
    token: String,
    callback: Option<MessageCallback>,
    http: Option<Arc<serenity::http::Http>>,
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
        }
    }

    /// 受信メッセージコールバックの登録
    pub fn register_callback(&mut self, callback: MessageCallback) {
        self.callback = Some(callback);
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
        };

        let mut client = Client::builder(&self.token, intents)
            .event_handler(handler)
            .await
            .context("Failed to create serenity client")?;

        tracing::info!("Connecting to Discord Gateway...");
        
        // 別タスクでクライアントを動かす
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
        tracing::info!("Sending message to Discord Channel {}: {}", channel_id, content);
        
        let http = self.http.as_ref()
            .context("DiscordConnector is in MOCK mode (or token not set). Cannot send real message.")?;

        let cid: u64 = channel_id.parse()
            .context("Failed to parse channel_id as u64")?;

        let serenity_channel_id = serenity::model::id::ChannelId::new(cid);
        
        serenity_channel_id.say(http, content).await
            .context("Failed to send message via serenity http client")?;

        Ok(())
    }
}

// ==============================================================================
// 3. Serenity イベントハンドラ (DiscordHandler)
// ==============================================================================

struct DiscordHandler {
    callback: Option<MessageCallback>,
}

#[serenity_async_trait]
impl EventHandler for DiscordHandler {
    async fn message(&self, _ctx: serenity::prelude::Context, msg: SerenityMessage) {
        // ボット自身の発言は無視
        if msg.author.bot {
            return;
        }

        tracing::info!("Received message on Discord from {}: {}", msg.author.name, msg.content);

        if let Some(ref cb) = self.callback {
            // セッションIDの命名規則：discord-C{チャンネルID}-{YYYYMMDD}
            let today = chrono::Utc::now().format("%Y%m%d").to_string();
            let session_id = format!("discord-C{}-{}", msg.channel_id, today);

            let incoming = IncomingMessage {
                session_id,
                user_id: msg.author.id.to_string(),
                channel_id: msg.channel_id.to_string(),
                content: msg.content.clone(),
            };

            cb(incoming);
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
}
