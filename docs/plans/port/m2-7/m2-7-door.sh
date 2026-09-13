#!/bin/sh
# M2-7 — the door: build the real `folio` this ticket's sweep is run against.
#
# Lane `m2-7`, its own target directory, `nice -n 10 -j 4` — the venue's rule
# for three concurrent lanes (`~/folio-port/README-agents.md` §Lanes). Nothing
# outside ~/folio-port is written.
set -u

export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-m2-7"
export CARGO_INCREMENTAL=0
CARGO="$HOME/.cargo/bin/cargo"
WT="$HOME/folio-port/wt/m2-7"

cd "$WT" || { echo "NO WORKTREE at $WT"; echo "ALL_DONE"; exit 1; }
echo "head: $(git rev-parse HEAD)"

echo "=== cargo build -p bt-app ==="
/usr/bin/time -l nice -n 10 "$CARGO" build -p bt-app -j 4 2>&1 | tail -25
echo "rc=$?"
ls -la "$CARGO_TARGET_DIR/debug/folio" 2>&1

echo "=== cargo test -p bt-platform -j 4 (the watch contracts this sweep leans on) ==="
nice -n 10 "$CARGO" test -p bt-platform -j 4 2>&1 | tail -12
echo "rc=$?"

echo "df: $(df -h "$HOME" | tail -1)"
echo "ALL_DONE"
