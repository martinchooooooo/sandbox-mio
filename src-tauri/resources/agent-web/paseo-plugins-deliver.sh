# Replaces the sandbox's app-owned Paseo plugins with the tar archive on stdin and records its hash.
# Run by `prepare_agent_web` when paseo-up.sh exits 12; arguments: $1 plugin directory, $2 bundle hash.
set -eu

dir=$1
hash=$2
staging="$dir.staging"
old="$dir.old"

# Shares paseo-up.sh's lock: a pre-warm and an Agents-tab open may both deliver at once.
mkdir -p "$HOME/.paseo" "$(dirname "$dir")"
exec 9>"$HOME/.paseo/mio-up.lock"
flock -w 20 9 || { echo "plugin delivery: lock busy after 20s" >&2; exit 1; }
# Another delivery may have finished while waiting; swapping an identical tree is needless churn.
[ "$(cat "$dir/.bundle-hash" 2>/dev/null)" = "$hash" ] && exit 0

rm -rf "$staging" "$old"
mkdir -p "$staging"
# Headers built on a Windows host may carry its uid, which the container cannot assign.
tar --no-same-owner -xf - -C "$staging"
printf '%s' "$hash" >"$staging/.bundle-hash"
if [ -d "$dir" ]; then
  mv "$dir" "$old"
fi
mv "$staging" "$dir"
rm -rf "$old"
