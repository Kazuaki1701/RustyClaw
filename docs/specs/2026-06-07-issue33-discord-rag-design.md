# ISSUE-33 設計書: Discord チャット向け RAG 改善

**日付**: 2026-06-07  
**案件番号**: ISSUE-33  
**ステータス**: 設計承認済み

---

## 概要

Discord チャットでの RAG 精度を2点改善する。

1. **案A — クエリ拡張**: RAG 検索クエリを現在のユーザーメッセージ単体から、直近 N 会話ターン + 現在メッセージに拡張する
2. **案B — `discord_top_k` 設定**: `EmbeddingConfig` に Discord 専用の `top_k` オーバーライドを追加する（`heartbeat_top_k` と同パターン）

両改善は独立しているが相補的なため、1PR で同時実装する。

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|----------|----------|
| `crates/rustyclaw-config/src/lib.rs` | `EmbeddingConfig` に `discord_top_k: Option<usize>` を追加 |
| `crates/rustyclaw-agent/src/lib.rs` | `execute_with_rig_agent` のクエリ構築ロジックを拡張 |

---

## 詳細設計

### 1. `EmbeddingConfig` への `discord_top_k` 追加

```rust
/// Discord チャット専用の RAG 検索上限件数（省略時は top_k を使用）
#[serde(default)]
pub discord_top_k: Option<usize>,
```

- デフォルト: `None`（未設定時は `top_k=5` にフォールバック）
- `heartbeat_top_k`（デフォルト 2）・`dashboard_top_k` と同じ宣言パターン

### 2. `execute_with_rig_agent` のクエリ拡張

#### 定数

```rust
const DISCORD_RAG_HISTORY_TURNS: usize = 2;
```

2〜3ターンの調整はこの定数を変えるだけで対応可能。

#### クエリ組み立てロジック

```
history を最新 N ターン分ロード（cron: セッションは空）
フォーマット:
  User: <turn[0].user>
  Assistant: <turn[0].assistant>
  ...
  User: <raw_user_message>   ← 末尾に現在メッセージを固定
```

- `tool` ロールおよび `tool_calls` のみのメッセージは除外する
- 空の assistant メッセージも除外する
- `cron:` セッション（履歴なし）は従来通り `raw_user_message` のみ使用

#### top_k の決定ロジック

```rust
let discord_top_k = self.config.embedding.as_ref()
    .and_then(|e| e.discord_top_k)
    .unwrap_or(top_k);  // top_k = e.top_k (デフォルト 5)
```

RAG 呼び出し時に `top_k` の代わりに `discord_top_k` を渡す。

---

## データフロー

```
Discord メッセージ受信
  └─ execute_with_rig_agent(raw_user_message, injected_user_message, ...)
       ├─ [NEW] load_history(session_id) → 直近 N ターン取得
       ├─ [NEW] build_rag_query(history_turns, raw_user_message)
       │         → "User: ...\nAssistant: ...\nUser: <current>"
       ├─ [NEW] discord_top_k = discord_top_k.unwrap_or(top_k)
       ├─ retrieve_rag_context*(rag_query, config, client, db, discord_top_k)
       └─ system_context に注入 → LLM 呼び出し
```

---

## エラーハンドリング

- `load_history` が失敗した場合はクエリ拡張をスキップし、`raw_user_message` のみで検索する（既存挙動に fallback）
- 会話履歴が N ターン未満の場合は取得できた分だけ使用する

---

## テスト方針

- `EmbeddingConfig` の `discord_top_k` デシリアライズテスト（省略時のフォールバック確認）
- クエリ組み立て関数の単体テスト（ヘルパー関数として抽出する場合）
- 既存 `execute_with_rig_agent` の呼び出しインターフェースは変更なし（ゲートウェイ側の変更不要）
