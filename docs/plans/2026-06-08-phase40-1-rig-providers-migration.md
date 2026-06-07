# Phase 40-1: `rustyclaw-providers` の rig-core Provider への置き換え実装計画書

> **ステータス**: `[ACTIVE]`

**Goal:** Groq / Cloudflare などの自前 HTTP ペイロード構築・送信処理（`reqwest`）を、`rig-core` の OpenAI 互換の共通クライアント（`CompletionsClient`）を用いた実装にリファクタリングする。

**Architecture / Approach:**
* **ライブラリ依存の統一**: 
  * `rig-core` 0.38 が要求する `reqwest::Client` (v0.13) と型を揃えるため、`rustyclaw-providers` 内の `reqwest` のバージョン指定を `0.12` から `0.13` に引き上げる。
  * また、v0.13 に伴い、`Cargo.toml` の TLS フィーチャー指定を `rustls-tls` から `rustls` に修正する（すでに検証・変更済み）。
* **ヘッダーインジェクションの統合**:
  * Cloudflare AI Gateway 経由時のカスタムヘッダー（`cf-aig-gateway-id` や `cf-aig-authorization`）は、`CompletionsClient` のビルダー生成時にあらかじめヘッダーを仕込んだ `reqwest::Client` を `.http_client(...)` メソッドで注入して対応する。
* **対話・ストリーミング処理の共通化**:
  * `OpenAiCompatProvider::complete` を `rig_core::completion::CompletionModel::completion` に置き換える。
  * `OpenAiCompatProvider::complete_stream` を `rig_core::completion::CompletionModel::stream` に置き換え、返されるストリームをマッピングするロジックに簡素化する。

**Tech Stack:** Rust 2024 / `crates/rustyclaw-providers` / `rig-core` 0.38 / `reqwest` 0.13

---

## 実装手順とチェックリスト

- [x] **Step 1: 依存関係の更新（Cargo.toml）**
  * `crates/rustyclaw-providers/Cargo.toml` 内の `reqwest` を `0.13` にし、`rustls` フィーチャーを設定する。（完了）

- [ ] **Step 2: `OpenAiCompatProvider` の構造体定義と初期化の変更**
  * `crates/rustyclaw-providers/src/lib.rs` の `OpenAiCompatProvider` 構造体の定義を変更する：
    * `client: reqwest::Client` を `client: rig_core::providers::openai::CompletionsClient` に変更。
  * `OpenAiCompatProvider::new` 内での初期化処理を変更する：
    * モデル構成（`cf-aig-gateway-id` 等）に応じてカスタムヘッダーを仕込んだ `reqwest::Client` をビルドする。
    * `CompletionsClient::builder()` を使い、`.api_key()`, `.base_url()`, `.http_client()` を設定して `CompletionsClient` を構築する。

- [ ] **Step 3: `complete` メソッドのリファクタリング**
  * 自前の HTTP JSON リクエストの構築と、`OpenAiResponse` 等のレスポンスパース記述を排除する。
  * 渡された `messages` や `tools` を `provider_messages_to_rig` や `rig_core` 型にマッピングする。
  * `self.client.completion_model(&opts.model)` を取得し、`CompletionRequest` を構築して `.completion(request).await` を呼ぶ。
  * 返却された completion レスポンスから `LlmResponse` を再構成し、既存の neurons 記録・IO ログ出力（`dump_llm_io`）を行う。

- [ ] **Step 4: `complete_stream` メソッドのリファクタリング**
  * 自前で SSE バイトストリームを `data: ` の行ごとに分解・デシリアライズしていた長いロジックを完全に削除する。
  * `self.client.completion_model(&opts.model)` から `.stream(request).await` を呼び出して Rig のストリームオブジェクトを取得する。
  * `async_stream::try_stream!` を使って Rig の `StreamedAssistantContent` を `StreamChunk` にマッピングして yield する軽量な処理へ差し替える。

- [ ] **Step 5: テスト・静的解析の実行**
  * `crates/rustyclaw-providers` 内の既存のモックテスト `test_openai_compat_complete` / `test_openai_compat_complete_stream` が正しく通るようにアサーションやモックレスポンス構造を修正する。
  * `cargo test --workspace` および `cargo clippy` を実行し、全件通過を確認する。

- [ ] **Step 6: PR 作成とマージ、仕様書の更新**
  * マージ後、`docs/task.md` の `40-1` に完了マーク（`[x]`）をつける。
