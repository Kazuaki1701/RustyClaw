# モデルフォールバックチェーン実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `config.json` の `agents` セクションで各 purpose のモデルを文字列または配列で指定し、primary エラー時に次のモデルへ即フォールバックする機構を実装する。

**Architecture:** `rustyclaw-config` に `ModelNames` enum（`#[serde(untagged)]` で文字列・配列両対応）と `get_model_chain()` を追加。`rustyclaw-agent` の `execute_with_tools()` 内に `complete_with_fallback()` ヘルパーを追加してチェーンを走査する。

**Tech Stack:** Rust / serde_json (`#[serde(untagged)]`) / tokio / anyhow

---

## ファイルマップ

| ファイル | 変更種別 | 概要 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | **変更** | `ModelNames` 追加、`AgentPurposeConfig` 廃止、`AgentsConfig` 変更、`resolve_model()` / `get_model_chain()` 追加、`get_model()` / `override_with_env()` 更新、テスト更新 |
| `crates/rustyclaw-agent/src/lib.rs` | **変更** | `complete_with_fallback()` 追加、`execute_with_tools()` の LLM 呼び出しをフォールバックループに変更 |
| `production/config/config.release.json` | **変更** | `agents` セクションを新形式（文字列・配列）に更新 |
| `production/config/config.debug.json` | **変更** | 同上 |

---

## Task 1: `ModelNames` enum の追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`（74行目付近、`AgentPurposeConfig` の直前に挿入）

- [ ] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-config/src/lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
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
    // 文字列と配列が同一 JSON に混在できることを確認
    let json = r#"{ "single": "groq-8b", "chain": ["groq-70b", "or-deepseek"] }"#;
    #[derive(serde::Deserialize)]
    struct Tmp { single: ModelNames, chain: ModelNames }
    let tmp: Tmp = serde_json::from_str(json).unwrap();
    assert_eq!(tmp.single.primary(), "groq-8b");
    assert_eq!(tmp.chain.primary(), "groq-70b");
    assert_eq!(tmp.chain.as_chain().len(), 2);
}
```

- [ ] **Step 2: テストを実行し失敗を確認する**

```bash
cargo test -p rustyclaw-config test_model_names 2>&1 | tail -10
```

期待: `error[E0412]: cannot find type 'ModelNames'`

- [ ] **Step 3: `ModelNames` enum を実装する**

`crates/rustyclaw-config/src/lib.rs` の 74行目（`/// 用途ごとの LLM 設定...` の直前）に挿入:

```rust
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
```

- [ ] **Step 4: テストを実行しパスを確認する**

```bash
cargo test -p rustyclaw-config test_model_names 2>&1 | tail -5
```

期待: `test result: ok. 3 passed`

- [ ] **Step 5: コミットする**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): add ModelNames enum for string/array model chain"
```

---

## Task 2: `AgentsConfig` 更新・`AgentPurposeConfig` 廃止

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`（80〜98行目の `AgentPurposeConfig` と `AgentsConfig` を変更）

- [ ] **Step 1: `AgentPurposeConfig` を削除し `AgentsConfig` を変更する**

74〜78行目の `AgentPurposeConfig` 定義を **削除**:

```rust
// 削除: AgentPurposeConfig 構造体
```

80〜98行目の `AgentsConfig` を以下に差し替える:

```rust
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
```

- [ ] **Step 2: コンパイルエラーを確認する**

```bash
cargo build -p rustyclaw-config 2>&1 | grep "^error" | head -20
```

期待: `AgentPurposeConfig` を参照する箇所でエラーが出る（テストの `make_test_config` 等）

- [ ] **Step 3: `override_with_env()` を更新する**

`lib.rs` の `override_with_env()` を以下に変更（333〜338行目付近）:

```rust
pub fn override_with_env(&mut self) {
    // RUSTYCLAW_MODEL_NAME: agents.default の model_name を上書き
    if let Ok(val) = std::env::var("RUSTYCLAW_MODEL_NAME") {
        self.agents.default = ModelNames::Single(val);
    }
}
```

- [ ] **Step 4: コンパイルが通ることを確認する**

```bash
cargo build -p rustyclaw-config 2>&1 | grep "^error" | head -10
```

期待: 0 errors（テストの `make_test_config` はまだ壊れていてよい）

- [ ] **Step 5: コミットする**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): replace AgentPurposeConfig with ModelNames in AgentsConfig"
```

---

## Task 3: `resolve_model()` と `get_model_chain()` の追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`（`impl Config` ブロック内）

- [ ] **Step 1: 失敗するテストを書く**

テストブロック末尾に追加:

```rust
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
                enabled: false,  // disabled
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
    // primary, fallback, global (discord chain は [primary, fallback] + global_fallback)
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0].api_base_url, "https://primary.example.com/v1");
    assert_eq!(chain[1].api_base_url, "https://fallback.example.com/v1");
    assert_eq!(chain[2].api_base_url, "https://global.example.com/v1");
}

#[test]
fn test_get_model_chain_global_fallback_appended() {
    let config = make_chain_test_config();
    let chain = config.get_model_chain("default");
    // default = "primary-model" + global_fallback = "global-model"
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].api_base_url, "https://primary.example.com/v1");
    assert_eq!(chain[1].api_base_url, "https://global.example.com/v1");
}

#[test]
fn test_get_model_chain_global_fallback_dedup() {
    let mut config = make_chain_test_config();
    // discord chain に global-model を重複追加
    config.agents.discord = Some(ModelNames::Chain(vec![
        "primary-model".to_string(),
        "global-model".to_string(),  // global_fallback_model_name と同じ
    ]));
    let chain = config.get_model_chain("discord");
    // 重複は除去されるため 2 件
    assert_eq!(chain.len(), 2);
}

#[test]
fn test_get_model_chain_disabled_model_excluded() {
    let mut config = make_chain_test_config();
    config.agents.discord = Some(ModelNames::Chain(vec![
        "disabled-model".to_string(),  // enabled: false
        "fallback-model".to_string(),
    ]));
    let chain = config.get_model_chain("discord");
    // disabled-model はスキップ → fallback-model + global
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
    // 全エントリの model_purpose が "discord" であること
    for entry in &chain { assert_eq!(entry.model_purpose, "discord"); }
}

#[test]
fn test_get_model_backward_compat_with_chain() {
    let config = make_chain_test_config();
    // get_model() は get_model_chain()[0] と同一
    let model = config.get_model("discord");
    let chain = config.get_model_chain("discord");
    assert_eq!(model.api_base_url, chain[0].api_base_url);
    assert_eq!(model.model_purpose, "discord");
}
```

- [ ] **Step 2: テストを実行し失敗を確認する**

```bash
cargo test -p rustyclaw-config test_get_model_chain 2>&1 | tail -5
```

期待: `error[E0599]: no method named 'get_model_chain'`

- [ ] **Step 3: `resolve_model()` と `get_model_chain()` を実装する**

`impl Config` ブロック内の `get_model()` の直前に追加:

```rust
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
```

- [ ] **Step 4: テストを実行しパスを確認する**

```bash
cargo test -p rustyclaw-config test_get_model_chain 2>&1 | tail -5
```

期待: `test result: ok. 6 passed`

```bash
cargo test -p rustyclaw-config test_get_model_backward_compat_with_chain 2>&1 | tail -5
```

期待: `test result: ok. 1 passed`

- [ ] **Step 5: コミットする**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "feat(config): add resolve_model() and get_model_chain() to Config"
```

---

## Task 4: `get_model()` を `get_model_chain()` ベースに更新

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`（`get_model()` メソッドを差し替え）

- [ ] **Step 1: `get_model()` を差し替える**

既存の `get_model()` メソッド（285〜330行目）を以下に差し替える:

```rust
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
```

- [ ] **Step 2: ビルドを確認する**

```bash
cargo build -p rustyclaw-config 2>&1 | grep "^error"
```

期待: エラー 0 件

- [ ] **Step 3: コミットする**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "refactor(config): rewrite get_model() on top of get_model_chain()"
```

---

## Task 5: 既存テストの修正

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`（テスト内の `AgentPurposeConfig` と旧 JSON 形式を更新）

- [ ] **Step 1: `make_test_config()` を更新する**

テスト内の `make_test_config()` 関数（438〜449行目付近）を以下に差し替える:

```rust
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
            default:  ModelNames::Single("test-8b".to_string()),
            summary:  Some(ModelNames::Single("test-70b".to_string())),
            memory:   Some(ModelNames::Single("test-70b".to_string())),
            tools:    None,
            discord:  None,
            line:     None,
            heartbeat: None,
            patrol:   None,
            global_fallback_model_name: None,
        },
        ..Default::default()
    }
}
```

- [ ] **Step 2: `test_load_config_success()` の JSON と assertion を更新する**

テスト内の JSON 文字列（468〜469行目）を変更:

```rust
// 変更前:
"agents": {
    "default": { "model_name": "groq-8b" }
}
// 変更後:
"agents": {
    "default": "groq-8b"
}
```

assertion 479行目を変更:

```rust
// 変更前: assert_eq!(config.agents.default.model_name, "groq-8b");
// 変更後:
assert_eq!(config.agents.default.primary(), "groq-8b");
```

- [ ] **Step 3: `test_get_model_new_purposes()` の `AgentPurposeConfig` を `ModelNames` に変更する**

518〜520行目付近:

```rust
// 変更前:
config.agents.discord   = Some(AgentPurposeConfig { model_name: "test-70b".to_string() });
config.agents.heartbeat = Some(AgentPurposeConfig { model_name: "test-70b".to_string() });
config.agents.tools     = Some(AgentPurposeConfig { model_name: "test-70b".to_string() });
// 変更後:
config.agents.discord   = Some(ModelNames::Single("test-70b".to_string()));
config.agents.heartbeat = Some(ModelNames::Single("test-70b".to_string()));
config.agents.tools     = Some(ModelNames::Single("test-70b".to_string()));
```

- [ ] **Step 4: `test_env_override()` の assertion を更新する**

531行目付近:

```rust
// 変更前: assert_eq!(config.agents.default.model_name, "test-70b");
// 変更後:
assert_eq!(config.agents.default.primary(), "test-70b");
```

- [ ] **Step 5: 全テストを実行しパスを確認する**

```bash
cargo test -p rustyclaw-config 2>&1 | tail -10
```

期待: `test result: ok. N passed, 0 failed`

- [ ] **Step 6: コミットする**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "fix(config): update tests for ModelNames / AgentPurposeConfig removal"
```

---

## Task 6: `config.json` ファイルを新形式に更新

**Files:**
- Modify: `production/config/config.release.json`
- Modify: `production/config/config.debug.json`

- [ ] **Step 1: `config.release.json` の `agents` セクションを確認する**

```bash
grep -A 20 '"agents"' production/config/config.release.json
```

- [ ] **Step 2: `config.release.json` の `agents` セクションを新形式に更新する**

現在の形式（例）:
```json
"agents": {
  "default":   { "model_name": "groq-llama-8b" },
  "tools":     { "model_name": "groq-qwen3-32b" },
  "discord":   { "model_name": "groq-llama-70b" },
  ...
}
```

新形式（実際のモデル名は既存の値をそのまま使用）:
```json
"agents": {
  "global_fallback_model_name": "or-llama-3.3-free",
  "default":   "groq-llama-8b",
  "tools":     "groq-qwen3-32b",
  "discord":   ["groq-llama-70b", "or-deepseek-v4-flash"],
  "heartbeat": ["groq-qwen3-32b", "groq-llama-8b"],
  "summary":   "groq-llama-8b",
  "memory":    "groq-llama-8b",
  "patrol":    "groq-qwen3-32b"
}
```

※ フォールバックモデルの選択は既存の `model_list` に存在するモデル名を使用すること。

- [ ] **Step 3: `config.debug.json` を同様に更新する**

`production/config/config.debug.json` も同形式で更新（debug 環境のモデル設定に合わせて）。

- [ ] **Step 4: ロード確認（`--no-agent` で起動テスト）**

```bash
cargo build --release -p rustyclaw-cli 2>&1 | grep "^error"
```

期待: エラー 0 件

```bash
target/release/rustyclaw-cli --config production/config/config.release.json --no-agent gateway &
sleep 3 && curl -s http://127.0.0.1:8080/api/concurrency && kill %1
```

期待: JSON レスポンスが返る（config パースエラーなし）

- [ ] **Step 5: コミットする**

```bash
git add production/config/config.release.json production/config/config.debug.json
git commit -m "chore(config): migrate agents section to ModelNames array format"
```

---

## Task 7: `complete_with_fallback()` の追加と `execute_with_tools()` の更新

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: 失敗するテストを書く**

テスト末尾に追加（既存テストの `#[cfg(test)]` ブロック内）:

```rust
#[tokio::test]
async fn test_complete_with_fallback_skips_disabled_model() {
    // disabled モデルが先頭の場合、get_model_chain が有効モデルだけを返すことを確認
    // （complete_with_fallback の前提条件としてのチェーン構成テスト）
    let config = rustyclaw_config::Config {
        model_list: vec![
            rustyclaw_config::ModelEntry {
                model_name: "disabled-m".to_string(),
                provider: "openai".to_string(),
                model: "disabled-api".to_string(),
                api_base: "https://disabled.example.com/v1".to_string(),
                api_key: "key".to_string(),
                max_tokens: Some(100),
                temperature: Some(0.5),
                enabled: false,  // disabled
                rpm: None, rpd: None, tpm: None, tpd: None,
                context_window: None,
            },
            rustyclaw_config::ModelEntry {
                model_name: "enabled-m".to_string(),
                provider: "openai".to_string(),
                model: "enabled-api".to_string(),
                api_base: "https://enabled.example.com/v1".to_string(),
                api_key: "key2".to_string(),
                max_tokens: Some(100),
                temperature: Some(0.5),
                enabled: true,
                rpm: None, rpd: None, tpm: None, tpd: None,
                context_window: None,
            },
        ],
        agents: rustyclaw_config::AgentsConfig {
            default: rustyclaw_config::ModelNames::Chain(vec![
                "disabled-m".to_string(),
                "enabled-m".to_string(),
            ]),
            ..Default::default()
        },
        ..Default::default()
    };
    // disabled は除外、enabled のみがチェーンに残る
    let chain = config.get_model_chain("default");
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].api_base_url, "https://enabled.example.com/v1");
}
```

- [ ] **Step 2: テストを実行し確認する**

```bash
cargo test -p rustyclaw-agent test_complete_with_fallback 2>&1 | tail -5
```

期待: `test result: ok. 1 passed`（config レイヤーのテストなのでパスする）

- [ ] **Step 3: `complete_with_fallback()` を `Pipeline` に追加する**

`impl Pipeline` ブロック内（`get_history_limit()` の前あたり）に追加:

```rust
/// モデルチェーンを走査し、最初に成功したレスポンスを返す。
/// チェーン内の全モデルが失敗した場合はエラーを返す。
/// フォールバックモデルが使用された場合は warn! ログを出力する。
async fn complete_with_fallback(
    &self,
    purpose: &str,
    session_id: &str,
    messages: &[Message],
    tools: &[rustyclaw_providers::ToolDef],
    timeout: std::time::Duration,
) -> Result<LlmResponse> {
    let chain = self.config.get_model_chain(purpose);
    if chain.is_empty() {
        return Err(anyhow::anyhow!("no available models for purpose: {}", purpose));
    }

    let category = resolve_category(session_id);
    let mut last_err: Option<rustyclaw_providers::ProviderError> = None;

    for (idx, model_cfg) in chain.iter().enumerate() {
        let opts = CompletionOptions {
            model: model_cfg.model_name.clone(),
            max_tokens: model_cfg.max_tokens,
            temperature: model_cfg.temperature,
            timeout,
            category: Some(category.clone()),
        };
        let provider = create_provider(model_cfg.clone());
        match provider.complete(messages, tools, &opts).await {
            Ok(response) => {
                if idx > 0 {
                    tracing::warn!(
                        purpose = purpose,
                        used_model = %model_cfg.model_name,
                        primary_model = %chain[0].model_name,
                        fallback_index = idx,
                        "fallback model used"
                    );
                }
                return Ok(response);
            }
            Err(e) => {
                tracing::warn!(
                    model = %model_cfg.model_name,
                    error = %e,
                    "model failed, trying next in chain"
                );
                last_err = Some(e);
            }
        }
    }

    Err(anyhow::anyhow!(
        "all models failed for purpose '{}': {}",
        purpose,
        last_err.map(|e| e.to_string()).unwrap_or_default()
    ))
}
```

- [ ] **Step 4: `execute_with_tools()` の LLM 呼び出しを差し替える**

910〜918行目付近（`let model_cfg = ...` から `let opts = ...` まで）を **削除** し、935行目付近の `self.provider.complete(...)` を差し替える:

```rust
// 削除するコード (910-918行付近):
// let model_cfg = self.config.get_model(purpose);
// let cat = resolve_category(session_id);
// let opts = CompletionOptions { ... };

// 変更前 (935行付近):
// let mut response = self.provider.complete(&active_messages, &provider_tools, &opts).await?;

// 変更後:
let mut response = self.complete_with_fallback(
    purpose,
    session_id,
    &active_messages,
    &provider_tools,
    Duration::from_secs(300),
).await?;
```

- [ ] **Step 5: ビルドと全テスト実行**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error"
```

期待: エラー 0 件

```bash
cargo test -p rustyclaw-agent 2>&1 | tail -10
```

期待: `test result: ok. N passed, 0 failed`

```bash
cargo test -p rustyclaw-config 2>&1 | tail -5
```

期待: `test result: ok. N passed, 0 failed`

- [ ] **Step 6: ワークスペース全体のテスト確認**

```bash
cargo test --workspace 2>&1 | tail -15
```

期待: 全クレートで `ok` / `0 failed`

- [ ] **Step 7: コミットする**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): add complete_with_fallback() and use model chain in execute_with_tools"
```

---

## 最終確認

- [ ] **`--no-agent` で gateway を起動し、config パースエラーがないことを確認する**

```bash
cargo build --release -p rustyclaw-cli
target/release/rustyclaw-cli --config production/config/config.release.json --no-agent gateway &
sleep 3 && curl -s http://127.0.0.1:8080/api/concurrency | python3 -m json.tool && kill %1
```

- [ ] **task.md の Phase 24 (Model Offloader) を部分完了として更新する**

`docs/task.md` の Phase 24 item 2（クォータ枯渇時の自動モデルオフローダー）にチェックを入れる。
