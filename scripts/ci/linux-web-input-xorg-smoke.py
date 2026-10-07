#!/usr/bin/env python3
"""Check loopback browser DOM keyboard and XIM input on an isolated Xorg session."""

import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from urllib.request import ProxyHandler, build_opener


REPO = Path(__file__).resolve().parents[2]
SMOKE_PATH = REPO / "scripts/ci/linux-input-smoke.py"
PROBE_PATH = REPO / "scripts/ci/linux_web_input_probe.py"


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


smoke = load_module("folio_linux_input_smoke", SMOKE_PATH)
probe = load_module("folio_linux_web_input_probe", PROBE_PATH)


def run(command, env, timeout=10):
    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"{command!r} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def fixture_events(url):
    with build_opener(ProxyHandler({})).open(url.rstrip("/") + "/state", timeout=3) as response:
        return json.loads(response.read())["events"]


def wait_web_event(url, predicate, app, label, timeout=20):
    deadline = time.monotonic() + timeout
    last = []
    while time.monotonic() < deadline:
        last = fixture_events(url)
        found = next((event for event in last if predicate(event)), None)
        if found is not None:
            return found
        if app.poll() is not None:
            raise RuntimeError(f"Folio exited waiting for {label}: {last[-12:]!r}")
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for {label}: {last[-12:]!r}")


def wait_trace(path, predicate, app, label, timeout=20):
    deadline = time.monotonic() + timeout
    last = ""
    while time.monotonic() < deadline:
        last = path.read_text(errors="replace") if path.exists() else ""
        if predicate(last):
            return last
        if app.poll() is not None:
            raise RuntimeError(f"Folio exited waiting for {label}: {last[-1600:]}")
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for {label}: {last[-1600:]}")


def dismiss_browser_first_run(trace_path, process, log_path, xdotool, env):
    started = time.monotonic()
    while time.monotonic() - started < 15:
        trace = trace_path.read_text(errors="replace") if trace_path.exists() else ""
        lines = trace.splitlines()
        modal = next(
            (index for index, line in enumerate(lines) if "new=Modal" in line and "cause=first_run" in line),
            None,
        )
        if modal is not None:
            if any(
                "new=Preview" in line and "previous_cause=first_run" in line
                for line in lines[modal + 1 :]
            ):
                return
            smoke.run_tool([xdotool, "key", "--clearmodifiers", "Escape"], env)
            break
        if process.poll() is not None:
            raise RuntimeError(
                f"Folio exited before the browser first-run modal appeared: "
                f"{log_path.read_text(errors='replace')[-1600:]}"
            )
        time.sleep(0.025)
    else:
        raise RuntimeError(f"browser first-run modal did not appear: {trace[-1200:]!r}")

    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        trace = trace_path.read_text(errors="replace") if trace_path.exists() else ""
        lines = trace.splitlines()
        modal = next(
            (index for index, line in enumerate(lines) if "new=Modal" in line and "cause=first_run" in line),
            None,
        )
        if modal is not None and any(
            "new=Preview" in line and "previous_cause=first_run" in line
            for line in lines[modal + 1 :]
        ):
            print("PASS first-run Escape returned keyboard ownership to the browser preview", flush=True)
            return
        if process.poll() is not None:
            break
        time.sleep(0.025)
    raise RuntimeError(
        f"Escape did not return keyboard ownership to the browser preview: "
        f"IME trace={trace[-1200:]!r}; log={log_path.read_text(errors='replace')[-1200:]}"
    )


def start_ibus(env, root, daemon_path, panel_path):
    private_socket_dir = Path(tempfile.mkdtemp(prefix="wi-bus-", dir="/tmp"))
    address_socket = private_socket_dir / "ibus.sock"
    daemon, log = smoke.start_logged(
        "private IBus XIM",
        [
            str(daemon_path),
            "--replace",
            "--xim",
            "--verbose",
            f"--panel={panel_path}",
            "--emoji-extension=disable",
            "--config=/usr/libexec/ibus-dconf",
            f"--address=unix:path={address_socket}",
        ],
        root,
        env,
    )
    address_root = Path(env["XDG_CONFIG_HOME"]) / "ibus/bus"
    deadline = time.monotonic() + 20
    bus_address = None
    try:
        while time.monotonic() < deadline:
            for address_file in address_root.glob("*") if address_root.is_dir() else ():
                for line in address_file.read_text(errors="replace").splitlines():
                    if line.startswith("IBUS_ADDRESS="):
                        bus_address = line.split("=", 1)[1]
                        break
                if bus_address:
                    response = subprocess.run(
                        [
                            "gdbus",
                            "call",
                            "--address",
                            bus_address,
                            "--dest",
                            "org.freedesktop.IBus",
                            "--object-path",
                            "/org/freedesktop/IBus",
                            "--method",
                            "org.freedesktop.IBus.GetUseGlobalEngine",
                        ],
                        env=env,
                        capture_output=True,
                        text=True,
                        timeout=3,
                    )
                    if response.returncode == 0:
                        env["IBUS_ADDRESS"] = bus_address
                        select_ibus_engine(env, bus_address, "xkb:us::eng")
                        print("PASS private IBus XIM bus is ready", flush=True)
                        return daemon, log, private_socket_dir, bus_address
            if daemon.poll() is not None:
                raise RuntimeError(f"IBus exited: {log_path_text(root / 'ibus-daemon.log')}")
            time.sleep(0.05)
        raise RuntimeError(f"private IBus bus did not become ready: {log_path_text(root / 'ibus-daemon.log')}")
    except BaseException:
        smoke.stop_process(daemon, "private IBus XIM")
        log.close()
        shutil.rmtree(private_socket_dir, ignore_errors=True)
        raise


def log_path_text(path):
    return path.read_text(errors="replace")[-3000:] if path.exists() else ""


def summarize_dom_route(route):
    return {
        "target": route["target"],
        "typed": route["typed"],
        "trusted_keydown_count": len(route["keydowns"]),
        "all_keydowns_trusted": all(event.get("trusted") is True for event in route["keydowns"]),
        "final_value": route["input"].get("value"),
    }


def ime_output_areas(trace):
    return [
        tuple(map(int, match))
        for match in re.findall(
            r"IME_OUT_AREA x=(-?\d+) y=(-?\d+) width=(\d+) height=(\d+) action=flushed",
            trace,
        )
    ]


def rounded_pixel(value):
    return math.floor(value + 0.5) if value >= 0 else math.ceil(value - 0.5)


def accepted_web_caret_area(web_trace):
    caret_matches = list(
        re.finditer(
            r"linux_ime_cursor page=(?P<page>.*?) generation=(?P<generation>\d+) "
            r".*?accepted=true rect=Some\(\[(?P<rect>[^\]]+)\]\)",
            web_trace,
        )
    )
    if not caret_matches:
        return None
    caret_match = caret_matches[-1]
    page = caret_match.group("page")
    generation = caret_match.group("generation")
    caret = [float(value.strip()) for value in caret_match.group("rect").split(",")]
    if len(caret) != 4:
        return None

    bounds_matches = list(
        re.finditer(
            rf"linux_frame layer page={re.escape(page)} generation={generation} "
            r".*?stage=Seat bounds=WebBounds \{ x: (-?\d+), y: (-?\d+), "
            r"width: (\d+), height: (\d+) \}",
            web_trace,
        )
    )
    if not bounds_matches:
        return None
    bounds = tuple(map(int, bounds_matches[-1].groups()))
    left = rounded_pixel(bounds[0] + caret[0])
    top = rounded_pixel(bounds[1] + caret[1])
    right = rounded_pixel(bounds[0] + caret[2])
    bottom = rounded_pixel(bounds[1] + caret[3])
    return {
        "page": page,
        "generation": int(generation),
        "rect": caret,
        "bounds": bounds,
        "area": (left, top, max(1, right - left), max(1, bottom - top)),
    }


def wait_for_web_caret_area(ime_trace_path, web_trace_path, baseline, app, label):
    def aligned(trace):
        areas = ime_output_areas(trace)
        caret = accepted_web_caret_area(log_path_text(web_trace_path))
        return len(areas) > baseline and caret is not None and areas[-1] == caret["area"]

    wait_trace(ime_trace_path, aligned, app, label)
    trace = log_path_text(ime_trace_path)
    return ime_output_areas(trace)[-1], accepted_web_caret_area(log_path_text(web_trace_path))


def select_ibus_engine(env, address, engine):
    result = subprocess.run(
        [
            "gdbus",
            "call",
            "--address",
            address,
            "--dest",
            "org.freedesktop.IBus",
            "--object-path",
            "/org/freedesktop/IBus",
            "--method",
            "org.freedesktop.IBus.SetGlobalEngine",
            engine,
        ],
        env=env,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode:
        raise RuntimeError(f"selecting private IBus engine {engine!r} failed: {result.stderr}")


def browser_smoke(
    executable,
    xdotool,
    env,
    root,
    *,
    url,
    chromium,
    daemon_path,
    panel_path,
    app_log_path,
    without_ibus,
):
    ibus = None if without_ibus else start_ibus(env, root, daemon_path, panel_path)
    app = None
    app_log = None
    child_pid = None
    child_start = None
    pty_path = root / "pty.dump"
    shell_pid = root / "shell.pid"
    shell = root / "probe-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "test -t 0 && test -t 1 || exit 9\n"
        "printf 'FOLIO_WEB_XORG_PTY_READY '; stty size\n"
        "printf '%s\\n' \"$$\" > \"$FOLIO_CHILD_PID\"\n"
        "exec /bin/bash --noprofile --norc -i\n",
        encoding="utf-8",
    )
    shell.chmod(0o700)
    app_env = env.copy()
    app_env.pop("LD_LIBRARY_PATH", None)
    app_env.update(
        SHELL=str(shell),
        FOLIO_CHILD_PID=str(shell_pid),
        BT_PTY_DUMP=str(pty_path),
        FOLIO_CHROMIUM_PATH=str(chromium),
        BT_WEB_DEV=url,
        BT_WEB_TRACE=str(root / "web.trace"),
        BT_CHROME_DUMP=str(root / "chrome.dump"),
        NO_PROXY="127.0.0.1,127.0.0.2,localhost",
        no_proxy="127.0.0.1,127.0.0.2,localhost",
    )
    try:
        app, app_log = smoke.start_logged(
            "folio",
            [str(executable), "--profile", "usershell", "--cwd", str(root)],
            root,
            app_env,
        )
        window = wait_browser_window(app, xdotool, env, pty_path, app_log_path)
        child_pid = int((wait_file(shell_pid, app, app_log_path, "PTY shell pid")).read_text().strip())
        child_identity = smoke.process_identity(child_pid)
        if child_identity is None:
            raise RuntimeError(f"PTY shell child {child_pid} exited before input")
        child_start = child_identity[1]

        run([xdotool, "windowactivate", "--sync", window], env, timeout=5)
        if run([xdotool, "getwindowfocus"], env) != window:
            raise RuntimeError("Folio did not acquire private Xorg focus")
        dismiss_browser_first_run(root / "ime.trace", app, app_log_path, xdotool, env)
        ime_trace = root / "ime.trace"
        initial_area_count = len(ime_output_areas(log_path_text(ime_trace)))
        if not without_ibus:
            app_log_text = log_path_text(app_log_path)
            focus_loss = app_log_text.find("reason=focus-loss")
            focus_gain = app_log_text.find("reason=focus-gain", focus_loss + 1)
            if focus_loss < 0 or focus_gain < 0:
                raise RuntimeError("private Xorg did not report the native window blur/refocus cycle")

        # The input probe uses the page screenshot to click real rendered fields.
        dom = probe.run_probe(
            argparse.Namespace(
                display=env["DISPLAY"],
                window=window,
                url=url,
                xdotool=Path(xdotool),
                import_bin=Path("/usr/bin/import"),
                tesseract=Path("/usr/bin/tesseract"),
                artifacts=root / "dom-input",
                focus_label="Top input",
                quiet=True,
            ),
            env=env,
        )

        if without_ibus:
            if b"FOLIO_XORG_KEY_OK" in pty_path.read_bytes() or b"FOLIO_OOPIF_KEY_OK" in pty_path.read_bytes():
                raise RuntimeError("browser DOM keyboard input leaked into PTY bytes")
            print(
                json.dumps(
                    {
                        "xorg_dom_keyboard": {
                            "top": summarize_dom_route(dom["top_input"]),
                            "oopif": summarize_dom_route(dom["oopif_input"]),
                        },
                        "ibus": "disabled for the Xorg navigation comparison",
                    },
                    ensure_ascii=False,
                    indent=2,
                ),
                flush=True,
            )
            smoke.run_tool([xdotool, "key", "--clearmodifiers", "ctrl+shift+q"], env)
            app.wait(timeout=15)
            smoke.wait_reaped(child_pid, child_start)
            print(f"PASS Xorg browser close reaped PTY shell child pid={child_pid}", flush=True)
            return

        wait_for_web_caret_area(
            ime_trace,
            root / "web.trace",
            initial_area_count,
            app,
            "browser caret area after native Xorg refocus",
        )

        # Exercise the production XIM -> WebIme -> CDP composition path.
        select_ibus_engine(env, ibus[3], "libpinyin")
        time.sleep(0.15)
        initial_trace = log_path_text(ime_trace)
        preedit_count = len(re.findall(r"IME_IN kind=Preedit bytes=[1-9]\d*", initial_trace))
        smoke.run_tool([xdotool, "type", "--clearmodifiers", "--delay", "50", "nihao"], env)
        wait_trace(
            root / "ime.trace",
            lambda value: len(re.findall(r"IME_IN kind=Preedit bytes=[1-9]\d*", value)) > preedit_count,
            app,
            "XIM preedit to reach Folio",
        )
        web_preedit = wait_web_event(
            url,
            lambda event: event.get("frame") == "top"
            and event.get("id") == "input"
            and event.get("event") == "compositionupdate",
            app,
            "browser DOM compositionupdate",
        )
        if "你好".encode() in pty_path.read_bytes():
            raise RuntimeError("uncommitted browser composition leaked into PTY bytes")
        caret_area, accepted_caret = wait_for_web_caret_area(
            ime_trace,
            root / "web.trace",
            initial_area_count,
            app,
            "XIM composition caret area to match the accepted browser caret",
        )
        screenshot = root / "xorg-ime-preedit.png"
        subprocess.run(
            ["/usr/bin/import", "-display", env["DISPLAY"], "-window", "root", str(screenshot)],
            env=env,
            check=True,
            timeout=10,
        )

        preedit_count = len(re.findall(r"IME_IN kind=Commit bytes=6", log_path_text(root / "ime.trace")))
        smoke.run_tool([xdotool, "key", "--clearmodifiers", "space"], env)
        wait_trace(
            root / "ime.trace",
            lambda value: value.count("IME_IN kind=Commit bytes=6") > preedit_count,
            app,
            "XIM commit to reach Folio",
        )
        committed = wait_web_event(
            url,
            lambda event: event.get("frame") == "top"
            and event.get("id") == "input"
            and event.get("event") == "input"
            and "你好" in str(event.get("value", "")),
            app,
            "Chinese text to commit into the browser input",
        )
        if "你好".encode() in pty_path.read_bytes():
            raise RuntimeError("browser XIM commit leaked into PTY bytes")
        print(
            json.dumps(
                {
                    "xorg_dom_keyboard": {
                        "top": summarize_dom_route(dom["top_input"]),
                        "oopif": summarize_dom_route(dom["oopif_input"]),
                    },
                    "browser_preedit": web_preedit,
                    "browser_commit": committed,
                    "candidate_caret_area": caret_area,
                    "accepted_browser_caret": accepted_caret,
                    "candidate_screenshot": str(screenshot),
                },
                ensure_ascii=False,
                indent=2,
            ),
            flush=True,
        )

        select_ibus_engine(env, ibus[3], "xkb:us::eng")
        smoke.run_tool([xdotool, "key", "--clearmodifiers", "ctrl+shift+q"], env)
        app.wait(timeout=15)
        smoke.wait_reaped(child_pid, child_start)
        print(f"PASS Xorg browser close reaped PTY shell child pid={child_pid}", flush=True)
    finally:
        if app is not None:
            smoke.stop_process(app, "Folio browser input")
        if app_log is not None:
            app_log.close()
        if child_pid is not None:
            smoke.terminate_test_child(child_pid, child_start)
        if ibus is not None:
            daemon, daemon_log, socket_dir, _address = ibus
            smoke.stop_process(daemon, "private IBus XIM")
            daemon_log.close()
            shutil.rmtree(socket_dir, ignore_errors=True)


def wait_file(path, process, log_path, label):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if path.exists():
            return path
        if process.poll() is not None:
            raise RuntimeError(f"Folio exited while waiting for {label}: {log_path.read_text(errors='replace')[-1600:]}")
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for {label}: {log_path.read_text(errors='replace')[-1600:]}")


def wait_browser_window(process, xdotool, env, pty_path, log_path):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        log = log_path.read_text(errors="replace") if log_path.exists() else ""
        output = pty_path.read_bytes() if pty_path.exists() else b""
        if process.poll() is not None:
            raise RuntimeError(f"Folio exited before its web input window appeared: {log[-1600:]}")
        if "BT_STARTUP first_text_present=" in log and b"FOLIO_WEB_XORG_PTY_READY" in output:
            for window in reversed(re.findall(r"WindowId\((\d+)\)", log)):
                geometry = subprocess.run(
                    [xdotool, "getwindowgeometry", "--shell", window],
                    env=env,
                    capture_output=True,
                    text=True,
                    timeout=5,
                )
                if geometry.returncode == 0:
                    return window
        time.sleep(0.05)
    raise RuntimeError(
        f"Folio did not publish a live X11 window: {log_path.read_text(errors='replace')[-1600:]}"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--chromium", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, default=REPO / "scripts/ci/linux-web-input-fixture.py")
    parser.add_argument("--artifacts", type=Path, default=REPO / "target/linux-web-input-xorg-smoke")
    parser.add_argument("--xorg", type=Path, required=True)
    parser.add_argument("--modulepath", type=Path, required=True)
    parser.add_argument("--xorg-config", type=Path, default=REPO / "scripts/ci/linux-input-xorg.conf")
    parser.add_argument("--xdotool", type=Path, required=True)
    parser.add_argument("--openbox", type=Path, required=True)
    parser.add_argument("--desktop-root", type=Path, required=True)
    parser.add_argument("--ibus-daemon", type=Path, default=Path("/usr/bin/ibus-daemon"))
    parser.add_argument("--ibus-panel", type=Path, default=Path("/usr/libexec/ibus-ui-gtk3"))
    parser.add_argument(
        "--without-ibus",
        action="store_true",
        help="run the same Xorg and DOM probe without private IBus to isolate the XIM environment",
    )
    args = parser.parse_args()
    if not args.without_ibus and not os.environ.get("DBUS_SESSION_BUS_ADDRESS"):
        parser.error("run this probe under dbus-run-session so IBus stays private")

    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    fixture_root = Path(tempfile.mkdtemp(prefix="fixture-", dir=artifacts))
    url_file = fixture_root / "url.txt"
    events_file = fixture_root / "events.jsonl"
    fixture_env = os.environ.copy()
    for name in (
        "DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS", "DBUS_SYSTEM_BUS_ADDRESS", "SESSION_MANAGER",
    ):
        fixture_env.pop(name, None)
    fixture_log = (fixture_root / "fixture.log").open("wb")
    fixture = subprocess.Popen(
        [sys.executable, str(args.fixture), "--url-file", str(url_file), "--events-file", str(events_file)],
        cwd=REPO,
        env=fixture_env,
        stdin=subprocess.DEVNULL,
        stdout=fixture_log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    try:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and not url_file.exists():
            if fixture.poll() is not None:
                raise RuntimeError(f"browser input fixture exited: {fixture_log.name}")
            time.sleep(0.05)
        if not url_file.exists():
            raise RuntimeError("browser input fixture did not publish its URL")
        url = url_file.read_text().strip()
        os.environ.update(
            BT_WEB_DEV=url,
            BT_WEB_TRACE=str(artifacts / "web.trace"),
            BT_CHROME_DUMP=str(artifacts / "chrome.dump"),
            FOLIO_CHROMIUM_PATH=str(args.chromium.resolve()),
            XDG_DATA_DIRS=os.pathsep.join((str(args.desktop_root / "usr/share"), "/usr/share")),
            XDG_CONFIG_DIRS=os.pathsep.join((str(args.desktop_root / "usr/etc/xdg"), "/etc/xdg")),
        )
        prior_library_path = os.environ.get("LD_LIBRARY_PATH", "")
        os.environ["LD_LIBRARY_PATH"] = os.pathsep.join(
            value for value in (str(args.desktop_root / "usr/lib64"), prior_library_path) if value
        )
        base_clean_environment = smoke.clean_environment
        if args.without_ibus:
            smoke.clean_environment = base_clean_environment
        else:
            private_bus = os.environ["DBUS_SESSION_BUS_ADDRESS"]

            def clean_environment(root, runtime):
                env = base_clean_environment(root, runtime)
                env.update(
                    DBUS_SESSION_BUS_ADDRESS=private_bus,
                    XMODIFIERS="@im=ibus",
                    GTK_IM_MODULE="ibus",
                    QT_IM_MODULE="ibus",
                    XDG_DATA_DIRS=os.environ["XDG_DATA_DIRS"],
                    XDG_CONFIG_DIRS=os.environ["XDG_CONFIG_DIRS"],
                )
                return env

            smoke.clean_environment = clean_environment
        smoke.smoke_app = lambda executable, xdotool, env, root: browser_smoke(
            executable,
            xdotool,
            env,
            root,
            url=url,
            chromium=args.chromium.resolve(),
            daemon_path=args.ibus_daemon.resolve(),
            panel_path=args.ibus_panel.resolve(),
            app_log_path=root / "folio.log",
            without_ibus=args.without_ibus,
        )
        smoke_args = [
            str(SMOKE_PATH),
            "--exe", str(args.exe.resolve()),
            "--xorg", str(args.xorg.resolve()),
            "--modulepath", str(args.modulepath.resolve()),
            "--xorg-config", str(args.xorg_config.resolve()),
            "--xdotool", str(args.xdotool.resolve()),
            "--openbox", str(args.openbox.resolve()),
            "--artifacts", str(artifacts),
        ]
        old_argv = sys.argv
        sys.argv = smoke_args
        try:
            smoke.main()
        finally:
            sys.argv = old_argv
    finally:
        try:
            os.killpg(fixture.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            fixture.wait(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(fixture.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            fixture.wait()
        fixture_log.close()
        print(f"FIXTURE_ARTIFACTS={fixture_root}", flush=True)


if __name__ == "__main__":
    main()
