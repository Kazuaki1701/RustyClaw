# 09. GeminiClaw / RustyClaw 機能比較

> [!NOTE]
> **ステータス**: `[ACTIVE]` (現時点スナップショット)
> **最終更新日**: 2026-05-31
> **対象コード**: プロジェクト全体

---

## 凡例

| 記号 | 意味 |
|---|---|
| ✅ | 実装済み・正常動作 |
| ⚠️ | 部分実装 / 動作不完全 |
| ❌ | 未実装 |
| N/A | 設計上不要（意図的に除外） |

---

## 1. コアアーキテクチャ

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| 非同期ランタイム | Node.js / Inngest | Tokio (Rust) | |
| LLM 呼び出し | ACP stdio JSON-RPC | reqwest HTTP 直呼び | GeminiClaw の ACP は意図的に除外 |
| 並列制御 | Inngest Lane Queue | gmn_sem(1) + LaneRegistry | |
| 設定ファイル | JSON (暗号化シークレット別管理) | config.json + vault.json | |
| Hot Reload | SIGHUP | SIGHUP ✅ | |
| systemd watchdog | — | WatchdogService ✅ | |
| Health HTTP | — | `/health` `/ready` `/reload` ✅ | |

---

## 2. Pipeline（エージェント実行）

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| ContextBuilder | ✅ | ✅ | SOUL/AGENTS/MEMORY/USER.md 注入 |
| ConversationHistory 蓄積 | ✅ | ✅ | |
| compact_if_needed（70/20/10） | ✅ | ✅ | limit 1500、×1.5 日本語補正済み |
| Session Continuation（日またぎ） | ✅ | ⚠️ | 実装済みだが summaries/ が少なく実質機能薄 |
| **Proactive Posts 注入** | ✅ | ✅ | 実装済み。翌ターンのシステムプロンプトに差し戻す |
| Memory Flush（セッション後非同期） | ✅ | ✅ | fail-open、15分ゲート・6ターン delta |
| execute_with_tools（ツール呼び出しループ） | ✅ | ✅ | OpenAiCompatProvider のみ対応 |
| Streaming SSE | ✅ | ✅ | |

---

## 3. Memory・ストレージ

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| MEMORY.md 自動書き換え | ✅ | ✅ | LLM 全書き直し方式 |
| logs/YYYY-MM-DD.md 日次ログ | ✅ | ✅ | fail-open |
| summaries/ セッションサマリー | ✅ | ✅ | アイドル5分で生成 |
| Daily Summary | ✅ | ✅ | 日付変更時 +5分オフセット |
| **heartbeat-digest.md 増分生成** | ✅ | ✅ | 実装済み。増分・ディープスキャンが正しく動作しダイジェスト出力 |
| SQLite（usage/patrol_state/seen_items） | ✅ | ✅ | WAL モード |
| sessions/*.jsonl（fail-closed） | ✅ | ✅ | |
| **tantivy BM25 全文検索（LLM 公開）** | qmd_query / qmd_get | ✅ | 実装済み。memory_search として LLM に公開 |
| atomic write（電源断対策） | ✅ | ✅ | tempfile → rename |

---

## 4. Heartbeat システム

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| Step 1: 活動レビュー | ✅ | ✅ | 正常稼働。ダイジェストに基づいて自己文脈を正しく認識 |
| Step 2: Memory 整理 | ✅ | ✅ | |
| Step 3: Calendar / Email チェック | ✅ | ✅ | gws_calendar_list_events / gws_gmail_list_messages |
| Step 4: 天気チェック | ✅ | ✅ | `yolp_weather` ツール実装済み（Open-Meteo バックエンド、Phase 32） |
| Step 5: 声掛け（Quiet Hours 考慮） | ✅ | ✅ | |
| Step 6: プロアクティブ作業 | ✅ | ✅ | |
| Step 7: HEARTBEAT_OK 応答 | ✅ | ✅ | |
| Heartbeat 間隔 | 10分 | 30分 | CF neurons 節約のため変更 |
| HEARTBEAT_OK 無音 | ✅ | ✅ | Discord への報告なし |

---

## 5. CronService・スケジューラー

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| Heartbeat 定期実行 | ✅ | ✅ | |
| Daily Summary 自動生成 | ✅ | ✅ | |
| Session Summary（アイドル検出） | ✅ | ✅ | 1件/60s 制限（CF 節約） |
| **cron.json 動的スケジューラー** | — | ✅ | RustyClaw 独自機能。ホットリロード対応 |
| 起動時分散発火（interval_at） | — | ✅ | T+30s/60s/90s でずらして同時発火防止 |
| http- セッションをサマリー除外 | — | ✅ | dashboard セッションの無限ループ修正 |

---

## 6. Skills システム

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| **Skills ファイルロード** | 標準仕様（YAML Frontmatter / `SKILL.md`）に完全準拠し、段階的開示（Discovery & Activation）をサポート。従来フラットファイルとの下位互換性も担保 | ✅ | `skills.rs` が `gray_matter` による YAML 解析と Discovery/Activation 注入に対応。スキル内 scripts 実行やトラバーサル防御もサポート（Phase 35） |
| daily-briefing skill | ✅ | ✅ | `skills/daily-briefing/SKILL.md` の標準パッケージ構造に移行完了（Phase 35） |
| vitals-coach skill | ✅ | ✅ | `skills/vitals-coach/SKILL.md` の標準パッケージ構造に移行・統合完了。データ取得、タイムラグ検証、医療警告、閾値分析を一本化（Phase 35） |
| topic-patrol skill | ✅ | ✅ | `skills/topic-patrol/SKILL.md` の標準パッケージ構造に移行完了（Phase 35） |
| deep-research skill | ✅ | ✅ | `skills/deep-research/SKILL.md` の標準パッケージ構造に移行完了（Phase 35） |
| coding-plan skill | ✅ | ✅ | `skills/coding-plan/SKILL.md` の標準パッケージ構造に移行完了（Phase 35） |
| todo-tracker skill | ✅ | ✅ | `skills/todo-tracker/SKILL.md` の標準パッケージ構造に移行完了（Phase 35） |
| workspace skill | ✅ | ✅ | `skills/workspace/SKILL.md` の標準パッケージ構造に移行完了（Phase 35） |
| session-logs skill | ✅ | ✅ | `skills/session-logs/SKILL.md` の標準パッケージ構造に移行完了。`session-stats.sh`・`session-search.sh` で分析クエリ対応（Phase 35） |
| karakeep skill | ✅ | ✅ | `skills/karakeep/SKILL.md` の標準パッケージ構造をTDDで新規作成。`501_karakeep-cleanup.sh`・`502_karakeep-tag-items.sh` による削除・推薦に対応（Phase 35） |
| agent-browser skill | ✅ | ❌ | `npx agent-browser:*` 依存。対応ツールなし |
| github skill | ✅ | ❌ | `run_shell_command` 依存 |

---

## 7. チャンネル・通知

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| Discord | ✅ | ✅ | serenity ライブラリ |
| Telegram | ✅ | ❌ | 未実装 |
| Slack | — | ❌ | 計画なし |
| geminiclaw_post_message ツール | ✅ | N/A | RustyClaw は返答テキストを自動投稿 |
| geminiclaw_list_channels ツール | ✅ | N/A | 同上 |

---

## 8. 外部ツール連携

| 機能 | GeminiClaw（ツール名） | RustyClaw（ツール名） | 備考 |
|---|---|---|---|
| Google Calendar 参照 | gog_calendar_events | gws_calendar_list_events ✅ | |
| Google Calendar 書き込み | gog_calendar_insert | gws_writable_calendar_insert ✅ | 許可カレンダーのみ。config で管理 |
| Gmail 参照 | gog_gmail_search | gws_gmail_list_messages ✅ | |
| **Gmail 送信** | gog_gmail_send | ❌ | 意図的に未実装（送信禁止） |
| Gmail 削除 | gog_gmail_trash | gws_gmail_trash_message ✅ | _ai-agent ラベル必須ガード付き |
| **Google Drive** | gog_drive_* | ❌ | 未実装 |
| **Google Sheets** | gog_sheets_* | ❌ | 未実装 |
| **Google Docs** | gog_docs_* | ❌ | 未実装 |
| Karakeep 参照 | Karakeep API スクリプト | karakeep_list_bookmarks ✅ | Rust 直実装 |
| Karakeep タグ付け | 502_karakeep-tag-items.sh | karakeep_tag_bookmark ✅ | Rust 直実装 |
| Obsidian 検索 | Obsidian MCP (SSE) | obsidian_search ✅ | Rust 直実装 |
| Obsidian 読み取り | Obsidian MCP (SSE) | obsidian_read_note ✅ | Rust 直実装 |
| **Obsidian 書き込み・追記** | Obsidian MCP (SSE) | ✅ | obsidian_write_note として LLM に公開 |
| **Obsidian Dataview クエリ** | Obsidian MCP (SSE) | ❌ | 未実装 |
| **全文検索（Memory）** | qmd_query / qmd_get | ✅ | memory_search として LLM に公開 |
| **天気** | 天気ツール | ✅ | `yolp_weather`（Open-Meteo 60分降水予報、Phase 32） |

---

## 9. Rate Limit 対策

| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| 429 検知・バックオフ | ✅ | ✅ | |
| **GLOBAL_COOLDOWN（CF 対応）** | — | ✅ | OpenAiCompatProvider で 429 時にセット |
| reset_after() CF RPM パース | — | ✅ | "too many requests" → デフォルト 60s |
| CF neurons 日次上限パース | — | ✅ | internalCode 4006 → 翌 09:00 JST まで待機 |
| gmn_sem(1) 全直列化 | — | ✅ | |

---

## 10. 実行環境・デプロイ

| 項目 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| 実行環境 | Node.js / TypeScript | Rust (aarch64) / RPi4 | |
| LLM プロバイダ | Gemini CLI (gmn) | Cloudflare Workers AI | |
| 外部プロセス常駐 | Node.js MCP × 複数 | **なし** ✅ | 全ツール Rust 直実装または gws subprocess |
| クロスコンパイル | — | cross / aarch64-linux-gnu ✅ | scripts/cross-build.sh |
| systemd サービス | — | ✅ | /etc/systemd/system/rustyclaw.service |
| OAuth スコープ削減 | — | ✅ | 12 → 7 スコープ（不要スコープ削除済み） |
