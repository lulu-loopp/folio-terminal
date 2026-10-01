# Folio — the architecture

This file is the shape of the program: what runs, who owns what, where work is
allowed to happen, and which of those are facts about today and which are
decisions about tomorrow. It is written to be a ticket's first read, so it is
short on purpose.

Three documents, three jobs:

- **`docs/ARCHITECTURE.md`** (this file) — the structure, as it **is** and as it
  is **ruled to become**. Every sentence names a crate, module, type or
  function. A sentence that describes a decision not yet built says so.
- **`docs/RULES.md`** — the rules **in force**, by subsystem.
- **`docs/DESIGN.md`** — the **history** of how those rules were decided. It is
  append-only and it is evidence, not instruction.

Nothing here carries a line number. `crates/bt-app/src/main.rs` is being split;
a line number written today is wrong within the week.

---

## 0. The map

Two pictures of what the sections below say in prose. They are drawn by hand
from the code and from this file, so a commit that changes a lane, a thread, a
door or a ruling redraws them in the same commit (`docs/architecture/PROVENANCE.md`).

- **[Today](architecture/today.svg)** — every production thread by name, the lane
  it serves and the `AppEvent` it answers with; the processes that talk to
  Folio, the children it starts, the owners as §4.1 finds them, and the crates.
- **[After the ruled migration](architecture/after-the-ruled-migration.svg)** —
  the same frame after §5.4 steps 2–5, §4.1 and §12, each move carrying the
  version this file rules for it; what this file leaves open says *not ruled*.

### 0.1 The census behind them

Counted on 2026-09-23 at `b6ca4329`, product code only: `#[cfg(test)]` and
`#[cfg(all(test, …))]` items, `tests/`, `src/bin/`, `*tests.rs` files and
`build.rs` are left out. The thread rows were recounted on 2026-09-26 at
`78a3699a` (the thread-door note, §5), and no spawn site changed up to `5effa23b`: one site more, `taskbar_lane`'s
`taskbar-state` (ticket 62). One more since, found by A1b on `4a8a8f3f`: `install_channel::begin`'s
`bt-install-channel` (ticket U-1), through the door. A1c (on `f2b31952`) moved the eighteen bare sites of `bt-app`
and `bt-platform` through the door without adding or removing one. And one more after it: `update_trial::begin_watch`'s
`folio-trial-watch` (0.4.6 U-13), through the door, started only in an update's trial. `crates/bt-platform/src/lib.rs` holds a NUL byte, so
ripgrep skips it as binary; search it with `grep -a`. To re-count, grep the patterns in the last column and
drop the test items; a number that moves edits this table and the pictures.

| what | count | pattern |
|---|---|---|
| thread-spawn sites | **50** — `bt-app` 33, `bt-platform` 13, `bt-pty` 4 — plus **one** rayon pool, `bt-term::inline_image::resample_pool` (`bt-image-resample-{index}`) | `spawn_at_priority(_with_stack)?\(`, `thread::spawn\(`, `thread::Builder::new\(\)`, `ThreadPoolBuilder::new\(\)` |
| through the thread door | **46** — `bt-app` 33, `bt-platform` 13; each is a `Worker` by its name and its body is lent a `WorkerCtx` (A1b, A1c; U-13's `folio-trial-watch`, T-KEYBOARD-CTRLALT's `folio-layout-tables`, and T-UNINSTALL-UX's `folio-remover-ready` joined). A source guard holds both crates to it (the assertion `every_thread_bt_app_and_bt_platform_start_comes_through_the_thread_door` of `hang_watch::window_waits_tests::every_door_is_where_the_registry_says`, since A1e) | `spawn_at_priority` |
| bare spawns | **4**, all in `bt-pty` and `Unset` by design: the reader, the writer, the dump publisher (unnamed) and `pty-retirement` | `thread::spawn\(`, `thread::Builder::new\(\)` |
| of those through the door, in a door process rather than the window process | **4**: `folio-attention-stdin` (`attention_wire::payload_on_stdin`), `folio-explorer-removal` (`explorer_menu::remove_from_explorer_menu`), `folio-explorer-cleanup` (`explorer_menu::cleanup_registrations`), and `folio-remover-ready` (`deferred_removal::schedule`'s readiness pipe); each process's main thread waits as a worker, entered once through `enter_standalone_main`. The uninstaller and copied-remover doors enter once for their whole runs, so their process waits and retry backoff through `wait::sleep_within` are workers' waits too | `enter_standalone_main\(` |
| named sites / distinct names | **47 / 42**, besides the pool | the first argument, or `.name(…)` |
| `spawn_blocking` | **0** — there is no async runtime | `spawn_blocking` |
| channel constructions | **34** — 28 `mpsc::channel`, 6 `mpsc::sync_channel`; `bt-app` 27 (T-KEYBOARD-CTRLALT adds the layout-table request and answer pair), `bt-platform` 7 (T-UNINSTALL-UX adds the remover readiness pipe) — and **6** `Condvar::new` (`bt-pty` 3, `bt-platform` 2, `bt-app` 1); no other channel crate | `(sync_)?channel(::<…>)?\(`, `Condvar::new\(` |
| `AppEvent` variants | **34** (T-KEYBOARD-CTRLALT added `LayoutTablesReady`; U-3 added `InstallChannelRead`, U-13 `TrialWritesReleased`, U-18 `UpdateJobOffer` and `UpdateJobProgress`, §5, §10) | `enum AppEvent` in `main.rs` |
| child-process construction | **one** `Command::new`, inside the doors `bt_platform::quiet_command` and `quiet_breakaway_command`, with **17** product callers — the existing probe, shell, Git, update, rescue and trial callers; `bt_platform::foreground_program` starts the private console-membership helper; `uninstall::leave_armed` starts `--uninstall [--remove-data] --after-pid <pid>`; and `bt_platform::deferred_removal::schedule` starts the internal native copy as `--uninstall-remove`. Both uninstall starts request `CREATE_BREAKAWAY_FROM_JOB` on Windows; a containing job that disallows breakaway makes creation fail and the caller reports failure instead of claiming a detached child exists. No command interpreter or mutable removal script is involved; besides this door, `bt-pty::PtySession::spawn`'s `spawn_command` and the one `ShellExecuteW` in `bt_platform::handoff` remain | `quiet_command(_named)?\(`, `quiet_breakaway_command\(`, `Command::new\(`, `spawn_command\(`, `ShellExecuteW\(` |
| `Runtime` methods | **1,424** — 1,223 in the 27 `runtime/*.rs` topics, 201 still in `main.rs` (§13) | a four-space-indented `fn` in an `impl Runtime<'_>` block |

---

## 1. How to read this file, and where to stop

**The reading rule: a subsystem's contract is the stopping point for a caller.**
An implementer reads this file, the contract of the subsystem the ticket names,
that subsystem's implementation, and its behavioural tests. Reading the history
that produced the contract is *evidence gathering* — it is what an investigator
does when the contract and the behaviour disagree — and it is not the price of
an ordinary change.

This exists because of one governing principle: the developers are agents with
bounded context, so **what must be read to finish one ticket must not grow with
the number of features.** Every rule below is a rule because it keeps that
true. A change that makes some other subsystem's history required reading has
broken the principle even if it ships correctly.

Consequences that are themselves rules:

- When a dated `DESIGN.md` entry overrides a rule, `docs/RULES.md` changes in
  the same commit. The dated entry is not the rule.
- A guard that outlives the understanding of its rule becomes superstition. The
  source-reading pins under `crates/bt-app/src/tests.rs`, `scripts/check-*.ps1`
  and the layer-shape tests enforce rules; the rules they enforce are stated in
  prose here or in `docs/RULES.md`, not only in the pin.

---

## 2. Processes

### 2.1 One binary, nine argv doors

The only shipped binary is `folio` (`crates/bt-app/Cargo.toml`'s `[[bin]]`,
`crates/bt-app/src/main.rs`). `bt-record`, `bt-replay`, `bt-zoom-perf`,
`render-info-plist`, `bt-conpty-width-probe` and `bt-repaint-oracle` are
development tools and are not packaged.

`fn main` checks nine argv doors in a fixed order, each headless and each ending
in `process::exit`; everything else is grammar for the ordinary window launch.

| door | parser | handler |
|---|---|---|
| `--console-members <shell-pid>` — exact private word, checked first | `cli::console_members` | one standalone worker main calls `bt_platform::foreground_program::write_console_members`, then exits; Windows only, no window or resident-process console attachment |
| `--uninstall-remove` — the exact internal, undocumented word accepted by a copied executable, checked second before console adoption and every path that can construct a window; exact grammar is routing, not authentication of the caller or inherited plan | `cli::uninstall_remove` | `bt_platform::deferred_removal::run_from_environment`, on the remover's standalone main; its waits and retries run on that worker, and final failure writes `result.txt` and raises `standalone_alert` |
| `--uninstall-cleanup [--purge]` (the package managers' hook: the cleanup, in English) and `--uninstall [--remove-data] [--after-pid <pid>]` (a person's uninstall, 0.4.7 T-UNINSTALL-UX: with `--after-pid`, the asker's end waited for first; the cleanup in the settings' language; then, after a cleanup that completed, the program's own files handed to `bt_platform::deferred_removal`) — checked third after the two private doors; the two public verbs' words never share a line | `cli::uninstall_cleanup` → `cli::UninstallDoor` | `uninstall::run`, on one standalone main (`uninstall::standalone`) |
| `attention <family>:<event>` — checked fourth, above the panic hook | `cli::attention` | `attention_wire::run_verb` |
| `--explorer-command` | `cli::explorer_command` | `explorer_menu::serve` |
| `--remove-shell-integration` | `cli::remove_shell_integration` | `shell_integration::remove_shell_integration` |
| `--remove-explorer-menu` | `cli::remove_explorer_menu` | `explorer_menu::remove_from_explorer_menu` |
| `--update-recover [<home>] [--from-trial <pid>:<started>:<ready\|unready> \| --then-launch <argument>…]` (the home: named by the macOS LaunchAgent, derived from the rescue build's own path on Windows, U-26), `--update-apply <home> <txn> <nonce>` (U-28: the home named as `--update-recover` names it) — the rescue build's, first word only | `cli::update_door` | `--update-apply`: on macOS `update_apply_macos::run_here` (U-28), on Windows `update_apply_windows::run_here` (U-23), each main thread entering through `enter_standalone_main`; elsewhere one line. `--update-recover`: `update_recover::run_here` (U-22) — the home and the installed program from its own path, one read of the journal's frozen header, one line on stderr and appended to the data directory's `diagnostics.log` (else `recover.log` in the home), and with `--then-launch` the installed `folio.exe` started with the handed arguments unless the class is `destructive`. Since U-29 its main thread enters through `enter_standalone_main`, and over a macOS bundle whose header is `destructive` with a decided outcome it holds the transaction lock and performs M9–M11 as R (`update_apply_macos::recover`: the trial stopped, the swap back, `RolledBack` or `Stuck`, the retirement; `Committed`'s retirement too), every line also appended to the log; a `rolled_back` outcome then starts the installed build with `--update-failed <journal>` before the handed arguments, past a `destructive` class too, and at login after a rollback it finished. Since U-29b it recovers a `destructive` header of **any** outcome — every phase a dead applier can leave (`Handoff`/`Armed` back to `Prepared`, `Moving` decided by the live identity and by a trial it starts, `Trial` waited for and committed or rolled back) — and then starts exactly one build (`update_apply_macos::Recovered::opens`): the new build before `Committed` only as a trial (`--update-trial`), the old one with `--update-failed` while the header is `destructive`, the live one by that rule when recovery itself fails; on macOS the ordinary start's own program runs this door when the rescue clone cannot be started. **On Windows since U-23**, on the same standalone main, the recovery of what a dead applier left, by the applier's own code (`update_apply_windows::recover`), every line also appended to the log; **since U-24** each step `update_txn::decide`'s answer for `Asker::Rescue` — `Handoff`/`Armed` → `Prepared`, `Moving` and an unanswered `Trial` rolled back from what is on disk, `RollbackIntent`/`Stuck`/`RolledBack` rolled back and retired (W9–W11: the trial stopped by pid, creation time and image, the new files to `rolledout\`, the old ones back from `backup\`, `RolledBack`, the `Run` value removed, `Retired{RolledBack}`; a failure `Stuck`, three attempts at most), `Committed` retired — and then exactly one start by U-29b's rules (`update_apply::Opens`): the old build plainly, or with `--update-failed <journal>` after a rollback or while `destructive`; the new set before `Committed` only as a trial (over a `Stuck` with the new set installed, one it starts, records as `RetrialBegan` and waits for); the rescue copy with `--update-failed` only where the install holds neither whole set (the fallback; U-23's rescue-copy opening for every `destructive` journal is retired). **On both platforms a `Handoff` while a process of the rescue executable that started no later than R runs is left to it** (`update_apply::an_earlier_holder`): nothing written, nothing waited for (U-23's 180 s wait is removed), nothing started. **Since U-34 every way out of both doors leaves through one exit guard** (`update_apply::ExitGuard`, a `Drop`; the doors' unwinding panic hook is installed on the first word, before the line is parsed; the recovery build's first statement is an outer guard): a successor still running by pid and start instant — the trial it started, the window mark's holder found at `Handoff` — opens Folio, otherwise one start of what the disk names (the rules above), counted only when a Folio acknowledges it by holding the data directory within 20 s (`update_apply::claimed_within`), else the next program (on Windows the rescue copy), else the failure window shown by the process itself; who has the duty to open is one mark, `H\<txn>\owner`, taken by the applier before it waits for the old build's lock and by the old build at its end, never inferred from who is alive (`update_apply::OWNER_FILE`), and the recovery build leaves a `Handoff` only to the mark's live holder; the one exception is the run at logon that did nothing a person is owed a window for. The applier retries a journal write refused because another program holds `journal.json` open for about 2 s (`update_apply::write_journal`). A malformed line, and `--update-apply` where there is no applier: one line from `cli::update_door_refusal`, exit 2 |
| `--help` / `--version` / any parse fault | `cli::parse` | `report_at_the_front_door` |

**The update's frozen words** (`docs/plans/design/self-update-2026-09-16.md`
(b).1 F-8; v1 from 0.4.6, read by builds other than the one that wrote them;
none is in the usage block):

| word | where | written by | read by |
|---|---|---|---|
| `--update-trial <txn> <nonce>` — exact, two values, no `=` form | the ordinary grammar: `cli::parse` → `CliRequest::update_trial`, the values kept as text | the applier, starting the new build as the trial | `update_startup::pass`, which makes it this start's trial only when the journal's header names that transaction |
| `--update-recover` | the rescue door above; spelled once, as `bt_platform::logon_hook::RECOVER_FLAG` | the logon entrance (`logon_hook::command`, U-22), and `--then-launch`'s writer | the rescue build (`update_recover`, U-22) |
| `--from-trial <pid>:<started>:<ready\|unready>` — after `--update-recover` (and its home) only, last, and never with `--then-launch`: the line is logon-shaped (0.4.7 U-37, design revision (h) H.4; the mixed line is refused) | `cli::update_door` (`UpdateDoor::Recover::handed_back`, `cli::HandedBack`) | a trial's watch handing its transaction back (`update_trial::hand_back`), only to a rescue build of 0.4.7 or later — an older one refuses the line | the rescue build, which ends that exact instance when it is unready, and counts it a candidate when it is ready (H.3) |
| `--then-launch <argument>…` — after `--update-recover` only, and last; everything after it is another start's command line, verbatim | `cli::update_door` (`UpdateDoor::Recover::then_launch`); in the ordinary grammar it is an unknown flag | an ordinary start that meets a destructive transaction (`cli::recover_command_line`, from `update_startup`; on macOS with the home named before it, U-29) | the rescue build, which starts the installed `folio.exe` with those arguments once the transaction is finished, so that start never sees the word (U-22) |
| `--update-failed <journal>` — exact, one value, written first (U-29) | the ordinary grammar: `cli::parse` → `CliRequest::update_failed`, kept as a path | the macOS applier after a rollback, finished or not (`update_apply::failed_words`, through `open -n -a`), and the rescue build before a handed command line whenever the header's outcome is `rolled_back`; **on Windows since U-24** the applier and the recovery after a rollback, finished or not (`<install>\folio.exe --update-failed <journal>`), before a trial started over `Stuck`, and before the rescue copy they fall back to | `update_startup::pass`: the card from the header alone (`update_txn::after_rollback` → `update_job::Failure::RolledBack`, or `Incomplete` with the journal's folder; `Job::after_rollback`), read before a retirement removes the journal; past a `destructive` header whose outcome is `rolled_back` the start continues instead of handing itself back (`StartView::sent_by_rollback`); since U-29b past a `destructive` header of any outcome, with *Update incomplete.* (the recovery sends the word on its failure roads too), a trial's start included |
| `--update-feed <file-URL>` — exact, one value (U-30b); typed by a person rehearsing an update, never written by a build | the ordinary grammar: `cli::parse` → `CliRequest::update_feed`, kept as text; the rescue build's doors, `attention` and `--uninstall-cleanup` refuse it (their grammars have no room for it) | the person running the clean-VM checklist (`docs/plans/release/clean-vm.md` §4.4, precondition 3; `scripts/release/cleanvm/updater/guest/keys.ps1`, its `launchfeed` step) | `main`, once the log is open: `update::use_feed` — the diagnostics line `update feed: <url>`; the check reads the feed's `releases.json` (`update::Feed` through `update::check_source`) and a press copies the offer's two files (`update_job::FeedCopy` through `update_job::transport_for`), both through `file_reads` on `Lane::Update`, never github.com. For this process only: no environment variable, no setting; what it writes is only whose answer it is, `update-check.json`'s `local_stamp` and `local_tag` beside the stamp and the tag its check wrote (U-42e), which the first ordinary start (no flag) reads to forget exactly those; a hand-over to a running Folio does not carry it. The checksum and the signer checks are unchanged, so a feed delivers only a build the same signer signed |

**After the doors and the parse, the update pass** (`update_startup::pass`,
0.4.6 U-12): on the window thread in `Starting`, before `persist::storage_dir`,
settings, sidecars and `launch_wire::hand_over` (which takes the pass's
`Admitted`), a start holds its installation's `admission` shared for its
lifetime and reads the update journal's frozen header once (§5.1).

`attention` is checked fourth, above the panic-log hook and above anything that
could build a window, because the caller that matters most is an agent holding
an approval open. **One instance owns the data directory**
(`bt_platform::instance::claim_data_directory`, a named mutex or a file lock
keyed by that directory, held for the life of the process in `persist`'s claim
table; on Unix a `flock` on `<runtime>/<tag>.lock`, whose file the claim's drop
unlinks under the lock, and whose leftovers — empty, unheld, older than an hour —
each start sweeps on its `folio-claim-sweep` worker
(`instance::sweep_stale_claims`); the claim holds `sweep.guard` shared and the
sweep holds it exclusive one file at a time, so a claim never meets a file the
sweep is holding, U-43). Both guard locks are non-blocking: a sweep stops its
pass while a claim holds the guard, and a claim answers the transient
`ClaimRefusal::Sweeping` while the sweep holds it, so there is no owner-thread
wait. The table has two writers: `persist::is_writer_of`, which takes the claim
on the first ask and remembers a settled answer, but never `Sweeping`; and
`persist::adopt_claim`, which puts a claim already taken with
`persist::try_claim` (asks now, remembers nothing) into the same row before
anything asks — the updated build's road (`docs/plans/design/self-update-2026-09-16.md`
§C.7, R-3). A process that never has a window never writes the table (§4.2, *who may
take the data directory's claim*). A second process hands its argv down the launch pipe
(`launch_wire::hand_over`) and leaves through `bt_platform::leave_process`.

### 2.2 The twenty-three kinds of child process

| kind | started by | note |
|---|---|---|
| `folio.exe --console-members <shell-pid>` | `bt_platform::foreground_program::foreground_program`, through `quiet_command`, on `bt-foreground-program-worker` | Windows; waited for on that worker within the foreground observation's five-second bound; attaches only to the named pane shell's console, prints its member pids, detaches, and exits; failure is non-zero with empty stdout |
| `OpenConsole.exe`, the ConPTY host | the DLL's `ConptyCreatePseudoConsole`; extracted at build time by `crates/bt-pty/build.rs` | Folio never spawns it and holds no handle to it |
| the pane child — the shell or agent | `PtySession::spawn` → `CreateProcessW` with the pseudoconsole attribute | program chosen by `bt_pty::shell::resolve_default_shell` |
| the pane's grandchildren | whatever the shell leaves running | held in an unnamed job object by `Job::holding`, so closing the pane kills them |
| WebView2 browser, renderer, GPU, utility | the Edge runtime, from the one `CreateCoreWebView2EnvironmentWithOptions` (`bt_platform::webview::create_environment`), asked for by a page's `WebHost::request_environment` or, once per process on an idle turn after startup, by `bt_platform::warm_web_environment` (ticket 54) | nothing in that module blocks; that is its stated contract; at most one creation call in flight (`bt_platform::EnvironmentSlot`) |
| `com.apple.WebKit.WebContent` | `bt_platform::macos_webview::WebHost::request_controller` | main thread only, one per seat, never pooled |
| `folio.exe --from-explorer --cwd <folder>` | Folio's own COM server, in `explorer_menu::serve` | detached, never reaped — the one deliberately orphaned child in production |
| the rescue build, `<H>\<txn>\rescue\folio.exe --update-recover --then-launch <argv>` (macOS: the rescue clone's executable, with the home named after `--update-recover` since U-29; when it cannot be started, the start's own program with the same line since U-29b) | `update_startup`, through `quiet_command`, when a start meets a destructive update transaction (0.4.6 U-12) | detached, never waited on: the start that started it leaves at once; inert until a build writes such a transaction |
| the applier, `<H>\<txn>\rescue\folio.exe --update-apply <home> <txn> <nonce>` (macOS: the rescue clone's executable; the home named since U-28) | the quit's way out, on the storage worker (`update_handoff::perform`, through `quiet_command` by the absolute path the journal's header names), after `Handoff{nonce}` is durable and only after the quit's session landed (0.4.6 U-21) | detached, never waited on: O leaves by the ordinary path, still holding the transaction lock until it exits; a start that fails journals `Abandoned` and O leaves anyway; the `--update-apply` grammar is U-28's, for both platforms (on Windows its door is U-23's); inert until offers are on (U-31/U-32) |
| the installed `folio.exe` with the arguments an ordinary start handed over (after a rollback on macOS, `--update-failed <journal>` first; at login, that word alone once a rollback finished, U-29; since U-29b on macOS exactly one start after any recovery — the new build before `Committed` only after `--update-trial <txn> <nonce>`, the word whenever the header is still `destructive`) | `update_recover::run`, through `quiet_command`, when `--update-recover --then-launch` meets a transaction an ordinary start would not hand back (0.4.6 U-22) | detached, never waited on; never while the class is `destructive`, so it cannot hand itself back. Since U-23 also the Windows applier's start of the old build again, with no argument, after a revert (`update_apply_windows::apply`); since U-24 the Windows applier's and recovery's start of the old build after a rollback, with `--update-failed <journal>` first (and the handed arguments, for a person's start), by `update_apply::Opens` |
| the rescue copy, `<H>\<txn>\rescue\folio.exe --update-failed <journal>` and the arguments an ordinary start handed over — the old build, as an ordinary Folio | `update_recover::run_windows` and `update_apply_windows::apply`, through `quiet_command` — since U-24 only the fallback: the journal still `destructive` and the install folder holding neither whole set (a rollback that could not finish, or a layout that cannot be read); U-23 opened it for every still-`destructive` journal (0.4.6 U-23, U-24) | Windows; detached, never waited on; its own home (inside `rescue\`) holds no journal, so it cannot hand itself back; while it runs, `H\<txn>` cannot be deleted, which the next ordinary start retries |
| the trial, `<install>\folio.exe --update-trial <txn> <nonce>` | the Windows applier (`update_apply_windows`'s `Machine::launch_trial`, through `quiet_command` by absolute path, 0.4.6 U-23), once per transaction after the moves; since U-24 also either Windows holder over a `Stuck` whose new set is installed (`--update-trial <txn> <nonce> --update-failed <journal>` and the handed command line, recorded as `RetrialBegan`) | Windows; detached, never waited on: its pid comes from the child and its start time from `GetProcessTimes`, both recorded in `Trial` (or `Stuck.trial`); a rollback stops it only through `install_flip::ask` |
| the uninstaller, `folio.exe --uninstall [--remove-data] --after-pid <pid>` (macOS: the bundle's executable) | a Folio's very last act after *Uninstall* on the Settings card, `uninstall::leave_armed`, through `quiet_breakaway_command` by `current_exe` (0.4.7 T-UNINSTALL-UX) | detached and not reaped; it waits for the exact pid/start identity it is given (60 s) before cleanup. On Windows it asks for `CREATE_BREAKAWAY_FROM_JOB`; if the containing job disallows that, creation fails, the leaving Folio shows `standalone_alert`, and no uninstall is claimed |
| the remover — a native copy of the running executable checked as a regular, single-link file with the expected size and SHA-256, in a random account-owned directory below `%LOCALAPPDATA%\Folio\` on Windows or a mode-0700 directory below `~/Library/Application Support/Folio/` on macOS, run through the internal, undocumented `--uninstall-remove` door | `bt_platform::deferred_removal::schedule`, from `uninstall::remove_the_program` (0.4.7 T-UNINSTALL-UX rounds 2 and 6) | the scheduler waits only for a readiness-pipe acknowledgement and then returns “scheduled”; the remover outlives the door, waits at most five minutes for every `(pid,start time)` in the explicit list plus every process whose image identity is the installed `folio.exe`, retries held files with bounded backoff, and checks each named file's single-link size and SHA-256 by path immediately before each deletion attempt. Final accounting uses a fallible existence read: only confirmed absence is success, and the last error for every uninspectable remaining path is written to `result.txt` and shown through bounded `standalone_alert`. The path-based check/delete instant is not defended against a deliberately racing same-account process. It removes the install folder only if empty and its copied executable last. On Windows it also requests breakaway; a job that refuses it makes scheduling fail synchronously. If the remover is ended or power is lost, deletion can be partial and no final reporter survives |
| `powershell.exe` — the PSReadLine probe | `psreadline::run_probe` | once per process; blocks its thread with no timeout |
| `powershell.exe` — the `$PROFILE` probe | `shell_integration::run_profile_probe` | once per distinct program; the only probe with a deadline |
| `cmd.exe /c "<copilot> --version"` | `attention_copilot::run_probe` | once per process, on opening the Agents page |
| `git` | `git::git_command`, always from `bt-git-worker` | never from the window thread; one status costs three threads |
| `explorer.exe`, the registered handler, Finder | `bt_platform::handoff` | fully detached: no handle, no wait, no kill |
| `defaults read -g AppleLocale`, `locale -a` | `bt_platform::read_system_locale_declaration` | macOS; memoised, so twice per process |
| `/usr/bin/codesign --verify` / `-d -r-`, `/usr/sbin/spctl --status` / `--assess` | `bt_platform::macos_identity` (0.4.6 U-16), for the updater's check of a copied bundle | macOS; worker only; each under a deadline (10 s to 120 s) and ended by its own handle past it; called by the macOS Prepare (`update_prepare_macos::System::verify`, U-27) on the `bt-update-job` worker, for the mounted bundle and again for its copy in `stage/`; inert until offers are on (U-32) |
| `/usr/bin/codesign --display` / `--verify`, `/usr/bin/ditto` | `bt_platform::macos_update::rescue_clone`, through `quiet_command` by absolute path (0.4.6 U-26) | macOS, worker only; each waited for within a deadline (60 s signing, 120 s copy) and ended by the pid it was started with if it overruns; `ditto` only where `clonefile` cannot clone; since U-27 also `macos_update::copy_bundle` (`ditto <mounted bundle> <H>/<txn>/stage/<Bundle>.app`, 120 s) and `code_identity` (`codesign --display`); both called by the macOS Prepare on the `bt-update-job` worker |
| `/usr/bin/hdiutil attach -nobrowse -readonly -noautoopen -mountrandom <H>/<txn>/mnt <image>`, `hdiutil detach [-force] <mount point>` | `bt_platform::macos_update::{attach, detach, detach_all_under, with_image}`, through `quiet_command` by absolute path, both paths resolved to absolute ones (0.4.6 U-17) | macOS, on the update job's worker only (each takes `&WorkerCtx`); attach within 120 s, detach within 30 s, each ended by the pid it was started with if it overruns; a detach refused as busy (exit 16) waits 2 s and forces once; every refused attach first detaches what the mount table lists under its mount directory; called by the macOS Prepare (`with_image`, U-27) and, for `detach_all_under`, by every road that deletes `H/<txn>` or the home: the Prepare's own abandonment and the job owner's sweep (`update_prepare_macos::clear`), a start's retirement or discard (on a `bt-update-sweep` worker the start does not wait for), and `--uninstall-cleanup`'s home row (a `folio-update-home-detach` worker it waits for) |
| `/usr/bin/open -n -a <parent>/<Bundle>.app --args --update-trial <txn> <nonce>` — the trial, through LaunchServices; and since U-29 the same `open -n -a` of the installed bundle again after a rollback (`--args --update-failed <journal>`) or a revert (no arguments) | the macOS applier (`update_apply_macos`'s `Machine::launch_trial`, through `quiet_command` by absolute path, 0.4.6 U-28), once per transaction after the exchange; since U-29b also the rescue build as recovery (`update_recover`'s `Machine::launch_trial`) over an exchange a dead applier left with the new bundle live, and either holder over a `Stuck` whose new bundle is live (`--args --update-trial <txn> <nonce> --update-failed <journal>` and the handed command line) | macOS; the relaunches are detached and never waited on; **the trial's launch is `open -n -W -a` since 0.4.7 U-38** (`update_apply_macos::open_trial`, held as a `Launch`): `open -W` returns when the application it opened ends, so a launch that exited with nothing seen ends the applier's wait at once; one ended by a signal is unknown and dropped (design revision (h) H.5); the helper is ended by its own handle when the watch lets it go — an applier ended from outside leaves it waiting until that application ends; the trial's own pid is then found by the process list (`bt_platform::install_flip::running_from`) |
| the rescue build, `<H>\<txn>\rescue\folio.exe --update-recover --from-trial <pid>:<started>:<ready\|unready>` (macOS: the rescue clone's executable, with the home named after `--update-recover`) | a trial's watch, `update_trial::hand_back`, through `quiet_command` on `folio-trial-watch` (0.4.7 U-37, design revision (h) H.2 and H.4) — at 102, 204, 408 and 816 s of an undecided transaction, never while the recovery it started before still runs, only to a rescue build whose version (`VERSIONINFO`; `CFBundleShortVersionString`) is 0.4.7 or later | detached, never waited on; the trial never ends itself — the recovery that holds the transaction ends an unready one by its pid and start instant |
| `/usr/bin/plutil -extract CFBundleShortVersionString raw -o - <bundle>/Contents/Info.plist` | `bt_platform::macos_update::short_version`, through `quiet_command` by absolute path (0.4.6 U-27) | macOS, worker only (`&WorkerCtx`); within 10 s, ended by the pid it was started with if it overruns; the macOS Prepare's version check of the mounted bundle, its copy, and the running bundle; since 0.4.7 U-37 also `update_trial::rescue_version`, on the trial's `folio-trial-watch` worker, reading the rescue clone's version before a hand-back (H.4 A2) |

Children reached through `bt_platform::quiet_command_named` are named by an
absolute path resolved by `handoff::program_on_path`, never a bare name, so a
program sitting in the working directory can never run.

**Not a child: the Windows identity check.** `bt_platform::trust` (0.4.6 U-15)
asks `WinVerifyTrust`, the time-stamp and chain calls and the package reader in
process; unlike the macOS check (`codesign`, `spctl` above) it starts nothing.

### 2.3 What is allowed to be orphaned

Each is a deliberate decision with a good local reason; stated together they are
a policy. The console host is outside the job object on purpose; grandchildren
are orphaned when `Job::holding` fails; the PTY writer thread is dropped and
never joined; a PTY reader past `READER_EXIT_BUDGET` is detached; teardowns
still running past `bt_pty::wait_for_retirements` are left running; the
`--from-explorer` child is never waited on; a browser that misses its exit
notification is ended by `leave_process`. **Anything allowed to outlive its
owner is counted by a ledger** — `Retirements::outstanding`, `Readers::inside`,
the video engines' outstanding count — so that "this may never come back" is a
number rather than a silence.

---

## 3. Crates and the layering rule

### 3.1 The graph

Seventeen first-party crates under `crates/`, plus `vendor/alacritty_terminal`.
Normal and target-specific edges as the manifests declare them (2026-09-23;
`bt-workbench` 2026-09-25):

```
bt-unicode      ← bt-transcript, bt-platform, bt-viewport, bt-render, bt-detect
bt-transcript   ← bt-doc, bt-detect, bt-viewport, bt-render, bt-term, bt-pty,
                  bt-platform
bt-doc          ← bt-detect, bt-viewport, bt-render, bt-term, bt-math
bt-layout       ← bt-workbench, bt-app (itself: no dependencies at all; pure solver)
bt-workbench    ← bt-app (itself: bt-layout only — §3.3's shrink-only exception)
bt-platform     ← bt-persist, bt-math, bt-render, bt-term, bt-app
bt-viewport     ← bt-render, bt-term
bt-detect       ← bt-term
bt-math         ← bt-term
bt-term         ← bt-app (bt-pty only as a dev-dependency, since 2026-09-21)
bt-pty          ← bt-app
bt-app          ← (nothing; the top)
```

`bt-winres` ← `bt-app` for SHA-256 alone (`bt_winres::digest`, the update pass's
image check, 0.4.6 U-12; it is also a build dependency, which is what it is for
everywhere else). `bt-corpus` is a tool; `bt-source` is read by tests only
(a dev-dependency of `bt-app`, `bt-layout` and `bt-render`). `bt-app` is the only crate that may ask
what platform it is on, and only in the files named by
`FILES_THAT_MAY_NAME_A_PLATFORM` in `main.rs`.

### 3.2 The three questioned edges, and their disposition

**`bt-pty → bt-term` — hygiene, not an inverted layer.** Production `bt-pty`
never names `bt_term`; the uses are `#[cfg(test)]` oracles and the development
binary `crates/bt-pty/src/bin/bt-conpty-width-probe.rs`. It cannot simply be
demoted, because a `src/bin/` target links against the package's *normal*
dependencies and deleting the edge leaves a broken target.
`docs/plans/bt-app-split-prep.md` §8.1 lists three faithful alternatives — move
the probe into `bt-corpus`, which already depends on both; make the need a
feature; or accept and record the edge — and rules the choice a ticket of its
own, outside the preparation and outside the relocation commit. **Done
2026-09-21** (`21cf1ef8`): the probe lives in `bt-corpus`, and `bt-pty`'s
manifest names `bt-term` only under `[dev-dependencies]`.

**`bt-term → bt-platform` — right direction, broader than its manifest says.**
The manifest comment calls it one call; there are three product import surfaces:
`inline_image::resample_pool` sets a thread priority, `session::verify_path`
calls `handoff::resolved_for_a_door`, and
`inline_image::read_and_decode_local_image` goes through the read ledger. The
ruled repair is to **extract a small headless observation/effect boundary**
rather than reorganise `bt-platform`; lifting `file_reads` alone does not remove
the edge. The manifest comment is a documentation defect fixed with the boundary.

**`bt-term → bt-math` — real coupling, recorded debt.** `session.rs` imports six
math types and calls into the math crate in product code,
`inline_image::decode_svg_bytes` rasterises through it,
`crates/bt-term/src/lib.rs` re-exports the engine, and
`crates/bt-term/src/bin/bt-repaint-oracle.rs` uses it in a binary target — the
same target trap. Hiding it behind re-exports changes nothing. **Recorded as
debt** until the composition layer is designed.

### 3.3 The dependency policy — this file is now its address

`docs/DESIGN.md`'s `## 8` heading was overwritten by commit `3d46a3e7`; its
one-paragraph body now dangles at the end of §7 and describes the vendored
terminal seam rather than the bar. Sixteen comment lines in four manifests —
the workspace `Cargo.toml`, `bt-app`, `bt-platform` and `bt-winres` — cite
"`docs/DESIGN.md` §8's bar" for a rule that has had no address since. The rule
those manifests actually practise, restated here from what they say:

1. **A dependency is a line somebody reads** — one in `Cargo.lock` and one in
   `THIRD-PARTY-NOTICES.md`. That line is the cost a new edge is weighed against.
2. **A dependency has to be worth more than the code it replaces.** `bt-winres`
   exists because two documented layouts of about two hundred lines were worth
   less than six packages and a build that has to find `rc.exe`.
3. **"No new package" is the strongest form of the bar** — naming a crate
   already in the lock file transitively costs nothing new, and that is the
   ground several dependencies were admitted on.
4. **Every version is pinned exactly**, with `default-features = false` and a
   named feature set, so that what is compiled is what somebody chose.
5. **The vendored terminal is a member, not a patch.** It stays in
   `workspace.members` at an exact version, checked by `cargo metadata` rather
   than by grep, because its upstream tests are the regression line for the
   patches.

**The layering rule, restated from what enforces it:**

- **`scripts/check-portable-core.ps1`** — fifteen named crates (`bt-source`,
  `bt-unicode`, `bt-doc`, `bt-detect`, `bt-layout`, `bt-persist`, `bt-winres`,
  `bt-math`, `bt-transcript`, `bt-viewport`, `bt-render`, `bt-term`, `bt-pty`,
  `bt-corpus`, `bt-workbench`) name no Win32 outside a `#[cfg(windows)]` gate. Platform-specific code lives
  behind `bt-platform`'s interface. This is the cheap local substitute for a
  non-Windows compile; CI proves the same property by compiling on macOS and
  Linux. `bt_app::platform_gate_tests` alone owns the separate rule that only
  the files in `FILES_THAT_MAY_NAME_A_PLATFORM` may select a platform.
- **`scripts/check-adapter-boundary.ps1`** — `crates/bt-term/src/adapter.rs` and
  `cell_capture.rs` may not name `bt_doc`, `bt_detect` or `bt_viewport`. The
  vendor seam answers "what did the terminal do", never "what shall we do
  about it".
- **Adding an edge edits this file.** A direction guard over
  `cargo metadata --no-deps --locked --offline`, reading normal and build
  dependencies including target-specific tables, with an exception set compared
  against the merge base so it can only shrink, is planned by
  `docs/plans/bt-app-split-prep.md` §8.4. Until it lands, the graph in §3.1 is
  the list.
- **`bt-workbench`'s entry, for that guard** (D-27 lands it; census-3 wrote it
  here because the guard does not exist yet —
  `docs/plans/design/ownership-census-2026-09-25.md` §5.4):

  ```
  bt-workbench   normal: bt-layout (exception: `Site.seat: SeatId`; shrink-only;
                         goes when `Site` names a session — D-1, 0.4.7)
                 build: none      dev: none
                 dependents: bt-app only
                 never: bt-app, bt-platform, bt-render, bt-term, bt-pty, winit,
                        any platform crate
  ```

  Tests that join the ledger to `bt-term`'s parser therefore live in `bt-app`
  (`tests::the_bytes_of_a_standing_request_become_one_episode_charged_to_the_osc_lane`
  and its two neighbours), not beside the ledger.

---

## 4. Ownership

### 4.1 Three owners — the ruled direction

**A session survives the disappearance or replacement of a view.** Today it does
not: `bt-app::main::LeafSession` holds process identity, launch profile, the
terminal session and the attention ledger *and* the viewport projection, the
fade clocks, `last_presented_frame` and `frame_image_references`.
`bt-app::main::create_leaf_session` resolves the shell, mints its capability,
starts the PTY, creates the `DualPlaneSession`, reads renderer metrics and
builds a viewport projection in one function, so a headless backend cannot reuse
the entry without answering questions that belong to a renderer.

The ruled split, preserving the existing implementations:

| owner | owns | today's anchor |
|---|---|---|
| **Session** | stable session identity and incarnation; PTY lifecycle; the actual launch profile, program, directory and namespace; the terminal parser and transcript; attention credentials, episodes, ordering and expiry | `bt-app::main::LeafSession`, `bt-term::session::DualPlaneSession`, `bt-pty::PtySession` |
| **Document** | editable content, revision, undo state, encoding, dirty status, disk baseline, recovery obligations | `bt-app::preview::PreviewBuffer` — already recognisably this |
| **View** | selection, scrolling, focus, layout, native resources, render caches, the last successfully presented picture | `bt-app::main::PreviewPane` — already closer to this than `LeafSession` is |

A view's inputs to a session are explicit: input commands, resize proposals,
seen observations, and actions that answer an attention request. **A view
disappearing must not implicitly mean the session ended.**

The entry that replaces `create_leaf_session` accepts these narrower objects.
If it accepts another wrapper holding `&mut App` and `&mut WindowRuntime`, the
must-read set has acquired a new name and nothing else.

### 4.2 The five ownership classes, one rule each

The process/thread survey lists twenty-two facts with more than one owner across
threads. They are **five different problems**, and treating them as one would
remove several sound designs. Each class has one rule.

| class | the one rule | anchors |
|---|---|---|
| **Observations of external state** | One service owns the accepted observation and its refresh policy, and nobody mistakes it for timeless external truth. | `DualPlaneSession::ask_about_reprinted_path` (a "yes" is deliberately retained) and `re_ask_about_link_target`; `profiles::title`'s cache; `psreadline::Probe` versus `installed_copy`, which answer different questions and are not two copies of one |
| **Asynchronous publication and competing operations** | The target's owner allocates the operation identity and alone accepts a result against the current request; supersession is checked before one result can displace another. | `schemes::rescan`, `settings::MonospaceFamilySlot`, `explorer_menu::run_request`/`finish_job`, `profile_runtime::begin_enable`/`begin_removal` — four independent implementations. Standardise the contract; keep the differing operation policies |
| **Durability and external transactions** | Desired state, submitted state and durably acknowledged state are separately named facts, and one transaction owner advances each resource. | `persist::SessionWriter`, which already distinguishes sent from landed; `quit::Quit`, the quit transaction, whose two triggers are a person asking and the update's Restart (`quit::Reason`, U-21) and whose update's quit is answered only by the receipt for its own generation (`SessionStore::landing_of`); `profile_runtime::install_recorded`, which holds the marks lock across read, change and write; `update::begin`; `update_job::Job` (U-18), the one update job per process, whose immutable offer carries the transaction it becomes and whose progress is accepted only under that transaction; `update_prepare_macos` (U-27) and `update_prepare_windows` (U-20), the job's two drivers, which journal `Allocated` before they acquire anything, `Prepared` only after the staged copy is checked again where it lies, and clear whatever they gave up in the protocol's order (detach on macOS, `H/<txn>`, journal — `update_prepare::abandon`, shared) |
| **Identity, admission and lifecycle** | The lifecycle owner issues an epoch or an admission reservation; observers may advise but cannot prove future liveness. | `launch_wire::admit` and `hang_watch::window_thread_can_serve` — neither proves the next turn will happen — against the two that work, `LeafWake::rebind` and `WebMachine::on_controller` |
| **Projections, delivery and loss** | Every publication declares whether it is a latest value, a receipt, an observation or a loss-bearing stream, and the consumer may not promote it into another kind of fact. | `video::engine::Shared` (depth 1, newest wins), `file_reads::Ledger::rotate`, `attention_wire::park` (bounded, oldest dropped, counted), `trace_sink::Queue::offer` (drop on full and on contention, counted), `present_gate::PresentGate::presented` (a record of acknowledged presentation, not a rival authority over the renderer) |

Generations solve supersession. They do not solve durable writes, freshness
policy, lost messages or admission guarantees — which is why this is five rules
and not one.

**A single-owner observation born after the survey: how this copy was
installed** (ticket U-1, 2026-09-26). Owner `bt-app::install_channel`, which
derives it once at start on the `bt-install-channel` worker from three reads of
the install folder — the marker (`folio-install.json` beside `folio.exe`; the
attribute `io.github.lulu-loopp.folio.install` on the macOS bundle), scoop's
receipt (`install.json` + `manifest.json`), and the folder's owner — and holds
it in `install_channel::FACT`, never refreshed within a run. `classify` is the
one judgement: every failed read is `Channel::Unknown`. Two readers today,
both through the one accessor `install_channel::channel()`: the
`diagnostics.log` line, and `first_run` (U-3), whose Explorer row arrives on
exactly where the fact is `Managed { uninstall_hook: true }`
(`first_run::explorer_arrives_on`). The card is built on the window thread and
never waits on the worker: it waits one turn for a fact that has not landed —
the worker wakes the loop with `AppEvent::InstallChannelRead` after publishing
it — and then builds a missing fact as `Unknown`. The update job's
eligibility reads the same `FACT` (U-18, `update_job::Gathered::now`), and
waits for it rather than reading a missing fact as anything.

**The quit transaction gained a reason and a trigger: the update's Restart**
(ticket U-21, 2026-09-26; `docs/plans/design/self-update-2026-09-16.md` §C.3).
Owner `bt-app::quit::Quit`, unchanged; `quit::Reason` is `Asked` (the chord,
the menu bar's Quit, the system's quit) or `UpdateRestart { txn }`, written by
`App::restart_for_update` through the one door `App::ask_to_quit` and spent in
`FolioApp::begin_quit_if_asked` (`Quit::begin_for`). Every cancelable step runs
unchanged. What the reason changes: the write is a named generation the loop
waits for across turns (`SessionStore::hand_over_final` / `landing_of`, bounded
by `quit::UPDATE_RECEIPT_DEADLINE` as a clock on the turn, never a wait); a
Cancel, an incomplete save or a refused write gives the update up with the
quit, and a receipt that does not come in time gives up the update and not
the quit; only a landing hands the transaction on, at `QuitStep::Exit`, through
`update_handoff` on the storage worker; the job hears each answer on the window
thread (`FolioApp::deliver_the_quits_update_report`). **The readers that assumed
only a person quits**, now that a quit keeps its windows up and the loop turning
after the photograph: `launch_wire::admit`, refused from the photograph's own
arm (`launch_wire::set_admitting(false)`) and at every turn's head through
`Quit::admits_launches`, with parked requests left unopened while the document
is fixed (`FolioApp::settle_launch_requests`); and the restore card, neither up
(`Runtime::restore_card_is_up`) nor answered (`FolioApp::settle_restore_answer`)
while `Quit::document_is_frozen`, which is also the one door onto the session
document's rule (`App::record_session`) from the photograph on.

**Fact 10, the Explorer registration, gained a trigger: the start's renewal**
(ticket U-25, 2026-09-26; D-52 touched, not widened). Its writers were the
switch (`explorer_menu::request` → `run_request`) and the launch's move repair;
now every start of every copy also registers the `folio.msix` beside it again
when the registration it reads serves **this** folder (`same_path`,
canonicalised) and its version (`msix::PackageRegistration::version`) is older
than this build's four-part package version
(`explorer_menu::this_package_version`) — `explorer_menu::renewal_wanted`. It is
the same probe on the same `folio-explorer-probe` worker, under the same `BUSY`
latch as a press (a press holding it wins; the next start asks again), behind
the same trial gate (`explorer_menu::probe_at_start` asks
`update_trial::defer(Writer::ExplorerRepair)`), and it never creates or
retargets a registration. Readers that assumed the old set of triggers: the
Explorer row (`explorer_menu::row_description`), which now also shows a refused
renewal in the words a refused registration has (`renewal_line`), and the
cached `PackageState` (`state()`), which the probe writes after a renewal as it
did after a repair; `diagnostics.log` gets one `BT_EXPLORER_PACKAGE renewal:`
line per start.

**Who may take the data directory's claim** (ticket B-EXPLORER-CLAIM,
2026-09-27; class *identity, admission and lifecycle*). The claim table
(`persist::is_writer_of`, `persist::adopt_claim`) is written only by the
process that will be the resident Folio: `main`'s `is_writer_of(&storage)`,
and before it `update_trial::take_the_claim` for an update's trial.
`is_writer_of` asks the kernel without waiting; `ClaimRefusal::Sweeping` is
transient, follows the existing hand-over road, and is not written into the
table, so the next writer question asks again. A process
with no window never asks it: `--explorer-command` (`explorer_menu::serve`) and
the front door's refusal (`say_at_the_front_door`, which carries `--version`
and `--help`) read their language through `main::door_language` →
`persist::SettingsStore::peek_language` over
`persist::storage_dir_as_it_stands()` — bytes on the settings lane, no claim,
no folder made or moved, no refused file kept. `--uninstall-cleanup` takes the
kernel claim outside the table (to refuse while a Folio runs) and lets it go
when it exits, and so does `--uninstall`, which reads its language through the
same `peek_language` over its own data roots (0.4.7 T-UNINSTALL-UX); the update doors (`--update-apply`, and `--update-recover`
through the macOS applier's steps) ask only through `persist::try_claim`,
which remembers nothing, and let go at once. `attention`,
`--remove-shell-integration` and `--remove-explorer-menu` never ask.

### 4.3 The `Deref` trap

`main.rs` declares `impl Deref for Runtime<'_>` and `impl DerefMut for Runtime<'_>`
with `type Target = TabState`, resolving through `active_item`/`active_item_mut`
on the active tab. **Any method may therefore reach the active tab with no
`self.window.` prefix at all.** Two consequences a ticket must respect:

- A census or grep that reads `self.foo` inside an `impl Runtime` method as a
  field of `Runtime` or `WindowRuntime` is wrong for every field that belongs to
  `TabState`, and wrong in the direction that makes `Runtime` look like the
  owner of state it only borrows. Every `self.` access must be resolved to
  `Runtime`, to `WindowRuntime`, or through `Deref` to `TabState`. Since
  0.4.6 census-1 that resolution is `bt_source::FieldCensus`. The query writes
  its inventory and sites reports under `target/`; the committed surfaces are
  only `docs/plans/design/ownership-census-unknowns.tsv` (unresolved sites,
  whose whole rows only shrink, with multiplicity, against the merge base) and
  the hand-edited `-annotations.tsv` (the class and proposed owner of every
  proven multi-writer fact). `bt-source`'s `census` test refuses a new unknown
  and a multi-writer fact with no annotation row;
  `scripts/ci/check-census-unknowns.ps1` refuses an unknown row added against
  the merge base and an annotation row added there that names no owner or says
  "proposed". Which module writes a fact, once it is single-writer or already
  annotated, is report data: a second writer in the same module, or a sole
  writer that moves, changes no committed file.
- `Runtime` grants every one of its methods mutable access to both `App` and
  `WindowRuntime`. Moving those methods into `runtime/*.rs` does not narrow
  that access. The file move is navigation and merge relief; it is **not** an
  ownership change and must not be reported as one.

The three hub fields — `tabs`, `active_tab`, `window` — are the document model.
No boundary that leaves them on the far side of an interface will hold, and no
interface that hands a subsystem `&mut tabs` is a boundary.

### 4.4 Facts born with one owner (0.4.6)

A fact a ticket creates is written here with its owner, so the census of §4.2
does not have to find it later.

| fact | owner | who writes it | class (§4.2) | status |
|---|---|---|---|---|
| **the update check's state** (fact 11: its memory, file and claim) — `update-check.json` v2 (`checked_at_ms`, `latest_tag`, `seen_tag`, `skipped_tag`, and U-42e's `local_stamp` / `local_tag`: whether the stamp and the tag came from an `--update-feed` check, each on its own, written only when true), this process's copy, and `update-check.lock` | `bt-app::update::OfferState`, one per process (`update::OWNER`); every change is `OfferState::transact`, one mutex held across read, change, atomic write and publication | the check's `bt-update-check` worker (the stamp, then the answer — never holding the lock across the request); the window thread's `answer_mark` (`seen_tag`); **Skip** (`OfferState::skip`: `skipped_tag` and `seen_tag`), a new writer with no caller until U-19; **an ordinary start** (`OfferState::load_for` without the feed: `forget_a_local_answer` clears a feed's stamp and tag, in memory at load and on the disk at its first write — U-42e, a new writer). Readers: the gear (`gear_mark_is_lit`), the General row and — while an update is offered — the About page's `Version` row (`update::offer`, T-GEAR-MARK-LANDS), all through `should_offer`, which compares a skipped tag by precedence and reads the switch the owner is told of | durability and external transactions | 0.4.6 U-6: one owner in this process; another process on the same data directory is kept from *asking* by the claim, not from writing |
| **the installation's transaction** — the update journal `H\journal.json`, its frozen header `{v, txn, rescue, class, outcome}` (`outcome` — `none`, `committed`, `rolled_back` — added before v1 shipped, coordinator ruling 2026-09-27, U-13), the trial's receipt, and the member inventories; the body's `RolledBack { untried }` and `Retired { outcome, untried }` (U-42a: no trial of the new build was ever begun; written only when true, so a journal of an earlier build reads as `false`) | `bt-app::update_txn` (pure: `Header`, `Phase` and `next`, `JOURNAL_WRITERS` and `EFFECT_RIGHTS`, `decide`, `at_start`, `Receipt`, `Inventories`, and `Home`, the locator — `Home::of` from the running executable, and on macOS `Home::for_bundle` from the bundle's path alone, `<parent>/.<Bundle>.folio-update/` with each transaction's `stage/<Bundle>`, `rescue/<Bundle>` and `mnt/`, U-12 and U-26) | O (the running build and its in-app job) writes `Allocated`, `Prepared`, `Handoff` and O's `Abandoned`; only the transaction-lock holder (the rescue copy of O, as applier or recovery) writes every later phase, and `Committed` only on N's receipt while the journal says `Trial`; N writes only its receipt (`docs/plans/design/self-update-2026-09-16.md` revision (b), §(b).2). **Since 0.4.7 U-37** (revision (h)): the receipt carries the optional `started` (still v1; a 0.4.6 reader ignores it), and N writes it through two writers of the same bytes, create-new — the storage worker, and after a refusal the trial's watch again (`update_trial::Gate::write_owed_receipt`); a lock holder may record a trial it did not start (`TrialBegan`/`RetrialBegan`), only one whose receipt names it exactly by pid and start instant (`update_apply::survey`, `adopting`), and before `decide` — in `settle` and before a `Stuck` retrial, whose deferral is then the road's end — it defers (`Ended::Deferred`) beside any other process of the new build, a held or unaskable data claim, a process list it cannot read, or a transaction folder `H\<txn>` whose receipts it cannot list (`update_apply::before_deciding`) | durability and external transactions | 0.4.6 U-20: the Windows Prepare (`update_prepare_windows`, the job's Windows driver, on the `bt-update-job` worker) is O's writer of `Allocated` (the old shipped list, `Layout::Members`, and the rescue path `H\<txn>\rescue\folio.exe`), `Prepared` (the inventories O measured under the lock — old shipped, present by digest, new — through `Journal::prepare_with`) and its own `Abandoned`; its staged set is `H\<txn>\set\` and its rescue a copy of the running executable (`Home::members_folder`, `Home::rescue_copy`), everything inside the install folder; the launch pass (`update_prepare::at_launch`, W1's sweep, W2's count and its discard at 2) is now shared with macOS, and the Windows resume's revalidation is `update_prepare_windows::revalidate`. 0.4.6 U-27: the macOS Prepare (`update_prepare_macos`, the job's macOS driver, on the `bt-update-job` worker) is O's writer of `Allocated` (the running bundle's identity and the offer's version, `Layout::BundleIntent`), `Prepared` (both identities, `Layout::Bundle`, through `Journal::prepare_with`) and its own `Abandoned`; it clears what it gave up — every image under `H/<txn>` detached, then `H/<txn>`, then the journal — and so do the job owner's launch pass (`at_launch`: M1's sweep, M2's count and its discard at 2) and the resume's revalidation; the rights table grants O `DetachMount`/`DeleteTxnDir`/`DeleteJournal` in `Allocated` and `Abandoned`, and an ordinary start `DetachMount` wherever it deletes `H/<txn>` (`StartAction::Retire`/`Discard`). 0.4.6 U-21: O's `Handoff{applier}` is written at the quit's way out, durably through `install_txn::durable_write` on the storage worker, before the applier starts (`update_handoff`); the O-authored event `ApplierNotStarted` (`Handoff` → `Abandoned`, terminal, outcome `none`) records a start that failed. 0.4.6 U-28: the macOS applier (`update_apply_macos`, the rescue clone as `Actor::Applier`, holding the lock from O's exit on) writes `Armed`, `Moving`, `Trial`, `Committed`, `Retired{Committed}` (the journal then kept for the trial's watch), `Reverted` (→ `Prepared`), `RollbackIntent`, and its own `Abandoned` through the new applier event `OldStayed` (`Handoff` → `Abandoned`: the old build did not let go of its claim within the wait); each through `Journal::advance`, `may_record` and `install_txn::durable_write`, each effect through `may`. 0.4.6 U-29: **(c′) the rollback** — the applier after `RollbackIntent`, and the rescue build as recovery (`update_recover` → `update_apply_macos::recover`, `Actor::Recovery`, from the LaunchAgent at login or from an ordinary start's hand-over) — writes `RolledBack`, `Stuck` (now `Stuck{trial, last_error, attempts}`: at `STUCK_ATTEMPT_LIMIT` = 3 `decide` answers `GiveUp` and nothing more is tried) and `Retired{RolledBack}`, the journal then kept for the relaunched build, which reads its header (`--update-failed`, `update_txn::after_rollback`) and retires it; recovery also finishes a `Committed` retirement (M11). 0.4.6 U-29b: **(c′) the recovery finishes every phase** — the rescue build as `update_txn::Asker::Rescue` writes `Reverted` over `Handoff`/`Armed` (as `Actor::Applier`) and `Moving`, **writes `Trial`** (`TrialBegan`'s authors and `JOURNAL_WRITERS` gain `Actor::Recovery`) over an exchange a dead applier left with the new bundle live, `Committed` on a found receipt, and — as the applier does — the new `RetrialBegan` (`Stuck` → `Stuck`, `Stuck.retrial`) when it starts the new build as a trial over a `Stuck` whose new bundle is live; `next` takes that trial's receipt from `Stuck` to `Committed` (the one road besides `Trial`'s). Readers that assumed the old sources: the ordinary start (the word `--update-failed` now continues past any `destructive` header, and `after_rollback` answers *Update incomplete.* for all of them) and the trial's watch (`trial_sight` reads a `destructive` rollback as undecided; only a `terminal` class without `committed` ends it). Readers that assumed recovery wrote nothing: the ordinary start (a `destructive`, `rolled_back` header is continued past by a start sent with `--update-failed`, `StartView::sent_by_rollback`) and the trial's watch (unchanged: `rolled_back` ends it). 0.4.6 U-23: the Windows applier (`update_apply_windows`, the rescue copy `H\<txn>\rescue\folio.exe` as `Actor::Applier`, holding the lock from O's exit on) writes `Armed`, `Moving` (durable before the first move), `Trial`, `Committed`, `Retired{Committed}`, `Reverted`, `RollbackIntent`, and its own `Abandoned` through `OldStayed` and the new applier event `Unverified` (`Handoff` → `Abandoned`: the staged set is no longer what was verified, `update_prepare_windows::staged_as_verified`, before the entrance); the Windows recovery (`update_apply_windows::recover`, the same code as R) writes `Reverted` from `Handoff`/`Armed`, `RollbackIntent` from `Moving` and from an unanswered `Trial`, `Committed` and `Retired{Committed}`; the journal's recording, the claim wait and the wait from `Trial` to an outcome are shared with macOS in `update_apply`. 0.4.6 U-24: **(c′) the Windows rollback** — the applier (`Actor::Applier`, after it declared the rollback) and the Windows recovery (`update_apply_windows::recover`, now `Asker::Rescue` through `decide`, as `Actor::Recovery`) write `RollbackIntent` (from `Moving` and an unanswered `Trial`), `RolledBack` (the old inventory verified by digest), `Stuck{attempts}` (any failed step; at 3 nothing more is tried), `Retired{RolledBack}`, and — over a `Stuck` whose new set is installed — `RetrialBegan`, and `Committed` on that trial's receipt; the trial's wait (`update_apply::watch_trial`, `Trial` and `Stuck.retrial`) and stop (`update_apply::stop_trial`) are both platforms' now. Readers that assumed only a macOS holder wrote them: the ordinary start (a Windows start sent with `--update-failed` continues past the `destructive` header and raises the card, as on macOS) and the trial's watch (unchanged). 0.4.6 U-10: protocol only, no product caller; the journal, lock and entrance effects arrive behind their own doors in U-11, U-22 and U-26; since U-22 the event `Armed` carries `bt_platform::logon_hook::Armed`, made only after the entrance is written, flushed and read back, and refused when made for another transaction |
| **the transaction's adapter** (0.4.7 U-41a1) — whose road a transaction takes: the journal body's `adapter` (`update_txn::Adapter`: `Ours`, `Homebrew`, `Scoop`, `Winget`; written only when it is not `Ours`, so an ordinary copy's journal is 0.4.6's bytes and a body without it reads as `Ours`), and whether each adapter's road is built (`update_adapter::HOMEBREW_ROAD`, `SCOOP_ROAD`, `WINGET_ROAD`, all off; `built_on`) | `bt-app::update_txn` (the field, pure) and `bt-app::update_adapter` (the choice from the channel, `of_channel`, and the constants); each road finds the layout a journal names through `update_adapter::Layouts` and calls its points — `update_prepare_windows::PreparePoint` / `update_prepare_macos::PreparePoint` (Prepare), `update_apply_windows::ApplyPoints` / `update_apply_macos::ApplyPoints` (Activate forward and back, Prove) — each road's `Ours` today's code moved | O's Prepare alone, once, at `Allocated` (`Journal::naming`, from the channel the press read, `update_prepare_*::eligible`); every later phase carries it unchanged (`Journal::advance`, `prepare_with`); the applier, the recovery and the resume's revalidation read it from the journal, never from the channel (managed-update R2), and a journal naming an adapter this build has not built is refused before any point is called (`update_adapter::NotBuilt`) | a process fact of the transaction: one writer, set once | eligibility reads the constants (`update_job::Evidence::eligibility`: a managed copy whose adapter's road is not built on this platform keeps `NotEligible::Managed` and the row's **Copy**; the Prepares' road checks refuse it before `Allocated`), so no journal names any adapter but `Ours` in this build; no door, phase, transition or writer right changes (`docs/plans/design/managed-update-2026-09-29.md` §1.1, revision (d)) |
| **the installed bundle (macOS)** — the `.app` at `<parent>/<Bundle>.app` that Launch Services and the Dock start, and what is in it | `bt_platform::install_flip` (the exchange) under `bt-app::update_apply_macos` (the applier) | the package manager or the person who put it there; **(c′) since 0.4.6 U-28, the applier**: one `renamex_np(RENAME_SWAP)` with `H/<txn>/stage/<Bundle>.app`, only while the journal durably says `Moving` (`update_txn::may(Applier, Swap, Moving)`), both bundles' identities checked against the journal first; **since U-29 the swap back** — the same exchange, by the applier or the rescue build as recovery, only while the journal says `RollbackIntent` or `Stuck` (`may(_, Swap, _)`), only while the live identity is the new one and `stage/` holds the old one (`update_txn::decide`), under exclusive admission, after the trial is stopped; the restored bundle is then checked against the rescue clone's own requirement before `RolledBack`. Readers that assumed only an installer changes it: a process running from it keeps its image (E-12: the kernel names it at `stage/` afterwards); `Home::for_bundle` and the ordinary start's admission read the path, not the contents | durability and external transactions | 0.4.6 U-28 |
| **the installed file set (Windows)** — the files `<install>\folio.exe`, `conpty.dll`, `OpenConsole.exe`, the command files and the rest the release manifest names, that a start runs | `bt_platform::install_txn::durable_move` (one `MoveFileExW(…, MOVEFILE_WRITE_THROUGH)` per file, never over a file, each folder flushed) under `bt-app::update_apply_windows` (the applier); `bt_platform::install_flip::held_open` is its process check | the package manager or the person who put it there; **(c′) since 0.4.6 U-23, the applier**: only while the journal durably says `Moving` (`update_txn::may(Applier, MoveOldOut / MoveNewIn, Moving)`), under exclusive admission, after every old file was found a regular file at its name and none held open; every old file present to `H\<txn>\backup\`, then every new file from `set\` in (`Inventories::forward_moves`), so after every move each old file is in exactly one of install and `backup\` and each new file in exactly one of `set\` and install (I1′); **since U-24 the rollback** is the second writer, by P or R only while the journal durably says `RollbackIntent` or `Stuck` (`may(Applier | Recovery, MoveNewOut / MoveOldBack, …)`), under exclusive admission, after the trial is stopped: every install file whose digest is a new member's to `H\<txn>\rolledout\`, then every old file back from `backup\` (`update_txn::rollback_moves`, reconciled by digest, never by the phase; a file at neither digest is never moved and the rollback is `Stuck`), so after every move each old file is in exactly one of install and `backup\` and each new file in exactly one of `set\`, install and `rolledout\`. Readers that assumed only an installer changes it: the ordinary start's admission and the trial read the path; a process running from an old file keeps its image, which is why the process check refuses one before any move | durability and external transactions | 0.4.6 U-23, U-24 |
| **this build may update itself** — the eligibility half of the owner's 2026-09-25 rule (signed *and* built with the flag); the signature half is not a stored fact but a read of the running file, `bt_platform::trust::running_capability(flagged)` (U-15), asked by the Windows Prepare | `crates/bt-app/build.rs` (`updater_flag`, deciding by `update_eligibility::decide`), read through `bt-app::update::eligible` | the build invocation only: `FOLIO_UPDATER=on` emits the cfg `folio_updater`, unset or empty emits nothing, any other value stops the build; set by `build-release.yml` on a `v*` tag or its `updater` dispatch input and by the macOS release build (`docs/RELEASING.md`), by nothing else | a compile-time constant: one writer, no runtime change | 0.4.6 U-8: read by `diagnostics::run_header` (`updater on`/`off`, checked by `smoke.ps1 -Updater` / `-ExpectSigned`); the update job (U-18) is its reader to come |
| **this start's trial** — the transaction and nonce of `--update-trial`, when the journal's header says the start is that transaction's trial (a destructive class naming the same transaction) | `bt-app::update_startup` (`TRIAL`, read through `update_startup::trial()`) | `update_startup::pass` alone, once, at start, from the command line and the header; never changes in a run | a process fact: one writer, set once | 0.4.6 U-12 wrote it; U-13 made `update_trial` its reader (the write gate, the watch, the claim and the receipt), with the trial's installation home beside it (`update_startup::trial_home()`) |
| **the trial's held-back writes** — whether this process may write anything durable yet, which writers it held back, the copies of refused documents it owes, whether its claim was adopted and whether its receipt was handed over | `bt-app::update_trial` (`GATE`; every writer asks `update_trial::defer(Writer)`, a read asks `update_trial::keeping()`) | the writers themselves record what they held back; the trial's watch (`folio-trial-watch`, read-only on `H\journal.json`'s frozen header — `class` and `outcome` only — `Lane::UpdateJournal`; since 0.4.7 U-37 also its receipt's retry and its watchdog, which hands an undecided transaction back at most four times, never on an unreadable journal, and never ends its own process) decides it once — `Committed` releases the writers to the window thread (`AppEvent::TrialWritesReleased` → `App::release_trial_writes`, which runs each again), any other end drops them and keeps the gate shut. **Inert outside a trial**: `update_trial::defer` answers `false` and records nothing. **(c′) — the gate is a new condition on each of these writers**, and every reader of their files meets it: `persist::storage_dir`'s move of the data folder (`Writer::DataFolderMove`), the stores' `create_dir_all` (`persist::make_data_folder`, `DataFolder`), `bt_persist`'s copy of a refused document (read with `Keeping::Owed`, `RefusedCopies`), `SessionStore`'s sentinel and hand-over (`Session`), `SettingsStore::write_now` (`Settings`), the keybindings, profiles and pins stores' `write_now` (`Keybindings`, `Profiles`, `Pins`), `update::begin` and `OfferState::transact` (`UpdateCheck`), `shell_integration::begin_startup_migration` (the integration marks, `ProfileMigration`), `script_path` and `zdotdir_path` (`BashScript`, `ZshScripts`), `psreadline::upgrade_recorded` (`PsReadLineUpgrade`), `explorer_menu::begin_probe`'s repair and renewal, the gate asked in `explorer_menu::probe_at_start` on the probe's worker (`ExplorerRepair`, U-25), and the toast identity in `NotificationDesk::show` (`Notifier::register_identity`, `ToastIdentity`). Exempt, as F-7 says: `diagnostics.log` and the run's other accounts of itself (a hang report, the panic log); the data directory's claim and endpoints live in the runtime directory for the process's life | a process fact with one owner; its one decision is made once | 0.4.6 U-13 |
| **the update job** — this process's one offer (`{txn, tag, asset, hash_doc, to_version}`, minted once and never re-derived), its state (`Pending` until both the check has settled and the channel is read — and, since U-33, until the job owner's pass over a transaction an earlier launch left has landed — then `Idle`, `Available`, `Downloading`, `Staged`, `Verified`, `Quitting`, `Committing`, `Failed`, `Updated`), whether this launch has raised its one card, the window it is raised in, and the last eligibility answer | the application: `App::update_job` (`bt-app::update_job::Job`), not a window | the window thread only: `FolioApp::consider_update_offer` on `AppEvent::UpdateJobOffer` (sent when the check settles — `update::begin`, every road — when `install_channel`'s worker has read the channel, and when the launch pass lands); `Job::drain_progress` on `AppEvent::UpdateJobProgress` (a driver's reports, each named by the offer's `txn`; a report for another transaction or a cancelled job is dropped); `Runtime::apply_update_check` (the switch turned off puts an offer away and cancels a download); the card's verbs (`Job::answer_verb`, from `Runtime::answer_update_card`); the card moving to the next ordinary window when its own closes (`Job::hand_over`, from `FolioApp::settle_update_card` once a turn); the General row's `Restart to update` (`Job::reopen`); a trial's commit read by its watch (`Job::after_commit` on `AppEvent::TrialWritesReleased`, U-32). **Restart** (`Job::restart` through `App::restart_for_update`, U-21) and the quit's two answers, delivered on the window thread in the turn the quit moved (`FolioApp::deliver_the_quits_update_report` → `Job::apply`: `QuitAbandoned(Abandon)` back to `Verified` with the reason kept for the card, `SessionLanded` on to `Committing`); a driver's `Verified` report hands the job the staged transaction — home, journal at `Prepared`, the transaction lock — through `Poster::verified` (U-21's seam for U-20/U-27), held until the process leaves. It reads, never writes, the check's state (`update::job_evidence`, `None` until this launch's check settles), `install_channel::channel()`, `update::eligible()` and `update_startup::trial()`; eligibility is pure (`update_job::Evidence::eligibility`) | durability and external transactions (§4.2) | 0.4.6 U-32: **the macOS gate is on** (`OFFERS_ENABLED_MACOS`); and **a third writer of the card's state** — a launch sent with `--update-failed` whose card says *Update incomplete.* (`Job::after_rollback`, remembered) moves to `Updated` when this process's trial watch reads `Committed` (a trial over `Stuck` committed forward by its receipt); its readers, `update_card::paint`, `Job::card_window` and `Job::hand_over`, draw and seat it like a failed card. 0.4.6 U-33: **a second source of `Verified`** — the start names the home whose `preparing`/`deferred` transaction it continued past (`update_startup::waiting`), `Job::after_start` holds it, and the first `consider` once the channel is known runs the job owner's pass on the `bt-update-job` worker (`update_prepare::settle_at_launch`: sweep, count, the platform's revalidating resume, discard); its answer (`update_job::Landed`) is read in `consider`, and a resumed set puts the job at `Verified` with the offer rebuilt from the set's own version and the staged transaction held, as a driver's `Verified` report does; `Busy` spends the launch's offer; the day's check (`update::begin`) is started by the job after the pass lands, and not when it resumed. 0.4.6 U-27: a press reaches `update_job::driver_for_this_copy` — on macOS `update_prepare_macos::MacPrepare` and, since U-20, on Windows `update_prepare_windows::WinPrepare`, each with the download door (`update_job::ReleaseDownload`), which report `Step::Received`, `Staged`, `Verified` (through `Poster::verified`) or `Stopped(Stop)` with a named reason (U-20 adds `Stop::Space { short_by }`, which the failed card names in megabytes), and stop at their next step once the job's Cancel or the switch sets the poster's cancel flag; elsewhere `Unsupported`. 0.4.6 U-19: its readers are the update card (`update_card::paint`, drawn by `restore::update_card_build` in `Job::card_window`) and the General row (`update_card::row_foot` through `SettingsValues::update_row`); neither holds a copy, and `App::update_shown` is only last turn's drawing, for the repaint comparison. 0.4.6 U-31: the gate is per platform (`Job::offers_enabled_on`, a build fact) — on for Windows, off for macOS until U-32, so a Windows build's job raises the card. 0.4.6 U-18: headless — offers were off (`Job::offers_enabled` was `false` until U-31/U-32), the only driver is `update_job::Unsupported` (refuses before any network, staging, flush or wait), and the decision is one `diagnostics.log` line per launch |
| **what this release contains** — the Windows archive's members other than `folio.exe` and `folio.msix`, each by name, SHA-256 and size, with the version, architecture, archive root, update protocol and oldest updater (`FOLIO_RELEASE_MANIFEST`, v1) | `crates/bt-app/build.rs` (`release_manifest`), from the one member list `scripts/release/archive-members.txt`; the format, `PROTOCOL` and `MIN_UPDATER` are `bt_winres::release_manifest` | the build only: for a Windows target it hashes the list's members (the ConPTY sidecar from `bt-pty`'s `links` metadata, the rest from the checkout) into an `RCDATA` resource of `folio.exe`, which the executable's signature signs; on macOS `render-info-plist` writes `PROTOCOL` and `MIN_UPDATER` as `Info.plist` keys | a compile-time constant: one writer, no runtime change | 0.4.6 U-9: read, never by running the executable, by `package.ps1` (refuses to pack a member that differs) and `smoke.ps1` (checks the packed archive); the updater's archive reader (U-14) is its reader to come |
| **the topology and pane scans of a live capture** (0.4.7, T-PANE-COLUMNS) — the ordered pane rectangles, excluded edge status row, screen-owned fence state (R9), compact foreground-program identity, and each pane's capture-bound rows/checkpoint (R7) | `bt_detect::LiveCapture`: its `CaptureInner` owns the original input allocation, checkpoint and options; its `OnceLock<Arc<CaptureFrame>>` owns one `Arc<ScreenFrame>` topology plus capture-local `PaneScan`s. `ScreenFrame` owns no input row or process tree | the first caller of `LiveCapture::frame` or `pane_scans` measures both once. Tasks, records and callers keep the `LiveCapture` and therefore its input allocation and pane scans; they die when the last capture holder drops. `DualPlaneSession::current_frame` may retain only the topology `Arc`. Readers: live resolution and ledgers in `bt-detect`; arming, completion and reconstruction in `bt-term` read `PaneScan`; frame comparison reads only `ScreenFrame` | a derived fact: one initialiser, write-once | 0.4.7 69a: the topology/input split is pinned by `the_sessions_frame_topology_does_not_retain_capture_rows`, `a_capture_measures_its_frame_once`, and `a_capture_shares_its_frame_between_the_tasks_that_hold_it`; the 30-screen unknown-provenance corpus remains byte exact |
| **a live block's pane, and the extent derived from it** (0.4.7, T-PANE-COLUMNS — **an ownership split recorded under RULES §55** in this table and in the reviewed design note `docs/plans/design/pane-columns-2026-09-29.md`) — the rectangle a live block was proven in, and so its horizontal extent, fit width, source width, fold width, drawing limits and hit area, which until now were the terminal pane's | the record: `DualPlaneSession::live_decorations`' `LiveDecorationRecord::pane` beside its `capture`; the task's `LiveDetectionTask::pane` beside its `capture`; derived at each projection by `bt-term` (`math_band_for`, `live_block_columns`, `live_block_limits`, `live_pane_narrower_than_marks`, `frame_rows_width_cells_in`), carried by `bt-viewport` (`ProjectedLiveMathArtifact::{left,right}_limit_columns`, `pane_narrower_than_marks`; `MathBlockPlacement::{left,right}_limit_columns`) and drawn by `bt-render` (`math_block_left_edge_px`, `math_block_right_px`) | `bt_detect::apply_live_detected_block` writes `task.pane` beside `start` and `end`; the record takes it at install (`apply_live_worker_completion`) and a new one whenever it is re-anchored onto another capture (`project_live_record` through `pane_holding`, `restore_offscreen_decorations` from the owning scan's task, `finish_alternate_repaint`'s bounded re-detection); never otherwise. **(c′)** a second input to every reader that assumed "band = pane width": `DualPlaneSession::math_band`, `math_pane_width_px`, `frame_rows_width_cells`, `InlineGridGeometry::pane_columns`, `bt_render`'s `math_block_geometry_px`, `math_horizontal_bounds`, `math_block_ground_bounds`, `math_band_face_for`, `math_tool_boxes_px` and the hit test that reads the marks' boxes first; and a second writer per row of the live height map wherever two panes' bands share rows (combined with `max`, the hidden top folded first — `ViewportProjection::sync_live_math_artifacts`) | projections (§4.2): derived each frame from the record's pane | 0.4.7 69a. On a screen no frame cut, every record's pane is the whole screen and every one of these numbers is today's. 69b (T-PANE-IDENTITY) keys math identity and stability by `(row, pane)` |
| **the session's current frame** (0.4.7, T-PANE-COLUMNS) — the topology of the capture the last scheduling read | `DualPlaneSession::current_frame` (`Option<Arc<ScreenFrame>>`) | `DualPlaneSession::observe_frame`, from `schedule_live_artifacts`, alone; compared by value (`frames_agree`: rectangles, status row, screen-owned fence state, and compact foreground-program identity). It retains no capture row, pane scan, checkpoint or process tree. A difference clears every pane's math and refuses a completion read against the prior frame | a single writer on the window thread | 0.4.7 69a builds the topology identity and full-frame invalidation; 69b adds the pane math tier it also clears (note §8.2) |
| **the pane's foreground program** (0.4.7, T-PANE-COLUMNS E8) — `Unknown` or one canonical local image name, never a process tree | `DualPlaneSession::foreground_program`, one fact per pane session | the addressed foreground-program worker answer, after window/tab/seat and shell-incarnation routing, is the only writer. `DualPlaneSession::live_capture` snapshots it into `ScreenFrame`; the detector classifies trust only through `multiplexer_allowlist.txt` | one window-thread writer; worker observation delivered by address | every OSC 133 command start is dispatched at once or retained as one pending successor behind the in-flight probe; a frame candidate asks every five seconds; WSL/ssh boundaries store `Unknown`; every identity change invalidates the full pane tier |

---

## 5. Execution lanes

### 5.1 The seven lanes

Fifty production thread-spawn sites exist across three crates (`bt-app` 33,
`bt-platform` 13, `bt-pty` 4), plus one lazy rayon pool in `bt-term` (§0.1).
All but `bt-pty`'s four go through the thread door (0.4.6, A1c). **The thread count is not the defect; the absence of a contract
is.** `MathWorker::spawn` starts path verification and image scaling as well as
math and returns all three through one `MathWorkerResult` — a historical hosting
decision wearing a subsystem's name. `Runtime::apply_psreadline` performs an
installation synchronously while `profile_runtime::begin_enable` spawns a
worker for the same kind of work.

These are **logical owners with return contracts**, not a demand for one thread
per row.

| lane | owns | return and isolation contract |
|---|---|---|
| **Window** | native windows, input routing, focus and IME, native view placement, the compositor tree, and applying accepted results | never waits for another lane; work is budgeted across all windows |
| **OS hand-off** | opening and revealing paths, URLs and system pages; executing an already-decided hand-off | request id plus target incarnation; completion means accepted or refused by the OS, never "the other application finished" |
| **Storage and integration transactions** | profile and marks changes, registration mutations, document saves and renames, configuration writes | serialize by affected resource; retain operation identity and the durable outcome; do not coalesce commands because their results share a slot |
| **Observation and computation** | font and machine probes, file, index and git observations, search, decoding, scaling | versioned requests; bounded queues and memory; latest-result replacement only where the semantics permit; math keeps its stack; potentially stuck filesystem calls stay isolated |
| **Session transport and lifecycle** | PTY birth, input and output transport, ordered resize, close, retirement | per-session incarnation and operation order; bounded input admission; no driver call while holding a lock the window needs |
| **Presentation** | surface acquisition, submission and presentation of an admitted frame | surface lease and frame identity; bounded pending picture; asynchronous completion; shared GPU preparation lifetime explicitly serialized |
| **Ingress and diagnostics** | endpoint listening and admission, watch delivery, trace writing, independent hang observation | publish before waking; explicit capacity and loss policy; the watchdog must stay able to observe a blocked owner |

The seams that already implement this shape: `bt-app::main::run_path_verify_worker`
(*one question, one call, one answer, and nothing else runs here*),
`bt-app::handoff_lane` (the OS hand-off lane: request id, the asking window's
`Pending` as the target incarnation, press order, a bound of 32 with no
coalescing, completion as the door's own `Result`),
`bt-pty::PtySession`, `bt-app::persist::SessionWriter`,
`bt-app::trace_sink::Queue`, `bt-app::main::Runtime::present_seats_and_commit`.

**The storage worker has a second job** (0.4.6 U-13): besides `session.json`, the
`SessionWriter` thread (`session-writer`) writes an update trial's receipt,
`H\<txn>\health-<nonce>`, create-new through `install_txn::durable_create`, when
the window thread hands it over at the trial's first pane text
(`SessionStore::write_receipt`, F-14). The answer comes back on a channel of its
own, not the session's receipts, and the trial's watch reads it into the log.
The window thread never writes the receipt, and **the journal keeps one writer,
the transaction-lock holder**: a receipt that lands after `RollbackIntent` is
ignored by that holder's rule (`update_txn::next`), not by anything here.
**The trial's watch** (`folio-trial-watch`, `BelowNormal`, only in a trial) reads
`H\journal.json` every 250 ms through `file_reads` on `Lane::UpdateJournal`,
writes nothing, and ends at the transaction's decision (`update_trial`).

**Keyboard layout Shift tables have one observation worker**
(`folio-layout-tables`, T-KEYBOARD-CTRLALT round 4), started only when
`GetKeyboardLayoutList` returned at least one layout: off Windows the list is
always empty, and there is no thread and no channel (round 7). At application
startup it builds every HKL returned by `GetKeyboardLayoutList`; an active HKL absent from
the application-owned map is offered on a bounded channel without waiting. The
worker reads the layout registry value and loads, copies and frees the System32
layout DLL through `bt_platform::keyboard_layout_shift_table`, publishes before
`AppEvent::LayoutTablesReady`, and the next lookup drains too; no registry or
loader call runs on the window thread. **Each HKL ends in one of two answers,
`Known(table)` or `Unavailable`** (round 7): `Unavailable` when the door answers
`None`, when eight requests are already admitted and unanswered, or when the
worker is gone (a `Disconnected` send or drain turns every pending HKL
`Unavailable` at once). An `Unavailable` HKL is never asked again, and a
Ctrl+Shift+Alt chord on it, like one made while its table is pending, uses the
un-shifted character; a refusal other than the door's `None` writes one
`diagnostics.log` line per HKL. The answer channel holds at most the startup
layouts plus the eight requests, so the worker's send never waits for room.

**The band rule — three tiers, not two.** The event and render loop is
`AboveNormal`; the PTY reader is `Normal`; **every** worker is `BelowNormal`.
The band is set by `bt_platform::spawn_at_priority` as the first statement of
the closure, because Windows hands a new thread `Normal` whatever its creator
stands in; the band call, `set_current_thread_priority`, is the one `unsafe`
boundary for thread priority, and the door itself lives in `admission`, which
forbids `unsafe`.
MMCSS is explicitly refused. Some threads stay at `Normal` by RULES 53's
exceptions (playback, first-frame extraction, the two ingress endpoints,
clipboard saves, the three standalone-process workers). The observation and
probe threads that also start at `Normal` today — `bt-dir-watch`,
`folio-video-prewarm`, `folio-video-canplay`, `folio-web-thumb`,
`folio-explorer-probe` and `folio-explorer-deploy` — break the rule. They go
through the door since A1c, at the band they had, and move to the workers' band
in a 0.4.7 ticket.

**Which kind of thread this is** (0.4.6 ticket A1a;
`docs/plans/design/thread-door-2026-09-26.md`, whose revisions (b)–(e) rule over
its earlier sections). `bt_platform::admission` owns three facts, and forbids
`unsafe`. **A thread's role** — `Unset`, `Window`, `Worker(name)`,
`Callback(name)` — is a thread-local written only by its entries:
`enter_window_thread`, once, in `fn main` directly after the argument parse (the
six argv doors above it never make a window); `enter_callback(name)` as the first
statement of each OS-owned callback entry — the console control handler, the
toast's `Activated` handler, Media Foundation's `EventNotify`, the `NSURLSession`
delegate, the notification centre's delegate and completions, the Finder open's
completion, `AVPlayer`'s end observer, and wgpu's device-lost and uncaptured-error
callbacks — whose `CallbackScope` names a thread nobody named for the callback's
length and gives it back, even on unwind, and leaves a thread that has a role as
it is (AppKit and the message pump deliver most callbacks on the window thread,
which is `Window` there); `enter_standalone_main`, once per process, for a door
process's main thread (its three callers, since A1c: the `attention` verb's
payload wait and the two Explorer-menu removals); and the thread door,
`spawn_at_priority`, which since A1b lives in `admission`: inside the new
thread it sets the band, then the role `Worker(name)`, then builds a `WorkerCtx`
on the thread's own stack and lends it to the body — private fields, `!Send`,
`!Sync`, made by one private function whose only callers are the door and
`enter_standalone_main`. **A worker-only door takes `&WorkerCtx`**, so code with
none — the window thread, a callback, a thread started outside the door — does
not compile against it; the one such door today is the hand-off's
(`ShellThread::enter`, below §6). **Every thread `bt-app` and `bt-platform`
start comes from the door** (A1c), in the window process and in the door
processes alike, so each of them is a `Worker` by its name. The only threads
left outside it are `bt-pty`'s four and `bt-term`'s resample pool, which stay
`Unset` by design, and `Unset` is never a worker and never the window. **The window
thread's phase** — `Starting`, `Running`, `Exiting` — has four writers at pinned
places: `enter_window_thread` (`Starting`); `loop_running` in
`FolioApp::new_events` on `StartCause::Init`; `exiting` in
`settle_quit`'s `Write` arm (at its head for a person's quit; for an update's
quit, whose write is a receipt the loop waits for with the windows up, once the
receipt's verdict is one that leaves — U-21), at the head of `App::finish`, after `run_app` returns
and in `fn main`'s event-loop build error arm (from `Running` or `Starting`, so a
loop that fails before its first turn still leaves through the exit doors); and
`quit_abandoned` at the head of the `Abandon` arm (back from `Exiting`; from
`Running`, where a cancelled quit or an incomplete save leaves it, nothing). A
writer called off the window thread, or on a transition not its own, changes
nothing and is counted. **An owner-thread wait is admitted** by
`admission::admitted::<D, _>(|token| door(token, …))`: on a thread that is not
`Window`, or in a phase outside the door's, it returns `Refused` without running
the work, and counts it on the door's counter and on the process total, which
`diagnostics::run_footer` writes as the run's last line. Otherwise the work gets
a `WaitToken` for that door alone, which cannot be returned, stored, sent,
copied or made anywhere else, and the call is measured by the meter
`hang_watch::start` installs once (`hang_watch::ADMISSION_METER`): the door's
station is entered before the work — so a call that never returns is already the
station a hang report names — and the station, call-tree node and scope it left
are restored after it, from a cookie the admitted frame keeps; a work that
panics is entered and never left. The doors are the types of
`admission::doors`, one per line of the registry (§5.3). **Every door takes its
token** (A1d): each door's function takes `WaitToken<'_, doors::X>` by value as its
first parameter, so it cannot be called outside an admission, and the admission
stands at the statement that made the call — the preparation around it stays
outside. The composition's commit, birth and window-size doors, the web engine's
controller, environment and rehost, `WindowRenderer`'s present and surface
birth, and the font lookup by name take theirs in `bt-platform` and `bt-render`,
on every platform arm; winit's calls go through `bt-app::owner_door` (title,
caret area, focus, visibility, cursor); `bt-pty`'s three — a shell's birth, a
leaf's resize, the quit's wait for retirements — through `bt-app::pty_door`,
because `bt-pty` has no edge to `bt-platform`; the first window's GPU through
`bt-app::gpu_door`. The commits inside an admitted batch (a tree's birth, the
window's ground, a page's rehost and its compensation) call
`Compositor::commit_now`, which is `pub(crate)` with seven callers. A present is
two sibling admissions under `during(PresentSeats)`: `PresentFrame`, a declared
batch (compose, configure, acquire, submit, present), then `CompositorCommit`.
A refusal is handled before anything the call would change, on the road the
door's own failure takes.

**The escapes are fenced by one source guard** (0.4.6 ticket A1e; the note's §9.1
as amended, and its revision (g)). `hang_watch::window_waits_tests::every_door_is_where_the_registry_says`
reads the product's source through `bt_source` — `bt-app` and every first-party
package its manifests reach, test modules out by their declaration, `vendor/` out —
and runs nine named assertions, each failure naming its assertion, the row and the
difference:
(1) every first-party package is the product or a declared tool (`bt-corpus`,
`bt-source`);
(2) `WaitToken`, `WorkerCtx` and `admission::doors` are never named inside
`unsafe` or inside a `transmute`, `zeroed`, `MaybeUninit` or `read` expression, and
`admission` keeps `#![forbid(unsafe_code)]` and writes no `unsafe`;
(3) one `WorkerCtx` struct literal, in `admission::lend_worker`, called by the
thread door and `enter_standalone_main` once each, and one inherent `impl` and no
derive for `WorkerCtx` and `WaitToken`;
(4) the role and phase writers at their pinned call sites, each after its landmark
(`enter_window_thread` after the parse; `exiting` in `settle_quit`'s `Write` arm,
at `App::finish`'s head, in the build-error arm and after `run_app`;
`quit_abandoned` in the `Abandon` arm; `loop_running` on `Init`; the three
standalone entries), each spelled only as the call `admission::writer(…)`, and
`enter_callback` at its nineteen entries;
(5) no `allow` or `expect` of `clippy::disallowed_methods`, `clippy::style`,
`clippy::all` or `warnings`, at any level, in either path spelling, inside
`cfg_attr` at any depth — only a door's own `expect`, of which there are none until
A2;
(6) `msg_send!`, `vtable(`, `GetProcAddress`, `extern` blocks, `#[link]` and
`#[macro_export]` only with the owners the registry's `# owners` section counts,
and no exported macro naming the vocabulary;
(7) every function that takes a `WaitToken` takes one registry door's and runs its
effect inside its own call: not `async`, returning no closure, future, iterator,
`dyn` object or `fn` pointer, keeping no `'static` closure;
(8) **the closed `Drop` inventory**: thirteen `Drop`s may reach the vocabulary —
`DirWatch` on both platforms, `trace_sink::Shutdown`, `AttentionPipe` and
`LaunchPipe` on both platforms, the two video engines, `VideoSeat`, `VideoSeats`,
`PtySession` and WinHTTP's `http::Request` — each with its chain pinned body by
body (the first-party edges in order, the vocabulary effects by call site) and a
repayment in `docs/plans/structural-debt.md` (D-40, D-78…D-82); any other `Drop`
that names a wait, or calls a door or a pinned body, is red;
(9) every thread `bt-app` and `bt-platform` start comes through the thread door
(A1c's guard, absorbed under its own name).
It is a check over the stated inventories, not a whole-program analysis: a `Drop`
outside the table that reaches a wait through a helper of its own is not seen, and
neither is an inference-typed `transmute` inside `bt-platform`'s own `unsafe` (the
note's (b)2).

**The update pass runs on the window thread in `Starting`** (0.4.6 U-12,
`update_startup::pass`; the design's startup-recovery row). Directly after
`enter_window_thread` and before anything touches the data directory, it holds
`H\admission` shared, reads `H\journal.json` once (`Lane::Install`) and, only when
a journal is there, asks for `H\lock` and removes a finished transaction's
folder and journal through `install_txn`. Every lock is one non-blocking
attempt; the reads and removals are the start's disk work before a loop exists,
which is §5.3 row 18's reasoning, and no `window_waits.tsv` door covers them
(none of the start's other file work before the loop has one either).

**Results come back three ways**, and the rule for which is the last paragraph
of this section:
(1) `AppEvent` through the event-loop proxy, drained with `try_recv` on the
application layer — all seven `bt-app` lanes and all ten one-shot probes; (2) a
callback run on the worker thread that may do nothing but park a value and nudge
the loop — every thread in `bt-platform`, which may not name `AppEvent`; (3) a
static lock or latch read later, sometimes with an atomic revision beside it.
The split between (1) and (2) is not arbitrary — it is a fact about the crate
graph — and that fact is stated here and nowhere else.

**The lane contract** (D-33; 2026-09-25, 0.4.6 ticket A5;
`docs/plans/design/window-thread-budget-2026-09-25.md` §3 and §R-D). A lane is
a worker the window thread gives requests to and hears answers from without
waiting. Every lane owes eight things, and states the policy each is judged by:
**(1)** a full lane answers the asker at once, and admits no more than its
declared bound — a bounded queue refuses the rest with a terminal answer, a
latest-value lane runs at most its declared rounds and answers the newest
request; **(2)** every request carries an identity its answer carries back;
**(3)** execution order and delivery order follow the declared policy, stated
separately (`Fifo`, `LatestValue` where "latest" means newest requested and
older-than-adopted is dropped, or `PerQuestion`); **(4)** an answer whose target
incarnation has gone is raised in no other target, and an answer older than the
one adopted is not raised; **(5)** every admitted request ends exactly once;
**(6)** the answer is published before the wake, and an answer whose wake was
lost is found by the next drain; **(7)** the answers held for a consumer that
has not drained are bounded; **(8)** a worker that dies leaves every admitted
request a terminal outcome, or the lane a fault the consumer sees. The
declarations are `bt-app::lane`'s constants (`HANDOFF`, `FONT`, `TASKBAR`,
`COMPUTATION`), and `lane_contract_tests` holds each lane to them through an
adapter that drives the lane's own admission, publication and acceptance. **A
lane that fails a claim is not declared conformant**: the failure is a row of
`lane::EXPECTED_FAILURES` with its exact kind and the ledger row that repairs
it, and the suite is red on an unexpected pass, a different failure, a skipped
lane and a claim that exercised no request. The hand-off lane is the reference
for its partial contract: it passes six claims and is declared to fail two
(D-70, D-71); the font and taskbar lanes fail per-request outcomes and worker
death (D-72, D-73); the computation lane fails bounds, identity and death
(D-74…D-76). Path verification, the ten probes (D-3) and the files, preview,
index and git workers have no adapter yet. **The return rule:** a lane whose
answer is the latest value of one fact publishes into a slot the window reads
(3) and wakes the loop through (1) or (2); a lane whose every request is owed
its own answer publishes through a channel the loop drains after the wake (1)
in `bt-app`, or through a parked value and a nudge (2) in `bt-platform`, which
may not name `AppEvent`.

### 5.2 What must stay on the window thread

Under the current interfaces these are **native affinity**, not habit. **IME** —
`Window::set_ime_cursor_area`, the platform system-caret update and the caret
destroy, from `Runtime::ime_input`, `apply_ime_cursor_area`,
`cancel_composition`, `let_go_of_this_window`. **The web view's controller and
native views** — `Runtime::sync_web_page`, `advance_web_page`,
`apply_web_outcomes` and `window_moved` act on live native views, and
`bt-platform::macos_webview::WebHost::request_controller` explicitly takes the
window-thread marker. **DirectComposition** — the covered-rectangle update and
the commit inside `Runtime::present_seats_and_commit`. **Window focus,
visibility, title and cursor** — `focus_window`, `set_visible`, `set_title`,
`set_cursor`, `request_redraw`.

Clipboard acquisition needs a platform-specific contract, not an assumption that
every clipboard object can move to a generic worker. **Short owner work may
stay**: accepting completions, model transitions, bounded input admission, hit
testing, and producing frame candidates.

### 5.3 The exception list — window-thread calls that block outside the process

Numbered because each is an exception and each carries an owner. A call not on
this list that blocks on something outside the process is a defect.
"Ticket to be issued" means the version is ruled and the id is not yet minted.
The single-instance claim is not a row: both its claim lock and its shared
`sweep.guard` lock use `LOCK_NB`; an exclusive sweep answers
`ClaimRefusal::Sweeping`, never an owner-thread wait.

**The table is generated** (2026-09-26, 0.4.6 ticket A1a): its one source is the
registry `crates/bt-app/src/window_waits.tsv`, and
`scripts/dev/generate-window-waits-table.ps1` writes it here;
`hang_watch::window_waits_tests::the_architecture_table_is_the_registry` fails the
build when the two differ. The registry's `# doors` section is the door types of
`bt_platform::admission::doors`, one per line, held equal to them by
`every_door_type_is_a_registry_line_and_says_what_the_line_says`: each door names
the row it serves, the `hang_watch` station its meter enters and the phases it is
admitted in (§5.1). Rows 16b and 23 were found by the thread-door note
(`docs/plans/design/thread-door-2026-09-26.md`, revisions (c)3 and (e)2) and are
`pending`: recorded, not ruled.

Since A2a (2026-09-26) the registry has two more sections the table does not
render. **`# entrances`** lists the cross-crate entrances that left the
vocabulary because their signature is the door — each takes, on every `cfg` arm,
exactly `WaitToken<'_, doors::<its door>>` (the source guard's assertion 9).
**`# effects`** lists the **effect functions**: the functions whose own body
holds a listed effect, with their entries and counts, kind, authority and the
door they serve — today the seven inside a registered door
(`gpu_door::open_first_window`, `SessionWriter::close` and `wait_for`,
`pty_door`'s three, the macOS `WebHost::request_environment`), held equal to the
door bodies by `the_effects_section_is_the_effect_functions_inside_the_doors`.
A2e puts one `expect` on each; revision (k) allocates the rest.

<!-- window_waits.tsv: generated by scripts/dev/generate-window-waits-table.ps1; edit the registry, not this table -->
| # | status | call | where | disposition |
|---|---|---|---|---|
| 1 | done | `ShellExecuteW` / `NSWorkspace::openURL:` — synchronous, no timeout; the measured ~1.4 s Ctrl+click stall | `bt-platform::handoff::{windows_handoff,macos_handoff}::hand_over`, reached from `Runtime::open_local_path`, `reveal_in_explorer`, `open_local_path_verified`, `reveal_verified`, `open_preview_link`, `hand_url_to_the_browser`, `activate_local_image_path`, `open_font_settings` | **done** — *a hand-off to the system runs on its own lane, and the window that receives it may take the front* (`DESIGN.md`, 2026-09-22) |
| 2 | open | the marks lock: a wait with no deadline behind our own writer, then `try_lock` and `sleep` up to `OUR_TURN` = 2 s for a holder in another process (`DESIGN.md`, 2026-09-23), then a dated `$PROFILE` copy, an atomic write and two marks writes | `Runtime::add_to_profile`, `spend_powershell_intent` → `profile_runtime::install_recorded`; the agent hook installs (`Runtime::apply_claude_hooks`, `apply_codex_notify`, `apply_copilot_hooks`) → `attention_ownership::record`; `psreadline::install_recorded`, from `Runtime::apply_psreadline`, `Runtime::create`'s `psreadline::upgrade_recorded` and `App::release_trial_writes` (the thread-door note, revision (k)9 item 1) | **0.4.4** — storage lane; the enable and removal halves are already on workers, the install half is not |
| 3 | open | `psreadline::apply_recorded` — nine files, ~429 KB, under the same lock | `Runtime::apply_psreadline` | **0.4.4** — storage lane; named by the 2026-09-21 history entry |
| 4 | open | `psreadline::installed_copy` — a recursive walk of the module directory | `Runtime::refresh_psreadline_installed` | **0.4.4** — observation lane |
| 5 | done | `bt_platform::monospace_font_families()` — the machine's whole font collection, enumerated inline; the traced cause of the frozen gear | `settings::monospace_family_files` ← `apply_stored_terminal_font` | **done** — *the font list is walked only on its lane, by a numbered request; the face in settings.json is found by its name* (`DESIGN.md`, 2026-09-24); what is left on the window thread is `bt_platform::monospace_family_named`, one family asked of the system collection (station `font family lookup`), measured at 2.6–3.4 ms cold against the walk's 71–80 ms on the development machine |
| 6 | done | `search::scan_history` / `scan_volatile` — the pattern re-run over every frozen line on every keystroke in the find box | `Runtime::refresh_search`, from the keystroke roads (`search_field_key`, `toggle_search_flag`, `search_ime`, `open_search`) and from `publish_frame_inner` | **done** — *a changed search scans one slice of history on the keystroke's frame and the rest on the following turns* (`DESIGN.md`, 2026-09-24; ticket 51). Not the observation lane: the frozen plane is the session's and mutable, so a worker needs a copy per question or an ownership change in `bt-transcript`; the bounded walk stays on this thread, one `search::SEARCH_HISTORY_SLICE` per keystroke and per turn (`Runtime::advance_search_scan`) |
| 7 | open | macOS `defaults read -g AppleLocale` and `locale -a`, blocking, no timeout, on the pane-birth road | `bt_platform::read_system_locale_declaration` | **0.4.4** — observation lane |
| 8 | open | `bt-platform::macos_watch::DirWatch::start_scoped` and, on Windows, `windows_impl::DirWatch::start_scoped` wait on `listening.recv()` with no deadline and join the watcher on their refusal arm; each `Drop` does `SetEvent` then an unbounded `join()` | the watch subscriptions: `DirNews::arm` (the scheme and storage watches, from `Runtime::create`; the scheme watch again from `Runtime::add_scheme`), and the `subscribe` roads of `files_watch`, `git_watch` and `preview_watch` (from `Runtime::advance_files_watch`, `advance_git_watch`, `advance_preview_watch` and `ask_the_unwatched_preview_files`); both platforms by the thread-door note's revision (l) | **0.4.4** — make watcher start and retirement asynchronous |
| 9 | open | surface acquire, queue submit, swapchain present, surface configure, DirectComposition size and commit | `Runtime::present_seats_and_commit` | **0.5** — presentation lane; the present mode itself comes from `get_default_config` and has no owner |
| 10 | open | device recovery's `pollster::block_on(rebuild_after_device_loss)` plus deliberate 150 ms and 450 ms sleeps across three attempts | `FolioApp::recovered_from_a_lost_device` | **0.5** — an explicit asynchronous state machine |
| 11 | open | `CreatePseudoConsole` + `CreateProcessW`, and a `stat` of the working directory | `create_leaf_session` → `PtySession::spawn_shell_in` | **0.5→0.6** — session lane, preserving input and resize ordering |
| 12 | open | the synchronous `ResizePseudoConsole` round trip | `Runtime::flush_pending_pty_resize` | **0.5→0.6** — session lane; moving it must preserve the ordering this function represents |
| 13 | done | `sample_window_place` — 4 to 8 syscalls, at three call sites for one instant | `drain_pty`, `advance_strip_animation`, `FolioApp::user_event` | **done** — *where the window is gets asked once per turn, at the turn's head; the drain and the strip tick read that answer* (`DESIGN.md`, 2026-09-24); one writer, `Runtime::observe_window_place`, also called at a window's birth and by an attention delivery between turns; each probe has its own station |
| 14 | done | `Window::set_title` at five call sites with no throttle | `drain_pty`, `activate_tab`, `dress_new_window`, `finish_synchronized_update_if_due`, `finish_rename` | **done** — *the window's title is one wanted value, written to the system only when it changes and at most once a frame* (`DESIGN.md`, 2026-09-24); the one remaining call is `Runtime::flush_title`, on this thread by §5.2 |
| 15 | ruled to stay | `bt_pty::wait_for_retirements` — a bounded `Condvar::wait_timeout` on the way out | `FolioApp::settle_quit`, `QuitStep::Retire` | **ruled to stay** (`T-QUIT-HAS-A-DEADLINE`, `T-QUIT-TIMEOUT-PROCEEDS`) |
| 16 | ruled to stay | `SessionWriter::wait_for` — `recv_timeout(SESSION_SAVE_BUDGET)` on the synchronous save | the quit write | **ruled to stay** — a timeout sets `stalled` and quit proceeds; a disconnect stops quit |
| 16b | pending | `SessionWriter::close` — a bounded poll for the session writer's end, then its join | `SessionStore::close` ← `App::finish`, on the way out | **pending** — found beside row 16 by the thread-door note's revision (c)3 (`docs/plans/design/thread-door-2026-09-26.md`), recorded and not ruled (`DESIGN.md`, 2026-09-26, *every thread that runs Folio's code has a role, the window thread has a phase, and each owner-thread wait is a door the registry lists*); bounded by the writer's own budget, past which the writer is left to process exit; its door is `SessionWriterRetire` |
| 17 | ruled to stay | `trace_sink::flush` — `recv_timeout(FLUSH_TIMEOUT)` | the way out of `fn main` | **ruled to stay** (`T-TRACE-OFF-THREAD`) |
| 18 | ruled to stay | `launch_wire::hand_over`, bounded by `HANDOVER_BUDGET` | `fn main`, before the loop exists | **ruled to stay** — there is no loop yet to be blocked |
| 19 | ruled to stay | `OutputRing::try_pop`, `InputRing::try_push` — both bounded to one lock, never split, never partly taken | `drain_leaf_pty`, `offer_pty_input` | **ruled to stay** — this is the design |
| 20 | open | `fs::rename`, the preserving atomic preview save, settings/keybindings/profiles writes, diagnostic file writes | `rename_preview_file`, `rename_files_row`, `save_preview_on`, `persist.rs`'s store methods | **0.4.4** — storage lane, with document-revision preconditions and receipts rather than a generic "background job finished" toast |
| 21 | open | a web page coming up: `CreateCoreWebView2CompositionController` on the first page of the process (88–269 ms synchronous, ~4,500 page faults: the engine's in-process half loading), then one engine dispatch of 70–303 ms on the message pump before the controller's callback; later pages ~3 ms and ~50 ms. `CreateCoreWebView2EnvironmentWithOptions` 10–38 ms on the first page, the install burst 2–6 ms. Measured headless on the development machine (ticket 43); the owner's next89 run held 4,099 ms | `WebSeat::step` → `WebHost::request_controller` (station `request_controller`), and the pump after it (station `message pump`); `WebSeat::start_environment` (station `request_environment`) | **open — narrowed by ticket 54**: the environment is asked for once on an idle turn after startup (`Runtime::warm_web_engine`, station `warm_web_engine`), which takes its 8.5–39 ms out of the first page's gesture; the environment starts no runtime process (measured), so the controller and the pump dispatch are still the first page's, and a new ruling is owed for them (D-64). **Narrowed again by ticket 60 (ruled 2026-09-25, option A):** for a profile that has opened a page (`web_pages_used`), the controller call (≤ 590 ms worst on the clean VM) moves to a quiet idle turn under `make_spare_web_controller`, and the first eligible page's window-thread cost is the rehost walk (`adopt_spare_web_controller`, 11–73 ms on the VM) and a navigate — 2318 → 146 ms median to the first page in spike 59. Still the first page's: a profile's first-ever page, a page that arrives before the spare has landed, and every page after the spare is used (~0.6 s warm, ~2.3 s cold); open for 0.4.6. Not movable to a lane: WebView2 refuses an environment used from any thread but the one that created it (`0x802A000C`, measured), so the environment, the controller and the engine's callbacks all belong to the window thread with the controller (§5.2); the pump after the first page is named per message since ticket 64 |
| 22 | done | `Window::set_ime_cursor_area` — `ImmSetCompositionWindow` + `ImmSetCandidateWindow`, answered by the input method; the owner's next93 caught 15 + 85 ms in one turn and a single call of 3,138 ms under load | `Runtime::apply_ime_cursor_area`, reached before ticket 63 from every offer: `publish_frame_inner`, `repaint_preview`, `reoffer_ime_cursor_area`, the turn's offer | **done** — *the input method's caret area is one wanted value, told to the system at most once a turn and only when it moved* (`DESIGN.md`, 2026-09-25); the one road is `Runtime::flush_ime_cursor_area`, from the turn's tail and from `Ime::Enabled`, on this thread by §5.2. A single slow answer still holds the thread; the repetition is gone |
| 23 | pending | `pollster::block_on(GpuContext::open(…))` — the first window's adapter, device and surface, asked for and waited on | `Runtime::create` ← `FolioApp::resumed` | **pending** — found by the thread-door note's revision (e)2 and recorded, not ruled (`DESIGN.md`, 2026-09-26, *every thread that runs Folio's code has a role, the window thread has a phase, and each owner-thread wait is a door the registry lists*); it stays on this thread (coordinator, 2026-09-26), its door is `GpuOpen`, and it moves when device recovery rebuilds on a worker (B9, D-42) — D-77 holds it until then |
| 24 | open | the data directory's endpoints start: on Windows each of `AttentionPipe::start` and `LaunchPipe::start` waits `recv_timeout(5 s)` for its listener's first word and, on a refusal or a timeout, joins the listener; on Unix each binds its socket synchronously | `Runtime::create` → `open_the_data_directorys_endpoints` → `attention_wire::open`, `launch_wire::open` | **open** — found by the thread-door note's revision (k)9 and ruled interim (`DESIGN.md`, 2026-09-27, *five window-thread waits the thread-door survey found are registry rows 24–28*): retained on the window thread, bounded by 5 s per endpoint for the first word plus a join of a listener already told to stop, so the total is not proven; repaid by B11 (version: owner); its door will be `EndpointStart` |
| 25 | open | a video seat's engine shut down outside a `Drop`: `video::engine::Engine::shutdown` (Windows) or `macos_player::Engine::shutdown` (macOS) — a 2 ms poll of the engine's stopped flag up to `SHUTDOWN_BUDGET` (2 s), then the join | `VideoSeats::{close, open, put}` from `Runtime::sweep_video_seats`, `stop_video_on`, `hide_file_peek`, `play_video_file_on`, `promote_file_peek`, `carry_the_recordings_of_moved_panes` | **open** — found by the thread-door note's revision (k)9 and ruled interim (`DESIGN.md`, 2026-09-27, *five window-thread waits the thread-door survey found are registry rows 24–28*): retained on the window thread, bounded by 2 s per engine plus the join of a thread that has said it stopped (not proven; a sweep of N seats is N × 2 s); repaid by D-80's ticket, which inherits this direct-close cost as scope added to D-80's recorded one (a shutdown through an explicit door instead of a `Drop`), amended by the thread-door note's revision (m) (`DESIGN.md`, 2026-09-27, *the thread-door note's revision (m)*); removing the destructor chain alone does not repay this row (version: 0.4.7, the ledger's; the owner may move it); its door will be `VideoShutdown` |
| 26 | open | the Windows clipboard's open: `retry_open_clipboard` sleeps 5, 10, 20 and 40 ms between five `OpenClipboard` attempts | `windows_impl::{clipboard_text, set_clipboard_text}` and `WindowsClipboard::begin`, from the copy and paste gestures | **open** — found by the thread-door note's revision (k)9 and ruled interim (`DESIGN.md`, 2026-09-27, *five window-thread waits the thread-door survey found are registry rows 24–28*): retained on the window thread, bounded by 75 ms of sleeps and five opens that do not wait; disposition made definite by the coordinator's ruling in the thread-door note's revision (m) (`DESIGN.md`, 2026-09-27, *the thread-door note's revision (m)*): an interim stay, repaid by the clipboard ticket B13 (version: 0.4.7); its door will be `ClipboardOpen` |
| 27 | open | the media session's quiet at exit: `Readers::quiet_within(MEDIA_QUIET_BUDGET)`, a `Condvar::wait_timeout` against one deadline, for first-frame readers still inside Media Foundation, before `MFShutdown` | `fn main` → `video::shutdown_media_session`, after the loop returns | **open** — found by the thread-door note's revision (k)9 and ruled interim (`DESIGN.md`, 2026-09-27, *five window-thread waits the thread-door survey found are registry rows 24–28*): retained on the window thread, with a configured wait deadline of 1.5 s (`MEDIA_QUIET_BUDGET`; the mutex taken first, the condition variable's reacquisition and scheduling can pass it, so it is not an elapsed-time guarantee, and the `MFShutdown` after it has no deadline); repaid by D-80's ticket as scope added to D-80's recorded one (reader quiescence and `MFShutdown`, amended by the thread-door note's revision (m), `DESIGN.md`, 2026-09-27, *the thread-door note's revision (m)*), unless the owner rules it stays as rows 15–17 do (version: owner); its door will be `MediaQuiet`, admitting the reader quiescence only |
| 28 | open | an update's trial takes the data directory's claim: `update_trial::take_the_claim_within` tries every 100 ms until the old build lets go | `fn main`, before `LaunchHandOver` (trials only) | **open** — found by the thread-door note's revision (k)9 and ruled interim (`DESIGN.md`, 2026-09-27, *five window-thread waits the thread-door survey found are registry rows 24–28*): retained on the window thread before the loop, bounded by `CLAIM_WAIT` (30 s) plus one last try; repaid by B11's startup claim ownership, unless the owner rules it stays as row 18 does (version: owner); its door will be `TrialClaim` |
| 29 | ruled to stay | an update's exit guard at the process's end: `update_handoff::leave_armed` runs the guard on a worker of its own and waits `recv_timeout(LEAVE_WITHIN)` (75 s) for its answer — the wait for the applier's decision, the election, the start, its acknowledgement, the fallback | `fn main`, after the loop (only after *Restart to update*) | **ruled to stay** (`DESIGN.md`, 2026-09-28, *every road process of an update leaves through one exit guard*): the loop has returned and every window is gone; the process must not end before the start that replaces it is delivered, and the wait is bounded; during it no window of this process exists, so the person sees Folio gone until the applier's trial or the replacement appears, and the message box appears when nothing was delivered: at the latest at the bound, earlier when every start has already failed. The box itself is shown on this thread after the wait (`bt_platform::standalone_alert`, U-32) and is synchronous: the process returns when it is answered or at the latest after `STANDALONE_ALERT_WITHIN` (15 min), when macOS takes its system alert away or Windows's ownerless `MessageBoxTimeoutW` returns |

**The doors** — one `bt_platform::admission::doors` type per line of the registry's `# doors` section; the station is the `hang_watch` station its meter enters. Every door takes its token by value, minted where the line says (A1d).

| door | row | station | admitted in | call | minted at | measures |
|---|---|---|---|---|---|---|
| `FontFamilyLookup` | 5 | `FontLookup` | Running | `bt_platform::monospace_family_named` | `settings::monospace_family_files` | one call |
| `PresentFrame` | 9 | `RenderCompose` | Running, Exiting | `WindowRenderer::present_frame_with_phases` | `Runtime::present_seats_and_commit` | one window's present |
| `CompositorCommit` | 9 | `CompositorCommit` | Running, Exiting | `bt_platform::Compositor::commit` | `Runtime::present_seats_and_commit`; `WebSeat::stand_parked`, `WebSeat::adopt`; `SpareSeat::advance`, `SpareSeat::retire` | one call |
| `CompositorBirth` | 9 | `CompositorBirth` | Running | `bt_platform::Compositor::new`, `bt_platform::spare_parent` | `Runtime::create`, `Runtime::open_window`, `Runtime::make_spare_web_controller` | the tree and its commit |
| `CompositorWindowSize` | 9 | `CompositorWindowSize` | Running, Exiting | `bt_platform::Compositor::set_window_size` | `Runtime::resize` | the ground and its commit |
| `SurfaceBirth` | 9 | `SurfaceConfigure` | Running | `WindowRenderer::new` | `Runtime::open_window` | the surface and its configure |
| `PtyBirth` | 11 | `PtyBirth` | Running, Exiting | `pty_door::spawn_shell` (`PtySession::spawn_shell_in`) | `create_leaf_session` | one call |
| `PtyResize` | 12 | `PtyResize` | Running, Exiting | `pty_door::resize` (`PtySession::resize`) | `commit_leaf_resize` | one call per leaf |
| `PlaceHidden` | 13 | `PlaceHidden` | Running, Exiting | `window_is_hidden` | `sample_window_place` | one call |
| `PlaceExposure` | 13 | `PlaceExposure` | Running, Exiting | `window_is_exposed` | `sample_window_place` | one call |
| `TitleFlush` | 14 | `WindowTitle` | Running, Exiting | `owner_door::set_title` | `Runtime::flush_title` | one call |
| `PaneRetirementWait` | 15 | `PaneRetirementWait` | Exiting | `pty_door::wait_for_retirements` (`bt_pty::wait_for_retirements`) | `FolioApp::settle_quit`, `Retire` | one call |
| `SessionWriteWait` | 16 | `SessionWriteWait` | Exiting | `SessionWriter::wait_for` | `SessionStore::wait_for_landing` | one call |
| `SessionWriterRetire` | 16b | `SessionWriterRetire` | Exiting | `SessionWriter::close` | `SessionStore::close` | one call |
| `TraceFlush` | 17 | `TraceFlush` | Exiting | `trace_sink::flush` | `fn main`; `trace_sink::Shutdown::drop` | one call |
| `UpdateLeave` | 29 | `UpdateLeave` | Exiting | `update_handoff::leave_armed` | `fn main` | one call |
| `LaunchHandOver` | 18 | `Starting` | Starting | `launch_wire::hand_over` | `fn main` | one call |
| `WebController` | 21 | `WebController` | Running, Exiting | `WebHost::request_controller` | `WebSeat::step` | one call |
| `WebEnvironment` | 21 | `WebEnvironment` | Running | `WebHost::request_environment` | `WebSeat::start_environment` | one call |
| `WebRehost` | 21 | `WebRehost` | Running | `WebHost::rehost` | `WebSeat::rehost` | the steps and their commits |
| `ImeCaretArea` | 22 | `ImeCursorArea` | Running, Exiting | `owner_door::set_ime_cursor_area` | `Runtime::apply_ime_cursor_area` | one call |
| `GpuOpen` | 23 | `GpuOpen` | Running | `gpu_door::open_first_window` (`pollster::block_on(GpuContext::open)`) | `Runtime::create` | one call |
| `FocusWindow` | §5.2 | `WindowFocus` | Running | `owner_door::focus_window` | `Runtime::open_from_notification` | one call |
| `SetVisible` | §5.2 | `WindowVisible` | Running, Exiting | `owner_door::set_visible` | `Runtime::put_the_window_on_the_glass`, `Runtime::hide_quake_window`, `Runtime::let_go_of_this_window` | one call |
| `SetCursor` | §5.2 | `WindowCursor` | Running | `owner_door::set_cursor` | `Runtime::apply_pointer_cursor` | one call |
<!-- window_waits.tsv: end -->

Row 21 after ticket 54: the ruling of 2026-09-24 warmed the environment on the
premise that it brings the runtime's processes up. A windowless probe over the
product's options found that it does not (no descendant process for eight seconds,
an empty profile folder, +2.2 MB in Folio's own process), so the first page's
residual on this thread is still `request_controller` and the pump after it —
ticket 43's 88–269 ms and 70–303 ms headless, the owner's 4,099 ms on next89 —
above a frame, and the row is narrowed, not done.

Row 13's taskbar probe, stated after its cell (ticket 62): `sample_window_place`
still asked `bt_platform::taskbar_is_auto_hidden` — `SHAppBarMessage`, a message
to Explorer — on the window thread, at every turn's head and again for a delivery
between turns; the owner's next93 stall report has two of those asks at 99 ms and
92 ms inside one 535 ms hold. **Done** — *whether the taskbar hides itself is asked
on a lane of its own and read from a numbered slot* (`DESIGN.md`, 2026-09-25):
`taskbar_lane` asks on its own thread (at launch, every 5 s while a window is on a
screen, and when Windows says a system setting moved) and the window thread reads
the latest answer with one atomic load (D-68, repaid).

Row 5's road, stated more exactly than its cell (ticket 50): the walk was reached
from `apply_stored_terminal_font` on the launch road — `FolioApp::create`, before
the first frame, for every launch whose `settings.json` names a terminal font
family — and on the face-change road, `Runtime::adopt_terminal_font` (the
Settings row's answer, a sibling window's `adopt_application_change`, and the
`FontsScanned` arm), whenever the picker's list was still the seed.

**The window thread never does a blocking `recv()`.** Every lane is drained with
`try_recv` in a loop, and `main.rs` contains no `recv()` and no `join()` outside
the worker bodies. The bounded `recv_timeout`s of rows 15–17 are all on the way
out. This is a rule, and rows 15–18 are its whole exception set.

**The wait budgets.** At least sixteen named budgets live in six crates
(`HANDOVER_BUDGET`, `OUR_TURN`, `SESSION_SAVE_BUDGET`, `FLUSH_TIMEOUT`,
`CHILD_EXIT_BUDGET`, `READER_EXIT_BUDGET`, `RETIREMENT_BUDGET`,
`PANE_RETIREMENT_DEADLINE`, `BROWSER_EXIT_DEADLINE`, `FIRST_FRAME_BUDGET`,
`SHUTDOWN_BUDGET`, `OPEN_BUDGET`, `GIT_COMMAND_TIMEOUT`, `REMOVAL_TIMEOUT`,
`STDIN_BUDGET`, and wgpu's unreachable hard-coded frame timeout). **Only the
four in rows 15–18 may be spent on the window thread.** A timeout does not
cancel an uninterruptible call; a stuck operation needs outstanding-work
accounting and a limit on further admissions.

**Every turn is accounted, and every admitted wait is measured per call**
(0.4.6 ticket A3; the budget note's §R-C and §C-7). The meter's `leave`
(`hang_watch::ADMISSION_METER`) folds each admitted call's inclusive duration —
the two instants `admitted` read around its work, in nanoseconds — into its
door's histogram (count, sum, maximum, 24 power-of-two buckets). At `park`,
before the slow-hold threshold and whatever any queue takes, the turn's wall
time, the union of its admitted calls, its unexplained time (wall time less that
union: a named station with no admitted call under it is unexplained too) and its
scheduling delay (the turn's start less the wake the loop parked for) go into
per-run atomics (`hang_watch::accounting`). The union is exact without a buffer:
admitted calls nest on one thread, so it is the sum of the outermost calls, and
the cookie carries the nesting depth. Four triggers each offer a budget line:
the wall time past the turn's frame (`T − t₀`, the shortest frame among the
visible windows that took a turn, else `TURN_BUDGET`, 16 ms), the union past
`WAIT_BUDGET` (8 ms), a call past its row's bound (`WAIT_ALLOWANCE`, 4 ms; the
registry rules no larger bound; the line names the turn's longest), and the
unexplained time past `WAIT_BUDGET`. Each trigger writes at most one line a
second (`COALESCE_WINDOW`, coordinator's ruling 2026-09-27); the turns inside
that second are counted and the trigger's next line carries the count. The
window thread `try_lock`s a ring of 16 lines and counts every refusal; the
watchdog writes them (§10). `fn main` writes the exit summary from the atomics
before the run's last line. The accounting reads no clock of its own, and a
parked loop runs none of it. `hang_watch` reports and never intervenes: a line
is a finding against its row, not a trigger for anything.

**Deferrable work yields to the earliest window's deadline** (0.4.6 ticket A4;
the budget note's §R-B): each turn fixes one `TurnAllowance` from its start — the
earliest next frame boundary among the windows on the glass whose `FrameClock`
is running, less `PRESENT_RESERVE` (2 ms), else `TURN_BUDGET` from the start —
and the search walk's slice, the web engine's warm-up ask and the spare web
controller's making and drain each ask `hang_watch::deferrable` before their
unit and keep their work for a later turn when nothing is left; input, the drain
and the present never ask, and the allowance moves no deadline.

### 5.4 The shippable migration order

1. **Before the move** — define request identity, resource ordering, capacity,
   completion, abandonment and wake obligations. Wrap the existing
   implementations; do not rederive their policy (`CONVENTIONS` §十 rule 9).
2. **0.4.4** — external hand-offs (row 1) and the synchronous integration
   mutations (rows 2–3). Narrow inputs, visible results; this is where the
   reusable contract is established.
3. **0.4.4** — the remaining observations and storage operations (rows 4–8,
   13–14, 20) on the same protocol.
4. **0.5** — presentation ownership (rows 9–10), separately, preserving the
   resize debt, the atlas lifetime and the presented-picture accounting. Retain
   `bt-render`'s designed surface-lease handshake and shared preparation permit —
   `GpuContext` owns the shared fonts and atlas, `WindowRenderer` owns per-window
   surface state, and the picture commits only after matching completion **and** a
   successful owner-thread compositor commit, as `present_seats_and_commit` does
   synchronously today. Surface configuration and GPU preparation remain residual
   owner-thread costs in the first cut, and the platform acquire affinity is a
   prerequisite. **Do not advertise the first cut as eliminating every
   window-thread stall.**
5. **0.5 toward 0.6** — PTY birth, resize and lifetime behind the session owner
   (rows 11–12), preserving input and resize ordering. A separate keyboard thread
   is not a prerequisite; keeping the existing event owner runnable is.

Each step retains its existing implementation behind the new door and ships
independently.

---

## 6. Doors

**The admission rule: a new side effect gets a door.** A door is the single
named entrance to one kind of effect, so that the question "where does this
happen" has one answer and a guard can hold it.

| effect | door | what holds it |
|---|---|---|
| restoring the program-owned mode state of one terminal pane | `TerminalAdapter::reset_program_modes(PtyTransport)`, reached only through the owning `DualPlaneSession::reset_program_modes`, which applies the screen switch through the session's own `apply_events` and moves `screen_revision`; the one app caller is `Runtime::reset_terminal_modes` (pane menu and command palette), which names the transport from the pane's `ConPtyKind`. It mutates the session-owned terminal on the thread that owns the session, writes no bytes to the child, starts no process and waits on no thread | `reset_program_modes_restores_the_shell_without_writing_to_it`, `reset_program_modes_leaves_focus_reporting_where_the_transport_keeps_it`, `a_reset_during_an_open_resize_survives_the_reconcile`, `a_reset_session_is_back_on_the_primary_screen_and_hears_the_next_prompt` in `bt-term`; the pane-menu and palette pins in `bt-app` |
| reading file bytes | `bt_platform::file_reads` — thirteen named lanes (U-13 added `UpdateJournal`, the trial's watch; U-14 added `Update`, the archive and its manifest), `Lane`, `Ledger::add`, the process-wide `LEDGER` | `file_reads_doors.txt` admits items as a set of keys, without per-site counts, plus a source guard |
| reading who owns an install folder, and the macOS install-marker attribute | `bt_platform::install_evidence` — `owner_of`, `current_account`, `attribute` (read-only: `GetNamedSecurityInfoW` and the process token on Windows, `stat`, `geteuid` and `getxattr` on Unix); the attribute's bytes are charged to `file_reads`' `Lane::Install` | its own module, one function per read; its one caller is `install_channel::read` |
| reading a resource out of an executable without running it (E-14) | `bt_platform::pe_resource::read_rcdata(path, name, limit)` (U-14) — `LoadLibraryExW(LOAD_LIBRARY_AS_DATAFILE \| LOAD_LIBRARY_AS_IMAGE_RESOURCE)`, `FindResourceW(RT_RCDATA)`, a copy, `FreeLibrary`; charged to `file_reads`' `Lane::Update` as one opaque load; `Unsupported` off Windows | its own module; its one caller is `update_archive::EmbeddedManifest` |
| reading a keyboard layout's Shift table | `bt_platform::keyboard_layout_shift_table(&WorkerCtx, layout)` (T-KEYBOARD-CTRLALT round 4) — `RegGetValueW` reads the layout's `Layout File`, `LoadLibraryExW(LOAD_LIBRARY_SEARCH_SYSTEM32)` loads it, `GetProcAddress(KbdLayerDescriptor)` exposes its tables, and the copied Shift column outlives `FreeLibrary`; Windows only, worker only | `window_waits.tsv`'s `worker-door-body` effect row and `GetProcAddress` owner; the `WorkerCtx` parameter is the refusal pin, `layout_tables` has the one product call, and no §5.3 window-thread row is added |
| deciding whether a downloaded Windows release is the same publisher's Folio, and whether this build may update itself | `bt_platform::trust` (U-15) — in process, no child: `WinVerifyTrust` (`WINTRUST_ACTION_GENERIC_VERIFY_V2`) and its provider data, `CryptVerifyTimeStampSignature`, `CertGetCertificateChain` / `CertVerifyCertificateChainPolicy`, `CertNameToStrW`, `GetFileVersionInfoW`, and `IAppxFactory`'s package reader for `folio.msix`; `verify_release_file`, `verify_release_package`, `verify_sidecar`, `running_identity`, `running_capability`. Each call's reads are charged to `file_reads`' `Lane::Update` as one opaque read; any revocation fetch is Windows' own, inside those calls. Opens memory certificate stores only and writes no store; `Policy::ExclusiveRoot` (tests) is a `CertCreateCertificateChainEngine` over a memory root store. Worker only; `Unsupported` off Windows | its own module; no product caller until the Windows Prepare (U-20); `update::tests::the_trust_door_opens_no_system_certificate_store` pins its stores |
| creating a file only where nothing of its name exists, under a directory held open | `bt_platform::exclusive_create::Directory` (U-14) — `open` (backup semantics and `FILE_FLAG_OPEN_REPARSE_POINT`, without `FILE_SHARE_DELETE`; `O_DIRECTORY \| O_NOFOLLOW` on Unix), refusing a link; `create_new` (`NtCreateFile(FILE_CREATE, FILE_OPEN_REPARSE_POINT)` relative to the held handle; `openat(O_CREAT \| O_EXCL \| O_NOFOLLOW)`); `rename_new` (`NtSetInformationFile(FileRenameInformation)` relative to the handle, never replacing; `linkat` then `unlinkat`); a name is one component; worker only | `exclusive_create::tests` over real folders; its one caller is `update_archive::expand` |
| making an update transaction durable, and the installation's two locks | `bt_platform::install_txn` (U-11) — `durable_write` (a temporary file beside the target → write → `FlushFileBuffers` / `F_FULLFSYNC` → rename over the target → the directory flushed: `FlushFileBuffers` on a handle opened with `FILE_FLAG_BACKUP_SEMANTICS`, `F_FULLFSYNC` on the directory's descriptor), `durable_move` (`MoveFileExW(MOVEFILE_WRITE_THROUGH)` / `renamex_np(RENAME_EXCL)`, never over an existing file, then both directories flushed), `flush_current_user_key` (`RegFlushKey`, Windows only), `durable_remove` (a file or a directory tree removed, then its directory flushed; nothing there is success — U-12), `durable_create` (`durable_write`'s steps with a rename that never replaces: the trial's receipt, on the storage worker — U-13), `durable_create_dir` (a folder that must not exist yet, then its parent flushed: the macOS home and a transaction's folders — U-27), `durable_copy` (`durable_create`'s steps with the bytes streamed from a reader: the Windows Prepare's staged set and rescue copy — U-20), `available_bytes` (`GetDiskFreeSpaceExW` / `statvfs`: the Windows Prepare's reservation — U-20), `try_hold` / `hold_within` with `Hold::Shared` (read-only open, existing file) or `Hold::Exclusive` (`LockFileEx` / `flock`, released by `Held`'s drop); three arms: Windows, macOS, and a refusal naming the door everywhere else; every failure names its `Stage`; worker only, except the start's update pass before the loop exists (`try_hold` and `durable_remove`, §5.1) | `install_txn::tests`: the order over a recording fake of its `Surface` trait, the real arms over temporary folders, the locks across two processes |
| the update's entrance at logon: one registry value | `bt_platform::logon_hook` (U-22) — `arm` (the command `"<H>\<txn>\rescue\folio.exe" --update-recover` measured against the documented 260 characters before anything is written, then `FolioUpdate-<txn8>` written under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (`RUN_KEY`), `RegFlushKey`, read back and compared byte for byte; only then the proof `logon_hook::Armed`, which `update_txn::Event::Armed` carries, so the journal cannot record `Armed` first), `disarm` (removed and flushed; absent is success), `clean` (`--uninstall-cleanup`'s per-copy row: a `FolioUpdate-*` value naming this copy's home or a vanished program); the key and the value prefix are the only registry surface the updater writes; the `_in` forms take the key and a `Registry` so tests use `HKCU\Software\Folio-Test\<random>` or a recording fake; Windows only, a refusal naming the door elsewhere; every refusal names its `Stage` | `logon_hook::tests` (the order over a recording fake; the real registry under a key of the test's own); the uninstall inventory row `Update entrance` |
| an update's entrance that survives a power cut, macOS | `bt_platform::launch_agent` (U-26) — `arm(agents, txn, rescue_exe, home)` writes `io.github.lulu-loopp.folio.update-<txn8>.plist` (a fixed template: `Label`, `ProgramArguments = [rescue_exe, "--update-recover", home]`, `RunAtLoad`) through `install_txn`'s durable write (file and folder `F_FULLFSYNC`), reads it back through `file_reads`' `Lane::Update` and compares, and only then answers `install_txn::Armed`, the proof the journal records `Armed` on; `disarm` removes it (absent is success) and flushes the folder; `sweep` removes every plist of that exact name shape for `--uninstall-cleanup`; the folder is a parameter; no `launchctl` call; arming refused by name off macOS | `launch_agent::tests`: the order over `install_txn`'s recording fake (`armed_follows_a_full_fsync_of_the_plist`), the template through `plutil -lint` on macOS, the sweep and `disarm` over temporary folders |
| the rescue clone of a macOS bundle | `bt_platform::macos_update::rescue_clone(old_bundle, clone)` (U-26) — `clonefile(CLONE_NOFOLLOW \| CLONE_NOOWNERCOPY)`, or `/usr/bin/ditto` on `ENOTSUP` / `EXDEV`; then `codesign --verify --deep --strict -R=<the old bundle's designated requirement>` and the clone's cdhash equal to the old one's; answers the clone's main executable; a clone that fails the check is removed; refused by name off macOS | `macos_update::tests`: a tiny ad-hoc-signed synthetic bundle cloned on APFS (byte-identical executable), another build refused |
| mounting the update image, macOS | `bt_platform::macos_update` (U-17) — `attach(worker, image, mount_dir)` runs `hdiutil attach -nobrowse -readonly -noautoopen -mountrandom` and answers a `Mount` (no `Clone`, no `Drop`, `#[must_use]`) that only `detach(worker, mount)` consumes; `with_image` attaches, hands the mount point to its body and detaches whatever the body answers; `mounts_under(root)` reads the mount table (`getfsstat`, `MNT_NOWAIT`) for every mount point strictly below `root`'s real path, so a mount under the home is found with no record, and `detach_all_under` is M1's step before `H/<txn>` is deleted; refused by name off macOS | `macos_update::mount_tests`: every exit road over a stand-in `hdiutil` with its own mount table, the attach's deadline, the device-table grammar from fixture output, and on macOS a real image attached under a temporary home and found without a record |
| exchanging the installed macOS bundle with the staged one, the processes running from a file, and whether a file is held open | `bt_platform::install_flip` (U-28, U-23) — `exchange(live, staged)`: `renamex_np(RENAME_SWAP)` through `install_txn`'s `Surface` (`Replace::Swap`, `durable_exchange_with`), then `F_FULLFSYNC` on each folder; Windows' arm refuses a swap by name (its flip is one `install_txn::durable_move` per file), and so does every platform without an arm. `running_from(executable)`: macOS `proc_listallpids` and `proc_pidpath`, matched by device and inode, each with its start instant (`proc_pidinfo` `PROC_PIDTBSDINFO`); Windows (U-23) `K32EnumProcesses` and `QueryFullProcessImageNameW`, matched by volume serial number and file index, each with its creation time (`GetProcessTimes`; a process that has exited is not running); `still_running`, `started_of`; **`parent_of_this_process` (0.4.7 U-37)** — this process's parent by pid and start instant, only when it started before this one (macOS `getppid`, Windows the process snapshot's parent pid, `CreateToolhelp32Snapshot`): the recovery build's own starter, which is no candidate; read only, no wait, not reachable from the window thread; refused elsewhere. `held_open(path)` (Windows, U-23): an open for reading and writing with no sharing, closed at once — a sharing violation (any other handle, or a running image's section) is `true`; E-7's process check. **Since U-29 one effect on a process**: `ask(process, images, Ask::Quit | Ask::End)` sends `SIGTERM` or `SIGKILL` only after `runs_from` shows that pid, with that start instant, running from one of `images` — the rollback's stop of a trial LaunchServices started (not the stopper's child). **Windows since U-24**: the process opened (`PROCESS_TERMINATE`) and its creation time read again from that handle, so a pid reused since the list is never touched; `Quit` posts `WM_CLOSE` to each of its visible, unowned, non-tool top-level windows (a person's close; Folio listens to no other quit road, so a process with no such window is asked by nothing and its grace runs out), `End` is `TerminateProcess` on the same handle; refused elsewhere. | `install_flip::tests` (the order over the recording fake, a real exchange of two folders, this test process found by its image, a started synthetic program listed while it runs and not after, a running image held open) and `update_apply_macos::tests`, `update_apply_windows::tests` |
| removing a program's own files once the processes that hold them have ended | `bt_platform::deferred_removal::schedule(&Removal, scripts)` (0.4.7 T-UNINSTALL-UX) — starts the remover (§2.2) and returns; removes exactly the `Item`s it is handed, and `folder` only if empty; what the items are is `uninstall::program_plan`'s, derived from the running executable alone (the release manifest's members, the executable, the Explorer package, the install marker and `.folio-update` on Windows; the bundle on macOS; the executable elsewhere). A link at an item or bundle name, or below it in an owned tree, refuses the plan; ancestors above that name are canonicalized | `deferred_removal::tests` (a real remover over a temporary folder, waiting on a real child); `uninstall::tests` (the plan, the refusals, the end-to-end road, including a refused linked bundle and a planned bundle below a linked ancestor) |
| a worker's sleep | `bt_platform::wait::sleep_within(&WorkerCtx, Duration)` (U-28; thread-door note (j)13) — one `std::thread::sleep`, asked for with the worker's capability, so no window thread can call it | `window_waits.tsv` `# effects` row `worker-door-body` / `WorkerCtx`, no admission identity; `hang_watch::window_waits_tests` counts its body as a door, never as inventory |
| the runtime folder's listing (Unix) | `bt_platform::instance::sweep_stale_claims_in(&WorkerCtx, …)` (U-43) — one `std::fs::read_dir` of Folio's runtime folder, on the start's `folio-claim-sweep` worker; its per-file `sweep.guard` lock is exclusive and non-blocking, as the claim's shared lock is, so neither side adds an owner-thread wait and a contended claim answers `ClaimRefusal::Sweeping` | `window_waits.tsv` `# effects` row `worker-door-body` / `WorkerCtx`, arm `[unix]`; counted as a door, never as inventory |
| observing the foreground program below a pane's shell | `bt_platform::foreground_program::foreground_program(&WorkerCtx, shell_pid)` (T-PANE-COLUMNS E8) — one Windows ToolHelp snapshot, narrowed to the shell's own descendants before any process is opened (an entry older than the parent it names is a reused pid, not a child), plus the pid set returned by the short-lived `folio.exe --console-members <shell-pid>` helper; chooses the deepest, then youngest descendant that shares the pane console, and a console host (`conhost`, `OpenConsole`) is never an answer or traversal node. A recursive macOS `proc_listchildpids` walk remains the youngest-leaf walk. It returns one canonical image or `Unknown`, stops at WSL/ssh boundaries, retains no process tree, and refuses unsupported targets by name. `bt-app::foreground_program::run(&WorkerCtx, …)` owns the request channel's blocking receive on the same worker | the door and helper's authority is their `&WorkerCtx` signature; `bt-app::foreground_program::run` and `bt_platform::wait::sleep_within` are the `window_waits.tsv` `# effects` rows `worker-door-body` / `WorkerCtx`, with no owner-thread admission identity; compile-fail authority, unsupported-target, fake-tree membership/order, console-host, and real child→named-grandchild pins; the application calls the platform door only on `bt-foreground-program-worker` |
| constructing a child process | `bt_platform::quiet_command_named` (and `quiet_command`) — absolute path resolved by `handoff::program_on_path` | pinned as the only `Command` construction |
| handing something to the operating system | `bt_platform::handoff` — the only `ShellExecuteW` and `NSWorkspace` sites in the workspace; the seven verbs are private to it and reached only through `ShellThread::hand_over`, and a `ShellThread` is entered only with the `WorkerCtx` the thread door lends (A1b), so a hand-off can happen only on a thread the door started | its own module, one function per verb; `compile_fail` doctests on `hand_over` name each verb by both spellings; `handoff_lane::no_handoff_runs_on_the_window_thread` |
| reaching the network | `bt_platform::http` — `https_get` (one `GET` into memory: the update check) and `https_download` (one `GET` streamed to a file under a ceiling, U-7), over the operating system's own stack (WinHTTP, `NSURLSession`), `https` only, no caller headers; the download's ceiling, temporary file, deadlines and stage vocabulary are `bt_platform::https_download`'s, shared by both real arms | `update_check_transport_tests` holds the three arms to one signature per door and one set of request types |
| starting a thread | `bt_platform::spawn_at_priority` / `spawn_at_priority_with_stack` (one definition, in `admission`) — a `&'static` name and a priority band; the new thread is `Worker(name)` (§5.1) and its body is `FnOnce(&WorkerCtx) -> T`, lent the capability a worker-only door takes (0.4.6, A1b) | the source guard's assertion `every_thread_bt_app_and_bt_platform_start_comes_through_the_thread_door` (`hang_watch::window_waits_tests::every_door_is_where_the_registry_says`, since A1e; A1c wrote it) finds `std::thread` spawning named in `bt-app`'s and `bt-platform`'s product code only inside the door; `admission`'s tests start their workers through it |
| waiting on the window thread (an owner-thread wait) | `bt_platform::admission::admitted` — a `WaitToken` for one door type of `admission::doors`, admitted only on the window thread and in that door's phases (§5.1) | the registry `window_waits.tsv`, held equal to the door types by `hang_watch::window_waits_tests`; every door's function takes its token by value (A1d), and `tests::every_owner_door_takes_its_own_token_by_value` checks each signature; the source guard (§5.1, A1e) holds every function that takes a token to one registry door and to running its effect inside its own call |
| creating a native window outside the framework | `bt_platform::SpareParent` / `spare_parent` — the spare web controller's never-shown `WS_POPUP` parent (ticket 60); dropped only on a pumping thread, left to process exit by an orderly stop | the one `CreateWindowExW` in product code, pinned by `web_spare::spare_wiring_tests::the_spare_parent_is_the_one_window_product_code_creates` |
| taking a native window's messages away from the framework | `bt_platform::let_the_system_translate_touch` — the touch subclass that hands `WM_TOUCH` and the three `WM_POINTER*` to `DefWindowProc` | a message table pinned by test; called once per window, from the two `create_window` sites |

Two corollaries. **Each lane declares itself**: a new kind of read joins
`file_reads` with a named lane rather than reading bytes beside it. **The worker
produces the door's input with the door's own function**, never a second
derivation of it.

**What stays pending until A2e** (the note's revisions (c)1, (c)6, (i) and (j)).
The source guard of §5.1 fences the escapes around the doors; nothing yet
refuses a raw effect written outside one — **the invariant of budget note C-1 is
pending**. What A2a established (2026-09-26) is the debt, frozen: **242 bare
sites** — every vocabulary effect in the product outside a registered door's
body, by crate, `cfg` arm, item and entry — in
`docs/plans/window-thread-bare-sites.tsv`, held equal to the code by
`window_waits_tests::every_bare_site_is_a_row_and_every_row_a_site` and held to
shrinking against the merge base by `scripts/ci/check-window-waits.ps1`, which
compares totals per `(crate, effect entry)` — so a move or a rename passes and a
new family or a grown total is refused — and prints the total in its footer.
Since T-GATES-047 there is no seed: a merge base without the inventory is an
error, not a baseline.
**The count is a reading of the source, not the compiler's** (note (j)12): a
method call counts when its receiver's type is written somewhere on its road,
so a receiver typed nowhere is not seen, and A2e's lint may find sites the file
never listed. Beside it: the registry's `# effects` section (§5.3); the two mints
fenced by reference (the source guard's assertion 2); the **configuration
fence** — five `clippy.toml` files, three of them shields, no `.clippy.toml`,
no `.cargo/config`, no `CLIPPY_CONF_DIR`, no flag lowering the lint, only the
workspace's and `bt-platform`'s lint tables naming it; the **exclusion forms**
at every Cargo target root (`#![cfg_attr(test, allow(clippy::disallowed_methods))]`
at product roots, `#![allow(clippy::disallowed_methods)]` at tests, examples,
build scripts and development binaries); and **the lint probe**,
`crates/bt-lint-probe`, whose **positive control**
(`scripts/ci/check-lint-probe.ps1`, on Windows and macOS) proves each vocabulary
entry assigned to a target is resolved and reported there. Still A2b–A2e's, and
not claimed here: **the lint on raw effects** (`disallowed_methods` denied in the
two lint tables, the vocabulary in the root `clippy.toml`, one `expect` per
`# effects` row); **the `file_writes` and `wait::*` doors**; **`file_reads`'
execution-level design**; and **the transport doors inside `bt-pty`**.

**D-2's state** (`docs/plans/structural-debt.md`). A1 (A1a–A1e), A3 and A4 have
landed: every owner-thread wait is a registry door admitted with its token,
every turn and every admitted call is accounted, and deferrable work yields to
the earliest window's deadline (§5.3). What A4 leaves of aggregate scheduling is
D-84. A2 is pending — its
survey counted 228 bare product sites on the Windows arms (the thread-door
note's revision (i)), and it lands in five tickets with the lint last. By the
owner's ruling of 2026-09-25 D-2 closes when A2 has landed.

Each door is held by a pin: `bt_app::file_reads_source_tests` reads
`file_reads_doors.txt` and fails the build when a product read appears in an
item the file does not admit. A row is a key — an item (`Type::method: .verb`)
or, for the rows still on MIGRATION-DEBT, a file and a function
(`trace.rs: create: .open`) — and the rows are a set with no occurrence counts,
so a second read inside an admitted item is not a new row; the lane each
counting adapter charges is checked separately. `quiet_command_named` is pinned as the only `Command`
construction; `handoff` holds the only `ShellExecuteW` and `NSWorkspace` sites.

**The known bypass, stated as a fact.** `docs/BT-ENVIRONMENT.md`'s file-read
self-report declares that **directory enumeration and metadata are excluded**
from the ledger's accounting. That is a statement about what the byte counters
measure. It is *not* a decision that directory enumeration needs no door — and
the consequence is that the files column's `bt_app::files::read_directory` has
no lane, no door and no guard, and nothing rules whether it should. Until that
is decided, the ledger's totals are not an account of what this process reads
from disk, and the next enumeration-shaped effect will land the same way.

**The thread door's bypass is repaid** (0.4.6, A1c). `folio-web-thumb` and the
five unnamed `std::thread::spawn` sites that skipped `spawn_at_priority` now
start through it, and so do `bt-platform`'s twelve. That covers `explorer_menu`'s
`begin_probe` and `run_request` in the window process, and
`remove_from_explorer_menu`, `cleanup_registrations` and
`attention_wire::payload_on_stdin` in door processes. **A door process's threads
owe the door** (the thread-door note's revision (b)5): their effects are
first-party effects every process must show. Each door process's main thread
waits for its thread as a worker, entered once through `enter_standalone_main`.
Each thread kept its band, so the ones RULES 53 does not except still stand at
`Normal` until its 0.4.7 ticket.

---

## 7. Cross-crate chains

A chain is a decision that no single module owns. **Adding a hop edits this
list.**

### 7.1 The printed path — recognition to hand-off

A bare absolute path a program printed in the terminal, made clickable.

| hop | crate | entry | carries | lane |
|---|---|---|---|---|
| recognition | `bt-transcript` | `paths::detect_absolute_path_candidates`, `paths::may_read_unasked`; an agent's `[Image #k]` placeholder through `paths::image_placeholder_ranges` against the pane's learned `ImagePlaceholderTargets` (T-IMAGE-N) | `PrintedPathCandidate` | the feed, synchronously |
| verdict | `bt-term` | `session::verify_path`, `DualPlaneSession::ask_about_reprinted_path` / `re_ask_about_link_target`, the `path_verdicts` ledger; beside it the `image_placeholders` table, learned by `learn_image_placeholders_from_fresh_rows` from `OSC 8` labels | `PathVerdict` | asked on `bt-path-verify-worker`; the ledger and the table are per pane and bounded |
| projection | `bt-viewport` | `implicit_hyperlinks`, `mark_osc_8_dotted`, `ViewportFrame::hyperlink_at` | `CellHyperlink` (defined in `bt-transcript`), `HyperlinkHit` | window thread, at frame build |
| activation | `bt-app` | `Runtime::activate_hyperlink`, `hyperlink_activation` → `reference_activation` (the one table; a previewed document's links read it too, through `preview_link_activation`), `verified_target_of` | `bt_platform::VerifiedTarget` | window thread, from `mouse_input` |
| hand-off | `bt-platform` | `handoff::resolved_for_a_door`, `handoff::open_local_path_verified`; a share on another machine under `Ctrl` through `handoff::open_local_path` (no ledger — a share is never asked about); any other scheme through `handoff::shell_execute` | the OS's acceptance or refusal | the OS hand-off lane (`bt-app::handoff_lane`), answered through `AppEvent::HandoffAnswered` |

The chain **should remain layered** — five crates are not five excessive
dependencies. Its defect is the absence of one current contract covering
recognition, namespace, observation freshness, gesture policy, activation and
OS completion. That contract is `docs/RULES.md`'s printed-path row.

Three things the chain already rules and a ticket must not re-derive.
**Freshness is asymmetric** — a "yes" is never re-asked and expires only at the
command boundary, a "no" is re-asked whenever the program prints the name onto a
freshly changed row (a repaint is not a printing), and the press puts the
question again as its own re-check. **A path carries its namespace** — the pane
recognises paths in its own shell's namespace only, read from the profile's
directory-namespace and integration pair and never guessed from the text; a
remote `PathBuf` must not silently become a path on the viewing machine.
**The worker produces the door's input with the door's own function** —
`handoff::resolved_for_a_door` is the single answer, consumed by both
`bt-term::session::verify_path` and `run_path_verify_worker`, never re-derived.
`bt-transcript::paths::may_read_unasked` and `Runtime::activate_hyperlink` are
where the namespace boundary is visible.

### 7.2 The other chains — first pass, 2026-09-23

Named so that the list exists and a hop cannot be added silently. Each line is
the hops as the code runs them today, in order, with the lane; each ends with
where the single current contract is written, or that it is not. None of them
has the §7.1 table yet.

- **Attention marks** — three ingress lanes: the escape sequence
  (`bt-term::adapter`'s `AdapterEvent::AttentionRequest` → `lifecycle` →
  `DualPlaneSession::apply_attention_request`, a level read off the status
  snapshot during `Runtime::drain_pty`); the endpoint (`bt-platform::attention_pipe`
  on `folio-attention-endpoint` → `attention_wire::park` → `AppEvent::AttentionSpoke`
  → `attention_wire::take`); and the `folio attention` verb process, which
  writes to that endpoint. They converge at `bt_workbench::attention::AttentionLedger::apply` through
  `main.rs`'s `settle_attention` and `deliver_attention`, then
  `notify::desktop_reach` → `Runtime::raise_attention` → `notify::interruption`
  (a flash or a desktop toast); input answers through `answer_attention` /
  `answer_attention_in` and `mark_attention_seen`; `TabState::mark_state`
  paints the tab mark at frame build. Everything after the endpoint runs on the
  window thread. Contract: `docs/RULES.md` §29, reach also in §30. Which
  function paints the per-pane mark: not traced.
- **Resize** — `ResizePlan` and `DualPlaneSession::resize_at`, the viewport's
  reflow, `bt-pty::PtySession::resize`, and roughly ten free functions in
  `main.rs` that sequence them. The ordering `flush_pending_pty_resize`
  represents is the contract.
- **Paste** — `runtime/clipboard.rs`'s `paste_from_clipboard` →
  `bt_platform::clipboard_payload`, **synchronous on the window thread**
  (`Station::ClipboardRead`) → `prepare_clipboard_paste` (paths through
  `shell_literal`) → `deliver_paste` → `stage_paste` (send, the input line, or
  held behind the multi-line card) → `send_paste` → `paste_text` →
  `bt-term::input::paste_bytes` (bracketed or not) → `offer_pty_input` →
  `PtySession::write_with_reason` → `InputRing::try_push` → `pump_pty_input` on
  the pane's writer thread. Contract: `docs/RULES.md` §9 and `deliver_paste`'s
  doc comment ("all four doors arrive here").
- **Preview** — `open_preview_file` → `open_preview_source_on` → the
  `PreviewWorker` on `bt-preview-worker` (the `file_reads` lane `Preview`) →
  `AppEvent::PreviewReady` → `drain_preview_answers` → `apply_preview_results`
  into the `PreviewBuffer`; formulas go to `bt-math-worker` and come back as
  `MathReady` → `apply_math_results`; `preview_watch` on `bt-dir-watch` →
  `PreviewFileChanged` → `advance_preview_watch` → `refresh_preview_file` asks
  the worker again; `save_preview_on` → `PreviewBuffer::save` runs
  **synchronously on the window thread** (`Station::PreviewSave`, §5.3 row 20).
  Contract: `docs/RULES.md` §18 (what a preview shows) and §19 (the buffer and
  its save); the load, watch and math sequence as one contract: not written.
- **Web pane** — `open_web_page` → `open_web_page_on` → `webhost::WebSeat::open`
  → `bt_platform::WebHost` (WebView2, or WKWebView), on the window thread; the
  host queues the engine's events and wakes the loop with
  `AppEvent::WebPageSpoke` → `drive_web_page` → `WebSeat::drive` →
  `apply_web_outcomes`; page pictures go to `web_thumb::PageShrinker` on
  `folio-web-thumb`, which wakes with the same event; `hand_url_to_the_browser`
  → `Runtime::hand_off` → the OS hand-off lane. Contract: not written —
  `docs/RULES.md` §49 is not yet folded. Which thread the engine's callbacks
  run on: not traced.
- **Settings** — `settings_mouse_input` → `apply_settings_choice` /
  `apply_settings_choice_announcing` (both still in `main.rs`) → the row's
  `*_requested` decoder in `settings.rs` → its `apply_*` →
  `persist::SettingsStore::store` → `bt_persist::write_settings_atomic`,
  **synchronous on the window thread** with no debounce
  (`Station::SettingsWrite`, §5.3 row 20). `settings.json` is read at
  `SettingsStore::open` and never again in a run; `storage_watch`
  (`bt-dir-watch` → `StorageChanged` → `advance_storage_watch`) re-reads only
  `profiles.json` and the pins (§9). Contract: the write-now rule is only in
  `SettingsStore`'s doc comment; `docs/RULES.md` §31 is not yet folded.

---

## 8. Asking and telling

**The rule: a message kind × urgency × modality decides the surface, from one
table. A new surface adds a row to that table before it adds a module.**

**The table itself is to be ruled before 0.5, by the project owner**, who rules
UI. What follows is the inventory it will be ruled over, not a proposal.

Fifteen surface types are in the tree before menus and inline fields, eighteen
to twenty-one with them: `toast.rs`, `notify.rs`, `notice.rs`,
`restore::RestorePrompt`, `restore::DirtyGate`, `restore::InviteTarget`,
`quit.rs`, `first_run.rs`, `keyhint.rs`, `cardhint.rs`, `tooltip.rs`,
`file_peek.rs`, `peek_strip.rs`, `search.rs`, `palette.rs`, `websheet.rs`,
`update.rs`'s dot, `cmdrail.rs`, the settings modal, the menu family, and the
inline prompts of `text_field.rs`. Five of them landed in one month.

Two facts the table must start from:

- **There is already a notification policy and it is preserved.**
  `bt-app::notify::desktop_reach` and `notify::interruption`, backed by
  `bt_workbench::attention::AttentionLedger`, decide whether a desktop interruption is
  allowed. **Attention state is distinct from notification delivery, and seeing
  a request is distinct from answering it.** What is missing is the broader
  allocation rule for durable questions, operation results and persistent pane
  state — not a reason to combine every visual surface into one widget.
- **Today the mutual priority of these surfaces exists only as rung order**
  inside `Runtime::keyboard_input` and `Runtime::mouse_input`. The rung order is
  the de-facto specification and nobody wrote it down. Each new feature adds a
  rung.

The enforcement shape, once the table is ruled, is the one
`scripts/check-shortcuts-table.ps1` already uses for the chord table: a
generator plus a diff gate.

---

## 9. Configuration entrances

Three today, each individually documented and with no cross-reference between
them. **A configuration fact declares which entrance it uses.** They are
different kinds of input, and a universal "CLI beats environment beats file"
ladder would be wrong.

| entrance | audience | carries | persists | reload discipline |
|---|---|---|---|---|
| `settings.json`, `profiles.json`, `keybindings.json`, `pins` — `bt-app::persist::SettingsStore`, `SettingsV1`, `SETTINGS_MIGRATIONS`, `ProfilesStore`, `PinsStore` | the person using Folio | durable preferences | yes, with forward-only migrations | **two disciplines, and that is the evidence drift is cheap**: `advance_storage_watch` re-reads `profiles.json` and the pins live (`reread_profiles`, `reread_pins`, behind the shared watch quiet window), while `settings.json` is applied in process at its own door and is not re-read from disk during a run |
| CLI flags — `bt-app::cli::parse`, `CliRequest` | whoever launches this run, including Explorer and another Folio | per-launch placement and the six doors of §2.1 | no | none; consumed once |
| `BT_*` environment variables — `docs/BT-ENVIRONMENT.md` | someone diagnosing this build | diagnostics only | no | read where used |

**The export is not a fourth entrance.** `Settings > About > Export…` writes
`settings.json`, `profiles.json`, `keybindings.json` and the reader's scheme files
as one JSON document (`bt_persist::export`, `folio_export` version 1), and
`Import…` hands each part to the door that document takes when it is edited by
hand — `bt_app::settings_bundle` names the doors, `Runtime::import_settings_from`
walks them: schemes through `parse_scheme` into the folder, profiles through
`take_profile_table`, shortcuts through `Shortcuts::apply_overrides`, and settings
through the file's own migrations and then each row's own apply function, never
through a re-read.

The `BT_*` catalogue is held complete by `bt_app::diagnostics::bt_environment_doc_tests`,
which scans every non-binary, non-integration-test `.rs` file under `crates/`
and `vendor/` for `BT_` literals and fails if the document and the scan disagree
in either direction. Two conventions apply to all of them: **set-but-empty is
off**, and **a name containing `TRACE` keeps the console**.

**The rule for a fourth entrance.** 0.5's outward interface (CLI and MCP
adapters) is a fourth entrance. It declares, in this table, its audience, what
it may carry, whether it persists, and its reload discipline — before it accepts
its first flag. An outward interface that accumulates flags by accretion will
drift from the in-app settings, and the two reload disciplines already in the
table are the evidence that drift is cheap.

---

## 10. Diagnostics as events

There is shared plumbing and no event model: `bt-app::trace::TraceFile`, the
bounded `trace_sink::Queue` with its counted drops,
`bt-app::diagnostics::Channel` and `diagnostics::note`, and
`bt-app::hang_watch::Heartbeat`. `trace.rs`'s own doc says it plainly — nothing
in it knows what it is tracing. Around that plumbing are 22 distinct `BT_*TRACE*`
names, roughly 380 `eprintln!` sites across 14 crates, and five destinations
(console, `diagnostics.log`, hang reports, the panic log, stderr).

**The ruled shape.** A diagnostic is an event with a **domain**, a **station**,
a **severity** and a **payload**, plus the operation vocabulary the lanes need:
**identity, owner, phase, outcome and loss**. `BT_*` variables **select
domains**; they do not invent formats. Destinations stay as they are — the
channel split (a synchronous answer goes to the console, a resident diagnostic
goes to the log) is a rule in force and is not being replaced.

Two things this is not. It is not one file replacing every diagnostic channel:
the destinations and their delivery semantics are already correctly separated.
And it is not a rename of `hang_watch::Station`, whose 195 variants are today
the only map of the window thread that exists anywhere in the tree — that enum
is a description of what was **measured**, not of what is **allowed**, and this
file is where what is allowed now lives.

`AppEvent` has thirty-one wake variants and `AppEvent::station` maps each to a
`hang_watch::Station`. **A new off-thread answer shares an existing lane unless
this file records why it cannot.** Recorded: `InstallChannelRead` (U-3) cannot
share `UpdateChecked` or any other member of the Chrome family, because every
one of their handlers rebuilds every window's chrome and this answer has one
reader, the first-run poll on the next turn's clock run; its handler does
nothing and it is charged to `Woken`, like `LaunchAsked`. Recorded:
`TrialWritesReleased` (U-13, sent once by an update trial's watch when it reads
`Committed`) cannot share a lane either: its handler is the one that writes —
the documents, the marks' migration, the PSReadLine upgrade and the
registrations a trial held back (`App::release_trial_writes`) — so it is charged
to a station of its own, `Station::TrialWritesReleased` (`STATION_COUNT` 220),
where a slow write is named. Recorded: the update job's two (U-18).
`UpdateJobOffer` cannot share `UpdateChecked` — the check sends that only on an
answer and its handler rebuilds every window's chrome, while the job must hear
every settling of the check and the channel's arrival too — nor
`InstallChannelRead`, whose handler does nothing; its handler decides
(`FolioApp::consider_update_offer`). `UpdateJobProgress` carries a driver's
reports, which no other lane applies (`update_job::Job::drain_progress`). Each is
charged to a station of its own, `Station::UpdateJobOffer` and
`Station::UpdateJobProgress` (`STATION_COUNT` 222).

**The window thread's budget lines** (0.4.6 A3; §5.3). Written to
`diagnostics.log` by the watchdog, never by the window thread; durations in
whole microseconds (nanoseconds until printed); `suppressed` is the turns that
crossed the same trigger since its last line and wrote none (at most one line
per trigger per second), `refused` the process's admission refusals and `lost`
the budget lines the ring refused since the line before, all three on every
line:

```text
Folio budget: turn=<n> wall us=<µs> frame_us=<µs> waits_us=<µs> unexplained_us=<µs> delay_us=<µs|none> suppressed=<n> refused=<n> lost=<n>
Folio budget: turn=<n> waits us=<µs> budget_us=8000 calls=<n> suppressed=<n> refused=<n> lost=<n>
Folio budget: turn=<n> call us=<µs> bound_us=4000 door=<Door> row=<row> station=<Station>[ more=<n>] suppressed=<n> refused=<n> lost=<n>
Folio budget: turn=<n> unexplained us=<µs> target_us=8000 wall_us=<µs> suppressed=<n> refused=<n> lost=<n>
```

`wall` is the turn past its frame (`frame_us`), with the scheduling delay
beside it and not in it; `waits` the union of its admitted calls past the budget;
`call` the turn's longest admitted call past its row's bound (`more` counts the
turn's other calls past their bound); `unexplained` its time outside every
admitted call past the budget — never a wait. The exit summary, written by
`fn main` through `trace_sink::stderr_line` just before the run's footer, is read
from the run's atomics, so it is whole however many lines were lost:

```text
Folio budget summary: <wall|waits|unexplained|delay> count=<n> max_us=<µs> sum_us=<µs> hist=<bucket>:<n>,…
Folio budget summary: allowance_used count=<n> max_us=<µs> sum_us=<µs> hist=<bucket>:<n>,… allowance_us=<µs> yielded=<n>
Folio budget summary: door=<Door> row=<row> count=<n> max_us=<µs> sum_us=<µs> hist=<bucket>:<n>,…
Folio budget summary: lost=<n> refused=<n>
```

A bucket is named by its lower edge (`<1us`, `1us`, `2us`, … `4194304us+`), and
only non-empty buckets are printed (`hist=-` when none is). `allowance_used`
(0.4.6 A4) is, per turn that offered deferrable work (a unit ran, or one was
asked for and yielded), what that work took; `allowance_us` is those turns'
allowances summed, so `sum_us / allowance_us` is the share taken; `yielded` counts
the turns whose deferrable work found its allowance spent. A door appears only
once it has been called. The slow-hold line (`Folio: the window thread held
control for …`) is unchanged in meaning and format.

---

## 11. The failure roads

**One preservation policy, two mechanisms.**

The policy: *dirty work has a preservation owner.* Normal quit, controlled
failure and crash recovery share the document identity and the recovery format;
they do not share a call stack.

**Normal quit** is a four-phase transaction: `FolioApp::settle_quit` advances
`bt-app::quit::QuitStep` through the dirty gate, the read-only photograph, the
judged atomic write (`Runtime::quit_save`; a refused disk means do not leave)
and retirement. It has two triggers (`quit::Reason`, U-21): a person asking,
and the update's Restart, whose write is a named generation judged by its own
receipt under a deadline and whose way out hands the transaction to its
applier. `bt-app::restore::DirtyGate` / `GateRequest` is reachable from
quit, close-tab, close-pane and git-discard.

**Controlled failure** is `FolioApp::fail` — twelve call sites. It attempts
device recovery first; otherwise it calls `Runtime::close_window(true)`, clears
the windows and finishes the application. `close_window` finishes a pending
rename and marks the session dirty. It **does not save dirty preview buffers**,
and `DirtyGate` is structurally unreachable from it.

**Emergency termination** is `install_panic_log_hook` / `install_panic_log_hook_at`:
write a report, exempt contained math panics, hide every window of the process
through a system enumeration, leave. It has no safe access to a coherent set of
dirty buffers.

**The fact, stated as a fact.** On both failure roads, unsaved preview edits are
**LOST today**, and no history entry records that as an accepted trade-off.
Session persistence does not substitute: `TabState::preview_content` emits
paths, names and source kinds, and `bt-persist::session::PreviewPoolEntryV1`
contains no edited content — the snapshot looks more protective than it is.
**0.4.4 owns this**, and the ticket is either the repair below or a written
ruling with quantified impact per `CONVENTIONS` §十 rule 7.

The ruled repair for controlled failure: stop accepting new mutations and freeze
a coherent revision; preserve the dirty content with its identity, encoding and
disk baseline to a recovery location — **never over the source file merely
because Folio is failing**, since `PreviewBuffer::save` already distinguishes a
disk conflict and the failure path must not bypass that distinction; take a
durable receipt or retain an explicit unpreserved state; retire only once that
outcome is known, reporting through a surviving native path if the renderer
failed.

**Do not solve this by calling `Runtime::quit_save` from the panic hook.** It
traverses mutable application state, does filesystem work and repaints; it is
unsuitable both for an arbitrary-thread panic and for some renderer failures.
For panic and abort, preserve what was already journaled, keep the mechanism
independent of the owner's locks, and **define the recoverable revision and the
bounded tail that may be lost**. No panic hook can promise the latest keystroke.

---

## 12. What 0.5 and 0.6 require of today's code

**0.5** is the agent workbench: the attention ledger, the notification model,
and an outward CLI/MCP interface. The structure carries it, provided the
asking/telling table (§8) and the configuration map (§9) are ruled first,
because an outward interface is exactly a new message surface plus a new
configuration entrance.

**0.6** is remote: a backend owns session state and clients are views. The
structure as it stands **disqualifies** that, for one reason — session state is
interleaved with chrome state on the window thread (§4.1) and PTY bytes are
drained inside the event loop.

### 12.1 `bt-workbench` may be born now

A crate holding the attention state machine, the semantic notification
decisions, and the commands and events an outward interface is offered. It does
not have to wait for the runtime file move.

| into the domain | out of the domain |
|---|---|
| stable session identity and incarnation | attention snapshot and delta |
| a normalized producer event | episode and ordering identity |
| a request / association id | notification intent |
| the supplied time | command acceptance and result |
| an explicit answer, withdrawal or seen action | a structured reason and operation identity |
| notification preferences | |
| a client-presence observation | |

`AttentionLedger::apply` already receives time and facts rather than reading the
window or the clock, which is why it moves first. Two adjustments come with it:
the dependency on `attention_wire::WAIT_TTL` reverses, because the semantic
expiry rule belongs with the ledger; and `Runtime::deliver_attention`'s
capability routing, which today traverses tabs, belongs beside the session
registry. `Runtime::raise_attention` stays where it is, a client adapter for
taskbar and native notification delivery. The order of extraction is attention
and session identity, then editable documents, then terminal lifecycle — not all
1,424 methods (§13).

**Born 2026-09-25 (0.4.6 census-3, D-57).** `crates/bt-workbench` holds
`attention` — the ledger, moved whole with its grid of tests — and
`attention::expiry` (`WAIT_TTL`, `WaitClock` and the clock's own three tests),
so the dependency on `attention_wire` now points the other way:
`attention_wire` re-imports `WaitClock`. `attention::is_consumed` is the "what
counts as seen" rule of §12.2 decision 2 (`bt-app` keeps calling it
`attention_is_consumed` through one root alias), and `attention::Places` is a
window's place allocator, whose counter and only mutator are private to the
ledger — a `compile_fail` doctest on `Places` is the proof that nothing outside
can advance it. **Its rule:** it reads no window, no clock, no file and no
environment; every fact arrives as an argument; it depends on `bt-layout` only
(§3.3's entry) and only `bt-app` depends on it. **Its public surface** is what
`bt-app` names and nothing more: `AttentionLedger` with `apply`, `weak_edge`,
`ticket`, `state`, `claim_episode`, `surrender_place`, `announce`,
`announce_turn_end` and `admits_a_frame`; the vocabulary `Site`, `Reach`,
`Credential`, `State`, `Transport`, `Via`, `NotificationSwitches` (and its two
fields), `WaitKind`, `WaitSlot`, `wait_key_is_well_formed`, `ClearSelector`,
`ClearClass`, `ClearReason`, `AnswerKind`, `Mode`, `Tier`, `IdSource`,
`ClearScope`, `MappedAction`, `MappingRow` (with `is_wait` and `slot`),
`duplicated_tier`, `kind_mode`, `Event`, `Raised`, `Why`, `Outcome`,
`claim_line`; `expiry::{WAIT_TTL, WaitClock}`, `is_consumed`, `Places`
(`issued` only). `Grounds`, `is_agent_seat` and
`MAX_FRAMES_PER_PANE_PER_SECOND` stay crate-private because `bt-app` names none
of them. The crate's `lib.rs` doc is the in/out table above and the ledger's
invariants (`docs/RULES.md` row 29). Still in `bt-app`, by
`docs/plans/design/ownership-census-2026-09-25.md` §5.2 and §R6: the
reach rule (`notify`, census-4), `attention_wire`, `attention_map` (it names
`bt-term`'s notification types), the installers, `attention_trace`,
`attention_words`, `runtime/attention.rs`, and the tab-walking routing —
`deliver_attention`, `settle_attention`, `deliver_osc_attention`,
`answer_attention_in`, `attention_delivery` — until D-1's session registry.

### 12.2 The three 0.6 decisions

These are expressed today through a single window in `settle_attention`,
`answer_attention` and `raise_attention`. Replaying them independently in every
client creates multiple authorities, so they must be decided **before** the
second client exists:

1. Which client may control PTY size and input ordering.
2. Which observations count as *seen*, and which actions actually *answer* a
   request.
3. Which client is entitled to deliver a desktop interruption.

### 12.3 External clients send domain commands, never runtime methods

CLI and MCP are adapters to the domain API. **Do not export `AppEvent`,
`Runtime`, window handles, rendered labels or `Instant` as the outward
protocol, and do not forward diagnostic prose as the event protocol.** A client
sends domain commands and consumes versioned domain observations. Today's
`bt-app::attention_wire::Message` and `bt_workbench::attention::Event` are the shape
to grow, not `Runtime`.

What breaks if this is done wrong: session identity changing during detach and
reconnect; delayed input delivered to a replacement shell; *seen* treated as
*answered*; duplicated desktop notifications; client-specific font and layout
state moved into the authoritative backend.

---

## 13. `bt-app`'s runtime topics — where a `Runtime` method lives

The address step between a subsystem's contract and its implementation (§1).
Every method below runs on the **window thread** and holds `&mut App` and
`&mut WindowRuntime` (§4.3); the file a method sits in is navigation, not
ownership. "Asks" names the lanes of §5.1 whose requests the file sends or
whose answers it applies; "doors and rows" names the §6 doors and the §5.3
rows its own methods reach. Counts are the census of §0.1, 2026-09-23.

| file | methods | owns | asks | doors and rows |
|---|---|---|---|---|
| `attention.rs` | 27 | toasts, pane notices, the Agents rows, terminal and turn-end notifications; raising, answering, marking seen and jumping to an attention request | ingress (the ledger the endpoint feeds, §7.2 — `bt_workbench::attention` since 2026-09-25, §12.1) | none; `notify::desktop_reach` and `interruption` decide what reaches the desktop |
| `clipboard.rs` | 19 | copy, copy on select, the paste target and its delivery, the multi-line paste card | session (bytes into `InputRing`) | the clipboard read, on this thread (§7.2) |
| `configuration.rs` | 10 | Settings ▸ About ▸ Export… and Import…, each imported part through its own door (§9) | — | `file_reads` (the settings lane, through `bt_persist::read_export`); store writes, row 20 |
| `diagnostics.rs` | 5 | the OS theme change, application-change notes, the trace drain, grid-change scheduling | ingress (trace) | — |
| `dpi.rs` | 11 | window resize, scale-factor change, DPI settling | session | row 12 (`flush_pending_pty_resize`) |
| `files.rs` | 59 | the files column and its float: the tree, rename, new, delete, locate, pins, drops, the palette's roots | observation (`bt-files-worker`, `bt-index-worker`); ingress (`files_watch`); hand-off (reveal) | `std::fs::rename`, `create_dir`, `File::create_new` here, row 20; `files::read_directory` has no door (§6) |
| `first_run.rs` | 24 | the first-run card, the PSReadLine invite and install, the Explorer package row, the agent hook installers, the PowerShell integration offer | observation (`psreadline-probe`, the package probe); storage (the package job) | rows 3 and 4 (`apply_psreadline`, `refresh_psreadline_installed`) |
| `floats.rs` | 61 | floating windows, popups and chevron menus; the file, terminal and page menus | observation (files, git); hand-off (reveal) | — |
| `frame.rs` | 23 | chrome refresh, frame publication and present attempts, `turn`, hyperlink activation | presentation (on this thread); session (the drain frame); observation (math); hand-off (refusals) | row 6 (the search refresh in `publish_frame`) |
| `git.rs` | 103 | the git column, graph, compare and checkout; its menus, prompts and writes to a repository | observation (`bt-git-worker`, whose `git` children go through `quiet_command`); ingress (`git_watch`) | — |
| `handoff.rs` | 4 | the `Runtime` side of the OS hand-off lane: submit, owe a duty, answer | hand-off | never calls `bt_platform::handoff` itself (`no_handoff_runs_on_the_window_thread`) |
| `i18n.rs` | 2 | applying and adopting the language | — | — |
| `keyboard.rs` | 35 | `keyboard_input` and its rung order (§8), shortcuts and key hints, the IME caret, cursor blink | session (input) | IME native affinity (§5.2); `store_keybindings`, row 20 |
| `math.rs` | 46 | formulas in terminal and preview: requests, results, hover tools, toggles, copying LaTeX | observation (`bt-math-worker`) | — |
| `mouse.rs` | 64 | `mouse_input` and its rungs, hover, drag and drop, wheel, the pointer cursor, asking about the link under the pointer | session (mouse reports); observation (path verification, through `main.rs`'s asks) | — |
| `palette.rs` | 15 | the command palette | observation (`bt-index-worker`) | — |
| `panes.rs` | 130 | seat layout, split, close, duplicate, zoom and move of panes; command rails; pane menus; `present_seats_and_commit` | session (pane birth through `create_leaf_session`); presentation (on this thread) | rows 9 and 11 |
| `peek.rs` | 59 | the layout peek and the file glance card | observation (math, preview, git; video) | — |
| `preview.rs` | 286 | preview panes and documents: open, land, rename, save, watch; markdown pictures, the background picture, video seats, the drop preview, clipboard pictures, the tab strip's animation | observation (`bt-preview-worker`, `bt-math-worker`; starts `background-picture` and `clipboard-picture`); ingress (`preview_watch`); hand-off | `spawn_at_priority` ×2; `std::fs::rename`, row 20; row 20 (`save_preview_on`) |
| `profiles.rs` | 23 | the profile table: launching, menus, the editor, store and re-read, the default profile, the root menu | storage; ingress (`storage_watch`) | row 2 (`add_to_profile`) |
| `quake.rs` | 9 | the summoned window: show, hide, its profile and arrangement | — | — |
| `search.rs` | 22 | the find box, its highlights and stepping | — | row 6 (`refresh_search`) |
| `tabs.rs` | 71 | the tab strip: new, close, activate, rename, drag, tear out; the tab menu; tables | — | — |
| `terminal.rs` | 34 | the terminal pane: `drain_pty`, command marks, scroll and column bars, restart, fonts, selection, the PowerShell intent | session (drain, pane birth) | rows 2 (`spend_powershell_intent`), 5 (`apply_terminal_font`), 19 |
| `tooltips.rs` | 9 | tooltips | — | — |
| `web.rs` | 36 | the web pane: open, sync and advance the page, apply its outcomes, dev tools, sheets, the colour scheme, handing a URL to the browser; the engine's warm-up (`warm_web_engine`, ticket 54); the spare controller's making and adoption (`make_spare_web_controller`, `adopt_spare_web_page`, `seat_a_web_page`, ticket 60) | hand-off; `folio-web-thumb` | web view native affinity (§5.2); row 21 |
| `windows.rs` | 38 | windows: open, dress, show, restore, the dirty gate, the quit save, close, retire and vault a window, moves; the window's title (`want_title`, `flush_title`) | storage (`quit_save` through `SessionWriter`) | `let_the_system_translate_touch`; `Window::set_title` (§5.2, `flush_title` only); row 16 |

**Still in `main.rs`** — 201 methods in the two `impl Runtime<'_>` blocks, by the
2026-09-21 manifest (`docs/plans/bt-app-split-2a-manifest-2026-09-21.tsv`):

| topic | methods | owns | why it has not moved |
|---|---|---|---|
| `launch` | 4 | `Runtime::create` — starts the endpoints and probes and registers their wakes — `reseed_editor_env`, `apply_launch_opens`, `arrival_fits` | a source reader bound to `main.rs` by `include_str!` (`docs/plans/bt-app-split-prep.md` Appendix C) |
| `settings` | 31 | the settings modal, `apply_settings_choice`, schemes and their watch, `open_font_settings` | a reader that compares door lines in universe order (same appendix) |
| `focus` | 51 | focus mode, cards and their hints, terminal thumbs, focus seats, the quit card | the same reader shape as `settings` |
| unassigned | 112 | appearance applies, the storage watch and pins, minted pages, `open_local_path` and `reveal_in_explorer` (hand-off), path-verification asks, `send_user_input`, composition, retirement, `finish_synchronized_update_if_due` | the theme regex assigns them no topic; not ruled |
| newer than the manifest | 3 | `apply_web_color_scheme`, `open_unverified_reference`, `hand_uri_to_the_system` | added after the move; no topic yet |

---

## Appendix — where the two reviews disagree

Two independent structural reviews were made on 2026-09-21: a **depth review**
(findings C-1…C-4) and a **breadth review** (findings K-1…K-14). Where they
agree, this file states the finding without attribution. Where they disagree,
both readings are recorded and **the coordinator decides**; the detail of each
is in the matching row of `docs/plans/structural-debt.md`.

| # | the question | depth review | breadth review | where |
|---|---|---|---|---|
| 1 | how many hops the printed-path chain has | four crates, and the layering is correct | five, because the projection hop mints the implicit hyperlink and hit-tests it | §7.1 lists five hops of code across four crates of policy |
| 2 | `bt-pty → bt-term` | target and dependency hygiene, not an inverted layer | genuinely inverted, an afternoon's work | the preparation plan adds that it is not demotable as stated; §3.2, D-13 |
| 3 | `bt-term → bt-platform`, and the `bt-platform` drawer | extract a small headless observation/effect boundary | lift the ledger, the file primitives and the process doors into a systems crate at 0.5 | undecided; D-12, D-14 |
| 4 | the ten one-shot probes | one contract, **not** one serial worker — a machine probe must not delay a user-requested operation | one probe lane with a request type and a default deadline | undecided; D-3 |
| 5 | the "22 multi-owner facts" | five distinct classes; entry 22's three publications check at different points and are related obligations, not one repair written three times; "22" is an assessment, not a census of violations | one repair written three times | §4.2 follows the depth reading |
| 6 | the asking/telling repair | keep the existing notification policy; what is missing is the allocation rule, not one widget | one ruled table enforced by a generator and a diff gate | §8 takes the table; the enforcement shape is the coordinator's call; D-8 |
| 7 | the diagnostics repair | a common operation vocabulary — identity, owner, phase, outcome, loss — and **not** one file replacing every channel | one event shape — domain, station, severity, payload | §10 states both; order undecided; D-10 |
| 8 | the failure-road repair | a preservation transaction plus recovery material kept *before* the failure; never call the quit save from the panic hook | either route the failing close through the quit transaction's vaulting half, **or** write the trade-off down as a ruling | §11 follows depth; D-4 keeps the alternative |
| 9 | the presentation lane | the existing design is stronger than "move render to a thread"; keep the surface lease and preparation permit; the platform acquire affinity is a prerequisite; do not sell the first cut as removing every stall | adopt one of the two designs and give the present mode an owner | §5.4 step 4 carries both |

**Corrections to the process and thread survey**, from the depth review, all
adopted above: `persist::SessionStore::flush_if_due` does **not** wait — it takes
receipts and hands work over, and `flush_judged` / `SessionWriter::wait_for` are
the waiting road; the blanket "the window thread never does a blocking receive"
is too strong transitively, because `macos_watch::DirWatch::start_scoped` waits
on `listening.recv()` (row 8 of §5.3); and the split inventory's destination
table holds 28 named topics rather than 26, while its 574 consumer rows are
explicitly not a structural-guard total, since they include fixtures. **Neither
count should become an architectural score.**

### The coordinator's decisions on the nine (2026-09-21)

| # | decision | reason |
|---|---|---|
| 1 | five hops | the projection hop mints the implicit hyperlink and hit-tests it; a chain document that omits it is the kind that let one fix miss a step per round |
| 2 | not demotable as stated; the probe binary moves to the tools crate and the normal dependency goes (ticketed) | the dev binary is the only production-side consumer |
| 3 | extract a small headless observation/effect boundary first; the systems-crate split of `bt-platform` waits for 0.5, after the `bt-app` move | one move at a time |
| 4 | one request/result contract, not one serial worker | a machine probe must never delay an operation the user asked for |
| 5 | five classes, one rule each | the depth reading survived being checked against the code |
| 6 | the table is the owner's to rule before 0.5; once ruled, a generator plus a diff gate enforces it | UI is the owner's; enforcement shape follows the chord table's precedent |
| 7 | the operation vocabulary first; an event shape is its carrier; no new file replaces the channels | vocabulary is what the channels lack, not a destination |
| 8 | the preservation transaction plus recovery material kept before the failure; the quit save is never called from the panic hook | the alternative ruling "accept the loss" was not taken: unsaved edits are a hard requirement |
| 9 | keep the surface lease and preparation permit of the existing presentation design; the platform acquire affinity is a prerequisite; the first cut is not sold as removing every stall | the stronger design already exists |
