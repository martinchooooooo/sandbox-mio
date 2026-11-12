#!/bin/sh
# Supervisor for the native chat: keeps one `pi --mode rpc` alive per sandbox, independent of any
# attach stream (docs/features/native-agent-ui/02_dev.md, In-sandbox supervisor).
# usage: pi-host.sh ensure <script-version> | status | stop | takeover
set -eu

# Bump with every change to this file or pi-attach.sh; the app reinstalls both on a mismatch.
SCRIPT_VERSION=2
STATE=/run/mio/pi-host
WORK=/root/work
# Mirrors CHAT_SESSION_DIR in src-tauri/src/agent_host/sessions.rs; keep the two equal.
CHAT_SESSION_DIR=/root/work/.mio/pi-agent/chat-sessions

# /run is not a tmpfs, so a stale pi.pid survives a sandbox restart and post-restart pids are low;
# the cmdline check rejects an unrelated process that reused the pid.
running() {
  [ -f "$STATE/pi.pid" ] || return 1
  pid=$(cat "$STATE/pi.pid")
  kill -0 "$pid" 2>/dev/null || return 1
  tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null | grep -q -- '--mode rpc'
}

start() {
  rm -f "$STATE/in.fifo" "$STATE/writer.pid"
  mkfifo "$STATE/in.fifo"
  # The input log starts with the journal: a dialog answered before it existed would stay pending.
  : > "$STATE/journal.jsonl"
  : > "$STATE/input.jsonl"
  : > "$STATE/stderr.log"
  date +%s%N > "$STATE/generation"
  cd "$WORK"
  mise exec -- pi --version 2>/dev/null | grep -o '[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*' | head -n 1 > "$STATE/pi.version" || true
  # The keeper holds a write end of the pipe, so pi never sees end-of-file between clients.
  # Killing it is the orderly stop: pi treats end-of-file on stdin as a shutdown request.
  setsid sh -c "exec sleep infinity > '$STATE/in.fifo'" </dev/null >/dev/null 2>&1 8>&- &
  echo $! > "$STATE/keeper.pid"
  setsid mise exec -- pi --mode rpc --session-dir "$CHAT_SESSION_DIR" \
    < "$STATE/in.fifo" >> "$STATE/journal.jsonl" 2>> "$STATE/stderr.log" 8>&- &
  echo $! > "$STATE/pi.pid"
  sleep 0.5
}

case "${1:-status}" in
ensure)
  [ "${2:-}" = "$SCRIPT_VERSION" ] || exit 10
  mkdir -p "$STATE"
  # Serialises concurrent ensures so two attaches never start two pi processes.
  exec 8>> "$STATE/start.lock"
  flock 8
  started=false
  if ! running; then
    start
    started=true
    if ! running; then
      tail -n 20 "$STATE/stderr.log" >&2
      exit 4
    fi
  fi
  printf '{"type":"mio_host","generation":"%s","pi_version":"%s","started":%s}\n' \
    "$(cat "$STATE/generation")" "$(cat "$STATE/pi.version" 2>/dev/null || true)" "$started"
  # Pending dialogs are derived by the app from these lines (Resolved Decision 3).
  grep -F '"type":"extension_ui_request"' "$STATE/journal.jsonl" || true
  grep -F '"type":"extension_ui_response"' "$STATE/input.jsonl" || true
  ;;
status)
  running || exit 1
  ;;
stop)
  [ -f "$STATE/keeper.pid" ] && kill "$(cat "$STATE/keeper.pid")" 2>/dev/null || true
  i=0
  while running && [ $i -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
  running && kill "$(cat "$STATE/pi.pid")" 2>/dev/null || true
  # Ends every attach stream, so each client sees its stream close instead of a silent pipe.
  pkill -f pi-attach.sh 2>/dev/null || true
  ;;
takeover)
  if [ -f "$STATE/writer.pid" ]; then
    pid=$(cat "$STATE/writer.pid")
    if tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null | grep -q pi-attach.sh; then
      pkill -P "$pid" 2>/dev/null || true
      kill "$pid" 2>/dev/null || true
    fi
    rm -f "$STATE/writer.pid"
  fi
  # Returns only once the lock is free, so the caller's next attach becomes the writer.
  i=0
  until flock -n "$STATE/writer.lock" true; do
    [ $i -lt 30 ] || exit 5
    sleep 0.1
    i=$((i + 1))
  done
  ;;
*)
  exit 2
  ;;
esac
