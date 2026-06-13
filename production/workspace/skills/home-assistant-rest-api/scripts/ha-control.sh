#!/bin/sh
# ha-control.sh — Home Assistant 統合制御スクリプト
# Usage: ha-control.sh <subcommand> [options]
#
# Subcommands:
#   logs          エラーログ取得（凝縮済み）
#   health        バッテリー・OFFLINE・停止センサー確認
#   summary       主要センサーの状態サマリー
#   all_states    全エンティティ一括取得
#   report        統合レポート（logs + health + summary）
#   env_snapshot  環境スナップショット（memory/ha-state.json 更新）

SCRIPTS_DIR="$(dirname "$0")"
CMD="${1:-report}"
shift 2>/dev/null || true

case "$CMD" in
    logs)         exec "$SCRIPTS_DIR/201_ha_logs.sh" "$@" ;;
    health)       exec "$SCRIPTS_DIR/202_ha_health.sh" "$@" ;;
    summary)      exec "$SCRIPTS_DIR/203_ha_summary.sh" "$@" ;;
    all_states)   exec "$SCRIPTS_DIR/204_ha_all_states.sh" "$@" ;;
    report)       exec "$SCRIPTS_DIR/210_ha_report.sh" "$@" ;;
    env_snapshot) exec "$SCRIPTS_DIR/220_ha_env_snapshot.sh" "$@" ;;
    *)
        echo "Unknown subcommand: $CMD" >&2
        echo "Available: logs health summary all_states report env_snapshot" >&2
        exit 1
        ;;
esac
