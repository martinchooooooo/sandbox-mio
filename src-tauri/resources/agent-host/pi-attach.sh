#!/bin/sh
# One attach per exec stream: streams the supervisor's journal from a byte offset to stdout and,
# for the single writer, copies stdin into pi's pipe (docs/features/native-agent-ui/02_dev.md).
# usage: pi-attach.sh <generation|-> <offset|end>
# The first stdout line is a synthetic `mio_attach` header; offsets count from the line after it.
# SCRIPT_VERSION=2 (installed together with pi-host.sh, which carries the checked version)
set -eu
STATE=/run/mio/pi-host

sh /usr/local/lib/mio/pi-host.sh status || {
  echo '{"type":"mio_attach_error","error":"not running"}'
  exit 3
}

gen=$(cat "$STATE/generation")
off=${2:-end}
if [ "$off" = end ]; then
  off=$(stat -c %s "$STATE/journal.jsonl")
elif [ "${1:--}" != "$gen" ]; then
  # pi restarted since the caller's cursor was taken, so its offset means nothing in this journal.
  off=0
fi

# The kernel drops the lock when this shell dies, so a lost connection never leaves it held.
# Children close descriptor 9: a lingering child must not keep the lock alive. The short wait
# covers the app replacing its own attach, whose old shell exits a moment after its client dies.
exec 9>> "$STATE/writer.lock"
if flock -w 2 9; then
  writer=true
  echo $$ > "$STATE/writer.pid"
else
  writer=false
fi
printf '{"type":"mio_attach","generation":"%s","offset":%s,"writer":%s}\n' "$gen" "$off" "$writer"

# --pid ends tail when this shell exits, so a dropped stream leaves no orphan reader.
tail -c +"$((off + 1))" -f --pid=$$ "$STATE/journal.jsonl" 9>&- &
tailpid=$!
if [ "$writer" = true ]; then
  # The input log is what makes pending dialogs derivable: pi never echoes a dialog answer.
  tee -a "$STATE/input.jsonl" > "$STATE/in.fifo" 9>&- || true
else
  cat > /dev/null 9>&-
fi
kill "$tailpid" 2>/dev/null || true
