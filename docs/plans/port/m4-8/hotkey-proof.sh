#!/bin/sh
# M4-8 — a real Carbon hot key, claimed and then pressed (`docs/DESIGN.md` §13.51).
#
# `RegisterEventHotKey` needs no TCC grant, and `CGEventPost(kCGHIDEventTap, …)`
# needs none either — which is the whole reason this ticket's mechanism is
# Carbon and not `CGEventTap`. So the proof can be driven from an ssh session,
# unlike the Dock menu's, and unlike it there is nothing here a human has to
# press.
#
# The binary under test is `tests/macos_hotkey`, inside a throwaway ad-hoc
# signed bundle with an identifier of its own and an **isolated HOME** —
# through a `CFBundleExecutable` wrapper script, because `LSEnvironment` cannot
# set `HOME` (DESIGN §13.31 ⑧(d)). It is started with `open`, which asks launchd
# to run it in the logged-in session, because an ssh session cannot reach the
# window server a winit event loop needs (X-1).
#
# **What it presses.** One `⌃` + backtick chord, once, to the key this process
# has just registered for itself, and only after the registration was accepted.
# Nothing else is posted, nothing is written to the pasteboard, and no process
# this script did not start is looked at or signalled.
#
# Everything lands under ~/folio-port/out-hotkey/. Nothing outside ~/folio-port
# is written apart from the four `~/Library` shells a bundle identifier leaves
# behind, which are removed at the end.
set -u

export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-hotkey"
export CARGO_INCREMENTAL=0
CARGO="$HOME/.cargo/bin/cargo"

REPO="$HOME/folio-port/wt/hotkey"
OUT="$HOME/folio-port/out-hotkey"
APP="$OUT/FolioHotkey.app"
ISO="$OUT/home"
BUNDLE_ID=io.github.lulu-loopp.folio.hotkey
REPORT="$OUT/m4-8-hotkey-report.log"

cd "$REPO" || { echo "NO REPO at $REPO"; echo "ALL_DONE"; exit 1; }
echo "=== HEAD ==="
git --no-pager log --oneline -1
git status --short | head -5

echo "=== build the probe binary ==="
# **The build's own answer is read, and a stale binary is refused.** Without
# this the `ls -t` below happily finds the previous run's executable and the
# report that comes back is a report about code that is no longer in the tree —
# which is a worse outcome than no report at all.
rm -f "$CARGO_TARGET_DIR"/debug/deps/macos_hotkey-*
if ! nice -n 10 "$CARGO" test --locked -p bt-platform --test macos_hotkey --no-run -j 2 2>&1 | tail -20; then
  echo "THE PROBE DID NOT BUILD"
  echo "ALL_DONE"
  exit 1
fi
BIN=$(ls -t "$CARGO_TARGET_DIR"/debug/deps/macos_hotkey-* 2>/dev/null | grep -v '\.d$' | head -1)
if [ ! -x "${BIN:-/nonexistent}" ]; then
  echo "NO PROBE BINARY under $CARGO_TARGET_DIR/debug/deps"
  echo "ALL_DONE"
  exit 1
fi
echo "probe binary: $BIN ($(stat -f %z "$BIN") bytes)"

echo "=== the bundle ==="
rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$ISO"
cp "$BIN" "$APP/Contents/MacOS/macos_hotkey"

# The wrapper is the bundle's executable, so that HOME is this run's own
# directory before a single AppKit call has been made.
cat > "$APP/Contents/MacOS/hotkey-launch" <<LAUNCH
#!/bin/sh
export HOME="$ISO"
exec "\$(dirname "\$0")/macos_hotkey" "\$@"
LAUNCH
chmod +x "$APP/Contents/MacOS/hotkey-launch"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>hotkey-launch</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>FolioHotkey</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

/usr/bin/plutil -lint "$APP/Contents/Info.plist"; echo "rc=$?"
/usr/bin/codesign --force --sign - --timestamp=none "$APP" 2>&1; echo "rc=$?"

echo "=== who is frontmost before the press ==="
/usr/bin/osascript -e 'tell application "System Events" to name of first application process whose frontmost is true' 2>&1 | head -1

echo "=== open ==="
/usr/bin/open "$APP" 2>&1; echo "rc=$?"

echo "=== waiting for the report ==="
i=0
while [ $i -lt 60 ]; do
  if [ -f "$REPORT" ] && grep -q ALL_DONE "$REPORT"; then break; fi
  sleep 1
  i=$((i + 1))
done
echo "waited ${i}s"
echo "--- report ---"
cat "$REPORT" 2>&1
echo "--- FAIL lines ---"
# The word, not the substring: the report's own last line is `FAILURES 0`, and a
# count that included it would say one failure on a clean run.
grep -c "\] FAIL " "$REPORT" 2>/dev/null

echo "--- this binary, read and never signalled ---"
/bin/ps -axo pid,comm= | grep macos_hotkey | grep -v grep

echo "=== remove ==="
# The process ends itself when its loop leaves; anything still standing is this
# script's own launch and is ended by the pid this script read, by number.
for pid in $(/bin/ps -axo pid=,command= | grep "$APP/Contents/MacOS/macos_hotkey" | grep -v grep | awk '{print $1}'); do
  echo "ending $pid"
  kill "$pid" 2>&1
done
rm -rf "$APP"
rm -rf "$HOME/Library/Caches/$BUNDLE_ID" \
       "$HOME/Library/Saved Application State/$BUNDLE_ID.savedState" \
       "$HOME/Library/WebKit/$BUNDLE_ID" \
       "$HOME/Library/HTTPStorages/$BUNDLE_ID"
echo "df: $(df -h "$HOME" | tail -1)"
echo "ALL_DONE"
