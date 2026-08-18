# mitmproxy addon: appends one metadata-only JSON line per finished flow to a bind-mounted NDJSON
# file the app polls (docs/features/proxy-traffic-viewer/02_dev.md).
#
# Header values, bodies, and injected secret values are never written here — only whether some rule
# matched (01_ideas.md, "What is deliberately not captured").
#
# Keys are camelCase to match `FlowEntry` in src-tauri/src/proxy/flowlog.rs, which deserialises
# these lines under `serde(rename_all = "camelCase")`.

import json

RULES_PATH = "/addons/rules.json"
FLOWS_PATH = "/addons/flows.ndjson"

# Must equal FLOW_LOG_MAX_LINES in src-tauri/src/proxy/flowlog.rs and
# src/lib/proxy/traffic.svelte.ts.
FLOW_LOG_MAX_LINES = 500


def _load_rules():
    try:
        with open(RULES_PATH, "r", encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError):
        return []


def _body_bytes(message):
    length = message.headers.get("content-length")
    if length is not None:
        try:
            return int(length)
        except ValueError:
            pass
    try:
        return len(message.content or b"")
    except Exception:
        return None


def _duration_ms(flow):
    start = flow.request.timestamp_start
    end = flow.response.timestamp_end
    if start and end:
        return (end - start) * 1000
    return None


class FlowLogger:
    def __init__(self):
        # Rules are read once at load, the same contract SecretInjector has in mitm_addon.py: a
        # rules change restarts the container, reloading both copies together.
        self.rules = _load_rules()
        self.counter = 0
        self.lines = self._count_lines()

    def _count_lines(self):
        try:
            with open(FLOWS_PATH, "r", encoding="utf-8") as f:
                return sum(1 for _ in f)
        except OSError:
            return 0

    # Whether some rule would have injected a secret into this request — matched here rather than
    # asked of SecretInjector, so the two addon classes stay independent (Resolved Decision 7).
    def _secret_injected(self, request):
        for rule in self.rules:
            if rule.get("host") != request.pretty_host:
                continue
            prefix = rule.get("path_prefix") or ""
            if prefix and not request.path.startswith(prefix):
                continue
            return True
        return False

    def _append(self, entry):
        self.counter += 1
        entry["id"] = self.counter
        try:
            with open(FLOWS_PATH, "a", encoding="utf-8") as f:
                f.write(json.dumps(entry) + "\n")
        except OSError:
            return
        self.lines += 1
        if self.lines > FLOW_LOG_MAX_LINES:
            self._truncate()

    def _truncate(self):
        try:
            with open(FLOWS_PATH, "r", encoding="utf-8") as f:
                kept = f.readlines()[-FLOW_LOG_MAX_LINES:]
            with open(FLOWS_PATH, "w", encoding="utf-8") as f:
                f.writelines(kept)
        except OSError:
            return
        self.lines = len(kept)

    def response(self, flow):
        if flow.request is None or flow.response is None:
            return
        self._append(
            {
                "method": flow.request.method,
                "host": flow.request.pretty_host,
                "path": flow.request.path,
                "status": flow.response.status_code,
                "ts": flow.request.timestamp_start,
                "durationMs": _duration_ms(flow),
                "reqBytes": _body_bytes(flow.request),
                "respBytes": _body_bytes(flow.response),
                "secretInjected": self._secret_injected(flow.request),
            }
        )

    # A flow that never got a response at all: connection refused, TLS failure, timeout, or an
    # earlier addon's flow.kill() (02_dev.md, Phase A).
    def error(self, flow):
        if flow.request is None:
            return
        self._append(
            {
                "method": flow.request.method,
                "host": flow.request.pretty_host,
                "path": flow.request.path,
                "status": None,
                "ts": flow.request.timestamp_start,
                "error": str(flow.error),
            }
        )


addons = [FlowLogger()]
