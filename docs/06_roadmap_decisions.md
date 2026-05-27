# 06. 実装ロードマップ・重要設計決定事項

## 1. 実装フェーズ計画

RustyClawの実装は、以下の 5 フェーズに分割してインクリメンタルに進行します。

### Phase 1 — 動く最小構成 (CLI対話)
- **開発クレート**: `rustyclaw-config`, `rustyclaw-providers`, `rustyclaw-agent`, `rustyclaw-cli`
- **作業内容**:
  - `config.json` の型定義と `serde_json` によるシリアライズの実装。
  - `LlmProvider` トレイトの実装および `OpenAiCompatProvider` (OpenAI互換) の作成。
  - `ContextBuilder` と Pipeline の基本骨格の実装。
- **完了条件**: CLIから `rustyclaw agent -m "hello"` を実行し、LLMと会話ができること。

### Phase 2 — Gateway 起動と基本ストレージ
- **開発クレート**: `rustyclaw-storage`, `rustyclaw-gateway`
- **作業内容**:
  - SQLite セッション永続化および `sessions/*.jsonl` ファイル出力。
  - `ConversationHistory` および会話継続 6 技法（履歴、圧縮など）の実装。
  - `Gateway` プロセスのシグナル処理、Hot Reload (SIGHUP) の実装。
- **完了条件**: `rustyclaw gateway` をプロセスとして起動・停止でき、セッションが正常に保存されること。

### Phase 3 — チャンネル接続と Lane Queue
- **開発クレート**: `rustyclaw-channels`
- **作業内容**:
  - Discord チャンネルの実装（WebSocket ゲートウェイ、または Webhook 受信）。※初期段階では Discord を最優先チャネルとする。
  - `LaneRegistry` と Semaphore による並列・直列制御 Lane Queue の実装。
  - 内部イベントの pub/sub をつなぐ `MessageBus` の実装。
- **完了条件**: Discordの外部アプリから話しかけ、並列リクエストが直列・優先度制御されて正常に応答されること。

### Phase 4 — Heartbeat と長期記憶 (Memory)
- **開発クレート**: `rustyclaw-gateway` (HeartbeatService), `rustyclaw-storage` (tantivy)
- **作業内容**:
  - `HeartbeatService` (HEARTBEAT.md Step 1, 2, 5, 7) の実装。
  - `heartbeat-digest.md` の自動生成 (増分/deep scan) の実装。
  - セッション終了後の非同期 Memory Flush (`MEMORY.md` への要約書き出し) の実装。
  - 日またぎの Session Continuation、Daily Summary の実装。
  - `tantivy` を用いた過去ログ・サマリーの純Rust全文検索の組み込み。
- **完了条件**: 長期記憶の要約、Heartbeatによる自発的な巡回、声掛けが動作すること。

### Phase 5 — 拡張機能
- **作業内容**:
  - カレンダー、Email、天気などのツール整備に伴う Heartbeat Step 3, 4 の実装。
  - 追加プロバイダ (Anthropic, Gemini, Ollama) のサポート。
  - 追加チャンネル (Telegram, Slack 等) のフィーチャーフラグによるサポート（後回し）。
  - Rust公式 SDK `rmcp` による外部MCPクライアント連携。
  - `USER.md` 内 Interests の自動監視とパトロール (Interest Patrol) の完全実装。

---

## 2. 12の重要な設計決定事項（変更不可）

以下の決定事項は、設計のシンプル化、RPi4動作環境での安定性確保、およびGeminiClawの仕様との一貫性を維持するため、**変更不可** とします。

1. **`INTERESTS.md` は `USER.md` に統合**
   独立したファイルを作らず、`USER.md ## Interests` セクションとして管理します。
2. **`IDENTITY.md` は使用しない**
   人格定義は `SOUL.md` に統合します。
3. **`PATROL.md` は使用しない**
   Heartbeat巡回の指示書は `HEARTBEAT.md` のみに集約します。
4. **LlmProviderは完全なステートレス HTTP 接続**
   外部プロセス (ACP) や接続プールの維持は行いません。
5. **Heartbeatの自発活動は `last_user_interaction_at` を更新しない**
   誤検出による自己トリガー無限ループを防ぐためです。
6. **`HEARTBEAT.md` はエージェント自身による自己改変を禁止**
   振る舞い定義の完全性を保つため、ユーザーのみが編集可能とします。
7. **`sessions/*.jsonl` への書き込みは fail-closed**
   会話履歴の保存に失敗した場合、パイプライン全体を停止させます。
8. **Memory Flush 処理は fail-open**
   セッション後の非同期メモリ書き出し失敗時は、エラーを記録するのみで会話を止めません。
9. **OpenSSL の依存を持ち込まない**
   クロスコンパイル互換性のため、`reqwest` には `rustls-tls` を強制します。
10. **Backgroundレーンのキュー容量は最大 1 件**
    Heartbeatタスクの積み上がりを完全に防止します。
11. **`logs/` と `summaries/` は物理ディレクトリを分ける**
    `memory/logs/` と `memory/summaries/` でそれぞれ異なるファイルスキーマで管理します。
12. **状態管理の2重化**
    `heartbeat-state.json` はエージェント用に書き出し、Rustシステム自身は `memory.db` 内 `patrol_state` で高速管理します。

---

## 3. GeminiClaw 参照ソースマッピング

実装時にコード設計で迷った際、元となった GeminiClaw リポジトリの以下のファイルを参考にします。

| GeminiClaw 参照ファイル | 参照すべき設計・ロジック |
|---|---|
| `templates/SOUL.md` | 人格定義ファイルの原版テキストテンプレート。 |
| `templates/AGENTS.md` | 行動ルール、およびHeartbeat応答（HEARTBEAT_OK）の判定規約。 |
| `templates/MEMORY.md` | 長期記憶のフォーマットと5KB制限ルール。 |
| `templates/USER.md` | ユーザー情報および Interests のテンプレート構成。 |
| `templates/HEARTBEAT.md` | Heartbeat 7 Step 指示書の完全な内容。 |
| `docs/memory.md` | 信頼性スペクトラム（fail-closed / fail-open）の概念図。 |
| `src/agent/context-builder.ts` | ContextBuilder のメッセージスタッキング、Proactive Posts注入、および `truncateWithContext` (70/20/10) アルゴリズム。 |
| `src/agent/session/store.ts` | JSONL出力設計および `session-titles.json` の管理。 |
| `src/agent/session/continuation.ts` | 日またぎの文脈復元ロジック。 |
| `src/agent/session/flush.ts` | 対話ログからLLMにメモリを抽出させるためのプロンプト定義。 |
| `src/agent/session/heartbeat-digest.ts` | incrementalスキャンおよび6回に1回のdeepスキャンによるダイジェスト生成設計。 |
| `src/agent/turn/pre-execution.ts` | 割り込み防止、およびHeartbeatセッションのチェックロジック。 |
| `src/agent/acp/process-pool.ts` | Lane Queueの数値根拠 (全体上限 6、BG予約 2、ユーザー 4)。 |
| `src/inngest/agent-run.ts` | Inngestの同時実行制御（同一セッション limit=1）の仕様。 |
