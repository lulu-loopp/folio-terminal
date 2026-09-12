#!/bin/sh
# X-2: build probe-x2 and wrap it in an ad-hoc signed .app bundle.
# Everything lands under ~/folio-port/wt/x2/out/.
set -u

export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target"
export CARGO_INCREMENTAL=0

CRATE="$HOME/folio-port/wt/x2/probe-x2"
OUT="$HOME/folio-port/wt/x2/out"
APP="$OUT/ProbeX2.app"

echo "=== cargo build ==="
cd "$CRATE" || exit 1
/usr/bin/time -l nice -n 10 "$HOME/.cargo/bin/cargo" build -j 6 2>&1
echo "rc=$?"

BIN="$CARGO_TARGET_DIR/debug/probe-x2"
if [ ! -x "$BIN" ]; then
  echo "NO BINARY at $BIN"
  echo "ALL_DONE"
  exit 1
fi
echo "binary bytes: $(stat -f %z "$BIN")"

echo "=== bundle ==="
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/probe-x2"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>probe-x2</string>
  <key>CFBundleIdentifier</key><string>io.github.lulu-loopp.folio.probe-x2</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>ProbeX2</string>
  <key>CFBundleDisplayName</key><string>Folio X-2 Probe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSAppTransportSecurity</key>
  <dict>
    <key>NSAllowsLocalNetworking</key><true/>
    <key>NSAllowsArbitraryLoads</key><true/>
  </dict>
</dict>
</plist>
PLIST

/usr/bin/plutil -lint "$APP/Contents/Info.plist"
echo "rc=$?"

echo "=== codesign (ad-hoc) ==="
/usr/bin/codesign --force --sign - --timestamp=none "$APP" 2>&1
echo "rc=$?"
/usr/bin/codesign -dv --verbose=2 "$APP" 2>&1
echo "rc=$?"

echo "ALL_DONE"
