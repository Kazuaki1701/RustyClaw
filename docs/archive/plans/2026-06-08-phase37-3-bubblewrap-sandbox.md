# Bubblewrap Sandbox Implementation Plan (Phase 37-3)

ラズパイ実機運用環境における安全性を最大化し、エージェントが実行するスクリプトによるホストOSやSSDの破壊を防ぐため、`bwrap` (Bubblewrap) による実行サンドボックス化を導入します。

---

## 1. 概要と要件

### Bubblewrap (`bwrap`) とは
非特権ユーザー向けの軽量なコンテナ・サンドボックス化ツールです。名前空間の隔離（Mount, UTS, IPC, PID, Network など）を利用して、特定のコマンドを隔離された環境で安全に実行できます。

### サンドボックスの隔離要件
- **ファイルシステムの保護**:
  - `/` 全体を**読み取り専用** (`--ro-bind / /`) でマウント。
  - `/dev`、`/proc`、`/tmp` はそれぞれ必要な隔離領域としてマウント。
  - エージェントの書き込み領域である `<workspace_dir>` のみ、**読み書き可能** (`--bind <workspace_dir> <workspace_dir>`) でマウント。
- **ネットワーク・通信**:
  - ネットワーク名前空間は共有 (`--share-net`) します。
- **ポータビリティ (自動フォールバック)**:
  - `bwrap` が利用不可能な環境では、警告ログを出力した上で従来の非隔離実行にフォールバック（fail-open）させ、利便性を維持します。

---

## 2. 具体的な実装設計

元の実行コマンドを `bwrap` コマンドラインでラッピングします。

```rust
        let mut cmd = if use_sandbox {
            tracing::info!("Running script '{}' inside Bubblewrap sandbox", script_name);
            let mut c = tokio::process::Command::new("bwrap");
            c.arg("--ro-bind").arg("/").arg("/")
             .arg("--dev").arg("/dev")
             .arg("--proc").arg("/proc")
             .arg("--tmpfs").arg("/tmp")
             .arg("--bind").arg(&self.workspace_dir).arg(&self.workspace_dir)
             .arg("--share-net")
             .arg("--")
             .arg(original_program);
            c.args(&original_args);
            c
        } else {
            tracing::warn!("bwrap command not found. Running script without sandbox protection!");
            let mut c = tokio::process::Command::new(original_program);
            c.args(&original_args);
            c
        };
```

---

## 3. テスト計画
- `/workspace` ディレクトリ配下へのファイルの読み書きが正常に行えることを検証。
- `/workspace` 以外のシステム領域への書き込みが `Permission denied`（`ReadOnly`）で拒否されることを検証。
- `bwrap` が存在しない場合のフォールバックの動作。
