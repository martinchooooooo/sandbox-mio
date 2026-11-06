# Spike script (Phase A1, docs/features/proxy-network-policy/02_dev.md).
#
# Confirms: a server_connect hook fires with the resolved upstream address available (for both
# a literal IP target and a hostname that resolves to a private range), and confirms the
# mechanism to prevent the connection is `data.server.error = "..."`, per mitmproxy's own
# mitmproxy.proxy.server_hooks.ServerConnectHook docstring ("Setting data.server.error kills
# the connection"), not flow.kill().
#
# Private-range check uses the same RFC 1918/loopback/link-local ranges the real
# mitm_policy_addon.py's NetworkPolicy will use (01_ideas.md Resolved Decision 8).

import ipaddress

BLOCKED_NETWORKS = [
    ipaddress.ip_network("10.0.0.0/8"),
    ipaddress.ip_network("172.16.0.0/12"),
    ipaddress.ip_network("192.168.0.0/16"),
    ipaddress.ip_network("127.0.0.0/8"),
    ipaddress.ip_network("169.254.0.0/16"),
]


def _is_blocked(host: str) -> bool:
    try:
        addr = ipaddress.ip_address(host)
    except ValueError:
        return False
    return any(addr in net for net in BLOCKED_NETWORKS)


class ServerConnectPolicy:
    def server_connect(self, data):
        address = data.server.address
        host = address[0] if address else None
        print(f"HOOKLOG server_connect address={address}", flush=True)
        if host and _is_blocked(host):
            print(f"HOOKLOG server_connect BLOCKING {host}", flush=True)
            data.server.error = "blocked by mio-spike policy: private/loopback/link-local range"


addons = [ServerConnectPolicy()]
