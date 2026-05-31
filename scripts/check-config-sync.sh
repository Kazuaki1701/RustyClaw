#!/usr/bin/env bash
# config.debug.json と config.release.json の model_list 差分を検出する。
# 差分があれば非ゼロ終了（CI/デプロイ前チェック用）。
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)/production/config"
DEBUG="$DIR/config.debug.json"
RELEASE="$DIR/config.release.json"

for f in "$DEBUG" "$RELEASE"; do
  [ -f "$f" ] || { echo "ERROR: not found: $f"; exit 2; }
done

# model_list を正規化して比較する。
# `enabled` は profile ごとに意図的に異なる（例: lms-gemma を release で無効化）ため除外し、
# モデルの追加/削除/設定値の偶発的ドリフトのみを検出する。
d=$(jq -S '[.model_list[] | del(.enabled)]' "$DEBUG")
r=$(jq -S '[.model_list[] | del(.enabled)]' "$RELEASE")
if [ "$d" == "$r" ]; then
  echo "[OK] model_list is in sync between debug and release."
  exit 0
else
  echo "[DRIFT] model_list differs between config.debug.json and config.release.json:"
  diff <(echo "$d") <(echo "$r") || true
  exit 1
fi
