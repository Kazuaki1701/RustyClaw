# Design: model_purpose フォールバックチェーン

> **ステータス**: `[DESIGN]` 承認済み・実装前  
> **作成日**: 2026-05-31  
> **対象クレート**: `rustyclaw-config`, `rustyclaw-gateway`

---

## 1. 目的

`config.json` の `agents` セクションにおける各 purpose のモデル指定に、**フォールバックチェーン**を持たせる。primary モデルがエラーを返した場合、即座に次のモデルへ移行し、すべて失敗した場合はグローバルフォールバックモデルを最終手段として使う。

---

## 2. Config スキーマ（新形式）

### 2-1. 記述形式

```json
"agents": {
  "global_fallback_model_name": "or-llama-3.3-free",
  "default":   "groq-llama-8b",
  "discord":   ["groq-llama-70b", "or-deepseek-v4-flash", "or-minimax-m2.5"],
  "heartbeat": ["groq-qwen3-32b", "groq-llama-8b"],
  "tools":     "groq-qwen3-32b",
  "summary":   "groq-llama-8b"
}
```

- **文字列**: primary モデルのみ（フォールバックなし）
- **配列**: `[primary, fallback1, fallback2, ...]` の順に試行
- 文字列と配列は同一 config 内で混在可能
- `global_fallback_model_name`: purpose チェーンをすべて消費した後の最終保険（省略可）

### 2-2. 破壊的変更

旧形式 `{ "model_name": "..." }` は廃止。既存の `config.release.json`・`config.debug.json` を新形式に更新する。

---

## 3. Rust 型定義

### 3-1. `ModelNames` enum（新規）

```rust
/// JSON 文字列 "foo" と JSON 配列 ["foo", "bar"] の両方をデシリアライズ
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelNames {
    Single(String),
    Chain(Vec<String>),
}

impl ModelNames {
    pub fn primary(&self) -> &str {
        match self {
            Self::Single(s) => s,
            Self::Chain(v)  => v.first().map(|s| s.as_str()).unwrap_or(""),
        }
    }

    pub fn as_chain(&self) -> Vec<&str> {
        match self {
            Self::Single(s) => vec![s.as_str()],
            Self::Chain(v)  => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}
```

### 3-2. `AgentsConfig`（変更）

`AgentPurposeConfig` 構造体を廃止し、purpose フィールドが直接 `ModelNames` を持つ:

```rust
pub struct AgentsConfig {
    pub default:  ModelNames,
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

---

## 4. `Config` メソッド変更

### 4-1. 新規: `get_model_chain()`

```rust
/// purpose のモデルチェーンを解決済み LlmModelConfig のリストとして返す。
/// 順序: purpose指定モデル群 → global_fallback（重複除去）
/// disabled なモデルはリストから除外される。
pub fn get_model_chain(&self, purpose: &str) -> Vec<LlmModelConfig>
```

**解決ロジック:**
1. purpose に対応する `ModelNames` を取得（未設定なら `default`）
2. `as_chain()` で `Vec<&str>` に展開
3. `global_fallback_model_name` を末尾に追加（重複は除去）
4. 各 model_name を `model_list` で解決 → `enabled: false` は除外

### 4-2. 変更: `get_model()`（後方互換維持）

```rust
/// 後方互換。get_model_chain()[0] を返す。
/// 既存の呼び出し元はこのまま使用可能。
pub fn get_model(&self, purpose: &str) -> LlmModelConfig {
    self.get_model_chain(purpose).into_iter().next().unwrap_or_default()
}
```

### 4-3. 新規: `resolve_model()`（内部ヘルパー）

`model_name` 文字列 → `LlmModelConfig` への解決ロジックを `get_model()` から抽出・共通化。

---

## 5. フォールバックループ（呼び出し元）

`rustyclaw-gateway/src/lib.rs` の LLM 呼び出し箇所:

```rust
let chain = config.get_model_chain(purpose);
let mut last_err = None;

for (idx, model_config) in chain.iter().enumerate() {
    match provider_for(model_config).complete(&messages, model_config).await {
        Ok(response) => {
            if idx > 0 {
                tracing::warn!(
                    purpose = purpose,
                    used_model = %model_config.model_name,
                    primary_model = %chain[0].model_name,
                    fallback_index = idx,
                    "fallback model used"
                );
            }
            return Ok(response);
        }
        Err(e) => {
            tracing::warn!(
                model = %model_config.model_name,
                error = %e,
                "model failed, trying next in chain"
            );
            last_err = Some(e);
        }
    }
}

Err(last_err.unwrap_or_else(|| anyhow!("no available models for purpose: {}", purpose)))
```

---

## 6. テストケース

| # | テスト名 | 検証内容 |
|---|---|---|
| ① | `test_model_names_single_deserialization` | 文字列 → `ModelNames::Single` |
| ② | `test_model_names_chain_deserialization` | 配列 → `ModelNames::Chain` |
| ③ | `test_mixed_single_and_chain` | 同一 config で文字列・配列が混在 |
| ④ | `test_global_fallback_appended` | global_fallback がチェーン末尾に追加 |
| ⑤ | `test_global_fallback_dedup` | global_fallback が purpose チェーンに既存なら重複除去 |
| ⑥ | `test_disabled_model_excluded_from_chain` | disabled モデルはチェーンから除外 |
| ⑦ | `test_unknown_purpose_uses_default` | 未定義 purpose は default チェーンを使用 |
| ⑧ | `test_get_model_backward_compat` | `get_model()` は `get_model_chain()[0]` と一致 |

---

## 7. 変更ファイル一覧

| ファイル | 変更種別 | 概要 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | 変更 | `ModelNames` 追加、`AgentPurposeConfig` 廃止、`AgentsConfig` 変更、`get_model_chain()` / `resolve_model()` 追加 |
| `crates/rustyclaw-gateway/src/lib.rs` | 変更 | LLM 呼び出し箇所をフォールバックループに変更 |
| `production/config/config.release.json` | 変更 | 新形式に更新 |
| `production/config/config.debug.json` | 変更 | 新形式に更新 |
