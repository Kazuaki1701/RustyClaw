# Phase 43-B 設計書: Dashboard 統一 — `execute_with_tools` 削除と dead code 除去

**日付**: 2026-06-07
**案件番号**: Phase 43-B
**ステータス**: 設計承認済み

---

## 概要

`execute_with_tools` は Dashboard チャット向けに書かれたが、ゲートウェイは全ユーザーセッション（Dashboard 含む）を `execute_with_rig_agent` 経由で処理しており、`execute_with_tools` は本番で一度も呼ばれていない dead code である。これを削除し、関連する `dashboard_top_k` 設定フィールドも除去して Dashboard / Discord チャットのコードパスを完全に統一する。合わせて Dashboard セッションの usage trigger ラベルが `"unknown"` になっているバグを修正する。

---

## 背景

| 項目 | 状況 |
|---|---|
| `execute_with_tools` | `crates/rustyclaw-agent/src/lib.rs` に存在するが、ゲートウェイ・CLI のいずれからも呼ばれていない。テストコード内のみで参照。 |
| `dashboard_top_k` | `EmbeddingConfig` に定義、production config に `8` が設定されているが、実際のコードパスで参照されない。 |
| `heartbeat-digest.md` 注入 | `execute_with_tools` 内に実装されているが、同関数が dead code のため動作していない。Phase 41-1 で導入された機能が実質未稼働。 |
| Dashboard の実際のパス | `execute_with_rig_agent`（Discord と同一）。`discord_top_k` または グローバル `top_k` を使用。 |
| usage trigger ラベル | `discord-*` → `"discord"`、`http-dashboard-*` → `"unknown"`（バグ）。 |

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | `execute_with_tools` 関数を削除 |
| `crates/rustyclaw-agent/src/lib.rs` | `test_pipeline_execute_with_tools`・`test_execute_with_tools_rig_core_registry` テストを削除 |
| `crates/rustyclaw-config/src/lib.rs` | `EmbeddingConfig.dashboard_top_k` フィールドを削除 |
| `crates/rustyclaw-config/src/lib.rs` | `test_embedding_config_dashboard_top_k_*` テスト 2 件を削除 |
| `production/config/config.release.json` | `"dashboard_top_k": 8` を削除 |
| `crates/rustyclaw-gateway/src/lib.rs` | trigger ラベル: `"unknown"` → `"dashboard"` に修正（`http-dashboard` セッション判定を追加） |
| `docs/specs/03_workspace_spec.md` | `EmbeddingConfig` テーブルから `dashboard_top_k` 行を削除 |

---

## 詳細設計

### 1. `execute_with_tools` の削除

`crates/rustyclaw-agent/src/lib.rs` の `pub async fn execute_with_tools(...)` 関数全体を削除する。

削除後に参照が残らないことを確認:
- `dashboard_top_k` 参照（同関数内のみ）
- `heartbeat-digest.md` 注入コード（同関数内のみ）
- テスト 2 件（`test_pipeline_execute_with_tools`・`test_execute_with_tools_rig_core_registry`）

### 2. `dashboard_top_k` の削除

`EmbeddingConfig` から以下を削除:
```rust
// 削除対象
#[serde(default)]
pub dashboard_top_k: Option<usize>,
```

`production/config/config.release.json` から削除:
```json
// 削除対象
"dashboard_top_k": 8,
```

### 3. Dashboard usage trigger 修正

`crates/rustyclaw-gateway/src/lib.rs` の trigger 判定ブロック（2 箇所）を以下に変更:

変更前:
```rust
} else if session_id.starts_with("discord-") {
    "discord"
} else if session_id.starts_with("cli-") {
    "cli"
} else {
    "unknown"
};
```

変更後:
```rust
} else if session_id.starts_with("discord-") {
    "discord"
} else if session_id.starts_with("http-dashboard") {
    "dashboard"
} else if session_id.starts_with("cli-") {
    "cli"
} else {
    "unknown"
};
```

---

## エラーハンドリング

- `execute_with_tools` の削除後、コンパイルが通ることで参照漏れがないことを保証する
- `production/config/config.release.json` から `dashboard_top_k` を削除しても、`#[serde(default)]` によりキーが存在しない場合は `None` になるため、デプロイ順序に依存しない

---

## テスト方針

- `cargo build --all` / `cargo test --all` / `cargo clippy --all-targets` が通ること
- 削除後に `execute_with_tools`・`dashboard_top_k` を grep して参照が残っていないことを確認

---

## スコープ外

- `execute_with_tools` が担っていた `heartbeat-digest.md` 注入の復活（将来 RAG インデックス化で対応）
- `discord_top_k` → `channel_top_k` リネーム（Phase 43-C）
