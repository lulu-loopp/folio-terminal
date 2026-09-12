#!/bin/sh
# X-2: run the probe in the logged-in GUI session.
#
# `open` is the only way in from an ssh session: a process started here is in a
# background session and cannot reach the window server. The probe writes its
# own pid as its first log line, which is the only pid this script will ever
# end — nothing is matched by name.
set -u
L="$HOME/folio-port/logs/x2/probe-x2-run.log"
APP="$HOME/folio-port/wt/x2/out/ProbeX2.app"
mkdir -p "$HOME/folio-port/logs/x2"
rm -f "$L"

open "$APP"
echo "open rc=$?"

i=0
while [ $i -lt 100 ]; do
  if [ -f "$L" ] && grep -q "PROBE_X2_DONE" "$L"; then
    echo "probe finished on its own"
    break
  fi
  sleep 1
  i=$((i + 1))
done

PID=$(sed -n 's/^probe-x2 start, pid \([0-9]*\)$/\1/p' "$L" 2>/dev/null | head -1)
echo "probe pid was ${PID:-unknown}"
if [ -n "${PID:-}" ] && kill -0 "$PID" 2>/dev/null; then
  echo "still running after the timeout; ending that pid"
  kill "$PID"
  sleep 2
  if kill -0 "$PID" 2>/dev/null; then
    echo "did not exit on TERM; sending KILL to that pid"
    kill -9 "$PID"
  fi
else
  echo "that pid is gone"
fi

echo "=== log"
cat "$L" 2>/dev/null
echo ALL_DONE
