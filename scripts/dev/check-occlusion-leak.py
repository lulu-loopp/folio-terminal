#!/usr/bin/env python3
"""M1.9u red gate ①: a rendered live math block whose occluded tail rows still
expose formula source in the visible cell plane (image233 "half render + source
tail"; the Jump chip on those rows is image234/235's ②a).

Feed it the BT_PROBE_VERBOSE stderr dump of bt-repaint-oracle (### FRAME / block[]
/ row[] lines). A leak = a visible row whose index is strictly below a
Rendered+occluded block's band_end and within its occluded span, yet still carries
formula-ish source tokens (backslash command, ` & ` alignment, $$, \\begin/\\end).

Run:
  BT_PROBE_VERBOSE=1 BT_PROBE_INPUT=.tmp-repaint-capture/cc-topbot.vt \
    BT_PROBE_CHUNKS=.tmp-repaint-capture/cc-topbot.vt.chunks \
    BT_PROBE_COLUMNS=106 BT_PROBE_ROWS=33 \
    cargo run --locked --offline -p bt-term --bin bt-repaint-oracle \
    1>trace.txt 2>verbose.txt
  python scripts/dev/check-occlusion-leak.py verbose.txt   # exit 0 = clean, 1 = leak

Baseline (M1.9t, HEAD b9faf31): 28 frames / 30 leak rows. Target after M1.9u ①: 0.
"""
import re, sys

FRAME_RE = re.compile(r"^### FRAME (\d+)")
BLOCK_RE = re.compile(
    r'^  block\[(\d+)\] display=(\w+) band=(-?\d+)\.\.=(-?\d+) '
    r'occluded=(\d+) occ_id=(\S+) src="(.*)"$'
)
ROW_RE = re.compile(r"^  row\[\s*(\d+)\] \|(.*)$")


def looks_like_math_source(text):
    t = text.strip()
    if not t:
        return False
    if "$$" in t or r"\begin{" in t or r"\end{" in t:
        return True
    if " & " in t or t.endswith("&") or " &= " in t:
        return True
    # backslash followed by a latex letter command (\nabla, \frac, \mathbf, \qquad ...)
    if re.search(r"\\[A-Za-z]{2,}", t):
        return True
    return False


def main(path):
    frame = None
    blocks = []
    rows = {}
    leaks = []

    def flush():
        for (_idx, disp, _bs, be, occ, occid, _src) in blocks:
            if disp != "Rendered" or occ <= 0:
                continue
            span_end = be + occ
            for r, text in rows.items():
                if be < r <= span_end and looks_like_math_source(text):
                    leaks.append((frame, occid, be, occ, r, text.strip()))

    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.rstrip("\n")
            m = FRAME_RE.match(line)
            if m:
                if frame is not None:
                    flush()
                frame, blocks, rows = int(m.group(1)), [], {}
                continue
            m = BLOCK_RE.match(line)
            if m:
                blocks.append(
                    (int(m.group(1)), m.group(2), int(m.group(3)),
                     int(m.group(4)), int(m.group(5)), m.group(6), m.group(7))
                )
                continue
            m = ROW_RE.match(line)
            if m:
                rows[int(m.group(1))] = m.group(2)
        if frame is not None:
            flush()

    frames_with_leak = len({l[0] for l in leaks})
    print(
        f"OCCLUSION-LEAK frames-with-leak={frames_with_leak} "
        f"total-leak-rows={len(leaks)}"
    )
    for (fr, occid, be, occ, r, text) in leaks[:25]:
        print(
            f"  frame={fr} block(occ_id={occid} band_end={be} occluded={occ}) "
            f"leaks row[{r}]: {text[:80]}"
        )
    sys.exit(1 if leaks else 0)


if __name__ == "__main__":
    main(sys.argv[1])
