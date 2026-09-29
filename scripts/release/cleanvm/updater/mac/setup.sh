#!/bin/bash
# The macOS updater rehearsal (clean-vm.md 4.4, macOS), setup. Runs ON THE MAC, in the
# account the rehearsal runs in, from ssh.
#
#   setup.sh <A.dmg> <B.dmg> <B's SHA256SUMS-macos.txt>
#
# A is the candidate installed (offers on), B the successor served from a local feed; both are
# signed, notarised and named Folio-<version>-macos-arm64.dmg, and B's version is higher.
#
# What it does, once, before the first row:
#   * refuses (exit 3) while any .../Contents/MacOS/folio runs outside the rehearsal folder;
#   * creates <folder>/{app,feed,evidence,data-before,a-pristine,mnt} and writes <folder>/versions
#     (A's and B's version, read from the image names; row.sh reads it);
#   * mounts A's dmg read-only (mount point read from `hdiutil attach -plist`, never from
#     the localised text output), ditto's Folio.app to a-pristine/ and app/, detaches;
#   * records A's signature, assessment and version (must read A's version);
#   * copies B's dmg and the sums file into feed/, checks B's digest against the sums file,
#     and writes feed/releases.json in the GitHub releases shape;
#   * backs the data folder ~/Library/Application Support/Folio up with ditto into
#     data-before/Folio (data-before/ABSENT when there is none) - once: a second run never
#     overwrites the first backup;
#   * records whether this ssh session may post key events (Accessibility), without asking.
#
# It starts no application and writes nothing outside the rehearsal folder; the data folder is
# only read here (osascript's no-op asks System Events one read-only question).
#
# The three writes the rows make outside the folder (clean-vm.md 4.4, macOS, "option B"): the data
# folder (restored after every row), the update LaunchAgent plist (removed), and Folio's own
# single-instance folder $TMPDIR/folio-<uid>/.
set -u

say() { printf '%s %s\n' "$(date '+%H:%M:%S')" "$*" | tee -a "$SETUP_LOG"; }
die() { say "SETUP STOPPED: $1"; exit "${2:-1}"; }

# The rehearsal folder: FOLIO_REHEARSAL, or ~/folio-rehearsal. Everything of the rehearsal but
# the three writes below lives in it.
R="${FOLIO_REHEARSAL:-$HOME/folio-rehearsal}"
case "$R" in /?*) ;; *) echo "FOLIO_REHEARSAL ($R) is not an absolute path; stopped"; exit 2 ;; esac
case "$R" in *"/../"* | *"/.." | */) echo "FOLIO_REHEARSAL ($R) must be a plain folder path; stopped"; exit 2 ;; esac
U="$R"
[ $# -eq 3 ] || { echo "usage: setup.sh <A.dmg> <B.dmg> <B's SHA256SUMS-macos.txt>"; exit 2; }
A_DMG="$1"
B_DMG="$2"
SUMS="$3"
version_of() { basename "$1" | sed -nE 's/^Folio-([0-9]+\.[0-9]+\.[0-9]+)-macos-arm64\.dmg$/\1/p'; }
A_VERSION=$(version_of "$A_DMG")
B_VERSION=$(version_of "$B_DMG")
[ -n "$A_VERSION" ] && [ -n "$B_VERSION" ] || { echo "the images must be named Folio-<version>-macos-arm64.dmg"; exit 2; }
B_NAME=$(basename "$B_DMG")
SUMS_NAME=$(basename "$SUMS")
DATA="$HOME/Library/Application Support/Folio"
LA="$HOME/Library/LaunchAgents"
LPFX="io.github.lulu-loopp.folio.update-"
FEED_PREFIX="file://$U/feed/"

mkdir -p "$U/evidence" || { echo "cannot create $U/evidence"; exit 1; }
SETUP_LOG="$U/evidence/setup.txt"
: >> "$SETUP_LOG"

# A path this script may delete or overwrite: strictly under the rehearsal folder.
under_u() {
  case "$1" in *"/../"* | *"/..") return 1 ;; "$U"/?*) return 0 ;; esac
  return 1
}
rm_u() {
  under_u "$1" || die "refused to remove a path outside $U: $1" 9
  [ -e "$1" ] || return 0
  chmod -R u+rwX "$1" 2>/dev/null
  rm -rf "$1"
}
# Every running .../Contents/MacOS/folio, as "pid path"; `comm` is the executable's full path.
folio_procs() {
  ps -Ao pid=,comm= | awk '{ p = $1; sub(/^[ ]*[0-9]+[ ]+/, ""); if ($0 ~ /\/Contents\/MacOS\/folio$/) print p " " $0 }'
}
foreign_procs() { folio_procs | awk -v u="$U/" '{ line = $0; sub(/^[0-9]+ /, ""); if (index($0, u) != 1) print line }'; }
ours_procs() { folio_procs | awk -v u="$U/" '{ line = $0; sub(/^[0-9]+ /, ""); if (index($0, u) == 1) print line }'; }

say "=== updater rehearsal setup, $(date '+%F %T %z'), uid $(id -u), macOS $(sw_vers -productVersion), folder $U, A $A_VERSION, B $B_VERSION"

# ---- the precondition: no Folio of anyone's but the rehearsal's ----
FOREIGN=$(foreign_procs)
if [ -n "$FOREIGN" ]; then
  say "a Folio of the owner's runs outside $U (the owner quits it; this script never ends it):"
  printf '%s\n' "$FOREIGN" | tee -a "$SETUP_LOG"
  exit 3
fi
OURS=$(ours_procs)
if [ -n "$OURS" ]; then
  say "a rehearsal process still runs (run row.sh's cleanup or teardown.sh first):"
  printf '%s\n' "$OURS" | tee -a "$SETUP_LOG"
  exit 3
fi
say "no Folio process runs"

# ---- inputs ----
for f in "$A_DMG" "$B_DMG" "$SUMS"; do
  [ -f "$f" ] || die "missing input $f"
done
python3 -c 'import plistlib, json' 2>/dev/null || die "python3 does not run (Command Line Tools)"
say "A dmg sha256 $(shasum -a 256 "$A_DMG" | awk '{print $1}')  $(stat -f %z "$A_DMG") bytes"
B_SHA=$(shasum -a 256 "$B_DMG" | awk '{print $1}')
SUMS_LINE=$(grep -F "$B_NAME" "$SUMS" | head -1)
say "B dmg sha256 $B_SHA; sums file line: $SUMS_LINE"
case "$SUMS_LINE" in "$B_SHA"*) say "B's digest matches its sums file" ;; *) die "B's digest is not the one $SUMS_NAME names" ;; esac

mkdir -p "$U/app" "$U/feed" "$U/data-before" "$U/a-pristine" "$U/mnt"
printf 'A %s\nB %s\n' "$A_VERSION" "$B_VERSION" > "$U/versions"

# ---- place A ----
ATTACH_PLIST="$U/evidence/setup-attach.plist"
if ! hdiutil attach -nobrowse -readonly -noautoopen -mountrandom "$U/mnt" -plist "$A_DMG" > "$ATTACH_PLIST" 2>> "$SETUP_LOG"; then
  die "hdiutil attach failed for $A_DMG"
fi
MP=$(python3 -c '
import plistlib, sys
with open(sys.argv[1], "rb") as f:
    d = plistlib.load(f)
points = [e["mount-point"] for e in d.get("system-entities", []) if "mount-point" in e]
print("\n".join(points))
' "$ATTACH_PLIST")
N_MP=$(printf '%s\n' "$MP" | grep -c .)
[ "$N_MP" = 1 ] || die "expected one mount point in the attach plist, found $N_MP: $MP"
under_u "$MP" || die "the mount point $MP is not under $U"
say "A mounted at $MP"
if [ ! -d "$MP/Folio.app" ]; then
  hdiutil detach "$MP" >> "$SETUP_LOG" 2>&1
  die "no Folio.app on A's image"
fi
rm_u "$U/a-pristine/Folio.app"
ditto "$MP/Folio.app" "$U/a-pristine/Folio.app" || { hdiutil detach "$MP" >> "$SETUP_LOG" 2>&1; die "ditto from the image failed"; }
if ! hdiutil detach "$MP" >> "$SETUP_LOG" 2>&1; then
  say "plain detach refused; detaching $MP with -force (our own mount under $U)"
  hdiutil detach -force "$MP" >> "$SETUP_LOG" 2>&1 || die "could not detach $MP"
fi
say "A detached"

rm_u "$U/app/.Folio.app.folio-update"
rm_u "$U/app/Folio.app"
ditto "$U/a-pristine/Folio.app" "$U/app/Folio.app" || die "ditto to app/ failed"
V=$(plutil -extract CFBundleShortVersionString raw -o - "$U/app/Folio.app/Contents/Info.plist" 2>/dev/null)
say "app/Folio.app version: $V"
[ "$V" = "$A_VERSION" ] || die "A reads $V, not $A_VERSION"
{
  echo "--- codesign -dv --verbose=4"
  codesign -dv --verbose=4 "$U/app/Folio.app" 2>&1 | grep -E "Identifier=|Authority=|TeamIdentifier|Timestamp|Notarization|CDHash="
  echo "--- codesign --verify --deep --strict"
  codesign --verify --deep --strict --verbose=2 "$U/app/Folio.app" 2>&1
  echo "--- spctl -a -vvv"
  spctl -a -vvv "$U/app/Folio.app" 2>&1
  echo "--- xattr"
  xattr -l "$U/app/Folio.app" 2>&1
  echo "--- ls -ld app"
  ls -ld "$U/app" "$U/app/Folio.app"
} >> "$SETUP_LOG"
say "signature and assessment recorded above (expected: Developer ID, accepted, source=Notarized Developer ID)"

# ---- serve B ----
cp "$B_DMG" "$U/feed/$B_NAME" && cp "$SUMS" "$U/feed/$SUMS_NAME" || die "copy of B into feed/ failed"
python3 -c '
import json, os, sys
feed, prefix, b, sums, version = sys.argv[1:6]
def asset(name):
    return {"name": name,
            "browser_download_url": prefix + name,
            "size": os.stat(os.path.join(feed, name)).st_size}
releases = [{"tag_name": "v" + version, "name": "Folio " + version, "draft": False,
             "prerelease": False, "assets": [asset(b), asset(sums)]}]
with open(os.path.join(feed, "releases.json"), "w", newline="\n") as f:
    json.dump(releases, f, indent=2)
    f.write("\n")
' "$U/feed" "$FEED_PREFIX" "$B_NAME" "$SUMS_NAME" "$B_VERSION" || die "releases.json not written"
say "feed/releases.json:"
cat "$U/feed/releases.json" >> "$SETUP_LOG"
say "the feed flag is: --update-feed $FEED_PREFIX"

# ---- the data folder, once ----
if [ -d "$U/data-before/Folio" ] || [ -f "$U/data-before/ABSENT" ]; then
  say "data-before/ was taken earlier ($(cat "$U/data-before/TAKEN" 2>/dev/null)); not overwritten"
elif [ -d "$DATA" ]; then
  ditto "$DATA" "$U/data-before/Folio" || die "backup of the data folder failed"
  date '+%F %T %z' > "$U/data-before/TAKEN"
  say "data folder backed up to data-before/Folio ($(du -sk "$U/data-before/Folio" | awk '{print $1}') KB)"
  ls -la "$DATA" >> "$SETUP_LOG"
else
  date '+%F %T %z' > "$U/data-before/ABSENT"
  date '+%F %T %z' > "$U/data-before/TAKEN"
  say "no data folder at $DATA; data-before/ABSENT written (restore = remove)"
fi

# ---- what is outside, before the first row ----
say "update LaunchAgents present now: $(ls "$LA" 2>/dev/null | grep -F "$LPFX" | tr '\n' ' ')"
say "single-instance folder: $(ls -ld "$(getconf DARWIN_USER_TEMP_DIR)folio-$(id -u)" 2>&1)"

# ---- key events: record, never ask ----
AX=$(perl -e 'alarm shift; exec @ARGV' 25 osascript \
  -e 'tell application "System Events" to get UI elements enabled' \
  -e 'tell application "System Events" to count (UI elements of process "Finder")' 2>&1)
AX_RC=$?
if [ "$AX_RC" = 0 ]; then
  say "key events: Accessibility granted to this ssh session (no-op answered: $AX)"
else
  say "key events: NOT available (rc $AX_RC: $AX). Every row stops before starting Folio until the owner grants it or presses through Screen Sharing."
fi

say "SETUP DONE"
