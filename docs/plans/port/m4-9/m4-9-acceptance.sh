#!/bin/sh
# M4-9 — §M4 acceptance ⑤ on the real binary.
#
# "Right-click a folder in Finder: Services ▸ Open in Folio opens a tab in that
# folder, including for a folder whose name contains a space and a CJK
# character." Finder itself cannot be driven by an agent, so the gesture is
# `NSPerformService` from a second bundle, which is the same delivery Finder
# makes (X-4). What is asserted is the *tab*: the shell the tab started stands
# in the folder, read off its own working directory, and the pty dump carries
# the folder's name.
#
# The application is the debug `folio` built by `m4-9-door.sh`, inside a
# throwaway bundle with an identifier of its own and an **isolated HOME** —
# through a `CFBundleExecutable` wrapper script, because `LSEnvironment` cannot
# set `HOME` (DESIGN §13.31 ⑧(d)). Nothing outside ~/folio-port is written.
set -u

WT="$HOME/folio-port/wt/m4-9"
OUT="$WT/out-acc"
APP="$OUT/FolioM49.app"
SENDER="$OUT/AcceptanceSender.app"
ISO="$OUT/home"
ROW="Open in Folio M4-9 App"
FOLDER="$OUT/fixtures/中文 folder"
PLAIN="$OUT/fixtures/plain"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
PBS=/System/Library/CoreServices/pbs
TARGET="$HOME/folio-port/target-m4-9"

FOLIO="$TARGET/debug/folio"
PROBE=$(ls -t "$TARGET"/debug/deps/macos_services-* 2>/dev/null | grep -v '\.d$' | head -1)
if [ ! -x "${FOLIO:-/nonexistent}" ] || [ ! -x "${PROBE:-/nonexistent}" ]; then
  echo "MISSING folio=$FOLIO probe=$PROBE"
  echo "ALL_DONE"
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$SENDER/Contents/MacOS" "$ISO" "$FOLDER" "$PLAIN" "$OUT/dump"
cp "$FOLIO" "$APP/Contents/MacOS/folio"
cp "$PROBE" "$SENDER/Contents/MacOS/macos_services"

cat > "$APP/Contents/MacOS/folio-launch" <<LAUNCH
#!/bin/sh
export HOME="$ISO"
export BT_PTY_DUMP="$OUT/dump/pty.dump"
exec "\$(dirname "\$0")/folio" "\$@"
LAUNCH
chmod +x "$APP/Contents/MacOS/folio-launch"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>folio-launch</string>
  <key>CFBundleIdentifier</key><string>io.github.lulu-loopp.folio.m4-9-acc</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>FolioM49</string>
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
      <key>NSPortName</key><string>FolioM49</string>
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
  <key>CFBundleIdentifier</key><string>io.github.lulu-loopp.folio.m4-9-acc-sender</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>AcceptanceSender</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

/usr/bin/plutil -lint "$APP/Contents/Info.plist"; echo "rc=$?"
/usr/bin/codesign --force --sign - --timestamp=none "$APP" 2>&1; echo "rc=$?"
/usr/bin/codesign --force --sign - --timestamp=none "$SENDER" 2>&1; echo "rc=$?"
"$LSREGISTER" -f "$APP"; echo "rc=$?"
"$LSREGISTER" -f "$SENDER"; echo "rc=$?"
"$PBS" -update; echo "rc=$?"
echo "pbs names this row: $("$PBS" -dump_pboard 2>&1 | grep -c 'm4-9-acc')"

echo "=== the cold delivery: a folder with a space and a CJK character ==="
"$SENDER/Contents/MacOS/macos_services" --send-service "$ROW" "$FOLDER" 2>&1
echo "rc=$?"

i=0
PID=""
while [ $i -lt 60 ]; do
  PID=$(/bin/ps -axo pid=,command= | grep "$APP/Contents/MacOS/folio" | grep -v grep | awk '{print $1}' | head -1)
  [ -n "$PID" ] && break
  sleep 1
  i=$((i + 1))
done
echo "folio pid=${PID:-none} after ${i}s"
if [ -z "$PID" ]; then
  echo "THE APPLICATION NEVER STARTED"
else
  sleep 20
  echo "=== the warm delivery: a plain folder ==="
  "$SENDER/Contents/MacOS/macos_services" --send-service "$ROW" "$PLAIN" 2>&1
  echo "rc=$?"
  sleep 15

  echo "--- the shells this application started, and where each one stands ---"
  for child in $(/bin/ps -axo pid=,ppid= | awk -v p="$PID" '$2==p {print $1}'); do
    echo "child $child: $(/usr/sbin/lsof -a -p "$child" -d cwd -Fn 2>/dev/null | grep '^n' | cut -c2-)"
  done
  echo "--- grandchildren (the shell under a pane) ---"
  for child in $(/bin/ps -axo pid=,ppid= | awk -v p="$PID" '$2==p {print $1}'); do
    for grand in $(/bin/ps -axo pid=,ppid= | awk -v p="$child" '$2==p {print $1}'); do
      echo "grandchild $grand: $(/usr/sbin/lsof -a -p "$grand" -d cwd -Fn 2>/dev/null | grep '^n' | cut -c2-)"
    done
  done

  echo "--- the pty dump, the folder names in it ---"
  for dump in "$OUT"/dump/*; do
    [ -f "$dump" ] || continue
    echo "dump $dump ($(stat -f %z "$dump") bytes)"
    /usr/bin/strings "$dump" | grep -n "中文 folder\|/plain" | head -20
  done

  echo "--- the isolated data directory ---"
  ls -la "$ISO/Library/Application Support/Folio" 2>&1 | head -20
  echo "--- session.json, the places it names ---"
  /usr/bin/grep -o '"cwd":"[^"]*"' "$ISO/Library/Application Support/Folio/session.json" 2>/dev/null | head -20

  echo "=== ending the application by the pid this script started ==="
  kill "$PID" 2>&1
  echo "rc=$?"
  sleep 5
  echo "still there: $(/bin/ps -axo pid= | awk -v p="$PID" '$1==p {print $1}')"
fi

echo "=== unregister and remove ==="
"$LSREGISTER" -u "$APP"; echo "rc=$?"
"$LSREGISTER" -u "$SENDER"; echo "rc=$?"
"$PBS" -update; echo "rc=$?"
echo "pbs still names this row: $("$PBS" -dump_pboard 2>&1 | grep -c 'm4-9-acc')"
rm -rf "$APP" "$SENDER"
# Anything a bundle-identified process of this ticket's left in the account's
# own library, which is the one place outside ~/folio-port a bundle identifier
# reaches. The application's own HOME was isolated, so these are only ever the
# empty shells a launch leaves behind.
for id in io.github.lulu-loopp.folio.m4-9-acc io.github.lulu-loopp.folio.m4-9-acc-sender; do
  rm -rf "$HOME/Library/WebKit/$id" "$HOME/Library/Caches/$id" "$HOME/Library/Saved Application State/$id.savedState"
done
echo "df: $(df -h "$HOME" | tail -1)"
echo "ALL_DONE"
