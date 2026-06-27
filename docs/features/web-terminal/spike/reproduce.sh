#!/bin/sh
# Web-terminal feasibility spike, run 2026-06-27. Reproduces the end-to-end path:
# flox-install zellij in the sandbox base image, run the zellij web client bound to
# container loopback, publish the port to the Windows host, and fetch it.
#
# Run from Git Bash on the host. MSYS_NO_PATHCONV=1 stops Git Bash mangling container
# paths like /bin/sh into C:/Program Files/Git/usr/bin/sh.
set -eu

IMG=sandbox-mio/base:flox   # flox-baked base image built by ensure_base_image
NAME=zellij-spike

podman rm --force "$NAME" >/dev/null 2>&1 || true

# --host-lo-to-ns-lo is the decisive pasta flag: it forwards a host-loopback-published
# port to the container's loopback, where zellij binds. Without it, pasta forwards to the
# container's eth0 and the loopback-bound server is unreachable.
MSYS_NO_PATHCONV=1 podman run -d --tty \
  --network pasta:--host-lo-to-ns-lo \
  --publish 127.0.0.1::8082 \
  --name "$NAME" "$IMG" /bin/sh

HOST_PORT=$(podman port "$NAME" 8082 | sed 's/.*://')
echo "host port -> 127.0.0.1:$HOST_PORT"

# Install zellij through flox (the runtime-manager path). Exec WITHOUT a login shell:
# /etc/profile sets NIX_REMOTE=daemon, and there is no nix daemon, so a login shell breaks nix.
MSYS_NO_PATHCONV=1 podman exec --workdir /root/work "$NAME" flox init
MSYS_NO_PATHCONV=1 podman exec --workdir /root/work "$NAME" flox install zellij

# The web server aborts with "Failed to find default config file path" unless a config exists.
MSYS_NO_PATHCONV=1 podman exec --workdir /root/work "$NAME" flox activate -- \
  sh -c 'mkdir -p /root/.config/zellij && zellij setup --dump-config > /root/.config/zellij/config.kdl'

MSYS_NO_PATHCONV=1 podman exec --workdir /root/work "$NAME" flox activate -- zellij web --daemonize
sleep 3
MSYS_NO_PATHCONV=1 podman exec --workdir /root/work "$NAME" flox activate -- zellij web --status

# A login token, displayed only once.
MSYS_NO_PATHCONV=1 podman exec --workdir /root/work "$NAME" flox activate -- zellij web --create-token

echo "Now fetch http://127.0.0.1:$HOST_PORT/ from the Windows host (expect HTTP 200)."
