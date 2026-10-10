#!/usr/bin/env python3
"""Run both patched clipboard suites without touching the user's desktop."""

import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from Xlib import display


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("linux_smoke", Path(__file__).with_name("linux-smoke.py"))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def test_suite(package, env):
    env = env.copy()
    env["CARGO_TARGET_DIR"] = str(ROOT / "target" / f"vendor-{package}")
    subprocess.run(
        ["cargo", "test", "--manifest-path", f"vendor/{package}/Cargo.toml", "--locked", "-j", "2",
         "--", "--test-threads=1"],
        cwd=ROOT, env=env, check=True,
    )


def main():
    xvfb = shutil.which("Xvfb")
    manager = shutil.which("xfce4-clipman")
    bus = shutil.which("dbus-run-session")
    if not all((xvfb, manager, bus)):
        raise RuntimeError("install Xvfb, xfce4-clipman and dbus-run-session before running this suite")

    with tempfile.TemporaryDirectory(prefix="folio-vendor-clipboard-") as directory:
        root = Path(directory)
        with smoke.private_display("x11", root, Path(xvfb)) as env:
            # The smoke owns HOME; Cargo still uses the runner's installed tools
            # and cache. The clipboard manager owns only this private display.
            env["CARGO_HOME"] = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))
            env["RUSTUP_HOME"] = os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
            log = root / "clipboard-manager.log"
            process = smoke._start_logged_process([bus, "--", manager], env, ROOT, log, "clipboard manager")
            try:
                connection = display.Display(env["DISPLAY"])
                try:
                    atom = connection.intern_atom("CLIPBOARD_MANAGER")
                    deadline = time.monotonic() + 15
                    while not connection.get_selection_owner(atom):
                        if process.poll() is not None or time.monotonic() >= deadline:
                            raise RuntimeError(f"clipboard manager did not acquire its selection:\n{smoke._log_text(log)}")
                        time.sleep(0.025)
                finally:
                    connection.close()
                print("READY private CLIPBOARD_MANAGER", flush=True)
                test_suite("arboard", env)
                # wl-clipboard-rs tests create their own protocol servers under
                # this private runtime directory; they do not use Xvfb.
                test_suite("wl-clipboard-rs", env)
            finally:
                smoke._stop_process(process, "clipboard manager")


if __name__ == "__main__":
    main()
