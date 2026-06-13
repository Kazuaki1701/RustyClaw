# RustyClaw — メモリ管理 & 会話継続感 仕様

> [!NOTE]
> **ステータス**: `[実装済 + 将来拡張を含む]`（§5.1 Layer2・§5.3 は `[将来拡張]`/`[検討中]`）
> **バージョン**: v0.3
> **最終更新日**: 2026-06-12（Phase 51-1 実装を反映：履歴制限トークン予算式・小コンテキスト時プロンプト圧縮・compact_if_needed 未呼び出し現状を明記）
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md)

---

## 5. メモリ管理

### 5.1 3層メモリ設計

| レイヤー | 役割 | 格納場所・制限 | 動作特性 |
|---|---|---|---|
| **Layer 1** 永続的事実 | 人格・ユーザー嗜好・現在のプロジェクト事実 | `SOUL.md`・`USER.md`・`MEMORY.md` (5KB 以内) | `SOUL.md` は常時注入。`USER.md` は大コンテキスト（> 8192 tokens）のみ注入。`MEMORY.md` は system prompt へ直接注入しない（`flush_memory` で LLM が書き換え、RAG 経由での動的注入は ISSUE-28）。`[実装済]` |
| **Layer 2** 手続き的スキル | 複雑タスクの再現手順・コマンド・検証チェックリスト | `workspace/skills/*.md`（PicoClaw 互換階層） | オンデマンドロード。`[将来拡張]` AuditorWorker による自動生成・更新。 |
| **Layer 3** エピソード記憶 | 過去の全セッション履歴・試行錯誤ログ | SQLite + tantivy BM25 / `[将来拡張]` rig-fastembed | `search_past_sessions` ツールを LLM が動的実行して過去ログを回収。`[実装済]` |

### 5.2 Sliding Window ローテーション `[実装済]`

**現行動作（Phase 51-1 時点）**:

1. 各 API 呼び出し前に `ConversationHistory::trim_to_last(N)` でバッファ末尾 N 件のみ保持する。
2. N = `get_history_message_limit(purpose)` = `(context_window_tokens × 65% / 350).clamp(min, 150)` で算出（Phase 51-1）。小コンテキスト（≤ 8192）は min=2、それ以外は min=20。
3. 古い履歴は単純に破棄（現在は RAG 退避なし）。

**`compact_if_needed()` について**（`rustyclaw-storage` に実装済み・main pipeline では未呼び出し）:

`ConversationHistory::compact_if_needed(limit)` として `rustyclaw-storage` に実装済みだが、main pipeline からは呼ばれていない（`trim_to_last` のみ使用）。仕様:
- 推定トークン数が `limit × 80%` を超えたらトリガー
- 先頭 70%（背景保持）＋ 末尾 20%（直近対話保持）＋ 中間 10% を `[N messages omitted]` で置換
- GeminiClaw の `truncateWithContext` に相当する設計

将来の ContextBuilder 統合時に有効化予定（§5.3 参照）。

### 5.3 70/20/10 コンテキスト戦略 `[一部実装済・v0.4 残課題]`

コンテキストウィンドウを「対話バッファ(70%)」「動的文脈(20%)」「人格(10%)」に厳格に切り分け、各枠に予算上限を設ける方式。

**Phase 51-1 で先行実装済み:**
- `LlmModelConfig.context_window_tokens` でモデル毎の context window サイズを取得
- 履歴件数を `(cw × 65% / 350).clamp(min, 150)` のトークン予算式で算出（§5.2 参照）
- 小コンテキスト（≤ 8192）では `build_system_context()` が SOUL.md のみ注入し、人格枠を節約

**v0.4 残課題（段階実装順）:**
- **Heartbeat Digest**: Heartbeat 実行前に増分セッションダイジェスト生成 → 動的文脈(20%)枠に注入
- **Session-level Summary**: アイドル 5 分後にサマリー生成 → `try_ctx_index` でエピソード記憶登録
- **ContextBuilder 予算管理**: `compact_if_needed()` を pipeline に統合し、各枠の token 上限を動的計算（`v0.4/00_rustyclaw.md §4.2・§7 #26` 参照）

---

## 9. 会話継続感 6 技法 `[実装済]`

ステートレス API で「会話が続いている」感覚を作るための技法。GeminiClaw のコードから確認した実装パターン。

### ① 会話履歴の蓄積

毎ターン `user_input` と `llm_response` を `ConversationHistory` へ push し、全メッセージを API の `messages[]` に渡す。

### ② コンテキスト圧縮（Sliding Window）

§5.2 参照。現行: `trim_to_last(N)` で末尾 N 件保持（N はトークン予算式で算出）。`compact_if_needed()` による 70%/20% 保持は pipeline への統合待ち。

### ③ Memory Flush（セッション後・非同期・fail-open）

Pipeline 完了後に `tokio::spawn` で切り離す。直近 20 エントリを LLM に渡し `MEMORY.md` + `logs/` の更新を依頼。失敗しても `warn` ログのみ。

### ④ Session Continuation（日またぎ）

翌日の初回ターンのみ発動。前日の summary TL;DR + 直近 5 エントリを system に注入。

### ⑤ Proactive Posts 注入

Heartbeat が自発的に送ったメッセージを「会話履歴外の自分の投稿」として system に注入。「自分が言ったこと」を忘れないための仕組み。

### ⑥ System Prompt 常時注入

毎回の API 呼び出しの system に以下を注入（`build_system_context()` — Phase 51-1 現行実装）:

| 注入対象 | 条件 |
|---|---|
| `SOUL.md` | 常時（全モデル） |
| `USER.md` | context_window_tokens > 8,192 のみ |
| `proactive-posts.md`（最終1件） | context_window_tokens > 8,192 のみ |
| `AGENTS.md` / `MEMORY.md` | 注入しない（MEMORY.md は `flush_memory` で LLM が更新・RAG 注入は ISSUE-28） |

Heartbeat 専用: `build_heartbeat_context()` が `SOUL.md` + `HEARTBEAT.md` のみ注入。

---

## 将来拡張 `[将来拡張]`

### MEMORY.md・知識構造のスリム化自動トリガー

稼働蓄積で肥大化する `MEMORY.md` や `knowledge/` 配下のナレッジファイルを自動的にクリーンアップする機構。

- `MEMORY.md` が 5KB 上限に近づいた際に、重複・陳腐化エントリを LLM に自律要約（GC）させるトリガー。
- Daily Summary cron と連動して定期的に知識構造全体をコンパクションする設計を検討する。
