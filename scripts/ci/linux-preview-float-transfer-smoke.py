#!/usr/bin/env python3
"""Move floated Markdown, HTML, and PDF preview tabs between native Xorg windows."""

import argparse
import ctypes
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import subprocess
import tempfile
import time

from PIL import Image


def clean_environment(root, runtime, library_dir):
    env = os.environ.copy()
    for name in (
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "WAYLAND_SOCKET",
        "NIRI_SOCKET",
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

    directories = {
        "HOME": root / "home",
        "XDG_CONFIG_HOME": root / "config",
        "XDG_DATA_HOME": root / "data",
        "XDG_CACHE_HOME": root / "cache",
        "XDG_STATE_HOME": root / "state",
        "TMPDIR": root / "tmp",
    }
    for directory in directories.values():
        directory.mkdir(parents=True, exist_ok=True)
    env.update(
        {name: str(path) for name, path in directories.items()},
        XDG_RUNTIME_DIR=str(runtime),
        XDG_SESSION_TYPE="x11",
        WINIT_UNIX_BACKEND="x11",
        TERM="xterm-256color",
        LC_ALL="C.UTF-8",
        HISTFILE="/dev/null",
        BT_GPU_PREFERENCE="low",
        BT_STARTUP_TRACE="1",
        BT_PTY_DUMP=str(root / "pty.dump"),
        BT_CHROME_DUMP=str(root / "chrome.dump"),
    )
    if library_dir is not None:
        env["LD_LIBRARY_PATH"] = str(library_dir)
    return env


def start_logged(name, argv, root, env, *, pass_fds=(), stdout=None):
    log = (root / f"{name}.log").open("wb")
    process = subprocess.Popen(
        argv,
        cwd=root,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=stdout if stdout is not None else log,
        stderr=log if stdout is not None else subprocess.STDOUT,
        pass_fds=pass_fds,
        start_new_session=True,
    )
    print(f"START {name} pid={process.pid}", flush=True)
    return process, log


def stop_process(process, name):
    if process is None or process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)
    print(f"STOP {name} pid={process.pid}", flush=True)


def read_display_number(process, read_fd, log_path):
    deadline = time.monotonic() + 15
    with selectors.DefaultSelector() as selector:
        selector.register(read_fd, selectors.EVENT_READ)
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(
                    f"private Xorg exited with {process.returncode}:\n"
                    f"{log_path.read_text(errors='replace')}"
                )
            events = selector.select(min(0.25, deadline - time.monotonic()))
            if not events:
                continue
            data = os.read(read_fd, 64)
            if not data:
                raise RuntimeError("private Xorg closed its display-number pipe")
            line, separator, _rest = data.partition(b"\n")
            if separator and line.isdigit():
                return int(line)
            if separator:
                raise RuntimeError(f"private Xorg returned an invalid display: {data!r}")
    raise RuntimeError("private Xorg did not publish a display number")


def run_tool(xdotool, env, *args, timeout=10):
    result = subprocess.run(
        [str(xdotool), *map(str, args)],
        cwd=Path(env["HOME"]),
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
    )
    if result.returncode:
        raise RuntimeError(
            f"xdotool {' '.join(map(str, args))} exited {result.returncode}: "
            f"{result.stderr.decode(errors='replace')}"
        )
    return result.stdout.decode(errors="replace").strip()


def visible_windows(xdotool, env, pid):
    result = subprocess.run(
        [str(xdotool), "search", "--onlyvisible", "--pid", str(pid)],
        cwd=Path(env["HOME"]),
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=5,
    )
    return sorted(result.stdout.decode().split(), key=int) if result.returncode == 0 else []


def read_log(path):
    try:
        return path.read_text(errors="replace")
    except FileNotFoundError:
        return ""


def pty_bytes(root):
    paths = [root / "pty.dump", *sorted(root.glob("pty.dump.[0-9]*"))]
    return b"\n".join(
        path.read_bytes()
        for path in paths
        if path.is_file() and not path.name.endswith(".chunks")
    )


def shell_births(path):
    return re.findall(r"^SHELL_BIRTH pid=(\d+)", read_log(path), re.MULTILINE)


def process_rows(pids=None):
    command = ["ps", "-eo", "pid=,ppid=,pgid=,sid=,tty=,stat=,args="]
    if pids:
        command = [
            "ps",
            "-p",
            ",".join(map(str, sorted(pids))),
            "-o",
            "pid=,ppid=,pgid=,sid=,tty=,stat=,args=",
        ]
    result = subprocess.run(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=5,
    )
    if result.returncode and not (pids and result.returncode == 1 and not result.stdout.strip()):
        raise RuntimeError(
            f"ps exited with {result.returncode}: {result.stderr.decode(errors='replace')}"
        )
    rows = {}
    for line in result.stdout.decode(errors="replace").splitlines():
        fields = line.split(maxsplit=6)
        if len(fields) == 7:
            rows[fields[0]] = fields
    return rows


def live_shell_processes(root, app_pid):
    births = shell_births(root / "shells.log")
    rows = process_rows()
    wrappers = {
        pid: rows[pid]
        for pid in births
        if pid in rows and "probe-shell" in rows[pid][6] and not rows[pid][5].startswith("Z")
    }
    bash_children = {
        fields[0]: fields
        for fields in rows.values()
        if fields[1] in wrappers
        and "bash --noprofile --norc" in fields[6]
        and not fields[5].startswith("Z")
    }
    if len(births) != 2 or len(wrappers) != 2 or len(bash_children) != 2:
        raise RuntimeError(
            f"expected two live shell wrappers and their bash children; "
            f"births={births} wrappers={wrappers} children={bash_children}"
        )
    if any(fields[1] != str(app_pid) for fields in wrappers.values()):
        raise RuntimeError(f"a shell wrapper is not owned by Folio pid {app_pid}: {wrappers}")
    if len({fields[4] for fields in wrappers.values()}) != 2:
        raise RuntimeError(f"the two shell wrappers do not own distinct PTYs: {wrappers}")
    return set(wrappers) | set(bash_children), wrappers, bash_children


def terminate_shell_processes(pids):
    if not pids:
        return
    for fields in process_rows(pids).values():
        try:
            os.killpg(int(fields[2]), signal.SIGKILL)
        except ProcessLookupError:
            pass


def wait_for(predicate, description, *, seconds=45):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {description}")


def screenshot(path, display_name):
    class XImage(ctypes.Structure):
        _fields_ = [
            (name, ctypes.c_int)
            for name in ("width", "height", "xoffset", "format")
        ] + [("data", ctypes.c_void_p)] + [
            (name, ctypes.c_int)
            for name in (
                "byte_order",
                "bitmap_unit",
                "bitmap_bit_order",
                "bitmap_pad",
                "depth",
                "bytes_per_line",
                "bits_per_pixel",
            )
        ] + [
            ("red_mask", ctypes.c_ulong),
            ("green_mask", ctypes.c_ulong),
            ("blue_mask", ctypes.c_ulong),
            ("obdata", ctypes.c_void_p),
            ("functions", ctypes.c_void_p * 6),
        ]

    lib = ctypes.CDLL("libX11.so.6")
    lib.XOpenDisplay.argtypes = [ctypes.c_char_p]
    lib.XOpenDisplay.restype = ctypes.c_void_p
    lib.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    lib.XDefaultRootWindow.restype = ctypes.c_ulong
    lib.XGetImage.argtypes = [
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_ulong,
        ctypes.c_int,
    ]
    lib.XGetImage.restype = ctypes.POINTER(XImage)
    lib.XDestroyImage.argtypes = [ctypes.POINTER(XImage)]
    lib.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = lib.XOpenDisplay(display_name.encode())
    if not display:
        raise RuntimeError("could not open private X display for capture")
    image = lib.XGetImage(
        display,
        lib.XDefaultRootWindow(display),
        0,
        0,
        1280,
        800,
        ctypes.c_ulong(-1).value,
        2,
    )
    if not image or image.contents.bits_per_pixel != 32:
        raise RuntimeError("private X display did not return a 32-bit root image")
    data = image.contents
    pixels = ctypes.string_at(data.data, data.bytes_per_line * data.height)
    Image.frombytes(
        "RGB",
        (data.width, data.height),
        pixels,
        "raw",
        "BGRX",
        data.bytes_per_line,
        1,
    ).save(path)
    lib.XDestroyImage(image)
    lib.XCloseDisplay(display)


def sprite_center(path, mark):
    for line in reversed(read_log(path).splitlines()):
        if "sprite [" not in line or f"mark={mark}" not in line:
            continue
        found = re.search(r"sprite \[([0-9.]+), ([0-9.]+), ([0-9.]+), ([0-9.]+)\]", line)
        if found:
            x1, y1, x2, y2 = map(float, found.groups())
            return round((x1 + x2) / 2), round((y1 + y2) / 2), line
    return None


def float_button_center(path):
    return sprite_center(path, "Float")


def latest_chrome_frame(path):
    frames = re.split(r"(?=--- chrome frame:)", read_log(path))
    return frames[-1]


def shell_ready(root):
    return pty_bytes(root).count(b"FOLIO_FLOAT_TRANSFER_SHELL_READY")


def write_pdf_fixture(path):
    stream = b"BT /F1 24 Tf 72 720 Td (FOLIO PDF FLOAT TRANSFER) Tj ET\n"
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


def latest_preview_bounds(trace_path, xdotool, env, app_pid):
    page_generation = re.compile(
        r"page=(PageVisual \{ tab: \d+, seat: \d+ \}) generation=(\d+)"
    )
    bounds = re.compile(
        r"bounds=WebBounds \{ x: (-?\d+), y: (-?\d+), width: (\d+), height: (\d+) \}"
    )
    lines = read_log(trace_path).splitlines()
    current_page = None
    for line in lines:
        if "linux_frame received" not in line or "accepted=true" not in line:
            continue
        match = page_generation.search(line)
        if match:
            current_page = match.groups()

    for line in reversed(lines):
        if "linux_frame layer page=PageVisual" not in line:
            continue
        identity = page_generation.search(line)
        match = bounds.search(line)
        if identity and match and (current_page is None or identity.groups() == current_page):
            x, y, width, height = map(int, match.groups())
            if width > 0 and height > 0:
                windows = visible_windows(xdotool, env, app_pid)
                if len(windows) != 1:
                    return None
                geometry = run_tool(
                    xdotool,
                    env,
                    "getwindowgeometry",
                    "--shell",
                    windows[0],
                )
                origin = dict(re.findall(r"(?m)^([A-Z]+)=(-?\d+)$", geometry))
                if "X" not in origin or "Y" not in origin:
                    return None
                return int(origin["X"]) + x, int(origin["Y"]) + y, width, height
    return None


def screenshot_has_marker(path, tesseract, marker, preview_bounds=None):
    image_path = path
    if preview_bounds is not None:
        x, y, width, height = preview_bounds
        with Image.open(path) as screen:
            left = max(0, x)
            top = max(0, y)
            right = min(screen.width, x + width)
            bottom = min(screen.height, y + height)
            if right > left and bottom > top:
                image_path = path.with_name(f"{path.stem}-preview-area{path.suffix}")
                screen.crop((left, top, right, bottom)).save(image_path)
    result = subprocess.run(
        [str(tesseract), str(image_path), "stdout", "--psm", "6"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=15,
    )
    if result.returncode:
        raise RuntimeError(f"tesseract exited {result.returncode}: {result.stderr.decode(errors='replace')}")
    recognized = re.sub(r"\s+", " ", result.stdout.decode(errors="replace")).upper()
    return marker in recognized


def browser_processes(root):
    rows = process_rows()
    return {
        pid: fields
        for pid, fields in rows.items()
        if "--user-data-dir=" in fields[6] and str(root) in fields[6]
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--xorg", type=Path, required=True)
    parser.add_argument("--modulepath", type=Path, required=True)
    parser.add_argument("--xdotool", type=Path, required=True)
    parser.add_argument("--openbox", type=Path, required=True)
    parser.add_argument("--openbox-config", type=Path, required=True)
    parser.add_argument("--xorg-config", type=Path, default=Path(__file__).with_name("linux-input-xorg.conf"))
    parser.add_argument("--library-dir", type=Path)
    parser.add_argument("--xdg-data-dirs", required=True)
    parser.add_argument("--xdg-config-dirs", required=True)
    parser.add_argument("--document-type", choices=("markdown", "html", "pdf"), default="markdown")
    parser.add_argument(
        "--continue-after-visual-failure",
        action="store_true",
        help="record a blank web surface, finish move/close/reap checks, then exit nonzero",
    )
    parser.add_argument(
        "--dock-before-quit",
        action="store_true",
        help="dock the moved web page into a pane and verify it remains visible before clean quit",
    )
    parser.add_argument("--artifacts", type=Path, default=Path("target/linux-preview-float-transfer"))
    args = parser.parse_args()

    executable = args.exe.resolve()
    xorg = args.xorg.resolve()
    modulepath = args.modulepath.resolve()
    xdotool = args.xdotool.resolve()
    openbox = args.openbox.resolve()
    openbox_config = args.openbox_config.resolve()
    xorg_config = args.xorg_config.resolve()
    for path, description in (
        (executable, "Folio executable"),
        (xorg, "Xorg executable"),
        (xdotool, "xdotool executable"),
        (openbox, "Openbox executable"),
        (openbox_config, "Openbox config"),
        (xorg_config, "Xorg config"),
    ):
        if not path.is_file() or ("executable" in description and not os.access(path, os.X_OK)):
            parser.error(f"{description} is missing or not executable: {path}")
    if not modulepath.is_dir():
        parser.error(f"Xorg module path is missing: {modulepath}")
    if args.library_dir is not None and not args.library_dir.resolve().is_dir():
        parser.error(f"library directory is missing: {args.library_dir}")

    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="linux-preview-float-transfer-", dir=artifacts))
    print(f"ARTIFACTS={root}", flush=True)
    for name in ("home", "config", "data", "cache", "state"):
        (root / name).mkdir(parents=True, exist_ok=True)
    if args.document_type == "pdf":
        fixture = root / "fixture.pdf"
        write_pdf_fixture(fixture)
        tesseract = shutil.which("tesseract")
        if tesseract is None:
            parser.error("PDF visual acceptance requires tesseract on PATH")
        marker = "FOLIO PDF FLOAT TRANSFER"
    elif args.document_type == "html":
        fixture = root / "fixture.html"
        write_html_fixture(fixture)
        tesseract = shutil.which("tesseract")
        if tesseract is None:
            parser.error("HTML visual acceptance requires tesseract on PATH")
        marker = "FOLIO HTML FLOAT TRANSFER"
    else:
        fixture = root / "fixture.md"
        fixture.write_text(
            "# Float transfer\n\n"
            "The preview stays attached to its tab.\n\n"
            "$$\\int_0^1 x^2\\,dx = \\frac{1}{3}$$\n"
        )
    visual_failures = []

    def check_visual_marker(path, stage, should_be_visible):
        deadline = time.monotonic() + 5.0
        while True:
            screenshot(path, env["DISPLAY"])
            visible = screenshot_has_marker(
                path,
                tesseract,
                marker,
                latest_preview_bounds(root / "web.trace", xdotool, env, app.pid),
            )
            if visible == should_be_visible:
                return True
            if time.monotonic() >= deadline:
                break
            time.sleep(0.25)
        verdict = "missing" if should_be_visible else "still visible"
        message = f"the {args.document_type.upper()} canary was {verdict} at {stage}"
        visual_failures.append(message)
        print(f"FAIL visual {message}; screenshot={path}", flush=True)
        if not args.continue_after_visual_failure:
            raise RuntimeError(message)
        return False
    shell = root / "probe-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "printf 'SHELL_BIRTH pid=%s ppid=%s tty=%s\\n' \"$$\" \"$PPID\" \"$(tty)\" >> \"$FOLIO_SHELL_LEDGER\"\n"
        "printf 'FOLIO_FLOAT_TRANSFER_SHELL_READY '; stty size\n"
        "/bin/bash --noprofile --norc\n"
    )
    shell.chmod(0o700)

    with tempfile.TemporaryDirectory(prefix="folio-float-transfer-runtime-") as runtime_name:
        runtime = Path(runtime_name)
        runtime.chmod(0o700)
        env = clean_environment(
            root,
            runtime,
            args.library_dir.resolve() if args.library_dir else None,
        )
        env.update(
            SHELL=str(shell),
            FOLIO_SHELL_LEDGER=str(root / "shells.log"),
            XDG_DATA_DIRS=args.xdg_data_dirs,
            XDG_CONFIG_DIRS=args.xdg_config_dirs,
        )
        server = None
        server_log = None
        wm = None
        wm_log = None
        app = None
        app_log = None
        shells_to_reap = set()
        try:
            read_fd, write_fd = os.pipe()
            try:
                server, server_log = start_logged(
                    "private-xorg",
                    [
                        str(xorg),
                        "-displayfd",
                        str(write_fd),
                        "-config",
                        str(xorg_config),
                        "-modulepath",
                        str(modulepath),
                        "-logfile",
                        str(root / "Xorg.log"),
                        "-nolisten",
                        "tcp",
                        "-ac",
                        "-noreset",
                        "-novtswitch",
                        "-sharevts",
                    ],
                    root,
                    env,
                    pass_fds=(write_fd,),
                )
            finally:
                os.close(write_fd)
            display_number = read_display_number(server, read_fd, root / "private-xorg.log")
            os.close(read_fd)
            env["DISPLAY"] = f":{display_number}"
            print(f"READY private Xorg display={env['DISPLAY']}", flush=True)

            wm, wm_log = start_logged(
                "openbox",
                [str(openbox), "--config-file", str(openbox_config)],
                root,
                env,
            )
            time.sleep(0.3)
            if wm.poll() is not None:
                raise RuntimeError(f"Openbox exited: {read_log(root / 'openbox.log')}")

            app_env = env.copy()
            app_env["BT_PTY_DUMP"] = str(root / "pty.dump")
            if args.document_type in ("html", "pdf"):
                app_env["BT_WEB_TRACE"] = str(root / "web.trace")
            app, app_log = start_logged(
                "folio",
                [str(executable), "--profile", "usershell", "--cwd", str(root), str(fixture)],
                root,
                app_env,
            )
            wait_for(
                lambda: visible_windows(xdotool, env, app.pid)
                and "BT_STARTUP first_text_present=" in read_log(root / "folio.log")
                and shell_ready(root) >= 1,
                "first window, preview, and shell",
            )
            source_window = visible_windows(xdotool, env, app.pid)[0]
            run_tool(xdotool, env, "windowactivate", "--sync", source_window)
            run_tool(xdotool, env, "key", "--clearmodifiers", "Escape")
            if args.document_type in ("html", "pdf"):
                wait_for(
                    lambda: any(
                        "navigation_completed" in line
                        and "success=1" in line
                        and fixture.name in line
                        for line in read_log(root / "web.trace").splitlines()
                    ),
                    f"the local {args.document_type.upper()} page to finish loading in Chromium",
                )
                pane_visible = check_visual_marker(
                    root / f"{args.document_type}-in-preview-pane.png",
                    "the preview pane",
                    True,
                )
                if pane_visible:
                    print(
                        f"PASS Chromium rendered the local {args.document_type.upper()} canary in the preview pane",
                        flush=True,
                    )
            run_tool(xdotool, env, "mousemove", "--window", source_window, 700, 55)
            time.sleep(0.35)
            float_button = float_button_center(root / "chrome.dump")
            if not float_button:
                screenshot(root / "missing-float-control.png", env["DISPLAY"])
                raise RuntimeError("the Markdown preview pane did not offer its Float control")
            run_tool(xdotool, env, "mousemove", "--window", source_window, float_button[0], float_button[1])
            run_tool(xdotool, env, "click", "1")
            wait_for(
                lambda: "mark=DockRight" in read_log(root / "chrome.dump"),
                "a pinned float with its Dock action",
            )
            if args.document_type in ("html", "pdf"):
                float_visible = check_visual_marker(
                    root / "preview-float-in-source-window.png",
                    "the floated preview",
                    True,
                )
                if float_visible:
                    print(
                        f"PASS the {args.document_type.upper()} canary stayed visible after the page was floated",
                        flush=True,
                    )
            else:
                screenshot(root / "preview-float-in-source-window.png", env["DISPLAY"])
            print(f"FLOAT_CONTROL {float_button[2]}", flush=True)

            run_tool(xdotool, env, "mousemove", "--window", source_window, 200, 200)
            run_tool(xdotool, env, "click", "1")
            time.sleep(0.3)
            run_tool(xdotool, env, "type", "--clearmodifiers", "export C=kept")
            run_tool(xdotool, env, "key", "Return")
            wait_for(
                lambda: b"export C=kept" in pty_bytes(root),
                "the source shell to keep its transfer cookie",
            )
            source_recording = next(
                path
                for path in [root / "pty.dump", *sorted(root.glob("pty.dump.[0-9]*"))]
                if path.is_file()
                and not path.name.endswith(".chunks")
                and b"export C=kept" in path.read_bytes()
            )

            run_tool(xdotool, env, "key", "--clearmodifiers", "ctrl+shift+m")
            wait_for(
                lambda: len(visible_windows(xdotool, env, app.pid)) == 2 and shell_ready(root) >= 2,
                "a second native window and its shell",
            )
            chrome_path = root / "chrome.dump"
            windows = visible_windows(xdotool, env, app.pid)
            source_window = windows[0]
            target_window = windows[1]
            run_tool(xdotool, env, "windowactivate", "--sync", source_window)
            run_tool(xdotool, env, "mousemove", "--window", source_window, 100, 20)
            run_tool(xdotool, env, "click", "3")
            time.sleep(0.2)
            for _ in range(5):
                run_tool(xdotool, env, "key", "Down")
            run_tool(xdotool, env, "key", "Right")
            run_tool(xdotool, env, "key", "Return")
            wait_for(
                lambda: len(visible_windows(xdotool, env, app.pid)) == 1,
                "the empty source window to retire after its tab moves",
            )

            destination = visible_windows(xdotool, env, app.pid)[0]
            if destination != target_window:
                raise RuntimeError(
                    f"tab move retired the target window instead of the source: "
                    f"source={source_window} target={target_window} remaining={destination} "
                    f"artifacts={root}"
                )
            chrome_size_after_source_close = chrome_path.stat().st_size
            run_tool(xdotool, env, "windowactivate", "--sync", destination)
            run_tool(xdotool, env, "mousemove", "--window", destination, 200, 200)
            run_tool(xdotool, env, "click", "1")
            time.sleep(0.3)
            run_tool(
                xdotool,
                env,
                "type",
                "--clearmodifiers",
                'printf "FLOAT_MOVE_COOKIE=%s\\n" "${C-absent}"',
            )
            run_tool(xdotool, env, "key", "Return")
            wait_for(
                lambda: b"FLOAT_MOVE_COOKIE=kept" in source_recording.read_bytes(),
                "the same shell session to remain in the moved tab",
            )
            shells_to_reap, wrappers, bash_children = live_shell_processes(root, app.pid)
            print(
                f"SHELL_OWNERSHIP wrappers={wrappers} bash_children={bash_children}",
                flush=True,
            )
            time.sleep(0.5)
            if args.document_type in ("html", "pdf"):
                destination_visible = check_visual_marker(
                    root / "preview-tab-in-destination-window.png",
                    "the destination window after the tab move",
                    True,
                )
                if destination_visible:
                    print(
                        f"PASS the {args.document_type.upper()} canary stayed visible after the tab moved windows",
                        flush=True,
                    )
            else:
                screenshot(root / "preview-tab-in-destination-window.png", env["DISPLAY"])
            transferred_frames = chrome_path.read_bytes()[chrome_size_after_source_close:].decode(
                errors="replace"
            )
            if not any(
                "mark=DockRight" in frame
                for frame in re.split(r"(?=--- chrome frame:)", transferred_frames)
            ):
                raise RuntimeError(
                    "the moved tab kept its shell cookie but no frame after source close drew the "
                    "floated preview or Dock control; "
                    f"window={destination} title={run_tool(xdotool, env, 'getwindowname', destination)!r} "
                    f"artifacts={root}"
                )
            print(
                f"STRUCTURAL_PASS floated {args.document_type} preview and its tab moved into the target native window; "
                f"cookie remained in {source_recording.name}",
                flush=True,
            )

            if args.document_type in ("html", "pdf"):
                docked_for_exit = False
                if args.dock_before_quit:
                    dock_button = sprite_center(chrome_path, "DockRight")
                    if dock_button is None:
                        raise RuntimeError("the moved floated preview did not expose DockRight")
                    chrome_size_before_dock = chrome_path.stat().st_size
                    web_trace_path = root / "web.trace"
                    web_trace_size_before_dock = web_trace_path.stat().st_size
                    run_tool(
                        xdotool,
                        env,
                        "mousemove",
                        "--window",
                        destination,
                        dock_button[0],
                        dock_button[1],
                    )
                    run_tool(xdotool, env, "click", "1")

                    def the_web_page_is_docked():
                        chrome = latest_chrome_frame(chrome_path)
                        trace = web_trace_path.read_text(errors="replace")
                        trace = trace[web_trace_size_before_dock:]
                        return (
                            chrome_path.stat().st_size > chrome_size_before_dock
                            and "mark=Globe" in chrome
                            and "mark=DockRight" not in chrome
                            and "floated=-" in trace
                            and "stage=Seat" in trace
                        )

                    wait_for(the_web_page_is_docked, "the moved web page to dock into its tab")
                    docked_visible = check_visual_marker(
                        root / f"{args.document_type}-after-dock.png",
                        "after docking the moved web page into its tab",
                        True,
                    )
                    if docked_visible:
                        print(f"PASS docking the {args.document_type.upper()} kept its page visible", flush=True)
                    docked_for_exit = True

                owned_browsers = browser_processes(root)
                if not owned_browsers:
                    raise RuntimeError(
                        f"the loaded {args.document_type.upper()} had no Chromium child using the private profile"
                    )
                print(f"BROWSER_OWNERSHIP {owned_browsers}", flush=True)
                if not docked_for_exit:
                    close_button = sprite_center(chrome_path, "PaneClose")
                    if close_button is None:
                        raise RuntimeError("the floated preview did not expose its Close control")
                    run_tool(
                        xdotool,
                        env,
                        "mousemove",
                        "--window",
                        destination,
                        close_button[0],
                        close_button[1],
                    )
                    run_tool(xdotool, env, "click", "1")
                    withdrawn = check_visual_marker(
                        root / f"{args.document_type}-after-float-close.png",
                        "after the preview float was closed",
                        False,
                    )
                    if withdrawn:
                        print(f"PASS closing the floated {args.document_type.upper()} withdrew its page", flush=True)

            run_tool(xdotool, env, "key", "--clearmodifiers", "ctrl+shift+q")
            if app.wait(timeout=20) != 0:
                raise RuntimeError(f"Folio exited with {app.returncode}")
            remaining_shells = process_rows(shells_to_reap)
            if remaining_shells:
                raise RuntimeError(
                    f"Folio exited but its two shell processes were not reaped: {remaining_shells}"
                )
            shells_to_reap.clear()
            if args.document_type in ("html", "pdf"):
                wait_for(
                    lambda: not browser_processes(root),
                    "the app's private Chromium process group to exit after clean application quit",
                    seconds=15,
                )
                print("PASS clean application quit reaped its private Chromium process group", flush=True)
            if visual_failures:
                raise RuntimeError(
                    "visual acceptance failed after diagnostic lifecycle checks: "
                    + "; ".join(visual_failures)
                )
            print("PASS both moved-tab shells were reaped after clean application quit", flush=True)
            print("PASS clean quit after preview transfer", flush=True)
        finally:
            if app is not None and app.poll() is None:
                windows = visible_windows(xdotool, env, app.pid)
                if windows:
                    try:
                        run_tool(xdotool, env, "windowactivate", "--sync", windows[-1])
                        run_tool(xdotool, env, "key", "--clearmodifiers", "ctrl+shift+q")
                        app.wait(timeout=10)
                    except (RuntimeError, subprocess.TimeoutExpired):
                        pass
            stop_process(app, "folio")
            terminate_shell_processes(shells_to_reap)
            stop_process(wm, "openbox")
            stop_process(server, "private-xorg")
            for log in (app_log, wm_log, server_log):
                if log is not None:
                    log.close()


if __name__ == "__main__":
    main()
