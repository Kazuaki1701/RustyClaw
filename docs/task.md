# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-05-31  
> **アーカイブ**: 完了済みフェーズ (Phase 2〜19) は `docs/archive/2026-05-30-completed-phases-2-to-19.md`、(Phase 20, 21, 22, 28, 29, 32, 33, 旧31) は `docs/archive/2026-05-31-completed-phases-20-21-28-31.md` に保存

> **優先方針（2026-05-31 更新）**: **GeminiClaw との機能ギャップ回収を最優先（🔴）とする。**  
> それ以外の独自機能・改善案件は一旦 🟢 に降格。GeminiClaw ギャップが解消され次第、改めて優先度を見直す。

---

## 🔴 GeminiClaw 機能ギャップ（最優先）

---

## Phase 29: Skills ファイルロードシステムの実装 ✅ 完了

- `[x]` **1. Skills ロードエンジンの実装**
  - `crates/rustyclaw-gateway/src/skills.rs` に `inject_skill_content()` 実装済み。gateway L.530 で cron dispatch 前に注入中。

- `[x]` **2. Skill 定義ファイルの作成**
  - `daily-briefing`・`vitals-coach`・`deep-research`・`todo-tracker`・`coding-plan`・`workspace`・`session-logs`・`topic-patrol` を `production/workspace/skills/` に作成。

- `[x]` **3. `docs/specs/09_geminiclaw_feature_comparison.md` の最新コードとの一致確認・更新** (DoD)
  - Skills セクション全行を更新。ロードエンジン・8スキルを ✅ に、session-logs を ⚠️ に変更。

---

## Phase 32: 天気チェックツールの実装（Heartbeat Step 4） ✅ 完了
> `YolpWeatherTool`（Open-Meteo バックエンド）として実装済み。gateway L.744 で LLM 登録済み。

- `[x]` **1. YOLP 雨雲レーダー API ツールの実装** — `yolp_weather` として実装・登録済み

- `[x]` **2. `docs/specs/09_geminiclaw_feature_comparison.md` の最新コードとの一致確認・更新** (DoD)
  - Heartbeat Step 4 および §8 天気行を ❌ → ✅ に更新。

---

## Phase 34: session-logs Skill 向け分析スクリプトの整備 ✅ 完了

- `[x]` **1. `scripts/session-stats.sh` の作成**
  - セッション一覧・メッセージ数。`--workspace`・`--date`・`--days` オプション対応。sqlite3 未インストール時もグレースフルに終了。

- `[x]` **2. `scripts/session-search.sh` の作成**
  - `<keyword>` で `sessions/*.jsonl` の content を grep。`--workspace`・`--date` オプション対応。マッチ行を role 付きで表示。

- `[x]` **3. `docs/specs/09_geminiclaw_feature_comparison.md` の最新コードとの一致確認・更新** (DoD)
  - session-logs skill を ⚠️ → ✅ に更新。

- `[x]` **テスト**: `test-session-scripts.sh` で 13 テスト全パス（TDD RED→GREEN 確認済み）

---

## Phase 35: 標準 Agent Skills 仕様 (agentskills.io) への対応と統合 ✅ 完了
> 計画書 (`docs/superpowers/plans/2026-05-31-standard-agent-skills-integration-plan.md`) に基づき、標準の `SKILL.md` (YAML Frontmatter付) と段階的開示 (Progressive Disclosure) に完全対応。

- `[x]` **1. Rustデータ構造の定義と Frontmatter YAML パーサーの実装 (Phase A)**
  - `Cargo.toml` に `gray_matter = "0.2"` を導入。
  - `crates/rustyclaw-gateway/src/skills.rs` に `SkillManifest` と `Skill` 構造体を実装。
  - YAMLのパースおよびエラーハンドリング処理の実装。

- `[x]` **2. ハイブリッドスキャンエンジンの実装と後方互換性の確保 (Phase A)**
  - `workspace/skills/` を巡回し、`[skill-name]/SKILL.md` ディレクトリ構造を優先スキャン。
  - 従来のフラットな `[skill-name].md` を検知した際、疑似的にメタデータを生成してロードするフォールバック処理を実装。

- `[x]` **3. Discovery (レベル1) システムプロンプト自動生成の実装 (Phase B)**
  - 全スキルの `name` と `description` のみを集約した「Skills Directory」を起動時にキャッシュ。
  - セッション開始時のシステムプロンプト末尾に自動で差し込むインジェクターの実装。

- `[x]` **4. Activation (レベル2) 動的インジェクションの実装 (Phase B)**
  - LLMリクエスト送信時に、プロンプトテキストおよび会話履歴にスキルのトリガー識別子（例: `use-skill: <name>` 等）が含まれているかをスキャンするエンジンを実装。
  - トリガーされたスキルの `SKILL.md` 本文（Instructions）のみをコンテキストに動的マージする処理の実装。

- `[x]` **5. Execution (レベル3) スキル内スクリプトの解決とトラバーサル防御の実装 (Phase C)**
  - `run_workspace_script` のパス解決を `skills/[skill-name]/scripts/[script-name]` に拡張。
  - 親ディレクトリ遡行 (`..` や `/`) などのトラバーサル攻撃を厳格に防御するバリデーター `resolve_secure_script_path` の実装。

- `[x]` **6. 既存8スキルのマイグレーション (Phase C)**
  - 現行の `vitals-coach.md` や `session-logs.md` などのフラット構成を、`[skill-name]/SKILL.md` 形式に移行し、YAML Frontmatterを追加。
  - `500_get-vital-data-garmin.sh` などのスクリプトをそれぞれの `scripts/` ディレクトリに移動し、パス定義を更新。

- `[x]` **7. 単体テストの記述と自動検証**
  - YAMLパース、ハイブリッドスキャン、Discovery提示、動的Activation、およびセキュリティパス解決の単体テストを `crates/rustyclaw-gateway/src/skills.rs` 等に記述。
  - `cargo test` で全テストがパスすることを確認。

- `[x]` **8. RPi4 実機検証と Discord 連携テスト**
  - RP1（Raspberry Pi 4）へ `./scripts/deploy.sh` でデプロイし、systemd の正常稼働および Skills 処理にクラッシュ・異常遅延がないことをログ検証。

- `[x]` **9. docs/specs/09_geminiclaw_feature_comparison.md の更新** (DoD)
  - 標準Skills仕様完全準拠および8スキルのマイグレーション完了を比較表に反映。

---

## Phase 24: LLM 接続プロバイダ層の耐障害性（レジリエンス）強化 🔴
> GeminiClaw は 429 検知・バックオフおよびモデルフォールバックを実装済み。RustyClaw での同等機能。

- `[ ]` **1. LLM プロバイダ層への指数バックオフ（Exponential Backoff）ネットワークリトライの実装**
  - 一過性接続エラーや 5xx エラーに対し、透過的リトライハンドラを導入。

- `[ ]` **2. クォータ枯渇時の自動モデルオフローダー (Model Offloader) の実装**
  - クォータ制限期間中、一時的に代替モデル（例: `gemini-3.5-flash` 等）へ自動フォールバック・自動復帰。

- `[ ]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## 🟢 その他の改善案件（独自機能・将来対応）

---

## Phase 25: 並行制御の最適化とフリーズ防止（Lane Queue 改善・回収）残り 🟢
> ※ 完了済みの Phase 1〜2 はアーカイブ済み。残 2 件は独自安全改善。  
> ※ **item 5（Lossless Resume）は GeminiClaw ギャップB に相当するため、GeminiClaw ギャップ回収完了後に 🔴 昇格を検討する。**

- `[x]` **1. Lane Queue（Inngest 代替）の機能ギャップ分析とロードマップ策定**

- `[ ]` **2. 実行キュー取得の安全待機タイムアウトの実装**
  - `crates/rustyclaw-gateway/src/lib.rs` におけるセマフォ取得処理への `tokio::time::timeout` 導入。

- `[x]` **3. Chat Progress Reporter (Typing... 送信) の実装 (Phase 1)**

- `[x]` **4. 並行数 4 への拡張に向けたファイルロック機構の導入 (Phase 2)**

- `[ ]` **5. 実行ステップのチェックポイント化と Lossless Resume の導入 (Phase 3)**（GeminiClaw ギャップB・昇格候補）
  - 途中でエラー（送信失敗など）が発生した際に、LLM API の再呼出を行わず失敗したフェーズから再試行できる中間状態の永続化と復旧機構。

- `[x]` **6. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 28b: ダッシュボード精度・起動最適化のフォローアップ 🟢
> 出典: 2026-05-31 の Phase 28 実機検証（`gateway --no-agent` 起動ログ点検）で判明した改善候補。

- `[ ]` **2. Gateway 起動時の設定ロード遅延（約11秒）の短縮検討** 🟢 優先度低
  - `Initializing daemon` から `loaded configuration` まで約11秒を要する（`--no-agent` でも発生）。遅延要素の遅延初期化（lazy）等で起動高速化を検討。
  - 対象: `crates/rustyclaw-gateway/src/lib.rs`（`Gateway::run` 初期化シーケンス）

---

## Phase 26: 外部 MCP クライアントの堅牢化とトランスポート拡張 🟢

- `[ ]` **1. 子プロセスクラッシュ時の自動再接続・復旧 (Auto-Reconnect) の実装**
  - `crates/rustyclaw-mcp/src/lib.rs` の接続ライフサイクルに異常終了監視と `spawn` 再試行ループを追加。

- `[ ]` **2. 外部 MCP サーバーの「メモリ回収（Idle Eviction）」機構の実装**
  - 一定時間 (例: 30分) 呼び出されていない MCP 子プロセスを一度安全にクローズしてメモリを回収、次回ツール呼び出し時にオンデマンドで自動再起動。

- `[ ]` **3. SSE (Server-Sent Events) トランスポートおよび Resources / Prompts 連携の追加**
  - HTTP/SSE 経由の外部リモート MCP サーバー接続サポートの実装。
  - Tools（工具）機能だけでなく、Resources や Prompts にもクエリ可能にするための I/O 拡張。

- `[ ]` **4. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 27: ハウスクリーニング、ディスク容量保護と Cron 拡張 🟢

- `[ ]` **1. ディスク空き容量監視と SSD 保護の導入**
  - 定期実行時に USB SSD の空き容量をチェックし、残り容量が 5% 以下になった際に Discord 等へ警告アラートを投げる保護タスクの実装。

- `[ ]` **2. Cron セッションおよびログの自動プルーニングの実装**
  - 古い `cron:` 実行ログやセッションファイルを自動消去するクリーンアップ機構の実装。
  - 対象: `crates/rustyclaw-gateway/src/cron.rs`

- `[ ]` **3. 1回限り (at / deleteAfterRun) jobの自動削除サポート**
  - 実行完了後に `cron.json` から自身のジョブ定義を自動削除し、アトミックに更新保存。

- `[ ]` **4. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 23: 安全ガードレールと構造化監査ログの構築 🟢
> ※ GeminiClaw に直接対応機能なし。RustyClaw 独自の安全機構として重要だが、GeminiClaw ギャップ回収優先のため降格。

- `[ ]` **1. 自律レベル制御 (Autonomy Level) と承認ゲート (Confirmation Gate) の実装**
  - `AutonomyLevel` (`Autonomous` / `Supervised` / `ReadOnly`) の導入。
  - `supervised`（監視モード）時、書き込みや破壊的アクションに対して `ask-user` ファイル監視で実行を非同期ブロッキングする承認ゲートの実装。

- `[ ]` **2. 構造化ツール監査ログ (Audit Logger) の実装**
  - ツール実行結果をパラメータ切り詰めの上 `{workspace}/memory/audit.jsonl` に保存する仕組みの実装。

- `[ ]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)

---

## Phase 30: Upstream 先進機能：Hook・Steering・Spawn タスクの統合 🟢
> ※ PicoClaw (Go Upstream) 参照。GeminiClaw ギャップではなく、RustyClaw の独自進化機能として位置付け。  
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

## Phase 31 — 保留（前提条件の解決後に着手）
- `[ ]` **ISSUE-22: `gmn_sem` capacity の config 化＋書き込み直列化の責務分離**（capacity 引き上げ検討時。**旧 Phase 25-1 を統合**。メモリ `project_user_sem_concurrency` 参照）
- `[ ]` **ISSUE-25: `●ACTIVE` → daemon STOP 制御**（無認証 LAN への破壊操作の露出・START 非対称性のセキュリティ前提を解決後）
- `[ ]` **ISSUE-09: rp1 の LM Studio 依存（単一障害点）のフェイルオーバ設計**
- 観察のみ: ISSUE-10（ローカル Gemma 品質）/ 13（一時 WS の context file WARN）/ 14（gws calendar WARN・現状解消）

---

## 次期大型対応検討案件 🟢 優先度低

> 現時点では保留。前提条件の整理・設計検討が必要な案件。

---

## 継続モニタリング 🟢 優先度低

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
  - `production/workspace/`（`memory.db`• `sessions/*.jsonl`• `patrol/findings.md` 等）を QNAP 等の NAS へ定時 rsync

- `[ ]` **MEMORY.md および知識構造のスリム化自動トリガー**
  - 稼働蓄積で肥大化するナレッジファイルの自動クリーンアップ検討

- `[ ]` **stn/rqmd によるローカル知識ベース RAG 構築**（Phase 13 積み残し）

- `[ ]` **Google Drive / Sheets / Docs ツール**
  - gws CLI 経由で実装可能。ユースケースが明確になった時点で追加
