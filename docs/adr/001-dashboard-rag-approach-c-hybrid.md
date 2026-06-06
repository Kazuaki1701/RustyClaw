# ADR 001: Dashboard チャット RAG 活用アーキテクチャ選定

- **ステータス**: `[ACCEPTED]`
- **決定日**: 2026-06-07
- **関連タスク**: Phase 41-1

## 1. コンテキスト（直面した課題）

Dashboard チャット（`POST /chat`）は静的システムプロンプト（SOUL.md + USER.md）のみで動作しており、Heartbeat 実行結果・KaraKeep 整理/推薦履歴・Topic Patrol 収集内容・過去の Dashboard 会話が参照できない。ユーザーは「今日の KaraKeep 推薦は？」「最近の Heartbeat 結果を教えて」といった実運用レポートに対して会話したいが、エージェントはそれらの情報を持っていない。

追加の技術課題:
- `http-dashboard.jsonl` が累積で 550KB に肥大し、Session Continuation が機能しない
- `estimated_tokens` が `ctx_limit` を超えて Memory Flush がスキップされ続ける
- Heartbeat は 30 分毎の高頻度のため、全実行ログを RAG 化するとベクトルストアが溢れる

## 2. 検討した選択肢（Options）

- **案A: リアルタイム RAG 注入（全 cron ジョブ + Heartbeat を全て RAG 化）**
  - メリット: 最も包括的。全実行結果が参照可能。
  - デメリット: Heartbeat が 30 分毎 × 7 Step × 複数チャンク = 1 日 50〜100 チャンク蓄積。TTL 管理が複雑。ベクトルストアが数週間で溢れる。

- **案B: Heartbeat Digest のみ（heartbeat-digest.md の静的注入）**
  - メリット: 実装が最小限（1 ファイル読み込み追加のみ）。トークンコスト固定（≤750 tokens）。
  - デメリット: KaraKeep 推薦・Patrol 収集など cron ジョブの実行結果が参照不可。「今日の推薦は？」に答えられない。

- **案C: ハイブリッド（採用）— cron サマリー RAG ＋ digest 動的注入 ＋ session_id ローテーション**
  - メリット: 既存の `generate_session_summary → ingest_session_summary` フローを流用するため追加インフラ不要。Heartbeat は digest 注入で対応（RAG 溢れなし）。cron ジョブは 1 日 1 回のサマリーのみ RAG 化。session_id 日付ローテーションで Session Continuation も機能する。
  - デメリット: 案B より実装箇所が多い（6 ファイル）。サマリー生成はジョブ完了の数分後に非同期で行われるため、ジョブ直後の質問ではサマリーが未反映の場合がある。

## 3. 決定と選定理由（Decision & Justification）

- **決定**: 案C（ハイブリッド）を採用
- **理由**: 
  - KaraKeep・Patrol 結果への質問という核心ユースケースは案Bでは対応不可。
  - 既存の session summary インフラを完全流用でき、新規コンポーネントが不要。
  - Heartbeat の高頻度問題は digest 注入（固定コスト≤750 tokens）で解決可能。
  - session_id ローテーションにより、過去の Dashboard 会話の Session Continuation も副次的に得られる。
  - 全処理が fail-open で設計できるため、失敗時の影響が局所的。

## 4. トレードオフ・今後の影響（Consequences）

**制約:**
- cron サマリー RAG 化は非同期のため、ジョブ完了直後（数分以内）の質問にはサマリーが未反映の場合がある。許容範囲と判断（ユースケースは翌朝の「昨日の推薦は？」が主）。
- `http-dashboard.jsonl`（550KB）は移行後も残るが読まれなくなる。定期プルーニング（Phase 27-2）で解消予定。

**将来の影響:**
- `cron:heartbeat` を除いた 7 種の cron ジョブサマリーが RAG に蓄積される。TTL（`session_summary_ttl_days: 7`）で自動削除されるため管理コストは低い。
- ISSUE-28（MEMORY.md の RAG 登録）実装後は `memory:` チャンクも top_k=8 の恩恵を受ける。
- 将来 Dashboard top_k をチューニングする場合は `config.json` の `dashboard_top_k` を変更するだけでよい。

**関連 ADR:** なし（初回決定）
