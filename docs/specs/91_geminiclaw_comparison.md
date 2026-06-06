> [!NOTE]
> **ステータス**: `[ACTIVE]` (移植進捗、機能マトリクス、およびコードレベルの比較仕様)  
> **最終更新日**: 2026-06-06  
> **対象コード**: プロジェクト全体
> **備考**: 旧 `09_geminiclaw_feature_comparison.md` を本ファイルに統合・集約

# GeminiClaw vs RustyClaw 比較 & 移植進捗レポート

本ドキュメントは、TypeScript 版エージェントである **GeminiClaw** と、Rust への移植版である **RustyClaw** のアーキテクチャ、機能マトリクス、およびソースコードレベルの実装差分を整理し、未移植機能（ギャップ）と今後の移植進捗を記録する技術仕様・比較書である。

---

## 1. アーキテクチャおよび主要コンポーネント比較

| 比較軸 | GeminiClaw (TypeScript) | RustyClaw (Rust) | 設計上の意図・メリット |
| :--- | :--- | :--- | :--- |
| **言語・ランタイム** | Bun / Node.js (V8) | Rust (`tokio` 非同期ランタイム) | Raspberry Pi 4 (8GB) の CPU/メモリリソース最適化、シングルバイナリ化。 |
| **LLM 接続方式** | **ACP (Agent Control Protocol)**<br>Gemini CLI を stdio JSON-RPC サブプロセスとして制御 | **LlmProvider (直接 HTTP SSE)**<br>`reqwest` + `rustls` を使用した直接のステートレス接続 | 外部プロセス起動の遅延および一時ファイル・プロセスの競合によるデッドロックリスクの完全排除。 |
| **プロセス・デーモン制御** | **PM2**<br>PM2による起動・管理 | **systemd**<br>・systemdによる定常起動・ライフサイクル管理を採用 | ホストOS標準のsystemdでデーモンプロセス管理を行うため、RustyClaw自身には不要な二重実装を行わない。 |
| **並列・排他制御** | Inngest / 自作プロセスプール | `tokio::sync::Semaphore` / Lane Registry | インプロセスで完結する軽量でスレッド安全な同時実行制御。 |
| **状態永続化** | `heartbeat-state.json` (ファイル) | SQLite WAL モード (`deadpool-sqlite`) ＋ JSONL | 電源断に対する堅牢性 (atomic write + SQLite WAL) の向上。 |
| **全文検索 (RAG)** | QMD (外部プロセス) | `tantivy` (インプロセス BM25 検索) | 外部プロセス依存を排除した純 Rust によるローカル検索。 |

---

## 2. 機能別 移植状況マトリクス

### 凡例
| 記号 | 意味 |
|---|---|
| ✅ | 実装済み・正常動作 |
| ⚠️ | 部分実装 / 動作不完全 |
| ❌ | 未実装 |
| N/A | 設計上不要（意図的に除外） |

### 2-1. コアアーキテクチャ
| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| 非同期ランタイム | Node.js / Inngest | Tokio (Rust) | |
| LLM 呼び出し | ACP stdio JSON-RPC | reqwest HTTP 直呼び | GeminiClaw の ACP は意図的に除外 |
| 並列制御 | Inngest Lane Queue | gmn_sem(1) + LaneRegistry | |
| 設定ファイル | JSON (暗号化シークレット別管理) | config.json + vault.enc (暗号化) / vault.json | 暗号化 vault.enc & 平文フォールバック、およびスクリプト向け `$vault:` 動的環境変数インジェクションに対応 ✅ |
| Hot Reload | SIGHUP | SIGHUP ✅ | |
| systemd watchdog | — | WatchdogService ✅ | |
| Health HTTP | — | `/health` `/ready` `/reload` ✅ | |

### 2-2. Pipeline（エージェント実行）
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

### 2-3. Memory・ストレージ
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

### 2-4. Heartbeat システム
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

### 2-5. CronService・スケジューラー
| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| Heartbeat 定期実行 | ✅ | ✅ | |
| Daily Summary 自動生成 | ✅ | ✅ | |
| Session Summary（アイドル検出） | ✅ | ✅ | 1件/60s 制限（CF 節約） |
| **cron.json 動的スケジューラー** | — | ✅ | RustyClaw 独自機能。ホットリロード対応 |
| 起動時分散発火（interval_at） | — | ✅ | T+30s/60s/90s でずらして同時発火防止 |
| http- セッションをサマリー除外 | — | ✅ | dashboard セッションの無限ループ修正 |

### 2-6. Skills システム
| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| **Skills ファイルロード** | 標準仕様（YAML Frontmatter / `SKILL.md`）に完全準拠し、段階的開示（Discovery & Activation）をサポート。従来フラットファイルとの下位互換性も担保 | ✅ | `skills.rs` が YAML 解析と Discovery/Activation 注入、および `env` を介した `$vault:` 動的解決・多層フォールバック、トラバーサル防御を完全サポート ✅ |
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

### 2-7. チャンネル・通知
| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| Discord | ✅ | ✅ | serenity ライブラリ |
| Telegram | ✅ | ❌ | 未実装 |
| Slack | — | ❌ | 計画なし |
| geminiclaw_post_message ツール | ✅ | N/A | RustyClaw は返答テキストを自動投稿 |
| geminiclaw_list_channels ツール | ✅ | N/A | 同上 |

### 2-8. 外部ツール連携
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
| Obsidian 参照 | Obsidian MCP (SSE) | obsidian_search ✅ / obsidian_read_note ✅ | Rust 直実装 |
| Obsidian 書き込み・追記 | Obsidian MCP (SSE) | ✅ | obsidian_write_note として LLM に公開 |
| **Obsidian Dataview クエリ** | Obsidian MCP (SSE) | ❌ | 未実装 |
| **全文検索（Memory）** | qmd_query / qmd_get | ✅ | memory_search として LLM に公開 |
| **天気** | 天気ツール | ✅ | `yolp_weather`（Open-Meteo 60分降水予報、Phase 32） |

### 2-9. Rate Limit 対策
| 機能 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| 429 検知・バックオフ | ✅ | ✅ | |
| GLOBAL_COOLDOWN（CF 対応） | — | ✅ | OpenAiCompatProvider で 429 時にセット |
| reset_after() CF RPM パース | — | ✅ | "too many requests" → デフォルト 60s |
| CF neurons 日次上限パース | — | ✅ | internalCode 4006 → 翌 09:00 JST まで待機 |
| CF neurons 使用量トラッキング | — | ✅ | `cf-ai-neurons` ヘッダー優先、不在時は `total_tokens` で代替計上。`~/.rustyclaw/neuron_usage.json` に UTC 日付単位で累積保存（Mutex で排他制御）|
| gmn_sem(1) 全直列化 | — | ✅ | |

### 2-10. 実行環境・デプロイ
| 項目 | GeminiClaw | RustyClaw | 備考 |
|---|---|---|---|
| 実行環境 | Node.js / TypeScript | Rust (aarch64) / RPi4 | |
| LLM プロバイダ | Gemini CLI (gmn) | Cloudflare Workers AI | |
| 外部プロセス常駐 | Node.js MCP × 複数 | **なし** ✅ | 全ツール Rust 直実装または gws subprocess |
| クロスコンパイル | — | cross / aarch64-linux-gnu ✅ | scripts/cross-build.sh |
| systemd サービス | — | ✅ | /etc/systemd/system/rustyclaw.service |
| OAuth スコープ削減 | — | ✅ | 12 → 7 スコープ（不要スコープ削除済み） |

---

## 3. ソースコードレベルの比較詳細

### ① ContextBuilder (システムプロンプト構築)
*   **GeminiClaw (`src/agent/context-builder.ts`):**
    Gemini CLI の `@filename` 自動インポート仕様に準拠するため、`SOUL.md` や `MEMORY.md` などの参照を含む**完全に静的な `GEMINI.md`** を事前にディスクへ書き出し、実行毎の動的情報（trigger、history、directives など）のみを `-p` 引数にインジェクトする設計。
*   **RustyClaw (`crates/rustyclaw-agent/src/lib.rs`):**
    インプロセスで動的プロンプトを合成する。毎実行時に `SOUL.md`, `AGENTS.md`, `MEMORY.md`, `USER.md` を読み込み、`strip_comments` を通して `//` で始まるコメント行を除去した上でメモリ上で結合し、直接 LLM API の `system` メッセージに格納する。
    また、Heartbeat 実行時には専用 of 軽量コンテキスト（`SOUL.md`, `MEMORY.md`, `HEARTBEAT.md` のみ）を構築する `build_heartbeat_context` が明確に独立した関数として定義されている。

### ② Session Continuation (日またぎ文脈引き継ぎ)
*   **GeminiClaw (`src/agent/session/continuation.ts`):**
    前日の `.md` サマリーファイルの中身を**正規表現でパース**し、構造化データ（TL;DR テキストと、`## Topics` 内の `- **トピック**: 要約`）としてオブジェクトに分解した上で、再度組み立てて注入する。
    ```typescript
    const tldrMatch = content.match(/## TL;DR\n([\s\S]*?)(?=\n## |$)/);
    const topicsMatch = content.match(/## Topics\n([\s\S]*?)(?=\n## |$)/);
    if (topicsMatch?.[1]) {
        for (const m of topicsMatch[1].matchAll(/^- \*\*(.+?)\*\*:\s*(.+)$/gm)) {
            topics.push({ topic: m[1], summary: m[2] });
        }
    }
    ```
*   **RustyClaw (`crates/rustyclaw-agent/src/lib.rs` : `get_session_continuation_context`):**
    正規表現によるパースは行わず、前日の個別セッションサマリー（または `daily-summary.md`）の**全体テキストを丸ごとそのまま読み込んで結合**する。
    ```rust
    if specific_summary_path.exists() {
        if let Ok(c) = std::fs::read_to_string(&specific_summary_path) {
            summary_content = c;
        }
    }
    ```
    これにより、LLM のサマリー出力フォーマットが微妙に揺れた場合でもパースエラーにならず文脈引き継ぎ自体が成功する、シンプルかつ頑強な設計になっている。

### ③ 圧縮アルゴリズム (`truncateWithContext`)
*   **GeminiClaw (`src/agent/context-builder.ts`):**
    文字数 (`maxChars`) を基準にし、頭 70%、尾 20%、省略マーク 10% の `string.substring()` で単純に切り詰める。
*   **RustyClaw (`crates/rustyclaw-agent/src/lib.rs` : `truncate_70_20`):**
    バイト数 (`max_bytes`) を基準にする。Rust の UTF-8 文字列境界を考慮した slice 処理を行うことで、マルチバイト文字（日本語）が境界で破損してパニックするのを防ぎつつ、厳密なバイト単位制御を行っている。
    ```rust
    fn truncate_70_20(content: &str, max_bytes: usize) -> String {
        if content.len() <= max_bytes { return content.to_string(); }
        let head_end = (max_bytes as f64 * 0.7) as usize;
        let tail_len = (max_bytes as f64 * 0.2) as usize;
        let tail_start = content.len().saturating_sub(tail_len);
        let omitted = content.len() - head_end - tail_len;
        format!(
            "{}\n\n[...{} bytes omitted...]\n\n{}",
            &content[..head_end], // UTF-8境界安全性に配慮が必要
            omitted,
            &content[tail_start..],
        )
    }
    ```

---

## 4. 移植済み機能（ギャップの解消）

### 【移植完了】Proactive Posts 注入
Heartbeat が自発的に送った Discord 等のメッセージを、翌日の会話セッション開始時に「会話履歴外の自分の発言」としてシステムプロンプトに差し戻す機能を完全実装しました。これにより、自分が自発的に発言した内容の文脈を次の対話で正しく認識できるようになりました。

#### 実装仕様:
1.  **対象コード**: `crates/rustyclaw-agent/src/lib.rs` の `execute` および `execute_with_tools` 内の `process_proactive_posts` ヘルパー関数。
2.  **スキャンの仕組み**:
    *   `SessionLogger::load_history` でセッション履歴（JSONL）を読み込む。
    *   `trigger == "proactive"` (自発投稿) かつ、最後にユーザーが発言したタイムスタンプ以降に記録されたエントリーをフィルタリングして会話履歴（`history.messages`）から除外（二重参照防止）。
    *   抽出した直近 5 件の発言を以下の Markdown フォーマットで `system_context` に動的に注入する。

```markdown
### Your Previous Posts in This Channel
You posted these messages (not in your conversation history):
- [YYYY-MM-DD HH:MM:SS]: (自発発言内容の先頭300文字...)
```

### 【並行制御完了】インプロセス非同期パスロックと並行数 4 へのスケーリング (Phase 2)
同一セッション workspace ファイル（`MEMORY.md`, `USER.md` 等）への並列アクセスによる競合や上書き破損を防止するため、`rustyclaw-storage` に `once_cell` を用いた「インプロセス非同期パスロック (`PATH_LOCKS`)」を実装しました。読み込みは並行（Shared）、書き込み（排他）は自動で `atomic_write` 内にて制御されます。この安全性確保により、Gateway 内のグローバル実行セマフォ容量を `1` から `4` に安全に拡張しました。

### 【シークレット疎結合化】$vault: 動的環境変数インジェクションと多層フォールバック
カスタムスクリプト（Garminバイタル取得、KaraKeepクリーンアップ等）から、環境依存の平文シークレット解決ロジックを完全に排除し、安全なシークレット注入システムを Rust 側で実装しました。

#### 実装仕様:
1. **対象コード**: `crates/rustyclaw-tools/src/lib.rs` 内の `WorkspaceExecuteScriptTool::execute()`
2. **解決システムと多層フォールバック**:
   * `$vault:key_name` プレフィックスを検知すると、以下の優先順位で動的にシークレットを解決・デコードして環境変数に注入する。
     1. 暗号化 Vault データベース (`vault.enc`) のロードと復号。
     2. 平文の `vault.json` からのロード（下位互換フォールバック）。
     3. UNIX環境変数（キー名を大文字化・アンダースコア化したもの、例: `TEST_VAL`）からのロード。
     4. いずれも解決できない場合は安全にエラーを返して即時中断（方式Aフェイルファスト）。
   * これにより、シークレットが設定されていない環境でも安全にフォールバックし、かつディスク上に平文ファイルがなくても安全に動作します。
3. **スキルのリファクタリング**:
   * `vitals-coach` や `karakeep` スキルのローカルスクリプトから Python などの不要なシークレットパースコードを完全削除。
   * スキル定義 `SKILL.md` の `env` パラメータとして `$vault:homeassistant-token` などのバインドを定義し、トークン効率を高めつつ完全なポータビリティ化を達成。

### 【LLM 耐障害性】Per-provider クールダウン (Phase 24)
GeminiClaw には 429 エラー発生時のバックオフ機構が実装されている。RustyClaw では `GLOBAL_COOLDOWN` 変数による全体停止方式から、プロバイダ単位のクールダウン管理へ移行完了。

#### 実装仕様:
1. **対象コード**: `crates/rustyclaw-providers/src/lib.rs`
2. **変更内容**:
   - `PROVIDER_COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>>` による per-provider 管理に変更。
   - `set_provider_cooldown_from_error()` / `set_provider_cooldown()` / `provider_cooldown_remaining()` の3関数を実装。
   - `GLOBAL_COOLDOWN` static 変数と旧関数群をすべて削除。
3. **ダッシュボード連携**: `health.rs` の PROVIDER COOLDOWNS パネルで残り時間を `XdXXh` / `XhXXm` / `XXmXXs` / `XXs` の段階フォーマットで表示。

---

## 5. 移植・改修実績

本調査結果に基づき、以下のタスクを完了しました：

1.  **Proactive Posts 注入の実装** (`crates/rustyclaw-agent/src/lib.rs`) ✅ 完了
    *   自発メッセージの差分ロードおよびプロンプトへの差し戻しロジック、およびユニットテストを追加。
2.  **heartbeat-digest.md の増分ロード不全・ツール対話抽出バグの改修** (`crates/rustyclaw-gateway/src/heartbeat.rs`) ✅ 完了
    *   ログ差分増分スキャンの境界タイムスタンプのバグ、およびツール呼び出し中の最終テキスト返答の抽出ロジックを修正。動的なLocalタイムスタンプ表示および構造化マークダウンヘッダー出力を追加。
3.  **tantivy 検索および Obsidian 書き込みツールの LLM 公開** (`crates/rustyclaw-tools/src/lib.rs`) ✅ 完了
    *   `MemorySearchTool` (tantivy) と `ObsidianWriteTool` の実装とツール登録、およびテストスイートを完備。
4.  **インプロセス非同期パスロックの導入と並行スロット 4 への拡張** (`crates/rustyclaw-storage/src/lib.rs`, `crates/rustyclaw-gateway/src/lib.rs`) ✅ 完了
    *   共有ファイルの並列アクセスを調停する非同期 `RwLock` マップを実装し、Gateway セマフォを容量 4 に拡張、デプロイ完了。

---

## 6. GeminiClaw 参照ソースマッピング

実装時にコード設計やロジック詳細で迷った際、元となった GeminiClaw リポジトリの以下のファイルを参考にします。

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

