# Adversarial code review, 2026-09-08

Reviewed: `main` at `e5e02b41e8a316376e8168ce126d05fc36955a71`, in five slices.

1. **From a program's output to the disk and the network.** Whether text printed inside a pane, or a file the reader only hovers or clicks, can make the window read, open, run, render or navigate to something nobody asked for.
2. **The unsafe surface in `bt-platform` and the process boundary.** Whether the `unsafe` sites are sound, and whether anything Folio exposes to other processes on the machine lets one of them interfere.
3. **Shell integration scripts and escape-sequence handling.** Whether the files injected into the reader's shells are correct in the shells they claim to support, and whether the control and escape sequence handling can be turned against them.
4. **Persisted state, migrations, first-run writes and the update check.** Whether an old, corrupt, truncated or concurrently written file under the data directory can stop startup, lose data or change the machine, and whether the writes outside the data directory stay reversible.
5. **The PTY-to-screen pipeline.** The invariants that reflow, synchronized updates, the alternate screen and the grid projections rely on, which are enforced and which are only assumed.

How: slices 1 and 2 were read by two independent reviewers each, slices 3 to 5 by one. Every finding was then re-verified against the code by a separate reader who owns the status and the severity, folds duplicates, and cites the lines. No build, no test and no compiler was run at any point, and nothing in the repository was modified.

## The numbers

| Slice | Reported | Deduplicated | Verified real | Critical | High | Medium | Low |
|---|---|---|---|---|---|---|---|
| 1 output to disk and network | 37 | 28 | 28 | 5 | 12 | 7 | 4 |
| 2 unsafe surface, process boundary | 39 | 28 | 28 | 0 | 5 | 18 | 5 |
| 3 shell integration, escape sequences | 19 | 18 | 18 | 0 | 5 | 6 | 7 |
| 4 persisted state, first-run writes | 14 | 14 | 14 | 0 | 1 | 6 | 7 |
| 5 PTY-to-screen pipeline | 10 | 8 | 8 | 0 | 5 | 3 | 0 |
| **Total** | **119** | **96** | **96** | **5** | **28** | **40** | **23** |

Reported counts both reviewers of a slice. Deduplication folds a reviewer's finding into the one a verifier marked it a duplicate of, and folds the six defects that two slices reached independently into a single row each. No finding was ruled not a defect.

## The findings

| id | Severity | Defect | Trigger | Anchor | Ticket |
|---|---|---|---|---|---|
| R1-1 | critical | Rasterising an SVG uses usvg's default resolver, which reads any file an `<image href>` names, a UNC share included, before any size or format check. | printed output | `crates/bt-math/src/lib.rs:644` | T1 |
| R1-2 | critical | The peek card chooses a picture, video or animation body before it reads the refusal, so a network path the preview already refused is stat-ed and handed to the decoder and to Media Foundation. | hover | `crates/bt-app/src/main.rs:7190` | T2 |
| R1-3 | critical | Device namespace paths pass both target parsers and the network-path test, and one blocking read of a pipe occupies the single preview worker for the rest of the session. | hover | `crates/bt-platform/src/lib.rs:1449` | T2 |
| R1-4 | critical | A rendered Markdown file's image sources and link targets carry no locality test, so hovering a local document reaches an author-chosen host over SMB. | hover | `crates/bt-app/src/preview.rs:5630` | T2 |
| R1-5 | critical | Reveal in Explorer wraps the path in quotes and escapes nothing, so a name containing a quote becomes a second argument on Explorer's command line, and the path is never checked for existence. Rated medium by the slice 1 verifier and critical by the slice 2 verifier, who traced the git pathname route. | click | `crates/bt-platform/src/lib.rs:1334` | T3 |
| R1-6 | high | A folder link opened into the Files column runs `git status` in that folder, and nothing overrides a repository's own `core.fsmonitor`, so the repository chooses a program to run. | click | `crates/bt-app/src/git.rs:2090` | T1 |
| R1-7 | high | Hovering a `.gif` reads the whole file with no size cap and decodes it with the image limits disabled, so a small file's declared logical screen allocates gigabytes. | hover | `crates/bt-app/src/animation.rs:167` | T4 |
| R1-8 | high | Three decoded-pixel caches are inserted into and never evicted, so hovering distinct files retains their pixels until the process ends. | hover | `crates/bt-app/src/main.rs:9711`, `crates/bt-term/src/inline_image.rs:523`, `crates/bt-app/src/main.rs:9407` | T4 |
| R1-9 | high | Hover hit-testing calls `is_dir` and card layout calls `metadata` on the window thread every frame, so a path that answers slowly stops the window painting and reading input. | hover | `crates/bt-app/src/main.rs:60573` | T2 |
| R1-10 | high | The web seat registers main-frame navigation only, so a frame or a subresource in a previewed local page loads without asking the gate. | click | `crates/bt-platform/src/webview.rs:1292` | T9 |
| R1-11 | high | `names_a_program` reads the extension off the untrimmed name, so a trailing dot or space hides `.exe` from the refusal while the shell still resolves the executable. | Ctrl+click | `crates/bt-platform/src/lib.rs:4691` | T3 |
| R1-12 | high | Every refusal card carries the same open button, so the card that refuses to read a network path hands that path to the shell on one press. | click | `crates/bt-app/src/main.rs:34628` | T3 |
| R1-13 | high | When the vendored parser force-ends a synchronized update on buffer overflow it clears its deadline, so the adapter's own flag stays armed, every later byte is retained, and each resize replays the lot. | printed output | `crates/bt-term/src/adapter.rs:870` | T4 |
| R1-14 | high | An unterminated OSC payload is retained with no cap in three places: both vendored parsers and the adapter's replay tail. | printed output | `crates/bt-term/src/adapter.rs:1281` | T4 |
| R1-15 | high | The row cache turns a shared OSC 8 target into an owned string for every cell it covers, so one long target costs rows times columns copies. | printed output | `crates/bt-term/src/cell_capture.rs:161` | T4 |
| R1-16 | high | When the mint refuses a candidate the navigation check falls through to the generic scheme test, so a previewed local page navigates anywhere, and a cancelled download then reaches the reader's real browser with no gesture. | click | `crates/bt-app/src/webnav.rs:351` | T3 |
| R1-17 | high | The agent probe runs `cmd /c copilot`, which resolves a bare name from the process's current directory before PATH, and the two shell calls pass a null directory. | normal use | `crates/bt-app/src/attention_copilot.rs:653` | T3 |
| R1-18 | medium | Existence checks and the directory reader follow a symlink with plain `metadata`, so a local path whose target is a share dials that share. | printed output | `crates/bt-term/src/session.rs:93` | T2 |
| R1-19 | medium | Translating a WSL mount path pastes the tail verbatim, so a `..` across the mount root resolves to a different drive than the pane meant. | printed output | `crates/bt-transcript/src/paths.rs:218` | T2 |
| R1-20 | medium | Counting a PDF's pages streams the whole file with no length cap and holds the single preview worker while it does. | hover | `crates/bt-app/src/pdf.rs:142` | T4 |
| R1-21 | medium | The seat's settings table sets neither the script switch nor the script-dialog default, so a previewed page holds the seat with repeated dialogs and two switches are nobody's decision. | click | `crates/bt-platform/src/webview.rs:512` | T9 |
| R1-22 | medium | The reserved-device test compares the stem exactly, so a quoted path whose stem ends in a space or a colon still names the device Windows opens. | printed output | `crates/bt-transcript/src/paths.rs:1753` | T2 |
| R1-23 | medium | The title and address sanitisers drop control characters only, so format characters reach the hover line and the tab title and reorder the host a reader is told to check before clicking. | printed output | `crates/bt-app/src/main.rs:96976` | T3 |
| R1-24 | medium | A forged OSC 133 command region is stored as the tab's last command and typed into the prompt of the restored pane. | printed output | `crates/bt-term/src/session.rs:4092` | T8 |
| R1-25 | low | The folder reader collects and sorts every entry in a directory before it applies the entry cap. | hover | `crates/bt-app/src/files.rs:1053` | T4 |
| R1-26 | low | The browser door spends an OSC 8 target on the shell without the userinfo refusal the address bar applies. | Ctrl+click | `crates/bt-app/src/main.rs:75422` | T3 |
| R1-27 | low | An OSC 52 store is base64-decoded in full into a string that is then discarded. | printed output | `crates/bt-term/src/adapter.rs:528` | T4 |
| R1-28 | low | Favicon and thumbnail decoding sets no decoder limits and runs on the event loop. | normal use | `crates/bt-app/src/favicon.rs:289` | T4 |
| R2-1 | high | The attention pipe re-arms and frees an overlapped structure after cancelling without waiting for the cancelled read to complete. Rated low by the first slice 2 verifier, who holds the completion lands on the same thread, and high by the second. | normal use | `crates/bt-platform/src/attention_pipe.rs:750` | T7 |
| R2-2 | high | The vendored command builder passes a registry string the registry crate never terminates to `ExpandEnvironmentStringsW`, which reads past the allocation, twice per pane spawn. | a crafted registry value | `vendor/conpty/portable-pty/src/cmdbuilder.rs:114` | T7 |
| R2-3 | high | Summon joins the event-loop thread's input queue to the foreground window's thread with no hung-window check and no timeout, so an unresponsive foreground application freezes the window. | normal use | `crates/bt-platform/src/hotkey.rs:338` | T7 |
| R2-4 | high | The 2D video buffer branch validates neither the pitch nor the buffer's own length before copying rows, unlike the flat branch beside it. Rated medium by the first slice 2 verifier and high by the second. | a crafted file on disk | `crates/bt-platform/src/video/mod.rs:722` | T7 |
| R2-5 | high | The summon accepts any hotkey message with a null window and id 1, and acts on it when the shortcut is disabled and when the registration was refused. | a message from another process of the same user | `crates/bt-platform/src/hotkey.rs:150` | T7 |
| R2-6 | medium | Pane shutdown drops the pseudoconsole before it closes the output pipe, which is the documented deadlock, and waits on the child with no bound; no job object exists, so grandchildren outlive the pane. | normal use | `crates/bt-pty/src/lib.rs:1351` | T7 |
| R2-7 | medium | Device-loss recovery installs the fresh loss latch before the per-window adopt loop, so a window that fails to adopt stays blank and the machine reports itself recovered. | normal use | `crates/bt-render/src/lib.rs:4942` | T7 |
| R2-8 | medium | A batch-file profile is wrapped with argv quoting rather than interpreter quoting, so the shell metacharacters in a space-free path reach `cmd` as syntax. Rated low by the first slice 2 verifier and medium by the second. | normal use | `crates/bt-pty/src/lib.rs:985` | T7 |
| R2-9 | medium | One rate bucket serves the whole attention listener and is charged before parsing, so one pane's runaway hook suppresses every other pane's events. Rated low by the first slice 2 verifier and medium by the second. | normal use | `crates/bt-platform/src/attention_pipe.rs:815` | T7 |
| R2-10 | medium | Environment-level web handlers are registered on every install and removed by nothing, so every closed host stays alive on the process-wide environment. Rated low by the first slice 2 verifier and medium by the second. | normal use | `crates/bt-platform/src/webview.rs:1699` | T9 |
| R2-11 | medium | The package location is turned into a URI by string replacement and then parsed, so a percent sign, a hash or a question mark in the installation path names a different location. | normal use | `crates/bt-platform/src/msix.rs:396` | T7 |
| R2-12 | medium | Closing a host never clears the cached environment, so a rebuild adopts the environment it exists to abandon, and the pointer comparison that follows makes that tab untearable for the rest of the session. | normal use | `crates/bt-platform/src/webview.rs:839` | T9 |
| R2-13 | medium | The pending controller slot carries no generation and closing leaves it in place, and the start-deadline path raises a card whose restart button cannot fire because the machine never enters the failed state. | normal use | `crates/bt-platform/src/webview.rs:959` | T9 |
| R2-14 | medium | Nothing refuses a modifier-less chord for the global summon, so recording a single letter registers it for the whole desktop and it survives restart. | normal use | `crates/bt-platform/src/hotkey.rs:91` | T7 |
| R2-15 | medium | The multithread interface is queried off the D3D11 device rather than its immediate context, so the query fails and the protection the video engine needs is never applied. | normal use | `crates/bt-platform/src/video/engine.rs:882` | T7 |
| R2-16 | medium | Installing a seat keeps the controller and the page on every error path, and the settings cast that precedes the loop means an older runtime applies none of the seven settings. | normal use | `crates/bt-platform/src/webview.rs:986` | T9 |
| R2-17 | medium | The notifier uninitialises its apartment before the field holding the toast notifier is dropped, so a release lands in a torn-down apartment on every window close. | normal use | `crates/bt-platform/src/lib.rs:4249` | T7 |
| R2-18 | medium | The first-frame decode thread is spawned with its handle discarded, and Media Foundation is shut down as soon as the app loop returns, under a reader that may still be inside a read. | normal use | `crates/bt-platform/src/video/mod.rs:426` | T7 |
| R2-19 | medium | The media engine counter is incremented before a fallible source assignment, so a failure leaks the count and the shutdown that releases the engine never runs; the drop join has no bound. | normal use | `crates/bt-platform/src/video/engine.rs:926` | T7 |
| R2-20 | medium | The launch-time package repair does not take the busy latch, and a failed state read reads as absent, so a removal reports success over a package that is still registered. | normal use | `crates/bt-app/src/explorer_menu.rs:659` | T7 |
| R2-21 | medium | The attention toast latches on the sentence rather than on the turn and filters control characters only, so a program in a pane repeats Folio-branded toasts and puts format and separator characters in them. | printed output | `crates/bt-app/src/attention.rs:1624` | T7 |
| R2-22 | medium | Every pane's environment is rebuilt from the registry, which discards the launching shell's PATH, and an environment name containing an equals sign is written into the block unchecked. | normal use | `vendor/conpty/portable-pty/src/cmdbuilder.rs:137` | T7 |
| R2-23 | medium | The summon chord dismisses the quake window whenever it is on screen, with no focus test, so the chord hides a visible unfocused window instead of raising it and then gives the keyboard to a third window. | normal use | `crates/bt-app/src/main.rs:94602` | T7 |
| R2-24 | low | Find callbacks are attached once behind a latch that closing never clears, so after a rebuild the match count and the active index stop updating. | normal use | `crates/bt-platform/src/webview.rs:2020` | T9 |
| R2-25 | low | Two early-error paths in the attention endpoint leak the stop event and a live pipe handle. | normal use | `crates/bt-platform/src/attention_pipe.rs:413` | T7 |
| R2-26 | low | A failed classic Explorer install rolls nothing back and leaves a verb key with no command, and the state reader calls that absent, so no later launch repairs it. | normal use | `crates/bt-platform/src/lib.rs:7332` | T7 |
| R2-27 | low | The attention client trusts the pipe name in its environment, requests no impersonation level, and writes to it with an unbounded blocking write. | normal use | `crates/bt-app/src/attention_wire.rs:594` | T7 |
| R2-28 | low | The hang watchdog answers quiet for an indefinite park before it asks the window thread anything, so a wedge entered on the wake path produces no report. | normal use | `crates/bt-app/src/hang_watch.rs:1005` | T7 |
| R3-1 | high | A percent-encoded backslash in an OSC 7 payload rebuilds a UNC path the local-authority check passes, and the next spawn calls `is_dir` on it from the window thread. The slice 1 reviewer reached the same decoder by another route. | printed output | `crates/bt-transcript/src/paths.rs:2000` | T3 |
| R3-2 | high | The boundary parser discards the vendored parser's ignore flag, so a synchronized-update sequence carrying more than 32 parameters arms a retention the real parser never armed and nothing can disarm. | printed output | `crates/bt-term/src/adapter.rs:472` | T4 |
| R3-3 | high | Every OSC 133 command mark appends a region nothing prunes, and the open one is found by a scan from the front, so repeated marks grow without bound and cost quadratic time. | printed output | `crates/bt-term/src/session.rs:4283` | T4 |
| R3-4 | high | With DEC 2027 on, a grapheme cluster has no length bound, and each added mark re-copies and re-measures the whole cluster on the window thread. | printed output | `vendor/alacritty_terminal/src/term/mod.rs:1797` | T4 |
| R3-5 | high | The bash hook assigns over element 0 of an array-valued `PROMPT_COMMAND`, which runs the reader's remaining hooks twice and emits a command-start mark at the prompt; that mark then suppresses the real command's mark. | normal use | `scripts/shell-integration/folio.bash:228` | T6 |
| R3-6 | medium | Automatic integration maps `sh` and `zsh` to bash's init-file option, which `sh` ignores in silence and `zsh` does not accept, so those panes get no marks and no directory. | normal use | `crates/bt-app/src/profiles.rs:787` | T6 |
| R3-7 | medium | Turning on bash integration builds the command line from literals and drops every argument the profile carries. | normal use | `crates/bt-app/src/shell_integration.rs:424` | T6 |
| R3-8 | medium | The injected init file sources the login chain and never the interactive one, and the shell is spawned interactive without the login flag, so the pane is neither a login shell nor a plain interactive one. | normal use | `scripts/shell-integration/folio.bash:56` | T6 |
| R3-9 | medium | The cmd prompt emits the directory raw while the decoder truncates at a hash and refuses an invalid percent escape, so a directory containing either is recorded wrongly or forgotten. | normal use | `crates/bt-app/src/shell_integration.rs:106` | T6 |
| R3-10 | medium | Shell integration markers are lifted out of the stream and acted on at once while the bytes around them sit in the synchronized-update buffer, so marks record the cursor and the screen from before the update. The slice 5 reviewer found the same defect. | printed output | `crates/bt-term/src/adapter.rs:781` | T6 |
| R3-11 | medium | An OSC 7 naming the POSIX root is rejected, so a WSL pane sitting at the root loses its directory and new tabs no longer inherit it. | normal use | `crates/bt-transcript/src/paths.rs:1974` | T6 |
| R3-12 | low | Column and vertical cursor moves pass an absolute line into the function that adds the scroll region offset, so origin mode adds it twice. | printed output | `vendor/alacritty_terminal/src/term/mod.rs:2014` | T6 |
| R3-13 | low | UTF-8 mouse mode is accepted and reported as enabled but never implemented, so coordinates past column 95 go out as bytes a decoder cannot frame. | printed output | `vendor/alacritty_terminal/src/term/mod.rs:2886` | T6 |
| R3-14 | low | The fallback PowerShell profile line wraps the script path in a double-quoted string, so a path holding a dollar sign or a backtick is interpolated by the shell. | normal use | `crates/bt-app/src/shell_integration.rs:902` | T6 |
| R3-15 | low | The PowerShell wrapper reports the directory before it invokes the chained prompt, so a prompt that changes directory leaves the pane one prompt behind. | normal use | `scripts/shell-integration/folio.ps1:692` | T6 |
| R3-16 | low | The PowerShell wrapper resets the success flag between capturing it and calling the chained prompt, so the reader's own prompt always sees success. | normal use | `scripts/shell-integration/folio.ps1:609` | T6 |
| R3-17 | low | Reading the previous DEBUG trap overwrites the shell's positional parameters on the launch path, and on the dot-source path the read returns nothing and the reader's own trap is replaced. | normal use | `scripts/shell-integration/folio.bash:171` | T6 |
| R3-18 | low | Cancelling a text OSC with a new escape sequence drops the escape and its introducer, so the sequence that cancelled it prints as text. | printed output | `crates/bt-term/src/inline_image.rs:1650` | T6 |
| R4-1 | high | Restoring a tab whose profile names a program that is no longer on the machine panics during startup, and the window cannot open until the session file is edited by hand. | normal use | `crates/bt-app/src/main.rs:29226` | T8 |
| R4-2 | medium | A persisted terminal font size of zero reaches the renderer unclamped and asserts inside the text layer before any window exists, on every launch. | a crafted file on disk | `crates/bt-app/src/main.rs:29109` | T8 |
| R4-3 | medium | A settings or session document that fails to parse or carries a future version is replaced by defaults with one line on stderr and no copy of the original bytes. | a crafted file on disk | `crates/bt-persist/src/migrate.rs:1340` | T8 |
| R4-4 | medium | The PowerShell profile is replaced with a truncating write rather than atomically, and the daily backup is skipped whenever the day's name is already taken. | normal use | `crates/bt-app/src/shell_integration.rs:1013` | T8 |
| R4-5 | medium | Two processes share one data directory with no lock and no re-read, so the second to write erases the first's preferences, windows and tabs. | normal use | `crates/bt-app/src/persist.rs:500` | T8 |
| R4-6 | medium | An installed PowerShell integration is never compared against the shipped script again, so an upgrade, a deletion or a truncation of that script is never noticed. | normal use | `crates/bt-app/src/shell_integration.rs:878` | T8 |
| R4-7 | medium | A restored window keeps a negative top whenever its recorded size fits the monitor, so it returns with its title bar above the desktop and out of reach of the pointer. | normal use | `crates/bt-app/src/main.rs:100833` | T8 |
| R4-8 | low | The session sentinel is created with a truncating open, so a hard link or a symlink planted at its name empties the target on every launch. | a crafted file on disk | `crates/bt-persist/src/sentinel.rs:43` | T8 |
| R4-9 | low | No persisted document has a size bound and no restore has a tab-count bound, so an oversized file is read whole and expanded into spawns before the first frame. | a crafted file on disk | `crates/bt-persist/src/migrate.rs:1313` | T8 |
| R4-10 | low | Every store assigns the new value before the write and reports success regardless, so a failed save is remembered as done and reselecting the same value retries nothing. | normal use | `crates/bt-app/src/persist.rs:529` | T8 |
| R4-11 | low | A failed rename leaves the whole temp file behind and the document is marked dirty again, so the debounce repeats it indefinitely. | normal use | `crates/bt-persist/src/atomic.rs:58` | T8 |
| R4-12 | low | Turning the Explorer entry off while its install is still running computes no removal job from the stale cached state, so the entry installs against the reader's last choice. | normal use | `crates/bt-app/src/main.rs:46192` | T8 |
| R4-13 | low | The sparse package is registered against its folder while its manifest names one fixed executable, so the first-page entry can launch a different binary than the one that registered it. | normal use | `crates/bt-app/src/explorer_menu.rs:465` | T8 |
| R4-14 | low | The update check writes back the state it read before the request, so an acknowledgement made while the request was in flight is undone and the badge lights again. | normal use | `crates/bt-app/src/update.rs:436` | T8 |
| R5-1 | high | Command-mark anchors are registered and never removed, so every prompt repaint leaks anchor ids that each later scroll and resize pass walks. | normal use | `crates/bt-doc/src/document.rs:56` | T4 |
| R5-2 | high | Reaping drops the child on the first answer, so the pane loop consumes the active tab's last exit and the tab loop reads that tab as alive; the tab never closes and the process never exits. | normal use | `crates/bt-pty/src/lib.rs:1317` | T5 |
| R5-3 | high | Resizing on the alternate screen reflows the hidden primary grid while witnesses refuse to reseat off primary, and the return to primary re-blesses the stale coordinates with a fresh generation. | normal use | `crates/bt-term/src/session.rs:4899` | T5 |
| R5-4 | high | The stability gate and the artifact scheduler run for the focused pane only, so a visible unfocused pane never typesets a block it has just printed. | normal use | `crates/bt-app/src/main.rs:72715` | T5 |
| R5-5 | high | A DPI, font, theme or language change gives every pane new metrics but sets the layout key on the focused pane only, so a sibling keeps rasters built for the old metrics. | normal use | `crates/bt-app/src/main.rs:86237` | T5 |
| R5-6 | medium | Reflow groups mark witnesses by line text alone, so two commands whose prompt lines read the same collapse onto the newest occurrence. | normal use | `crates/bt-term/src/session.rs:5027` | T5 |
| R5-7 | medium | The synchronized-update timeout release bumps the pane's revision and never tells the cards, so a focus card keeps the picture from before the release. | normal use | `crates/bt-app/src/main.rs:73238` | T5 |
| R5-8 | medium | Staging search labels cell columns as grapheme offsets while the projection builds anchors in grapheme units, so a highlight lands one cell right per preceding wide glyph. | normal use | `crates/bt-app/src/search.rs:797` | T5 |

## Tickets

### T1 `fix/svg-href-and-git-hooks` (merged, `95d9bd0`)

R1-1, R1-6.

Why together: both are a third party's configuration file choosing what Folio's own subprocess or parser reaches. The fix in each is one explicit override where the tool's options are built: a resolver that returns nothing for an external `href`, and `core.fsmonitor=false` beside the `core.quotepath=false` already passed.

### T2 `fix/untrusted-path-locality` (merged, `a7fc271`; left open: a junction in an ancestor directory is still traversed, and `\?\` verbatim paths are now refused everywhere)

R1-2, R1-3, R1-4, R1-9, R1-18, R1-19, R1-22.

Why together: one question, may this window touch this path without a click, is currently answered by a handful of lexical prefix tests spread across the preview, the decoder, the detector and the window thread, and each row is a route that misses one of them. The design is a single locality answer computed once per reference and off the window thread, covering the device namespace, a symlink's target, a share and a reserved device, which every hover, card layout, decoder and document-image route asks instead of testing a prefix itself.

### T3 `fix/hand-off-to-the-machine` (merged, `9f5c4d0`; left open: `open_local_path` does not stat before the hand-off)

R1-5, R1-11, R1-12, R1-16, R1-17, R1-23, R1-26, R3-1.

Why together: every row is on the route where Folio stops rendering and gives something to the machine, by shell, by browser or by spawn. The design is one hand-off point that normalises the target the way Windows will, refuses the shapes a real target never has, names every program absolutely, and states in one place which cards and which modifiers may reach it.

### T4 `fix/bytes-from-the-child-are-bounded` (merged in two halves: terminal side `becba80`, app side `0ac921b` as `fix/decoders-and-caches-are-bounded`)

R1-7, R1-8, R1-13, R1-14, R1-15, R1-20, R1-25, R1-27, R1-28, R3-2, R3-3, R3-4, R5-1.

Why together: each row is a place where the size of what the window keeps or does is chosen by the child rather than by Folio. The design is a budget at each intake point: in the scanner for OSC payloads, in the caches as a byte ceiling with eviction, in the decoders as limits set before the first allocation, and in the mark and anchor registries as a release path owned by whoever drops the id. R3-2 and R1-13 are two armings of one wedge and a single disarm rule covers both.

### T5 `fix/pipeline-invariants` (merged, `21b7cea`; left open: `mark_pty_resize_requested_at` re-blesses coordinates without a witness snapshot, and a reflow on the alternate screen still loses the rows that leave the top)

R5-2, R5-3, R5-4, R5-5, R5-6, R5-7, R5-8.

Why together: all seven are a fact that is window-wide or transcript-wide being computed on the focused leaf, the visible plane or the newest match. The design is to move each of these onto the road that already visits every leaf of every tab, and to key marks and search on the identity the other consumer uses rather than on a coordinate that looks close enough. R3-10 is the eighth row of this family and is filed under T6 with the rest of that slice.

### T6 `fix/shell-integration-scripts` (in flight)

R3-5, R3-6, R3-7, R3-8, R3-9, R3-10, R3-11, R3-12, R3-13, R3-14, R3-15, R3-16, R3-17, R3-18.

Why together: the injected scripts and the sequences they emit assume one shell, one prompt shape and one spelling of a directory, and every row is a shell or a directory that does not match the assumption. The design is to ask the shell what it is rather than assume it, and to accept on the reading side the spellings the emitting side genuinely cannot encode.

### T7 `fix/platform-boundary` (first half in flight as `fix/platform-boundary-summon-and-pipes`: R2-1, R2-3, R2-5, R2-9, R2-11, R2-14, R2-20, R2-21, R2-23, R2-25, R2-26, R2-27, R2-28; second half queued: R2-2, R2-4, R2-6, R2-7, R2-8, R2-15, R2-17, R2-18, R2-19, R2-22)

R2-1, R2-2, R2-3, R2-4, R2-5, R2-6, R2-7, R2-8, R2-9, R2-11, R2-14, R2-15, R2-17, R2-18, R2-19, R2-20, R2-21, R2-22, R2-23, R2-25, R2-26, R2-27, R2-28.

Why together: each row is a Win32 or COM contract the code half keeps, on ownership order, on waiting for what it cancelled, on bounding what it joins, on validating what the OS returned, or on deciding what another process may make it do. The design is to make each contract explicit at its one call site: release before uninitialise, close before drop, bound every wait, validate every length and pitch, and treat a message or a claim as authoritative only when Folio itself registered it. R2-14, R2-15, R2-21 and R2-27 are not in this ticket's original list and sit here as the nearest home.

### T8 `fix/persisted-state`

R1-24, R4-1, R4-2, R4-3, R4-4, R4-5, R4-6, R4-7, R4-8, R4-9, R4-10, R4-11, R4-12, R4-13, R4-14.

Why together: every row is a value read back from disk and trusted as though this process had written it a moment ago. The design is one intake rule for persisted documents, bound the read, keep the rejected bytes, resolve every reference against the machine as it is now rather than as it was, and one write rule, atomic, retried, and reported when it fails. R1-24 is filed here because the forged mark's damage lands in the restore path; R4-8 and R4-9 are not in this ticket's original list and sit here as the nearest home.

### T9 `fix/webview-seat`

R1-10, R1-21, R2-10, R2-12, R2-13, R2-16, R2-24.

Why together: the seat's guarantees are established once at install and never re-established, so every row is a gate, a setting or a subscription that a frame, a subresource or a rebuild slips past. The design is to make install and rebuild the same operation over the same table, gating every request rather than the main frame, failing closed when a row cannot be applied, and clearing on close everything install created.

## What was read and found sound

- **The update and network boundary.** `crates/bt-platform/src/http.rs` and `crates/bt-app/src/update.rs`, read in full by both slice 1 reviewers and by slice 4: one outbound request to a constant destination, no certificate or revocation weakening, the response cap applied before each resize, a whole-call deadline over the phase timeouts, a strict version parser, and nothing downloaded or executed.
- **Path detection and URI decoding.** `crates/bt-transcript/src/paths.rs` and `crates/bt-transcript/src/lib.rs`: the local-authority test, the drive-prefix requirement for opening a candidate, the interior-empty-component refusal, the authority allow-list, lexical `..` folding that cannot climb out of its base, and the URL rules that reject userinfo and keep the bare-domain table short. R1-19, R1-22 and R3-1 are the exceptions.
- **Replies written back into the pty.** `crates/bt-term/src/adapter.rs` in full plus the vendored terminal's reply paths: every reply a child can provoke is formatted from integers or build constants, no reply can carry text, a newline or a carriage return, and there is no answerback, no version report, no capability report and no title report.
- **The image and inline-payload intake.** `crates/bt-term/src/inline_image.rs`: file and pixel budgets checked before allocation, decoder allocation limits set, decoded dimensions re-verified against the buffer, and every OSC payload the scanner owns capped. The SVG arm and the pre-gate stat are the exceptions.
- **The navigation allow-list.** `crates/bt-app/src/webnav.rs`: script, data, blob, browser-internal and unknown schemes refused at the gate, the file mint refusing UNC and encoding the four re-parsing characters, and the reverse decoder rejecting `..` and anything not drive-absolute.
- **The web seat's own event handlers.** `crates/bt-platform/src/webview.rs`: new windows handled without opening one, downloads cancelled before anything is read, permissions denied, external scheme launches cancelled, no host objects, no web message bridge, no injected script, and a user data folder built from two literal components.
- **The whole platform crate's unsafe surface.** `crates/bt-platform/src/lib.rs` read line by line with all its `unsafe` sites: the compositor tree, the custom frame subclass, the taskbar's COM balance, the deferred dialogs, the clipboard pair, the registry layer's sizing and closing, the directory watcher, and the console attachment. Also read in full: `crates/bt-platform/src/hang.rs`, `explorer_command.rs`, `hotkey.rs`, `attention_pipe.rs`, `msix.rs`, and the video pair `video/mod.rs` and `video/engine.rs`.
- **The pipe's access control and wire.** `crates/bt-platform/src/attention_pipe.rs`: a protected one-ACE descriptor naming the logon session, remote clients rejected, first-instance protection, a token with no logon session failing closed, and a decode path with no attacker-controlled allocation, no trusted length field and no reachable panic.
- **The process boundary.** `crates/bt-pty/src/lib.rs` and `vendor/conpty/portable-pty/src/win/`: the child is created with inheritance off and its standard handles invalidated, the sidecar loader requires absolute paths beside the running executable, and no handle-inheritance leak exists anywhere in the process. `crates/bt-app/src/git.rs` uses an absolute program, argv elements, both pipes drained on their own threads, and a timeout.
- **The registry inventory.** Everything under the current user, nothing under the machine hive, no elevation anywhere, and no executable path is ever read back out of the registry and run.
- **The persistence primitives.** `crates/bt-persist/src/error.rs`, `debounce.rs`, `write_tracker.rs` and `lib.rs` read in full: the migration steps do not write over their input, and a successful commit is a same-directory temp file, a sync and a rename.
- **The pipeline's own building blocks.** `crates/bt-term/src/cell_capture.rs`, `crates/bt-term/src/lifecycle.rs`, `crates/bt-term/src/command_marks.rs`, `crates/bt-app/src/termscroll.rs` and `crates/bt-app/src/settling.rs` read in full, plus the bounded PTY ring and wake handoff, the single bottom-relieved extent, the wrapped-line reconstruction and the worker's source and layout revalidation.
- **The pure layout modules.** `crates/bt-app/src/file_peek.rs`, `hex_peek.rs`, `peek_strip.rs`, `websheet.rs`, `web_thumb.rs`, `video_seat.rs` and `crates/bt-doc/`: layout, parsing and geometry only, with no filesystem, network, process or engine call in any of them.
- **The shell scripts' sound halves.** `scripts/shell-integration/folio.bash` and `scripts/shell-integration/folio.ps1` read in full, plus `crates/bt-app/src/wsl.rs`: the distribution lookup preserves Unicode names and resolves the default by identifier, the directory emitters use a fixed output format rather than evaluating directory text, the WSL launch passes the script as a quoted positional argument, and bracketed paste strips embedded end markers and control characters.

## Method

Two independent reviewers on slice 1 arrived at the same three criticals from different starting files, which is the strongest evidence in this document that those three are real and the cheapest argument for reading a security-facing slice twice. The verification step moved severities in both directions and earned its cost either way: it raised a hover finding to critical once it established that the trigger needs no click, raised a shell-integration finding when the forged mark turned out to suppress the real one, and cut several reports down to the half that survived being traced, including one whose stated reproduction executes nothing. The recurring root cause across slices 1 to 3 is a locality check that is lexical and duplicated: the same question, whether a path is local enough to touch, is asked by prefix in eight places and answered differently in each, and the criticals are the routes that reach a copy which has not learned about device paths, symlink targets or a document's own image sources. Worth keeping for the next round: pin every finding to the gesture a reader actually makes, because the difference between a click and printed output was the difference between medium and critical three times here.
