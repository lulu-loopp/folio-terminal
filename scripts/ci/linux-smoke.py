#!/usr/bin/env python3
"""Present PTY text in Folio on an owned, headless Linux display."""

import argparse
from contextlib import contextmanager
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import socket
import subprocess
import tempfile
import time


def _clean_environment():
    env = os.environ.copy()
    for name in (
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "WAYLAND_SOCKET",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "DBUS_SESSION_BUS_ADDRESS",
        "SESSION_MANAGER",
        "DESKTOP_SESSION",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "PULSE_SERVER",
        "PIPEWIRE_REMOTE",
        "JACK_SERVER",
    ):
        env.pop(name, None)
    return env


def _stop_process(process, name):
    if process is None:
        return
    process_group = process.pid
    try:
        os.killpg(process_group, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process_group, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
    else:
        # Folio can own a shell after the window process receives SIGTERM. The
        # process group was created for this smoke only, so retire any member
        # that outlived its leader before releasing the private display.
        try:
            os.killpg(process_group, signal.SIGKILL)
        except ProcessLookupError:
            pass
    print(f"STOP {name} pid={process.pid}", flush=True)


def _start_logged_process(argv, env, cwd, log_path, name, *, pass_fds=(), stdout=None):
    with log_path.open("wb") as log:
        process = subprocess.Popen(
            argv,
            cwd=cwd,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=stdout if stdout is not None else log,
            stderr=log if stdout is not None else subprocess.STDOUT,
            pass_fds=pass_fds,
            start_new_session=True,
        )
    print(f"START {name} pid={process.pid}", flush=True)
    return process


def _log_text(path):
    try:
        return path.read_text(errors="replace")
    except FileNotFoundError:
        return ""


def _wait_for_socket(process, path, log_path, name):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(
                f"{name} exited with {process.returncode}:\n{_log_text(log_path)}"
            )
        probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            probe.connect(str(path))
            return
        except OSError:
            time.sleep(0.025)
        finally:
            probe.close()
    raise RuntimeError(f"{name} did not open {path}:\n{_log_text(log_path)}")


def _read_display_number(process, read_fd, log_path):
    deadline = time.monotonic() + 15
    with selectors.DefaultSelector() as selector:
        selector.register(read_fd, selectors.EVENT_READ)
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(
                    f"Xvfb exited with {process.returncode}:\n{_log_text(log_path)}"
                )
            events = selector.select(min(0.25, deadline - time.monotonic()))
            if not events:
                continue
            data = os.read(read_fd, 64)
            if not data:
                raise RuntimeError(f"Xvfb closed its display-number pipe:\n{_log_text(log_path)}")
            line, separator, _rest = data.partition(b"\n")
            if separator and line.isdigit():
                return int(line)
            if separator:
                raise RuntimeError(f"Xvfb returned an invalid display number: {data!r}")
    raise RuntimeError(f"Xvfb did not choose a display number:\n{_log_text(log_path)}")


@contextmanager
def private_display(backend, root, server_path):
    """Start one headless display and yield an environment isolated from the desktop."""
    home = root / "home"
    config = root / "config"
    cache = root / "cache"
    data = root / "data"
    for directory in (home, config, cache, data):
        directory.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="folio-smoke-") as runtime_name:
        runtime = Path(runtime_name)
        runtime.chmod(0o700)
        env = _clean_environment()
        env.update(
            HOME=str(home),
            XDG_RUNTIME_DIR=str(runtime),
            XDG_DATA_HOME=str(data),
            XDG_CONFIG_HOME=str(config),
            XDG_CACHE_HOME=str(cache),
            XDG_STATE_HOME=str(root / "state"),
            XDG_SESSION_TYPE=backend,
            LIBGL_ALWAYS_SOFTWARE="1",
            GST_AUDIO_SINK="fakesink",
            BT_GPU_PREFERENCE="low",
        )

        process = None
        read_fd = None
        try:
            if backend == "x11":
                read_fd, write_fd = os.pipe()
                try:
                    process = _start_logged_process(
                        [
                            str(server_path),
                            "-displayfd",
                            str(write_fd),
                            "-screen",
                            "0",
                            "1280x800x24",
                            "-nolisten",
                            "tcp",
                            "-ac",
                            "-noreset",
                        ],
                        env,
                        root,
                        root / "xvfb.log",
                        "private Xvfb",
                        pass_fds=(write_fd,),
                        stdout=subprocess.DEVNULL,
                    )
                finally:
                    os.close(write_fd)
                display_number = _read_display_number(process, read_fd, root / "xvfb.log")
                os.close(read_fd)
                read_fd = None
                display_socket = Path("/tmp/.X11-unix") / f"X{display_number}"
                _wait_for_socket(process, display_socket, root / "xvfb.log", "private Xvfb")
                env["DISPLAY"] = f":{display_number}"
                env["WINIT_UNIX_BACKEND"] = "x11"
                print(f"READY private Xvfb display={env['DISPLAY']}", flush=True)
            else:
                socket_name = f"folio-smoke-{os.getpid()}"
                process = _start_logged_process(
                    [
                        str(server_path),
                        "--backend=headless",
                        "--renderer=gl",
                        f"--socket={socket_name}",
                        "--width=1280",
                        "--height=800",
                        "--idle-time=0",
                        "--no-config",
                        f"--log={root / 'weston.log'}",
                    ],
                    env,
                    root,
                    root / "weston-console.log",
                    "private Weston",
                )
                display_socket = runtime / socket_name
                _wait_for_socket(process, display_socket, root / "weston.log", "private Weston")
                env["WAYLAND_DISPLAY"] = socket_name
                env["WINIT_UNIX_BACKEND"] = "wayland"
                print(f"READY private Weston socket={display_socket}", flush=True)
            yield env
        finally:
            if read_fd is not None:
                os.close(read_fd)
            _stop_process(process, f"private {backend} display")


def smoke_app(backend, executable, root, env):
    shell = root / "smoke-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "test -t 0 && test -t 1 || exit 9\n"
        "printf 'FOLIO_LINUX_PTY_READY '\n"
        "stty size\n"
        "exec /bin/sh\n"
    )
    shell.chmod(0o700)
    env = env.copy()
    env.update(
        SHELL=str(shell),
        BT_STARTUP_TRACE="1",
        BT_PERF_TRACE="1",
        BT_PTY_DUMP=str(root / "pty.dump"),
    )
    log = root / "startup.log"
    dump = root / "pty.dump"
    dump.unlink(missing_ok=True)
    process = None
    try:
        with log.open("wb") as output:
            process = subprocess.Popen(
                [str(executable), "--profile", "usershell", "--cwd", str(root)],
                cwd=root,
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=output,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
        print(f"START Folio pid={process.pid}", flush=True)
        deadline = time.monotonic() + 60
        last_activity = time.monotonic()
        observed_bytes = 0
        while True:
            trace = _log_text(log)
            pty = dump.read_bytes() if dump.exists() else b""
            if "GPU device reported an error" in trace:
                raise RuntimeError(f"GPU error during startup:\n{trace}")
            size = re.search(rb"FOLIO_LINUX_PTY_READY (\d+) (\d+)", pty)
            if (
                "BT_STARTUP first_text_present=" in trace
                and size
                and all(int(dimension) > 0 for dimension in size.groups())
                and "BT_HANG_PROBE dispatched=" in trace
            ):
                if "Folio's window thread has not answered" in trace:
                    raise RuntimeError(f"Hang reported on a responsive window:\n{trace}")
                print(
                    f"PASS {backend}: PTY output presented and hang probe dispatched; "
                    f"artifacts: {root}",
                    flush=True,
                )
                return
            if process.poll() is not None:
                raise RuntimeError(f"Folio exited with {process.returncode}:\n{trace}")
            now = time.monotonic()
            received = log.stat().st_size + len(pty)
            if received != observed_bytes:
                observed_bytes = received
                last_activity = now
            if now >= deadline or now - last_activity >= 15:
                raise RuntimeError(
                    f"No presented PTY text; idle={now - last_activity:.1f}s, "
                    f"PTY bytes={len(pty)}:\n{trace}"
                )
            time.sleep(0.05)
    finally:
        _stop_process(process, "Folio")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("backend", choices=("x11", "wayland"))
    parser.add_argument("--exe", type=Path, default=Path("target/debug/folio"))
    parser.add_argument("--artifacts", type=Path, default=Path("target/linux-smoke"))
    parser.add_argument("--xvfb", default=os.environ.get("FOLIO_XVFB"))
    parser.add_argument("--weston", default=os.environ.get("FOLIO_WESTON"))
    args = parser.parse_args()

    executable = args.exe.resolve()
    if not executable.is_file() or not os.access(executable, os.X_OK):
        parser.error(f"Folio executable is missing or not executable: {executable}")
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix=f"{args.backend}-", dir=artifacts))

    requested = args.xvfb if args.backend == "x11" else args.weston
    command = requested or ("Xvfb" if args.backend == "x11" else "weston")
    server_path = shutil.which(command)
    if server_path is None:
        parser.error(f"{command} is required for the private {args.backend} smoke")

    with private_display(args.backend, root, server_path) as env:
        smoke_app(args.backend, executable, root, env)


if __name__ == "__main__":
    main()
