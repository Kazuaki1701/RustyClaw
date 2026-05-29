#!/bin/bash
set -euo pipefail

HOST="rp1"
SERVICE="rustyclaw"

usage() {
    echo "Usage: $0 {start|stop|restart|status}"
    exit 1
}

[[ $# -ne 1 ]] && usage

case "$1" in
    start)
        echo "Starting $SERVICE on $HOST..."
        ssh "$HOST" "sudo systemctl start $SERVICE"
        sleep 2
        ssh "$HOST" "sudo systemctl status $SERVICE --no-pager | head -6"
        ;;
    stop)
        echo "Stopping $SERVICE on $HOST..."
        ssh "$HOST" "sudo systemctl stop $SERVICE"
        ssh "$HOST" "sudo systemctl status $SERVICE --no-pager | head -4"
        ;;
    restart)
        echo "Restarting $SERVICE on $HOST..."
        ssh "$HOST" "sudo systemctl restart $SERVICE"
        sleep 2
        ssh "$HOST" "sudo systemctl status $SERVICE --no-pager | head -6"
        ;;
    status)
        ssh "$HOST" "sudo systemctl status $SERVICE --no-pager"
        ;;
    *)
        usage
        ;;
esac
