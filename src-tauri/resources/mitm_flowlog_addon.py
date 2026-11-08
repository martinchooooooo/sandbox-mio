# mitmproxy addon: appends one metadata-only JSON line per finished flow to a bind-mounted NDJSON
# file the app polls (docs/features/proxy-traffic-viewer/02_dev.md).
#
# Header values, bodies, and injected secret values are never written here — only whether some rule
# matched (01_ideas.md, "What is deliberately not captured").
#
# Keys are camelCase to match `FlowEntry` in src-tauri/src/proxy/flowlog.rs, which deserialises
# these lines under `serde(rename_all = "camelCase")`.

import json
import os

# The microsandbox sidecar mounts directories rather than single files, so it overrides both
# (docs/features/microsandbox-network-policy/02_dev.md, Resolved Decision 19).
RULES_PATH = os.environ.get("MIO_RULES_PATH", "/addons/rules.json")
FLOWS_PATH = os.environ.get("MIO_FLOWS_PATH", "/addons/flows.ndjson")

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
    # A streamed body is never stored, so its size is unknown.
    if message.raw_content is None:
        return None
    try:
        return len(message.raw_content)
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
        self.lines, self.counter = self._scan_existing()

    # Ids continue across sidecar restarts, since the log outlives them and the viewer keys rows by id.
    def _scan_existing(self):
        lines, last_id = 0, 0
        try:
            with open(FLOWS_PATH, "r", encoding="utf-8") as f:
                for line in f:
                    lines += 1
                    try:
                        last_id = max(last_id, int(json.loads(line).get("id", 0)))
                    except (ValueError, TypeError, AttributeError):
                        continue
        except OSError:
            pass
        return lines, last_id

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
