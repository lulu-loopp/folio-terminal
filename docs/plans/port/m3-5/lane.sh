#!/bin/bash
# M3-5 lane: bt-platform's suite on the Mac, and bt-app checked.
set -u
export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-m3-5"
export CARGO_INCREMENTAL=0
CARGO="$HOME/.cargo/bin/cargo"
WT="$HOME/folio-port/wt/m3-5"
cd "$WT" || { echo "no worktree"; exit 1; }

echo "== HEAD =="
git log --oneline -1

echo "== cargo test --locked -p bt-platform -j 4 =="
/usr/bin/time -l nice -n 10 "$CARGO" test --locked -p bt-platform -j 4
echo "rc=$?"

echo "== cargo check --locked -p bt-app --all-targets -j 4 =="
/usr/bin/time -l nice -n 10 "$CARGO" check --locked -p bt-app --all-targets -j 4
echo "rc=$?"

echo "== df =="
df -h "$HOME" | tail -1
echo ALL_DONE
