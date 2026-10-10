#!/usr/bin/env python3
"""Run Folio web-policy scenarios in a private Xorg session."""

import argparse
import json
import os
from pathlib import Path
import re
import selectors
import shlex
import shutil
import signal
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

REPO = Path(__file__).resolve().parents[2]
CHROME = REPO / "target/luna-web-policy-deps/chrome-full-154/chrome-linux64/chrome"
DESKTOP_ROOT = REPO / "target/luna-web-policy-deps/desktop-tools/root"
XORG = DESKTOP_ROOT / "usr/libexec/Xorg"
XORG_MODULES = REPO / "target/luna-web-policy-deps/xorg-module-overlay"
XORG_CONFIG = REPO / "scripts/ci/linux-input-xorg.conf"
OPENBOX = DESKTOP_ROOT / "usr/bin/openbox"
OPENBOX_CONFIG = DESKTOP_ROOT / "etc/xdg/openbox/rc.xml"
XDOTOOL = DESKTOP_ROOT / "usr/bin/xdotool"

hits = []
websockets = []
input_fixture_mode = False
blank_navigation_fixture_mode = False


class Handler(BaseHTTPRequestHandler):
    def _send(self, body, mime="text/html; charset=utf-8", status=200):
        self.send_response(status)
        self.send_header("content-type", mime)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        hits.append({"path": self.path, "host": self.headers.get("Host")})
        if self.path.startswith("/ws"):
            websockets.append(self.headers.get("Upgrade", ""))
            key = self.headers.get("Sec-WebSocket-Key")
            if key:
                import base64
                import hashlib
                accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
                self.send_response(101)
                self.send_header("Upgrade", "websocket")
                self.send_header("Connection", "Upgrade")
                self.send_header("Sec-WebSocket-Accept", accept)
                self.end_headers()
                self.wfile.flush()
            return
        if self.path == "/state":
            self._send(json.dumps({"hits": hits, "websockets": websockets}).encode(), "application/json")
        elif input_fixture_mode and self.path == "/":
            port = self.server.server_port
            self._send(
                f"<!doctype html><meta charset=utf-8><title>Folio browser input fixture</title>"
                f"<h1>FOLIO_INPUT_ROOT</h1><input id=input value=fixture>"
                f"<iframe title=same-origin src=/same-frame></iframe>"
                f"<iframe title=cross-origin src=http://127.0.0.2:{port}/oopif></iframe>".encode()
            )
        elif input_fixture_mode and self.path == "/same-frame":
            port = self.server.server_port
            self._send(
                f"<!doctype html><title>Folio same-origin frame</title><h2>FOLIO_SAME_FRAME</h2>"
                f"<iframe src=http://127.0.0.2:{port}/oopif></iframe>".encode()
            )
        elif input_fixture_mode and self.path == "/oopif":
            self._send(b"<!doctype html><title>Folio OOPIF</title><h2>FOLIO_OOPIF_FRAME</h2>")
        elif self.path == "/file-network.js":
            self._send(b"window.folioFileNetworkAllowed=true;", "application/javascript")
        elif self.path in ("/", "/?from=redirect"):
            body = """<!doctype html><meta charset=utf-8><title>Folio web policy</title>
<style>body{font:28px sans-serif;background:#f3f8ff;color:#112;padding:20px}
button{font:22px sans-serif;padding:14px;background:#fff;border:3px solid #3478c6}
.mark{color:#125c29;font-weight:bold}</style>
<h1 class=mark>FOLIO_WEB_NOTHING_MARKER</h1>
<button id=user-open>USER POPUP TEST</button>
__BLANK_NAV_LINK__
<p id=status>waiting</p>
<img alt=allowed src=/pixel.svg>
<img alt=file-denied src=file:///etc/hosts>
<img alt=share-denied src=file://example.invalid/share/pixel.svg>
<script src=/redirect.js></script>
<script src=/redirect-file.js></script>
<script src=/redirect-chrome.js></script>
<iframe id=about-blank src=about:blank></iframe>
<iframe id=oopif src=http://localhost:__PORT__/frame></iframe>
<script>
document.querySelector('#user-open').addEventListener('click',event=>{fetch('/user-button-click?trusted='+event.isTrusted);window.open('/popup-user?source=button','_blank')});
for(const type of ['pointerdown','mousedown','mouseup','click'])document.addEventListener(type,event=>fetch('/event?type='+type+'&trusted='+event.isTrusted+'&target='+encodeURIComponent(event.target.id||event.target.tagName)+'&x='+event.clientX+'&y='+event.clientY),true);
setTimeout(()=>{window.open('/popup-script?source=timer','_blank');__TIMER_COMPLETE__},1800);
Promise.all([fetch('/allowed-fetch').then(r=>r.text()), fetch('/through-sw').then(r=>r.text()).catch(e=>'sw-error')])
  .then(v=>document.querySelector('#status').textContent='HTTP '+v.join(' '));
navigator.serviceWorker.register('/sw.js').then(()=>navigator.serviceWorker.ready)
  .then(()=>navigator.serviceWorker.controller ? true : new Promise(resolve=>navigator.serviceWorker.addEventListener('controllerchange',()=>resolve(true),{once:true})))
  .then(()=>fetch('/through-sw').then(r=>r.text()).then(v=>document.body.dataset.sw=v).catch(()=>{}));
const shared=new SharedWorker('/shared-worker.js','folio-policy-shared');shared.port.start();shared.port.postMessage('ui');
window.socket=new WebSocket('ws://127.0.0.1:__PORT__/ws');
</script>""".replace("__PORT__", str(self.server.server_port)).replace(
                "__BLANK_NAV_LINK__",
                "<a id=blank-nav href=about:blank style='position:fixed;left:10px;top:330px;font:18px sans-serif;background:white;padding:8px'>BLANK NAVIGATION GUARD</a>"
                if blank_navigation_fixture_mode else "",
            ).replace(
                "__TIMER_COMPLETE__",
                "fetch('/timer-popup-attempted')" if blank_navigation_fixture_mode else "",
            ).encode()
            self._send(body)
        elif self.path == "/frame":
            body = b"<!doctype html><title>OOPIF</title><h2>FOLIO_OOPIF_MARKER</h2><script src=/frame.js></script><img src=/pixel.svg>"
            self._send(body)
        elif self.path == "/frame.js":
            self._send(b"window.folioOopifScript='ready';", "application/javascript")
        elif self.path == "/sw.js":
            self._send(
                b"self.addEventListener('install',e=>e.waitUntil(fetch('/install-init').then(()=>self.skipWaiting())));"
                b"self.addEventListener('activate',e=>e.waitUntil(self.clients.claim()));"
                b"self.addEventListener('fetch',e=>{if(new URL(e.request.url).pathname==='/through-sw')"
                b"e.respondWith(fetch('/worker-side').then(r=>r.text()).then(t=>new Response('SW_'+t)))})",
                "application/javascript",
            )
        elif self.path == "/shared-worker.js":
            self._send(
                b"self.onconnect=e=>{const p=e.ports[0];p.start();p.onmessage=async()=>{"
                b"const r=await fetch('/worker-side');p.postMessage(await r.text())}}",
                "application/javascript",
            )
        elif self.path.startswith("/user-button-click") or self.path in ("/allowed-fetch", "/worker-side", "/install-init", "/final.js", "/timer-popup-attempted"):
            body = b"window.folioRedirect='FOLIO_REDIRECT_FINAL';" if self.path == "/final.js" else self.path.encode()
            mime = "application/javascript" if self.path == "/final.js" else "text/plain"
            self._send(body, mime)
        elif self.path == "/redirect.js":
            self.send_response(302)
            self.send_header("location", "/final.js")
            self.send_header("content-length", "0")
            self.end_headers()
        elif self.path == "/redirect-file.js":
            self.send_response(302)
            self.send_header("location", "file:///etc/hosts")
            self.send_header("content-length", "0")
            self.end_headers()
        elif self.path == "/redirect-chrome.js":
            self.send_response(302)
            self.send_header("location", "chrome://resources/js/assert.js")
            self.send_header("content-length", "0")
            self.end_headers()
        elif self.path == "/pixel.svg":
            self._send(b"<svg xmlns='http://www.w3.org/2000/svg' width='60' height='40'><rect width='60' height='40' fill='#19a35b'/></svg>", "image/svg+xml")
        elif self.path.startswith("/popup-user"):
            self._send(b"<!doctype html><body><h1>FOLIO_USER_POPUP_ROUTED</h1></body>")
        elif self.path.startswith("/popup-script"):
            self._send(b"<!doctype html><body><h1>FOLIO_SCRIPT_POPUP</h1></body>")
        else:
            self._send(b"not found", "text/plain", 404)

    def do_POST(self):
        self._send(b"ok", "text/plain")

    def do_GET_WS(self):
        pass

    def log_message(self, *_):
        pass


class Server(ThreadingHTTPServer):
    daemon_threads = True


def stop_process(process, name):
    if process is None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
    print(f"STOP {name} pid={process.pid}", flush=True)


def read_display_number(process, read_fd, log_path):
    deadline = time.monotonic() + 15
    with selectors.DefaultSelector() as selector:
        selector.register(read_fd, selectors.EVENT_READ)
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f"private Xorg exited: {log_path.read_text(errors='replace')}")
            events = selector.select(min(0.25, deadline-time.monotonic()))
            if not events:
                continue
            data = os.read(read_fd, 64)
            line, sep, _ = data.partition(b"\n")
            if sep and line.isdigit():
                return int(line)
            if sep:
                raise RuntimeError(f"invalid Xorg display number {data!r}")
    raise RuntimeError("private Xorg did not publish a display number")


def wait_for(predicate, description, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {description}")


def read_log(path):
    try:
        return Path(path).read_text(errors="replace")
    except FileNotFoundError:
        return ""


def run_tool(xdotool, env, *args):
    result = subprocess.run(
        [str(xdotool), *map(str, args)], env=env, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors="replace"))
    return result.stdout.decode(errors="replace").strip()


def click_preview_button(xdotool, env, window, web_bounds):
    match = re.search(r"bounds tab=\d+ seat=\d+ x=(-?\d+) y=(-?\d+) w=(\d+) h=(\d+)", web_bounds)
    if not match:
        return False
    left, top, width, height = map(int, match.groups())
    x = left + min(60, max(20, width // 8))
    y = top + height * 2 // 5
    run_tool(xdotool, env, "windowactivate", "--sync", str(window))
    run_tool(xdotool, env, "mousemove", "--sync", str(x), str(y))
    run_tool(xdotool, env, "click", "1")
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--folio", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, default=REPO / "target/linux-web-policy-e2e")
    parser.add_argument("--desktop-root", type=Path, default=DESKTOP_ROOT)
    parser.add_argument("--xorg", type=Path)
    parser.add_argument("--modulepath", type=Path, default=XORG_MODULES)
    parser.add_argument("--xdotool", type=Path)
    parser.add_argument("--openbox", type=Path)
    parser.add_argument("--openbox-config", type=Path)
    parser.add_argument("--xorg-config", type=Path, default=XORG_CONFIG)
    parser.add_argument("--cft", type=Path, default=CHROME)
    parser.add_argument("--scenario", choices=("nothing", "file", "unsupported-mv3", "missing-chromium"))
    parser.add_argument("--input-fixture", action="store_true")
    parser.add_argument("--blank-navigation-check", action="store_true", help="exercise an unminted top-level about:blank click after the script-popup probe")
    parser.add_argument("--document", type=Path, help="open this local document in the file-mint scenario")
    args = parser.parse_args()
    global input_fixture_mode, blank_navigation_fixture_mode
    input_fixture_mode = args.input_fixture
    blank_navigation_fixture_mode = args.blank_navigation_check
    if args.blank_navigation_check and args.scenario not in (None, "nothing"):
        parser.error("--blank-navigation-check applies only to the Nothing-mint scenario")
    args.xorg = args.xorg or args.desktop_root / "usr/libexec/Xorg"
    args.xdotool = args.xdotool or args.desktop_root / "usr/bin/xdotool"
    args.openbox = args.openbox or args.desktop_root / "usr/bin/openbox"
    args.openbox_config = args.openbox_config or args.desktop_root / "etc/xdg/openbox/rc.xml"

    for path, label in ((args.folio, "Folio executable"), (args.xorg, "Xorg"),
                        (args.modulepath, "Xorg module path"), (args.xdotool, "xdotool"),
                        (args.openbox, "Openbox"), (args.openbox_config, "Openbox config"),
                        (args.xorg_config, "Xorg config"), (args.cft, "full Chromium")):
        if not path.exists():
            parser.error(f"{label} does not exist: {path}")
    if not os.access(args.folio, os.X_OK) or not os.access(args.xorg, os.X_OK):
        parser.error("Folio and Xorg must be executable")
    if args.document is not None and not args.document.is_file():
        parser.error(f"document does not exist: {args.document}")

    args.artifacts.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="web-policy-", dir=args.artifacts))
    runtime_base = Path(tempfile.mkdtemp(prefix="folio-policy-") )
    runtime = runtime_base / "runtime"
    runtime.mkdir(mode=0o700)
    private_dirs = {
        "HOME": root / "home",
        "XDG_CONFIG_HOME": root / "config",
        "XDG_DATA_HOME": root / "data",
        "XDG_CACHE_HOME": root / "cache",
        "XDG_STATE_HOME": root / "state",
    }
    for directory in private_dirs.values():
        directory.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    for name in (
        "DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "XAUTHORITY", "XDG_RUNTIME_DIR",
        "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME",
        "DBUS_SESSION_BUS_ADDRESS", "DBUS_SYSTEM_BUS_ADDRESS", "SESSION_MANAGER",
        "DESKTOP_SESSION", "XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE", "PULSE_SERVER",
        "PIPEWIRE_REMOTE", "JACK_SERVER", "NIRI_SOCKET",
    ):
        env.pop(name, None)
    env.update({name: str(path) for name, path in private_dirs.items()})
    env.update(
        XDG_RUNTIME_DIR=str(runtime), XDG_SESSION_TYPE="x11", WINIT_UNIX_BACKEND="x11",
        TERM="xterm-256color", LC_ALL="C.UTF-8", HISTFILE="/dev/null",
        LIBGL_ALWAYS_SOFTWARE="1", GST_AUDIO_SINK="fakesink", BT_GPU_PREFERENCE="low",
        BT_STARTUP_TRACE="1", BT_PTY_DUMP=str(root / "pty.dump"),
        BT_WEB_TRACE=str(root / "web.trace"), BT_CHROME_DUMP=str(root / "chrome.dump"),
        BT_MOUSE_TRACE=str(root / "mouse.trace"),
        FOLIO_CHROMIUM_PATH=str(args.cft.resolve()),
    )
    web_server = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=web_server.serve_forever, daemon=True).start()
    secondary_server = Server(("127.0.0.2", web_server.server_port), Handler)
    threading.Thread(target=secondary_server.serve_forever, daemon=True).start()
    base_url = f"http://127.0.0.1:{web_server.server_port}/"
    shell = root / "probe-shell"
    shell.write_text(
        "#!/bin/sh\n"
        "printf 'FOLIO_WEB_TERMINAL_READY\\n'\n"
        "printf 'SHELL_STARTED pid=%s ppid=%s tty=%s\\n' \"$$\" \"$PPID\" \"$(tty)\" >> \"$FOLIO_SHELL_LEDGER\"\n"
        "exec /bin/bash --noprofile --norc -i\n"
    )
    shell.chmod(0o700)
    env["SHELL"] = str(shell)
    env["FOLIO_SHELL_LEDGER"] = str(root / "shells.log")
    xorg_log = (root / "xorg.log").open("wb")
    read_fd, write_fd = os.pipe()
    xorg_env = env.copy()
    xorg_env["LD_LIBRARY_PATH"] = str(args.desktop_root / "usr/lib64")
    xorg = subprocess.Popen(
        [str(args.xorg), "-displayfd", str(write_fd), "-config", str(args.xorg_config),
         "-modulepath", str(args.modulepath), "-logfile", str(root / "Xorg.log"),
         "-nolisten", "tcp", "-ac", "-noreset", "-novtswitch", "-sharevts"],
        env=xorg_env, cwd=root, stdin=subprocess.DEVNULL, stdout=xorg_log,
        stderr=subprocess.STDOUT, pass_fds=(write_fd,), start_new_session=True,
    )
    os.close(write_fd)
    display = read_display_number(xorg, read_fd, root / "xorg.log")
    os.close(read_fd)
    env["DISPLAY"] = f":{display}"
    xdotool_env = env | {"LD_LIBRARY_PATH": str(args.desktop_root / "usr/lib64")}
    wm_log = (root / "openbox.log").open("wb")
    wm = subprocess.Popen(
        [str(args.openbox), "--config-file", str(args.openbox_config)], env=xorg_env | {"DISPLAY": f":{display}"},
        cwd=root, stdin=subprocess.DEVNULL, stdout=wm_log, stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    print(json.dumps({"root": str(root), "display": env["DISPLAY"], "webUrl": base_url}), flush=True)
    try:
        server_state = {"process": None, "scenario": None}
        scenarios = [
            {"name": "nothing", "env": {"BT_WEB_DEV": base_url}},
            {"name": "file", "env": {}},
            {"name": "unsupported-mv3", "env": {"BT_WEB_DEV": base_url}},
            {"name": "missing-chromium", "env": {"BT_WEB_DEV": base_url}},
        ]
        if args.scenario:
            scenarios = [scenario for scenario in scenarios if scenario["name"] == args.scenario]
        no_extension_chrome = root / "chrome-without-unpacked-mv3"
        no_extension_chrome.write_text(
            "#!/bin/bash\n"
            "args=()\n"
            "for arg in \"$@\"; do\n"
            "  case \"$arg\" in\n"
            "    --load-extension=*|--disable-extensions-except=*) ;;\n"
            "    *) args+=(\"$arg\");;\n"
            "  esac\n"
            "done\n"
            f"exec {shlex.quote(str(args.cft.resolve()))} --disable-extensions \"${{args[@]}}\"\n"
        )
        no_extension_chrome.chmod(0o700)
        for scenario in scenarios:
            server_state["scenario"] = scenario["name"]
            scenario_root = root / scenario["name"]
            scenario_root.mkdir()
            scenario_env = env.copy()
            scenario_env.update(scenario["env"])
            # Use one private app state namespace per launch.
            for variable, directory in (
                ("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state"),
            ):
                path = scenario_root / directory
                path.mkdir(parents=True, exist_ok=True)
            scenario_env[variable] = str(path)
            scenario_runtime = runtime_base / scenario["name"]
            scenario_runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
            scenario_env["XDG_RUNTIME_DIR"] = str(scenario_runtime)
            for name, suffix in (("BT_PTY_DUMP", "pty.dump"), ("BT_WEB_TRACE", "web.trace"),
                                 ("BT_CHROME_DUMP", "chrome.dump"), ("BT_MOUSE_TRACE", "mouse.trace")):
                scenario_env[name] = str(scenario_root / suffix)
            scenario_env["BT_STARTUP_TRACE"] = "1"
            scenario_env["FOLIO_SHELL_LEDGER"] = str(scenario_root / "shells.log")
            scenario_env.pop("FOLIO_CHROMIUM_PATH", None)
            file_page = scenario_root / "web-fixture.html"
            file_page.write_text(
                "<!doctype html><meta charset=utf-8><title>Folio file policy</title>"
                "<style>body{font:28px sans-serif;background:#f3f8ff;color:#112;padding:20px}</style>"
                "<h1>FOLIO_WEB_FILE_MARKER</h1><img src='file-pixel.svg'>"
                f"<script src='http://127.0.0.1:{web_server.server_port}/file-network.js'></script>"
                "<img src='file://example.invalid/share/pixel.svg'>"
                "<p>FOLIO_FILE_REMOTE_RESOURCE_ALLOWED</p>"
            )
            (scenario_root / "file-pixel.svg").write_text(
                "<svg xmlns='http://www.w3.org/2000/svg' width='60' height='40'>"
                "<rect width='60' height='40' fill='#19a35b'/></svg>"
            )
            if scenario["name"] == "unsupported-mv3":
                scenario_env["FOLIO_CHROMIUM_PATH"] = str(no_extension_chrome)
            elif scenario["name"] == "missing-chromium":
                scenario_env["FOLIO_CHROMIUM_PATH"] = str(scenario_root / "missing-chromium")
            else:
                scenario_env["FOLIO_CHROMIUM_PATH"] = str(args.cft.resolve())
            app_args = ["--profile", "usershell", "--cwd", str(scenario_root)]
            if scenario["name"] == "file":
                app_args.append(str(args.document.resolve() if args.document else file_page))
            else:
                scenario_env.update(scenario["env"])
            app_log = scenario_root / "folio.log"
            hits_start = len(hits)
            with app_log.open("wb") as output:
                app = subprocess.Popen(
                    [str(args.folio), *app_args], cwd=root, env=scenario_env,
                    stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                    start_new_session=True,
                )
            server_state["process"] = app
            print(f"START Folio scenario={scenario['name']} pid={app.pid}", flush=True)
            wait_for(lambda: (scenario_root / "shells.log").exists(), "PTY shell process", seconds=60)
            wait_for(lambda: "BT_STARTUP first_text_present=" in read_log(app_log),
                     "terminal text presented", seconds=60)
            wait_for(lambda: "Welcome to Folio" in read_log(scenario_env["BT_CHROME_DUMP"]),
                     "first-run welcome page", seconds=10)
            window_search = subprocess.run(
                [str(args.xdotool), "search", "--onlyvisible", "--name", scenario["name"]],
                env=xdotool_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5,
            )
            windows = window_search.stdout.decode().split()
            if not windows:
                raise RuntimeError(
                    "could not find Folio's visible X11 window: "
                    + window_search.stderr.decode(errors="replace")
                )
            run_tool(args.xdotool, xdotool_env, "windowactivate", "--sync", windows[0])
            run_tool(args.xdotool, xdotool_env, "mousemove", "--sync", "645", "362")
            run_tool(args.xdotool, xdotool_env, "click", "1")
            run_tool(args.xdotool, xdotool_env, "key", "--clearmodifiers", "Return")
            wait_for(lambda: "Welcome to Folio" not in read_log(scenario_env["BT_CHROME_DUMP"])[-10000:],
                     "dismissed first-run welcome page", seconds=10)
            if scenario["name"] == "nothing":
                wait_for(lambda: any(item["path"] == "/" for item in hits[hits_start:]), "main web document", seconds=60)
                if args.blank_navigation_check:
                    wait_for(lambda: any(item["path"] == "/timer-popup-attempted" for item in hits[hits_start:]), "script popup attempt without user activation", seconds=10)
                    run_tool(args.xdotool, xdotool_env, "windowactivate", "--sync", windows[0])
                    run_tool(args.xdotool, xdotool_env, "mousemove", "--sync", "550", "450")
                    run_tool(args.xdotool, xdotool_env, "click", "1")
                    wait_for(lambda: re.search(r"navigation_starting .* uri=about:blank .* verdict=refuse:", read_log(scenario_env["BT_WEB_TRACE"])) is not None, "top-frame about:blank refused by Nothing mint", seconds=8)
            elif scenario["name"] == "file":
                if args.document is None:
                    wait_for(lambda: any(item["path"] == "/file-network.js" for item in hits[hits_start:]), "network resource from a file mint", seconds=60)
                else:
                    wait_for(
                        lambda: re.search(
                            r"navigation_completed .* uri=file:.* success=1",
                            read_log(scenario_env["BT_WEB_TRACE"]),
                        ) is not None,
                        "local document navigation completed",
                        seconds=60,
                    )
            time.sleep(2)
            screenshot = scenario_root / "screen.png"
            subprocess.run(
                ["import", "-display", env["DISPLAY"], "-window", "root", str(screenshot)],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=True,
            )
            ocr = subprocess.run(["tesseract", str(screenshot), "stdout", "--psm", "6"],
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
            trace = read_log(scenario_env["BT_WEB_TRACE"])
            pty = Path(scenario_env["BT_PTY_DUMP"]).read_bytes() if Path(scenario_env["BT_PTY_DUMP"]).exists() else b""
            popup_click = None
            popup_routed = None
            if scenario["name"] == "nothing":
                popup_click = click_preview_button(args.xdotool, xdotool_env, windows[0], trace)
                if popup_click:
                    try:
                        wait_for(lambda: any(item["path"].startswith("/popup-user") for item in hits[hits_start:]),
                                 "trusted window.open routed into the existing pane", seconds=10)
                        popup_routed = True
                    except RuntimeError:
                        popup_routed = False
                    trace = read_log(scenario_env["BT_WEB_TRACE"])
            state = {
                "scenario": scenario["name"],
                "appPid": app.pid,
                "appAlive": app.poll() is None,
                "terminalShellStarted": (scenario_root / "shells.log").read_text(errors="replace"),
                "terminalPtyReady": b"FOLIO_WEB_TERMINAL_READY" in pty,
                "visibleWindows": windows,
                "popupClickAttempted": popup_click,
                "popupRouted": popup_routed,
                "blankNavigationDenied": bool(re.search(r"navigation_starting .* uri=about:blank .* verdict=refuse:", trace)),
                "screenshot": str(screenshot),
                "screenshotOcr": ocr.stdout.decode(errors="replace"),
                "webTrace": trace,
                "serverHits": hits,
                "chromeDumpTail": read_log(scenario_env["BT_CHROME_DUMP"])[-10000:],
                "startupTail": read_log(app_log)[-5000:],
            }
            print(json.dumps(state, ensure_ascii=False, indent=2), flush=True)
            subprocess.run([str(args.xdotool), "key", "--clearmodifiers", "ctrl+shift+q"],
                           env=xdotool_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
            try:
                app.wait(timeout=15)
            except subprocess.TimeoutExpired:
                stop_process(app, f"Folio {scenario['name']}")
            server_state["process"] = None
    finally:
        if server_state.get("process") is not None:
            stop_process(server_state["process"], "Folio")
        web_server.shutdown()
        secondary_server.shutdown()
        (root / "server-state.json").write_text(
            json.dumps({"hits": hits, "websockets": websockets}, ensure_ascii=False, indent=2)
        )
        stop_process(wm, "Openbox")
        stop_process(xorg, "Xorg")
        xorg_log.close()
        wm_log.close()
        shutil.rmtree(runtime_base, ignore_errors=True)


if __name__ == "__main__":
    main()
