#!/usr/bin/env bash
# Spike probe for docs/features/namespaced-ssh-alias/spike/01_dotted-alias-and-names.md (run 2026-11-26, Git Bash).
# Needs a running podman-machine-default, the sandbox-mio base image and the bundled Mutagen sidecar.
# Appends a probe block to ~/.ssh/sandbox-mio/config; the cleanup at the end restores the backup.
set -u
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'
P="podman --connection podman-machine-default"
IMG=localhost/sandbox-mio/base:10
M=src-tauri/binaries/mutagen-x86_64-pc-windows-msvc.exe
WORK=${1:?usage: probe.sh <scratch dir>}
mkdir -p "$WORK/mutagen-data" "$WORK/backup" "$WORK/alpha"
export MUTAGEN_DATA_DIRECTORY=$(cygpath -w "$WORK/mutagen-data")
cp -p ~/.ssh/config "$WORK/backup/config"
cp -p ~/.ssh/sandbox-mio/config "$WORK/backup/sandbox-mio-config"
cp -p ~/.ssh/known_hosts "$WORK/backup/known_hosts"

echo "--- target container with its own key and a loopback listener on 7777"
ssh-keygen -q -t ed25519 -N "" -f "$WORK/id_probe" -C probe
$P run -d --name probe-s1 "$IMG" sleep 3600 >/dev/null
cat "$WORK/id_probe.pub" | $P exec -i probe-s1 sh -c 'cat > /etc/ssh/authorized_keys.container'
$P exec -d probe-s1 sh -c 'socat TCP-LISTEN:7777,bind=127.0.0.1,fork,reuseaddr SYSTEM:"echo probe-ok"'

echo "--- S1: managed block for the dotted alias"
cat >> ~/.ssh/sandbox-mio/config <<BLOCK
# >>> sandbox-mio probe-s1 >>>
Host probe.mio
    ProxyCommand podman --connection podman-machine-default exec -i probe-s1 mio-sshd
    User root
    IdentityFile "$(cygpath -m "$WORK")/id_probe"
    UserKnownHostsFile "$(cygpath -m "$WORK")/known_hosts"
    IdentitiesOnly yes
    StrictHostKeyChecking accept-new
    ServerAliveInterval 15
    ServerAliveCountMax 3
# <<< sandbox-mio probe-s1 <<<
BLOCK
ssh -G probe.mio | grep -iE '^(hostname|user|proxycommand) '
ssh -o BatchMode=yes probe.mio 'echo connected-as $(whoami)'
/c/WINDOWS/System32/OpenSSH/ssh.exe -o BatchMode=yes probe.mio 'echo win-openssh-connected $(whoami)'
$M forward create --name=probe-fwd tcp:127.0.0.1:47811 probe.mio:tcp:127.0.0.1:7777
curl -s --max-time 5 telnet://127.0.0.1:47811 </dev/null
$M forward list | grep -E '^(Name|Status|Connections)|URL'

echo "--- S2: Mutagen session-name rules"
for n in a.b agentweb-a.b expose-a.b-8080 a_b 1ab -ab a-b A1 ab1 a-; do
  printf '%s: ' "$n"
  $M forward create --name="$n" tcp:127.0.0.1:47814 probe.mio:tcp:127.0.0.1:7777 2>&1 | tr '\r' '\n' | tail -1
  $M forward terminate "$n" >/dev/null 2>&1
done
$M sync create --name=a.b "$(cygpath -w "$WORK/alpha")" probe.mio:/tmp/x 2>&1 | tr '\r' '\n' | tail -1

echo "--- IANA root zone"
curl -sS -o "$WORK/tlds.txt" https://data.iana.org/TLD/tlds-alpha-by-domain.txt
head -1 "$WORK/tlds.txt"; grep -ix mio "$WORK/tlds.txt" || echo "mio: not delegated"

echo "--- cleanup"
$M forward terminate --all; $M sync terminate --all; $M daemon stop
$P rm -f probe-s1 >/dev/null
cp -p "$WORK/backup/config" ~/.ssh/config
cp -p "$WORK/backup/sandbox-mio-config" ~/.ssh/sandbox-mio/config
cp -p "$WORK/backup/known_hosts" ~/.ssh/known_hosts
