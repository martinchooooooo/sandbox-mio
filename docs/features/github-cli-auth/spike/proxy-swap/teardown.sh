# Removes every spike-ghs-* container and network. Never touches the default podman network.
. "$(dirname "$0")/lib.sh"
for c in $($P ps -a --format '{{.Names}}' | grep '^spike-ghs-'); do $P rm -f "$c" >/dev/null; done
for n in $($P network ls --format '{{.Name}}' | grep '^spike-ghs-'); do $P network rm -f "$n" >/dev/null; done
$P ps -a --format '{{.Names}}' | grep '^spike-ghs-' || echo "no spike-ghs containers left"
