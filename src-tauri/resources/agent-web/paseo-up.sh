# Brings the sandbox's Paseo daemon up, or confirms the running one still matches its launch inputs.
# Run by `prepare_agent_web` (src-tauri/src/runtime/mod.rs) with `sh -c` from the workspace dir;
# POSIX sh, since the image's /bin/sh is dash.
#
# Exit codes: 0 ready; 10 the pinned node/paseo are missing from mise.toml or not installed (the
# caller installs them and runs this again); 11 still starting (the caller runs this again).
# Anything else is a failure, with the daemon log's tail on stderr.
#
# Inputs: PASEO_PASSWORD (secret), MIO_PASEO_PORT, MIO_PASEO_NODE / MIO_PASEO_NODE_VERSION,
# MIO_PASEO_CLI / MIO_PASEO_CLI_VERSION (mise tool name and version), MIO_PASEO_PROJECT_NAME.
set -u

port=$MIO_PASEO_PORT
node_tool=$MIO_PASEO_NODE
node_version=$MIO_PASEO_NODE_VERSION
cli_tool=$MIO_PASEO_CLI
cli_version=$MIO_PASEO_CLI_VERSION
project_name=$MIO_PASEO_PROJECT_NAME
# The daemon inherits this environment, and through it every agent it spawns.
unset MIO_PASEO_PORT MIO_PASEO_NODE MIO_PASEO_NODE_VERSION MIO_PASEO_CLI MIO_PASEO_CLI_VERSION \
  MIO_PASEO_PROJECT_NAME

# Bump when the daemon's launch environment below changes, so running daemons pick it up.
launch_rev=1

home="$HOME/.paseo"
mkdir -p "$home"

# A pre-warm after sandbox start can race the user opening the Agents tab. The daemon launch
# below closes fd 9: an inherited copy would hold the lock for the daemon's lifetime.
exec 9>"$home/mio-up.lock"
flock -w 5 9 || exit 11

current=$(mise current 2>/dev/null)
printf '%s\n' "$current" | grep -qx "$node_tool $node_version" || exit 10
printf '%s\n' "$current" | grep -qx "$cli_tool $cli_version" || exit 10
mise where "$node_tool@$node_version" >/dev/null 2>&1 || exit 10
mise where "$cli_tool@$cli_version" >/dev/null 2>&1 || exit 10

alive() {
  curl -sf -o /dev/null --max-time 2 -H "Authorization: Bearer $PASEO_PASSWORD" \
    "http://127.0.0.1:$port/api/status"
}

running() {
  pid=$(sed -n 's/.*"pid":\([0-9]*\).*/\1/p' "$home/paseo.pid" 2>/dev/null)
  [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null
}

# mise.toml carries the tools on the daemon's PATH and the [env] its agents inherit.
want=$( { cat mise.toml 2>/dev/null; printf '%s\n' "$launch_rev" "$cli_version" "$PASEO_PASSWORD"; } \
  | sha256sum | cut -c1-64)

if [ "$(cat "$home/mio-launch" 2>/dev/null)" != "$want" ] || ! running; then
  if running; then
    mise exec -- paseo daemon stop >/dev/null 2>&1 || true
  fi
  # `daemon run` is the only launch mode that honours PASEO_* settings from the environment.
  # Dictation is off because its first start downloads speech models.
  PASEO_LISTEN="0.0.0.0:$port" \
    PASEO_WEB_UI_ENABLED=true \
    PASEO_HOSTNAMES=true \
    PASEO_RELAY_ENABLED=false \
    PASEO_DICTATION_ENABLED=false \
    setsid -f mise exec -- paseo daemon run </dev/null >/dev/null 2>&1 9>&-
  printf '%s' "$want" >"$home/mio-launch"
fi

# Lock wait, stop, wait and registration together must fit the process layer's 30s exec bound.
started=$(date +%s)
until alive; do
  waited=$(( $(date +%s) - started ))
  # The pid file appears only once the daemon has initialised, so allow it a few seconds.
  if [ "$waited" -ge 5 ] && ! running; then
    echo "paseo daemon exited during startup; daemon.log tail:" >&2
    tail -n 30 "$home/daemon.log" >&2 2>/dev/null
    rm -f "$home/mio-launch"
    exit 1
  fi
  [ "$waited" -ge 15 ] && exit 11
  sleep 0.25
done

register_project() {
  created=$(mise exec -- paseo project create "$PWD" --json) || return 1
  id=$(printf '%s\n' "$created" | sed -n 's/.*"projectId": "\([^"]*\)".*/\1/p')
  [ -n "$id" ] || return 1
  mise exec -- paseo project rename "$id" "$project_name" >/dev/null
}

if [ ! -f "$home/mio-project" ]; then
  if register_project; then
    touch "$home/mio-project"
  else
    echo "paseo project registration failed; it is retried on the next open" >&2
  fi
fi

echo ready
