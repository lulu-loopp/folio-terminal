#!/usr/bin/env python3
"""Exercise Xorg keyboard input, terminal modes, PTY resize, and child retirement."""

import argparse
import os
from pathlib import Path
import re
import select
import selectors
import shlex
import signal
import subprocess
import tempfile
import time


PTY_QUIET_LIMIT_SECONDS = 5.0
PTY_ABSOLUTE_LIMIT_SECONDS = 30.0
APP_QUIET_LIMIT_SECONDS = 15.0
APP_ABSOLUTE_LIMIT_SECONDS = 60.0


KEY_READER = r'''#!/usr/bin/env python3
import os
from pathlib import Path
import select
import sys
import termios
import time
import tty

mode, ready_name, result_name = sys.argv[1:]
ready = Path(ready_name)
result = Path(result_name)
expected_reply = b"\x1b[0n"
quiet_limit = 5.0
absolute_limit = 30.0


def read_exact(length):
    answer = bytearray()
    started = last_byte = time.monotonic()
    while len(answer) < length:
        now = time.monotonic()
        quiet = now - last_byte
        elapsed = now - started
        if quiet >= quiet_limit or elapsed >= absolute_limit:
            raise RuntimeError(
                f"silent waiting for {length} bytes; quiet={quiet:.2f}s "
                f"elapsed={elapsed:.2f}s bytes={bytes(answer)!r}"
            )
        if not select.select([0], [], [], min(quiet_limit - quiet, absolute_limit - elapsed))[0]:
            continue
        chunk = os.read(0, length - len(answer))
        if not chunk:
            raise RuntimeError(f"PTY closed; bytes={bytes(answer)!r}")
        answer.extend(chunk)
        last_byte = time.monotonic()
    return bytes(answer)


if not os.isatty(0) or not os.isatty(1):
    raise SystemExit("key probe needs a PTY on stdin and stdout")
saved = termios.tcgetattr(0)
reset = b""
try:
    tty.setraw(0, termios.TCSANOW)
    if mode == "kitty":
        os.write(1, b"\x1b[>1u\x1b[5n")
        if read_exact(len(expected_reply)) != expected_reply:
            raise RuntimeError("terminal did not answer DSR after the kitty mode request")
        expected = b"\x1b[13;2u"
        reset = b"\x1b[<u"
    elif mode == "modify-other-keys":
        os.write(1, b"\x1b[>4;2m\x1b[5n")
        if read_exact(len(expected_reply)) != expected_reply:
            raise RuntimeError("terminal did not answer DSR after the modifyOtherKeys request")
        expected = b"\x1b[27;5;13~"
        reset = b"\x1b[>4;0m"
    elif mode == "alt":
        expected = b"\x1bq"
    else:
        raise RuntimeError(f"unknown mode {mode!r}")
    ready.write_text("ready")
    actual = read_exact(len(expected))
    result.write_text(actual.hex())
    if actual != expected:
        raise RuntimeError(f"wanted {expected.hex()}, received {actual.hex()}")
finally:
    if reset:
        os.write(1, reset)
    termios.tcsetattr(0, termios.TCSANOW, saved)
if mode == "kitty":
    os.write(1, b"\r\nFOLIO_KITTY_KEY=1b5b31333b3275\r\n")
elif mode == "modify-other-keys":
    os.write(1, b"\r\nFOLIO_MOK2_KEY=1b5b32373b353b31337e\r\n")
else:
    os.write(1, b"\r\nFOLIO_ALT_KEY=1b71\r\n")
'''


def clean_environment(root, runtime):
    env = os.environ.copy()
    for name in (
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "WAYLAND_SOCKET",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "DBUS_SYSTEM_BUS_ADDRESS",
        "SESSION_MANAGER",
        "IBUS_ADDRESS",
        "IBUS_DAEMON",
        "XMODIFIERS",
        "GTK_IM_MODULE",
        "QT_IM_MODULE",
    ):
        env.pop(name, None)
    home = root / "home"
    config = root / "config"
    data = root / "data"
    cache = root / "cache"
    for directory in (home, config, data, cache):
        directory.mkdir(parents=True, exist_ok=True)
    env.update(
        HOME=str(home),
        XDG_CONFIG_HOME=str(config),
        XDG_DATA_HOME=str(data),
        XDG_CACHE_HOME=str(cache),
        XDG_STATE_HOME=str(root / "state"),
        XDG_RUNTIME_DIR=str(runtime),
        XDG_SESSION_TYPE="x11",
        WINIT_UNIX_BACKEND="x11",
        TERM="xterm-256color",
        LC_ALL="C.UTF-8",
        HISTFILE="/dev/null",
        LIBGL_ALWAYS_SOFTWARE="1",
        BT_GPU_PREFERENCE="low",
        BT_STARTUP_TRACE="1",
        BT_PTY_DUMP=str(root / "pty.dump"),
        BT_IME_TRACE=str(root / "ime.trace"),
        BT_MOUSE_TRACE=str(root / "mouse.trace"),
    )
    return env


def start_logged(name, argv, root, env, *, pass_fds=()):
    log = (root / f"{name}.log").open("wb")
    process = subprocess.Popen(
        argv,
        cwd=root,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=subprocess.STDOUT,
        pass_fds=pass_fds,
        start_new_session=True,
    )
    return process, log


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
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    if Path(f"/proc/{process.pid}").exists():
        raise RuntimeError(f"{name} process group leader {process.pid} survived cleanup")


def read_display_number(process, read_fd, log_path):
    deadline = time.monotonic() + 15
    with selectors.DefaultSelector() as selector:
        selector.register(read_fd, selectors.EVENT_READ)
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(
                    f"Xorg exited with {process.returncode}:\n"
                    f"{log_path.read_text(errors='replace')}"
                )
            if not selector.select(min(0.25, deadline - time.monotonic())):
                continue
            data = os.read(read_fd, 64)
            if not data:
                raise RuntimeError(f"Xorg closed its display pipe:\n{log_path.read_text(errors='replace')}")
            number, separator, _rest = data.partition(b"\n")
            if separator and number.isdigit():
                return int(number)
            if separator:
                raise RuntimeError(f"Xorg returned an invalid display number: {data!r}")
    raise RuntimeError(f"Xorg displayfd timed out:\n{log_path.read_text(errors='replace')}")


def run_tool(command, env, *, timeout=10):
    result = subprocess.run(
        command,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=timeout,
    )
    if result.returncode:
        raise RuntimeError(
            f"{command!r} failed with {result.returncode}: {result.stderr.strip()}"
        )
    return result.stdout.strip()


def window_geometry(xdotool, env, window):
    fields = {}
    output = run_tool([xdotool, "getwindowgeometry", "--shell", window], env)
    for line in output.splitlines():
        key, separator, value = line.partition("=")
        if separator and key in {"X", "Y", "WIDTH", "HEIGHT"}:
            fields[key.lower()] = int(value)
    if set(fields) != {"x", "y", "width", "height"}:
        raise RuntimeError(f"xdotool returned incomplete window geometry: {fields!r}")
    return fields


def drag_pointer(xdotool, env, start, end):
    run_tool([xdotool, "mousemove", "--sync", str(start[0]), str(start[1])], env)
    run_tool([xdotool, "mousedown", "1"], env)
    time.sleep(0.1)
    run_tool([xdotool, "mousemove", "--sync", str(end[0]), str(end[1])], env)
    run_tool([xdotool, "mouseup", "1"], env)


def wait_for_window_geometry(xdotool, env, window, process, log_path, matches, label):
    deadline = time.monotonic() + 10
    geometry = window_geometry(xdotool, env, window)
    while time.monotonic() < deadline:
        if matches(geometry):
            return geometry
        if process.poll() is not None:
            raise RuntimeError(
                f"Folio exited while waiting for {label}: "
                f"{log_path.read_text(errors='replace')[-1200:]}"
            )
        time.sleep(0.05)
        geometry = window_geometry(xdotool, env, window)
    raise RuntimeError(
        f"{label} did not change X window geometry; got {geometry!r}; "
        f"log={log_path.read_text(errors='replace')[-1200:]}"
    )


def window_for_process(xdotool, env, app, log):
    ids = re.findall(r"WindowId\((\d+)\)", log)
    for window in reversed(ids):
        try:
            if run_tool([xdotool, "getwindowpid", window], env) == str(app.pid):
                return window
        except (RuntimeError, subprocess.TimeoutExpired):
            continue
    result = subprocess.run(
        [xdotool, "search", "--onlyvisible", "--pid", str(app.pid)],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=5,
    )
    if result.returncode == 0 and result.stdout.split():
        return result.stdout.split()[0]
    raise RuntimeError(
        f"no X window belongs to Folio PID {app.pid}; "
        f"search={result.stderr.decode(errors='replace') if isinstance(result.stderr, bytes) else result.stderr}; "
        f"log={log[-1800:]}"
    )


def pty_bytes(path):
    try:
        return path.read_bytes()
    except FileNotFoundError:
        return b""


def wait_for_bytes(path, needle, process, log_path, label, *, quiet_limit, absolute_limit):
    started = last_byte = time.monotonic()
    observed = 0
    while needle not in (output := pty_bytes(path)):
        if process.poll() is not None:
            raise RuntimeError(
                f"Folio exited with {process.returncode} waiting for {label}; "
                f"PTY={output[-800:]!r}; log={log_path.read_text(errors='replace')[-1800:]}"
            )
        now = time.monotonic()
        if len(output) != observed:
            observed = len(output)
            last_byte = now
        quiet = now - last_byte
        elapsed = now - started
        if quiet >= quiet_limit or elapsed >= absolute_limit:
            raise RuntimeError(
                f"PTY silent waiting for {label}; quiet={quiet:.2f}s elapsed={elapsed:.2f}s "
                f"bytes={output[-800:]!r}; log={log_path.read_text(errors='replace')[-1800:]}"
            )
        time.sleep(min(0.025, quiet_limit - quiet, absolute_limit - elapsed))
    return output


def wait_for_pty_pattern(path, pattern, process, log_path, label):
    started = last_byte = time.monotonic()
    observed = 0
    match = None
    while match is None:
        output = pty_bytes(path)
        match = pattern.search(output)
        if match is not None:
            break
        if process.poll() is not None:
            raise RuntimeError(
                f"Folio exited with {process.returncode} waiting for {label}; "
                f"PTY={output[-800:]!r}; log={log_path.read_text(errors='replace')[-1800:]}"
            )
        now = time.monotonic()
        if len(output) != observed:
            observed = len(output)
            last_byte = now
        quiet = now - last_byte
        elapsed = now - started
        if quiet >= PTY_QUIET_LIMIT_SECONDS or elapsed >= PTY_ABSOLUTE_LIMIT_SECONDS:
            raise RuntimeError(
                f"PTY did not complete {label}; quiet={quiet:.2f}s elapsed={elapsed:.2f}s "
                f"bytes={output[-800:]!r}"
            )
        time.sleep(min(0.025, PTY_QUIET_LIMIT_SECONDS - quiet, PTY_ABSOLUTE_LIMIT_SECONDS - elapsed))
    if match is None:
        raise RuntimeError(f"PTY closed before {label}; bytes={output[-800:]!r}")
    return output, match


def wait_for_prompt_after(path, process, log_path, previous_count):
    prompt = b"FOLIO_INPUT_PROMPT> "
    started = last_byte = time.monotonic()
    observed = len(pty_bytes(path))
    while (output := pty_bytes(path)).count(prompt) <= previous_count:
        if process.poll() is not None:
            raise RuntimeError(
                f"Folio exited before the shell prompt returned: "
                f"{log_path.read_text(errors='replace')[-1800:]}"
            )
        now = time.monotonic()
        if len(output) != observed:
            observed = len(output)
            last_byte = now
        quiet = now - last_byte
        elapsed = now - started
        if quiet >= PTY_QUIET_LIMIT_SECONDS or elapsed >= PTY_ABSOLUTE_LIMIT_SECONDS:
            raise RuntimeError(
                f"shell prompt did not return; quiet={quiet:.2f}s elapsed={elapsed:.2f}s "
                f"bytes={output[-500:]!r}"
            )
        time.sleep(min(0.025, PTY_QUIET_LIMIT_SECONDS - quiet, PTY_ABSOLUTE_LIMIT_SECONDS - elapsed))
    return output


def dismiss_private_first_run_card(trace_path, process, log_path, xdotool, env):
    started = time.monotonic()
    while time.monotonic() - started < 15:
        trace = trace_path.read_text(errors="replace") if trace_path.exists() else ""
        lines = trace.splitlines()
        first_run = next(
            (index for index, line in enumerate(lines) if "new=Modal" in line and "cause=first_run" in line),
            None,
        )
        if first_run is not None:
            if any(
                "new=Shell" in line and "previous_cause=first_run" in line
                for line in lines[first_run + 1 :]
            ):
                return
            run_tool([xdotool, "key", "--clearmodifiers", "Escape"], env)
            break
        if process.poll() is not None:
            raise RuntimeError(
                f"Folio exited before the initial input owner settled: "
                f"{log_path.read_text(errors='replace')[-1800:]}"
            )
        time.sleep(0.025)
    else:
        raise RuntimeError(
            f"first-run modal did not appear in the private profile; IME trace={trace[-1200:]!r}; "
            f"log={log_path.read_text(errors='replace')[-1200:]}"
        )

    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        trace = trace_path.read_text(errors="replace") if trace_path.exists() else ""
        lines = trace.splitlines()
        first_run = next(
            (index for index, line in enumerate(lines) if "new=Modal" in line and "cause=first_run" in line),
            None,
        )
        if first_run is not None and any(
            "new=Shell" in line and "previous_cause=first_run" in line
            for line in lines[first_run + 1 :]
        ):
            print("PASS first-run Escape returned keyboard ownership to the shell")
            return
        if process.poll() is not None:
            break
        time.sleep(0.025)
    raise RuntimeError(
        f"Escape did not dismiss the first-run modal; IME trace={trace[-1200:]!r}; "
        f"log={log_path.read_text(errors='replace')[-1200:]}"
    )


def wait_for_window(app, xdotool, env, pty_path, log_path):
    started = last_activity = time.monotonic()
    observed = (0, 0)
    while time.monotonic() - started < APP_ABSOLUTE_LIMIT_SECONDS:
        log = log_path.read_text(errors="replace")
        output = pty_bytes(pty_path)
        if "BT_STARTUP first_text_present=" in log and b"FOLIO_INPUT_PTY_READY" in output:
            try:
                return window_for_process(xdotool, env, app, log), log
            except RuntimeError:
                pass
        if app.poll() is not None:
            raise RuntimeError(f"Folio exited with {app.returncode}:\n{log}")
        current = (log_path.stat().st_size, len(output))
        now = time.monotonic()
        if current != observed:
            observed = current
            last_activity = now
        quiet = now - last_activity
        if quiet >= APP_QUIET_LIMIT_SECONDS:
            raise RuntimeError(
                f"Folio made no startup progress for {quiet:.1f}s; "
                f"PTY={output[-800:]!r}; log={log[-1800:]}"
            )
        time.sleep(min(0.05, APP_QUIET_LIMIT_SECONDS - quiet))
    raise RuntimeError(
        f"Folio startup exceeded {APP_ABSOLUTE_LIMIT_SECONDS:.0f}s; "
        f"log={log_path.read_text(errors='replace')[-1800:]}"
    )


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
    current = process_identity(pid)
    raise RuntimeError(
        f"shell child {pid} remains in the process table as {current[0] if current else 'unknown'}"
    )


def terminate_test_child(pid, start_time):
    if start_time is None:
        return
    current = process_identity(pid)
    if current is None or current[1] != start_time:
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        current = process_identity(pid)
        if current is None or current[1] != start_time:
            return
        time.sleep(0.025)
    current = process_identity(pid)
    if current is not None and current[1] == start_time:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            return
    wait_reaped(pid, start_time)


def type_line(xdotool, env, text):
    run_tool([xdotool, "type", "--clearmodifiers", "--delay", "1", text], env)
    run_tool([xdotool, "key", "Return"], env)


def run_raw_probe(
    mode,
    chord,
    expected_hex,
    root,
    xdotool,
    env,
    app,
    log_path,
    pty_path,
    prompt_count,
):
    ready = root / f"{mode}.ready"
    result = root / f"{mode}.bytes"
    reader = root / "key-reader.py"
    reader.write_text(KEY_READER)
    command = " ".join(
        shlex.quote(part)
        for part in ("python3", str(reader), mode, str(ready), str(result))
    )
    type_line(xdotool, env, command)
    wait_file(ready, app, log_path, f"{mode} raw PTY handshake")
    run_tool([xdotool, "key", "--clearmodifiers", chord], env)
    wait_file(result, app, log_path, f"{mode} key bytes")
    actual = result.read_text().strip()
    if actual != expected_hex:
        raise RuntimeError(f"{mode} expected {expected_hex}, got {actual}")
    marker = {
        "kitty": f"FOLIO_KITTY_KEY={expected_hex}",
        "modify-other-keys": f"FOLIO_MOK2_KEY={expected_hex}",
        "alt": f"FOLIO_ALT_KEY={expected_hex}",
    }[mode].encode()
    output = wait_for_bytes(
        pty_path,
        marker,
        app,
        log_path,
        f"{mode} visible shell marker",
        quiet_limit=PTY_QUIET_LIMIT_SECONDS,
        absolute_limit=PTY_ABSOLUTE_LIMIT_SECONDS,
    )
    prompt = b"FOLIO_INPUT_PROMPT> "
    started = last_activity = time.monotonic()
    observed = len(output)
    while output.count(prompt) <= prompt_count:
        now = time.monotonic()
        if now - last_activity >= PTY_QUIET_LIMIT_SECONDS or now - started >= PTY_ABSOLUTE_LIMIT_SECONDS:
            raise RuntimeError(
                f"shell prompt did not return after {mode} probe; "
                f"quiet={now - last_activity:.2f}s bytes={output[-500:]!r}"
            )
        time.sleep(0.025)
        output = pty_bytes(pty_path)
        if len(output) != observed:
            observed = len(output)
            last_activity = time.monotonic()


def wait_file(path, process, log_path, label):
    started = last_change = time.monotonic()
    while not path.exists():
        if process.poll() is not None:
            raise RuntimeError(
                f"Folio exited with {process.returncode} waiting for {label}: "
                f"{log_path.read_text(errors='replace')[-1800:]}"
            )
        now = time.monotonic()
        quiet = now - last_change
        elapsed = now - started
        if quiet >= PTY_QUIET_LIMIT_SECONDS or elapsed >= PTY_ABSOLUTE_LIMIT_SECONDS:
            raise RuntimeError(
                f"child did not reach {label}; quiet={quiet:.2f}s elapsed={elapsed:.2f}s "
                f"log={log_path.read_text(errors='replace')[-1800:]}"
            )
        time.sleep(min(0.025, PTY_QUIET_LIMIT_SECONDS - quiet, PTY_ABSOLUTE_LIMIT_SECONDS - elapsed))


def smoke_app(executable, xdotool, env, root):
    pty_path = root / "pty.dump"
    log_path = root / "folio.log"
    pid_path = root / "shell.pid"
    shell = root / "probe-shell"
    bash_rc = root / "bashrc"
    bash_rc.write_text("PS1='FOLIO_INPUT_PROMPT> '\n")
    shell.write_text(
        "#!/bin/sh\n"
        "test -t 0 && test -t 1 || exit 9\n"
        "printf 'FOLIO_INPUT_PTY_READY '; stty size\n"
        "printf '%s\\n' \"$$\" > \"$FOLIO_CHILD_PID\"\n"
        "printf 'FOLIO_CJK_EMOJI: ASCII 中日韩 😀 👨‍👩‍👧‍👦 👍🏽\\n'\n"
        'exec /bin/bash --noprofile --rcfile "$FOLIO_BASH_RC"\n'
    )
    shell.chmod(0o700)
    env = env.copy()
    env.update(
        SHELL=str(shell),
        FOLIO_CHILD_PID=str(pid_path),
        FOLIO_BASH_RC=str(bash_rc),
        BT_PTY_DUMP=str(pty_path),
    )
    app_log_path = root / "folio.log"
    app, app_log_handle = start_logged(
        "folio",
        [str(executable), "--profile", "usershell", "--cwd", str(root)],
        root,
        env,
    )
    child_pid = None
    child_start_time = None
    try:
        window, log = wait_for_window(app, xdotool, env, pty_path, app_log_path)
        output = pty_bytes(pty_path)
        for sample in ("中日韩", "😀", "👨‍👩‍👧‍👦", "👍🏽"):
            if sample.encode("utf-8") not in output:
                raise RuntimeError(f"shell's CJK/emoji output is missing {sample!r}: {output!r}")
        initial = re.search(rb"FOLIO_INPUT_PTY_READY (\d+) (\d+)\r\n", output)
        if not initial or any(int(dimension) <= 0 for dimension in initial.groups()):
            raise RuntimeError(f"shell did not report nonzero PTY dimensions: {output!r}")
        if not pid_path.is_file():
            raise RuntimeError("shell did not publish its child pid")
        child_pid = int(pid_path.read_text().strip())
        child_identity = process_identity(child_pid)
        if child_identity is None:
            raise RuntimeError(f"shell process {child_pid} was gone before keyboard input")
        child_start_time = child_identity[1]
        prompt = b"FOLIO_INPUT_PROMPT> "
        wait_for_bytes(
            pty_path,
            prompt,
            app,
            app_log_path,
            "interactive shell prompt",
            quiet_limit=PTY_QUIET_LIMIT_SECONDS,
            absolute_limit=PTY_ABSOLUTE_LIMIT_SECONDS,
        )
        focus = run_tool([xdotool, "getwindowfocus"], env)
        if focus != window:
            try:
                run_tool([xdotool, "windowactivate", "--sync", window], env, timeout=5)
            except (RuntimeError, subprocess.TimeoutExpired):
                run_tool([xdotool, "windowfocus", "--sync", window], env, timeout=5)
            focus = run_tool([xdotool, "getwindowfocus"], env)
        if focus != window:
            run_tool([xdotool, "windowfocus", "--sync", window], env)
            focus = run_tool([xdotool, "getwindowfocus"], env)
        if focus != window:
            raise RuntimeError(f"Xorg focus is {focus}; Folio window is {window}")
        print(f"PASS Xorg window focused id={window}")
        dismiss_private_first_run_card(
            root / "ime.trace", app, app_log_path, xdotool, env
        )

        prompt_count = pty_bytes(pty_path).count(prompt)
        type_line(xdotool, env, "printf 'FOLIO_KEY_ROUTE_OK\\n'")
        wait_for_bytes(
            pty_path,
            b"FOLIO_KEY_ROUTE_OK\r\n",
            app,
            app_log_path,
            "XTest keyboard input reaching the shell",
            quiet_limit=PTY_QUIET_LIMIT_SECONDS,
            absolute_limit=PTY_ABSOLUTE_LIMIT_SECONDS,
        )
        wait_for_prompt_after(pty_path, app, app_log_path, prompt_count)
        print("PASS XTest keyboard text reached the shell through the PTY")

        run_raw_probe(
            "kitty",
            "shift+Return",
            "1b5b31333b3275",
            root,
            xdotool,
            env,
            app,
            app_log_path,
            pty_path,
            pty_bytes(pty_path).count(b"FOLIO_INPUT_PROMPT> "),
        )
        print("PASS kitty keyboard protocol Shift+Enter bytes")
        run_raw_probe(
            "modify-other-keys",
            "ctrl+Return",
            "1b5b32373b353b31337e",
            root,
            xdotool,
            env,
            app,
            app_log_path,
            pty_path,
            pty_bytes(pty_path).count(b"FOLIO_INPUT_PROMPT> "),
        )
        print("PASS xterm modifyOtherKeys protocol Ctrl+Enter bytes")
        run_raw_probe(
            "alt",
            "alt+q",
            "1b71",
            root,
            xdotool,
            env,
            app,
            app_log_path,
            pty_path,
            pty_bytes(pty_path).count(b"FOLIO_INPUT_PROMPT> "),
        )
        print("PASS Linux Alt modifier prefix")

        before_caption = window_geometry(xdotool, env, window)
        mouse_trace = root / "mouse.trace"
        title_trace_count = (
            mouse_trace.read_text(errors="replace").count("at=press-title-bar")
            if mouse_trace.exists()
            else 0
        )
        title_start = (
            before_caption["x"] + before_caption["width"] // 2,
            before_caption["y"] + 20,
        )
        drag_pointer(
            xdotool,
            env,
            title_start,
            (title_start[0] + 40, title_start[1] + 30),
        )
        moved = wait_for_window_geometry(
            xdotool,
            env,
            window,
            app,
            app_log_path,
            lambda geometry: geometry["x"] == before_caption["x"] + 40
            and geometry["y"] == before_caption["y"] + 30,
            "caption drag",
        )
        title_trace = mouse_trace.read_text(errors="replace") if mouse_trace.exists() else ""
        if title_trace.count("at=press-title-bar") <= title_trace_count:
            raise RuntimeError(
                f"caption press did not reach Folio's title-bar drag path: {title_trace[-1200:]!r}"
            )
        print(
            f"PASS XTest caption drag: ({before_caption['x']},{before_caption['y']}) "
            f"-> ({moved['x']},{moved['y']})"
        )

        old_rows, old_columns = (int(value) for value in initial.groups())
        run_tool([xdotool, "windowsize", window, "1000", "700"], env)
        direct_resize = wait_for_window_geometry(
            xdotool,
            env,
            window,
            app,
            app_log_path,
            lambda geometry: geometry["width"] == 1000 and geometry["height"] == 700,
            "direct test window resize",
        )
        print(f"PASS XTest harness positioned window at {direct_resize['width']}x{direct_resize['height']}")
        prompt_count = pty_bytes(pty_path).count(prompt)
        resize_probe = (
            f'for ((i=0; i<100; i++)); do set -- $(stty size); '
            f'if [ "$1" -ne {old_rows} ] || [ "$2" -ne {old_columns} ]; then break; fi; '
            "sleep 0.02; done; printf 'FOLIO_RESIZE '; stty size"
        )
        type_line(xdotool, env, resize_probe)
        resized, match = wait_for_pty_pattern(
            pty_path,
            re.compile(rb"FOLIO_RESIZE (\d+) (\d+)\r\n"),
            app,
            app_log_path,
            "PTY resize response",
        )
        wait_for_prompt_after(pty_path, app, app_log_path, prompt_count)
        rows, columns = (int(value) for value in match.groups())
        if rows <= 0 or columns <= 0 or (rows, columns) == (old_rows, old_columns):
            raise RuntimeError(
                f"PTY remained {old_rows}x{old_columns} after resize; got {rows}x{columns}"
            )
        print(f"PASS Xorg resize reached PTY: {old_rows}x{old_columns} -> {rows}x{columns}")

        before_edge = window_geometry(xdotool, env, window)
        edge_start = (
            before_edge["x"] + before_edge["width"] - 1,
            before_edge["y"] + before_edge["height"] // 2,
        )
        drag_pointer(
            xdotool,
            env,
            edge_start,
            (edge_start[0] + 80, edge_start[1]),
        )
        resized_window = wait_for_window_geometry(
            xdotool,
            env,
            window,
            app,
            app_log_path,
            lambda geometry: geometry["width"] == before_edge["width"] + 80,
            "east edge resize",
        )
        print(
            f"PASS XTest east-edge resize: {before_edge['width']} -> "
            f"{resized_window['width']} pixels"
        )

        run_tool([xdotool, "key", "--clearmodifiers", "ctrl+shift+q"], env)
        try:
            return_code = app.wait(timeout=15)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError("Ctrl+Shift+Q did not retire the Folio window") from error
        if return_code != 0:
            raise RuntimeError(
                f"Folio exited with {return_code}: "
                f"{app_log_path.read_text(errors='replace')[-1200:]}"
            )
        wait_reaped(child_pid, child_identity[1])
        print(f"PASS app quit reaped its PTY shell child pid={child_pid}")
    finally:
        stop_process(app, "Folio")
        if child_pid is not None:
            terminate_test_child(child_pid, child_start_time)
        app_log_handle.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=Path("target/debug/folio"))
    parser.add_argument("--xorg", default=os.environ.get("FOLIO_XORG"))
    parser.add_argument("--modulepath", default=os.environ.get("FOLIO_XORG_MODULEPATH"))
    parser.add_argument("--xorg-config", type=Path, default=Path(__file__).with_name("linux-input-xorg.conf"))
    parser.add_argument("--xdotool", default=os.environ.get("FOLIO_XDOTOOL"))
    parser.add_argument("--openbox", default=os.environ.get("FOLIO_OPENBOX"))
    parser.add_argument("--artifacts", type=Path, default=Path("target/linux-input-smoke"))
    args = parser.parse_args()

    executable = args.exe.resolve()
    if not executable.is_file() or not os.access(executable, os.X_OK):
        parser.error(f"Folio executable is missing or not executable: {executable}")
    for name, value in (("--xorg", args.xorg), ("--modulepath", args.modulepath), ("--xdotool", args.xdotool)):
        if not value:
            parser.error(f"{name} is required")
    xorg = Path(args.xorg).resolve()
    modulepath = Path(args.modulepath).resolve()
    xdotool = Path(args.xdotool).resolve()
    if not xorg.is_file() or not xdotool.is_file() or not modulepath.is_dir():
        parser.error("Xorg, xdotool, and an Xorg module path must exist")
    openbox = Path(args.openbox).resolve() if args.openbox else None
    if openbox is not None and (not openbox.is_file() or not os.access(openbox, os.X_OK)):
        parser.error(f"Openbox is missing or not executable: {openbox}")
    config = args.xorg_config.resolve()
    if not config.is_file():
        parser.error(f"Xorg config is missing: {config}")

    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="xorg-input-", dir=artifacts))
    for name in ("home", "config", "data", "cache"):
        (root / name).mkdir()
    with tempfile.TemporaryDirectory(prefix="folio-input-runtime-") as runtime_name:
        runtime = Path(runtime_name)
        runtime.chmod(0o700)
        env = clean_environment(root, runtime)
        read_fd, write_fd = os.pipe()
        server, server_log = start_logged(
            "xorg",
            [
                str(xorg),
                "-displayfd",
                str(write_fd),
                "-config",
                str(config),
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
        os.close(write_fd)
        try:
            display = read_display_number(server, read_fd, root / "xorg.log")
        finally:
            os.close(read_fd)
        env["DISPLAY"] = f":{display}"
        env["XDG_DATA_DIRS"] = os.environ.get("XDG_DATA_DIRS", "/usr/share")
        env["XDG_CONFIG_DIRS"] = os.environ.get("XDG_CONFIG_DIRS", "/etc/xdg")
        processes = [(server, server_log)]
        try:
            geometry = run_tool([str(xdotool), "getdisplaygeometry"], env)
            if not geometry:
                raise RuntimeError("Xorg did not report its private screen geometry")
            if openbox is not None:
                wm, wm_log = start_logged("openbox", [str(openbox), "--sm-disable"], root, env)
                processes.append((wm, wm_log))
            smoke_app(executable, str(xdotool), env, root)
            xorg_log = (root / "Xorg.log").read_text(errors="replace")
            for required in (
                'Option "AutoAddDevices" "false"',
                'Option "AutoEnableDevices" "false"',
                'Option "AutoAddGPU" "false"',
                "Using input driver 'void' for 'FolioVirtualKeyboard'",
                "FolioVirtualKeyboard: always reports core events",
            ):
                if required not in xorg_log:
                    raise RuntimeError(f"Xorg input isolation evidence is missing {required!r}")
            if "Using input driver 'libinput' for" in xorg_log:
                raise RuntimeError("Xorg attached a libinput device instead of the void keyboard")
            print(f"PASS private Xorg input isolation; display={env['DISPLAY']} geometry={geometry}")
            print(f"ARTIFACTS={root}")
        finally:
            for process, log in reversed(processes):
                stop_process(process, str(process.args[0]))
                log.close()


if __name__ == "__main__":
    main()
