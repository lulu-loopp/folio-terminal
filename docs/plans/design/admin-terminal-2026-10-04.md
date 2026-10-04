# Administrator terminals on Windows

Design target: Folio 0.4.7, Windows only. This note adopts the ticket's first road: an unelevated Folio window delegates one administrator pane to one elevated, headless `folio.exe --elevated-host` process. The window continues to own the tab, terminal model, rendering, input policy, persistence, and user-facing state; the host owns that pane's ConPTY, shell process tree, and privileged process inspection.

## 1. Today

### Pane process and terminal ownership

`bt-app` owns the pane. [`LeafSession`](../../../crates/bt-app/src/main.rs) contains `pty: Option<bt_pty::PtySession>` and a `bt_term::DualPlaneSession`. [`create_leaf_session`](../../../crates/bt-app/src/main.rs) resolves the profile, program, environment, starting directory, and initial grid, then crosses `admission::doors::PtyBirth` through `pty_door::spawn_shell` to `PtySession::spawn_shell_in`.

[`PtySession`](../../../crates/bt-pty/src/lib.rs) owns the portable-pty master and child, the remembered exit result, bounded input and output rings, and one reader and one writer thread. `PtySession::spawn` opens the pty, starts the child, clones the master reader, takes the writer, and starts `read_pty_output` and `pump_pty_input`. The reader blocks in the pty read, applies output-ring backpressure, and wakes the window; the writer drains whole queued input records into the pty. The window thread only takes the rings' bounded locks: `drain_leaf_pty` calls `read_output_slice`, and `offer_pty_input` calls `try_push`.

Resize remains synchronous today. `commit_leaf_resize` crosses `admission::doors::PtyResize` through `pty_door::resize` to `PtySession::resize`, which reaches `ResizePseudoConsole` on Windows. Exit observation is `PtySession::try_wait`; close/drop calls `PtySession::shutdown`, closes input, gives the writer a bounded retirement, kills the child, closes output before the master, and gives the reader a bounded retirement. `retire_session` and the global `Retirements` move slow retirement away from an ordinary pane close; `wait_for_retirements` is the bounded quit wait.

`bt_term` does not own the operating-system transport. Its [`DualPlaneSession`](../../../crates/bt-term/src/session.rs) owns the parsed terminal/transcript state. [`bt_term::PtyTransport`](../../../crates/bt-term/src/adapter.rs) is only the two-value ConPTY/Unix fact used by `TerminalAdapter::reset_program_modes` for T-RESET-MODES, especially the different focus-reporting reset. It is not an I/O or lifecycle abstraction.

### Resident identity and handoff

[`bt_platform::instance`](../../../crates/bt-platform/src/instance.rs) identifies a resident by a `Local` named mutex derived from the folded data-directory identity. The claim lasts for the resident lifetime. It prevents accidental duplicate residents for that data directory; it is not a hostile same-user boundary.

[`bt_platform::launch_pipe`](../../../crates/bt-platform/src/launch_pipe.rs) is the resident's well-known handoff endpoint. On Windows its name combines the logon SID tag and data-directory tag; the server uses a protected DACL for the exact logon SID, `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE`, overlapped I/O, bounded frames, and `GetNamedPipeServerProcessId`/image checks. [`launch_wire::hand_over`](../../../crates/bt-app/src/launch_wire.rs) sends the versioned JSON v2 request, waits within `HANDOVER_BUDGET`, and confirms receipt. An elevated host must open neither this mutex nor this endpoint.

### Update and uninstall census

The update road uses [`install_flip::running_from`](../../../crates/bt-platform/src/install_flip.rs) to enumerate processes by executable file identity, including PID and start time, rather than by process name. Windows apply additionally calls [`UpdateInstall::ready_to_move`](../../../crates/bt-app/src/update_apply_windows.rs), which refuses to replace an old member while any process still maps it.

[`uninstall::remove_the_program`](../../../crates/bt-app/src/uninstall.rs) gives the copied remover every `running_from(program)` identity. [`deferred_removal`](../../../crates/bt-platform/src/deferred_removal.rs) rechecks those identities while waiting and reports files or processes left at its deadline. Because the proposed host runs the installed `folio.exe`, both file-identity mechanisms see it without a special process name.

## 2. The elevated host

### Process model

Use one host per elevated pane. A pane close then has one pipe, one job, one ConPTY, and one privileged lifetime to retire; a crash or protocol failure affects one pane; and the host cannot become a long-lived privileged broker for later panes. A shared host would reduce UAC prompts, but it would retain authority after the pane whose gesture obtained it had closed and would enlarge both routing complexity and failure radius. Each newly created elevated pane therefore produces one UAC prompt; duplicating or splitting an elevated pane also creates a new host and prompt.

The resident is always the server and the host is always the client. Before asking Windows to elevate, the resident creates and listens on a unique pipe, queues an `ElevationAttempt` state for the pane, and sends an asynchronous launch request to a worker. The host owns no visible top-level window.

### Launch and exact pane states

The worker calls `ShellExecuteExW` with verb `runas`, `SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI`, and the installed `folio.exe`. `SEE_MASK_FLAG_NO_UI` suppresses shell error UI, not the UAC consent or credential surface. `SEE_MASK_NOCLOSEPROCESS` supplies the process handle and PID needed for lifetime and peer checks. This follows Microsoft's recommendation to isolate administrative work in a separate process and uses the documented `runas` verb: [ShellExecuteEx](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfoa), [Running with administrator privileges](https://learn.microsoft.com/en-us/windows/win32/secbp/running-with-administrator-privileges).

The command line is private and exact:

```text
folio.exe --elevated-host <wire-version> <pipe-name> <parent-pid> <parent-start-id> <capability>
```

The pipe name and capability are unguessable per attempt. The command line carries no command, environment, profile contents, or path to run; those cross the authenticated pipe. `main` recognizes this exact grammar before console adoption, uninstall/update doors, ordinary CLI parsing, diagnostics, window creation, settings loading, instance claim, or launch-pipe startup, calls `elevated_host::serve`, and exits.

The tab is created immediately and carries its shield while it shows `Waiting for administrator approval…`. There is no Folio confirmation dialog. These are the terminal-body states and actions:

| Event | Persistent pane content | Action |
| --- | --- | --- |
| `ShellExecuteExW` returns `ERROR_CANCELLED` | `Administrator access was canceled.` | `Try again` |
| Launch, connect, hello, or start exceeds 15 seconds | `Administrator terminal couldn't start.` | `Try again` |
| Windows, pipe, or shell start returns a reason | `Administrator terminal couldn't start.` followed by the returned reason | `Try again` |
| Host disappears after start | `Administrator terminal stopped.` followed by a known reason, if any | `Restart shell…` |

Retry is a new explicit gesture and therefore may show UAC again. Failure never falls back to an unelevated shell. Closing the pane cancels its attempt by closing the server and process handle; the host's own 15-second connect deadline makes an accepted but orphaned elevation exit without the parent having to terminate a higher-integrity process.

### Pipe and peer

The parent chooses `\\.\pipe\folio-elevated-<session-tag>-<parent-pid>-<256-bit-random-tag>`, creates the first instance as a local, duplex, overlapped message pipe, and listens before `ShellExecuteExW`. The security descriptor is protected and grants the parent logon SID and Builtin Administrators only; it grants neither World, Anonymous, nor Network. Administrators must be allowed because a standard user may answer UAC with a different administrator account.

The host connects. The parent accepts only a client whose kernel-reported named-pipe client PID equals the PID returned by `ShellExecuteExW`, and the initial `Hello` must repeat the wire version and one-use 256-bit capability. The host requires the kernel-reported server PID to equal the command-line parent PID and requires the server's first authenticated frame to repeat that capability and the parent start identity; an exact mismatch closes both sides.

**Threat model.** The boundary prevents accidental, remote, other-session, and uncredentialed connections from becoming an elevated terminal; the parent and host authenticate the kernel-reported peer PID plus a one-use random capability. It does not defend against a malicious process already running as the same logged-on user or as an administrator, either of which can already launch or inspect this user's programs.

### Wire protocol

Use a small binary protocol, not the launch pipe's JSON schema. Every little-endian frame has `magic`, `u16 wire_version`, `u16 kind`, `u64 generation`, and `u32 payload_length`; reject an unknown version, kind, oversized length, or invalid state transition. Control payloads are capped at 1 MiB and output frames at 64 KiB. Length-prefixed UTF-16 strings carry Windows paths and arguments without a lossy conversion; input and output remain raw bytes.

| Direction | Frames |
| --- | --- |
| Host → parent | `Hello(capability, pid)`, `Started(child_pid)`, `StartFailed(error)`, `Output(bytes)`, `Exit(status?)`, `ForegroundResult(request_id, process)`, `ShutdownAck` |
| Parent → host | `Authenticate(capability, parent_start_id)`, `Spawn(spawn_spec, cwd, grid, environment_overlay)`, `Input(bytes)`, `Resize(rows, cols)`, `ForegroundQuery(request_id)`, `Restart(spawn_spec, cwd, grid, environment_overlay)`, `Shutdown(reason)` |

One ordered parent writer queue preserves input, resize, restart, and shutdown order. Output, exit, foreground results, and child PID are generation-tagged; the parent advances the generation before restart and discards stale frames. The host constructs a fresh environment for the identity represented by its elevated token, then applies the terminal and profile overlays sent by the parent. It never reads `profiles.json` or `settings.json`.

Both directions use bounded rings. A slow Folio reader backpressures host output as the local `OutputRing` does; it does not create an unbounded privileged queue. Protocol errors are pane-local failures and are recorded through the parent's existing diagnostics path without making the host open a trace file.

### Lifetime and prohibited responsibilities

Inside the host, the existing `PtySession` owns the ConPTY and shell. Add a Windows Job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assign the shell process tree, and make the host's session owner hold the job handle. `Shutdown`, pipe break, or parent-process death closes/kills the job and exits the host. Shell exit drains the already-read output, sends `Exit`, and then exits the host. Process exit is the final backstop because it closes the job handle.

The host watches both the pipe and the `(parent PID, start identity)` process handle, so PID reuse is not accepted. It must not create a window, take the single-instance claim, serve the launch or attention endpoints, load or write settings/profiles/session state, run first-run work, check or apply updates, install integrations, uninstall, open links, use the clipboard, or write diagnostics. All user-visible state and durable writes stay in the unelevated resident.

## 3. Fit with existing owners

### Transport boundary

Introduce `bt_pty::PaneTransport`, an explicit enum with `Local(PtySession)` and Windows-only `Elevated(ElevatedSession)`, and replace `LeafSession::pty`. It owns the common nonblocking operations: offer input, drain output, queue resize, observe exit/child ID/ConPTY kind, begin shutdown, and retire. An enum keeps the local fast path and lifecycle visible and avoids a trait whose remote implementation would conceal ordering and generation rules.

Do not enlarge `bt_term::PtyTransport`. T-RESET-MODES made that type a terminal-mode policy input, not a byte transport; both local and elevated Windows sessions report `PtyTransport::ConPty` to `DualPlaneSession::reset_program_modes`.

The host itself continues to use `PtySession`. The resident's `ElevatedSession` owns only the pipe state, launch process handle, bounded queues, reader/writer workers, generation, and last remote exit. `LeafSession` remains the pane owner and `DualPlaneSession` remains the terminal-state owner.

### Local-child assumptions and required changes

| Assumption today | Evidence | Change |
| --- | --- | --- |
| A pane has an in-process `PtySession`. | `LeafSession::pty`, `create_leaf_session`, `drain_leaf_pty`, `offer_pty_input`, `write_pty_input`, and `commit_leaf_resize` in [`main.rs`](../../../crates/bt-app/src/main.rs). | Route these operations through `PaneTransport`; preserve the existing `PtyBirth`/`PtyResize` calls only in `Local`. Elevated resize and input enqueue without an OS wait on the window thread. |
| The shell PID is locally inspectable. | `foreground_program::dispatch` calls `PtySession::shell_process_id`; [`bt_platform::foreground_program::foreground_program`](../../../crates/bt-platform/src/foreground_program.rs) inspects the process tree. | `ElevatedSession` sends `ForegroundQuery`; the host performs the probe at the child's integrity and returns the same logical result tagged with request and generation. Cadence and terminal arming remain in the parent. |
| The private console-members helper is spawned by the resident. | `--console-members` is `CONSOLE_MEMBERS_FLAG`; `foreground_program::run` starts it for Windows provenance. | When serving an elevated query, the host starts the existing exact helper normally. It inherits the host token and needs no second UAC prompt. The helper remains short-lived and returns only console membership. |
| Working directory comes from the child. | `restart_shell` uses `DualPlaneSession::working_directory`; `restart_seed` consumes the last trusted OSC 7 report. | No privileged query is needed. OSC bytes already traverse `Output`; the parent parser remains authoritative. Initial/restart cwd is sent in `Spawn`/`Restart`. |
| Close and quit can kill/drop a local child. | `PtySession::shutdown`, `retire_session`, `wait_for_retirements`, and pane removal in `main.rs`. | `ElevatedSession::shutdown` sends `Shutdown`, closes its pipe at the deadline, and retires its transport workers through the same bounded `Retirements` owner. The host, not the medium-integrity parent, kills the job. |
| “Process still running” is answered by `try_wait`. | `PtySession::try_wait` and the quit/close paths consuming the remembered result. | Cache remote `Exit` and host-process state behind the transport method. A connected host without `Exit` is running; a broken host is stopped/failed, never silently considered an ordinary clean shell exit. |
| Shell integration shares a local PTY and parent-owned endpoints. | Output parsing in `DualPlaneSession`; attention endpoints start in `open_the_data_directorys_endpoints`. | OSC shell integration needs no change beyond the byte relay. Integration variables still name parent endpoints, but split-token and over-the-shoulder credential access to the attention endpoint is a required spike; this ticket does not silently broaden that endpoint's ACL. |
| Reset modes can ask `PtySession::conpty_kind`. | `Runtime::reset_terminal_modes` in [`runtime/terminal.rs`](../../../crates/bt-app/src/runtime/terminal.rs). | Ask `PaneTransport::conpty_kind`; elevated Windows always returns `ConPtyKind::Shipped` or `Inbox` as reported by `Started`. Keep `bt_term::PtyTransport` unchanged. |
| Restart creates a new local leaf first and drops the old one. | `Runtime::restart_shell`, `restart_seed`, and [`M2-restart-shell-contract.md`](../../M2-restart-shell-contract.md) §§1.1–1.7. | A local pane keeps that road. An elevated pane reuses its live host, which kills the old job and creates a fresh ConPTY/shell under a new generation. This keeps elevation with no new UAC; if the host is gone, `Restart shell…` is the new gesture and starts a new host/UAC attempt. Transcript boundary, fresh program modes/environment, input-generation exclusion, and pane-local failure still follow M2. |

The restart decision supersedes the provisional “always unelevated” clause in M2 §1.7 and the matching [`DESIGN.md`](../../DESIGN.md) §7.1.7 item 3. That documentation must be updated in the implementation ticket, not in this design-only ticket. Reusing the pane's live host is narrower than caching elevation across panes and satisfies the owner's ruling that restart keeps elevation.

## 4. Window thread, doors, census, and ownership

### Wait inventory

The window-thread gesture creates state and performs bounded `try_send` operations only. It does not call `ShellExecuteExW`, connect a pipe, wait for UAC, spawn ConPTY, block on pipe I/O, or wait for the host. Completion arrives as a user event and is checked against pane attempt ID and generation.

| Wait or OS effect | Thread/phase | Bound and result |
| --- | --- | --- |
| `ShellExecuteExW(runas)` | elevation-launch worker | One call. Success returns the host handle/PID; refusal maps to canceled; any other error maps to failed. |
| Parent `ConnectNamedPipe`/overlapped event | elevated transport listener | 15 seconds from successful launch; timeout closes the attempt and yields failed. |
| Host `WaitNamedPipe`/`CreateFileW` | standalone host startup | Same 15-second absolute deadline; timeout exits the host. |
| Pipe `ReadFile` | one reader on each side | Blocking transport wait; pipe close releases it. Bounded output rings provide backpressure. |
| Pipe `WriteFile` | one ordered writer on each side | Blocking transport wait; pipe close releases it. Input/control queues are bounded. |
| `PtySession::spawn_shell_in`, resize, and retirement | host session lane | Existing PTY operations, never the resident window thread. Shutdown retains the existing bounded thread-retirement policy. |
| Parent process handle wait | host lifetime watcher | Ends on parent death; pipe close also ends the session. |
| Host process-handle observation | parent transport worker | Nonblocking poll during ordinary operation; bounded join only through the existing exit retirement budget. |

[`window_waits.tsv`](../../../crates/bt-app/src/window_waits.tsv) gains no `# rows` exception because this design adds no owner/window-thread wait, and it gains no `# doors` admission identity for the same reason. The implementation does add vocabulary entries for `ShellExecuteExW`, named-pipe connect/read/write, and process-handle wait; `# effects` gets a `worker-door-body` entry for the launch worker with `WorkerCtx`, and `transport` entries for the pipe reader/writer and host lifetime loop with `none (transport)`. Any implementation that puts one of those effects on the window thread contradicts this note and must add and rule a numbered row before landing.

All threads use the repository's named `spawn_at_priority` road. The host's PTY calls are registered as headless session-lane effects, not smuggled through the resident's owner-token doors. The existing row 15 `PaneRetirementWait` includes outstanding local and elevated pane retirements at quit.

### Child-process census

Add `folio.exe --elevated-host …` as the twenty-fourth process kind in [`ARCHITECTURE.md`](../../ARCHITECTURE.md) §2.2 and update the exact census and its test. Its parent is the unelevated resident in the ordinary consent case, its lifetime is one pane, and its role is a private headless elevated PTY host. The shell and `--console-members` helper remain their existing kinds, now spawned from that host for an elevated pane. `--elevated-host` is also an exact early argv door in §2.1, before all resident work.

### Ownership-table additions

Add these rows to [`ARCHITECTURE.md`](../../ARCHITECTURE.md) §4 when implementing:

| Resource | Owner | Retirement |
| --- | --- | --- |
| Elevated pane state, output/input queues, host process handle, pipe workers, generation | Resident `LeafSession` → `PaneTransport::Elevated(ElevatedSession)` | Pane close/restart failure/quit; bounded transport retirement |
| ConPTY, shell child, reader/writer threads, job handle | One `elevated_host::HostSession` | Shell exit, `Shutdown`, pipe break, or parent death |
| Pipe instance and launch attempt | Resident `ElevationAttempt` until authenticated, then `ElevatedSession` | Cancel, 15-second deadline, authentication failure, or session retirement |
| Protocol version and frame codec | `bt-platform` elevation transport module | Process lifetime; pure codec has no OS owner |

## 5. UI

An elevated pane gets a shield glyph in its pane head. A tab gets the same small shield badge when any pane in that tab is elevated, waiting, canceled, or failed; the existing profile/content mark remains the primary mark. The shield's accessible name and tooltip are `Administrator`. State is never communicated by color alone, and no permanent explanatory sentence is added to chrome.

The rows that can start a program currently appear on two surfaces. The tab-strip chevron is [`profiles::ProfileMenu`](../../../crates/bt-app/src/profiles.rs): its program choices are `profiles::MenuRow::Profile`, and `profiles::layout`/`profiles::build` draw the shell and built-in agent rows returned by `ProfileTable::offered_to_start`. The pane-head menu carries `profiles::PaneMenuRow::SplitWith`; `profiles::pane_menu_layout` passes the same `ProfileTable::offered_to_start` result through `profiles::pane_submenu_layout` to `profiles::push_submenu`. The 0.4.7 switch therefore appears at the top of both the tab-strip chevron menu and the pane's `Split with` child, immediately before their first profile row.

The control's glyph is the same shield used on an elevated pane and its exact label is `Administrator`. It is a switch, not a creation row. Off is an unlit shield in ordinary menu ink with the switch thumb at the start of its track and the accessible state `Administrator, off`; on is a lit shield with the thumb at the end of its track and the accessible state `Administrator, on`. The changed thumb position and exposed on/off state carry the distinction even without color. Clicking the control, or pressing Space or Enter while it has keyboard focus, changes the state without closing the menu.

The state belongs to that open menu instance. Opening either menu creates it as off; closing it discards the state, so reopening is off even when the preceding choice was elevated. It is not a setting, is not persisted, and does not alter the durable `Run as administrator` profile option. In keyboard order the switch is the first item: entering the tab-strip menu lands on it before the first profile, and Down moves from it to that profile. Opening `Split with` by Right lands on the switch before the first child profile; Down reaches the profile list and Left/Esc returns to or closes the parent. Up/Down can return to the switch, and the switch and every program row remain operable without a pointer.

With the switch on, clicking a shell profile row consumes the open-menu state and calls the one terminal-creation action with that profile, the surface's ordinary directory and split placement, and administrator forced on. Clicking a built-in or user-created agent profile row calls that same action: the elevated host creates the row profile's shell/ConPTY under the administrator token and that shell runs the agent profile's command. The shell, the command, and the resulting agent process tree are elevated; the resident Folio window is not. Agent hooks and attention delivery make only the guarantees established by spike 4's measurements, especially when UAC uses another administrator's credentials; the guaranteed first result is the elevated agent command in a working terminal.

The switch does not affect the current chevron menu's `Files pane`, `New terminal in folder…`, or Recent rows, nor any pane-menu action outside the `Split with` child. Those rows keep their ordinary glyphs, labels, availability and behavior while the switch is on: they are neither tinted nor greyed and gain no administrator badge. The same rule applies to the 0.5 panel's Files, Preview and Browser rows. Profile and agent rows also gain no per-row administrator badge; the one visible switch is the state owner. A profile whose durable `Run as administrator` option is on still elevates when chosen with the transient switch off.

The command palette carries the exact verb `New administrator tab`. It calls the same creation action for the default profile in the directory the ordinary `New tab` verb would use, with administrator forced on. It does not expose or remember menu state, and there is no shortcut requirement in 0.4.7.

In the 0.5 `where × what` panel, `Administrator` remains the third independent choice beside the pane copy's split-direction selector, rather than becoming a badge on each Shell or Agent row. It applies to the next Shell or Agent choice in the selected folder and leaves Files, Preview and Browser unchanged; the shared panel keeps this control in the same place when shown from the tab strip, where no split direction is needed.

Settings → Profiles adds `Run as administrator` immediately after `Program` and before `Login shell`/the remaining launch controls. It is Windows-only and defaults off. Proposed English description: `Windows asks for administrator access when this profile starts.` It is declarative and fits the Profiles editor's two-line budget in the [copy guide](../copy/user-facing-copy-guide.md). Chinese is outside this ticket.

Waiting, refusal, failure, and retry use the exact body copy in §2. The restored state in §6 uses one short action. There is no new Folio dialog, toast-only error, or pre-UAC explanation.

## 6. Persistence, restore, and restart

Add `administrator: bool` with `#[serde(default, skip_serializing_if = "is_false")]` to [`TermLeafV1`](../../../crates/bt-persist/src/layout.rs) and to `RecentSeedV1::Term` in [`session.rs`](../../../crates/bt-persist/src/session.rs). Add `run_as_administrator: Option<bool>` to [`ProfileEntryV1`](../../../crates/bt-persist/src/profiles.rs), with absence meaning the shipped `false`; the app-level [`profiles::Profile`](../../../crates/bt-app/src/profiles.rs) holds the resolved bool. These are additive default-valued keys: the new reader accepts old documents and the old reader ignores the new keys, so neither `session.json` v15 nor `profiles.json` v1 needs a version step. Round-trip, future-version, default-omission, seeded-profile departure, Recent, and multi-pane tests must pin that claim.

At creation, effective elevation is `forced_by_menu_switch_or_palette || profile.run_as_administrator`. Persist the effective pane value, not a live reference to the profile setting, so later profile edits do not change an existing or restored pane. Split/duplicate carries the source pane's effective value; because it creates a second elevated pane, it prompts once for that new pane.

Restore constructs the tab, pane geometry, profile, cwd, manual name, and transcript state but never launches its elevated host during application startup. Even if it is the active restored tab, its pane shows a shield, `Administrator access required.`, and the inline action `Open as administrator`. Only that action begins `ShellExecuteExW`; merely restoring, selecting, or focusing the tab does not. A tab with several elevated panes asks separately as each pane is explicitly opened.

`Restart shell…` on a running elevated pane reuses that pane's host and therefore causes no new UAC prompt. The host ends the old job, creates a fresh ConPTY and environment, increments generation, and reports start success before the parent commits the new transcript generation. If the host no longer exists, the restart action starts a new host and UAC prompt. This is pane-scoped reuse, not an elevation cache.

## 7. Update, uninstall, and single instance

The host is the installed `folio.exe`, so [`install_flip::running_from`](../../../crates/bt-platform/src/install_flip.rs) includes it by file identity. A normal in-app restart-to-update first closes panes; each close shuts down its host. If a host remains, `UpdateInstall::ready_to_move` finds the mapped old member and the applier refuses/rolls back rather than replacing a live image. Do not special-case or kill the host from the updater.

Person-initiated uninstall already passes every same-image identity from `remove_the_program` to the remover, so it waits for an elevated host as well as the resident. The managed-package cleanup path returns to the package manager before that full program-removal census and its cleanup door only takes the data-directory claim. Before any `--uninstall-cleanup` mutation, add a `running_from(scope.exe)` check that allows the cleanup process itself and refuses while any other same-image identity, including an elevated host, remains. This closes the Scoop/Winget cleanup gap without giving the host a data-directory claim.

The host never takes [`instance::Claim`](../../../crates/bt-platform/src/instance.rs) and never opens [`LaunchPipe`](../../../crates/bt-platform/src/launch_pipe.rs). Consequently, a user launch while only an orphaned host exists may become the resident, while a user launch with a resident hands over to that resident. File identity is an update/uninstall census, not a second single-instance claim.

The zip build, Scoop, and Winget execute an ordinary external Win32 `folio.exe`; `ShellExecuteExW(runas)` can address that same path, subject to normal UAC policy and executable access. Folio must not ask users to change UAC, package, or machine settings.

The current [`packaging/msix/AppxManifest.xml`](../../../packaging/msix/AppxManifest.xml) is a sparse package with external location and `runFullTrust`, not a fully packaged application. Microsoft documents sparse identity as identity for an external-location Win32 app ([overview](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps-overview)); its capability documentation also names `allowElevation` for packaged apps that elevate ([capability declarations](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations)). The current manifest does not declare `allowElevation`. Treat elevation from the registered sparse-package state as unproven: spike the current manifest and a narrowly amended manifest before deciding whether 0.4.7 needs that capability. A future fully packaged MSIX is a separate deployment model and is not claimed supported by this design without its own spike.

## 8. Integrity-level effects

- Clipboard ownership stays in the unelevated resident. Copy reads the parent's terminal model; paste reads the clipboard after the user's gesture and sends bytes over the pipe. The host never opens the clipboard. OSC 52 remains disabled by [`RULES.md`](../../RULES.md) §9.

- File drop targets remain the unelevated Folio window. Existing parent code resolves/quotes the dropped paths and sends the resulting input bytes. No drag crosses into an elevated window, so UIPI does not remove the gesture; the elevated command can still report an ordinary access or path failure for an unavailable source.

- Links are parsed and validated from the parent's terminal model and opened only through the existing [`bt_platform::handoff`](../../../crates/bt-platform/src/handoff.rs) lane. Browser and Explorer processes therefore receive the unelevated Folio token. The host has no link-open frame.

- Pane and tab dragging, including cross-window moves, remains entirely among unelevated Folio windows. Moving a `LeafSession` moves its pipe-backed `ElevatedSession`; it does not move or recreate the host, change integrity, or prompt again. Closing the source window must not retire a moved session now owned by the destination.

## 9. Owner rulings and required spikes

### Owner rulings, 2026-10-04

1. Over-the-shoulder administrator credentials support the basic shell using the credentialed account's fresh environment. Attention integrations are promised only where spike 4 proves the parent endpoints admit the actual token; the ticket does not broaden an endpoint ACL without that evidence.
2. A restored administrator pane starts elevation only from its explicit inline `Open as administrator` action. Restore, tab selection and focus do not prompt.
3. The creation entry is the transient `Administrator` switch for a chosen shell or agent profile, not a `New administrator tab` menu row. The command-palette verb `New administrator tab` is the single default-profile form: it uses the ordinary New-tab directory and forces administrator on.
4. Split and duplicate inherit the source pane's effective elevation even when the durable profile option is off. Each new elevated pane has its own host and UAC prompt.
5. If the sparse-package spike proves `allowElevation` is required, add that narrow capability to the shipped sparse manifest. Do not ask the user to unregister Folio or change Windows settings.

There are no open owner questions in this note. The measurements below remain required implementation evidence.

### Spikes before implementation

1. **Launch matrix.** A signed test build calls `ShellExecuteExW(runas)` from an ordinary account: consent, refusal, credential entry with another administrator account, missing executable, policy denial, and a 15-second orphan. Record return codes, returned PID/handle behavior, and whether the headless host produces any shell UI.
2. **IPC identity matrix.** Parent-created protected pipe, same-account split token, standard-user/other-admin credentials, remote rejection, wrong PID, wrong capability, parent death, pipe break, and PID/start-identity mismatch. Verify the job and host disappear in every parent-loss case.
3. **ConPTY relay.** Run an elevated-host prototype at ordinary integrity and stress raw output, input, resize, output backpressure, shell exit, pipe failure, generation rollover, and restart ordering. Compare byte/transcript behavior with a local `PtySession`.
4. **Foreground and integration.** At high integrity and with other-admin credentials, exercise foreground probing, the inherited `--console-members` helper, OSC 7 cwd, PowerShell integration, and attention delivery. Record which parent endpoint ACLs admit the actual token.
5. **Packaging/update matrix.** Launch by absolute path from zip, Scoop, Winget, current sparse registration, and a test sparse manifest with `allowElevation`. While the host maps the image, prove `running_from`, update replacement refusal, person uninstall, and managed cleanup all count it. Repeat future full-MSIX work only if that package becomes a 0.4.7 requirement.

## 10. Ticket split

Each ticket below is one implementation session. Every automated test names a mutation that makes it fail. No automated test invokes `runas` or needs a UAC prompt: launch is injected for state-machine tests, and end-to-end host tests start the exact `folio.exe --elevated-host` door with ordinary `CreateProcessW`, so the production host code runs unelevated under test.

1. **Protocol and pure state model.** Add frame codec, version/state validation, generation rules, size bounds, and launch-attempt state. Tests reject a changed version, kind, length, ordering, capability, and stale generation; each mutation makes one assertion red.
2. **Windows pipe and launch primitives.** Add protected unique server creation, kernel peer-PID queries, parent-process identity, injected `ShellExecuteExW` launcher, and bounded connect. Tests use ordinary helper processes and fake launcher results for success, `ERROR_CANCELLED`, other errors, wrong peer, wrong capability, and timeout. Mutating any ACL/identity/deadline decision goes red.
3. **Local transport extraction.** Introduce `PaneTransport::Local` and mechanically route input, output, resize, exit, reset-modes transport, and retirement without behavior change. Existing PTY tests plus focused dispatch tests fail if any call bypasses the enum; mutation tests replace one routed operation at a time.
4. **Headless host and remote transport.** Add the early `--elevated-host` door, `HostSession`, job ownership, protocol loop, and `PaneTransport::Elevated`. Integration tests start that exact host normally, run a deterministic child, round-trip bytes/resize/exit, break each side of the pipe, kill the parent fixture, and restart generations. Mutations that omit shutdown, job close, or stale-frame rejection go red.
5. **Creation UI and profile option.** Add the shield badge/accessibility, the first-focus `Administrator` switch to `ProfileMenu` and the pane's `Split with` child, the `New administrator tab` palette verb, the Profiles row, exact waiting/canceled/failed copy, and one shared creation action. With an injected launcher, tests use no UAC prompt and go red if a menu opens with the switch on, pointer or keyboard cannot toggle it, on/off differs only by color, a shell or agent row fails to pass the switch's value to the shared action, an unaffected row changes, the pane and tab surfaces diverge, the palette does not force the default profile, the shield is absent, retry does not create a new attempt, failure falls back to local, or the description exceeds its measured two-line budget.
6. **Persistence and dormant restore.** Add pane/Recent/profile fields and conversions. Round-trip and fixture tests fail if defaults are not omitted, old documents stop reading, older-shape output changes, effective elevation tracks a later profile edit, or restore calls the launcher before `Open as administrator`.
7. **Restart and local-child assumptions.** Move foreground probe, child/exit state, close/quit, console-members, cwd, and reset-modes through transport; implement live-host restart. Tests use the unelevated host and fakes to make output-before-boundary, stale input, a second launch on live-host restart, wrong cwd/profile, or a host-loss restart without a new explicit attempt go red. Update M2 §1.7 and DESIGN §7.1.7 item 3 in this ticket.
8. **Census, updater, uninstall, and registries.** Add the host to the argv/process census, ownership table, wait/effect registry, same-image tests, and managed-cleanup guard. Tests hold the executable through an unelevated host and fail if update/uninstall/cleanup ignores it or if the host takes the resident claim.
9. **Manual Windows and packaging acceptance.** Run the five spikes on supported Windows versions and shipping channels, record exact evidence, and make only the manifest decision supported by the sparse-package result. This ticket's automated regression tests still use ordinary host launch; its signed UAC matrix is a manual acceptance record, never a CI test.

## 11. Revision (b) — 2026-10-04: the entry is a switch

This revision incorporates the owner's 2026-10-04 rulings and supersedes every earlier reference in this note to a `New administrator tab` row in a menu. In 0.4.7, `Administrator` is transient open-menu state, initially off, at the head of both current surfaces that list startable profiles: `profiles::ProfileMenu` and the `profiles::PaneMenuRow::SplitWith` submenu. A shell or agent profile chosen while it is on goes through the one elevated creation action; other rows do not acquire elevation or administrator decoration. The palette retains `New administrator tab` as the direct default-profile verb, and the durable per-profile option remains separate.

The same switch carries into the 0.5 `where × what` panel as an independent choice beside split direction, applying to the next Shell or Agent selection and not to Files, Preview or Browser. An elevated agent means its command is run by the selected row profile's elevated shell, so that shell and the agent process tree—not the resident Folio UI—hold the administrator token; hooks and attention behavior follow spike 4's measured result.

Owner questions 1–5 are ruled as recorded in §9. The process model, authenticated pipe, one-host-per-pane lifetime, persistence, dormant restore, update/uninstall census and UAC-free automated-test strategy in §§1–4 and §§6–10 otherwise stand.
