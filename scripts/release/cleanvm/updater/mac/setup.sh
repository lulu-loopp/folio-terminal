#!/bin/bash
# The macOS updater rehearsal (clean-vm.md 4.4, macOS), setup. Runs ON THE MAC, in the
# account the rehearsal runs in, from ssh.
#
#   setup.sh <A.dmg> <B.dmg> <folder>/in/RELEASE-STATEMENT.txt <folder>/in/RELEASE-STATEMENT.txt.sig
#   setup.sh --feed-dry-run <B.dmg> <statement> <signature>    the feed step alone (the tests')
#
# The statement and its signature are taken only from <folder>/in/, under the names the
# updater fetches (scp them there; clean-vm.md 4.4), so that every copy of them on this Mac
# is one teardown.sh deletes.
#
# A is the candidate installed (offers on), B the successor served from a local feed; both are
# signed, notarised and named Folio-<version>-macos-arm64.dmg, and B's version is higher. B's
# statement is the rehearsal's one statement of both halves (0.4.10 plan row 104): composed on
# the Windows host by the rehearsal's make-feed.ps1 through release-statement.ps1 with B's
# Windows archive and this dmg, signed there by the key holder with the live key and checked
# there by `check-release-statement verify`. It is never uploaded; teardown.sh deletes this
# folder's copy after the last row.
#
# What it does, once, before the first row:
#   * refuses (exit 3) while any .../Contents/MacOS/folio runs outside the rehearsal folder;
#   * creates <folder>/{app,feed,evidence,data-before,a-pristine,mnt} and writes <folder>/versions
#     (A's and B's version, read from the image names; row.sh reads it);
#   * mounts A's dmg read-only (mount point read from `hdiutil attach -plist`, never from
#     the localised text output), ditto's Folio.app to a-pristine/ and app/, detaches;
#   * records A's signature, assessment and version (must read A's version);
#   * holds B's dmg to the statement - its macos-arm64 line names the image, its SHA-256 and
#     its size, and the statement's version is B's, a rehearsal version (patch 1000 or more,
#     as make-feed.ps1 holds it) - and the signature to 64 bytes (the
#     signature itself is the updater's to verify; the host checked it before it came here);
#   * copies the dmg, the statement and the signature into feed/ and nothing else - the Windows
#     archive the statement also lists is not this feed's - and writes feed/releases.json in the
#     GitHub releases shape, under the statement's own tag, listing those three;
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
# The folder is written into releases.json's URLs as it is: no character JSON would escape.
case "$R" in *'"'* | *"\\"*) echo "FOLIO_REHEARSAL ($R) holds a quote or a backslash; stopped"; exit 2 ;; esac
U="$R"
DRY_FEED=0
if [ "${1:-}" = "--feed-dry-run" ]; then
  [ $# -eq 4 ] || { echo "usage: setup.sh --feed-dry-run <B.dmg> <statement> <signature>"; exit 2; }
  DRY_FEED=1
  A_DMG=""
  B_DMG="$2"
  STATEMENT="$3"
  SIGNATURE="$4"
else
  [ $# -eq 4 ] || { echo "usage: setup.sh <A.dmg> <B.dmg> <B's RELEASE-STATEMENT.txt> <its RELEASE-STATEMENT.txt.sig>"; exit 2; }
  A_DMG="$1"
  B_DMG="$2"
  STATEMENT="$3"
  SIGNATURE="$4"
fi
version_of() { basename "$1" | sed -nE 's/^Folio-([0-9]+\.[0-9]+\.[0-9]+)-macos-arm64\.dmg$/\1/p'; }
B_VERSION=$(version_of "$B_DMG")
A_VERSION=$B_VERSION
[ "$DRY_FEED" = 1 ] || A_VERSION=$(version_of "$A_DMG")
[ -n "$A_VERSION" ] && [ -n "$B_VERSION" ] || { echo "the images must be named Folio-<version>-macos-arm64.dmg"; exit 2; }
B_NAME=$(basename "$B_DMG")
# The names the updater fetches them by (bt_winres::release_statement), whatever the inputs' names.
STATEMENT_NAME="RELEASE-STATEMENT.txt"
SIGNATURE_NAME="RELEASE-STATEMENT.txt.sig"
# A rehearsal successor's patch number is this or more (make-feed.ps1's rule, held here too):
# no release of Folio carries one, so its statement is never taken for a release's.
REHEARSAL_PATCH_FLOOR=1000
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
size_of() { wc -c < "$1" | tr -d ' '; }

# ---- serve B: the dmg held to the statement, then the feed of exactly three files ----
serve_b() {
  for f in "$B_DMG" "$STATEMENT" "$SIGNATURE"; do
    [ -f "$f" ] || die "missing input $f"
  done
  [ "$STATEMENT" = "$U/in/$STATEMENT_NAME" ] && [ "$SIGNATURE" = "$U/in/$SIGNATURE_NAME" ] \
    || die "the statement and its signature are taken from $U/in/$STATEMENT_NAME and $U/in/$SIGNATURE_NAME only, where teardown.sh deletes them"
  [ "$(head -n 1 "$STATEMENT")" = "folio-release-statement 1" ] || die "$STATEMENT is not a release statement (v1)"
  ST_VERSION=$(sed -n 's/^version //p' "$STATEMENT")
  ST_TAG=$(sed -n 's/^tag //p' "$STATEMENT")
  [ "$ST_VERSION" = "$B_VERSION" ] || die "the statement is of version '$ST_VERSION', and B is $B_VERSION"
  case "$ST_TAG" in "v$B_VERSION" | "v$B_VERSION-preview") ;; *) die "the statement's tag '$ST_TAG' does not name $B_VERSION" ;; esac
  [ "${B_VERSION##*.}" -ge "$REHEARSAL_PATCH_FLOOR" ] \
    || die "B is $B_VERSION, which a release may carry; a rehearsal successor's patch number is $REHEARSAL_PATCH_FLOOR or more"
  # An asset line is the only line of four words: <sha256> <size> <platform> <name>.
  ST_LINES=$(awk 'NF == 4 && $3 == "macos-arm64"' "$STATEMENT")
  [ "$(printf '%s\n' "$ST_LINES" | grep -c .)" = 1 ] || die "the statement has no single macos-arm64 line"
  B_SHA=$(shasum -a 256 "$B_DMG" | awk '{print $1}')
  B_SIZE=$(size_of "$B_DMG")
  say "B dmg sha256 $B_SHA, $B_SIZE bytes; the statement's line: $ST_LINES"
  [ "$ST_LINES" = "$B_SHA $B_SIZE macos-arm64 $B_NAME" ] || die "B's image is not the one the statement lists"
  [ "$(size_of "$SIGNATURE")" = 64 ] || die "$SIGNATURE is $(size_of "$SIGNATURE") bytes, not an Ed25519 signature's 64"
  say "B's image is the statement's macos-arm64 asset, tag $ST_TAG"

  mkdir -p "$U/feed"
  for name in "$B_NAME" "$STATEMENT_NAME" "$SIGNATURE_NAME"; do rm_u "$U/feed/$name"; done
  rm_u "$U/feed/releases.json"
  cp "$B_DMG" "$U/feed/$B_NAME" && cp "$STATEMENT" "$U/feed/$STATEMENT_NAME" && cp "$SIGNATURE" "$U/feed/$SIGNATURE_NAME" \
    || die "copy of B into feed/ failed"
  asset_json() { printf '{"name": "%s", "browser_download_url": "%s%s", "size": %s}' "$1" "$FEED_PREFIX" "$1" "$(size_of "$U/feed/$1")"; }
  {
    printf '[\n  {\n'
    printf '    "tag_name": "%s",\n    "name": "Folio %s",\n    "draft": false,\n    "prerelease": false,\n' "$ST_TAG" "$B_VERSION"
    printf '    "assets": [\n      %s,\n      %s,\n      %s\n    ]\n  }\n]\n' \
      "$(asset_json "$B_NAME")" "$(asset_json "$STATEMENT_NAME")" "$(asset_json "$SIGNATURE_NAME")"
  } > "$U/feed/releases.json" || die "releases.json not written"
  say "feed/releases.json:"
  cat "$U/feed/releases.json" >> "$SETUP_LOG"
  say "the feed flag is: --update-feed $FEED_PREFIX"
}

if [ "$DRY_FEED" = 1 ]; then
  serve_b
  say "FEED DRY RUN DONE"
  exit 0
fi

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
for f in "$A_DMG" "$B_DMG" "$STATEMENT" "$SIGNATURE"; do
  [ -f "$f" ] || die "missing input $f"
done
python3 -c 'import plistlib' 2>/dev/null || die "python3 does not run (Command Line Tools)"
say "A dmg sha256 $(shasum -a 256 "$A_DMG" | awk '{print $1}')  $(size_of "$A_DMG") bytes"

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
serve_b

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
