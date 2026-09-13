#!/bin/bash
# M3-5 acceptance: the §M3 sentence on the real binary.
#   "Start a second copy from a terminal with a different data directory and it
#    runs independently; start one with the same data directory and it hands
#    over rather than writing. `ls ~/Library/Application Support/Folio` lists
#    session.json and settings.json."
#
# Every process this script starts has its pid written down and only those pids
# are ever ended.
set -u
export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-m3-5"
export CARGO_INCREMENTAL=0
CARGO="$HOME/.cargo/bin/cargo"
WT="$HOME/folio-port/wt/m3-5"
PROBE="$WT/.probe"
cd "$WT" || { echo "no worktree"; exit 1; }

rm -rf "$PROBE"
mkdir -p "$PROBE/homeA" "$PROBE/homeB"

echo "== cargo build --locked -p bt-app -j 4 =="
nice -n 10 "$CARGO" build --locked -p bt-app -j 4
echo "rc=$?"
FOLIO="$CARGO_TARGET_DIR/debug/folio"
ls -l "$FOLIO" || exit 1

RUNTIME="${TMPDIR}folio-$(id -u)"
echo "== runtime directory before =="
ls -la "$RUNTIME" 2>&1 | head -20

start() {  # start <home> <logname>
  env -u BT_STARTUP_TRACE HOME="$1" BT_PTY_DUMP="$PROBE/pty.dump" \
    "$FOLIO" > "$PROBE/$2.log" 2>&1 &
  echo $!
}

echo "== ① the first copy, data directory A =="
A1=$(start "$PROBE/homeA" a1)
echo "A1=$A1"
sleep 12
ps -p "$A1" -o pid=,stat= || echo "A1 is not running"
echo "-- ls the data directory --"
ls -1 "$PROBE/homeA/Library/Application Support/Folio"

echo "== ② a second copy on the SAME data directory: it hands over =="
BEGAN=$(date +%s)
env -u BT_STARTUP_TRACE HOME="$PROBE/homeA" BT_PTY_DUMP="$PROBE/pty.dump" \
  "$FOLIO" > "$PROBE/a2.log" 2>&1
echo "second-copy-rc=$?"
echo "second-copy-seconds=$(( $(date +%s) - BEGAN ))"
echo "-- what the handed-over copy said --"
cat "$PROBE/a2.log"
echo "-- how many folio processes are alive --"
ps -axo pid=,comm= | grep -c "/debug/folio$"
ps -p "$A1" -o pid=,stat= || echo "A1 died"

echo "== ③ a copy on a DIFFERENT data directory: it runs independently =="
B1=$(start "$PROBE/homeB" b1)
echo "B1=$B1"
sleep 12
ps -p "$A1" -o pid=,stat= || echo "A1 died"
ps -p "$B1" -o pid=,stat= || echo "B1 died"
echo "-- ls both data directories --"
ls -1 "$PROBE/homeA/Library/Application Support/Folio"
ls -1 "$PROBE/homeB/Library/Application Support/Folio"

echo "== ④ the runtime directory: one lock and one socket per data directory =="
ls -la "$RUNTIME"
stat -f '%Sp %u %N' "$RUNTIME"
for f in "$RUNTIME"/*.sock "$RUNTIME"/*.lock; do
  [ -e "$f" ] && stat -f '%Sp %u %N' "$f"
done

echo "== ⑤ a second copy on data directory B hands over too =="
env -u BT_STARTUP_TRACE HOME="$PROBE/homeB" BT_PTY_DUMP="$PROBE/pty.dump" \
  "$FOLIO" > "$PROBE/b2.log" 2>&1
echo "second-copy-B-rc=$?"
cat "$PROBE/b2.log"

echo "== ending the two pids this script started, and nothing else =="
kill "$A1" 2>&1
kill "$B1" 2>&1
sleep 6
ps -p "$A1" -o pid= >/dev/null 2>&1 && { echo "A1 still up; SIGKILL $A1"; kill -9 "$A1"; }
ps -p "$B1" -o pid= >/dev/null 2>&1 && { echo "B1 still up; SIGKILL $B1"; kill -9 "$B1"; }
sleep 2
echo "-- the endpoints after both quit --"
ls -la "$RUNTIME"
echo "-- the two logs --"
tail -5 "$PROBE/a1.log"
tail -5 "$PROBE/b1.log"
echo ALL_DONE
