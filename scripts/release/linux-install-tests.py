#!/usr/bin/env python3
"""Run the Linux package scripts in private home and working directories."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
DESKTOP = "applications/io.github.lulu-loopp.folio.desktop"


class LinuxInstallTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="folio-install-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "home"
        self.cwd = self.root / "working"
        self.artifact = self.root / "artifact"
        self.home.mkdir()
        self.cwd.mkdir()
        files = {
            "bin/folio": '#!/bin/sh\nprintf "Folio 0.4.6\\n"\n',
            f"share/{DESKTOP}": "[Desktop Entry]\nType=Application\nName=Folio\nExec=/usr/bin/env -- folio\n",
            "share/icons/hicolor/512x512@2/apps/io.github.lulu-loopp.folio.png": "icon",
        }
        for notice in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md", "TRADEMARK.md"):
            files[f"share/doc/folio/{notice}"] = "notice"
        for name, content in files.items():
            path = self.artifact / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        (self.artifact / "bin/folio").chmod(0o755)
        self.environment = dict(os.environ, HOME=str(self.home))
        self.environment.pop("XDG_DATA_HOME", None)

    def script(self, name, *arguments):
        subprocess.run(
            ["sh", str(ROOT / "scripts/release" / name), *map(str, arguments)],
            env=self.environment,
            cwd=self.cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def install_then_uninstall(self, data_home, prefix=None):
        arguments = [] if prefix is None else ["--prefix", prefix]
        self.script("install-linux.sh", "--from", self.artifact, *arguments)
        self.assertTrue((data_home / DESKTOP).is_file())
        sentinel = data_home / "Folio" / "settings.json"
        sentinel.parent.mkdir(parents=True, exist_ok=True)
        sentinel.write_bytes(b"user settings")
        self.script("uninstall-linux.sh", *arguments)
        self.assertFalse((data_home / DESKTOP).exists())
        self.assertEqual(sentinel.read_bytes(), b"user settings")

    def test_relative_data_home_uses_default_and_preserves_cwd_files(self):
        self.environment["XDG_DATA_HOME"] = "relative-data"
        outside = self.cwd / "relative-data" / DESKTOP
        outside.parent.mkdir(parents=True)
        outside.write_bytes(b"unrelated desktop entry")
        self.install_then_uninstall(self.home / ".local/share")
        self.assertEqual(outside.read_bytes(), b"unrelated desktop entry")

    def test_unset_empty_and_absolute_data_home(self):
        for value in (None, "", str(self.root / "data home")):
            with self.subTest(value=value):
                if value is None:
                    self.environment.pop("XDG_DATA_HOME", None)
                else:
                    self.environment["XDG_DATA_HOME"] = value
                data_home = self.home / ".local/share" if not value else Path(value)
                self.install_then_uninstall(data_home)

    def test_prefix_overrides_data_home(self):
        self.environment["XDG_DATA_HOME"] = "relative-data"
        prefix = self.root / "local prefix"
        self.install_then_uninstall(prefix / "share", prefix)
        self.assertFalse((self.cwd / "relative-data").exists())


if __name__ == "__main__":
    unittest.main()
