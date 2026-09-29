#!/bin/bash
# The macOS updater rehearsal (clean-vm.md 4.4, macOS): one row per invocation. Runs ON THE MAC,
# after setup.sh, one row at a time.
#
#   row.sh <row> [--cut all|named] [--no-clean]
#
# rows: happy M1 M2 M3 M4 M5 M6 M7 M8 rollback M9cut stuck-a stuck-b
#       M11-rolledback M11-abandoned M11-committed R-W15 R-D5 D14
#
# Each run: precondition (no Folio outside the folder; exit 3) -> Accessibility no-op (exit 4 when
# refused; nothing is started then) -> app/ re-placed from a-pristine (a fresh ditto of A's image
# copy) and the data folder restored from data-before/ -> A started with
#   open -n -a <folder>/app/Folio.app --args --update-feed file://<folder>/feed/
# -> the card driven with key events (osascript, only to a Folio of this folder that is verified
# frontmost first) -> a watcher polls H/journal.json (the phase by the same text the Windows
# watcher matches: body.phase.phase) every 2 ms and, at the row's condition, stops (SIGSTOP)
# every process of this folder; the cut is then SIGKILL on them (--cut all, default: the
# power-cut stand-in) or only on the roles the row names (--cut named; the others get SIGCONT)
# -> the row's next actors (a plain `open -n -a`, or the login stand-in `launchctl bootstrap
# gui/<uid> <plist>`) -> evidence in evidence/<row>/ -> cleanup (only this folder's processes,
# the plist it found, app/ and the data folder reset).
#
# **kill -9 after SIGSTOP is weaker evidence than a power cut**: the page cache survives, so no
# row here can show a missing fsync/F_FULLFSYNC or a torn rename.
#
# Paths: the install is app/Folio.app, its home H = app/.Folio.app.folio-update, the journal is
# H/journal.json (update_txn.rs Home::journal; not H/<txn>/journal.json), a transaction's files
# are H/<txn>/{stage,rescue,owner,health-<nonce>,mnt}.
#
# Writes outside the folder ("option B"): the data folder (~/Library/Application Support/Folio,
# restored from data-before/), the update LaunchAgent plist (removed), $TMPDIR/folio-<uid>/
# (Folio's own single-instance socket). Nothing else.
set -u

ROW="${1:-}"
[ $# -gt 0 ] && shift
CUT=all
CLEAN=1
while [ $# -gt 0 ]; do
  case "$1" in
    --cut) CUT="${2:-}"; shift 2 ;;
    --no-clean) CLEAN=0; shift ;;
    *) echo "unknown argument: $1"; exit 2 ;;
  esac
done
case "$CUT" in all | named) ;; *) echo "--cut is all or named"; exit 2 ;; esac

# The rehearsal folder: FOLIO_REHEARSAL, or ~/folio-rehearsal. Everything of the rehearsal but
# the three writes below lives in it.
R="${FOLIO_REHEARSAL:-$HOME/folio-rehearsal}"
case "$R" in /?*) ;; *) echo "FOLIO_REHEARSAL ($R) is not an absolute path; stopped"; exit 2 ;; esac
case "$R" in *"/../"* | *"/.." | */) echo "FOLIO_REHEARSAL ($R) must be a plain folder path; stopped"; exit 2 ;; esac
U="$R"
APP="$U/app/Folio.app"
EXE="$APP/Contents/MacOS/folio"
INFO="$APP/Contents/Info.plist"
H="$U/app/.Folio.app.folio-update"
J="$H/journal.json"
PRISTINE="$U/a-pristine/Folio.app"
DATA="$HOME/Library/Application Support/Folio"
DIAG="$DATA/diagnostics.log"
LA="$HOME/Library/LaunchAgents"
LPFX="io.github.lulu-loopp.folio.update-"
UIDN=$(id -u)
FEED_URL="file://$U/feed/"
[ -f "$U/versions" ] || { echo "no $U/versions: run setup.sh first"; exit 1; }
OLD_V=$(awk '$1 == "A" { print $2 }' "$U/versions")
NEW_V=$(awk '$1 == "B" { print $2 }' "$U/versions")

# ------------------------------------------------------------------ the rows (clean-vm.md 4.4, macOS)
# PLAN:  steps before the cut.   COND: the watcher's freeze condition ("" = no cut).
# NAMED: the roles the row cuts (used by --cut named).   WOPTS: watcher extras.
# THEN:  the next actors and what is recorded after the cut (or after the plan).
PLAN=""; COND=""; NAMED=""; WOPTS=""; THEN=""
P_UPD="offer update ready restart"
case "$ROW" in
  happy)          PLAN="$P_UPD";                  THEN="settle shot:trial-window collect:after-road quit plain quit plain" ;;
  M1)             PLAN="offer update";            COND="phase:Allocated"; NAMED="O";   THEN="plain quit plain" ;;
  M2)             PLAN="offer update ready later quit";                               THEN="plain quit plain" ;;
  M3)             PLAN="$P_UPD";                  COND="phase:Handoff";   NAMED="P O"; THEN="plain quit plain" ;;
  M4)             PLAN="$P_UPD";                  COND="phase:Armed";     NAMED="P";   THEN="login quit plain" ;;
  M5)             PLAN="$P_UPD";                  COND="moving-old";      NAMED="P";   THEN="login quit plain" ;;
  M6)             PLAN="$P_UPD";                  COND="moving-new";      NAMED="P";   THEN="plain settle quit plain" ;;
  M7)             PLAN="$P_UPD";                  COND="trial-noreceipt"; NAMED="P N"; THEN="login settle quit plain" ;;
  M8)             PLAN="$P_UPD";                  COND="trial-receipt";   NAMED="P";   THEN="login settle plain" ;;
  rollback)       PLAN="$P_UPD"; WOPTS="--kill-trial";                                THEN="settle" ;;
  M9cut)          PLAN="$P_UPD"; WOPTS="--kill-trial"; COND="phase:RollbackIntent"; NAMED="P"; THEN="login settle" ;;
  stuck-a)        PLAN="$P_UPD"; WOPTS="--kill-trial --chmod-stage-after-kill";      THEN="settle-stuck stage-back quit plain settle" ;;
  stuck-b)        PLAN="$P_UPD"; WOPTS="--kill-trial --chmod-stage-after-kill"; COND="stuck-noretrial"; NAMED="P"; THEN="plain-go retrial settle stage-back" ;;
  M11-rolledback) PLAN="$P_UPD"; WOPTS="--kill-trial"; COND="phase:RolledBack"; NAMED="P"; THEN="plain quit plain" ;;
  M11-abandoned)  PLAN="offer update-cancel";                                         THEN="settle plain quit plain" ;;
  M11-committed)  PLAN="$P_UPD";                  COND="phase:Committed"; NAMED="P";   THEN="plain quit plain" ;;
  R-W15)          PLAN="offer update ready rescue-000 restart o-gone rescue-back";     THEN="settle" ;;
  R-D5)           PLAN="offer update ready rescue-000 exe-000 restart alert";         THEN="exe-back rescue-back" ;;
  D14)            PLAN="$P_UPD"; WOPTS="--lock-home-at-new";                          THEN="settle-door home-back quit plain settle" ;;
  *) echo "unknown row '$ROW'"; sed -n 5,6p "$0"; exit 2 ;;
esac

EV="$U/evidence/$ROW"
if [ -d "$EV" ]; then mv "$EV" "$EV.prev-$(date '+%Y%m%d-%H%M%S')"; fi
mkdir -p "$EV/shots" || { echo "cannot create $EV"; exit 1; }
LOG="$EV/row.log"

ts() { local t; t=$(perl -MTime::HiRes=time -e 'printf "%.3f", time'); printf '%s.%s' "$(date -r "${t%.*}" '+%H:%M:%S')" "${t#*.}"; }
say() { printf '%s %s\n' "$(ts)" "$*" | tee -a "$LOG"; }

# ------------------------------------------------------------------ guards
under_u() {
  case "$1" in *"/../"* | *"/..") return 1 ;; "$U"/?*) return 0 ;; esac
  return 1
}
rm_u() {
  under_u "$1" || { say "REFUSED: removing $1 (outside $U)"; exit 9; }
  [ -e "$1" ] || return 0
  chmod -R u+rwX "$1" 2>/dev/null
  rm -rf "$1"
}
folio_procs() { # every running .../Contents/MacOS/folio as "pid path" (comm = full executable path)
  ps -Ao pid=,comm= | awk '{ p = $1; sub(/^[ ]*[0-9]+[ ]+/, ""); if ($0 ~ /\/Contents\/MacOS\/folio$/) print p " " $0 }'
}
foreign_procs() { folio_procs | awk -v u="$U/" '{ line = $0; sub(/^[0-9]+ /, ""); if (index($0, u) != 1) print line }'; }
ours_now() { folio_procs | awk -v u="$U/" '{ p = $1; sub(/^[0-9]+ /, ""); if (index($0, u) == 1) print p }'; }
args_of() { ps -o args= -p "$1" 2>/dev/null; }
alive() { kill -0 "$1" 2>/dev/null; }
is_ours_pid() {
  local c
  c=$(ps -o comm= -p "$1" 2>/dev/null)
  under_u "$c" || return 1
  case "$c" in */Contents/MacOS/folio) return 0 ;; esac
  return 1
}
record_pid() { printf '%s %s %s %s\n' "$(ts)" "$1" "$2" "$(args_of "$1")" >> "$EV/pids.txt"; }
signal_ours() { # signal_ours <signal> <pid> <why>: 0 only when the signal was delivered
  if is_ours_pid "$2"; then
    record_pid "$2" "signal-$1"
    kill "-$1" "$2" 2>/dev/null && { say "kill -$1 $2 ($3)"; return 0; }
    say "kill -$1 $2 failed ($3)"
    return 1
  fi
  say "not signalled: pid $2 is not a process of $U ($3)"
  return 1
}
precondition() {
  local f
  f=$(foreign_procs)
  if [ -n "$f" ]; then
    say "PRECONDITION: a Folio of the owner's runs outside $U - stop and ask the owner; it is never ended here:"
    printf '%s\n' "$f" | tee -a "$LOG"
    echo "STOPPED precondition" > "$EV/RESULT.txt"
    say "ROW $ROW END (stopped: precondition)"
    exit 3
  fi
}

# ------------------------------------------------------------------ reads
version() { plutil -extract CFBundleShortVersionString raw -o - "$INFO" 2>/dev/null || echo "?"; }
jget() { # jget <expr over d> : prints the value, "absent" without a journal
  python3 -c '
import json, sys
try:
    with open(sys.argv[1], "rb") as f:
        d = json.loads(f.read())
except FileNotFoundError:
    print("absent"); sys.exit(0)
except ValueError:
    print("unreadable"); sys.exit(0)
try:
    print(eval(sys.argv[2], {"d": d}))
except Exception:
    print("")
' "$J" "$1"
}
phase() { jget 'd["body"]["phase"]["phase"]'; }
txn() { jget 'd["txn"]'; }
DIAG_OFF=0
diag_new() {
  [ -f "$DIAG" ] || return 0
  local sz
  sz=$(stat -f %z "$DIAG")
  if [ "$sz" -ge "$DIAG_OFF" ]; then tail -c +"$((DIAG_OFF + 1))" "$DIAG"; else cat "$DIAG"; fi
}
wait_diag() { # wait_diag <fixed text> <seconds>
  local t0=$SECONDS
  while [ $((SECONDS - t0)) -lt "$2" ]; do
    if diag_new | grep -F -q -- "$1"; then say "diagnostics: '$1' seen"; return 0; fi
    sleep 0.5
  done
  say "diagnostics: '$1' NOT seen within $2 s"
  return 1
}
wait_phase() { # wait_phase <regex> <seconds>
  local t0=$SECONDS p
  while [ $((SECONDS - t0)) -lt "$2" ]; do
    p=$(phase)
    if printf '%s' "$p" | grep -E -q "^($1)$"; then say "journal phase $p"; return 0; fi
    sleep 0.2
  done
  say "journal phase /$1/ NOT seen within $2 s (now $(phase))"
  return 1
}
door_pids() { # the road's helper processes: the applier P and the recovery build R
  local pid
  for pid in $(ours_now); do
    case "$(args_of "$pid")" in *--update-apply* | *--update-recover*) echo "$pid" ;; esac
  done
}
app_pids() { # every other process of this folder: O, the trial N, a plain start
  local pid
  for pid in $(ours_now); do
    case "$(args_of "$pid")" in *--update-apply* | *--update-recover*) ;; *) echo "$pid" ;; esac
  done
}
wait_doors_gone() { # wait_doors_gone <max seconds>: no P or R for 3 s running, at least 3 s
  local t0=$SECONDS quiet=0
  while [ $((SECONDS - t0)) -lt "$1" ]; do
    if [ -z "$(door_pids)" ]; then quiet=$((quiet + 1)); else quiet=0; fi
    [ "$quiet" -ge 3 ] && { say "no applier or recovery process for 3 s (journal $(phase))"; return 0; }
    sleep 1
  done
  say "an applier or recovery process still runs after $1 s: $(door_pids | tr '\n' ' ')"
  return 1
}

# ------------------------------------------------------------------ key events (osascript)
osa() { perl -e 'alarm shift; exec @ARGV' 25 osascript "$@" 2>&1; }
ax_check() {
  local out rc
  out=$(osa -e 'tell application "System Events" to get UI elements enabled' \
            -e 'tell application "System Events" to count (UI elements of process "Finder")')
  rc=$?
  printf '%s rc=%s %s\n' "$(ts)" "$rc" "$out" >> "$EV/accessibility.txt"
  [ "$rc" = 0 ]
}
press() { # press <pid> <label> <System Events statements...>: only after <pid> is verified frontmost
  local pid=$1 label=$2 out rc
  shift 2
  if ! is_ours_pid "$pid"; then say "keys $label: pid $pid is not a Folio of $U; nothing sent"; return 1; fi
  local a=(-e 'tell application "System Events"'
           -e "set p to first process whose unix id is $pid"
           -e 'set frontmost of p to true'
           -e 'delay 0.5'
           -e "if (unix id of first process whose frontmost is true) is not $pid then error \"Folio $pid is not frontmost; nothing sent\"")
  local s
  for s in "$@"; do a+=(-e "$s"); done
  a+=(-e 'end tell')
  out=$(osa "${a[@]}")
  rc=$?
  say "keys $label -> pid $pid: rc=$rc $out"
  return "$rc"
}
K_TAB='key code 48'; K_STAB='key code 48 using {shift down}'; K_ENTER='key code 36'
K_ESC='key code 53'; K_QUIT='key code 12 using {command down}'

# ------------------------------------------------------------------ starting Folio
LAST_PID=""
start_folio() { # start_folio <tag> [args...]: open -n -a, then the new process of app/Folio.app
  local tag=$1 before pid t
  shift
  before=" $(ours_now | tr '\n' ' ') "
  if [ $# -gt 0 ]; then open -n -a "$APP" --args "$@"; else open -n -a "$APP"; fi
  say "open -n -a $APP ${*:+--args $*} -> rc=$? ($tag)"
  LAST_PID=""
  for t in $(seq 1 150); do
    for pid in $(ours_now); do
      case "$before" in *" $pid "*) continue ;; esac
      if [ "$(ps -o comm= -p "$pid" 2>/dev/null)" = "$EXE" ]; then
        LAST_PID=$pid; record_pid "$pid" "$tag"; say "$tag: pid $pid ($(args_of "$pid"))"; return 0
      fi
    done
    sleep 0.2
  done
  say "$tag: no new process of $EXE within 30 s"
  return 1
}

# ------------------------------------------------------------------ evidence
shot() { screencapture -x "$EV/shots/$1.png" 2>> "$LOG" && say "screenshot shots/$1.png"; }
collect() { # collect <stage>
  local d="$EV/$1" t f
  mkdir -p "$d"
  date '+%F %T %z' > "$d/when.txt"
  [ -f "$J" ] && cp "$J" "$d/journal.json"
  printf 'phase %s\ninstalled version %s\n' "$(phase)" "$(version)" > "$d/state.txt"
  ls -laR "$U/app" > "$d/ls-app-parent.txt" 2>&1
  ls -laR "$H" > "$d/ls-update-home.txt" 2>&1
  mkdir -p "$d/txn-files"
  for t in "$H"/*/; do
    [ -d "$t" ] || continue
    for f in "$t"owner "$t"health-* "$t"*.json "$t"*.txt; do
      [ -f "$f" ] && cp "$f" "$d/txn-files/$(basename "$t")-$(basename "$f")"
    done
  done
  [ -f "$DIAG" ] && cp "$DIAG" "$d/diagnostics.log"
  diag_new > "$d/diagnostics-this-row.log"
  grep -E 'BT_UPDATE_|update|hand-over|Update' "$d/diagnostics-this-row.log" > "$d/update-lines.txt"
  [ -f "$DATA/update-check.json" ] && cp "$DATA/update-check.json" "$d/"
  ls -la "$DATA" > "$d/data-folder.txt" 2>&1
  ls -la "$LA" 2>&1 | grep -F "$LPFX" > "$d/launchagents.txt"
  for f in "$LA/$LPFX"*.plist; do [ -f "$f" ] && cp "$f" "$d/"; done
  launchctl print "gui/$UIDN" 2>&1 | grep -i folio > "$d/launchctl-folio.txt"
  {
    echo "PID PPID STAT STARTED ARGS"
    for t in $(folio_procs | awk '{print $1}'); do ps -o pid=,ppid=,stat=,lstart=,args= -p "$t"; done
  } > "$d/processes.txt" 2>&1
  mount | grep -F "$U" > "$d/mounts.txt"
  ls -la "$(getconf DARWIN_USER_TEMP_DIR)folio-$UIDN" > "$d/single-instance-dir.txt" 2>&1
  screencapture -x "$d/screen.png" 2>> "$LOG"
  say "evidence: $1/ (phase $(phase), installed $(version), processes: $(ours_now | tr '\n' ' '))"
}

# ------------------------------------------------------------------ the watcher (python3)
cat > "$EV/watch.py" <<'PY'
# The rehearsal watcher: the journal's every change, the folder's processes, the row's cut.
import argparse, json, os, re, signal, subprocess, time, plistlib

ap = argparse.ArgumentParser()
for name in ("--u", "--app", "--home", "--ev", "--tag"):
    ap.add_argument(name, required=True)
ap.add_argument("--cond", default="")
ap.add_argument("--timeout", type=float, default=3600)
ap.add_argument("--o-pid", type=int, default=0)
ap.add_argument("--old", required=True)
ap.add_argument("--new", required=True)
ap.add_argument("--kill-trial", action="store_true")
ap.add_argument("--chmod-stage-after-kill", action="store_true")
ap.add_argument("--lock-home-at-new", action="store_true")
a = ap.parse_args()

U = a.u.rstrip("/") + "/"
J = os.path.join(a.home, "journal.json")
INFO = os.path.join(a.app, "Contents", "Info.plist")
SUFFIX = "/Contents/MacOS/folio"
STOP = os.path.join(a.ev, "watch.stop")
log = open(os.path.join(a.ev, "watch-%s.log" % a.tag), "a", buffering=1)
hist = open(os.path.join(a.ev, "journal-history.txt"), "a", buffering=1)
pidf = open(os.path.join(a.ev, "pids.txt"), "a", buffering=1)

def now():
    t = time.time()
    return time.strftime("%H:%M:%S", time.localtime(t)) + ".%03d" % int((t % 1) * 1000)

def say(s):
    log.write("%s %s\n" % (now(), s))

def ps(fields, pid=None):
    cmd = ["ps", "-o", fields, "-p", str(pid)] if pid else ["ps", "-Ao", fields]
    return subprocess.run(cmd, capture_output=True, text=True).stdout

def ours():
    found = {}
    for line in ps("pid=,comm=").splitlines():
        line = line.strip()
        pid, _, comm = line.partition(" ")
        comm = comm.strip()
        if comm.startswith(U) and comm.endswith(SUFFIX):
            found[int(pid)] = comm
    return found

def still_ours(pid):
    comm = ps("comm=", pid).strip()
    return comm.startswith(U) and comm.endswith(SUFFIX)

def role(pid, args):
    if pid == a.o_pid:
        return "O"
    for word, name in (("--update-apply", "P"), ("--update-recover", "R"), ("--update-trial", "N")):
        if word in args:
            return name
    return "start"

known = {}  # pid -> args, every process of the folder seen in this run
live = set()  # the processes of the folder at the last scan
tried = set()  # trial pids already asked about

def scan():
    global live
    live = set(ours())
    for pid in live:
        if pid not in known:
            args = ps("args=", pid).strip()
            known[pid] = args
            pidf.write("%s %d seen-%s %s\n" % (now(), pid, role(pid, args), args))
            say("process %d %s: %s" % (pid, role(pid, args), args))

def read_journal():
    try:
        with open(J, "rb") as f:
            return f.read().decode("utf-8", "replace")
    except FileNotFoundError:
        return None

def parse(text):
    try:
        d = json.loads(text)
        return d["body"]["phase"]["phase"], d
    except Exception:
        m = re.search(r'"body"\s*:\s*\{\s*"phase"\s*:\s*\{\s*"phase"\s*:\s*"(\w+)"', text)
        return (m.group(1) if m else "?"), None

def version():
    try:
        with open(INFO, "rb") as f:
            return plistlib.load(f).get("CFBundleShortVersionString", "?")
    except Exception:
        # an Info.plist a strict XML reader refuses (U-32 defect 3) is still read by plutil
        out = subprocess.run(["plutil", "-extract", "CFBundleShortVersionString", "raw", "-o", "-", INFO],
                             capture_output=True, text=True)
        return out.stdout.strip() if out.returncode == 0 and out.stdout.strip() else "?"

def receipts(doc):
    if not doc:
        return []
    try:
        return sorted(n for n in os.listdir(os.path.join(a.home, doc["txn"])) if n.startswith("health-"))
    except OSError:
        return []

def freeze():
    frozen = []
    for pid in list(known):
        try:
            os.kill(pid, signal.SIGSTOP)
            frozen.append(pid)
        except ProcessLookupError:
            pass
    scan()
    for pid in list(known):
        if pid not in frozen:
            try:
                os.kill(pid, signal.SIGSTOP)
                frozen.append(pid)
            except ProcessLookupError:
                pass
    return frozen

def listing(root, depth=4):
    out = []
    base = root.count(os.sep)
    for d, dirs, files in os.walk(root):
        if d.count(os.sep) - base >= depth:
            dirs[:] = []
        for n in sorted(dirs + files):
            p = os.path.join(d, n)
            try:
                st = os.lstat(p)
                out.append("%o %10d %s" % (st.st_mode & 0o7777, st.st_size, p))
            except OSError:
                out.append("? %s" % p)
    return out

FORWARD = ["Allocated", "Prepared", "Handoff", "Armed", "Moving", "Trial", "Committed", "Retired"]
BACK = ["RollbackIntent", "RolledBack", "Stuck", "Abandoned"]

def beyond(target, ph, seen):
    if ph is None:
        return seen
    if ph == target:
        return False
    if target in FORWARD and ph in FORWARD:
        return FORWARD.index(ph) > FORWARD.index(target)
    if target in FORWARD[:6] and ph in BACK:
        return True
    return {"RollbackIntent": ["RolledBack", "Stuck", "Retired"], "RolledBack": ["Retired"],
            "Stuck": ["RolledBack", "Committed", "Retired"], "Abandoned": ["Retired"]}.get(target, []).count(ph) > 0

def target_phase(cond):
    return {"moving-old": "Moving", "moving-new": "Moving", "trial-noreceipt": "Trial",
            "trial-receipt": "Trial", "stuck-noretrial": "Stuck"}.get(cond, cond[6:] if cond.startswith("phase:") else "")

def retrial(doc):
    try:
        return bool(doc["body"]["phase"].get("retrial"))
    except Exception:
        return False

def finish(result, text, doc, frozen, note=""):
    if result != "REACHED":
        # a missed row is not cut: the frozen processes go on, so the road runs on as recorded (H2)
        for pid in frozen:
            try:
                os.kill(pid, signal.SIGCONT)
            except ProcessLookupError:
                pass
    with open(os.path.join(a.ev, "frozen.txt"), "w") as f:
        for pid in frozen:
            args = known.get(pid, "")
            f.write("%d %s %s\n" % (pid, role(pid, args), args))
    lines = ["%s %s cond=%s phase=%s installed=%s receipts=%s frozen=[%s] %s" % (
        now(), result, a.cond, parse(text)[0] if text else "<absent>", version(), receipts(doc),
        " ".join("%d:%s" % (p, role(p, known.get(p, ""))) for p in frozen), note)]
    lines += ["", "== journal", text or "<absent>", "", "== owner mark"]
    if doc:
        try:
            with open(os.path.join(a.home, doc["txn"], "owner")) as f:
                lines.append(f.read())
        except OSError as e:
            lines.append("<none: %s>" % e)
    lines += ["", "== processes of the folder"] + ["%d %s %s" % (p, role(p, known.get(p, "")), known.get(p, "")) for p in sorted(known)]
    lines += ["", "== update home"] + listing(a.home)
    with open(os.path.join(a.ev, "at-cut.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")
        f.flush()
        os.fsync(f.fileno())
    with open(os.path.join(a.ev, "cut.txt"), "w") as f:
        f.write(lines[0] + "\n")
    say(lines[0])

say("watch start cond=%s kill_trial=%s chmod_stage=%s lock_home=%s o_pid=%d" % (
    a.cond, a.kill_trial, a.chmod_stage_after_kill, a.lock_home_at_new, a.o_pid))
deadline = time.time() + a.timeout
sig, text, doc, ph, seen = False, None, None, None, False
last_scan = 0.0
trial_killed = home_locked = False
target = target_phase(a.cond)
while time.time() < deadline:
    if os.path.exists(STOP):
        say("stop file seen")
        break
    t = time.time()
    if t - last_scan > 0.1:
        scan()
        last_scan = t
    try:
        st = os.stat(J)
        s = (st.st_ino, st.st_mtime_ns, st.st_size)
    except FileNotFoundError:
        s = None
    if s != sig:
        if s is None:
            text, doc, ph = None, None, None
            if seen:
                hist.write("%s <absent>\n" % now())
                say("journal absent")
            sig = s
        else:
            got = read_journal()
            if got is None:
                continue
            sig, text = s, got
            ph, doc = parse(text)
            seen = True
            hist.write("%s %s %s\n" % (now(), ph, text))
            say("journal phase=%s installed=%s receipts=%s" % (ph, version(), receipts(doc)))
    # the trial ended at once (rollback rows): the journal's Trial pid, or a new --update-trial process
    if a.kill_trial and not trial_killed:
        victim = None
        if ph == "Trial" and doc:
            victim = doc["body"]["phase"]["process"]["pid"]
        else:
            for pid in live:
                if "--update-trial" in known.get(pid, ""):
                    victim = pid
        if victim in tried:
            victim = None
        if victim:
            tried.add(victim)
        if victim and not still_ours(victim):
            say("trial %d is not a live process of the folder; not signalled" % victim)
        elif victim:
            try:
                os.kill(victim, signal.SIGKILL)
                pidf.write("%s %d signal-KILL-trial %s\n" % (now(), victim, known.get(victim, "")))
                say("TRIAL %d ended by the watcher (SIGKILL) at phase %s" % (victim, ph))
            except ProcessLookupError:
                say("trial %d already gone" % victim)
            trial_killed = True
            if a.chmod_stage_after_kill and doc:
                stage = os.path.join(a.home, doc["txn"], "stage")
                mode = os.stat(stage).st_mode & 0o7777
                os.chmod(stage, mode & ~0o222)
                with open(os.path.join(a.ev, "stage-mode.txt"), "w") as f:
                    f.write("%o %s\n" % (mode, stage))
                say("STAGE made read-only: %s (was %o)" % (stage, mode))
    # D-14's twin: the home made unwritable once the exchange is done, before the trial is recorded
    if a.lock_home_at_new and not home_locked and ph == "Moving" and version() == a.new:
        frozen = freeze()
        mode = os.stat(a.home).st_mode & 0o7777
        os.chmod(a.home, mode & ~0o222)
        with open(os.path.join(a.ev, "home-mode.txt"), "w") as f:
            f.write("%o %s\n" % (mode, a.home))
        for pid in frozen:
            try:
                os.kill(pid, signal.SIGCONT)
            except ProcessLookupError:
                pass
        home_locked = True
        say("HOME made read-only at Moving with %s live (was %o); processes %s stopped for it and continued" % (a.new, mode, frozen))
    if a.cond:
        reached = False
        if ph == target:
            if a.cond == "moving-new":
                reached = version() == a.new
            elif a.cond == "trial-receipt":
                reached = bool(receipts(doc))
            elif a.cond == "stuck-noretrial":
                reached = not retrial(doc)
            else:
                reached = True
        if reached:
            frozen = freeze()
            text2 = read_journal()
            ph2, doc2 = parse(text2) if text2 else (None, None)
            if a.cond == "moving-old" and version() != a.old:
                finish("MISSED", text2, doc2, frozen, "(the swap was already done: this is M6's state, not M5's)")
            elif a.cond == "trial-noreceipt" and receipts(doc2):
                finish("MISSED", text2, doc2, frozen, "(the receipt was already written: this is M8's state, not M7's)")
            elif a.cond == "stuck-noretrial" and retrial(doc2):
                finish("MISSED", text2, doc2, frozen, "(the applier had already recorded its own retrial)")
            elif ph2 != target:
                finish("MISSED", text2, doc2, frozen, "(the phase moved to %s before the freeze took)" % ph2)
            else:
                finish("REACHED", text2, doc2, frozen)
            raise SystemExit(0)
        if beyond(target, ph, seen) or (a.cond == "stuck-noretrial" and ph == "Stuck" and retrial(doc)):
            finish("MISSED", text, doc, [], "(no freeze: the phase is already past %s)" % target)
            raise SystemExit(0)
    time.sleep(0.002)
if a.cond:
    finish("NOT_REACHED", text, doc, [], "(timeout or stop)")
say("watch end")
PY

WPID=""
start_watch() { # start_watch <tag> <python args...>
  local tag=$1
  shift
  rm -f "$EV/watch.stop"
  python3 "$EV/watch.py" --u "$U" --app "$APP" --home "$H" --ev "$EV" --tag "$tag" \
    --o-pid "${OPID:-0}" --old "$OLD_V" --new "$NEW_V" "$@" >> "$EV/watch-$tag.stderr" 2>&1 &
  WPID=$!
  say "watcher $tag started (python pid $WPID): $*"
}
stop_watch() {
  [ -n "$WPID" ] || return 0
  touch "$EV/watch.stop"
  local i
  for i in $(seq 1 50); do alive "$WPID" || break; sleep 0.1; done
  alive "$WPID" && say "watcher $WPID did not stop within 5 s"
  WPID=""
}

# ------------------------------------------------------------------ outside state
restore_data() {
  [ "$DATA" = "$HOME/Library/Application Support/Folio" ] || { say "REFUSED: data path $DATA"; exit 9; }
  if [ -n "$(ours_now)" ] || [ -n "$(foreign_procs)" ]; then
    say "data folder NOT restored: a Folio process runs"
    return 1
  fi
  if [ -d "$U/data-before/Folio" ]; then
    rm -rf "$DATA" && ditto "$U/data-before/Folio" "$DATA" && say "data folder restored from data-before/Folio"
  elif [ -f "$U/data-before/ABSENT" ]; then
    rm -rf "$DATA" && say "data folder removed (there was none before the rehearsal)"
  else
    say "no data-before/ backup: run setup.sh first"
    exit 1
  fi
}
reset_app() {
  [ -d "$PRISTINE" ] || { say "no $PRISTINE: run setup.sh first"; exit 1; }
  if [ -n "$(ours_now)" ]; then say "app/ NOT reset: a process of $U runs"; return 1; fi
  rm_u "$H"
  rm_u "$APP"
  ditto "$PRISTINE" "$APP" || { say "ditto of A failed"; exit 1; }
  [ "$(version)" = "$OLD_V" ] || { say "app/Folio.app reads $(version), not $OLD_V"; exit 1; }
  say "app/ re-placed from a-pristine (version $(version))"
}
BOOTED=""
login_standin() { # the stand-in for a login: launchctl bootstrap of the plist the product wrote
  local f home label rc prog flag n=0
  for f in "$LA/$LPFX"*.plist; do
    [ -f "$f" ] || continue
    home=$(plutil -extract ProgramArguments.2 raw -o - "$f" 2>/dev/null)
    if [ "$home" != "$H" ]; then say "login: $f names $home, not this rehearsal's home; left alone"; continue; fi
    label=$(plutil -extract Label raw -o - "$f" 2>/dev/null)
    cp "$f" "$EV/"
    launchctl bootstrap "gui/$UIDN" "$f" >> "$LOG" 2>&1
    rc=$?
    say "login stand-in: launchctl bootstrap gui/$UIDN $f -> rc=$rc (label $label)"
    if [ "$rc" = 0 ]; then
      BOOTED="$BOOTED $label"
    else
      # the second stand-in, when launchd refuses the bootstrap from ssh: the plist's
      # ProgramArguments run by hand, detached (recorded as such in the write-up)
      prog=$(plutil -extract ProgramArguments.0 raw -o - "$f" 2>/dev/null)
      flag=$(plutil -extract ProgramArguments.1 raw -o - "$f" 2>/dev/null)
      under_u "$prog" || { say "login: $prog is not under $U; not run"; continue; }
      nohup "$prog" "$flag" "$home" > "$EV/login-by-hand-$n.log" 2>&1 < /dev/null &
      record_pid "$!" "login-by-hand"
      say "login stand-in FALLBACK: bootstrap refused, ran ProgramArguments by hand: $prog $flag $home (pid $!)"
    fi
    n=$((n + 1))
  done
  [ "$n" = 0 ] && say "login stand-in: no update LaunchAgent of this home exists - nothing runs at this login"
  return 0
}
quit_apps() {
  local pid n
  for pid in $(app_pids); do press "$pid" "Cmd+Q" "$K_QUIT"; done
  for n in $(seq 1 40); do [ -z "$(app_pids)" ] && break; sleep 0.5; done
  if [ -n "$(app_pids)" ]; then
    say "still running after Cmd+Q: $(app_pids | tr '\n' ' ')"
    shot "quit-left-$(date '+%H%M%S')"
  else
    say "every window process of $U has quit"
  fi
}
mode_save_zero() { # mode_save_zero <name> <path>: records the mode, then chmod 000
  under_u "$2" || { say "REFUSED chmod outside $U: $2"; exit 9; }
  [ -e "$2" ] || { say "$1: $2 does not exist"; return 1; }
  local m
  m=$(stat -f %Lp "$2")
  printf '%s %s %s\n' "$1" "$m" "$2" >> "$EV/modes.txt"
  chmod 000 "$2" && say "$1: chmod 000 $2 (was $m)"
}
mode_back() { # mode_back <name>
  local name m p
  while read -r name m p; do
    [ "$name" = "$1" ] || continue
    under_u "$p" || continue
    chmod "$m" "$p" && say "$1: chmod $m $p (restored)"
  done < "$EV/modes.txt"
}
octal_back() { # octal_back <file written by the watcher: "<mode> <path>">
  local m p
  [ -f "$1" ] || { say "no $1: nothing to restore"; return 0; }
  read -r m p < "$1"
  under_u "$p" || { say "REFUSED chmod outside $U: $p"; exit 9; }
  chmod "$m" "$p" && say "chmod $m $p (restored)"
}

CAF=""
cleanup() {
  stop_watch
  local pid label f home mp
  for pid in $(ours_now); do signal_ours KILL "$pid" "cleanup: a process of this row still running"; done
  sleep 1
  for label in $BOOTED; do
    launchctl bootout "gui/$UIDN/$label" >> "$LOG" 2>&1
    say "launchctl bootout gui/$UIDN/$label -> rc=$?"
  done
  for f in "$LA/$LPFX"*.plist; do
    [ -f "$f" ] || continue
    home=$(plutil -extract ProgramArguments.2 raw -o - "$f" 2>/dev/null)
    if [ "$home" = "$H" ]; then
      mkdir -p "$EV/cleanup"
      cp "$f" "$EV/cleanup/" && rm -f "$f" && say "removed $f (it named this rehearsal's home)"
    else
      say "left $f alone (names $home)"
    fi
  done
  mount | sed -n "s|^.* on \($U/.*\) (.*$|\1|p" | while read -r mp; do
    under_u "$mp" || continue
    hdiutil detach "$mp" >> "$LOG" 2>&1 || hdiutil detach -force "$mp" >> "$LOG" 2>&1
    say "detached $mp"
  done
  [ -n "$CAF" ] && kill "$CAF" 2>/dev/null
  reset_app
  restore_data
  shot "after-cleanup"
}

# ------------------------------------------------------------------ the row
say "=== row $ROW (cut=$CUT, clean=$CLEAN) $(date '+%F %T %z'); plan: $PLAN; cond: ${COND:-none}; then: $THEN"
precondition
if ! ax_check; then
  say "STOPPED: this ssh session may not post key events (Accessibility refused: $(tail -1 "$EV/accessibility.txt")). The row needs Update/Restart presses; nothing was started. The owner grants Accessibility for the ssh session or presses the card through Screen Sharing."
  echo "STOPPED accessibility" > "$EV/RESULT.txt"
  say "ROW $ROW END (stopped)"
  exit 4
fi
say "Accessibility answered the no-op: $(tail -1 "$EV/accessibility.txt")"
if [ -n "$(ours_now)" ]; then say "a process of $U still runs from an earlier row: $(ours_now | tr '\n' ' ') - run teardown.sh or the cleanup"; exit 3; fi
caffeinate -d -u -t 3600 &
CAF=$!
reset_app
restore_data || exit 3
DIAG_OFF=$( [ -f "$DIAG" ] && stat -f %z "$DIAG" || echo 0 )

start_folio "O" --update-feed "$FEED_URL" || { collect "no-start"; [ "$CLEAN" = 1 ] && cleanup; say "ROW $ROW END (A did not start)"; exit 5; }
OPID=$LAST_PID
start_watch "$( [ -n "$COND" ] && echo cut || echo observe)" ${COND:+--cond "$COND"} $WOPTS

RESULT="done"
for step in $PLAN; do
  say "step $step"
  case "$step" in
    offer)
      if ! wait_diag "is offered" 180; then RESULT="no offer card"; break; fi
      sleep 3; shot offer-card ;;
    update) press "$OPID" "Update (Shift+Tab, Enter)" "$K_STAB" 'delay 0.4' "$K_ENTER" || { RESULT="keys refused"; break; } ;;
    update-cancel)
      press "$OPID" "Update then Cancel (Shift+Tab, Enter, Tab, Enter)" "$K_STAB" 'delay 0.3' "$K_ENTER" 'delay 0.4' "$K_TAB" 'delay 0.2' "$K_ENTER" || { RESULT="keys refused"; break; }
      sleep 2; shot after-cancel ;;
    ready)
      if ! wait_phase "Prepared" 300; then RESULT="never Ready"; break; fi
      sleep 3; shot restart-card ;;
    restart) press "$OPID" "Restart (Shift+Tab, Enter)" "$K_STAB" 'delay 0.4' "$K_ENTER" || { RESULT="keys refused"; break; } ;;
    later) press "$OPID" "Later (Escape)" "$K_ESC"; sleep 2; shot after-later ;;
    quit) quit_apps ;;
    rescue-000) mode_save_zero rescue "$H/$(txn)/rescue/Folio.app/Contents/MacOS/folio" || { RESULT="no rescue executable"; break; } ;;
    exe-000) mode_save_zero exe "$EXE" ;;
    o-gone)
      for n in $(seq 1 300); do alive "$OPID" || break; sleep 0.5; done
      if alive "$OPID"; then say "O ($OPID) still runs after 150 s"; else say "O ($OPID) has left"; fi ;;
    rescue-back) mode_back rescue ;;
    alert)
      wait_diag "no start after the update was delivered" 150
      sleep 3; shot alert; collect at-alert ;;
    *) say "unknown step $step"; RESULT="bad plan"; break ;;
  esac
done

if [ "$RESULT" = "done" ] && [ -n "$COND" ]; then
  for n in $(seq 1 1800); do [ -f "$EV/cut.txt" ] && break; sleep 0.2; done
  CUTLINE=$(cat "$EV/cut.txt" 2>/dev/null)
  say "watcher: ${CUTLINE:-no answer within 360 s}"
  case "$CUTLINE" in
    *" REACHED "*)
      while read -r pid rl rest; do
        [ -n "$pid" ] || continue
        if [ "$CUT" = all ] || printf ' %s ' "$NAMED" | grep -q " $rl "; then
          if signal_ours KILL "$pid" "the cut ($rl)"; then
            echo "KILLED $pid $rl $rest" >> "$EV/cut.txt"
          else
            echo "NOT-SIGNALLED $pid $rl (already gone) $rest" >> "$EV/cut.txt"
          fi
        else
          signal_ours CONT "$pid" "left running by --cut named ($rl)"
          echo "CONTINUED $pid $rl $rest" >> "$EV/cut.txt"
        fi
      done < "$EV/frozen.txt"
      say "cut done: kill -9 (synthetic-process evidence, weaker than a power cut)"
      sleep 2
      collect at-cut ;;
    *)
      RESULT="row not reached: ${CUTLINE:-no watcher answer}"
      say "$RESULT - the road continues uncut; recording where it ends"
      wait_doors_gone 240; sleep 15
      collect after-miss ;;
  esac
  stop_watch
  [ "$RESULT" = "done" ] && start_watch then
fi

if [ "$RESULT" = "done" ]; then
  n_plain=0; n_login=0
  for step in $THEN; do
    say "then $step"
    case "$step" in
      plain)
        n_plain=$((n_plain + 1))
        start_folio "plain$n_plain"
        sleep 5; wait_doors_gone 180; sleep 20
        shot "after-plain$n_plain"; collect "after-plain$n_plain" ;;
      plain-go) # a plain start whose road is watched by the next step, not waited for here
        n_plain=$((n_plain + 1))
        start_folio "plain$n_plain" ;;
      login)
        n_login=$((n_login + 1))
        login_standin
        sleep 5; wait_doors_gone 180; sleep 20
        shot "after-login$n_login"; collect "after-login$n_login" ;;
      quit) quit_apps ;;
      settle) sleep 3; wait_doors_gone 240; sleep 15; shot settled; collect settled ;;
      settle-door) sleep 3; wait_doors_gone 240; sleep 15; shot road-ended; collect road-ended ;;
      settle-stuck)
        wait_phase "Stuck" 240; wait_doors_gone 240; sleep 15
        shot stuck-window; collect at-stuck ;;
      retrial)
        for n in $(seq 1 1200); do
          [ "$(jget '"yes" if d["body"]["phase"].get("retrial") else "no"')" = yes ] && break
          case "$(phase)" in Committed | Retired | absent) break ;; esac
          sleep 0.2
        done
        say "retrial: journal $(phase) $(jget 'd["body"]["phase"].get("attempts", "")') attempts, retrial $(jget 'd["body"]["phase"].get("retrial", "")')"
        sleep 8; shot retrial-at-launch
        wait_phase "Committed|Retired|absent" 240
        sleep 6; shot retrial-after-commit; collect after-retrial ;;
      stage-back) octal_back "$EV/stage-mode.txt" ;;
      home-back) octal_back "$EV/home-mode.txt" ;;
      exe-back) mode_back exe ;;
      rescue-back) mode_back rescue ;;
      shot:*) shot "${step#shot:}" ;;
      collect:*) collect "${step#collect:}" ;;
      *) say "unknown step $step" ;;
    esac
  done
fi

collect end
echo "$RESULT" > "$EV/RESULT.txt"
if [ "$CLEAN" = 1 ]; then cleanup; else stop_watch; say "--no-clean: processes, plist, app/ and the data folder left as they are"; fi
say "ROW $ROW END ($RESULT)"
