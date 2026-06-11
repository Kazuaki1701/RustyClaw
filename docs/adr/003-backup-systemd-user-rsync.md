# ADR 003: 本番バックアップ方式の選定

- **ステータス**: `[ACCEPTED]`
- **決定日**: 2026-06-12
- **関連タスク**: 本番自動バックアップ（将来課題）

## 1. コンテキスト（直面した課題）

`production/workspace/`（`memory.db` 61 MB・`sessions/` 5 MB・`patrol/` 24 KB）は Gateway の停止によってデータが失われるリスクがある。定期的に NAS へ退避する仕組みが必要。

## 2. 検討した選択肢（Options）

- **案 A: systemd user timer + rsync**
  - メリット: Gateway 死活と独立して動作、失敗時 journald 記録、追加 crate 不要
  - デメリット: NAS への SSH 鍵設定が別途必要

- **案 B: RustyClaw cron ジョブとして bash 実行**
  - メリット: ダッシュボードから状態確認可能
  - デメリット: Gateway が落ちていると backup も止まる

- **案 C: sessions/patrol のみ git push**
  - メリット: 追加インフラ不要
  - デメリット: memory.db（最重要・61 MB）が対象外

## 3. 決定と選定理由（Decision & Justification）

- **決定**: 案 A（systemd user timer + rsync）
- **理由**: バックアップは Gateway の死活と独立して動くことが最重要。user モードのため root 不要。`Persistent=true` により RPi4 停止中にスキップされた分も次回起動時に補完される。

## 4. トレードオフ・今後の影響（Consequences）

- NAS への SSH 公開鍵登録が初期設定として必要（`ssh-copy-id` または QNAP 管理画面）
- `BACKUP_DEST` を `backup.sh` 内または `RUSTYCLAW_BACKUP_DEST` 環境変数で設定する運用手順が必要
