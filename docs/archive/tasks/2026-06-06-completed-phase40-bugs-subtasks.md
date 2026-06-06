# Archive — Phase 40 バグ修正・完了済みサブタスク（2026-06-06）

> 2026-06-06 に `docs/task.md` から移動。Phase 40-7 最優先昇格に伴うクリーンアップ。

---

## 🔴 GeminiClaw 機能ギャップ（完了済み）

### Phase 40-5 バグ修正: CF Embedding `'input' field is required` ✅ 完了
> 2026-06-05 修正・デプロイ済み。commit `55f773f`。  
> 根本原因: LM Studio（OpenAI 互換）に `{"text":}` を送っていたが `{"input":}` が正しい。URL末尾 `/embeddings` 検出で分岐、レスポンスパースも OpenAI 形式対応。

- `[x]` **1. `CloudflareEmbeddingClient.embed()` のリクエストボディを調査・修正**
- `[x]` **2. 修正後の動作確認 + deploy**

### Memory Flush バグ修正: コンテキスト制限超過によるスキップ ✅ 完了
> 2026-06-05 修正済み。  
> 根本原因: スキルファイルがインジェクションされた bloated なユーザーメッセージがそのまま履歴ファイル（http-dashboard.jsonl）に user role として保存され、履歴サイズが肥大化。Memory Flush 時のトークン見積もりがコンテキスト制限（13,107 tokens）を超過しスキップされていた。  
> 対応内容: `execute_with_rig_agent` に `raw_user_message`（ログ・RAG用）と `injected_user_message`（LLM/agent実行用）を分離して渡すように修正。

- `[x]` **1. `execute_with_rig_agent` のシグネチャ・内部処理変更**
- `[x]` **2. `rustyclaw-gateway/src/lib.rs` での呼び出し処理アップデート**

### seen_items による既読通知フィルタリング ✅ 完了（2026-06-06）
> 2026-06-05 ログ点検で発覚。  
> 現象: 重複検知を避けるための `seen_items` テーブルが一度も使用されておらず、毎回同一のメールを Important 検知して Proactive Speak (Discord 通知) を 30分おきに送り続けている。  
> 対処: `execute_heartbeat` のツール呼び出しループで `run_workspace_script` 結果（Gmail/Calendar）を `is_item_seen` でフィルタし、新規アイテムのみ LLM へ渡す。`mark_item_seen` で既読登録。fail-open 設計。  
> 5コミット（`9eb5a51`〜`98c3544`）、6テスト追加、全159テスト通過。

- `[x]` **1. `execute_heartbeat` に `db_path` パラメータ追加・Gateway 呼び出し元更新**
- `[x]` **2. `filter_seen_tool_result` ヘルパー実装（Gmail/Calendar 既読フィルタ）+ ツールループへの組み込み**

### Phase 40-7 — Static Docs RAG ✅ 完了（2026-06-06）
> 2026-06-06 完了。
> 静的ドキュメント（AGENTS.md / skills/*.md）をチャンク化・差分インジェストし、ユーザー入力との類似度で動的にシステムプロンプトへ注入することで、不要なコンテキスト送信を削減。起動時および設定リロード時にバックグラウンドで実行。
> 実装計画: `docs/plans/2026-06-05-static-docs-rag.md`

- `[x]` **静的ドキュメントをチャンク化・差分インジェストし、ユーザー入力との類似度で動的にシステムプロンプトへ注入**

---

## Phase 40-2: rig-core Tool トレイト移行 ✅ 完了（2026-06-06）

> `rig_core::tool::Tool` を全ツールに直接実装。`RigToolAdapter`・カスタム `Tool` トレイト・`async-trait` 依存を削除。  
> 10コミット、約754行削減。テスト 152 件全通過。  
> 実装計画: `docs/plans/2026-06-05-phase40-2-rig-tool-trait-migration.md`

---

## Phase 40 完了済みサブタスク

### 40-2: rig-core Tool トレイト直接実装 ✅ 完了（2026-06-06）
- 全ツールに `rig_core::tool::Tool` を直接実装し、typed `Args` struct で型安全な引数パースを実現。
- `RigToolAdapter`・カスタム `Tool` トレイト・`ToolResult`・`async-trait` 依存を削除。

### 40-3: ベクトル検索（RAG）による長期記憶の拡張 ✅ 完了
- MEMORY.md バレット行を CF AI Gateway `@cf/baai/bge-m3` (1024次元、多言語) でベクトル化し SQLite 保存。
- Fail-open 設計。実装計画: `docs/plans/2026-06-04-rag-memory-implementation-plan.md`

### 40-5: Unified RAG with rig-core InMemoryVectorStore ✅ 完了
- `InMemoryVectorStore` 採用、MEMORY.md チャンクとセッション要約のインメモリ統合 RAG 化。
- 実装済み・稼働中。実装計画: `docs/plans/2026-06-05-rig-core-unified-rag.md`

### 40-6: rig-core 全面リファクタリング ✅ 完了（2026-06-05）
- `rmcp` クライアントへの移行、`rig::agent::Agent` 移行による ReAct/RAG ループの一本化。
- 実装計画: `docs/plans/2026-06-05-rig-core-refactoring.md`
- ✅ `RigToolAdapter` + `ToolRegistry::to_dyn_tools()` 実装（commit `1837b64`）
- ✅ `Pipeline::execute_with_rig_agent()` 実装（`RustyclawCompletionModel` + `AgentBuilder` + `Chat::chat()`、commit `e311cb1`）
- ✅ `rustyclaw-mcp` → rig-core `rmcp` 移行・クレート削除（commit `112ba30`, `d671dfd`, `2020af1`）
  - `execute_with_rig_agent` を `ToolServerHandle` 引数に変更、`AgentBuilder::tool_server_handle()` 使用
  - Gateway: `McpClientHandler` + `ToolServer` で MCP サーバー接続を管理
- worktree `.worktrees/phase-40-6` およびローカルブランチ `phase-40-6` 削除済み（2026-06-06）。

---

## Phase 25 完了済みサブタスク

- `[x]` **1. Lane Queue（Inngest 代替）の機能ギャップ分析とロードマップ策定**
- `[x]` **3. Chat Progress Reporter (Typing... 送信) の実装 (Phase 1)**
- `[x]` **4. 並行数 4 への拡張に向けたファイルロック機構の導入 (Phase 2)**
- `[x]` **6. `docs/specs/91_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 28b 完了済みサブタスク

- `[x]` **3. LANE QUEUE 表示名を `{cron title} ({HH:MM})` 形式に変更**
  - `cron.json` の `name` フィールドと `trigger.expression`（HH:MM）を組み合わせた形式で表示。
  - 例: `Topic Patrol Explore (02:00)` / `Daily Briefing (05:05)` / `Vital Check Morning (06:00)`
  - `queue_update_or_insert()` の `desc` 引数を `format!("{} ({})", job.name, job.trigger.expression)` 形式で生成。
  - Heartbeat（`"Heartbeat Patrol / Activity Scan"`）はそのまま維持。
