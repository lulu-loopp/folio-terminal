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
| `BT_PTY_INPUT_DUMP` | file path; unset or empty is off | Records queued input through `PtyDump` to `<path>.in`, then `<path>.in.2`, etc. Raw bytes plus one `.chunks` line per write: sequence, elapsed microseconds, byte count, pane ordinal, quoted reason, hex bytes. Shares the receive dump clock and pane identity. A queued write does not prove the child consumed it. | **records your keystrokes, including anything typed at a password prompt; for a diagnosis you run yourself, never to be shared unread** | off, including release |
| `BT_IME_TRACE` | file path, used verbatim | Appends inbound IME events, text-free startup order and native focus snapshots, outbound calls, ownership/routing rulings and changes in terminal pre-edit drawing. See line formats below. | Kinds, byte lengths, cursor ranges, rectangles, static reasons and results only. **No composed or committed text.** Older builds wrote literal text. | off |
| `BT_CHROME_DUMP` | file path, used verbatim | Appends one block per chrome rebuild and per overlay frame: rectangles, colours, sprite marks, and label text. | **Every visible label**: tab titles, pane-head captions, file names in the files column, path foots, tooltips, menu rows. | off |
| `BT_DECOR_TRACE` | file path, used verbatim | Appends one snapshot per call: the lifecycle state of each frozen or live formula decoration and why it failed. | **Up to 96 characters of the terminal line** the decoration was drawn from. | off |
| `BT_WEB_TRACE` | file path, used verbatim | Appends one line per web-preview decision. | **Full navigation URLs**, including query and fragment, and the file names of refused downloads. | off |
| `BT_MOUSE_TRACE` | file path, used verbatim | Appends one line per mouse-routing decision — a button's and, since 2026-09-13, **a wheel notch's**: the report as the driver sent it and as the merged burst spent it, the pointer both live and remembered, the window's scale and its two sizes, the rail's own yes or no with the card column as it is *painted* beside the column the aim *walks*, what the aim did with the fraction it was carrying, and one word for which surface took the notch home (`rail-aim`, `rail-scroll`, `tab-strip`, `overlay`, `page`, `pane`, `terminal-pane`, `focused-leaf-fallback`, `pty`, `nobody`). Since 2026-09-17 a press on the command marks rail writes `rail-jump` too: the seat, which tick was hit and which mark it names, the anchor's own place in the document and the lift the jump asks for, and the three numbers that say where the jump can land — the window top it works out, the furthest the view may travel, and how much of that the blank rows under the prompt are already spending on a formula standing taller than the pane. A `window_top` equal to `extent` is a jump that has reached the end of the document rather than one that missed. | Hit tests and routing, and **the path of every file opened into the preview and the target of every activated link**. Seat, tab and window identifiers, rectangles and scroll offsets. No screen text on the wheel's lines. | off |
| `BT_CARD_TRACE` | file path, used verbatim | Appends one line per station on a focus card's road: `card walk` for every call of the per-frame pass (the pane's grid, the card's rows, the stored offset before and after — one number, since the pass reports and never writes — how far the walk reached, and the offset the drawing will actually use, which is allowed to be a smaller number), `card aim` for every `Alt`+wheel notch (the detents, the stored offset, what the notch's own clamp made of it, what was asked for and what was given), `card pane resized` for every new grid a pane behind a card is given, and `card scale` for the two halves of a display change. The clock is the process's, so this file and a `BT_MOUSE_TRACE` of the same run merge on their first column. **A traced run is slower than an untraced one**: `card walk` takes a bounded transcript walk of its own. | **Two lines of the card's own transcript per `card walk` line** — its top and bottom row, each cut to 40 characters with control characters escaped, which is terminal text like any other. Otherwise seat, tab and window identifiers, grids and row counts. | off |
| `BT_SEMANTIC_TRACE` | switch | Writes one line per matched screen region to `stderr` (so: to the console, since the name contains `TRACE`). | **The matched screen text** — a path, a URL, a hyperlink label. | off |
| `BT_GIT_TRACE` | switch | Writes repository- and preview-watch messages to `stderr`. | **Repository and file paths being watched.** | off |
| `BT_ATTENTION_TRACE` | file path, used verbatim | Appends one line per decision the attention queue makes, plus one line when a hooks installer resolves its target to a different path than it was asked for. | Tab indices, request and ticket identifiers, claim states, and the two paths of a resolved install target. No screen text. | off |
| `BT_PREVIEW_TRACE` | file path, used verbatim | Appends one line per preview station, including one per page that holds a formula and one per formula the engine answers. | Seat, scale, rectangle, byte counts, and how many formulas a page has, was drawn and asked for. No screen text — a formula's own source is counted in characters, never written. | off |
| `BT_HOTKEY_TRACE` | file path, used verbatim | Appends one line per station on the road a summon press travels, on both platforms: `reconcile wanted=<chord>` when the claim moves to a new chord (not once per frame), `install handler -> Ok/Err(<OSStatus>)` and `register keycode=<n> modifiers=<hex> -> Ok/Err(<OSStatus>)` where the claim is made, `handler fired id=<n> signature=<four characters> live=yes/no` for **every** hot key event this process is offered, and `summons_wake called` then `QuakeSummoned handled` as the press crosses into the event loop. A file that stops part way names the station the press did not reach. | The summon chord as `keybindings.json` spells it, key codes, modifier masks, `OSStatus` values and four-character signatures. No screen text and nothing else the reader typed. | off |
| `BT_GLYPH_CENSUS` | file path, used verbatim | Appends one line each time a frame's demand on the shared glyph atlas changes: glyph instances, distinct rasters, distinct faces-and-sizes, rasters two surfaces share, the area they cover and the device's texture limit. **Slows every frame**: a raster's size is learned by rasterizing it a second time. | Counters and font identifiers. No screen text. | off |
| `BT_FOCUS_THUMB_DUMP` | file path, used verbatim | Appends one counter line per frame that spent the thumbnail budget. | Integer counters only. | off |
| `BT_PERF_TRACE` | switch | Per-frame and per-task timings and counters to `stderr`; includes one `BT_PERF_TRACE attempt` record per redraw/retained attempt, including empty and failed outcomes (identity, actual configuration, separate native/attention readings and phase timings; [format and freshness diagnostics](PRESENT-DIAGNOSTICS.md)); also turns on shaping-cache counting. Its `projection` line carries, since 2026-09-17, the three scroll numbers the frame it is written after decided: `scroll_offset_subpixels` (how far into the history the view stands), `scroll_extent_subpixels` (how far it could stand) and `bottom_relief_subpixels` (how much of that the blank rows under the prompt are spending). Among them, `live_math_result_dropped row=… candidate_row=… reason=…` for a rendered live formula the session refused to install — the one outcome that leaves a formula at source with nothing else to show for it. The reason is `screen`, `grid-generation`, `detection-revision`, `layout`, `source-changed`, `no-longer-detected` or `unproven`; the first four say the grid moved on while the raster was being made, and the last three say the rows no longer prove what was scanned. | Counters. One line carries the error text of a formula that failed to render. | off |
| `BT_STARTUP_TRACE` | switch | Startup phase timings, DPI snapshots, surface-size clamps and the diagnostics channel, to `stderr`. | Timings and geometry. | off |
| `BT_RESIZE_TRACE` | switch | Surface-size clamp lines during resize, to `stderr`. | Geometry. | off |
| `BT_LAYOUT_EVENTS` | switch | Seat geometry changes to `stderr`. Note the name does not contain `TRACE`, so with nothing else set these lines go to `diagnostics.log`. | Geometry. | off |
| `BT_PROBE_INPUT` | file path, used verbatim | **Reads** the named file and feeds its bytes to the terminal instead of starting a shell. Writes nothing. | — | off; a shell is started |
| `BT_WEB_DEV` | URL | Opens a preview seat at startup and navigates it to this URL. | The page is loaded, so its cache and cookies land in the WebView2 profile under `%LOCALAPPDATA%\Folio\WebView2` like any other previewed page. | off; no page is opened |
| `BT_POWERSHELL_PROFILE` | file path, used verbatim | Redirects the shell-integration installer: the `$PROFILE` it reads and writes becomes this file instead of the real one. | The file you name is created and edited by the installer. | the real `$PROFILE`, asked of a running PowerShell |
| `BT_PSREADLINE_DOCUMENTS` | directory path, used as a base | Redirects the bundled PSReadLine installer's Documents root. The module goes to `<dir>\WindowsPowerShell\Modules\PSReadLine\<version>\`. | Nine files are written under, and deleted from, the directory you name. | the real Documents folder |
| `BT_UNINSTALL_ROOT` | absolute sandbox directory; empty/relative refuses | **Read only by a debug or test build; a shipped `folio.exe` ignores it entirely** (`uninstall.rs: SANDBOX_DOOR`). Isolates `--uninstall-cleanup` and `--purge`: data, profiles, modules, agents, temp files and instance claims resolve below this directory. Registry/MSIX/toast operations are replaced with absent sandbox readings. | Deletes only resolved sandbox data paths when `--purge` is present; never the application folder. Recorded directories outside the sandbox refuse; a recorded `$PROFILE` is held to its kind (absolute, no `..`, a `.ps1`) and is not contained by the sandbox, so a fixture must never record a real one. | real account roots and system registrations |
| `BT_PSREADLINE_PROBE` | `<version>[,<policy>]` | Makes the machine read as if it had that PSReadLine version and execution policy, so the upgrade invitation can be photographed. Redirects no write. | — | the real probe, which runs `powershell -NoProfile`. **Set-but-empty is not off here**: `=` engages the override at version `0.0.0`. |
| `BT_FIRST_RUN_CARD` | switch | Raises the first-run card on a machine that has already answered it, so it can be photographed in a second language or looked at again. Overrides the appearance gate and nothing else: which rows are offered, what `Done` does and what is written down are exactly what they would be on a real first run. | — | off; the card appears only on a machine with no `settings.json` that has never shown it |
| `BT_SHELL` | program path or bare name, used verbatim | Overrides the default shell program, and is what the `PowerShell` profile row resolves to. Never checked for existence; a bare name is resolved by `CreateProcess` against `PATH`, and a spawn failure falls back to `powershell.exe`. | — | `pwsh.exe` if found, else `powershell.exe` |
| `BT_BG` | `#RRGGBB` | Overrides the terminal background colour and locks the theme for the run. An unreadable value is reported on the diagnostics channel and ignored. | — | `#1B1B1B`, unlocked. **Set-but-empty is not silently off**: it is reported as invalid and ignored. |
| `BT_GPU_PREFERENCE` | `low` \| `high`, either case | Which GPU Folio asks the driver for. On a laptop with two, `low` means the integrated adapter and `high` the discrete one. It is a request and not a command: the adapter has to be able to present to Folio's own window, and where only one can, that is the one Folio gets whatever it asked for. The `GPU adapter` line in `diagnostics.log` carries both halves — `asked=…` beside the adapter that answered — so a recording says which of the two runs it is from. A value that is neither word is named on that same line and the default stands. | — | `high`, which is what every build before this one asked for |
| `BT_CONPTY_FORCE_SYSTEM` | presence | Skips the packaged `conpty.dll`/`OpenConsole.exe` and uses the ConPTY that ships with Windows. Read in the vendored `portable-pty`. | — | the packaged pair is preferred. **Presence-only**: `BT_CONPTY_FORCE_SYSTEM=` counts as on. |
| `BT_SHELL_INTEGRATION` | `login` \| `interactive` | Not read by `folio.exe` — **written** into the environment of a bash launched with `--init-file`, and read by the shipped `folio.bash` to decide **which** startup chain it must source in place of the one the flag displaced. `login` is `/etc/profile` then the first of `~/.bash_profile`, `~/.bash_login`, `~/.profile`; `interactive` is `~/.bashrc` alone. Which of the two is a fact about the profile's own arguments. | — | not set |
| `BT_USER_ZDOTDIR` | a directory | Not read by `folio.exe` — **written** into the environment of a zsh whose `ZDOTDIR` this terminal has taken, carrying the one the session already had so that the shipped `folio.zsh` can source the reader's own startup files out of it. Absent when the session had none, which says the files are in `$HOME`. | — | not set |

### `BT_IME_TRACE` line formats

All lines share the existing `Instant` timestamp and file-writer queue. Shared
window observations include `window=<numeric id>`. Native Windows observations
occur between the shared call and return records, including synchronous IME
callbacks. These are trace-file records only, not diagnostics-log records.
Unset or empty is off; the producer checks a cached gate before formatting,
examining diagnostic state or reading the clock. No native queries are added.

| Kind | Fields and meaning |
| --- | --- |
| `IME_IN` | `kind=Enabled/Disabled/Preedit/Commit`, `bytes` (UTF-8 length); Preedit also has `cursor=Option<(byte,byte)>`. Written before routing. |
| `IME_OUT_ALLOWED` | `value`, `reason=window_construction`, immediately before each existing `set_ime_allowed` call. |
| `IME_OUT_CANCEL` | `owner`, static caller `reason=take_keyboard_into/settle_composition_owner`, `result=None` before the call and `Some(true/false)` after it. A false result does not change the existing local cleanup. |
| `IME_OUT_NOTIFY` | Windows only: `notification=NI_COMPOSITIONSTR`, `index=CPS_CANCEL`, `value=0`, caller `reason`, `phase=call/return`, `result=None/Some(bool)`. No notify line means the native notification was not reached (for example, no input context). |
| `IME_OUT_AREA` | Client-pixel `x,y,width,height`; `action=sent/flushed/reoffered` immediately before `set_ime_cursor_area`; `throttled/reoffer_throttled` means deferred and `unchanged` means suppressed by the existing throttle. |
| `IME_OUT_CARET` | Shared `action=update/destroy`, static `reason`, `position=Some((x,y))/None`; update follows a cursor-area call, destroy names `ime_disabled/cancel_composition/window_teardown/window_blur`. Portable no-op calls are still calls and are recorded. |
| `IME_OUT_NATIVE_CARET` | Windows only: `action=update/create/position/destroy`, `x,y,width=1,height=1`, prior `active`, `result=call/layout_not_chinese/inactive/ok/error`. Destroy has no position; its `x,y` are zero placeholders. Includes internal layout-change and Drop destruction. Error text is never included. |
| `IME_OWNER` | Previous `old=Option<owner>` and `new`, `previous_cause`, `cause`. Emitted at the authoritative keyboard-owner reading when its kind changes (and once initially). Causes identify rename, git prompt, palette, quit dialog, dirty gate, first-run card, PSReadLine invitation, settings, popup, files tree, graph search, preview, search or shell. |
| `IME_RULING` | Text-free inbound metadata, `origin=Option<owner>`, `destination`, `same_origin`, `deliver`, `next`, and `reason=origin_mismatch/owner_swallows/field_route/shell_route`. Recorded before applying the existing ruling; `same_origin` distinguishes different instances of the same kind without exposing names or paths. `deliver=true` passes the composition-origin barrier; Modal/FilesTree can still swallow it. |
| `IME_DRAW` | `surface=terminal`, `drawn`, `reason`, `bytes`, `cursor_visible`, grid `row,column`, `alt`, and client-pixel `x,y,width,height`. One line when the terminal pre-edit outcome changes; clearing/committing/ending resets the latch for the next composition. Reasons: `drawn`, `owner_mismatch`, `cursor_invisible`, `zero_size_rectangle`, `no_visible_cells`. |

`IME_DRAW` observes actual writes by the terminal pre-edit compositor, before
unchanged-frame suppression; it does not claim swapchain presentation or field
widget drawing. Field rerouting is identified by `IME_RULING`. Alternate-screen
mode is recorded as context and does **not** prevent drawing. The rectangle is
the starting cursor cell, before advancing through the pre-edit; the actual native
candidate anchor is recorded by `IME_OUT_AREA`. The current renderer clamps cell
rectangle dimensions to at least one pixel. `no_visible_cells` includes zero-width-only input and clusters that
wrap beyond the grid. Length or rectangle changes alone do not repeat an
unchanged draw answer. Ownership and draw latches are per window and diagnostic
only. macOS gets the shared call-site records; no new macOS native calls or
backend-specific probes are introduced.

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

`BT_SURFACE_SELFTEST` (the same shape again: an integer number of seconds,
once) throws away this process's GPU device and builds every window's swapchain
again, through the same path a device the driver took away goes through. It is
here because the event it stands in for cannot be asked for: the device-loss
latch is set by a driver reset, and what a rebuild has to be watched doing —
on macOS, land its new `CAMetalLayer` on a view the old one was cleared off
(`docs/DESIGN.md` §13.14) — is invisible in every other run of the program.
Compiled out of release builds.

## 2. Read only when the tests are compiled

Never present in a release binary. `BT_PSREADLINE_MODULE_PATH`, `BT_BURST_EMIT`,
`BT_BURST_ONLY`, `BT_DEFER_EMIT`, `BT_MATH_ROBUSTNESS_TEST_CHILD` (the marker a
test process sets on the child it spawns to prove the decoration worker survives a
hostile formula), and `BT_INSTALL_TXN_CHILD_ADMISSION`, `BT_INSTALL_TXN_CHILD_READY`
and `BT_INSTALL_TXN_CHILD_GO` — the three paths `bt-platform`'s
`install_txn::tests::admission_shared_blocks_exclusive_across_processes` sets on the
second copy of its own test binary: the admission file the child holds shared, the
file the child creates once it holds it, and the file whose appearance tells it to
let go. Only that test's child half reads them; no product code does. And
`BT_UPDATE_TRIAL_TEST_CHILD` and `BT_UPDATE_TRIAL_TEST_ROOT` — the test name and
the private folder `bt-app`'s `update_trial` tests set on the copy of their own
test binary that runs a start's writers in a process of its own (the trial is a
fact once per process), with `APPDATA`, `LOCALAPPDATA`, `HOME`, `XDG_DATA_HOME`
and `BT_POWERSHELL_PROFILE` pointed inside that folder. Only the child half of
those two tests reads them.

| Variable | Value | What it does | What can end up in the file | Default |
| --- | --- | --- | --- | --- |
| `BT_PASTE_CRT_CONSUMER` | absolute path to the separately compiled `paste_paths_crt.exe` fixture | Set by the developer or acceptance coordinator when explicitly running the ignored Windows direct-CRT test. Names the consumer that receives the encoder's literal through the quiet process door; the test checks its captured argument count and UTF-16 units. | No file is written by this switch; consumer output is captured through pipes. | not set; the test is ignored by default and fails if explicitly run without this value |

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

The macOS sheet probe (`crates/bt-platform/tests/macos_sheet.rs`) is outside the
check for the same reason — the walk excludes `tests/` — and reads two of its
own. `BT_MAC_GUI` set to `1` is consent: without it that target prints one line and exits
rather than opening real windows on somebody's desk. The same consent gates the two
`bt-platform` handoff cases that put a Finder window or an editor on the desk
(`crates/bt-platform/src/handoff.rs`): without it they print one line and pass.
`BT_MAC_GUI_SHOT=<dir>`
names a directory, and when it is set the probe writes each sheet's window number
into it and then holds that sheet up for two and a half seconds, so that the
session which started the probe can photograph it — the grant that lets a
process photograph a window belongs to that session and not to a throwaway
bundle. Neither name is read by `folio` itself.

The macOS glyph measurement (`crates/bt-app/tests/macos_glyph_surface.rs`) reads
the same two names and means the same things by them, with one difference worth
stating: there `BT_MAC_GUI_SHOT=<dir>` is **required** rather than optional, because
the photograph is not an illustration. A swapchain cannot be mapped, so the only
way to read the pixels a Metal surface actually presented is a picture of the
window, and a run with nowhere to put one has nothing to compare the offscreen
texture against. The target publishes its window number into
`<BT_MAC_GUI_SHOT>/glyph.window` and reads the picture from
`<BT_MAC_GUI_SHOT>/glyph.png`, taking it itself when this process holds the
Screen Recording grant and waiting for the session that started it when it does
not.

The macOS composition proof (`crates/bt-platform/tests/macos_compose.rs`) reads
the same two names and nothing else. `BT_MAC_GUI=1` is the same consent — that
target opens a window at backing scale 2 and reads its own pixels back — and
`BT_MAC_GUI_SHOT=<dir>` names a directory the captures are written into, one
`.ppm` per step, for a reader who would rather look than read numbers. It needs
no photographer outside the process: a window this process owns can be read back
with `CGWindowListCreateImage` without a Screen Recording grant, which the sheet
probe's own note explains is not true of a sheet somebody else has to photograph.

## Files Folio writes without being asked

For completeness beside the list above, and because none of these needs a variable
set. Where a path below is written `%APPDATA%\Folio\`, a macOS build reads `HOME`
instead and keeps the same files under `~/Library/Application Support/Folio/`
(`docs/DESIGN.md` §13.15); a process with neither variable set falls back to its
temp directory.

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

### IME first-focus self-report

`BT_IME_TRACE` no longer carries typed text: inbound events are written as kinds
and byte lengths. `Folio: IME observation` and `Folio: keys are arriving as plain text`
lines contain no typed text. Focus snapshots and the single snapshot about one
second after the process's first focus also go to `diagnostics.log` without an
environment switch. The delayed sample uses the event loop's deadline and is
retired after one observation; a busy event loop can deliver it late.

Fields:

- `reason`: `focus-gain`, `focus-loss`, `first-focus+1s`, `plain-text`, or a
  trace-only startup station. `at_ms` is monotonic time since the first window's
  instrumentation began, not wall time or time since the trace file opened.
- `created`, `shown`, `first_focus`, `first_enabled`, `first_key`, and `focus`:
  per-window order number followed by `@<milliseconds>ms`. `unknown` means the
  event has not been observed. Trace-only `IME startup` lines use the equivalent
  debug spelling `Some(Stamp { order, ms })` / `None`. `shown` records completion of the first show
  request; native activation can occur synchronously inside that request.
- `allowed`: the last `set_ime_allowed` argument and its observation stamp.
  This records what Folio requested, not proof that the native context accepted
  it. `enabled_since_focus` and `ime_since_focus` describe delivered winit
  events in the current focus epoch, before surface routing.
- `focused`, `terminal`, `web_host`: the application focus event, keyboard
  owner's terminal status, and presence of a WebView host in this window.
- `latin_keys`: consecutive qualifying key presses, saturated at four. Three
  triggers a fresh native reading. Synthetic events and releases do not count;
  controls, non-Latin presses, shortcuts, and non-terminal surfaces reset it.
- `native`: `hwnd`, `focused_hwnd`, `focus_matches` (`GetFocus` vs the supplied
  winit HWND); `context` (`ImmGetContext` non-null); `open`
  (`ImmGetOpenStatus`); raw `conversion` (`ImmGetConversionStatus`, bit 0 is
  `IME_CMODE_NATIVE`); `hkl_low` / `hkl_high` (`GetKeyboardLayout(0)` words);
  `imm_is_ime` (`ImmIsIME`); `tsf_profile_type` (`GetActiveProfile`, 1 text
  service, 2 keyboard layout); `tsf_error` (HRESULT if the read failed).
  `None` means unknown/unavailable, never false. Native handles are numeric;
  no profile names, window titles, key values, or composition strings are read.

Two further always-on lines name a broken composition bracket —
`shape=restarted-inside-a-live-composition` (a second `Enabled` while a pre-edit
is live) and `shape=ended-without-a-start` (a `Disabled` nobody opened) — with
the live pre-edit's byte length and no text, at most 32 per window. On Windows
the first is followed by winit dropping every composition message until the
next start, so it marks the moment typed text began to be lost.

The always-on symptom line requires three consecutive presses carrying ASCII
Latin letters (optionally printable ASCII punctuation/spaces) on a focused
terminal, no IME event since focus, an active input method (TSF text service or
`ImmIsIME`), a context, open status, and native conversion mode. It is written
at most once per focus. Any delivered IME event, including `Disabled`, suppresses
it for that focus. Alphanumeric/closed/unknown mode suppresses the line. Layout
language alone never proves an IME. Native reads happen only at focus changes,
the one delayed sample, and the third-key candidate; not on ordinary keys.
Within one focus the third-key reading is taken at most once per ten seconds:
a reading that does not confirm would otherwise be repeated at every word.

`mode_source=imm-compat` is deliberate: IMM open/conversion flags are the
Windows compatibility view, not authoritative proof of a TSF-only service's
private English/Chinese mode. A service can leave them stale. Unknown flags
suppress reporting; stale positive flags can still cause a false positive, and
stale negative flags can hide the symptom. The line states observable API facts,
not a diagnosis or proof of the user's intended language. The local TSF profile
manager is queried without initializing COM, activating a profile, creating a
text store, or changing focus. A missing apartment/profile remains unknown.
macOS/native facts not already available are unknown; no AppKit reader is added.

No report can detect keystrokes that never reach Folio. The focus and delayed
snapshots are intended to leave evidence for that version of the symptom too.

### File-read self-report

Folio's process-wide content-read ledger reports a minute exceeding **50,000,000
bytes** when no user input arrived, or when any one lane exceeds that budget
even with input. This is observation only: it does not throttle reads or change
caches. `diagnostics.log` gets one `Folio: file reads ...` line at the start,
at most one repeat per ten minutes, and one `ended after ... min, ... GB` line
on the first below-budget minute. MB and GB are decimal units.

The existing hang-watch worker checks the ledger using its session clock. It
adds no worker, timer, window deadline or idle window wake. An unchanged,
untraced ledger with no open report needs no collection, formatting or output;
an open report still gets its one closing line. `BT_PERF_TRACE`
additionally emits `BT_PERF_TRACE file_reads minute=...` with exact integer
byte and read-pass totals for every lane each minute, including zero minutes,
through the existing stderr trace sink.

Lanes are `inline_image`, `peek`, `animation`, `preview`, `pdf`, `git_pipe`,
`settings`, `fonts`, `attention`, `install` and `other`. Bytes count content delivered by
the instrumented readers; directory enumeration and metadata are excluded.
`git_pipe` measures the child's pipe output consumed here, not the child's
disk reads. Reads mean logical passes, including bounded heads/tails and
animation restarts, not OS read calls or a claim that every pass reached EOF.
Streaming bytes accrue as chunks arrive; a pass spanning a boundary can have
bytes in a later minute with no new read pass. Whole-file convenience reads
are charged when they return successfully. Partial-error bytes from adapted
`Read` streams are counted too.

Native video loaders and font libraries own some reads internally. Their
invocations appear separately as `opaque_loads` / `opaque loads (bytes unknown)`;
their byte counts are **not** guessed from file sizes. Consequently these
totals are not a replacement for the process's OS I/O counters. A watchdog
delayed by other work collects on its next existing wake; sampling is not a
real-time timer. `window_ms` states the actual interval; the budget comparison
and total MB/min rate are normalized to that interval, while lane bytes and
episode GB remain actual totals. No missing minute is invented as a zero.

The largest lane names up to three repeated **basenames only**, never parent
directories or file content. Each minute uses a fixed 64-slot table per lane;
names are limited to 96 bytes and control characters are replaced. A full
table leaves byte/read totals intact and marks `top tracked` and the number
of reads with untracked paths. It does not claim an exhaustive ranking in that
case. The table is cleared at collection. No path string is allocated per read.

### Picture freshness (always on)

A shown, non-minimized window with outstanding picture debt writes a
`Folio: window <id> has shown no new picture ...` line to `diagnostics.log`
after one second, at each subsequent decade, and once when a picture lands.
This is independent of `BT_PERF_TRACE` and hang detection. It rides existing
turns and attempt completion, adds no idle wake, and prints no user text.
See [Presentation diagnostics](PRESENT-DIAGNOSTICS.md) for field definitions,
clock boundaries, and how to join an attempt to a stalled station.

### Uninstall sandbox layout

`BT_UNINSTALL_ROOT` replaces all cleanup discovery, including the real profile
and agent overrides. Windows data is `roaming/Folio`, `roaming/BetterTerminal`
and `local/Folio`; macOS data uses the six table entries below `home/Library`.
The profile is `profiles/profile.ps1`, PSReadLine's Documents base is
`documents`, and agent roots are `home/.claude`, `home/.codex`, `home/.copilot`.
Temporary data is below `temp`. No registry operation is made in this mode.
An injected root is a destructive test target when purge is requested: create a
fresh disposable directory. Unit tests inject these paths directly and supply
fake system readings without changing process-wide environment variables.

**A release build does not read this variable at all.** Redirection there would
not be a sandbox but a silent failure: a stray or inherited value would make a
production cleanup report every real integration "not present" and exit 0 with
the machine untouched, and whether the machine is clean is the one thing the
command exists to answer. Nothing under `scripts/` sets the variable, so no
release-binary test depends on it; `uninstall_sandbox_door_is_not_read_by_a_shipped_build`
pins both halves. Use a debug build, or a disposable VM, to exercise the door.

Two further rules the door keeps, in every build: it **creates nothing** — a data
root that does not exist means "no marks", and it is still absent afterwards — and
a path read from `integration-marks.json` is data, not authority, so it is checked
for shape and kind before it is used, sandbox or no sandbox.
