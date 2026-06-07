# Phase 43-C 設計書: チャンネル統一 — `discord_top_k` → `channel_top_k` リネーム

**日付**: 2026-06-07
**案件番号**: Phase 43-C
**ステータス**: 設計承認済み

---

## 概要

`discord_top_k` という設定フィールド名は Discord 専用を示唆するが、Phase 43-B 以降は `execute_with_rig_agent` が Discord・Dashboard・将来の LINE の全チャンネルを処理する唯一のエントリポイントになる。フィールド名を `channel_top_k` に変更し、「全チャンネル共通の top_k 設定」であることを明示する。合わせて ISSUE-34（コードでは既解決）をクローズする。

---

## 背景

| 項目 | 状況 |
|---|---|
| `discord_top_k` | Phase 43-B 以降、Discord・Dashboard・LINE（Phase 39 予定）の全チャンネルに適用される設定となる |
| 命名の問題 | `discord_top_k` は Discord 専用を示唆し、LINE 追加時に混乱を招く |
| ISSUE-34 | `history_for_rag` の `unwrap_or_default()` 問題は後続コミット（9500471）で解決済み。現在のコードに `history_for_rag` 変数は存在しない |
| 後方互換 | `#[serde(default)]` により、旧 config に `discord_top_k` キーが残っていても `None`（グローバル `top_k` フォールバック）になるため安全 |

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-config/src/lib.rs` | `discord_top_k` → `channel_top_k` にリネーム |
| `crates/rustyclaw-config/src/lib.rs` | テスト `test_embedding_config_discord_top_k_*` 2 件をリネーム・更新 |
| `crates/rustyclaw-agent/src/lib.rs` | `execute_with_rig_agent` 内の `discord_top_k` 参照を `channel_top_k` に変更 |
| `production/config/config.release.json` | `"discord_top_k"` キーを `"channel_top_k"` に変更 |
| `docs/specs/03_workspace_spec.md` | `EmbeddingConfig` テーブルの `discord_top_k` 行を `channel_top_k` に更新（説明: LINE/Discord/Dashboard 共通） |
| `docs/task.md` | ISSUE-34 をクローズ（コードで既に解決済みのためアーカイブ） |

---

## 詳細設計

### 1. `EmbeddingConfig` フィールドのリネーム

`crates/rustyclaw-config/src/lib.rs`:

変更前:
```rust
#[serde(default)]
pub discord_top_k: Option<usize>,
```

変更後:
```rust
#[serde(default)]
pub channel_top_k: Option<usize>,
```

### 2. `execute_with_rig_agent` の参照更新

`crates/rustyclaw-agent/src/lib.rs` の `execute_with_rig_agent` 内:

変更前:
```rust
let discord_top_k = self
    .config
    .embedding
    .as_ref()
    .and_then(|e| e.discord_top_k)
    .unwrap_or(top_k);
```

変更後:
```rust
let channel_top_k = self
    .config
    .embedding
    .as_ref()
    .and_then(|e| e.channel_top_k)
    .unwrap_or(top_k);
```

以降の `discord_top_k` 変数参照（`retrieve_rag_context_local`・`retrieve_rag_context` への引数）も `channel_top_k` に変更する。

### 3. production config 更新

`production/config/config.release.json`:

変更前:
```json
"discord_top_k": <値>,
```

変更後:
```json
"channel_top_k": <値>,
```

### 4. ISSUE-34 クローズ

`docs/task.md` の ISSUE-34 エントリを `[x]` に変更し、アーカイブファイルに移動する。

---

## エラーハンドリング

- `config.release.json` の `discord_top_k` を `channel_top_k` に変更する前にデプロイした場合、古いキーは `#[serde(default)]` で無視されグローバル `top_k` にフォールバックする（fail-open）
- `channel_top_k` を設定してから古いバイナリで読んだ場合も同様に `None` になるだけで安全

---

## テスト方針

- `cargo build --all` / `cargo test --all` / `cargo clippy --all-targets` が通ること
- 削除後に `discord_top_k` を grep して参照が残っていないことを確認

---

## スコープ外

- `channel_top_k` のチャンネル別設定化（将来: `channel_top_k_discord`・`channel_top_k_line` など）
- `truncate_70_20` 関数の削除（全パス廃止確認後に別タスクで実施）
