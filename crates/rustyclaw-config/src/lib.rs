use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmModelConfig {
    pub model_purpose: String,
    pub model_provider: String,
    pub model_name: String,
    pub api_key: String,
    pub api_base_url: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParsedLlmModelConfig {
    pub model_purpose: String,
    #[serde(default)]
    pub model_provider: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServerConfig {
    pub enabled: bool,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub model_provider: String,
    pub model_name: String,
    pub api_key: String,
    pub api_base_url: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    #[serde(default)]
    pub debug_dump: bool,
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
    /// MCP サーバーの設定
    #[serde(default)]
    pub mcp: HashMap<String, McpServerConfig>,
    /// 複数 LLM モデルの設定
    #[serde(default)]
    pub models: Vec<ParsedLlmModelConfig>,
}

fn resolve_value(val: &str) -> String {
    if val.starts_with("$env:") {
        let env_name = &val[5..];
        std::env::var(env_name).unwrap_or_else(|_| val.to_string())
    } else if val.starts_with("$vault:") {
        let vault_key = &val[7..];
        if let Ok(env_val) = std::env::var(vault_key) {
            return env_val;
        }
        if let Ok(env_val) = std::env::var(format!("RUSTYCLAW_VAULT_{}", vault_key.to_uppercase())) {
            return env_val;
        }
        if let Ok(home) = std::env::var("HOME") {
            let vault_path = std::path::PathBuf::from(home).join(".rustyclaw").join("vault.json");
            if vault_path.exists() {
                if let Ok(file) = std::fs::File::open(vault_path) {
                    if let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file) {
                        if let Some(v) = json.get(vault_key).and_then(|v| v.as_str()) {
                            return v.to_string();
                        }
                    }
                }
            }
        }
        val.to_string()
    } else {
        val.to_string()
    }
}

impl Config {
    /// 用途 (model_purpose) に基づいて適切なモデル設定を取得する。
    /// 一致するモデルが設定されていない場合は、デフォルトのルート設定フォールバックを返す。
    pub fn get_model(&self, model_purpose: &str) -> LlmModelConfig {
        if let Some(m) = self.models.iter().find(|m| m.model_purpose == model_purpose) {
            LlmModelConfig {
                model_purpose: m.model_purpose.clone(),
                model_provider: m.model_provider.clone().unwrap_or_else(|| self.model_provider.clone()),
                model_name: m.model_name.clone().unwrap_or_else(|| self.model_name.clone()),
                api_key: m.api_key.clone().unwrap_or_else(|| self.api_key.clone()),
                api_base_url: m.api_base_url.clone().unwrap_or_else(|| self.api_base_url.clone()),
                max_tokens: m.max_tokens.or(self.max_tokens),
                temperature: m.temperature.or(self.temperature),
            }
        } else {
            LlmModelConfig {
                model_purpose: model_purpose.to_string(),
                model_provider: self.model_provider.clone(),
                model_name: self.model_name.clone(),
                api_key: self.api_key.clone(),
                api_base_url: self.api_base_url.clone(),
                max_tokens: self.max_tokens,
                temperature: self.temperature,
            }
        }
    }

    /// 環境変数による設定値のオーバーライドを適用する
    pub fn override_with_env(&mut self) {
        if let Ok(val) = std::env::var("RUSTYCLAW_API_KEY") {
            self.api_key = val.clone();
            if let Some(m) = self.models.iter_mut().find(|m| m.model_purpose == "default") {
                m.api_key = Some(val);
            }
        }
        if let Ok(val) = std::env::var("RUSTYCLAW_MODEL_NAME") {
            self.model_name = val.clone();
            if let Some(m) = self.models.iter_mut().find(|m| m.model_purpose == "default") {
                m.model_name = Some(val);
            }
        }
        if let Ok(val) = std::env::var("RUSTYCLAW_API_BASE_URL") {
            self.api_base_url = val.clone();
            if let Some(m) = self.models.iter_mut().find(|m| m.model_purpose == "default") {
                m.api_base_url = Some(val);
            }
        }
    }

    /// $vault: や $env: のプレフィックスを解決する
    pub fn resolve_secrets(&mut self) {
        self.model_name = resolve_value(&self.model_name);
        self.api_key = resolve_value(&self.api_key);
        self.api_base_url = resolve_value(&self.api_base_url);
        
        for model in self.models.iter_mut() {
            if let Some(ref name) = model.model_name {
                model.model_name = Some(resolve_value(name));
            }
            if let Some(ref key) = model.api_key {
                model.api_key = Some(resolve_value(key));
            }
            if let Some(ref url) = model.api_base_url {
                model.api_base_url = Some(resolve_value(url));
            }
        }

        if let Some(ref token) = self.discord_token {
            self.discord_token = Some(resolve_value(token));
        }
        if let Some(ref channel) = self.discord_home_channel_id {
            self.discord_home_channel_id = Some(resolve_value(channel));
        }
        
        for server in self.mcp.values_mut() {
            for val in server.env.values_mut() {
                *val = resolve_value(val);
            }
        }
    }
}

/// システムの IANA タイムゾーン名を検出する。
///
/// 優先順位:
///   1. /etc/timezone（Debian / Raspberry Pi OS）
///   2. /etc/localtime シンボリックリンクから zoneinfo パスを抽出
///   3. TZ 環境変数
///   4. None（呼び出し側は chrono::Local にフォールバック）
pub fn detect_timezone() -> Option<String> {
    if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
        let tz = tz.trim().to_string();
        if !tz.is_empty() {
            return Some(tz);
        }
    }
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let s = link.to_string_lossy();
        if let Some(idx) = s.find("zoneinfo/") {
            return Some(s[idx + 9..].to_string());
        }
    }
    std::env::var("TZ").ok()
}

/// 指定されたパスから `config.json` をロードする
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let file = File::open(&path)
        .with_context(|| format!("Failed to open config file at {:?}", path.as_ref()))?;
    let reader = BufReader::new(file);
    let mut config: Config = serde_json::from_reader(reader)
        .with_context(|| "Failed to parse config.json schema")?;

    // 環境変数によるオーバーライドの適用
    config.override_with_env();

    // シークレットの解決
    config.resolve_secrets();

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_success() -> Result<()> {
        let mut tmp_file = NamedTempFile::new()?;
        let json_data = r#"{
            "model_provider": "openai",
            "model_name": "gpt-4o-mini",
            "api_key": "test_key",
            "api_base_url": "https://api.openai.com/v1",
            "max_tokens": 100,
            "temperature": 0.5,
            "debug_dump": true
        }"#;
        tmp_file.write_all(json_data.as_bytes())?;

        let config = load_config(tmp_file.path())?;
        assert_eq!(config.model_provider, "openai");
        assert_eq!(config.model_name, "gpt-4o-mini");
        assert_eq!(config.api_key, "test_key");
        assert_eq!(config.max_tokens, Some(100));
        assert_eq!(config.temperature, Some(0.5));
        assert!(config.debug_dump);

        Ok(())
    }

    #[test]
    fn test_env_override() -> Result<()> {
        let mut config = Config {
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "original_key".to_string(),
            api_base_url: "https://api.openai.com/v1".to_string(),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
            mcp: HashMap::new(),
            models: vec![],
        };

        unsafe {
            std::env::set_var("RUSTYCLAW_API_KEY", "env_key");
            std::env::set_var("RUSTYCLAW_MODEL_NAME", "env_model");
        }
        config.override_with_env();

        assert_eq!(config.api_key, "env_key");
        assert_eq!(config.model_name, "env_model");

        unsafe {
            std::env::remove_var("RUSTYCLAW_API_KEY");
            std::env::remove_var("RUSTYCLAW_MODEL_NAME");
        }
        Ok(())
    }

    #[test]
    fn test_resolve_secrets() -> Result<()> {
        let mut config = Config {
            model_provider: "openai".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            api_key: "$env:TEST_SECRET_API_KEY".to_string(),
            api_base_url: "$vault:cf-base-url".to_string(),
            max_tokens: None,
            temperature: None,
            debug_dump: false,
            discord_token: None,
            discord_home_channel_id: None,
            discord_respond_in_channels: vec![],
            mcp: HashMap::new(),
            models: vec![],
        };

        unsafe {
            std::env::set_var("TEST_SECRET_API_KEY", "resolved_env_key");
            std::env::set_var("cf-base-url", "resolved_vault_base_url");
        }

        config.resolve_secrets();

        assert_eq!(config.api_key, "resolved_env_key");
        assert_eq!(config.api_base_url, "resolved_vault_base_url");

        unsafe {
            std::env::remove_var("TEST_SECRET_API_KEY");
            std::env::remove_var("cf-base-url");
        }
        Ok(())
    }

    #[test]
    fn test_detect_timezone_returns_string() {
        // 検出結果が Some の場合、空文字列でないこと
        if let Some(tz) = detect_timezone() {
            assert!(!tz.is_empty());
        }
        // None の場合は chrono::Local にフォールバックするため問題なし
    }
}
