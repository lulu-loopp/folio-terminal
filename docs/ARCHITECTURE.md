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
`build.rs` are left out. To re-count, grep the patterns in the last column and
drop the test items; a number that moves edits this table and the pictures.

| what | count | pattern |
|---|---|---|
| thread-spawn sites | **45** — `bt-app` 29, `bt-platform` 12, `bt-pty` 4 — plus **one** rayon pool, `bt-term::inline_image::resample_pool` (`bt-image-resample-{index}`) | `spawn_at_priority(_with_stack)?\(`, `thread::spawn\(`, `thread::Builder::new\(\)`, `ThreadPoolBuilder::new\(\)` |
| through the thread door | **23**, every one in `bt-app` | `spawn_at_priority` |
| bare spawns | **22**: in `bt-app`, `folio-web-thumb` and five unnamed (`explorer_menu` ×4, `attention_wire::payload_on_stdin`); in `bt-platform`, twelve named `Builder`s (the two endpoints and the directory watch on each platform, six video threads); in `bt-pty`, the reader, the writer, the dump publisher (unnamed) and `pty-retirement` | as above |
| of those, in a door process rather than the window process | **3**: `explorer_menu::remove_from_explorer_menu`, `explorer_menu::cleanup_registrations`, `attention_wire::payload_on_stdin` | — |
| named sites / distinct names | **37 / 32**, besides the pool | the first argument, or `.name(…)` |
| `spawn_blocking` | **0** — there is no async runtime | `spawn_blocking` |
| channel constructions | **30** — 26 `mpsc::channel`, 4 `mpsc::sync_channel`; `bt-app` 24, `bt-platform` 6 — and **6** `Condvar::new` (`bt-pty` 3, `bt-platform` 2, `bt-app` 1); no other channel crate | `(sync_)?channel(::<…>)?\(`, `Condvar::new\(` |
| `AppEvent` variants | **29** | `enum AppEvent` in `main.rs` |
| child-process construction | **one** `Command::new`, inside the door `bt_platform::quiet_command`, with **6** product callers — `attention_copilot::run_probe`, `explorer_menu::serve`, `git::git_command`, `psreadline::run_probe`, `shell_integration::run_profile_probe`, and `bt_platform`'s macOS `quiet_command_text` (two programs); besides the door, `bt-pty::PtySession::spawn`'s `spawn_command` and the one `ShellExecuteW` in `bt_platform::handoff` | `quiet_command(_named)?\(`, `Command::new\(`, `spawn_command\(`, `ShellExecuteW\(` |
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

### 2.1 One binary, six argv doors

The only shipped binary is `folio` (`crates/bt-app/Cargo.toml`'s `[[bin]]`,
`crates/bt-app/src/main.rs`). `bt-record`, `bt-replay`, `bt-zoom-perf`,
`render-info-plist`, `bt-conpty-width-probe` and `bt-repaint-oracle` are
development tools and are not packaged.

`fn main` checks six argv doors in a fixed order, each headless and each ending
in `process::exit`; everything else is grammar for the ordinary window launch.

| door | parser | handler |
|---|---|---|
| `--uninstall-cleanup [--purge]` — checked first | `cli::uninstall_cleanup` | `uninstall::run` |
| `attention <family>:<event>` — checked second, above the panic hook | `cli::attention` | `attention_wire::run_verb` |
| `--explorer-command` | `cli::explorer_command` | `explorer_menu::serve` |
| `--remove-shell-integration` | `cli::remove_shell_integration` | `shell_integration::remove_shell_integration` |
| `--remove-explorer-menu` | `cli::remove_explorer_menu` | `explorer_menu::remove_from_explorer_menu` |
| `--help` / `--version` / any parse fault | `cli::parse` | `report_at_the_front_door` |

`attention` is checked second, above the panic-log hook and above anything that
could build a window, because the caller that matters most is an agent holding
an approval open. **One instance owns the data directory**
(`bt_platform::instance::claim_data_directory`, a named mutex or a file lock
keyed by that directory, held for the life of the process by
`persist::is_writer_of`); a second process hands its argv down the launch pipe
(`launch_wire::hand_over`) and leaves through `bt_platform::leave_process`.

### 2.2 The twelve kinds of child process

| kind | started by | note |
|---|---|---|
| `OpenConsole.exe`, the ConPTY host | the DLL's `ConptyCreatePseudoConsole`; extracted at build time by `crates/bt-pty/build.rs` | Folio never spawns it and holds no handle to it |
| the pane child — the shell or agent | `PtySession::spawn` → `CreateProcessW` with the pseudoconsole attribute | program chosen by `bt_pty::shell::resolve_default_shell` |
| the pane's grandchildren | whatever the shell leaves running | held in an unnamed job object by `Job::holding`, so closing the pane kills them |
| WebView2 browser, renderer, GPU, utility | the Edge runtime, from the one `CreateCoreWebView2EnvironmentWithOptions` (`bt_platform::webview::create_environment`), asked for by a page's `WebHost::request_environment` or, once per process on an idle turn after startup, by `bt_platform::warm_web_environment` (ticket 54) | nothing in that module blocks; that is its stated contract; at most one creation call in flight (`bt_platform::EnvironmentSlot`) |
| `com.apple.WebKit.WebContent` | `bt_platform::macos_webview::WebHost::request_controller` | main thread only, one per seat, never pooled |
| `folio.exe --from-explorer --cwd <folder>` | Folio's own COM server, in `explorer_menu::serve` | detached, never reaped — the one deliberately orphaned child in production |
| `powershell.exe` — the PSReadLine probe | `psreadline::run_probe` | once per process; blocks its thread with no timeout |
| `powershell.exe` — the `$PROFILE` probe | `shell_integration::run_profile_probe` | once per distinct program; the only probe with a deadline |
| `cmd.exe /c "<copilot> --version"` | `attention_copilot::run_probe` | once per process, on opening the Agents page |
| `git` | `git::git_command`, always from `bt-git-worker` | never from the window thread; one status costs three threads |
| `explorer.exe`, the registered handler, Finder | `bt_platform::handoff` | fully detached: no handle, no wait, no kill |
| `defaults read -g AppleLocale`, `locale -a` | `bt_platform::read_system_locale_declaration` | macOS; memoised, so twice per process |

Children reached through `bt_platform::quiet_command_named` are named by an
absolute path resolved by `handoff::program_on_path`, never a bare name, so a
program sitting in the working directory can never run.

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
                  bt-platform (Windows only)
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

`bt-corpus` and `bt-winres` are tools; `bt-source` is read by tests only
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
  behind `bt-platform`'s interface. Its second half reads
  `FILES_THAT_MAY_NAME_A_PLATFORM` out of `main.rs` — one list, two readers,
  the other being `bt_app::platform_gate_tests`.
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
| **Durability and external transactions** | Desired state, submitted state and durably acknowledged state are separately named facts, and one transaction owner advances each resource. | `persist::SessionWriter`, which already distinguishes sent from landed; `profile_runtime::install_recorded`, which holds the marks lock across read, change and write; `update::begin` |
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
one judgement: every failed read is `Channel::Unknown`. The only reader today is
the one `diagnostics.log` line; the Explorer default (U-3) and the updater's
eligibility will read the same `FACT`.

### 4.3 The `Deref` trap

`main.rs` declares `impl Deref for Runtime<'_>` and `impl DerefMut for Runtime<'_>`
with `type Target = TabState`, resolving through `active_item`/`active_item_mut`
on the active tab. **Any method may therefore reach the active tab with no
`self.window.` prefix at all.** Two consequences a ticket must respect:

- A census or grep that reads `self.foo` inside an `impl Runtime` method as a
  field of `Runtime` or `WindowRuntime` is wrong for every field that belongs to
  `TabState`, and wrong in the direction that makes `Runtime` look like the
  owner of state it only borrows. Every `self.` access must be resolved to
  `Runtime`, to `WindowRuntime`, or through `Deref` to `TabState`.
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
| **the installation's transaction** — the update journal `H\journal.json`, its frozen header `{v, txn, rescue, class}`, the trial's receipt, and the member inventories | `bt-app::update_txn` (pure: `Header`, `Phase` and `next`, `JOURNAL_WRITERS` and `EFFECT_RIGHTS`, `decide`, `at_start`, `Receipt`, `Inventories`) | O (the running build and its in-app job) writes `Allocated`, `Prepared`, `Handoff` and O's `Abandoned`; only the transaction-lock holder (the rescue copy of O, as applier or recovery) writes every later phase, and `Committed` only on N's receipt while the journal says `Trial`; N writes only its receipt (`docs/plans/design/self-update-2026-09-16.md` revision (b), §(b).2) | durability and external transactions | 0.4.6 U-10: protocol only, no product caller; the journal, lock and entrance effects arrive behind their own doors in U-11, U-22 and U-26 |
| **this build may update itself** — the eligibility half of the owner's 2026-09-25 rule (signed *and* built with the flag) | `crates/bt-app/build.rs` (`updater_flag`, deciding by `update_eligibility::decide`), read through `bt-app::update::eligible` | the build invocation only: `FOLIO_UPDATER=on` emits the cfg `folio_updater`, unset or empty emits nothing, any other value stops the build; set by `build-release.yml` on a `v*` tag or its `updater` dispatch input and by the macOS release build (`docs/RELEASING.md`), by nothing else | a compile-time constant: one writer, no runtime change | 0.4.6 U-8: read by `diagnostics::run_header` (`updater on`/`off`, checked by `smoke.ps1 -Updater` / `-ExpectSigned`); the update job (U-18) is its reader to come |

---

## 5. Execution lanes

### 5.1 The seven lanes

Forty-five production thread-spawn sites exist across three crates (`bt-app`
29, `bt-platform` 12, `bt-pty` 4), plus one lazy rayon pool in `bt-term` (§0.1). **The thread count is not the defect; the absence of a contract
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

**The band rule — three tiers, not two.** The event and render loop is
`AboveNormal`; the PTY reader is `Normal`; **every** worker is `BelowNormal`.
The band is set by `bt_platform::spawn_at_priority` as the first statement of
the closure, because Windows hands a new thread `Normal` whatever its creator
stands in, and that call is the one `unsafe` boundary for thread priority.
MMCSS is explicitly refused. `folio-web-thumb` breaks the rule with a bare
`Builder` at inherited `Normal`, beside the loop, and nothing goes red.

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

| # | call | where | disposition |
|---|---|---|---|
| 1 | `ShellExecuteW` / `NSWorkspace::openURL:` — synchronous, no timeout; the measured ~1.4 s Ctrl+click stall | `bt-platform::handoff::{windows_handoff,macos_handoff}::hand_over`, reached from `Runtime::open_local_path`, `reveal_in_explorer`, `open_local_path_verified`, `reveal_verified`, `open_preview_link`, `hand_url_to_the_browser`, `activate_local_image_path`, `open_font_settings` | **done** — *a hand-off to the system runs on its own lane, and the window that receives it may take the front* (`DESIGN.md`, 2026-09-22) |
| 2 | the marks lock: a wait with no deadline behind our own writer, then `try_lock` and `sleep` up to `OUR_TURN` = 2 s for a holder in another process (`DESIGN.md`, 2026-09-23), then a dated `$PROFILE` copy, an atomic write and two marks writes | `Runtime::add_to_profile`, `spend_powershell_intent` → `profile_runtime::install_recorded` | **0.4.4** — storage lane; the enable and removal halves are already on workers, the install half is not |
| 3 | `psreadline::apply_recorded` — nine files, ~429 KB, under the same lock | `Runtime::apply_psreadline` | **0.4.4** — storage lane; named by the 2026-09-21 history entry |
| 4 | `psreadline::installed_copy` — a recursive walk of the module directory | `Runtime::refresh_psreadline_installed` | **0.4.4** — observation lane |
| 5 | `bt_platform::monospace_font_families()` — the machine's whole font collection, enumerated inline; the traced cause of the frozen gear | `settings::monospace_family_files` ← `apply_stored_terminal_font` | **done** — *the font list is walked only on its lane, by a numbered request; the face in settings.json is found by its name* (`DESIGN.md`, 2026-09-24); what is left on the window thread is `bt_platform::monospace_family_named`, one family asked of the system collection (station `font family lookup`), measured at 2.6–3.4 ms cold against the walk's 71–80 ms on the development machine |
| 6 | `search::scan_history` / `scan_volatile` — the pattern re-run over every frozen line on every keystroke in the find box | `Runtime::refresh_search`, from the keystroke roads (`search_field_key`, `toggle_search_flag`, `search_ime`, `open_search`) and from `publish_frame_inner` | **done** — *a changed search scans one slice of history on the keystroke's frame and the rest on the following turns* (`DESIGN.md`, 2026-09-24; ticket 51). Not the observation lane: the frozen plane is the session's and mutable, so a worker needs a copy per question or an ownership change in `bt-transcript`; the bounded walk stays on this thread, one `search::SEARCH_HISTORY_SLICE` per keystroke and per turn (`Runtime::advance_search_scan`) |
| 7 | macOS `defaults read -g AppleLocale` and `locale -a`, blocking, no timeout, on the pane-birth road | `bt_platform::read_system_locale_declaration` | **0.4.4** — observation lane |
| 8 | `bt-platform::macos_watch::DirWatch::start_scoped` waits on `listening.recv()`; `Drop` does `SetEvent` then an unbounded `join()` | the watch subscriptions | **0.4.4** — make watcher start and retirement asynchronous |
| 9 | surface acquire, queue submit, swapchain present, surface configure, DirectComposition size and commit | `Runtime::present_seats_and_commit` | **0.5** — presentation lane; the present mode itself comes from `get_default_config` and has no owner |
| 10 | device recovery's `pollster::block_on(rebuild_after_device_loss)` plus deliberate 150 ms and 450 ms sleeps across three attempts | `FolioApp::recovered_from_a_lost_device` | **0.5** — an explicit asynchronous state machine |
| 11 | `CreatePseudoConsole` + `CreateProcessW`, and a `stat` of the working directory | `create_leaf_session` → `PtySession::spawn_shell_in` | **0.5→0.6** — session lane, preserving input and resize ordering |
| 12 | the synchronous `ResizePseudoConsole` round trip | `Runtime::flush_pending_pty_resize` | **0.5→0.6** — session lane; moving it must preserve the ordering this function represents |
| 13 | `sample_window_place` — 4 to 8 syscalls, at three call sites for one instant | `drain_pty`, `advance_strip_animation`, `FolioApp::user_event` | **done** — *where the window is gets asked once per turn, at the turn's head; the drain and the strip tick read that answer* (`DESIGN.md`, 2026-09-24); one writer, `Runtime::observe_window_place`, also called at a window's birth and by an attention delivery between turns; each probe has its own station |
| 14 | `Window::set_title` at five call sites with no throttle | `drain_pty`, `activate_tab`, `dress_new_window`, `finish_synchronized_update_if_due`, `finish_rename` | **done** — *the window's title is one wanted value, written to the system only when it changes and at most once a frame* (`DESIGN.md`, 2026-09-24); the one remaining call is `Runtime::flush_title`, on this thread by §5.2 |
| 15 | `bt_pty::wait_for_retirements` — a bounded `Condvar::wait_timeout` on the way out | `FolioApp::settle_quit`, `QuitStep::Retire` | **ruled to stay** (`T-QUIT-HAS-A-DEADLINE`, `T-QUIT-TIMEOUT-PROCEEDS`) |
| 16 | `SessionWriter::wait_for` — `recv_timeout(SESSION_SAVE_BUDGET)` on the synchronous save | the quit write | **ruled to stay** — a timeout sets `stalled` and quit proceeds; a disconnect stops quit |
| 17 | `trace_sink::flush` — `recv_timeout(FLUSH_TIMEOUT)` | the way out of `fn main` | **ruled to stay** (`T-TRACE-OFF-THREAD`) |
| 18 | `launch_wire::hand_over`, bounded by `HANDOVER_BUDGET` | `fn main`, before the loop exists | **ruled to stay** — there is no loop yet to be blocked |
| 19 | `OutputRing::try_pop`, `InputRing::try_push` — both bounded to one lock, never split, never partly taken | `drain_leaf_pty`, `offer_pty_input` | **ruled to stay** — this is the design |
| 20 | `fs::rename`, the preserving atomic preview save, settings/keybindings/profiles writes, diagnostic file writes | `rename_preview_file`, `rename_files_row`, `save_preview_on`, `persist.rs`'s store methods | **0.4.4** — storage lane, with document-revision preconditions and receipts rather than a generic "background job finished" toast |
| 21 | a web page coming up: `CreateCoreWebView2CompositionController` on the first page of the process (88–269 ms synchronous, ~4,500 page faults: the engine's in-process half loading), then one engine dispatch of 70–303 ms on the message pump before the controller's callback; later pages ~3 ms and ~50 ms. `CreateCoreWebView2EnvironmentWithOptions` 10–38 ms on the first page, the install burst 2–6 ms. Measured headless on the development machine (ticket 43); the owner's next89 run held 4,099 ms | `WebSeat::step` → `WebHost::request_controller` (station `request_controller`), and the pump after it (station `message pump`); `WebSeat::start_environment` (station `request_environment`) | **open — narrowed by ticket 54**: the environment is asked for once on an idle turn after startup (`Runtime::warm_web_engine`, station `warm_web_engine`), which takes its 8.5–39 ms out of the first page's gesture; the environment starts no runtime process (measured), so the controller and the pump dispatch are still the first page's, and a new ruling is owed for them (D-64). **Narrowed again by ticket 60 (ruled 2026-09-25, option A):** for a profile that has opened a page (`web_pages_used`), the controller call (≤ 590 ms worst on the clean VM) moves to a quiet idle turn under `make_spare_web_controller`, and the first eligible page's window-thread cost is the rehost walk (`adopt_spare_web_controller`, 11–73 ms on the VM) and a navigate — 2318 → 146 ms median to the first page in spike 59. Still the first page's: a profile's first-ever page, a page that arrives before the spare has landed, and every page after the spare is used (~0.6 s warm, ~2.3 s cold); open for 0.4.6. Not movable to a lane: WebView2 refuses an environment used from any thread but the one that created it (`0x802A000C`, measured), so the environment, the controller and the engine's callbacks all belong to the window thread with the controller (§5.2); the pump after the first page is named per message since ticket 64 |
| 22 | `Window::set_ime_cursor_area` — `ImmSetCompositionWindow` + `ImmSetCandidateWindow`, answered by the input method; the owner's next93 caught 15 + 85 ms in one turn and a single call of 3,138 ms under load | `Runtime::apply_ime_cursor_area`, reached before ticket 63 from every offer: `publish_frame_inner`, `repaint_preview`, `reoffer_ime_cursor_area`, the turn's offer | **done** — *the input method's caret area is one wanted value, told to the system at most once a turn and only when it moved* (`DESIGN.md`, 2026-09-25); the one road is `Runtime::flush_ime_cursor_area`, from the turn's tail and from `Ime::Enabled`, on this thread by §5.2. A single slow answer still holds the thread; the repetition is gone |

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
| reading file bytes | `bt_platform::file_reads` — eleven named lanes, `Lane`, `Ledger::add`, the process-wide `LEDGER` | `file_reads_doors.txt` plus a source guard |
| reading who owns an install folder, and the macOS install-marker attribute | `bt_platform::install_evidence` — `owner_of`, `current_account`, `attribute` (read-only: `GetNamedSecurityInfoW` and the process token on Windows, `stat`, `geteuid` and `getxattr` on Unix); the attribute's bytes are charged to `file_reads`' `Lane::Install` | its own module, one function per read; its one caller is `install_channel::read` |
| making an update transaction durable, and the installation's two locks | `bt_platform::install_txn` (U-11) — `durable_write` (a temporary file beside the target → write → `FlushFileBuffers` / `F_FULLFSYNC` → rename over the target → the directory flushed: `FlushFileBuffers` on a handle opened with `FILE_FLAG_BACKUP_SEMANTICS`, `F_FULLFSYNC` on the directory's descriptor), `durable_move` (`MoveFileExW(MOVEFILE_WRITE_THROUGH)` / `renamex_np(RENAME_EXCL)`, never over an existing file, then both directories flushed), `flush_current_user_key` (`RegFlushKey`, Windows only), `try_hold` / `hold_within` with `Hold::Shared` (read-only open, existing file) or `Hold::Exclusive` (`LockFileEx` / `flock`, released by `Held`'s drop); three arms: Windows, macOS, and a refusal naming the door everywhere else; every failure names its `Stage`; worker only | `install_txn::tests`: the order over a recording fake of its `Surface` trait, the real arms over temporary folders, the locks across two processes |
| constructing a child process | `bt_platform::quiet_command_named` (and `quiet_command`) — absolute path resolved by `handoff::program_on_path` | pinned as the only `Command` construction |
| handing something to the operating system | `bt_platform::handoff` — the only `ShellExecuteW` and `NSWorkspace` sites in the workspace | its own module, one function per verb |
| reaching the network | `bt_platform::http` — `https_get` (one `GET` into memory: the update check) and `https_download` (one `GET` streamed to a file under a ceiling, U-7), over the operating system's own stack (WinHTTP, `NSURLSession`), `https` only, no caller headers; the download's ceiling, temporary file, deadlines and stage vocabulary are `bt_platform::https_download`'s, shared by both real arms | `update_check_transport_tests` holds the three arms to one signature per door and one set of request types |
| starting a thread | `bt_platform::spawn_at_priority` / `spawn_at_priority_with_stack` — a name and a priority band | every call site is in `bt-app`, so the name is the thread |
| creating a native window outside the framework | `bt_platform::SpareParent` / `spare_parent` — the spare web controller's never-shown `WS_POPUP` parent (ticket 60); dropped only on a pumping thread, left to process exit by an orderly stop | the one `CreateWindowExW` in product code, pinned by `web_spare::spare_wiring_tests::the_spare_parent_is_the_one_window_product_code_creates` |
| taking a native window's messages away from the framework | `bt_platform::let_the_system_translate_touch` — the touch subclass that hands `WM_TOUCH` and the three `WM_POINTER*` to `DefWindowProc` | a message table pinned by test; called once per window, from the two `create_window` sites |

Two corollaries. **Each lane declares itself**: a new kind of read joins
`file_reads` with a named lane rather than reading bytes beside it. **The worker
produces the door's input with the door's own function**, never a second
derivation of it.

Each door is held by a pin: `bt_app::file_reads_source_tests` reads
`file_reads_doors.txt` and fails the build when a product read appears outside
an inventoried door; `quiet_command_named` is pinned as the only `Command`
construction; `handoff` holds the only `ShellExecuteW` and `NSWorkspace` sites.

**The known bypass, stated as a fact.** `docs/BT-ENVIRONMENT.md`'s file-read
self-report declares that **directory enumeration and metadata are excluded**
from the ledger's accounting. That is a statement about what the byte counters
measure. It is *not* a decision that directory enumeration needs no door — and
the consequence is that the files column's `bt_app::files::read_directory` has
no lane, no door and no guard, and nothing rules whether it should. Until that
is decided, the ledger's totals are not an account of what this process reads
from disk, and the next enumeration-shaped effect will land the same way.

`folio-web-thumb` is the matching bypass of the thread door: a bare
`Builder::new().name(...)` at inherited `Normal` priority. It is not the only
one in `bt-app`: five unnamed `std::thread::spawn` sites also skip
`spawn_at_priority` — `explorer_menu`'s `begin_probe` and `run_request` in the
window process, and `remove_from_explorer_menu`, `cleanup_registrations` and
`attention_wire::payload_on_stdin` in door processes (§0.1). Whether a door
process's thread owes the door is not ruled.

---

## 7. Cross-crate chains

A chain is a decision that no single module owns. **Adding a hop edits this
list.**

### 7.1 The printed path — recognition to hand-off

A bare absolute path a program printed in the terminal, made clickable.

| hop | crate | entry | carries | lane |
|---|---|---|---|---|
| recognition | `bt-transcript` | `paths::detect_absolute_path_candidates`, `paths::may_read_unasked` | `PrintedPathCandidate` | the feed, synchronously |
| verdict | `bt-term` | `session::verify_path`, `DualPlaneSession::ask_about_reprinted_path` / `re_ask_about_link_target`, the `path_verdicts` ledger | `PathVerdict` | asked on `bt-path-verify-worker`; the ledger is per pane and bounded |
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

`AppEvent` has twenty-nine wake variants and `AppEvent::station` maps each to a
`hang_watch::Station`. **A new off-thread answer shares an existing lane unless
this file records why it cannot.**

---

## 11. The failure roads

**One preservation policy, two mechanisms.**

The policy: *dirty work has a preservation owner.* Normal quit, controlled
failure and crash recovery share the document identity and the recovery format;
they do not share a call stack.

**Normal quit** is a four-phase transaction: `FolioApp::settle_quit` advances
`bt-app::quit::QuitStep` through the dirty gate, the read-only photograph, the
judged atomic write (`Runtime::quit_save`; a refused disk means do not leave)
and retirement. `bt-app::restore::DirtyGate` / `GateRequest` is reachable from
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
