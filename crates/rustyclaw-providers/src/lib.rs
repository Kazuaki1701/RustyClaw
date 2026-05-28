use anyhow::Context;
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use rustyclaw_config::Config;
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
                None
            }
            _ => None,
        }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub role: String,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub content: String,
}

pub struct CompletionOptions {
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub timeout: Duration,
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
    config: Config,
}

impl OpenAiCompatProvider {
    pub fn new(config: Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(900)) // デフォルト15分
            .build()
            .expect("Failed to build reqwest HTTP client with rustls");
        
        Self { client, config }
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
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
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
        let url = format!("{}/chat/completions", self.config.api_base_url);
        
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

        let request_body = OpenAiRequest {
            model: &opts.model,
            messages: openai_messages,
            max_tokens: opts.max_tokens.or(self.config.max_tokens),
            temperature: opts.temperature.or(self.config.temperature),
            stream: None,
            tools: openai_tools,
        };

        tracing::info!("Sending LLM request to OpenAI compat API at {}", url);
        
        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&request_body)
            .send()
            .await
            .context("HTTP POST request failed during LLM call")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || error_text.contains("insufficient_quota") {
                return Err(ProviderError::RateLimit(error_text));
            }
            return Err(ProviderError::ExecutionFailed(format!("LLM API returned error status {}: {}", status, error_text)));
        }

        let resp_data: OpenAiResponse = response.json()
            .await
            .context("Failed to parse LLM JSON response")?;

        let choice = resp_data.choices.first()
            .context("LLM returned empty choices in response")?;

        Ok(LlmResponse {
            content: choice.message.content.clone().unwrap_or_default(),
            role: choice.message.role.clone(),
            tool_calls: choice.message.tool_calls.clone(),
        })
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>, ProviderError> {
        let url = format!("{}/chat/completions", self.config.api_base_url);
        
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

        let request_body = OpenAiRequest {
            model: &opts.model,
            messages: openai_messages,
            max_tokens: opts.max_tokens.or(self.config.max_tokens),
            temperature: opts.temperature.or(self.config.temperature),
            stream: Some(true),
            tools: None,
        };

        tracing::info!("Sending streaming LLM request to OpenAI compat API at {}", url);

        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&request_body)
            .send()
            .await
            .context("HTTP POST stream request failed during LLM call")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || error_text.contains("insufficient_quota") {
                return Err(ProviderError::RateLimit(error_text));
            }
            return Err(ProviderError::ExecutionFailed(format!("LLM Stream API returned error status {}: {}", status, error_text)));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        let output_stream = async_stream::try_stream! {
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
                                    yield StreamChunk {
                                        content: content.clone(),
                                    };
                                }
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(output_stream))
    }
}

static GLOBAL_COOLDOWN: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

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
        // グローバルなクールダウン（レート制限の解除待ち）をチェック
        {
            let cooldown = {
                let lock = GLOBAL_COOLDOWN.lock().unwrap();
                *lock
            };
            if let Some(reset_instant) = cooldown {
                let now = std::time::Instant::now();
                if now < reset_instant {
                    let wait_dur = reset_instant - now;
                    tracing::warn!(
                        "Global rate limit active. Waiting {:.1}s before spawning gmn...",
                        wait_dur.as_secs_f64()
                    );
                    tokio::time::sleep(wait_dur).await;
                }
            }
        }

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
                if let Some(dur) = err.reset_after() {
                    let mut lock = GLOBAL_COOLDOWN.lock().unwrap();
                    *lock = Some(std::time::Instant::now() + dur + std::time::Duration::from_secs(2));
                }
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
        })
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>, ProviderError> {
        // グローバルなクールダウン（レート制限の解除待ち）をチェック
        {
            let cooldown = {
                let lock = GLOBAL_COOLDOWN.lock().unwrap();
                *lock
            };
            if let Some(reset_instant) = cooldown {
                let now = std::time::Instant::now();
                if now < reset_instant {
                    let wait_dur = reset_instant - now;
                    tracing::warn!(
                        "Global rate limit active. Waiting {:.1}s before spawning gmn (stream)...",
                        wait_dur.as_secs_f64()
                    );
                    tokio::time::sleep(wait_dur).await;
                }
            }
        }

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
                    if let Some(dur) = err.reset_after() {
                        let mut lock = GLOBAL_COOLDOWN.lock().unwrap();
                        *lock = Some(std::time::Instant::now() + dur + std::time::Duration::from_secs(2));
                    }
                    Err(err)?;
                } else {
                    Err(ProviderError::ExecutionFailed(format!("gmn CLI process exited with non-zero status: {:?}", status.code())))?;
                }
            }
        };

        Ok(Box::pin(output_stream))
    }
}

/// 設定に基づいて LLM プロバイダの具象インスタンスを生成するファクトリ
pub fn create_provider(config: Config) -> Box<dyn LlmProvider> {
    if config.model_provider == "openai" {
        Box::new(OpenAiCompatProvider::new(config))
    } else {
        Box::new(GmnCliProvider::new(config))
    }
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

        let config = Config {
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
            mcp: std::collections::HashMap::new(),
        };

        let provider = OpenAiCompatProvider::new(config);
        let opts = CompletionOptions {
            model: "gpt-4o-mini".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
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

        let config = Config {
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
            mcp: std::collections::HashMap::new(),
        };

        let provider = OpenAiCompatProvider::new(config);
        let opts = CompletionOptions {
            model: "gpt-4o-mini".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
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

        // Create provider and call complete
        let config = Config {
            model_provider: "gmn".to_string(),
            model_name: "gemini-2.5-pro".to_string(),
            api_key: "".to_string(),
            api_base_url: "".to_string(),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
            mcp: std::collections::HashMap::new(),
        };
        let provider = GmnCliProvider::new(config);
        let opts = CompletionOptions {
            model: "gemini-2.5-pro".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
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

        let config = Config {
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
            mcp: std::collections::HashMap::new(),
        };

        let provider = OpenAiCompatProvider::new(config);
        let opts = CompletionOptions {
            model: "gpt-4o-mini".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
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
        let config = Config {
            model_provider: "gmn".to_string(),
            model_name: "gemini-2.5-pro".to_string(),
            api_key: "".to_string(),
            api_base_url: "".to_string(),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
            mcp: std::collections::HashMap::new(),
        };
        let provider = GmnCliProvider::new(config);
        let opts = CompletionOptions {
            model: "gemini-2.5-pro".to_string(),
            max_tokens: None,
            temperature: None,
            timeout: Duration::from_secs(5),
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
    }
}
