#!/bin/bash
# findings.md のセクション数が 14 を超えた場合、古いセクションを削除する。
# アルゴリズム: ## の数を N とし、N > 14 のとき
#   上から (N-14)+1 個目の ## の直前の行まで削除する。

set -euo pipefail

WORKSPACE="${WORKSPACE_DIR:-$(cd "$(dirname "$0")/../../.." && pwd)}"
FINDINGS="$WORKSPACE/patrol/findings.md"

if [ ! -f "$FINDINGS" ]; then
    echo "findings.md not found, nothing to prune"
    exit 0
fi

N=$(grep -c "^## " "$FINDINGS" || true)

if [ "$N" -le 14 ]; then
    echo "Sections: $N (≤ 14, no pruning needed)"
    exit 0
fi

TARGET=$((N - 14 + 1))
LINE=$(grep -n "^## " "$FINDINGS" | sed -n "${TARGET}p" | cut -d: -f1)

if [ -z "$LINE" ]; then
    echo "ERROR: could not locate line for section $TARGET" >&2
    exit 1
fi

tail -n +"$LINE" "$FINDINGS" > "${FINDINGS}.tmp" && mv "${FINDINGS}.tmp" "$FINDINGS"

REMOVED=$((TARGET - 1))
KEPT=$((N - REMOVED))
echo "Pruned: removed $REMOVED sections, kept $KEPT sections (line $LINE onward)"
