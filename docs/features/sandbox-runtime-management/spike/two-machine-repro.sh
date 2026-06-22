#!/usr/bin/env bash
# Two-machine flox reproduction spike for sandbox-runtime-management Phase D.
#
# Proves: a package installed via a flox manifest on a source Podman machine
# reproduces, bit-for-bit, on a different target Podman machine from manifest.lock
# alone — the closure is fetched from the Nix binary cache, NOT transferred.
#
# Run from a host with the Podman client and two machine connections:
#   LOCAL  = mio-tests          (source / "laptop")
#   REMOTE = mio-tests-remote   (deploy target)
#
# Prereqs:
#   - both machines started; rootless socket activated on each (see e2e/lib/podman-machine.ts
#     startPodmanSocket; the remote needs re-activation after every machine restart)
#   - the flox base image built from ./Containerfile and present on BOTH connections
#
# Recorded result (2026-06-23): remote printed "Hello, world!"; source and remote
# both hold /nix/store/zi2bj2hlavv8q743li2s9diqbcpmrf9b-hello-2.12.3; manifest
# transferred was 7,295 bytes; one-time base image was 1.27 GB.
set -euo pipefail

LOCAL=mio-tests
REMOTE=mio-tests-remote
IMAGE=sandbox-mio/base:flox
NAME=flox-repro
PKG=hello

# --- 0. Build base image (run once) and stage onto both connections -----------
# podman --connection "$LOCAL" build -t "$IMAGE" -f Containerfile .
# image scp between two machine connections is broken on Windows (DNS lookup of
# the connection name), so move the image with a save|load pipe instead:
# podman --connection "$LOCAL" save "$IMAGE" | podman --connection "$REMOTE" load

# --- 1. Source: install a package into a flox env in the workspace ------------
podman --connection "$LOCAL" rm -f "$NAME" 2>/dev/null || true
podman --connection "$LOCAL" run -d --name "$NAME" "$IMAGE" sleep 7200
podman --connection "$LOCAL" exec "$NAME" sh -c \
  "cd /root && mkdir -p work && cd work && flox init && flox install $PKG"
podman --connection "$LOCAL" exec "$NAME" sh -c "cd /root/work && flox activate -- $PKG"

# --- 2. Target: create container, confirm the closure is absent --------------
podman --connection "$REMOTE" rm -f "$NAME" 2>/dev/null || true
podman --connection "$REMOTE" run -d --name "$NAME" "$IMAGE" sleep 7200
podman --connection "$REMOTE" exec "$NAME" sh -c 'mkdir -p /root/work'
echo "remote $PKG store paths before: $(podman --connection "$REMOTE" exec "$NAME" sh -c "ls /nix/store | grep -c '$PKG-' || true")"

# --- 3. Transfer ONLY the manifest dir (no /nix/store closure) ---------------
# MSYS_NO_PATHCONV=1 is required on Git Bash so /root/work is not rewritten to a
# Windows path in the tar -C argument.
MSYS_NO_PATHCONV=1 \
  podman --connection "$LOCAL" exec "$NAME" tar -C /root/work -cf - .flox \
  | podman --connection "$REMOTE" exec -i "$NAME" tar -C /root/work -xf -

# --- 4. Reproduce on the target from the lockfile ----------------------------
podman --connection "$REMOTE" exec "$NAME" sh -c "cd /root/work && flox activate -- $PKG"
echo "remote $PKG store paths after:  $(podman --connection "$REMOTE" exec "$NAME" sh -c "ls /nix/store | grep -c '$PKG-' || true")"

# --- 5. Prove determinism: same store hash on both machines ------------------
echo "source: $(podman --connection "$LOCAL"  exec "$NAME" sh -c "ls /nix/store | grep '$PKG-2'")"
echo "remote: $(podman --connection "$REMOTE" exec "$NAME" sh -c "ls /nix/store | grep '$PKG-2'")"

# --- cleanup -----------------------------------------------------------------
# podman --connection "$LOCAL"  rm -f "$NAME"
# podman --connection "$REMOTE" rm -f "$NAME"
