#!/usr/bin/env python3
"""Exercise Chromium preview floats and tab moves in private native Wayland Niri."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import threading
import time

from PIL import Image


repo = Path(__file__).resolve().parents[2]
input_smoke_path = repo / "scripts" / "ci" / "linux-input-smoke.py"


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


input_smoke = load_module("linux_input_smoke", input_smoke_path)


def write_pdf_fixture(path):
    stream = b"BT /F1 48 Tf 72 720 Td (FOLIO PDF) Tj ET\n"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"endstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    pdf = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for index, body in enumerate(objects, start=1):
        offsets.append(len(pdf))
        pdf.extend(f"{index} 0 obj\n".encode())
        pdf.extend(body)
        pdf.extend(b"\nendobj\n")
    xref = len(pdf)
    pdf.extend(f"xref\n0 {len(objects) + 1}\n".encode())
    pdf.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        pdf.extend(f"{offset:010d} 00000 n \n".encode())
    pdf.extend(
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref}\n%%EOF\n".encode()
    )
    path.write_bytes(pdf)


def write_html_fixture(path):
    path.write_text(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Folio HTML transfer</title>"
        "<style>body{background:#fff;color:#111;font:28px sans-serif;padding:48px}</style></head>"
        "<body>FOLIO HTML FLOAT TRANSFER</body></html>\n"
    )


class HeldPageFixture:
    def __init__(self, log_path):
        self.log_path = log_path
        self.requested = threading.Event()
        self.headers_sent = threading.Event()
        self.release = threading.Event()
        self.response_finished = threading.Event()
        self.executed = threading.Event()
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, format, *args):
                with fixture.log_path.open("a", encoding="utf-8") as stream:
                    stream.write((format % args) + "\n")

            def do_GET(self):
                if self.path == "/executed":
                    fixture.executed.set()
                    self.send_response(204)
                    self.end_headers()
                    return
                if self.path != "/slow":
                    self.send_error(404)
                    return

                fixture.requested.set()
                payload = (
                    "<!doctype html><html><head><meta charset=utf-8>"
                    "<title>Folio cancelled load</title><style>body{font:48px sans-serif}"
                    "</style></head><body>FOLIO LATE PAGE"
                    "<script>fetch('/executed').catch(()=>{});</script></body></html>"
                ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(payload)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.flush()
                fixture.headers_sent.set()
                fixture.release.wait(60)
                try:
                    self.wfile.write(payload)
                    self.wfile.flush()
                    with fixture.log_path.open("a", encoding="utf-8") as stream:
                        stream.write("response body delivered\n")
                except (BrokenPipeError, ConnectionResetError) as error:
                    with fixture.log_path.open("a", encoding="utf-8") as stream:
                        stream.write(f"browser disconnected: {error!r}\n")
                finally:
                    fixture.response_finished.set()

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}/slow"

    def close(self):
        self.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)


def process_rows():
    result = subprocess.run(
        ["ps", "-eo", "pid=,ppid=,pgid=,sid=,tty=,stat=,args="],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=5,
        check=True,
    )
    rows = {}
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 6)
        if len(fields) == 7:
            rows[fields[0]] = fields
    return rows


def live_shells(root, app_pid):
    rows = process_rows()
    wrappers = {
        pid: fields
        for pid, fields in rows.items()
        if fields[1] == str(app_pid) and "probe-shell" in fields[6]
    }
    bash_children = {
        pid: fields
        for pid, fields in rows.items()
        if fields[1] in wrappers and "bash --noprofile --norc" in fields[6]
    }
    if len(wrappers) != 2 or len(bash_children) != 2:
        candidates = {
            pid: fields
            for pid, fields in rows.items()
            if str(root) in fields[6]
            and ("probe-shell" in fields[6] or "bash --noprofile --norc" in fields[6])
        }
        folio_processes = {
            pid: fields
            for pid, fields in rows.items()
            if str(root) in fields[6] and "folio" in fields[6].lower()
        }
        raise RuntimeError(
            f"expected two live Folio-owned shells: {wrappers=} {bash_children=} "
            f"{candidates=} {folio_processes=}"
        )
    ttys = {fields[4] for fields in wrappers.values()}
    if len(ttys) != 2:
        raise RuntimeError(f"moved tab shells share a TTY: {wrappers}")
    return set(wrappers) | set(bash_children), wrappers, bash_children


def shell_ready(root):
    count = 0
    for path in [root / "pty.dump", *sorted(root.glob("pty.dump.[0-9]*"))]:
        if path.is_file() and not path.name.endswith(".chunks"):
            count += path.read_bytes().count(b"FOLIO_WAYLAND_FLOAT_SHELL_READY")
    return count


def pty_bytes(root):
    return b"".join(
        path.read_bytes()
        for path in [root / "pty.dump", *sorted(root.glob("pty.dump.[0-9]*"))]
        if path.is_file() and not path.name.endswith(".chunks")
    )


def wait_for(predicate, description, app, log_path, *, seconds=45):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        if app is not None and app.poll() is not None:
            raise RuntimeError(
                f"Folio exited while waiting for {description}: "
                f"{log_path.read_text(errors='replace')[-1600:]}"
            )
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {description}; artifacts={log_path.parent}")


def read_text(path):
    try:
        return path.read_text(errors="replace")
    except FileNotFoundError:
        return ""


def dismiss_first_run_preview(trace_path, app, log_path, xdotool, env):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        lines = read_text(trace_path).splitlines()
        modal = next(
            (
                index
                for index, line in enumerate(lines)
                if "new=Modal" in line and "cause=first_run" in line
            ),
            None,
        )
        if modal is not None:
            if any(
                "previous_cause=first_run" in line and "new=Modal" not in line
                for line in lines[modal + 1 :]
            ):
                return
            input_smoke.run_tool([xdotool, "key", "--clearmodifiers", "Escape"], env)
            break
        if app.poll() is not None:
            raise RuntimeError(f"Folio exited before the first-run modal: {read_text(log_path)[-1600:]}")
        time.sleep(0.025)
    else:
        raise RuntimeError(f"first-run modal did not appear: {read_text(trace_path)[-1200:]}")

    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        lines = read_text(trace_path).splitlines()
        if any(
            "previous_cause=first_run" in line and "new=Modal" not in line
            for line in lines
        ):
            return
        if app.poll() is not None:
            break
        time.sleep(0.025)
    raise RuntimeError(
        f"Escape did not dismiss the first-run modal: {read_text(trace_path)[-1200:]}; "
        f"log={read_text(log_path)[-1200:]}"
    )


def niri_windows(niri, env):
    result = subprocess.run(
        [str(niri), "msg", "--json", "windows"],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=10,
    )
    if result.returncode:
        raise RuntimeError(f"niri msg windows failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def focus_niri_window(niri, env, window_id, app):
    result = subprocess.run(
        [str(niri), "msg", "action", "focus-window", "--id", str(window_id)],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=10,
    )
    if result.returncode:
        raise RuntimeError(f"Niri could not focus Folio window {window_id}: {result.stderr.strip()}")
    wait_for(
        lambda: any(
            row.get("id") == window_id and row.get("is_focused")
            for row in niri_windows(niri, env)
        ),
        f"Niri window {window_id} to focus",
        app,
        Path(env["BT_WEB_TRACE"]),
        seconds=15,
    )


def focused_window_origin(niri, niri_env, grim, app, log_path, root):
    rows = niri_windows(niri, niri_env)
    focused = next(
        (row for row in rows if row.get("app_id") == "io.github.lulu-loopp.folio" and row.get("is_focused")),
        None,
    )
    if focused is None:
        raise RuntimeError(f"no focused Folio window in Niri: {rows}")
    screenshot = root / f"niri-window-{focused['id']}.png"
    result = subprocess.run(
        [str(grim), str(screenshot)],
        env=niri_env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
    )
    if result.returncode:
        raise RuntimeError(f"grim failed: {result.stderr.decode(errors='replace')}")
    image = Image.open(screenshot).convert("RGB")
    points = [
        (x, y)
        for y in range(image.height)
        for x in range(image.width)
        if image.getpixel((x, y)) == (127, 200, 255)
    ]
    if not points:
        raise RuntimeError(f"Niri focus outline was missing from {screenshot}")
    left = min(x for x, _ in points)
    top = min(y for _, y in points)
    right = max(x for x, _ in points)
    bottom = max(y for _, y in points)
    size = focused["layout"]["window_size"]
    if abs((right - left + 1) - size[0]) > 12 or abs((bottom - top + 1) - size[1]) > 12:
        raise RuntimeError(
            f"Niri focus outline does not match the focused Folio window: "
            f"outline={(left, top, right, bottom)} layout_size={size} screenshot={screenshot}"
        )
    return left, top, focused, screenshot


def marker_visible(image_path, tesseract, marker):
    result = subprocess.run(
        [str(tesseract), str(image_path), "stdout", "--psm", "6"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=15,
    )
    if result.returncode:
        raise RuntimeError(f"tesseract failed: {result.stderr.strip()}")
    recognized = re.sub(r"\s+", " ", result.stdout).upper()
    return marker in recognized


def web_page_bounds(trace_path):
    for line in reversed(read_text(trace_path).splitlines()):
        if "linux_frame layer " not in line:
            continue
        match = re.search(
            r"bounds=WebBounds \{ x: (-?\d+), y: (-?\d+), width: (\d+), height: (\d+) \}",
            line,
        )
        if match:
            return tuple(map(int, match.groups()))
    return None


def web_page_marker_visible(image_path, trace_path, origin, tesseract, marker, crop_path):
    bounds = web_page_bounds(trace_path)
    if bounds is None:
        return marker_visible(image_path, tesseract, marker)
    x, y, width, height = bounds
    with Image.open(image_path) as image:
        crop = image.crop((origin[0] + x, origin[1] + y, origin[0] + x + width, origin[1] + y + height))
        crop.save(crop_path)
    return marker_visible(crop_path, tesseract, marker)


def sprite_center(path, mark):
    for line in reversed(path.read_text(errors="replace").splitlines()):
        if "sprite [" not in line or f"mark={mark}" not in line:
            continue
        match = re.search(r"sprite \[([0-9.]+), ([0-9.]+), ([0-9.]+), ([0-9.]+)\]", line)
        if match:
            x1, y1, x2, y2 = map(float, match.groups())
            return round((x1 + x2) / 2), round((y1 + y2) / 2), line
    return None


def latest_chrome_frame(path):
    frames = re.split(r"(?=--- chrome frame:)", read_text(path))
    return frames[-1]


def label_center(path, label):
    quoted = f'"{label}"'
    for line in reversed(path.read_text(errors="replace").splitlines()):
        if not line.startswith("label ") or quoted not in line:
            continue
        match = re.search(r"label\s+\[([0-9.]+), ([0-9.]+), ([0-9.]+), ([0-9.]+)\]", line)
        if match:
            x1, y1, x2, y2 = map(float, match.groups())
            return round((x1 + x2) / 2), round((y1 + y2) / 2), line
    return None


def window_menu_center(path):
    contents = path.read_text(errors="replace")
    frames = re.split(r"(?=--- chrome frame:)", contents)
    for line in reversed(frames[-1].splitlines()):
        if not line.startswith("label "):
            continue
        label = re.search(r'"(Window \d+ · \d+ tab)"', line)
        if label is None:
            continue
        bounds = re.search(
            r"label\s+\[([0-9.]+), ([0-9.]+), ([0-9.]+), ([0-9.]+)\]",
            line,
        )
        if bounds:
            x1, y1, x2, y2 = map(float, bounds.groups())
            return round((x1 + x2) / 2), round((y1 + y2) / 2), label.group(1)
    return None


def browser_processes(root):
    return {
        pid: fields
        for pid, fields in process_rows().items()
        if "--user-data-dir=" in fields[6] and str(root) in fields[6]
    }


def clean_environment(root, runtime):
    env = os.environ.copy()
    for name in (
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "WAYLAND_SOCKET",
        "NIRI_SOCKET",
        "NIRI_CONFIG",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "DBUS_SESSION_BUS_ADDRESS",
        "DBUS_SYSTEM_BUS_ADDRESS",
        "SESSION_MANAGER",
        "DESKTOP_SESSION",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "PULSE_SERVER",
        "PIPEWIRE_REMOTE",
        "JACK_SERVER",
        "TMPDIR",
        "TMP",
        "TEMP",
    ):
        env.pop(name, None)
    env.update(
        HOME=str(root / "home"),
        XDG_CONFIG_HOME=str(root / "config"),
        XDG_DATA_HOME=str(root / "data"),
        XDG_CACHE_HOME=str(root / "cache"),
        XDG_STATE_HOME=str(root / "state"),
        XDG_RUNTIME_DIR=str(runtime),
        TMPDIR=str(runtime),
        TERM="xterm-256color",
        LC_ALL="C.UTF-8",
        HISTFILE="/dev/null",
        BT_GPU_PREFERENCE="low",
        BT_STARTUP_TRACE="1",
        XDG_SESSION_TYPE="x11",
    )
    return env


def start_logged(name, argv, cwd, env, log_path, *, pass_fds=()):
    log = log_path.open("wb")
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=subprocess.STDOUT,
        pass_fds=pass_fds,
        start_new_session=True,
    )
    print(f"START {name} pid={process.pid}", flush=True)
    return process, log


def private_window_sockets(runtime, niri, log_path):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        displays = [path for path in runtime.glob("wayland-*") if path.is_socket()]
        sockets = sorted(runtime.glob("niri.*.sock"))
        if displays and sockets:
            return displays[0], sockets[0]
        if niri.poll() is not None:
            raise RuntimeError(
                f"nested Niri exited: {log_path.read_text(errors='replace')[-2000:]}"
            )
        time.sleep(0.05)
    raise RuntimeError(f"nested Niri did not create its private sockets: {log_path}")


def stop_process(process, name):
    if process is None or process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)
    print(f"STOP {name} pid={process.pid}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--chromium", type=Path, required=True)
    parser.add_argument("--xorg", type=Path, required=True)
    parser.add_argument("--modulepath", type=Path, required=True)
    parser.add_argument("--xorg-config", type=Path, default=Path(__file__).with_name("linux-input-xorg.conf"))
    parser.add_argument("--xdotool", type=Path, required=True)
    parser.add_argument("--xdotool-libdir", type=Path, required=True)
    parser.add_argument("--niri", type=Path, default=Path("/usr/bin/niri"))
    parser.add_argument("--grim", type=Path, default=Path("/usr/bin/grim"))
    parser.add_argument("--tesseract", type=Path, default=Path("/usr/bin/tesseract"))
    parser.add_argument("--document-type", choices=("html", "pdf"), required=True)
    parser.add_argument(
        "--cancel-midload",
        action="store_true",
        help="cancel a held loopback HTML navigation, reject late frames, and verify shutdown reaping",
    )
    parser.add_argument(
        "--dock-before-quit",
        action="store_true",
        help="dock the moved web page into its seat, verify it remains visible, then quit cleanly",
    )
    parser.add_argument("--artifacts", type=Path, default=Path("target/linux-wayland-web-preview-smoke"))
    args = parser.parse_args()
    for path, description in (
        (args.exe, "Folio executable"),
        (args.chromium, "Chromium executable"),
        (args.xorg, "Xorg executable"),
        (args.xdotool, "xdotool executable"),
        (args.grim, "grim executable"),
        (args.tesseract, "tesseract executable"),
    ):
        if not path.is_file() or not os.access(path, os.X_OK):
            parser.error(f"{description} is missing or not executable: {path}")
    if not args.modulepath.is_dir() or not args.xdotool_libdir.is_dir():
        parser.error("Xorg modulepath and xdotool library directory must exist")

    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="session-", dir=artifacts))
    root.chmod(0o700)
    for name in ("home", "config", "data", "cache", "state"):
        (root / name).mkdir(parents=True, exist_ok=True)
    runtime_context = tempfile.TemporaryDirectory(prefix=f"folio-niri-{os.getpid()}-")
    runtime = Path(runtime_context.name)
    runtime.chmod(0o700)
    env = clean_environment(root, runtime)
    browser_profile = root / "data" / "Folio" / "Chromium"
    slow_fixture = HeldPageFixture(root / "slow-server.log") if args.cancel_midload else None
    if slow_fixture is not None:
        fixture = None
        marker = "FOLIO LATE PAGE"
    elif args.document_type == "html":
        fixture = root / "fixture.html"
        write_html_fixture(fixture)
        marker = "FOLIO HTML FLOAT TRANSFER"
    else:
        fixture = root / "fixture.pdf"
        write_pdf_fixture(fixture)
        marker = "FOLIO PDF"
    shell = root / "probe-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "printf 'SHELL_BIRTH pid=%s ppid=%s tty=%s\\n' \"$$\" \"$PPID\" \"$(tty)\" >> \"$FOLIO_SHELL_LEDGER\"\n"
        "printf 'FOLIO_WAYLAND_FLOAT_SHELL_READY '; stty size\n"
        "/bin/bash --noprofile --norc\n"
    )
    shell.chmod(0o700)
    xlog = nlog = alog = None
    xorg = niri = app = None
    shell_pids = set()
    try:
        config = root / "niri.kdl"
        config.write_text(
            "input {\n"
            "    keyboard {\n"
            "        xkb {\n"
            "            layout \"us\"\n"
            "        }\n"
            "    }\n"
            "}\n"
            "xwayland-satellite {\n    off\n}\n"
            "hotkey-overlay {\n    skip-at-startup\n}\n"
        )
        validation = subprocess.run(
            [str(args.niri), "validate", "--config", str(config)],
            cwd=root,
            env=env,
            capture_output=True,
            text=True,
            timeout=10,
        )
        if validation.returncode:
            raise RuntimeError(f"private Niri config is invalid: {validation.stderr or validation.stdout}")

        read_fd, write_fd = os.pipe()
        xlog = (root / "xorg.log").open("wb")
        xorg = subprocess.Popen(
            [
                str(args.xorg),
                "-displayfd",
                str(write_fd),
                "-config",
                str(args.xorg_config.resolve()),
                "-modulepath",
                str(args.modulepath.resolve()),
                "-logfile",
                str(root / "Xorg.log"),
                "-nolisten",
                "tcp",
                "-ac",
                "-noreset",
                "-novtswitch",
                "-sharevts",
            ],
            cwd=root,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=xlog,
            stderr=subprocess.STDOUT,
            pass_fds=(write_fd,),
            start_new_session=True,
        )
        os.close(write_fd)
        with os.fdopen(read_fd, "rb") as display_pipe:
            display = display_pipe.readline().decode().strip()
        if not display.isdigit():
            raise RuntimeError(f"private Xorg returned invalid display {display!r}")
        env["DISPLAY"] = f":{display}"

        niri_env = env.copy()
        niri_env["RUST_LOG"] = "niri=info"
        niri, nlog = start_logged(
            "nested private Niri",
            [str(args.niri), "--config", str(config)],
            root,
            niri_env,
            root / "niri.log",
        )
        wayland_socket, niri_socket = private_window_sockets(runtime, niri, root / "niri.log")

        client_env = env.copy()
        client_env.pop("DISPLAY", None)
        client_env.pop("WAYLAND_SOCKET", None)
        client_env.update(
            WAYLAND_DISPLAY=wayland_socket.name,
            NIRI_SOCKET=str(niri_socket),
            XDG_SESSION_TYPE="wayland",
            WINIT_UNIX_BACKEND="wayland",
            FOLIO_CHROMIUM_PATH=str(args.chromium.resolve()),
            BT_CHROME_DUMP=str(root / "chrome.dump"),
            BT_WEB_TRACE=str(root / "web.trace"),
            BT_PTY_DUMP=str(root / "pty.dump"),
            BT_STARTUP_TRACE="1",
            BT_IME_TRACE=str(root / "ime.trace"),
            FOLIO_SHELL_LEDGER=str(root / "shells.log"),
            SHELL=str(shell),
        )
        if slow_fixture is not None:
            client_env.update(
                BT_WEB_DEV=slow_fixture.url,
                NO_PROXY="127.0.0.1,127.0.0.2,localhost",
                no_proxy="127.0.0.1,127.0.0.2,localhost",
            )
        app_command = [str(args.exe.resolve()), "--profile", "usershell", "--cwd", str(root)]
        if fixture is not None:
            app_command.append(str(fixture))
        app, alog = start_logged(
            "Folio native Wayland",
            app_command,
            root,
            client_env,
            root / "folio.log",
        )
        if slow_fixture is not None:
            wait_for(
                lambda: shell_ready(root) >= 1 and slow_fixture.headers_sent.is_set(),
                "held local page response to reach Chromium",
                app,
                root / "folio.log",
                seconds=75,
            )
        else:
            wait_for(
                lambda: shell_ready(root) >= 1
                and "navigation_completed" in read_text(root / "web.trace")
                and "success=1" in read_text(root / "web.trace"),
                f"local {args.document_type.upper()} page to load in native Wayland",
                app,
                root / "folio.log",
                seconds=75,
            )

        xenv = env.copy()
        xenv["LD_LIBRARY_PATH"] = str(args.xdotool_libdir.resolve())
        xwindows = input_smoke.run_tool(
            [str(args.xdotool), "search", "--onlyvisible", "--name", "."], xenv
        ).splitlines()
        if not xwindows:
            raise RuntimeError("nested Niri has no visible X11 host window for XTest input")
        niri_window = xwindows[0]
        input_smoke.run_tool([str(args.xdotool), "windowfocus", "--sync", niri_window], xenv)
        dismiss_first_run_preview(
            root / "ime.trace", app, root / "folio.log", str(args.xdotool), xenv
        )

        initial = next(
            row for row in niri_windows(args.niri, client_env)
            if row.get("app_id") == "io.github.lulu-loopp.folio"
        )
        initial_origin = focused_window_origin(
            args.niri, client_env, args.grim, app, root / "folio.log", root
        )
        input_smoke.run_tool(
            [
                str(args.xdotool),
                "mousemove",
                "--window",
                niri_window,
                str(initial_origin[0] + initial["layout"]["window_size"][0] // 2),
                str(initial_origin[1] + 55),
            ],
            xenv,
        )
        wait_for(
            lambda: sprite_center(root / "chrome.dump", "Float") is not None,
            "Wayland page hover to expose its Float control",
            app,
            root / "folio.log",
            seconds=10,
        )
        if slow_fixture is not None:
            trace_path = root / "web.trace"
            if slow_fixture.response_finished.is_set():
                raise RuntimeError("held response completed before the cancellation interaction")
            if not browser_processes(root):
                raise RuntimeError("held Wayland navigation had no private Chromium process")
            held_image = root / "cancel-held-response.png"
            subprocess.run([str(args.grim), str(held_image)], env=client_env, check=True, timeout=10)
            shell_rows = process_rows()
            wrappers = {
                pid: fields
                for pid, fields in shell_rows.items()
                if fields[1] == str(app.pid) and str(root) in fields[6] and "probe-shell" in fields[6]
            }
            bash_children = {
                pid: fields
                for pid, fields in shell_rows.items()
                if fields[1] in wrappers and "bash --noprofile --norc" in fields[6]
            }
            if len(wrappers) != 1 or len(bash_children) != 1:
                raise RuntimeError(f"held Wayland page did not retain one app-owned shell: {wrappers=} {bash_children=}")
            shell_pids = set(wrappers) | set(bash_children)

            close_button = sprite_center(root / "chrome.dump", "PaneClose")
            if close_button is None:
                raise RuntimeError("held Wayland navigation did not expose its PaneClose control")
            close_offset = trace_path.stat().st_size
            input_smoke.run_tool(
                [
                    str(args.xdotool),
                    "mousemove",
                    "--sync",
                    "--window",
                    niri_window,
                    str(initial_origin[0] + close_button[0]),
                    str(initial_origin[1] + close_button[1]),
                    "click",
                    "1",
                ],
                xenv,
            )
            wait_for(
                lambda: any(
                    "place tab=" in line and "presence=Hidden" in line
                    for line in read_text(trace_path)[close_offset:].splitlines()
                ),
                "PaneClose to withdraw the still-loading Wayland page",
                app,
                root / "folio.log",
                seconds=10,
            )
            hidden_offset = trace_path.stat().st_size
            slow_fixture.release.set()
            wait_for(
                lambda: slow_fixture.response_finished.is_set(),
                "held fixture response to finish or observe the browser disconnect",
                app,
                root / "folio.log",
                seconds=10,
            )
            if slow_fixture.executed.wait(2):
                raise RuntimeError("the canceled late page executed its post-load fetch after PaneClose")
            after_release = read_text(trace_path)[hidden_offset:]
            late_frames = [line for line in after_release.splitlines() if "linux_frame received" in line]
            if late_frames:
                raise RuntimeError(f"late frames arrived after PaneClose: {late_frames[-4:]}")
            cancelled_image = root / "cancel-after-response.png"
            subprocess.run([str(args.grim), str(cancelled_image)], env=client_env, check=True, timeout=10)
            print(
                f"PASS PaneClose canceled held Wayland navigation; no late frame or script execution; "
                f"server={read_text(root / 'slow-server.log').strip()!r}",
                flush=True,
            )

            focus_niri_window(args.niri, client_env, initial["id"], app)
            input_smoke.run_tool([str(args.xdotool), "key", "--clearmodifiers", "ctrl+shift+q"], xenv)
            app.wait(timeout=20)
            remaining = process_rows()
            if shell_pids & remaining.keys():
                raise RuntimeError(f"cancel-and-quit left PTY shell processes: {shell_pids & remaining.keys()}")
            wait_for(
                lambda: not browser_processes(root),
                "canceled private Chromium group to exit",
                None,
                root / "folio.log",
                seconds=15,
            )
            print("PASS canceled native Wayland preview clean quit reaped Chromium and its PTY shell", flush=True)
            print(f"ARTIFACTS={root}", flush=True)
            return

        marker_path = root / f"{args.document_type}-in-pane.png"
        pane_origin = focused_window_origin(
            args.niri, client_env, args.grim, app, root / "folio.log", root
        )[:2]
        pane_crop = root / f"{args.document_type}-in-pane-crop.png"
        deadline = time.monotonic() + 8
        pane_visible = False
        while time.monotonic() < deadline:
            subprocess.run([str(args.grim), str(marker_path)], env=client_env, check=True, timeout=10)
            if web_page_marker_visible(
                marker_path,
                root / "web.trace",
                pane_origin,
                args.tesseract.resolve(),
                marker,
                pane_crop,
            ):
                pane_visible = True
                break
            time.sleep(0.25)
        if not pane_visible:
            raise RuntimeError(f"{args.document_type.upper()} canary missing in Wayland pane: {marker_path}")
        print(f"PASS native Wayland rendered the {args.document_type.upper()} canary in its pane", flush=True)

        def click_folio(local_x, local_y, button=1):
            left, top, focused, screenshot = focused_window_origin(
                args.niri, client_env, args.grim, app, root / "folio.log", root
            )
            input_smoke.run_tool(
                [str(args.xdotool), "mousemove", "--window", niri_window, str(left + local_x), str(top + local_y)],
                xenv,
            )
            input_smoke.run_tool([str(args.xdotool), "click", str(button)], xenv)
            return focused["id"], screenshot

        def drag_folio(start, end):
            left, top, _focused, _screenshot = focused_window_origin(
                args.niri, client_env, args.grim, app, root / "folio.log", root
            )
            input_smoke.run_tool(
                [
                    str(args.xdotool),
                    "mousemove",
                    "--window",
                    niri_window,
                    str(left + start[0]),
                    str(top + start[1]),
                ],
                xenv,
            )
            input_smoke.run_tool([str(args.xdotool), "mousedown", "1"], xenv)
            input_smoke.run_tool(
                [
                    str(args.xdotool),
                    "mousemove",
                    "--sync",
                    "--window",
                    niri_window,
                    str(left + end[0]),
                    str(top + end[1]),
                ],
                xenv,
            )
            input_smoke.run_tool([str(args.xdotool), "mouseup", "1"], xenv)

        float_button = sprite_center(root / "chrome.dump", "Float")
        if float_button is None:
            raise RuntimeError("Wayland preview pane did not expose its Float control")
        click_folio(float_button[0], float_button[1])
        wait_for(
            lambda: "mark=DockRight" in (root / "chrome.dump").read_text(errors="replace"),
            "Wayland preview float to appear",
            app,
            root / "folio.log",
        )
        floated_image = root / "preview-float.png"
        subprocess.run([str(args.grim), str(floated_image)], env=client_env, check=True, timeout=10)
        float_origin = focused_window_origin(
            args.niri, client_env, args.grim, app, root / "folio.log", root
        )[:2]
        if not web_page_marker_visible(
            floated_image,
            root / "web.trace",
            float_origin,
            args.tesseract.resolve(),
            marker,
            root / f"{args.document_type}-float-crop.png",
        ):
            raise RuntimeError(f"{args.document_type.upper()} canary missing in Wayland float: {floated_image}")
        print(f"PASS native Wayland kept the {args.document_type.upper()} canary visible in its float", flush=True)

        trace_path = root / "web.trace"
        trace_size_before_drag = trace_path.stat().st_size
        drag_folio((300, 90), (300, 340))

        def the_float_moved_below_the_tab_menu():
            trace = read_text(trace_path)[trace_size_before_drag:]
            placements = [
                line
                for line in trace.splitlines()
                if "place tab=" in line and "floated=1" in line
            ]
            if not placements:
                return False
            match = re.search(
                r"body=\[[0-9.]+, ([0-9.]+), [0-9.]+, [0-9.]+\]",
                placements[-1],
            )
            return match is not None and float(match.group(1)) >= 250.0

        wait_for(
            the_float_moved_below_the_tab_menu,
            "Wayland float to move below the tab transfer submenu",
            app,
            root / "folio.log",
            seconds=10,
        )
        moved_float_image = root / "preview-float-below-tab-menu.png"
        subprocess.run([str(args.grim), str(moved_float_image)], env=client_env, check=True, timeout=10)
        moved_float_origin = focused_window_origin(
            args.niri, client_env, args.grim, app, root / "folio.log", root
        )[:2]
        if not web_page_marker_visible(
            moved_float_image,
            trace_path,
            moved_float_origin,
            args.tesseract.resolve(),
            marker,
            root / f"{args.document_type}-float-below-menu-crop.png",
        ):
            raise RuntimeError(f"moving the Wayland float hid its page: {moved_float_image}")

        shell_click_x, shell_click_y = 80, max(200, initial["layout"]["window_size"][1] - 120)
        click_folio(shell_click_x, shell_click_y)
        input_smoke.run_tool([str(args.xdotool), "type", "--clearmodifiers", "export C=kept"], xenv)
        input_smoke.run_tool([str(args.xdotool), "key", "Return"], xenv)
        wait_for(
            lambda: b"export C=kept" in pty_bytes(root),
            "Wayland source shell cookie",
            app,
            root / "folio.log",
        )
        source_dump = next(
            path for path in [root / "pty.dump", *sorted(root.glob("pty.dump.[0-9]*"))]
            if path.is_file() and not path.name.endswith(".chunks") and b"export C=kept" in path.read_bytes()
        )

        source_id = initial["id"]
        focus_niri_window(args.niri, client_env, source_id, app)
        input_smoke.run_tool([str(args.xdotool), "key", "--clearmodifiers", "ctrl+shift+m"], xenv)
        wait_for(
            lambda: len([row for row in niri_windows(args.niri, client_env) if row.get("app_id") == "io.github.lulu-loopp.folio"]) == 2
            and shell_ready(root) >= 2,
            "second native Wayland window and shell",
            app,
            root / "folio.log",
        )
        niri_rows = [row for row in niri_windows(args.niri, client_env) if row.get("app_id") == "io.github.lulu-loopp.folio"]
        target = next(row for row in niri_rows if row["id"] != source_id)
        focus_niri_window(args.niri, client_env, source_id, app)
        click_folio(100, 20, button=3)
        wait_for(
            lambda: label_center(root / "chrome.dump", "Move to window") is not None,
            "tab context menu's Move to window row",
            app,
            root / "folio.log",
        )
        move_to_window = label_center(root / "chrome.dump", "Move to window")
        if move_to_window is None:
            raise RuntimeError("tab context menu lost its Move to window row")
        left, top, focused, _screenshot = focused_window_origin(
            args.niri, client_env, args.grim, app, root / "folio.log", root
        )
        menu_frame_size = (root / "chrome.dump").stat().st_size
        input_smoke.run_tool(
            [
                str(args.xdotool),
                "mousemove",
                "--window",
                niri_window,
                str(left + move_to_window[0]),
                str(top + move_to_window[1]),
            ],
            xenv,
        )
        wait_for(
            lambda: (root / "chrome.dump").stat().st_size > menu_frame_size,
            "pointer hover to reach Move to window",
            app,
            root / "folio.log",
            seconds=10,
        )
        submenu_frame_size = (root / "chrome.dump").stat().st_size
        input_smoke.run_tool([str(args.xdotool), "key", "Right"], xenv)
        wait_for(
            lambda: (root / "chrome.dump").stat().st_size > submenu_frame_size,
            "Move to window submenu to open",
            app,
            root / "folio.log",
            seconds=10,
        )
        destination_row = [None]

        def target_window_row_is_visible():
            destination_row[0] = window_menu_center(root / "chrome.dump")
            return destination_row[0] is not None

        wait_for(
            target_window_row_is_visible,
            "target window row in tab submenu",
            app,
            root / "folio.log",
            seconds=10,
        )
        if destination_row[0] is None:
            raise RuntimeError("Move to window submenu lost its destination row")
        input_smoke.run_tool(
            [
                str(args.xdotool),
                "mousemove",
                "--sync",
                "--window",
                niri_window,
                str(left + destination_row[0][0]),
                str(top + destination_row[0][1]),
                "click",
                "1",
            ],
            xenv,
        )
        input_smoke.run_tool([str(args.xdotool), "key", "Return"], xenv)
        wait_for(
            lambda: [row for row in niri_windows(args.niri, client_env) if row.get("app_id") == "io.github.lulu-loopp.folio"]
            == [next(row for row in niri_windows(args.niri, client_env) if row.get("id") == target["id"])],
            "source Wayland window to retire after tab move",
            app,
            root / "folio.log",
        )

        focus_niri_window(args.niri, client_env, target["id"], app)
        click_folio(80, max(200, target["layout"]["window_size"][1] - 120))
        input_smoke.run_tool(
            [str(args.xdotool), "type", "--clearmodifiers", 'printf "FLOAT_MOVE_COOKIE=%s\\n" "${C-absent}"'],
            xenv,
        )
        input_smoke.run_tool([str(args.xdotool), "key", "Return"], xenv)
        wait_for(
            lambda: b"FLOAT_MOVE_COOKIE=kept" in source_dump.read_bytes(),
            "same shell session after Wayland tab move",
            app,
            root / "folio.log",
        )
        shell_pids, wrappers, children = live_shells(root, app.pid)
        print(f"SHELL_OWNERSHIP wrappers={wrappers} bash_children={children}", flush=True)
        destination_image = root / "preview-tab-in-destination.png"
        subprocess.run([str(args.grim), str(destination_image)], env=client_env, check=True, timeout=10)
        destination_origin = focused_window_origin(
            args.niri, client_env, args.grim, app, root / "folio.log", root
        )[:2]
        if not web_page_marker_visible(
            destination_image,
            root / "web.trace",
            destination_origin,
            args.tesseract.resolve(),
            marker,
            root / f"{args.document_type}-destination-crop.png",
        ):
            raise RuntimeError(f"{args.document_type.upper()} canary missing after Wayland tab move: {destination_image}")
        print(f"PASS native Wayland kept the {args.document_type.upper()} canary visible after tab move", flush=True)

        owned_browsers = browser_processes(root)
        if not owned_browsers:
            raise RuntimeError("loaded Wayland web page had no Chromium process using its private profile")
        print(f"BROWSER_OWNERSHIP process_count={len(owned_browsers)}", flush=True)

        focus_niri_window(args.niri, client_env, target["id"], app)
        if args.dock_before_quit:
            dock_button = sprite_center(root / "chrome.dump", "DockRight")
            if dock_button is None:
                raise RuntimeError("moved Wayland float did not expose DockRight")
            trace_size_before_dock = (root / "web.trace").stat().st_size
            click_folio(dock_button[0], dock_button[1])

            def dock_frame_arrived():
                trace = read_text(root / "web.trace")[trace_size_before_dock:]
                latest = latest_chrome_frame(root / "chrome.dump")
                return (
                    "floated=-" in trace
                    and "stage=Seat" in trace
                    and "mark=Globe" in latest
                    and "mark=DockRight" not in latest
                )

            wait_for(
                dock_frame_arrived,
                "Wayland web page to dock into its seat",
                app,
                root / "folio.log",
            )
            docked_image = root / "preview-tab-docked.png"
            subprocess.run([str(args.grim), str(docked_image)], env=client_env, check=True, timeout=10)
            docked_origin = focused_window_origin(
                args.niri, client_env, args.grim, app, root / "folio.log", root
            )[:2]
            if not web_page_marker_visible(
                docked_image,
                root / "web.trace",
                docked_origin,
                args.tesseract.resolve(),
                marker,
                root / f"{args.document_type}-docked-crop.png",
            ):
                raise RuntimeError(f"docking the Wayland page hid its canary: {docked_image}")
            print(f"PASS docking the Wayland {args.document_type.upper()} kept its page visible", flush=True)
        else:
            close_button = sprite_center(root / "chrome.dump", "PaneClose")
            if close_button is None:
                raise RuntimeError("moved Wayland float did not expose PaneClose")
            click_folio(close_button[0], close_button[1])
            closed_image = root / f"{args.document_type}-after-float-close.png"
            deadline = time.monotonic() + 8
            withdrawn = False
            while time.monotonic() < deadline:
                subprocess.run([str(args.grim), str(closed_image)], env=client_env, check=True, timeout=10)
                if not marker_visible(closed_image, args.tesseract.resolve(), marker):
                    withdrawn = True
                    break
                time.sleep(0.25)
            if not withdrawn:
                raise RuntimeError(f"closing the Wayland float left its page visible: {closed_image}")
            print(f"PASS closing the native Wayland {args.document_type.upper()} float withdrew its page", flush=True)

        focus_niri_window(args.niri, client_env, target["id"], app)
        input_smoke.run_tool([str(args.xdotool), "key", "--clearmodifiers", "ctrl+shift+q"], xenv)
        app.wait(timeout=20)
        remaining = process_rows()
        if any(pid in remaining for pid in shell_pids):
            raise RuntimeError(f"Wayland Folio exit left shell processes: {shell_pids & remaining.keys()}")
        wait_for(lambda: not browser_processes(root), "private Chromium group to exit", None, root / "folio.log", seconds=15)
        print("PASS native Wayland clean quit reaped private Chromium and both moved-tab shells", flush=True)
        print(f"ARTIFACTS={root}", flush=True)
    finally:
        if slow_fixture is not None:
            slow_fixture.close()
        stop_process(app, "Folio")
        stop_process(niri, "nested private Niri")
        stop_process(xorg, "private Xorg")
        runtime_context.cleanup()
        for log in (xlog, nlog, alog):
            if log is not None:
                log.close()


if __name__ == "__main__":
    main()
