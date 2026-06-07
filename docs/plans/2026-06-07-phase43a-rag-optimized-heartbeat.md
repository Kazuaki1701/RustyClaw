# Phase 43-A RAG 最適化 Heartbeat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** RAG 導入前の Context Window 削減策（MEMORY.md サイズ上限・flush Δ 閾値・heartbeat_top_k 抑制）を廃止し、`chunk_memory_md` のチャンク戦略を section prefix 付き隣接バレット結合に刷新することで、Heartbeat パスの RAG 検索精度を向上させる。

**Architecture:** 変更対象は `crates/rustyclaw-agent/src/lib.rs` 1ファイルのみ。`chunk_memory_md`（行1917）の実装置き換え、`flush_memory`（行495・582・598・653-661）の制約削除、`execute_heartbeat`（行754）の top_k 引き上げ、`ingest_static_documents`（行2049）への USER.md 追加の4箇所を独立したタスクで順番に実施する。

**Tech Stack:** Rust 2024 Edition、`rustyclaw-agent`、`tempfile`（テスト用）

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 修正 | 4箇所（chunk_memory_md・flush_memory・execute_heartbeat・ingest_static_documents） |
| `docs/task.md` | 修正 | Phase 43-A 完了マーク |

---

## Task 1: `chunk_memory_md` の再設計

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:1917-1950`（実装）
- Modify: `crates/rustyclaw-agent/src/lib.rs:3360-3382`（既存テスト更新）+ 新テスト追加

### 背景

現在の `chunk_memory_md` はバレット行を1件ずつ独立したチャンクにし、セクション情報を付与しない。新実装では各チャンク先頭に `[SectionName]` を付与し、隣接するバレットを 800 bytes 以内で結合する。これにより RAG 検索時のコンテキストが改善される。

---

- [ ] **Step 1: 新しいテストを書く（まだ失敗する）**

`crates/rustyclaw-agent/src/lib.rs` の `mod tests` 内、`test_chunk_memory_md_skips_headers` テスト（行3378）の直後に以下を追加する:

```rust
    #[test]
    fn test_chunk_memory_md_section_prefix() {
        let content = "## User Preferences\n\n- First bullet\n- Second bullet";
        let chunks = chunk_memory_md(content);
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].starts_with("[User Preferences]"),
            "expected [User Preferences] prefix, got: {}",
            chunks[0]
        );
        assert!(chunks[0].contains("First bullet"));
        assert!(chunks[0].contains("Second bullet"));
    }

    #[test]
    fn test_chunk_memory_md_adjacent_bullets_merged() {
        let content = "## Section\n\n- bullet one\n- bullet two\n- bullet three";
        let chunks = chunk_memory_md(content);
        assert_eq!(
            chunks.len(),
            1,
            "adjacent bullets within 800 bytes should be merged"
        );
        assert!(chunks[0].contains("bullet one"));
        assert!(chunks[0].contains("bullet two"));
        assert!(chunks[0].contains("bullet three"));
    }

    #[test]
    fn test_chunk_memory_md_split_on_overflow() {
        // 各バレット 298 bytes × 3 = 894 bytes → 800 を超えるので分割される
        let long_bullet = format!("- {}", "x".repeat(295));
        let content = format!(
            "## Section\n\n{}\n{}\n{}",
            long_bullet, long_bullet, long_bullet
        );
        let chunks = chunk_memory_md(&content);
        assert!(
            chunks.len() >= 2,
            "should split when total exceeds 800 bytes, got {} chunk(s)",
            chunks.len()
        );
        for chunk in &chunks {
            assert!(
                chunk.starts_with("[Section]"),
                "each chunk must carry section prefix, got: {}",
                &chunk[..chunk.len().min(30)]
            );
        }
    }

    #[test]
    fn test_chunk_memory_md_no_section_uses_general() {
        let content = "- bullet without any section header";
        let chunks = chunk_memory_md(content);
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].starts_with("[General]"),
            "no-section content must use [General] prefix, got: {}",
            chunks[0]
        );
    }
```

- [ ] **Step 2: テストが失敗することを確認する**

```bash
cargo test -p rustyclaw-agent -- test_chunk_memory_md_section_prefix test_chunk_memory_md_adjacent_bullets_merged test_chunk_memory_md_split_on_overflow test_chunk_memory_md_no_section_uses_general 2>&1 | tail -15
```

期待: 4件すべて FAILED（現在の実装は prefix を付与しない）

- [ ] **Step 3: `chunk_memory_md` を新実装に置き換える**

`crates/rustyclaw-agent/src/lib.rs` の行 1917-1950 の `chunk_memory_md` 関数全体を以下に置き換える:

```rust
pub(crate) fn chunk_memory_md(content: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_section = "General".to_string();
    let mut current_bullets: Vec<String> = Vec::new();
    let mut current_len: usize = 0;
    const MAX_CHUNK: usize = 800;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            if !current_bullets.is_empty() {
                chunks.push(format!("[{}] {}", current_section, current_bullets.join("\n")));
                current_bullets.clear();
                current_len = 0;
            }
            let section = trimmed.trim_start_matches('#').trim().to_string();
            current_section = if section.is_empty() {
                "General".to_string()
            } else {
                section
            };
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if current_len + trimmed.len() + 1 > MAX_CHUNK && !current_bullets.is_empty() {
                chunks.push(format!("[{}] {}", current_section, current_bullets.join("\n")));
                current_bullets.clear();
                current_len = 0;
            }
            current_len += trimmed.len() + 1;
            current_bullets.push(trimmed.to_string());
        }
    }
    if !current_bullets.is_empty() {
        chunks.push(format!("[{}] {}", current_section, current_bullets.join("\n")));
    }
    chunks.into_iter().filter(|s| !s.is_empty()).collect()
}
```

- [ ] **Step 4: 既存テストを新しい挙動に合わせて更新する**

既存の 3 つのテストは新実装で期待値が変わるため更新する。

**`test_chunk_memory_md_basic`（行3360-3367）を以下に置き換える:**

```rust
    #[test]
    fn test_chunk_memory_md_basic() {
        let content = "# Memory\n\n- First bullet\n- Second bullet\n  continued\n- Third bullet";
        let chunks = chunk_memory_md(content);
        // 3バレットが800bytes以内なので1チャンクに結合される
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].starts_with("[Memory]"), "got: {}", chunks[0]);
        assert!(chunks[0].contains("First bullet"));
        assert!(chunks[0].contains("Second bullet"));
        assert!(chunks[0].contains("Third bullet"));
    }
```

**`test_chunk_memory_md_truncates_long`（行3370-3375）を以下に置き換える:**

```rust
    #[test]
    fn test_chunk_memory_md_long_bullet_single_chunk() {
        // 600 bytes のバレット1件は単独でチャンクになる（800 以内）
        let long = format!("- {}", "x".repeat(597));
        let chunks = chunk_memory_md(&long);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].starts_with("[General]"), "got: {}", chunks[0]);
    }
```

**`test_chunk_memory_md_skips_headers`（行3378-3382）を以下に置き換える:**

```rust
    #[test]
    fn test_chunk_memory_md_section_boundary() {
        // セクションをまたぐバレットは別チャンクになる
        let content = "## Section A\n\n- bullet a\n\n## Section B\n\n- bullet b";
        let chunks = chunk_memory_md(content);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with("[Section A]"), "got: {}", chunks[0]);
        assert!(chunks[1].starts_with("[Section B]"), "got: {}", chunks[1]);
    }
```

- [ ] **Step 5: 新テストと既存テストが通ることを確認する**

```bash
cargo test -p rustyclaw-agent -- test_chunk_memory_md 2>&1 | tail -15
```

期待:
```
test tests::test_chunk_memory_md_basic ... ok
test tests::test_chunk_memory_md_long_bullet_single_chunk ... ok
test tests::test_chunk_memory_md_section_boundary ... ok
test tests::test_chunk_memory_md_section_prefix ... ok
test tests::test_chunk_memory_md_adjacent_bullets_merged ... ok
test tests::test_chunk_memory_md_split_on_overflow ... ok
test tests::test_chunk_memory_md_no_section_uses_general ... ok
```

- [ ] **Step 6: Clippy・全テストを確認する**

```bash
cargo clippy --all-targets 2>&1 | grep "^error" && cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```

期待: error なし、全テスト ok

- [ ] **Step 7: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 43-A chunk_memory_md を section prefix + 隣接バレット結合に再設計"
```

---

## Task 2: `flush_memory` の制約撤廃

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:495`（DELTA_THRESHOLD）
- Modify: `crates/rustyclaw-agent/src/lib.rs:582,598`（LLM プロンプト）
- Modify: `crates/rustyclaw-agent/src/lib.rs:653-661`（truncate 削除）

### 背景

`flush_memory` には RAG 導入前の名残が 3 箇所ある: (1) 6メッセージ未満はスキップする Δ 閾値、(2) LLM プロンプトに「5000文字以下」と指示している文言、(3) LLM 出力が 5000 bytes を超えた場合の `truncate_70_20` フェイルセーフ。これらを廃止する。

---

- [ ] **Step 1: DELTA_THRESHOLD を 6 → 3 に変更する**

行 495 を以下に変更する:

```rust
        const DELTA_THRESHOLD: usize = 3;
```

- [ ] **Step 2: LLM プロンプトのサイズ制約指示を削除する**

行 576-600 の `system_prompt` 変数内から以下の 2 行を削除する（前後の行はそのまま残す）:

削除対象行:
```
   - Stays strictly under 5KB (≤ 5000 characters)
```
および:
```
- Keep MEMORY.md total under 5000 characters.
```

変更後の `system_prompt` の該当部分:

```rust
        let system_prompt = "\
You are a memory manager. Given the current MEMORY.md and a recent conversation, your tasks are:

1. Produce a fully rewritten MEMORY.md that:
   - Incorporates important new facts, decisions, preferences, and learnings from the conversation
   - Removes outdated or redundant information
   - Uses concise, factual bullet points under clear headings
   - If nothing new is worth adding and the existing content is fine, output it unchanged

2. Produce a concise bulleted daily log entry summarising what happened in the conversation.

Output using exactly these delimiters (no extra text outside them):
---NEW_MEMORY---
<complete rewritten MEMORY.md content>
---END_MEMORY---
---DAILY_LOG---
<bulleted activity summary>
---END_DAILY_LOG---

Rules:
- Never truncate mid-sentence inside ---NEW_MEMORY---.
- Write in the same language as the existing MEMORY.md content.
";
```

- [ ] **Step 3: truncate フェイルセーフを削除する**

行 651-661 を以下に置き換える（`if content.len() > 5000 { ... } else { content }` を削除し直接書き込みに変更）:

変更前:
```rust
        // 1. MEMORY.md の全書き換え (fail-open)
        if let Some(content) = new_memory {
            // LLM が 5KB を超えて返した場合のフェイルセーフとして 70/20 トランケート
            let final_content = if content.len() > 5000 {
                tracing::warn!(
                    actual_bytes = content.len(),
                    "memory flush: LLM returned oversized MEMORY.md, truncating"
                );
                truncate_70_20(&content, 5000)
            } else {
                content
            };
```

変更後:
```rust
        // 1. MEMORY.md の全書き換え (fail-open)
        if let Some(final_content) = new_memory {
```

- [ ] **Step 4: Clippy・全テストを確認する**

```bash
cargo clippy --all-targets 2>&1 | grep "^error" && cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```

期待: error なし、全テスト ok

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 43-A flush_memory の Δ 閾値緩和・5000byte 上限・truncate 廃止"
```

---

## Task 3: `heartbeat_top_k` 引き上げ + USER.md RAG 追加

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:754`（top_k デフォルト値）
- Modify: `crates/rustyclaw-agent/src/lib.rs:2049`（USER.md 追加）

---

- [ ] **Step 1: `heartbeat_top_k` デフォルトを 2 → 3 に変更する**

行 754 の `unwrap_or(2)` を `unwrap_or(3)` に変更する:

変更前:
```rust
            .unwrap_or(2);
```

変更後:
```rust
            .unwrap_or(3);
```

（前後の文脈: `.and_then(|e| e.heartbeat_top_k)` の直後の行）

- [ ] **Step 2: `ingest_static_documents` に USER.md を追加する**

行 2049 を以下に変更する:

変更前:
```rust
    let mut files = vec![workspace_dir.join("AGENTS.md")];
```

変更後:
```rust
    let mut files = vec![workspace_dir.join("AGENTS.md"), workspace_dir.join("USER.md")];
```

- [ ] **Step 3: Clippy・全テストを確認する**

```bash
cargo clippy --all-targets 2>&1 | grep "^error" && cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```

期待: error なし、全テスト ok

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "feat(agent): Phase 43-A heartbeat_top_k を 3 に引き上げ・USER.md を RAG コーパスに追加"
```

---

## Task 4: `docs/task.md` 更新と PR 作成

**Files:**
- Modify: `docs/task.md`

---

- [ ] **Step 1: `docs/task.md` の Phase 43-A 行を追加する**

`docs/task.md` の Phase 42 ブロック直後（`- \`[x]\` **Phase 42: ...` の次の空行以降）に以下を追加する:

```markdown
- `[x]` **Phase 43-A: RAG 最適化 Heartbeat（旧 Context 削減策の廃止）**
  - `[x]` chunk_memory_md: section prefix + 隣接バレット結合（800 chars）
  - `[x]` flush_memory: Δ 閾値 6→3、5000 byte 上限廃止、truncate_70_20 廃止
  - `[x]` heartbeat_top_k: 2→3（TPM 安全マージン確保）
  - `[x]` USER.md を ingest_static_documents の RAG コーパスに追加
```

- [ ] **Step 2: コミット**

```bash
git add docs/task.md
git commit -m "chore(task): Phase 43-A 完了マーク"
```

- [ ] **Step 3: push して PR を作成する**

```bash
git push origin <current-branch>
gh pr create \
  --title "feat(agent): Phase 43-A RAG 最適化 Heartbeat — 旧 Context 削減策廃止と chunk_memory_md 再設計" \
  --body "$(cat <<'EOF'
## Summary

- \`chunk_memory_md\`: section prefix \`[SectionName]\` 付与 + 隣接バレット 800 bytes 以内で結合（RAG 検索精度向上）
- \`flush_memory\`: Δ 閾値 6→3 に緩和、MEMORY.md 5000 byte 上限廃止、\`truncate_70_20\` フェイルセーフ廃止
- \`execute_heartbeat\`: \`heartbeat_top_k\` デフォルト 2→3（groq TPM=6,000 に対し ~79% でマージン確保）
- \`ingest_static_documents\`: USER.md を RAG コーパスに追加（Step 2 クエリで自然に召喚）

## Test Plan

- [x] \`cargo test --all\` — 全テスト通過
- [x] \`cargo clippy --all-targets\` — エラーなし
- [x] \`test_chunk_memory_md_section_prefix\` — \`[SectionName]\` prefix が付与される
- [x] \`test_chunk_memory_md_adjacent_bullets_merged\` — 隣接バレットが結合される
- [x] \`test_chunk_memory_md_split_on_overflow\` — 800 bytes 超過で分割される
- [x] \`test_chunk_memory_md_no_section_uses_general\` — section なしは \`[General]\`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: PR をマージして main を更新する**

```bash
gh pr merge --merge
git checkout main
git pull origin main
git branch -d <feature-branch>
git push origin --delete <feature-branch>
```

---

## Self-Review チェックリスト（実施済み）

**1. Spec coverage:**
- ✅ MEMORY.md 5000 byte 上限廃止 → Task 2 Step 3
- ✅ truncate_70_20 廃止 → Task 2 Step 3
- ✅ flush Δ 閾値 6→3 → Task 2 Step 1
- ✅ LLM プロンプトの文字数指示削除 → Task 2 Step 2
- ✅ chunk_memory_md 再設計（section prefix + 800 chars 結合） → Task 1
- ✅ heartbeat_top_k 2→3 → Task 3 Step 1
- ✅ USER.md を RAG コーパスに追加 → Task 3 Step 2

**2. Placeholder scan:** なし ✅

**3. Type consistency:** `chunk_memory_md` の戻り値 `Vec<String>` は変更なし。呼び出し元 `ingest_memory_md` の `chunks` 変数型に影響なし ✅
