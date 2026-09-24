# Architecture debt ledger

**The rule (2026-09-23).** This is the one ledger of architecture debt. Every row
is assigned to a ticket and a version, and **the ledger reads zero before 0.5
starts**: 0.4.5 is typing stability, per-pane zoom and what fits beside them;
0.4.6 is the updater and architecture closure. A row that cannot be cleared by
0.4.6 says why and where it goes (`deferred →` below). Two obligations follow:

- **Every ticket's *Architecture impact* section cites the rows it repays,
  moves or adds**, by ID. A ticket that repays a row sets its status to
  `repaid on <sha>` in the same commit; a ticket that finds new debt adds a row
  here rather than a sentence in its report.
- **A row leaves only by being repaid, or by a dated ruling that it is not
  debt.** Rows are never renumbered; a repaid row stays, with its sha.

The file began as the 2026-09-21 structure review's debt list (D-1…D-18, kept
below with their IDs and text). On 2026-09-23 it gained the ledger columns and
every debt the repository already recorded elsewhere (D-19…D-63); on
2026-09-24, two rows found by the 0.4.5 drafts (D-64, D-65). It is
**ordered by consequence, not by severity** — nothing here is a bug; each row is
a shape that makes the next hundred tickets more expensive, and the cost it
charges is the one the project named: *what must be read to finish one ticket
must not grow with the number of features.*

**Ledger columns.** *Source* — where the debt was first recorded. *Ticket* — the
ticket that repays it (a number from the 0.4.4/0.4.5 ticket set, a preparation
step such as P14, or `none yet`). *Version* — 0.4.5 · 0.4.6 ·
`deferred → <where>` with the reason. *Status* — `open` · `in ticket` ·
`repaid on <sha>`; a note after a dash records a part already repaid inside an
open row. The versions are proposals until the release plan adopts them.

**Not on this ledger.** Defects (the adversarial-review ledgers, and the
incidental list below); UI constants
(`docs/design/UI-DEVIATIONS.md`, which tickets 16–31 take to zero on their own
track); rows 15–19 of `docs/ARCHITECTURE.md` §5.3, which are *ruled to stay*;
and the probes in `scripts/ci/ignored-tests.txt`, which are ignored by policy —
they answer "what does this machine do" — not by debt.

## By version

| version | rows | open | repaid |
|---|---:|---:|---:|
| 0.4.5 | 22 | 22 | 0 |
| 0.4.6 | 34 | 34 | 0 |
| deferred (reason on the row) | 8 | 8 | 0 |
| already repaid | 1 | 0 | 1 |
| **total** | **65** | **64** | **1** |

Parts already repaid inside open rows, by the 0.4.4 tickets: ticket 10
(`2657e5e3`) — §5.3 row 1, the OS hand-off lane, and the first instance of the
lane contract (D-2, D-33); ticket 34 (`fbfab1ff`) — the marks-lock wait behind
our own writer (D-2, D-34) and the `profile_runtime` CI failures (D-58, repaid
whole); ticket 14 (`5d4c7aff`) — the printed-path chain's hand-off hop kept
current in §7.1 (D-6); ticket 05 (`5f433943`) — §9's paragraph that export is
not a fourth entrance (D-9). Ticket 32 (`4ba5df7e`) repaid no row and added none.
Ticket 39 (`66174d8a`) repaid no row and added none: the web pane's arrow now
hands an address to the browser through the OS hand-off lane, one more caller of
the lane §5.3 row 1 already made, not a part of any open row. As of 2026-09-24
no part of D-64 or D-65 is repaid.

## The ledger

"§" alone means a section of `docs/ARCHITECTURE.md`. "Split prep" is
`docs/plans/bt-app-split-prep.md`. "Survey Part 4" is the 2026-09-21 process and
thread survey's list of twenty-two facts with more than one owner, which
§4.2 sorts into five classes; the fact-to-class assignment in D-51…D-55 is this
ledger's.

| ID | what | source | ticket | version | status |
|---|---|---|---|---|---|
| D-1 | session state has no owner independent of the window | structure review C-1 · K-1 | none yet | deferred → 0.5 (attention and session identity), 0.6 (backend) — it *is* the 0.5/0.6 work; its 0.4.6 first step is D-57 | open |
| D-2 | the window thread's blocking set is a list, not a budget | C-2 · K-6 | through D-33…D-47 | 0.4.6 — closes when its rows close | open — §5.3 row 1 repaid on `2657e5e3` |
| D-3 | ten one-shot probes with no common contract | K-9 · C-2 | none yet | 0.4.6 | open |
| D-4 | controlled failure loses dirty preview edits | C-3 · K-8 | none yet | 0.4.5 — ruled for 0.4.4 and never ticketed; unsaved edits are a hard requirement | open |
| D-5 | the rules existed only as history — 35 `docs/RULES.md` rows not yet folded | K-2 · C-4 | the ticket that depends on each row | 0.4.6; a row a 0.4.5 ticket depends on (resize, PTY, IME, keyboard and mouse routing, fonts, GPU lifecycle) folds in that ticket | open — 19 folded |
| D-6 | cross-crate chains are visible nowhere | K-10 · C-4 | through D-48…D-50 | 0.4.6 | open — printed path written; its hand-off hop updated on `2657e5e3` and `5d4c7aff` |
| D-7 | source-reading guards are the architecture document; their rules owe prose | K-14 · C-4 | with D-28 | 0.4.6 | open |
| D-8 | the asking/telling family has no taxonomy | K-3 · C-4 | none yet | 0.4.6 — the owner rules the table first | open |
| D-9 | configuration entrances: the fourth row | K-4 · C-4 | none yet | deferred → 0.5's outward-interface design — the fourth entrance does not exist until then | open — §9 table written; export ruled not an entrance on `5f433943` |
| D-10 | diagnostics have plumbing but no event model | K-5 · C-4 | none yet | 0.4.6 — the operation vocabulary; its event carrier waits for a subscriber | open |
| D-11 | the split fixes file size, not coupling — the ownership census | K-7 · C-4 | none yet | 0.4.6, with D-32 | open |
| D-12 | `bt-platform` is a drawer | K-11 | none yet | deferred → 0.5 — one move at a time, and the `bt-app` move ends with D-32 in 0.4.6 | open |
| D-13 | the `bt-pty → bt-term` edge | K-12 · C-4 · split prep P21 | P21 | 0.4.5 | open |
| D-14 | `bt-term → bt-platform` is broader than its manifest | C-4 · K-11 | none yet | 0.4.6 | open |
| D-15 | `bt-term → bt-math` is real coupling | C-4 · K-11 | recorded by D-27 | deferred → the composition-layer design (0.5) — nothing to repay before that layer exists | open (recorded debt) |
| D-16 | the door pattern: the enumeration lane and the thumbnail thread's band | K-13 | none yet | 0.4.6 | open — rule stated |
| D-17 | preview selections have no revisioned mapping to the document | C-4 | none yet | deferred → 0.5, with D-1's document owner | open |
| D-18 | the census reads a query's argument as a file-bound subject | split prep, 2026-09-22 | none yet | 0.4.5 — D-29…D-32 need a true census | open |
| D-19 | MIGRATION-DEBT class P0 — the documentation generators (3 rows) | `docs/plans/MIGRATION-DEBT.tsv`; split prep §6 | P0 | 0.4.5 | open |
| D-20 | MIGRATION-DEBT class P10 — the portable-core walk (1 row) | same | P10 | 0.4.5 | open |
| D-21 | MIGRATION-DEBT class P12 — `bt-platform`'s walkers and the `stand_in` guard (5 rows) | same | P12 | 0.4.5 | open |
| D-22 | MIGRATION-DEBT class P13 — the remaining source-text walks (3 rows) | same | P13 | 0.4.5 | open |
| D-23 | MIGRATION-DEBT class P14 — named-body pins (192 rows) | same | P14 | 0.4.6 | open |
| D-24 | MIGRATION-DEBT class P16 — ledger keys naming a file (63 rows), and the ten text `#[cfg(test)]` splits | same | P16 | 0.4.6 | open |
| D-25 | MIGRATION-DEBT class P17 — cross-crate and script readers (13 rows) | same | P17 | 0.4.5 | open |
| D-26 | the ~110 `[..].concat()` needle halves written whole | split prep §6 | P18 | 0.4.6 | open |
| D-27 | the CI dependency-direction guard, and `bt-term → bt-math` recorded | split prep §8.4–§8.5 | P19 | 0.4.5 | open |
| D-28 | MIGRATION-DEBT to zero and deleted; the allowlist final | split prep §7.2 | P20 | 0.4.6 | open |
| D-29 | the unmoved topic `launch` (4 methods) | split prep Appendix C | none yet | 0.4.5 | open |
| D-30 | the unmoved topic `settings` (31 methods) | split prep Appendix C | none yet | 0.4.5 | open |
| D-31 | the unmoved topic `focus` (51 methods) | split prep Appendix C | none yet | 0.4.5 | open |
| D-32 | the unassigned `Runtime` methods still in `main.rs` (112 at the move, 115 today) | split prep §7.1, Appendix C | none yet | 0.4.6 | open |
| D-33 | the lane contract as one shape, wrapping the existing lanes | §5.4 step 1, §5.1 | none yet | 0.4.5 — the presentation lane (D-41) is its second client | open — first instance, `handoff_lane`, on `2657e5e3` |
| D-34 | §5.3 row 2 — the marks lock's install half on the window thread | §5.3 | none yet | 0.4.6 | open — the wait behind our own writer repaid on `fbfab1ff` |
| D-35 | §5.3 row 3 — `psreadline::apply_recorded`, nine files under the lock | §5.3 | none yet | 0.4.6 | open |
| D-36 | §5.3 row 4 — `psreadline::installed_copy`'s recursive walk | §5.3 | none yet | 0.4.6 | open |
| D-37 | §5.3 row 5 — the machine's whole font collection enumerated inline | §5.3 | none yet | 0.4.5 — the traced frozen gear | open |
| D-38 | §5.3 row 6 — the find box re-scans every frozen line per keystroke | §5.3 | none yet | 0.4.5 — a per-keystroke cost | open |
| D-39 | §5.3 row 7 — macOS locale children on the pane-birth road | §5.3 | none yet | 0.4.6 | open |
| D-40 | §5.3 row 8 — macOS `DirWatch` start and drop wait without a bound | §5.3 | none yet | 0.4.6 | open |
| D-41 | §5.3 row 9 — presentation on the window thread; the present mode has no owner | §5.3; §5.4 step 4 | none yet | 0.4.5 — presenting off the input thread is the typing-stability work | open |
| D-42 | §5.3 row 10 — device recovery blocks and sleeps on the window thread | §5.3; §5.4 step 4 | none yet | 0.4.6, after D-41 | open |
| D-43 | §5.3 row 11 — PTY birth on the window thread | §5.3; §5.4 step 5 | none yet | deferred → 0.5 toward 0.6 — needs D-1's session owner to keep input and resize order | open |
| D-44 | §5.3 row 12 — the synchronous `ResizePseudoConsole` round trip | §5.3; §5.4 step 5 | none yet | deferred → 0.5 toward 0.6 — as D-43 | open |
| D-45 | §5.3 row 13 — `sample_window_place` resampled at three sites for one instant | §5.3 | none yet | 0.4.5 — one site is `drain_pty` | open |
| D-46 | §5.3 row 14 — `Window::set_title` at five sites with no throttle | §5.3 | none yet | 0.4.5 — one site is `drain_pty` | open |
| D-47 | §5.3 row 20 — renames, the preserving save and store writes on the window thread | §5.3 | none yet | 0.4.6 | open |
| D-48 | §7.2 chain stub — attention ingress | §7.2 | none yet | 0.4.6, with D-57 | open |
| D-49 | §7.2 chain stub — resize | §7.2 | none yet | 0.4.6 | open |
| D-50 | §7.2 chain stub — paste convergence | §7.2 | none yet | 0.4.5 — tickets 02 and 03 have just walked it | open |
| D-51 | §4.2 class — observations of external state (survey facts 1, 5, 6, 7, 12) | §4.2; survey Part 4 | none yet | 0.4.6 | open |
| D-52 | §4.2 class — asynchronous publication and competing operations (facts 4, 10, 13, 19, 22) | §4.2; survey Part 4 | none yet | 0.4.6, with D-3 | open |
| D-53 | §4.2 class — durability and external transactions (facts 8, 9, 11) | §4.2; survey Part 4 | none yet | 0.4.6, with D-34 and D-47 | open |
| D-54 | §4.2 class — identity, admission and lifecycle (facts 2, 3, 14, 20) | §4.2; survey Part 4 | none yet | 0.4.6 | open |
| D-55 | §4.2 class — projections, delivery and loss (facts 15, 16, 17, 18, 21) | §4.2; survey Part 4 | none yet | 0.4.6 | open |
| D-56 | §11 emergency termination — a journal and a defined recoverable revision | §11 | none yet | 0.4.6 | open |
| D-57 | §12.1 — `bt-workbench` is born | §12.1 | none yet | 0.4.6 | open |
| D-58 | `profile_runtime`'s two tests failing `WouldBlock` on a slow CI disk | `docs/DESIGN.md`, 2026-09-23 | 34 | — | repaid on `fbfab1ff` |
| D-59 | `bt-render`'s two atlas soaks, ignored under protest | `scripts/ci/ignored-tests.txt` | none yet | deferred → the version that gains a CI runner with a real graphics adapter; whether to provision one is decided in 0.4.6 | open |
| D-60 | two macOS `http` tests that reach the network | `docs/plans/port/m4-7/transcript.md` | none yet | 0.4.6 | open |
| D-61 | a Mac-only red test in `webnav` | ticket 13's report | none yet | 0.4.5 | open |
| D-62 | `bt-render` fails clippy on macOS: three unused constants | ticket 13's report | none yet | 0.4.5 | open |
| D-63 | the macOS CI job tests none of `bt-app`, `bt-term`, `bt-render` and lints only `bt-platform` | `.github/workflows/ci.yml`, `core-macos` | none yet | 0.4.6 | open |
| D-64 | opening a web page holds the window thread for seconds: WebView2 environment and controller creation and `drive_web_page`'s install burst, unprobed inside `window_event` | the 2026-09-23 investigation of hover cards, float drag and web-open stutter, §3; ticket 43 | 43 | 0.4.5 — a multi-second hold on the input thread is the typing-stability work | open |
| D-65 | overlay fades are folded per primitive and blended in linear light: a fading surface shows its text before its plate, and translucent inks differ from the CSS mock | the 2026-09-23 fade audit, §0–§2 and §7; ticket 46 | 46; the L variant none yet | 0.4.6 — the group composite (ticket 46, M) in 0.4.5; the L variant, all overlay translucency in encoded space, in 0.4.6 before the 0.5 restyle, and the row closes with it | open |

---

# The rows

## How the original rows read

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
removes the class, not the instance. *Version* — as ruled on 2026-09-21 (before
the move · with the move · 0.4.4 · 0.5 · 0.6). *Status* — as of 2026-09-21
(open · partly discharged · decided). **The ledger table above supersedes both
fields**; each row's *Ledger* line repeats its current entry.

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

**Ledger.** source: structure review C-1 · K-1 · ticket: none yet · version: deferred → 0.5 (attention and session identity), 0.6 (backend) — it *is* the 0.5/0.6 work; its 0.4.6 first step is D-57 · status: open.

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

**Ledger.** source: C-2 · K-6 · ticket: through D-33…D-47 · version: 0.4.6 — closes when its rows close · status: open — §5.3 row 1 repaid on `2657e5e3`.

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

**Ledger.** source: K-9 · C-2 · ticket: none yet · version: 0.4.6 · status: open.

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

**Ledger.** source: C-3 · K-8 · ticket: none yet · version: 0.4.5 — ruled for 0.4.4 and never ticketed; unsaved edits are a hard requirement · status: open.

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

**Ledger.** source: K-2 · C-4 · ticket: the ticket that depends on each row · version: 0.4.6; a row a 0.4.5 ticket depends on (resize, PTY, IME, keyboard and mouse routing, fonts, GPU lifecycle) folds in that ticket · status: open — 19 folded.

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

**Ledger.** source: K-10 · C-4 · ticket: through D-48…D-50 · version: 0.4.6 · status: open — printed path written; its hand-off hop updated on `2657e5e3` and `5d4c7aff`.

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

**Ledger.** source: K-14 · C-4 · ticket: with D-28 · version: 0.4.6 · status: open.

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

**Inventory rows, added by the tickets that add a case (the owner rules the
table; these rows only record what exists).**

| added | message kind | urgency | modality | surface | where the surface is absent |
|---|---|---|---|---|---|
| 2026-09-24, ticket 37 | persistent pane state — a terminal pane's text size while it is not 100 % | non-urgent | non-modal | the existing pane-head control (a slot of `seats::PaneHeadGeometry`); a click resets it | open (owner) — a lone terminal and a narrow head wear no head |

**Ledger.** source: K-3 · C-4 · ticket: none yet · version: 0.4.6 — the owner rules the table first · status: open.

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

**Ledger.** source: K-4 · C-4 · ticket: none yet · version: deferred → 0.5's outward-interface design — the fourth entrance does not exist until then · status: open — §9 table written; export ruled not an entrance on `5f433943`.

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

**Ledger.** source: K-5 · C-4 · ticket: none yet · version: 0.4.6 — the operation vocabulary; its event carrier waits for a subscriber · status: open.

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

**Ledger.** source: K-7 · C-4 · ticket: none yet · version: 0.4.6, with D-32 · status: open.

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

**Ledger.** source: K-11 · ticket: none yet · version: deferred → 0.5 — one move at a time, and the `bt-app` move ends with D-32 in 0.4.6 · status: open.

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

**Ledger.** source: K-12 · C-4 · split prep P21 · ticket: P21 · version: 0.4.5 · status: open.

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

**Ledger.** source: C-4 · K-11 · ticket: none yet · version: 0.4.6 · status: open.

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

**Ledger.** source: C-4 · K-11 · ticket: recorded by D-27 · version: deferred → the composition-layer design (0.5) — nothing to repay before that layer exists · status: open (recorded debt).

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

**Ledger.** source: K-13 · ticket: none yet · version: 0.4.6 · status: open — rule stated.

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

**Ledger.** source: C-4 · ticket: none yet · version: deferred → 0.5, with D-1's document owner · status: open.

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
  `docs/plans/bt-app-split-prep.md`. What remains of it is in the ledger as
  D-19…D-32.
- **The dependency direction guard.** A script over the workspace metadata
  reading normal and build dependencies including target-specific tables, with an
  exception set compared against the merge base so it can only shrink, and stale
  exceptions rejected; paired with a per-target scan because the metadata cannot
  see that an edge exists only for a binary target. Second half of the
  preparation; in the ledger as D-27.
- **`bt-workbench` may be born now.** A crate holding the attention state
  machine, the semantic notification decisions, and the commands and events an
  outward interface is offered. It does **not** wait for the runtime move. Its
  boundary table, the three 0.6 decisions it forces, and the rule that external
  clients send domain commands rather than runtime methods are in
  `docs/ARCHITECTURE.md` §12. Its birth is in the ledger as D-57.

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

Six more were found on 2026-09-23 by the fade audit (*which surfaces show their
content before their plate*, §4), beside D-65. **All six are fixed by ticket
46.**

- Video and GIF pictures ignore every fade: `Runtime::video_layers` builds
  every video and animation layer at opacity 1.0, so a recording on the glance
  card is at full strength from the card's first frame — fixed by ticket 46.
- The docked video bar's labels never fade: `VideoSeat::bar` fades its quads
  and sprites by hand and returns a layer at opacity 1.0, and a label carries no
  alpha — fixed by ticket 46.
- The files flyout travels on exit, against UI-SPEC §7's "nothing travels on
  exit": `float::fade` with `reverse` still returns a rise — fixed by ticket 46.
- Stale doc comments: `tooltip::hover_fade_opacity` says there is no fade out,
  and `OverlayLayer::opacity`'s "under 2.5% of an already-invisible ink" is
  false in linear light — fixed by ticket 46.
- `palette::build`'s opacity parameter is dead; its one caller passes 1.0 —
  fixed by ticket 46.
- Opacity assigned, not multiplied: `Runtime::float_layer` assigns
  `bar.opacity` and `dock_overlay_layers` assigns `layer.opacity`, against the
  multiplying rule `Runtime::file_peek_layer` states — fixed by ticket 46.

## D-18 — the inventory's subject extraction reads a query's argument as a file-bound subject (2026-09-22)

`scripts/dev/bt-app-split-freshness.py`'s census extracts a reader's subjects lexically, so a migrated body pin such as `method_body("Runtime", "apply_psreadline")` is counted as if the test still read `apply_psreadline` out of a file, and the row's impact reads *subject moves: retarget atomically* although the reading follows the item. This is §2.6's "a reader's own needle is not an occurrence" one level up, in subject extraction rather than search exclusion, and it will misclassify every migrated pin that names a `Runtime` method. The generator also bound a subject by bare name until 2026-09-22 (`add_to_profile`, `graph_filter_branches` — each declared twice); it now refuses a name whose declarations disagree about the move. Owed: subject extraction that tells a `bt-source` query argument from a file reading's needle, or a census that asks `bt-source` for the reader's subjects instead of scanning text.

**Ledger.** source: split prep, 2026-09-22 · ticket: none yet · version: 0.4.5 — D-29…D-32 need a true census · status: open.

---

## D-19…D-63 — the rows added on 2026-09-23

One line each: what it is, its anchor by name, and why the proposed version.
Ticket, version and status are in the ledger table.

### The split's second half — MIGRATION-DEBT, by class (280 rows)

`docs/plans/MIGRATION-DEBT.tsv` lists every source reader that still names a
file; it only shrinks, and `scripts/ci/check-migration-debt.ps1` holds that.
Every 0.4.4 ticket reported it at 280 → 280. The classes are the preparation
plan's own batching (split prep §6, the ticket table); the counts are the
file's `ticket` column.

- **D-19 · P0 (3 rows).** The three Python documentation generators — a
  directory walk and named files, named files out of git blobs, named modules;
  anchors `scripts/dev/bt-app-graph.py`, `scripts/dev/bt-app-split-freshness.py`.
  0.4.5, because D-29…D-32 regenerate the inventory through them.
- **D-20 · P10 (1 row).** The portable-core directory walk and its Rust twin;
  anchor `scripts/check-portable-core.ps1`, the agreement test
  `the_gate_and_its_script_walk_the_same_files`. 0.4.5: small.
- **D-21 · P12 (5 rows).** `bt-platform`'s three directory walkers and the
  `stand_in` guard, one ticket because they share a file. 0.4.5: CI-only work
  that can run beside the typing lane.
- **D-22 · P13 (3 rows).** The remaining source-text walks — one directory walk,
  two `include_str!`. 0.4.5.
- **D-23 · P14 (192 rows).** Named-body pins: 153 `include_str!`, 37 fixture
  or manifest reads, 2 runtime reads — 49 call sites, 13 helpers, 16 module
  batches in the plan's count. 0.4.6: the largest class, mechanical, batched by
  module.
- **D-24 · P16 (63 rows).** `file_reads_doors.txt` keys that name a file, plus
  the ten text `#[cfg(test)]` splits in seven files, each decided into a named
  scope. 0.4.6.
- **D-25 · P17 (13 rows).** Cross-crate and script readers — five
  `include_str!`, four runtime reads, four script reads of named files;
  anchors the two `bt-term` integration-test readers,
  `uninstall_tests`' hand-supplied file list, `context_menu` and `msix`. 0.4.5:
  `uninstall_tests` reads a module graph rather than a list before settings
  moves.
- **D-26 · P18 (not on the list).** The ~110 `[..].concat()` needle halves,
  written whole now that `bt-source`'s provenance prevents self-match. A
  readability change with no coverage consequence; 0.4.6.
- **D-27 · P19.** The CI dependency-direction guard of split prep §8.4 over
  `cargo metadata` plus a per-target scan, with the exception set compared
  against the merge base; and the line recording `bt-term → bt-math` (D-15).
  0.4.5: cheap, independent of `bt-source`, and it keeps D-13's repair from
  drifting back.
- **D-28 · P20.** MIGRATION-DEBT at zero and deleted; `bt_source::FileScoped`
  final at four entries or fewer, each with a reason; the tripwire
  (`crates/bt-source/tests/tripwire.rs`) green. 0.4.6: it is the class sum.

### The relocation's residue

Step 2a moved 25 of 28 topics (1,195 methods) into `crates/bt-app/src/runtime/`.
Each unmoved topic is held by a reader the move turns red — by split prep §6.0
the reader's defect, not the move's. **0.4.5 for all three**: each is one
reader fix plus a pure move, and moving them before the typing-stability work
edits `main.rs` saves that work a rebase.

- **D-29 · `launch` (4: `create`, `reseed_editor_env`, `apply_launch_opens`,
  `arrival_fits`).** Blocked by
  `shell_integration::tests::shell_integration_startup_and_removal_doors_are_above_window_work`,
  which reads `include_str!("main.rs")` for `shell_integration::begin_startup_migration();`
  — a positive bound to a file. Fix: a positive that follows `Runtime::create`.
- **D-30 · `settings` (31).** Blocked by
  `focus_mode_door_tests::only_the_chord_and_the_settings_row_write_the_bit`,
  which compares the `self.set_focus_mode(` sites in universe order against a
  fixed-order list — two sets compared as sequences. Moving it also orphans
  the root's `KeyEventExtModifierSupplement` import and its three-line
  rationale comment, which need a new home.
- **D-31 · `focus` (51).** Blocked by
  `focus_mode_door_tests::the_cards_offer_is_spent_in_one_place_and_given_back_in_one`,
  the same ordered-list shape over `settings.cards_gesture_hint_offer =`.
  Moving settings and focus together fixes this one and breaks D-30's, so both
  readers become multiset comparisons first.
- **D-32 · the unassigned methods.** 112 `Runtime` methods the theme regex
  assigns to no topic stayed in `main.rs`'s two `impl Runtime<'_>` blocks
  (listed in the move's record); three more have been added there since
  (`apply_web_color_scheme`, `hand_uri_to_the_system`,
  `open_unverified_reference`), so the blocks hold 201 methods today: 115
  unassigned plus the 86 of D-29…D-31. Owed: an item-level destination for
  each, drafted against D-11's census rather than name clusters, then the move.
  0.4.6, after D-18 and D-11.

### `docs/ARCHITECTURE.md` — lanes (§5)

- **D-33 · §5.4 step 1.** One lane contract — request identity, resource
  ordering, capacity, completion, abandonment, wake obligation — and the
  unwritten rule for which of the three return mechanisms of §5.1 a lane uses.
  `bt-app::handoff_lane` (ticket 10) is its first instance; the other lanes are
  wrapped, not rederived. 0.4.5, because the presentation lane (D-41) needs it.
- **D-34…D-47 · the open §5.3 exceptions.** Each is one row of §5.3's table,
  anchored there by call site; §5.4 steps 2–5 are these rows in order. Rows 2,
  3, 4, 7, 8 and 20 are the storage and observation lanes' first passengers and
  go to 0.4.6 with D-3 and D-53. Rows 5, 6, 9, 13 and 14 cost a keystroke or a
  frame on the typing path (`apply_stored_terminal_font`, `Runtime::turn`'s
  search refresh, `Runtime::present_seats_and_commit`, `drain_pty`) and go to
  0.4.5. Row 10 follows row 9 into 0.4.6. Rows 11 and 12 wait for the session
  owner (D-1): moving PTY birth or resize without it loses the ordering
  `flush_pending_pty_resize` represents.

### `docs/ARCHITECTURE.md` — chains (§7.2)

- **D-48 · attention ingress.** Three lanes into `AttentionLedger::apply`
  (the escape sequence through `AdapterEvent`, the pipe through
  `attention_wire`, the `folio attention` verb) and out through
  `deliver_attention`, `settle_attention`, `answer_attention`,
  `raise_attention`. 0.4.6, written with D-57, which moves its core.
- **D-49 · resize.** `ResizePlan`, `DualPlaneSession::resize_at`, the reflow,
  `PtySession::resize`, and the free functions in `main.rs` that sequence them;
  the order `flush_pending_pty_resize` represents is the contract. 0.4.6.
- **D-50 · paste convergence.** `Runtime::prepare_clipboard_paste`,
  `paste_text`, `bt-term`'s `input::paste_bytes`, and the clipboard read on
  the window thread. 0.4.5: tickets 02 and 03 changed `deliver_paste` and the
  hops are fresh.

### `docs/ARCHITECTURE.md` — ownership (§4)

§4.1's three-owner split is D-1; §4.3's `Deref` trap is D-11's census. §4.2
rules that the survey's twenty-two multi-owner facts are five problems with
one rule each; none of the five rules is yet a shared shape in the code.
Collapsing a class means its facts follow its one rule through one contract,
with each fact's differing policy kept. All five are 0.4.6.

- **D-51 · observations of external state** — facts 1 (the printed-path
  verdict ledger), 5 (`profiles::title`'s cache, keyed on one of its two
  inputs), 6 (PSReadLine's three copies), 7 (settings, profiles and pins as
  read), 12 (`shell_integration::PROFILE_ANSWERS`, never re-asked).
- **D-52 · asynchronous publication and competing operations** — facts 4
  (`schemes::CATALOGUE` and `REVISION`), 10 (Explorer registration's three
  copies), 13 (the font slots, two writers), 19 (`profile_runtime::REMOVAL`,
  one slot, two writers, no latch), 22 (the generation check re-derived three
  times: `window.background_decode`, `window.clipboard_picture`, the web host).
- **D-53 · durability and external transactions** — facts 8 (the session
  snapshot's three copies), 9 (the marks record), 11 (the update check's
  memory, file and claim).
- **D-54 · identity, admission and lifecycle** — facts 2
  (`launch_wire::ADMITTING`, one turn old), 3 (`hang_watch`'s opinion consumed
  by `launch_wire::admit`), 14 (`LeafWake::rebind`, the repair to copy), 20
  (the WebView2 generations).
- **D-55 · projections, delivery and loss** — facts 15 (video frames), 16
  (`file_reads::LEDGER`), 17 (`attention_wire::INBOX`), 18 (`trace_sink`'s
  queue), 21 (`PresentGate`): each publication declares its kind.

### `docs/ARCHITECTURE.md` — failure roads (§11) and 0.5 (§12)

- **D-56 · emergency termination.** `install_panic_log_hook` has no safe access
  to dirty buffers. Owed: a journal kept before the failure, independent of the
  owner's locks, and a stated recoverable revision with the bounded tail that
  may be lost. D-4 is the controlled-failure half. 0.4.6: the journal is new
  storage and belongs with the storage lane.
- **D-57 · `bt-workbench` born.** The attention state machine, the semantic
  notification decisions and the outward commands and events, as §12.1's
  table; `AttentionLedger::apply` moves first, `attention_wire::WAIT_TTL`'s
  dependency reverses. 0.4.6: it may be born now and is D-1's first step.

### Tests

- **D-58 · `profile_runtime` under a slow disk.** Two tests
  (`shell_integration_first_run_done_tells_the_window_nothing`,
  `shell_integration_two_of_our_own_writers_queue_and_both_finish`) failed
  `WouldBlock` on CI because our own waiter gave up after `OUR_TURN`. Ticket 34
  made a wait behind our own writer a queue; repaid on `fbfab1ff`.
- **D-59 · the two atlas soaks.** `tests::a_long_chinese_session_never_runs_the_atlas_out_of_room`
  and `tests::a_session_long_enough_to_wear_the_packer_out_gets_its_text_back`
  in `bt-render` are regression gates, not probes, and are ignored only because
  CI's software adapter dies before the packer is under pressure. Deferred:
  they come off the list when a runner with a real adapter exists, and nothing
  a ticket can do to the code changes that.
- **D-60 · macOS network tests.** `macos_http`'s
  `the_releases_list_comes_back_as_json` and
  `a_body_longer_than_the_cap_is_an_error` reach the network and failed with
  "The request timed out" in an unrelated run. Owed: a local server, or a
  reason recorded on the ignored list. 0.4.6.

The other entries of `scripts/ci/ignored-tests.txt` are probes, one writer and
two privilege-bound fixtures, each with its reason there; they are not debt.

### macOS

- **D-61 · `webnav::tests::a_local_file_is_shown_and_typed_as_a_path_and_loaded_as_a_uri`**
  is red on a Mac: `file_url_of_local_path` receives a `D:\…` path there and
  answers `None`. The test states a Windows fact on every platform. 0.4.5: small.
- **D-62 · `bt-render` clippy on macOS.** `CJK_FALLBACK_FAMILIES`,
  `CJK_FALLBACK_FONT_FILES` and `CHROME_SANS_FONT_FILES` are never used there,
  so `cargo clippy -p bt-app` stops in `bt-render`. 0.4.5: small.
- **D-63 · the macOS CI job's reach.** `core-macos` checks `bt-app`, `bt-term`
  and `bt-render` but tests none of them, and runs clippy on `bt-platform`
  only — which is why D-61 and D-62 were found by a person. 0.4.6, after D-61
  and D-62 make widening it green.

---

## D-64…D-65 — the rows added on 2026-09-24

- **D-64 · a web page's open holds the window thread.** A stall self-report on
  the next89 candidate recorded two holds while a web preview opened: 4,099 ms
  and 2,884 ms, `window_event` 3,979 and 2,703 ms with every named child under
  130 ms, thread CPU 437 ms, +8,793 and +2,931 page faults. The time is in an
  unnamed remainder of `window_event`: the first page's
  `CreateCoreWebView2EnvironmentWithOptions` (charged to the gesture's station),
  `request_controller`'s `CreateCoreWebView2CompositionController`, and the
  `WebEffect::InstallEvents` burst inside `drive_web_page` — `attach_web_visual`,
  `WebHost::install` with its settings calls, `SetRootVisualTarget`,
  `stand_on_the_floor`, the first `Navigate`, `refresh_chrome`. No row of §5.3
  covers WebView creation: §5.2 keeps the controller and its native views on the
  window thread, but not the waiting for them. **D-64 is a new row of §5.3's
  table**, one of D-34…D-47's kind, written there by ticket 43 with its number.
  Owed: the phases probed as stations of the self-report, the environment
  requested on a lane, the controller's callback not pumped inside a gesture,
  and the install burst split across turns. 0.4.5: ticket 43 adds the probes
  first and repays or narrows the row; a residue under the frame budget closes it.
- **D-65 · overlay fades composited per primitive in linear light.**
  `OverlayLayer::opacity` is folded into each primitive (`faded_quads`,
  `faded_icons`, `faded_document_rasters`, `shape_chrome_labels_with_cjk`), and
  the swapchain is sRGB (`configure_window_surface`) with glyphon's
  `ColorMode::Accurate`, so blending happens in linear light. Mid-fade the plate
  overshoots (`settings::push_float_window`'s whole-frame hairline shows through
  the interior: `#3B3B3B` at half opacity against a final `#2A2A2A`) and the text
  reaches its contrast before the plate and the shadow do. At rest, every
  translucent ink over an unknown ground differs from the CSS mock: the `--border`
  hairline over `menu_surface` reads +14 ΔL* on dark, washes and the scrim's dim
  of text +13 to +17, light-theme hairlines and shadows fainter. No token retune
  fixes it, because the linear alpha that reproduces CSS depends on the ground.
  Owed, in two halves: **ticket 46 (M, 0.4.5)** — group opacity, each fading
  surface rendered at full strength offscreen and composited once onto a
  non-sRGB view of the swapchain, byte-identical at rest; **the L variant
  (0.4.6)** — the whole overlay pass in encoded space (glyphon `ColorMode::Web`),
  so the 0.5 restyle compares like with like. The row closes with the second.
