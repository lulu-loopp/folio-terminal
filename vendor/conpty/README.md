# ConPTY sidecar

BetterTerminal pins the official Microsoft NuGet release asset as its active source archive:

- Package: `Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg`
- Package version: `1.25.260710002-preview`
- Positioning: **preview sidecar**, deliberately newer than the Windows inbox implementation
- Windows Terminal release: [`v1.25.1912.0`](https://github.com/microsoft/terminal/releases/tag/v1.25.1912.0), published 2026-07-16 as a pre-release
- Source asset: <https://github.com/microsoft/terminal/releases/download/v1.25.1912.0/Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg>
- Package SHA-256: `05fe9b571ea4fb198f5012405cb39a132cf23eee50feaa496524c149b2502692`
- Upstream repository: <https://github.com/microsoft/terminal>
- License: MIT, as declared by `Microsoft.Windows.Console.ConPTY.nuspec`; upstream license text is at <https://github.com/microsoft/terminal/blob/main/LICENSE>

The package contains a NuGet repository signature at `.signature.p7s`. The x64 `conpty.dll` and
`OpenConsole.exe` also carry valid Authenticode signatures whose signer subject is
`Microsoft Corporation` and whose chain is issued by `Microsoft Code Signing PCA 2024`. The
checked x64 SHA-256 values are:

| Package entry | SHA-256 |
| --- | --- |
| `runtimes/win-x64/native/conpty.dll` | `e2fe87e2258c4e46ffc5157f727218cc25f34a174902f72eb8a5b49edd9a6458` |
| `build/native/runtimes/x64/OpenConsole.exe` | `2525c351aa136d555e5df9a3c9d6ce9be43f785e37e3c993b8f23b3f0a53c7fa` |

`crates/bt-pty/build.rs` verifies the package and entry hashes, then extracts only those two x64
entries into Cargo's profile output and test directories. The profile copy sits beside
`bt-app.exe`; the `deps` copy lets the real-ConPTY test executable exercise the identical strict
loader. `OpenConsole.exe` is also mirrored below each output's `x64/` directory because that is the
architecture-host layout required by the package's native `.targets`. Extracted binaries are build
artifacts under `target/`; they must not be checked in. `bt-corpus` depends on `bt-pty`, so a clean
standalone build of `bt-record` also runs this extraction and records the same sidecar rather than
silently falling back to the inbox implementation.

The previous official `Microsoft.Windows.Console.ConPTY.1.24.260512001.nupkg` is retained beside
the active package solely as a reproducible A/B archive. Its package SHA-256 is
`3c66a99d38b5c2ac4c7552b7632cbbef23a1911aca5e20370109eb555a15d077`; neither `build.rs` nor the
extractor references it.

## Pin verdict and real-ConPTY oracle

The upstream bug is [`#18725`](https://github.com/microsoft/terminal/issues/18725). Its fix,
[`#19535`](https://github.com/microsoft/terminal/pull/19535), asks for a cursor-position report
after resize and was merged for packaging as a NuGet sidecar. The official
[Windows Terminal 1.25 release notes](https://github.com/microsoft/terminal/releases/tag/v1.25.622.0)
explicitly list that ConPTY cursor synchronization change. The later `v1.25.1912.0` servicing
release supplies the newer `1.25.260710002-preview` binary pinned here.

The same PowerShell 5 resize-storm + history-up/history-down oracle was run against both official
packages. It drives many local terminal resizes, commits one final ConPTY resize, answers ConPTY's
DSR through the real terminal adapter, then checks the prompt after CSI A and CSI B.

| Implementation | DSR after committed resize | CPR observed | CSI A / CSI B oracle |
| --- | ---: | ---: | --- |
| sidecar `1.24.260512001` | 0 | 0 | **FAIL**; no post-resize cursor synchronization, history navigation did not preserve a clean prompt row |
| sidecar `1.25.260710002-preview` | 1 | 1 | **PASS**; `BT_PROMPT> echo BTHT`, then `BT_PROMPT>` |
| Windows inbox ConPTY | 0 on the acceptance host | 0 | **known FAIL** at the synchronization gate; retained as the ignored `system_conpty_known_resize_cursor_desync_oracle` regression record |

Reproduce the accepted sidecar result with:

```powershell
cargo test -p bt-pty sidecar_resize_keeps_history_navigation_on_a_clean_prompt_line -- --nocapture
```

Reproduce the current inbox failure in a fresh process with:

```powershell
$env:BT_CONPTY_FORCE_SYSTEM = '1'
cargo test -p bt-pty system_conpty_known_resize_cursor_desync_oracle -- --ignored --nocapture
```

`bt-app` startup/resize trace lines and BTCRP002 record/replay metadata use the selected loader's
display value. It is explicit and parseable: `source=sidecar version=1.25.260710002-preview dll=...`
for this pin, or `source=system version=windows-inbox` for the OS implementation.

## portable-pty patch

`portable-pty/` is the MIT-licensed `portable-pty 0.9.0` source with a deliberately narrow Windows
loader patch. Upstream already prefers `conpty.dll`, but loads it by a bare name; Windows may then
find an unrelated copy through `PATH`, and the private loader does not report what it selected.
The patch loads only the absolute `conpty.dll` beside the current executable, requires the paired
`OpenConsole.exe`, calls the NuGet package's official prefixed `Conpty*` ABI, falls back to the
unprefixed system `kernel32` ConPTY, and exposes the selected source for trace/test evidence. PTY
I/O, resize, and process lifecycle ownership are otherwise unchanged. The packaged ABI receives
the documented default creation flags (`0`), matching Microsoft's `node-pty` sidecar integration;
portable-pty's private `0x2`/`0x4` flags remain confined to the inbox-system compatibility path.
After attaching the child, the package path also calls its required `ConptyReleasePseudoConsole`
API; the final `ConptyClosePseudoConsole` remains owned by the existing RAII handle.
`BT_CONPTY_FORCE_SYSTEM=1` is reserved for the ignored upstream-regression oracle.

The workspace uses an exact `portable-pty = "=0.9.0"` requirement plus `[patch.crates-io]`. This
keeps every existing consumer on one API-compatible implementation while avoiding a fork of PTY
I/O and lifecycle ownership; the exact requirement prevents a later registry 0.9.x from silently
out-resolving the local patch.

The cursor synchronization fix is tracked by
[`microsoft/terminal#18725`](https://github.com/microsoft/terminal/issues/18725) and landed in
[`microsoft/terminal#19535`](https://github.com/microsoft/terminal/pull/19535).
