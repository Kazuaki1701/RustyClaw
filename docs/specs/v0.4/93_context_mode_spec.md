# context-mode MCP サーバー 機能・性能仕様

> [!NOTE]
> **ステータス**: `[ACTIVE]`
> **最終更新日**: 2026-06-13
> **出典**: https://github.com/mksglu/context-mode（README 調査・2026-06-13）
> **関連**: [`docs/adr/006-context-mode-integration-scope.md`](../adr/006-context-mode-integration-scope.md) / [`docs/specs/v0.3/10_mcp.md`](../v0.3/10_mcp.md)

---

## 1. 概要・設計思想

context-mode は「コンテキスト問題」を解決するための MCP サーバー。
大型ツール出力（56 KB の Playwright スナップショット、59 KB の GitHub Issues、45 KB のログ等）が
コンテキスト窓を急速に消費し、会話コンパクション後に状態を忘れる問題に対処する。

**4 つのコア機構:**

1. **サンドボックス実行** — コードを隔離実行し、stdout のみをコンテキストに注入（56 KB → 299 B、99% 削減）
2. **セッション永続化** — SQLite にファイル編集・git 操作・タスク・エラーを記録。コンパクション後も FTS5 検索で関連状態を復元
3. **コードファースト分析** — ファイルを直読みするかわりにスクリプトで結果だけを出力（100× コンテキスト節約）
4. **柔軟な出力** — データの格納は構造化しつつ、モデルの出力スタイルは自由

**ライセンス**: Elastic License 2.0（ソース公開。マネージドサービスとしての再販売・ライセンス表示削除は禁止）

---

## 2. MCP ツール一覧（11 種）

### 2.1 サンドボックス実行系

| ツール | 概要 |
|---|---|
| `ctx_execute` | 12 言語（JS / TS / Python / Shell / Ruby / Go / Rust / PHP / Perl / R / Elixir / C#）でコードを隔離実行。stdout のみ返却。gh / aws / gcloud / kubectl / docker 等の認証 CLI はパススルーで環境変数を継承 |
| `ctx_execute_file` | ファイルをサンドボックス内で処理。生コンテンツをコンテキストに露出しない（45 KB → 155 B） |
| `ctx_batch_execute` | 複数コマンド・検索クエリを 1 呼び出しに集約。`concurrency: 1-8` で I/O バウンドバッチを並列実行（986 KB → 62 KB） |

### 2.2 知識ベース系

| ツール | 概要 |
|---|---|
| `ctx_index` | Markdown をヘッディング単位でチャンク化（コードブロック保護）し SQLite FTS5 に格納。タイトル・見出しは BM25 スコアを **5× 重み付け** |
| `ctx_search` | デュアル戦略ランキング: Porter stemming（語形変化対応）＋ trigram 部分一致 を **Reciprocal Rank Fusion（RRF）** でマージ。複数語の近傍リランキング、Levenshtein 誤字補正あり |
| `ctx_fetch_and_index` | URL を取得→HTML→Markdown 変換→チャンク→インデックス登録。TTL キャッシュ（デフォルト 24h、per-call 上書き可）。14 日後に自動パージ。`force: true` でキャッシュ強制更新 |

### 2.3 ユーティリティ系

| ツール | 概要 |
|---|---|
| `ctx_stats` | ツール別コンテキスト削減量・呼び出し回数・キャッシュヒット率等のセッション統計を表示 |
| `ctx_doctor` | ランタイム・フック・FTS5 可用性・プラグイン登録・バージョン互換性を全 15 プラットフォームで検証 |
| `ctx_upgrade` | GitHub から最新リリースを取得、ネイティブバインディングをリビルド、フックを再構成、キャッシュコンテンツを移行 |
| `ctx_purge` | 知識ベース DB のインデックス済みコンテンツを完全削除 |
| `ctx_insight` | ローカル Web UI のアナリティクスダッシュボード。90 メトリクス・37 インサイトパターン・4 複合スコア（生産性・品質・委任度・コンテキスト健全性）を 23 イベントカテゴリで表示 |

---

## 3. セッション継続メカニズム

5 種のフックでセッション状態を自動キャプチャする。

| フック | タイミング | 役割 |
|---|---|---|
| `PreToolUse` | ツール実行前 | 危険なコマンド（直接 `curl`・ファイル読み・bash）をインターセプトしサンドボックスへリダイレクト |
| `PostToolUse` | ツール実行後 | ファイル編集・git 操作・エラー・タスクを SQLite に記録 |
| `UserPromptSubmit` | ユーザー入力時 | ユーザーの判断・修正内容を記録 |
| `PreCompact` | コンテキストコンパクション直前 | 優先度階層付き XML スナップショット（≤ 2 KB）を生成 |
| `SessionStart` | セッション開始時 | スナップショットを復元し「Session Guide」（15 カテゴリ）を注入 |

**Session Guide の 15 カテゴリ**: 直近リクエスト / タスク / 計画 / 重要決定 / 変更ファイル / 未解決エラー / 制約 / ブロッカー / git 操作 / プロジェクトルール / 使用 MCP ツール / サブエージェントタスク / 使用スキル / 棄却アプローチ / 外部参照

**スナップショット優先度**: アクティブファイル・タスク・ルール（CLAUDE.md 等）・決定・エラーが高優先。空間不足時はインテント・MCP カウント等が削除される。

---

## 4. 知識ベース実装詳細

| 要素 | 実装 |
|---|---|
| ストレージ | SQLite FTS5 仮想テーブル（Porter stemming トークナイザー） |
| ランキング | BM25（TF-IDF ベース）。タイトル/見出し 5× 重み付け |
| マージ | Reciprocal Rank Fusion（Porter 結果 + trigram 結果） |
| 語形変化対応 | Porter stemming（"caching" → "cached"/"caches" を同一視） |
| 部分一致 | trigram substring（"useEff" → "useEffect" を発見） |
| 誤字補正 | Levenshtein 距離で補正後に再検索 |
| 近傍ランキング | 複数クエリ語が隣接して現れる結果をブースト |
| 結果形式 | クエリマッチ周辺のコンテンツウィンドウを返却（単純な切り詰めでない） |
| キャッシュ | `~/.context-mode/content/` 配下にプロジェクト別保存。14 日で自動クリーンアップ |

---

## 5. ベンチマーク

| シナリオ | Raw サイズ | サンドボックス後 | 削減率 |
|---|---|---|---|
| Playwright スナップショット | 56.2 KB | 299 B | **99%** |
| GitHub Issues（20 件） | 58.9 KB | 1.1 KB | **98%** |
| アクセスログ（500 リクエスト） | 45.1 KB | 155 B | **100%** |
| React ドキュメント | 5.9 KB | 261 B | **96%** |
| CSV（500 行） | 85.5 KB | 222 B | **100%** |
| Git log（153 コミット） | 11.6 KB | 107 B | **99%** |
| リポジトリ調査（サブエージェント） | 986 KB | 62 KB | **94%** |

**セッション持続時間**: Raw アプローチ → 約 30 分でコンテキスト枯渇。context-mode 使用時 → **約 3 時間**（315 KB → 5.4 KB）

---

## 6. プラットフォーム対応（フックカバレッジ）

| プラットフォーム | PreToolUse | PostToolUse | SessionStart | PreCompact | UserPromptSubmit |
|---|:---:|:---:|:---:|:---:|:---:|
| **Claude Code** | ✅ | ✅ | ✅ | ✅ | ✅ |
| Gemini CLI | ✅ | ✅ | ✅ | ✅ | — |
| VS Code Copilot | ✅ | ✅ | ✅ | ✅ | — |
| JetBrains Copilot | ✅ | ✅ | ✅ | ✅ | — |
| Cursor | ✅ | ✅ | — | — | — |
| OpenCode | Plugin | Plugin | Surrogate | Plugin | Plugin |
| Codex CLI | ✅ | ✅ | ✅ | ✅ | ✅ |

Claude Code は全 5 フックに対応（最もフルサポート）。

---

## 7. セキュリティ

- **ローカル完結** — クラウド同期・テレメトリ・アカウント不要。SQLite はホームディレクトリに保存
- **権限継承** — Claude Code の settings.json（deny/allow パターン）をサンドボックス内でも適用。deny が always override
- **ネットワーク制限** — `ctx_fetch_and_index` は非 HTTP(S) スキーム・AWS/GCP/Azure メタデータエンドポイント（169.254.169.254）・マルチキャスト（224.0.0.0/4）・予約済みアドレスをブロック。ループバック/RFC1918 はデフォルト許可。`CTX_FETCH_STRICT=1` で共有環境向けにプライベートアドレスもブロック可
- **クレデンシャル秘匿** — MCP ツール引数の authorization / token / secret / password / api_key / cookie フィールドを保存前にマスク

---

## 8. RustyClaw での統合状況（2026-06-13 時点）

| 機能 | 利用状況 | 備考 |
|---|---|---|
| `ctx_execute` | ✅ Heartbeat ToolRegistry に登録（LLM 自律実行） | bwrap サンドボックスで実行 |
| `ctx_search` | ✅ Chat・Heartbeat・Patrol 全目的で利用 | Dynamic Skill Selection・Memory RAG・エピソード相関検索（Phase 52） |
| `ctx_index` | ✅ cron 完了後・MEMORY.md チャンク・daily-summary に利用 | フラッシュ後の再インデックスも対応（Phase 52） |
| `ctx_patch` | ✅ Heartbeat ToolRegistry に登録 | LLM 自律利用 |
| `ctx_batch_execute` | ❌ 未使用 | — |
| `ctx_execute_file` | ❌ 未使用 | — |
| `ctx_fetch_and_index` | ✅ Topic Patrol で外部 URL 事前キャッシュに利用 | Phase 52-4 で追加 |
| `ctx_stats` / `ctx_doctor` 等 | ❌ 未統合（Claude Code プラグインで利用可） | — |

### Phase 52 完了内容（2026-06-13）

- **Dynamic Skill Selection**: ユーザー発話で `ctx_search` し BM25 上位スキルのみ inject（cron 除外）
- **Memory RAG**: MEMORY.md をセクション単位でチャンク分割し `ctx_index` 登録。チャット時は `ctx_search` で関連チャンクのみ動的注入
- **Topic Patrol RAG**: `ctx_fetch_and_index` で外部 URL を事前キャッシュし、巡回時は必要部分のみ参照
- **daily-summary エピソード記憶**: `cron:daily-summary` 完了後に `[daily-summary:{date}]` タグ付きで自動登録
- **バイタル相関検索**: Heartbeat digest に睡眠・疲労キーワードが含まれる場合のみ過去の類似エピソードを注入

**ギャップ詳細**: [`docs/adr/006-context-mode-integration-scope.md`](../adr/006-context-mode-integration-scope.md) を参照。
