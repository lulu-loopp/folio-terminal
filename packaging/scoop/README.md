# `packaging/scoop`

`folio.json` is the source of `bucket/folio.json` in the
**`lulu-loopp/scoop-folio`** repository, which `scoop install folio` reads.
`scripts/release/update-manifests.ps1` renders it for a release — `version`, the
64-bit `url`, `hash` and `extract_dir`, out of the release's `SHA256SUMS.txt` and
the file's own `autoupdate` templates — and `-Apply` commits the result to the
bucket (`docs/RELEASING.md`, "Distribution manifests"). A change to the bucket
file is made here; an edit made in the bucket by hand is taken away by the next
release.

## The hooks (0.4.6, U-2)

- **`post_install`** writes `folio-install.json` into `$dir`, the version
  folder: `{"v":1,"manager":"scoop","uninstall_hook":true}`, the install channel
  marker Folio reads at start (`crates/bt-app/src/install_channel.rs`). scoop
  runs it at every install and every `scoop update`, so every version folder
  has its own.
- **`pre_uninstall`** runs `folio.exe --uninstall-cleanup` from the version
  folder when scoop's `$cmd` is `uninstall` — not on `scoop update`, which runs
  `pre_uninstall` too and must keep the right-click menu, the agent hooks and
  the PowerShell profile line. The door's exit 2 (a Folio is running, nothing
  changed) throws, and scoop stops with the app intact; exit 1 (a removal
  refused) and 0 go on. The door's output is piped, not assigned: `folio.exe` is
  a window-subsystem program, and PowerShell waits for one only when its output
  goes down a pipe.

Checked with no installer by `scripts/ci/check-manager-hooks.ps1`, and installed
by `scripts/release/check-scoop-hooks-in-vm.ps1` on a test VM.
