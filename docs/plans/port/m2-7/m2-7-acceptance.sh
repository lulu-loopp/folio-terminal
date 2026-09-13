#!/bin/sh
# M2-7 — §M2's acceptance line, every row, on the real binary.
#
# "With a clean data directory: open a folder in the files column; click a `.md`
# file with a table and a CJK paragraph and the preview renders it; change a
# heading in place, press the save chord, and `md5 <file>` in a pane shows bytes
# that changed; then replace that file from a pane with
# `printf '# other\n' > new && mv new <file>` and the preview updates without a
# click; create a file in a *subdirectory* of the open folder from a pane and
# the tree shows it; click a `.png` and it shows; open a file containing
# `$$\int_0^1 x\,dx$$` and the integral is typeset, not printed as source."
#
# The application is the debug `folio` built by `m2-7-door.sh`, inside a
# throwaway bundle with an identifier of its own and an **isolated HOME** —
# through a `CFBundleExecutable` wrapper script, because `LSEnvironment` cannot
# set `HOME` (DESIGN §13.31 ⑧(d)). The fixture folder is **under that HOME**,
# so the `~` rule §13.32 ③ gave the breadcrumbs is on the page and can be read.
#
# How each row is driven, and why:
#
# * **the folder in the files column** — `folio <folder>` opens a terminal tab
#   in that folder and no column (`cli.rs::resolve`), and the gesture that opens
#   one cannot be posted: CGEvent keyboard injection into Folio produces nothing
#   on this Mac (§13.34 ⑦). The column is opened through the application's own
#   door for a column that was open before — a seeded `session.json` at schema
#   15 whose tab is **pinned**, so the launch opens it straight away instead of
#   raising the restore card (`main.rs`: "a window that held a pinned tab opens
#   straight away").
# * **"from a pane"** — the watcher cannot tell one writer from another, so the
#   replacement and the subdirectory create are run from this script's own shell
#   against the same folder. What is asserted is the *watch*, and the latency
#   from the write to the frame that carries it is printed.
# * **the clicks** — CGEvent mouse posting at a window this session started, at a
#   point read out of the window's own chrome dump. The first-run card is
#   dismissed by clicking its own button, which is both the honest way past it
#   and the first evidence about whether a press reaches this window's content.
# * **the typing rows** — not driven at all. They are written down as a hand
#   procedure in DESIGN §13.40 for the owner.
#
# Nothing outside ~/folio-port is written; every process ended is one this
# script started, by the pid it wrote down.
set -u

WT="$HOME/folio-port/wt/m2-7"
TARGET="$HOME/folio-port/target-m2-7"
OUT="$WT/out-acc"
APP="$OUT/FolioM27.app"
ISO="$OUT/home"
PAGES="$ISO/pages"
SHOTS="$OUT/shots"
DUMP="$OUT/dump"
BID="io.github.lulu-loopp.folio.m2-7-acc"
SUPPORT="$ISO/Library/Application Support/Folio"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
LAUNCHERS="$HOME/folio-port/launchers"
FOLIO="$TARGET/debug/folio"
PID=""
WINID=""
WINBOX=""

if [ ! -x "$FOLIO" ]; then
  echo "MISSING $FOLIO"
  echo "ALL_DONE"
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$ISO" "$PAGES/sub" "$SHOTS" "$DUMP" "$SUPPORT"
cp "$FOLIO" "$APP/Contents/MacOS/folio"

# ---------------------------------------------------------------- the fixtures
# A table and a CJK paragraph on one page, so one photograph answers both halves
# of that row.
cat > "$PAGES/table-cjk.md" <<'MD'
# 阅读面 Reading surface

一段中英混排的正文:这台机器上的字体、行高和断行都要和 Windows 那一侧读起来是
同一份文档,and this sentence is long enough that a narrow pane has to wrap it
somewhere in the middle rather than clip it.

| 列 A | Column B | 列 C |
|---|---|---|
| 表格 | table | 表格 |
| cell | cell | cell |
MD

cat > "$PAGES/math.md" <<'MD'
# 公式 Typeset mathematics

$$\int_0^1 x\,dx$$

正文一行,给排版一个参照 — a line of prose for the typesetting to stand against.
MD

cp "$WT/assets/readme/surfaces-light.png" "$PAGES/picture.png"
printf 'the folder was here before the run\n' > "$PAGES/sub/before.txt"
echo "fixtures:"
ls -la "$PAGES" "$PAGES/sub"
echo "md5 table-cjk.md at rest: $(md5 -q "$PAGES/table-cjk.md")"

# ------------------------------------------------------------- the data folder
# Schema 15. `leaf-N` is the in-order index of the seat in this tree, the token
# `focused_leaf` and every preview pane row already use (`main.rs`,
# `preview_content`): files = leaf-0, preview = leaf-1, term = leaf-2.
write_session() {
  cat > "$SUPPORT/session.json" <<SESSION
{
  "schema_version": 15,
  "windows": [
    {
      "tabs": [
        {
          "root": {
            "dir": "row",
            "ratio": 300000,
            "children": [
              { "kind": "files", "root": "$PAGES", "open": ["/sub"], "sel": null, "width": 320, "view": "files" },
              {
                "dir": "col",
                "ratio": 720000,
                "children": [
                  { "kind": "preview", "pinned": false },
                  { "kind": "term", "profile_id": "zsh", "cwd": "$PAGES", "manual_name": null }
                ]
              }
            ]
          },
          "pinned": true,
          "focused_leaf": "leaf-1",
          "preview": {
            "panes": [ { "leaf": "leaf-1", "cur": "$PAGES/$1" } ],
            "pool": [ { "path": "$PAGES/$1", "name": "$1" } ]
          }
        }
      ],
      "active_tab": 0
    }
  ],
  "recent": []
}
SESSION
  /usr/bin/python3 -c "import json,sys;json.load(open(sys.argv[1]))" "$SUPPORT/session.json"
  echo "session.json for $1 parses: rc=$?"
}

# The pane's own shell runs this without a key being pressed: `folio.zsh` hands
# `ZDOTDIR` back at the end of itself, so zsh then reads `$HOME/.zshrc`, and
# `$HOME` is this run's isolated one. The digest it prints is the "before" the
# owner's hand procedure for the save chord compares against.
cat > "$ISO/.zshrc" <<ZSHRC
export PS1='m2-7 %1~ %# '
print -r -- "M27-PANE-READY"
print -r -- "M27-MD5-BEFORE \$(md5 -q "$PAGES/table-cjk.md")"
# **The differential for the typeset row.** A terminal pane sets display maths
# too, through the *same* worker thread the preview's formulas go to
# (\`MathWorkerRequest::Math\` beside \`MathWorkerRequest::PreviewMath\`). If this
# line arrives set and the preview's does not, the engine and the worker are
# both alive and the defect is in the preview's road to them; if neither is set,
# the worker is the suspect.
print -r -- "M27-TERMINAL-FORMULA"
print -r -- '\$\$\\int_0^1 x\\,dx\$\$'
ZSHRC

# ------------------------------------------------------------------ the bundle
cat > "$APP/Contents/MacOS/folio-launch" <<LAUNCH
#!/bin/sh
export HOME="$ISO"
export BT_PTY_DUMP="$DUMP/pty.dump"
export BT_CHROME_DUMP="$DUMP/chrome.dump"
export BT_MOUSE_TRACE="$DUMP/mouse.trace"
export BT_PREVIEW_TRACE="$DUMP/preview.trace"
exec "\$(dirname "\$0")/folio" "\$@"
LAUNCH
chmod +x "$APP/Contents/MacOS/folio-launch"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>folio-launch</string>
  <key>CFBundleIdentifier</key><string>${BID}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>FolioM27</string>
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
"$LSREGISTER" -f "$APP"; echo "rc=$?"

# ------------------------------------------------------------------- the tools
swiftc -O -o "$OUT/winid" "$LAUNCHERS/winid.swift" 2>&1 | head -5
swiftc -O -o "$OUT/click" "$LAUNCHERS/mac_lights_click.swift" 2>&1 | head -5
# **A pair, and not two singles.** A second press on a file row is the verb that
# opens it (`press_files_row`, K156), and `FilesRowClicks::register` pairs two
# presses only inside `MULTI_CLICK_INTERVAL` — 500 ms. Two runs of the
# single-click probe are four seconds apart and are two first presses, which is
# a selection and a glance card and no open at all.
cat > "$OUT/dclick.swift" <<'SWIFT'
// One double click at one point: down, up, down, up, with the click state
// AppKit reads for a pair, inside Folio's own 500 ms window.
import CoreGraphics
import Foundation

let argv = CommandLine.arguments
guard argv.count >= 3, let x = Double(argv[1]), let y = Double(argv[2]) else {
    print("usage: dclick <x> <y>")
    exit(2)
}
let at = CGPoint(x: x, y: y)
let source = CGEventSource(stateID: .hidSystemState)

func post(_ type: CGEventType, _ clicks: Int64) {
    guard let event = CGEvent(
        mouseEventSource: source,
        mouseType: type,
        mouseCursorPosition: at,
        mouseButton: .left
    ) else {
        print("could not build \(type.rawValue)")
        return
    }
    event.setIntegerValueField(.mouseEventClickState, value: clicks)
    event.post(tap: .cghidEventTap)
}

post(.mouseMoved, 0)
usleep(150_000)
post(.leftMouseDown, 1)
usleep(60_000)
post(.leftMouseUp, 1)
usleep(80_000)
post(.leftMouseDown, 2)
usleep(60_000)
post(.leftMouseUp, 2)
print("DOUBLE CLICK at \(x),\(y)")
SWIFT
swiftc -O -o "$OUT/dclick" "$OUT/dclick.swift" 2>&1 | head -5
ls -la "$OUT/winid" "$OUT/click" "$OUT/dclick" 2>&1

# --------------------------------------------------------------------- helpers
shot() {
  # `-o` drops the shadow, so pixel (0,0) is the window's own corner.
  /usr/sbin/screencapture -x -o -l"$WINID" "$SHOTS/$1.png"
  echo "shot $1 rc=$? bytes=$(stat -f %z "$SHOTS/$1.png" 2>/dev/null)"
}

frame() {
  # The whole of the last chrome frame — quads, sprites and labels — so a
  # rectangle whose owner is in doubt can be identified by what it stands among.
  /usr/bin/awk '/^--- chrome frame/ { n = NR } { line[NR] = $0 } END { for (i = n; i <= NR; i++) print line[i] }' "$DUMP/chrome.dump" 2>/dev/null
}

last_labels() {
  frame | grep '^label'
}

launch() {
  /usr/bin/open -a "$APP"
  i=0
  PID=""
  while [ $i -lt 60 ]; do
    PID=$(/bin/ps -axo pid=,command= | grep "$APP/Contents/MacOS/folio" | grep -v grep | awk '{print $1}' | head -1)
    [ -n "$PID" ] && break
    sleep 1
    i=$((i + 1))
  done
  echo "folio pid=${PID:-none} after ${i}s"
  [ -z "$PID" ] && return 1
  sleep 14
  echo "windows of this pid:"
  "$OUT/winid" "$PID"
  WINID=$("$OUT/winid" "$PID" | head -1 | sed -e 's/^id=//' -e 's/ .*//')
  WINBOX=$("$OUT/winid" "$PID" | head -1 | sed -e 's/^id=[0-9]* //')
  echo "WINID=$WINID WINBOX=$WINBOX"
  return 0
}

end_run() {
  [ -z "$PID" ] && return 0
  echo "ending the pid this script started: $PID"
  kill "$PID" 2>&1
  sleep 4
  echo "still there: $(/bin/ps -axo pid= | awk -v p="$PID" '$1==p {print $1}')"
  PID=""
}

# A label's centre, in the global point space `CGEvent` posts into. `$1` is a
# `grep -E` pattern the label line has to match; the line chosen is printed.
point_of() {
  /usr/sbin/screencapture -x -o -l"$WINID" "$OUT/scale.png" 2>/dev/null
  # The frame goes to a file rather than down a pipe: this python's standard
  # input is the here-document that carries the program, so a pipe into it
  # reaches nothing.
  frame > "$OUT/frame.txt"
  /usr/bin/python3 - "$1" "$WINBOX" "$OUT/scale.png" "$OUT/frame.txt" <<'PY'
import re, sys
from PIL import Image
pattern, box, shot, frame = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
lines = [l for l in open(frame, encoding="utf-8", errors="replace").read().splitlines()
         if l.startswith("label") and re.search(pattern, l)]
if not lines:
    print("NO-LABEL")
    sys.exit(0)
line = lines[-1]
print("LABEL-LINE", line.strip(), file=sys.stderr)
rect = [float(v) for v in re.findall(r"-?\d+\.\d+|-?\d+", line.split("]")[0])[:4]]
m = re.match(r"(-?\d+),(-?\d+) (\d+)x(\d+)", box)
wx, wy, ww = (int(m.group(i)) for i in (1, 2, 3))
# The rectangles are the window's own physical pixels and CGEvent speaks points,
# so the scale is this window's photograph over this window's point width — read
# rather than assumed, because the desk this runs on can change.
scale = Image.open(shot).size[0] / ww
print("SCALE", scale, file=sys.stderr)
print("%.0f %.0f" % ((rect[0] + rect[2]) / 2 / scale + wx,
                     (rect[1] + rect[3]) / 2 / scale + wy))
PY
}

click_at() {
  MOUSE_BEFORE=$(wc -l < "$DUMP/mouse.trace" 2>/dev/null || echo 0)
  "$OUT/click" $1 $2
  sleep 3
  echo "--- the mouse-routing lines this press added ---"
  tail -n +$((MOUSE_BEFORE + 1)) "$DUMP/mouse.trace" 2>/dev/null | head -20
}

# ============================================================ ROWS ① ② — the
# folder is in the files column and the .md with a table and a CJK paragraph is
# rendered in the preview.
echo "=================================================== RUN A: table + CJK"
write_session table-cjk.md
launch || { echo "THE APPLICATION NEVER STARTED"; echo "ALL_DONE"; exit 1; }
shot 00-clean-data-directory
echo "--- the labels on the glass, clean data directory ---"
last_labels | head -60

# The first-run card stands over the reading surfaces on a clean data directory,
# which is what "clean" means and not a defect. It is dismissed the way a reader
# dismisses it: by pressing its own primary button, whose ink is the only
# `#ffffff` label on the page.
echo "--- dismissing the first-run card through its own button ---"
CARD=$(point_of '#ffffff')
echo "card button point: $CARD"
case "$CARD" in
  NO-LABEL|"") echo "no first-run card on the glass" ;;
  *) click_at $CARD ;;
esac
sleep 4
shot 01-md-table-cjk
echo "--- the labels on the glass ---"
last_labels | head -60
echo "--- the whole last frame, the band under the document ---"
frame | /usr/bin/awk '/^(quad|sprite|label)/ { if ($0 ~ /1[01][0-9][0-9]\.0|1[12][0-9][0-9]\.0/) print }' | head -30
echo "--- preview trace ---"
tail -12 "$DUMP/preview.trace" 2>/dev/null
echo "--- the pane's own shell (pty dump) ---"
for d in "$DUMP"/pty.dump*; do
  [ -f "$d" ] || continue
  echo "dump $d ($(stat -f %z "$d") bytes)"
  /usr/bin/strings "$d" | grep -E "M27-" | head -5
done

# ==================================================== ROW ④ — the file is
# replaced from outside the process and the preview follows with no click.
echo "=================================================== ROW: replace, no click"
# A Markdown body goes to `bt_render::PreviewBody` and not to a label, so what
# says the page was rebuilt is `BT_PREVIEW_TRACE`'s `document bytes=…` station.
# `# other\n` is 8 bytes and the page it replaces is 362, so the number is the
# event.
/usr/bin/python3 - "$PAGES" "$DUMP/preview.trace" <<'PY'
import os, subprocess, sys, time
pages, trace = sys.argv[1], sys.argv[2]
target = os.path.join(pages, "table-cjk.md")
new = os.path.join(pages, "new")
before = open(trace, "rb").read() if os.path.exists(trace) else b""
t0 = time.time()
with open(new, "w", encoding="utf-8") as f:
    f.write("# other\n")
os.replace(new, target)
print("replaced; md5 now",
      subprocess.run(["md5", "-q", target], capture_output=True, text=True).stdout.strip())
found = None
while time.time() - t0 < 25:
    try:
        blob = open(trace, "rb").read()
    except OSError:
        blob = b""
    if b"document bytes=8 " in blob[len(before):]:
        found = time.time() - t0
        break
    time.sleep(0.02)
print("REPLACE-LATENCY", ("%.3f" % found) if found is not None else "NOT SEEN in 25s")
print("--- the stations this replacement added ---")
print(open(trace, "rb").read()[len(before):].decode("utf-8", "replace").strip())
PY
sleep 2
shot 02-md-replaced

# ==================================================== ROW ⑤ — a file created in
# a subdirectory of the open folder shows in the tree.
echo "=================================================== ROW: create in a subdirectory"
/usr/bin/python3 - "$PAGES" "$DUMP/chrome.dump" <<'PY'
import os, sys, time
pages, dump = sys.argv[1], sys.argv[2]
made = os.path.join(pages, "sub", "made-by-the-sweep.txt")
t0 = time.time()
with open(made, "w", encoding="utf-8") as f:
    f.write("created from a second shell against the same folder\n")
found = None
while time.time() - t0 < 25:
    try:
        blob = open(dump, "rb").read()
    except OSError:
        blob = b""
    if b"made-by-the-sweep" in blob.rsplit(b"--- chrome frame", 3)[-1]:
        found = time.time() - t0
        break
    time.sleep(0.02)
print("SUBDIR-TREE-LATENCY", ("%.3f" % found) if found is not None else "NOT SEEN in 25s")
PY
sleep 2
shot 03-tree-subdir
echo "--- the tree's own rows ---"
last_labels | grep -E "sub|before|made-by-the-sweep|picture|math|table|pages|~" | head -30

echo "--- control: a create in the watched root itself ---"
/usr/bin/python3 - "$PAGES" "$DUMP/chrome.dump" <<'PY'
import os, sys, time
pages, dump = sys.argv[1], sys.argv[2]
made = os.path.join(pages, "root-level.txt")
t0 = time.time()
with open(made, "w", encoding="utf-8") as f:
    f.write("at the root of the watched folder\n")
found = None
while time.time() - t0 < 25:
    try:
        blob = open(dump, "rb").read()
    except OSError:
        blob = b""
    if b"root-level" in blob.rsplit(b"--- chrome frame", 3)[-1]:
        found = time.time() - t0
        break
    time.sleep(0.02)
print("ROOT-TREE-LATENCY", ("%.3f" % found) if found is not None else "NOT SEEN in 25s")
PY
sleep 2
shot 04-tree-root

# ==================================================== ROW ⑥ — click a .png and
# it shows.
echo "=================================================== ROW: click the .png"
PNG=$(point_of 'picture\.png')
echo "picture.png row point: $PNG"
case "$PNG" in
  NO-LABEL|"") echo "the files column never drew a row for picture.png" ;;
  *)
    echo "--- one press: the row is selected and glanced at, and that is all ---"
    click_at $PNG
    shot 05a-png-one-press
    echo "--- the pair, which is the verb ---"
    MOUSE_BEFORE=$(wc -l < "$DUMP/mouse.trace" 2>/dev/null || echo 0)
    "$OUT/dclick" $PNG
    sleep 4
    echo "--- the mouse-routing lines the pair added ---"
    tail -n +$((MOUSE_BEFORE + 1)) "$DUMP/mouse.trace" 2>/dev/null | head -20
    ;;
esac
sleep 6
shot 05-png-after-click
echo "--- preview trace ---"
tail -8 "$DUMP/preview.trace" 2>/dev/null
echo "--- the preview head and rail now ---"
last_labels | grep -E "picture|PNG|×" | head -10
end_run

# ==================================================== ROW ⑥′ — the picture,
# reached the way the session reaches it, so the row has an answer either way.
echo "=================================================== RUN B: the .png in the preview"
rm -f "$DUMP/chrome.dump"
write_session picture.png
launch || { echo "RUN B NEVER STARTED"; echo "ALL_DONE"; exit 1; }
shot 06-png
echo "--- preview trace ---"
tail -8 "$DUMP/preview.trace" 2>/dev/null
echo "--- labels ---"
last_labels | head -50
end_run

# ==================================================== ROW ⑦ — the integral is
# typeset, not printed as source.
echo "=================================================== RUN C: the integral"
rm -f "$DUMP/chrome.dump"
write_session math.md
launch || { echo "RUN C NEVER STARTED"; echo "ALL_DONE"; exit 1; }
shot 07-math
echo "--- labels ---"
last_labels | head -50
echo "--- preview trace ---"
tail -8 "$DUMP/preview.trace" 2>/dev/null
# **Is the picture late, or is it never coming?** A page whose formula is still
# pending stands on its source, and so does one whose formula was refused — the
# two look identical on the glass (`PreviewMathArtifact`). Three readings tell
# them apart: a long wait, a pointer that makes the window draw again, and the
# worker thread's own stack.
echo "--- the math worker's thread, thirty seconds in ---"
sleep 30
/usr/bin/sample "$PID" 1 -file "$OUT/sample-math.txt" > /dev/null 2>&1
echo "sample rc=$?"
grep -n "bt-math-worker" -A 12 "$OUT/sample-math.txt" 2>/dev/null | head -30
echo "threads named in the sample: $(grep -c 'Thread_' "$OUT/sample-math.txt" 2>/dev/null)"
shot 08-math-after-30s
echo "--- a pointer over the document, then another frame ---"
MOUSE_BEFORE=$(wc -l < "$DUMP/mouse.trace" 2>/dev/null || echo 0)
"$OUT/click" 900 400
sleep 4
shot 09-math-after-a-press
tail -n +$((MOUSE_BEFORE + 1)) "$DUMP/mouse.trace" 2>/dev/null | head -10
echo "--- preview trace, the whole of it ---"
tail -20 "$DUMP/preview.trace" 2>/dev/null
echo "--- the terminal pane's own formula (the differential) ---"
for d in "$DUMP"/pty.dump*; do
  [ -f "$d" ] || continue
  /usr/bin/strings "$d" | grep -E "M27-TERMINAL-FORMULA|int_0" | head -5
done
end_run

# ------------------------------------------------------------------ the reads
echo "=================================================== the pixel reads"
/usr/bin/python3 - "$SHOTS" "$PAGES/picture.png" <<'PY'
import collections, os, sys
from PIL import Image

shots, source = sys.argv[1], sys.argv[2]


def ground_of(px, x0, x1, y0, y1):
    """The page's own ground: the commonest colour in the band, not a corner
    pixel — a window whose corner is a rounded transparent one answers black to
    that and then every pixel on the page counts as ink."""
    counter = collections.Counter()
    for y in range(y0, y1, 3):
        for x in range(x0, x1, 3):
            counter[px[x, y]] += 1
    return counter.most_common(1)[0][0]


def bands(counts, floor=1):
    out, start = [], None
    for i, n in enumerate(counts):
        if n >= floor and start is None:
            start = i
        elif n < floor and start is not None:
            out.append((start, i, i - start))
            start = None
    if start is not None:
        out.append((start, len(counts), len(counts) - start))
    return out


def read(name):
    path = os.path.join(shots, name + ".png")
    if not os.path.exists(path):
        print(name, "NO SHOT")
        return
    img = Image.open(path).convert("RGB")
    px = img.load()
    w, h = img.size
    # The seeded column is 320 logical px of a 1280-point window at scale 2, and
    # the document starts after the pane's head and rail: x from 660, y from 200
    # to the pane's floor.
    x0, x1, y0, y1 = 660, w - 20, 200, 1120
    ground = ground_of(px, x0, x1, y0, y1)
    rows, ink, total, aa, peak = [], 0, 0.0, 0, 0.0
    for y in range(y0, y1):
        n = 0
        for x in range(x0, x1):
            r, g, b = px[x, y]
            d = max(abs(r - ground[0]), abs(g - ground[1]), abs(b - ground[2])) / 255.0
            if d > 0.02:
                n += 1
                ink += 1
                total += d
                peak = max(peak, d)
                if 0.05 < d < 0.95:
                    aa += 1
        rows.append(n)
    rb = bands(rows, floor=2)
    print(f"{name}: size={w}x{h} document box=({x0},{y0})-({x1},{y1}) ground={ground}")
    print(f"{name}: ink={ink} mean={(total / ink if ink else 0):.4f} "
          f"aa={(aa / ink if ink else 0):.4f} peak={peak:.4f}")
    print(f"{name}: {len(rb)} line boxes; tallest={max((b[2] for b in rb), default=0)}px; "
          f"widest row={max(rows) if rows else 0}px of {x1 - x0}")
    print(f"{name}: boxes(y+{y0})={[(b[0] + y0, b[1] + y0, b[2]) for b in rb][:20]}")
    return img, px, (x0, y0, x1, y1), ground


for name in ["00-clean-data-directory", "01-md-table-cjk", "02-md-replaced",
             "03-tree-subdir", "04-tree-root", "05a-png-one-press",
             "05-png-after-click", "06-png", "07-math", "08-math-after-30s",
             "09-math-after-a-press"]:
    read(name)
    print()

# The picture against its own file: what is on the glass follows the file's own
# colour, and the aspect it is drawn at follows the file's own aspect.
src = Image.open(source).convert("RGB")
sw, sh = src.size
spx = src.load()
acc, n = [0, 0, 0], 0
for y in range(0, sh, 8):
    for x in range(0, sw, 8):
        r, g, b = spx[x, y]
        acc[0] += r
        acc[1] += g
        acc[2] += b
        n += 1
print("the file picture.png:", sw, "x", sh, "mean rgb",
      [round(c / n, 1) for c in acc], "aspect %.4f" % (sw / sh))

for name in ["06-png", "05-png-after-click"]:
    path = os.path.join(shots, name + ".png")
    if not os.path.exists(path):
        continue
    img = Image.open(path).convert("RGB")
    px = img.load()
    w, h = img.size
    x0, x1, y0, y1 = 660, w - 20, 200, 1120
    ground = ground_of(px, x0, x1, y0, y1)
    # A photograph is inked all the way across; prose is inked in words.
    wide = []
    for y in range(y0, y1):
        n = 0
        for x in range(x0, x1):
            r, g, b = px[x, y]
            if max(abs(r - ground[0]), abs(g - ground[1]), abs(b - ground[2])) / 255.0 > 0.02:
                n += 1
        wide.append(1 if n > (x1 - x0) * 0.5 else 0)
    solid = sorted(bands(wide), key=lambda b: -b[2])
    print(f"{name}: bands inked across more than half the document: "
          f"{[(b[0] + y0, b[1] + y0, b[2]) for b in solid[:4]]}")
    if solid:
        by0, by1 = solid[0][0] + y0, solid[0][1] + y0
        cols = []
        for x in range(x0, x1):
            n = 0
            for y in range(by0, by1, 2):
                r, g, b = px[x, y]
                if max(abs(r - ground[0]), abs(g - ground[1]), abs(b - ground[2])) / 255.0 > 0.02:
                    n += 1
            cols.append(1 if n > (by1 - by0) * 0.25 else 0)
        cb = sorted(bands(cols), key=lambda b: -b[2])
        acc, n = [0, 0, 0], 0
        for y in range(by0, by1, 2):
            for x in range(x0, x1, 2):
                r, g, b = px[x, y]
                acc[0] += r
                acc[1] += g
                acc[2] += b
                n += 1
        width = cb[0][2] if cb else 0
        print(f"{name}: the picture stands y={by0}..{by1} ({by1 - by0}px), "
              f"x width {width}px, drawn aspect "
              f"{(width / (by1 - by0)) if by1 > by0 else 0:.4f}, mean rgb "
              f"{[round(c / n, 1) for c in acc]}")
PY

echo "=================================================== teardown"
echo "chrome.dump: $(stat -f %z "$DUMP/chrome.dump" 2>/dev/null) bytes"
"$LSREGISTER" -u "$APP"; echo "lsregister -u rc=$?"
rm -rf "$HOME/Library/WebKit/$BID" "$HOME/Library/Caches/$BID" \
       "$HOME/Library/Saved Application State/$BID.savedState"
echo "df: $(df -h "$HOME" | tail -1)"
echo "ALL_DONE"
