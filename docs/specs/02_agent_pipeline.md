# 02. エージェントパイプライン・LLMプロバイダ仕様

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: 2026-05-28  
> **対象コード**: `rustyclaw-agent`, `rustyclaw-providers` の最新実装

## 1. Pipeline の 4 ステージ

エージェントの1ターン（Turn）の処理は、以下の 4 つの論理ステージから構成されるパイプラインを通じて実行されます。

```
[ContextBuilder] ──(構築されたメッセージ群)──> [CallLLM] ──(最終テキスト応答)──> [PublishResponse]
                                                  │  ↑
                                   (ツール呼び出し要求)│  │(ツール実行結果)
                                                  ▼  │
                                            [ExecuteTools]
```

### ① ContextBuilder (コンテキスト構築)
- インプット：現在の会話入力、セッション履歴、ワークスペースの人格定義（`SOUL.md`、`AGENTS.md`、`MEMORY.md`、`USER.md`）。
- 処理内容：
  - 各種人格・コンテキストファイルの読み込みとシステムプロンプトへの結合。
  - セッション会話履歴（`ConversationHistory`）の取得とトークン上限チェック。**※ `cron:` から始まるセッションIDの場合は履歴のロードをスキップし、完全にステートレスとして扱うことでコンテキストの無限の肥大化を防止します。**
  - 必要に応じた会話履歴の圧縮（`compact_if_needed`）。
  - 日またぎの文脈復元（`Session Continuation`）や自発メッセージ（`Proactive Posts`）の注入。
- アウトプット：LLM APIに送信可能な形式の `Vec<Message>`。

### ② CallLLM (LLMの呼び出し)
- インプット：構築されたメッセージ群、利用可能なツール定義のリスト、実行オプション。
- 処理内容：
  - 指定されたプロバイダ（OpenAI互換, Anthropic, Gemini, Ollama）をディスパッチ。
  - API呼び出し（ストリーミング応答、あるいはツールコール要求）。
  - フォールバックチェーン（一次プロバイダが障害時に代替プロバイダへ切り替え）の制御。
- アウトプット：LLMからのテキスト応答、またはツール実行要求リスト。

### ③ ExecuteTools (ツールの実行)
- インプット：LLMから要求されたツール実行指示（ツール名、引数）。
- 処理内容：
  - インプロセスで動作する組み込みツール（`rustyclaw-tools`）、または外部MCPサーバー経由のツールを呼び出し。
  - 各ツールの実行結果（成功・失敗・出力データ）を取得。
  - 実行結果を会話履歴に格納し、必要に応じて再帰的に `CallLLM` へ再フィードバック（LLMが最終的な回答を生成するまでループ）。
- アウトプット：ツールの実行完了通知および会話ログ。

### ④ PublishResponse (応答の配信)
- インプット：LLMの最終応答コンテンツ。
- 処理内容：
  - **JSON Leak Filter**: ユーザーやチャンネルへの配信前に、アシスタントの応答テキストから未加工のツール呼び出し用JSON（例: ````json {"action": ...} ````）等のリークを自動検知・除去するフィルタを適用します。
  - 出力先チャンネル（Telegram、Discord、CLI等）へ応答データを送信。
  - 送信成功時、セッションログ（`sessions/*.jsonl`）への追記（atomic/fail-closed）。
  - セッション完了に伴う非同期処理（Memory Flush 等）のトリガー。

---

## 2. 会話継続感を作る 6 技法

ステートレスなLLM APIを使用しながら、エージェントがユーザーと「常に文脈の繋がった会話」をしている感覚を構築するための 6 つの技術的アプローチです。

### ① 会話履歴の蓄積
セッション中のやり取りを `ConversationHistory` に蓄積し、毎ターンのリクエストメッセージ群に過去のログを結合して渡します。
```rust
pub struct ConversationHistory {
    messages:         Vec<Message>,
    estimated_tokens: usize,
}
```

### ② コンテキスト圧縮 (70/20/10 戦略による Quota 制限対策)
トークン上限および API クォータ制限（Quota）の逼迫を防ぐため、単純な履歴切り捨てを行わない知的な圧縮アルゴリズム（GeminiClawの `truncateWithContext` 相当）を実装します。
- **仕組みと比率**:
  - 会話履歴（`ConversationHistory`）の総トークン数がモデル制限値の 80% に達した際にトリガーされます。
  - **先頭の 70%**（会話の背景や前提となる初期情報）を保持します。
  - **末尾の 20%**（最もアクティブで直近のやり取り）を保持します。
  - 中間の残り **約 10% 相当** を単一の省略マーカーメッセージ `[...N tokens omitted for context compression]` で置換します。
- **効果**: 会話の初期前提と直近の話題をロストさせずに、入力トークン数を常に安全なレベルに保ち、余分なAPIコールのトークン消費（＝Quotaの浪費）を防ぎます。

### ③ Memory Flush (ターン完了後の全書き直し方式メモリ更新)

ターン完了後に `tokio::spawn` で非同期起動し、前回 Flush 以降の**デルタ会話**と現在の `MEMORY.md` を LLM に渡して**全書き直し版**を返させます。その結果で `MEMORY.md` を上書きし、活動ログを `memory/logs/YYYY-MM-DD.md` に追記します（GeminiClaw の silent memory flush 相当）。

**実行判定（二重ゲート）**:
初回ターンは無条件実行。2 回目以降は以下を **両方** 満たす場合のみ実行します。
- **デルタ制御**: 前回 Flush 以降の新着メッセージ数 ≥ **6**（≒ 3 ターン分）
- **時間ゲート**: 前回 Flush から **15 分以上** 経過

**トークン最適化**:
- LLM に渡す会話は「前回 Flush 以降の新着分のみ」（最大 10 件上限）。全履歴の末尾 20 件を固定送信する旧方式と比べ入力トークンを約 4〜5 倍削減。
- 出力 `max_tokens: 1500`（MEMORY.md ≦ 1250 tok ＋ daily log ≒ 100 tok で収まる）。

**LLM 出力フォーマット**:
```
---NEW_MEMORY---
<5KB 以内の完全な MEMORY.md 全文>
---END_MEMORY---
---DAILY_LOG---
<箇条書き活動サマリー>
---END_DAILY_LOG---
```

- MEMORY.md は **上書き**（追記ではない）。5KB 超過時のフェイルセーフとして Rust 側で 70/20 トランケートを適用。
- LLM への指示: 古い情報を削除しながら新情報を組み込み、5000 文字以内に収めること。
- `cron:*` セッションでは Flush をスキップ（バックグラウンドジョブは対象外）。
- **fail-open**: 失敗は `WARN` ログのみ。チャット本体は無停止継続。
- **セマフォ制御**: `flush_sem`（容量 1）を取得してから実行し、同時 gmn プロセス数が意図した上限を超えないよう保護（→ `05_gateway_spec.md` §2 参照）。

### ④ Session Continuation (日またぎの文脈復元)
日付が変わった後の初回ターンにおいて、前日のサマリー（`summaries/` の TL;DR）および直近5件のやり取りを自動的にシステムコンテキストに注入し、「昨日話していたことの続き」からスムーズに対話を再開させます。

### ⑤ Proactive Posts 注入
Heartbeatサービス（自発的アクション）によってエージェントから自発的に送信されたメッセージを、単なるシステム通知ではなく「自分が過去に発信した対話」として `ConversationHistory` に適切に挿入します。これにより、エージェント自身の発言内容の忘れ防止を図ります。

### ⑥ System Prompt 常時注入
毎回のAPI呼び出しのシステムプロンプトの最上部に、ワークスペースの4ファイル（`SOUL.md`、`AGENTS.md`、`MEMORY.md`、`USER.md`）の最新テキストを常に注入し、一貫した人格とユーザープロフィールを維持させます。

---

## 3. LlmProvider 設計

### 重要な設計原則
- **LlmProviderは完全なステートレス。**
- すべての接続は使い切りの HTTP リクエストであり、セッション管理やコンテキスト管理の責任を持ちません。

### `LlmProvider` トレイト定義

プロバイダ呼び出しで発生し得るエラーを厳密に区別するため、以下の `ProviderError` カスタムエラー型を導入し、`Result<T, ProviderError>` を返却します。

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Rate limit or quota exceeded: {0}")]
    RateLimit(String),
    #[error("API or CLI execution failed: {0}")]
    ExecutionFailed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

```rust
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// テキスト・ツールの一括補完
    async fn complete(
        &self,
        messages: &[Message],
        tools:    &[ToolDef],
        opts:     &CompletionOptions,
    ) -> std::result::Result<LlmResponse, ProviderError>;

    /// SSEを利用したストリーミング補完
    async fn complete_stream(
        &self,
        messages: &[Message],
        tools:    &[ToolDef],
        opts:     &CompletionOptions,
    ) -> std::result::Result<
        Pin<Box<dyn Stream<Item = std::result::Result<StreamChunk, ProviderError>> + Send>>,
        ProviderError,
    >;
}
```
pub struct CompletionOptions {
    pub model:       String,
    pub max_tokens:  Option<u32>,   // None の場合はプロバイダ既定値に委ねる
    pub temperature: Option<f32>,
    pub timeout:     Duration,      // デフォルト 15分 (900s)
}
```

### プロバイダファクトリ
`config.json` の `model_provider` フィールドに基づいて具象プロバイダを生成します。
```rust
pub fn create_provider(config: Config) -> Box<dyn LlmProvider> {
    match config.model_provider.as_str() {
        "openai" => Box::new(OpenAiCompatProvider::new(config)),
        "gmn"    => Box::new(GmnCliProvider::new(config)),
        _        => panic!("Unsupported model provider: {}", config.model_provider),
    }
}
```

| `model_provider` | 実装状態 | 用途 |
|---|---|---|
| `"openai"` | ✅ 実装済み | OpenAI 互換 API（reqwest + SSE） |
| `"gmn"` | ✅ 実装済み | デバッグ用 gmn CLI サブプロセス |
| `"anthropic"` | 🔲 Phase 2+ | Anthropic Claude API |
| `"gemini"` | 🔲 Phase 2+ | Gemini REST API |
| `"ollama"` | 🔲 Phase 2+ | ローカル LLM (Ollama) |

### `GmnCliProvider`（デバッグ用 gmn CLI プロバイダ）
開発・デバッグ時に gmn コマンドラインツール（Google Code Assist API の非インタラクティブ CLI）を LLM バックエンドとして使用するための実装です。`config.json` の `model_provider` を `"gmn"` に設定することで有効化されます。

```json
{ "model_provider": "gmn", "model_name": "flash", "debug_dump": true }
```

**動作仕様**:
- `tokio::process::Command` で `gmn -p <prompt> -m <model>` をサブプロセスとして起動します。
- `Vec<Message>` をシステム・ユーザー・アシスタントのロールごとにフラットなテキストへ変換し（`build_prompt()`）、プロンプトのサイズ制限を回避するため**標準入力（stdin）**経由で `gmn` CLI に流し込みます（詳細は後述の OS error 7 対策を参照）。
- stdout から JSON Lines 形式（`{"type":"content","text":"..."}`)を読み取りテキストを組み立てます。プレーンテキスト行はそのままストリームに流します。

**gmn パッチビルド（RustyClaw fork）の前提**:
インストールされている `gmn` は RustyClaw 向けにパッチを当てたビルドであることが必要です。`gmn --version` で `+rustyclaw` サフィックスが付いていることを確認してください。

```bash
gmn --version
# → gmn version <tag>+rustyclaw
```

パッチ内容（`/home/kazuaki/Projects/gmn/master/src/` 配下に適用）:
- `GMN_MAX_RETRIES` 環境変数サポート（内部リトライ回数の上書き）
- `--think` フラグ（拡張思考モード）
- `--stop` フラグ（ストップシーケンス）
- stdout への Force-flush

**レートリミット・Quota 保護および非ブロッキング・バックオフリトライ**:

| 保護レイヤー | 内容 |
|---|---|
| 環境変数 `GMN_MAX_RETRIES=0` | gmn 内部の 429 リトライを無効化し、制御を RustyClaw 側に委譲 |
| `-t <duration>` タイムアウト | バックオフ待機中にコンテキストキャンセルを発火させ早期終了させる暫定手段 |
| `complete()` quota 検知 | stderr に `quota`・`RESOURCE_EXHAUSTED`・`429` が含まれる場合、`ProviderError::RateLimit` を即座にバブルアップし、プロバイダ内部ではスリープやリトライを行わず制御を呼び出し元へ戻す |
| `complete_stream()` 終了コード確認 | ストリーム完了後にプロセス終了コードを検証し、異常終了（429エラー等）を確実に `ProviderError::RateLimit` として伝播 |
| `rustyclaw-gateway` の非ブロッキング・バックオフリトライ | `gmn_sem` のセマフォを取得した状態で LLM 呼び出しを実行し、`ProviderError::RateLimit` を検知した瞬間に **セマフォ許可証（Permit）を即時解放（drop）** する。その後、5秒、10秒、20秒と指数関数バックオフ（Exponential Backoff）で `tokio::time::sleep` を行い、スリープ明けにセマフォを再取得して最大3回まで再試行する。これにより、レートリミット待機中もグローバルセマフォを占有せず、他の独立したレーンやタスクの進行を阻害しない。 |
| LaneRegistry セマフォ | 同時起動数を user: 2 / bg: 1（最大 3）に制限（→ `05_gateway_spec.md` §3 参照） |

```rust
// GmnCliProvider での起動例 (標準入力経由方式)
let mut child = tokio::process::Command::new("gmn")
    .env("GMN_MAX_RETRIES", "0")  // gmn 内部リトライ無効化
    .arg("-m").arg(&opts.model)
    .arg("-t").arg(format!("{}s", opts.timeout.as_secs()))
    .arg("--no-agent")            // エージェントループ無効化（単一ターン）
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .context("Failed to spawn gmn CLI process")?;

if let Some(mut stdin) = child.stdin.take() {
    use tokio::io::AsyncWriteExt;
    stdin.write_all(prompt.as_bytes()).await?;
    drop(stdin); // EOFを伝達
}
```

> **`Argument list too long` (OS error 7) 対策と標準入力設計**:  
> 当初、マージされたプロンプト全体を引数 `-p` で直接 `gmn` コマンドに渡していましたが、会話履歴や Heartbeat 巡回コンテキストの肥大化に伴い、Linux OS の引数最大長（`ARG_MAX`）を超過し、子プロセスの起動時に OS エラー 7 でクラッシュする問題が発生しました。  
> そこで、`gmn` CLI の標準入力パース機能（`-p` が空の際に stdin からプロンプトを吸い上げる仕様）を利用し、プロンプトデータを引数リストではなく子プロセスの `stdin` に書き込んで流し込む方式に移行しました。これにより、無制限に長いプロンプトを安全に受け渡し可能となりました。

> **`--no-agent` は必須**: gmn のデフォルトはエージェントモード ON（最大 25 ターン）。
> このフラグなしでは `complete()` 1 回で Gemini API が最大 25 回呼ばれ、rate limit の直接原因になる。
> RustyClaw は自前の Pipeline でコンテキスト・ツール管理を行うため、gmn 側のエージェントループは不要。

---

## 4. デバッグ用通信ダンプ・ログ仕様

開発時および本番稼働時のトラブルシューティングにおいて、LLMとの生リクエスト/レスポンス、および中間ツールの実行データを簡単に点検（デバッグ）するための仕組みです。

### ① 生リクエスト・生レスポンスの即時ダンプ（オーバーライト・ダンプ）
毎回の LLM 呼び出し（`CallLLM`）の直前および直後に、送信した「生の入力JSON」と受信した「生の応答データ」を特定のフォルダに上書き保存し、最新のAPIやり取りを即座に確認できるようにします。

- **出力先フォルダ**: `workspace/memory/debug/`
- **ダンプファイル**:
  - `last_request.json`: LLMに送信された最新のシステムプロンプト、会話履歴、利用可能なツール定義を含む生のJSON。
  - `last_response.json`: LLMから受信した最新のテキスト応答、または要求されたツールコールリストを含む生のJSON。
- **制御フラグ**: 
  - 設定ファイル `config.json` の `debug_dump: true` でオン・オフを切り替えます。
  - ディスクI/Oを節約するため、本番環境の通常稼働時は無効（`false`）にすることを推奨します。

### ② `tracing` を用いた各子ライブラリ（クレート）の稼働ログ記録仕様
Rust標準の `tracing` エコシステムを活用し、エージェントシステムの各論理レイヤー（8つのクレート）がそれぞれの役割に応じた構造化デバッグログを `~/.rustyclaw/rustyclaw.log` に書き出します。

#### クレート別のログ記録内容一覧

| クレート名 | ログレベル | 主なログ記録内容・点検項目 |
|---|---|---|
| **`rustyclaw-cli`** | `INFO`<br>`ERROR` | ・コマンドライン引数（コマンド名、実行オプション）の受け入れ。<br>・起動前初期化エラー、設定ファイル不足、クラッシュ時のスタックトレース。 |
| **`rustyclaw-gateway`** | `INFO`<br>`DEBUG`<br>`TRACE` | ・MessageBus の初期化、シグナル受信（SIGHUPによるホットリロード検知）。<br>・定時パトロール（Heartbeat）のスケジューラ着火と実行推移。<br>・`LaneRegistry` へのリクエストキューイングおよびセマフォスロットの獲得時間。<br>・Heartbeat実行時の `HEARTBEAT_OK` 判定結果（通知配信か、静的終了か）。<br>・（TRACEレベル）MessageBusを流れる全生イベント、Watchdogの心拍送信シグナル。 |
| **`rustyclaw-agent`** | `INFO`<br>`DEBUG`<br>`TRACE` | ・Pipeline（1ターン）の開始と完了、応答テキストのサイズとセッションID。<br>・`ContextBuilder` での人格ファイル群（SOUL/AGENTS等）のロード結果と総トークン数。<br>・履歴圧縮アルゴリズム（70/20/10）のトリガー判定、圧縮前後のトークン削減量のダンプ。<br>・`ExecuteTools` によるツールの呼び出し開始（ツール名、署名）および戻り値ステータス。<br>・（TRACEレベル）最終的にマージされた完全なシステムプロンプトの生テキストダンプ。 |
| **`rustyclaw-providers`** | `INFO`<br>`DEBUG`<br>`TRACE` | ・LLM APIへのHTTP接続開始、HTTP 200等のステータス返却。<br>・通信遅延（Latency）、プロバイダエラー、リトライおよび代替LLMへのフォールバック発生ログ。<br>・消費トークン数（Prompt / Completion / Total）の解析。<br>・（TRACEレベル）送信する生リクエストJSON、SSEストリームで受信する生チャンクテキスト。 |
| **`rustyclaw-channels`** | `INFO`<br>`DEBUG`<br>`TRACE` | ・Discord WebSocket / Webhook 接続の確立とネットワーク切断・再接続プロセス。<br>・チャット着信時のパース結果（送信元ID、チャンネルID、入力メッセージ概要）。<br>・チャット応答配信時のAPIレスポンスコード。<br>・（TRACEレベル）Discord Gateway等から受信する未加工の生イベント（Raw JSON）。 |
| **`rustyclaw-storage`** | `INFO`<br>`DEBUG`<br>`TRACE` | ・SQLite `memory.db` 接続プールの初期化、tantivy 全文検索インデックスの再構築完了。<br>・JSONLセッションログへの対話データの原子性書き込み（atomic write）の完了。<br>・SQLite PRAGMA（WAL/キャッシュ）の適用結果。<br>・既読テーブル（`seen_items`）によるニュースやイベントの重複パトロール検知除外。<br>・（TRACEレベル）実行された生SQLテキスト、トランザクションの開始・コミット・ロールバック。 |
| **`rustyclaw-tools`** | `INFO`<br>`DEBUG` | ・外部MCPサーバー（std-io等）のプロセスのスポーンおよび接続ハンドシェイク完了。<br>・内製およびMCPツールの実行開始（引数のダンプ）と戻り値の内容。 |
| **`rustyclaw-config`** | `INFO`<br>`DEBUG` | ・`config.json` のロード成功、シークレット（`.security.yml`）の `age` による復号完了。<br>・環境変数による設定値の動的オーバーライド検出。 |

#### 即時点検コマンド例
```bash
# 最新のAPI通信の生プロンプトを確認する
cat workspace/memory/debug/last_request.json | jq .

# エージェントの動作ログをリアルタイムで監視する
tail -f ~/.rustyclaw/rustyclaw.log | grep -E "DEBUG|ERROR"

# 特定のサブライブラリ（例: LLMプロバイダ）の挙動のみを抽出する
tail -f ~/.rustyclaw/rustyclaw.log | grep "rustyclaw-providers"
```

### ③ ログ活用のガイドライン（ログレベルの切り替え挙動）

本エージェントシステムでは、環境変数（例: `RUST_LOG`）や設定ファイルによってログレベルを切り替えることで、フットプリントと詳細度のバランスを動的に制御します。

- **`INFO` レベル (通常稼働時・推奨)**
  - **対象**: システムの正常起動、会話の開始・完了、APIの主要なエラー。
  - **目的**: ディスクI/Oを抑えつつ、システム全体の「健康状態」と「会話の流れ」を最低限のログサイズで定常監視します。
- **`DEBUG` レベル (開発・テスト・簡易デバッグ時)**
  - **対象**: 履歴圧縮（`truncateWithContext`）の動作、ツールの呼出引数と実行結果、消費トークン数のサマリー、接続エラー時のプロバイダリトライ・フォールバック検知。
  - **目的**: エージェントの「思考プロセスの流れ」や「ツール呼び出しの整合性」を詳細に追い、ロジックバグを特定します。
- **`TRACE` レベル (徹底的な解析・通信トラブルシュート時)**
  - **対象**: 送信された巨大なシステムプロンプトの完全な生テキスト、SSEストリームで受信した未加工の生チャンクデータ、Discord Gatewayから届く生イベントRaw JSON、SQLiteの全実行生SQL文。
  - **目的**: データ破損、パース不良、ミリ秒単位の通信ラグなど、ブラックボックスになりやすいLLMやDB、ネットワークの深部で発生するバグを100%特定するために使用します。


