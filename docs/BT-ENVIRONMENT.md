# `BT_*` — every environment variable Folio reads

Folio reads no environment variable of its own unless you set one. This file lists
every `BT_`-prefixed name that appears in the code, what setting it does, and —
where a variable makes the program write a file — exactly what can end up in that
file.

**Why this file exists.** Several of these switches write terminal content to a
path you name. None of them grants any privilege: to set an environment variable
for Folio you must already be able to run programs as this user, and a program
that can do that can read the same content directly. What they are is a way for
this build to put screen contents on disk, and a public build owes a list of them.

`docs/BT-ENVIRONMENT.md` is checked against the source by a test —
`bt_app::diagnostics::bt_environment_doc_tests` — which scans every non-binary,
non-integration-test `.rs` file under `crates/` and `vendor/` for `BT_` string
literals and fails if this file and that scan disagree in either direction.

## Two conventions that apply to all of them

**Set-but-empty is off.** `BT_PTY_DUMP=` is a shell saying "not this run", and
every switch below reads it that way, with three exceptions noted in their own
rows. Whitespace is not trimmed: a value of `" "` is a filename.

**A name containing `TRACE` keeps the console.** When Folio starts from a console
it normally lets that console go and sends `stdout`/`stderr` to
`%APPDATA%\Folio\diagnostics.log`. If **any** environment variable in the process
whose name starts with `BT_` also contains `TRACE`, the console is kept instead
and those streams go there. The rule is a shape, not a list, so it covers a switch
added later; `BT_PTY_DUMP` and `BT_HANG_SELFTEST` deliberately do not match it.

## 1. Read by a release build

| Variable | Value | What it does | What can end up in the file | Default |
| --- | --- | --- | --- | --- |
| `BT_PTY_DUMP` | file path, used verbatim | Records every byte the ConPTY reader receives, per pane. `File::create` — **the named file is truncated**. A `.chunks` sidecar beside it records arrival times. The first pane takes the named path; later panes take `<path>.2`, `<path>.3`. | **Everything on the screen and everything typed.** Shell output, prompts, the echo of what you type, the contents of any file printed to the terminal, anything a program prints including secrets. The `.chunks` header also records the process id and the wall-clock start. | off |
| `BT_IME_TRACE` | file path, used verbatim | Appends one line per IME event, written before any routing. | **Composed and committed IME text** — the literal characters an input method produces. | off |
| `BT_CHROME_DUMP` | file path, used verbatim | Appends one block per chrome rebuild and per overlay frame: rectangles, colours, sprite marks, and label text. | **Every visible label**: tab titles, pane-head captions, file names in the files column, path foots, tooltips, menu rows. | off |
| `BT_DECOR_TRACE` | file path, used verbatim | Appends one snapshot per call: the lifecycle state of each frozen or live formula decoration and why it failed. | **Up to 96 characters of the terminal line** the decoration was drawn from. | off |
| `BT_WEB_TRACE` | file path, used verbatim | Appends one line per web-preview decision. | **Full navigation URLs**, including query and fragment, and the file names of refused downloads. | off |
| `BT_MOUSE_TRACE` | file path, used verbatim | Appends one line per mouse-routing decision. | Hit tests and routing, and **the path of every file opened into the preview and the target of every activated link**. | off |
| `BT_SEMANTIC_TRACE` | switch | Writes one line per matched screen region to `stderr` (so: to the console, since the name contains `TRACE`). | **The matched screen text** — a path, a URL, a hyperlink label. | off |
| `BT_GIT_TRACE` | switch | Writes repository- and preview-watch messages to `stderr`. | **Repository and file paths being watched.** | off |
| `BT_ATTENTION_TRACE` | file path, used verbatim | Appends one line per decision the attention queue makes, plus one line when a hooks installer resolves its target to a different path than it was asked for. | Tab indices, request and ticket identifiers, claim states, and the two paths of a resolved install target. No screen text. | off |
| `BT_PREVIEW_TRACE` | file path, used verbatim | Appends one line per preview station. | Seat, scale, rectangle and byte counts. No screen text. | off |
| `BT_FOCUS_THUMB_DUMP` | file path, used verbatim | Appends one counter line per frame that spent the thumbnail budget. | Integer counters only. | off |
| `BT_PERF_TRACE` | switch | Per-frame and per-task timings and counters to `stderr`; also turns on shaping-cache counting. | Counters. One line carries the error text of a formula that failed to render. | off |
| `BT_STARTUP_TRACE` | switch | Startup phase timings, DPI snapshots, surface-size clamps and the diagnostics channel, to `stderr`. | Timings and geometry. | off |
| `BT_RESIZE_TRACE` | switch | Surface-size clamp lines during resize, to `stderr`. | Geometry. | off |
| `BT_LAYOUT_EVENTS` | switch | Seat geometry changes to `stderr`. Note the name does not contain `TRACE`, so with nothing else set these lines go to `diagnostics.log`. | Geometry. | off |
| `BT_PROBE_INPUT` | file path, used verbatim | **Reads** the named file and feeds its bytes to the terminal instead of starting a shell. Writes nothing. | — | off; a shell is started |
| `BT_WEB_DEV` | URL | Opens a preview seat at startup and navigates it to this URL. | The page is loaded, so its cache and cookies land in the WebView2 profile under `%LOCALAPPDATA%\Folio\WebView2` like any other previewed page. | off; no page is opened |
| `BT_POWERSHELL_PROFILE` | file path, used verbatim | Redirects the shell-integration installer: the `$PROFILE` it reads and writes becomes this file instead of the real one. | The file you name is created and edited by the installer. | the real `$PROFILE`, asked of a running PowerShell |
| `BT_PSREADLINE_DOCUMENTS` | directory path, used as a base | Redirects the bundled PSReadLine installer's Documents root. The module goes to `<dir>\WindowsPowerShell\Modules\PSReadLine\<version>\`. | Nine files are written under, and deleted from, the directory you name. | the real Documents folder |
| `BT_PSREADLINE_PROBE` | `<version>[,<policy>]` | Makes the machine read as if it had that PSReadLine version and execution policy, so the upgrade invitation can be photographed. Redirects no write. | — | the real probe, which runs `powershell -NoProfile`. **Set-but-empty is not off here**: `=` engages the override at version `0.0.0`. |
| `BT_SHELL` | program path or bare name, used verbatim | Overrides the default shell program, and is what the `PowerShell` profile row resolves to. Never checked for existence; a bare name is resolved by `CreateProcess` against `PATH`, and a spawn failure falls back to `powershell.exe`. | — | `pwsh.exe` if found, else `powershell.exe` |
| `BT_BG` | `#RRGGBB` | Overrides the terminal background colour and locks the theme for the run. An unreadable value is reported on the diagnostics channel and ignored. | — | `#1B1B1B`, unlocked. **Set-but-empty is not silently off**: it is reported as invalid and ignored. |
| `BT_CONPTY_FORCE_SYSTEM` | presence | Skips the packaged `conpty.dll`/`OpenConsole.exe` and uses the ConPTY that ships with Windows. Read in the vendored `portable-pty`. | — | the packaged pair is preferred. **Presence-only**: `BT_CONPTY_FORCE_SYSTEM=` counts as on. |
| `BT_SHELL_INTEGRATION` | `1` | Not read by `folio.exe` — **written** into the environment of a bash launched with `--init-file`, and read by the shipped `folio.bash` to decide whether it must source the login chain itself. | — | not set |

`BT_HANG_SELFTEST` (an integer number of seconds; holds the window thread that
long, once, to prove the hang watchdog writes a report) is compiled out of release
builds — in a release build the code that reads it is a no-op.

`BT_PANIC_SELFTEST` (the same shape: an integer number of seconds, once) faults
the window thread on purpose, to prove that a crash leaves by the exit a shut
leaves by rather than through the loader's teardown — `docs/DESIGN.md` §7.43 ④.
It is compiled out of release builds for the same reason and one more: a release
build is required to have **no** controlled way to panic, which
`scripts/release/cleanvm/in-guest.ps1` checks by requiring
`folio.exe --panic-selftest` to be refused as an unknown argument.

## 2. Read only when the tests are compiled

Never present in a release binary. `BT_PSREADLINE_MODULE_PATH`, `BT_BURST_EMIT`,
`BT_BURST_ONLY`, `BT_DEFER_EMIT`.

## 3. `BT_` names in the source that are not environment variables

Listed so that the check described at the top of this file can tell them apart
from switches, and so nobody looks for a variable that does not exist.

**Labels in a diagnostic line**, written into a message and never read:
`BT_STARTUP`. (Others of this kind — the persistence, web, DPI, theme, focus and
resize labels — carry a space inside the same string literal and so are not names
at all.)

**Trace-file header tokens**, the version suffix of a file's first line:
`BT_MOUSE_TRACE_V`, `BT_WEB_TRACE_V`.

**Half of a name, spelled in two pieces** so that a test asserting the name's
absence is not its own counter-example: `BT_APP_`, `BT_TRANSFER`.

**Fixture strings and markers** written by tests into a terminal or a probe and
matched back out: `BT_APP_INPUT_OK`, `BT_FILL_080_XXXXXXXXXXXXXXXXXXXXXXXX`,
`BT_HISTORY_SEEDED`, `BT_HISTORY_TRANSPARENCY_SEEDED`, `BT_INVOKE_HISTORY`,
`BT_KEY_CASF12`, `BT_KEY_F24`, `BT_OLD_RESIDUE_MUST_DISAPPEAR`,
`BT_PANIC_SURVIVED`, `BT_PSREADLINE_NOOP`, `BT_PSREADLINE_REANCHOR_FALLBACK`,
`BT_PTY_OK`, `BT_SEAT_TYPED_INPUT_LONG_ENOUGH_TO_WRAP`.

## 4. Development binaries

`bt-repaint-oracle`, `bt-zoom-perf`, `bt-replay`, `bt-record` and the ConPTY
probes read a further set of `BT_PROBE_*`, `BT_ZOOM_PERF_*` and `BT_CONPTY_*`
names. Those binaries are not part of a release archive and are not covered by the
check above; their switches are documented where they are read.

## Files Folio writes without being asked

For completeness beside the list above, and because none of these needs a variable
set:

- `%APPDATA%\Folio\diagnostics.log` — `stdout` and `stderr` for a run that did not
  keep its console. Checked **once, at startup**: if it is already 4 MiB or larger
  it is moved to `diagnostics.prev.log`, replacing whatever generation was there.
  There is no throttling during a run, so a single run's log can grow past 4 MiB;
  what is bounded is the history kept between runs, which is two files.
- `%APPDATA%\Folio\hang-reports\hang-<timestamp>.txt` — written when the window
  thread stops answering. The report carries the instruction and stack pointers,
  how many bytes of stack were scanned, how many modules were mapped, and each
  candidate return address **as a module name and an offset**. The stack bytes
  themselves are read but not written to the file.
- `%TEMP%\bt-app-panic.log` — appended by the panic hook.
