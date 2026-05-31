# RustyClaw システム改善計画書 (RustyClaw System Improvement Plan)

本計画書は、`rustyclaw.log.2026-05-30` の点検で明らかになった課題（外部APIの接続・スキーマ検証失敗、レート制限超過、およびデーモンのライフサイクル安定性）に対する、具体的かつ実用的な改善ロードマップを提示します。

---

## 1. 識別された主要課題と影響分析

| 課題ID | 発生現象 | 根本原因 | システムへの影響 | 優先度 |
| :--- | :--- | :--- | :--- | :---: |
| **ISSUE-01** | `obsidian_search` および `gws_gmail_list_messages` 呼び出し時の `400 Bad Request` | LLM が数値パラメーターを `"10"` のように文字列として出力し、APIゲートウェイ側の厳格な型検証（`type: integer`）に衝突した。 | エージェント実行の異常終了（自動タスクのクラッシュ） | **高 (High)** |
| **ISSUE-02** | `http-dashboard` セッションにおける `413 Payload Too Large` | Groq (`llama-3.1-8b-instant`) の無料枠におけるトークン制限（6,000 TPM）に対し、リクエストが7,806トークンに達した。 | LLM呼び出し自体の失敗（ダッシュボード更新の停止） | **高 (High)** |
| **ISSUE-03** | 頻繁な `SIGTERM` によるデーモンの再起動サイクル | watchdog通知スレッドと重いAPI初期化スレッドの競合、または外部オーケストレーターによるヘルスチェック失敗の可能性。 | 稼働率の低下、初期化時の一時的な未反応期間の発生 | **中 (Medium)** |

---

## 2. 改善プラン詳細

### 🛠️ 対策1: ツールパラメータの柔軟化と自動型キャストの導入 (ISSUE-01 の解決)

LLMの出力からダブルクォートを取り除くのは完璧には制御できないため、**「スキーマの許容度を広げ、コード側で安全に型キャストする」** アプローチを採用します。

#### 具体的なアプローチ (2つの選択肢)

* **アプローチA: JSON Schema を `"type": "string"` に統合する (最も堅牢)**
  * 各ツールのパラメータ定義で、数値型のパラメーターを `"type": "string"` に設定し、説明に `"(integer represented as string, e.g., '10')"` と記述します。
  * これにより、LLMが `"10"` と出力しても `10` と出力しても（一部のゲートウェイは自動で文字列化します）、APIゲートウェイのバリデーションを100%通過します。
  * ツール側の `execute` 内で、受け取った `Value` を数値に安全にパースします。
  
  ```rust
  // 例: `gws_gmail_list_messages` のパース処理の堅牢化
  let max_results = match &args["max_results"] {
      Value::Number(n) => n.as_u64().unwrap_or(10),
      Value::String(s) => s.parse::<u64>().unwrap_or(10),
      _ => 10,
  };
  ```

* **アプローチB: グローバルなパラメータ・アサーション & コエーション（型強制）層の追加**
  * エージェントが LLM からツール呼び出しを受信した直後（かつプロバイダーに投げる前、またはローカル検証時）、ツールが期待するスキーマ情報（`integer`）を元に、文字列の `"10"` を数値の `10` に自動変換してパースするミドルウェア層を実装します。

---

### 🛠️ 対策2: コンテキスト制御の高度化とトークン予算管理の強化 (ISSUE-02 の解決)

Groq の 6,000 TPM 制限に対応するため、よりスマートなトークン管理を導入します。

1. **トークン推定ロジックの改善 (`ConversationHistory::estimate_tokens`)**
   * 現在のロジックはメッセージの `content` 文字数のみをカウントしていますが、システムプロンプトやツール定義（`ToolDef` の JSON スキーマ）が占めるトークン数（約 1,500〜2,500 トークン）が計算から漏れています。
   * **改善案:** LLMプロバイダー呼び出しの直前に、**「システムプロンプト ＋ ツールスキーマ ＋ 履歴 ＋ ユーザー入力」** の総推定トークン数を計算するロジックを統合します。

2. **動的履歴圧縮制限の適用**
   * `compact_if_needed(1500)` の固定値ではなく、モデルごとの TPM に応じた制限値を動的に適用します。
   * 例えば、TPMが 6,000 のモデル（`groq-llama-8b` など）に対しては、履歴上限を `800` に設定し、システムプロンプトやツールスキーマが入っても安全に 4,000 トークン未満に収まるように制御します。

3. **セッションに応じたモデルの最適ルーティング**
   * 大量のコンテキストを必要とする `http-dashboard` セッションに対しては、TPM 制限が 12,000 と2倍の容量を持つ `groq-llama-70b` (モデル: `llama-3.3-70b-versatile`) をルーティングするように `config.json` を再調整します。

---

### 🛠️ 対策3: デーモン起動の非同期化と Watchdog の最適化 (ISSUE-03 の解決)

頻繁な再起動サイクルによる接続断を防ぐための設計見直しを行います。

1. **GWS および Discord 接続処理の非同期バックグラウンド化**
   * 起動シーケンスにおいて、Google API のカレンダーメタデータ取得や Discord Gateway への接続といったブロッキングが発生しうる処理を、メインの初期化スレッドから完全に `tokio::spawn` で切り離します。
   * これにより、メインスレッドが即座に起動完了状態に移行し、Systemd の watchdog 通知タスクが遅延なく開始されるため、起動遅延による watchdog 強制終了を防止できます。

2. **ヘルスチェックサーバーの強化**
   * `http://0.0.0.0:8080` の `/health` エンドポイントが、外部連携サービスの応答遅延（GWSやObsidianのAPI遅延）に巻き込まれてタイムアウトしないよう、ヘルスステータスをキャッシュ化または非同期判定にします。

---

## 3. 推奨実施ロードマップ (フェーズ分け)

### 短期アクション (Phase 1: 即時実行可能 - 安全性の確保)
* 🛠️ **対象ツール:** `obsidian_search`, `gws_gmail_list_messages`
* **作業内容:** 数値パラメータを受け取るツールすべてにおいて、スキーマを `"type": "string"` もしくはコエーション対応に変更し、`execute` 側で `Value::String` / `Value::Number` 両対応の安全パース処理を書き込む。
* **目標:** `400 Bad Request` の完全な撲滅。

### 中期アクション (Phase 2: 安定性の向上)
* 🛠️ **対象コンポーネント:** `rustyclaw-agent`, `rustyclaw-config`
* **作業内容:**
  1. `http-dashboard` の使用モデルを `groq-llama-70b` (`llama-3.3-70b-versatile`) にアップグレードし、TPM クォータに余裕を持たせる。
  2. 総トークン数を考慮した `estimate_tokens` の計算見直しと、モデルに応じた履歴圧縮しきい値の動的適用。
* **目標:** `413 Payload Too Large` の発生防止と最適な推論リソースの配分。

### 長期アクション (Phase 3: アーキテクチャの堅牢化)
* 🛠️ **対象コンポーネント:** `rustyclaw-gateway`
* **作業内容:** 外部ネットワーク依存処理（GWS API、Discord Shard Manager）の初期化フローの完全非同期化。
* **目標:** watchdog 再起動ループの解消と、連続稼働時間の向上。

---

## 4. 追加課題（2026-05-31 セッションで洗い出し）

LM Studio ローカル Gemma 導入・NVIDIA ドライバ更新・cron 調査の過程で、chat 応答とログから新たに識別した課題。

### 4-1. エージェント挙動（chat 応答）の課題

| 課題ID | 発生現象 | 根本原因（推定） | 影響 | 優先度 |
| :--- | :--- | :--- | :--- | :---: |
| **ISSUE-04** | 自己状態の質問（cron 予定一覧）に対し回答が不正確。`cron.json` の有効6件中、正答は karakeep-recommendation(04:45) の1件のみ。Vitals を「cron とは異なる別システム」と誤説明、daily-briefing/topic-patrol/karakeep-cleanup を欠落。 | ツール（`workspace_read` 等）で `cron.json` を読まず、MEMORY.md 等の記憶ベースで回答。事実確認のためのツール活用が促されていない。 | ユーザーへの誤情報提供、自己状態の不可視化 | **高** |
| **ISSUE-05** | 「shell コマンドは LLM だから原理的に実行できない」と回答し、保有する16ツールに一切言及せず過度に謝罪。 | capability の誤った自己認識（プロンプト/SOUL.md 起因）。「ツール未登録」と「原理的不可能」を混同。 | 能力の過小申告、ユーザー体験の低下 | **中** |

### 4-2. cron / ツール機能の課題

| 課題ID | 発生現象 | 根本原因 | 影響 | 優先度 |
| :--- | :--- | :--- | :--- | :---: |
| **ISSUE-06** | cron ジョブ `karakeep-cleanup`(04:00) / `karakeep-recommendation`(04:45) が機能していない疑い。prompt が `bash scripts/501_karakeep-cleanup.sh` / `502_karakeep-tag-items.sh` の実行を前提。 | (a) 汎用 shell 実行ツールが未登録（`process::Command` は gws CLI 専用）、(b) スクリプト本体が dev・rp1 双方に存在しない。 | 定期タスクの無言の失敗（recommendation は `karakeep_tag_bookmark` で代替可だが prompt は bash 指定） | **高** |
| **ISSUE-07** | 任意/制約付き shell 実行手段が存在しない。 | shell 実行ツール未実装。 | bash 前提の自動化が組めない。※追加する場合は許可リスト/サンドボックス必須（任意 shell は重大リスク） | **中** |

### 4-3. LM Studio / ローカルモデル運用の課題

| 課題ID | 発生現象 | 根本原因 | 影響 | 優先度 |
| :--- | :--- | :--- | :--- | :---: |
| **ISSUE-08** | `n_keep 4735 >= n_ctx 4096` で 400 エラー多発。LM Studio の `loaded_context_length` が既定 4096 のまま。 | LM Studio はモデル再ロードのたびに既定 context 長に戻る。RustyClaw 側 `context_window` 値（当初128k）と実ロード長(4096)の整合が手動。 | 起動プロンプト超過で全推論が即失敗 | **高** |
| **ISSUE-09** | debug 構成で rp1 が dev 機の LM Studio(192.168.1.110) に依存。dev 機リブート/LM Studio 停止で rp1 応答不可。 | ローカルモデルを別ホスト依存で本番運用。フェイルオーバー無し。 | 可用性低下（単一障害点） | **中** |
| **ISSUE-10** | ローカル Gemma(gemma-4-e4b) の応答品質・ツール活用がクラウドモデル比で劣る可能性（ISSUE-04 の誤答の一因か）。 | 小型モデルの能力限界。tool-use/事実確認の精度。 | デバッグ時の挙動が本番(クラウド)と乖離 | **低（要観察）** |

### 4-4. 設定管理・運用の課題

| 課題ID | 発生現象 | 根本原因 | 影響 | 優先度 |
| :--- | :--- | :--- | :--- | :---: |
| **ISSUE-11** | `config.debug.json` と `config.release.json` の `model_list` が重複。共通変更（モデル追加等）が二重メンテ。 | 切替を最小実装（ファイル複製＋symlink）で導入したため DRY でない。 | 設定ドリフト・反映漏れリスク | **中** |
| **ISSUE-12** | `config.json` symlink が `config.debug.json` を指したまま git コミット済。 | デバッグ状態のままコミット。 | 本番デプロイ時にデバッグ構成が混入する事故リスク | **中** |
| **ISSUE-13** | 一時ワークスペース(`/tmp/tmp.*`)での CLI 実行時に `Failed to read context file (SOUL/AGENTS/MEMORY/USER.md)` WARN。 | fail-open 仕様（実害なし）。一時 WS に context ファイル無し。 | ログノイズのみ。WARN→debug 降格 or テンプレ配置で抑制可 | **低** |
| **ISSUE-14** | 過去ログで gws `Failed to fetch calendar info` WARN が多発（×10）。現状は解消。 | 過去の gws 設定/接続問題。 | 現状再発なし。要経過観察 | **低** |
| **ISSUE-15** | rp1 に `sqlite3` 未導入で state DB の点検が手動でできない。 | 運用ツール不足。 | 診断性の低下（minor） | **低** |
| **ISSUE-16** | LM Studio の埋め込みモデル(`nomic-embed-text`) が `not-loaded`。 | JIT ロード依存。 | `memory_search` 初回の遅延 or 失敗の可能性 | **低** |

### 4-5. Dashboard（LANE QUEUE）の課題

| 課題ID | 発生現象 | 根本原因 | 影響 | 優先度 |
| :--- | :--- | :--- | :--- | :---: |
| **ISSUE-17** | Dashboard「LANE QUEUE」パネルが常に空に見える（ユーザー報告）。※バックエンドはライブ検証の結果**正常動作**（実行中は Executing item を返す）。 | (a) 単一ユーザー＋タスク短命＋`gmn_sem` capacity=1 の直列処理で、アイドル時間が支配的。(b) bus→worker→permit の流れ上、permit 取得まで QUEUE_STATE に積まれず `Waiting` 状態がほぼ出現しない（`queue_depth` も常に0）。 | 「キューイング可視化」の本来価値（待ち行列・滞留把握）が現構成で発揮されず、ユーザーには故障に見える | **低〜中** |
| **ISSUE-18** | LANE QUEUE に「**cron でスケジュール待機中（未発火）の案件**」を表示したかったが未実装。現状は発火後の実行中タスクのみ表示し、04:45 karakeep / 05:00 daily-briefing 等の予定は発火するまで一切現れない。 | 動的ローダーは60秒ごとに `現在時刻 == expression` を単純比較して発火するのみ。**次回実行時刻を算出・公開する仕組みが存在しない**。QUEUE_STATE は発火時にしか積まれない設計。 | 「これから何がいつ動くか」を一覧できず、当初意図したスケジュール可視化が未達 | **中** |

**ISSUE-17 改善案:** ①直近完了タスクの履歴を数件表示し常に情報がある状態にする ②`Waiting` を確実に記録するため bus 受信直後（permit 取得前）に enqueue する ③アイドル時の文言を「待機中（正常）」等に変更し故障でないことを明示。

| **ISSUE-19** | Dashboard「LLM REQUEST」パネルが毎回同じ内容に見え、変化が分からない（ユーザー報告）。 | `health.rs:677` が `txt.substring(0,4000)` で**先頭4000文字のみ表示**。リクエスト全文(≈19,498字)の先頭は static な system prompt(SOUL.md＋ツール定義)で毎回同一。可変部（会話履歴・最新発話）は末尾にあり truncate で消える。 | LLM リクエスト inspector が実質機能していない（デバッグ価値の喪失） | **中** |

**ISSUE-19 改善案（1行修正）:** REQUEST パネルは末尾を残す `'...(truncated head)\n'+txt.slice(-4000)`、または「先頭800＋末尾3200」のハイブリッド表示にする。RESPONSE パネル(先頭3000)は通常は応答が先頭=本文だが、長文応答や tool_calls を含む場合に末尾が切れるため、REQUEST と同等の表示制御（末尾保持 or スクロール可）を一貫して適用する。

| **ISSUE-20** | LLM REQUEST **および RESPONSE** に「全 LLM API の送受信内容」を表示したいが、一部しか捕捉していない（ユーザー要望）。CHAT 指示内容や memory/summary 用途の呼び出しが反映されない。 | dump が**エージェント層**で `last_request.json`/`last_response.json` を**単一上書き**。`dump_request`(7呼出中4) と `dump_response`(同4) ともに `execute`/`execute_stream`/`execute_with_tools`/`execute_heartbeat` のみ。`flush_memory`(memory)・`generate_session_summary`(summary) は req/res とも未 dump。連続呼び出しは上書きで消失し2秒ポーリングが取りこぼす。実プロバイダ層には dump 無し。 | LLM I/O inspector が全体像を反映せず、デバッグ・監査に使えない | **中** |

**ISSUE-20 改善案（req/res 全件捕捉）:**
1. **dump をプロバイダ層へ集約** — `OpenAiCompatProvider::complete`/`complete_stream` は**全 LLM API が必ず通る単一点**で、かつ「入力 messages(=request)」と「戻り値 LlmResponse(=response)」を**同一地点でペア保持**できる。ここで dump すれば memory/summary/tools/heartbeat/cron/chat の req/res を100%・メンテ不要で捕捉。エージェント層の `dump_request`/`dump_response`(各4箇所) は撤去。
2. **単一上書き → リングバッファ** — 直近N件を `timestamp ＋ purpose ＋ model ＋ request ＋ response` のペアで保持し、ダッシュボードはリスト/ストリーム表示（req/res を対で確認可能）。
3. ISSUE-19（表示の truncate）と併せて REQUEST/RESPONSE 両パネルに適用。
※ purpose（chat/memory/summary/tools/heartbeat/cron）と model をラベル付けすると、どの用途でどのモデルに何を送り何が返ったかが一目で分かる。streaming（`complete_stream`）は応答がチャンク逐次のため、完了時に結合して response を確定保存する点に注意。

**ISSUE-18 改善案（当初意図の実現）:**
1. **次回実行時刻の算出** — `cron.json` の各有効ジョブから次回発火時刻を計算（`cron`型=今日/明日の該当 HH:MM、`interval`型=`cron_last_run:<id>` DB値 + minutes）。
2. **API 公開** — `/api/queue` に `status:"Scheduled"` + `next_run_ms` でマージ、または新規 `/api/schedule` を追加。
3. **表示** — LANE QUEUE に `SCHED` ピル＋次回までのカウントダウンを追加し、実行中（WAIT/EXEC/COOL）と統合表示。
※ ISSUE-04（エージェントが cron 予定を把握できない）も、この次回実行時刻 API を内部ツール化すれば同時に解消可能。

---

## 5. 仕様: タブ式 LLM I/O インスペクタ（ISSUE-21）

ISSUE-19/20 を統合・発展させ、LLM REQUEST/RESPONSE を**単一ペイン＋用途別タブ**に再設計する（ユーザー仕様、2026-05-31 決定）。

### 5-1. 要件
- LLM REQUEST と LLM RESPONSE を**単一ペインに統合**（上下スタックで req/res をセット表示）。
- **用途（カテゴリ）別タブ**を設け、各タブはその用途の**最新の request/response ペア**を表示。
- 表示は ISSUE-19 の truncate 方針（末尾保持 or スクロール）、捕捉は ISSUE-20 のプロバイダ層集約を前提。

### 5-2. カテゴリ分類ルール（決定事項）
LLM 呼び出しを以下で分類してタグ付けする。**判定は session_id ＋ 応答内容**で行う:

| カテゴリ（タブ） | 判定条件 |
| :--- | :--- |
| `tools` | `execute_with_tools` のツールループ反復＝**応答に `tool_calls` を含む推論途中**の呼び出し（全 tool 利用元から集約） |
| `discord` | 最終応答（`tool_calls` なし）かつ session `discord-*` |
| `dashboard` | 最終応答かつ session `http-dashboard` |
| `briefing` | 最終応答かつ session `cron:daily-briefing` |
| `vitals` | 最終応答かつ session `cron:vitals-*` |
| `karakeep` | 最終応答かつ session `cron:karakeep-*` |
| `patrol` | 最終応答かつ session `cron:topic-patrol` |
| `heartbeat` | `execute_heartbeat` |
| `summary` | `generate_session_summary`（session-summary） |
| `daily` | `execute`（`cron:daily-summary`、purpose=default） |
| `memory` | `flush_memory` |

> 決定: ①ツールループ反復を `tools` として分離（最終応答は session 別タブへ）②`discord` purpose を session 種別で細分タブ化（dashboard/briefing/vitals/karakeep を分離）③`default`/daily-summary は `daily` 専用タブを追加。

### 5-3. 実装方針
1. **カテゴリ伝搬**: `CompletionOptions` に `category: Option<String>` を追加し、各エントリポイント（agent）で設定 → プロバイダ層 dump（ISSUE-20）へ伝搬。`tools` 判定のみ応答後（`tool_calls` 有無）に確定するため、dump はレスポンス受信後にカテゴリを最終決定して書く。
2. **保存**: カテゴリ別キーで**最新1ペアを保持**（例 `memory/debug/llm/<category>.json` に `{ts, model, request, response}`）。「用途別の最新」を自然表現。※ISSUE-20 のリングバッファはカテゴリ横断の時系列、本タブ表示はカテゴリ別最新、と役割分担。
3. **API**: `GET /api/llm/io`（全カテゴリの最新を一括 or 一覧）＋ `GET /api/llm/io?cat=<category>`（個別）。
4. **UI**: 既存の2パネル（reqPanel/resPanel）を1ペインに統合。上部にタブバー（カテゴリ）、本体は REQUEST→RESPONSE の縦スタック。ヘッダに `category / model / timestamp`。truncate は末尾保持（ISSUE-19）。空カテゴリは「未実行」を明示。

### 5-4. 留意点
- タブ数が多い（11）。session prefix → category のマッピングは**1関数に集約**し、cron 追加時に拡張しやすくする（将来は config 駆動も可）。
- `tools` は全 tool 利用セッション横断で集約されるため、「どの session 由来か」を item メタに残すと混乱しない。
- streaming（`complete_stream`）は応答結合後に確定保存（ISSUE-20 と同じ）。

---

## 5.5. CONCURRENCY パネル / gmn_sem capacity 引き上げ（ISSUE-22・保留）

**ステータス: 保留**（現状 capacity=1 で実害なし。capacity 引き上げを検討する段階で着手）。

### 現状仕様（CONCURRENCY パネルの数値）
- `gmn_sem` = 全 LLM 処理を直列化する統合セマフォ。`Semaphore::new(1)`（`lib.rs:780`, `:871`）で**容量1にハードコード**。user 対話・heartbeat・cron・flush_memory の全 LLM 起動がこの単一枠を奪い合い**直列実行**される。
- パネル表示: **Active**=`active/capacity`（active=capacity−available_permits=実行中数、capacity 固定1）、**Queue depth**=`Waiting` 件数、**Cooldown**/**Global limit**=グローバルレート制限の残秒（後述の重複あり）。

### 課題と引き上げ時の検討事項
| 観点 | 内容 |
| :--- | :--- |
| **capacity が config 化されていない** | コード内ハードコード（1）。引き上げ可能にするには `config.json` 等から設定注入できるようにする。 |
| **【重要】単一セマフォが2つの責務を兼務** | gmn_sem は「LLM 同時実行数の制限」と「MEMORY.md 等ワークスペースファイルへの**直列書き込み保護**」を兼ねている（容量1の本来目的、`lib.rs:130-132`）。**capacity>1 にすると並列書き込みによるデータ消失リスクが再燃**する。引き上げ時は「LLM 同時実行数」と「ワークスペース書き込みの直列化」を**別機構に分離**する必要がある（例: 書き込み専用 lock、または per-session 直列化＋ LLM 呼び出しは並列許可）。 |
| **Cooldown と Global limit の重複表示** | `cooldown_secs` と `global_cooldown` は同一値を別ラベルで表示しているだけ（`health.rs:164-165`）。片方に統合 or 役割を分ける。 |

※ 拡張可否の前提・経緯はメモリ `project_user_sem_concurrency`（user_sem/gmn_sem=1 採用の経緯と >1 拡張条件）参照。capacity 引き上げ時はそちらと本項をセットで検討する。

---

## 5.6. 仕様: DAILY TOKEN USAGE を時間別グラフ化（ISSUE-23）

### 現象と原因
- パネルが空表示。**ただしデータは存在**（rp1 実測: 今日 11 runs / 72,721 tokens、`record_usage` は LM Studio のトークン数も記録できている）。
- 原因: `get_usage_timeline` が `GROUP BY DATE(created_at)` の**日別集計**で、稼働が今日のみ＝**データ点が1個**。`renderTimeline` は `xStep=(W-40)/(rows.length-1)` で点数1だと面パスが幅ゼロになり**何も描画されない**。

### 要望
**時間別（時刻バケット）グラフ**にする。さらに**期間セレクタを `7D/30D/ALL` → `1D/7D/ALL` に変更**し、期間ごとに横軸バケット粒度を切り替える。

| ボタン | 期間 (since) | 横軸バケット粒度 | バケット数（目安） |
| :--- | :--- | :--- | :--- |
| **1D** | 直近24h（ローカル当日基準） | **10分**（600s） | 144 |
| **7D** | 直近7日 | **1時間**（3600s） | 168 |
| **ALL** | 全期間 | **1時間**（3600s） | 履歴依存（要上限検討） |

### 実装方針
1. **期間ボタン変更（フロント）**: `setPeriod(7)/setPeriod(30)/setPeriod(0)` → **`1D/7D/ALL`** に。各ボタンが `(since, granularity)` を決定して API に渡す。デフォルト active は 1D を想定。
   - 注意: 現状 `since` は `toISOString().slice(0,10)`＝**日付精度**。1D（10分粒度）には粗いので、1D は `since` を**日時精度（now−24h）またはローカル当日0時**に変更する。
2. **DB（`get_usage_timeline` に granularity 対応）**: `DATE(created_at)` → **epoch フロアでバケット化**（TZ 非依存・堅牢）。
   `bucket_epoch = (strftime('%s', created_at) / :gran) * :gran` で `:gran` = 600 or 3600。`GROUP BY bucket_epoch ORDER BY bucket_epoch`。
3. **タイムゾーン（ラベル表示）**: バケット境界は epoch 絶対値で取り、**表示ラベルのみ** `config.timezone`（Asia/Tokyo）でローカル整形（1D→`HH:MM`、7D/ALL→`MM/DD HH:00`）。`created_at` は UTC 保存（`Utc::now().to_rfc3339()`, `storage/lib.rs:93`）。
   ※ 現状の日別集計は UTC 基準で JST の1日とズレている（軽微な既存バグ。本対応で解消）。
4. **空きバケットの 0 埋め**: GROUP BY はデータのある区間しか返さないため、対象窓の全バケット（1D=144, 7D=168, ALL=最古〜現在）を生成して 0 埋めし、時間軸を連続化（フロントは index 等間隔描画のため欠損で歪む）。
5. **API**: `/api/usage/timeline?since=<ts>&granularity=<600|3600>`（既存に `granularity` 追加）。行は `bucket`（ラベル）＋ `bucket_epoch`＋ `input_tokens`/`completion_tokens`/`total_tokens`。
6. **フロント（`renderTimeline`）**: X 軸ラベルを `r.date` → `r.bucket` に。折れ線ロジックは N 点対応済みで変更最小。
7. **単一点ガード**: 0 埋めにより、データが1バケットしか無くても窓全体の点が並び degenerate 描画を回避（ISSUE-17/19 と同種の「単一点で壊れる」問題の根本対処）。
8. **ALL の上限検討**: ALL×1時間は履歴増大で点数が膨らむ（1年≒8,760点）。将来的に点数上限を超えたら自動で粒度を粗く（日別等）するダウンサンプリングを検討（当面は保留可）。

---

## 5.7. ダッシュボードヘッダ（ISSUE-24 / ISSUE-25）

### ISSUE-24: ヘッダのポート表記を `IP:PORT` に【低】
- 現象: ヘッダ右上が `:8080`（ポートのみ、`health.rs:526` にハードコード）。
- 要望: `192.168.1.12:8080` のように **IP アドレス:ポート**で表示。
- 改善案（最小・堅牢）: サーバ側で IP を埋め込まず、**フロントで `window.location.host`** を表示（接続に使ったホスト:ポートを自動表示。ハードコード不要・常に正しい）。`document.getElementById('hostLabel').textContent = location.host` を初期化時に実行。

### ISSUE-25: `●ACTIVE` バッジを daemon STOP 制御ボタンに【中・要セキュリティ配慮】
**ステータス: 保留**（2026-05-31 ユーザー判断。START 非対称性・無認証 LAN への破壊的操作の露出というセキュリティ前提が未解決のため、設計のみ記録し着手は見送り）。
- 現象: 右上 `●ACTIVE` は静的表記（`health.rs:527`）。
- 要望: このボタンで **daemon の稼働状態制御（systemd STOP）** を行いたい。
- 設計の前提（rp1 ユニット実測）: `Restart=on-failure` / `RestartSec=5s` / `WatchdogSec=60s` / `Type=simple` / `User=kazuaki`（NOPASSWD sudo 可）。
- **STOP 実装案（2通り）**:
  - **A. グレースフル自己終了 exit(0)（推奨）** — `Restart=on-failure` のため**正常終了(0)では systemd が再起動しない** → sudo 不要・Web 経路に権限昇格を持ち込まずに「真の停止」を実現。エンドポイントは進行中タスクの後始末＋watchdog 通知停止の上で `std::process::exit(0)` 相当のグレースフルシャットダウンを発火。
  - **B. `sudo -n systemctl stop rustyclaw`** — 確実に inactive 化。NOPASSWD sudo が有効なため可能だが、HTTP ハンドラから shell 実行＝権限昇格経路が増える。
- **START の非対称性（重要）**: ダッシュボードは daemon が配信しているため、**停止すると同時にダッシュボードも消える → ダッシュボードからは START 不可**。再開は外部手段（`ssh rp1 'sudo systemctl start rustyclaw'`）が必須。ボタンは実質 **STOP 専用**になる点を UI で明示。
- **⚠️ セキュリティ（必須対応）**: ダッシュボードは現状 `0.0.0.0:8080` で**無認証**。破壊的な STOP を無防備に晒すと**LAN 上の誰でも daemon を停止可能**になる。最低限: ①確認ダイアログ ②トークン/簡易認証 or 制御エンドポイントのバインド制限（localhost 限定＋リバースプロキシ等）を併せて実装する。
- **実装**: `POST /control/stop`（確認後に発火）。UI はバッジをボタン化し、押下→確認→停止→接続断検知で `OFFLINE` 表示に切替（接続喪失で実 status を反映）。

## 6. 実装ロードマップ（難易度別・STEP 整理）

ISSUE-01〜25 を**難易度（スコープ）**で分類し、**実行単位（STEP）**に再構成する。

### 6-1. 難易度・スコープ分類

| Tier | 定義 | 該当 ISSUE |
| :--- | :--- | :--- |
| **T1 — Dashboard 表現のみ** | フロント（HTML/JS）のみ。バックエンド変更なし | ISSUE-19（truncate 表示）, ISSUE-24（IP:PORT）, ISSUE-17 の表示文言部分 |
| **T2 — Dashboard ＋ 局所バックエンド** | API/DB クエリの追加・変更。影響範囲が限定的 | ISSUE-23（時間別グラフ）, ISSUE-18（cron 予定可視化）, ISSUE-17 の Waiting 捕捉 |
| **T3 — クロスカッティング / アーキテクチャ** | providers/agent/gateway 横断、設計変更を伴う | ISSUE-20（dump 集約＋リングバッファ）, ISSUE-21（タブ式 inspector）, ISSUE-06/07（shell 実行手段）, ISSUE-22※, ISSUE-25※, ISSUE-09 |
| **T4 — エージェント挙動 / プロンプト** | SOUL.md・プロンプト調整中心、コード変更小 | ISSUE-04（事実確認にツール使用）, ISSUE-05（capability 自己認識） |
| **T5 — 設定・運用衛生** | config / ops / 環境整備 | ISSUE-08, ISSUE-11, ISSUE-12, ISSUE-15, ISSUE-16 |
| **T6 — 信頼性・安定性（既存）** | 05-30 ログ由来の堅牢化 | ISSUE-01（**済の可能性**）, ISSUE-02, ISSUE-03 |
| **保留 / 観察** | 着手見送り or 経過観察 | 保留: ISSUE-22, ISSUE-25, ISSUE-09 ／ 観察: ISSUE-10, ISSUE-13, ISSUE-14 |

> ※ ISSUE-01: **解消済み確認（2026-05-31）**。`rustyclaw-tools` の数値系パラメータは `"type":"string"` ＋ `Value::String(s).parse()` で安全化済み（`lib.rs:114/123/394/407` ほか）。`cargo test -p rustyclaw-tools` 44件 PASS。Phase 20 で対処・本 STEP で検証クローズ。

### 6-2. STEP（実行単位）

各 STEP は「まとめて実装・検証・反映できる一貫した単位」。括弧内は主な変更対象。

| STEP | 内容 | Tier | 含む ISSUE | 主な変更対象 | 依存 |
| :---: | :--- | :---: | :--- | :--- | :--- |
| **STEP 1** | Dashboard 即効表示改善（quick win） | T1 | 19, 24, 17(文言) | `health.rs` フロント JS のみ | なし |
| **STEP 2** | トークン使用量グラフ実用化 | T2 | 23 | `storage`(クエリ)＋`health.rs`(API/フロント) | なし |
| **STEP 3** | LANE QUEUE スケジュール可視化 | T2 | 18, 17(Waiting捕捉) | `gateway`(次回実行算出/enqueue)＋API＋フロント | cron.json 解析（既存） |
| **STEP 4** | LLM I/O インスペクタ刷新（最大単位） | T3 | 20→21（19適用） | `providers`(dump集約)＋`agent`(category付与)＋`config`(opts)＋`health.rs`(API/UI) | 20→21 は密結合 |
| **STEP 5** | エージェント自己認識・事実確認 | T4 | 04, 05 | `SOUL.md`/プロンプト（＋任意で schedule API ツール化＝STEP3 連携） | STEP3(任意) |
| **STEP 6** | cron 信頼性回復 | T3 | 06(＋07) | 要方針決定: A)既存 API ツールで prompt 書換（軽）/ B)制約付き shell ツール新設（重・セキュリティ） | 方針決定 |
| **STEP 7** | 設定・運用衛生 | T5 | 08, 11, 12, 15, 16 | config 運用・symlink・LM Studio 手順・rp1 環境（個別に着手可） | なし |
| **STEP 8** | 信頼性・安定性（既存） | T6 | 01(再確認), 02, 03 | `tools`/`agent`/`gateway` | ISSUE-01 状態確認後 |
| **保留** | — | T3 | 22, 25, 09 | capacity 引き上げ時 / セキュリティ前提解決後 / フェイルオーバ設計時 | — |

### 6-3. 推奨実行順（価値 × 労力 × リスク）

1. **STEP 6 / ISSUE-06**（cron の無言失敗）— 実害が最大。まず A 案（`karakeep_tag_bookmark` 等の既存ツールで prompt 書換）で軽く塞ぐ。
2. **STEP 1**（Dashboard quick win）— ほぼゼロリスクで即効。19/24 は数行。
3. **STEP 2**（時間別グラフ）— 自己完結、ユーザー要望明確。
4. **STEP 5**（プロンプト是正）— 低労力で UX 改善。STEP3 完了後なら schedule をツール化して ISSUE-04 を確実化。
5. **STEP 3**（QUEUE スケジュール可視化）。
6. **STEP 4**（LLM I/O inspector 刷新）— 最大単位。設計（§5 ISSUE-21）が固まっているので、まとまった時間が取れる時に。
7. **STEP 7**（運用衛生）— 随時。特に ISSUE-12（symlink コミット衛生）は早めに。
8. **STEP 8**（信頼性）— ISSUE-01 の解消状況を確認してから 02/03 を判断。
