# Spike script (Phase A2, docs/features/proxy-network-policy/02_dev.md).
#
# Tests whether an `async def request(self, flow)` hook can `await` a coroutine that performs a
# blocking-shaped wait (standing in for a blocking TCP round trip to the approval server) without
# stalling OTHER concurrent flows on the same mitmdump process. Both hosts are synthetic --
# `flow.response` is set directly inside the hook, so no real upstream connection is ever opened
# and no real DNS resolution of the fake hostnames is needed; this isolates the test to mitmdump's
# own hook-scheduling behavior, which is the only thing Phase A2 needs to confirm here.
#
# Drive with two concurrent `curl --proxy` calls (see run.sh) -- one to slow.mio-spike.test (hook
# awaits asyncio.sleep(SLOW_DELAY_SECS)), one to fast.mio-spike.test (hook returns immediately).
# If hooks were serialized, the fast request would also wait ~SLOW_DELAY_SECS. If concurrent (the
# assumption this spike is checking), the fast request completes almost immediately regardless of
# the slow one being in flight.

import asyncio
import time

from mitmproxy import http

SLOW_DELAY_SECS = 5
START = time.monotonic()


class AsyncConcurrency:
    async def request(self, flow):
        host = flow.request.pretty_host
        t = time.monotonic() - START
        if host == "slow.mio-spike.test":
            print(f"HOOKLOG slow request start host={host} t={t:.3f}", flush=True)
            await asyncio.sleep(SLOW_DELAY_SECS)
            t2 = time.monotonic() - START
            print(f"HOOKLOG slow request end host={host} t={t2:.3f}", flush=True)
        else:
            print(f"HOOKLOG fast request host={host} t={t:.3f}", flush=True)
        flow.response = http.Response.make(
            200,
            f"handled {host}".encode(),
            {"X-Handled-Host": host},
        )


addons = [AsyncConcurrency()]
