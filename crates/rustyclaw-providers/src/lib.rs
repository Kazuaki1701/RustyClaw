use anyhow::Context;
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use rustyclaw_config::{get_app_dir, Config, LlmModelConfig};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Rate limit or quota exceeded: {0}")]
    RateLimit(String),
    #[error("API or CLI execution failed: {0}")]
    ExecutionFailed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl ProviderError {
    /// エラーメッセージ内に "Your Quota will reset after XXs" や "XXm YYs" 等の記述があればその時間を解析して Duration として返す。
    pub fn reset_after(&self) -> Option<Duration> {
        match self {
            ProviderError::RateLimit(msg) => {
                let lower = msg.to_lowercase();
                
                // Cloudflare 10,000 neurons daily limit check
                if lower.contains("used up your daily free allocation") || lower.contains("10,000 neurons") {
                    let now = chrono::Local::now();
                    let mut next_reset = chrono::Local::now()
                        .date_naive()
                        .and_hms_opt(9, 0, 0)
                        .unwrap()
                        .and_local_timezone(chrono::Local)
                        .unwrap();
                    if now >= next_reset {
                        next_reset = next_reset + chrono::Duration::days(1);
                    }
                    let remaining_secs = next_reset.signed_duration_since(now).num_seconds();
                    if remaining_secs > 0 {
                        return Some(Duration::from_secs(remaining_secs as u64));
                    }
                }

                if let Some(pos) = lower.find("your quota will reset after ") {
                    let start = pos + "your quota will reset after ".len();
                    let sub = &msg[start..];

                    // リセット時間文の直後の区切り（ピリオド、改行、ダブルクォーテーション等）で切り出し、
                    // メッセージ後半に含まれる "model" 等の 'm' に誤反応するのを防ぐ
                    let limit_part = if let Some(end_pos) = sub.find(|c| c == '.' || c == '\n' || c == '"' || c == '\\') {
                        &sub[..end_pos]
                    } else {
                        sub
                    };

                    let mut total_secs = 0;
                    let mut has_parsed = false;

                    if let Some(m_pos) = limit_part.find('m') {
                        // 'm' の前の数字をパース (分)
                        let m_str = limit_part[..m_pos].trim();
                        let m_num_str: String = m_str.chars().rev().take_while(|c| c.is_numeric()).collect();
                        let m_num_str: String = m_num_str.chars().rev().collect();
                        if let Ok(mins) = m_num_str.parse::<u64>() {
                            total_secs += mins * 60;
                            has_parsed = true;
                        }

                        // 'm' の後ろから 's' の間の数字をパース (秒)
                        let remaining = &limit_part[m_pos + 1..];
                        if let Some(s_pos) = remaining.find('s') {
                            let s_str = remaining[..s_pos].trim();
                            let s_num_str: String = s_str.chars().take_while(|c| c.is_numeric()).collect();
                            if let Ok(secs) = s_num_str.parse::<u64>() {
                                total_secs += secs;
                            }
                        }
                    } else if let Some(s_pos) = limit_part.find('s') {
                        // 's' の前の数字をパース (秒のみ)
                        let s_str = limit_part[..s_pos].trim();
                        let s_num_str: String = s_str.chars().rev().take_while(|c| c.is_numeric()).collect();
                        let s_num_str: String = s_num_str.chars().rev().collect();
                        if let Ok(secs) = s_num_str.parse::<u64>() {
                            total_secs += secs;
                            has_parsed = true;
                        }
                    }

                    if has_parsed {
                        return Some(Duration::from_secs(total_secs));
                    }
                }

                // Cloudflare RPM 429 ("Too Many Requests" 形式、リセット時刻なし) → デフォルト 60s
                if lower.contains("too many requests") {
                    return Some(Duration::from_secs(60));
                }

                None
            }
            _ => None,
        }
    }
}

pub fn resolve_provider_id(model: &LlmModelConfig) -> String {
    let base = model.api_base_url.to_lowercase();
    if base.contains("groq.com") {
        "groq".to_string()
    } else if base.contains("cloudflare.com") {
        "cloudflare".to_string()
    } else if base.contains("openai.com") {
        "openai".to_string()
    } else if base.contains("huggingface.co") {
        "huggingface".to_string()
    } else {
        model.model_provider.clone()
    }
}

/// RateLimit エラーからプロバイダごとのクールダウンを設定する共通ヘルパー。
pub fn set_provider_cooldown_from_error(provider: &str, err: &ProviderError) {
    let dur = err.reset_after().unwrap_or(std::time::Duration::from_secs(60));
    set_provider_cooldown(provider, dur);
}

/// 後方互換性のためのグローバルクールダウン設定ヘルパー。
pub fn set_global_cooldown_from_error(err: &ProviderError) {
    let dur = err.reset_after().unwrap_or(std::time::Duration::from_secs(60));
    set_provider_cooldown("global", dur);
}

fn get_workspace_dir() -> std::path::PathBuf {
    if let Ok(env_val) = std::env::var("RUSTYCLAW_WORKSPACE_DIR") {
        std::path::PathBuf::from(env_val)
    } else {
        let p_ws = std::path::PathBuf::from("production/workspace");
        if p_ws.exists() {
            p_ws
        } else {
            std::path::PathBuf::from("workspace")
        }
    }
}

fn dump_llm_io(
    category: &str,
    model: &str,
    messages: &[Message],
    response: &LlmResponse,
) {
    use chrono::Local;

    let ws_dir = get_workspace_dir();
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H-%M-%S").to_string();

    let category_dir = ws_dir.join("memory").join("debug").join("llm").join(category);
    let date_dir = category_dir.join(&date_str);

    if let Err(e) = std::fs::create_dir_all(&date_dir) {
        tracing::error!("Failed to create llm dump directory {:?}: {}", date_dir, e);
        return;
    }

    // Delete date folders older than 5 days
    if let Ok(entries) = std::fs::read_dir(&category_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == date_str {
                continue;
            }
            if let Ok(folder_date) = chrono::NaiveDate::parse_from_str(&name_str, "%Y-%m-%d") {
                let today = now.date_naive();
                if (today - folder_date).num_days() > 5 {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    let file_path = date_dir.join(format!("{}.json", time_str));

    #[derive(serde::Serialize)]
    struct LlmIoDump<'a> {
        timestamp: i64,
        model: &'a str,
        request: &'a [Message],
        response: &'a LlmResponse,
    }

    let dump = LlmIoDump {
        timestamp: now.timestamp(),
        model,
        request: messages,
        response,
    };

    match std::fs::File::create(&file_path) {
        Ok(file) => {
            if let Err(e) = serde_json::to_writer_pretty(file, &dump) {
                tracing::error!("Failed to write llm io dump to {:?}: {}", file_path, e);
            }
        }
        Err(e) => tracing::error!("Failed to create llm io dump file {:?}: {}", file_path, e),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String, // "function"
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // Stringified JSON arguments
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub role: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub model_used: Option<String>,
    pub provider_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub content: String,
}

pub struct CompletionOptions {
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub timeout: Duration,
    pub category: Option<String>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<LlmResponse, ProviderError>;

    async fn complete_stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>, ProviderError>;
}

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    model: LlmModelConfig,
}

impl OpenAiCompatProvider {
    pub fn new(model: LlmModelConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(900))
            .build()
            .expect("Failed to build reqwest HTTP client with rustls");
        Self { client, model }
    }

    fn resolve_model(&self, model: &str) -> String {
        if model == "flash" {
            if let Ok(env_val) = std::env::var("RUSTYCLAW_FLASH_MODEL_NAME") {
                return env_val;
            }
        }
        model.to_string()
    }
}

#[derive(Debug, Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<&'a [ToolCall]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    r#type: String, // always "function"
    function: OpenAiFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessageResponse {
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamResponse {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiDelta,
}

#[derive(Debug, Deserialize)]
struct OpenAiDelta {
    content: Option<String>,
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<LlmResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.model.api_base_url);
        
        let openai_messages: Vec<OpenAiMessage> = messages
            .iter()
            .map(|msg| OpenAiMessage {
                role: &msg.role,
                content: if msg.content.is_empty() && msg.tool_calls.is_some() {
                    None
                } else {
                    Some(&msg.content)
                },
                name: msg.name.as_deref(),
                tool_calls: msg.tool_calls.as_deref(),
                tool_call_id: msg.tool_call_id.as_deref(),
            })
            .collect();

        let openai_tools = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| OpenAiTool {
                        r#type: "function".to_string(),
                        function: OpenAiFunction {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        },
                    })
                    .collect(),
            )
        };

        let resolved_model = self.resolve_model(&opts.model);
        let request_body = OpenAiRequest {
            model: &resolved_model,
            messages: openai_messages,
            max_tokens: opts.max_tokens.or(self.model.max_tokens),
            temperature: opts.temperature.or(self.model.temperature),
            stream: None,
            tools: openai_tools,
        };

        tracing::info!("Sending LLM request to OpenAI compat API at {}", url);
        
        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.model.api_key))
            .json(&request_body)
            .send()
            .await
            .context("HTTP POST request failed during LLM call")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || error_text.contains("insufficient_quota") {
                let err = ProviderError::RateLimit(error_text);
                let provider_id = resolve_provider_id(&self.model);
                set_provider_cooldown_from_error(&provider_id, &err);
                return Err(err);
            }
            return Err(ProviderError::ExecutionFailed(format!("LLM API returned error status {}: {}", status, error_text)));
        }

        if let Some(neurons_header) = response.headers().get("cf-ai-neurons") {
            if let Ok(neurons_str) = neurons_header.to_str() {
                if let Ok(neurons) = neurons_str.parse::<f64>() {
                    record_neuron_usage(neurons);
                }
            }
        }

        let resp_data: OpenAiResponse = response.json()
            .await
            .context("Failed to parse LLM JSON response")?;

        let choice = resp_data.choices.first()
            .context("LLM returned empty choices in response")?;

        let res = LlmResponse {
            content: choice.message.content.clone().unwrap_or_default(),
            role: choice.message.role.clone(),
            tool_calls: choice.message.tool_calls.clone(),
            prompt_tokens: resp_data.usage.as_ref().map(|u| u.prompt_tokens),
            completion_tokens: resp_data.usage.as_ref().map(|u| u.completion_tokens),
            total_tokens: resp_data.usage.as_ref().map(|u| u.total_tokens),
            model_used: resp_data.model.clone(),
            provider_id: None,
        };

        let mut resolved_category = opts.category.clone().unwrap_or_else(|| "discord".to_string());
        if res.tool_calls.as_ref().map_or(false, |tc| !tc.is_empty()) {
            resolved_category = "tools".to_string();
        }
        dump_llm_io(&resolved_category, &resolved_model, messages, &res);

        Ok(res)
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>, ProviderError> {
        let url = format!("{}/chat/completions", self.model.api_base_url);
        
        let openai_messages: Vec<OpenAiMessage> = messages
            .iter()
            .map(|msg| OpenAiMessage {
                role: &msg.role,
                content: if msg.content.is_empty() && msg.tool_calls.is_some() {
                    None
                } else {
                    Some(&msg.content)
                },
                name: msg.name.as_deref(),
                tool_calls: msg.tool_calls.as_deref(),
                tool_call_id: msg.tool_call_id.as_deref(),
            })
            .collect();

        let resolved_model = self.resolve_model(&opts.model);
        let request_body = OpenAiRequest {
            model: &resolved_model,
            messages: openai_messages,
            max_tokens: opts.max_tokens.or(self.model.max_tokens),
            temperature: opts.temperature.or(self.model.temperature),
            stream: Some(true),
            tools: None,
        };

        tracing::info!("Sending streaming LLM request to OpenAI compat API at {}", url);

        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.model.api_key))
            .json(&request_body)
            .send()
            .await
            .context("HTTP POST stream request failed during LLM call")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || error_text.contains("insufficient_quota") {
                let err = ProviderError::RateLimit(error_text);
                let provider_id = resolve_provider_id(&self.model);
                set_provider_cooldown_from_error(&provider_id, &err);
                return Err(err);
            }
            return Err(ProviderError::ExecutionFailed(format!("LLM Stream API returned error status {}: {}", status, error_text)));
        }

        if let Some(neurons_header) = response.headers().get("cf-ai-neurons") {
            if let Ok(neurons_str) = neurons_header.to_str() {
                if let Ok(neurons) = neurons_str.parse::<f64>() {
                    record_neuron_usage(neurons);
                }
            }
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let resolved_model_clone = resolved_model.clone();
        let resolved_category = opts.category.clone().unwrap_or_else(|| "discord".to_string());
        let messages_vec = messages.to_vec();

        let output_stream = async_stream::try_stream! {
            let mut full_response_content = String::new();
            while let Some(chunk_res) = stream.next().await {
                let chunk = chunk_res.context("Error reading byte chunk from stream")?;
                let chunk_str = std::str::from_utf8(&chunk)
                    .context("Failed to parse SSE bytes as UTF-8 string")?;
                
                buffer.push_str(chunk_str);

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    if line.starts_with("data: ") {
                        let data = &line["data: ".len()..];
                        if data == "[DONE]" {
                            break;
                        }

                        if let Ok(parsed) = serde_json::from_str::<OpenAiStreamResponse>(data) {
                            if let Some(choice) = parsed.choices.first() {
                                if let Some(content) = &choice.delta.content {
                                    full_response_content.push_str(content);
                                    yield StreamChunk {
                                        content: content.clone(),
                                    };
                                }
                            }
                        }
                    }
                }
            }
            let llm_res = LlmResponse {
                content: full_response_content,
                role: "assistant".to_string(),
                tool_calls: None,
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
                model_used: Some(resolved_model_clone.clone()),
                provider_id: None,
            };
            dump_llm_io(&resolved_category, &resolved_model_clone, &messages_vec, &llm_res);
        };

        Ok(Box::pin(output_stream))
    }
}

static PROVIDER_COOLDOWNS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> = std::sync::OnceLock::new();

fn get_cooldowns_map() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
    PROVIDER_COOLDOWNS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// プロバイダごとのクールダウン残り時間を取得する。
pub fn provider_cooldown_remaining(provider: &str) -> Option<std::time::Duration> {
    let lock = get_cooldowns_map().lock().unwrap();
    if let Some(&reset_instant) = lock.get(provider) {
        let now = std::time::Instant::now();
        if now < reset_instant {
            return Some(reset_instant - now);
        }
    }
    None
}

/// プロバイダごとのクールダウンを設定する。
pub fn set_provider_cooldown(provider: &str, dur: std::time::Duration) {
    let mut lock = get_cooldowns_map().lock().unwrap();
    lock.insert(provider.to_string(), std::time::Instant::now() + dur + std::time::Duration::from_secs(2));
}

/// グローバルなクールダウンの残り時間を取得する（全プロバイダ中の最大残り時間）。
/// 後方互換性およびダッシュボード表示のために維持。
pub fn global_cooldown_remaining() -> Option<std::time::Duration> {
    let lock = get_cooldowns_map().lock().unwrap();
    let now = std::time::Instant::now();
    let mut max_dur = None;
    for reset_instant in lock.values() {
        if now < *reset_instant {
            let rem = *reset_instant - now;
            if max_dur.is_none() || rem > max_dur.unwrap() {
                max_dur = Some(rem);
            }
        }
    }
    max_dur
}

pub struct GmnCliProvider {
    #[allow(dead_code)]
    config: Config,
}

impl GmnCliProvider {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// メッセージ群を gmn 用のフラットなプロンプトテキストにマージする
    fn build_prompt(&self, messages: &[Message]) -> String {
        let mut prompt = String::new();
        for msg in messages {
            match msg.role.as_str() {
                "system" => prompt.push_str(&format!("# System Context\n{}\n\n", msg.content)),
                "user" => prompt.push_str(&format!("User: {}\n\n", msg.content)),
                "assistant" => prompt.push_str(&format!("Assistant: {}\n\n", msg.content)),
                _ => prompt.push_str(&format!("{}: {}\n\n", msg.role, msg.content)),
            }
        }
        prompt.push_str("Assistant: ");
        prompt
    }
}

#[async_trait]
impl LlmProvider for GmnCliProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<LlmResponse, ProviderError> {
        if !tools.is_empty() {
            return Err(ProviderError::ExecutionFailed("GmnCliProvider does not support tool calling".to_string()));
        }
        let prompt = self.build_prompt(messages);
        
        tracing::debug!(
            model = %opts.model,
            timeout_secs = opts.timeout.as_secs(),
            prompt_len = prompt.len(),
            prompt_head = %&prompt[..prompt.len().min(200)],
            "gmn spawn: args"
        );
        tracing::trace!(prompt = %prompt, "gmn spawn: full prompt");

        let mut child = tokio::process::Command::new("gmn")
            .env("GMN_MAX_RETRIES", "0")
            .arg("-m")
            .arg(&opts.model)
            .arg("-t")
            .arg(format!("{}s", opts.timeout.as_secs()))
            .arg("--no-agent") // RustyClaw が自前でループ制御するためエージェントモード無効化
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::ExecutionFailed(format!("Failed to spawn gmn CLI process: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(prompt.as_bytes()).await
                .map_err(|e| ProviderError::ExecutionFailed(format!("Failed to write prompt to gmn CLI stdin: {}", e)))?;
            drop(stdin);
        }

        let output = child.wait_with_output().await
            .map_err(|e| ProviderError::ExecutionFailed(format!("Error waiting for gmn CLI execution: {}", e)))?;

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);

        tracing::debug!(
            status = ?output.status.code(),
            stdout_len = stdout_str.len(),
            stderr_len = stderr_str.len(),
            stdout_head = %&stdout_str[..stdout_str.len().min(200)],
            "gmn exit: response"
        );
        tracing::trace!(stdout = %stdout_str, stderr = %stderr_str, "gmn exit: full output");

        let combined_err = format!("{}\n{}", stdout_str, stderr_str);

        if !output.status.success() || combined_err.contains("quota") || combined_err.contains("RESOURCE_EXHAUSTED") || combined_err.contains("429") || combined_err.contains("rate limited") {
            if combined_err.contains("quota") || combined_err.contains("RESOURCE_EXHAUSTED") || combined_err.contains("429") || combined_err.contains("rate limited") {
                let err = ProviderError::RateLimit(combined_err);
                set_provider_cooldown_from_error("gmn", &err);
                return Err(err);
            }
            if !output.status.success() {
                return Err(ProviderError::ExecutionFailed(combined_err));
            }
        }

        let content = String::from_utf8(output.stdout)
            .map_err(|e| ProviderError::ExecutionFailed(format!("Failed to parse gmn CLI output as UTF-8: {}", e)))?;

        let mut final_content = String::new();
        let mut raw_lines = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            raw_lines.push(line);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if val["type"] == "content" {
                    if let Some(text) = val["text"].as_str() {
                        final_content.push_str(text);
                    }
                }
            } else {
                final_content.push_str(line);
                final_content.push('\n');
            }
        }

        if final_content.trim().is_empty() && !raw_lines.is_empty() {
            final_content = raw_lines.join("\n");
        }

        Ok(LlmResponse {
            content: final_content.trim().to_string(),
            role: "assistant".to_string(),
            tool_calls: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            model_used: None,
            provider_id: None,
        })
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>, ProviderError> {
        let prompt = self.build_prompt(messages);
        let model = opts.model.clone();
        let timeout_secs = opts.timeout.as_secs();

        tracing::debug!(
            model = %model,
            timeout_secs = timeout_secs,
            prompt_len = prompt.len(),
            prompt_head = %&prompt[..prompt.len().min(200)],
            "gmn spawn (stream): args"
        );
        tracing::trace!(prompt = %prompt, "gmn spawn (stream): full prompt");

        let output_stream = async_stream::try_stream! {
            let mut child = tokio::process::Command::new("gmn")
                .env("GMN_MAX_RETRIES", "0")
                .arg("-m")
                .arg(&model)
                .arg("-t")
                .arg(format!("{}s", timeout_secs))
                .arg("--no-agent") // RustyClaw が自前でループ制御するためエージェントモード無効化
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| ProviderError::ExecutionFailed(format!("Failed to spawn gmn CLI process for streaming: {}", e)))?;

            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(prompt.as_bytes()).await
                    .map_err(|e| ProviderError::ExecutionFailed(format!("Failed to write prompt to gmn CLI stdin for streaming: {}", e)))?;
                drop(stdin);
            }

            let stdout = child.stdout.take()
                .ok_or_else(|| ProviderError::ExecutionFailed("Failed to capture gmn CLI stdout".to_string()))?;
            let stderr = child.stderr.take()
                .ok_or_else(|| ProviderError::ExecutionFailed("Failed to capture gmn CLI stderr".to_string()))?;

            let mut reader = tokio::io::BufReader::new(stdout);
            let mut buffer = [0u8; 1024];
            let mut line_buffer = String::new();
            let mut has_yielded = false;
            let mut rate_limit_error = None;

            loop {
                use tokio::io::AsyncReadExt;
                let n = reader.read(&mut buffer).await
                    .map_err(|e| ProviderError::ExecutionFailed(format!("Error reading byte chunk from gmn CLI stdout: {}", e)))?;
                if n == 0 {
                    break;
                }
                let text = std::str::from_utf8(&buffer[..n])
                    .map_err(|e| ProviderError::ExecutionFailed(format!("Failed to decode gmn CLI stdout as UTF-8: {}", e)))?;
                
                line_buffer.push_str(text);

                while let Some(pos) = line_buffer.find('\n') {
                    let line = line_buffer[..pos].trim().to_string();
                    line_buffer = line_buffer[pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                        if val["type"] == "content" {
                            if let Some(content_text) = val["text"].as_str() {
                                has_yielded = true;
                                yield StreamChunk {
                                    content: content_text.to_string(),
                                };
                            }
                        } else if val["type"] == "error" {
                            if let Some(err_str) = val["error"].as_str() {
                                if err_str.contains("quota") || err_str.contains("RESOURCE_EXHAUSTED") || err_str.contains("429") || err_str.contains("rate limited") {
                                    rate_limit_error = Some(err_str.to_string());
                                    break;
                                }
                            }
                        }
                    } else {
                        if line.contains("quota") || line.contains("RESOURCE_EXHAUSTED") || line.contains("429") || line.contains("rate limited") {
                            rate_limit_error = Some(line.clone());
                            break;
                        }
                        has_yielded = true;
                        yield StreamChunk {
                            content: line,
                        };
                    }
                }
                if rate_limit_error.is_some() {
                    break;
                }
            }

            let status = child.wait().await
                .map_err(|e| ProviderError::ExecutionFailed(format!("Error waiting for gmn CLI process to exit: {}", e)))?;

            let mut stderr_content = String::new();
            let mut stderr = stderr;
            let mut err_buf = String::new();
            if tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut err_buf).await.is_ok() {
                stderr_content = err_buf;
            }

            tracing::debug!(
                status = ?status.code(),
                has_yielded = has_yielded,
                stderr_len = stderr_content.len(),
                stderr_head = %&stderr_content[..stderr_content.len().min(200)],
                "gmn exit (stream): response"
            );
            tracing::trace!(stderr = %stderr_content, "gmn exit (stream): full stderr");

            let combined_err = if let Some(ref e) = rate_limit_error {
                format!("{}\n{}", e, stderr_content)
            } else {
                stderr_content.clone()
            };

            let is_rate_limit = combined_err.contains("quota") || combined_err.contains("RESOURCE_EXHAUSTED") || combined_err.contains("429") || combined_err.contains("rate limited");

            if (!status.success() || is_rate_limit) && !has_yielded {
                if is_rate_limit {
                    let err = ProviderError::RateLimit(combined_err);
                    set_provider_cooldown_from_error("gmn", &err);
                    Err(err)?;
                } else {
                    Err(ProviderError::ExecutionFailed(format!("gmn CLI process exited with non-zero status: {:?}", status.code())))?;
                }
            }
        };

        Ok(Box::pin(output_stream))
    }
}

/// デバッグ用 No-op プロバイダ（--no-agent / RUSTYCLAW_NO_AGENT=1 時に使用）
/// API への送信は行わず、受信したメッセージ内容をログ出力してダミー応答を返す
pub struct NoopProvider;

#[async_trait]
impl LlmProvider for NoopProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        _opts: &CompletionOptions,
    ) -> std::result::Result<LlmResponse, ProviderError> {
        Self::dump(messages, tools);
        Ok(LlmResponse {
            role: "assistant".to_string(),
            content: "[NO-AGENT] Debug mode. No API call made.".to_string(),
            tool_calls: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            model_used: None,
            provider_id: None,
        })
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        _opts: &CompletionOptions,
    ) -> std::result::Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>, ProviderError> {
        Self::dump(messages, tools);
        let chunk = StreamChunk { content: "[NO-AGENT] Debug mode. No API call made.".to_string() };
        let stream = futures_util::stream::once(async move { Ok(chunk) });
        Ok(Box::pin(stream))
    }
}

impl NoopProvider {
    fn dump(messages: &[Message], tools: &[ToolDef]) {
        tracing::info!("[NO-AGENT] Would send {} message(s), {} tool(s)", messages.len(), tools.len());
        for (i, m) in messages.iter().enumerate() {
            let preview = m.content.chars().take(200).collect::<String>();
            let ellipsis = if m.content.len() > 200 { "…" } else { "" };
            tracing::info!("[NO-AGENT] messages[{}] role={} | {}{}", i, m.role, preview, ellipsis);
        }
        if !tools.is_empty() {
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            tracing::info!("[NO-AGENT] tools: {}", names.join(", "));
        }
    }
}

/// 解決済み LlmModelConfig から LLM プロバイダを生成するファクトリ
/// RUSTYCLAW_NO_AGENT=1 が設定されている場合は NoopProvider を返す
pub fn create_provider(model: LlmModelConfig) -> Box<dyn LlmProvider> {
    if std::env::var("RUSTYCLAW_NO_AGENT").as_deref() == Ok("1") {
        tracing::info!("[NO-AGENT] Debug mode active — API calls suppressed");
        return Box::new(NoopProvider);
    }
    Box::new(OpenAiCompatProvider::new(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::io::{AsyncWriteExt, BufWriter};

    #[tokio::test]
    async fn test_openai_compat_complete() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        
        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"Hello response!\"\
                        }\
                    }]\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let model = LlmModelConfig {
            model_purpose: "default".to_string(),
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
        };
        let provider = OpenAiCompatProvider::new(model);
        let opts = CompletionOptions {
            model: "gpt-4o-mini".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
            category: None,
        };

        let resp = provider.complete(&[], &[], &opts).await?;
        assert_eq!(resp.content, "Hello response!");
        assert_eq!(resp.role, "assistant");

        let _ = server_task.await;
        Ok(())
    }

    #[tokio::test]
    async fn test_openai_compat_complete_stream() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        
        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut writer = BufWriter::new(&mut socket);
                let response_header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
                let _ = writer.write_all(response_header.as_bytes()).await;
                let _ = writer.flush().await;

                let chunks = vec![
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
                    "data: [DONE]\n\n",
                ];

                for chunk in chunks {
                    let _ = writer.write_all(chunk.as_bytes()).await;
                    let _ = writer.flush().await;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                let _ = writer.shutdown().await;
            }
        });

        let model = LlmModelConfig {
            model_purpose: "default".to_string(),
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
        };
        let provider = OpenAiCompatProvider::new(model);
        let opts = CompletionOptions {
            model: "gpt-4o-mini".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
            category: None,
        };

        let stream_res = provider.complete_stream(&[], &[], &opts).await?;
        let mut chunks = Vec::new();
        let mut pin_stream = stream_res;
        while let Some(chunk_res) = pin_stream.next().await {
            let chunk = chunk_res?;
            chunks.push(chunk.content);
        }

        assert_eq!(chunks.join(""), "Hello world");

        let _ = server_task.await;
        Ok(())
    }

    #[tokio::test]
    async fn test_gmn_cli_fallback_raw_output() -> anyhow::Result<()> {
        use std::fs::File;
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Create a temporary directory for the dummy `gmn` CLI
        let temp_dir = tempfile::tempdir()?;
        let dummy_gmn_path = temp_dir.path().join("gmn");

        // Write a mock `gmn` script that outputs JSON without any `type == "content"` lines
        let mut f = File::create(&dummy_gmn_path)?;
        f.write_all(b"#!/bin/sh\ncat > /dev/null\necho '{\"type\": \"tool_use\", \"name\": \"do_something\"}'\n")?;
        f.flush()?;
        drop(f);

        // Make the script executable
        let mut perms = std::fs::metadata(&dummy_gmn_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dummy_gmn_path, perms)?;

        // Prepend temp_dir to PATH
        let old_path = std::env::var("PATH").unwrap_or_default();
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", temp_dir.path().to_string_lossy(), old_path));
        }

        let provider = GmnCliProvider::new(Config::default());
        let opts = CompletionOptions {
            model: "gemini-2.5-pro".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
            category: None,
        };

        let result = provider.complete(&[], &[], &opts).await;

        // Restore PATH
        unsafe {
            std::env::set_var("PATH", old_path);
        }

        let resp = result?;
        assert_eq!(resp.content, "{\"type\": \"tool_use\", \"name\": \"do_something\"}");
        assert_eq!(resp.role, "assistant");

        Ok(())
    }

    #[tokio::test]
    async fn test_openai_compat_complete_with_tools() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        
        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"choices\": [{\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"Hello, using tool now.\",\
                            \"tool_calls\": [{\
                                \"id\": \"call_xyz\",\
                                \"type\": \"function\",\
                                \"function\": {\
                                    \"name\": \"add\",\
                                    \"arguments\": \"{\\\"a\\\": 2, \\\"b\\\": 3}\"\
                                }\
                            }]\
                        }\
                    }]\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let model = LlmModelConfig {
            model_purpose: "default".to_string(),
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
        };
        let provider = OpenAiCompatProvider::new(model);
        let opts = CompletionOptions {
            model: "gpt-4o-mini".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
            category: None,
        };

        let tool = ToolDef {
            name: "add".to_string(),
            description: "Adds numbers".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                }
            }),
        };

        let resp = provider.complete(&[], &[tool], &opts).await?;
        assert_eq!(resp.content, "Hello, using tool now.");
        assert_eq!(resp.role, "assistant");
        
        let calls = resp.tool_calls.expect("Should contain tool calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_xyz");
        assert_eq!(calls[0].function.name, "add");
        assert_eq!(calls[0].function.arguments, "{\"a\": 2, \"b\": 3}");

        let _ = server_task.await;
        Ok(())
    }

    #[tokio::test]
    async fn test_gmn_cli_complete_with_tools_fails() -> anyhow::Result<()> {
        let provider = GmnCliProvider::new(Config::default());
        let opts = CompletionOptions {
            model: "gemini-2.5-pro".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
            category: None,
        };

        let tool = ToolDef {
            name: "add".to_string(),
            description: "Adds numbers".to_string(),
            parameters: serde_json::json!({}),
        };

        let result = provider.complete(&[], &[tool], &opts).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ProviderError::ExecutionFailed(msg) => {
                assert!(msg.contains("does not support tool calling"));
            }
            other => panic!("Expected ExecutionFailed error, got: {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn dump_llm_io_writes_dated_file_and_cleans_old_dirs() {
        use std::fs;
        use chrono::{Local, Duration};

        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", tmp.path().to_str().unwrap()); }

        // Create a dummy folder 6 days old (should be deleted)
        let old_date = (Local::now() - Duration::days(6)).format("%Y-%m-%d").to_string();
        let old_dir = tmp.path().join("memory/debug/llm/tools").join(&old_date);
        fs::create_dir_all(&old_dir).unwrap();

        let messages = vec![];
        let response = LlmResponse {
            content: "test".into(),
            role: "assistant".into(),
            tool_calls: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            model_used: None,
            provider_id: None,
        };
        dump_llm_io("tools", "test-model", &messages, &response);

        // Today's dir should contain exactly one JSON file
        let today = Local::now().format("%Y-%m-%d").to_string();
        let today_dir = tmp.path().join("memory/debug/llm/tools").join(&today);
        let files: Vec<_> = fs::read_dir(&today_dir).unwrap().collect();
        assert_eq!(files.len(), 1);
        let filename = files[0].as_ref().unwrap().file_name();
        assert!(filename.to_str().unwrap().ends_with(".json"));

        // 6-day-old folder must be deleted
        assert!(!old_dir.exists());

        unsafe { std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR"); }
    }

    #[test]
    fn test_provider_error_reset_after() {
        let err = ProviderError::RateLimit("Error: Your Quota will reset after 56s. Please try again.".to_string());
        assert_eq!(err.reset_after(), Some(std::time::Duration::from_secs(56)));

        let err2 = ProviderError::RateLimit("Your Quota will reset after 5s".to_string());
        assert_eq!(err2.reset_after(), Some(std::time::Duration::from_secs(5)));

        // 小文字 "quota" パターン (実際のログで検出されたもの)
        let err_real = ProviderError::RateLimit("You have exhausted your capacity on this model. Your quota will reset after 2s.".to_string());
        assert_eq!(err_real.reset_after(), Some(std::time::Duration::from_secs(2)));

        // メッセージ後半に "model" や "gemini-3-flash-preview" 等の 'm' が含まれるが、
        // 秒数のみ表記であるため誤認識せずに正確に 48 秒とパースできるか検証するテストケース
        let err_with_model_word = ProviderError::RateLimit("You have exhausted your capacity on this model. Your quota will reset after 48s. details: model: gemini-3-flash-preview".to_string());
        assert_eq!(err_with_model_word.reset_after(), Some(std::time::Duration::from_secs(48)));

        let err3 = ProviderError::RateLimit("Your Quota will reset after 1m 30s.".to_string());
        assert_eq!(err3.reset_after(), Some(std::time::Duration::from_secs(90)));

        let err4 = ProviderError::RateLimit("Your Quota will reset after 2m".to_string());
        assert_eq!(err4.reset_after(), Some(std::time::Duration::from_secs(120)));

        let err5 = ProviderError::RateLimit("Your Quota will reset after 3m 5s".to_string());
        assert_eq!(err5.reset_after(), Some(std::time::Duration::from_secs(185)));

        let err_none = ProviderError::RateLimit("Some other quota error without reset time".to_string());
        assert_eq!(err_none.reset_after(), None);

        let err_exec = ProviderError::ExecutionFailed("Your Quota will reset after 10s".to_string());
        assert_eq!(err_exec.reset_after(), None);

        // Cloudflare daily limit parsing test (簡易文字列)
        let err_cf = ProviderError::RateLimit("you have used up your daily free allocation of 10,000 neurons".to_string());
        assert!(err_cf.reset_after().is_some());
        assert!(err_cf.reset_after().unwrap().as_secs() > 0);

        // Cloudflare 実際の HTTP レスポンスボディ (internalCode: 4006 の JSON 全文)
        let err_cf_real = ProviderError::RateLimit(r#"{"name":"AiError","internalCode":4006,"httpCode":429,"message":"AiError: AiError: you have used up your daily free allocation of 10,000 neurons, please upgrade to Cloudflare's Workers Paid plan if you would like to continue usage. (35c8551a-c0e6-4dd5-8f67-2ce2f9a112d6)","description":"you have used up your daily free allocation of 10,000 neurons, please upgrade to Cloudflare's Workers Paid plan if you would like to continue usage.","requestId":"35c8551a-c0e6-4dd5-8f67-2ce2f9a112d6"}"#.to_string());
        assert!(err_cf_real.reset_after().is_some(), "CF JSON body must be parseable");
        // 翌 9:00 JST リセット想定なので最低 1 秒以上の待機時間が返ること
        assert!(err_cf_real.reset_after().unwrap().as_secs() >= 1, "CF daily limit must return meaningful wait duration");

        // set_global_cooldown_from_error がクールダウンをセットする
        {
            let mut lock = get_cooldowns_map().lock().unwrap();
            lock.clear();
        }
        assert!(global_cooldown_remaining().is_none());
        set_global_cooldown_from_error(&ProviderError::RateLimit("Too Many Requests".to_string()));
        assert!(global_cooldown_remaining().is_some(), "global_cooldown_remaining must be set after rate limit");

        // Cloudflare RPM 429 (JSON body, no reset time) → default 60s
        let err_cf_rpm = ProviderError::RateLimit(r#"{"errors":[{"code":10014,"message":"Too Many Requests"}]}"#.to_string());
        assert_eq!(err_cf_rpm.reset_after(), Some(std::time::Duration::from_secs(60)));

        // Cloudflare RPM 429 (plain string)
        let err_cf_rpm2 = ProviderError::RateLimit("Too Many Requests".to_string());
        assert_eq!(err_cf_rpm2.reset_after(), Some(std::time::Duration::from_secs(60)));

        // Cloudflare RPM 429 (lowercase)
        let err_cf_rpm3 = ProviderError::RateLimit("too many requests".to_string());
        assert_eq!(err_cf_rpm3.reset_after(), Some(std::time::Duration::from_secs(60)));
    }
}

fn record_neuron_usage(neurons: f64) {
    {
        let neuron_path = get_app_dir().join("neuron_usage.json");
        let today_utc = chrono::Utc::now().format("%Y-%m-%d").to_string();
        
        let mut neurons_used = 0.0;
        let mut last_reset_date = today_utc.clone();
        
        if neuron_path.exists() {
            if let Ok(file) = std::fs::File::open(&neuron_path) {
                if let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file) {
                    if let Some(date_str) = json.get("last_reset_date").and_then(|v| v.as_str()) {
                        if date_str == today_utc {
                            neurons_used = json.get("neurons_used").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            last_reset_date = date_str.to_string();
                        }
                    }
                }
            }
        }
        
        neurons_used += neurons;
        
        let updated_json = serde_json::json!({
            "last_reset_date": last_reset_date,
            "neurons_used": neurons_used
        });
        if let Ok(serialized) = serde_json::to_string_pretty(&updated_json) {
            let _ = std::fs::create_dir_all(neuron_path.parent().unwrap());
            let _ = std::fs::write(&neuron_path, serialized);
        }
    }
}

pub fn get_neuron_stats() -> serde_json::Value {
    let today_utc = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut neurons_used = 0.0;
    
    {
        let neuron_path = get_app_dir().join("neuron_usage.json");
        if neuron_path.exists() {
            if let Ok(file) = std::fs::File::open(&neuron_path) {
                if let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file) {
                    if let Some(date_str) = json.get("last_reset_date").and_then(|v| v.as_str()) {
                        if date_str == today_utc {
                            neurons_used = json.get("neurons_used").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        }
                    }
                }
            }
        }
    }
    
    let now = chrono::Local::now();
    let mut next_reset = chrono::Local::now()
        .date_naive()
        .and_hms_opt(9, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap();
    if now >= next_reset {
        next_reset = next_reset + chrono::Duration::days(1);
    }
    
    let remaining_secs = next_reset.signed_duration_since(now).num_seconds();
    let hours = remaining_secs / 3600;
    let minutes = (remaining_secs % 3600) / 60;
    let reset_in = format!("{}h {}m", hours, minutes);

    serde_json::json!({
        "neurons_used": neurons_used,
        "quota_limit": 10000.0,
        "remaining": (10000.0 - neurons_used).max(0.0),
        "reset_in": reset_in,
        "next_reset_jst": next_reset.format("%Y-%m-%d %H:%M:%S").to_string()
    })
}
