# Spike script (Phase A1, docs/features/proxy-network-policy/02_dev.md).
#
# Plays the role of the future mitm_policy_addon.py: loaded first (-s a.py before -s b.py),
# and for path "/block" sets flow.response before b.py's request() hook runs, to confirm
# hook-order composition and that flow.response short-circuits a later script's mutation.
#
# Logs to stdout with a distinctive prefix so ordering can be read back from `podman logs`.

from mitmproxy import http


class A:
    def request(self, flow):
        print(f"HOOKLOG a.request path={flow.request.path}", flush=True)
        if flow.request.path == "/block":
            flow.response = http.Response.make(
                403,
                b"blocked-by-a",
                {"X-A-Blocked": "true"},
            )


addons = [A()]
