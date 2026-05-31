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

# model_list を正規化（キー順ソート）して比較
d=$(jq -S '.model_list' "$DEBUG")
r=$(jq -S '.model_list' "$RELEASE")
if [ "$d" == "$r" ]; then
  echo "[OK] model_list is in sync between debug and release."
  exit 0
else
  echo "[DRIFT] model_list differs between config.debug.json and config.release.json:"
  diff <(echo "$d") <(echo "$r") || true
  exit 1
fi
