# ISSUE-26: Heartbeat Loop Crash Fix Implementation Plan

> **ステータス**: `[DONE]`

**Goal:** 30分ごとの Heartbeat patrol 実行時に複数のツール（例: カレンダー取得と Gmail 取得）が順次実行された際、対話履歴が途中でトリミングされてエージェントが過去の実行結果を忘れてしまい、無限にツール呼び出しを繰り返して5ステップ上限でクラッシュするバグ（ISSUE-26）を修正する。

**Architecture / Approach:**
`trim_heartbeat_messages` を完全に廃止する。
* **廃止の理由と選定の背景**:
  * 元々 `trim_heartbeat_messages` は、長いループによるトークン超過を防ぐために導入された。
  * しかし、同一セッション（最大 5 ステップの `max_loops` 制限）内で過去の tool 応答を破棄すると、エージェントが「すでにカレンダーやメールを確認した」という前提事実を忘れ、再度同じツールを呼び出す無限ループを誘発する。
  * 現在は `truncate_70_20` によって個々のツール応答が 3,000 bytes（約 750 トークン程度）以下に制限されており、さらに RAG 注入トークン数も `heartbeat_top_k = 2` に引き下げて最適化されている。
  * 最大 5 ループを実行して全てのツール呼び出し履歴（最大 11 メッセージ）を保持した場合でも、総トークン数はモデルの制限（Groq 6,000 トークン等）を十分下回る。
  * よって、履歴の破棄（トリミング）は行わずに最後まで全対話コンテキストをモデルに入力することが、安全性・確実性の両面から最適である。

**Tech Stack:** Rust 2024 / tokio / `crates/rustyclaw-agent` / `docs/specs/04_heartbeat_spec.md`

---

## 実装手順とチェックリスト

- [x] **Step 1: テストコードの作成 (TDD)**
  * `trim_heartbeat_messages` が廃止された後に、メッセージ履歴が一切トリミングされないことを検証するテスト `test_heartbeat_messages_are_not_trimmed` を `crates/rustyclaw-agent/src/lib.rs` に追加する。
  * 既存の `test_trim_heartbeat_messages_*` の 5 つのテストは削除またはコメントアウト対象にする（機能が不要になるため）。

- [x] **Step 2: `crates/rustyclaw-agent/src/lib.rs` の修正**
  * `execute_heartbeat` 内の `trim_heartbeat_messages(&mut messages);` 呼び出し（line ~849 付近）を削除する。
  * `trim_heartbeat_messages` 関数定義本体（line ~2477 付近）を削除する。

- [x] **Step 3: テストと Clippy の実行**
  * ローカル環境でテストおよび静的解析を実行し、エラーや警告が 0 件であることを確認する。
    ```bash
    TZ=UTC cargo test --all-features --workspace
    cargo clippy --all-targets --all-features -- -D warnings
    ```

- [x] **Step 4: ドキュメント（仕様書）の更新**
  * [docs/specs/04_heartbeat_spec.md](file:///home/kazuaki/Projects/RustyClaw/docs/specs/04_heartbeat_spec.md) の「### ② 世代ローテーション（`trim_heartbeat_messages`）」に関連する記述を削除または廃止の旨に更新する。
  * `docs/specs/04_heartbeat_spec.md` の最終更新日を本日の日付にする。

- [x] **Step 5: `docs/task.md` の更新**
  * [docs/task.md](file:///home/kazuaki/Projects/RustyClaw/docs/task.md) 内の `ISSUE-26` を完了 (`[x]`) に更新する。

---

## トレードオフと今後の影響
* **トレードオフ**: `trim_heartbeat_messages` の廃止により、ループ終盤 of API リクエストサイズが数百〜数千トークン程度増加する。
* **対策**: `truncate_70_20` を引き続き適用しているため、巨大なツール出力によるコンテキスト急増は完全に抑制されており、安全レベルを逸脱するリスクは極めて低い。
