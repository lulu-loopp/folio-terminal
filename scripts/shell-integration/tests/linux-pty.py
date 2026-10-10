#!/usr/bin/env python3
"""Exercise Folio's Bash and Zsh startup hooks on Linux pseudoterminals."""

import os
import pty
import re
import select
import shlex
import shutil
import tempfile
import time
from pathlib import Path
from urllib.parse import quote


QUIET_LIMIT_SECONDS = 5.0
ABSOLUTE_LIMIT_SECONDS = 30.0
OSC_A = b"\x1b]133;A\x07"
OSC_B = b"\x1b]133;B\x07"
OSC_C = b"\x1b]133;C\x07"
OSC_D_FAILED = b"\x1b]133;D;1\x07"
OSC_MARKER = re.compile(rb"\x1b\](7;[^\x07]*|133;[^\x07]*)\x07")
INITIAL_PROMPT_TRACE = (b"7", OSC_A, OSC_B)
COMMAND_PROMPT_TRACE = (OSC_C, OSC_D_FAILED, b"7", OSC_A, OSC_B)
ONE_COMMAND_TRACE = INITIAL_PROMPT_TRACE + COMMAND_PROMPT_TRACE


def read_until(fd, marker):
    data = bytearray()
    started = last_byte = time.monotonic()
    while marker not in data:
        now = time.monotonic()
        quiet = now - last_byte
        elapsed = now - started
        if quiet >= QUIET_LIMIT_SECONDS or elapsed >= ABSOLUTE_LIMIT_SECONDS:
            raise RuntimeError(
                f"PTY silent waiting for {marker!r}; quiet={quiet:.2f}s "
                f"elapsed={elapsed:.2f}s bytes={bytes(data)!r}"
            )
        ready, _, _ = select.select(
            [fd],
            [],
            [],
            min(QUIET_LIMIT_SECONDS - quiet, ABSOLUTE_LIMIT_SECONDS - elapsed),
        )
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError as error:
            raise RuntimeError(f"PTY read failed; bytes={bytes(data)!r}") from error
        if not chunk:
            raise RuntimeError(f"PTY closed waiting for {marker!r}; bytes={bytes(data)!r}")
        data.extend(chunk)
        last_byte = time.monotonic()
    return bytes(data)


def spawn(program, arguments, environment, cwd):
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.environ.clear()
        os.environ.update(environment)
        os.execve(program, [program, *arguments], environment)
    return pid, fd


def stop_at_prompt(pid, fd, command, visible_prompt):
    prompt_end = visible_prompt + OSC_B
    first = read_until(fd, prompt_end)
    os.write(fd, command.encode("utf-8") + b"\n")
    action = read_until(fd, prompt_end)
    tail, status = send_eof_and_wait(pid, fd)
    return first, action, tail, status


def send_eof_and_wait(pid, fd):
    os.write(fd, b"\x04")
    started = last_byte = time.monotonic()
    tail = bytearray()
    while True:
        waited, status = os.waitpid(pid, os.WNOHANG)
        if waited:
            return bytes(tail), status
        now = time.monotonic()
        quiet = now - last_byte
        elapsed = now - started
        if quiet >= QUIET_LIMIT_SECONDS or elapsed >= ABSOLUTE_LIMIT_SECONDS:
            raise RuntimeError(
                f"shell did not exit after Ctrl-D; quiet={quiet:.2f}s "
                f"elapsed={elapsed:.2f}s output={bytes(tail)!r}"
            )
        ready, _, _ = select.select(
            [fd],
            [],
            [],
            min(QUIET_LIMIT_SECONDS - quiet, ABSOLUTE_LIMIT_SECONDS - elapsed),
        )
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            continue
        if chunk:
            tail.extend(chunk)
            last_byte = time.monotonic()


def reap(pid):
    try:
        waited, _ = os.waitpid(pid, os.WNOHANG)
    except ChildProcessError:
        return
    if not waited:
        os.kill(pid, 9)
        os.waitpid(pid, 0)


def require(condition, message, output=b""):
    if not condition:
        raise AssertionError(f"{message}; output={output!r}")


def marker_trace(output):
    return tuple(
        b"7" if marker.startswith(b"7;") else b"\x1b]" + marker + b"\x07"
        for marker in OSC_MARKER.findall(output)
    )


def verify_prompt_trace(first, action, prompt_name):
    require(
        marker_trace(first) == INITIAL_PROMPT_TRACE,
        f"{prompt_name}: ordered initial prompt markers",
        first,
    )
    require(
        marker_trace(action) == COMMAND_PROMPT_TRACE,
        f"{prompt_name}: ordered command and returned-prompt markers",
        action,
    )
    require(
        marker_trace(first + action) == ONE_COMMAND_TRACE,
        f"{prompt_name}: ordered one-command marker trace",
        first + action,
    )


def path_uri(path):
    encoded = quote(str(path), safe="/:@!$&'()*+,;=-._~")
    return ("file://" + encoded).encode("ascii")


def shell_command(cwd):
    return (
        "printf 'USER_COMMAND_RAN\\n'; cd -- "
        + shlex.quote(str(cwd))
        + "; printf 'COMMAND_CWD:%s\\n' \"$PWD\"; false"
    )


def verify_command_turn(action, cwd, hooks, prompt_name):
    expected_cwd = b"\x1b]7;" + path_uri(cwd) + b"\x07"
    require(action.count(OSC_C) == 1, f"{prompt_name}: one command-start marker", action)
    require(action.count(OSC_D_FAILED) == 1, f"{prompt_name}: exit status 1", action)
    require(action.count(OSC_A) == 1, f"{prompt_name}: one returned prompt-start", action)
    require(action.count(OSC_B) == 1, f"{prompt_name}: one returned input-start", action)
    require(expected_cwd in action, f"{prompt_name}: OSC 7 reports changed cwd", action)
    require(b"USER_COMMAND_RAN" in action, f"{prompt_name}: typed command ran", action)
    require(b"COMMAND_CWD:" + os.fsencode(cwd) in action, f"{prompt_name}: shell cwd changed", action)
    for hook in hooks:
        require(action.count(hook) == 1, f"{prompt_name}: user's prompt hook {hook!r} ran once", action)


def plain_shell_command(name, cwd):
    marker = f"PLAIN_{name.upper()}"
    directory = f"'{cwd}'" if name == "fish" else shlex.quote(str(cwd))
    status = "$status" if name == "fish" else "$?"
    return (
        f"test -t 0 && test -t 1; printf '{marker}_TTY:%s\\n' \"{status}\"; "
        f"cd -- {directory}; printf '{marker}_COMMAND_RAN\\n'; "
        f"printf '{marker}_CWD:%s\\n' \"$PWD\"; false; "
        f"printf '{marker}_STATUS:%s\\n' \"{status}\""
    )


def verify_plain_shell(name, program, cwd, environment):
    prompt = f"FOLIO_{name.upper()}_PTY> "
    env = dict(environment, PS1=prompt)
    if name == "fish":
        env["TERM"] = "dumb"
        fish_config = Path(environment["XDG_CONFIG_HOME"]) / "fish" / "config.fish"
        fish_config.parent.mkdir(parents=True, exist_ok=True)
        fish_config.write_text(
            "function fish_prompt\n"
            f"    printf '{prompt}'\n"
            "end\n"
        )

    pid, fd = spawn(program, [], env, Path(env["HOME"]))
    try:
        first = read_until(fd, prompt.encode())
        os.write(fd, plain_shell_command(name, cwd).encode("utf-8") + b"\n")
        status_marker = f"PLAIN_{name.upper()}_STATUS:1".encode()
        output = read_until(fd, status_marker)
        require(
            f"PLAIN_{name.upper()}_TTY:0".encode() in output,
            f"{name}: stdin and stdout are terminals",
            output,
        )
        require(
            f"PLAIN_{name.upper()}_COMMAND_RAN".encode() in output,
            f"{name}: command reached the shell",
            output,
        )
        require(
            f"PLAIN_{name.upper()}_CWD:".encode() + os.fsencode(cwd) in output,
            f"{name}: command changed cwd",
            output,
        )
        require(
            b"\x1b]133;" not in first + output and b"\x1b]7;" not in first + output,
            f"{name}: no unsupported Folio shell hooks were injected",
            first + output,
        )
        send_eof_and_wait(pid, fd)
    finally:
        os.close(fd)
        reap(pid)


def main():
    if not sys_platform_linux():
        raise SystemExit("linux-pty.py only runs on Linux")

    tests_dir = Path(__file__).resolve().parent
    scripts_dir = tests_dir.parent
    bash_program = shutil.which("bash")
    zsh_program = shutil.which("zsh")
    if bash_program is None and zsh_program is None:
        raise SystemExit("no installed Bash or Zsh to probe")

    with tempfile.TemporaryDirectory(prefix="folio-linux-shell-pty-") as temporary:
        root = Path(temporary)
        home = root / "isolated home"
        xdg_config = root / "xdg config"
        xdg_data = root / "xdg data"
        cwd = root / "cwd with spaces 中"
        for directory in (home, xdg_config, xdg_data, cwd):
            directory.mkdir(parents=True, exist_ok=True)
        integration = xdg_data / "Folio" / "shell-integration"
        integration.mkdir(parents=True)
        command = shell_command(cwd)
        environment = {
            "HOME": str(home),
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "TERM": "xterm-256color",
            "LC_ALL": "C.UTF-8",
            "HISTFILE": "/dev/null",
            "XDG_CONFIG_HOME": str(xdg_config),
            "XDG_DATA_HOME": str(xdg_data),
        }

        bash_files = []
        if bash_program:
            bash_file = integration / "folio.bash"
            shutil.copyfile(scripts_dir / "folio.bash", bash_file)
            prompt = b"BASH_PTY> "
            bash_rc = home / ".bashrc"
            bash_profile = home / ".bash_profile"
            bash_login = home / ".bash_login"
            profile = home / ".profile"
            bash_login.write_text("printf 'WRONG_BASH_LOGIN\\n'\n")
            profile.write_text("printf 'WRONG_PROFILE\\n'\n")
            bash_files = [bash_rc, bash_profile, bash_login, profile]
            hook_cases = (
                (
                    "scalar PROMPT_COMMAND",
                    "mark_user_prompt() { printf 'USER_BASH_HOOK_ONE\\n'; }\n"
                    "PROMPT_COMMAND=mark_user_prompt\n",
                    (b"USER_BASH_HOOK_ONE",),
                ),
                (
                    "array PROMPT_COMMAND",
                    "mark_user_prompt() { printf 'USER_BASH_HOOK_ONE\\n'; }\n"
                    "mark_user_prompt_tail() { printf 'USER_BASH_HOOK_TWO\\n'; }\n"
                    "PROMPT_COMMAND=(mark_user_prompt mark_user_prompt_tail)\n",
                    (b"USER_BASH_HOOK_ONE", b"USER_BASH_HOOK_TWO"),
                ),
            )
            startup_modes = (
                ("interactive", b"USER_BASHRC", (b"USER_BASH_PROFILE",)),
                ("login", b"USER_BASH_PROFILE", (b"USER_BASHRC", b"WRONG_BASH_LOGIN", b"WRONG_PROFILE")),
            )
            for hook_form, hook_definition, hooks in hook_cases:
                bash_rc.write_text("printf 'USER_BASHRC\\n'\nPS1='BASH_PTY> '\n" + hook_definition)
                bash_profile.write_text("printf 'USER_BASH_PROFILE\\n'\nPS1='BASH_PTY> '\n" + hook_definition)
                before = {path: path.read_bytes() for path in bash_files}
                for mode, expected_marker, forbidden in startup_modes:
                    prompt_name = f"Bash {mode}, {hook_form}"
                    env = dict(environment, BT_SHELL_INTEGRATION=mode)
                    pid, fd = spawn(bash_program, ["--init-file", str(bash_file), "-i"], env, home)
                    try:
                        first, action, _, _ = stop_at_prompt(pid, fd, command, prompt)
                        require(expected_marker in first, f"{prompt_name}: expected startup file", first)
                        for hook in hooks:
                            require(first.count(hook) == 1, f"{prompt_name}: initial prompt hook {hook!r} ran once", first)
                        require(not any(marker in first + action for marker in forbidden), f"{prompt_name}: did not source another startup chain", first + action)
                        require(first.count(OSC_A) == 1 and first.count(OSC_B) == 1, f"{prompt_name}: one initial prompt pair", first)
                        verify_prompt_trace(first, action, prompt_name)
                        verify_command_turn(action, cwd, hooks, prompt_name)
                    finally:
                        os.close(fd)
                        reap(pid)
                require(before == {path: path.read_bytes() for path in bash_files}, f"Bash {hook_form} startup files were not edited")
            print("Bash interactive and login startup, scalar and array prompt hooks, ordered OSC 133 trace, OSC 7 cwd: PASS")
        else:
            print("skipped Bash: no installed bash")

        if zsh_program:
            zdotdir = integration / "zdotdir"
            zdotdir.mkdir()
            for name in (".zshenv", ".zprofile", ".zshrc"):
                shutil.copyfile(scripts_dir / "folio.zsh", zdotdir / name)
            user_zdotdir = root / "user zsh config"
            user_zdotdir.mkdir()
            user_files = {
                ".zshenv": "print -r -- USER_ZSHENV\n",
                ".zprofile": "print -r -- USER_ZPROFILE\n",
                ".zshrc": (
                    "print -r -- USER_ZSHRC\n"
                    "PROMPT='ZSH_PTY> '\n"
                    "user_precmd() { print -r -- USER_ZSH_HOOK; }\n"
                    "autoload -Uz add-zsh-hook\nadd-zsh-hook precmd user_precmd\n"
                ),
                ".zlogin": "print -r -- USER_ZLOGIN\n",
            }
            for name, contents in user_files.items():
                (user_zdotdir / name).write_text(contents)
            before = {path: path.read_bytes() for path in user_zdotdir.iterdir()}
            env = dict(environment, ZDOTDIR=str(zdotdir), BT_USER_ZDOTDIR=str(user_zdotdir))
            prompt = b"ZSH_PTY> "
            pid, fd = spawn(zsh_program, ["-l", "-i"], env, home)
            try:
                first, action, _, _ = stop_at_prompt(pid, fd, command, prompt)
                for marker in (b"USER_ZSHENV", b"USER_ZPROFILE", b"USER_ZSHRC", b"USER_ZLOGIN", b"USER_ZSH_HOOK"):
                    require(marker in first, f"Zsh login: startup marker {marker.decode()}", first)
                require(first.count(OSC_A) == 1 and first.count(OSC_B) == 1, "Zsh: one initial prompt pair", first)
                verify_prompt_trace(first, action, "Zsh login")
                verify_command_turn(action, cwd, (b"USER_ZSH_HOOK",), "Zsh login")
            finally:
                os.close(fd)
                reap(pid)
            require(before == {path: path.read_bytes() for path in user_zdotdir.iterdir()}, "Zsh startup files were not edited")
            print("Zsh login startup, user ZDOTDIR restoration, hook, OSC 133 status, OSC 7 cwd: PASS")
        else:
            print("skipped Zsh: no installed zsh")

        plain_shells = (
            ("sh", "/bin/sh" if os.path.isfile("/bin/sh") and os.access("/bin/sh", os.X_OK) else None),
            ("dash", shutil.which("dash")),
            ("fish", shutil.which("fish")),
        )
        for name, program in plain_shells:
            if program is None:
                print(f"skipped {name}: no installed executable")
                continue
            verify_plain_shell(name, program, cwd, environment)
            print(f"{name} command, cwd, exit status, and no-hook boundary: PASS")


def sys_platform_linux():
    return os.name == "posix" and Path("/proc/sys/kernel/ostype").exists()


if __name__ == "__main__":
    main()
