#!/usr/bin/env python3
"""Serve a loopback-only page for browser keyboard and IME acceptance probes."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import signal
import socket
import threading
from urllib.parse import urlsplit


class FixtureState:
    def __init__(self, events_file):
        self.events = []
        self.lock = threading.Lock()
        self.events_file = events_file

    def append(self, event):
        with self.lock:
            self.events.append(event)
            if self.events_file is not None:
                with self.events_file.open("a", encoding="utf-8") as stream:
                    stream.write(json.dumps(event, ensure_ascii=False) + "\n")

    def snapshot(self):
        with self.lock:
            return list(self.events)


def recorder(frame):
    return f"""<script>
(() => {{
  const frame = {json.dumps(frame)};
  let sequence = 0;
  let writes = Promise.resolve();
  const names = ["keydown", "beforeinput", "input", "compositionstart",
                 "compositionupdate", "compositionend", "focusin", "focusout"];
  for (const name of names) document.addEventListener(name, event => {{
    const target = event.target?.closest?.("[data-editable]");
    if (!target) return;
    const selection = window.getSelection();
    let caret = null;
    if (selection?.rangeCount) {{
      const rect = selection.getRangeAt(0).getBoundingClientRect();
      caret = {{x: rect.x, y: rect.y, width: rect.width, height: rect.height}};
    }}
    const snapshot = {{
      frame,
      sequence: ++sequence,
      event: name,
      id: target.id,
      tag: target.tagName.toLowerCase(),
      trusted: event.isTrusted,
      key: event.key ?? null,
      code: event.code ?? null,
      repeat: event.repeat ?? false,
      ctrl: event.ctrlKey ?? false,
      shift: event.shiftKey ?? false,
      alt: event.altKey ?? false,
      meta: event.metaKey ?? false,
      inputType: event.inputType ?? null,
      data: event.data ?? null,
      value: "value" in target ? target.value : target.innerText,
      selectionStart: "selectionStart" in target ? target.selectionStart : null,
      selectionEnd: "selectionEnd" in target ? target.selectionEnd : null,
      selectionAnchor: selection?.anchorOffset ?? null,
      selectionFocus: selection?.focusOffset ?? null,
      caret,
      activeId: document.activeElement?.id ?? null,
      activeTag: document.activeElement?.tagName?.toLowerCase() ?? null,
      focusedFrame: frame,
      url: location.href
    }};
    writes = writes.then(() => fetch("/record", {{
        method: "POST",
        headers: {{"Content-Type": "application/json"}},
        body: JSON.stringify(snapshot),
        keepalive: true
      }}).catch(() => {{}}));
  }}, true);
}})();
</script>"""


def frame_page(frame, body):
    return f"""<!doctype html>
<html lang="en" data-frame="{frame}">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Folio input fixture: {frame}</title>
<style>
  body {{ font: 16px sans-serif; margin: 16px; }}
  h1 {{ font-size: 24px; margin: 10px 0; }}
  h2 {{ font-size: 18px; margin: 8px 0; }}
  label {{ display: block; margin: 8px 0; }}
  textarea {{ display: block; width: 90%; height: 40px; }}
  [contenteditable] {{ border: 1px solid #888; min-height: 22px; margin: 8px 0; }}
  iframe {{ display: block; width: 95%; height: 200px; margin: 8px 0; border: 2px solid #466; }}
</style>
{body}
{recorder(frame)}
</html>"""


def page_for(path, port):
    if path == "/":
        body = f"""<h1>Folio browser input fixture</h1>
<label>Top input <input id="input" data-editable></label>
<iframe id="same-origin-frame" title="same-origin editable" src="/same-frame"></iframe>
<label>Top textarea <textarea id="textarea" data-editable></textarea></label>
<div id="editable" data-editable contenteditable="true">top editable</div>
<p id="focused-frame">top</p>"""
        return frame_page("top", body)
    if path == "/same-frame":
        body = f"""<h2>Same-origin frame</h2>
<label>Iframe input <input id="iframe-input" data-editable></label>
<div id="same-contenteditable" data-editable contenteditable="true">same frame editable</div>
<iframe id="oopif-frame" title="cross-site editable" src="http://127.0.0.2:{port}/oopif"></iframe>
<p id="focused-frame">same-origin</p>"""
        return frame_page("same-origin", body)
    if path == "/oopif":
        body = """<h2>Cross-site OOPIF</h2>
<label>OOPIF input <input id="oopif-input" data-editable></label>
<div id="oopif-contenteditable" data-editable contenteditable="true">OOPIF editable</div>
<p id="focused-frame">oopif-127.0.0.2</p>"""
        return frame_page("oopif-127.0.0.2", body)
    return None


class Handler(BaseHTTPRequestHandler):
    server_version = "FolioInputFixture/1"

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/state":
            payload = json.dumps({"events": self.server.state.snapshot()}, ensure_ascii=False).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(payload)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(payload)
            return
        document = page_for(path, self.server.fixture_port)
        if document is None:
            self.send_error(404)
            return
        payload = document.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(payload)

    def do_POST(self):
        if urlsplit(self.path).path != "/record":
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            event = json.loads(self.rfile.read(length))
            if not isinstance(event, dict):
                raise ValueError("event must be a JSON object")
        except (ValueError, json.JSONDecodeError) as error:
            self.send_error(400, str(error))
            return
        self.server.state.append(event)
        self.send_response(204)
        self.send_header("Cache-Control", "no-store")
        self.end_headers()

    def log_message(self, _format, *_args):
        pass


class IPv4LoopbackHTTPServer(ThreadingHTTPServer):
    address_family = socket.AF_INET
    daemon_threads = True
    allow_reuse_address = True


def serve(args):
    if args.events_file is not None:
        args.events_file.parent.mkdir(parents=True, exist_ok=True)
        args.events_file.write_text("", encoding="utf-8")
    state = FixtureState(args.events_file)
    primary = IPv4LoopbackHTTPServer(("127.0.0.1", args.port), Handler)
    port = primary.server_address[1]
    secondary = IPv4LoopbackHTTPServer(("127.0.0.2", port), Handler)
    for server in (primary, secondary):
        server.state = state
        server.fixture_port = port

    stopped = threading.Event()
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, lambda _signal, _frame: stopped.set())
    threads = [
        threading.Thread(target=server.serve_forever, name=f"fixture-{index}", daemon=True)
        for index, server in enumerate((primary, secondary))
    ]
    for thread in threads:
        thread.start()
    if args.url_file is not None:
        args.url_file.parent.mkdir(parents=True, exist_ok=True)
        args.url_file.write_text(f"http://127.0.0.1:{port}/\n", encoding="utf-8")
    print(f"READY http://127.0.0.1:{port}/", flush=True)
    try:
        stopped.wait()
    finally:
        for server in (primary, secondary):
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=0, help="loopback port; zero chooses a free port")
    parser.add_argument("--url-file", type=Path, help="write the root page URL after binding")
    parser.add_argument("--events-file", type=Path, help="append recorded browser events as JSONL")
    serve(parser.parse_args())


if __name__ == "__main__":
    main()
