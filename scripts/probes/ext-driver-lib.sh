# D-72 checks: helpers for the bash drivers of scripts/probes/ext-probe.sh (source this file).
#
#   wait_for <word> [timeout s]   waits until the probe reported `@@<word>`
#   signal_done <word>            notes in the driver's output that the step ran (with the time)
#   shot <file.png>               captures the window of APP_PID only (xwd), like capture-window.sh;
#                                 look at every capture: a window on top ends up in it
#   sample_cpu <secs> <file>      every 0.5 s for <secs> seconds writes `time ps-%cpu rss-kB ticks`
#                                 of APP_PID (`ps` gives the lifetime average; `ticks` is
#                                 utime+stime from /proc, so the CPU of each interval is
#                                 Δticks / (Δtime × CLK_TCK))
#
# APP_PID and PROBE_OUT come from ext-probe.sh.
here_lib=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

wait_for() {
  local word=$1 limit=${2:-180} n=0
  while ! grep -q " @@$word\$" "$PROBE_OUT" 2>/dev/null; do
    sleep 0.05
    n=$((n + 1))
    if [ "$n" -gt $((limit * 20)) ]; then
      echo "driver: gave up waiting for @@$word"
      return 1
    fi
  done
}

signal_done() { echo "driver $(date +%s.%N | cut -c1-14): $*"; }

shot() {
  local out=$1 win="" id p w
  for id in $(xwininfo -root -tree | awk '/\("cairn" "Cairn"\)/ {print $1}'); do
    p=$(xprop -id "$id" _NET_WM_PID 2>/dev/null | awk '{print $3}')
    w=$(xwininfo -id "$id" | awk '/Width:/ {print $2}')
    [ "$p" = "$APP_PID" ] && [ "${w:-0}" -gt 100 ] && win=$id
  done
  if [ -n "$win" ]; then
    xwd -silent -id "$win" -out "${out%.png}.xwd" && python3 -I "$here_lib/xwd2png.py" "${out%.png}.xwd" >/dev/null && rm -f "${out%.png}.xwd"
    echo "driver: shot $out"
  else
    echo "driver: no window for pid $APP_PID"
  fi
}

sample_cpu() {
  local secs=$1 file=$2 end
  end=$(($(date +%s) + secs))
  while [ "$(date +%s)" -lt "$end" ] && [ -d "/proc/$APP_PID" ]; do
    echo "$(date +%s.%N | cut -c1-14) $(ps -o %cpu=,rss= -p "$APP_PID") $(awk '{print $14 + $15}' "/proc/$APP_PID/stat")" >> "$file"
    sleep 0.5
  done
}
