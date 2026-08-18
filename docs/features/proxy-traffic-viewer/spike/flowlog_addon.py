# Spike script (Phase A, docs/features/proxy-traffic-viewer/02_dev.md).
#
# Stand-in for the future src-tauri/resources/mitm_flowlog_addon.py (01_ideas.md Proposed Model /
# 02_dev.md Phase B). Hooks response() and error(), appends one JSON line per finished flow to
# /addons/flows.ndjson (mounted from the host so this spike can read the result directly), and also
# prints a HOOKLOG line to stdout for quick confirmation while watching `podman logs`.
#
# Used for three things in this spike:
#   1. Confirming flow.request.timestamp_start / flow.response.timestamp_end are populated and
#      their difference is a sane millisecond-scale duration on an ordinary successful flow.
#   2. Confirming error() fires (with no matching response() line) for a flow to an unreachable host.
#   3. As the third -s script in the three-script composition check (addon.py, policy_addon_stub.py,
#      flowlog_addon.py all loaded together).

import json

FLOWS_PATH = "/addons/flows.ndjson"

_counter = 0


def _next_id():
    global _counter
    _counter += 1
    return _counter


class FlowLogger:
    def response(self, flow):
        entry = {
            "id": _next_id(),
            "method": flow.request.method,
            "host": flow.request.pretty_host,
            "path": flow.request.path,
            "status": flow.response.status_code,
            "ts_start": flow.request.timestamp_start,
            "ts_end": flow.response.timestamp_end,
            "duration_ms": (
                (flow.response.timestamp_end - flow.request.timestamp_start) * 1000
                if flow.response.timestamp_end and flow.request.timestamp_start
                else None
            ),
        }
        print(f"HOOKLOG flowlog.response {entry}", flush=True)
        with open(FLOWS_PATH, "a", encoding="utf-8") as f:
            f.write(json.dumps(entry) + "\n")

    def error(self, flow):
        entry = {
            "id": _next_id(),
            "method": flow.request.method,
            "host": flow.request.pretty_host,
            "path": flow.request.path,
            "status": None,
            "ts_start": flow.request.timestamp_start,
            "error": str(flow.error),
        }
        print(f"HOOKLOG flowlog.error {entry}", flush=True)
        with open(FLOWS_PATH, "a", encoding="utf-8") as f:
            f.write(json.dumps(entry) + "\n")


addons = [FlowLogger()]
