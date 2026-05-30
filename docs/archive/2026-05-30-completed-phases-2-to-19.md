# 完了済みフェーズ — RustyClaw（〜2026-05-30）

> アーカイブ日: 2026-05-30  
> 元ファイル: `docs/task.md`

---

## Phase 2 & 4: Gateway Services, Heartbeat System, Long-Term Memory ✅

- `[x]` **1. rustyclaw-storage の強化**
  - `[x]` `tantivy` 追加・`SearchIndexManager` 実装
  - `[x]` セッション ID のファイルシステムセーフなファイル名マッピング

- `[x]` **2. rustyclaw-agent の強化**
  - `[x]` Session Continuation（日またぎ文脈復元）実装
  - `[x]` Memory Flush 非同期トリガー実装（`execute` / `execute_stream`）

- `[x]` **3. rustyclaw-gateway の強化**
  - `[x]` `WatchdogService`（systemd watchdog）
  - `[x]` `HealthServer`（軽量 TCP HTTP サーバー）
  - `[x]` `CronService`（Heartbeat 10 分 / Daily Summary 毎時チェック）
  - `[x]` `HeartbeatService`（digest 生成・Quiet Hours・HEARTBEAT_OK 判定・Proactive Post）
  - `[x]` Background レーンキュー容量 1 制限
  - `[x]` `lastUserContact` 追跡

---

## Phase 5: Rate Limit 対策・Memory Flush 改善・運用品質向上 ✅

- `[x]` **gmn プロバイダ — `--no-agent` 必須化**
- `[x]` **gmn パッチビルド**（`GMN_MAX_RETRIES` 環境変数・`--version` サフィックス）
- `[x]` **セマフォ値削減**（`user_sem`: 4→2、`bg_sem`: 2→1）
- `[x]` **`user_sem` を 1 に削減**（共有ファイル競合防止）
- `[x]` **`gmn_sem` 統合セマフォへの一本化**（user / bg / flush を直列化）
- `[x]` **`flush_sem` 専用セマフォ追加**
- `[x]` **Memory Flush — GeminiClaw 方式全書き直し**
- `[x]` **Memory Flush トークン最適化**（delta 分のみ・max_tokens 1500）
- `[x]` **Memory Flush 実行頻度最適化**（delta 閾値 6・時間ゲート 15 分）
- `[x]` **ログタイムスタンプのローカルタイム化（JST）**
- `[x]` **Dashboard 更新**（セッションログ動的検索・各種エンドポイント追加）

---

## Phase 6: 次回セッション向け Todo ✅

- `[x]` gmn デバッグログの稼働確認
- `[x]` Memory Flush の動作確認
- `[x]` Dashboard の動作確認
- `[x]` rate limit 時のリトライ戦略検討
- `[x]` Session Continuation の動作確認
- `[x]` コードレビュー指摘の対応（コンパイラ警告解消）
- `[x]` 仕様書へのフィードバック（DoD の適用）

---

## Phase 7: 稼働観察で判明した要修正・要点検項目 ✅

> 5/28 00:00〜06:55 のセッションログ分析で判明した問題。

- `[x]` **【バグ】Heartbeat セッション履歴の無制限肥大化**（ステートレス化で対処）
- `[x]` **【バグ】Heartbeat の LLM 応答が空**（GmnCliProvider フォールバック実装）
- `[x]` **【バグ】heartbeat-digest.md が 0 バイト**（deep scan 常時実行に変更）
- `[x]` **【既知問題】MCP ツール呼び出し JSON がチャットに漏出**（JSON Leak Filter 追加）
- `[x]` **【要確認】73 分間 Heartbeat が停止**（Daily Summary に 300s オフセット導入）

---

## Phase 8: Context Management 改善 ✅

- `[x]` **A. Heartbeat Digest の真の実装**（generate_heartbeat_digest + 増分スキャン）
- `[x]` **B. Session-level Summary の実装**（アイドル 5 分後にセッションサマリー生成）
- `[x]` **C. JSONL 削減（truncateBefore）**（Daily Summary 後 30 日超エントリ削除）
- `[x]` **D. Session Summary の増分更新**
- `[x]` **仕様書の更新**（`04_heartbeat_spec.md` / `02_agent_pipeline.md`）

---

## Phase 9: 自前 MCP クライアント実装 (rustyclaw-mcp) と外部サーバー統合 ✅

- `[x]` rustyclaw-tools クレート新設（`Tool` トレイト・`ToolRegistry`）
- `[x]` rustyclaw-mcp クレート新設（JSON-RPC 2.0 stdio・初期化ハンドシェイク）
- `[x]` Gateway & Agent アジェンティックループへの統合
- `[x]` 外部 MCP サーバー接続設定（Google Calendar / Gmail / Karakeep / Obsidian）
- `[x]` Karakeep 運用スクリプト点検 + AGENTS.md への指示追記

---

## Phase 10: gmn エラーヘルプ表示の抑制（SilenceUsage） ✅

- `[x]` `root.go` に `SilenceUsage: true` 追記
- `[x]` gmn バイナリのリビルド・デプロイ
- `[x]` 動作検証（`Usage:` 出力の完全抑制確認）

---

## Phase 11: 動的レートリミットバックオフ待機（Quota Reset 解析） ✅

- `[x]` `ProviderError` に `reset_after()` を追加（分秒混在パース）
- `[x]` ユニットテスト拡張
- `[x]` Gateway 3 リトライループへの動的バックオフ適用

---

## Phase 12: rustyclaw-cli --version およびビルド時刻表示の追加 ✅

- `[x]` `build.rs` 新設（コンパイル時タイムスタンプ注入）
- `[x]` clap CLI メタデータ拡張
- `[x]` ログ初期化順序の最適化

---

## Phase 13: Lightweight RPi4 Optimization (Rust In-process Tools) ✅

- `[x]` Gateway を `execute_with_tools` + `McpManager` に切り替え
- `[x]` Karakeep の Rust インプロセス直実装化（`KarakeepListTool` / `KarakeepTagTool`）
- `[x]` Obsidian の Rust インプロセス直実装化（`ObsidianSearchTool` / `ObsidianReadTool`）
- `[x]` 全 MCP 外部プロセス無効化（`enabled: false`）
- `[x]` production/config.json モデル統一（Cloudflare Llama-3-8b）
- `[x]` aarch64 クロスビルド（`scripts/cross-build.sh` 作成・26MB バイナリ生成）
- `[x]` RPi4 (`rp1`) デプロイ・systemd 常駐化
- `[x]` `gws`（Rust 製）による Google Workspace 連携（subprocess 方式）
  - `GwsCalendarTool` / `GwsGmailTool` 実装・aarch64 クロスビルド・OAuth 認証・疎通確認

---

## Phase 14: スクリプト本番移行と動的 cron.json スケジューラー実装 ✅

- `[x]` 本番環境用 scripts/ のマージと整理（`500_`〜`502_` インデックス付きリネーム）
- `[x]` 旧 patrol/ データの新環境移行
- `[x]` 動的 cron.json ホットリロード式スケジューラーの構築
- `[x]` AGENTS.md の参照コマンド例更新
- `[x]` cargo check / cargo test（全 46 テスト）オールグリーン確認

---

## Phase 15: CF rate limit バースト対策 ✅

- `[x]` `OpenAiCompatProvider` の 429 で `GLOBAL_COOLDOWN` を設定
- `[x]` `reset_after()` に CF RPM 429 パース追加・デフォルト 60s
- `[x]` Session Summary の複数セッション同時発火を抑制（1件/60s tick に制限）

---

## Phase 17: Vault コマンド実装 & 本番環境 symlink 再構成 ✅

### #1 rustyclaw vault コマンドの実装

- `[x]` `vault set / get / list / delete / status / migrate / import-json` 実装
- `[x]` AES-256-GCM + PBKDF2-HMAC-SHA256 暗号化（`vault.enc`・chmod 600）
- `[x]` vault.json 後方互換フォールバック

### #2 `~/.rustyclaw → production/` symlink 再構成

- `[x]` vault.json → vault.enc 移行（6 件）
- `[x]` workspace rsync
- `[x]` symlink 切り替え・後処理

---

## Phase 18: デバッグ機能・コンテキスト最適化 ✅

- `[x]` `--no-agent` デバッグモード（`NoopProvider`・`RUSTYCLAW_NO_AGENT=1` 対応）
- `[x]` Heartbeat 専用軽量コンテキスト（SOUL + MEMORY + HEARTBEAT のみ・▲43%）
- `[x]` `//` コメントによる API 送信除外（SOUL.md 英語優先化・約 26% 削減）

---

## Phase 19: LLM 用途別モデル割り当て最適化（Provider 分散） ✅

| purpose | モデル | Provider |
|---------|--------|---------|
| `default` | groq-llama-8b | Groq |
| `tools` | groq-qwen3-32b | Groq |
| `discord` | groq-llama-70b | Groq |
| `line` | groq-llama-70b | Groq |
| `heartbeat` | groq-llama-8b | Groq |
| `summary` | cf-gemma-4-26b | CF |
| `memory` | cf-qwen3-30b | CF |

- `[x]` `AgentsConfig` 拡張（tools / discord / line / heartbeat フィールド追加）
- `[x]` `execute_with_tools()` に purpose 引数追加
- `[x]` `execute_heartbeat()` → `get_model("heartbeat")` 変更
- `[x]` `get_model()` fallback ロジック拡張
- `[x]` config.json 更新（全 7 purpose・CF モデル有効化）
- `[x]` Pi へのデプロイ・本番稼働確認
