#!/bin/bash
# The macOS updater rehearsal (clean-vm.md 4.4, macOS), teardown. Runs ON THE MAC after the
# last row.
#
#   * refuses (exit 3) while a Folio of anyone's runs outside the rehearsal folder;
#   * ends any process of the rehearsal folder still running (by pid, after checking that its
#     executable is under the folder; recorded in evidence/teardown.txt);
#   * boots out any loaded io.github.lulu-loopp.folio.update-<txn8> job and removes every
#     ~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-<txn8>.plist (exactly that
#     shape: eight lowercase hex digits; nothing else in the folder is touched);
#   * detaches any image still mounted under the folder and reports what remains;
#   * restores ~/Library/Application Support/Folio from data-before/ (the final restore);
#   * removes the update home app/.Folio.app.folio-update; leaves evidence/, feed/, app/Folio.app,
#     a-pristine/ and data-before/ in place (the folder is deleted by hand after the write-up).
set -u

# The rehearsal folder: FOLIO_REHEARSAL, or ~/folio-rehearsal. Everything of the rehearsal but
# the three writes below lives in it.
R="${FOLIO_REHEARSAL:-$HOME/folio-rehearsal}"
case "$R" in /?*) ;; *) echo "FOLIO_REHEARSAL ($R) is not an absolute path; stopped"; exit 2 ;; esac
case "$R" in *"/../"* | *"/.." | */) echo "FOLIO_REHEARSAL ($R) must be a plain folder path; stopped"; exit 2 ;; esac
U="$R"
H="$U/app/.Folio.app.folio-update"
DATA="$HOME/Library/Application Support/Folio"
LA="$HOME/Library/LaunchAgents"
LPFX="io.github.lulu-loopp.folio.update-"
UIDN=$(id -u)
mkdir -p "$U/evidence" || exit 1
TLOG="$U/evidence/teardown.txt"
say() { printf '%s %s\n' "$(date '+%H:%M:%S')" "$*" | tee -a "$TLOG"; }

under_u() {
  case "$1" in *"/../"* | *"/..") return 1 ;; "$U"/?*) return 0 ;; esac
  return 1
}
folio_procs() {
  ps -Ao pid=,comm= | awk '{ p = $1; sub(/^[ ]*[0-9]+[ ]+/, ""); if ($0 ~ /\/Contents\/MacOS\/folio$/) print p " " $0 }'
}
foreign_procs() { folio_procs | awk -v u="$U/" '{ line = $0; sub(/^[0-9]+ /, ""); if (index($0, u) != 1) print line }'; }
ours_now() { folio_procs | awk -v u="$U/" '{ p = $1; sub(/^[0-9]+ /, ""); if (index($0, u) == 1) print p }'; }

say "=== teardown $(date '+%F %T %z')"
F=$(foreign_procs)
if [ -n "$F" ]; then
  say "a Folio of the owner's runs outside $U; stopped (the owner quits it; it is never ended here):"
  printf '%s\n' "$F" | tee -a "$TLOG"
  exit 3
fi

# ---- the rehearsal's own processes ----
for pid in $(ours_now); do
  c=$(ps -o comm= -p "$pid" 2>/dev/null)
  under_u "$c" || continue
  say "ending rehearsal process $pid: $(ps -o args= -p "$pid" 2>/dev/null)"
  kill -KILL "$pid" 2>/dev/null
done
sleep 1
[ -z "$(ours_now)" ] || say "still running: $(ours_now | tr '\n' ' ')"

# ---- the update LaunchAgents ----
for label in $(launchctl print "gui/$UIDN" 2>/dev/null | grep -o "${LPFX}[0-9a-f]\{8\}" | sort -u); do
  launchctl bootout "gui/$UIDN/$label" >> "$TLOG" 2>&1
  say "launchctl bootout gui/$UIDN/$label -> rc=$?"
done
for f in "$LA/$LPFX"*.plist; do
  [ -f "$f" ] || continue
  case "$(basename "$f")" in
    "$LPFX"[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f].plist)
      mkdir -p "$U/evidence/teardown-plists"
      cp "$f" "$U/evidence/teardown-plists/"
      rm -f "$f" && say "removed $f (copy in evidence/teardown-plists/)" ;;
    *) say "left $f alone (not the update door's name shape)" ;;
  esac
done
say "update LaunchAgents now: $(ls "$LA" 2>/dev/null | grep -F "$LPFX" | tr '\n' ' ')"

# ---- mounts ----
mount | sed -n "s|^.* on \($U/.*\) (.*$|\1|p" | while read -r mp; do
  under_u "$mp" || continue
  hdiutil detach "$mp" >> "$TLOG" 2>&1 || hdiutil detach -force "$mp" >> "$TLOG" 2>&1
  say "detached $mp -> rc=$?"
done
LEFT=$(mount | grep -F "$U")
if [ -n "$LEFT" ]; then say "STILL MOUNTED under $U: $LEFT"; else say "unmount check: nothing is mounted under $U"; fi
hdiutil info 2>/dev/null | grep -F "$U" >> "$TLOG" && say "hdiutil info still names an image under $U (see teardown.txt)"

# ---- the data folder ----
if [ -n "$(ours_now)" ]; then
  say "data folder NOT restored: a rehearsal process still runs"
  exit 1
fi
[ "$DATA" = "$HOME/Library/Application Support/Folio" ] || { say "REFUSED: data path $DATA"; exit 9; }
if [ -d "$U/data-before/Folio" ]; then
  rm -rf "$DATA" && ditto "$U/data-before/Folio" "$DATA" && say "data folder restored from data-before/Folio"
  if diff -rq "$U/data-before/Folio" "$DATA" >> "$TLOG" 2>&1; then say "restore check: identical to data-before/Folio"; else say "restore check: DIFFERS (see teardown.txt)"; fi
elif [ -f "$U/data-before/ABSENT" ]; then
  rm -rf "$DATA" && say "data folder removed (there was none before the rehearsal)"
else
  say "no data-before/ backup found; the data folder is left as it is"
fi

# ---- the update home ----
if under_u "$H" && [ -e "$H" ]; then
  chmod -R u+rwX "$H" 2>/dev/null
  rm -rf "$H" && say "removed $H"
fi
say "single-instance folder (Folio's own; left): $(ls -la "$(getconf DARWIN_USER_TEMP_DIR)folio-$UIDN" 2>&1 | tr '\n' ' ')"
say "TEARDOWN DONE (evidence/ kept)"
