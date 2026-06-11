> [!NOTE]
> **ステータス**: `[ACTIVE]` (Upstream とのコードレベル比較仕様・先進機能導入計画)  
> **最終更新日**: 2026-05-30  
> **対象コード**: `crates/rustyclaw-agent/`, `crates/rustyclaw-gateway/`, `crates/rustyclaw-tools/`, `crates/rustyclaw-cli/`

# PicoClaw vs RustyClaw アーキテクチャ比較 ＆ 先進機能導入設計書

本ドキュメントは、Go製 AI エージェントランタイムである Upstream プロジェクト **PicoClaw** と、その設計思想を引き継ぎつつ拡張を行っている Rust 移植版 **RustyClaw** のアーキテクチャ・機能を比較し、RustyClaw へ取り入れるべき先進的な機能やアイデア、およびロードマップを記録する技術仕様・比較書である。

---

## 1. 全体アーキテクチャ比較

| 比較軸 | PicoClaw (Go / Upstream) | RustyClaw (Rust / 本プロジェクト) | 設計上の意図・メリット |
| :--- | :--- | :--- | :--- |
| **言語・ランタイム** | Go (Goroutines) | Rust (`tokio` 非同期ランタイム) | RPi4 (8GB) のハードウェアリソースを極限まで活用し、高速・省メモリ・スレッド安全なシングルバイナリを実現。 |
| **定期実行・Cron** | **イベント駆動・非同期分散**<br>・Inngest `sleepUntil` 連携<br>・ポーリング完全排除<br>・自発的再スケジュール | **インプロセス・Tokio 統合**<br>・`tokio::spawn` 内製ループ<br>・60秒 / 30分 / 日次ポーリング<br>・SQLite 状態比較による重複防止 | RPi4 上で外部インフラ（Inngest）の依存関係を完全に排除し、インプロセスで完全に自己完結してゼロレイテンシー動作する。 |
| **Web 検索ツール** | **極めて豊富**<br>・Brave / Tavily / Perplexity / SearXNG / Baidu / DuckDuckGo フォールバック | **限定的**<br>・Brave Search `web_search` / `web_fetch` のみ | 接続可能な API の選択肢の多さ、およびキー切れ時の内蔵 DuckDuckGo 無料フォールバックによる冗長性の確保。 |
| **拡張機能 (Skill)** | **ClawHub リモートレジストリ**<br>・リモートからの動的検索・ダウンロード・自動導入 | **静的ロードのみ**<br>・`.md` 仕様書をローカル配置し、起動・ホットリロード時に読み込む | コミュニティ共有や外部リポジトリからのスキル動的インストールによる、エージェントエコシステムの大幅な拡張。 |
| **並行制御とタスク** | **SubTurn & Async Spawn**<br>・サブエージェント調整<br>・メインチャットをブロックしない長時間タスクの非同期実行 | **限定的 (直列化または直列化解消中)**<br>・`gmn_sem` の並行制御<br>・セッションごとの直列化 (mpsc) | 重いタスク（ reindex など）をバックグラウンドへ spawn し、メイン対話の応答性を 100% 維持したまま並行動作させる。 |

---

## 2. 取り入れるべき PicoClaw の 4大先進的アイデア

PicoClaw のアーキテクチャと機能から、本番運用のユーザビリティと拡張性を劇的に高める以下の 4 つの先進的アイデアを抽出し、RustyClaw への導入設計を行う。

### ① リアルタイム・ステアリング (Steering - 実行中の方向修正と割り込み)
*   **設計思想**: エージェントが大規模なデータ収集やコードの並列コンパイルなどの「長いループ」を実行している最中、ユーザーがリアルタイムにメッセージ（割り込みシグナルやガイダンス）を注入し、実行中のエージェントの行動をリアルタイムに中断・方向修正・制御できる。
*   **RustyClaw での実装**:
    - `crates/rustyclaw-gateway` の `LaneRegistry` または `MessageBus` が `tokio::sync::broadcast` 等の割り込み用シグナルチャンネルを監視。
    - `execute_with_tools` ループのターン実行ステップ（各ツール実行前後など）に割り込みメッセージを注入する `steering_tx` ポートを設け、外部チャンネルや CLI からの「そのURLの読み込みは中断して」「カレンダー登録のタイトルを〜に変更して」といった指令を安全に処理する。

### ② イベント駆動 Hook システム (Hook System - Observers & Interceptors)
*   **設計思想**: エージェントパイプラインの特定のライフサイクルイベント（LLM 呼び出し前後、ツール実行直前、エラー発生時など）に対して、**Hook（オブザーバー、インターセプター、承認 Hook）** をアタッチして動的に動作を変更できる。
*   **RustyClaw での実装**:
    - `crates/rustyclaw-agent` 内に `HookManager` を実装。
    - これにより、開発中の「ツール実行の承認ゲート (Confirmation Gate)」や「構造化監査ログ (Audit Logger)」を Hook として疎結合に実装でき、コードの美しさと責務分離が極めて強固になる。

### ③ ClawHub 互換のリモート Skill レジストリ ＆ インストーラー
*   **設計思想**: コミュニティレジストリ等から `picoclaw skills install <skill-name>` などのコマンドで、モジュール化された Skill（仕様書・プロンプト・テストコード）を動的にローカルにダウンロード・導入できる。
*   **RustyClaw での実装**:
    - `rustyclaw-cli` に `rustyclaw skills install <skill-name>` などのコマンドを追加。
    - `reqwest` でリモート ZIP/markdown を取得し、`workspace/skills/` に自動展開・インポートする CLI サブタスク。

### ④ バックグラウンド Spawn ＆ 非同期サブエージェント (Async Spawn Tasks)
*   **設計思想**: メイン対話ループをブロックすることなく、数時間かかるような重いタスク（例: 深い Web スクレイピング、 reindex など）をバックグラウンドスレッドで `spawn` し、完了時に自動で MessageBus に結果を戻す。
*   **RustyClaw での実装**:
    - `tokio::spawn` と `MessageBus` のアペンドを利用し、長時間タスク完了時に `cron:spawn-task` などの擬似セッションを介して Telegram/Discord 等へ自動通知する非同期実行マネージャの構築。

---

## 3. 今後の導入・改修タスク

### Phase 30: Upstream 先進機能：Hook・Steering・Spawn タスクの統合 🟡 優先度中
1.  **イベント駆動 Hook システム (Hook Manager) の実装**
    *   LLM呼出前後やツール実行前後に動作をアタッチする `Hook`（オブザーバー、インターセプター、承認 Hook）機構の構築。
    *   `Confirmation Gate` (Phase 23) などを Hook 側に移行・美しくリファクタリング。
2.  **リアルタイム・ステアリング (Steering) 割り込み機構の実装**
    *   `broadcast` または `mpsc` を用いた、実行中の `execute_with_tools` ループへの割り込みメッセージ注入・ターン早期終了処理の実装。
3.  **長時間タスクの非同期 `spawn` ＆ サブエージェント機構の実装**
    *   チャット応答をフリーズさせない長時間非同期ジョブ実行と、完了時の `MessageBus` アペンド通知。
4.  **ClawHub 互換の動的 Skill ダウンローダー・インストーラーの実装**
    *   `rustyclaw skills install <skill-name>` サブコマンドの実装および `workspace/skills/` へのリモート展開ロジック。
5.  **`docs/specs/PicoClaw_comparison.md` の最新コードとの一致確認・更新** (DoD)
