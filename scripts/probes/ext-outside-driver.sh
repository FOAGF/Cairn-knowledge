#!/bin/bash
# D-72 driver for ext-outside.js (PROBE_DRIVER of ext-probe.sh): writes files in the knowledge
# folder when the probe asks, and samples the app's CPU and memory. Env: ROOT (the knowledge
# folder), CPU (file for `sample_cpu` lines: time ps-%cpu rss-kB ticks, for the whole run).
source "$(dirname "$0")/ext-driver-lib.sh"
sample_cpu 400 "$CPU" &
wait_for b4-edit && { printf '\nOUTSIDE-EDIT made by another program.\n' >> "$ROOT/Learning/Rust ownership.md"; signal_done "edited Rust ownership"; }
wait_for b4-new && { printf '# Made outside\n\nA page written by another tool.\n' > "$ROOT/Learning/Made outside.md"; signal_done "new file"; }
wait_for b4-remove && { rm "$ROOT/Learning/Made outside.md"; signal_done "removed file"; }
wait_for b4-folder && { mkdir -p "$ROOT/Projects" && printf 'Plans for next year.\n' > "$ROOT/Projects/Plans.md"; signal_done "new folder Projects"; }
wait_for b5-burst && {
  python3 -I -c "
import os, sys
root = sys.argv[1]
for g in range(5):
    d = os.path.join(root, 'Projects', 'Burst', 'G%d' % g)
    os.makedirs(d, exist_ok=True)
    for i in range(100):
        with open(os.path.join(d, 'Note %03d.md' % (g * 100 + i)), 'w') as f:
            f.write('# Note %03d\n\nWritten in a burst. See [Plans](../../Plans.md).\n' % (g * 100 + i) + 'Some text. ' * 40 + '\n')
" "$ROOT"
  signal_done "500 files written"
}
wait_for b5-edits && {
  python3 -I -c "
import os, sys, time
root = sys.argv[1]
t0 = time.time()
for n in range(500):
    g = n // 100
    p = os.path.join(root, 'Projects', 'Burst', 'G%d' % g, 'Note %03d.md' % n)
    with open(p, 'a') as f:
        f.write('\nEDITED %d\n' % n)
    time.sleep(max(0, t0 + (n + 1) * 0.01 - time.time()))
print('driver: 500 edits in %.2f s' % (time.time() - t0))
" "$ROOT"
  signal_done "500 edits written"
}
wait_for b6-rename && { mv "$ROOT/Learning/Rust ownership.md" "$ROOT/Learning/Rust ownership (renamed).md"; signal_done "renamed"; }
wait_for b6-move && { mv "$ROOT/Learning/Rust ownership (renamed).md" "$ROOT/Operations/"; signal_done "moved"; }
wait_for b6-zh && { mv "$ROOT/Learning/中文笔记.md" "$ROOT/Learning/中文笔记 2.md" && printf '\nchanged\n' >> "$ROOT/Learning/中文笔记 2.md"; signal_done "renamed and changed"; }
wait
