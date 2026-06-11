# RustyClaw — メモリ管理 & 会話継続感 仕様

> [!NOTE]
> **ステータス**: `[実装済 + 将来拡張を含む]`（§5.1 Layer2・§5.3 は `[将来拡張]`/`[検討中]`）
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md)

---

## 5. メモリ管理

### 5.1 3層メモリ設計

| レイヤー | 役割 | 格納場所・制限 | 動作特性 |
|---|---|---|---|
| **Layer 1** 永続的事実 | 人格・ユーザー嗜好・現在のプロジェクト事実 | `SOUL.md`・`MEMORY.md` (5KB 以内)・`USER.md` | 毎ターン system プロンプトへ直接注入。予算超過時は LLM が自律要約（GC）。`[実装済]` |
| **Layer 2** 手続き的スキル | 複雑タスクの再現手順・コマンド・検証チェックリスト | `workspace/skills/*.md`（PicoClaw 互換階層） | オンデマンドロード。`[将来拡張]` AuditorWorker による自動生成・更新。 |
| **Layer 3** エピソード記憶 | 過去の全セッション履歴・試行錯誤ログ | SQLite + tantivy BM25 / `[将来拡張]` rig-fastembed | `search_past_sessions` ツールを LLM が動的実行して過去ログを回収。`[実装済]` |

### 5.2 Sliding Window ローテーション `[実装済]`

1. 会話バッファが予算（設定可能）を超過したとき、最も古い「User 発言 ✕ Agent 応答」の 1 ペアをバッファ先頭からポップする。
2. ポップしたペアは消去前に Markdown チャンクへ整形し、バックグラウンドの MemoryWorker（Lane B）へ非同期コミット（RAG への退避）。
3. GeminiClaw の `truncateWithContext`（先頭 40%・末尾 40% 保持、中間を `[N messages omitted]` で置換）と同等の実装。

### 5.3 70/20/10 コンテキスト戦略 `[検討中]`

コンテキストウィンドウを「対話バッファ(70%)」「動的文脈(20%)」「人格(10%)」に厳格に切り分け、各枠に予算上限を設ける方式。
現状は Sliding Window ローテーションで代替。将来的に rig-core 統合（§15）と合わせて実装を検討。

---

## 9. 会話継続感 6 技法 `[実装済]`

ステートレス API で「会話が続いている」感覚を作るための技法。GeminiClaw のコードから確認した実装パターン。

### ① 会話履歴の蓄積

毎ターン `user_input` と `llm_response` を `ConversationHistory` へ push し、全メッセージを API の `messages[]` に渡す。

### ② コンテキスト圧縮（Sliding Window）

§5.2 参照。80% 閾値超過時に先頭 40% + 末尾 40% 保持、中間を省略。

### ③ Memory Flush（セッション後・非同期・fail-open）

Pipeline 完了後に `tokio::spawn` で切り離す。直近 20 エントリを LLM に渡し `MEMORY.md` + `logs/` の更新を依頼。失敗しても `warn` ログのみ。

### ④ Session Continuation（日またぎ）

翌日の初回ターンのみ発動。前日の summary TL;DR + 直近 5 エントリを system に注入。

### ⑤ Proactive Posts 注入

Heartbeat が自発的に送ったメッセージを「会話履歴外の自分の投稿」として system に注入。「自分が言ったこと」を忘れないための仕組み。

### ⑥ System Prompt 常時注入

毎回の API 呼び出しの system に `SOUL.md` / `AGENTS.md` / `MEMORY.md` / `USER.md`（Interests 含む）を含める。

---

## 将来拡張 `[将来拡張]`

### MEMORY.md・知識構造のスリム化自動トリガー

稼働蓄積で肥大化する `MEMORY.md` や `knowledge/` 配下のナレッジファイルを自動的にクリーンアップする機構。

- `MEMORY.md` が 5KB 上限に近づいた際に、重複・陳腐化エントリを LLM に自律要約（GC）させるトリガー。
- Daily Summary cron と連動して定期的に知識構造全体をコンパクションする設計を検討する。
