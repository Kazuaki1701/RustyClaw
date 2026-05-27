# RustyClaw Phase 1 修正計画 — デバッグ用 `gmn` CLI プロバイダの追加

デバッグおよびローカル環境での動作確認を安全かつ確実に行うため、本物の API 通信の代わりに、すでに認証済みのローカルコマンド **`gmn` (Gemini CLI)** を子プロセスとして起動し、LLM 通信を行うための特別プロバイダ `GmnCliProvider` を実装・追加します。

---

## 1. Goal Description

- `rustyclaw-providers` に `GmnCliProvider` 構造体を新規追加します。
- `config.json` で `"model_provider": "gmn"` が指定された場合、このプロバイダをファクトリから動的生成します。
- `tokio::process::Command` を使用して `gmn` プロセスをバックグラウンド起動し、対話を仲介します。
- ストリーミング対話（`complete_stream`）では、子プロセスの標準出力（`stdout`）を非同期に逐次読み取ってコンソールへストリーム配送します。

---

## 2. User Review Required

> [!NOTE]
> **gmn CLI の動作要件**
> 本実装は、開発環境（および実機）のパス上に `gmn` バイナリがインストールされており、`~/.gemini/` の認証が完了していることを前提とします。
> 
> **プロンプトとコンテキストの統合**
> `gmn` コマンドへシステムプロンプトや対話履歴を渡すため、メッセージ群（`Vec<Message>`）を一時的に構造化されたフラットテキスト（例: `[SYSTEM]: ... \n [USER]: ...`）に整形し、`gmn` の入力プロンプトとして統合して渡します。

---

## 3. Proposed Changes

### [Component] rustyclaw-providers (LLMプロバイダライブラリ)

#### [MODIFY] [provider.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-providers/src/lib.rs)
- `GmnCliProvider` 構造体の実装。
- `tokio::process::Command` による非同期実行および標準出力バッファのパース処理。
  - ストリーミング：`tokio::io::ReaderStream` を用いて、子プロセスの生出力を `StreamChunk` にマッピングします。
- `create_provider` ファクトリに `"gmn"` のディスパッチを追加。

---

### [Component] プロジェクト構成・設定

#### [MODIFY] [config.json](file:///home/kazuaki/Projects/RustyClaw/config.json)
- デバッグ用に初期プロバイダを `"gmn"` に切り替えます。
  ```json
  {
    "model_provider": "gmn",
    "model_name": "flash",
    "api_key": "not_required",
    "api_base_url": "not_required",
    "max_tokens": 2048,
    "temperature": 0.7,
    "debug_dump": true
  }
  ```

---

## 4. Verification Plan

### 自動検証（ビルドチェック）
```bash
cargo check
cargo test -p rustyclaw-providers
```

### 手動検証
1. `config.json` を `"model_provider": "gmn"` に変更します。
2. 動作確認スクリプトを実行し、ローカルの `gmn` 経由で Google Gemini からのストリーミング応答が正常にコンソールに流れることを検証します。
   ```bash
   ./test_api.sh
   ```
