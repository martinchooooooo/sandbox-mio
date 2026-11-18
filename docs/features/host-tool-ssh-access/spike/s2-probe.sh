#!/bin/bash
# S2 probe (docs/features/host-tool-ssh-access/spike/s2-path.md). Run as `ssh <alias> /tmp/s2-probe.sh`:
# sshd's `bash -c` reads ~/.bashrc and then execs this script, which is a non-interactive shell that
# reads no startup file and only inherits the session's environment -- the position a client's remote
# agent is in when it spawns a tool with a working directory set, without a shell `cd` hook.
probe() { # $1 = directory the tool is run from
  local out
  out=$(cd "$1" && { p=$(command -v node) && v=$(node -v 2>&1 | head -1) && echo "$p $v" || echo "node: NOT FOUND ($(node -v 2>&1 | head -1))"; })
  printf '  %-12s %s\n' "$1" "$out"
}
echo "probe SHLVL=$SHLVL PATH=$PATH"
probe /root
probe /root/work
probe /root/work/sub
