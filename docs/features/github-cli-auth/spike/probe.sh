#!/usr/bin/env bash
# Spike probe for docs/features/github-cli-auth/spike/gh-device-flow.md (run 2026-11-18, Git Bash).
# Needs a running podman-machine-default with a sandbox-mio base image. Token values are redacted
# from every printed line; the step marked MANUAL needs a human to enter the device code.
set -u
export MSYS_NO_PATHCONV=1
P="podman --connection podman-machine-default"
IMG=localhost/sandbox-mio/base:9
GH=2.105.0
redact() { sed -E 's/gh[opsu]_[A-Za-z0-9_*]+/REDACTED/g'; }

install_gh() {
  $P exec "$1" sh -c "curl -fsSL https://github.com/cli/cli/releases/download/v$GH/gh_${GH}_linux_amd64.tar.gz \
    | tar xz -C /tmp && cp /tmp/gh_${GH}_linux_amd64/bin/gh /usr/local/bin/gh && gh --version | head -1"
}

$P run -d --name spike-gh-auth "$IMG" sleep infinity >/dev/null
$P run -d --name spike-gh-target "$IMG" sleep infinity >/dev/null
install_gh spike-gh-auth
install_gh spike-gh-target

echo "--- Risk 1/2: login without a TTY; stdin closed, killed by timeout"
timeout 20 $P exec -i spike-gh-auth gh auth login </dev/null 2>&1; echo "exit=$?"

echo "--- MANUAL: complete the device flow in a browser"
$P exec -i spike-gh-auth gh auth login --hostname github.com --git-protocol https --web </dev/null 2>&1; echo "exit=$?"

echo "--- Risk 3: where the token landed"
$P exec spike-gh-auth gh auth status 2>&1 | redact
$P exec spike-gh-auth sh -c 'ls -la /root/.config/gh; cat /root/.gitconfig 2>&1' | redact

echo "--- Risk 4: inject over stdin, configure git"
$P exec spike-gh-auth gh auth token | $P exec -i spike-gh-target gh auth login --with-token --hostname github.com --git-protocol https 2>&1; echo "exit=$?"
$P exec spike-gh-target gh auth setup-git; $P exec spike-gh-target cat /root/.gitconfig
$P exec -i spike-gh-target sh -c 'printf "protocol=https\nhost=github.com\n\n" | git credential fill' | sed -E 's/(password=).*/\1REDACTED/'

echo "--- Risk 5: bogus token, then logout"
echo "ghp_notarealtoken000000000000000000000000" | $P exec -i spike-gh-target sh -c 'GH_CONFIG_DIR=/tmp/ghx gh auth login --with-token --hostname github.com' 2>&1; echo "exit=$?"
$P exec -i spike-gh-target gh auth logout --hostname github.com </dev/null 2>&1; echo "exit=$?"

$P rm -f spike-gh-auth spike-gh-target
