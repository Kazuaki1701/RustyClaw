use crate::session::{ChatSession, SUMMARY_INTERVAL};
use anyhow::{Context, Result};
use rig_core::{
    client::CompletionClient,
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

    pub async fn chat(&self, user_input: &str) -> Result<String> {
        // 1. 現在のセッション状態を読み取る
        let (system_prompt, history) = {
            let s = self.session.read().await;
            let system = if s.current_summary.is_empty() {
                "あなたは親切なアシスタントです。".to_string()
            } else {
                format!(
                    "あなたは親切なアシスタントです。\n\n## これまでの会話の要約\n{}",
                    s.current_summary
                )
            };
            (system, s.recent_messages.clone())
        };

        // 2. メインLLMを呼び出す
        let client = make_client(&self.config.base_url, &self.config.api_key)?;
        let model = client.completion_model(&self.config.main_model);

        let request = model
            .completion_request(Message::user(user_input))
            .preamble(system_prompt)
            .messages(history)
            .build();

        let response = model
            .completion(request)
            .await
            .context("main LLM call failed")?;

        let assistant_text = extract_text(&response.choice)?;

        // 3. セッション状態を更新し、サマリートリガー判定を行う
        let maybe_snapshot = {
            let mut s = self.session.write().await;
            s.raw_history
                .push(("user".to_string(), user_input.to_string()));
            s.raw_history
                .push(("assistant".to_string(), assistant_text.clone()));
            s.recent_messages.push(Message::user(user_input));
            s.recent_messages.push(Message::assistant(&assistant_text));
            s.counter += 1;

            if s.counter >= SUMMARY_INTERVAL {
                let snapshot = s.recent_messages.clone();
                s.recent_messages.clear();
                s.counter = 0;
                Some(snapshot)
            } else {
                None
            }
        };

        // 4. 必要に応じてバックグラウンド要約を起動
        if let Some(snapshot) = maybe_snapshot {
            self.spawn_summary_task(snapshot);
        }

        Ok(assistant_text)
    }

    fn spawn_summary_task(&self, snapshot: Vec<Message>) {
        let Ok(permit) = self.summary_sem.clone().try_acquire_owned() else {
            tracing::warn!("summary semaphore busy – skipping this update");
            return;
        };

        let config = self.config.clone();
        let session = self.session.clone();

        tokio::spawn(async move {
            let _permit = permit; // タスク終了時に自動解放
            if let Err(e) = run_summary(config, session, snapshot).await {
                tracing::warn!("background summary failed: {:#}", e);
            }
        });
    }
}

async fn run_summary(
    config: Arc<Config>,
    session: Arc<RwLock<ChatSession>>,
    snapshot: Vec<Message>,
) -> Result<()> {
    let history_text = snapshot
        .iter()
        .map(message_to_text)
        .collect::<Vec<_>>()
        .join("\n");

    let summary_prompt = format!(
        "以下の会話を200字以内の日本語で要約してください。重要なトピックと結論に焦点を当ててください。\n\n{}",
        history_text
    );

    let client = make_client(&config.base_url, &config.api_key)?;
    let model = client.completion_model(&config.summary_model);

    let request = model
        .completion_request(Message::user(&summary_prompt))
        .preamble("あなたは会話を簡潔に要約するアシスタントです。".to_string())
        .build();

    let response = model
        .completion(request)
        .await
        .context("summary LLM call failed")?;

    let new_summary = extract_text(&response.choice)?;

    {
        let mut s = session.write().await;
        s.current_summary = new_summary;
        if let Err(e) = s.persist_summary() {
            tracing::warn!("failed to persist summary.md: {:#}", e);
        }
    }

    tracing::info!("background summary updated successfully");
    Ok(())
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

    /// LM Studio が稼働していることが前提の結合テスト。
    /// cargo test -p rustyclaw-summary-proto -- --ignored で実行。
    #[tokio::test]
    #[ignore]
    async fn integration_chat_reaches_lm_studio() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::from_env().unwrap();
        let session = crate::session::ChatSession::load(dir.path()).unwrap();
        let proto = SummaryProto::new(config, session);

        let resp = proto.chat("こんにちは").await;
        assert!(resp.is_ok(), "LLM call failed: {:?}", resp.err());
        assert!(!resp.unwrap().is_empty());
    }
}
