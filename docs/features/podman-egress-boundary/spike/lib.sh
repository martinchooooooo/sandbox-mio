# Shared settings for the Phase A spikes (docs/features/podman-egress-boundary/02_dev.md).
# Source from Git Bash: `. lib.sh`. Every resource is named spike-peb-* and removed by teardown.sh.
export MSYS_NO_PATHCONV=1
P="podman --connection ${SPIKE_CONNECTION:-podman-machine-default}"
IRON_IMAGE="${IRON_IMAGE:-docker.io/ironsh/iron-proxy:0.56.0}"
BASE_IMAGE="${BASE_IMAGE:-localhost/sandbox-mio/base:7}"
NET=spike-peb-int
SUBNET=10.90.250.0/24
PROXY_IP=10.90.250.2
SIDECAR=spike-peb-proxy
SANDBOX=spike-peb-sbx
# Host state directory (the stand-in for <app_data_root>/proxy/<sandbox>/), forward-slashed for podman -v.
STATE="${SPIKE_STATE:?set SPIKE_STATE to a host directory}"
MGMT_TOKEN=spike-mgmt-token

# Runs a shell snippet inside the sandbox.
sbx() { $P exec "$SANDBOX" sh -c "$1"; }
# Calls the management API from inside the sidecar with busybox wget (Resolved Decision 13).
mgmt() {
  $P exec "$SIDECAR" wget -q -S -O - --header "Authorization: Bearer $MGMT_TOKEN" \
    --post-data= "http://127.0.0.1:9092/v1/$1" 2>&1
}
