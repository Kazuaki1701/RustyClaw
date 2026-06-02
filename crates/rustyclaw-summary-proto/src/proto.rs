use crate::session::{ChatSession, SUMMARY_INTERVAL};
use anyhow::{Context, Result};
use rig_core::{
    client::{CompletionClient, ProviderClient},
    completion::{AssistantContent, CompletionModel, Message},
    message::UserContent,
    providers::openai::CompletionsClient,
};
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};

pub struct Config {
    pub base_url: String,
    pub api_key: String,
    pub main_model: String,
    pub summary_model: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            base_url: std::env::var("LMS_BASE_URL")
                .unwrap_or_else(|_| "http://192.168.1.110:1234/v1".to_string()),
            api_key: std::env::var("LMS_API_KEY")
                .unwrap_or_else(|_| "lm-studio".to_string()),
            main_model: std::env::var("MAIN_MODEL")
                .unwrap_or_else(|_| "google/gemma-4-e4b".to_string()),
            summary_model: std::env::var("SUMMARY_MODEL")
                .unwrap_or_else(|_| "google/gemma-4-e4b".to_string()),
        })
    }
}

pub struct SummaryProto {
    config: Arc<Config>,
    session: Arc<RwLock<ChatSession>>,
    summary_sem: Arc<Semaphore>,
}

fn make_client(base_url: &str, api_key: &str) -> Result<CompletionsClient> {
    CompletionsClient::builder()
        .api_key(api_key)
        .base_url(base_url)
        .build()
        .context("failed to build CompletionsClient")
}

fn extract_text(choice: &rig_core::OneOrMany<AssistantContent>) -> Result<String> {
    let text: String = choice
        .iter()
        .filter_map(|c| match c {
            AssistantContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        Err(anyhow::anyhow!("assistant returned no text content"))
    } else {
        Ok(text)
    }
}

fn message_to_text(msg: &Message) -> String {
    match msg {
        Message::User { content } => {
            let text = content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("User: {text}")
        }
        Message::Assistant { content, .. } => {
            let text = content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("Assistant: {text}")
        }
        Message::System { content } => format!("System: {content}"),
    }
}

impl SummaryProto {
    pub fn new(config: Config, session: ChatSession) -> Self {
        Self {
            config: Arc::new(config),
            session: Arc::new(RwLock::new(session)),
            summary_sem: Arc::new(Semaphore::new(1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::completion::message::{Text, ToolCall, ToolFunction};
    use rig_core::OneOrMany;

    #[test]
    fn extract_text_returns_text_from_choice() {
        let choice = OneOrMany::one(AssistantContent::Text(Text::new("Hello")));
        assert_eq!(extract_text(&choice).unwrap(), "Hello");
    }

    #[test]
    fn extract_text_errors_on_empty() {
        let choice = OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            "1".to_string(),
            ToolFunction::new("test".to_string(), serde_json::json!({})),
        )));
        assert!(extract_text(&choice).is_err());
    }

    #[test]
    fn message_to_text_user() {
        let m = Message::user("こんにちは");
        assert_eq!(message_to_text(&m), "User: こんにちは");
    }

    #[test]
    fn message_to_text_assistant() {
        let m = Message::assistant("はい");
        assert_eq!(message_to_text(&m), "Assistant: はい");
    }
}
