# 実装計画書: Phase 2 ファイルロックの導入と並行数 4 への拡張 (ギャップ A の解消)

本ドキュメントは、RustyClaw のグローバル並行実行スロット (`gmn_sem`) を容量 `1` から **`4`** へ安全に拡張するため、共有ワークスペースファイルへの並行アクセスを調停する **「インプロセス非同期パスロック (In-process Path Lock)」** を実装する詳細計画書です。

---

## 1. 目的

現在、RustyClaw はデータ破損を防ぐためにシステム全体での同時実行数を最大 1 件に制限していますが、これにより複数ユーザーや複数チャンネルからの並行対話がすべて直列にブロックされる問題が発生しています。

本フェーズでは以下の2点を達成し、安全性とスケーラビリティを両立します。
1. **インプロセス非同期パスロックの導入**:
   - `MEMORY.md` や `USER.md` などの主要な共有ファイルパスに対して、非同期の `RwLock` (Read-Write Lock) を動的に生成して調停します。
   - 複数セッションが同時にファイルを「読み込む」ことは並行で許可し、「書き込む」ときは完全に排他制御します。これによりデータのロストアップデート（上書き消失）を防ぎます。
2. **並行数制限の緩和 (Capacity: 4)**:
   - ファイルの安全性が確保されたため、Gateway 内のグローバル実行セマフォ `gmn_sem` の容量を `1` から **`4`** に拡張します。これにより、異なるセッション同士であれば最大4件まで完全に並行実行できるようになります。

---

## 2. 変更予定のファイル一覧

* **`crates/rustyclaw-storage/Cargo.toml`**
  - `once_cell` への依存を追加（インプロセスでスレッドセーフなグローバルロックマップを保持するため）。
* **`crates/rustyclaw-storage/src/lib.rs`**
  - `PATH_LOCKS` グローバルマップの定義。
  - `acquire_read_lock(path)` / `acquire_write_lock(path)` 関数の実装。
  - `atomic_write` 実行時の書き込みロック自動取得の組み込み。
* **`crates/rustyclaw-agent/src/lib.rs`**
  - `MEMORY.md` や `USER.md` を読み込む処理（`build_system_context` や `flush_memory` 内など）の直前で `acquire_read_lock` / `acquire_write_lock` を適切に呼び出すよう修正。
* **`crates/rustyclaw-gateway/src/lib.rs`**
  - グローバルセマフォ `gmn_sem` の容量定義を `1` から `4` に変更。
  - `crates/rustyclaw-gateway/src/health.rs` 内の HTTP ヘルスチェックにおける容量表示 (`gmn_capacity`) も `4` に追従。

---

## 3. 詳細設計

### 3.1. `rustyclaw-storage` でのパスロック実装

```rust
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{RwLock, OwnedRwLockReadGuard, OwnedRwLockWriteGuard};

static PATH_LOCKS: Lazy<StdMutex<HashMap<PathBuf, Arc<RwLock<()>>>>> = Lazy::new(|| {
    StdMutex::new(HashMap::new())
});

/// パスを正規化（実在しなくても親ディレクトリから辿れるようにフォールバック）
fn canonicalize_path(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    }
}

/// 指定したファイルパスの非同期読み込みロック（共有ロック）を取得する
pub async fn acquire_read_lock(path: &Path) -> OwnedRwLockReadGuard<()> {
    let normalized = canonicalize_path(path);
    let lock = {
        let mut locks = PATH_LOCKS.lock().unwrap();
        locks.entry(normalized).or_insert_with(|| Arc::new(RwLock::new(()))).clone()
    };
    lock.read_owned().await
}

/// 指定したファイルパスの非同期書き込みロック（排他ロック）を取得する
pub async fn acquire_write_lock(path: &Path) -> OwnedRwLockWriteGuard<()> {
    let normalized = canonicalize_path(path);
    let lock = {
        let mut locks = PATH_LOCKS.lock().unwrap();
        locks.entry(normalized).or_insert_with(|| Arc::new(RwLock::new(()))).clone()
    };
    lock.write_owned().await
}
```

* **`atomic_write` への統合**:
  `atomic_write` 自体の中で自動的に `acquire_write_lock(path).await` を取得するようにし、すべてのファイル書き込み操作において意識することなく排他制御が行われるようにします。
  ※ `atomic_write` が `async fn` になるため、呼び出し元の箇所も一部 `await` を付与する必要があります。

### 3.2. ゲートウェイの並行セマフォ拡張

* `rustyclaw-gateway/src/lib.rs` (809行目付近):
  ```rust
  let gmn_sem = Arc::new(Semaphore::new(4)); // 容量を4に拡張
  ```
* `rustyclaw-gateway/src/health.rs` (ヘルスチェックの初期化):
  ```rust
  let gmn_capacity = 4;
  ```

---

## 4. 具体的な実装タスクリスト

### [ ] タスク 1: `rustyclaw-storage` に非同期パスロックを追加
- [ ] `crates/rustyclaw-storage/Cargo.toml` に `once_cell = "1.18"` 依存関係を追加。
- `crates/rustyclaw-storage/src/lib.rs` に `PATH_LOCKS` および `acquire_read_lock` / `acquire_write_lock` を実装。
- [ ] `atomic_write` を `pub async fn atomic_write` に変更し、内部で自動的に `_guard = acquire_write_lock` を取得・保持して書き込みを実行するように改修。
- [ ] `DbManager` などの SQLite アクセスは、すでに `WAL` モードおよび SQLite 自体の接続プールロックによって並行安全になっているため追加のパスロックは不要であることを確認。

### [ ] タスク 2: `rustyclaw-agent` での読み込み時のパスロック調調停と、`atomic_write` の非同期化対応
- [ ] `crates/rustyclaw-agent/src/lib.rs` 内で `atomic_write` を呼び出している箇所を `.await` に変更。
- [ ] エージェントの `build_system_context` 実行時、`MEMORY.md` や `USER.md` などの読み込みの直前で `acquire_read_lock` を非同期で取得して借用。
- [ ] `flush_memory` 内での読み書きシーケンス時に、書き込みロック `acquire_write_lock` を取得して処理全体を包む。

### [ ] タスク 3: ゲートウェイでの `gmn_sem` の容量拡張 (容量 4)
- [ ] `crates/rustyclaw-gateway/src/lib.rs` にて `Semaphore::new(1)` から `Semaphore::new(4)` に変更。
- [ ] `crates/rustyclaw-gateway/src/health.rs` の `gmn_capacity` の値を `4` に更新。
- [ ] 既存のテストコードにおけるセマフォ容量も確認し、適切に同期。

---

## 5. 検証およびテスト計画

### 5.1. ユニットテストの実行
- [ ] `cargo test` を実行して、ビルドが通り既存のテスト（特に atomic_write や LaneRegistry 関連）が全て成功することを確認。
- [ ] `rustyclaw-storage` において、同じファイルを複数タスクが同時に並行で読み書きしようとした際、正しく排他制御されデッドロックしないことを検証する新しいユニットテストを追加。

### 5.2. 統合手動テスト（RPi 環境での動作確認）
- [ ] 開発完了後、RPi 環境 (`rp1`) にデプロイ。
- [ ] ダッシュボード (`http://192.168.1.12:8080/`) でヘルスステータスのセマフォ容量が `4` になっていることを確認。
- [ ] 異なる2つの Discord チャンネルで、同時にエージェントへ対話を送信し、両チャンネルの「タイピング中...」が同時に表示され、完全に並行して応答が生成されることを確認。
