#!/bin/sh
# T-MAC-DOCKMENU — `applicationDockMenu:` proved on the real delegate.
#
# Right-clicking a Dock tile cannot be driven by an agent: it needs `System
# Events`, and therefore Accessibility *and* Automation, behind a TCC prompt an
# ssh session cannot reach (X-4 measured it). So what is driven here is the whole
# of what the Dock does when it is right-clicked — send the application delegate
# `applicationDockMenu:`, then send the chosen item's own action — asked of this
# process's own delegate and this process's own items, with no window server
# gesture at all.
#
# The binary under test is `tests/macos_dock_menu`, inside a throwaway ad-hoc
# signed bundle with an identifier of its own and an **isolated HOME** — through
# a `CFBundleExecutable` wrapper script, because `LSEnvironment` cannot set
# `HOME` (DESIGN §13.31 ⑧(d)). It is started with `open`, which asks launchd to
# run it in the logged-in session, because an ssh session cannot reach the window
# server a winit event loop needs (X-1).
#
# Everything lands under ~/folio-port/out-dockmenu/. Nothing outside
# ~/folio-port is written apart from the four `~/Library` shells a bundle
# identifier leaves behind, which are removed at the end, and no process this
# script did not start is looked at or signalled.
set -u

export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-dock"
export CARGO_INCREMENTAL=0
CARGO="$HOME/.cargo/bin/cargo"

REPO="$HOME/folio-port/wt/dockmenu"
OUT="$HOME/folio-port/out-dockmenu"
APP="$OUT/FolioDockMenu.app"
ISO="$OUT/home"
BUNDLE_ID=io.github.lulu-loopp.folio.dockmenu
REPORT="$OUT/t-mac-dockmenu-report.log"

cd "$REPO" || { echo "NO REPO at $REPO"; echo "ALL_DONE"; exit 1; }
echo "=== HEAD ==="
git --no-pager log --oneline -1
git status --short | head -5

echo "=== cargo test --locked -p bt-platform -j 2 ==="
nice -n 10 "$CARGO" test --locked -p bt-platform -j 2 2>&1 | tail -40
echo "rc=$?"

echo "=== build the probe binary ==="
nice -n 10 "$CARGO" test --locked -p bt-platform --test macos_dock_menu --no-run -j 2 2>&1 | tail -10
echo "rc=$?"
BIN=$(ls -t "$CARGO_TARGET_DIR"/debug/deps/macos_dock_menu-* 2>/dev/null | grep -v '\.d$' | head -1)
if [ ! -x "${BIN:-/nonexistent}" ]; then
  echo "NO PROBE BINARY under $CARGO_TARGET_DIR/debug/deps"
  echo "ALL_DONE"
  exit 1
fi
echo "probe binary: $BIN ($(stat -f %z "$BIN") bytes)"

echo "=== the bundle ==="
rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$ISO"
cp "$BIN" "$APP/Contents/MacOS/macos_dock_menu"

# The wrapper is the bundle's executable, so that HOME is this run's own
# directory before a single AppKit call has been made.
cat > "$APP/Contents/MacOS/dock-menu-launch" <<LAUNCH
#!/bin/sh
export HOME="$ISO"
exec "\$(dirname "\$0")/macos_dock_menu" "\$@"
LAUNCH
chmod +x "$APP/Contents/MacOS/dock-menu-launch"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>dock-menu-launch</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>FolioDockMenu</string>
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

echo "=== open ==="
/usr/bin/open "$APP" 2>&1; echo "rc=$?"

echo "=== waiting for the report ==="
i=0
while [ $i -lt 120 ]; do
  if [ -f "$REPORT" ] && grep -q ALL_DONE "$REPORT"; then break; fi
  sleep 1
  i=$((i + 1))
done
echo "waited ${i}s"
echo "--- report ---"
cat "$REPORT" 2>&1
echo "--- FAIL lines ---"
grep -c FAIL "$REPORT" 2>/dev/null

echo "--- this binary, read and never signalled ---"
/bin/ps -axo pid,comm= | grep macos_dock_menu | grep -v grep

echo "=== remove ==="
# The process ends itself when its loop leaves; anything still standing is this
# script's own launch and is ended by the pid this script read, by number.
for pid in $(/bin/ps -axo pid=,command= | grep "$APP/Contents/MacOS/macos_dock_menu" | grep -v grep | awk '{print $1}'); do
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
