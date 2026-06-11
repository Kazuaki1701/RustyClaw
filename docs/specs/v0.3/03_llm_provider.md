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
