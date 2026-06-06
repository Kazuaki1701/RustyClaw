# Local Embedding & Complete RAG Unification Implementation Plan (Phase 40-8)

> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書 - 開発完了済み)  
> **完了日**: 2026-06-06  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

**Goal:** Embedding（ベクトル化）処理を RPi4 ローカルで実行させ、外部 API 依存を排除した完全ローカル完結型 RAG システムを構築する。

**Architecture:** 
1. `fastembed-rs` (ONNX Runtimeベース) を `rustyclaw-providers` に導入し、ローカルで `intfloat/multilingual-e5-small` モデル (384次元) を使って Embedding を生成する。
2. データベース (`memory.db`) のベクトル保存テーブルを 1024次元（現在の BGE-M3 用）から **384次元** に変更。次元変更に伴う SQLite マイグレーションを実装。
3. インジェスト処理および検索処理でローカル Embedding クライアントを使用するように切り替える。

**Tech Stack:** Rust 2024 edition, `fastembed` crate, SQLite (`rustyclaw-storage`), tokio async

---

## ファイル構造変更マップ

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-providers/Cargo.toml` | `fastembed = "3.2"` の依存関係を追加 |
| `crates/rustyclaw-providers/src/lib.rs` | `LocalEmbeddingClient` の追加（`fastembed` を使用） |
| `crates/rustyclaw-storage/src/lib.rs` | `memory_embeddings` テーブルの初期化 SQL の変更、および次元数変更に伴うマイグレーション処理 |
| `crates/rustyclaw-config/src/lib.rs` | `config.json` にローカル Embedding 設定オプションを追加 |
| `crates/rustyclaw-agent/src/lib.rs` | RAG インジェスト、検索での Embedding クライアント呼び出しの差し替え |

---

## 実施タスク

### Task 1: providers — `fastembed` の導入とローカルクライアントの実装
- [x] **Step 1**: `crates/rustyclaw-providers/Cargo.toml` に `fastembed` 依存関係を追加。
- [x] **Step 2**: `crates/rustyclaw-providers/src/lib.rs` に `LocalEmbeddingClient` 構造体を実装。
  - 初期化時に `fastembed::TextEmbedding::try_new` を使用して `multilingual-e5-small` モデルを読み込む。
  - `embed` メソッドを実装し、与えられた文字列スライスから `Vec<f32>` (384次元) の配列を出力する。
- [x] **Step 3**: 開発環境で動作確認する単体テストを追加し、テストがパスすることを確認。

### Task 2: storage — ベクトル次元数変更とデータベース・マイグレーションの実装
- [x] **Step 1**: `crates/rustyclaw-storage/src/lib.rs` の `DbManager::new` で `memory_embeddings` テーブル作成時の次元数制約やコメントを更新。
- [x] **Step 2**: データベースのベクトル次元数の移行マイグレーション（1024次元 ➡️ 384次元）の処理を実装。
  - 起動時に既存 DB の次元数が 1024 の場合、`memory_embeddings` テーブルをクリーンアップ（truncate）し、次元数を 384 に切り替える。
  - ドキュメント状態テーブル（`document_states`）をクリアして、起動時に全静的ドキュメントが 384次元で再インジェストされるように促す。
- [x] **Step 3**: マイグレーションのユニットテストを記述・実行。

### Task 3: config & agent — ローカルクライアントの適用と動作検証
- [x] **Step 1**: `crates/rustyclaw-config/src/lib.rs` を更新し、ローカル Embedding モードの有効化オプションをサポート。
- [x] **Step 2**: `crates/rustyclaw-agent/src/lib.rs` の `ingest_static_documents` と類似度検索部分において、設定がローカルモードの場合は `LocalEmbeddingClient` を経由するように修正。
- [x] **Step 3**: ワークスペース全体のテスト（`cargo test --workspace`）を実行し、エラーがないことを確認。
- [x] **Step 4**: RPi4 (aarch64) でのクロスコンパイル環境を確認し、ビルドエラーがないか検証。
