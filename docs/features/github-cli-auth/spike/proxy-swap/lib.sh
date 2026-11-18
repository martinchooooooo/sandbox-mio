# Shared settings for the GitHub placeholder-swap spike (proxy-swap.md).
# Source from Git Bash: `. lib.sh`. Every resource is named spike-ghs-* and removed by teardown.sh.
# Topology copied from docs/features/podman-egress-boundary/spike/ (setup.sh, render_config.sh).
export MSYS_NO_PATHCONV=1
P="podman --connection ${SPIKE_CONNECTION:-podman-machine-default}"
IRON_IMAGE="${IRON_IMAGE:-docker.io/ironsh/iron-proxy:0.56.0}"
BASE_IMAGE="${BASE_IMAGE:-localhost/sandbox-mio/base:10}"
NET=spike-ghs-int
SUBNET=10.90.251.0/24
PROXY_IP=10.90.251.2
SIDECAR=spike-ghs-proxy
SANDBOX=spike-ghs-sbx
# Host state directory: CA, config and secret files. Never inside the repository.
STATE="${SPIKE_STATE:?set SPIKE_STATE to a host directory outside the repository}"
MGMT_TOKEN=spike-mgmt-token
# A fixed placeholder shaped like a classic GitHub token: ghp_ plus 36 characters.
PLACEHOLDER=ghp_MioPlaceholder0000000000000000000000
[ "${#PLACEHOLDER}" -eq 40 ] || { echo "placeholder must be 40 characters" >&2; exit 1; }

sbx() { $P exec "$SANDBOX" sh -c "$1"; }
mgmt() {
  $P exec "$SIDECAR" wget -q -S -O - --header "Authorization: Bearer $MGMT_TOKEN" \
    --post-data= "http://127.0.0.1:9092/v1/$1" 2>&1
}
