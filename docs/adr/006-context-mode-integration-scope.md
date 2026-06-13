# ADR 006: MCP context-mode 統合範囲と実装ギャップの記録

- **ステータス**: `[ACCEPTED]`
- **決定日**: 2026-06-13
- **関連タスク**: Phase 52（Context 最適化）、`docs/specs/v0.4/91_context_upstream_comparison.md`

## 1. コンテキスト（直面した課題）

`docs/specs/v0.4/91_context_upstream_comparison.md`（2026-06-12 作成）にて 4 upstream
（GeminiClaw / gemini-cli / PicoClaw / Hermes Agent）との横断比較を実施し、
RustyClaw v0.4 への取り込み計画（高優先度 3 施策）を策定した。

2026-06-13 に実コードと計画書の照合（再点検）を実施し、以下の実装状況と
ギャップを確認した。

---

## 2. context-mode ツール登録状況

| ツール | 登録先 | 実装状況 |
|---|---|---|
| `ctx_execute` | Heartbeat 用 ToolRegistry | ✅ LLM が自律実行可能 |
| `ctx_search` | Heartbeat 用 ToolRegistry + `try_ctx_search()` | ⚠️ Heartbeat のみ（通常チャット未使用） |
| `ctx_index` | `try_ctx_index()` | ⚠️ cron セッション限定 |
| `ctx_patch` | Heartbeat 用 ToolRegistry | ✅ 登録済み（LLM 自律利用） |

---

## 3. 高優先度 3 施策の実装対照

### A. Heartbeat Digest 増分生成（参照: GeminiClaw）
**✅ 実装済み（計画と合致）**
- `run_count % 6 == 0` で deep scan 判定
- `heartbeat_last_run_ts`（SQLite）を参照した増分スキャン
- `is_modified_since_last` フィルタで未変更ファイルをスキップ

### B. ContextBuilder context window 対応（参照: gemini-cli / PicoClaw）
**⚠️ 部分実装**
- `truncate_70_20()` 関数は存在するが **ツールコンテンツのトランケートのみ**（`lib.rs:261`）
- 会話履歴に対する HEAD 70% + TAIL 20% の Sliding Window は未実装
- `get_history_message_limit()` がトークン予算式のメッセージ件数制限に置き換え済みだが、
  turn boundary 尊重・HEAD/TAIL 比率保護はない

### C. Session-level Summary → ctx_index（参照: GeminiClaw / Hermes Agent）
**⚠️ cron 限定・アイドルトリガーなし**
- `SUMMARIZE_CRON_SESSIONS` ホワイトリスト（8 種の cron セッション）完了後にのみ発行
- 通常ユーザーチャットのセッションは対象外
- 計画書の「アイドル 5 分後トリガー」は未実装。実装は「cron 完了後イベント発行」パターン

---

## 4. 検討した選択肢（Options）

### G1: 通常チャットでの ctx_search 未使用
- **案A: 毎リクエストで ctx_search を実行（不採用・現在）**
  - メリット: エピソード記憶を常に参照できる
  - デメリット: context-mode 未起動時の fail-open 遅延、Heartbeat 以外での追加 RTT
- **案B: Heartbeat のみ（現行・採用）**
  - メリット: リクエストレイテンシに影響しない
  - デメリット: 通常チャットでエピソード記憶が活用されない
- **案C: Phase 52-2 でスキル文脈に応じた動的 ctx_search（将来実装候補）**
  - ユーザー文脈を前処理で判定し、関連ありと判断された場合のみ ctx_search を実行

### G2: ctx_index の対象
- **案A: cron のみ（現行・採用）**
  - メリット: サマリー品質が高い（cron の構造化されたやりとりのみ対象）
  - デメリット: ユーザーチャットのエピソードが記憶されない
- **案B: ユーザーチャットも対象に拡張（Phase 52-2 候補）**
  - memory flush 完了後に session-summary を ctx_index する
  - デメリット: 日常会話の全セッションをサマライズするトークンコストが増加

### G3: アイドルトリガー（Session-level Summary の発火タイミング）
- **案A: cron 完了後（現行・採用）**
  - 実装が単純。Heartbeat との干渉なし
- **案B: アイドル 5 分後（計画書記載・未実装）**
  - 参照: GeminiClaw `summary.ts`
  - デメリット: タイマー管理と並行制御が必要。Phase 52 スコープ外として先送り

### G4: 会話履歴 HEAD/TAIL 保護
- **案A: メッセージ件数上限のみ（現行・採用）**
  - `get_history_message_limit()` のトークン予算式で制限
- **案B: HEAD 70% + TAIL 20% Sliding Window（計画書記載・未実装）**
  - 参照: GeminiClaw、PicoClaw
  - デメリット: turn boundary 検出の実装コストが高い。Phase 52-1 本実装で対応

---

## 5. 決定と選定理由（Decision & Justification）

現行実装（G1〜G4 すべて「現行」案）を **v0.4 の暫定状態として承認** する。
各ギャップについて以下の方針で v0.4 内で解消する。

| Gap | 対応 Phase | 方針 |
|---|---|---|
| G1（通常チャット ctx_search） | Phase 52-2 | ブリーフィング等の文脈判定後に動的 ctx_search |
| G2（ctx_index のユーザーチャット拡張） | Phase 52-2 | memory flush 完了後の session-summary を ctx_index |
| G3（アイドルトリガー） | Phase 52 以降で別途検討 | スコープ抜けとして Phase 52 計画書に追記 |
| G4（HEAD/TAIL 保護） | Phase 52-1 | 会話履歴クレンジングの一部として実装 |

---

## 6. トレードオフ・今後の影響（Consequences）

- **G3 のアイドルトリガー**: 「5 分後にサマリーを生成」するには、レスポンス送信後に
  Tokio タイマーを仕掛けて非同期にサマリー発行するループが必要。
  Heartbeat やその他の cron タスクとのリソース競合に注意が必要。
- **G2 の拡張**: ユーザーチャット全件サマライズはトークン消費が増加する。
  `MIN_INTERVAL_MINS`（memory flush の最小間隔）と同様のゲートを設けて過剰発行を抑制する。
- **G1 の動的 ctx_search**: context-mode の未起動（初回起動遅延・クラッシュ復旧中）には
  既存の `try_ctx_search` が fail-open（None 返却）で対応済み。
