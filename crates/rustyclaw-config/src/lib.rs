use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

pub mod vault;

/// アプリケーションホームディレクトリを返す。
///
/// 解決順:
/// 1. `RUSTYCLAW_HOME` 環境変数
/// 2. `~/.rustyclaw`（デフォルト）
pub fn get_app_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RUSTYCLAW_HOME") {
        return PathBuf::from(custom);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".rustyclaw")
}

/// 設定ファイルディレクトリを返す: {app_dir}/config/
pub fn get_config_dir() -> PathBuf {
    get_app_dir().join("config")
}

// ─────────────────────────────────────────────
// モデル設定
// ─────────────────────────────────────────────

/// model_list の各エントリ（config.json に記述する生の値、$vault: 参照含む）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// 識別子（agents から参照するキー）
    pub model_name: String,
    /// プロバイダ種別（"openai" = OpenAI 互換 API）
    #[serde(default = "default_provider")]
    pub provider: String,
    /// API に渡す実際のモデル ID
    pub model: String,
    /// API ベース URL（$vault: 参照可）
    pub api_base: String,
    /// API キー（$vault: 参照可）
    pub api_key: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(default = "default_temperature")]
    pub temperature: Option<f32>,
    #[serde(default = "bool_true")]
    pub enabled: bool,
    /// 1分間のリクエスト数上限（Requests Per Minute）
    #[serde(default)]
    pub rpm: Option<u64>,
    /// 1日のリクエスト数上限（Requests Per Day）
    #[serde(default)]
    pub rpd: Option<u64>,
    /// 1分間のトークン数上限（Tokens Per Minute）
    #[serde(default)]
    pub tpm: Option<u64>,
    /// 1日のトークン数上限（Tokens Per Day）
    #[serde(default)]
    pub tpd: Option<u64>,
    /// コンテキストウィンドウサイズ（tokens。"131k", "1M" 等の表記可）
    #[serde(default)]
    pub context_window: Option<String>,
}

fn default_provider() -> String { "openai".to_string() }
fn default_max_tokens() -> Option<u32> { Some(2048) }
fn default_temperature() -> Option<f32> { Some(0.7) }
fn bool_true() -> bool { true }

/// JSON 文字列 "foo" と JSON 配列 ["foo", "bar"] の両方をデシリアライズできる enum。
/// 配列の場合、先頭が primary モデル、以降がフォールバックモデル。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelNames {
    Single(String),
    Chain(Vec<String>),
}

impl Default for ModelNames {
    fn default() -> Self { Self::Single(String::new()) }
}

impl ModelNames {
    /// 先頭（primary）モデル名を返す。
    pub fn primary(&self) -> &str {
        match self {
            Self::Single(s) => s,
            Self::Chain(v)  => v.first().map(|s| s.as_str()).unwrap_or(""),
        }
    }

    /// [primary, fallback1, fallback2, ...] のスライスを返す。
    pub fn as_chain(&self) -> Vec<&str> {
        match self {
            Self::Single(s) => vec![s.as_str()],
            Self::Chain(v)  => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// agents セクション（default 必須、各 purpose は省略時 default にフォールバック）
/// model_name は文字列（単一）または配列（フォールバックチェーン）で指定可能。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentsConfig {
    pub default: ModelNames,
    /// すべての purpose チェーンが失敗した場合の最終フォールバックモデル名
    #[serde(default)]
    pub global_fallback_model_name: Option<String>,
    #[serde(default)]
    pub summary:   Option<ModelNames>,
    #[serde(default)]
    pub memory:    Option<ModelNames>,
    #[serde(default)]
    pub tools:     Option<ModelNames>,
    #[serde(default)]
    pub discord:   Option<ModelNames>,
    #[serde(default)]
    pub line:      Option<ModelNames>,
    #[serde(default)]
    pub heartbeat: Option<ModelNames>,
    #[serde(default)]
    pub patrol:    Option<ModelNames>,
}

/// get_model() が返す解決済みモデル設定（$vault: 参照解決済み）
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

// ─────────────────────────────────────────────
// MCP（stdio プロトコル経由のサーバー専用）
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServerConfig {
    pub enabled: bool,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

// ─────────────────────────────────────────────
// Channels（入力チャネル設定）
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscordConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub home_channel_id: Option<String>,
    #[serde(default)]
    pub respond_in_channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LineConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub channel_access_token: String,
    #[serde(default)]
    pub channel_secret: String,
    /// Webhook 受信ポート（デフォルト 8443）
    #[serde(default)]
    pub webhook_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub discord: Option<DiscordConfig>,
    #[serde(default)]
    pub line: Option<LineConfig>,
}

// ─────────────────────────────────────────────
// Tools（ネイティブ実装ツールの接続設定）
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KarakeepConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    #[serde(default)]
    pub server_addr: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObsidianConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BraveSearchConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoogleWorkspaceConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    /// gws CLI バイナリのパス（省略時は既定パスを自動探索）
    #[serde(default)]
    pub gws_path: Option<String>,
    /// 書き込みを許可するカレンダー ID リスト
    #[serde(default)]
    pub writable_calendar_ids: Vec<String>,
    /// このラベルを持つスレッドを削除可能とする Gmail ラベル名
    #[serde(default)]
    pub gmail_deletable_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    #[serde(default)]
    pub karakeep: Option<KarakeepConfig>,
    #[serde(default)]
    pub obsidian: Option<ObsidianConfig>,
    #[serde(default, rename = "google-workspace")]
    pub google_workspace: Option<GoogleWorkspaceConfig>,
    #[serde(default, rename = "brave-search")]
    pub brave_search: Option<BraveSearchConfig>,
}

// ─────────────────────────────────────────────
// Config
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// 利用可能な LLM モデルリスト
    #[serde(default)]
    pub model_list: Vec<ModelEntry>,
    /// 用途別 LLM 選択
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub debug_dump: bool,
    /// 入力チャネル設定（Discord・LINE など）
    #[serde(default)]
    pub channels: ChannelsConfig,
    /// ネイティブツールの接続設定
    #[serde(default)]
    pub tools: ToolsConfig,
    /// MCP stdio プロトコル経由のサーバー設定
    #[serde(default)]
    pub mcp: HashMap<String, McpServerConfig>,
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
        if let Ok(secrets) = vault::load_vault(None) {
            if let Some(v) = secrets.get(vault_key) {
                return v.clone();
            }
        }
        {
            let json_path = get_config_dir().join("vault.json");
            if json_path.exists() {
                if let Ok(file) = std::fs::File::open(json_path) {
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
    /// model_name (config キー) → 解決済み LlmModelConfig に変換する内部ヘルパー。
    /// enabled: false のエントリは None を返す。
    fn resolve_model(&self, model_name: &str, purpose: &str) -> Option<LlmModelConfig> {
        self.model_list.iter()
            .find(|m| m.model_name == model_name && m.enabled)
            .map(|e| LlmModelConfig {
                model_purpose: purpose.to_string(),
                model_provider: e.provider.clone(),
                model_name: e.model.clone(),
                api_key: e.api_key.clone(),
                api_base_url: e.api_base.clone(),
                max_tokens: e.max_tokens,
                temperature: e.temperature,
            })
    }

    /// purpose の ModelNames を返す（未設定なら default）。
    fn get_model_names_for_purpose(&self, purpose: &str) -> &ModelNames {
        match purpose {
            "summary"   => self.agents.summary.as_ref().unwrap_or(&self.agents.default),
            "memory"    => self.agents.memory.as_ref().unwrap_or(&self.agents.default),
            "tools"     => self.agents.tools.as_ref().unwrap_or(&self.agents.default),
            "discord"   => self.agents.discord.as_ref().unwrap_or(&self.agents.default),
            "line"      => self.agents.line.as_ref().unwrap_or(&self.agents.default),
            "heartbeat" => self.agents.heartbeat.as_ref().unwrap_or(&self.agents.default),
            "patrol"    => self.agents.patrol.as_ref().unwrap_or(&self.agents.default),
            _           => &self.agents.default,
        }
    }

    /// purpose のモデルチェーンを解決済み LlmModelConfig のリストとして返す。
    /// 順序: purpose 指定モデル群 → global_fallback（重複除去）
    /// disabled なモデルはリストから除外される。
    pub fn get_model_chain(&self, purpose: &str) -> Vec<LlmModelConfig> {
        let names = self.get_model_names_for_purpose(purpose);
        let mut name_list: Vec<&str> = names.as_chain();

        // global_fallback を末尾に追加（重複は除去）
        if let Some(ref gf) = self.agents.global_fallback_model_name {
            if !name_list.contains(&gf.as_str()) {
                name_list.push(gf.as_str());
            }
        }

        name_list.iter()
            .filter_map(|name| self.resolve_model(name, purpose))
            .collect()
    }

    /// 用途に対応する解決済み LlmModelConfig を返す。後方互換維持。
    /// 内部では get_model_chain()[0] を使用。
    /// チェーンが空（全モデル disabled）の場合は model_list 先頭 enabled モデルを返す。
    pub fn get_model(&self, purpose: &str) -> LlmModelConfig {
        // チェーンの先頭を返す
        if let Some(first) = self.get_model_chain(purpose).into_iter().next() {
            return first;
        }
        // 全 named モデルが disabled → model_list 先頭 enabled モデルを最終手段として返す
        self.model_list.iter()
            .find(|m| m.enabled)
            .map(|e| LlmModelConfig {
                model_purpose: purpose.to_string(),
                model_provider: e.provider.clone(),
                model_name: e.model.clone(),
                api_key: e.api_key.clone(),
                api_base_url: e.api_base.clone(),
                max_tokens: e.max_tokens,
                temperature: e.temperature,
            })
            .unwrap_or_else(|| LlmModelConfig {
                model_purpose: purpose.to_string(),
                ..Default::default()
            })
    }

    /// 環境変数による設定オーバーライド
    pub fn override_with_env(&mut self) {
        // RUSTYCLAW_MODEL_NAME: agents.default の model_name を上書き
        if let Ok(val) = std::env::var("RUSTYCLAW_MODEL_NAME") {
            self.agents.default = ModelNames::Single(val);
        }
    }

    /// $vault: / $env: 参照を解決する
    pub fn resolve_secrets(&mut self) {
        for entry in self.model_list.iter_mut() {
            entry.api_key = resolve_value(&entry.api_key);
            entry.api_base = resolve_value(&entry.api_base);
            entry.model = resolve_value(&entry.model);
        }
        if let Some(ref mut d) = self.channels.discord {
            d.token = resolve_value(&d.token);
        }
        if let Some(ref mut l) = self.channels.line {
            l.channel_access_token = resolve_value(&l.channel_access_token);
            l.channel_secret = resolve_value(&l.channel_secret);
        }
        if let Some(ref mut k) = self.tools.karakeep {
            k.server_addr = resolve_value(&k.server_addr);
            k.api_key = resolve_value(&k.api_key);
        }
        if let Some(ref mut o) = self.tools.obsidian {
            o.host = resolve_value(&o.host);
            o.api_key = resolve_value(&o.api_key);
        }
        if let Some(ref mut b) = self.tools.brave_search {
            b.api_key = resolve_value(&b.api_key);
        }
        for server in self.mcp.values_mut() {
            for val in server.env.values_mut() {
                *val = resolve_value(val);
            }
        }
    }
}

/// システムの IANA タイムゾーン名を検出する。
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

/// 指定されたパスから config.json をロードする
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let file = File::open(&path)
        .with_context(|| format!("Failed to open config file at {:?}", path.as_ref()))?;
    let reader = BufReader::new(file);
    let mut config: Config = serde_json::from_reader(reader)
        .with_context(|| "Failed to parse config.json schema")?;

    config.override_with_env();
    config.resolve_secrets();

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_test_config() -> Config {
        Config {
            model_list: vec![
                ModelEntry {
                    model_name: "test-8b".to_string(),
                    provider: "openai".to_string(),
                    model: "llama-3.1-8b-instant".to_string(),
                    api_base: "https://api.test.com/v1".to_string(),
                    api_key: "test-key".to_string(),
                    max_tokens: Some(2048),
                    temperature: Some(0.7),
                    enabled: true,
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: Some("131k".to_string()),
                },
                ModelEntry {
                    model_name: "test-70b".to_string(),
                    provider: "openai".to_string(),
                    model: "llama-3.3-70b-versatile".to_string(),
                    api_base: "https://api.test.com/v1".to_string(),
                    api_key: "test-key-70b".to_string(),
                    max_tokens: Some(1500),
                    temperature: Some(0.3),
                    enabled: true,
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: None,
                },
            ],
            agents: AgentsConfig {
                default: ModelNames::Single("test-8b".to_string()),
                global_fallback_model_name: None,
                summary: Some(ModelNames::Single("test-70b".to_string())),
                memory: Some(ModelNames::Single("test-70b".to_string())),
                tools: None,
                discord: None,
                line: None,
                heartbeat: None,
                patrol: None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_load_config_success() -> Result<()> {
        let mut tmp_file = NamedTempFile::new()?;
        let json_data = r#"{
            "model_list": [
                {
                    "model_name": "groq-8b",
                    "provider": "openai",
                    "model": "llama-3.1-8b-instant",
                    "api_base": "https://api.groq.com/openai/v1",
                    "api_key": "test_key",
                    "max_tokens": 2048,
                    "temperature": 0.7,
                    "enabled": true
                }
            ],
            "agents": {
                "default": "groq-8b"
            },
            "debug_dump": true
        }"#;
        tmp_file.write_all(json_data.as_bytes())?;

        let config = load_config(tmp_file.path())?;
        assert_eq!(config.model_list.len(), 1);
        assert_eq!(config.model_list[0].model_name, "groq-8b");
        assert_eq!(config.agents.default.primary(), "groq-8b");
        assert!(config.debug_dump);

        let model = config.get_model("default");
        assert_eq!(model.model_name, "llama-3.1-8b-instant");
        assert_eq!(model.model_provider, "openai");
        assert_eq!(model.api_key, "test_key");

        Ok(())
    }

    #[test]
    fn test_get_model_purpose_fallback() {
        let config = make_test_config();

        let default = config.get_model("default");
        assert_eq!(default.model_name, "llama-3.1-8b-instant");

        let summary = config.get_model("summary");
        assert_eq!(summary.model_name, "llama-3.3-70b-versatile");
        assert_eq!(summary.api_key, "test-key-70b");

        let memory = config.get_model("memory");
        assert_eq!(memory.model_name, "llama-3.3-70b-versatile");

        // heartbeat は明示設定なし → default にフォールバック（設定済みなら専用モデルを返す）
        let unknown = config.get_model("heartbeat");
        assert_eq!(unknown.model_name, "llama-3.1-8b-instant");
    }

    #[test]
    fn test_get_model_new_purposes() {
        let mut config = make_test_config();
        // 未設定 → default にフォールバック
        assert_eq!(config.get_model("tools").model_name,     "llama-3.1-8b-instant");
        assert_eq!(config.get_model("discord").model_name,   "llama-3.1-8b-instant");
        assert_eq!(config.get_model("line").model_name,      "llama-3.1-8b-instant");
        assert_eq!(config.get_model("heartbeat").model_name, "llama-3.1-8b-instant");

        // 明示設定した場合はそちらを返す
        config.agents.discord   = Some(ModelNames::Single("test-70b".to_string()));
        config.agents.heartbeat = Some(ModelNames::Single("test-70b".to_string()));
        config.agents.tools     = Some(ModelNames::Single("test-70b".to_string()));
        assert_eq!(config.get_model("discord").model_name,   "llama-3.3-70b-versatile");
        assert_eq!(config.get_model("heartbeat").model_name, "llama-3.3-70b-versatile");
        assert_eq!(config.get_model("tools").model_name,     "llama-3.3-70b-versatile");
    }

    #[test]
    fn test_env_override() {
        let mut config = make_test_config();
        unsafe { std::env::set_var("RUSTYCLAW_MODEL_NAME", "test-70b"); }
        config.override_with_env();
        assert_eq!(config.agents.default.primary(), "test-70b");
        let model = config.get_model("default");
        assert_eq!(model.model_name, "llama-3.3-70b-versatile");
        unsafe { std::env::remove_var("RUSTYCLAW_MODEL_NAME"); }
    }

    #[test]
    fn test_resolve_secrets() {
        let mut config = Config {
            model_list: vec![ModelEntry {
                model_name: "test".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                api_base: "https://api.openai.com/v1".to_string(),
                api_key: "$env:TEST_API_KEY_CFG".to_string(),
                max_tokens: Some(2048),
                temperature: Some(0.7),
                enabled: true,
                rpm: None, rpd: None, tpm: None, tpd: None,
                context_window: None,
            }],
            ..Default::default()
        };
        unsafe { std::env::set_var("TEST_API_KEY_CFG", "resolved_key"); }
        config.resolve_secrets();
        assert_eq!(config.model_list[0].api_key, "resolved_key");
        unsafe { std::env::remove_var("TEST_API_KEY_CFG"); }
    }

    #[test]
    fn test_detect_timezone_returns_string() {
        if let Some(tz) = detect_timezone() {
            assert!(!tz.is_empty());
        }
    }

    #[test]
    fn test_model_names_single_deserialization() {
        let s: ModelNames = serde_json::from_str(r#""groq-llama-8b""#).unwrap();
        assert_eq!(s.primary(), "groq-llama-8b");
        assert_eq!(s.as_chain(), vec!["groq-llama-8b"]);
    }

    #[test]
    fn test_model_names_chain_deserialization() {
        let c: ModelNames = serde_json::from_str(r#"["groq-70b", "or-deepseek"]"#).unwrap();
        assert_eq!(c.primary(), "groq-70b");
        assert_eq!(c.as_chain(), vec!["groq-70b", "or-deepseek"]);
    }

    #[test]
    fn test_model_names_mixed_in_config() {
        let json = r#"{ "single": "groq-8b", "chain": ["groq-70b", "or-deepseek"] }"#;
        #[derive(serde::Deserialize)]
        struct Tmp { single: ModelNames, chain: ModelNames }
        let tmp: Tmp = serde_json::from_str(json).unwrap();
        assert_eq!(tmp.single.primary(), "groq-8b");
        assert_eq!(tmp.chain.primary(), "groq-70b");
        assert_eq!(tmp.chain.as_chain().len(), 2);
    }

    fn make_chain_test_config() -> Config {
        Config {
            model_list: vec![
                ModelEntry {
                    model_name: "primary-model".to_string(),
                    provider: "openai".to_string(),
                    model: "primary-api-model".to_string(),
                    api_base: "https://primary.example.com/v1".to_string(),
                    api_key: "key-primary".to_string(),
                    max_tokens: Some(2048),
                    temperature: Some(0.7),
                    enabled: true,
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: None,
                },
                ModelEntry {
                    model_name: "fallback-model".to_string(),
                    provider: "openai".to_string(),
                    model: "fallback-api-model".to_string(),
                    api_base: "https://fallback.example.com/v1".to_string(),
                    api_key: "key-fallback".to_string(),
                    max_tokens: Some(1500),
                    temperature: Some(0.5),
                    enabled: true,
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: None,
                },
                ModelEntry {
                    model_name: "global-model".to_string(),
                    provider: "openai".to_string(),
                    model: "global-api-model".to_string(),
                    api_base: "https://global.example.com/v1".to_string(),
                    api_key: "key-global".to_string(),
                    max_tokens: Some(1024),
                    temperature: Some(0.5),
                    enabled: true,
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: None,
                },
                ModelEntry {
                    model_name: "disabled-model".to_string(),
                    provider: "openai".to_string(),
                    model: "disabled-api-model".to_string(),
                    api_base: "https://disabled.example.com/v1".to_string(),
                    api_key: "key-disabled".to_string(),
                    max_tokens: Some(2048),
                    temperature: Some(0.7),
                    enabled: false,
                    rpm: None, rpd: None, tpm: None, tpd: None,
                    context_window: None,
                },
            ],
            agents: AgentsConfig {
                default: ModelNames::Single("primary-model".to_string()),
                global_fallback_model_name: Some("global-model".to_string()),
                discord: Some(ModelNames::Chain(vec![
                    "primary-model".to_string(),
                    "fallback-model".to_string(),
                ])),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_get_model_chain_returns_primary_and_fallback() {
        let config = make_chain_test_config();
        let chain = config.get_model_chain("discord");
        // discord = [primary, fallback] + global_fallback → 3 entries
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].api_base_url, "https://primary.example.com/v1");
        assert_eq!(chain[1].api_base_url, "https://fallback.example.com/v1");
        assert_eq!(chain[2].api_base_url, "https://global.example.com/v1");
    }

    #[test]
    fn test_get_model_chain_global_fallback_appended() {
        let config = make_chain_test_config();
        let chain = config.get_model_chain("default");
        // default = "primary-model" + global_fallback = "global-model" → 2 entries
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].api_base_url, "https://primary.example.com/v1");
        assert_eq!(chain[1].api_base_url, "https://global.example.com/v1");
    }

    #[test]
    fn test_get_model_chain_global_fallback_dedup() {
        let mut config = make_chain_test_config();
        // discord chain に global-model を含める（重複）
        config.agents.discord = Some(ModelNames::Chain(vec![
            "primary-model".to_string(),
            "global-model".to_string(),
        ]));
        let chain = config.get_model_chain("discord");
        // global-model は重複除去 → 2 entries のみ
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn test_get_model_chain_disabled_model_excluded() {
        let mut config = make_chain_test_config();
        config.agents.discord = Some(ModelNames::Chain(vec![
            "disabled-model".to_string(),
            "fallback-model".to_string(),
        ]));
        let chain = config.get_model_chain("discord");
        // disabled-model はスキップ → fallback-model + global → 2 entries
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].api_base_url, "https://fallback.example.com/v1");
    }

    #[test]
    fn test_get_model_chain_unknown_purpose_uses_default() {
        let config = make_chain_test_config();
        let chain_unknown = config.get_model_chain("unknown-purpose");
        let chain_default = config.get_model_chain("default");
        assert_eq!(chain_unknown.len(), chain_default.len());
        assert_eq!(chain_unknown[0].api_base_url, chain_default[0].api_base_url);
    }

    #[test]
    fn test_get_model_chain_model_purpose_field() {
        let config = make_chain_test_config();
        let chain = config.get_model_chain("discord");
        for entry in &chain {
            assert_eq!(entry.model_purpose, "discord");
        }
    }

    #[test]
    fn test_get_model_backward_compat_with_chain() {
        let config = make_chain_test_config();
        let model = config.get_model("discord");
        let chain = config.get_model_chain("discord");
        assert_eq!(model.api_base_url, chain[0].api_base_url);
        assert_eq!(model.model_purpose, "discord");
    }
}
