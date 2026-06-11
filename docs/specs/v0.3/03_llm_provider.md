# RustyClaw — LlmProvider 仕様

> [!NOTE]
> **ステータス**: `[実装済]`（rig-core への段階移行のみ `[将来拡張]`）
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw_hermes_featured.md`](00_rustyclaw_hermes_featured.md)

---

## 8. LlmProvider 設計 `[実装済]`

### 8.1 重要な設計原則

**LlmProvider は完全ステートレス。** 毎回の API 呼び出しは「初対面」。
会話が続いている感覚はすべて Rust コード（ConversationHistory）が作り出す。

### 8.2 trait 定義

```rust
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools:    &[ToolDef],
        opts:     &CompletionOptions,
    ) -> Result<LlmResponse>;

    async fn complete_stream(
        &self,
        messages: &[Message],
        tools:    &[ToolDef],
        opts:     &CompletionOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>>;
}

pub struct CompletionOptions {
    pub model:        String,
    pub max_tokens:   u32,
    pub timeout:      Duration,           // デフォルト 15 分
    pub cancel_token: CancellationToken,  // turn キャンセル用
}
```

### 8.3 ファクトリ

```rust
pub fn create_provider(cfg: &ModelConfig) -> Box<dyn LlmProvider> {
    match cfg.protocol.as_str() {
        "openai"    => Box::new(OpenAiCompatProvider::new(cfg)),
        "anthropic" => Box::new(AnthropicProvider::new(cfg)),
        "gemini"    => Box::new(GeminiProvider::new(cfg)),
        "ollama"    => Box::new(OllamaProvider::new(cfg)),
        _           => panic!("unknown provider: {}", cfg.protocol),
    }
}
```

`[将来拡張]` rig-core 統合後は `rig::providers` ベースの実装へ段階移行（§15 参照）。

---

## 用途別 model_purpose 一覧 `[実装済]`

`config.json` の `models` 配列で、処理の目的ごとに異なる LLM・パラメータを割り当てられる。

### purpose 定義

| purpose | 呼び出し元 | 特性 | 推定頻度/日 |
|---|---|---|---|
| `default` | `execute` / `execute_stream` | 対話・応答速度優先 | 高（会話の都度） |
| `tools` | `execute_with_tools`（非チャンネル） | ツール呼び出し・推論優先 | 中 |
| `discord` | Discord メッセージ dispatch | 日本語会話品質 ＋ ツール呼び出し | 中 |
| `line` | LINE メッセージ dispatch（予約） | 日本語特化・LINE 実装後に有効化 | 未定 |
| `heartbeat` | `execute_heartbeat` | 定期実行（30分毎） | ~48 |
| `summary` | `generate_session_summary` | 構造化テキスト品質優先 | 低（セッション終了時） |
| `memory` | `flush_memory` | 精度・低コスト優先 | 低（セッション終了時） |

### スマート継承（省略時のデフォルトフォールバック）

`models` 配列内で省略されたフィールドは、ルート階層（第1レベル）の設定値を自動継承する。

```json
{
  "model_provider": "openai",
  "model_name": "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
  "api_key": "$vault:cf-token",
  "models": [
    { "model_purpose": "default" },
    {
      "model_purpose": "summary",
      "model_provider": "gmn",
      "model_name": "gemini-2.5-flash",
      "temperature": 0.3
    },
    {
      "model_purpose": "memory",
      "model_name": "@cf/meta/llama-3-8b-instruct",
      "temperature": 0.3
    }
  ]
}
```

### 用途別モデル割り当て（現状設定）

| purpose | モデル | Provider | 根拠 |
|---|---|---|---|
| `default` | groq-llama-8b | Groq | 応答速度最優先。RPD 14,400 で対話頻度を吸収 |
| `tools` | groq-qwen3-32b | Groq | ツール呼び出し・推論特化 |
| `discord` | groq-llama-70b | Groq | 70B で日本語品質向上 |
| `line` | groq-llama-70b | Groq | discord と共用。LINE 実装まで予約 |
| `heartbeat` | groq-llama-8b | Groq | 48回/日の高頻度。default と同モデル共用 |
| `summary` | cf-gemma-4-26b | Cloudflare | 1日数回・256k context |
| `memory` | cf-qwen3-30b | Cloudflare | 1日数回・32k context で十分 |

---

## Provider 選定・レートリミット `[参照情報]`

> **注意**: レートリミット・モデル一覧は変動が多い。最新情報は cheahjs/free-llm-api-resources 等で確認すること。

### Provider 特性比較

| Provider | 強み | 弱み |
|---|---|---|
| **Groq** | 超高速（LPU）・高 RPD（llama-8b: 14,400） | context 131k 固定 |
| **Cloudflare** | RPM 300・RPD/TPD 制限なし・256k context | Neurons 10,000/日上限 |
| **OpenRouter** | 1M context・大型モデル無料 | RPD **50**（最大の制約） |
| **Google AI Studio** | Gemma 3 が 14,400 RPD・無料 | 日本からの利用はデータ学習対象 |
| **Cerebras** | 14,400 RPD・TPM/TPD 余裕大・gpt-oss-120B 無料 | モデル数少ない |
| **Hugging Face** | RPD 1,000・5M tokens/月 | 現代 LLM は実質 context 2k〜4k で不向き |

### Groq Free Tier レートリミット（主要モデル）

| model_name | model | RPM | RPD | TPD | Context |
|---|---|---|---|---|---|
| groq-llama-8b | llama-3.1-8b-instant | 30 | **14,400** | 500,000 | 131,072 |
| groq-llama-70b | llama-3.3-70b-versatile | 30 | 1,000 | 100,000 | 131,072 |
| groq-qwen3-32b | qwen/qwen3-32b | 60 | 1,000 | 500,000 | 131,072 |

### Cloudflare Workers AI Free Tier

| 項目 | 値 |
|---|---|
| RPM | 300（全モデル共通） |
| RPD / TPD | なし（トークン数制限なし） |
| Neurons/日 | **10,000** |

Neurons 使用量は `~/.rustyclaw/neuron_usage.json` に UTC 日付単位で累積保存。`/api/neurons` エンドポイントで参照可能。

### Provider 日次予算試算（現状）

| Provider | 用途 | 消費量/日 | 上限 |
|---|---|---|---|
| Groq llama-8b | default + heartbeat | ~194K tokens | TPD 500K ✓ |
| Groq qwen3-32b | tools | ~60K tokens | TPD 500K ✓ |
| Groq llama-70b | discord | ~25K tokens | TPD 100K ✓ |
| CF gemma-4-26b | summary | ~177 neurons | 10,000/日 ✓ |
| CF qwen3-30b | memory | ~150 neurons | 10,000/日 ✓ |
| **CF 合計** | | **~327 neurons/日** | **余裕 97%** |

### Provider 追加候補 `[将来拡張]`

レートリミット・品質の観点で追加を検討中の Provider とモデル。

| Provider | モデル候補 | 特性 |
|---|---|---|
| **Cerebras** | `gpt-oss-120b` | 14,400 RPD・TPM/TPD 余裕大・120B 無料 |
| **Google AI Studio** | Gemma 3 27B | 14,400 RPD・無料（日本からはデータ学習対象に注意） |
| **OpenRouter** | `qwen3-coder:free`・`qwen3-next-80b:free` | 1M context・新モデル随時追加 |
