# Brings the sandbox's Paseo daemon up, or confirms the running one still matches its launch inputs.
# Run by `prepare_agent_web` (src-tauri/src/runtime/mod.rs) with `sh -c` from the workspace dir;
# POSIX sh, since the image's /bin/sh is dash.
#
# Exit codes: 0 ready; 10 the pinned node/paseo are missing from mise.toml or not installed (the
# caller installs them and runs this again); 11 still starting (the caller runs this again); 12 the
# delivered app-owned plugins are not the expected bundle (the caller delivers it with
# paseo-plugins-deliver.sh and runs this again). Anything else is a failure, with the daemon log's
# tail on stderr.
#
# Inputs: PASEO_PASSWORD (secret), MIO_PASEO_PORT, MIO_PASEO_NODE / MIO_PASEO_NODE_VERSION,
# MIO_PASEO_CLI / MIO_PASEO_CLI_VERSION (mise tool name and version), MIO_PASEO_PROJECT_NAME,
# MIO_PASEO_PLUGIN_DIR / MIO_PASEO_PLUGIN_HASH / MIO_PASEO_PLUGIN_IDS (the delivered plugin
# directory, the expected bundle hash, and the space-separated plugin ids), MIO_PASEO_PREWARM (1 for a
# pre-warm, which leaves a running daemon as is).
#
# Stdout carries one `timing name=ms ...` line per run, on every exit path, which the caller logs at
# debug level. Stderr stays empty on success: the caller treats any stderr on exit 0 as a problem.
set -u

now_ms() { echo $(( $(date +%s%N) / 1000000 )); }
t_start=$(now_ms)
t_mark=$t_start
timings=""
# Appends the time since the previous mark under `$1`.
phase() {
  t_now=$(now_ms)
  timings="$timings $1=$(( t_now - t_mark ))"
  t_mark=$t_now
}
trap 'printf "timing%s total=%s\n" "$timings" "$(( $(now_ms) - t_start ))"' EXIT

port=$MIO_PASEO_PORT
node_tool=$MIO_PASEO_NODE
node_version=$MIO_PASEO_NODE_VERSION
cli_tool=$MIO_PASEO_CLI
cli_version=$MIO_PASEO_CLI_VERSION
project_name=$MIO_PASEO_PROJECT_NAME
plugin_dir=$MIO_PASEO_PLUGIN_DIR
plugin_hash=$MIO_PASEO_PLUGIN_HASH
plugin_ids=$MIO_PASEO_PLUGIN_IDS
prewarm=$MIO_PASEO_PREWARM
# The daemon inherits this environment, and through it every agent it spawns.
unset MIO_PASEO_PORT MIO_PASEO_NODE MIO_PASEO_NODE_VERSION MIO_PASEO_CLI MIO_PASEO_CLI_VERSION \
  MIO_PASEO_PROJECT_NAME MIO_PASEO_PLUGIN_DIR MIO_PASEO_PLUGIN_HASH MIO_PASEO_PLUGIN_IDS \
  MIO_PASEO_PREWARM

# Bump when the daemon's launch environment below changes, so running daemons pick it up.
launch_rev=3

home="$HOME/.paseo"
mkdir -p "$home"

# Caches compiled JS (Node 22.1+) across the many short node processes below and the daemon, whose
# agents inherit it: harmless for them, the cache is keyed by file content.
export NODE_COMPILE_CACHE="$home/node-compile-cache"

running() {
  pid=$(sed -n 's/.*"pid":\([0-9]*\).*/\1/p' "$home/paseo.pid" 2>/dev/null)
  [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null
}

# A pre-warm after sandbox start can race the user opening the Agents tab. The daemon launch
# below closes fd 9: an inherited copy would hold the lock for the daemon's lifetime.
exec 9>"$home/mio-up.lock"
flock -w 5 9 || exit 11
phase lock

# A pre-warm never restarts a running daemon, even a stale one: that would end the agent sessions
# of a closed pane. The next open applies the change.
if [ "$prewarm" = 1 ] && running; then
  exit 0
fi

current=$(mise current 2>/dev/null)
printf '%s\n' "$current" | grep -qx "$node_tool $node_version" || exit 10
printf '%s\n' "$current" | grep -qx "$cli_tool $cli_version" || exit 10
mise where "$node_tool@$node_version" >/dev/null 2>&1 || exit 10
mise where "$cli_tool@$cli_version" >/dev/null 2>&1 || exit 10
[ "$(cat "$plugin_dir/.bundle-hash" 2>/dev/null)" = "$plugin_hash" ] || exit 12
phase checks

# Registers the delivered app-owned plugins in Paseo's config.json, keeping every other key and
# entry, since the daemon rewrites the file itself. Only read at daemon start, so run while stopped.
merge_plugins='
const fs = require("fs");
const path = require("path");
const file = process.argv[1];
const dir = process.env.MIO_PLUGIN_DIR;
const ids = process.env.MIO_PLUGIN_IDS.split(" ").filter(Boolean);
let config = {};
try {
  config = JSON.parse(fs.readFileSync(file, "utf8"));
} catch (e) {
  if (e.code !== "ENOENT") throw e;
}
const plugins = config.plugins && typeof config.plugins === "object" ? config.plugins : {};
for (const id of Object.keys(plugins)) {
  if (id.startsWith("mio-") && !fs.existsSync(path.join(dir, id))) delete plugins[id];
}
for (const id of ids) plugins[id] = { ...plugins[id], source: "directory", path: path.join(dir, id) };
config.pluginsEnabled = true;
config.plugins = plugins;
fs.writeFileSync(file + ".mio-tmp", JSON.stringify(config, null, 2) + "\n");
fs.renameSync(file + ".mio-tmp", file);
'

alive() {
  curl -sf -o /dev/null --max-time 2 -H "Authorization: Bearer $PASEO_PASSWORD" \
    "http://127.0.0.1:$port/api/status"
}

# mise.toml carries the tools on the daemon's PATH and the [env] its agents inherit. The plugin
# ids restart the daemon when the plugin set changes; content changes only reload (02_dev.md of
# paseo-plugins, Resolved Decision 1).
want=$( { cat mise.toml 2>/dev/null; printf '%s\n' "$launch_rev" "$cli_version" "$PASEO_PASSWORD" \
  "$plugin_ids"; } | sha256sum | cut -c1-64)

if [ "$(cat "$home/mio-launch" 2>/dev/null)" != "$want" ] || ! running; then
  if running; then
    mise exec -- paseo daemon stop >/dev/null 2>&1 || true
  fi
  phase stop
  if MIO_PLUGIN_DIR="$plugin_dir" MIO_PLUGIN_IDS="$plugin_ids" \
    mise exec -- node -e "$merge_plugins" "$home/config.json"; then
    printf '%s' "$plugin_hash" >"$home/mio-plugins-loaded"
  else
    echo "paseo plugin registration failed; the daemon starts without app plugins" >&2
  fi
  phase merge
  # `daemon run` is the only launch mode that honours PASEO_* settings from the environment.
  # Dictation is off because its first start downloads speech models.
  PASEO_LISTEN="0.0.0.0:$port" \
    PASEO_WEB_UI_ENABLED=true \
    PASEO_HOSTNAMES=true \
    PASEO_RELAY_ENABLED=false \
    PASEO_DICTATION_ENABLED=false \
    setsid -f mise exec -- paseo daemon run </dev/null >/dev/null 2>&1 9>&-
  printf '%s' "$want" >"$home/mio-launch"
  timings="$timings launched=1"
  t_mark=$(now_ms)
fi

# Lock wait, stop, wait, plugin reload and registration together must fit the process layer's
# 30s exec bound.
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
phase alive

# Same plugin set, new contents: reload in place, so running agent sessions survive. The marker
# moves only when every reload succeeded, so a failed one is retried on the next open.
if [ "$(cat "$home/mio-plugins-loaded" 2>/dev/null)" != "$plugin_hash" ]; then
  reloaded=true
  for id in $plugin_ids; do
    if ! mise exec -- paseo plugin reload "$id" >/dev/null 2>&1; then
      echo "paseo plugin reload $id failed; see 'paseo plugin logs $id'" >&2
      reloaded=false
    fi
  done
  if [ "$reloaded" = true ]; then
    printf '%s' "$plugin_hash" >"$home/mio-plugins-loaded"
  fi
fi
phase reload

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

phase project
echo ready
