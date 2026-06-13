# Phase 52 用途別 LLM コンテキスト最適化・軽量化 実装計画書

> [!NOTE]
> **ステータス**: `[ACTIVE]` (Phase 52-6 完了（Phase 52 完了）)  
> **最終更新日**: 2026-06-13  
> **対象コード**: `crates/rustyclaw-agent/src/lib.rs`, `crates/rustyclaw-gateway/src/lib.rs` 等  
> **設計仕様書**: [`docs/specs/2026-06-13-phase52-context-optimization-design.md`](../specs/2026-06-13-phase52-context-optimization-design.md)  

## 開発タスクチェックリスト

- [x] **【Phase 52-1】全体共通の静的・基礎最適化の実装**（完了: 2026-06-13）:
  - [x] デリミタの XML タグ化 (`<mem>`, `</mem>`, `<log>`, `</log>`) への変更とパースロジックの修正。
  - [x] 各システムプロンプトのシンプル英語化（memory flush prompt を圧縮英語化、~100 トークン削減）。
  - [x] 同一機能の外部スクリプト（Home Assistant）を `ha-control.sh` に集約（SKILL.md トークン ~60% 削減）。
  - [x] 会話履歴からの一時的なノイズ（進捗バー等）を削る `cleanse_for_memory_flush()` 関数の実装。
  - [x] 外部ツール（Gmail snippet / Calendar location）を 200 文字以内に切り詰める `truncate_tool_item_fields()` の実装。
- [x] **【Phase 52-2】用途別最適化 - Heartbeat（自動巡回監視）の実装**（完了: 2026-06-13）:
  - [x] `build_heartbeat_context` から `SOUL.md` を除外（HEARTBEAT.md のみに限定）。
  - [x] Heartbeat 専用 `ToolRegistry` から `WorkspaceWriteTool` を除外（書き込みリスク排除）。
  - [x] `ctx_execute` 利用は HEARTBEAT.md 既存指示（Calendar/Gmail）で対応済み。
- [x] **【Phase 52-3 + 52-3b】用途別最適化 - Chat（ユーザー対話）の実装**（完了: 2026-06-13）:
  - [x] `ctx_search` を用いた動的スキル選択（Dynamic Skill Selection）機能の実装。
  - [x] `USER.md` 興味関心（Interests）を RAG で動的注入する機能の実装。
  - [x] Keep / Gmail 返却値が長大な場合の文字数制限（トリミング）または事前要約の実装。
  - [x] `PreCompact` / `SessionStart` フックの有効化と SQLite スナップショット退避・復元の確認済み。（context-mode plugin が 15 カテゴリのセッションイベントを自動キャプチャ・compact_count=2、resume snapshot 生成確認）
- [x] **【Phase 52-4】用途別最適化 - Topic Patrol の実装**（完了: 2026-06-13）:
  - [x] `ctx_fetch_and_index` を用いた巡回先Web/フィードのキャッシュ・インデックス化の実装。
  - [x] ニュース要約に特化した極小コンテキスト構築処理の実装。
- [x] **【Phase 52-5】長期記憶（MEMORY.md）のセマンティック分割（Memory RAG）の実装**（完了: 2026-06-13）:
  - [x] `MEMORY.md` のセクション分割と `ctx_index` による SQLite FTS5 同期の実装。
  - [x] チャット開始時のメモリ動的ロード（`ctx_search`）の実装。
  - [x] `ctx_patch` を用いた部分メモリ書き換えの実装。（スコープ外: Phase 52-5b として延期）
- [x] **【Phase 52-6】エピソード記憶連携とブリーフィング高度化の実装**（完了: 2026-06-13）:
  - [x] ブリーフィング結果の自動 `ctx_index` 登録処理の実装。
  - [x] `ctx_search` を用いた過去のバイタル・予定傾向の相関検索とアドバイス注入の実装。
- [ ] **【共通】適応的クォータガード（Adaptive Quota Guard）の実装**:
  - [ ] `RateLimiter` による TPM / TPD 逼迫検知と、それと連動した「動的ツールしきい値上昇」「履歴バジェット引き下げ」の実装。
- [ ] **ドキュメント整理・DoDの達成 (Phase 52)**:
  - [ ] 基本仕様書（`docs/specs/`）の同期更新と最終更新日のアップデート。
  - [ ] 本計画書のタスクチェックリストをすべて `[x]`（完了）に更新。
  - [ ] 本計画書および検証報告書（作成された場合）の `docs/archive/` への退避。
  - [ ] `docs/task.md` の完了マーク更新と目次のクレンジング。

---

## 1. 詳細実装ステップ

### 1.1. Phase 52-1: 全体共通の静的・基礎最適化
1.  **デリミタの更新**:
    *   `crates/rustyclaw-agent/src/lib.rs` の `extract_delimited_block` 関数を変更し、XMLタグ `<mem>`, `</mem>`, `<log>`, `</log>` に対応させます。
2.  **システムプロンプトの英語化とコメント併記**:
    *   `SOUL.md` や `USER.md` 内の英語指示を簡素化し、日本語翻訳には行頭に `// ` を付けたコメントアウト形式を採用します。`Pipeline::strip_comments` がこれらを自動で取り除くことをユニットテストで保証します。
3.  **ツールの集約**:
    *   Home Assistant関連の露出スクリプトを1つに集約し、引数でコマンドをディスパッチするように実装。
4.  **外部ツール出力のフィルタリング**:
    *   `rustyclaw-tools` クレートの Gmail/Calendar 取得関数において、不要なJSONキー（`etag`, `creator_email`等）やメールヘッダーをLLMへ渡す前にフィルタリングする前処理を追加します。

### 1.2. Phase 52-2: Heartbeat の最適化
1.  **コンテキスト構築ルーチン（`build_heartbeat_context`）の改修**:
    *   `SOUL.md` や `USER.md` をロードせず、タイムゾーンと最小限の監視指示のみを含むプロンプトに変更。
2.  **露出スキルの限定**:
    *   Heartbeat 用に不要な操作系ツールを除外し、読み取りと通知のみを許可するツールリスト制限ロジックを追加。

### 1.3. Phase 52-3: Chat の最適化
1.  **Dynamic Skill Selection の実装**:
    *   `ctx_search` ツールを実行し、ユーザーの発話に最も合致するスキルのみを動的にロード。
2.  **興味関心の RAG 注入**:
    *   `USER.md` の「Interests」を `ctx_index` でデータベースに保持し、対話時に `ctx_search` でマッチしたセクションのみを注入。
3.  **スナップショット（PreCompact/SessionStart）**:
    *   Gateway側でコンパクション発生タイミングを検知し、`context-mode` のSQLiteセッション永続化フックを実行。

*(※ Phase 52-4以降の各ステップの詳細な実装コード修正箇所は、各フェーズ移行時にその時のコンテキストに合わせて詳細を詰めます)*

---

## 2. 検証とテスト方針

1.  **単体テスト**:
    *   `cargo test` を使用し、新しく実装された XML パーサー、Gmail/Calendar のJSONフィルタリングロジック、およびコメントストリップ機能が正しく動作することを確認。
2.  **統合テスト (トークン測定)**:
    *   疑似対話シミュレーターを走らせ、従来の約 10k トークン消費から、設計通り 4k トークン前後（約50%以上削減）に収まっているかを計測・検証。
