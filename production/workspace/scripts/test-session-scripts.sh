#!/bin/bash
# test-session-scripts.sh — TDD test harness for session-stats.sh and session-search.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PASS=0
FAIL=0
ERRORS=()

# ── ヘルパー ─────────────────────────────────────────────
assert_contains() {
  local label="$1" expected="$2" actual="$3"
  if echo "$actual" | grep -q "$expected"; then
    PASS=$((PASS + 1))
    echo "  ✅ $label"
  else
    FAIL=$((FAIL + 1))
    ERRORS+=("FAIL: $label — expected to contain: '$expected'")
    echo "  ❌ $label"
    echo "     expected: '$expected'"
    echo "     actual:   $(echo "$actual" | head -5)"
  fi
}

assert_not_contains() {
  local label="$1" unexpected="$2" actual="$3"
  if echo "$actual" | grep -q "$unexpected"; then
    FAIL=$((FAIL + 1))
    ERRORS+=("FAIL: $label — expected NOT to contain: '$unexpected'")
    echo "  ❌ $label"
  else
    PASS=$((PASS + 1))
    echo "  ✅ $label"
  fi
}

assert_exit() {
  local label="$1" expected_code="$2"
  shift 2
  set +e
  "$@" > /dev/null 2>&1
  local actual_code=$?
  set -e
  if [ "$actual_code" -eq "$expected_code" ]; then
    PASS=$((PASS + 1))
    echo "  ✅ $label (exit $actual_code)"
  else
    FAIL=$((FAIL + 1))
    ERRORS+=("FAIL: $label — expected exit $expected_code, got $actual_code")
    echo "  ❌ $label (expected exit $expected_code, got $actual_code)"
  fi
}

setup_workspace() {
  local ws
  ws=$(mktemp -d)
  mkdir -p "$ws/sessions"
  echo "$ws"
}

teardown() {
  local ws="$1"
  rm -rf "$ws"
}

# ── session-stats.sh テスト ──────────────────────────────
echo ""
echo "=== session-stats.sh ==="

# Test 1: セッションが存在しない場合でもヘッダーを出力する
WS=$(setup_workspace)
OUT=$("$SCRIPT_DIR/session-stats.sh" --workspace "$WS" 2>&1 || true)
assert_contains "ヘッダーを出力する" "Session Files" "$OUT"
teardown "$WS"

# Test 2: セッションファイルを一覧表示し、メッセージ数を示す
WS=$(setup_workspace)
printf '{"role":"user","content":"hello"}\n{"role":"assistant","content":"world"}\n' \
  > "$WS/sessions/test-session.jsonl"
OUT=$("$SCRIPT_DIR/session-stats.sh" --workspace "$WS" 2>&1 || true)
assert_contains "セッション名を表示する" "test-session" "$OUT"
assert_contains "メッセージ数 2 を表示する" "2 msgs" "$OUT"
teardown "$WS"

# Test 3: --date フィルターで対象日のみ表示する
WS=$(setup_workspace)
printf '{"role":"user","content":"a"}\n' > "$WS/sessions/discord-C123-20260531.jsonl"
printf '{"role":"user","content":"b"}\n' > "$WS/sessions/discord-C123-20260530.jsonl"
OUT=$("$SCRIPT_DIR/session-stats.sh" --workspace "$WS" --date "20260531" 2>&1 || true)
assert_contains     "--date: 対象日のセッションを含む"   "20260531" "$OUT"
assert_not_contains "--date: 対象外日のセッションを除く" "20260530" "$OUT"
teardown "$WS"

# Test 4: sqlite3 が利用不可でも終了コード 0 で完了する
WS=$(setup_workspace)
assert_exit "sqlite3 未インストールでも正常終了する" 0 \
  "$SCRIPT_DIR/session-stats.sh" --workspace "$WS"
teardown "$WS"

# ── session-search.sh テスト ─────────────────────────────
echo ""
echo "=== session-search.sh ==="

# Test 5: キーワードなしで使用法を表示して exit 1
assert_exit "キーワードなしで exit 1" 1 \
  "$SCRIPT_DIR/session-search.sh"

WS=$(setup_workspace)
OUT=$("$SCRIPT_DIR/session-search.sh" 2>&1 || true)
assert_contains "使用法を表示する" "Usage" "$OUT"

# Test 6: キーワードにマッチするセッションを表示する
WS=$(setup_workspace)
printf '{"role":"user","content":"hello world test"}\n{"role":"assistant","content":"got it"}\n' \
  > "$WS/sessions/discord-C999-20260531.jsonl"
OUT=$("$SCRIPT_DIR/session-search.sh" "hello" --workspace "$WS" 2>&1 || true)
assert_contains "マッチしたセッション名を表示する" "discord-C999-20260531" "$OUT"
assert_contains "マッチ数を表示する" "match" "$OUT"
teardown "$WS"

# Test 7: マッチなしのとき "No matches found" を出力する
WS=$(setup_workspace)
printf '{"role":"user","content":"something else entirely"}\n' \
  > "$WS/sessions/other.jsonl"
OUT=$("$SCRIPT_DIR/session-search.sh" "zzznomatch" --workspace "$WS" 2>&1 || true)
assert_contains "マッチなしのメッセージを出力する" "No matches found" "$OUT"
teardown "$WS"

# Test 8: --date フィルターで対象日のセッションのみ検索する
WS=$(setup_workspace)
printf '{"role":"user","content":"target keyword here"}\n' \
  > "$WS/sessions/discord-C1-20260531.jsonl"
printf '{"role":"user","content":"target keyword here too"}\n' \
  > "$WS/sessions/discord-C1-20260530.jsonl"
OUT=$("$SCRIPT_DIR/session-search.sh" "target" --workspace "$WS" --date "20260531" 2>&1 || true)
assert_contains     "--date: 対象日のセッションを含む"   "discord-C1-20260531" "$OUT"
assert_not_contains "--date: 対象外日のセッションを除く" "discord-C1-20260530" "$OUT"
teardown "$WS"

# ── 結果 ─────────────────────────────────────────────────
echo ""
echo "================================"
echo "Results: $PASS passed, $FAIL failed"
if [ "${#ERRORS[@]}" -gt 0 ]; then
  echo ""
  for e in "${ERRORS[@]}"; do echo "  $e"; done
fi
echo "================================"

[ "$FAIL" -eq 0 ] && exit 0 || exit 1
