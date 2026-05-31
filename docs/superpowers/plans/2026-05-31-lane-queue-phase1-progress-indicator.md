# 実装計画書: Phase 1 進捗インジケータの実装 (ギャップ D の解消)

本ドキュメントは、GeminiClaw の `ChatProgressReporter` をモデルにし、RustyClaw の自前インプロセス環境で **Discord タイピング状態の維持** および **ツール実行時などの中間進捗表示** を実現するための詳細な実装計画書です。

---

## 1. 目的

ユーザーがプロンプトを送信してから最終的な回答を受け取るまでの間（10〜60秒）、Discord 側で以下を表示し、ユーザー体験 (UX) を劇的に向上させます。
1. **「タイピング中...」の継続表示**: Discord のタイピング状態は約10秒で消えるため、バックグラウンドのタイピングキープアライブタスクで約 7〜8 秒ごとに定期的に送信し続けます。
2. **中間進捗メッセージの表示と更新**: 処理開始時にプレースホルダーメッセージを投稿し、LLM の呼び出しやツール実行（例：「`view_file` を実行中...」）のたびにそのメッセージを `edit` して進捗を可視化します。完了時にはこのメッセージを自動的に `delete` し、最終回答を新規投稿として配送します。

---

## 2. 変更予定のファイル一覧

* **`crates/rustyclaw-channels/src/lib.rs`**
  - `ProgressReporter` トレイトの新規定義。
  - `DiscordProgressReporter` 構造体の追加（タイピングキープアライブ、進捗メッセージのライフサイクル制御）。
  - `DiscordConnector` から Serenity `Http` インスタンスや `Context` にアクセスするためのヘルパーまたはクローン機能の公開。
* **`crates/rustyclaw-agent/src/lib.rs`**
  - `Pipeline::execute_with_tools` の引数に `progress: Option<Arc<dyn ProgressReporter>>` を追加。
  - LLM 完了時や各ツールの実行開始時・終了時に `progress.update_status(...)` を呼び出すようフックを挿入。
* **`crates/rustyclaw-gateway/src/lib.rs`**
  - メッセージを受信し、エージェントを実行する直前に `DiscordProgressReporter` を構築・開始 (`start`)。
  - 実行終了時（正常・異常問わず）に終了 (`finish`)。

---

## 3. 詳細設計

### 3.1. `ProgressReporter` トレイト

`crates/rustyclaw-channels/src/lib.rs` に定義します。

```rust
#[async_trait]
pub trait ProgressReporter: Send + Sync {
    /// 進行インジケーター（タイピング表示、および中間メッセージ）の開始
    async fn start(&self) -> Result<()>;
    
    /// 現在の処理状況やツール実行ステータスを更新する
    async fn update_status(&self, status: &str) -> Result<()>;
    
    /// 処理が完了した時のクリーンアップ（タイピングの停止、メッセージの削除など）
    async fn finish(&self) -> Result<()>;
}
```

### 3.2. `DiscordProgressReporter` の内部設計

```rust
pub struct DiscordProgressReporter {
    http: Arc<serenity::http::Http>,
    channel_id: u64,
    // タイピングキープアライブタスクの停止信号
    typing_abort_handle: Arc<std::sync::Mutex<Option<tokio::task::AbortHandle>>>,
    // 進捗メッセージのID (edit, delete 用)
    progress_message_id: Arc<tokio::sync::Mutex<Option<u64>>>,
    // 更新時のレート制限対策 (編集の最小間隔を 2〜3秒に抑えるためのタイムスタンプ)
    last_update: Arc<tokio::sync::Mutex<std::time::Instant>>,
}
```

* **`start()` の挙動**:
  1. `thinking` 状態の中間メッセージ「*エージェントが考え中...*」を対象チャンネルに送信し、メッセージIDを `progress_message_id` に保持する。
  2. ループを回すバックグラウンドタスク（`tokio::spawn`）を起動し、7〜8秒間隔で `broadcast_typing` を繰り返し送信する。起動したタスクの `AbortHandle` を `typing_abort_handle` に格納する。
* **`update_status(status)` の挙動**:
  1. `progress_message_id` が存在する場合、該当メッセージを編集 (`edit`) して、「*エージェントが実行中: {}*」などのテキストに更新する。
  2. 短時間に頻繁に更新が呼ばれた場合、Discord のメッセージ編集レート制限（1チャンネル5秒に数回など）に引っかからないよう、前回の更新から2秒以上空けるスロットリング処理を入れる。
* **`finish()` の挙動**:
  1. `typing_abort_handle` からキープアライブタスクを即座に `abort` する。
  2. `progress_message_id` が存在する場合、メッセージを削除 (`delete`) する。これにより、ユーザーには最終的な長い回答のみが新規投稿（プッシュ通知あり）として届くようになり、余計なプレースホルダーメッセージがタイムラインに残らない。

---

## 4. 具体的な実装タスクリスト

### [ ] タスク 1: `rustyclaw-channels` に `ProgressReporter` と `DiscordProgressReporter` を実装
- [ ] `crates/rustyclaw-channels/src/lib.rs` に `ProgressReporter` トレイトを定義。
- [ ] `DiscordConnector` の `http` フィールドや、チャンネル送信用メソッドを使い `DiscordProgressReporter` を実装。
- [ ] タイピング維持タスク (`tokio::spawn`) とアボートハンドラを実装。
- [ ] 進捗メッセージの投稿 (`ChannelId::say`)、編集 (`edit_message`)、および完了時削除 (`delete_message`) を実装。
- [ ] スロットリング機能を組み込み。
- [ ] `DiscordConnector` から `ProgressReporter` を生成するためのファクトリメソッドを追加。

### [ ] タスク 2: `rustyclaw-agent` で進捗コールバックを受け取れるように拡張
- [ ] `crates/rustyclaw-agent/src/lib.rs` の `Pipeline::execute_with_tools` のシグネチャを修正。
- [ ] LLM API 呼び出しの直前に「*LLMの応答を待っています...*」に進捗を更新。
- [ ] ツールループの開始時に、呼び出すツール名と引数をもとに「*ツール `%tool_name%` を実行しています...*」に進捗を更新。
- [ ] モック用の `ProgressReporter` または `None` を渡すユニットテストの修正。

### [ ] タスク 3: `rustyclaw-gateway` に進捗表示のライフサイクルを統合
- [ ] `crates/rustyclaw-gateway/src/lib.rs` の通常ユーザーメッセージディスパッチ部分 (`dispatch()`) を改修。
- [ ] エージェントの実行開始前に、`DiscordProgressReporter` を作成し `start()` を呼び出す。
- [ ] `Pipeline::execute_with_tools` に進捗レポーターの `Arc` インスタンスを渡す。
- [ ] 実行成功・失敗に関わらず、`finally` のように `finish()` を確実に呼び出すように `tokio::select` または `defer` パターンで囲む。

---

## 5. 検証およびテスト計画

### 5.1. ユニットテストの実行
- [ ] `cargo test -p rustyclaw-channels` で既存テストが通ることを確認。
- [ ] `cargo test -p rustyclaw-agent` でシグネチャ変更に伴うテストエラーを解消し、モックを用いて進捗フックが適切に呼ばれるか検証するユニットテストを追加。

### 5.2. 統合手動テスト（RPi 環境での動作確認）
- [ ] 開発完了後、RPi 環境 (`rp1`) にデプロイ。
- [ ] Discord チャンネルでボットに「ファイルを検索して」などのツール呼び出しが発生するプロンプトを送信。
- [ ] 応答中に、Discord のタイピングインジケータ（「RustyClaw is typing...」）が表示され続け、かつ「ツール `find_files` を実行中...」といった中間メッセージが表示された後、最終回答が得られた瞬間に中間メッセージが消えることを目視で確認。
