# Phase 43-B Dashboard 統一 Implementation Plan

> **ステータス**: `[DONE]` — 実装完了・main にマージ済み（2026-06-07、コミット 8803d03）

**Goal:** `execute_with_tools`（dead code）の削除・`dashboard_top_k` 設定フィールドの除去・Dashboard セッションの usage trigger ラベルバグ修正を行い、Dashboard / Discord チャットのコードパスを完全統一する。

**Architecture:** 変更は 4 ファイルに及ぶが互いに疎結合。`execute_with_tools` 内に `dashboard_top_k` への参照があるため、agent の dead code を先に削除してから config フィールドを除去する順序が必要。gateway と production config は独立して実施可能。

**Tech Stack:** Rust 2024 Edition、`crates/rustyclaw-config`、`crates/rustyclaw-agent`、`crates/rustyclaw-gateway`

---

## ファイルマップ

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | 削除 | `execute_with_tools` 関数（l.1225–1445）と 2 テスト |
| `crates/rustyclaw-config/src/lib.rs` | 削除 | `dashboard_top_k` フィールドと 2 テスト |
| `crates/rustyclaw-gateway/src/lib.rs` | 修正 | trigger 判定 2 箇所に `http-dashboard` → `"dashboard"` を追加 |
| `production/config/config.release.json` | 削除 | `"dashboard_top_k": 8` キー |

**削除順序の注意**: `execute_with_tools` が `dashboard_top_k` を参照しているため、Task 1（agent）→ Task 2（config）の順に実施する。Task 1 では関数とテストをまとめて削除してから初めてビルドすること（途中状態はコンパイルエラーになる）。

---

### Task 1: `execute_with_tools` 関数と関連テストを削除

**Files:**
- Modify: `crates/rustyclaw-agent/src/lib.rs`

- [ ] **Step 1: ベースラインを確認**

```bash
cargo test -p rustyclaw-agent 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed; ...`（N は現在の件数）

- [ ] **Step 2: `execute_with_tools` 関数本体を削除（l.1225–1445）**

`crates/rustyclaw-agent/src/lib.rs` の以下ブロックを丸ごと削除する。削除範囲は doc comment から関数末尾 `}` まで（空行 l.1446 も含めて削除）:

```rust
    /// 対話実行（ツール対応のマルチターンアジェンティックループ）
    pub async fn execute_with_tools(
        &self,
        workspace_dir: &Path,
        session_id: &str,
        user_message: &str,
        tool_registry: &ToolRegistry,
        purpose: &str,
        progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
    ) -> Result<LlmResponse> {
```
から始まる関数全体。末尾は:
```rust
            return Ok(response);
        }
    }
```
で終わる（次の `/// rig::agent::Agent を使ったツール実行。` というコメントの直前まで）。

- [ ] **Step 3: `test_pipeline_execute_with_tools` テストを削除（l.3085–3214）**

以下のブロックを削除する:

```rust
    #[tokio::test]
    async fn test_pipeline_execute_with_tools() -> Result<()> {
        let _guard = ENV_MUTEX.lock().unwrap();
```
から始まり:
```rust
        let _ = server_task.await;
        Ok(())
    }
```
で終わるテスト全体（次の `#[test]` の直前まで）。

- [ ] **Step 4: `test_execute_with_tools_rig_core_registry` テストを削除（l.3616–3643）**

以下のブロックを削除する:

```rust
    #[tokio::test]
    async fn test_execute_with_tools_rig_core_registry() {
        use rustyclaw_tools::{ToolCallError, ToolRegistry};
        use std::sync::Arc;
```
から始まり:
```rust
        assert_eq!(defs[0].name, "echo");
    }
```
で終わるテスト全体（次の `// ── Task 1: execute_heartbeat db_path シグネチャ ──` の直前まで）。

- [ ] **Step 5: ビルドとテストで確認**

```bash
cargo build -p rustyclaw-agent 2>&1 | grep -E "^error"
```
Expected: 出力なし（エラーゼロ）

```bash
cargo test -p rustyclaw-agent 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`（件数は 2 減少）

- [ ] **Step 6: コミット**

```bash
git add crates/rustyclaw-agent/src/lib.rs
git commit -m "refactor(agent): Phase 43-B execute_with_tools dead code を削除"
```

---

### Task 2: `dashboard_top_k` フィールドと関連テストを削除

**Files:**
- Modify: `crates/rustyclaw-config/src/lib.rs`

Task 1 完了後に実施すること（Task 1 で agent 側の参照を消してから config フィールドを削除する）。

- [ ] **Step 1: `dashboard_top_k` フィールドを削除（l.130–132）**

`crates/rustyclaw-config/src/lib.rs` の以下 3 行を削除する:

```rust
    /// ダッシュボードチャット専用の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub dashboard_top_k: Option<usize>,
```

削除後、`discord_top_k` フィールドの直前が `heartbeat_top_k` のブロックになる:
```rust
    #[serde(default)]
    pub heartbeat_top_k: Option<usize>,
    /// Discord チャット専用の RAG 検索上限件数（省略時は top_k を使用）
    #[serde(default)]
    pub discord_top_k: Option<usize>,
```

- [ ] **Step 2: `test_embedding_config_dashboard_top_k_*` テスト 2 件を削除（l.1029–1039）**

以下のブロックを削除する:

```rust
    #[test]
    fn test_embedding_config_dashboard_top_k_default() {
        let cfg: EmbeddingConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(cfg.dashboard_top_k.is_none(), "dashboard_top_k default should be None");
    }

    #[test]
    fn test_embedding_config_dashboard_top_k_value() {
        let cfg: EmbeddingConfig =
            serde_json::from_str(r#"{"dashboard_top_k": 8}"#).unwrap();
        assert_eq!(cfg.dashboard_top_k, Some(8));
    }
```

- [ ] **Step 3: ビルドとテストで確認**

```bash
cargo build -p rustyclaw-config 2>&1 | grep -E "^error"
```
Expected: 出力なし

```bash
cargo test -p rustyclaw-config 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`（件数は 2 減少）

- [ ] **Step 4: コミット**

```bash
git add crates/rustyclaw-config/src/lib.rs
git commit -m "refactor(config): Phase 43-B dashboard_top_k フィールドとテストを削除"
```

---

### Task 3: Gateway trigger ラベル修正

**Files:**
- Modify: `crates/rustyclaw-gateway/src/lib.rs`

`dispatch` 関数内に trigger 判定ブロックが 2 箇所ある（l.485–496 と l.776–787）。両方に `http-dashboard` → `"dashboard"` の分岐を追加する。

- [ ] **Step 1: trigger 判定ブロック 1 箇所目を修正（l.485–496）**

`crates/rustyclaw-gateway/src/lib.rs` の以下を（`DbManager::new` 直後のブロック、1 箇所目）:

```rust
                                            if let Ok(db) =
                                                rustyclaw_storage::DbManager::new(&db_path)
                                            {
                                                let trigger =
                                                    if session_id.starts_with("cron:heartbeat") {
                                                        "heartbeat"
                                                    } else if session_id.starts_with("cron:") {
                                                        "cron"
                                                    } else if session_id.starts_with("discord-") {
                                                        "discord"
                                                    } else if session_id.starts_with("cli-") {
                                                        "cli"
                                                    } else {
                                                        "unknown"
                                                    };
```

以下に変更する（`http-dashboard` ブランチを追加）:

```rust
                                            if let Ok(db) =
                                                rustyclaw_storage::DbManager::new(&db_path)
                                            {
                                                let trigger =
                                                    if session_id.starts_with("cron:heartbeat") {
                                                        "heartbeat"
                                                    } else if session_id.starts_with("cron:") {
                                                        "cron"
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

- [ ] **Step 2: trigger 判定ブロック 2 箇所目を修正（l.776–787）**

同ファイルの 2 つ目のブロック（`atomic_write` の直後、空行 + `let trigger =` で始まる箇所）:

変更前:
```rust
                                                }

                                                let trigger =
                                                    if session_id.starts_with("cron:heartbeat") {
                                                        "heartbeat"
                                                    } else if session_id.starts_with("cron:") {
                                                        "cron"
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
                                                }

                                                let trigger =
                                                    if session_id.starts_with("cron:heartbeat") {
                                                        "heartbeat"
                                                    } else if session_id.starts_with("cron:") {
                                                        "cron"
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

- [ ] **Step 3: 2 箇所修正されていることを確認**

```bash
grep -c '"dashboard"' crates/rustyclaw-gateway/src/lib.rs
```
Expected: `2`（heartbeat/cron/discord/dashboard/cli の "dashboard" が 2 箇所存在）

```bash
grep -n 'http-dashboard' crates/rustyclaw-gateway/src/lib.rs
```
Expected: 2 行（それぞれ `"dashboard"` を返す箇所）

- [ ] **Step 4: ビルドとテストで確認**

```bash
cargo build -p rustyclaw-gateway 2>&1 | grep -E "^error"
```
Expected: 出力なし

```bash
cargo test -p rustyclaw-gateway 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: `test result: ok. N passed; 0 failed;`

- [ ] **Step 5: コミット**

```bash
git add crates/rustyclaw-gateway/src/lib.rs
git commit -m "fix(gateway): Phase 43-B http-dashboard セッションの trigger ラベルを dashboard に修正"
```

---

### Task 4: Production config 更新と最終検証

**Files:**
- Modify: `production/config/config.release.json`

- [ ] **Step 1: `dashboard_top_k` を production config から削除（l.292）**

`production/config/config.release.json` の以下の行を削除する:

```json
    "dashboard_top_k": 8,
```

削除後の `embedding` セクションは:
```json
    "top_k": 5,
    "heartbeat_top_k": 2,
    "similarity_threshold": 0.60,
```
のように続く。

- [ ] **Step 2: JSON の整合性確認**

```bash
python3 -m json.tool production/config/config.release.json > /dev/null && echo "JSON valid"
```
Expected: `JSON valid`

- [ ] **Step 3: 参照漏れが残っていないことを確認**

```bash
grep -r "execute_with_tools\|dashboard_top_k" crates/ production/ --include="*.rs" --include="*.json"
```
Expected: 出力なし

- [ ] **Step 4: `docs/specs/03_workspace_spec.md` の確認（no-op 検証）**

```bash
grep "dashboard_top_k" docs/specs/03_workspace_spec.md
```
Expected: 出力なし（`dashboard_top_k` 行は既に存在しない）

- [ ] **Step 5: 全体ビルドとテストで最終確認**

```bash
cargo build --all 2>&1 | grep -E "^error"
```
Expected: 出力なし

```bash
cargo test --all 2>&1 | grep -E "^(test result|FAILED)"
```
Expected: すべての crate で `test result: ok. N passed; 0 failed;`

```bash
cargo clippy --all-targets 2>&1 | grep -E "^error"
```
Expected: 出力なし

- [ ] **Step 6: コミット**

```bash
git add production/config/config.release.json
git commit -m "chore(config): Phase 43-B production config から dashboard_top_k を削除"
```
