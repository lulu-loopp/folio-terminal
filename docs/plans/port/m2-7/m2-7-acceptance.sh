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
# set `HOME` (DESIGN §13.31 ⑧(d)).
#
# How each row is driven, and why:
#
# * **the folder in the files column** — `folio <folder>` opens a terminal tab
#   in that folder and no column (`cli.rs::resolve`), and the gesture that opens
#   one cannot be posted: CGEvent keyboard injection into Folio produces nothing
#   on this Mac (§13.34 ⑦). The column is therefore opened through the
#   application's own door for a column that was open before — a seeded
#   `session.json` at schema 15 whose tab is **pinned**, so the launch opens it
#   straight away instead of raising the restore card (`main.rs`: "a window that
#   held a pinned tab opens straight away").
# * **"from a pane"** — the watcher cannot tell one writer from another, so the
#   replacement and the subdirectory create are run from this script's own shell
#   against the same folder. What is asserted is the *watch*.
# * **the clicks** — CGEvent mouse posting at a window this session started is
#   the one input that works here; the chrome dump and the mouse trace say
#   whether it arrived.
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
PAGES="$OUT/pages"
SHOTS="$OUT/shots"
DUMP="$OUT/dump"
BID="io.github.lulu-loopp.folio.m2-7-acc"
SUPPORT="$ISO/Library/Application Support/Folio"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
LAUNCHERS="$HOME/folio-port/launchers"
FOLIO="$TARGET/debug/folio"
PID=""
WINID=""

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

# The pane's own shell runs this without a key being pressed: with no shell
# integration installed `ZDOTDIR` is unset, so zsh reads `$HOME/.zshrc`, and
# `$HOME` is this run's isolated one.
cat > "$ISO/.zshrc" <<ZSHRC
export PS1='m2-7 %1~ %# '
print -r -- "M27-PANE-READY"
print -r -- "M27-MD5-BEFORE \$(md5 -q "$PAGES/table-cjk.md")"
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
ls -la "$OUT/winid" "$OUT/click" 2>&1

cat > "$OUT/measure.py" <<'PY'
"""Landmark reads off a window photograph.

Numbers are physical pixels of the capture. `ink`, `mean` and `aa` are M2-5's
three (DESIGN §13.22 ③) read the same way — the distance from the page's own
ground on the encoded byte, because that is what a reader's eye meets — so this
table and that one can be laid beside each other.
"""
import sys
from PIL import Image


def stats(px, box, ground):
    x0, y0, x1, y1 = box
    ink = 0
    total = 0.0
    aa = 0
    peak = 0.0
    for y in range(y0, y1):
        for x in range(x0, x1):
            r, g, b = px[x, y][:3]
            d = max(abs(r - ground[0]), abs(g - ground[1]), abs(b - ground[2])) / 255.0
            if d > 0.02:
                ink += 1
                total += d
                peak = max(peak, d)
                if 0.05 < d < 0.95:
                    aa += 1
    return ink, (total / ink if ink else 0.0), (aa / ink if ink else 0.0), peak


def bands(counts, floor=1):
    out, start = [], None
    for i, n in enumerate(counts):
        if n >= floor and start is None:
            start = i
        elif n < floor and start is not None:
            out.append((start, i))
            start = None
    if start is not None:
        out.append((start, len(counts)))
    return out


def profile(px, box, ground, axis):
    x0, y0, x1, y1 = box
    out = []
    outer = range(y0, y1) if axis == "row" else range(x0, x1)
    inner = range(x0, x1) if axis == "row" else range(y0, y1)
    for a in outer:
        n = 0
        for b in inner:
            r, g, bl = px[(b, a)][:3] if axis == "row" else px[(a, b)][:3]
            if max(abs(r - ground[0]), abs(g - ground[1]), abs(bl - ground[2])) / 255.0 > 0.02:
                n += 1
        out.append(n)
    return out


if __name__ == "__main__":
    img = Image.open(sys.argv[1]).convert("RGB")
    px = img.load()
    w, h = img.size
    print("size", w, h)
    ground = px[6, h - 6]
    print("ground read at (6, h-6):", ground)
    for arg in sys.argv[2:]:
        name, rest = arg.split("=", 1)
        box = tuple(int(v) for v in rest.split(","))
        box = (max(0, box[0]), max(0, box[1]), min(w, box[2]), min(h, box[3]))
        ink, mean, aa, peak = stats(px, box, ground)
        print(f"{name} box={box} ink={ink} mean={mean:.4f} aa={aa:.4f} peak={peak:.4f}")
        print(f"{name} rows={bands(profile(px, box, ground, 'row'))[:16]}")
        print(f"{name} cols={bands(profile(px, box, ground, 'col'))[:16]}")
PY

# --------------------------------------------------------------------- helpers
shot() {
  # `-o` drops the shadow, so pixel (0,0) is the window's own corner.
  /usr/sbin/screencapture -x -o -l"$WINID" "$SHOTS/$1.png"
  echo "shot $1 rc=$? bytes=$(stat -f %z "$SHOTS/$1.png" 2>/dev/null)"
}

last_labels() {
  /usr/bin/awk '/^--- chrome frame/ { n = NR } { line[NR] = $0 } END { for (i = n; i <= NR; i++) if (line[i] ~ /^label/) print line[i] }' "$DUMP/chrome.dump" 2>/dev/null
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

# ============================================================ ROWS ① ② — the
# folder is in the files column and the .md with a table and a CJK paragraph is
# rendered in the preview.
echo "=================================================== RUN A: table + CJK"
write_session table-cjk.md
launch || { echo "THE APPLICATION NEVER STARTED"; echo "ALL_DONE"; exit 1; }
shot 01-md-table-cjk
echo "--- the labels on the glass (chrome dump, last frame) ---"
last_labels | head -80
echo "--- the files column's foot and the preview rail's crumbs ---"
last_labels | grep -E "pages|~|table-cjk|picture|math|sub" | head -40
echo "--- preview trace ---"
tail -20 "$DUMP/preview.trace" 2>/dev/null
echo "--- the pane's own shell (pty dump) ---"
for d in "$DUMP"/pty.dump*; do
  [ -f "$d" ] || continue
  echo "dump $d ($(stat -f %z "$d") bytes)"
  /usr/bin/strings "$d" | grep -E "M27-" | head -5
done

# ==================================================== ROW ④ — the file is
# replaced from outside the process and the preview follows with no click.
echo "=================================================== ROW: replace, no click"
# The document's own text is not chrome — a Markdown body goes to
# `bt_render::PreviewBody`, not to a label — so what says the page was rebuilt
# is `BT_PREVIEW_TRACE`'s `document bytes=…` station. `# other\n` is 8 bytes,
# and the page it replaced is several hundred, so the number is the event.
/usr/bin/python3 - "$PAGES" "$DUMP/preview.trace" <<'PY'
import os, subprocess, sys, time
pages, trace = sys.argv[1], sys.argv[2]
target = os.path.join(pages, "table-cjk.md")
new = os.path.join(pages, "new")
before = open(trace, "rb").read() if os.path.exists(trace) else b""
print("preview.trace was", len(before), "bytes; last station:",
      before.decode("utf-8", "replace").strip().splitlines()[-1:] )
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
try:
    print(open(trace, "rb").read()[len(before):].decode("utf-8", "replace").strip())
except OSError as error:
    print("no trace:", error)
PY
sleep 2
shot 02-md-replaced
echo "--- labels after the replacement ---"
last_labels | head -40

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
        with open(dump, "rb") as f:
            blob = f.read()
    except OSError:
        blob = b""
    if b"made-by-the-sweep" in blob.rsplit(b"--- chrome frame", 3)[-1]:
        found = time.time() - t0
        break
    time.sleep(0.02)
print("SUBDIR-TREE-LATENCY", "%.3f" % found if found is not None else "NOT SEEN in 25s")
PY
sleep 2
shot 03-tree-subdir
echo "--- the tree's own rows ---"
last_labels | grep -E "sub|before.txt|made-by-the-sweep|picture|math|table" | head -30

# A control: the same create in the *root* of the open folder, which the
# shallow watch is the one that must see.
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
        with open(dump, "rb") as f:
            blob = f.read()
    except OSError:
        blob = b""
    if b"root-level" in blob.rsplit(b"--- chrome frame", 3)[-1]:
        found = time.time() - t0
        break
    time.sleep(0.02)
print("ROOT-TREE-LATENCY", "%.3f" % found if found is not None else "NOT SEEN in 25s")
PY
sleep 2
shot 04-tree-root

# ==================================================== ROW ⑥ — click a .png and
# it shows. The point is read out of the window's own chrome dump.
echo "=================================================== ROW: click the .png"
echo "--- the row's own rectangle, out of the last chrome frame ---"
CLICKPT=$(/usr/bin/python3 - "$DUMP/chrome.dump" "$WINBOX" "$SHOTS/04-tree-root.png" <<'PY'
import re, sys
from PIL import Image
dump, box, shot = sys.argv[1], sys.argv[2], sys.argv[3]
blob = open(dump, "r", errors="replace").read()
frame = blob.rsplit("--- chrome frame", 1)[-1]
rect = None
for line in frame.splitlines():
    if line.startswith("label") and "picture.png" in line:
        nums = re.findall(r"-?\d+\.\d+|-?\d+", line)
        rect = [float(v) for v in nums[:4]]
        print("LABEL-LINE", line.strip(), file=sys.stderr)
        break
if rect is None:
    print("NO-ROW")
    sys.exit(0)
m = re.match(r"(-?\d+),(-?\d+) (\d+)x(\d+)", box)
wx, wy, ww, wh = (int(m.group(i)) for i in (1, 2, 3, 4))
img = Image.open(shot)
scale = img.size[0] / ww
cx = (rect[0] + rect[2]) / 2 / scale + wx
cy = (rect[1] + rect[3]) / 2 / scale + wy
print("%.0f %.0f" % (cx, cy))
PY
)
echo "click point (global points): $CLICKPT"
MOUSE_BEFORE=$(wc -l < "$DUMP/mouse.trace" 2>/dev/null || echo 0)
case "$CLICKPT" in
  NO-ROW|"") echo "the files column never drew a row for picture.png — no click posted" ;;
  *) "$OUT/click" $CLICKPT; sleep 1; "$OUT/click" $CLICKPT ;;
esac
sleep 6
shot 05-png-after-click
echo "--- mouse trace, the lines this click added ---"
tail -n +$((MOUSE_BEFORE + 1)) "$DUMP/mouse.trace" 2>/dev/null | head -30
echo "--- preview trace ---"
tail -6 "$DUMP/preview.trace" 2>/dev/null
echo "--- labels now ---"
last_labels | head -40

end_run

# ==================================================== ROW ⑥′ — the picture,
# reached the way the session reaches it, so the row has an answer either way.
echo "=================================================== RUN B: the .png in the preview"
rm -f "$DUMP/chrome.dump"
write_session picture.png
launch || { echo "RUN B NEVER STARTED"; echo "ALL_DONE"; exit 1; }
shot 06-png
echo "--- preview trace ---"
tail -10 "$DUMP/preview.trace" 2>/dev/null
echo "--- labels ---"
last_labels | head -40
end_run

# ==================================================== ROW ⑦ — the integral is
# typeset, not printed as source.
echo "=================================================== RUN C: the integral"
rm -f "$DUMP/chrome.dump"
write_session math.md
launch || { echo "RUN C NEVER STARTED"; echo "ALL_DONE"; exit 1; }
shot 07-math
echo "--- labels: the source would be in here if it were printed as source ---"
last_labels | head -60
echo "--- does any label carry the raw delimiters? ---"
last_labels | grep -c '\$\$'
echo "--- preview trace ---"
tail -10 "$DUMP/preview.trace" 2>/dev/null
end_run

# ------------------------------------------------------------------ the reads
echo "=================================================== the pixel reads"
# The files column's own width is in the seed (320 logical px); everything to
# the right of it at this backing scale is the preview half. The reads are of
# *line boxes*: how many bands of ink the page stands in and how tall each is,
# which is what says a table has rows, a CJK paragraph has lines that do not
# overlap, and an integral is taller than the prose beside it.
/usr/bin/python3 - "$SHOTS" "$PAGES/picture.png" <<'PY'
import os, sys
from PIL import Image

shots, source = sys.argv[1], sys.argv[2]


def ground_of(img):
    px = img.load()
    w, h = img.size
    return px[6, h - 6]


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


def read(name, left_fraction=0.34):
    path = os.path.join(shots, name + ".png")
    if not os.path.exists(path):
        print(name, "NO SHOT")
        return None
    img = Image.open(path).convert("RGB")
    px = img.load()
    w, h = img.size
    ground = ground_of(img)
    x0 = int(w * left_fraction)
    rows = []
    ink = 0
    total = 0.0
    aa = 0
    for y in range(h):
        n = 0
        for x in range(x0, w):
            r, g, b = px[x, y]
            d = max(abs(r - ground[0]), abs(g - ground[1]), abs(b - ground[2])) / 255.0
            if d > 0.02:
                n += 1
                ink += 1
                total += d
                if 0.05 < d < 0.95:
                    aa += 1
        rows.append(n)
    rb = bands(rows, floor=2)
    print(f"{name}: size={w}x{h} ground={ground} preview half x>={x0}")
    print(f"{name}: ink={ink} mean={(total/ink if ink else 0):.4f} aa={(aa/ink if ink else 0):.4f}")
    print(f"{name}: {len(rb)} line boxes, tallest={max((b[2] for b in rb), default=0)}px")
    print(f"{name}: boxes={rb[:18]}")
    return img, x0, ground


for name in ["01-md-table-cjk", "02-md-replaced", "03-tree-subdir",
             "04-tree-root", "05-png-after-click", "06-png", "07-math"]:
    read(name)
    print()

# The picture, against its own file: if what is on the glass is this picture, the
# mean colour of the region it stands in follows the file's own mean colour.
src = Image.open(source).convert("RGB")
sw, sh = src.size
spx = src.load()
n = 0
acc = [0, 0, 0]
for y in range(0, sh, 4):
    for x in range(0, sw, 4):
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
    ground = ground_of(img)
    x0 = int(w * 0.34)
    # The largest run of rows in the preview half that is inked edge to edge is
    # the picture: prose is inked in words, a photograph is inked all across.
    wide = []
    for y in range(h):
        n = 0
        for x in range(x0, w):
            r, g, b = px[x, y]
            if max(abs(r - ground[0]), abs(g - ground[1]), abs(b - ground[2])) / 255.0 > 0.02:
                n += 1
        wide.append(n)
    span = w - x0
    solid = bands([1 if n > span * 0.55 else 0 for n in wide], floor=1)
    solid.sort(key=lambda b: -b[2])
    print(f"{name}: bands inked across more than half the preview: {solid[:4]}")
    if solid:
        y0, y1, _ = solid[0]
        acc = [0, 0, 0]
        n = 0
        for y in range(y0, y1, 2):
            for x in range(x0, w, 2):
                r, g, b = px[x, y]
                acc[0] += r
                acc[1] += g
                acc[2] += b
                n += 1
        print(f"{name}: that band is y={y0}..{y1} ({y1 - y0}px tall), mean rgb",
              [round(c / n, 1) for c in acc])
PY

echo "=================================================== teardown"
echo "chrome.dump: $(stat -f %z "$DUMP/chrome.dump" 2>/dev/null) bytes"
echo "shots kept at $SHOTS for the transcript; they are deleted by m2-7-teardown.sh"
"$LSREGISTER" -u "$APP"; echo "lsregister -u rc=$?"
for id in "$BID"; do
  rm -rf "$HOME/Library/WebKit/$id" "$HOME/Library/Caches/$id" "$HOME/Library/Saved Application State/$id.savedState"
done
echo "df: $(df -h "$HOME" | tail -1)"
echo "ALL_DONE"
