#!/usr/bin/env python3
"""Exercise the production navigation bootstrap with private headless Chromium."""

import argparse
from collections import deque
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import tempfile
import threading
import time


REPO = Path(__file__).resolve().parents[2]
BOOTSTRAP = (
    REPO / "crates/bt-platform/src/linux_web_extension/navigation_gate.js"
)
TOKEN = "folio-navigation-correlation-smoke-token"
BINDING = "folioNavigationGateSmoke"
PERMIT = "__folioNavigationPermitSmoke"


class CdpPipe:
    def __init__(self, process, read_fd, write_fd):
        self.process = process
        self.read_fd = read_fd
        self.write_fd = write_fd
        self.messages = queue.Queue()
        self.buffer = bytearray()
        self.pending = {}
        self.events = deque()
        self.next_id = 1
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        while True:
            try:
                chunk = os.read(self.read_fd, 65536)
            except OSError as error:
                self.messages.put(error)
                return
            if not chunk:
                self.messages.put(EOFError("Chromium closed its CDP pipe"))
                return
            self.buffer.extend(chunk)
            while b"\0" in self.buffer:
                raw, _, remainder = self.buffer.partition(b"\0")
                self.buffer[:] = remainder
                if raw:
                    try:
                        self.messages.put(json.loads(raw))
                    except (UnicodeDecodeError, json.JSONDecodeError) as error:
                        self.messages.put(error)

    def _next_message(self, timeout):
        try:
            message = self.messages.get(timeout=timeout)
        except queue.Empty as error:
            raise TimeoutError("timed out waiting for Chromium CDP") from error
        if isinstance(message, BaseException):
            raise message
        return message

    def send(self, method, params=None, session=None):
        message_id = self.next_id
        self.next_id += 1
        message = {"id": message_id, "method": method, "params": params or {}}
        if session:
            message["sessionId"] = session
        payload = memoryview(
            json.dumps(message, separators=(",", ":")).encode() + b"\0"
        )
        while payload:
            written = os.write(self.write_fd, payload)
            payload = payload[written:]
        return message_id

    def command(self, method, params=None, session=None, timeout=15):
        message_id = self.send(method, params, session)
        deadline = time.monotonic() + timeout
        while True:
            message = self.pending.pop(message_id, None)
            if message is None:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(f"Chromium CDP command timed out: {method}")
                message = self._next_message(remaining)
            if message.get("id") == message_id:
                if "error" in message:
                    raise RuntimeError(f"CDP {method} failed: {message['error']}")
                return message.get("result", {})
            if "id" in message:
                self.pending[message["id"]] = message
            else:
                self.events.append(message)

    def event(self, predicate, timeout=15):
        deadline = time.monotonic() + timeout
        while True:
            for index, message in enumerate(self.events):
                if predicate(message):
                    del self.events[index]
                    return message
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Chromium did not emit the expected CDP event")
            message = self._next_message(remaining)
            if "id" in message:
                self.pending[message["id"]] = message
            elif predicate(message):
                return message
            else:
                self.events.append(message)

    def close(self):
        for descriptor in (self.read_fd, self.write_fd):
            try:
                os.close(descriptor)
            except OSError:
                pass


def start_chromium(executable, profile, cache, runtime):
    to_browser_read, to_browser_write = os.pipe()
    from_browser_read, from_browser_write = os.pipe()
    child_read = os.dup(to_browser_read)
    child_write = os.dup(from_browser_write)
    for descriptor in (to_browser_read, from_browser_write):
        os.close(descriptor)
    os.set_inheritable(child_read, True)
    os.set_inheritable(child_write, True)

    environment = os.environ.copy()
    for name in (
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "WAYLAND_SOCKET",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "DBUS_SYSTEM_BUS_ADDRESS",
        "SESSION_MANAGER",
    ):
        environment.pop(name, None)
    environment.update(
        HOME=str(profile / "home"),
        TMPDIR=str(runtime),
        XDG_CACHE_HOME=str(profile / "xdg-cache"),
        XDG_CONFIG_HOME=str(profile / "xdg-config"),
        XDG_DATA_HOME=str(profile / "xdg-data"),
        XDG_RUNTIME_DIR=str(runtime),
        XDG_STATE_HOME=str(profile / "xdg-state"),
    )
    for directory in (
        profile / "home",
        profile / "xdg-cache",
        profile / "xdg-config",
        profile / "xdg-data",
        profile / "xdg-state",
    ):
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    chromium_args = [
        "--headless=new",
        "--remote-debugging-pipe",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-sync",
        "--site-per-process",
        "--window-size=640,480",
        f"--user-data-dir={profile / 'profile'}",
        f"--disk-cache-dir={cache}",
    ]
    remap_and_exec = (
        "import os, sys; "
        "os.dup2(int(sys.argv[1]), 3); "
        "os.dup2(int(sys.argv[2]), 4); "
        "os.execv(sys.argv[3], sys.argv[3:])"
    )
    process = subprocess.Popen(
        [
            sys.executable,
            "-c",
            remap_and_exec,
            str(child_read),
            str(child_write),
            str(executable),
            *chromium_args,
        ],
        pass_fds=(child_read, child_write),
        start_new_session=True,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        env=environment,
    )
    os.close(child_read)
    os.close(child_write)
    return process, CdpPipe(process, from_browser_read, to_browser_write)


def click(pipe, session, x, y):
    pipe.command(
        "Input.dispatchMouseEvent",
        {"type": "mouseMoved", "x": x, "y": y},
        session,
    )
    for event_type in ("mousePressed", "mouseReleased"):
        pipe.command(
            "Input.dispatchMouseEvent",
            {
                "type": event_type,
                "x": x,
                "y": y,
                "button": "left",
                "clickCount": 1,
            },
            session,
        )


def stop_chromium(process, pipe):
    try:
        pipe.command("Browser.close", timeout=2)
    except (OSError, RuntimeError, TimeoutError, EOFError):
        pass
    pipe.close()
    try:
        process.wait(timeout=4)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()


def run_probe(executable, target):
    target.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="folio-nav-smoke-", dir=target) as name:
        profile = Path(name)
        cache = profile / "cache"
        cache.mkdir(mode=0o700)
        with tempfile.TemporaryDirectory(prefix="fn-", dir="/tmp") as runtime_name:
            runtime = Path(runtime_name)
            runtime.chmod(0o700)
            fixture = profile / "navigation.html"
            fixture.write_text(
                """<!doctype html><meta charset=utf-8><title>Folio gate fixture</title>
                <style>body{margin:0}a{position:absolute;left:20px}</style>
                <a id=older href="about:blank#older-request" style="top:20px">older</a>
                <a id=newer href="about:blank#newer-request" style="top:70px">newer</a>""",
                encoding="utf-8",
            )
            process, pipe = start_chromium(executable, profile, cache, runtime)
            target_id = None
            session = None
            try:
                version = pipe.command("Browser.getVersion")
                target_id = pipe.command(
                    "Target.createTarget", {"url": "about:blank"}
                )["targetId"]
                session = pipe.command(
                    "Target.attachToTarget",
                    {"targetId": target_id, "flatten": True},
                )["sessionId"]
                pipe.command("Page.enable", session=session)
                pipe.command("Runtime.enable", session=session)
                pipe.command("Runtime.addBinding", {"name": BINDING}, session)
                source = BOOTSTRAP.read_text(encoding="utf-8")
                source = (
                    source.replace("__FOLIO_BINDING_NAME__", BINDING)
                    .replace("__FOLIO_PERMIT_NAME__", PERMIT)
                    .replace("__FOLIO_AUTH_TOKEN__", TOKEN)
                )
                pipe.command(
                    "Page.addScriptToEvaluateOnNewDocument",
                    {"source": source},
                    session,
                )
                pipe.command(
                    "Runtime.evaluate",
                    {"expression": source, "returnByValue": True},
                    session,
                )

                fixture_url = fixture.as_uri()
                permit_expression = (
                    f"globalThis[{json.dumps(PERMIT)}]("
                    f"{json.dumps(fixture_url)}, {json.dumps(TOKEN)})"
                )
                pipe.command(
                    "Runtime.evaluate",
                    {"expression": permit_expression, "returnByValue": True},
                    session,
                )
                pipe.command("Page.navigate", {"url": fixture_url}, session)
                pipe.event(
                    lambda message: message.get("sessionId") == session
                    and message.get("method") == "Page.loadEventFired"
                )
                first_navigation = pipe.command(
                    "Runtime.evaluate",
                    {
                        "expression": "location.href",
                        "returnByValue": True,
                    },
                    session,
                )["result"]["value"]
                if first_navigation != fixture_url:
                    raise RuntimeError(
                        f"fixture navigation did not complete: {first_navigation}"
                    )

                requests = []
                for x, y in ((35, 29), (35, 79)):
                    click(pipe, session, x, y)
                    event = pipe.event(
                        lambda message: message.get("sessionId") == session
                        and message.get("method") == "Runtime.bindingCalled"
                        and message.get("params", {}).get("name") == BINDING
                    )
                    payload = json.loads(event["params"]["payload"])
                    requests.append(payload)

                expected_urls = [
                    "about:blank#older-request",
                    "about:blank#newer-request",
                ]
                for index, (payload, expected_url) in enumerate(
                    zip(requests, expected_urls), start=1
                ):
                    if payload.get("token") != TOKEN:
                        raise RuntimeError(f"unexpected policy token: {payload}")
                    if payload.get("id") != index:
                        raise RuntimeError(f"unexpected request order: {payload}")
                    if payload.get("url") != expected_url:
                        raise RuntimeError(f"unexpected requested URL: {payload}")
                    if payload.get("cancelable") is not True:
                        raise RuntimeError(
                            f"navigation was not cancelable: {payload}"
                        )

                after_navigation = pipe.command(
                    "Runtime.evaluate",
                    {"expression": "location.href", "returnByValue": True},
                    session,
                )["result"]["value"]
                if after_navigation != fixture_url:
                    raise RuntimeError(
                        "the production bootstrap did not keep both navigations "
                        f"on the fixture: {after_navigation}"
                    )

                print(
                    json.dumps(
                        {
                            "chromium": version.get("product"),
                            "fixture_url": fixture_url,
                            "binding_events": requests,
                            "final_url": after_navigation,
                            "result": "production bootstrap emitted two ordered, "
                            "cancelable top-level navigation requests",
                        },
                        ensure_ascii=False,
                        indent=2,
                    )
                )
            finally:
                if target_id and session:
                    try:
                        pipe.command(
                            "Target.closeTarget", {"targetId": target_id}, timeout=2
                        )
                    except (OSError, RuntimeError, TimeoutError, EOFError):
                        pass
                stop_chromium(process, pipe)
                stderr = process.stderr.read() if process.stderr else b""
                if stderr and process.returncode not in (0, -signal.SIGTERM):
                    print(stderr.decode(errors="replace")[-3000:])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--chromium",
        type=Path,
        default=Path(os.environ.get("FOLIO_CHROMIUM_PATH", "")),
        help="full Chromium binary; defaults to FOLIO_CHROMIUM_PATH",
    )
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=REPO / "target/linux-navigation-correlation-smoke",
        help="private directory for Chromium's profile, cache and fixture",
    )
    args = parser.parse_args()
    if not str(args.chromium) or not args.chromium.is_file():
        parser.error("pass --chromium or set FOLIO_CHROMIUM_PATH to full Chromium")
    run_probe(args.chromium.resolve(), args.target_dir.resolve())


if __name__ == "__main__":
    main()
