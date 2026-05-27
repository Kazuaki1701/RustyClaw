use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use rustyclaw_config::Config;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    ) -> Result<LlmResponse>;

    async fn complete_stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>>;
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
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    role: String,
    content: Option<String>,
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
        _tools: &[ToolDef], // Phase 1 ではツール実行はスキップ
        opts: &CompletionOptions,
    ) -> Result<LlmResponse> {
        let url = format!("{}/chat/completions", self.config.api_base_url);
        
        let request_body = OpenAiRequest {
            model: &opts.model,
            messages,
            max_tokens: opts.max_tokens.or(self.config.max_tokens),
            temperature: opts.temperature.or(self.config.temperature),
            stream: None,
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
            return Err(anyhow::anyhow!("LLM API returned error status {}: {}", status, error_text));
        }

        let resp_data: OpenAiResponse = response.json()
            .await
            .context("Failed to parse LLM JSON response")?;

        let choice = resp_data.choices.first()
            .context("LLM returned empty choices in response")?;

        Ok(LlmResponse {
            content: choice.message.content.clone().unwrap_or_default(),
            role: choice.message.role.clone(),
        })
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        let url = format!("{}/chat/completions", self.config.api_base_url);
        
        let request_body = OpenAiRequest {
            model: &opts.model,
            messages,
            max_tokens: opts.max_tokens.or(self.config.max_tokens),
            temperature: opts.temperature.or(self.config.temperature),
            stream: Some(true),
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
            return Err(anyhow::anyhow!("LLM Stream API returned error status {}: {}", status, error_text));
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

fn parse_reset_seconds(stderr: &str) -> Option<u64> {
    if let Some(idx) = stderr.find("reset after ") {
        let sub = &stderr[idx + "reset after ".len()..];
        let digits: String = sub.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            if let Ok(secs) = digits.parse::<u64>() {
                return Some(secs);
            }
        }
    }
    None
}

#[async_trait]
impl LlmProvider for GmnCliProvider {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> Result<LlmResponse> {
        let prompt = self.build_prompt(messages);
        
        let mut retries = 3;
        loop {
            tracing::debug!(
                model = %opts.model,
                timeout_secs = opts.timeout.as_secs(),
                prompt_len = prompt.len(),
                prompt_head = %&prompt[..prompt.len().min(200)],
                "gmn spawn: args"
            );
            tracing::trace!(prompt = %prompt, "gmn spawn: full prompt");

            let child = tokio::process::Command::new("gmn")
                .env("GMN_MAX_RETRIES", "0")
                .arg("-p")
                .arg(&prompt)
                .arg("-m")
                .arg(&opts.model)
                .arg("-t")
                .arg(format!("{}s", opts.timeout.as_secs()))
                .arg("--no-agent") // RustyClaw が自前でループ制御するためエージェントモード無効化
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .context("Failed to spawn gmn CLI process")?;

            let output = child.wait_with_output().await
                .context("Error waiting for gmn CLI execution")?;

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
                    if retries > 0 {
                        let wait_secs = parse_reset_seconds(&combined_err).unwrap_or(23);
                        tracing::warn!("gmn rate-limit exceeded. Waiting {} seconds before retry...", wait_secs);
                        tokio::time::sleep(std::time::Duration::from_secs(wait_secs + 1)).await;
                        retries -= 1;
                        continue;
                    }
                    return Err(anyhow::anyhow!("gmn quota/rate-limit exceeded: {}", combined_err));
                }
                if !output.status.success() {
                    return Err(anyhow::anyhow!("gmn CLI execution failed: {}", combined_err));
                }
            }

            let content = String::from_utf8(output.stdout)
                .context("Failed to parse gmn CLI output as UTF-8")?;

            let mut final_content = String::new();
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
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

            return Ok(LlmResponse {
                content: final_content.trim().to_string(),
                role: "assistant".to_string(),
            });
        }
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
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
            let mut retries = 3;
            loop {
                let mut child = tokio::process::Command::new("gmn")
                    .env("GMN_MAX_RETRIES", "0")
                    .arg("-p")
                    .arg(&prompt)
                    .arg("-m")
                    .arg(&model)
                    .arg("-t")
                    .arg(format!("{}s", timeout_secs))
                    .arg("--no-agent") // RustyClaw が自前でループ制御するためエージェントモード無効化
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .context("Failed to spawn gmn CLI process for streaming")?;

                let stdout = child.stdout.take()
                    .context("Failed to capture gmn CLI stdout")?;
                let stderr = child.stderr.take()
                    .context("Failed to capture gmn CLI stderr")?;

                let mut reader = tokio::io::BufReader::new(stdout);
                let mut buffer = [0u8; 1024];
                let mut line_buffer = String::new();
                let mut has_yielded = false;
                let mut rate_limit_error = None;

                loop {
                    use tokio::io::AsyncReadExt;
                    let n = reader.read(&mut buffer).await
                        .context("Error reading byte chunk from gmn CLI stdout")?;
                    if n == 0 {
                        break;
                    }
                    let text = std::str::from_utf8(&buffer[..n])
                        .context("Failed to decode gmn CLI stdout as UTF-8")?;
                    
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
                    .context("Error waiting for gmn CLI process to exit")?;

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
                        if retries > 0 {
                            let wait_secs = parse_reset_seconds(&combined_err).unwrap_or(23);
                            tracing::warn!("gmn rate-limit exceeded in streaming mode. Waiting {} seconds before retry...", wait_secs);
                            tokio::time::sleep(std::time::Duration::from_secs(wait_secs + 1)).await;
                            retries -= 1;
                            let _ = child.kill().await;
                            continue;
                        }
                        Err(anyhow::anyhow!("gmn quota/rate-limit exceeded: {}", combined_err))?;
                    }
                    Err(anyhow::anyhow!("gmn CLI process exited with non-zero status: {:?}", status.code()))?;
                }
                break;
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
    async fn test_openai_compat_complete() -> Result<()> {
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
    async fn test_openai_compat_complete_stream() -> Result<()> {
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
}
