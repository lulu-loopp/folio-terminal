#!/usr/bin/env python3
"""Exercise Folio's native Wayland keyboard protocols through nested Niri."""

import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time

repo = Path(__file__).resolve().parents[2]


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


smoke = load_module("folio_linux_smoke", repo / "scripts/ci/linux-smoke.py")
input_smoke = load_module("folio_linux_input_smoke", repo / "scripts/ci/linux-input-smoke.py")

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--exe", type=Path, default=repo / "target/debug/folio", help="Linux Folio executable")


def default_executable(name):
    found = shutil.which(name)
    return Path(found) if found else None


parser.add_argument("--niri", type=Path, default=default_executable("niri"), help="Niri compositor executable")
parser.add_argument("--xvfb", type=Path, default=default_executable("Xvfb"), help="private outer X server")
parser.add_argument("--xdotool", type=Path, default=default_executable("xdotool"), help="XTest keyboard driver")
parser.add_argument("--setxkbmap", type=Path, default=default_executable("setxkbmap"), help="private outer XKB layout setter")
parser.add_argument("--xdotool-libdir", type=Path, help="library directory for an extracted xdotool binary")
parser.add_argument("--artifacts", type=Path, default=repo / "target/linux-wayland-keyboard-smoke")
args = parser.parse_args()
for name in ("niri", "xvfb", "xdotool", "setxkbmap"):
    if getattr(args, name) is None:
        parser.error(f"{name} executable not found; pass --{name.replace('_', '-')}")

exe = args.exe.resolve()
niri = args.niri.resolve()
xvfb = args.xvfb.resolve()
xdotool = args.xdotool.resolve()
setxkbmap = args.setxkbmap.resolve()
artifacts = args.artifacts
artifacts.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix="session-", dir=artifacts))
root.chmod(0o700)
processes = []


def start(name, argv, env, log_path):
    log = log_path.open("wb")
    process = subprocess.Popen(
        argv,
        cwd=root,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    print(f"START {name} pid={process.pid}", flush=True)
    processes.append((name, process, log))
    return process


def run(argv, env, timeout=8):
    return subprocess.run(argv, env=env, capture_output=True, text=True, timeout=timeout)


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
        print(f"STOP {name} pid={process.pid}", flush=True)
        log.close()
    processes.clear()
    print(f"ARTIFACTS {root}", flush=True)


def focus_state(env):
    result = input_smoke.run_tool([str(niri), "msg", "--json", "windows"], env)
    windows = __import__("json").loads(result)
    return next((window["is_focused"] for window in windows if window.get("app_id") == "io.github.lulu-loopp.folio"), None)


display_context = smoke.private_display("x11", root, xvfb)
host_env = display_context.__enter__()
try:
    runtime = Path(host_env["XDG_RUNTIME_DIR"])
    for name in ("NIRI_SOCKET", "NIRI_CONFIG", "WAYLAND_DISPLAY", "WAYLAND_SOCKET"):
        host_env.pop(name, None)
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
        [str(niri), "validate", "--config", str(config)],
        cwd=root,
        env=host_env,
        capture_output=True,
        text=True,
        timeout=10,
    )
    if validation.returncode:
        raise RuntimeError(f"private Niri config is invalid: {validation.stderr or validation.stdout}")
    niri_env = host_env.copy()
    niri_env["RUST_LOG"] = "niri=info"
    niri_process = start("nested private Niri", [str(niri), "--config", str(config)], niri_env, root / "niri.log")

    def find_sockets():
        displays = [path for path in runtime.glob("wayland-*") if path.is_socket()]
        ipcs = sorted(runtime.glob("niri.*.sock"))
        return (displays[0], ipcs[0]) if displays and ipcs else None

    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and find_sockets() is None:
        if niri_process.poll() is not None:
            raise RuntimeError(f"nested Niri exited: {smoke._log_text(root / 'niri.log')[-2000:]}")
        time.sleep(0.025)
    if find_sockets() is None:
        raise RuntimeError(f"nested Niri did not create private sockets: {smoke._log_text(root / 'niri.log')[-2000:]}")
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

    x11_env = host_env.copy()
    if args.xdotool_libdir:
        old_library_path = x11_env.get("LD_LIBRARY_PATH", "")
        x11_env["LD_LIBRARY_PATH"] = os.pathsep.join(
            value for value in (str(args.xdotool_libdir.resolve()), old_library_path) if value
        )
    input_smoke.run_tool([str(setxkbmap), "-layout", "us"], x11_env)

    def xtest(*args):
        result = input_smoke.run_tool([str(xdotool), *args], x11_env)
        return result

    pty_path = root / "pty.dump"
    pty_input_path = root / "pty.input"
    ime_trace = root / "ime.trace"
    pid_path = root / "shell.pid"
    prompt_path = root / "bashrc"
    prompt_path.write_text("PS1='FOLIO_INPUT_PROMPT> '\n")
    key_reader = root / "key-reader.py"
    key_reader.write_text(input_smoke.KEY_READER)
    ready_dir = root / "key-reader-ready"
    result_dir = root / "key-reader-results"
    ready_dir.mkdir()
    result_dir.mkdir()
    start_readers = root / "start-key-readers"
    shell = root / "probe-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "test -t 0 && test -t 1 || exit 9\n"
        "printf 'FOLIO_WAYLAND_PTY_READY '; stty size\n"
        "printf '%s\\n' \"$$\" > \"$FOLIO_CHILD_PID\"\n"
        "while [ ! -f \"$FOLIO_START_KEY_READERS\" ]; do sleep 0.025; done\n"
        "python3 \"$FOLIO_KEY_READER\" kitty \"$FOLIO_KEY_READY_DIR/kitty.ready\" \"$FOLIO_KEY_RESULT_DIR/kitty.bytes\" || exit 21\n"
        "python3 \"$FOLIO_KEY_READER\" modify-other-keys \"$FOLIO_KEY_READY_DIR/modify-other-keys.ready\" \"$FOLIO_KEY_RESULT_DIR/modify-other-keys.bytes\" || exit 22\n"
        "python3 \"$FOLIO_KEY_READER\" alt \"$FOLIO_KEY_READY_DIR/alt.ready\" \"$FOLIO_KEY_RESULT_DIR/alt.bytes\" || exit 23\n"
        "exec /bin/bash --noprofile --rcfile \"$FOLIO_BASH_RC\"\n"
    )
    shell.chmod(0o700)
    app_env = client_env.copy()
    app_env.update(
        SHELL=str(shell),
        FOLIO_CHILD_PID=str(pid_path),
        FOLIO_BASH_RC=str(prompt_path),
        FOLIO_KEY_READER=str(key_reader),
        FOLIO_KEY_READY_DIR=str(ready_dir),
        FOLIO_KEY_RESULT_DIR=str(result_dir),
        FOLIO_START_KEY_READERS=str(start_readers),
        TERM="xterm-256color",
        LC_ALL="C.UTF-8",
        HISTFILE="/dev/null",
        BT_STARTUP_TRACE="1",
        BT_PTY_DUMP=str(pty_path),
        BT_PTY_INPUT_DUMP=str(pty_input_path),
        BT_IME_TRACE=str(ime_trace),
    )
    app = start("Folio native Wayland", [str(exe), "--profile", "usershell", "--cwd", str(root)], app_env, root / "folio.log")
    wait_for_window = time.monotonic() + 60
    while time.monotonic() < wait_for_window:
        if b"FOLIO_WAYLAND_PTY_READY" in input_smoke.pty_bytes(pty_path) and "BT_STARTUP first_text_present=" in smoke._log_text(root / "folio.log"):
            break
        if app.poll() is not None:
            raise RuntimeError(f"Folio exited before PTY presentation: {smoke._log_text(root / 'folio.log')[-1800:]}")
        time.sleep(0.05)
    else:
        raise RuntimeError(f"Folio did not present PTY text: {smoke._log_text(root / 'folio.log')[-1800:]}")
    input_smoke.wait_file(pid_path, app, root / "folio.log", "PTY shell pid")
    child_pid = int(pid_path.read_text().strip())
    child_start_time = input_smoke.process_identity(child_pid)[1]
    wait_until = time.monotonic() + 20
    while time.monotonic() < wait_until and focus_state(client_env) is not True:
        time.sleep(0.025)
    if focus_state(client_env) is not True:
        raise RuntimeError("Folio did not acquire nested Niri focus")

    xwindows = xtest("search", "--onlyvisible", "--name", ".")
    if not xwindows:
        raise RuntimeError("nested Niri outer window is not visible on private Xvfb")
    niri_window = xwindows.splitlines()[0]
    xtest("windowfocus", "--sync", niri_window)
    if xtest("getwindowfocus") != niri_window:
        raise RuntimeError("nested Niri outer window did not receive XTest focus")
    input_smoke.dismiss_private_first_run_card(ime_trace, app, root / "folio.log", str(xdotool), x11_env)
    print("PASS private XTest keyboard reached focused nested Niri", flush=True)

    start_readers.touch()
    for mode, chord, expected, label in (
        ("kitty", "shift+Return", "1b5b31333b3275", "Kitty Shift+Enter"),
        ("modify-other-keys", "ctrl+Return", "1b5b32373b353b31337e", "modifyOtherKeys Ctrl+Enter"),
        ("alt", "alt+q", "1b71", "Alt prefix"),
    ):
        ready = ready_dir / f"{mode}.ready"
        result = result_dir / f"{mode}.bytes"
        input_smoke.wait_file(ready, app, root / "folio.log", f"{mode} PTY reader")
        xtest("key", "--clearmodifiers", chord)
        input_smoke.wait_file(result, app, root / "folio.log", f"{mode} key bytes")
        actual = result.read_text().strip()
        if actual != expected:
            raise RuntimeError(f"{mode} expected {expected}, received {actual}")
        marker = {
            "kitty": f"FOLIO_KITTY_KEY={expected}",
            "modify-other-keys": f"FOLIO_MOK2_KEY={expected}",
            "alt": f"FOLIO_ALT_KEY={expected}",
        }[mode].encode()
        input_smoke.wait_for_bytes(
            pty_path,
            marker,
            app,
            root / "folio.log",
            f"{mode} visible PTY marker",
            quiet_limit=input_smoke.PTY_QUIET_LIMIT_SECONDS,
            absolute_limit=input_smoke.PTY_ABSOLUTE_LIMIT_SECONDS,
        )
        print(f"PASS native Wayland {label}: {actual}", flush=True)

    prompt = b"FOLIO_INPUT_PROMPT> "
    input_smoke.wait_for_bytes(
        pty_path,
        prompt,
        app,
        root / "folio.log",
        "interactive shell after raw-key probes",
        quiet_limit=input_smoke.PTY_QUIET_LIMIT_SECONDS,
        absolute_limit=input_smoke.PTY_ABSOLUTE_LIMIT_SECONDS,
    )
    xtest("key", "--clearmodifiers", "a")
    input_smoke.wait_for_bytes(
        pty_path,
        prompt + b"a",
        app,
        root / "folio.log",
        "Wayland XTest key reaching the PTY",
        quiet_limit=input_smoke.PTY_QUIET_LIMIT_SECONDS,
        absolute_limit=input_smoke.PTY_ABSOLUTE_LIMIT_SECONDS,
    )
    print("PASS XTest printable key reached the Wayland shell PTY", flush=True)

    xtest("key", "--clearmodifiers", "ctrl+shift+q")
    try:
        app.wait(timeout=15)
    except subprocess.TimeoutExpired:
        raise RuntimeError("Ctrl+Shift+Q did not close Folio on native Wayland")
    input_smoke.wait_reaped(child_pid, child_start_time)
    print("PASS native Wayland Ctrl+Shift+Q quit and reaped its PTY child", flush=True)
finally:
    stop_all()
    display_context.__exit__(None, None, None)
