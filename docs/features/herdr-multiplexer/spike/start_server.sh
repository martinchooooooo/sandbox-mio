# The single-exec idempotent start: runs inside the container as `sh start_server.sh <session>`; returns at once, leaves the server running.
herdr --session "$1" status server | grep -q '^status: running' || setsid herdr --session "$1" server >/dev/null 2>&1 </dev/null &
