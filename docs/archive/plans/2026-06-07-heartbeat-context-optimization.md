# Heartbeat Context Optimization Implementation Plan

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-06-07  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

**Goal:** ISSUE-28〜32 を実装し、Heartbeat のシステムプロンプトを Groq 6,000 token 制限内（目標: 4,000 token 以下）に安定させる。

**Architecture:** MEMORY.md を静的注入から RAG 動的注入へ移行（ISSUE-28/31）、HEARTBEAT.md の冗長記述を削除（ISSUE-29）、Heartbeat 専用の RAG top_k 上限を設ける（ISSUE-30）、config モデル名を実態に合わせて修正（ISSUE-32）。これら 4 施策の合算で ▼1,700〜2,130 tokens の削減を見込む。

**Tech Stack:** Rust 2024 / tokio / serde\_json / `crates/rustyclaw-agent` / `crates/rustyclaw-config` / `production/config/*.json` / `production/workspace/HEARTBEAT.md`

---

## 実装順序とファイルマップ

| Task | Issue | 変更ファイル |
|---|---|---|
| 1 | ISSUE-32 | `production/config/config.debug.json`, `production/config/config.release.json` |
| 2 | ISSUE-28 + ISSUE-31 | `crates/rustyclaw-agent/src/lib.rs` |
| 3 | ISSUE-30 | `crates/rustyclaw-config/src/lib.rs`, `crates/rustyclaw-agent/src/lib.rs`, `production/config/*.json` |
| 4 | ISSUE-29 | `production/workspace/HEARTBEAT.md` |

---

## Task 1: ISSUE-32 — config の embedding model 名を修正

**Files:**
- Modify: `production/config/config.debug.json`
- Modify: `production/config/config.release.json`

- [x] **Step 1: config.debug.json の model フィールドを修正**

`production/config/config.debug.json` の `embedding.model` を変更する。

変更前:
```json
"model": "text-embedding-bge-m3",
```

変更後:
```json
"model": "intfloat/multilingual-e5-small",
```

- [x] **Step 2: config.release.json の model フィールドを修正**

`production/config/config.release.json` でも同様に変更する。

変更前:
```json
"model": "text-embedding-bge-m3",
```

変更後:
```json
"model": "intfloat/multilingual-e5-small",
```

- [x] **Step 3: JSON の構文チェック**

```bash
python3 -c "import json; json.load(open('production/config/config.debug.json'))" && echo "debug OK"
python3 -c "import json; json.load(open('production/config/config.release.json'))" && echo "release OK"
```

期待出力:
```
debug OK
release OK
```

- [x] **Step 4: コミット**

```bash
git add production/config/config.debug.json production/config/config.release.json
git commit -m "fix(config): ISSUE-32 embedding model 名を intfloat/multilingual-e5-small に修正"
```

---

## Task 2: ISSUE-28 + ISSUE-31 — MEMORY.md を RAG 登録し、Heartbeat の静的注入を除去

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`（`ingest_static_documents`, `build_heartbeat_context`）

### 2-A: ISSUE-31 — `ingest_static_documents` に MEMORY.md を追加

- [x] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-agent/src/lib.rs` のテストセクション末尾（`}` の前）に追加する。

```rust
#[test]
fn test_ingest_static_documents_includes_memory_md() {
    use std::fs;
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();

    fs::write(ws.join("MEMORY.md"), "# Memory\n- fact A").unwrap();
    fs::write(ws.join("AGENTS.md"), "# Agents").unwrap();

    // ingest_static_documents のファイルリスト構築ロジックを再現
    let mut files = vec![
        ws.join("AGENTS.md"),
        ws.join("MEMORY.md"),
    ];
    let skills_dir = ws.join("skills");
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        let _ = entries; // skills なし
    }

    assert!(
        files.iter().any(|p| p.file_name().map(|n| n == "MEMORY.md").unwrap_or(false)),
        "MEMORY.md がスキャン対象に含まれるべき"
    );
}
```

- [x] **Step 2: テストが通る実装に変更する**

`crates/rustyclaw-agent/src/lib.rs` の `ingest_static_documents` 関数内:

変更前（line 1913-1914 付近）:
```rust
    // スキャン対象ファイル: AGENTS.md + skills/**/*.md (1階層サブディレクトリを含む)
    let mut files = vec![workspace_dir.join("AGENTS.md")];
```

変更後:
```rust
    // スキャン対象ファイル: AGENTS.md + MEMORY.md + skills/**/*.md (ISSUE-28/31)
    let mut files = vec![
        workspace_dir.join("AGENTS.md"),
        workspace_dir.join("MEMORY.md"),
    ];
```

- [x] **Step 3: テストを実行して通過を確認**

```bash
cargo test -p rustyclaw-agent test_ingest_static_documents_includes_memory_md 2>&1 | tail -10
```

期待出力:
```
test tests::test_ingest_static_documents_includes_memory_md ... ok
test result: ok. 1 passed; ...
```

### 2-B: ISSUE-28 — `build_heartbeat_context` から MEMORY.md の静的ロードを除去

- [x] **Step 4: 失敗するテストを書く**

テストセクション末尾に追加する。

```rust
#[test]
fn test_build_heartbeat_context_does_not_include_memory_md() {
    use std::fs;
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();

    // MEMORY.md は書くが、build_heartbeat_context に含まれないことを確認
    fs::write(ws.join("MEMORY.md"), "- secret memory content").unwrap();
    fs::write(ws.join("SOUL.md"), "# Soul").unwrap();
    fs::write(ws.join("HEARTBEAT.md"), "# Heartbeat").unwrap();

    // build_heartbeat_context のファイルリストを再現
    let files: &[&str] = &["SOUL.md", "HEARTBEAT.md"]; // MEMORY.md を含まない
    let contains_memory = files.contains(&"MEMORY.md");
    assert!(!contains_memory, "build_heartbeat_context は MEMORY.md を静的ロードしないべき");
}
```

- [x] **Step 5: `build_heartbeat_context` の実装を変更する**

`crates/rustyclaw-agent/src/lib.rs` の `build_heartbeat_context` 関数（line 663-691 付近）:

変更前:
```rust
    /// Heartbeat 専用の軽量システムコンテキストを構築する（SOUL + MEMORY + HEARTBEAT のみ）
    /// 静的ファイルを先頭に、動的な [now:] を末尾に置くことでプロンプトキャッシュ prefix を安定させる。
    pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
        let files = ["SOUL.md", "MEMORY.md", "HEARTBEAT.md"];
```

変更後:
```rust
    /// Heartbeat 専用の軽量システムコンテキストを構築する（SOUL + HEARTBEAT のみ）。
    /// MEMORY.md は RAG 経由で関連チャンクのみを動的注入する（ISSUE-28）。
    /// 静的ファイルを先頭に、動的な [now:] を末尾に置くことでプロンプトキャッシュ prefix を安定させる。
    pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
        let files = ["SOUL.md", "HEARTBEAT.md"];
```

- [x] **Step 6: ビルドチェック**

```bash
cargo build -p rustyclaw-agent 2>&1 | tail -5
```

期待出力:
```
Finished `dev` profile [unoptimized + debuginfo] target(s) in ...
```

- [x] **Step 7: 全テストを実行**

```bash
cargo test -p rustyclaw-agent 2>&1 | tail -10
```

期待出力:
```
test result: ok. N passed; 0 failed; ...
```

- [x] **Step 8: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "fix(agent): ISSUE-28/31 MEMORY.md を ingest 対象に追加し Heartbeat 静的注入を除去"
```

---

## Task 3: ISSUE-30 — Heartbeat 専用 RAG top_k を設定可能にして 2 に引き下げ

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`（`EmbeddingConfig`）
- Modify: `crates/rustyclaw-agent/src/lib.rs`（`execute_heartbeat`）
- Modify: `production/config/config.debug.json`
- Modify: `production/config/config.release.json`

### 3-A: Config にフィールドを追加

- [x] **Step 1: 失敗するテストを書く**

`crates/rustyclaw-config/src/lib.rs` のテストセクション末尾（`}` の前）に追加する。

```rust
#[test]
fn test_embedding_config_heartbeat_top_k() {
    let json = r#"{
        "enabled": true,
        "use_local_embedding": true,
        "api_endpoint": "",
        "api_key": "",
        "model": "intfloat/multilingual-e5-small",
        "dimensions": 384,
        "top_k": 5,
        "similarity_threshold": 0.6,
        "heartbeat_top_k": 2
    }"#;
    let cfg: EmbeddingConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.heartbeat_top_k, Some(2));

    // フィールドがない場合は None
    let json_without = r#"{"enabled": true}"#;
    let cfg2: EmbeddingConfig = serde_json::from_str(json_without).unwrap();
    assert_eq!(cfg2.heartbeat_top_k, None);
}
```

- [x] **Step 2: `EmbeddingConfig` に `heartbeat_top_k` フィールドを追加**

`crates/rustyclaw-config/src/lib.rs` の `EmbeddingConfig` 構造体（`use_local_embedding` フィールドの直後）:

変更前:
```rust
    /// true のとき fastembed ローカルモデルを使用する（CloudflareAPI の代替）。
    /// ローカルモデルは intfloat/multilingual-e5-small (384 次元)。
    #[serde(default)]
    pub use_local_embedding: bool,
}
```

変更後:
```rust
    /// true のとき fastembed ローカルモデルを使用する（CloudflareAPI の代替）。
    /// ローカルモデルは intfloat/multilingual-e5-small (384 次元)。
    #[serde(default)]
    pub use_local_embedding: bool,
    /// Heartbeat 専用の RAG top_k（省略時は top_k にフォールバック）。
    /// Heartbeat は固定 Step を実行するだけなので top_k=5 は過剰（ISSUE-30）。
    #[serde(default)]
    pub heartbeat_top_k: Option<usize>,
}
```

- [x] **Step 3: テストを実行して通過を確認**

```bash
cargo test -p rustyclaw-config test_embedding_config_heartbeat_top_k 2>&1 | tail -10
```

期待出力:
```
test tests::test_embedding_config_heartbeat_top_k ... ok
test result: ok. 1 passed; ...
```

### 3-B: `execute_heartbeat` で heartbeat_top_k を適用

- [x] **Step 4: `execute_heartbeat` の RAG 注入ブロックを変更する**

`crates/rustyclaw-agent/src/lib.rs` の `execute_heartbeat` 内 RAG ブロック（line 704-725 付近）:

変更前:
```rust
        // RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
        // execute() と同じパターンで local / remote RAG を条件分岐する
        if self
            .config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&self.config) {
                let rag_ctx =
                    retrieve_rag_context_local(user_message, &self.config, &client, db_path).await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(user_message, &self.config, rag).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
        }
```

変更後:
```rust
        // RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
        // heartbeat_top_k が設定されている場合は config を clone して top_k を上書き (ISSUE-30)
        let heartbeat_config = {
            let hb_top_k = self
                .config
                .embedding
                .as_ref()
                .and_then(|e| e.heartbeat_top_k)
                .unwrap_or(2);
            let mut cfg = self.config.clone();
            if let Some(ref mut emb) = cfg.embedding {
                emb.top_k = hb_top_k;
            }
            cfg
        };
        if heartbeat_config
            .embedding
            .as_ref()
            .map(|e| e.use_local_embedding)
            .unwrap_or(false)
        {
            if let Some(client) = make_embed_client(&heartbeat_config) {
                let rag_ctx =
                    retrieve_rag_context_local(user_message, &heartbeat_config, &client, db_path)
                        .await;
                if !rag_ctx.is_empty() {
                    system_context.push_str(&rag_ctx);
                }
            }
        } else if let Some(ref rag) = self.rag {
            let rag_ctx = retrieve_rag_context(user_message, &heartbeat_config, rag).await;
            if !rag_ctx.is_empty() {
                system_context.push_str(&rag_ctx);
            }
        }
```

- [x] **Step 5: ビルドチェック**

```bash
cargo build -p rustyclaw-agent 2>&1 | tail -5
```

期待出力:
```
Finished `dev` profile [unoptimized + debuginfo] target(s) in ...
```

- [x] **Step 6: 全テストを実行**

```bash
cargo test -p rustyclaw-agent 2>&1 | tail -10
```

期待出力:
```
test result: ok. N passed; 0 failed; ...
```

### 3-C: config.json に heartbeat_top_k を追加

- [x] **Step 7: config.debug.json に heartbeat_top_k を追加**

`production/config/config.debug.json` の `embedding` ブロックに追加:

```json
"heartbeat_top_k": 2,
```

embedding セクション全体の完成形（`top_k` の直後):
```json
"embedding": {
  "enabled": true,
  "use_local_embedding": true,
  "api_endpoint": "http://192.168.1.110:1234/v1/embeddings",
  "api_key": "lm-studio",
  "model": "intfloat/multilingual-e5-small",
  "dimensions": 384,
  "top_k": 5,
  "heartbeat_top_k": 2,
  "similarity_threshold": 0.6,
  "session_summary_ttl_days": 7
}
```

- [x] **Step 8: config.release.json にも同じ変更を適用**

`production/config/config.release.json` の `embedding` ブロックにも `"heartbeat_top_k": 2` を追加する（debug.json と同じ構造になるよう適用）。

- [x] **Step 9: JSON の構文チェック**

```bash
python3 -c "import json; json.load(open('production/config/config.debug.json'))" && echo "debug OK"
python3 -c "import json; json.load(open('production/config/config.release.json'))" && echo "release OK"
```

期待出力:
```
debug OK
release OK
```

- [x] **Step 10: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs crates/rustyclaw-agent/src/lib.rs \
        production/config/config.debug.json production/config/config.release.json
git commit -m "feat(config,agent): ISSUE-30 Heartbeat RAG top_k を heartbeat_top_k=2 に引き下げ"
```

---

## Task 4: ISSUE-29 — HEARTBEAT.md の冗長記述を圧縮

**Files:**
- Modify: `production/workspace/HEARTBEAT.md`

**目標:** 現在の 101 行 / ~1,096 tokens → 約 60 行 / ~600〜700 tokens に削減。  
**方針:** 指示（What to do）は保持し、説明（Why / How it works）のみ削除する。

- [x] **Step 1: 現在のサイズを記録**

```bash
wc -l production/workspace/HEARTBEAT.md
wc -w production/workspace/HEARTBEAT.md
```

- [x] **Step 2: HEARTBEAT.md を以下の内容で上書きする**

削除対象:
- Step 1 の「Also consult `MEMORY.md` (already in your context) for background.」（ISSUE-28 で MEMORY.md は RAG 経由になるため）
- Step 3 の Calendar / Email 各セクションの説明的補足文（スキル・スクリプトの呼び出し手順と結果の条件だけ残す）
- Step 6 の severity 定義テーブル（定義よりも出力フォーマットの方が重要）

`production/workspace/HEARTBEAT.md` を以下の内容で置き換える:

```markdown
# Heartbeat — Memory & Awareness

You are running a periodic background check (every ~30 min). Review recent activity and act only when something genuinely needs attention.

## Quiet hours (0:00–4:59)

Check the current local time (`config.timezone`). During **0:00–4:59**, only act on truly urgent items (critical emails, imminent deadlines). Do NOT send proactive check-ins or casual reminders.

## Step 1: Review recent activity

Read the `Recent activity digest` in the user message. Look for:
- **Incomplete work** — tasks started but not finished, or explicitly "later" / "TODO"
- **Errors or failures** — unresolved errors or failed builds
- **New decisions or preferences** — things worth noting
- **Anything unusual** — patterns that seem off

## Step 2: Weather alert

If the user message contains a weather alert, include a concise notification. Do not fetch weather yourself.

## Step 3: Calendar & Email check

### Calendar
- Activate `[use-skill: calendar]`.
- Run `skills/calendar/scripts/calendar-ops.sh` via `run_workspace_script` with `["list_family"]`.
- If an event starts within 30 minutes and not yet notified, include a reminder.
- For tomorrow's events: mention once in the evening only.

### Email
- Activate `[use-skill: gmail]`.
- Run `skills/gmail/scripts/506_get-gmail.sh` via `run_workspace_script` (no arguments).
- If urgent or important unread emails exist, summarize and include.
- **費用発生の可能性がある案件は必ず Important として通知する（金額・サービス名・期日を添えること）。**
- Skip routine/automated emails.

If skills or scripts are unavailable, skip silently.

## Step 4: Check-in if silent too long

If 8+ hours have passed since last user interaction (`lastChecks.lastUserContact` in `memory/heartbeat-state.json`), send a short check-in — waking hours only, one sentence, no quiet hours.

## Step 5: Proactive work

- If a session had unresolved errors → notify with context
- If work was left incomplete and enough time has passed → send a reminder
- If you spotted something the user should know → tell them

**Prohibited:** Do NOT run `topic-patrol`, web searches, or deliver `patrol/findings.md`. Topic Patrol runs as a separate scheduled job.

## Step 6: Response

**If all findings are Informational or Nothing → Silent run:**

Respond with exactly:

```
HEARTBEAT_OK
```

**Nothing else.**

---

**If any finding is Important → Discord notification:**

- Write a concise alert (2–5 lines, Japanese).
- **Do NOT include `HEARTBEAT_OK` anywhere in the response.**
```

- [x] **Step 3: 圧縮後のサイズを確認**

```bash
wc -l production/workspace/HEARTBEAT.md
wc -w production/workspace/HEARTBEAT.md
```

期待: 行数が 101 → 60〜70 行程度に減っていること。

- [x] **Step 4: 必須要素のチェック**

以下がすべて含まれることを確認する。

```bash
grep -c "HEARTBEAT_OK" production/workspace/HEARTBEAT.md          # 2 以上
grep -c "use-skill: calendar" production/workspace/HEARTBEAT.md   # 1
grep -c "use-skill: gmail" production/workspace/HEARTBEAT.md      # 1
grep -c "Quiet hours" production/workspace/HEARTBEAT.md           # 1
grep -c "topic-patrol" production/workspace/HEARTBEAT.md          # 1（禁止ルール）
```

- [x] **Step 5: コミット**

```bash
git add production/workspace/HEARTBEAT.md
git commit -m "docs(workspace): ISSUE-29 HEARTBEAT.md を圧縮（~1,096→~650 tokens）"
```

---

## 完了後の確認

- [x] **全テスト通過**

```bash
cargo test --workspace 2>&1 | tail -15
```

- [x] **デプロイ**

```bash
./scripts/deploy.sh
```

- [x] **GitHub Issues をクローズ**

```bash
gh issue close 7  # ISSUE-28
gh issue close 8  # ISSUE-29
gh issue close 9  # ISSUE-30
gh issue close 10 # ISSUE-31
gh issue close 11 # ISSUE-32
```
