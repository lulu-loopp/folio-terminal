#!/usr/bin/env python3
"""Exercise Linux window launch, shell ownership, and confirmed session restore in private Xorg."""

import argparse
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tempfile
import time


PTY_READY = b"FOLIO_WINDOW_PTY_READY"
STARTUP_READY = "BT_STARTUP first_text_present="


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
    ):
        env.pop(name, None)

    home = root / "home"
    config = root / "config"
    data = root / "data"
    cache = root / "cache"
    state = root / "state"
    for directory in (home, config, data, cache, state):
        directory.mkdir(parents=True, exist_ok=True)
    env.update(
        HOME=str(home),
        XDG_CONFIG_HOME=str(config),
        XDG_DATA_HOME=str(data),
        XDG_CACHE_HOME=str(cache),
        XDG_STATE_HOME=str(state),
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
        process.wait(timeout=10)
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
    if result.returncode:
        return []
    return result.stdout.decode().split()


def pty_bytes(path):
    paths = [path, *sorted(path.parent.glob(f"{path.name}.[0-9]*"))]
    return b"\n".join(
        path.read_bytes()
        for path in paths
        if path.is_file() and not path.name.endswith(".chunks")
    )


def process_snapshot(root, stage):
    result = subprocess.run(
        ["ps", "-eo", "pid=,ppid=,pgid=,sid=,tty=,stat=,etimes=,args="],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=5,
    )
    if result.returncode:
        raise RuntimeError(f"ps exited with {result.returncode}: {result.stderr.decode(errors='replace')}")
    selected = [
        line.strip()
        for line in result.stdout.decode(errors="replace").splitlines()
        if "folio-window-session-" in line or "probe-shell" in line or "bash --noprofile --norc" in line
    ]
    path = root / f"processes-{stage}.txt"
    path.write_text("\n".join(selected) + "\n")
    shell_count = len(live_shell_details(selected))
    print(f"PROCESS_SNAPSHOT {stage} live_probe_shells={shell_count} path={path}", flush=True)
    return selected


def shell_ledger(path):
    try:
        return path.read_text(errors="replace")
    except FileNotFoundError:
        return ""


def shell_births(path):
    return re.findall(r"^SHELL_BIRTH pid=(\d+)", shell_ledger(path), re.MULTILINE)


def shell_wrappers(process_rows, *, live_only):
    pids = set()
    for row in process_rows:
        if "probe-shell" not in row:
            continue
        fields = row.split(maxsplit=7)
        if len(fields) >= 7 and (not live_only or not fields[5].startswith("Z")):
            pids.add(fields[0])
    return pids


def live_shell_details(process_rows):
    details = {}
    for row in process_rows:
        fields = row.split(maxsplit=7)
        if len(fields) < 8 or "probe-shell" not in fields[7] or fields[5].startswith("Z"):
            continue
        details[fields[0]] = {
            "ppid": fields[1],
            "pgid": fields[2],
            "sid": fields[3],
            "tty": fields[4],
            "state": fields[5],
        }
    return details


def require_shell_ownership(rows, app_pid, expected, description):
    live = live_shell_details(rows)
    owned = {pid: facts for pid, facts in live.items() if facts["ppid"] == str(app_pid)}
    if len(owned) != expected:
        raise RuntimeError(
            f"{description}: expected {expected} live probe shells owned by app {app_pid}; got {live}"
        )
    if len({facts["tty"] for facts in owned.values()}) != len(owned):
        raise RuntimeError(f"{description}: live shells do not own distinct PTYs: {owned}")
    return owned


def wait_for(predicate, description, *, seconds=45):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {description}")


def read_log(path):
    try:
        return path.read_text(errors="replace")
    except FileNotFoundError:
        return ""


def start_folio(executable, args, name, root, env, pty_path, chrome_path):
    app_env = env.copy()
    app_env["BT_PTY_DUMP"] = str(pty_path)
    app_env["BT_CHROME_DUMP"] = str(chrome_path)
    return start_logged(name, [str(executable), *map(str, args)], root, app_env)


def wait_for_live_window(process, log_path, xdotool, env, *, pty_path, seconds=45):
    def ready():
        pty = pty_path.read_bytes() if pty_path.exists() else b""
        return (
            bool(visible_windows(xdotool, env, process.pid))
            and STARTUP_READY in read_log(log_path)
            and PTY_READY in pty
        )

    wait_for(ready, f"visible Folio window and presented PTY for pid {process.pid}", seconds=seconds)


def focus_terminal(xdotool, env, window):
    run_tool(xdotool, env, "windowactivate", "--sync", window)
    time.sleep(0.3)
    run_tool(xdotool, env, "key", "--clearmodifiers", "Escape")
    run_tool(xdotool, env, "mousemove", "--window", window, 200, 200)
    run_tool(xdotool, env, "click", 1)
    time.sleep(0.2)


def type_line(xdotool, env, line):
    run_tool(xdotool, env, "type", "--clearmodifiers", line)
    run_tool(xdotool, env, "key", "Return")


def quit_cleanly(process, name, xdotool, env):
    windows = visible_windows(xdotool, env, process.pid)
    if windows:
        focus_terminal(xdotool, env, windows[-1])
        run_tool(xdotool, env, "key", "--clearmodifiers", "ctrl+shift+q")
    try:
        code = process.wait(timeout=20)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"{name} did not quit through Ctrl+Shift+Q") from error
    if code != 0:
        raise RuntimeError(f"{name} exited with {code}")
    print(f"PASS {name} clean quit", flush=True)


def clear_folio_claims(root):
    data = root / "data" / "Folio"
    for name in ("session.json", "settings.json", "session.lock", "settings.lock"):
        (data / name).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--xorg", type=Path, required=True)
    parser.add_argument("--modulepath", type=Path, required=True)
    parser.add_argument("--xdotool", type=Path, required=True)
    parser.add_argument("--openbox", type=Path, required=True)
    parser.add_argument("--xorg-config", type=Path, default=Path(__file__).with_name("linux-input-xorg.conf"))
    parser.add_argument("--openbox-config", type=Path)
    parser.add_argument("--library-dir", type=Path)
    parser.add_argument("--xdg-data-dirs", default=os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share"))
    parser.add_argument("--xdg-config-dirs", default=os.environ.get("XDG_CONFIG_DIRS", "/etc/xdg"))
    parser.add_argument("--artifacts", type=Path, default=Path("target/linux-window-session-smoke"))
    args = parser.parse_args()

    executable = args.exe.resolve()
    xorg = args.xorg.resolve()
    modulepath = args.modulepath.resolve()
    xdotool = args.xdotool.resolve()
    openbox = args.openbox.resolve()
    config = args.xorg_config.resolve()
    for path, description in (
        (executable, "Folio executable"),
        (xorg, "Xorg executable"),
        (xdotool, "xdotool executable"),
        (openbox, "Openbox executable"),
        (config, "Xorg input configuration"),
    ):
        if not path.is_file() or (description.endswith("executable") and not os.access(path, os.X_OK)):
            parser.error(f"{description} is missing or not executable: {path}")
    if not modulepath.is_dir():
        parser.error(f"Xorg module path is missing: {modulepath}")
    if args.library_dir is not None and not args.library_dir.resolve().is_dir():
        parser.error(f"library directory is missing: {args.library_dir}")
    openbox_config = args.openbox_config.resolve() if args.openbox_config else None
    if openbox_config is not None and not openbox_config.is_file():
        parser.error(f"Openbox config is missing: {openbox_config}")

    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="linux-window-session-", dir=artifacts))
    print(f"ARTIFACTS={root}", flush=True)
    for name in ("home", "config", "data", "cache", "state"):
        (root / name).mkdir(parents=True, exist_ok=True)
    fixture = root / "fixture.md"
    fixture.write_text("# Linux window session\n\nThis is the independent document launch.\n")
    shell = root / "probe-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "printf 'SHELL_BIRTH pid=%s ppid=%s cwd=%s tty=%s\\n' \"$$\" \"$PPID\" \"$PWD\" \"$(tty)\" >> \"$FOLIO_SHELL_LEDGER\"\n"
        "printf 'FOLIO_WINDOW_PTY_READY '; stty size\n"
        "/bin/bash --noprofile --norc\n"
        "status=$?\n"
        "printf 'SHELL_EXIT pid=%s status=%s\\n' \"$$\" \"$status\" >> \"$FOLIO_SHELL_LEDGER\"\n"
        "exit \"$status\"\n"
    )
    shell.chmod(0o700)

    with tempfile.TemporaryDirectory(prefix="folio-window-runtime-") as runtime_name:
        runtime = Path(runtime_name)
        runtime.chmod(0o700)
        env = clean_environment(root, runtime, args.library_dir.resolve() if args.library_dir else None)
        env.update(
            SHELL=str(shell),
        )
        env["FOLIO_SHELL_LEDGER"] = str(root / "primary-shells.log")
        env["XDG_DATA_DIRS"] = args.xdg_data_dirs
        env["XDG_CONFIG_DIRS"] = args.xdg_config_dirs
        server = None
        server_log = None
        wm = None
        wm_log = None
        primary = None
        primary_log = None
        document = None
        document_log = None
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
            finally:
                os.close(write_fd)
            display_number = read_display_number(server, read_fd, root / "private-xorg.log")
            os.close(read_fd)
            env["DISPLAY"] = f":{display_number}"
            print(f"READY private Xorg display={env['DISPLAY']}", flush=True)

            openbox_args = [str(openbox)]
            if openbox_config is not None:
                openbox_args.extend(["--config-file", str(openbox_config)])
            wm, wm_log = start_logged("openbox", openbox_args, root, env)
            time.sleep(0.3)
            if wm.poll() is not None:
                raise RuntimeError(f"Openbox exited: {read_log(root / 'openbox.log')}")

            clear_folio_claims(root)
            for path in root.glob("pty.dump*"):
                path.unlink(missing_ok=True)
            primary, primary_log = start_folio(
                executable,
                ["--profile", "usershell", "--cwd", str(root)],
                "primary",
                root,
                env,
                root / "pty.dump",
                root / "chrome.dump",
            )
            wait_for_live_window(primary, root / "primary.log", xdotool, env, pty_path=root / "pty.dump")
            wait_for(lambda: len(visible_windows(xdotool, env, primary.pid)) == 1, "first Folio window")
            primary_only_processes = process_snapshot(root, "primary-only")
            primary_only_shells = require_shell_ownership(
                primary_only_processes, primary.pid, 1, "primary-only state"
            )
            print(f"SHELL_OWNERSHIP primary-only {primary_only_shells}", flush=True)
            print("SHELL_LEDGER primary-only\n" + shell_ledger(root / "primary-shells.log"), flush=True)
            print("PASS first process owns one native Xorg window", flush=True)

            # A folder can cross the launch wire when the existing process can
            # serve it. A refusal falls back to an independent GUI process, so
            # watch the process/window outcome instead of waiting for a GUI
            # process to exit on a fixed deadline.
            directory_env = env.copy()
            directory_env["BT_PTY_DUMP"] = str(root / "directory-client.pty.dump")
            directory_env["BT_CHROME_DUMP"] = str(root / "directory-client.chrome.dump")
            directory_env["FOLIO_SHELL_LEDGER"] = str(root / "directory-client-shells.log")
            directory, directory_log = start_logged(
                "directory-client",
                [str(executable), "--profile", "usershell", "--new-window", "--cwd", str(root)],
                root,
                directory_env,
            )
            deadline = time.monotonic() + 30
            directory_outcome = None
            while time.monotonic() < deadline:
                if directory.poll() is not None:
                    if directory.returncode != 0:
                        raise RuntimeError(f"directory client exited with {directory.returncode}")
                    directory_outcome = "handed-over"
                    break
                if visible_windows(xdotool, directory_env, directory.pid):
                    directory_outcome = "independent-fallback"
                    break
                time.sleep(0.1)
            if directory_outcome is None:
                raise RuntimeError(
                    "directory CLI produced neither a handover reply nor a fallback window; "
                    f"client log:\n{read_log(root / 'directory-client.log')}"
                )
            if directory_outcome == "independent-fallback":
                fallback_window = visible_windows(xdotool, directory_env, directory.pid)[0]
                run_tool(xdotool, directory_env, "windowactivate", "--sync", fallback_window)
                run_tool(xdotool, directory_env, "key", "--clearmodifiers", "Escape")
                geometry = dict(
                    line.split("=", 1)
                    for line in run_tool(xdotool, directory_env, "getwindowgeometry", "--shell", fallback_window).splitlines()
                )
                run_tool(xdotool, directory_env, "mousemove", "--window", fallback_window, int(geometry["WIDTH"]) - 23, 20)
                run_tool(xdotool, directory_env, "click", 1)
                if directory.wait(timeout=20) != 0:
                    raise RuntimeError("directory fallback window did not close cleanly")
                primary_log_text = read_log(root / "primary.log")
                directory_log_text = read_log(root / "directory-client.log")
                if primary.poll() is None:
                    quit_cleanly(primary, "primary after failed directory forwarding", xdotool, env)
                    primary = None
                raise RuntimeError(
                    "directory --new-window --cwd opened an independent process instead of "
                    "being forwarded to the ready same-profile process; "
                    f"primary log:\n{primary_log_text}\nclient log:\n{directory_log_text}"
                )
            wait_for(
                lambda: len(visible_windows(xdotool, env, primary.pid)) == 2,
                "directory handover creates exactly two windows in the primary process",
            )
            if visible_windows(xdotool, directory_env, directory.pid):
                raise RuntimeError("the successful directory client unexpectedly retained a native window")
            print("PASS same-profile directory launch was handed to the existing process", flush=True)
            directory_processes = process_snapshot(root, "directory-handover")
            primary_live_at_handover_details = require_shell_ownership(
                directory_processes, primary.pid, 2, "directory handover"
            )
            print(f"SHELL_OWNERSHIP directory-handover {primary_live_at_handover_details}", flush=True)
            print("SHELL_LEDGER directory-handover\n" + shell_ledger(root / "primary-shells.log"), flush=True)

            # A document path has no field on the launch wire. Its cold GUI
            # process owns the document window and therefore stays resident.
            secondary_env = env.copy()
            secondary_env["BT_PTY_DUMP"] = str(root / "document-secondary.pty.dump")
            secondary_env["BT_CHROME_DUMP"] = str(root / "document-secondary.chrome.dump")
            secondary_env["FOLIO_SHELL_LEDGER"] = str(root / "document-secondary-shells.log")
            document, document_log = start_logged(
                "document-secondary",
                [str(executable), "--new-window", str(fixture)],
                root,
                secondary_env,
            )
            wait_for_live_window(
                document,
                root / "document-secondary.log",
                xdotool,
                secondary_env,
                pty_path=root / "document-secondary.pty.dump",
            )
            document_live_processes = process_snapshot(root, "document-secondary-live")
            primary_live_before_close_details = require_shell_ownership(
                document_live_processes, primary.pid, 2, "document launch while primary remains live"
            )
            document_shell_details = require_shell_ownership(
                document_live_processes, document.pid, 1, "independent document launch"
            )
            if set(live_shell_details(document_live_processes)) != (
                set(primary_live_before_close_details) | set(document_shell_details)
            ):
                raise RuntimeError("a live probe shell has no primary or document app owner")
            print(
                f"SHELL_OWNERSHIP primary {primary_live_before_close_details} document {document_shell_details}",
                flush=True,
            )
            print("SHELL_LEDGER primary-live\n" + shell_ledger(root / "primary-shells.log"), flush=True)
            print("SHELL_LEDGER document-secondary-live\n" + shell_ledger(root / "document-secondary-shells.log"), flush=True)
            if document.poll() is not None:
                raise RuntimeError("document launch returned instead of owning its GUI window")
            if len(visible_windows(xdotool, env, primary.pid)) != 2:
                raise RuntimeError("document launch unexpectedly changed the primary process windows")
            print("PASS document --new-window owns an independent resident process/window", flush=True)
            doc_window = visible_windows(xdotool, secondary_env, document.pid)[0]
            geometry = dict(
                line.split("=", 1)
                for line in run_tool(xdotool, secondary_env, "getwindowgeometry", "--shell", doc_window).splitlines()
            )
            run_tool(xdotool, secondary_env, "windowactivate", "--sync", doc_window)
            run_tool(xdotool, secondary_env, "mousemove", "--window", doc_window, int(geometry["WIDTH"]) - 23, 20)
            run_tool(xdotool, secondary_env, "click", 1)
            try:
                document_code = document.wait(timeout=20)
            except subprocess.TimeoutExpired as error:
                raise RuntimeError("the document window did not close cleanly") from error
            if document_code != 0:
                raise RuntimeError(f"document process exited with {document_code}")
            document_closed_processes = process_snapshot(root, "document-secondary-closed")
            primary_live_after_close_details = require_shell_ownership(
                document_closed_processes, primary.pid, 2, "document close leaves primary shells owned"
            )
            print("SHELL_LEDGER primary-after-document-close\n" + shell_ledger(root / "primary-shells.log"), flush=True)
            print("SHELL_LEDGER document-secondary-after-close\n" + shell_ledger(root / "document-secondary-shells.log"), flush=True)
            print("PASS document window closes cleanly; the 10s CLI wait is not a valid close condition", flush=True)

            if len(visible_windows(xdotool, env, primary.pid)) != 2:
                raise RuntimeError("the independent document process changed the primary window count")
            primary_births = shell_births(root / "primary-shells.log")
            document_births = shell_births(root / "document-secondary-shells.log")
            primary_live_at_handover = set(primary_live_at_handover_details)
            primary_live_before_close = shell_wrappers(document_live_processes, live_only=True) - set(
                document_births
            )
            primary_live_after_close = shell_wrappers(document_closed_processes, live_only=True)
            document_live_before_close = set(document_births) & shell_wrappers(
                document_live_processes, live_only=True
            )
            document_live_after_close = set(document_births) & shell_wrappers(
                document_closed_processes, live_only=True
            )
            if (
                len(primary_live_after_close) != 2
                or primary_live_after_close != primary_live_before_close
                or primary_live_after_close != set(primary_live_after_close_details)
            ):
                raise RuntimeError(
                    f"primary shell ownership changed across secondary close: "
                    f"before={primary_live_before_close}, after={primary_live_after_close}"
                )
            if len(document_births) != 1 or document_live_before_close != set(document_births):
                raise RuntimeError(
                    f"document launch did not own one live shell: "
                    f"births={document_births}, live={document_live_before_close}"
                )
            if document_live_after_close:
                raise RuntimeError(f"document shell stayed alive after close: {document_live_after_close}")
            recordings = [
                root / "pty.dump",
                *sorted(path for path in root.glob("pty.dump.[0-9]*") if not path.name.endswith(".chunks")),
            ]
            if len(recordings) != len(primary_births):
                raise RuntimeError(
                    f"PTY recording/birth ledger disagree: files={len(recordings)}, births={len(primary_births)}"
                )
            for index, (recording, pid) in enumerate(zip(recordings, primary_births)):
                print(
                    f"PRIMARY_SHELL recording={index} file={recording.name} pid={pid} "
                    f"live_at_handover={pid in primary_live_at_handover} "
                    f"live_after_document_close={pid in primary_live_after_close}",
                    flush=True,
                )
            primary_retired = set(primary_births) - primary_live_after_close
            handover_states = {
                row.split(maxsplit=7)[0]: row.split(maxsplit=7)[5]
                for row in directory_processes
                if "probe-shell" in row
            }
            closed_states = {
                row.split(maxsplit=7)[0]: row.split(maxsplit=7)[5]
                for row in document_closed_processes
                if "probe-shell" in row
            }
            retired_states = {
                pid: handover_states.get(pid, "not-in-first-snapshot")
                for pid in sorted(primary_retired)
            }
            retired_still_present = set(primary_retired) & set(closed_states)
            if retired_still_present:
                raise RuntimeError(f"transient primary shell processes were not reaped: {retired_still_present}")
            document_processes_after_close = {
                row.split(maxsplit=7)[0]
                for row in document_closed_processes
                if "probe-shell" in row
            }
            if set(document_births) & document_processes_after_close:
                raise RuntimeError(
                    f"document shell process remained after the document app exited: "
                    f"{set(document_births) & document_processes_after_close}"
                )
            print(
                f"PASS primary two-window state survived document launch; "
                f"primary live shells={len(primary_live_after_close)}, "
                f"secondary shell pid={document_births[0]} reaped={not document_live_after_close}, "
                f"transient primary shell states at handover={retired_states} reaped=True",
                flush=True,
            )

            quit_cleanly(primary, "primary", xdotool, env)
            session_path = root / "data" / "Folio" / "session.json"
            if not session_path.is_file():
                raise RuntimeError("clean quit did not write session.json")
            saved = json.loads(session_path.read_text())
            if len(saved.get("windows", [])) != 2:
                raise RuntimeError(f"session.json saved {len(saved.get('windows', []))} windows instead of 2")
            print("PASS clean quit saved two windows to session.json", flush=True)

            restored_env = env.copy()
            restored_env["FOLIO_SHELL_LEDGER"] = str(root / "restored-shells.log")
            restored_pty = root / "restored.pty.dump"
            restored_chrome = root / "restored.chrome.dump"
            restored, restored_log = start_folio(
                executable,
                [],
                "restored",
                root,
                restored_env,
                restored_pty,
                restored_chrome,
            )
            wait_for_live_window(restored, root / "restored.log", xdotool, env, pty_path=restored_pty)
            restore_labels = read_log(restored_chrome)
            if "Reopen your other tabs?" not in restore_labels or '"Restore"' not in restore_labels:
                raise RuntimeError("the restore question and Restore action were not present in the UI trace")
            print("PASS restore card and focused Restore action are present in the UI trace", flush=True)
            restore_window = visible_windows(xdotool, env, restored.pid)[0]
            run_tool(xdotool, env, "windowactivate", "--sync", restore_window)
            run_tool(xdotool, env, "key", "--clearmodifiers", "Return")
            wait_for(
                lambda: len(visible_windows(xdotool, env, restored.pid)) >= 2
                and pty_bytes(restored_pty).count(PTY_READY) >= 2,
                "Enter accepts restore and recreates two native windows and shells",
            )
            print("PASS Enter on Restore recreated two native windows and shell panes", flush=True)
            quit_cleanly(restored, "restored process", xdotool, env)

            xorg_text = read_log(root / "Xorg.log")
            if "AutoAddDevices is off" not in xorg_text:
                raise RuntimeError("Xorg did not prove the configured virtual input isolation")
            print("PASS private Xorg input isolation", flush=True)
        finally:
            stop_process(document, "document-secondary")
            stop_process(primary, "primary")
            if 'restored' in locals():
                stop_process(restored, "restored")
            stop_process(wm, "openbox")
            stop_process(server, "private-xorg")
            for log in (primary_log, document_log, restored_log if 'restored_log' in locals() else None, wm_log, server_log):
                if log is not None:
                    log.close()
    print("PASS Linux window/session smoke completed", flush=True)


if __name__ == "__main__":
    main()
