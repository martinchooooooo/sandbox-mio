# Spike script (Phase A, docs/features/proxy-traffic-viewer/02_dev.md).
#
# Plays the role of a future policy addon that blocks by setting flow.response directly (NOT
# flow.kill()) -- loaded first (-s a_synth.py before -s b_synth.py). For path "/synth" it sets a
# synthetic 403 response with a distinctive body and header, before b_synth.py's response() hook
# runs, to confirm the mechanism a flow-log addon would need to still capture a blocked request as
# a captured (not missing) entry.

from mitmproxy import http


class ASynth:
    def request(self, flow):
        print(f"HOOKLOG a_synth.request path={flow.request.path}", flush=True)
        if flow.request.path == "/synth":
            flow.response = http.Response.make(
                403,
                b"synthetic-blocked-body",
                {"X-Synth-Blocked": "true"},
            )


addons = [ASynth()]
