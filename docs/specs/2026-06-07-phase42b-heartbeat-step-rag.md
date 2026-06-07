# Phase 42-B 設計書: Heartbeat オンデマンド・ステップ別 RAG

**日付**: 2026-06-07  
**案件番号**: Phase 42-B  
**ステータス**: 設計承認済み

---

## 概要

Heartbeat patrol の RAG 注入を、現状の単一クエリ（Phase 42-A 実装済み）から **Step 別の複数クエリ** に拡張する。Step 2（記憶・興味整理）と Step 6（自発作業）に専用クエリを追加し、各ステップが必要とする知識を的確に検索注入する。

---

## 背景

Phase 42-A で `execute_heartbeat` 起動時に digest 末尾 10 行 + 固定テンプレートによる単一 RAG 注入を実装した。この注入は主に Step 1（活動把握）に対応しており、Step 2・Step 6 は依然として汎用コンテキストのみに頼っている。

**RAG 追加対象の選定理由:**

| Step | 内容 | RAG 付加価値 |
|---|---|---|
| Step 1 | 活動把握 | Phase 42-A で対応済み — 追加不要 |
| Step 2 | MEMORY.md・USER.md 整理 | 高: 長期記憶の関連コンテキストが必要 |
| Step 3 | カレンダー・メールチェック | 低: gws ツールが実データを取得 |
| Step 4 | 天気予報 | 不要: ツール依存 |
| Step 5 | 声掛け判定 | 低: 時刻・経過時間ベース |
| Step 6 | 自発作業 | 高: タスク・エラー・改善候補の検索 |
| Step 7 | 評価・HEARTBEAT_OK | 不要: 結果まとめのみ |

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 定数2件追加 + `execute_heartbeat` の RAG ブロックを3クエリに拡張 |

ゲートウェイ側（`crates/rustyclaw-gateway/src/lib.rs`）は変更なし。

---

## 詳細設計

### 1. クエリ定数

```rust
const HEARTBEAT_STEP2_RAG_QUERY: &str =
    "user interests hobbies routine habits long-term memory";
const HEARTBEAT_STEP6_RAG_QUERY: &str =
    "errors bugs pending tasks todo improvements";
```

`HEARTBEAT_RAG_TAIL_LINES` / `DISCORD_RAG_HISTORY_TURNS` と同じパターンで、定数を変えるだけで調整可能。

### 2. `execute_heartbeat` の RAG ブロック変更

ローカル embed クライアントを1回だけ生成して3クエリに再利用する。

**変更前（Phase 42-A 時点）:**
```
if use_local {
    client = make_embed_client()
    rag_ctx_1 = retrieve_local(effective_rag)  // 1クエリのみ
    system_context += rag_ctx_1
} else if rag {
    rag_ctx_1 = retrieve_remote(effective_rag)
    system_context += rag_ctx_1
}
```

**変更後（Phase 42-B）:**
```
if use_local {
    client = make_embed_client()  // 1回生成
    rag_ctx_1 = retrieve_local(effective_rag)
    rag_ctx_2 = retrieve_local(HEARTBEAT_STEP2_RAG_QUERY)
    rag_ctx_6 = retrieve_local(HEARTBEAT_STEP6_RAG_QUERY)
    system_context += rag_ctx_1
    if !rag_ctx_2.is_empty() { system_context += "## Step 2 関連記憶\n" + rag_ctx_2 }
    if !rag_ctx_6.is_empty() { system_context += "## Step 6 関連記憶\n" + rag_ctx_6 }
} else if rag {
    rag_ctx_1 = retrieve_remote(effective_rag)
    rag_ctx_2 = retrieve_remote(HEARTBEAT_STEP2_RAG_QUERY)
    rag_ctx_6 = retrieve_remote(HEARTBEAT_STEP6_RAG_QUERY)
    system_context += rag_ctx_1
    if !rag_ctx_2.is_empty() { system_context += "## Step 2 関連記憶\n" + rag_ctx_2 }
    if !rag_ctx_6.is_empty() { system_context += "## Step 6 関連記憶\n" + rag_ctx_6 }
}
```

`top_k` は既存の `hb_top_k`（デフォルト 2）を全クエリで共用。

### 3. system_context の構造（変更後）

```
# SOUL.md

<SOUL.md の内容>

# HEARTBEAT.md

<HEARTBEAT.md の内容>

<42-A の RAG コンテキスト（Step 1 活動把握）>
## Step 2 関連記憶
<MEMORY.md / USER.md 関連チャンク>
## Step 6 関連記憶
<タスク・エラー・改善候補チャンク>
[now: 2026-06-07T...]
```

---

## エラーハンドリング

- RAG クエリのいずれかが空結果の場合: `is_empty()` チェックで注入をスキップ（既存挙動と同等）
- ローカル embed クライアント生成失敗: 既存の `if let Some(client)` パターンで全 RAG をスキップ
- リモート RAG 未設定: 既存の `else if let Some(ref rag)` パターンでスキップ

---

## テスト方針

- 定数の値確認: `HEARTBEAT_STEP2_RAG_QUERY` と `HEARTBEAT_STEP6_RAG_QUERY` が期待する文字列であることをアサート
- `execute_heartbeat` シグネチャは変更なし（既存コンパイル時テスト継続）
- `cargo build --all` / `cargo test --all` / `cargo clippy --all-targets` 通過確認
