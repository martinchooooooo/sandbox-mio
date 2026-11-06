#!/usr/bin/env bash
# Spike script (Phase A2, docs/features/proxy-network-policy/02_dev.md).
#
# Confirms host.containers.internal:<port> reachability from a container attached ONLY to a
# DNS-disabled bridge network with a pinned static IP -- mirroring exactly how create_proxy
# (src-tauri/src/proxy/mod.rs) attaches the real proxy container to its own mio-proxy-<sandbox>
# network (allocate_network: `podman network create --disable-dns --subnet <cidr> <name>`;
# create_sidecar: `podman create --network <name>:ip=<ip> ...`, no second network attachment).
# docs/features/network-proxy/02_dev.md's own Spike Results proved this reachability for the
# *sandbox*-side container on the same kind of bridge; this script proves it independently for a
# container built the same way the *proxy* container itself is built (same image family, same
# --disable-dns + static-ip attachment, no pasta).
#
# The target listener runs INSIDE the podman machine's own root namespace (not in any container),
# bound 0.0.0.0, started via `podman machine ssh` -- reachability from a proxy-side container to
# that listener via host.containers.internal is exactly what Phase D/E's approval channel needs
# (PROXY_APPROVAL_VM_PORT), just with a throwaway HTTP listener standing in for the real
# tokio::net::TcpListener.
#
# All resources are prefixed mio-spike- and removed at the end of this script, including the
# backgrounded listener inside the podman machine. Run from a shell with `podman` on PATH; the
# machine `podman-machine-default` must already be running (this script does not start/stop it).

set -euo pipefail

LISTEN_PORT=18080
NETWORK=mio-spike-proxynet
SUBNET=10.91.0.0/24
CONTAINER=mio-spike-proxycontainer
CONTAINER_IP=10.91.0.2
IMAGE=docker.io/mitmproxy/mitmproxy:11.1.3   # same pinned image the real proxy container runs

cleanup() {
    echo "--- cleanup ---"
    podman rm -f "$CONTAINER" >/dev/null 2>&1 || true
    podman network rm --force "$NETWORK" >/dev/null 2>&1 || true
    podman machine ssh podman-machine-default -- "pkill -f 'http.server $LISTEN_PORT' || true; rm -f /tmp/mio-spike-httpd.log" || true
}
trap cleanup EXIT

echo "--- starting 0.0.0.0-bound listener inside the podman machine (not a container) ---"
podman machine ssh podman-machine-default -- \
    "nohup python3 -m http.server $LISTEN_PORT --bind 0.0.0.0 > /tmp/mio-spike-httpd.log 2>&1 < /dev/null & disown"
sleep 1
podman machine ssh podman-machine-default -- "ss -tln | grep $LISTEN_PORT"

echo "--- creating throwaway DNS-disabled bridge network (mirrors allocate_network) ---"
podman network create --disable-dns --subnet "$SUBNET" "$NETWORK"

echo "--- creating a container attached ONLY to that network with a pinned static IP (mirrors create_sidecar's SidecarSpec.network) ---"
podman create --network "$NETWORK:ip=$CONTAINER_IP" --name "$CONTAINER" "$IMAGE" sleep 3600
podman start "$CONTAINER"

echo "--- curl-equivalent from inside the container to host.containers.internal:$LISTEN_PORT ---"
echo "(the mitmproxy image ships python3 but not curl, so this uses urllib -- the mechanism under"
echo " test is TCP/HTTP reachability over the bridge, not the client tool)"
podman exec "$CONTAINER" python3 -c "
import urllib.request
r = urllib.request.urlopen('http://host.containers.internal:$LISTEN_PORT/', timeout=5)
print('STATUS', r.status)
"

echo "--- SUCCESS: proxy-side container reached host.containers.internal ---"
