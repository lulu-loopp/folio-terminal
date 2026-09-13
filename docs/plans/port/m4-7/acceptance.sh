#!/bin/bash
# M4-7 acceptance: the attention endpoint over a Unix socket, on the real binary.
#
#   "A hook posts `folio attention <family>:<event>` from a shell and the pane it
#    names is raised — the line the Windows path writes into BT_ATTENTION_TRACE."
#
# Every process this script starts has its pid written down and only those pids
# are ever ended. Nothing here writes the pasteboard, injects a key, or touches
# the owner's own ~/.claude, ~/.codex or ~/.copilot.
set -u
export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-m4-7"
WT="$HOME/folio-port/wt/m4-7"
PROBE="$WT/.probe"
FOLIO="$CARGO_TARGET_DIR/debug/folio"
cd "$WT" || { echo "no worktree"; exit 1; }

rm -rf "$PROBE"
mkdir -p "$PROBE/home"
ISO="$PROBE/home"

ls -l "$FOLIO" || exit 1

echo "== the shape of \$TMPDIR on this machine =="
echo "TMPDIR=$TMPDIR"
RUNTIME="${TMPDIR}folio-$(id -u)"
echo "RUNTIME=$RUNTIME"
echo "-- is it per-session? two different logins, same directory: --"
echo "DARWIN_USER_TEMP_DIR=$(getconf DARWIN_USER_TEMP_DIR)"

# **How the pane's three variables get out of the pane.** No key injection and no
# pasteboard: the shell Folio spawns runs the isolated HOME's own startup files,
# so the startup file writes them down. Every spelling, because which shell this
# machine hands a pane is the machine's business and not this script's.
DUMP='if [ -n "${FOLIO_ATTENTION_PIPE:-}" ]; then
  printf "%s\n" "$FOLIO_ATTENTION_PIPE" > "$HOME/pane-endpoint.txt"
  printf "%s\n" "${FOLIO_ATTENTION:-}"  > "$HOME/pane-capability.txt"
  printf "%s\n" "${FOLIO_PANE:-}"       > "$HOME/pane-name.txt"
fi'
for rc in .zshenv .zshrc .profile .bashrc .bash_profile; do
  printf '%s\n' "$DUMP" > "$ISO/$rc"
done

echo "== runtime directory before =="
ls -la "$RUNTIME" 2>&1 | head -20

echo "== (1) start Folio on an isolated HOME =="
env -u BT_STARTUP_TRACE HOME="$ISO" \
  BT_PTY_DUMP="$PROBE/pty.dump" \
  BT_ATTENTION_TRACE="$PROBE/attention.trace" \
  "$FOLIO" > "$PROBE/folio.log" 2>&1 &
P1=$!
echo "P1=$P1"
sleep 15
ps -p "$P1" -o pid=,stat= || echo "P1 is not running"

echo "== (2) what the pane was told =="
for f in pane-name.txt pane-endpoint.txt; do
  printf '%s: ' "$f"
  cat "$ISO/$f" 2>&1 || echo "(absent)"
done
printf 'pane-capability.txt: '
if [ -s "$ISO/pane-capability.txt" ]; then
  echo "present, $(wc -c < "$ISO/pane-capability.txt" | tr -d ' ') bytes (not printed)"
else
  echo "(absent)"
fi

echo "== (3) the doorbell on disk =="
ls -la "$RUNTIME" | grep -E "attn|sock|lock|^total|^d" || true
ENDPOINT=$(cat "$ISO/pane-endpoint.txt" 2>/dev/null || true)
echo "endpoint=$ENDPOINT"
if [ -n "$ENDPOINT" ]; then
  echo "-- stat --"
  stat -f '%Sp %Su %N' "$ENDPOINT"
  echo "-- the directory above --"
  stat -f '%Sp %Su %N' "$RUNTIME"
  echo "-- path length (sun_path is 104 including the terminator) --"
  printf '%s' "$ENDPOINT" | wc -c
fi

echo "== (4) the hook script, exactly as the installer renders it on this platform =="
printf '#!/bin/sh\n%s\n' "'$FOLIO' attention claude-code:PermissionRequest" > "$PROBE/hook.sh"
chmod +x "$PROBE/hook.sh"
cat "$PROBE/hook.sh"

echo "== (5) the hook posts, with the pane's own capability =="
env FOLIO_ATTENTION_PIPE="$ENDPOINT" \
    FOLIO_ATTENTION="$(cat "$ISO/pane-capability.txt")" \
    FOLIO_PANE="$(cat "$ISO/pane-name.txt")" \
    sh "$PROBE/hook.sh"
echo "hook-rc=$?"
sleep 4

echo "== (6) what the window wrote down =="
cat "$PROBE/attention.trace" 2>&1 || echo "(no trace)"

echo "== (7) a capability that names no pane is delivered and refused =="
BEFORE=$(grep -c mint "$PROBE/attention.trace" 2>/dev/null || echo 0)
env FOLIO_ATTENTION_PIPE="$ENDPOINT" \
    FOLIO_ATTENTION="00000000000000000000000000000000" \
    sh "$PROBE/hook.sh"
echo "forged-hook-rc=$?"
sleep 4
AFTER=$(grep -c mint "$PROBE/attention.trace" 2>/dev/null || echo 0)
echo "mint-lines-before=$BEFORE mint-lines-after=$AFTER"

echo "== (8) a name outside this user's runtime directory never reaches a socket =="
env FOLIO_ATTENTION_PIPE="/tmp/folio.sock" \
    FOLIO_ATTENTION="00000000000000000000000000000000" \
    sh "$PROBE/hook.sh"
echo "off-grammar-hook-rc=$?"

echo "== (9) end the one pid this script started =="
kill "$P1"
sleep 4
ps -p "$P1" -o pid=,stat= || echo "P1 has ended"

echo "== (10) the name it left behind, and the next holder clearing it =="
ls -la "$RUNTIME" | grep -E "attn|\.sock|\.lock" || echo "(nothing left)"
env -u BT_STARTUP_TRACE HOME="$ISO" \
  BT_PTY_DUMP="$PROBE/pty2.dump" \
  BT_ATTENTION_TRACE="$PROBE/attention2.trace" \
  "$FOLIO" > "$PROBE/folio2.log" 2>&1 &
P2=$!
echo "P2=$P2"
sleep 15
ps -p "$P2" -o pid=,stat= || echo "P2 is not running"
echo "-- the second run's doorbell, which is a socket bound over the cleared name --"
ls -la "$RUNTIME" | grep -E "attn|\.sock" || true
stat -f '%Sp %Su %N' "$ENDPOINT" 2>&1 || true
echo "-- and it answers --"
env FOLIO_ATTENTION_PIPE="$ENDPOINT" \
    FOLIO_ATTENTION="$(cat "$ISO/pane-capability.txt")" \
    sh "$PROBE/hook.sh"
echo "second-run-hook-rc=$?"
sleep 4
cat "$PROBE/attention2.trace" 2>&1 || echo "(no trace)"
kill "$P2"
sleep 4
ps -p "$P2" -o pid=,stat= || echo "P2 has ended"

echo "== folio's own stdout, both runs =="
echo "-- run 1 --"; cat "$PROBE/folio.log"
echo "-- run 2 --"; cat "$PROBE/folio2.log"

echo "== the data directory the isolated HOME got =="
ls -1 "$ISO/Library/Application Support/Folio" 2>&1

echo ALL_DONE
