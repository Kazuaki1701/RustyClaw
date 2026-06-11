#!/bin/bash
# backup.sh — RustyClaw 本番 workspace を NAS へ rsync バックアップ
# systemd user timer (rustyclaw-backup.timer) から起動される。

set -euo pipefail

# ── 設定（初回セットアップ時に BACKUP_DEST を設定すること） ──────────────────
# SSH 鍵認証済みホストへのパス例: "qnap:/backup/rustyclaw"
# 環境変数 RUSTYCLAW_BACKUP_DEST でもオーバーライド可能。
BACKUP_DEST="${RUSTYCLAW_BACKUP_DEST:-}"
# ────────────────────────────────────────────────────────────────────────────

WORKSPACE="$HOME/.rustyclaw/workspace"
STAMP="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

log() { echo "[rustyclaw-backup] $*"; }

if [[ -z "$BACKUP_DEST" ]]; then
    log "ERROR: BACKUP_DEST が未設定。RUSTYCLAW_BACKUP_DEST 環境変数か本スクリプト冒頭の変数を設定してください。" >&2
    exit 1
fi

log "開始 → $BACKUP_DEST  ($STAMP)"

# memory.db: SQLite ファイル（上書き転送）
rsync -az --checksum --inplace \
    "$WORKSPACE/memory.db" \
    "$BACKUP_DEST/memory.db"

# sessions/: 増分転送、NAS 側の不要ファイルを削除
rsync -az --checksum --delete \
    "$WORKSPACE/sessions/" \
    "$BACKUP_DEST/sessions/"

# patrol/: 同上
rsync -az --checksum --delete \
    "$WORKSPACE/patrol/" \
    "$BACKUP_DEST/patrol/"

log "完了 ($STAMP)"
