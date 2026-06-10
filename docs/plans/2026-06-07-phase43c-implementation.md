# Phase 43-C channel_top_k 統一 Implementation Plan

> **ステータス**: `[DONE]` — 実装完了・main にマージ済み（2026-06-07、コミット 4b842ed）

**Goal:** `discord_top_k` という Discord 専用名称を `channel_top_k` にリネームし、LINE / Discord / Dashboard 全チャンネル共通の設定であることを明示する。合わせて ISSUE-34（コードで既解決）をクローズする。

**Architecture:** リネームは config → agent の順に実施する。config で `discord_top_k` → `channel_top_k` にリネームすると agent が一時的にコンパイル不可になるため、Task 1 の検証は `rustyclaw-config` 単体のみとし、Task 2 で agent を修正後に全体ビルドを通す。

**Tech Stack:** Rust 2024 Edition、`crates/rustyclaw-config`、`crates/rustyclaw-agent`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-config/src/lib.rs` | リネーム | `discord_top_k` フィールド・コメント・2 テスト → `channel_top_k` |
| `crates/rustyclaw-agent/src/lib.rs` | リネーム | ローカル変数・コメント・フィールドアクセス → `channel_top_k` |
| `docs/specs/03_workspace_spec.md` | 更新 | `discord_top_k` 行 → `channel_top_k`（説明も更新） |
| `docs/task.md` | 更新 | ISSUE-34 を `[x]` にクローズ（コードで既解決） |

**注意**: `production/config/config.release.json` には `discord_top_k` キーが存在しない（グローバル `top_k: 5` フォールバックを使用中）。config JSON の変更は不要。

---

### Task 1: `discord_top_k` → `channel_top_k` リネーム（config crate）

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs:130-132, 1026-1036`

- [ ] **Step 1: ベースラインを確認**

```bash
cargo test -p rustyclaw-config 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`

- [ ] **Step 2: テストを `channel_top_k` に更新（先にテストを変更して失敗させる）**

`crates/rustyclaw-config/src/lib.rs` の以下ブロック（l.1025–1036）を:

```rust
    #[test]
    fn test_embedding_config_discord_top_k_default() {
        let cfg: EmbeddingConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(cfg.discord_top_k.is_none(), "discord_top_k default should be None");
    }

    #[test]
    fn test_embedding_config_discord_top_k_value() {
        let cfg: EmbeddingConfig =
            serde_json::from_str(r#"{"discord_top_k": 3}"#).unwrap();
        assert_eq!(cfg.discord_top_k, Some(3));
    }
```

以下に変更する:

```rust
    #[test]
    fn test_embedding_config_channel_top_k_default() {
        let cfg: EmbeddingConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(cfg.channel_top_k.is_none(), "channel_top_k default should be None");
    }

    #[test]
    fn test_embedding_config_channel_top_k_value() {
        let cfg: EmbeddingConfig =
            serde_json::from_str(r#"{"channel_top_k": 3}"#).unwrap();
        assert_eq!(cfg.channel_top_k, Some(3));
    }
```

- [ ] **Step 3: テストが失敗する（コンパイルエラー）ことを確認**

```bash
cargo build -p rustyclaw-config 2>&1 | grep "^error"
```
Expected: `error[E0609]: no field 'channel_top_k'` または `discord_top_k` への参照エラー（フィールドがまだ旧名のため）

- [ ] **Step 4: フィールド定義をリネーム（l.130–132）**

`crates/rustyclaw-config/src/lib.rs` の以下を:

```rust
    /// Discord チャット専用の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub discord_top_k: Option<usize>,
```

以下に変更する:

```rust
    /// LINE / Discord / Dashboard チャット共通の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub channel_top_k: Option<usize>,
```

- [ ] **Step 5: `rustyclaw-config` 単体のビルドとテストで確認**

```bash
cargo build -p rustyclaw-config 2>&1 | grep "^error"
```
Expected: 出力なし

```bash
cargo test -p rustyclaw-config 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`（件数変化なし、テスト名だけ変わる）

注意: この時点で `cargo build -p rustyclaw-agent` は失敗する（`e.discord_top_k` 参照が残っているため）。それで正常。

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "refactor(config): Phase 43-C discord_top_k → channel_top_k リネーム"
```

---

### Task 2: `discord_top_k` → `channel_top_k` リネーム（agent crate）

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs:1262-1294`

Task 1 完了後に実施する。

- [ ] **Step 1: コメントとローカル変数を更新（l.1262–1269）**

`crates/rustyclaw-agent/src/lib.rs` の以下ブロックを:

```rust
        // discord_top_k 優先、未設定時はグローバル top_k にフォールバック
        let top_k = self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
        let discord_top_k = self
            .config
            .embedding
            .as_ref()
            .and_then(|e| e.discord_top_k)
            .unwrap_or(top_k);
```

以下に変更する:

```rust
        // channel_top_k 優先、未設定時はグローバル top_k にフォールバック
        let top_k = self.config.embedding.as_ref().map(|e| e.top_k).unwrap_or(5);
        let channel_top_k = self
            .config
            .embedding
            .as_ref()
            .and_then(|e| e.channel_top_k)
            .unwrap_or(top_k);
```

- [ ] **Step 2: `retrieve_rag_context_local` 呼び出しを更新（l.1288）**

以下を:

```rust
                    retrieve_rag_context_local(&rag_query, &self.config, &client, &db_path, discord_top_k).await;
```

以下に変更する:

```rust
                    retrieve_rag_context_local(&rag_query, &self.config, &client, &db_path, channel_top_k).await;
```

- [ ] **Step 3: `retrieve_rag_context` 呼び出しを更新（l.1294）**

以下を:

```rust
            let rag_ctx = retrieve_rag_context(&rag_query, &self.config, rag, discord_top_k).await;
```

以下に変更する:

```rust
            let rag_ctx = retrieve_rag_context(&rag_query, &self.config, rag, channel_top_k).await;
```

- [ ] **Step 4: 全体ビルドとテストで確認**

```bash
cargo build --all 2>&1 | grep "^error"
```
Expected: 出力なし

```bash
cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: すべての crate で `test result: ok. N passed; 0 failed;`

参照漏れがないことを確認:
```bash
grep -r "discord_top_k" /mnt/Projects/RustyClaw/crates/ --include="*.rs"
```
Expected: 出力なし

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "refactor(agent): Phase 43-C discord_top_k → channel_top_k リネーム"
```

---

### Task 3: 仕様書更新・ISSUE-34 クローズ・最終検証

**Files:**
- Modify: `docs/specs/03_workspace_spec.md:198`
- Modify: `docs/task.md`

- [ ] **Step 1: `03_workspace_spec.md` の `discord_top_k` 行を更新（l.198）**

`docs/specs/03_workspace_spec.md` の以下の行を:

```markdown
| `discord_top_k` | `Option<usize>` | `None` | Discord RAG の top-k 件数 |
```

以下に変更する:

```markdown
| `channel_top_k` | `Option<usize>` | `None` | LINE / Discord / Dashboard チャット共通の RAG top-k 件数 |
```

- [ ] **Step 2: ISSUE-34 をクローズ（`docs/task.md`）**

`docs/task.md` の以下の行を:

```markdown
- `[ ]` **ISSUE-34: Discord RAG `history_for_rag` のエラーハンドリングを `history_messages` と統一**
```

以下に変更する:

```markdown
- `[x]` **ISSUE-34: Discord RAG `history_for_rag` のエラーハンドリングを `history_messages` と統一**
```

理由: ISSUE-34 が報告した `history_for_rag` 変数は後続コミット（9500471）で既に除去されており、現在のコードに `history_for_rag` は存在しない。コードレベルでは解決済み。

- [ ] **Step 3: production config に `discord_top_k` がないことを確認（no-op 検証）**

```bash
grep "discord_top_k" /mnt/Projects/RustyClaw/production/config/config.release.json
```
Expected: 出力なし（`discord_top_k` キーは元々設定されていなかった）

- [ ] **Step 4: codebase 全体の `discord_top_k` 残存参照を確認**

```bash
grep -r "discord_top_k" /mnt/Projects/RustyClaw/crates/ /mnt/Projects/RustyClaw/production/ /mnt/Projects/RustyClaw/docs/specs/ --include="*.rs" --include="*.json" --include="*.md"
```
Expected: 出力なし

- [ ] **Step 5: 最終ビルド・テスト・clippy**

```bash
cargo build --all 2>&1 | grep "^error"
```
Expected: 出力なし

```bash
cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: すべての crate で `test result: ok. N passed; 0 failed;`

```bash
cargo clippy --all-targets 2>&1 | grep "^error"
```
Expected: 出力なし

- [ ] **Step 6: コミット**

```bash
git add docs/specs/03_workspace_spec.md docs/task.md
git commit -m "docs(specs): Phase 43-C channel_top_k 仕様書更新・ISSUE-34 クローズ"
```
