# Plan: Prompt Budget & Compaction Threshold (Phase 37-4)

エージェントの送信プロンプトのコンテキストトークン数を制御し、日本語補正を含むコンテキスト制限（TPM）を超えないようにするため、`config.json` に `prompt_budget` の上限値を導入し、会話履歴の 70/20/10 圧縮（コンパクション）と連動させます。

---

## 1. 概要と要件

### プロンプト予算の導入
- `Config` 構造体および `config.json` のルートレベルに `prompt_budget: Option<usize>` を追加します。

### 会話圧縮のしきい値との動的連動
- LLM 送信プロンプト構築時（`system_context` + `history` + `user_message`）に、`system_context` と `user_message` の合計文字数からオーバーヘッドとなる推定トークン数を算出します。
- `prompt_budget` が設定されている場合、`ConversationHistory::compact_if_needed_with_overhead(prompt_budget, overhead)` を実行し、会話履歴を 70/20/10 の比率で自動的に圧縮します。
- 圧縮のトリガーしきい値は `prompt_budget` からオーバーヘッドを引いた「実効上限」の **80%** となり、これに動的に連動します。
- 圧縮処理を適用した後、モデルの最大コンテキストサイズに基づいた安全策として、従来の `trim_to_last` ハードキャップも適用します。

---

## 2. 具体的な実装設計

### ① `crates/rustyclaw-config/src/lib.rs` (Config 構造体の拡張)
```rust
pub struct Config {
    // ...
    /// プロンプト予算（トークン数上限）
    #[serde(default)]
    pub prompt_budget: Option<usize>,
}
```

### ② `crates/rustyclaw-agent/src/lib.rs` (Pipeline での圧縮連動)
`Pipeline` に `compact_history_if_needed` 内部メソッドを追加します。

```rust
    fn compact_history_if_needed(
        &self,
        history: &mut ConversationHistory,
        system_context: &str,
        user_message: &str,
    ) {
        if let Some(budget) = self.config.prompt_budget {
            let system_chars = system_context.chars().count();
            let user_chars = user_message.chars().count();
            // ConversationHistory::estimate_tokens と同様の 1.5 倍補正係数を使用
            let overhead = ((system_chars + user_chars) * 3) / 2;
            history.compact_if_needed_with_overhead(budget, overhead);
        }
    }
```

このメソッドを以下の3つのメッセージ構築処理に組み込みます：
1. `Pipeline::run` (line 1087 付近)
2. `Pipeline::execute_with_tools` (line 1423 付近)
3. `Pipeline::execute_stream` (line 1539 付近)

---

## 3. テスト計画

### ユニットテストの作成
- `test_prompt_budget_trigger_compaction`: `prompt_budget` が設定されており、履歴＋オーバーヘッドがしきい値を超える場合に `70/20/10` 圧縮がトリガーされることを検証。
- `test_prompt_budget_no_trigger`: しきい値を超えない場合は圧縮がスキップされることを検証。
