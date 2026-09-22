# Structural debt

This ledger is separate from the defect ledgers because it is **ordered by
consequence for the next twelve months, not by severity**. Nothing here is a
bug. Each row is a shape that makes the next hundred tickets more expensive,
and the cost it charges is the one the project owner named: *what must be read
to finish one ticket must not grow with the number of features.*

Two independent structural reviews were made on 2026-09-21 — a **depth review**
(findings C-1…C-4) and a **breadth review** (findings K-1…K-14). A finding both
reviews made is **one row here citing both ids**. Where they prescribed
different repairs, the row says so and
`docs/ARCHITECTURE.md`'s closing appendix holds the two readings for the
coordinator.

**Fields.** *Class* — one of: no owner · wrong layer · no rule ·
history-only knowledge · crosses too many places · must-read set grows with
features. *Evidence* — anchors only, never line numbers. *If left* — the
twelve-month consequence. *Smallest change* — the least structural thing that
removes the class, not the instance. *Version* — before the move · with the
move · 0.4.4 · 0.5 · 0.6. *Status* — open · partly discharged · decided.

---

## D-1 — Session state has no owner independent of the window

*C-1 · K-1* · **Class:** no owner / wrong layer / must-read grows with features.

**Evidence.** `bt-app::main::LeafSession` holds process identity, incarnation,
launch profile, program, spawn place, the terminal session and the attention
ledger **and** the viewport projection, the fade clocks, `last_presented_frame`
and `frame_image_references`. `WindowRuntime` owns the tabs that hold them and
the attention ticket counter. `create_leaf_session` resolves the shell, mints
the capability, starts the PTY, builds the `DualPlaneSession`, reads renderer
metrics and constructs a viewport projection in one function. `drain_leaf_pty`
pulls PTY bytes inside the event loop. `Runtime: Deref<Target = TabState>` hands
out the active tab implicitly, so every method reaches state it does not own.

**If left.** Every new agent action, reconnect path and client surface adds
another traversal of window/tab/session state, and the outward interface becomes
automation of the window's internals. 0.6 — a backend that owns session state
with clients as views — is unimplementable, and 0.5's outward interface bolts
onto `Runtime` and deepens it.

**Smallest change.** An authoritative session model behind commands and
observations, **initially on the existing thread**; physical concurrency and a
separate process come later. Split the ownership three ways, preserving the
existing implementations: session (identity, incarnation, PTY lifecycle, launch
facts, parser and transcript, attention credentials and expiry), document
(content, revision, undo, encoding, dirty status, disk baseline —
`PreviewBuffer` is already this), view (selection, scrolling, focus, layout,
native resources, caches, the last presented picture — `PreviewPane` is already
closer to this). A view's inputs are explicit, and a view disappearing is not a
session ending. **The entry must accept the narrower objects**; another wrapper
holding `&mut App` and `&mut WindowRuntime` renames the must-read set and
changes nothing.

**Version.** The ownership decision **before the move** (it changes how the
runtime files should be grouped); the attention and session-identity extraction
**0.5**; complete backend ownership and transport **0.6**. First extraction is
attention and session identity, then editable documents, then terminal
lifecycle — not all 1,310 methods.

**What breaks if done wrong.** Session identity changing during detach and
reconnect; delayed input delivered to a replacement shell; *seen* treated as
*answered*; duplicated desktop notifications; client-specific font and layout
state moved into the authoritative backend.

**Status.** open. Direction stated in `docs/ARCHITECTURE.md` §4.1.

---

## D-2 — The window thread's blocking set is a list, not a budget; lanes are named by feature, not by contract

*C-2 · K-6* · **Class:** no rule / crosses too many places.

**Evidence.** Forty-five production thread-spawn sites across five crates, plus
a lazy rayon pool. `MathWorker::spawn` starts path verification and image
scaling as well as math and returns all three through one result type — a
hosting decision wearing a subsystem's name. `Runtime::apply_psreadline` runs an
installation synchronously while `profile_runtime::begin_enable` spawns a worker
for the same kind of work. Roughly thirty window-thread calls can block outside
the process, headed by the hand-off (the measured ~1.4 s stall), the marks lock,
the nine-file module write, the whole-machine font enumeration and the history
scan on every keystroke in the find box. Sixteen named wait budgets live in six
crates and nothing says which may be spent on the window thread. The
presentation lane was specified twice and built neither time; the swapchain
present mode comes from the surface's default configuration and nobody chose it.

**If left.** Every feature invents its own worker, cache, wake, shutdown and
retry policy, and agents must inspect unrelated features to discover precedent.
Input-latency work stays per-incident, and 0.6's clients multiply the cost of
every stall.

**Smallest change.** Seven lanes defined **by blocking and ordering contract**,
not by feature name (window · OS hand-off · storage and integration transactions
· observation and computation · session transport and lifecycle · presentation ·
ingress and diagnostics), each stating ordering, supersession, queue bounds,
cancellation or abandonment, completion and wake obligations. The window-thread
blocking set becomes a **numbered exception list, each row carrying a ticket or
"ruled to stay"**, with the rule that input never waits on the compositor beyond
one display interval. Existing implementations are wrapped, not rederived
(`CONVENTIONS` §十 rule 9).

**Version.** Contract **before the move**; the narrow removals (hand-off,
integration mutations, the remaining observations and storage operations)
**0.4.4**; presentation and device recovery **0.5**; PTY birth, resize and
lifetime **0.5 toward 0.6**.

**Two readings, for the coordinator.** The breadth review asks for one of the
two presentation designs to be adopted and the present mode given an owner. The
depth review adds that the existing design is stronger than "move render to a
thread": the surface-lease handshake and the shared preparation permit must be
retained, the macOS acquire affinity is a prerequisite, and surface
configuration and GPU preparation remain residual owner-thread costs — **the
first cut must not be advertised as eliminating every window-thread stall**.

**Status.** open. Lanes, the exception list and the migration order are in
`docs/ARCHITECTURE.md` §5.

---

## D-3 — Ten one-shot probes are one lane in ten costumes

*K-9 · C-2 (refinement)* · **Class:** no owner.

**Evidence.** `psreadline-probe` and `copilot-version-probe` block on their
child with **no timeout**; `bt-update-check`, `font-families`,
`powershell-profile-probe` (the only one with a deadline), the profile
migration, enable and removal threads, and the two unnamed Explorer-menu threads
complete the set. Each has its own static, its own wake, its own event variant
and its own idea of whether presses coalesce. `powershell-profile-enable` and
`powershell-profile-removal` **share one result slot with no in-flight latch** —
two fast presses start two threads and the last report written wins — while
`explorer_menu` next door has both a busy latch and a one-slot press queue. The
codebase has already noticed the commonality: `hang_watch::Station::Chrome` is
one station for nine of their events.

**If left.** Every new probe invents lifetime, timeout, latch and wake policy
anew, and two of them already share a slot unsafely.

**Smallest change.** One request/result contract with a default deadline and the
`install_wake` idiom the codebase already uses seven times. **The two reviews
differ on the mechanism**: the breadth review asks for one probe lane; the depth
review warns that enable, removal, migration and registration are *mutations*
rather than probes, and that **blocking on one machine probe must not delay an
unrelated operation the user asked for** — a common contract, yes; one serial
worker, no. The PSReadLine debt named by the 2026-09-21 history entry is the
natural first passenger either way.

**Version.** 0.4.4. **Status.** open.

---

## D-4 — One preservation policy is missing; both failure roads lose dirty work

*C-3 · K-8* · **Class:** no owner / no rule.

**Evidence.** `FolioApp::fail` — twelve call sites — attempts device recovery,
then calls `Runtime::close_window(true)`, clears the windows and finishes the
application; `close_window` finishes a rename and marks the session dirty and
**does not save dirty preview buffers**. The panic hook writes a report, hides
every window through a system enumeration and leaves, with no safe access to a
coherent set of dirty buffers. Normal quit is a different protocol:
`settle_quit` advances `QuitStep` through gate, photograph, judged write and
retirement, and `restore::DirtyGate` is reachable from quit, close-tab,
close-pane and git-discard — and **structurally unreachable from both failure
roads**. Session persistence does not substitute: `TabState::preview_content`
emits paths, names and source kinds, and `PreviewPoolEntryV1` carries no edited
content. **No history entry records the loss as accepted** (see
`docs/RULES.md` row 43).

**If left.** Live editing makes unsaved work a first-class artefact; every crash
or viewport error discards it silently, while a session snapshot goes on looking
more protective than it is. Each new editor or agent-authored document adds
another special case to quit and to failure.

**Smallest change.** **One policy — dirty work has a preservation owner. Two
mechanisms — controlled quiescence and emergency termination.** For controlled
failure: stop accepting mutations and freeze a coherent revision; preserve the
dirty content with its identity, encoding and disk baseline to a **recovery
location, never over the source file**; take a durable receipt or retain an
explicit unpreserved state; retire only after the outcome is known, reporting
through a surviving native path if the renderer failed. For panic and abort:
preserve what was already journaled, keep the mechanism independent of the
owner's locks, and **define the recoverable revision and the bounded tail that
may be lost**. **Do not call `Runtime::quit_save` from the panic hook** — it
traverses mutable application state, does filesystem work and repaints.

**Version.** 0.4.4, before editing is widened through the 0.5 outward interface.
The journal can begin locally; it does not wait for a backend process.

**Two readings.** The breadth review offers an either/or: route the failing
close through the vaulting half of the quit transaction, **or** write the
trade-off down as a ruling with quantified impact per `CONVENTIONS` §十 rule 7.
The depth review rules out the first as stated and asks for the transaction
above. Both are acceptable exits; the coordinator picks one.

**Status.** open.

---

## D-5 — The rules existed only as history

*K-2 · C-4* · **Class:** history-only knowledge / no rule.

**Evidence.** `DESIGN.md` is append-only with duplicated section numbers (§7.14,
§7.19, §7.45, §7.46 each twice) and about twenty-five trailing entries with no
number; superseded rules stay inline; currency is recoverable only by reading an
entry plus every later correction; the `## 8` heading was overwritten by commit
`3d46a3e7` while sixteen comment lines in four manifests cite it; no file
indexed what was in force.

**If left.** The must-read set for "what is the rule here" grows with every
incident — precisely the stated anti-goal — and agents increasingly cite dead or
overridden rules.

**Smallest change.** `docs/RULES.md`, plus one process rule: **when a dated entry
overrides a rule, the override folds into that file in the same commit**, and
`DESIGN.md` becomes write-only history.

**Version.** 0.4.4 — the cheapest finding, and a prerequisite for scaling the
number of agents.

**Status.** **partly discharged by this commit.** `docs/RULES.md` exists;
eighteen of fifty-three rows are folded; thirty-five are marked `not yet folded`
with their addresses listed. The remaining work is folding, one subsystem at a
time, by the ticket that needs it.

---

## D-6 — Cross-crate chains are visible nowhere

*K-10 · C-4* · **Class:** crosses too many places.

**Evidence.** The printed-path chain runs recognition
(`bt-transcript::paths::detect_absolute_path_candidates`) → verdict
(`bt-term::session::verify_path` and the per-pane verdict ledger) → projection
(`bt-viewport::implicit_hyperlinks`, `ViewportFrame::hyperlink_at`) → activation
(`Runtime::activate_hyperlink`, `verified_target_of`) → hand-off
(`bt-platform::handoff::resolved_for_a_door`, `open_local_path_verified`). **No
module doc spans more than two hops**, and §7.1.5j's fold predates the worker
lane, which is documented only in the trailing 2026-09-20/21 entries. The
verdict ledger — a "yes" never re-asked — is a correctness-relevant cache that
nobody owns end to end.

**If left.** A link-behaviour ticket opens five crates, and the eight rounds of
review that branch already cost were each the same defect: new code re-deriving
existing logic and missing one step.

**Smallest change.** A chain registry listing each chain's hops, their types and
their lane, and the single contract that covers recognition, namespace,
freshness, gesture policy, activation and OS completion. **The chain stays
layered** — five crates are not five excessive dependencies; four separately
discoverable policy fragments are the defect.

**Version.** Document **0.4.4**; consider a chain-owner type **0.5**.

**Two readings.** The depth review counts four crates and calls the layering
correct; the breadth review counts five, because the projection hop is real.
Both are verifiable; `docs/ARCHITECTURE.md` §7.1 lists five hops of code across
four crates of policy.

**Status.** **partly discharged.** The printed-path chain is written hop by hop;
attention ingress, resize and paste convergence are named as stubs to fill.

---

## D-7 — Source-reading guards are the de-facto architecture document, and two disagree

*K-14 · C-4* · **Class:** history-only knowledge / no rule.

**Evidence.** 574 source-reading pins over 50 files.
`scripts/check-adapter-boundary.ps1` and `scripts/check-portable-core.ps1` **are**
the crate-layering rule. `the_shell_page_is_gone` walks the crate's source
non-recursively while the platform-gate pin descends — a disagreement between
two whole-program prohibition guards, so a future `runtime/` directory escapes
one of them silently. `FILES_THAT_MAY_NAME_A_PLATFORM` must stay physically in
`main.rs` because a script reads it by name.

**If left.** A guard that outlives the understanding of its rule becomes
superstition, and the guards are where the architecture is actually recorded.

**Smallest change.** The preparation plan's source-reading crate (item identity,
declared universes, mutation as acceptance) is the correct fix and is already
decided. This row adds only: **when the pins stop naming files, the rules they
enforce are stated in prose** — in `docs/RULES.md` or
`docs/ARCHITECTURE.md`.

**Version.** With the move. **Status.** decided (it is the preparation plan); the
prose half is open.

---

## D-8 — The asking/telling family has no taxonomy

*K-3 · C-4 (refinement)* · **Class:** no rule.

**Evidence.** Fifteen surface types before menus and inline fields, eighteen to
twenty-one with them; five landed in one month. Each has a founding ruling and
**nothing maps message kind to surface kind**. Priority exists only as the rung
order inside `Runtime::keyboard_input` and `Runtime::mouse_input`, and that rung
order is the de-facto specification nobody wrote.

**If left.** 0.5's notification model and every outward question arrive as
surface twenty-two and later, chosen by whichever module the agent happened to
be in; the ladders grow a rung per feature.

**Smallest change.** One ruled table — **message kind × urgency × modality →
surface** — with a new surface adding a row before it adds a module.

**Two readings.** The breadth review wants it enforced the way the chord table
already is: a generator plus a diff gate. The depth review adds that the
existing notification policy in `notify::desktop_reach` / `interruption`, backed
by `AttentionLedger`, **is already correct and is preserved**, and that what is
missing is the allocation rule for durable questions, operation results and
persistent pane state — **not a reason to combine every visual surface into one
widget**.

**Version.** Rule written **0.4.4**; enforced before **0.5**. **The table itself
is ruled by the project owner**, who rules UI.

**Status.** open. The inventory and the two fixed points are in
`docs/ARCHITECTURE.md` §8.

---

## D-9 — The three configuration entrances have no map, and 0.5 adds a fourth

*K-4 · C-4 (refinement)* · **Class:** no rule.

**Evidence.** The settings file, the CLI and the environment are each
individually documented — a schema document, a module doc plus §7.2, and
`docs/BT-ENVIRONMENT.md` with a doc-diff test — with **zero cross-references**.
153 `env::var` call sites have no owner module. The two reload disciplines
already differ: `profiles.json` and the pins are watched and re-read live,
`settings.json` is not.

**If left.** An outward CLI/MCP interface is a new entrance; without an
entrance-to-purpose rule it accumulates flags by accretion and drifts from the
in-app settings.

**Smallest change.** One table — **entrance × audience × persistence × reload
discipline** — plus the rule that a new configuration fact declares its
entrance. **Not** a universal "CLI beats environment beats file" ladder: a
launch request and a stored preference are different kinds of input, and the
depth review is explicit that a ladder would be inappropriate.

**Version.** Before 0.5. **Status.** **partly discharged** — the table is in
`docs/ARCHITECTURE.md` §9; the fourth row is written when the outward interface
is designed.

---

## D-10 — Diagnostics have plumbing but no event model

*K-5 · C-4 (refinement)* · **Class:** no rule / history-only knowledge.

**Evidence.** 22 distinct trace variables, about 380 direct-print sites across
14 crates, per-domain fixed line formats, and `trace.rs`'s own doc saying
nothing in it knows what it is tracing. Hang reports, the panic log, stall lines
and standard error are four more destinations. The doc-diff test keeps the
catalogue complete, not coherent.

**If left.** 0.5's self-report and any remote observability have nothing to
subscribe to; each incident adds another variable and another format.

**Smallest change.** **Two readings, and they are compatible.** The breadth
review: one event shape — domain, station, severity, payload — behind the
existing sink, with variables selecting domains rather than inventing formats.
The depth review: the missing piece is a common **operation vocabulary** —
identity, owner, phase, outcome, loss — and explicitly **not one file replacing
every diagnostic channel**, because the destinations and their delivery
semantics are already correctly separated. Which comes first is the
coordinator's call.

**Version.** 0.5. **Status.** open; the ruled shape is in
`docs/ARCHITECTURE.md` §10.

---

## D-11 — The planned split fixes file size, not coupling

*K-7 · C-4 (refinement)* · **Class:** must-read set grows with features.

**Evidence.** The plan says so itself: a theme is not a subsystem; what the move
buys is navigation and merges, with compile time "zero to very slightly
negative". 99 methods fit no topic and are genuinely cross-cutting. 1,073
visibility widenings slightly **grow** the semantic surface. The state census —
`WindowRuntime` 245 fields, `App` 78 — is untouched by the move, and
`Runtime: Deref<Target = TabState>` means it cannot even be measured by a field
grep.

**If left.** The move lands, the files are smaller, and the coupling that makes
the must-read set grow is unchanged — while the completion is read as evidence
of decoupling.

**Smallest change.** Not re-litigation: **aim it**. Pair the move with an
ownership census — resolving each `self.` access to `Runtime`, to
`WindowRuntime`, or through `Deref` to `TabState`, and recording per call,
per field, per effect and per candidate boundary — so the theme files are
drafted against future owners rather than today's name clusters, and so the
orchestrator step starts with targets instead of a standing start. **The
preparation plan proceeds exactly as written**; it is the best-governed document
in the tree.

**Version.** With the move. **Status.** decided (the move and its preparation);
the census is open.

---

## D-12 — `bt-platform` is a drawer whose magnet is stable and whose growth is elsewhere

*K-11* · **Class:** wrong layer / must-read grows with features.

**Evidence.** 64,102 lines, 52 files, 19 external dependencies, five dependents —
and they are the five biggest. `file_reads` is imported by four of the five and
had one commit in the month the crate saw 262 (the platform port). The natural
seams are already visible: the read ledger (pure accounting, no platform
dependencies), the file primitives, the process doors, the inter-process
transports, the window core, the heavy engines, and the platform-specific
modules.

**If left.** Every platform-adjacent ticket pays the whole crate's compile and
read surface.

**Smallest change.** Extract the first three groups into one systems crate. No
new rule is needed — §13.1 already supplies it.

**Version.** 0.5, **after** the runtime move lands; do not run two moves at once.

**Caution, from the preparation plan.** Lifting the read ledger **does not**
remove `bt-term → bt-platform` (see D-14), and the ledger itself must keep one
owner — its lanes and process-wide static are read by five crates, and splitting
them would be a second copy of a fact.

**Status.** open.

---

## D-13 — The `bt-pty → bt-term` edge

*K-12 · C-4 (refinement)* · **Class:** wrong layer (contested).

**Evidence.** Production `bt-pty` never names `bt_term` outside `#[cfg(test)]`;
the only non-test consumer is the development binary
`crates/bt-pty/src/bin/bt-conpty-width-probe.rs`, and the manifest comment
overclaims. **A `src/bin/` target links against the package's normal
dependencies**, so the edge cannot simply be deleted or demoted — that leaves a
broken target.

**If left.** A misleading edge in the crate graph and a stale manifest comment.
Low consequence; it is on this list because two reviews disagreed about what it
means.

**Smallest change.** One of three, chosen in a ticket of its own, **outside the
preparation and outside the relocation commit**: move the probe into
`bt-corpus`, which already depends on both; make the need a feature so the
default graph does not carry it; or accept the edge and record it with that
reason. Verified by building the moved target, never by reading the manifest.

**Three readings.** Breadth: genuinely inverted, an afternoon's work. Depth:
target and dependency hygiene, not evidence that the transport owns terminal
policy. The in-repo preparation plan: not deletable as stated; three
alternatives; choice deferred.

**Version.** 0.4.4. **Status.** open, with the choice already scoped.

---

## D-14 — `bt-term → bt-platform` is broader than its manifest says

*C-4 (refinement) · K-11* · **Class:** wrong layer.

**Evidence.** The manifest comment calls it one call. There are three product
import surfaces: `inline_image::resample_pool` sets a thread priority through
`bt-platform`, `session::verify_path` calls `handoff::resolved_for_a_door`, and
`inline_image::read_and_decode_local_image` goes through the read ledger.

**If left.** A stale comment that a reader trusts, and a portable crate whose
real coupling to the platform crate is invisible in its own manifest.

**Smallest change.** Extract a **small headless observation/effect boundary**
that `bt-term` depends on, rather than reorganising `bt-platform`. Concretely:
the worker priority becomes an injected start handler supplied by whoever
constructs the decode pool, and the path-verification orchestration moves up
while **the function itself does not move** — `handoff::resolved_for_a_door`
stays the single answer, consumed by both `bt-term::verify_path` and
`run_path_verify_worker`, and the result must be bit-identical
(`CONVENTIONS` §十 rule 9). Fix the manifest comment in the same commit.

**Version.** 0.5 — both halves change product code, which a preparation ticket
forbids. **Status.** open.

---

## D-15 — `bt-term → bt-math` is real coupling

*C-4 (refinement) · K-11* · **Class:** wrong layer — **recorded debt, not a task**.

**Evidence.** `session.rs` imports six math types and calls into the math crate
in product code; `inline_image::decode_svg_bytes` rasterises through it;
`crates/bt-term/src/lib.rs` re-exports the engine; and
`crates/bt-term/src/bin/bt-repaint-oracle.rs` uses it in a binary target — the
same target trap as D-13. Hiding the dependency behind re-exports changes
nothing.

**If left.** The terminal crate carries decoration policy. Accepted for now.

**Smallest change.** None attempted until the composition layer is designed.
**This row exists so that the edge is recorded rather than rediscovered.**

**Version.** 0.5 at the earliest. **Status.** decided — recorded as debt.

---

## D-16 — The door pattern has no admission rule

*K-13* · **Class:** no rule.

**Evidence.** The read ledger has ten lanes with a manifest and a source guard;
`quiet_command_named` is pinned as the only child-process construction;
`handoff` holds the only hand-off sites. But **nothing says that a new side
effect gets a door**, and the files column's directory enumeration has no lane,
no door and no guard. `docs/BT-ENVIRONMENT.md` excludes enumeration and metadata
from the ledger's *accounting*, which is a statement about the counters and not
a decision that enumeration needs no door. `folio-web-thumb` is the matching gap
in the thread door: a bare builder at inherited priority, beside the loop.

**If left.** Each new side effect re-litigates where it goes, and the next
enumeration-shaped bypass lands silently.

**Smallest change.** One ruled paragraph: file bytes through the read ledger
with a named lane (**enumeration included**), child processes through the quiet
command door, OS hand-offs through the hand-off module, threads through the
priority spawner with a name and a band — plus one lane-admission line in each
manifest.

**Version.** 0.4.4. **Status.** **partly discharged** — the rule and the gap are
stated in `docs/ARCHITECTURE.md` §6 and `docs/RULES.md` row 52; the enumeration
lane and the thumbnail thread's band are open.

---

## D-17 — The preview's selections have no revisioned mapping to the document

*C-4 (refinement)* · **Class:** no rule.

**Evidence.** `PreviewPane` carries a source caret and a rendered selection;
`preview_edit::EditCaret` works in source byte offsets while
`preview_select::Place` works in block/piece/offset coordinates. **Different
coordinate systems are legitimate and the three models are a ruling**
(`docs/RULES.md` row 8). What is missing is the revisioned mapping from each to
the single editable document, so that a selection taken at one revision cannot
be applied at another.

**If left.** Every editing feature that touches both faces re-derives the
mapping, which is the defect class that already cost eight review rounds
elsewhere.

**Smallest change.** Name the mapping, give it the document's revision, and make
both faces consume it. Do **not** unify the three selection representations.

**Version.** 0.5, with the document owner of D-1. **Status.** open.

---

## Decisions on the reviews' disagreements

The nine points where the two reviews differed are decided in `docs/ARCHITECTURE.md`'s appendix (2026-09-21); the rows above follow those decisions.

## Already decided, recorded here so they are not re-opened

- **The runtime file move.** `main.rs` into themed files. Step 1 (the
  translation-string cut) has landed. It buys navigation and merges; it is
  **not** decoupling and must not be reported as such. See
  `docs/plans/bt-app-split.md`.
- **The preparation.** Every source reader asks the crate rather than a file:
  item identity, declared universes, mutation as acceptance, "a green suite is
  not acceptance". Proceeds exactly as written. See
  `docs/plans/bt-app-split-prep.md`.
- **The dependency direction guard.** A script over the workspace metadata
  reading normal and build dependencies including target-specific tables, with an
  exception set compared against the merge base so it can only shrink, and stale
  exceptions rejected; paired with a per-target scan because the metadata cannot
  see that an edge exists only for a binary target. Second half of the
  preparation.
- **`bt-workbench` may be born now.** A crate holding the attention state
  machine, the semantic notification decisions, and the commands and events an
  outward interface is offered. It does **not** wait for the runtime move. Its
  boundary table, the three 0.6 decisions it forces, and the rule that external
  clients send domain commands rather than runtime methods are in
  `docs/ARCHITECTURE.md` §12.

---

## Incidental — not structural, for the ordinary defect ledger

These were found by the two reviews while reading. They are bugs and
documentation faults, not shapes. **They belong in the ordinary defect ledger
for the version that picks them up, not here**; they are listed once so the
finding is not lost.

- `Runtime::reload_background_picture` can let an older completion overwrite a
  newer unconsumed answer, after which the mailbox's take rejects the older
  generation and **the newer result is lost**.
- `profile_runtime::REMOVAL` is one slot with two writers and no in-flight
  latch; the last report written wins (also the subject of D-3).
- `psreadline-probe` and `copilot-version-probe` block on their child with no
  timeout; a hung probe thread is never reclaimed.
- `folio-web-thumb` is spawned with a bare builder at inherited priority,
  breaking the band rule, and **panics on spawn failure**.
  `folio-video-canplay` holds the video stack's last unbounded join.
  `bt-dir-watch` has an unbounded receive in its start and an unbounded join in
  its drop.
- `crates/bt-pty/Cargo.toml`'s comment claims the library depends on the
  terminal crate; production code never names it.
- `crates/bt-term/Cargo.toml`'s "one call" comment is stale — there are three
  import surfaces (D-14).
- `bt-term::session::opening_it_would_run_it` has a doc comment saying a platform
  helper answers the question off Unix, while that arm returns a constant and the
  helper is never called in that crate.
- `the_shell_page_is_gone` walks the crate's source non-recursively, so a future
  `runtime/` directory silently escapes a whole-program prohibition guard (D-7).
- `crates/bt-platform/src/lib.rs` contains an embedded NUL byte; the file is not
  clean UTF-8.
- macOS uninstall cleanup cannot honour its own safety check: the
  process-holding probe is a no-op off Windows, so "no process holds the data"
  is unverifiable there.
- The 2026-09-21 process and thread survey is internally inconsistent about its
  own lane count (one table lists six, a later section says seven, because math,
  scaling and path verification share one spawn site).
- `docs/plans/bt-app-split.md` and `docs/plans/bt-app-split-prep.md` are
  saturated with line-number locators, against `CONVENTIONS` §十 rule 7 — which
  was written because of this split — and the preparation's §5 cites the wrong
  section for the heavy-operation rule, which lives in §十 rule 8.

## D-18 — the inventory's subject extraction reads a query's argument as a file-bound subject (2026-09-22)

`scripts/dev/bt-app-split-freshness.py`'s census extracts a reader's subjects lexically, so a migrated body pin such as `method_body("Runtime", "apply_psreadline")` is counted as if the test still read `apply_psreadline` out of a file, and the row's impact reads *subject moves: retarget atomically* although the reading follows the item. This is §2.6's "a reader's own needle is not an occurrence" one level up, in subject extraction rather than search exclusion, and it will misclassify every migrated pin that names a `Runtime` method. The generator also bound a subject by bare name until 2026-09-22 (`add_to_profile`, `graph_filter_branches` — each declared twice); it now refuses a name whose declarations disagree about the move. Owed: subject extraction that tells a `bt-source` query argument from a file reading's needle, or a census that asks `bt-source` for the reader's subjects instead of scanning text.
