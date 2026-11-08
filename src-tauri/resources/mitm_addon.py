# mitmproxy addon: injects a real credential header into outbound requests matched by host (and
# optional path prefix) so the sandbox itself never holds the value
# (docs/features/network-proxy/02_dev.md).
#
# Rules are read once at load, not watched: mitmproxy has no built-in reload for a mounted data
# file, so a rules change requires restarting the container (Resolved Decision 11).

import json

RULES_PATH = "/addons/rules.json"


def _load_rules():
    try:
        with open(RULES_PATH, "r", encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError):
        return []


class SecretInjector:
    def __init__(self):
        self.rules = _load_rules()

    # Not `request`: a streamed request's headers are already sent by the time that hook fires.
    def requestheaders(self, flow):
        host = flow.request.pretty_host
        path = flow.request.path
        for rule in self.rules:
            if rule.get("host") != host:
                continue
            prefix = rule.get("path_prefix") or ""
            if prefix and not path.startswith(prefix):
                continue
            # Lowercase: HTTP/2 rejects uppercase header names and mitmproxy only warns before
            # lowercasing them itself, so the addon lowercases up front.
            header = (rule.get("header") or "").lower()
            value = rule.get("value")
            if header and value is not None:
                flow.request.headers[header] = value


addons = [SecretInjector()]
