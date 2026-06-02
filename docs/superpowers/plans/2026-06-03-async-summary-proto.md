# Async Rolling Summary Prototype Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `crates/rustyclaw-summary-proto` として新規クレートを追加し、5会話ごとにバックグラウンドで要約を更新するローリングサマリーパターンを実証する。

**Architecture:** `Arc<tokio::sync::RwLock<ChatSession>>` で状態を管理し、メインLLM呼び出し後に `tokio::spawn` + `Arc<Semaphore>` でバックグラウンド要約タスクを起動する。要約は `production/workspace/proto/summary.md` に永続化する。

**Tech Stack:** rig-core 0.38.1 (`CompletionsClient` + Chat Completions API), tokio 1, anyhow 1, tracing 0.1

---

## File Map

| ファイル | 役割 |
|---------|------|
| `crates/rustyclaw-summary-proto/Cargo.toml` | 新規作成 |
| `crates/rustyclaw-summary-proto/src/main.rs` | 新規作成: 対話ループ（stdin → chat → stdout） |
| `crates/rustyclaw-summary-proto/src/session.rs` | 新規作成: `ChatSession` 定義・永続化 |
| `crates/rustyclaw-summary-proto/src/proto.rs` | 新規作成: `SummaryProto`・LLM呼び出し・バックグラウンド要約 |
| `Cargo.toml` | 修正: `members` に追加 |

---

## Task 1: クレートのスキャフォルド

**Files:**
- Create: `crates/rustyclaw-summary-proto/Cargo.toml`
- Create: `crates/rustyclaw-summary-proto/src/main.rs`
- Create: `crates/rustyclaw-summary-proto/src/session.rs`
- Create: `crates/rustyclaw-summary-proto/src/proto.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Cargo.toml を作成する**

```toml
# crates/rustyclaw-summary-proto/Cargo.toml
[package]
name    = "rustyclaw-summary-proto"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "summary-proto"
path = "src/main.rs"

[dependencies]
rig-core           = "0.38"
tokio              = { version = "1", features = ["full"] }
anyhow             = "1"
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 2: 空のソースファイルを作成する**

```rust
// crates/rustyclaw-summary-proto/src/session.rs
// （後のタスクで実装）
```

```rust
// crates/rustyclaw-summary-proto/src/proto.rs
// （後のタスクで実装）
```

```rust
// crates/rustyclaw-summary-proto/src/main.rs
mod session;
mod proto;

fn main() {}
```

- [ ] **Step 3: ワークスペースに追加する**

`Cargo.toml` の `members` 配列末尾に `"crates/rustyclaw-summary-proto"` を追加する。

```toml
members = [
    "crates/rustyclaw-cli",
    "crates/rustyclaw-gateway",
    "crates/rustyclaw-agent",
    "crates/rustyclaw-providers",
    "crates/rustyclaw-channels",
    "crates/rustyclaw-tools",
    "crates/rustyclaw-config",
    "crates/rustyclaw-storage",
    "crates/rustyclaw-mcp",
    "crates/rustyclaw-summary-proto",  # 追加
]
```

- [ ] **Step 4: ビルドが通ることを確認する**

```bash
cargo check -p rustyclaw-summary-proto
```

Expected: `Finished` (warning はあっても可)

- [ ] **Step 5: コミット**

```bash
git add Cargo.toml crates/rustyclaw-summary-proto/
git commit -m "chore(proto): scaffold rustyclaw-summary-proto crate"
```

---

## Task 2: session.rs — ChatSession の実装

**Files:**
- Modify: `crates/rustyclaw-summary-proto/src/session.rs`

- [ ] **Step 1: session.rs を実装する**

```rust
// crates/rustyclaw-summary-proto/src/session.rs
use anyhow::Result;
use rig_core::completion::Message;
use std::path::{Path, PathBuf};

pub const SUMMARY_INTERVAL: u32 = 5;

pub struct ChatSession {
    pub raw_history: Vec<(String, String)>,
    pub recent_messages: Vec<Message>,
    pub current_summary: String,
    pub counter: u32,
    pub summary_path: PathBuf,
}

impl ChatSession {
    pub fn load(workspace_dir: &Path) -> Result<Self> {
        let summary_path = workspace_dir.join("summary.md");
        let current_summary = if summary_path.exists() {
            std::fs::read_to_string(&summary_path)?
        } else {
            String::new()
        };
        Ok(Self {
            raw_history: Vec::new(),
            recent_messages: Vec::new(),
            current_summary,
            counter: 0,
            summary_path,
        })
    }

    pub fn persist_summary(&self) -> Result<()> {
        if let Some(parent) = self.summary_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.summary_path, &self.current_summary)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tempdir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn load_creates_empty_session_when_no_file() {
        let dir = tempdir();
        let session = ChatSession::load(dir.path()).unwrap();
        assert!(session.current_summary.is_empty());
        assert_eq!(session.counter, 0);
        assert!(session.recent_messages.is_empty());
        assert!(session.raw_history.is_empty());
    }

    #[test]
    fn load_reads_existing_summary() {
        let dir = tempdir();
        fs::write(dir.path().join("summary.md"), "これまでの要約です").unwrap();
        let session = ChatSession::load(dir.path()).unwrap();
        assert_eq!(session.current_summary, "これまでの要約です");
    }

    #[test]
    fn persist_summary_creates_file() {
        let dir = tempdir();
        let mut session = ChatSession::load(dir.path()).unwrap();
        session.current_summary = "新しい要約".to_string();
        session.persist_summary().unwrap();
        let content = fs::read_to_string(dir.path().join("summary.md")).unwrap();
        assert_eq!(content, "新しい要約");
    }

    #[test]
    fn persist_summary_creates_parent_dirs() {
        let dir = tempdir();
        let nested = dir.path().join("sub").join("dir");
        let mut session = ChatSession::load(&nested).unwrap();
        session.current_summary = "ネスト".to_string();
        session.persist_summary().unwrap();
        let content = fs::read_to_string(nested.join("summary.md")).unwrap();
        assert_eq!(content, "ネスト");
    }
}
```

- [ ] **Step 2: `tempfile` を Cargo.toml の dev-dependencies に追加する**

```toml
# crates/rustyclaw-summary-proto/Cargo.toml に追記
[dev-dependencies]
tempfile   = "3"
serde_json = "1"
```

- [ ] **Step 3: テストを実行して全通過を確認する**

```bash
cargo test -p rustyclaw-summary-proto session
```

Expected:
```
running 4 tests
....
test result: ok. 4 passed; 0 failed
```

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-summary-proto/
git commit -m "feat(proto): implement ChatSession with file persistence"
```

---

## Task 3: proto.rs — ヘルパー関数と SummaryProto の骨格

**Files:**
- Modify: `crates/rustyclaw-summary-proto/src/proto.rs`

- [ ] **Step 1: proto.rs にヘルパー関数と SummaryProto 構造体を実装する**

```rust
// crates/rustyclaw-summary-proto/src/proto.rs
use crate::session::{ChatSession, SUMMARY_INTERVAL};
use anyhow::{Context, Result};
use rig_core::{
    client::{CompletionClient, ProviderClient},
    completion::{AssistantContent, CompletionModel, Message, UserContent},
    providers::openai::CompletionsClient,
};
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};

pub struct Config {
    pub base_url: String,
    pub api_key: String,
    pub main_model: String,
    pub summary_model: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            base_url: std::env::var("LMS_BASE_URL")
                .unwrap_or_else(|_| "http://192.168.1.110:1234/v1".to_string()),
            api_key: std::env::var("LMS_API_KEY")
                .unwrap_or_else(|_| "lm-studio".to_string()),
            main_model: std::env::var("MAIN_MODEL")
                .unwrap_or_else(|_| "google/gemma-4-e4b".to_string()),
            summary_model: std::env::var("SUMMARY_MODEL")
                .unwrap_or_else(|_| "google/gemma-4-e4b".to_string()),
        })
    }
}

pub struct SummaryProto {
    config: Arc<Config>,
    session: Arc<RwLock<ChatSession>>,
    summary_sem: Arc<Semaphore>,
}

/// rig-core の CompletionsClient（Chat Completions API）を構築する。
fn make_client(base_url: &str, api_key: &str) -> Result<CompletionsClient> {
    CompletionsClient::builder()
        .api_key(api_key)
        .base_url(base_url)
        .build()
        .context("failed to build CompletionsClient")
}

/// AssistantContent のリストからテキストを結合して返す。
fn extract_text(
    choice: &rig_core::OneOrMany<AssistantContent>,
) -> Result<String> {
    let text: String = choice
        .iter()
        .filter_map(|c| match c {
            AssistantContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        Err(anyhow::anyhow!("assistant returned no text content"))
    } else {
        Ok(text)
    }
}

/// Message を「role: content」形式のテキストに変換する。
fn message_to_text(msg: &Message) -> String {
    match msg {
        Message::User { content } => {
            let text = content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("User: {text}")
        }
        Message::Assistant { content, .. } => {
            let text = content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("Assistant: {text}")
        }
        Message::System { content } => format!("System: {content}"),
    }
}

impl SummaryProto {
    pub fn new(config: Config, session: ChatSession) -> Self {
        Self {
            config: Arc::new(config),
            session: Arc::new(RwLock::new(session)),
            summary_sem: Arc::new(Semaphore::new(1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::completion::Message;

    #[test]
    fn extract_text_returns_text_from_choice() {
        use rig_core::completion::message::Text;
        use rig_core::OneOrMany;

        let choice = OneOrMany::one(AssistantContent::Text(Text {
            text: "Hello".to_string(),
        }));
        assert_eq!(extract_text(&choice).unwrap(), "Hello");
    }

    #[test]
    fn extract_text_errors_on_empty() {
        use rig_core::completion::message::{ToolCall, ToolFunction};
        use rig_core::OneOrMany;

        let choice = OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            "1".to_string(),
            ToolFunction::new("test".to_string(), serde_json::json!({})),
        )));
        assert!(extract_text(&choice).is_err());
    }

    #[test]
    fn message_to_text_user() {
        let m = Message::user("こんにちは");
        assert_eq!(message_to_text(&m), "User: こんにちは");
    }

    #[test]
    fn message_to_text_assistant() {
        let m = Message::assistant("はい");
        assert_eq!(message_to_text(&m), "Assistant: はい");
    }
}
```

- [ ] **Step 2: テストを実行して全通過を確認する**

```bash
cargo test -p rustyclaw-summary-proto proto
```

Expected:
```
running 4 tests
....
test result: ok. 4 passed; 0 failed
```

- [ ] **Step 3: コミット**

```bash
git add crates/rustyclaw-summary-proto/src/proto.rs
git commit -m "feat(proto): add SummaryProto scaffold and helper functions"
```

---

## Task 4: proto.rs — chat() メソッド（メインLLM呼び出し）

**Files:**
- Modify: `crates/rustyclaw-summary-proto/src/proto.rs`

- [ ] **Step 1: `SummaryProto::chat()` を実装する**

`impl SummaryProto` ブロックの中に、`new()` の直後に以下を追加する。

```rust
    pub async fn chat(&self, user_input: &str) -> Result<String> {
        // 1. 現在のセッション状態を読み取る
        let (system_prompt, history) = {
            let s = self.session.read().await;
            let system = if s.current_summary.is_empty() {
                "あなたは親切なアシスタントです。".to_string()
            } else {
                format!(
                    "あなたは親切なアシスタントです。\n\n## これまでの会話の要約\n{}",
                    s.current_summary
                )
            };
            (system, s.recent_messages.clone())
        };

        // 2. メインLLMを呼び出す
        let client = make_client(&self.config.base_url, &self.config.api_key)?;
        let model = client.completion_model(&self.config.main_model);

        let request = model
            .completion_request(Message::user(user_input))
            .preamble(system_prompt)
            .messages(history)
            .build();

        let response = model
            .completion(request)
            .await
            .context("main LLM call failed")?;

        let assistant_text = extract_text(&response.choice)?;

        // 3. セッション状態を更新し、サマリートリガー判定を行う
        let maybe_snapshot = {
            let mut s = self.session.write().await;
            s.raw_history
                .push(("user".to_string(), user_input.to_string()));
            s.raw_history
                .push(("assistant".to_string(), assistant_text.clone()));
            s.recent_messages.push(Message::user(user_input));
            s.recent_messages.push(Message::assistant(&assistant_text));
            s.counter += 1;

            if s.counter >= SUMMARY_INTERVAL {
                let snapshot = s.recent_messages.clone();
                s.recent_messages.clear();
                s.counter = 0;
                Some(snapshot)
            } else {
                None
            }
        };

        // 4. 必要に応じてバックグラウンド要約を起動
        if let Some(snapshot) = maybe_snapshot {
            self.spawn_summary_task(snapshot);
        }

        Ok(assistant_text)
    }
```

- [ ] **Step 2: コンパイルが通ることを確認する**

```bash
cargo check -p rustyclaw-summary-proto
```

Expected: `Finished` (warning はあっても可)

- [ ] **Step 3: コミット**

```bash
git add crates/rustyclaw-summary-proto/src/proto.rs
git commit -m "feat(proto): implement SummaryProto::chat() with session state management"
```

---

## Task 5: proto.rs — バックグラウンド要約タスク

**Files:**
- Modify: `crates/rustyclaw-summary-proto/src/proto.rs`

- [ ] **Step 1: `spawn_summary_task` と `run_summary` を実装する**

`impl SummaryProto` ブロックの末尾（`chat()` の後）に追加する。

```rust
    fn spawn_summary_task(&self, snapshot: Vec<Message>) {
        let Ok(permit) = self.summary_sem.clone().try_acquire_owned() else {
            tracing::warn!("summary semaphore busy – skipping this update");
            return;
        };

        let config = self.config.clone();
        let session = self.session.clone();

        tokio::spawn(async move {
            let _permit = permit; // タスク終了時に自動解放
            if let Err(e) = run_summary(config, session, snapshot).await {
                tracing::warn!("background summary failed: {:#}", e);
            }
        });
    }
```

`impl SummaryProto` ブロックの**外**（同じファイル内）に追加する。

```rust
async fn run_summary(
    config: Arc<Config>,
    session: Arc<RwLock<ChatSession>>,
    snapshot: Vec<Message>,
) -> Result<()> {
    let history_text = snapshot
        .iter()
        .map(message_to_text)
        .collect::<Vec<_>>()
        .join("\n");

    let summary_prompt = format!(
        "以下の会話を200字以内の日本語で要約してください。重要なトピックと結論に焦点を当ててください。\n\n{}",
        history_text
    );

    let client = make_client(&config.base_url, &config.api_key)?;
    let model = client.completion_model(&config.summary_model);

    let request = model
        .completion_request(Message::user(&summary_prompt))
        .preamble("あなたは会話を簡潔に要約するアシスタントです。".to_string())
        .build();

    let response = model
        .completion(request)
        .await
        .context("summary LLM call failed")?;

    let new_summary = extract_text(&response.choice)?;

    {
        let mut s = session.write().await;
        s.current_summary = new_summary;
        if let Err(e) = s.persist_summary() {
            tracing::warn!("failed to persist summary.md: {:#}", e);
        }
    }

    tracing::info!("background summary updated successfully");
    Ok(())
}
```

- [ ] **Step 2: コンパイルが通ることを確認する**

```bash
cargo check -p rustyclaw-summary-proto
```

Expected: `Finished`

- [ ] **Step 3: コミット**

```bash
git add crates/rustyclaw-summary-proto/src/proto.rs
git commit -m "feat(proto): implement background summary task with semaphore guard"
```

---

## Task 6: main.rs — 対話ループ

**Files:**
- Modify: `crates/rustyclaw-summary-proto/src/main.rs`

- [ ] **Step 1: main.rs を実装する**

```rust
// crates/rustyclaw-summary-proto/src/main.rs
mod proto;
mod session;

use anyhow::Result;
use proto::{Config, SummaryProto};
use session::ChatSession;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("summary_proto=info".parse().unwrap()),
        )
        .init();

    let workspace_dir = PathBuf::from(
        std::env::var("WORKSPACE_DIR")
            .unwrap_or_else(|_| "./production/workspace/proto".to_string()),
    );

    let config = Config::from_env()?;
    let session = ChatSession::load(&workspace_dir)?;

    tracing::info!(
        model = %config.main_model,
        base_url = %config.base_url,
        workspace = %workspace_dir.display(),
        summary_loaded = !session.current_summary.is_empty(),
        "SummaryProto starting"
    );

    let proto = SummaryProto::new(config, session);

    let stdin = io::stdin();
    print!("> ");
    io::stdout().flush()?;

    for line in stdin.lock().lines() {
        let input = line?;
        let trimmed = input.trim();

        if trimmed.is_empty() {
            print!("> ");
            io::stdout().flush()?;
            continue;
        }
        if trimmed == "/quit" || trimmed == "/exit" {
            println!("Goodbye!");
            break;
        }

        match proto.chat(trimmed).await {
            Ok(response) => println!("Assistant: {response}"),
            Err(e) => eprintln!("Error: {e:#}"),
        }

        print!("> ");
        io::stdout().flush()?;
    }

    Ok(())
}
```

- [ ] **Step 2: ビルドが通ることを確認する**

```bash
cargo build -p rustyclaw-summary-proto
```

Expected: `Finished`（バイナリ生成）

- [ ] **Step 3: コミット**

```bash
git add crates/rustyclaw-summary-proto/src/main.rs
git commit -m "feat(proto): implement interactive chat loop in main.rs"
```

---

## Task 7: IgnoreTest & smoke テスト

**Files:**
- Modify: `crates/rustyclaw-summary-proto/src/proto.rs`（テスト追加）

- [ ] **Step 1: ignore 付きの結合テストを追加する**

`proto.rs` の `#[cfg(test)]` ブロック末尾に追加する。

```rust
    /// LM Studio が稼働していることが前提の結合テスト。
    /// cargo test -p rustyclaw-summary-proto -- --ignored で実行。
    #[tokio::test]
    #[ignore]
    async fn integration_chat_reaches_lm_studio() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::from_env();
        let session = crate::session::ChatSession::load(dir.path()).unwrap();
        let proto = SummaryProto::new(config.unwrap(), session);

        let resp = proto.chat("こんにちは").await;
        assert!(resp.is_ok(), "LLM call failed: {:?}", resp.err());
        assert!(!resp.unwrap().is_empty());
    }
```

- [ ] **Step 2: 通常テスト（ignore なし）が全通過することを確認する**

```bash
cargo test -p rustyclaw-summary-proto
```

Expected:
```
running N tests
...
test result: ok. N passed; 0 failed
```

- [ ] **Step 3: ワークスペース全テストが通ることを確認する**

```bash
cargo test --workspace --quiet
```

Expected: 全テスト通過、新規テスト N 件追加

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-summary-proto/
git commit -m "test(proto): add integration test (ignored) and verify workspace tests pass"
```

---

## Task 8: workspace proto ディレクトリの初期化

**Files:**
- Create: `production/workspace/proto/.gitkeep`

- [ ] **Step 1: ディレクトリを作成する**

```bash
mkdir -p production/workspace/proto
touch production/workspace/proto/.gitkeep
```

- [ ] **Step 2: コミット**

```bash
git add production/workspace/proto/.gitkeep
git commit -m "chore(proto): create workspace/proto directory for summary persistence"
```

---

## 完了確認チェックリスト

- [ ] `cargo check -p rustyclaw-summary-proto` が通る
- [ ] `cargo test -p rustyclaw-summary-proto` が全通過（ignore を除く）
- [ ] `cargo test --workspace` が全通過
- [ ] `cargo build -p rustyclaw-summary-proto` でバイナリが生成される
- [ ] `production/workspace/proto/` が存在する
