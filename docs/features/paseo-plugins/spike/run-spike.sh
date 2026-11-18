#!/usr/bin/env bash
# Reproduces the 2026-11-18 paseo-plugins spike in a throwaway container (see spike.md beside this file).
# Run from Git Bash on the host with the Podman machine up. Removes its container on exit.
set -euo pipefail

CONN=${PODMAN_CONNECTION:-podman-machine-default}
NAME=paseo-plugin-spike
HERE=$(cd "$(dirname "$0")" && pwd)
PLUGIN_DIR=/usr/local/lib/mio/paseo-plugins/mio-probe
pm() { podman --connection "$CONN" "$@"; }
in_box() { pm exec "$NAME" sh -c "cd /root/work; $1"; }
trap 'pm rm -f "$NAME" >/dev/null 2>&1 || true' EXIT

launch='PASEO_PASSWORD=spike PASEO_LISTEN=0.0.0.0:6767 PASEO_WEB_UI_ENABLED=true PASEO_HOSTNAMES=true \
  PASEO_RELAY_ENABLED=false PASEO_DICTATION_ENABLED=false \
  setsid -f mise exec -- paseo daemon run </dev/null >/dev/null 2>&1
  for i in $(seq 1 60); do curl -sf -o /dev/null -H "Authorization: Bearer spike" http://127.0.0.1:6767/api/status && break; sleep 0.5; done'

echo "== container with the pinned node and paseo"
pm run -d --name "$NAME" localhost/sandbox-mio/base:10 sleep infinity >/dev/null
pm exec "$NAME" sh -c 'mkdir -p /root/work'
in_box 'mise use node@24.19.0 npm:@getpaseo/cli@0.16.1 >/dev/null 2>&1'

echo "== risk 1: plugin streamed over exec stdin + config.json pre-written before the first daemon start"
tar -C "$HERE/probe-plugin" -cf - . | pm exec -i "$NAME" sh -c "mkdir -p $PLUGIN_DIR && tar --no-same-owner -xf - -C $PLUGIN_DIR"
in_box "mkdir -p ~/.paseo && printf '%s' '{\"pluginsEnabled\":true,\"plugins\":{\"mio-probe\":{\"source\":\"directory\",\"path\":\"$PLUGIN_DIR\"}}}' > ~/.paseo/config.json"
in_box "$launch; mise exec -- paseo plugin ls; mise exec -- paseo plugin logs mio-probe"

echo "== risk 2: changed source applied by plugin reload, daemon pid unchanged"
in_box "sed -i 's/probe-v1/probe-v2/; s/v1\"/v2\"/' $PLUGIN_DIR/index.server.ts $PLUGIN_DIR/index.client.tsx; cat ~/.paseo/paseo.pid; echo"
in_box 'mise exec -- paseo plugin reload mio-probe; mise exec -- paseo plugin logs mio-probe | tail -3; cat ~/.paseo/paseo.pid; echo'

echo "== risk 3: plugin install is not idempotent"
in_box "mise exec -- paseo plugin install $PLUGIN_DIR || echo 'second install failed as expected'"

echo "== risk 4: a configured plugin whose folder is missing fails alone"
in_box 'mise exec -- paseo daemon stop >/dev/null 2>&1; sleep 1
  mise exec -- node -e "const f=process.env.HOME+\"/.paseo/config.json\";const c=JSON.parse(require(\"fs\").readFileSync(f));c.plugins[\"mio-ghost\"]={source:\"directory\",path:\"/nonexistent\"};require(\"fs\").writeFileSync(f,JSON.stringify(c))"'
in_box "$launch; mise exec -- paseo plugin ls"

echo "== risk 5: a browser-type client receives the compiled client bundle"
pm cp "$HERE/catalog-probe.mjs" "$NAME:/tmp/catalog-probe.mjs"
in_box 'PASEO_CLIENT_DIR=$(mise where npm:@getpaseo/cli@0.16.1)/lib/node_modules/@getpaseo/cli/node_modules \
  mise exec -- node /tmp/catalog-probe.mjs ws://127.0.0.1:6767/ws spike | grep mio-probe'
