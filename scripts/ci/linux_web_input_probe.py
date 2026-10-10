#!/usr/bin/env python3
"""Send XTest keys through a private X11 surface into Chromium form controls."""

import argparse
import csv
import json
import os
from pathlib import Path
import subprocess
import time
from urllib.request import ProxyHandler, build_opener


def run(args, env):
    result = subprocess.run(args, env=env, capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise RuntimeError(f"{args!r} failed: {result.stderr.strip()}")
    return result.stdout


def screenshot_text(args, env):
    screenshot = args.artifacts / "screen-before-input.png"
    subprocess.run(
        [args.import_bin, "-display", args.display, "-window", "root", str(screenshot)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        check=True,
        timeout=10,
    )
    result = subprocess.run(
        [args.tesseract, str(screenshot), "stdout", "--psm", "6", "tsv"],
        capture_output=True,
        text=True,
        timeout=10,
        check=True,
    )
    groups = {}
    for row in csv.DictReader(result.stdout.splitlines(), delimiter="\t"):
        if row.get("level") != "5" or not row.get("text", "").strip():
            continue
        key = tuple(row.get(name, "") for name in ("block_num", "par_num", "line_num"))
        groups.setdefault(key, []).append(row)
    return screenshot, groups


def label_point(groups, phrase):
    wanted = phrase.casefold().split()
    for rows in groups.values():
        words = [row["text"].strip().casefold() for row in rows]
        for start in range(len(words) - len(wanted) + 1):
            if words[start : start + len(wanted)] == wanted:
                matched = rows[start : start + len(wanted)]
                right = max(int(row["left"]) + int(row["width"]) for row in matched)
                top = min(int(row["top"]) for row in matched)
                bottom = max(int(row["top"]) + int(row["height"]) for row in matched)
                return right + 42, (top + bottom) // 2
    raise RuntimeError(f"page label {phrase!r} was not visible in the private Xorg screenshot")


def locate_label(args, env, groups, phrase):
    try:
        return label_point(groups, phrase)
    except RuntimeError:
        # The nested OOPIF sits below the same-origin frame. Scroll the top page
        # from its visible right margin, outside the iframe, then OCR the new view.
        for _ in range(4):
            run([args.xdotool, "mousemove", "1100", "600"], env)
            run([args.xdotool, "click", "--repeat", "5", "--delay", "50", "5"], env)
            _, groups = screenshot_text(args, env)
            try:
                return label_point(groups, phrase)
            except RuntimeError:
                pass
        raise RuntimeError(f"page label {phrase!r} stayed outside the visible fixture after scrolling")


def events(url):
    with build_opener(ProxyHandler({})).open(url.rstrip("/") + "/state", timeout=3) as response:
        return json.loads(response.read())["events"]


def wait_for_value(url, element, frame, expected, timeout=15):
    deadline = time.monotonic() + timeout
    last = []
    while time.monotonic() < deadline:
        last = events(url)
        for event in last:
            if (
                event.get("frame") == frame
                and event.get("id") == element
                and event.get("event") == "input"
                and expected in str(event.get("value", ""))
            ):
                return last, event
        time.sleep(0.05)
    raise RuntimeError(f"{frame}/{element} did not receive {expected!r}; last events={last[-12:]!r}")


def type_at(args, env, groups, phrase, element, frame, text):
    x, y = locate_label(args, env, groups, phrase)
    run([args.xdotool, "mousemove", "--sync", str(x), str(y)], env)
    run([args.xdotool, "click", "1"], env)
    before = len(events(args.url))
    run([args.xdotool, "type", "--clearmodifiers", "--delay", "15", text], env)
    all_events, received = wait_for_value(args.url, element, frame, text)
    added = all_events[before:]
    keydowns = [
        event
        for event in added
        if event.get("frame") == frame
        and event.get("id") == element
        and event.get("event") == "keydown"
    ]
    if not keydowns or not all(event.get("trusted") is True for event in keydowns):
        raise RuntimeError(f"{frame}/{element} did not receive trusted DOM keydowns: {keydowns!r}")
    return {
        "target": f"{frame}/{element}",
        "typed": text,
        "keydowns": keydowns,
        "input": received,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--display", required=True, help="the owned X11 display, such as :7")
    parser.add_argument("--window", required=True, help="Folio window or nested compositor window id")
    parser.add_argument("--url", required=True, help="the fixture's loopback root URL")
    parser.add_argument("--xdotool", type=Path, required=True)
    parser.add_argument("--import-bin", type=Path, default=Path("/usr/bin/import"))
    parser.add_argument("--tesseract", type=Path, default=Path("/usr/bin/tesseract"))
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--focus-label", help="leave the pointer and keyboard focused on this field")
    args = parser.parse_args()
    run_probe(args)


def run_probe(args, env=None):
    args.artifacts.mkdir(parents=True, exist_ok=True)
    env = env or os.environ.copy()
    for name in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "XAUTHORITY"):
        env.pop(name, None)
    env["DISPLAY"] = args.display

    focus = run([args.xdotool, "getwindowfocus"], env).strip()
    if focus != args.window:
        run([args.xdotool, "windowactivate", "--sync", args.window], env)
        focus = run([args.xdotool, "getwindowfocus"], env).strip()
    if focus != args.window:
        raise RuntimeError(f"Xorg focus is window {focus}, expected Folio {args.window}")

    screenshot, groups = screenshot_text(args, env)
    top = type_at(args, env, groups, "Top input", "input", "top", "FOLIO_XORG_KEY_OK")

    screenshot, groups = screenshot_text(args, env)
    oopif = type_at(
        args,
        env,
        groups,
        "OOPIF input",
        "oopif-input",
        "oopif-127.0.0.2",
        "FOLIO_OOPIF_KEY_OK",
    )
    if args.focus_label:
        _, groups = screenshot_text(args, env)
        x, y = locate_label(args, env, groups, args.focus_label)
        run([args.xdotool, "mousemove", "--sync", str(x), str(y)], env)
        run([args.xdotool, "click", "1"], env)
    if not getattr(args, "quiet", False):
        print(
            json.dumps(
                {
                    "screenshot": str(screenshot),
                    "top_frame": top,
                    "oopif_frame": oopif,
                    "result": "XTest keydowns and committed input reached the focused DOM controls",
                },
                ensure_ascii=False,
                indent=2,
            ),
            flush=True,
        )
    return {"top_input": top, "oopif_input": oopif}


if __name__ == "__main__":
    main()
