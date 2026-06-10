use anyhow::Context;
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use rustyclaw_config::{Config, LlmModelConfig, get_app_dir};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rig_core::OneOrMany;
use rig_core::client::CompletionClient;
use rig_core::completion::CompletionModel;
use rig_core::completion::CompletionRequest;

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
                if lower.contains("used up your daily free allocation")
                    || lower.contains("10,000 neurons")
                {
                    let now = chrono::Local::now();
                    let mut next_reset = chrono::Local::now()
                        .date_naive()
                        .and_hms_opt(9, 0, 0)
                        .unwrap()
                        .and_local_timezone(chrono::Local)
                        .unwrap();
                    if now >= next_reset {
                        next_reset += chrono::Duration::days(1);
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
                    let limit_part = if let Some(end_pos) =
                        sub.find(['.', '\n', '"', '\\'])
                    {
                        &sub[..end_pos]
                    } else {
                        sub
                    };

                    let mut total_secs = 0;
                    let mut has_parsed = false;

                    if let Some(m_pos) = limit_part.find('m') {
                        // 'm' の前の数字をパース (分)
                        let m_str = limit_part[..m_pos].trim();
                        let m_num_str: String =
                            m_str.chars().rev().take_while(|c| c.is_numeric()).collect();
                        let m_num_str: String = m_num_str.chars().rev().collect();
                        if let Ok(mins) = m_num_str.parse::<u64>() {
                            total_secs += mins * 60;
                            has_parsed = true;
                        }

                        // 'm' の後ろから 's' の間の数字をパース (秒)
                        let remaining = &limit_part[m_pos + 1..];
                        if let Some(s_pos) = remaining.find('s') {
                            let s_str = remaining[..s_pos].trim();
                            let s_num_str: String =
                                s_str.chars().take_while(|c| c.is_numeric()).collect();
                            if let Ok(secs) = s_num_str.parse::<u64>() {
                                total_secs += secs;
                            }
                        }
                    } else if let Some(s_pos) = limit_part.find('s') {
                        // 's' の前の数字をパース (秒のみ)
                        let s_str = limit_part[..s_pos].trim();
                        let s_num_str: String =
                            s_str.chars().rev().take_while(|c| c.is_numeric()).collect();
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
    // config_name（config.json の model_name キー）のプレフィックスで分類する。
    // config_name が空の場合は api_base_url にフォールバック。
    let name = model.config_name.to_lowercase();
    if name.starts_with("lms-") {
        return "local".to_string();
    } else if name.starts_with("cf-") {
        return "cloudflare".to_string();
    } else if name.starts_with("groq-") {
        return "groq".to_string();
    } else if name.starts_with("or-") {
        return "openrouter".to_string();
    } else if name.starts_with("hf-") {
        return "huggingface".to_string();
    } else if name.starts_with("gmn") {
        return "gmn".to_string();
    }
    // フォールバック: api_base_url で判別
    let base = model.api_base_url.to_lowercase();
    if base.contains("groq.com") {
        "groq".to_string()
    } else if base.contains("cloudflare.com") {
        "cloudflare".to_string()
    } else if base.contains("openrouter.ai") {
        "openrouter".to_string()
    } else if base.contains("openai.com") {
        "openai".to_string()
    } else if base.contains("huggingface.co") {
        "huggingface".to_string()
    } else if base.contains("192.168.") || base.contains("localhost") || base.contains("127.0.0.1")
    {
        "local".to_string()
    } else {
        model.model_provider.clone()
    }
}

/// RateLimit エラーからプロバイダごとのクールダウンを設定する共通ヘルパー。
pub fn set_provider_cooldown_from_error(provider: &str, err: &ProviderError) {
    let dur = err
        .reset_after()
        .unwrap_or(std::time::Duration::from_secs(60));
    set_provider_cooldown(provider, dur);
}

/// 後方互換性のためのグローバルクールダウン設定ヘルパー。
pub fn set_global_cooldown_from_error(err: &ProviderError) {
    let dur = err
        .reset_after()
        .unwrap_or(std::time::Duration::from_secs(60));
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

fn dump_llm_io(category: &str, model: &str, messages: &[Message], response: &LlmResponse) {
    use chrono::Local;

    let ws_dir = get_workspace_dir();
    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H-%M-%S").to_string();

    let category_dir = ws_dir
        .join("memory")
        .join("debug")
        .join("llm")
        .join(category);
    let date_dir = category_dir.join(&date_str);

    if let Err(e) = std::fs::create_dir_all(&date_dir) {
        tracing::error!("Failed to create llm dump directory {:?}: {}", date_dir, e);
        return;
    }

    // Delete date folders older than 5 days
    // Spec: "5日超" = older than 5 days → retain today + 5 prior days (6 days total)
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

    // ── last_request.json (コンパクト版) ──────────────────────────────
    const CONTENT_PREVIEW_CHARS: usize = 500;
    let preview_str = |s: &str| -> String {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() <= CONTENT_PREVIEW_CHARS {
            s.to_string()
        } else {
            format!(
                "{}…[省略: 全 {} 文字中 先頭 {} 文字]",
                chars[..CONTENT_PREVIEW_CHARS].iter().collect::<String>(),
                chars.len(),
                CONTENT_PREVIEW_CHARS
            )
        }
    };

    let compact_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content_preview": preview_str(&m.content),
            })
        })
        .collect();

    let compact = serde_json::json!({
        "timestamp": now.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "category": category,
        "model": model,
        "message_count": messages.len(),
        "messages": compact_messages,
        "response_preview": preview_str(&response.content),
    });

    if let Ok(json_str) = serde_json::to_string_pretty(&compact) {
        let last_req_path = ws_dir
            .join("memory")
            .join("debug")
            .join("llm")
            .join("last_request.json");
        if let Some(parent) = last_req_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&last_req_path, &json_str) {
            tracing::warn!("last_request.json の書き込みに失敗: {}", e);
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
    ) -> std::result::Result<
        Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>,
        ProviderError,
    >;
}

pub struct OpenAiCompatProvider {
    client: rig_core::providers::openai::CompletionsClient,
    model: LlmModelConfig,
}

impl OpenAiCompatProvider {
    pub fn new(model: LlmModelConfig) -> Self {
        use reqwest::header::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();

        if let Some(gateway_id) = &model.cf_aig_gateway_id {
            if let Ok(val) = HeaderValue::from_str(gateway_id) {
                headers.insert("cf-aig-gateway-id", val);
            }
            if model.api_base_url.contains("gateway.ai.cloudflare.com") {
                let auth_header = format!("Bearer {}", model.api_key);
                if let Ok(val) = HeaderValue::from_str(&auth_header) {
                    headers.insert("cf-aig-authorization", val);
                }
            }
        } else if model.api_base_url.contains("gateway.ai.cloudflare.com") {
            let auth_header = format!("Bearer {}", model.api_key);
            if let Ok(val) = HeaderValue::from_str(&auth_header) {
                headers.insert("cf-aig-authorization", val);
            }
        }

        let reqwest_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(900))
            .default_headers(headers)
            .build()
            .expect("Failed to build reqwest HTTP client with rustls");

        let client = rig_core::providers::openai::CompletionsClient::builder()
            .api_key(&model.api_key)
            .base_url(&model.api_base_url)
            .http_client(reqwest_client)
            .build()
            .expect("Failed to build rig CompletionsClient");

        Self { client, model }
    }

    fn resolve_model(&self, model: &str) -> String {
        if model == "flash"
            && let Ok(env_val) = std::env::var("RUSTYCLAW_FLASH_MODEL_NAME")
        {
            return env_val;
        }
        model.to_string()
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<LlmResponse, ProviderError> {
        let resolved_model = self.resolve_model(&opts.model);
        let rig_messages = provider_messages_to_rig(messages);
        let chat_history = if rig_messages.is_empty() {
            OneOrMany::one(rig_core::completion::Message::user(""))
        } else {
            OneOrMany::many(rig_messages).unwrap()
        };

        let rig_tools: Vec<rig_core::completion::ToolDefinition> = tools
            .iter()
            .map(|t| rig_core::completion::ToolDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect();

        let model = self.client.completion_model(&resolved_model);

        let request = CompletionRequest {
            preamble: None,
            chat_history,
            tools: rig_tools,
            temperature: opts.temperature.map(|t| t as f64),
            max_tokens: opts.max_tokens.map(|t| t as u64),
            documents: vec![],
            model: None,
            tool_choice: None,
            additional_params: None,
            output_schema: None,
        };

        tracing::info!("Sending LLM request to OpenAI compat API (via rig-core) for model: {}", resolved_model);

        let response = model
            .completion(request)
            .await
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("insufficient_quota") || err_str.contains("rate_limit") || err_str.contains("429") {
                    let err = ProviderError::RateLimit(err_str);
                    let provider_id = resolve_provider_id(&self.model);
                    set_provider_cooldown_from_error(&provider_id, &err);
                    err
                } else {
                    ProviderError::ExecutionFailed(err_str)
                }
            })?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();
        use rig_core::completion::message::AssistantContent;
        for c in response.choice.iter() {
            match c {
                AssistantContent::Text(t) => {
                    content.push_str(&t.text);
                }
                AssistantContent::ToolCall(tc) => {
                    tool_calls.push(ToolCall {
                        id: tc.id.clone(),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.to_string(),
                        },
                    });
                }
                _ => {}
            }
        }
        let tool_calls_opt = if tool_calls.is_empty() { None } else { Some(tool_calls) };

        let prompt_tokens = response.usage.input_tokens as u32;
        let completion_tokens = response.usage.output_tokens as u32;
        let total_tokens = response.usage.total_tokens as u32;

        if self.model.api_base_url.contains("cloudflare.com")
            && (prompt_tokens > 0 || completion_tokens > 0)
        {
            let neurons = calc_cf_neurons(&resolved_model, prompt_tokens, completion_tokens);
            tracing::info!(
                "CF Neurons calculated: {:.2} (prompt={}, completion={}, model={})",
                neurons,
                prompt_tokens,
                completion_tokens,
                resolved_model
            );
            record_neuron_usage(neurons);
        }

        let res = LlmResponse {
            content,
            role: "assistant".to_string(),
            tool_calls: tool_calls_opt,
            prompt_tokens: Some(prompt_tokens),
            completion_tokens: Some(completion_tokens),
            total_tokens: Some(total_tokens),
            model_used: Some(resolved_model.clone()),
            provider_id: Some(resolve_provider_id(&self.model)),
        };

        let mut resolved_category = opts
            .category
            .clone()
            .unwrap_or_else(|| "discord".to_string());
        if res.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty()) {
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
    ) -> std::result::Result<
        Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>,
        ProviderError,
    > {
        let resolved_model = self.resolve_model(&opts.model);
        let rig_messages = provider_messages_to_rig(messages);
        let chat_history = if rig_messages.is_empty() {
            OneOrMany::one(rig_core::completion::Message::user(""))
        } else {
            OneOrMany::many(rig_messages).unwrap()
        };

        let model = self.client.completion_model(&resolved_model);

        let request = CompletionRequest {
            preamble: None,
            chat_history,
            tools: vec![],
            temperature: opts.temperature.map(|t| t as f64),
            max_tokens: opts.max_tokens.map(|t| t as u64),
            documents: vec![],
            model: None,
            tool_choice: None,
            additional_params: None,
            output_schema: None,
        };

        tracing::info!(
            "Sending streaming LLM request to OpenAI compat API (via rig-core) for model: {}",
            resolved_model
        );

        let mut res = model
            .stream(request)
            .await
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("insufficient_quota") || err_str.contains("rate_limit") || err_str.contains("429") {
                    let err = ProviderError::RateLimit(err_str);
                    let provider_id = resolve_provider_id(&self.model);
                    set_provider_cooldown_from_error(&provider_id, &err);
                    err
                } else {
                    ProviderError::ExecutionFailed(err_str)
                }
            })?;

        let resolved_model_clone = resolved_model.clone();
        let resolved_category = opts
            .category
            .clone()
            .unwrap_or_else(|| "discord".to_string());
        let messages_vec = messages.to_vec();
        let provider_id_clone = resolve_provider_id(&self.model);

        let output_stream = async_stream::try_stream! {
            let mut full_response_content = String::new();
            use rig_core::streaming::StreamedAssistantContent;
            while let Some(chunk_res) = res.next().await {
                let chunk = chunk_res.map_err(|e| ProviderError::ExecutionFailed(e.to_string()))?;
                if let StreamedAssistantContent::Text(t) = chunk {
                    full_response_content.push_str(&t.text);
                    yield StreamChunk {
                        content: t.text.clone(),
                    };
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
                provider_id: Some(provider_id_clone.clone()),
            };
            dump_llm_io(&resolved_category, &resolved_model_clone, &messages_vec, &llm_res);
        };

        Ok(Box::pin(output_stream))
    }
}

// ==============================================================================
// Cloudflare Workers AI Embedding Client
// ==============================================================================

#[derive(Debug, serde::Deserialize)]
struct CfEmbedResult {
    data: Vec<Vec<f32>>,
}

#[derive(Debug, serde::Deserialize)]
struct CfEmbedResponse {
    result: Option<CfEmbedResult>,
    success: bool,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenAiEmbedItem {
    embedding: Vec<f32>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbedItem>,
}

/// Cloudflare Workers AI の embedding エンドポイントに POST してベクトルを返すクライアント。
/// reqwest（既存依存）のみ使用。rig-core 不要。
#[derive(Clone)]
pub struct CloudflareEmbeddingClient {
    client: reqwest::Client,
    api_endpoint: String,
    api_key: String,
}

impl CloudflareEmbeddingClient {
    pub fn new(api_endpoint: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_endpoint: api_endpoint.into(),
            api_key: api_key.into(),
        }
    }

    /// エンドポイント種別に応じたリクエストボディを構築する。
    /// `/embeddings` で終わる場合は OpenAI 互換形式（`input`）、それ以外は CF Workers AI ネイティブ形式（`text`）。
    pub(crate) fn build_embed_body(&self, texts: &[&str]) -> serde_json::Value {
        if self.api_endpoint.ends_with("/embeddings") {
            serde_json::json!({ "input": texts })
        } else {
            serde_json::json!({ "text": texts })
        }
    }

    /// テキストのスライスをベクトル化して返す。入力と同順・同数の Vec<Vec<f32>>。
    pub async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let is_openai_compat = self.api_endpoint.ends_with("/embeddings");
        let body = self.build_embed_body(texts);
        let resp = self
            .client
            .post(&self.api_endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("CF embedding: HTTP request failed")?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .context("CF embedding: failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!(
                "CF embedding: HTTP {} — {}",
                status,
                &text[..text.len().min(200)]
            );
        }

        if is_openai_compat {
            let parsed: OpenAiEmbedResponse = serde_json::from_str(&text)
                .context("CF embedding: failed to parse OpenAI response JSON")?;
            Ok(parsed.data.into_iter().map(|item| item.embedding).collect())
        } else {
            let parsed: CfEmbedResponse = serde_json::from_str(&text)
                .context("CF embedding: failed to parse response JSON")?;
            if !parsed.success {
                anyhow::bail!("CF embedding: API error — {:?}", parsed.errors);
            }
            parsed
                .result
                .map(|r| r.data)
                .ok_or_else(|| anyhow::anyhow!("CF embedding: result field is null"))
        }
    }
}

/// fastembed (ONNX Runtime) を使ったローカル埋め込みクライアント。
/// `intfloat/multilingual-e5-small` (384 次元) をローカル推論する。
/// 初回呼び出し時にモデルファイルをダウンロード・キャッシュする (~30MB)。
#[derive(Clone)]
pub struct LocalEmbeddingClient {
    // fastembed 5 の embed は &mut self を要求するため Mutex で包む
    model: Arc<Mutex<fastembed::TextEmbedding>>,
}

impl LocalEmbeddingClient {
    /// ONNX モデルを初期化する。
    /// cache_dir: モデルキャッシュ先ディレクトリ（例: `~/.rustyclaw/models/`）。
    pub fn new(cache_dir: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let model = fastembed::TextEmbedding::try_new(
            fastembed::InitOptions::new(fastembed::EmbeddingModel::MultilingualE5Small)
                .with_show_download_progress(true)
                .with_cache_dir(cache_dir.as_ref().to_path_buf()),
        )
        .map_err(|e| anyhow::anyhow!("fastembed init failed: {}", e))?;
        Ok(Self {
            model: Arc::new(Mutex::new(model)),
        })
    }

    /// テキストをベクトル化する（384 次元）。空の入力は即座に `Ok(vec![])` を返す。
    pub async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
        let model = Arc::clone(&self.model);
        tokio::task::spawn_blocking(move || {
            let mut guard = model
                .lock()
                .map_err(|e| anyhow::anyhow!("fastembed mutex poisoned: {}", e))?;
            guard
                .embed(owned, None)
                .map_err(|e| anyhow::anyhow!("fastembed embed failed: {}", e))
        })
        .await
        .context("LocalEmbeddingClient: spawn_blocking panicked")?
    }
}

/// rig-core の EmbeddingModel トレイトを実装したラッパー。
/// CloudflareEmbeddingClient（OpenAI互換 / CF Workers AI）を rig の VectorStore と統合する。
#[derive(Clone)]
pub struct CloudflareEmbeddingModel {
    client: CloudflareEmbeddingClient,
    dims: usize,
}

impl CloudflareEmbeddingModel {
    pub fn new(client: CloudflareEmbeddingClient, dims: usize) -> Self {
        Self { client, dims }
    }
}

impl rig_core::embeddings::EmbeddingModel for CloudflareEmbeddingModel {
    const MAX_DOCUMENTS: usize = 100;
    type Client = CloudflareEmbeddingClient;

    fn make(client: &Self::Client, _model: impl Into<String>, dims: Option<usize>) -> Self {
        Self::new(client.clone(), dims.unwrap_or(1024))
    }

    fn ndims(&self) -> usize {
        self.dims
    }

    async fn embed_texts(
        &self,
        texts: impl IntoIterator<Item = String> + rig_core::wasm_compat::WasmCompatSend,
    ) -> Result<Vec<rig_core::embeddings::Embedding>, rig_core::embeddings::EmbeddingError> {
        let texts: Vec<String> = texts.into_iter().collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let vecs = self
            .client
            .embed(&text_refs)
            .await
            .map_err(|e| rig_core::embeddings::EmbeddingError::ProviderError(e.to_string()))?;
        Ok(texts
            .into_iter()
            .zip(vecs)
            .map(|(doc, vec)| rig_core::embeddings::Embedding {
                document: doc,
                vec: vec.iter().map(|&x| x as f64).collect(),
            })
            .collect())
    }
}

static PROVIDER_COOLDOWNS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
> = std::sync::OnceLock::new();

fn get_cooldowns_map()
-> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
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
    lock.insert(
        provider.to_string(),
        std::time::Instant::now() + dur + std::time::Duration::from_secs(2),
    );
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
            return Err(ProviderError::ExecutionFailed(
                "GmnCliProvider does not support tool calling".to_string(),
            ));
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
            .map_err(|e| {
                ProviderError::ExecutionFailed(format!("Failed to spawn gmn CLI process: {}", e))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
                ProviderError::ExecutionFailed(format!(
                    "Failed to write prompt to gmn CLI stdin: {}",
                    e
                ))
            })?;
            drop(stdin);
        }

        let output = child.wait_with_output().await.map_err(|e| {
            ProviderError::ExecutionFailed(format!("Error waiting for gmn CLI execution: {}", e))
        })?;

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

        if !output.status.success()
            || combined_err.contains("quota")
            || combined_err.contains("RESOURCE_EXHAUSTED")
            || combined_err.contains("429")
            || combined_err.contains("rate limited")
        {
            if combined_err.contains("quota")
                || combined_err.contains("RESOURCE_EXHAUSTED")
                || combined_err.contains("429")
                || combined_err.contains("rate limited")
            {
                let err = ProviderError::RateLimit(combined_err);
                set_provider_cooldown_from_error("gmn", &err);
                return Err(err);
            }
            if !output.status.success() {
                return Err(ProviderError::ExecutionFailed(combined_err));
            }
        }

        let content = String::from_utf8(output.stdout).map_err(|e| {
            ProviderError::ExecutionFailed(format!(
                "Failed to parse gmn CLI output as UTF-8: {}",
                e
            ))
        })?;

        let mut final_content = String::new();
        let mut raw_lines = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            raw_lines.push(line);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if val["type"] == "content"
                    && let Some(text) = val["text"].as_str()
                {
                    final_content.push_str(text);
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
            provider_id: Some("gmn".to_string()),
        })
    }

    async fn complete_stream(
        &self,
        messages: &[Message],
        _tools: &[ToolDef],
        opts: &CompletionOptions,
    ) -> std::result::Result<
        Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>,
        ProviderError,
    > {
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
                        if val["type"] == "content"
                            && let Some(content_text) = val["text"].as_str()
                        {
                            has_yielded = true;
                            yield StreamChunk {
                                content: content_text.to_string(),
                            };
                        } else if val["type"] == "error"
                            && let Some(err_str) = val["error"].as_str()
                            && (err_str.contains("quota") || err_str.contains("RESOURCE_EXHAUSTED") || err_str.contains("429") || err_str.contains("rate limited"))
                        {
                            rate_limit_error = Some(err_str.to_string());
                            break;
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
    ) -> std::result::Result<
        Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>,
        ProviderError,
    > {
        Self::dump(messages, tools);
        let chunk = StreamChunk {
            content: "[NO-AGENT] Debug mode. No API call made.".to_string(),
        };
        let stream = futures_util::stream::once(async move { Ok(chunk) });
        Ok(Box::pin(stream))
    }
}

impl NoopProvider {
    fn dump(messages: &[Message], tools: &[ToolDef]) {
        tracing::info!(
            "[NO-AGENT] Would send {} message(s), {} tool(s)",
            messages.len(),
            tools.len()
        );
        for (i, m) in messages.iter().enumerate() {
            let preview = m.content.chars().take(200).collect::<String>();
            let ellipsis = if m.content.len() > 200 { "…" } else { "" };
            tracing::info!(
                "[NO-AGENT] messages[{}] role={} | {}{}",
                i,
                m.role,
                preview,
                ellipsis
            );
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

// =========================================================
// rig-core CompletionModel integration
// =========================================================

/// Converts rig-core `CompletionRequest` messages (plus optional preamble) to
/// the RustyClaw provider `Message` format.
pub fn rig_messages_to_provider<'a>(
    preamble: Option<&str>,
    history: impl IntoIterator<Item = &'a rig_core::completion::Message>,
) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();

    if let Some(sys) = preamble {
        out.push(Message {
            role: "system".to_string(),
            content: sys.to_string(),
            ..Default::default()
        });
    }

    for msg in history {
        match msg {
            rig_core::completion::Message::System { content } => {
                out.push(Message {
                    role: "system".to_string(),
                    content: content.clone(),
                    ..Default::default()
                });
            }
            rig_core::completion::Message::User { content } => {
                for item in content.iter() {
                    match item {
                        rig_core::completion::message::UserContent::Text(t) => {
                            out.push(Message {
                                role: "user".to_string(),
                                content: t.text.clone(),
                                ..Default::default()
                            });
                        }
                        rig_core::completion::message::UserContent::ToolResult(tr) => {
                            let text = tr
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    rig_core::completion::message::ToolResultContent::Text(t) => {
                                        Some(t.text.as_str())
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            out.push(Message {
                                role: "tool".to_string(),
                                content: text,
                                tool_call_id: Some(tr.id.clone()),
                                ..Default::default()
                            });
                        }
                        _ => {}
                    }
                }
            }
            rig_core::completion::Message::Assistant { content, .. } => {
                let mut text = String::new();
                let mut tcs: Vec<ToolCall> = Vec::new();
                for item in content.iter() {
                    match item {
                        rig_core::completion::message::AssistantContent::Text(t) => {
                            text.clone_from(&t.text);
                        }
                        rig_core::completion::message::AssistantContent::ToolCall(tc) => {
                            tcs.push(ToolCall {
                                id: tc.id.clone(),
                                r#type: "function".to_string(),
                                function: FunctionCall {
                                    name: tc.function.name.clone(),
                                    arguments: tc.function.arguments.to_string(),
                                },
                            });
                        }
                        _ => {}
                    }
                }
                out.push(Message {
                    role: "assistant".to_string(),
                    content: text,
                    tool_calls: if tcs.is_empty() { None } else { Some(tcs) },
                    ..Default::default()
                });
            }
        }
    }

    out
}

/// Converts a `LlmResponse` to rig-core's `CompletionResponse<LlmResponse>`.
pub fn llm_response_to_rig(
    resp: LlmResponse,
) -> rig_core::completion::CompletionResponse<LlmResponse> {
    use rig_core::OneOrMany;
    use rig_core::completion::message::{
        AssistantContent, Text, ToolCall as RigToolCall, ToolFunction,
    };
    use rig_core::completion::{CompletionResponse, Usage};

    let choice = if resp
        .tool_calls
        .as_ref()
        .map(|tcs| !tcs.is_empty())
        .unwrap_or(false)
    {
        let items: Vec<AssistantContent> = resp
            .tool_calls
            .as_ref()
            .unwrap()
            .iter()
            .map(|tc| {
                AssistantContent::ToolCall(RigToolCall::new(
                    tc.id.clone(),
                    ToolFunction::new(
                        tc.function.name.clone(),
                        serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(serde_json::json!({})),
                    ),
                ))
            })
            .collect();
        OneOrMany::many(items).unwrap_or_else(|_| {
            OneOrMany::one(AssistantContent::Text(Text {
                text: resp.content.clone(),
                additional_params: None,
            }))
        })
    } else {
        OneOrMany::one(AssistantContent::Text(Text {
            text: resp.content.clone(),
            additional_params: None,
        }))
    };

    CompletionResponse {
        choice,
        usage: Usage {
            input_tokens: resp.prompt_tokens.unwrap_or(0) as u64,
            output_tokens: resp.completion_tokens.unwrap_or(0) as u64,
            total_tokens: resp.total_tokens.unwrap_or(0) as u64,
            ..Default::default()
        },
        raw_response: resp,
        message_id: None,
    }
}

/// Converts RustyClaw provider `Message` list to rig-core `Message` format.
/// Inverse of `rig_messages_to_provider`.
pub fn provider_messages_to_rig(messages: &[Message]) -> Vec<rig_core::completion::Message> {
    use rig_core::OneOrMany;
    use rig_core::completion::message::{
        AssistantContent, Text, ToolCall as RigToolCall, ToolFunction, ToolResult,
        ToolResultContent, UserContent,
    };

    let mut out = Vec::new();
    for msg in messages {
        match msg.role.as_str() {
            "system" => {
                out.push(rig_core::completion::Message::System {
                    content: msg.content.clone(),
                });
            }
            "user" => {
                out.push(rig_core::completion::Message::User {
                    content: OneOrMany::one(UserContent::Text(Text {
                        text: msg.content.clone(),
                        additional_params: None,
                    })),
                });
            }
            "tool" => {
                let tool_call_id = msg
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                out.push(rig_core::completion::Message::User {
                    content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                        id: tool_call_id,
                        call_id: None,
                        content: OneOrMany::one(ToolResultContent::Text(Text {
                            text: msg.content.clone(),
                            additional_params: None,
                        })),
                    })),
                });
            }
            "assistant" => {
                let mut items: Vec<AssistantContent> = Vec::new();
                if let Some(ref tcs) = msg.tool_calls {
                    for tc in tcs {
                        items.push(AssistantContent::ToolCall(RigToolCall::new(
                            tc.id.clone(),
                            ToolFunction::new(
                                tc.function.name.clone(),
                                serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or(serde_json::json!({})),
                            ),
                        )));
                    }
                }
                if !msg.content.is_empty() {
                    items.push(AssistantContent::Text(Text {
                        text: msg.content.clone(),
                        additional_params: None,
                    }));
                }
                if items.is_empty() {
                    items.push(AssistantContent::Text(Text {
                        text: String::new(),
                        additional_params: None,
                    }));
                }
                out.push(rig_core::completion::Message::Assistant {
                    id: None,
                    content: OneOrMany::many(items).unwrap_or_else(|_| {
                        OneOrMany::one(AssistantContent::Text(Text {
                            text: String::new(),
                            additional_params: None,
                        }))
                    }),
                });
            }
            _ => {}
        }
    }
    out
}

/// Maps a session_id to a routing category for provider selection.
pub fn session_id_to_category(session_id: &str) -> String {
    if session_id == "memory" {
        "memory"
    } else if session_id == "summary" || session_id.starts_with("cron:session-summary:") {
        "summary"
    } else if session_id.contains("daily-summary") {
        "daily"
    } else if session_id.contains("heartbeat") {
        "heartbeat"
    } else if session_id.contains("briefing") {
        "briefing"
    } else if session_id.contains("vitals") {
        "vitals"
    } else if session_id.contains("karakeep") {
        "karakeep"
    } else if session_id.contains("patrol") {
        "patrol"
    } else if session_id.contains("dashboard") {
        "dashboard"
    } else {
        "discord"
    }
    .to_string()
}

/// Client handle for constructing a `RustyclawCompletionModel`.
#[derive(Clone)]
pub struct RustyclawProviderClient {
    pub config: rustyclaw_config::Config,
    pub purpose: String,
    pub session_id: String,
}

impl RustyclawProviderClient {
    pub fn new(
        config: rustyclaw_config::Config,
        purpose: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            config,
            purpose: purpose.into(),
            session_id: session_id.into(),
        }
    }
}

/// A rig-core `CompletionModel` adapter that wraps RustyClaw's failover provider chain.
/// Enables `rig::agent::Agent<RustyclawCompletionModel>` for tool-using agents.
/// The model drives rig's ReAct loop while RustyClaw's provider failover chain is preserved.
#[derive(Clone)]
pub struct RustyclawCompletionModel {
    config: rustyclaw_config::Config,
    purpose: String,
    session_id: String,
    pub model_name: String,
    usage_sink: Arc<Mutex<Option<LlmResponse>>>,
}

impl RustyclawCompletionModel {
    pub fn new(
        config: rustyclaw_config::Config,
        purpose: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        let purpose = purpose.into();
        let model_name = config
            .get_model_chain(&purpose)
            .first()
            .map(|m| m.model_name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            config,
            purpose,
            session_id: session_id.into(),
            model_name,
            usage_sink: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns a handle to the usage sink shared across all clones of this model.
    /// After `Chat::chat()` completes, the sink holds the last `LlmResponse` returned
    /// by the provider chain, which can be used to propagate token usage metadata.
    pub fn usage_sink(&self) -> Arc<Mutex<Option<LlmResponse>>> {
        Arc::clone(&self.usage_sink)
    }

    /// Implements the failover chain: tries each model in the purpose's chain until one succeeds.
    async fn complete_with_chain(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<LlmResponse, rig_core::completion::CompletionError> {
        let chain = self.config.get_model_chain(&self.purpose);
        if chain.is_empty() {
            return Err(rig_core::completion::CompletionError::ProviderError(
                format!("no models configured for purpose '{}'", self.purpose),
            ));
        }

        let category = session_id_to_category(&self.session_id);

        for (idx, model_cfg) in chain.iter().enumerate() {
            let provider_id =
                if model_cfg.model_provider == "gmn" || model_cfg.model_provider == "gemini" {
                    "gmn".to_string()
                } else {
                    resolve_provider_id(model_cfg)
                };
            if provider_cooldown_remaining(&provider_id).is_some() {
                continue;
            }

            let opts = CompletionOptions {
                model: model_cfg.model_name.clone(),
                max_tokens: max_tokens.or(model_cfg.max_tokens),
                temperature: temperature.or(model_cfg.temperature),
                timeout: std::time::Duration::from_secs(300),
                category: Some(category.clone()),
            };
            let provider = create_provider(model_cfg.clone());
            match provider.complete(messages, tools, &opts).await {
                Ok(resp) => {
                    if idx > 0 {
                        tracing::warn!(
                            purpose = %self.purpose,
                            used_model = %model_cfg.model_name,
                            fallback_index = idx,
                            "fallback model used in rig agent"
                        );
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    tracing::warn!(
                        model = %model_cfg.model_name,
                        error = %e,
                        "model failed in rig agent chain, trying next"
                    );
                }
            }
        }

        Err(rig_core::completion::CompletionError::ProviderError(
            format!("all models failed for purpose '{}'", self.purpose),
        ))
    }
}

impl rig_core::completion::CompletionModel for RustyclawCompletionModel {
    type Response = LlmResponse;
    type StreamingResponse = ();
    type Client = RustyclawProviderClient;

    fn make(client: &Self::Client, _model: impl Into<String>) -> Self {
        Self::new(
            client.config.clone(),
            client.purpose.clone(),
            client.session_id.clone(),
        )
    }

    async fn completion(
        &self,
        request: rig_core::completion::CompletionRequest,
    ) -> Result<
        rig_core::completion::CompletionResponse<LlmResponse>,
        rig_core::completion::CompletionError,
    > {
        let messages =
            rig_messages_to_provider(request.preamble.as_deref(), request.chat_history.iter());

        let tools: Vec<ToolDef> = request
            .tools
            .iter()
            .map(|td| ToolDef {
                name: td.name.clone(),
                description: td.description.clone(),
                parameters: td.parameters.clone(),
            })
            .collect();

        let llm_resp = self
            .complete_with_chain(
                &messages,
                &tools,
                request.max_tokens.map(|n| n as u32),
                request.temperature.map(|t| t as f32),
            )
            .await?;

        *self.usage_sink.lock().unwrap() = Some(llm_resp.clone());
        Ok(llm_response_to_rig(llm_resp))
    }

    async fn stream(
        &self,
        _request: rig_core::completion::CompletionRequest,
    ) -> Result<
        rig_core::streaming::StreamingCompletionResponse<()>,
        rig_core::completion::CompletionError,
    > {
        Err(rig_core::completion::CompletionError::ProviderError(
            "Streaming via rig::agent::Agent not supported; use execute_stream() directly"
                .to_string(),
        ))
    }
}

/// CF Workers AI のモデル別 neurons/M tokens レートから消費 neurons を計算する。
/// レートは CF 公式ドキュメントの値（2026-05-30 確認）。
/// 未知のモデルは gemma-4-26b 相当のレートにフォールバック。
fn calc_cf_neurons(model: &str, prompt_tokens: u32, completion_tokens: u32) -> f64 {
    // (input_neurons_per_m, output_neurons_per_m)
    let (input_rate, output_rate): (f64, f64) = if model.contains("qwen3-30b") {
        (4_625.0, 30_475.0)
    } else if model.contains("granite") {
        (1_542.0, 10_158.0)
    } else {
        // gemma-4-26b および未知モデルのデフォルト
        (9_091.0, 27_273.0)
    };
    prompt_tokens as f64 * input_rate / 1_000_000.0
        + completion_tokens as f64 * output_rate / 1_000_000.0
}

static NEURON_USAGE_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

fn record_neuron_usage(neurons: f64) {
    let _guard = NEURON_USAGE_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap();

    let neuron_path = get_app_dir().join("neuron_usage.json");
    let today_utc = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let mut neurons_used = 0.0;

    if neuron_path.exists()
        && let Ok(file) = std::fs::File::open(&neuron_path)
        && let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file)
        && let Some(date_str) = json.get("last_reset_date").and_then(|v| v.as_str())
        && date_str == today_utc
    {
        neurons_used = json
            .get("neurons_used")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
    }

    neurons_used += neurons;

    let updated_json = serde_json::json!({
        "last_reset_date": today_utc,
        "neurons_used": neurons_used
    });
    if let Ok(serialized) = serde_json::to_string_pretty(&updated_json) {
        let _ = std::fs::create_dir_all(neuron_path.parent().unwrap());
        if let Err(e) = std::fs::write(&neuron_path, serialized) {
            tracing::error!("Failed to write neuron_usage.json: {}", e);
        }
    }
}

pub fn get_neuron_stats() -> serde_json::Value {
    let today_utc = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut neurons_used = 0.0;

    let neuron_path = get_app_dir().join("neuron_usage.json");
    if neuron_path.exists()
        && let Ok(file) = std::fs::File::open(&neuron_path)
        && let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file)
        && let Some(date_str) = json.get("last_reset_date").and_then(|v| v.as_str())
        && date_str == today_utc
    {
        neurons_used = json
            .get("neurons_used")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
    }

    let now = chrono::Local::now();
    let mut next_reset = chrono::Local::now()
        .date_naive()
        .and_hms_opt(9, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap();
    if now >= next_reset {
        next_reset += chrono::Duration::days(1);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufWriter};
    use tokio::net::TcpListener;

    // RUSTYCLAW_WORKSPACE_DIR を操作するテストを直列化するための mutex
    static WORKSPACE_DIR_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
        std::sync::OnceLock::new();
    fn workspace_dir_lock() -> &'static std::sync::Mutex<()> {
        WORKSPACE_DIR_LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[tokio::test]
    async fn test_openai_compat_complete() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
                    \"id\": \"chatcmpl-123\",\
                    \"object\": \"chat.completion\",\
                    \"created\": 1677652288,\
                    \"model\": \"gpt-4o-mini\",\
                    \"choices\": [{\
                        \"index\": 0,\
                        \"message\": {\
                            \"role\": \"assistant\",\
                            \"content\": \"Hello response!\"\
                        },\
                        \"finish_reason\": \"stop\"\
                    }],\
                    \"usage\": {\
                        \"prompt_tokens\": 9,\
                        \"total_tokens\": 21\
                    }\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let model = LlmModelConfig {
            model_purpose: "default".to_string(),
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            config_name: String::new(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
            cf_aig_gateway_id: None,
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
            config_name: String::new(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
            cf_aig_gateway_id: None,
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
            std::env::set_var(
                "PATH",
                format!("{}:{}", temp_dir.path().to_string_lossy(), old_path),
            );
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
        assert_eq!(
            resp.content,
            "{\"type\": \"tool_use\", \"name\": \"do_something\"}"
        );
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
                    \"id\": \"chatcmpl-123\",\
                    \"object\": \"chat.completion\",\
                    \"created\": 1677652288,\
                    \"model\": \"gpt-4o-mini\",\
                    \"choices\": [{\
                        \"index\": 0,\
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
                        },\
                        \"finish_reason\": \"tool_calls\"\
                    }],\
                    \"usage\": {\
                        \"prompt_tokens\": 9,\
                        \"total_tokens\": 21\
                    }\
                }";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let model = LlmModelConfig {
            model_purpose: "default".to_string(),
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            config_name: String::new(),
            api_key: "dummy_key".to_string(),
            api_base_url: format!("http://{}", addr),
            max_tokens: None,
            temperature: None,
            cf_aig_gateway_id: None,
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
        assert_eq!(calls[0].function.arguments, "{\"a\":2,\"b\":3}");

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
        use chrono::{Duration, Local};
        use std::fs;

        let _lock = workspace_dir_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", tmp.path().to_str().unwrap());
        }

        // Create a dummy folder 6 days old (should be deleted)
        let old_date = (Local::now() - Duration::days(6))
            .format("%Y-%m-%d")
            .to_string();
        let old_dir = tmp.path().join("memory/debug/llm/tools").join(&old_date);
        fs::create_dir_all(&old_dir).unwrap();

        // 4-day-old folder — must be retained
        let recent_date = (Local::now() - Duration::days(4))
            .format("%Y-%m-%d")
            .to_string();
        let recent_dir = tmp.path().join("memory/debug/llm/tools").join(&recent_date);
        fs::create_dir_all(&recent_dir).unwrap();

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

        // 4-day-old folder must still exist
        assert!(recent_dir.exists(), "4-day-old dir must be retained");

        unsafe {
            std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
        }
    }

    #[test]
    fn resolve_provider_id_detects_openrouter() {
        let model = LlmModelConfig {
            model_purpose: "default".into(),
            model_provider: "openai".into(),
            model_name: "google/gemma-4-31b-it:free".into(),
            config_name: "or-gemma-4-31b".into(),
            api_key: "key".into(),
            api_base_url: "https://openrouter.ai/api/v1".into(),
            max_tokens: None,
            temperature: None,
            cf_aig_gateway_id: None,
        };
        assert_eq!(resolve_provider_id(&model), "openrouter");
    }

    #[test]
    fn resolve_provider_id_detects_cloudflare() {
        let model = LlmModelConfig {
            model_purpose: "default".into(),
            model_provider: "openai".into(),
            model_name: "@cf/qwen/qwen3-30b-a3b-fp8".into(),
            config_name: "cf-qwen3-30b".into(),
            api_key: "key".into(),
            api_base_url: "https://api.cloudflare.com/client/v4/accounts/xxx/ai/v1".into(),
            max_tokens: None,
            temperature: None,
            cf_aig_gateway_id: None,
        };
        assert_eq!(resolve_provider_id(&model), "cloudflare");
    }

    #[test]
    fn resolve_provider_id_detects_local() {
        let model = LlmModelConfig {
            model_purpose: "default".into(),
            model_provider: "openai".into(),
            model_name: "google/gemma-4-e4b".into(),
            config_name: "lms-gemma-4-e4b".into(),
            api_key: "lm-studio".into(),
            api_base_url: "http://192.168.1.110:1234/v1".into(),
            max_tokens: None,
            temperature: None,
            cf_aig_gateway_id: None,
        };
        assert_eq!(resolve_provider_id(&model), "local");
    }

    #[test]
    fn test_cf_embedding_parse_response() {
        let json = r#"{
            "result": {"data": [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]},
            "success": true,
            "errors": []
        }"#;
        let parsed: CfEmbedResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.result.as_ref().unwrap().data.len(), 2);
        assert!((parsed.result.unwrap().data[0][0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn test_cf_embedding_parse_error_response() {
        let json = r#"{
            "result": null,
            "success": false,
            "errors": [{"code": 1001, "message": "Invalid model"}]
        }"#;
        let parsed: CfEmbedResponse = serde_json::from_str(json).unwrap();
        assert!(!parsed.success);
        assert!(parsed.result.is_none());
        assert_eq!(parsed.errors.len(), 1);
    }

    #[test]
    fn test_embed_body_uses_input_for_openai_compat_endpoint() {
        let client =
            CloudflareEmbeddingClient::new("http://192.168.1.110:1234/v1/embeddings", "lm-studio");
        let body = client.build_embed_body(&["hello", "world"]);
        assert!(
            body.get("input").is_some(),
            "OpenAI-compat endpoint must use 'input' field"
        );
        assert!(
            body.get("text").is_none(),
            "OpenAI-compat endpoint must NOT use 'text' field"
        );
    }

    #[test]
    fn test_embed_body_uses_text_for_cf_native_endpoint() {
        let client = CloudflareEmbeddingClient::new(
            "https://gateway.ai.cloudflare.com/v1/abc/rustyclaw-gateway/workers-ai/@cf/baai/bge-m3",
            "cf-key",
        );
        let body = client.build_embed_body(&["hello"]);
        assert!(
            body.get("text").is_some(),
            "CF native endpoint must use 'text' field"
        );
        assert!(
            body.get("input").is_none(),
            "CF native endpoint must NOT use 'input' field"
        );
    }

    #[test]
    fn test_parse_openai_embed_response() {
        let json = r#"{
            "object": "list",
            "data": [
                {"object": "embedding", "embedding": [0.1, 0.2, 0.3], "index": 0},
                {"object": "embedding", "embedding": [0.4, 0.5, 0.6], "index": 1}
            ],
            "model": "text-embedding-bge-m3",
            "usage": {"prompt_tokens": 5, "total_tokens": 5}
        }"#;
        let parsed: OpenAiEmbedResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert!((parsed.data[0].embedding[0] - 0.1_f32).abs() < 1e-6);
        assert!((parsed.data[1].embedding[2] - 0.6_f32).abs() < 1e-6);
    }

    #[test]
    fn test_provider_error_reset_after() {
        let err = ProviderError::RateLimit(
            "Error: Your Quota will reset after 56s. Please try again.".to_string(),
        );
        assert_eq!(err.reset_after(), Some(std::time::Duration::from_secs(56)));

        let err2 = ProviderError::RateLimit("Your Quota will reset after 5s".to_string());
        assert_eq!(err2.reset_after(), Some(std::time::Duration::from_secs(5)));

        // 小文字 "quota" パターン (実際のログで検出されたもの)
        let err_real = ProviderError::RateLimit(
            "You have exhausted your capacity on this model. Your quota will reset after 2s."
                .to_string(),
        );
        assert_eq!(
            err_real.reset_after(),
            Some(std::time::Duration::from_secs(2))
        );

        // メッセージ後半に "model" や "gemini-3-flash-preview" 等の 'm' が含まれるが、
        // 秒数のみ表記であるため誤認識せずに正確に 48 秒とパースできるか検証するテストケース
        let err_with_model_word = ProviderError::RateLimit("You have exhausted your capacity on this model. Your quota will reset after 48s. details: model: gemini-3-flash-preview".to_string());
        assert_eq!(
            err_with_model_word.reset_after(),
            Some(std::time::Duration::from_secs(48))
        );

        let err3 = ProviderError::RateLimit("Your Quota will reset after 1m 30s.".to_string());
        assert_eq!(err3.reset_after(), Some(std::time::Duration::from_secs(90)));

        let err4 = ProviderError::RateLimit("Your Quota will reset after 2m".to_string());
        assert_eq!(
            err4.reset_after(),
            Some(std::time::Duration::from_secs(120))
        );

        let err5 = ProviderError::RateLimit("Your Quota will reset after 3m 5s".to_string());
        assert_eq!(
            err5.reset_after(),
            Some(std::time::Duration::from_secs(185))
        );

        let err_none =
            ProviderError::RateLimit("Some other quota error without reset time".to_string());
        assert_eq!(err_none.reset_after(), None);

        let err_exec =
            ProviderError::ExecutionFailed("Your Quota will reset after 10s".to_string());
        assert_eq!(err_exec.reset_after(), None);

        // Cloudflare daily limit parsing test (簡易文字列)
        let err_cf = ProviderError::RateLimit(
            "you have used up your daily free allocation of 10,000 neurons".to_string(),
        );
        assert!(err_cf.reset_after().is_some());
        assert!(err_cf.reset_after().unwrap().as_secs() > 0);

        // Cloudflare 実際の HTTP レスポンスボディ (internalCode: 4006 の JSON 全文)
        let err_cf_real = ProviderError::RateLimit(r#"{"name":"AiError","internalCode":4006,"httpCode":429,"message":"AiError: AiError: you have used up your daily free allocation of 10,000 neurons, please upgrade to Cloudflare's Workers Paid plan if you would like to continue usage. (35c8551a-c0e6-4dd5-8f67-2ce2f9a112d6)","description":"you have used up your daily free allocation of 10,000 neurons, please upgrade to Cloudflare's Workers Paid plan if you would like to continue usage.","requestId":"35c8551a-c0e6-4dd5-8f67-2ce2f9a112d6"}"#.to_string());
        assert!(
            err_cf_real.reset_after().is_some(),
            "CF JSON body must be parseable"
        );
        // 翌 9:00 JST リセット想定なので最低 1 秒以上の待機時間が返ること
        assert!(
            err_cf_real.reset_after().unwrap().as_secs() >= 1,
            "CF daily limit must return meaningful wait duration"
        );

        // set_global_cooldown_from_error がクールダウンをセットする
        {
            let mut lock = get_cooldowns_map().lock().unwrap();
            lock.clear();
        }
        assert!(global_cooldown_remaining().is_none());
        set_global_cooldown_from_error(&ProviderError::RateLimit("Too Many Requests".to_string()));
        assert!(
            global_cooldown_remaining().is_some(),
            "global_cooldown_remaining must be set after rate limit"
        );

        // Cloudflare RPM 429 (JSON body, no reset time) → default 60s
        let err_cf_rpm = ProviderError::RateLimit(
            r#"{"errors":[{"code":10014,"message":"Too Many Requests"}]}"#.to_string(),
        );
        assert_eq!(
            err_cf_rpm.reset_after(),
            Some(std::time::Duration::from_secs(60))
        );

        // Cloudflare RPM 429 (plain string)
        let err_cf_rpm2 = ProviderError::RateLimit("Too Many Requests".to_string());
        assert_eq!(
            err_cf_rpm2.reset_after(),
            Some(std::time::Duration::from_secs(60))
        );

        // Cloudflare RPM 429 (lowercase)
        let err_cf_rpm3 = ProviderError::RateLimit("too many requests".to_string());
        assert_eq!(
            err_cf_rpm3.reset_after(),
            Some(std::time::Duration::from_secs(60))
        );
    }

    #[tokio::test]
    async fn test_cloudflare_embedding_model_implements_embedding_model() {
        use rig_core::embeddings::EmbeddingModel;
        let client = CloudflareEmbeddingClient::new("http://127.0.0.1:1", "dummy");
        let model = CloudflareEmbeddingModel::new(client, 1024);
        assert_eq!(model.ndims(), 1024);

        // 空入力は即 Ok(vec![]) を返す（HTTP 呼び出しなし）
        let result = model.embed_texts(vec![]).await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_rig_messages_from_completion_request() {
        use rig_core::OneOrMany;
        use rig_core::completion::Message as RigMsg;
        use rig_core::completion::message::{
            AssistantContent, Text, ToolResult, ToolResultContent,
            UserContent,
        };

        let history = [
            RigMsg::User {
                content: OneOrMany::one(UserContent::Text(Text {
                    text: "hello".to_string(),
                    additional_params: None,
                })),
            },
            RigMsg::Assistant {
                id: None,
                content: OneOrMany::one(AssistantContent::Text(Text {
                    text: "hi".to_string(),
                    additional_params: None,
                })),
            },
            RigMsg::User {
                content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                    id: "call-1".to_string(),
                    call_id: None,
                    content: OneOrMany::one(ToolResultContent::Text(Text {
                        text: "tool output".to_string(),
                        additional_params: None,
                    })),
                })),
            },
        ];

        let msgs = rig_messages_to_provider(None, history.iter());
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(msgs[2].content, "tool output");
    }

    #[test]
    fn test_rig_messages_preamble_injected() {
        let msgs = rig_messages_to_provider(Some("system prompt"), std::iter::empty());
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "system prompt");
    }

    #[test]
    fn test_completion_model_usage_sink_starts_empty() {
        let model = RustyclawCompletionModel::new(
            rustyclaw_config::Config::default(),
            "default",
            "test-session",
        );
        let sink = model.usage_sink();
        assert!(sink.lock().unwrap().is_none());
    }

    #[tokio::test]
    #[ignore = "requires fastembed model download (~30MB) on first run; cached after"]
    async fn test_local_embedding_client_embed_dims() {
        let cache_dir = std::env::temp_dir().join("rustyclaw_fastembed_test");
        let client = LocalEmbeddingClient::new(&cache_dir).expect("model init failed");
        let result = client
            .embed(&["Hello world", "こんにちは"])
            .await
            .expect("embed failed");
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].len(),
            384,
            "expected 384 dims for multilingual-e5-small"
        );
        assert_ne!(
            result[0], result[1],
            "different texts should produce different vectors"
        );
    }

    #[test]
    fn test_dump_llm_io_writes_last_request_json() {
        use std::fs;
        use tempfile::TempDir;

        let _lock = workspace_dir_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        unsafe {
            std::env::set_var("RUSTYCLAW_WORKSPACE_DIR", workspace.to_str().unwrap());
        }

        let messages = vec![
            Message {
                role: "system".to_string(),
                content: "S".repeat(1_000),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                trigger: None,
                timestamp: None,
            },
            Message {
                role: "user".to_string(),
                content: "Hello".to_string(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                trigger: None,
                timestamp: None,
            },
        ];

        let response = LlmResponse {
            content: "Hi there".to_string(),
            role: "assistant".to_string(),
            tool_calls: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            model_used: None,
            provider_id: None,
        };
        dump_llm_io("test", "test-model", &messages, &response);

        let last_req_path = workspace
            .join("memory")
            .join("debug")
            .join("llm")
            .join("last_request.json");

        assert!(
            last_req_path.exists(),
            "last_request.json が生成されているはず"
        );

        let content = fs::read_to_string(&last_req_path).unwrap();
        let size = content.len();
        assert!(
            size < 5_120,
            "last_request.json が 5KB 未満のはず (実際: {} bytes)",
            size
        );

        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(val["model"].is_string());
        assert!(val["message_count"].is_number());
        assert!(val["messages"].is_array());

        unsafe {
            std::env::remove_var("RUSTYCLAW_WORKSPACE_DIR");
        }
    }
}

