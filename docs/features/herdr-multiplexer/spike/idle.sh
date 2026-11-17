# Runs inside the container: RSS and CPU over a 10 s window for every process matching the pgrep -f pattern $1, sampled together.
pids=$(pgrep -f "$1" | grep -vx $$)
for p in $pids; do eval "a_$p=$(awk '{print $14+$15}' /proc/$p/stat)"; done
sleep 10
for p in $pids; do
  b=$(awk '{print $14+$15}' /proc/$p/stat 2>/dev/null) || continue
  eval "a=\$a_$p"
  echo "pid=$p cmd=$(tr '\0' ' ' </proc/$p/cmdline) rss_kb=$(awk '/VmRSS/{print $2}' /proc/$p/status) cpu_ticks_10s=$((b-a))"
done
