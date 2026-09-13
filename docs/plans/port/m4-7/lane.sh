#!/bin/sh
# M4-7 — the attention endpoint over a Unix socket. Build and test lane.
set -u
export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-m4-7"
export CARGO_INCREMENTAL=0
CARGO="$HOME/.cargo/bin/cargo"
cd "$HOME/folio-port/wt/m4-7" || exit 1

echo "== HEAD =="
git log --oneline -1

echo "== cargo test -p bt-platform -j 4 =="
nice -n 10 "$CARGO" test --locked -p bt-platform -j 4
echo "rc=$?"

echo "== cargo check -p bt-app --all-targets -j 4 =="
nice -n 10 "$CARGO" check --locked -p bt-app --all-targets -j 4
echo "rc=$?"

echo "== cargo build -p bt-app -j 4 =="
nice -n 10 "$CARGO" build --locked -p bt-app -j 4
echo "rc=$?"

echo "== the binary =="
ls -l "$CARGO_TARGET_DIR/debug/folio"

echo ALL_DONE
