> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去のタスクリスト - 開発完了済み)  
> **完了日**: 2026-06-09  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

---

# アーカイブ: 2026-06-09 完了バグ修正 (calendar args / Dashboard 応答タイミング)

## 完了したタスク

### 1. calendar-ops.sh を引数なしで実行している

- **完了日**: 2026-06-09
- **発見日時**: 2026-06-09 08:02:42 (rp1 ログ)
- **症状**: Dashboard Chat で「今日の予定は？」と問いかけた際、LLM が `run_workspace_script` の args に `list_family` を渡さず引数なしで実行。エラー出力: `Usage: calendar-ops.sh {list_family|list_ai_agent|...} ...` (stderr)
- **原因**: MEMORY.md のカレンダースキル定義に `args` の記載がなく、LLM が引数不要と判断。
- **修正**: `production/workspace/MEMORY.md` L30 に `args: ["list_family"]` を明示的に追記。
  ```
  → run_workspace_script: "skills/calendar/scripts/calendar-ops.sh", args: ["list_family"]
  ```
- **関連ファイル**: `production/workspace/MEMORY.md`

---

### 2. Dashboard Chat が LLM 応答待ち中に新規メッセージを受信すると無応答になる

- **完了日**: 2026-06-09
- **発見日時**: 2026-06-09 08:03:33〜08:03:34 (rp1 ログ)
- **再現手順**: メッセージ A を送信 → LLM 処理中にメッセージ B を送信 → A の応答が返るも B の session リスナーが見ていないため無視 → 5分後に `Error: Request timeout`
- **症状**: `AgentResponse` は正常生成されているにもかかわらず、`POST /chat` の `rx.recv()` ループが新規コネクションの subscriber になっているため前の応答を受け取れない
- **原因**: `http-dashboard-{today}` の session_id 共有 + broadcast のタイミング問題。B の `rx.subscribe()` 前に A の応答が流れてしまっている。
- **修正**: `crates/rustyclaw-gateway/src/health.rs` にて、リクエストごとにタイムスタンプ＋乱数のユニークな `session_id` を生成し、`rx.subscribe()` を `publish()` より先に実行する順序を保証。さらに `session_id` 一致チェックで他セッションの応答を除外。
  ```rust
  let session_id = format!("http-dashboard-{}-{}", timestamp, random_suffix);
  let mut rx = bus_clone.subscribe();  // subscribe を先に実行
  bus_clone.publish(event)             // その後 publish
  ```
- **関連ファイル**: `crates/rustyclaw-gateway/src/health.rs` L460-L502
