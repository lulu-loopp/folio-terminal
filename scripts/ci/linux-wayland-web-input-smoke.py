#!/usr/bin/env python3
"""Exercise browser keyboard, composition, caret and popup routes on private Wayland."""

import csv
import importlib.util
import argparse
import json
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import tempfile
import time
from urllib.request import urlopen

repo = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("folio_linux_smoke", repo / "scripts/ci/linux-smoke.py")
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)
probe_spec = importlib.util.spec_from_file_location(
    "folio_linux_web_input_probe", repo / "scripts/ci/linux_web_input_probe.py"
)
web_probe = importlib.util.module_from_spec(probe_spec)
probe_spec.loader.exec_module(web_probe)

parser = argparse.ArgumentParser(description="Probe Folio's native Wayland text-input-v3/IME-v2 path.")
parser.add_argument("--exe", type=Path, default=repo / "target/debug/folio", help="Linux Folio executable")
parser.add_argument("--ime-driver", type=Path, required=True, help="built wayland-ime-driver executable")
parser.add_argument("--web-url", help="root URL for an already-running loopback fixture")
parser.add_argument("--chromium", type=Path, required=True, help="full Chromium build with unpacked MV3 support")
parser.add_argument("--fixture", type=Path, default=repo / "scripts/ci/linux-web-input-fixture.py")


def default_executable(name):
    found = shutil.which(name)
    return Path(found) if found else None


parser.add_argument("--niri", type=Path, default=default_executable("niri"), help="Niri compositor executable")
parser.add_argument("--xvfb", type=Path, default=default_executable("Xvfb"), help="private outer X server")
parser.add_argument("--xdotool", type=Path, default=default_executable("xdotool"), help="XTest keyboard driver")
parser.add_argument("--xdotool-libdir", type=Path, help="library directory for an extracted xdotool binary")
parser.add_argument("--artifacts", type=Path, default=repo / "target/linux-wayland-web-input-smoke")
args = parser.parse_args()
for name in ("niri", "xvfb", "xdotool"):
    if getattr(args, name) is None:
        parser.error(f"{name} executable not found; pass --{name.replace('_', '-')}")

artifacts = args.artifacts
artifacts.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix="session-", dir=artifacts))
root.chmod(0o700)
exe = args.exe.resolve()
ime_driver = args.ime_driver.resolve()
xvfb = args.xvfb.resolve()
niri = args.niri.resolve()
xdotool_bin = args.xdotool.resolve()
processes = []


def start(name, argv, cwd, env, log_path):
    log = log_path.open("wb")
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    processes.append((name, process, log))
    print(f"START {name} pid={process.pid}", flush=True)
    return process


def stop_all():
    for name, process, log in reversed(processes):
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pass
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        if process.poll() is None:
            process.wait()
        log.close()
        print(f"STOP {name} pid={process.pid}", flush=True)
    processes.clear()
    print(f"ARTIFACTS {root}", flush=True)


def run(argv, env, timeout=8):
    return subprocess.run(argv, env=env, capture_output=True, text=True, timeout=timeout)


def focus_state(env):
    result = run([str(niri), "msg", "--json", "windows"], env)
    if result.returncode:
        raise RuntimeError(result.stderr)
    windows = __import__("json").loads(result.stdout)
    return next((window["is_focused"] for window in windows if window.get("app_id") == "io.github.lulu-loopp.folio"), None)


def trace(path):
    return path.read_text(errors="replace") if path.exists() else ""


def pty(path):
    return path.read_bytes() if path.exists() else b""


def wait_until(predicate, process, label, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        if process is not None and process.poll() is not None:
            raise RuntimeError(f"{label}: child exited {process.returncode}")
        time.sleep(0.025)
    raise RuntimeError(f"timed out waiting for {label}")


def wait_count(path, needle, previous_count, process, label, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        current = trace(path).count(needle)
        if current > previous_count:
            return current
        if process.poll() is not None:
            raise RuntimeError(f"Folio exited waiting for {label}: {trace(path)[-1200:]}")
        time.sleep(0.025)
    raise RuntimeError(f"timed out waiting for {label}: {trace(path)[-1500:]}")


def wait_bytes(path, needle, process, label, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        actual = pty(path)
        if needle in actual:
            return actual
        if process.poll() is not None:
            raise RuntimeError(f"Folio exited waiting for {label}: {actual[-800:]!r}")
        time.sleep(0.025)
    raise RuntimeError(f"timed out waiting for {label}: {pty(path)[-800:]!r}")


def caret_areas(path):
    return [tuple(map(int, match)) for match in re.findall(
        r"IME_OUT_AREA x=(-?\d+) y=(-?\d+) width=(\d+) height=(\d+) action=flushed", trace(path)
    )]


def popup_rectangles(path):
    return [tuple(map(int, match)) for match in re.findall(
        r"POPUP_RECT x=(-?\d+) y=(-?\d+) width=(\d+) height=(\d+)", trace(path)
    )]


def web_events():
    with urlopen(args.web_url.rstrip("/") + "/state", timeout=3) as response:
        return json.loads(response.read())["events"]


def wait_web_event(predicate, process, label, timeout=20, after_sequence=0):
    deadline = time.monotonic() + timeout
    last = []
    while time.monotonic() < deadline:
        last = web_events()
        found = next(
            (
                event
                for event in last
                if event.get("sequence", 0) > after_sequence and predicate(event)
            ),
            None,
        )
        if found is not None:
            return found
        if process.poll() is not None:
            raise RuntimeError(f"Folio exited waiting for {label}: {last[-12:]!r}")
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for {label}: {last[-12:]!r}")


def label_position(label, client_env):
    screenshot = root / "browser-input-layout.png"
    captured = run(["/usr/bin/grim", str(screenshot)], client_env)
    if captured.returncode:
        raise RuntimeError(f"grim browser screenshot failed: {captured.stderr}")
    ocr = run(["/usr/bin/tesseract", str(screenshot), "stdout", "--psm", "6", "tsv"], client_env)
    if ocr.returncode:
        raise RuntimeError(f"tesseract browser screenshot failed: {ocr.stderr}")
    grouped = {}
    for row in csv.DictReader(ocr.stdout.splitlines(), delimiter="\t"):
        if row.get("level") != "5" or not row.get("text", "").strip():
            continue
        key = tuple(row.get(name, "") for name in ("block_num", "par_num", "line_num"))
        grouped.setdefault(key, []).append(row)
    wanted = label.casefold().split()
    for rows in grouped.values():
        words = [row["text"].strip().casefold() for row in rows]
        for start in range(len(words) - len(wanted) + 1):
            if words[start : start + len(wanted)] != wanted:
                continue
            matched = rows[start : start + len(wanted)]
            right = max(int(row["left"]) + int(row["width"]) for row in matched)
            top = min(int(row["top"]) for row in matched)
            bottom = max(int(row["top"]) + int(row["height"]) for row in matched)
            return right + 42, (top + bottom) // 2
    raise RuntimeError(f"fixture label {label!r} was not visible in {screenshot}")


def focus_web_field(label, xdotool, xdotool_bin, x11_env, artifacts):
    screenshot_args = argparse.Namespace(
        display=x11_env["DISPLAY"],
        import_bin=Path("/usr/bin/import"),
        tesseract=Path("/usr/bin/tesseract"),
        xdotool=xdotool_bin,
        artifacts=artifacts,
    )
    _, groups = web_probe.screenshot_text(screenshot_args, x11_env)
    x, y = web_probe.locate_label(screenshot_args, x11_env, groups, label)
    xdotool("mousemove", str(x), str(y))
    xdotool("click", "1")


def type_into_web_field(label, element_id, frame, value, xdotool, xdotool_bin, x11_env, artifacts, process):
    focus_web_field(label, xdotool, xdotool_bin, x11_env, artifacts)
    before = len(web_events())
    xdotool("type", "--clearmodifiers", "--delay", "15", value)
    deadline = time.monotonic() + 20
    last = []
    while time.monotonic() < deadline:
        last = web_events()
        recent = last[before:]
        keydowns = [
            event
            for event in recent
            if event.get("frame") == frame
            and event.get("id") == element_id
            and event.get("event") == "keydown"
        ]
        inputs = [
            event
            for event in recent
            if event.get("frame") == frame
            and event.get("id") == element_id
            and event.get("event") == "input"
            and value in str(event.get("value", ""))
        ]
        if keydowns and inputs:
            if not all(event.get("trusted") is True for event in keydowns):
                raise RuntimeError(f"{frame}/{element_id} received untrusted keydowns: {keydowns!r}")
            return {"target": f"{frame}/{element_id}", "keydowns": keydowns, "input": inputs[-1]}
        if process.poll() is not None:
            raise RuntimeError(f"Folio exited waiting for {frame}/{element_id}: {last[-12:]!r}")
        time.sleep(0.05)
    raise RuntimeError(f"{frame}/{element_id} did not receive {value!r}: {last[-12:]!r}")


def web_sequence(frame, element_id):
    return max(
        (
            event.get("sequence", 0)
            for event in web_events()
            if event.get("frame") == frame and event.get("id") == element_id
        ),
        default=0,
    )


def summarize_dom_route(route):
    return {
        "target": route["target"],
        "typed": route["typed"],
        "trusted_keydown_count": len(route["keydowns"]),
        "all_keydowns_trusted": all(event.get("trusted") is True for event in route["keydowns"]),
        "final_value": route["input"].get("value"),
    }


def field_is_focused(frame, element_id):
    focus_events = [
        event
        for event in web_events()
        if event.get("frame") == frame
        and event.get("id") == element_id
        and event.get("event") in ("focusin", "focusout")
    ]
    return bool(focus_events) and focus_events[-1]["event"] == "focusin"


def process_identity(pid):
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except FileNotFoundError:
        return None
    fields = stat[stat.rfind(")") + 2 :].split()
    return fields[0], fields[19]


def wait_reaped(pid, start_time):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        current = process_identity(pid)
        if current is None or current[1] != start_time:
            return
        time.sleep(0.025)
    raise RuntimeError(f"PTY shell child {pid} remains in process table")


display_context = smoke.private_display("x11", root, xvfb)
host_env = display_context.__enter__()
try:
    runtime = Path(host_env["XDG_RUNTIME_DIR"])
    for name in ("NIRI_SOCKET", "NIRI_CONFIG", "WAYLAND_DISPLAY", "WAYLAND_SOCKET"):
        host_env.pop(name, None)
    config = root / "niri.kdl"
    config.write_text("xwayland-satellite {\n    off\n}\nhotkey-overlay { skip-at-startup; }\n")
    niri_env = host_env.copy()
    niri_env["RUST_LOG"] = "niri=info"
    niri_process = start("nested private Niri", [str(niri), "--config", str(config)], root, niri_env, root / "niri.log")

    def find_sockets():
        displays = [path for path in runtime.glob("wayland-*") if path.is_socket()]
        ipcs = sorted(runtime.glob("niri.*.sock"))
        return (displays[0], ipcs[0]) if displays and ipcs else None

    wait_until(lambda: find_sockets() is not None, niri_process, "nested Niri sockets", timeout=30)
    wayland_socket, ipc_socket = find_sockets()
    client_env = host_env.copy()
    client_env.pop("DISPLAY", None)
    client_env.pop("WAYLAND_SOCKET", None)
    client_env.update(
        WAYLAND_DISPLAY=wayland_socket.name,
        NIRI_SOCKET=str(ipc_socket),
        XDG_SESSION_TYPE="wayland",
        WINIT_UNIX_BACKEND="wayland",
    )
    (root / "private-endpoints.txt").write_text(
        f"XDG_RUNTIME_DIR={runtime}\nWAYLAND_DISPLAY={wayland_socket.name}\nNIRI_SOCKET={ipc_socket}\n"
    )
    if args.web_url is None:
        fixture_url_file = root / "fixture-url.txt"
        fixture_env = host_env.copy()
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "XAUTHORITY"):
            fixture_env.pop(name, None)
        fixture = start(
            "loopback browser input fixture",
            [
                sys.executable,
                str(args.fixture.resolve()),
                "--url-file",
                str(fixture_url_file),
                "--events-file",
                str(root / "web-events.jsonl"),
            ],
            root,
            fixture_env,
            root / "web-fixture.log",
        )
        wait_until(
            lambda: fixture_url_file.read_text().strip() if fixture_url_file.exists() else None,
            fixture,
            "loopback browser input fixture",
        )
        args.web_url = fixture_url_file.read_text().strip()
    elif not args.web_url.startswith("http://127.0.0.1:"):
        raise ValueError("the input fixture must be served from 127.0.0.1")
    info = run(["/usr/bin/wayland-info"], client_env)
    (root / "wayland-input-method-globals.txt").write_text(info.stdout + info.stderr)
    if info.returncode or "zwp_text_input_manager_v3" not in info.stdout or "zwp_input_method_manager_v2" not in info.stdout:
        raise RuntimeError(f"private Niri lacks IME v2/text-input v3 globals: {info.stdout[-1200:]}")
    print("PASS private Niri advertises text-input-v3 and input-method-v2", flush=True)

    x11_env = host_env.copy()
    if args.xdotool_libdir:
        old_library_path = x11_env.get("LD_LIBRARY_PATH", "")
        x11_env["LD_LIBRARY_PATH"] = os.pathsep.join(
            value for value in (str(args.xdotool_libdir.resolve()), old_library_path) if value
        )

    def xdotool(*args):
        result = run([str(xdotool_bin), *args], x11_env)
        if result.returncode:
            raise RuntimeError(f"xdotool {' '.join(args)} failed: {result.stderr}")
        return result.stdout.strip()

    shell_pid = root / "shell.pid"
    pty_path = root / "pty.dump"
    ime_trace = root / "ime.trace"
    shell = root / "probe-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "test -t 0 && test -t 1 || exit 9\n"
        "printf 'FOLIO_WAYLAND_PTY_READY '; stty size\n"
        "printf '%s\\n' \"$$\" > \"$FOLIO_CHILD_PID\"\n"
        "exec /bin/bash --noprofile --norc -i\n"
    )
    shell.chmod(0o700)
    app_env = client_env.copy()
    app_env.update(
        SHELL=str(shell),
        FOLIO_CHILD_PID=str(shell_pid),
        TERM="xterm-256color",
        LC_ALL="C.UTF-8",
        HISTFILE="/dev/null",
        BT_STARTUP_TRACE="1",
        BT_PTY_DUMP=str(pty_path),
        BT_IME_TRACE=str(ime_trace),
        BT_WEB_TRACE=str(root / "web.trace"),
        BT_CHROME_DUMP=str(root / "chrome.dump"),
        BT_WEB_DEV=args.web_url,
        FOLIO_CHROMIUM_PATH=str(args.chromium.resolve()),
        NO_PROXY="127.0.0.1,127.0.0.2,localhost",
        no_proxy="127.0.0.1,127.0.0.2,localhost",
    )
    folio = start("Folio native Wayland", [str(exe), "--profile", "usershell", "--cwd", str(root)], root, app_env, root / "folio.log")
    wait_until(lambda: b"FOLIO_WAYLAND_PTY_READY" in pty(pty_path), folio, "Folio PTY readiness", timeout=60)
    wait_until(lambda: "BT_STARTUP first_text_present=" in trace(root / "folio.log"), folio, "Folio rendered PTY text", timeout=60)

    windows = xdotool("search", "--onlyvisible", "--name", ".")
    if not windows:
        raise RuntimeError("private Niri outer window was not visible in Xvfb")
    niri_window = windows.splitlines()[0]
    xdotool("windowfocus", "--sync", niri_window)
    if xdotool("getwindowfocus") != niri_window:
        raise RuntimeError("private Niri outer window did not receive XTest focus")
    xdotool("key", "Escape")
    wait_until(lambda: "new=Preview previous_cause=first_run" in trace(ime_trace), folio, "Folio first-run dismissal")
    print("PASS XTest key reached focused private Niri", flush=True)

    probe_args = argparse.Namespace(
        display=x11_env["DISPLAY"],
        window=niri_window,
        url=args.web_url,
        xdotool=xdotool_bin,
        import_bin=Path("/usr/bin/import"),
        tesseract=Path("/usr/bin/tesseract"),
        artifacts=root,
        focus_label="Top input",
        quiet=True,
    )
    raw_dom = web_probe.run_probe(probe_args, env=x11_env)
    top_keys = raw_dom["top_input"]
    oopif_keys = raw_dom["oopif_input"]
    top_keyboard = summarize_dom_route(top_keys)
    oopif_keyboard = summarize_dom_route(oopif_keys)
    print(
        f"PASS XTest DOM keyboard route: top={top_keyboard}; oopif={oopif_keyboard}",
        flush=True,
    )
    focus_web_field("Top input", xdotool, xdotool_bin, x11_env, root)
    wait_until(lambda: field_is_focused("top", "input"), folio, "top browser input focus")
    wait_until(
        lambda: "new=Preview" in trace(ime_trace),
        folio,
        "browser input field to own the Wayland IME",
    )

    driver = start("input-method-v2 protocol peer", [str(ime_driver)], root, client_env, root / "ime-driver.log")
    wait_until(lambda: "READY" in trace(root / "ime-driver.log"), driver, "input-method-v2 peer registration")
    wait_until(lambda: "ACTIVE" in trace(root / "ime-driver.log"), folio, "input-method activation for focused Folio")

    wait_until(lambda: bool(caret_areas(ime_trace)), folio, "initial native IME caret area")
    preedit_count = len(re.findall(r"IME_IN kind=Preedit bytes=[1-9]\d*", trace(ime_trace)))
    pty_before_preedit = pty(pty_path)
    caret_before_preedit = caret_areas(ime_trace)[-1]
    browser_sequence_before_preedit = web_sequence("top", "input")
    xdotool("type", "--clearmodifiers", "n")
    wait_count(root / "ime-driver.log", "PREEDIT_SENT", 0, driver, "protocol preedit request")
    wait_count(ime_trace, "IME_IN kind=Preedit bytes=5", preedit_count, folio, "text-input-v3 preedit")
    if pty(pty_path) != pty_before_preedit:
        raise RuntimeError("preedit key or composition leaked into PTY output")
    composition_start = wait_web_event(
        lambda event: event.get("frame") == "top"
        and event.get("id") == "input"
        and event.get("event") == "compositionstart",
        folio,
        "browser DOM compositionstart",
        after_sequence=browser_sequence_before_preedit,
    )
    preedit = wait_web_event(
        lambda event: event.get("frame") == "top"
        and event.get("id") == "input"
        and event.get("event") == "compositionupdate"
        and event.get("data") == "nihao",
        folio,
        "browser DOM compositionupdate",
        after_sequence=composition_start["sequence"],
    )
    wait_count(root / "ime-driver.log", "POPUP_RECT", 0, driver, "input popup caret rectangle")
    areas = caret_areas(ime_trace)
    if not areas or not any(width > 0 and height > 0 for _, _, width, height in areas):
        raise RuntimeError(f"no nonempty candidate caret area during Wayland preedit: {trace(ime_trace)[-1200:]}")
    wait_until(
        lambda: bool(caret_areas(ime_trace)) and caret_areas(ime_trace)[-1] != caret_before_preedit,
        folio,
        "caret to move to the preedit cursor",
    )
    preedit_caret = caret_areas(ime_trace)[-1]
    wait_until(
        lambda: preedit_caret in popup_rectangles(root / "ime-driver.log"),
        driver,
        "compositor popup to follow the preedit caret",
    )
    matched_popup_rect = preedit_caret
    print(
        json.dumps(
            {
                "preedit_event": preedit,
                "composition_start": composition_start,
                "caret_before": caret_before_preedit,
                "caret_during_preedit": preedit_caret,
                "top_keyboard": top_keyboard,
                "oopif_keyboard": oopif_keyboard,
            },
            ensure_ascii=False,
        ),
        flush=True,
    )
    screenshot = run(["/usr/bin/grim", str(root / "preedit.png")], client_env)
    if screenshot.returncode:
        raise RuntimeError(f"grim preedit screenshot failed: {screenshot.stderr}")
    print(f"PASS compositor popup rect={matched_popup_rect} matched the latest caret; screenshot={root / 'preedit.png'}", flush=True)

    commit_count = trace(ime_trace).count("IME_IN kind=Commit bytes=6")
    browser_sequence_before_commit = web_sequence("top", "input")
    xdotool("key", "--clearmodifiers", "space")
    wait_count(root / "ime-driver.log", "COMMIT_SENT", 0, driver, "protocol commit request")
    wait_count(ime_trace, "IME_IN kind=Commit bytes=6", commit_count, folio, "text-input-v3 committed text")
    committed = wait_web_event(
        lambda event: event.get("frame") == "top"
        and event.get("id") == "input"
        and event.get("event") == "input"
        and "你好" in str(event.get("value", "")),
        folio,
        "Chinese commit in the browser input",
        after_sequence=browser_sequence_before_commit,
    )
    composition_end = wait_web_event(
        lambda event: event.get("frame") == "top"
        and event.get("id") == "input"
        and event.get("event") == "compositionend",
        folio,
        "browser DOM compositionend",
        after_sequence=composition_start["sequence"],
    )
    if "你好" in pty(pty_path).decode(errors="replace"):
        raise RuntimeError("browser IME commit leaked into PTY output")
    print(
        json.dumps(
            {
                "composition_end": composition_end,
                "committed_dom_input": committed,
                "pty_contains_commit": False,
            },
            ensure_ascii=False,
        ),
        flush=True,
    )

    blurred = trace(ime_trace).count("IME_OUT_CARET action=destroy reason=window_blur")
    deactivated = trace(root / "ime-driver.log").count("DEACTIVATE")
    run([str(niri), "msg", "action", "focus-workspace", "2"], client_env)
    wait_until(lambda: not focus_state(client_env), folio, "private Niri workspace blur")
    wait_count(ime_trace, "IME_OUT_CARET action=destroy reason=window_blur", blurred, folio, "candidate caret withdrawal")
    wait_count(root / "ime-driver.log", "DEACTIVATE", deactivated, driver, "input-method deactivation on blur")
    print("PASS blur withdrew the native caret and deactivated the input method", flush=True)

    active_count = trace(root / "ime-driver.log").count("ACTIVE")
    preedit_count = len(re.findall(r"IME_IN kind=Preedit bytes=[1-9]\d*", trace(ime_trace)))
    commit_count = trace(ime_trace).count("IME_IN kind=Commit bytes=6")
    composition_count = sum(
        event.get("event") == "compositionupdate" for event in web_events()
    )
    run([str(niri), "msg", "action", "focus-workspace", "1"], client_env)
    wait_until(lambda: focus_state(client_env), folio, "private Niri workspace focus restoration")
    wait_count(root / "ime-driver.log", "ACTIVE", active_count, driver, "input-method reactivation")
    xdotool("windowfocus", "--sync", niri_window)
    if xdotool("getwindowfocus") != niri_window:
        raise RuntimeError("private Niri lost its outer XTest focus after workspace restoration")
    area_count_before_oopif = len(caret_areas(ime_trace))
    popup_count_before_oopif = len(popup_rectangles(root / "ime-driver.log"))
    focus_web_field("OOPIF input", xdotool, xdotool_bin, x11_env, root)
    wait_web_event(
        lambda event: event.get("frame") == "oopif-127.0.0.2"
        and event.get("id") == "oopif-input"
        and event.get("event") == "focusin",
        folio,
        "OOPIF input focus after window refocus",
    )
    wait_until(
        lambda: len(caret_areas(ime_trace)) > area_count_before_oopif,
        folio,
        "browser engine caret after OOPIF focus",
    )
    caret_before_oopif_preedit = caret_areas(ime_trace)[-1]
    browser_sequence_before_resume = web_sequence("oopif-127.0.0.2", "oopif-input")
    xdotool("type", "--clearmodifiers", "n")
    wait_count(ime_trace, "IME_IN kind=Preedit bytes=5", preedit_count, folio, "preedit after restored focus")
    oopif_preedit = wait_web_event(
        lambda event: event.get("frame") == "oopif-127.0.0.2"
        and event.get("id") == "oopif-input"
        and event.get("event") == "compositionupdate"
        and event.get("data") == "nihao",
        folio,
        "OOPIF composition after restored focus",
        after_sequence=browser_sequence_before_resume,
    )
    wait_until(
        lambda: bool(caret_areas(ime_trace))
        and caret_areas(ime_trace)[-1] != caret_before_oopif_preedit,
        folio,
        "caret update from the OOPIF preedit cursor",
    )
    oopif_caret = caret_areas(ime_trace)[-1]
    wait_until(
        lambda: oopif_caret in popup_rectangles(root / "ime-driver.log")
        and len(popup_rectangles(root / "ime-driver.log")) > popup_count_before_oopif,
        driver,
        "input popup to follow the OOPIF caret",
    )
    browser_sequence_before_resume_commit = web_sequence("oopif-127.0.0.2", "oopif-input")
    xdotool("key", "--clearmodifiers", "space")
    wait_count(ime_trace, "IME_IN kind=Commit bytes=6", commit_count, folio, "commit after restored focus")
    resumed_commit = wait_web_event(
        lambda event: event.get("frame") == "oopif-127.0.0.2"
        and event.get("id") == "oopif-input"
        and event.get("event") == "input"
        and "你好" in str(event.get("value", "")),
        folio,
        "OOPIF browser commit after restored focus",
        after_sequence=browser_sequence_before_resume_commit,
    )
    if composition_count == 0:
        raise RuntimeError("first browser composition was not recorded before the focus cycle")
    if "你好" in pty(pty_path).decode(errors="replace"):
        raise RuntimeError("OOPIF browser commit leaked into PTY output")
    print(
        json.dumps(
            {
                "oopif_preedit": oopif_preedit,
                "oopif_caret": oopif_caret,
                "oopif_commit": resumed_commit,
                "pty_contains_commit": False,
            },
            ensure_ascii=False,
        ),
        flush=True,
    )

    shell_process = int(shell_pid.read_text().strip())
    shell_start_time = process_identity(shell_process)[1]
    run([str(niri), "msg", "action", "close-window"], client_env)
    try:
        folio.wait(timeout=15)
    except subprocess.TimeoutExpired:
        raise RuntimeError("Folio did not close after compositor close-window")
    wait_reaped(shell_process, shell_start_time)
    print("PASS native Wayland window close reaped the PTY shell", flush=True)
finally:
    stop_all()
    display_context.__exit__(None, None, None)
