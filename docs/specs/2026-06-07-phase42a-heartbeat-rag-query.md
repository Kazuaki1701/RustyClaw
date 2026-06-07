# Phase 42-A 設計書: Heartbeat RAG クエリ最適化

**日付**: 2026-06-07  
**案件番号**: Phase 42-A  
**ステータス**: 設計承認済み

---

## 概要

Heartbeat patrol の RAG 検索クエリを最適化する。現状は `heartbeat_prompt` 全体（preamble + digest、512字で打ち切り）を RAG クエリとして使用しているため、前文ボイラープレートが検索精度を下げている。

改善: digest の末尾 10 行 + 固定テンプレートプレフィックスで構成したコンパクトな「意味クエリ」に置き換える。

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | `build_heartbeat_rag_query` 関数追加 + `execute_heartbeat` シグネチャに `rag_query: Option<&str>` 追加 |
| `crates/rustyclaw-gateway/src/lib.rs` | `build_heartbeat_rag_query` 呼び出し、`execute_heartbeat` に `Some(&rag_query)` を渡す |

---

## 詳細設計

### 1. 定数と helper 関数（`rustyclaw-agent`）

```rust
const HEARTBEAT_RAG_TAIL_LINES: usize = 10;

pub fn build_heartbeat_rag_query(digest: &str) -> String {
    let lines: Vec<&str> = digest.lines().collect();
    let start = lines.len().saturating_sub(HEARTBEAT_RAG_TAIL_LINES);
    let tail = lines[start..].join("\n");
    format!("recent errors tasks memory updates: {}", tail)
}
```

- `HEARTBEAT_RAG_TAIL_LINES = 10` はこの定数を変えるだけで調整可能
- テンプレートプレフィックス `"recent errors tasks memory updates: "` でベクトル検索がエラー・タスク・記憶に関するチャンクに向くよう誘導する
- `digest` が空の場合: `tail = ""` → `"recent errors tasks memory updates: "` でフォールバック（RAG ヒットなしでも動作継続）

### 2. `execute_heartbeat` シグネチャ変更（`rustyclaw-agent`）

```rust
pub async fn execute_heartbeat(
    &self,
    workspace_dir: &Path,
    session_id: &str,
    user_message: &str,
    rag_query: Option<&str>,   // ← 追加
    tool_registry: &ToolRegistry,
    db_path: &Path,
) -> Result<LlmResponse>
```

関数内での使用:

```rust
let effective_rag = rag_query.unwrap_or(user_message);
// RAG 呼び出し時は effective_rag を使用
// LLM へのメッセージは user_message をそのまま使用（変更なし）
```

`rag_query: None` の場合は従来通り `user_message` で RAG を検索する（後方互換）。

### 3. 呼び出し側（`rustyclaw-gateway`）

```rust
// digest は既存の let Some((digest, ...)) バインドから利用可能
let rag_query = rustyclaw_agent::build_heartbeat_rag_query(&digest);

pipeline.execute_heartbeat(
    &workspace_path,
    &session_id,
    &heartbeat_prompt,
    Some(&rag_query),   // ← 追加
    &tool_registry,
    &db_path,
).await
```

---

## データフロー

```
Heartbeat cron trigger
  └─ heartbeat_svc.generate_digest(&db_path)
       └─ digest: String  (セッションログから生成)
  └─ [NEW] build_heartbeat_rag_query(&digest)
       └─ rag_query: "recent errors tasks memory updates: {last 10 lines}"
  └─ pipeline.execute_heartbeat(heartbeat_prompt, Some(&rag_query), ...)
       └─ effective_rag = rag_query  (← 以前は user_message 全体)
       └─ retrieve_rag_context*(effective_rag, ...)
       └─ system_context に注入 → LLM 呼び出し
```

---

## エラーハンドリング

- `digest` が空: `build_heartbeat_rag_query` はプレフィックスのみ返す → RAG がヒットしない場合は system_context に注入されず、既存挙動と同等
- `rag_query: None`（将来の呼び出し元向け後方互換）: `user_message` で検索するフォールバック

---

## テスト方針

`build_heartbeat_rag_query` の単体テスト:

- 空文字列 → プレフィックスのみ
- 10行未満 → 全行が含まれる
- ちょうど 10行 → 全行が含まれる
- 11行以上 → 最新 10行のみ（最古行は除外）
- テンプレートプレフィックス `"recent errors tasks memory updates: "` が先頭にある

`execute_heartbeat` 変更の影響:

- 既存の呼び出しテストは `rag_query` に `None` を渡すことで動作継続
- シグネチャ変更に伴いコンパイルエラーが出る箇所をすべて修正する
