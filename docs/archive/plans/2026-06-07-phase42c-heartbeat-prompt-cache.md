# Phase 42-C 設計書: Heartbeat プロンプトキャッシュ最適化

**日付**: 2026-06-07  
**案件番号**: Phase 42-C  
**ステータス**: `[HISTORICAL]` (過去の設計書 — 実装完了済み)

---

## 概要

`build_heartbeat_context` が `[now: timestamp]` を返値に含めているため、`execute_heartbeat` が後からRAGチャンクを追加しても `[now:]` が静的プレフィックス末尾ではなくRAGの前に挟まれた状態になっている。`[now:]` を `build_heartbeat_context` から取り除き、`execute_heartbeat` のRAG注入直前に移動することで、静的コンテンツ（SOUL.md / HEARTBEAT.md）と動的コンテンツ（`[now:]` / RAGチャンク）の境界を明確にする。

---

## 背景

Phase 42-A/B で `execute_heartbeat` のRAGブロックを3クエリに拡張した結果、`system_context` の実際の構造は以下になっている:

```
build_heartbeat_context の返値:
  # SOUL.md
  # HEARTBEAT.md
  [now: 2026-06-07T...]   ← ここが問題

execute_heartbeat が追加:
  <RAG chunk>
  ## Step 2 関連記憶
  <RAG chunk>
  ## Step 6 関連記憶
  <RAG chunk>
```

`[now:]` のコメント（line 707）には「プロンプトキャッシュ prefix を安定させる」と書かれているが、RAGチャンクがその後に追加されるため、安定している静的prefixは SOUL.md + HEARTBEAT.md のみである。`[now:]` を `build_heartbeat_context` の内側に置く意味がなく、むしろ「この関数の出力は静的か否か」を曖昧にしている。

比較: `build_system_context`（通常チャット用）は RAGを追加しないため、`[now:]` を末尾に置く既存実装が正しく機能している。

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | `build_heartbeat_context` から `[now:]` 削除 + `execute_heartbeat` に移動 + テスト追加 |

ゲートウェイ側・その他クレートの変更なし。

---

## 詳細設計

### 1. `build_heartbeat_context` の変更

#### 変更前

```rust
pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
    let files = ["SOUL.md", "HEARTBEAT.md"];
    let mut context = String::new();
    // ... ファイル読み込み ...
    // 動的ブロック（現在時刻）は末尾に配置
    let now = chrono::Local::now();
    context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));
    Ok(context)
}
```

#### 変更後

```rust
pub fn build_heartbeat_context(&self, workspace_dir: &Path) -> Result<String> {
    let files = ["SOUL.md", "HEARTBEAT.md"];
    let mut context = String::new();
    // ... ファイル読み込み（変更なし）...
    Ok(context)
}
```

関数が純粋な静的コンテンツのみを返すようになる。docコメントも「[now:] を末尾に置く」記述を削除する。

### 2. `execute_heartbeat` の変更

`build_heartbeat_context` 呼び出しの直後・RAG注入ブロックの直前に `[now:]` を追加する。

#### 変更前（line 746付近）

```rust
let mut system_context = self.build_heartbeat_context(workspace_dir)?;

// RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
let hb_top_k = ...
```

#### 変更後

```rust
let mut system_context = self.build_heartbeat_context(workspace_dir)?;
let now = chrono::Local::now();
system_context.push_str(&format!("[now: {}]\n", now.format("%Y-%m-%dT%H:%M:%S%:z")));

// RAG: heartbeat プロンプトに関連チャンクを注入 (ISSUE-27)
let hb_top_k = ...
```

### 3. `system_context` の構造（変更後）

```
# SOUL.md
<SOUL.md の内容>

# HEARTBEAT.md
<HEARTBEAT.md の内容>

─── 静的 / 動的 境界 ───────────────
[now: 2026-06-07T...]
<RAG chunk（Step 1 活動把握）>
## Step 2 関連記憶
<RAG chunk>
## Step 6 関連記憶
<RAG chunk>
```

---

## エラーハンドリング

変更なし。`[now:]` の生成は `chrono::Local::now()` で失敗しない。

---

## テスト方針

### 新規テスト: `test_build_heartbeat_context_is_static`

`build_heartbeat_context` の出力に `[now:]` が含まれないことを確認する。`build_system_context` の同等テスト（line 2798付近）に相当する逆方向のアサーション。

```rust
#[test]
fn test_build_heartbeat_context_is_static() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();
    std::fs::write(ws.join("SOUL.md"), "# Soul").unwrap();
    std::fs::write(ws.join("HEARTBEAT.md"), "# Heartbeat").unwrap();

    let config = make_test_config_with_url("http://localhost");
    let flush_sem = Arc::new(Semaphore::new(1));
    let pipeline = Pipeline::new(config, flush_sem);
    let context = pipeline.build_heartbeat_context(ws).unwrap();

    assert!(context.contains("# SOUL.md"));
    assert!(context.contains("# HEARTBEAT.md"));
    assert!(
        !context.contains("[now: "),
        "build_heartbeat_context must not include [now:] — dynamic content belongs in execute_heartbeat"
    );
}
```

### 既存テストの変更

`test_build_heartbeat_context_does_not_include_memory_md`（line 4024）は `[now:]` をチェックしていないため変更不要。

`cargo build --all` / `cargo test --all` / `cargo clippy --all-targets` の通過確認。
