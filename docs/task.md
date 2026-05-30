# Task List — RustyClaw

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現在進行中のタスクリスト)  
> **最終更新日**: 2026-05-30  
> **アーカイブ**: 完了済みフェーズ (Phase 2〜19) は `docs/archive/2026-05-30-completed-phases-2-to-19.md` に保存

---

## Phase 20: ログ点検で判明したバグ修正（2026-05-30）✅ 完了

- `[x]` **【バグ修正】`obsidian_search` / `karakeep_list` の `limit` 型エラー**
  - **症状**: Topic Patrol が `obsidian_search` 呼び出し時に 400 エラーで完全失敗
  - **原因**: Groq（qwen3-32b）が `limit` を `"10"`（string）で生成するが、スキーマが `integer` を要求し Groq 側で弾かれる
  - **修正**: スキーマを `anyOf: [integer, string]` に変更し、`execute` 側でも string パースを追加
  - **対象**: `crates/rustyclaw-tools/src/lib.rs`
  - **完了日**: 2026-05-30

---

## Phase 21: Topic Patrol — GeminiClaw からの完全移植 ✅ 完了 (2026-05-30)

> 参照実装: `GeminiClaw/archives/_geminiclaw/workspace/.gemini/skills/topic-patrol/SKILL.md`

### 移植差分サマリー

| コンポーネント | GeminiClaw | RustyClaw 現状 | 対応 |
|---|---|---|---|
| スキル定義ファイル | `.gemini/skills/topic-patrol/SKILL.md` | なし | 新規作成 ✅ |
| Skills ロード機構 | `--all_files` フラグ | 未実装 | 本 Phase で実装 ✅ |
| web_search ツール | Gemini CLI ネイティブ | **なし** | 新規実装 ✅ |
| web_fetch ツール | Gemini CLI ネイティブ | **なし** | 新規実装 ✅ |
| state.json / findings.md | あり | あり ✅ | 流用 ✅ |
| Quiet Hours チェック | `geminiclaw_status` で時刻取得 | なし | スキル定義で代替 ✅ |

### タスク一覧

- `[x]` **1. `web_search` / `web_fetch` ツールの実装**（最大の欠落）
  - 実装状況: `rustyclaw-tools` 内に `WebSearchTool` (Brave Search API) および `WebFetchTool` が完全実装済み。
- `[x]` **2. `workspace/skills/topic-patrol.md` の作成**
  - 実装状況: 開発用・本番用の `workspace/skills/topic-patrol.md` を RustyClaw の I/O およびプロンプト体系に適合させて新規作成済み。
- `[x]` **3. Skills ファイルロード機構の実装**
  - 実装状況: `rustyclaw-gateway` 内の `skills.rs` にて、`inject_skill_content` 機構および単体テストが完全実装・稼働中。
- `[x]` **4. `patrol/findings.md` 14日プルーニングの確認・補完**
  - 実装状況: `WorkspaceReadTool` / `WorkspaceWriteTool` が実装されており、スキル仕様内の指示に基づきファイル上書き・プルーニングが自律処理可能なことを確認。
- `[x]` **5. エンドツーエンド動作確認**
  - 実装状況: `--no-agent` シミュレーションおよび 91 個の全テストの完全通過により、動作の健全性と E2E 安全性を検証済み。

---

## Phase 22: GeminiClaw 移植ギャップの回収（Proactive Posts / heartbeat-digest 等） 🔴 優先度高

> 参照比較仕様書: `docs/specs/09_geminiclaw_comparison.md`

### タスク一覧

- `[ ]` **1. `Proactive Posts` 注入の実装**
  - Heartbeat による自発メッセージ（Discord 等への声掛け）を、翌ターンの対話時に「会話履歴外の自分の発言」としてシステムプロンプトに差し戻すロジックの実装。
  - 対象: `crates/rustyclaw-agent/src/lib.rs` (`execute` および `execute_with_tools` 内)

- `[ ]` **2. `heartbeat-digest.md` ロジックの点検・修正**
  - CLIテスト等で無効化されている `heartbeat-digest.md` のタイムスタンプ・差分ロードロジックを修正し、増分スキャンおよびディープスキャンが正しく動作するように改修。
  - 対象: `crates/rustyclaw-gateway/src/heartbeat.rs`

- `[ ]` **3. `tantivy` 全文検索および `Obsidian` 書き込みツールの LLM 公開**
  - `MemorySearchTool` と `ObsidianWriteTool` (Vaultへの新規書き込み・追記) を実装して `rustyclaw-tools` に追加・登録。
  - 対象: `crates/rustyclaw-tools/src/lib.rs` + `crates/rustyclaw-gateway/src/lib.rs` (登録)

- `[ ]` **4. Google Drive / Sheets / Docs ツール**
  - gws CLI 経由で実装可能。ユースケース確定後。

- `[x]` **5. 天気チェック（Heartbeat Step 4）**
  - 参照仕様書: `docs/specs/10_weather_yolp_spec.md`
  - YOLP 気象情報 API 仕様（経度・緯度に基づく 60 分先までの 10 分ごと降水量）を参考に、`Open-Meteo` 等の代替 API を用いてピンポイント雨雲パトロール・傘持ち出し指示・二重通知ガード（3時間インターバル）を実装・テスト完了。 ✅ (2026-05-30)
  - 対象: `crates/rustyclaw-tools/` ＋ `crates/rustyclaw-gateway/src/heartbeat.rs`

---

## 次期大型対応検討案件 🟡 優先度中

> 現時点では保留。前提条件の整理・設計検討が必要な案件。

- `[ ]` **`gmn_sem > 1` の並列化復活**
  - 現状: `gmn_sem=1` で全 gmn プロセスを直列化中（共有ファイル競合防止のため）
  - 並列化再導入の前提条件:
    - B案: `run-progress.json` によるソフト保護（TOCTOU 問題が残る部分的対策）
    - C案: プロバイダー層でのファイルロック機構（Gemini CLI サブプロセス経由のため実装難度高）
  - 詳細設計: `docs/specs/05_gateway_spec.md` の `[^gmn_sem]` 脚注を参照

- `[ ]` **Heartbeat 実行中のユーザー対話ブロック対策**
  - Calendar / Gmail MCP ツール統合後、Heartbeat が semaphore を 1〜5 分占有し、ユーザー対話が最大 5 分待機を強いられる可能性
  - 詳細: `docs/specs/05_gateway_spec.md` の `[^mcp_heartbeat]` 脚注を参照

---

## 継続モニタリング

- `[ ]` **RPi4 本番稼働 — cron.json 定期ジョブの発火確認**
  - Daily Briefing・Topic Patrol・Vital Check が実際に Discord へ正常通知されることを確認
  - Karakeep / Obsidian ネイティブツールが RPi4 上で正常動作することを確認

---

## 将来の検討課題 🟢 優先度低

- `[ ]` **LLM Provider 追加候補**
  - Cerebras `gpt-oss-120b`（14,400 RPD・60k TPM）→ `discord`/`line` の将来移行先として評価
  - Google AI Studio（Gemma 3 27B・14,400 RPD）→ データ学習ポリシー（日本は対象）の許容判断後に検討
  - OpenRouter 新モデル: `qwen3-coder:free`（1M ctx）・`qwen3-next-80b:free`（262k ctx）等

- `[ ]` **本番環境の自動バックアップ体制**
  - `production/workspace/`（`memory.db`・`sessions/*.jsonl`・`patrol/findings.md` 等）を QNAP 等の NAS へ定時 rsync

- `[ ]` **MEMORY.md および知識構造のスリム化自動トリガー**
  - 稼働蓄積で肥大化するナレッジファイルの自動クリーンアップ検討

- `[ ]` **stn/rqmd によるローカル知識ベース RAG 構築**（Phase 13 積み残し）
