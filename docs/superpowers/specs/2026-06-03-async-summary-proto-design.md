# 非同期ローリング要約プロトタイプ 設計仕様

**作成日:** 2026-06-03  
**ステータス:** Draft  
**対象ブランチ:** feature/async-summary-proto（新規）  
**削除予定:** RustyClaw 本体統合完了後

---

## 1. 目的

LM Studio（OpenAI互換API）を使い、「5会話ごとにバックグラウンドで要約を更新する」パターンを Rust で実証する。検証後、同パターンを `rustyclaw-agent` へ移植する。

---

## 2. 全体アーキテクチャ

```
crates/rustyclaw-summary-proto/
├── Cargo.toml
└── src/
    ├── main.rs      // 対話ループ（標準入力 → chat() → 結果出力）
    ├── session.rs   // ChatSession 定義（状態構造体）
    └── proto.rs     // SummaryProto（メインLLM呼び出し + バックグラウンド要約）
```

### コンポーネント関係

```
main.rs
  └─ SummaryProto (proto.rs)
       ├─ Arc<RwLock<ChatSession>> (session.rs)   ← 状態の唯一の源
       ├─ Arc<Semaphore>                           ← 要約の同時実行を 1 本に制限
       ├─ main LLM  (rig-core OpenAI provider)
       └─ summary LLM (同じプロバイダー・同じモデル)
```

### 処理フロー

```
1. ユーザー入力を受信
2. RwLock::read()  → current_summary + recent_messages を取得
3. メインLLMへリクエスト → ユーザーへ即座に応答返却
4. RwLock::write() → raw_history / recent_messages に追記、counter++
5. counter >= SUMMARY_INTERVAL(5) かつ Semaphore に空き → tokio::spawn
   ├─ recent_messages をスナップショットにコピー
   ├─ recent_messages = [] / counter = 0（先行クリア）
   ├─ write lock 解放
   └─ 要約LLM 呼び出し
       ├─ 成功 → current_summary 更新 → summary.md 上書き保存
       └─ 失敗 → warn ログのみ（current_summary は前回値を保持）
       → Semaphore permit を drop（自動解放）
```

**スナップショット先行クリア**: spawn 前に `recent_messages` を空にすることで、要約中に届いた新規メッセージが誤って消えることを防ぐ。要約LLM 失敗時も `raw_history` にはスナップショットが残る。

---

## 3. 状態構造体

### `ChatSession`（`session.rs`）

```rust
// rig-core の Message 型を再エクスポートして使用
// use rig::completion::message::Message;

pub struct ChatSession {
    pub raw_history: Vec<(String, String)>,  // (role, content) 全会話ログ
    pub recent_messages: Vec<Message>,       // 直近N件 → LLMに毎回送信（rig-core の Message 型）
    pub current_summary: String,             // 要約文 → system prompt に埋め込む
    pub counter: u32,                        // 0〜SUMMARY_INTERVAL-1
}
```

- 起動時に `summary.md` が存在すれば `current_summary` にロード、なければ空文字列で開始
- `Arc<tokio::sync::RwLock<ChatSession>>` でラップしてスレッドセーフに共有

### メインLLM 呼び出し時のプロンプト組み立て

```
[system]  "{BASE_SYSTEM_PROMPT}\n\n## これまでの要約\n{current_summary}"
  + recent_messages  (直近 SUMMARY_INTERVAL 件)
  + [user] 今回の入力
```

`current_summary` が空の場合、要約セクションは省略する。

---

## 4. 依存関係

### `Cargo.toml`

```toml
[package]
name    = "rustyclaw-summary-proto"
version = "0.1.0"
edition = "2021"

[dependencies]
rig-core           = "0.9"   # 要 crates.io で最新版確認
tokio              = { version = "1", features = ["full"] }
serde              = { version = "1", features = ["derive"] }
serde_json         = "1"
anyhow             = "1"
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

ワークスペークルートの `Cargo.toml` の `members` に `"crates/rustyclaw-summary-proto"` を追加する。

---

## 5. 設定（環境変数）

| 変数 | 内容 | デフォルト値 |
|------|------|------------|
| `LMS_BASE_URL` | LM Studio エンドポイント | `http://127.0.0.1:1234/v1` |
| `LMS_API_KEY` | プレースホルダー | `"lm-studio"` |
| `MAIN_MODEL` | メインLLMモデル名 | `google/gemma-4-8b`（LM Studio にロード済みのモデル識別子に合わせる） |
| `SUMMARY_MODEL` | 要約LLMモデル名 | 同上 |
| `WORKSPACE_DIR` | ファイル保存先 | `./production/workspace/proto` |

- `.env` ファイル対応なし（shell export で指定）
- `LMS_BASE_URL` は開発機ローカル実行前提のため `127.0.0.1`。rp1 デプロイ時は `192.168.1.110:1234` に変更し LM Studio の「Serve on Local Network」を ON にすること

---

## 6. ファイル永続化

```
production/workspace/proto/   ← WORKSPACE_DIR
└── summary.md                ← current_summary の永続化（上書き保存）
```

- 起動時に読み込み、要約更新のたびに上書き
- `summary.md` 書き込み失敗時は `warn!` ログのみ（in-memory の `current_summary` は更新済みなので会話継続）
- `.gitignore` への追加は任意（ブランチ実証中はコミット対象でも可）

---

## 7. エラーハンドリング

| ケース | 挙動 |
|--------|------|
| メインLLM 失敗 | `anyhow::Error` をユーザーに返して会話継続。counter はインクリメントしない |
| 要約LLM 失敗 | `warn!` ログのみ。`current_summary` は前回値を保持。次の5件で再試行 |
| `summary.md` 書き込み失敗 | `warn!` ログのみ。in-memory の値は更新済みなので会話継続 |
| Semaphore 取得失敗（busy） | 今回の要約をスキップ。次の5件でトリガー |

**方針**: 要約はベストエフォート。失敗しても会話を止めない。

---

## 8. RustyClaw 統合後の削除対象

| 削除対象 | タイミング |
|---------|-----------|
| `crates/rustyclaw-summary-proto/` | 統合完了後 |
| `production/workspace/proto/` | 統合完了後 |
| 本ブランチ | 統合完了後 |
