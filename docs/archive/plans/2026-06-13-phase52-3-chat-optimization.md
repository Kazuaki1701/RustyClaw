# Phase 52-3: Chat 最適化 実装計画書

> [!NOTE]
> **ステータス**: `[DONE]` (完了: 2026-06-13、PreCompact/SessionStart は Phase 52-3b に分割)
> **最終更新日**: 2026-06-13
> **対象コード**: `crates/rustyclaw-agent/src/lib.rs`, `crates/rustyclaw-gateway/src/lib.rs`, `crates/rustyclaw-gateway/src/skills.rs`
> **設計仕様書**: [`docs/specs/2026-06-13-phase52-context-optimization-design.md`](../specs/2026-06-13-phase52-context-optimization-design.md)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Chat（ユーザー対話）時のコンテキストを動的に最適化し、スキルの ctx_search 選択・USER.md 興味関心の RAG 注入・Karakeep 結果トリミングを実装する。

**Architecture:** 起動時に全 SKILL.md と USER.md Interests を context-mode にインデックス登録し、チャット受信時に ctx_search で関連スキルと興味関心を動的選択してシステムプロンプトに注入する。Karakeep ツール結果は `filter_seen_tool_result` で summary フィールドをトリミングする。`execute_with_rig_agent` に `extra_system_context` 引数を追加して動的コンテキスト注入経路を確立する。

**Tech Stack:** Rust 2024 Edition, `rustyclaw-agent`, `rustyclaw-gateway`, context-mode MCP (ctx_index / ctx_search)

**除外範囲（別計画）:** `PreCompact` / `SessionStart` フックによる SQLite スナップショット退避・復元は Phase 52-3b として別途計画する。

---

## 開発タスクチェックリスト

- [x] **Task 1: 起動時スキルインデックス登録**
- [x] **Task 2: ctx_search による動的スキル選択**
- [x] **Task 3: Karakeep 結果 summary トリミング**
- [x] **Task 4: USER.md Interests インデックス + execute_with_rig_agent への動的注入経路確立**
- [x] **Task 5: テスト・ビルド確認・コミット・ドキュメント更新**

---

## 前提知識: コード構造の概要

### スキル注入フロー（現状）

```
[gateway/lib.rs] ~line 708
  inject_skill_content(workspace_path, user_message)
    → load_skills()                    // 全 SKILL.md を読み込む
    → generate_skills_directory()      // Discovery: 全スキルの名前+説明一覧
    → keyword match in user_message    // 名前が含まれるなら全文注入 (Activation)
    → return injected_user_message     // Discovery + 選択スキル全文

[gateway/lib.rs] ~line 753
  pipeline.execute_with_rig_agent(
    workspace_path, session_id,
    &content,             // raw user message (DB 保存用)
    &injected_content,    // スキル注入済みユーザーメッセージ (LLM 送信用)
    tool_server_handle, run_purpose, progress_tx,
  )

[agent/lib.rs] execute_with_rig_agent()
  build_system_context()  // SOUL.md + USER.md 読み込み
  → system preamble に設定
  → injected_user_message を LLM に送信
```

### 関連する既存関数の場所

| 関数 | ファイル:行 |
|------|-----------|
| `inject_skill_content` | `gateway/src/skills.rs` (末尾付近) |
| `try_ctx_search` | `gateway/src/lib.rs:1037` |
| `try_ctx_index` | `gateway/src/lib.rs:1058` |
| `execute_with_rig_agent` | `agent/src/lib.rs:1421` |
| `build_system_context` | `agent/src/lib.rs:502` |
| `truncate_tool_item_fields` | `agent/src/lib.rs:1883` |
| `filter_seen_tool_result` | `agent/src/lib.rs:2028` |
| `start_context_mode` 呼び出し | `gateway/src/lib.rs:1151` |

---

## Task 1: 起動時スキルインデックス登録

context-mode が起動したあと、全 SKILL.md の内容を `[skill:NAME]` プレフィックス付きで ctx_index に登録する。これにより Task 2 の ctx_search がスキルを検索できるようになる。

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

- [ ] `Gateway::run()` 内の `start_context_mode` 呼び出し（~line 1151）の直後に以下を追加する

```rust
// スキルを context-mode に非同期インデックス登録（context-mode 起動後 3 秒待ってから実行）
{
    let ws = self.workspace_path.clone();
    let tsh = tool_server_handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        index_skills_to_context_mode(&ws, &tsh).await;
    });
}
```

- [ ] `start_context_mode` 関数の近く（~line 968）に以下の関数を追加する

```rust
/// 起動時に全 SKILL.md を context-mode にインデックス登録する。
/// [skill:NAME] プレフィックスを付けることで Task 2 の parse_skill_names_from_ctx が検索できる。
async fn index_skills_to_context_mode(
    workspace_path: &Path,
    handle: &rig_core::tool::server::ToolServerHandle,
) {
    let skills = crate::skills::load_skills(workspace_path);
    if skills.is_empty() {
        return;
    }
    for skill in &skills {
        let content = format!(
            "[skill:{}]\n{}\n{}",
            skill.manifest.name,
            skill.manifest.description,
            skill.instructions.trim()
        );
        let source = format!("skill:{}", skill.manifest.name);
        try_ctx_index(handle, &content, &source).await;
    }
    tracing::info!(
        "context-mode: {} スキルをインデックス登録完了",
        skills.len()
    );
}
```

- [ ] ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし（エラーなし）

---

## Task 2: ctx_search による動的スキル選択

チャット受信時に ctx_search でユーザーメッセージに関連するスキルを検索し、キーワードマッチング（全体一致）に代わって RAG ベースで Activation するスキルを決定する。

**Files:**
- Modify: `crates/rustyclaw-gateway/src/skills.rs`
- Modify: `crates/rustyclaw-gateway/src/lib.rs` (~line 708)

### Step 1: inject_skill_content_with_filter の追加

- [ ] `skills.rs` の既存 `inject_skill_content` 関数を **リファクタリング** して以下に置き換える

```rust
/// ゲートウェイ L708 で呼ばれるメインエントリーポイント（後方互換ラッパー）。
/// ctx_skill_names が None の場合はキーワードマッチングフォールバック。
pub fn inject_skill_content(workspace_path: &Path, content: &str) -> String {
    inject_skill_content_with_filter(workspace_path, content, None)
}

/// ctx_search 結果のスキル名リストを受け取り、そのスキルのみを Activation する版。
/// ctx_skill_names が Some([]) の場合は Activation なし（Discovery のみ）。
/// ctx_skill_names が None の場合はキーワードマッチングフォールバック（既存の動作）。
pub fn inject_skill_content_with_filter(
    workspace_path: &Path,
    content: &str,
    ctx_skill_names: Option<&[String]>,
) -> String {
    let skills = load_skills(workspace_path);
    if skills.is_empty() {
        return content.to_string();
    }

    let skills_directory = generate_skills_directory(&skills);
    let search_target = content.to_lowercase();
    let mut injected_instructions = String::new();

    for skill in &skills {
        let should_inject = match ctx_skill_names {
            Some(names) => names.iter().any(|n| n == &skill.manifest.name),
            None => {
                // フォールバック: キーワードマッチング（既存の動作）
                let trigger_tag = format!("use-skill: {}", skill.manifest.name);
                let name_match = format!("skill:{}", skill.manifest.name);
                search_target.contains(&trigger_tag)
                    || search_target.contains(&name_match)
                    || search_target.contains(&skill.manifest.name)
            }
        };

        if should_inject {
            tracing::info!(
                "Activation: Dynamic loading of skill '{}' into prompt",
                skill.manifest.name
            );
            injected_instructions.push_str(&format!(
                "\n\n--- [ACTIVE SKILL: {}] ---\n{}\n",
                skill.manifest.name,
                rewrite_relative_links(skill.instructions.trim(), &skill.manifest.name)
            ));
        }
    }

    let mut final_content = content.to_string();
    if !skills_directory.is_empty() {
        final_content = format!("{}{}", final_content, skills_directory);
    }
    if !injected_instructions.is_empty() {
        final_content = format!(
            "{}\n\n---\n\n{}",
            injected_instructions.trim(),
            final_content
        );
    }
    final_content
}
```

### Step 2: parse_skill_names_from_ctx の追加

- [ ] `gateway/src/lib.rs` に以下の関数を追加する（`try_ctx_index` の近く）

```rust
/// ctx_search の返却テキストから [skill:NAME] パターンを抽出してスキル名リストに変換する。
fn parse_skill_names_from_ctx(ctx: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\[skill:([a-z][a-z0-9-]*)\]").expect("valid regex");
    let names: std::collections::HashSet<String> = re
        .captures_iter(ctx)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();
    names.into_iter().collect()
}
```

### Step 3: ゲートウェイチャットハンドラの更新

- [ ] `gateway/src/lib.rs` ~line 707-711 の `inject_skill_content` 呼び出しを以下に置き換える

```rust
// ctx_search でスキルを動的選択（cron セッション以外のみ）
let ctx_skill_names: Option<Vec<String>> = if !session_id.starts_with("cron:") {
    try_ctx_search(&tool_server_handle, &content)
        .await
        .map(|ctx| parse_skill_names_from_ctx(&ctx))
        .filter(|names| !names.is_empty())
} else {
    None
};

// スキルファイル注入（ctx_search 結果のスキルのみ Activation、Discovery は全件）
let injected_content = crate::skills::inject_skill_content_with_filter(
    &workspace_path,
    &content,
    ctx_skill_names.as_deref(),
);
```

- [ ] ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 3: Karakeep 結果 summary トリミング

Karakeep ブックマーク一覧 (`503_karakeep-list.sh`) の返却 JSON は `{"bookmarks": [...]}` 形式で各アイテムに `summary` フィールドを持つ（長大になりうる）。`filter_seen_tool_result` の手前でトリミングする。

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

### Step 1: truncate_tool_item_fields に karakeep を追加

- [ ] `lib.rs:1885` の `match category` ブロックに `"karakeep"` を追加する

```rust
fn truncate_tool_item_fields(category: &str, mut item: serde_json::Value) -> serde_json::Value {
    const MAX_CHARS: usize = 200;
    let field = match category {
        "gmail" => "snippet",
        "calendar" => "location",
        "karakeep" => "summary",   // ← 追加
        _ => return item,
    };
    if let Some(val) = item.get(field).and_then(|v| v.as_str())
        && val.chars().count() > MAX_CHARS
    {
        let truncated: String = val.chars().take(MAX_CHARS).collect();
        item[field] = serde_json::Value::String(format!("{}…", truncated));
    }
    item
}
```

### Step 2: truncate_karakeep_result 関数の追加

Karakeep API は `{bookmarks: [...]}` 形式（Gmail/Calendar のフラット配列とは異なる）のため、専用の処理関数を追加する。

- [ ] `truncate_tool_item_fields` 関数の直後に以下を追加する

```rust
/// Karakeep ツール結果の summary フィールドをトリミングする。
/// Karakeep API は {"bookmarks": [...]} 形式を返すため、Gmail/Calendar とは別処理。
fn truncate_karakeep_result(tool_result: &str) -> String {
    let stdout = extract_stdout(tool_result);
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return tool_result.to_string();
    };
    let Some(arr) = root
        .get("bookmarks")
        .and_then(|b| b.as_array())
        .map(|a| a.clone())
    else {
        return tool_result.to_string();
    };
    let truncated: Vec<serde_json::Value> = arr
        .into_iter()
        .map(|item| truncate_tool_item_fields("karakeep", item))
        .collect();
    root["bookmarks"] = serde_json::Value::Array(truncated);
    let new_json = serde_json::to_string_pretty(&root)
        .unwrap_or_else(|_| tool_result.to_string());
    rebuild_tool_result(tool_result, &new_json)
}
```

### Step 3: filter_seen_tool_result に karakeep 検出を追加

- [ ] `lib.rs:2028` の `filter_seen_tool_result` 関数先頭（`if tool_name != "run_workspace_script"` チェックのあと、`gmail`/`calendar` 判定の前）に以下を追加する

```rust
// karakeep: summary トリミングのみ（重複除外なし）
if script_name.contains("karakeep") {
    return truncate_karakeep_result(tool_result);
}
```

- [ ] ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-agent 2>&1 | grep "^error"
```

期待: 出力なし

---

## Task 4: USER.md Interests インデックス + execute_with_rig_agent への動的注入経路確立

USER.md の Interests セクションを起動時に ctx_index 登録し、チャット時に ctx_search で取得して `execute_with_rig_agent` に渡す。
エージェント側では新設の `extra_system_context: Option<String>` 引数でシステムプロンプトに末尾追加する。

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs` (~line 1421)
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

### Step 1: execute_with_rig_agent に extra_system_context 引数を追加

- [ ] `agent/src/lib.rs:1421` の `execute_with_rig_agent` のシグネチャを変更する

```rust
pub async fn execute_with_rig_agent(
    &self,
    workspace_dir: &Path,
    session_id: &str,
    raw_user_message: &str,
    injected_user_message: &str,
    tool_handle: rig_core::tool::server::ToolServerHandle,
    purpose: &str,
    progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
    extra_system_context: Option<String>,  // 追加: 動的注入コンテキスト（USER.md Interests 等）
) -> Result<LlmResponse> {
```

- [ ] 同関数の `system_context` 構築完了直後（`let now = chrono::Local::now();` の直前）に以下を追加する

```rust
// 動的注入コンテキスト（ctx_search で取得した USER.md Interests 等）を追記
if let Some(extra) = extra_system_context {
    system_context.push_str(&extra);
}
```

- [ ] `gateway/src/lib.rs:753` の呼び出し箇所を更新する（末尾に `None` を追加、Task 4 Step 3 で実際の値に差し替え）

```rust
pipeline
    .execute_with_rig_agent(
        &workspace_path,
        &session_id,
        &content,
        &injected_content,
        tool_server_handle.clone(),
        run_purpose,
        progress_tx_opt,
        None, // extra_system_context: Task 4 Step 3 で実装
    )
    .await
```

- [ ] ビルドが通ることを確認する

```bash
cargo build -p rustyclaw-agent -p rustyclaw-gateway 2>&1 | grep "^error"
```

期待: 出力なし

### Step 2: 起動時 USER.md Interests インデックス登録

- [ ] `gateway/src/lib.rs` に以下の関数 2 つを追加する（`index_skills_to_context_mode` の近く）

```rust
/// USER.md の "## Interests" セクションの本文を抽出して返す。
fn extract_interests_section(content: &str) -> String {
    let mut in_section = false;
    let mut lines: Vec<&str> = Vec::new();
    for line in content.lines() {
        if line.trim_start().starts_with("## Interests") {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with("## ") {
                break;
            }
            if !line.trim().is_empty() {
                lines.push(line);
            }
        }
    }
    lines.join("\n")
}

/// 起動時に USER.md の Interests セクションを context-mode にインデックス登録する。
/// [user-interests] プレフィックスを付けることで ctx_search 結果から識別できる。
async fn index_user_interests(
    workspace_path: &Path,
    handle: &rig_core::tool::server::ToolServerHandle,
) {
    let user_md_path = workspace_path.join("USER.md");
    let content = match std::fs::read_to_string(&user_md_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("USER.md 読み込み失敗（Interests インデックス登録スキップ）: {}", e);
            return;
        }
    };
    let interests = extract_interests_section(&content);
    if interests.is_empty() {
        tracing::info!("USER.md に Interests セクションが見つからないためスキップ");
        return;
    }
    let indexed = format!("[user-interests]\n{}", interests);
    try_ctx_index(handle, &indexed, "user-interests").await;
    tracing::info!("context-mode: USER.md Interests インデックス登録完了");
}
```

- [ ] `Gateway::run()` の `index_skills_to_context_mode` spawn ブロックの直後に以下を追加する

```rust
// USER.md Interests を context-mode に非同期インデックス登録（4 秒後）
{
    let ws = self.workspace_path.clone();
    let tsh = tool_server_handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        index_user_interests(&ws, &tsh).await;
    });
}
```

### Step 3: チャットハンドラで ctx_search 呼び出し → extra_system_context に渡す

- [ ] `gateway/src/lib.rs` の `execute_with_rig_agent` 呼び出し箇所（Task 4 Step 1 で `None` にした箇所）の直前に以下を追加する

```rust
// USER.md Interests を ctx_search で動的取得（cron 以外のみ）
let user_interests_extra: Option<String> = if !session_id.starts_with("cron:") {
    let query = format!("{} user interests hobbies", content);
    try_ctx_search(&tool_server_handle, &query)
        .await
        .filter(|r| r.contains("[user-interests]"))
        .map(|r| format!("\n\n# Relevant User Interests\n{}", r))
} else {
    None
};
```

- [ ] 同箇所の `execute_with_rig_agent` 呼び出しの `None` を `user_interests_extra` に差し替える

```rust
pipeline
    .execute_with_rig_agent(
        &workspace_path,
        &session_id,
        &content,
        &injected_content,
        tool_server_handle.clone(),
        run_purpose,
        progress_tx_opt,
        user_interests_extra,  // None から変更
    )
    .await
```

---

## Task 5: テスト・Clippy・コミット・ドキュメント更新

**Files:**
- Modify: `crates/rustyclaw-gateway/src/skills.rs` (テスト追加)
- Modify: `crates/rustyclaw-agent/src/lib.rs` (テスト追加)
- Modify: `docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md`
- Modify: `docs/task.md`

### Step 1: テストを追加する

#### gateway/src/skills.rs に追加

```rust
#[test]
fn test_inject_skill_content_with_filter_named_skills_only() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("skills");

    let skill_a = skills_dir.join("skill-alpha");
    std::fs::create_dir_all(&skill_a).unwrap();
    std::fs::write(
        skill_a.join("SKILL.md"),
        "---\nname: skill-alpha\ndescription: Alpha skill.\n---\n# Alpha Instructions\nDo alpha.",
    )
    .unwrap();

    let skill_b = skills_dir.join("skill-beta");
    std::fs::create_dir_all(&skill_b).unwrap();
    std::fs::write(
        skill_b.join("SKILL.md"),
        "---\nname: skill-beta\ndescription: Beta skill.\n---\n# Beta Instructions\nDo beta.",
    )
    .unwrap();

    let names = vec!["skill-alpha".to_string()];
    let result = inject_skill_content_with_filter(dir.path(), "hello", Some(&names));

    // Discovery には両スキルが含まれる
    assert!(result.contains("skill-alpha"), "Discovery に skill-alpha が含まれること");
    assert!(result.contains("skill-beta"), "Discovery に skill-beta が含まれること");

    // Activation は skill-alpha のみ（skill-beta は除外）
    assert!(result.contains("Alpha Instructions"), "skill-alpha の本文が含まれること");
    assert!(!result.contains("Beta Instructions"), "skill-beta の本文は除外されること");
}

#[test]
fn test_inject_skill_content_with_filter_none_falls_back_to_keyword() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("skills");

    let skill_a = skills_dir.join("skill-gamma");
    std::fs::create_dir_all(&skill_a).unwrap();
    std::fs::write(
        skill_a.join("SKILL.md"),
        "---\nname: skill-gamma\ndescription: Gamma skill.\n---\n# Gamma Instructions\nDo gamma.",
    )
    .unwrap();

    // None: フォールバック（スキル名が user message に含まれる場合 Activation）
    let result_with_name = inject_skill_content_with_filter(dir.path(), "skill-gamma rules", None);
    assert!(result_with_name.contains("Gamma Instructions"), "名前一致でActivationされること");

    let result_without_name = inject_skill_content_with_filter(dir.path(), "hello world", None);
    assert!(!result_without_name.contains("Gamma Instructions"), "名前なしでActivationされないこと");
}
```

#### gateway/src/lib.rs に追加

```rust
#[cfg(test)]
mod tests_ctx {
    use super::*;

    #[test]
    fn test_parse_skill_names_from_ctx() {
        let ctx = "Some context\n[skill:home-assistant-rest-api]\nSome instructions\n[skill:gmail]\nMore text";
        let names = parse_skill_names_from_ctx(ctx);
        assert!(names.contains(&"home-assistant-rest-api".to_string()));
        assert!(names.contains(&"gmail".to_string()));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_parse_skill_names_from_ctx_empty() {
        let names = parse_skill_names_from_ctx("no skill markers here");
        assert!(names.is_empty());
    }
}
```

#### agent/src/lib.rs に追加

```rust
#[test]
fn test_truncate_tool_item_fields_karakeep_summary() {
    let long_summary = "A".repeat(300);
    let item = serde_json::json!({
        "id": "bookmark1",
        "title": "My Bookmark",
        "summary": long_summary,
    });
    let result = truncate_tool_item_fields("karakeep", item);
    let summary = result["summary"].as_str().unwrap();
    assert!(summary.chars().count() <= 202, "200文字 + '…' で201文字以内");
    assert!(summary.ends_with('…'));
}

#[test]
fn test_extract_interests_section() {
    // この関数は gateway にあるため、gateway/src/lib.rs のテストモジュールに追加する
    // （下記は gateway のテスト例）
}
```

#### gateway/src/lib.rs の tests_ctx モジュールに追加

```rust
#[test]
fn test_extract_interests_section_extracts_correctly() {
    let user_md = "# User Profile\n\n## Basics\n- Name: K\n\n## Interests\n- AI Agent\n  sources: HN\n- Cloudflare\n  sources: blog\n\n## Work Context\n- Workplace: Atsugi";
    let interests = extract_interests_section(user_md);
    assert!(interests.contains("AI Agent"), "Interests セクションが抽出されること");
    assert!(interests.contains("Cloudflare"), "複数行が含まれること");
    assert!(!interests.contains("Basics"), "Basics セクションは除外されること");
    assert!(!interests.contains("Work Context"), "Work Context セクションは除外されること");
}
```

### Step 2: 全テストを実行する

```bash
TZ=UTC cargo test --all-features --workspace 2>&1 | grep -E "^test result|FAILED"
```

期待: 全 crate で `test result: ok`

### Step 3: Clippy を確認する

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep "^error"
```

期待: 出力なし

### Step 4: コミットする

```bash
git checkout -b feat/phase52-3
git add crates/rustyclaw-agent/src/lib.rs crates/rustyclaw-gateway/src/lib.rs crates/rustyclaw-gateway/src/skills.rs
git commit -m "feat(chat): Phase 52-3 スキル動的選択・Interests RAG 注入・Karakeep トリミング"
```

### Step 5: ドキュメント更新とマージ

- [ ] 本計画書のチェックリストをすべて `[x]` に更新する
- [ ] `docs/plans/2026-06-13-phase52-context-optimization-implementation-plan.md` の Phase 52-3 チェックリストを `[x]` に更新する
- [ ] `docs/task.md` の Phase 52-3 に完了日を追記する

```bash
git add docs/
git commit -m "docs(phase52): Phase 52-3 完了チェックリスト更新"
git checkout main
git merge --no-ff feat/phase52-3 -m "feat(phase52-3): Chat 専用コンテキスト最適化（スキル動的選択・Interests RAG・Karakeep トリミング）"
git branch -d feat/phase52-3
```
