#!/bin/sh
# Spike attach: one per SSH session. Streams the journal from a byte offset to stdout and copies
# stdin into the supervisor's pipe. usage: pi-attach.sh <generation|-> <offset|end>
# The first stdout line is a synthetic `mio_attach` header; byte offsets count from the line after it.
# If the caller's generation differs from the running one, pi restarted and the offset is reset to 0.
set -eu
STATE=/run/mio/pi-host
sh /usr/local/lib/mio/pi-host.sh status >/dev/null || { echo '{"type":"mio_attach_error","error":"not running"}'; exit 3; }

gen=$(cat "$STATE/generation")
off=${2:-end}
if [ "$off" = end ]; then
  off=$(stat -c %s "$STATE/journal.jsonl")
elif [ "${1:--}" != "$gen" ]; then
  off=0
fi
printf '{"type":"mio_attach","generation":"%s","offset":%s}\n' "$gen" "$off"

# --pid ends tail when this script exits, so a dropped session leaves no orphan reader.
tail -c +"$((off + 1))" -f --pid=$$ "$STATE/journal.jsonl" &
tailpid=$!
# The input log is what makes pending dialogs derivable: pi never echoes an extension_ui_response.
tee -a "$STATE/input.jsonl" > "$STATE/in.fifo"
kill "$tailpid" 2>/dev/null || true
