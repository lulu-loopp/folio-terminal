#!/usr/bin/env python3
"""Per-source RENDERED<->SOURCE flip trace for bt-repaint-oracle stdout.

Judge criterion for the formula flash work: the number of `RENDERED -> SOURCE`
flips per distinct block source (0 == no flash). Unlike the oracle EXIT code
(which has diagnostic exemptions), this walks every frame line and tracks each
source string's rendered/source presence, reporting exactly which frame a block
reverts from rendered to source.

Usage:
    bt-repaint-oracle ... > frames.txt
    python scripts/dev/trace_blocks.py frames.txt [--source SUBSTR]
"""
import ast
import re
import sys

FRAME_RE = re.compile(
    r"frame=(\d+).*?rendered=(\[.*?\]) source_rows=(\[.*?\]) occluded=(\[.*?\])"
    r"(?:.*?source_plane=(\"(?:[^\"\\]|\\.)*\"))?"
)

_UNESCAPE = {
    "n": "\n", "t": "\t", "r": "\r", "0": "\0", '"': '"', "\\": "\\", "'": "'",
}


def _decode_rust_debug(literal):
    """Decode a Rust `{:?}` string literal (surrounding quotes included) to text.

    Rust Debug keeps printable Unicode (CJK, math) literal and only escapes control
    characters, quotes and backslashes, so a small hand-rolled unescape is enough and
    avoids ast.literal_eval choking on `\\u{...}` forms.
    """
    body = literal[1:-1]
    out = []
    i = 0
    while i < len(body):
        ch = body[i]
        if ch != "\\":
            out.append(ch)
            i += 1
            continue
        nxt = body[i + 1] if i + 1 < len(body) else ""
        if nxt == "u" and i + 2 < len(body) and body[i + 2] == "{":
            end = body.index("}", i + 3)
            out.append(chr(int(body[i + 3:end], 16)))
            i = end + 1
        else:
            out.append(_UNESCAPE.get(nxt, nxt))
            i += 2
    return "".join(out)


def norm(s):
    return " ".join(s.split())


def parse(path):
    frames = []
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = FRAME_RE.search(line)
            if not m:
                continue
            frame = int(m.group(1))
            rendered = [norm(x) for x in ast.literal_eval(m.group(2))]
            source_rows = [norm(x) for x in ast.literal_eval(m.group(3))]
            occluded = [norm(x) for x in ast.literal_eval(m.group(4))]
            source_plane = _decode_rust_debug(m.group(5)) if m.group(5) else ""
            frames.append((frame, rendered, source_rows, occluded, source_plane))
    return frames


def _environment_name(source):
    head = source.split(r"\begin{", 1)
    if len(head) < 2:
        return None
    name = head[1].split("}", 1)
    return name[0] if len(name) == 2 else None


def source_exposes(source, source_rows, source_plane):
    """Mirror bt-term's `source_rows_expose`: a rendered source is exposed if a bare row equals
    its body/delimited form, or — for a multi-line block whose delimiter-free body rows are
    dropped from `source_rows` — if the whole cell plane still contains the environment or the
    delimited body."""
    body = source.strip()
    if not body:
        return False
    for row in source_rows:
        r = row.strip()
        if not r:
            continue
        if r == body:
            return True
        if r.startswith("$$") and r.endswith("$$") and len(r) > 4 and r[2:-2].strip() == body:
            return True
        if r.startswith(r"\[") and r.endswith(r"\]") and len(r) > 4 and r[2:-2].strip() == body:
            return True
    plane = " ".join(source_plane.split()) if source_plane else ""
    env = _environment_name(body)
    if env and (r"\begin{%s}" % env) in source and \
            (r"\begin{%s}" % env) in source_plane and (r"\end{%s}" % env) in source_plane:
        return True
    # Multi-line delimiter-on-its-own-line body: the plane retains rows source_rows drops. Match
    # the normalized (whitespace-collapsed) body against the normalized plane.
    if plane:
        nbody = " ".join(body.split())
        for delimited in (f"$$ {nbody} $$", f"$${nbody}$$",
                          r"\[ %s \]" % nbody, r"\[%s\]" % nbody):
            if " ".join(delimited.split()) in plane:
                return True
    return False


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    path = sys.argv[1]
    filt = None
    if "--source" in sys.argv:
        filt = sys.argv[sys.argv.index("--source") + 1]
    frames = parse(path)
    if not frames:
        print("no frames parsed")
        sys.exit(1)

    # Track, per source string, last known display state across frames.
    RENDERED, SOURCE = "R", "S"
    state = {}          # source -> last state
    flips = {}          # source -> count of R->S transitions
    flip_frames = {}    # source -> list of frames where R->S happened
    ever_rendered = set()

    for (frame, rendered, source_rows, occluded, source_plane) in frames:
        rendered_set = set(rendered)
        occluded_set = set(occluded)
        ever_rendered |= rendered_set
        # Update rendered sources -> R
        for s in rendered_set:
            state[s] = RENDERED
        # For every source ever rendered, check exposure this frame.
        for s in list(ever_rendered):
            if s in rendered_set:
                continue
            if s in occluded_set:
                continue
            if source_exposes(s, source_rows, source_plane):
                if state.get(s) == RENDERED:
                    flips[s] = flips.get(s, 0) + 1
                    flip_frames.setdefault(s, []).append(frame)
                state[s] = SOURCE

    last_frame = frames[-1]
    print(f"frames parsed: {len(frames)}  last frame index: {last_frame[0]}")
    print(f"distinct sources ever rendered: {len(ever_rendered)}")
    print("--- final frame ---")
    print(f"  rendered ({len(last_frame[1])}):")
    for s in last_frame[1]:
        print(f"    R {s[:70]}")
    print(f"  source_rows ({len(last_frame[2])}): {last_frame[2]}")
    print("--- R->S flips per source ---")
    total = 0
    for s in sorted(ever_rendered):
        if filt and filt not in s:
            continue
        n = flips.get(s, 0)
        total += n
        mark = "" if n == 0 else f"  <-- flips at {flip_frames[s]}"
        finalst = "RENDERED" if state.get(s) == RENDERED else "SOURCE"
        print(f"  flips={n} final={finalst:8} {s[:60]}{mark}")
    print(f"TOTAL R->S flips: {total}")


if __name__ == "__main__":
    main()
