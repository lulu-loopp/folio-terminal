#!/bin/sh
# M4-9 — the Services door, proved end to end.
#
# Builds the worktree, wraps `tests/macos_services` in two ad-hoc signed
# bundles (the application under test and a sender with an identifier of its
# own, which X-4 measured is required), registers both, performs the Service
# cold and then five times warm from inside the application's own script, and
# unregisters and deletes both at the end.
#
# Everything lands under ~/folio-port/wt/m4-9/out/. Nothing outside
# ~/folio-port is written and no process this script did not start is looked at.
set -u

export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
export CARGO_TARGET_DIR="$HOME/folio-port/target-m4-9"
export CARGO_INCREMENTAL=0
CARGO="$HOME/.cargo/bin/cargo"

WT="$HOME/folio-port/wt/m4-9"
OUT="$WT/out"
APP="$OUT/FolioServicesM49.app"
SENDER="$OUT/ServiceSender.app"
ROW="Open in Folio M4-9"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
PBS=/System/Library/CoreServices/pbs

cd "$WT" || { echo "NO WORKTREE at $WT"; echo "ALL_DONE"; exit 1; }

echo "=== cargo test -p bt-platform ==="
nice -n 10 "$CARGO" test -p bt-platform -j 4 2>&1 | tail -60
echo "rc=$?"

echo "=== cargo check -p bt-app --all-targets ==="
nice -n 10 "$CARGO" check -p bt-app --all-targets -j 4 2>&1 | tail -25
echo "rc=$?"

echo "=== cargo build -p bt-app ==="
nice -n 10 "$CARGO" build -p bt-app -j 4 2>&1 | tail -15
echo "rc=$?"

echo "=== build the probe binary ==="
nice -n 10 "$CARGO" test -p bt-platform --test macos_services --no-run -j 4 2>&1 | tail -15
echo "rc=$?"
BIN=$(ls -t "$CARGO_TARGET_DIR"/debug/deps/macos_services-* 2>/dev/null | grep -v '\.d$' | head -1)
if [ ! -x "${BIN:-/nonexistent}" ]; then
  echo "NO PROBE BINARY under $CARGO_TARGET_DIR/debug/deps"
  echo "ALL_DONE"
  exit 1
fi
echo "probe binary: $BIN ($(stat -f %z "$BIN") bytes)"

echo "=== bundles ==="
rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$SENDER/Contents/MacOS" "$OUT/fixtures"
cp "$BIN" "$APP/Contents/MacOS/macos_services"
cp "$BIN" "$SENDER/Contents/MacOS/macos_services"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>macos_services</string>
  <key>CFBundleIdentifier</key><string>io.github.lulu-loopp.folio.m4-9</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>FolioServicesM49</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSServices</key>
  <array>
    <dict>
      <key>NSMenuItem</key>
      <dict>
        <key>default</key><string>${ROW}</string>
      </dict>
      <key>NSMessage</key><string>openInFolio</string>
      <key>NSPortName</key><string>FolioServicesM49</string>
      <key>NSSendTypes</key>
      <array>
        <string>public.file-url</string>
      </array>
    </dict>
  </array>
</dict>
</plist>
PLIST

cat > "$SENDER/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>macos_services</string>
  <key>CFBundleIdentifier</key><string>io.github.lulu-loopp.folio.m4-9-sender</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>ServiceSender</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

/usr/bin/plutil -lint "$APP/Contents/Info.plist"; echo "rc=$?"
/usr/bin/plutil -lint "$SENDER/Contents/Info.plist"; echo "rc=$?"
/usr/bin/codesign --force --sign - --timestamp=none "$APP" 2>&1; echo "rc=$?"
/usr/bin/codesign --force --sign - --timestamp=none "$SENDER" 2>&1; echo "rc=$?"

echo "=== register ==="
"$LSREGISTER" -f "$APP"; echo "rc=$?"
"$LSREGISTER" -f "$SENDER"; echo "rc=$?"
"$PBS" -update; echo "rc=$?"
echo "--- pbs -dump_pboard, the rows naming this bundle ---"
"$PBS" -dump_pboard 2>&1 | grep -A 4 -B 4 "m4-9" | head -60
echo "--- lsregister -dump, the bundle's own row ---"
"$LSREGISTER" -dump 2>&1 | grep -c "io.github.lulu-loopp.folio.m4-9"

echo "=== the cold delivery ==="
# The one fixture the application cannot make for itself: it does not exist yet.
mkdir -p "$OUT/fixtures/cold folder"
"$SENDER/Contents/MacOS/macos_services" --send-service "$ROW" "$OUT/fixtures/cold folder" 2>&1
echo "rc=$?"

echo "=== waiting for the report ==="
REPORT="$OUT/m4-9-report.log"
i=0
while [ $i -lt 180 ]; do
  if [ -f "$REPORT" ] && grep -q ALL_DONE "$REPORT"; then break; fi
  sleep 1
  i=$((i + 1))
done
echo "waited ${i}s"
echo "--- report ---"
cat "$REPORT" 2>&1
echo "--- ps, this binary only (read, never signalled) ---"
/bin/ps -axo pid,comm= | grep macos_services | grep -v grep

echo "=== unregister and remove ==="
"$LSREGISTER" -u "$APP"; echo "rc=$?"
"$LSREGISTER" -u "$SENDER"; echo "rc=$?"
"$PBS" -update; echo "rc=$?"
echo "pbs still names this bundle: $("$PBS" -dump_pboard 2>&1 | grep -c 'm4-9')"
echo "lsregister still names this bundle: $("$LSREGISTER" -dump 2>&1 | grep -c 'io.github.lulu-loopp.folio.m4-9')"
rm -rf "$APP" "$SENDER"
for id in io.github.lulu-loopp.folio.m4-9 io.github.lulu-loopp.folio.m4-9-sender; do
  rm -rf "$HOME/Library/WebKit/$id" "$HOME/Library/Caches/$id" "$HOME/Library/Saved Application State/$id.savedState"
done
echo "df: $(df -h "$HOME" | tail -1)"
echo "ALL_DONE"
