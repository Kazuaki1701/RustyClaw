#!/usr/bin/env bash
# LM Studio のロード済み context 長と、RustyClaw config の context_window を比較する。
# 使い方: ./scripts/check-lmstudio-context.sh <base_host:port> <model_id> <config_ctx_tokens>
# 例:      ./scripts/check-lmstudio-context.sh 192.168.1.110:1234 google/gemma-4-e4b 16384
set -euo pipefail
HOSTPORT="${1:?base host:port required}"
MODEL="${2:?model id required}"
CFG_CTX="${3:?config context tokens required}"

json=$(curl -s -m 5 "http://${HOSTPORT}/api/v0/models") || { echo "ERROR: cannot reach LM Studio at ${HOSTPORT}"; exit 2; }
loaded=$(echo "$json" | jq -r --arg m "$MODEL" '[.data[]? | select(.id==$m) | .loaded_context_length // empty][0] // empty')
state=$(echo "$json" | jq -r --arg m "$MODEL" '[.data[]? | select(.id==$m) | .state // "unknown"][0] // "unknown"')

if [ -z "$loaded" ] || [ "$loaded" == "null" ]; then
  echo "[WARN] model '$MODEL' not loaded (state=$state). Load it in LM Studio first."
  exit 1
fi
echo "model=$MODEL state=$state loaded_context_length=$loaded config_context=$CFG_CTX"
if [ "$CFG_CTX" -gt "$loaded" ]; then
  echo "[MISMATCH] config context_window ($CFG_CTX) > LM Studio loaded ($loaded). Reload model with larger context or lower config."
  exit 1
fi
echo "[OK] config context fits within LM Studio loaded context."
