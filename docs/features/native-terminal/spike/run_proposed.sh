#!/usr/bin/env bash
# Runs task 2 (podman) and task 3 (msb) of the spike: the russh probe in each mode on a warm container, a
# cold-container variant, and, for podman, the plain OpenSSH baseline.
# Output: results/proposed_*.jsonl, results/*_ms.txt.
# Usage: ./run_proposed.sh <engine podman|msb> <warm-container> <cold-container> [iterations]
# For msb pass the same sandbox name twice; export MSB_EXE first.
set -u
cd "$(dirname "$0")"; source ./env.sh 2>/dev/null
ENGINE=$1; WARM=$2; COLD=$3; N=${4:-8}
PROBE="$SCRATCH/target-probe/release/ssh-probe.exe"; KEY="$SCRATCH/keys/id_ed25519"
P="$PODMAN_EXE --connection $PODMAN_CONNECTION"
mkdir -p results
now() { date +%s%N; }
if [ "$ENGINE" = podman ]; then MODES="true shell zellij zellij-new"; else MODES="true shell zellij"; fi
for m in $MODES; do
  "$PROBE" "$ENGINE" "$WARM" "$m" "$N" "$KEY" > "results/proposed_${ENGINE}_warm_$m.jsonl"
done
if [ "$ENGINE" = podman ]; then
  # Cold: a stopped container, `podman start` timed separately, session created fresh by attach.
  : > results/proposed_podman_cold_zellij-new.jsonl; : > results/proposed_podman_cold_start_ms.txt
  for i in $(seq "$N"); do
    $P stop -t 0 "$COLD" >/dev/null 2>&1
    s=$(now); $P start "$COLD" >/dev/null 2>&1; e=$(now)
    echo $(( (e-s)/1000000 )) >> results/proposed_podman_cold_start_ms.txt
    "$PROBE" podman "$COLD" zellij-new 1 "$KEY" >> results/proposed_podman_cold_zellij-new.jsonl
  done
  # OpenSSH baseline over the same ProxyCommand (warm container).
  : > results/openssh_true_ms.txt
  for i in $(seq "$N"); do
    s=$(now); "$SSH_EXE" -F "$SPIKE_SSH_CONFIG" -tt "$WARM" true >/dev/null 2>&1; e=$(now)
    echo $(( (e-s)/1000000 )) >> results/openssh_true_ms.txt
  done
  # Floor: one bare `podman exec` of a no-op.
  : > results/podman_exec_true_ms.txt
  for i in $(seq "$N"); do
    s=$(now); $P exec "$WARM" true >/dev/null 2>&1; e=$(now)
    echo $(( (e-s)/1000000 )) >> results/podman_exec_true_ms.txt
  done
else
  # Cold msb: stop the microVM, time `msb start`, then attach (the zellij session died with the VM).
  : > results/proposed_msb_cold_zellij.jsonl; : > results/proposed_msb_cold_start_ms.txt
  for i in $(seq "$N"); do
    "$MSB_EXE" stop "$COLD" >/dev/null 2>&1
    s=$(now); "$MSB_EXE" start "$COLD" >/dev/null 2>&1; e=$(now)
    echo $(( (e-s)/1000000 )) >> results/proposed_msb_cold_start_ms.txt
    "$PROBE" msb "$COLD" zellij 1 "$KEY" >> results/proposed_msb_cold_zellij.jsonl
  done
  : > results/msb_exec_true_ms.txt
  for i in $(seq "$N"); do
    s=$(now); "$MSB_EXE" exec --stream "$WARM" -- true >/dev/null 2>&1; e=$(now)
    echo $(( (e-s)/1000000 )) >> results/msb_exec_true_ms.txt
  done
fi
