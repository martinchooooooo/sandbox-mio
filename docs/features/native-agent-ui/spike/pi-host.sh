#!/bin/sh
# Spike supervisor: keeps one `pi --mode rpc` alive per sandbox, independent of any SSH session.
# usage: pi-host.sh start|status|stop
# State lives in /run, sandbox-local, so it never reaches the workspace or Mutagen. Under Podman
# /run is NOT a tmpfs: the state survives a sandbox restart, and so does a stale pid file.
set -eu
STATE=/run/mio/pi-host
WORK=/root/work

# PIDs restart low after a sandbox restart, so a bare `kill -0` on a stale pid can hit an unrelated
# process; the cmdline check rejects that.
running() {
  [ -f "$STATE/pi.pid" ] || return 1
  pid=$(cat "$STATE/pi.pid")
  kill -0 "$pid" 2>/dev/null && tr '\0' ' ' < "/proc/$pid/cmdline" | grep -q -- '--mode rpc'
}

case "${1:-status}" in
start)
  if running; then echo "running generation=$(cat "$STATE/generation")"; exit 0; fi
  mkdir -p "$STATE"
  rm -f "$STATE/in.fifo"
  mkfifo "$STATE/in.fifo"
  : > "$STATE/journal.jsonl"
  date +%s%N > "$STATE/generation"
  # The keeper holds a write end of the pipe, so pi never sees end-of-file between clients.
  # Killing the keeper is the orderly-stop lever: pi treats stdin EOF as a shutdown request.
  setsid sh -c "exec sleep infinity > '$STATE/in.fifo'" </dev/null >/dev/null 2>&1 &
  echo $! > "$STATE/keeper.pid"
  cd "$WORK"
  setsid mise exec -- pi --mode rpc < "$STATE/in.fifo" >> "$STATE/journal.jsonl" 2>> "$STATE/stderr.log" &
  echo $! > "$STATE/pi.pid"
  sleep 0.5
  running && echo "started generation=$(cat "$STATE/generation") pid=$(cat "$STATE/pi.pid")"
  ;;
status)
  if running; then echo "running generation=$(cat "$STATE/generation") pid=$(cat "$STATE/pi.pid")"; else echo stopped; exit 1; fi
  ;;
stop)
  [ -f "$STATE/keeper.pid" ] && kill "$(cat "$STATE/keeper.pid")" 2>/dev/null || true
  i=0
  while running && [ $i -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
  if running; then kill "$(cat "$STATE/pi.pid")"; echo "stopped (killed after timeout)"; else echo "stopped (orderly)"; fi
  ;;
esac
