#!/usr/bin/env bash
# Spike script (Phase A2, docs/features/proxy-network-policy/02_dev.md).
#
# Drives async_concurrency.py's mitmdump addon: fires two concurrent proxied requests, one to a
# hostname the addon holds inside `await asyncio.sleep(SLOW_DELAY_SECS)`, one to a hostname the
# addon answers immediately. If the fast request completes almost immediately while the slow one
# is still in flight, the addon's async hook is not serializing the whole mitmdump process across
# flows -- the assumption Phase D/E's interactive-approval `async def request` hook depends on
# (one flow awaiting a synchronous approval-server round trip must not stall every other flow on
# the same proxy).
#
# NOTE on this environment: published container ports (`podman create -p`) did not forward through
# to the Windows host on this machine (confirmed independently with a plain nginx container --
# curl from the Windows host got connection-refused/status 000 even though the port was listening
# inside the podman machine's own namespace). This is an unrelated environment/WSL-forwarding
# quirk, not a finding about mitmproxy or this feature -- worked around by driving the curl calls
# from inside the podman machine (`podman machine ssh`) instead of from the true Windows host. Both
# vantage points reach the container over the same published-port mechanism; only the extra
# Windows<->WSL hop differs, which has no bearing on mitmdump's own hook-concurrency behavior.
#
# Cleans up its own container; nothing else is created.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTAINER=mio-spike-mitmdump-async
IMAGE=docker.io/mitmproxy/mitmproxy:11.1.3
HOST_PORT=18081

cleanup() {
    echo "--- cleanup ---"
    podman rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "--- starting mitmdump with the async addon, port published for driving from the host/machine ---"
MSYS_NO_PATHCONV=1 podman run -d --name "$CONTAINER" \
    -p "$HOST_PORT:8080" \
    -v "$HERE/async_concurrency.py:/addons/async_concurrency.py:ro" \
    "$IMAGE" \
    mitmdump --listen-host 0.0.0.0 --listen-port 8080 -s /addons/async_concurrency.py
sleep 2

echo "--- firing two concurrent proxied requests from inside the podman machine ---"
podman machine ssh podman-machine-default -- "
( t0=\$(date +%s%N); curl -s -o /tmp/mio-spike-slow.out --proxy http://127.0.0.1:$HOST_PORT http://slow.mio-spike.test/; t1=\$(date +%s%N); echo \$(( (t1 - t0) / 1000000 )) > /tmp/mio-spike-slow.ms ) &
SLOWPID=\$!
( t0=\$(date +%s%N); curl -s -o /tmp/mio-spike-fast.out --proxy http://127.0.0.1:$HOST_PORT http://fast.mio-spike.test/; t1=\$(date +%s%N); echo \$(( (t1 - t0) / 1000000 )) > /tmp/mio-spike-fast.ms ) &
FASTPID=\$!
wait \$FASTPID
echo 'FAST_DONE_FIRST_WAIT_RETURNED'
wait \$SLOWPID
echo 'SLOW_DONE'
echo '--- fast ms ---'; cat /tmp/mio-spike-fast.ms
echo '--- slow ms ---'; cat /tmp/mio-spike-slow.ms
rm -f /tmp/mio-spike-slow.out /tmp/mio-spike-fast.out /tmp/mio-spike-slow.ms /tmp/mio-spike-fast.ms
"

echo "--- container hook log ---"
podman logs "$CONTAINER"

# Actual run (2026-08-18) produced: fast=10ms, slow=5011ms, wait order fast-then-slow, and
# HOOKLOG lines showing both hooks starting at the same relative timestamp (t=8.136) with the fast
# one responding to its client immediately and the slow one not resuming until t=13.136 (5s later)
# -- concurrent scheduling confirmed, not serial blocking.
