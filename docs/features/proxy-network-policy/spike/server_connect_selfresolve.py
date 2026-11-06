# Spike script (Phase A1, docs/features/proxy-network-policy/02_dev.md).
#
# Follow-up to server_connect.py's finding: data.server.address at server_connect time is the
# unresolved (hostname, port) tuple for a hostname target -- mitmproxy's own DNS resolution
# happens later, inside asyncio.open_connection, with no hook between resolution and connect.
# This script tests whether the addon can work around that by doing its OWN resolution
# (socket.getaddrinfo) inside server_connect, before mitmproxy ever attempts the real connect.

import ipaddress
import socket

BLOCKED_NETWORKS = [
    ipaddress.ip_network("10.0.0.0/8"),
    ipaddress.ip_network("172.16.0.0/12"),
    ipaddress.ip_network("192.168.0.0/16"),
    ipaddress.ip_network("127.0.0.0/8"),
    ipaddress.ip_network("169.254.0.0/16"),
]


def _is_blocked_ip(ip_str: str) -> bool:
    try:
        addr = ipaddress.ip_address(ip_str)
    except ValueError:
        return False
    return any(addr in net for net in BLOCKED_NETWORKS)


class ServerConnectPolicySelfResolve:
    def server_connect(self, data):
        host, port = data.server.address
        print(f"HOOKLOG server_connect address=({host!r}, {port})", flush=True)

        # Literal IP: no resolution needed.
        if _is_blocked_ip(host):
            print(f"HOOKLOG BLOCKING literal IP {host}", flush=True)
            data.server.error = "blocked by mio-spike policy: literal IP in private range"
            return

        # Hostname: do our own resolution since mitmproxy has not resolved it yet at this
        # point (confirmed by server_connect.py's spike run).
        try:
            infos = socket.getaddrinfo(host, port, proto=socket.IPPROTO_TCP)
        except socket.gaierror as e:
            print(f"HOOKLOG resolution failed for {host}: {e}", flush=True)
            return
        resolved_ips = sorted({info[4][0] for info in infos})
        print(f"HOOKLOG resolved {host} -> {resolved_ips}", flush=True)
        for ip_str in resolved_ips:
            if _is_blocked_ip(ip_str):
                print(f"HOOKLOG BLOCKING {host} (resolves to {ip_str})", flush=True)
                data.server.error = (
                    f"blocked by mio-spike policy: {host} resolves to {ip_str} (private range)"
                )
                return


addons = [ServerConnectPolicySelfResolve()]
