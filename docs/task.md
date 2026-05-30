# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-05-30  
> **アーカイブ**: 完了済みフェーズ (Phase 2〜19) は `docs/archive/2026-05-30-completed-phases-2-to-19.md` に保存

---

## Phase 20: ログ点検で判明したバグ修正（2026-05-30）✅ 完了

- `[x]` **【バグ修正】`obsidian_search` / `karakeep_list` の `limit` 型エラー**
  - **完了日**: 2026-05-30

---

## Phase 21: Topic Patrol — GeminiClaw からの完全移植 ✅ 完了 (2026-05-30)

- `[x]` **1. `web_search` / `web_fetch` ツールの実装**
- `[x]` **2. `workspace/skills/topic-patrol.md` の作成**
- `[x]` **3. Skills ファイルロード機構の実装**
- `[x]` **4. `patrol/findings.md` 14日プルーニングの確認・補完**
- `[x]` **5. エンドツーエンド動作確認**

---

## Phase 22: GeminiClaw 移植ギャップの回収（Proactive Posts / heartbeat-digest 等） 🔴 優先度高

- `[ ]` **1. `Proactive Posts` 注入の実装**
  - Heartbeat による自発メッセージ（Discord 等への声掛け）を、翌ターンの対話時に「会話履歴外の自分の発言」としてシステムプロンプトに差し戻すロジックの実装。
  - 対象: `crates/rustyclaw-agent/src/lib.rs` (`execute` および `execute_with_tools` 内)

- `[ ]` **2. `heartbeat-digest.md` ロジックの点検・修正**
  - CLIテスト等で無効化されている `heartbeat-digest.md` のタイムスタンプ・差分ロードロジックを修正し、増分スキャンおよびディープスキャンが正しく動作するように改修。
  - 対象: `crates/rustyclaw-gateway/src/heartbeat.rs`

- `[ ]` **3. `tantivy` 全文検索および `Obsidian` 書き込みツールの LLM 公開**
  - `MemorySearchTool` と `ObsidianWriteTool` (Vaultへの新規書き込み・追記) を実装して `rustyclaw-tools` に追加・登録。
  - 対象: `crates/rustyclaw-tools/src/lib.rs` + `crates/rustyclaw-gateway/src/lib.rs` (登録)

- `[x]` **4. 天気チェック（Heartbeat Step 4）** ✅ 完了 (2026-05-30)

- `[ ]` **5. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 28: 統合型リアルタイム・ダッシュボード (HTML UI) の構築 ✅ 完了 (2026-05-31)
> 目的: ヘッドレスで稼働する RPi4 上で、現在のキューの待機状況や、LLM の累積トークン利用・コスト統計を美麗な Web UI で可視化する（開発デバッグを促進するため優先度を格上げ）。
> 対象: `crates/rustyclaw-gateway/` (`health.rs`), `crates/rustyclaw-storage/`
> 実装計画: `docs/superpowers/plans/2026-05-30-phase28-dashboard.md`（サイバー CSS + Monitor/Stats 2タブ構成）

- `[x]` **1. 統合ステータス JSON API の実装**
  - `/api/concurrency`（`gmn_sem` 取得状態・キュー深度・クールダウン）追加。
  - `/api/usage/summary` `/api/usage/timeline` `/api/usage/by-trigger`（SQLite `usage` 集計、`?since=` 期間フィルタ対応）追加。
  - `LlmResponse` にトークンフィールド追加し、`usage` テーブルを拡張して LLM 呼び出し毎にトークン使用量を記録。

- `[x]` **2. 美麗なダッシュボード HTML UI (GET /) の組み込み**
  - サイバー CSS（スキャンライン・ネオングロー・グリッチヘッダー）+ Monitor/Stats 2タブ構成。
  - Monitor: Lane Queue・Concurrency・CF Neurons・LLM Request/Response Inspector・App Log・Chat。
  - Stats: KPI ゲージ・SVG 時系列チャート・モデル別/トリガー別ブレークダウン（`/api/usage/*` をライブ取得）。

- `[ ]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 28b: ダッシュボード精度・起動最適化のフォローアップ 🟡 優先度中
> 出典: 2026-05-31 の Phase 28 実機検証（`gateway --no-agent` 起動ログ点検）で判明した改善候補。

- `[ ]` **1. memory flush の LLM 呼び出しをトークン計上対象に含める** ⚠️ ダッシュボード精度に直結
  - 1回の対話で、メインの `execute_with_tools` 応答に加えて memory flush（MEMORY.md 再生成）用の LLM 呼び出しが走るが、後者は `record_usage` を発火せず Stats が**実消費より過少計上**になる。
  - flush 経路（`crates/rustyclaw-agent/src/lib.rs` の `flush_memory` 系）でも `LlmResponse` のトークンを取得し、`trigger_type` を `memory-flush` 等として `record_usage` する。
  - 対象: `crates/rustyclaw-agent/src/lib.rs` + 記録呼び出し箇所（`crates/rustyclaw-gateway/src/lib.rs`）

- `[ ]` **2. Gateway 起動時の設定ロード遅延（約11秒）の短縮検討** 🟢 優先度低
  - `Initializing daemon` から `loaded configuration` まで約11秒を要する（`--no-agent` でも発生）。プロバイダ生成・vault 初期化まわりのコスト要因を特定し、遅延要素の遅延初期化（lazy）等で起動高速化を検討。
  - 対象: `crates/rustyclaw-gateway/src/lib.rs`（`Gateway::run` 初期化シーケンス）

---

## Phase 23: 安全ガードレールと構造化監査ログの構築 🔴 優先度高

- `[ ]` **1. 自律レベル制御 (Autonomy Level) と承認ゲート (Confirmation Gate) の実装**
  - `AutonomyLevel` (`Autonomous` / `Supervised` / `ReadOnly`) の導入。
  - `supervised`（監視モード）時、書き込みや破壊的アクションに対して `ask-user` ファイル監視で実行を非同期ブロッキングする承認ゲートの実装。

- `[ ]` **2. 構造化ツール監査ログ (Audit Logger) の実装**
  - ツール実行結果をパラメータ切り詰めの上 `{workspace}/memory/audit.jsonl` に保存する仕組みの実装。

- `[ ]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 24: LLM 接続プロバイダ層の耐障害性（レジリエンス）強化 🔴 優先度高

- `[ ]` **1. LLM プロバイダ層への指数バックオフ（Exponential Backoff）ネットワークリトライの実装**
  - 一過性接続エラーや 5xx エラーに対し、透過的リトライハンドラを導入。

- `[ ]` **2. クォータ枯渇時の自動モデルオフローダー (Model Offloader) の実装**
  - クォータ制限期間中、一時的に代替モデル（例: `gemini-3.5-flash` 等）へ自動フォールバック・自動復帰。

- `[ ]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 25: 並行制御の最適化とフリーズ防止（Lane Queue 改善） 🔴 優先度高

- `[ ]` **1. `gmn_sem` の並列化開放とファイルレベルロックの導入**
  - LLM API 呼び出しの並列実行を開放。
  - `crates/rustyclaw-storage` にファイルアトミックロック機構を追加し、`MEMORY.md` やセッションログへの書き込み競合をファイルレベルの精密な排他制御で保護する。
  - 対象: `crates/rustyclaw-gateway/src/lib.rs` ＋ `crates/rustyclaw-storage/src/lib.rs`

- `[ ]` **2. 実行キュー取得の安全待機タイムアウトの実装**
  - `crates/rustyclaw-gateway/src/lib.rs` におけるセマフォ取得処理への `tokio::time::timeout` 導入。

- `[ ]` **3. Chat Progress Reporter (Typing... 送信) の実装**
  - `crates/rustyclaw-channels` にツール実行中の Typing アクション定時送信機構を導入し、`execute_with_tools` のライフサイクルと結合。

- `[ ]` **4. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 26: 外部 MCP クライアントの堅牢化とトランスポート拡張 🟡 優先度中

- `[ ]` **1. 子プロセスクラッシュ時の自動再接続・復旧 (Auto-Reconnect) の実装**
  - `crates/rustyclaw-mcp/src/lib.rs` の接続ライフサイクルに異常終了監視と `spawn` 再試行ループを追加。

- `[ ]` **2. 外部 MCP サーバーの「メモリ回収（Idle Eviction）」機構の実装**
  - 一定時間 (例: 30分) 呼び出されていない MCP 子プロセスを一度安全にクローズしてメモリを回収、次回ツール呼び出し時にオンデマンドで自動再起動。

- `[ ]` **3. SSE (Server-Sent Events) トランスポートおよび Resources / Prompts 連携の追加**
  - HTTP/SSE 経由の外部リモート MCP サーバー接続サポートの実装。
  - Tools（工具）機能だけでなく、Resources や Prompts にもクエリ可能にするための I/O 拡張。

- `[ ]` **4. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 27: ハウスクリーニング、ディスク容量保護と Cron 拡張 🟡 優先度中

- `[ ]` **1. ディスク空き容量監視と SSD 保護の導入**
  - 定期実行時に USB SSD の空き容量をチェックし、残り容量が 5% 以下になった際に Discord 等へ警告アラートを投げる保護タスクの実装。

- `[ ]` **2. Cron セッションおよびログの自動プルーニングの実装**
  - 古い `cron:` 実行ログやセッションファイルを自動消去するクリーンアップ機構の実装。
  - 対象: `crates/rustyclaw-gateway/src/cron.rs`

- `[ ]` **3. 1回限り (at / deleteAfterRun) ジョブの自動削除サポート**
  - 実行完了後に `cron.json` から自身のジョブ定義を自動削除し、アトミックに更新保存。

- `[ ]` **4. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 30: Upstream 先進機能：Hook・Steering・Spawn タスクの統合 🟡 優先度中
> 目的: Go 製 Upstream (PicoClaw) の優れた先進的設計思想（動的割り込み、イベントHook、非同期 Spawn、ClawHub Skills）を取り入れ、RustyClaw を次世代エージェントランタイムへと昇華させる。
> 対象: `crates/rustyclaw-agent/`, `crates/rustyclaw-gateway/`, `crates/rustyclaw-tools/`, `crates/rustyclaw-cli/`

- `[ ]` **1. イベント駆動 Hook システム (Hook Manager) の実装**
  - LLM 呼出前後、ツール実行前後、エラー発生時などに動作をアタッチできる `Hook`（オブザーバー、インターセプター、承認 Hook）機構の実装。
  - `Confirmation Gate` (Phase 23) などを Hook 側に移行・美しくリファクタリング。

- `[ ]` **2. リアルタイム・ステアリング (Steering) 割り込み機構の実装**
  - `broadcast` または `mpsc` を監視し、長いツール実行の最中にユーザーが割り込み（Interruption）およびガイダンス（行動方向修正）を注入する仕組みの実装。

- `[ ]` **3. 長時間タスクの非同期 `spawn` ＆ サブエージェント機構の実装**
  - チャット応答をフリーズさせない長時間非同期ジョブ実行と、完了時の `MessageBus` アペンド通知。

- `[ ]` **4. ClawHub 互換の動的 Skill ダウンローダー・インストーラーの実装**
  - `rustyclaw skills install <skill-name>` サブコマンドの実装および `workspace/skills/` へのリモート展開ロジック。

- `[ ]` **5. `docs/specs/PicoClaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## 次期大型対応検討案件 🟡 優先度中

> 現時点では保留。前提条件の整理・設計検討が必要な案件。

- `[x]` **`gmn_sem > 1` の並列化復活** (✅ 完了: Phase 25 のファイルレベルロック導入タスクに統合・マージ)
- `[x]` **Heartbeat 実行中のユーザー対話ブロック対策** (✅ 完了: Phase 25 の並列実行開放タスクに統合・マージ)

---

## 継続モニタリング 🟡 優先度中

- `[ ]` **RPi4 本番稼働 — cron.json 定期ジョブの発火確認**
  - Daily Briefing・Topic Patrol・Vital Check が実際に Discord へ正常通知されることを確認
  - Karakeep / Obsidian ネイティブツールが RPi4 上で正常動作することを確認

---

## 将来の検討課題 🟢 優先度低

- `[ ]` **LLM Provider 追加候補**
  - Cerebras `gpt-oss-120b`（14,400 RPD・60k TPM）
  - Google AI Studio（Gemma 3 27B）
  - OpenRouter 新モデル: `qwen3-coder:free`（1M ctx）・`qwen3-next-80b:free` ...等

- `[ ]` **本番環境の自動バックアップ体制**
  - `production/workspace/`（`memory.db`・`sessions/*.jsonl`• `patrol/findings.md` 等）を QNAP 等の NAS へ定時 rsync

- `[ ]` **MEMORY.md および知識構造のスリム化自動トリガー**
  - 稼働蓄積で肥大化するナレッジファイルの自動クリーンアップ検討

- `[ ]` **stn/rqmd によるローカル知識ベース RAG 構築**（Phase 13 積み残し）

- `[ ]` **Google Drive / Sheets / Docs ツール**
  - gws CLI 経由で実装可能。ユースケースが明確になった時点で追加
