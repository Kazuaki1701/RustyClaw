# Phase 19: Purpose-Based Model Assignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 7つの `purpose`（default / tools / discord / line / heartbeat / summary / memory）それぞれに最適な LLM モデルを割り当て、Provider 分散を実現する。

**Architecture:** `AgentsConfig` に新 purpose フィールドを追加し、`get_model()` の fallback ロジックを拡張。`execute_with_tools()` に `purpose` 引数を追加して呼び出し元（gateway）がチャンネル種別を渡せるようにする。`execute_heartbeat()` は `get_model("heartbeat")` を使用するよう変更。

**Tech Stack:** Rust / serde_json / tokio — 既存クレート内の変更のみ。新依存なし。

---

## 変更ファイル一覧

| ファイル | 変更内容 |
|---------|---------|
| `crates/rustyclaw-config/src/lib.rs` | `AgentsConfig` に 4 フィールド追加・`get_model()` 拡張 |
| `crates/rustyclaw-agent/src/lib.rs` | `execute_heartbeat()` purpose 変更・`execute_with_tools()` に purpose 引数追加 |
| `crates/rustyclaw-gateway/src/lib.rs` | Discord dispatch に "discord" 引数を渡す |
| `production/config/config.json` | agents 全 purpose 設定・CF/HF モデル有効化 |

---

## Task 1: AgentsConfig 拡張と get_model() フォールバック追加

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs:82-92`（AgentsConfig）
- Modify: `crates/rustyclaw-config/src/lib.rs:265-295`（get_model）
- Test: `crates/rustyclaw-config/src/lib.rs`（既存 tests モジュール内）

- [ ] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-config/src/lib.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
#[test]
fn test_get_model_new_purposes() {
    let mut config = make_test_config();
    // tools / discord / line / heartbeat が未設定 → default にフォールバック
    assert_eq!(config.get_model("tools").model_name,     "llama-3.1-8b-instant");
    assert_eq!(config.get_model("discord").model_name,   "llama-3.1-8b-instant");
    assert_eq!(config.get_model("line").model_name,      "llama-3.1-8b-instant");
    assert_eq!(config.get_model("heartbeat").model_name, "llama-3.1-8b-instant");

    // 明示設定した場合はそちらを返す
    config.agents.discord   = Some(AgentPurposeConfig { model_name: "test-70b".to_string() });
    config.agents.heartbeat = Some(AgentPurposeConfig { model_name: "test-70b".to_string() });
    config.agents.tools     = Some(AgentPurposeConfig { model_name: "test-70b".to_string() });
    assert_eq!(config.get_model("discord").model_name,   "llama-3.3-70b-versatile");
    assert_eq!(config.get_model("heartbeat").model_name, "llama-3.3-70b-versatile");
    assert_eq!(config.get_model("tools").model_name,     "llama-3.3-70b-versatile");
}
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cargo test -p rustyclaw-config test_get_model_new_purposes 2>&1 | tail -5
```
期待: `FAILED` または `error[E0609]`（フィールド未定義）

- [ ] **Step 3: AgentsConfig にフィールドを追加**

`crates/rustyclaw-config/src/lib.rs` の `AgentsConfig` を以下に置き換える:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentsConfig {
    pub default: AgentPurposeConfig,
    #[serde(default)]
    pub summary: Option<AgentPurposeConfig>,
    #[serde(default)]
    pub memory: Option<AgentPurposeConfig>,
    #[serde(default)]
    pub tools: Option<AgentPurposeConfig>,
    #[serde(default)]
    pub discord: Option<AgentPurposeConfig>,
    #[serde(default)]
    pub line: Option<AgentPurposeConfig>,
    #[serde(default)]
    pub heartbeat: Option<AgentPurposeConfig>,
}
```

- [ ] **Step 4: get_model() に新 purpose を追加**

`get_model()` 内の `match purpose` ブロックを以下に置き換える:

```rust
let model_name = match purpose {
    "summary" => self.agents.summary.as_ref()
        .map(|s| s.model_name.as_str())
        .unwrap_or(self.agents.default.model_name.as_str()),
    "memory" => self.agents.memory.as_ref()
        .map(|s| s.model_name.as_str())
        .unwrap_or(self.agents.default.model_name.as_str()),
    "tools" => self.agents.tools.as_ref()
        .map(|s| s.model_name.as_str())
        .unwrap_or(self.agents.default.model_name.as_str()),
    "discord" => self.agents.discord.as_ref()
        .map(|s| s.model_name.as_str())
        .unwrap_or(self.agents.default.model_name.as_str()),
    "line" => self.agents.line.as_ref()
        .map(|s| s.model_name.as_str())
        .unwrap_or(self.agents.default.model_name.as_str()),
    "heartbeat" => self.agents.heartbeat.as_ref()
        .map(|s| s.model_name.as_str())
        .unwrap_or(self.agents.default.model_name.as_str()),
    _ => self.agents.default.model_name.as_str(),
};
```

- [ ] **Step 5: 既存テストのコメント更新**

同ファイル `test_get_model_purpose_fallback` 内の heartbeat テストコメントを更新:

```rust
// heartbeat は明示設定なし → default にフォールバック（設定済みなら専用モデルを返す）
let unknown = config.get_model("heartbeat");
assert_eq!(unknown.model_name, "llama-3.1-8b-instant");
```

- [ ] **Step 6: テストが通ることを確認**

```bash
cargo test -p rustyclaw-config 2>&1 | tail -5
```
期待: `test result: ok. N passed`

- [ ] **Step 7: コミット**

```bash
git -C /mnt/Projects/RustyClaw add crates/rustyclaw-config/src/lib.rs
git -C /mnt/Projects/RustyClaw commit -m "feat(config): add tools/discord/line/heartbeat to AgentsConfig with fallback"
```

---

## Task 2: execute_heartbeat() を get_model("heartbeat") に変更

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:486-488`

- [ ] **Step 1: 変更対象行を確認**

```bash
grep -n 'get_model("default")' /mnt/Projects/RustyClaw/crates/rustyclaw-agent/src/lib.rs
```
期待: `execute_heartbeat` 内の 486・487・488 行目に 3 件表示される

- [ ] **Step 2: execute_heartbeat 内のモデル取得を置き換える**

`execute_heartbeat()` 内で `CompletionOptions` を構築している箇所（約 485 行目付近）の 3 行を以下に置き換える:

```rust
        let heartbeat_model = self.config.get_model("heartbeat");
        let opts = CompletionOptions {
            model: heartbeat_model.model_name,
            max_tokens: heartbeat_model.max_tokens,
            temperature: heartbeat_model.temperature,
```

- [ ] **Step 3: ビルドが通ることを確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep -E "^error|Finished"
```
期待: `Finished`

- [ ] **Step 4: コミット**

```bash
git -C /mnt/Projects/RustyClaw add crates/rustyclaw-agent/src/lib.rs
git -C /mnt/Projects/RustyClaw commit -m "feat(agent): execute_heartbeat uses get_model(heartbeat)"
```

---

## Task 3: execute_with_tools() に purpose 引数を追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:758-964`（execute_with_tools 関数）

- [ ] **Step 1: 関数シグネチャに purpose 引数を追加**

`execute_with_tools` の定義（758 行目付近）を以下に変更:

```rust
    pub async fn execute_with_tools(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
        tool_registry: &ToolRegistry,
        purpose: &str,
    ) -> Result<LlmResponse> {
```

- [ ] **Step 2: 関数内のモデル取得を purpose ベースに変更**

関数内に `get_model("default")` が 6 箇所（819・820・821・947・948・949 行）ある。
それぞれ `purpose` を使うよう、**2 箇所の CompletionOptions 構築** を以下のパターンで置き換える:

1 回目（819-821 付近）:
```rust
        let model_cfg = self.config.get_model(purpose);
        let opts = CompletionOptions {
            model: model_cfg.model_name.clone(),
            max_tokens: model_cfg.max_tokens,
            temperature: model_cfg.temperature,
```

2 回目（947-949 付近、ツールループ内の再呼び出し）:
```rust
        let model_cfg = self.config.get_model(purpose);
        let opts = CompletionOptions {
            model: model_cfg.model_name.clone(),
            max_tokens: model_cfg.max_tokens,
            temperature: model_cfg.temperature,
```

> **注意**: 各箇所で `model_cfg` を再取得しているが、同じ設定オブジェクトなので重複は軽微。

- [ ] **Step 3: ビルドエラーを確認（gateway がコンパイルエラーになるはず）**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep -E "^error|Finished"
```
期待: agent 単体は `Finished`（gateway はまだ古い呼び出し形式のためエラー）

- [ ] **Step 4: コミット（Task 4 と連続で行う）**

この段階では gateway が壊れているためコミットは Task 4 完了後にまとめて行う。

---

## Task 4: Gateway の Discord dispatch に purpose を渡す

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs:477`

- [ ] **Step 1: execute_with_tools の呼び出しを更新**

`lib.rs` 477 行目付近:

```rust
// 変更前
pipeline.execute_with_tools(&workspace_path, &session_id, &content, &tool_reg).await

// 変更後
pipeline.execute_with_tools(&workspace_path, &session_id, &content, &tool_reg, "discord").await
```

- [ ] **Step 2: ビルドが通ることを確認**

```bash
cargo build -p rustyclaw-cli 2>&1 | grep -E "^error|Finished"
```
期待: `Finished`

- [ ] **Step 3: 全テストが通ることを確認**

```bash
cargo test 2>&1 | tail -5
```
期待: `test result: ok`

- [ ] **Step 4: コミット（Task 3 + 4 まとめて）**

```bash
git -C /mnt/Projects/RustyClaw add \
  crates/rustyclaw-agent/src/lib.rs \
  crates/rustyclaw-gateway/src/lib.rs
git -C /mnt/Projects/RustyClaw commit -m \
  "feat(agent/gateway): execute_with_tools accepts purpose, discord uses get_model(discord)"
```

---

## Task 5: config.json — agents 全設定・モデル有効化

**Files:**
- Modify: `production/config/config.json`

- [ ] **Step 1: agents セクションを全 purpose で更新**

`config.json` の `"agents"` を以下に置き換える:

```json
  "agents": {
    "default":   { "model_name": "groq-llama-8b" },
    "tools":     { "model_name": "groq-qwen3-32b" },
    "discord":   { "model_name": "hf-qwen2.5-7b" },
    "line":      { "model_name": "hf-qwen2.5-7b" },
    "heartbeat": { "model_name": "groq-llama-8b" },
    "summary":   { "model_name": "cf-gemma-4-26b" },
    "memory":    { "model_name": "cf-qwen3-30b" }
  },
```

- [ ] **Step 2: HF モデルを有効化**

`hf-qwen2.5-7b` エントリの `"enabled": false` を `"enabled": true` に変更。

- [ ] **Step 3: CF モデルを有効化**

`cf-gemma-4-26b` エントリの `"enabled": false` を `"enabled": true` に変更。
`cf-qwen3-30b` エントリの `"enabled": false` を `"enabled": true` に変更。

- [ ] **Step 4: check-config でローカル検証**

```bash
VAULT_PASSPHRASE="" cargo run -p rustyclaw-cli -- \
  --config production/config/config.json check-config 2>&1 | grep -E "\[ERR\]|\[OK\].*agents|Result"
```
期待:
```
[OK]   agents.default   → 'groq-llama-8b' (enabled)
[OK]   agents.summary   → 'cf-gemma-4-26b' (enabled)
[OK]   agents.memory    → 'cf-qwen3-30b' (enabled)
Result: All checks passed.
```

- [ ] **Step 5: コミット**

```bash
git -C /mnt/Projects/RustyClaw add production/config/config.json
git -C /mnt/Projects/RustyClaw commit -m \
  "feat(config): enable HF/CF models, assign all agent purposes"
```

---

## Task 6: デプロイ・本番検証

**Files:** なし（デプロイのみ）

- [ ] **Step 1: deploy.sh でデプロイ**

```bash
bash /mnt/Projects/RustyClaw/scripts/deploy.sh 2>&1 | grep -E "✓|Error"
```
期待: `✓ RPi4 側の ~/.local/bin/rustyclaw を更新しました。` など全 ✓

- [ ] **Step 2: Pi 上で check-config 実行**

```bash
ssh rp1 '~/.local/bin/rustyclaw --config ~/.rustyclaw/config/config.json check-config'
```
期待:
```
[OK]   agents.default   → 'groq-llama-8b' (enabled)
[OK]   agents.summary   → 'cf-gemma-4-26b' (enabled)
[OK]   agents.memory    → 'cf-qwen3-30b' (enabled)
...
[OK]   hf-qwen2.5-7b (...) → 200 (XXXms)
[OK]   cf-gemma-4-26b (...) → 200 (XXXms)
Result: All checks passed.
```

- [ ] **Step 3: Gateway サービスのログ確認**

```bash
ssh rp1 'journalctl -u rustyclaw --since "1 minute ago" --no-pager | grep -E "provider|model|purpose"'
```
期待: `Configuration loaded successfully: provider=openai, model=llama-3.1-8b-instant` など

- [ ] **Step 4: Discord テストメッセージ送信**

Discord からテストメッセージを送り、`hf-qwen2.5-7b` で応答が返ることを確認する。
ログで `model=Qwen2.5-7B` または API レスポンス確認:

```bash
ssh rp1 '~/.local/bin/rustyclaw --config ~/.rustyclaw/config/config.json debug request 2>&1 | head -5'
```

---

## 実装完了チェックリスト

- [ ] `AgentsConfig` に tools / discord / line / heartbeat フィールドが追加されている
- [ ] `get_model("discord")` → `hf-qwen2.5-7b`、未設定時は default fallback
- [ ] `execute_heartbeat()` が `get_model("heartbeat")` を使用
- [ ] `execute_with_tools()` が `purpose: &str` 引数を受け取る
- [ ] Gateway が Discord メッセージに `"discord"` を渡す
- [ ] `config.json` の agents に全 7 purpose が設定済み
- [ ] `hf-qwen2.5-7b` / `cf-gemma-4-26b` / `cf-qwen3-30b` が `enabled: true`
- [ ] `cargo test` 全パス
- [ ] Pi 上で `check-config` 全 OK
