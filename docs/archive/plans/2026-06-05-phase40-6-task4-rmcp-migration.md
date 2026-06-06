# Phase 40-6 Task 4: rustyclaw-mcp → rig-core rmcp 移行 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rustyclaw-mcp` クレートを削除し、rig-core の `rmcp` feature が提供する `McpClientHandler` + `ToolServer` に完全移行する。

**Architecture:**
- Gateway 起動時に `ToolServer::new().run()` で `ToolServerHandle` を生成し、ネイティブツール（`RigToolAdapter` 経由）と MCP ツール（`McpClientHandler` 経由）を両方登録。
- `execute_with_rig_agent` の引数を `&ToolRegistry` → `ToolServerHandle` に変更し、`AgentBuilder::tool_server_handle()` を使用。
- `ToolRegistry` は heartbeat 用に維持（ネイティブツールのみ）。

**Tech Stack:** Rust, rig-core 0.38.1 (rmcp feature), rmcp 1.7.0, tokio async

---

## Task 1: rustyclaw-agent — rmcp feature 有効化と execute_with_rig_agent 引数変更

**Files:**
- Modify: `crates/rustyclaw-agent/Cargo.toml`
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: Cargo.toml に rmcp feature を追加**

  `crates/rustyclaw-agent/Cargo.toml` の `rig-core` 依存を更新：
  ```toml
  rig-core = { version = "0.38", features = ["rmcp"] }
  ```

- [ ] **Step 2: `execute_with_rig_agent` の引数を変更**

  `crates/rustyclaw-agent/src/lib.rs` の `execute_with_rig_agent` シグネチャを変更：
  ```rust
  // 変更前:
  pub async fn execute_with_rig_agent(
      &self,
      workspace_dir: &Path,
      session_id: &str,
      user_message: &str,
      tool_registry: &ToolRegistry,
      purpose: &str,
      progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
  ) -> Result<LlmResponse>
  
  // 変更後:
  pub async fn execute_with_rig_agent(
      &self,
      workspace_dir: &Path,
      session_id: &str,
      user_message: &str,
      tool_handle: rig_core::tool::server::ToolServerHandle,
      purpose: &str,
      progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
  ) -> Result<LlmResponse>
  ```

- [ ] **Step 3: 関数内の AgentBuilder 呼び出しを更新**

  `execute_with_rig_agent` 内の以下を変更：
  ```rust
  // 削除:
  let rig_tools = tool_registry.to_dyn_tools();
  
  // 変更前:
  let agent = AgentBuilder::new(model)
      .preamble(&system_context)
      .tools(rig_tools)
      .build();
  
  // 変更後:
  let agent = AgentBuilder::new(model)
      .preamble(&system_context)
      .tool_server_handle(tool_handle)
      .build();
  ```

- [ ] **Step 4: コンパイル確認**
  ```bash
  cargo check -p rustyclaw-agent 2>&1 | grep "^error" | head -10
  ```
  Expected: エラーなし（gateway が未更新なのでワークスペースビルドは通らないが agent 単体は通る）

- [ ] **Step 5: コミット**
  ```bash
  git add crates/rustyclaw-agent/
  git commit -m "feat(agent): Task 4 — execute_with_rig_agent accepts ToolServerHandle"
  ```

---

## Task 2: rustyclaw-gateway — McpManager を rmcp に置換

**Files:**
- Modify: `crates/rustyclaw-gateway/Cargo.toml`
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] **Step 1: Cargo.toml 更新**

  `crates/rustyclaw-gateway/Cargo.toml`:
  - `rustyclaw-mcp = { path = "../rustyclaw-mcp" }` を削除
  - 以下を追加：
  ```toml
  rig-core = { version = "0.38", features = ["rmcp"] }
  rmcp = "1.7"
  ```

- [ ] **Step 2: LaneRegistry 構造体に `tool_server_handle` を追加**

  `crates/rustyclaw-gateway/src/lib.rs` の `LaneRegistry` 構造体：
  ```rust
  // 追加:
  tool_server_handle: rig_core::tool::server::ToolServerHandle,
  ```

- [ ] **Step 3: LaneRegistry::new() シグネチャ更新**

  ```rust
  pub fn new(
      config: Config,
      workspace_path: PathBuf,
      bus: Arc<MessageBus>,
      tool_registry: Arc<rustyclaw_tools::ToolRegistry>,
      tool_server_handle: rig_core::tool::server::ToolServerHandle,  // 追加
      gmn_sem: Arc<Semaphore>,
      discord_connector: Arc<DiscordConnector>,
  ) -> Self {
      Self {
          ...
          tool_registry,
          tool_server_handle,  // 追加
          ...
      }
  }
  ```

- [ ] **Step 4: dispatch() 内で tool_server_handle を伝播**

  `dispatch()` の冒頭（`tool_registry` クローン付近）に追加：
  ```rust
  let tool_server_handle = self.tool_server_handle.clone();
  ```
  
  `execute_with_rig_agent` 呼び出しを更新：
  ```rust
  // 変更前:
  pipeline.execute_with_rig_agent(&workspace_path, &session_id, &content, &tool_reg, run_purpose, progress_tx_opt).await
  
  // 変更後:
  pipeline.execute_with_rig_agent(&workspace_path, &session_id, &content, tool_server_handle, run_purpose, progress_tx_opt).await
  ```
  （`tool_reg` の使用も削除）

- [ ] **Step 5: Gateway::run() の MCP 初期化を置換**

  以下のブロックを削除：
  ```rust
  let mcp_manager = Arc::new(rustyclaw_mcp::McpManager::new());
  if !config.mcp.is_empty() {
      mcp_manager.connect_all(&config.mcp).await.ok();
  }
  let mut tool_registry = rustyclaw_tools::ToolRegistry::new();
  for mcp_tool in mcp_manager.get_tools().await {
      tool_registry.register(mcp_tool);
  }
  ```
  
  以下に置換：
  ```rust
  // ToolServer (rig エージェント用) 初期化
  let tool_server_handle = rig_core::tool::server::ToolServer::new().run();
  let mut mcp_service_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
  
  // MCP サーバーへの接続 (rmcp 経由)
  for (name, conf) in &config.mcp {
      if conf.enabled {
          let handler = rig_core::tool::rmcp::McpClientHandler::new(
              rmcp::model::ClientInfo::default(),
              tool_server_handle.clone(),
          );
          let mut cmd = tokio::process::Command::new(&conf.command);
          cmd.args(&conf.args)
             .envs(&conf.env)
             .stdin(std::process::Stdio::piped())
             .stdout(std::process::Stdio::piped());
          match cmd.spawn() {
              Ok(mut child) => {
                  if let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) {
                      match handler.connect((stdout, stdin)).await {
                          Ok(service) => {
                              let n = name.clone();
                              mcp_service_tasks.push(tokio::spawn(async move {
                                  if let Err(e) = service.waiting().await {
                                      tracing::warn!("MCP service {} ended: {e}", n);
                                  }
                              }));
                              tokio::spawn(async move { child.wait().await.ok(); });
                              tracing::info!("Connected to MCP server: {}", name);
                          }
                          Err(e) => tracing::error!("Failed to connect MCP server {}: {e}", name),
                      }
                  }
              }
              Err(e) => tracing::error!("Failed to spawn MCP process {}: {e}", name),
          }
      }
  }
  
  let mut tool_registry = rustyclaw_tools::ToolRegistry::new();
  ```

- [ ] **Step 6: ネイティブツール登録を両方に追加**

  各ネイティブツール登録の後に `tool_server_handle.add_tool()` を追加：
  ```rust
  // Brave Search
  if let Some(b) = config.tools.brave_search.as_ref().filter(|b| b.enabled) {
      if !b.api_key.is_empty() {
          let t = Arc::new(rustyclaw_tools::WebSearchTool::new(b.api_key.clone()));
          tool_registry.register(t.clone());
          tool_server_handle.add_tool(rustyclaw_tools::RigToolAdapter::new(t)).await.ok();
          tracing::info!("Registered WebSearchTool (Brave Search).");
      }
  }
  // WebFetchTool
  {
      let t = Arc::new(rustyclaw_tools::WebFetchTool::new());
      tool_registry.register(t.clone());
      tool_server_handle.add_tool(rustyclaw_tools::RigToolAdapter::new(t)).await.ok();
  }
  // WorkspaceReadTool / WorkspaceWriteTool / MemorySearchTool / WorkspaceExecuteScriptTool
  {
      let t = Arc::new(rustyclaw_tools::WorkspaceReadTool::new(self.workspace_path.clone()));
      tool_registry.register(t.clone());
      tool_server_handle.add_tool(rustyclaw_tools::RigToolAdapter::new(t)).await.ok();
  }
  // ...同様に他のツール...
  ```

- [ ] **Step 7: SIGINT/SIGTERM クリーンアップ更新**

  以下の行を削除（2箇所）：
  ```rust
  mcp_manager.close_all().await;
  ```
  
  以下に置換（1箇所にまとめてもよい）：
  ```rust
  for h in &mcp_service_tasks { h.abort(); }
  ```

- [ ] **Step 8: LaneRegistry::new() の呼び出し側を更新**

  `Gateway::run()` 内の `LaneRegistry::new()` 呼び出しに `tool_server_handle.clone()` を追加：
  ```rust
  let registry = Arc::new(LaneRegistry::new(
      config.clone(),
      self.workspace_path.clone(),
      bus.clone(),
      tool_registry.clone(),
      tool_server_handle.clone(),  // 追加
      gmn_sem.clone(),
      discord_client.clone(),
  ));
  ```

- [ ] **Step 9: テストの `LaneRegistry::new()` 呼び出しを更新**

  `test_lane_registry_serialization_and_semaphore` テスト内：
  ```rust
  let tool_server_handle = rig_core::tool::server::ToolServer::new().run();
  let registry = LaneRegistry::new(
      config,
      ws_dir.path().to_path_buf(),
      bus.clone(),
      tool_registry,
      tool_server_handle,  // 追加
      test_gmn_sem,
      mock_discord
  );
  ```

- [ ] **Step 10: ビルド確認**
  ```bash
  cargo build -p rustyclaw-gateway 2>&1 | grep "^error" | head -10
  ```
  Expected: エラーなし

- [ ] **Step 11: コミット**
  ```bash
  git add crates/rustyclaw-gateway/
  git commit -m "feat(gateway): Task 4 — replace McpManager with rmcp McpClientHandler + ToolServer"
  ```

---

## Task 3: ワークスペースクリーンアップ

**Files:**
- Modify: `Cargo.toml` (root)
- Delete: `crates/rustyclaw-mcp/`

- [ ] **Step 1: root Cargo.toml から rustyclaw-mcp を削除**

  `Cargo.toml` の `[workspace]` `members` から `"crates/rustyclaw-mcp"` を削除。

- [ ] **Step 2: rustyclaw-mcp ディレクトリを削除**
  ```bash
  rm -rf crates/rustyclaw-mcp
  ```

- [ ] **Step 3: フルビルドと全テスト**
  ```bash
  cargo build 2>&1 | grep "^error" | head -10
  cargo test 2>&1 | grep -E "^test result|FAILED" | head -20
  ```
  Expected: エラーなし、全テスト通過

- [ ] **Step 4: コミット**
  ```bash
  git add -A
  git commit -m "chore: remove rustyclaw-mcp crate (replaced by rig-core rmcp feature)"
  ```

---

## Task 4: task.md 更新

- [ ] **Step 1: task.md の Phase 40-6 を更新**

  Task 4 を完了済みに更新し、Phase 40-6 全体を完了としてマーク。

- [ ] **Step 2: コミット**
  ```bash
  git add docs/task.md
  git commit -m "docs(task): Phase 40-6 Task 4 完了 — rmcp 移行"
  ```
