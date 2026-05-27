use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

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

impl Config {
    /// 環境変数による設定値のオーバーライドを適用する
    pub fn override_with_env(&mut self) {
        if let Ok(val) = std::env::var("RUSTYCLAW_API_KEY") {
            self.api_key = val;
        }
        if let Ok(val) = std::env::var("RUSTYCLAW_MODEL_NAME") {
            self.model_name = val;
        }
        if let Ok(val) = std::env::var("RUSTYCLAW_API_BASE_URL") {
            self.api_base_url = val;
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
    fn test_detect_timezone_returns_string() {
        // 検出結果が Some の場合、空文字列でないこと
        if let Some(tz) = detect_timezone() {
            assert!(!tz.is_empty());
        }
        // None の場合は chrono::Local にフォールバックするため問題なし
    }
}
