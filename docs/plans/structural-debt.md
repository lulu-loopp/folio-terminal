# Architecture debt ledger

**The rule (2026-09-23).** This is the one ledger of architecture debt. Every row
is assigned to a ticket and a version, and **the ledger reads zero at the end
of 0.4.7**: 0.4.5 is typing stability, per-pane text size and UI; 0.4.6 is the
updater and the first half of architecture closure; 0.4.7 is the second half. A
row that cannot be cleared by 0.4.7 says why and where it goes (`deferred →`
below). Two obligations follow:

- **Every ticket's *Architecture impact* section cites the rows it repays,
  moves or adds**, by ID. A ticket that repays a row sets its status to
  `repaid on <sha>` in the same commit; a ticket that finds new debt adds a row
  here rather than a sentence in its report.
- **A row leaves only by being repaid, or by a dated ruling that it is not
  debt.** Rows are never renumbered; a repaid row stays, with its sha.

The file began as the 2026-09-21 structure review's debt list (D-1…D-18, kept
below with their IDs and text). On 2026-09-23 it gained the ledger columns and
every debt the repository already recorded elsewhere (D-19…D-63); on
2026-09-24, two rows found by the 0.4.5 drafts (D-64, D-65) and one artefact
ticket 46 introduced (D-66), and one Mac defect ticket 55 found (D-67); on
2026-09-25, one wait ticket 62 found and repaid in the same commit (D-68), and
the seven exceptions 0.4.6 ticket A5's lane contract declared (D-70…D-76). It is
**ordered by consequence, not by severity** — nothing here is a bug; each row is
a shape that makes the next hundred tickets more expensive, and the cost it
charges is the one the project named: *what must be read to finish one ticket
must not grow with the number of features.*

**Ledger columns.** *Source* — where the debt was first recorded. *Ticket* — the
ticket that repays it (a number from the 0.4.4/0.4.5 ticket set, a preparation
step such as P14, or `none yet`). *Version* — 0.4.5 · 0.4.6 · 0.4.7 ·
`deferred → <where>` with the reason. *Status* — `open` · `in ticket` ·
`repaid on <sha>`; a note after a dash records a part already repaid inside an
open row.

**The versions (owner ruling, 2026-09-24).** The release plan adopts these
versions, superseding the proposals of 2026-09-23: 0.4.5 (typing stability,
per-pane text size, UI) takes D-33, D-37, D-38, D-41, D-45, D-46 and D-64, and
keeps D-61 and D-62 because they are small and the Mac CI job is in 0.4.6;
0.4.6 (the updater, and architecture closure's first half) takes the rest of
what the proposals put in 0.4.5 and keeps most of what they put in 0.4.6;
0.4.7 (architecture closure's second half) takes D-6, D-16, D-49, D-51,
D-54, D-55, D-56 and D-65, and the first slices the owner wants before 0.5 of
rows once deferred — D-1, D-9, D-12, D-15, D-17. Only D-43, D-44 and D-59 stay
deferred.

**After 0.4.7 (2026-10-08, plan J4).** The ledger did not read zero at the end
of 0.4.7: 70 rows are open. Each is re-homed against the 0.4.8 plan
(`00-PLAN-048`, groups A–K): *0.4.8* cites the plan ticket that takes it;
*0.5* holds the ownership and contract rows the 2026-09-21 review put before
0.5's new subsystems, and three of them are **0.5 preconditions** — D-1 (who
owns session, document and view) and the side-effect contract, whose admission
half is D-16 and whose execution and completion half is D-33; the two notes
are 0.4.8's J5, written before 0.5 builds on them. *Unassigned* rows fit no
0.4.8 plan ticket and wait for the coordinator, who cuts a ticket, rules the
row into 0.5, or rules it not debt. The *Version* column now takes 0.4.8,
0.5 and `unassigned — <why>` beside the earlier values; a re-homed row keeps
its former version in its status (*was 0.4.6*).

**Not on this ledger.** Defects (the adversarial-review ledgers, and the
incidental list below); UI constants
(`docs/design/UI-DEVIATIONS.md`, which tickets 16–31 take to zero on their own
track); rows 15–19 of `docs/ARCHITECTURE.md` §5.3, which are *ruled to stay*;
and the probes in `scripts/ci/ignored-tests.txt`, which are ignored by policy —
they answer "what does this machine do" — not by debt.

## By version

| version | rows | open | repaid |
|---|---:|---:|---:|
| 0.4.5 | 8 | 0 | 8 |
| 0.4.6 | 4 | 0 | 4 |
| 0.4.7 | 0 | 0 | 0 |
| 0.4.8 (plan ticket on the row) | 19 | 19 | 0 |
| 0.5 (reason on the row; D-1, D-16 and D-33 are its preconditions) | 13 | 13 | 0 |
| deferred (reason on the row) | 4 | 4 | 0 |
| unassigned (no 0.4.8 plan ticket; listed for the coordinator) | 34 | 34 | 0 |
| already repaid | 2 | 0 | 2 |
| **total** | **84** | **70** | **14** |

(2026-09-25, A5: D-33 moved from 0.4.5 to 0.4.6 — the owner deferred the
presentation lane, its second client, and revision (b) R8 of
`docs/plans/design/window-thread-budget-2026-09-25.md` aligns the row — and
D-70…D-76 were added to 0.4.6. The totals are recounted from the table below:
the line before this read 68 rows and 8 repaid, which counted neither D-69 nor
the already-repaid D-58.)

(2026-09-27: D-41 moved from 0.4.5 to deferred — its cell had kept the 0.4.5 assignment after the owner deferred the presentation lane on 2026-09-24 (§R-E); the 0.5 plan's A10/I7 quote the row. Bookkeeping, not a repayment: 0.4.5 is 9 rows / 1 open, deferred 4 / 4.)

(2026-09-26, A1a: D-77 added to 0.4.6.)

(2026-09-26, census-1: D-11 repaid.)

(2026-09-26, ticket 72: D-63 and D-67 repaid; D-83 added to 0.4.7, the version
proposed by the ticket for the coordinator to confirm.)

(2026-09-27, A4: D-84 added to 0.4.7. Recounted from the table below: the 0.4.7
line before this one read 15 rows, which did not count A1e's D-78…D-82.)

(2026-10-08, J4 — the reconciliation after 0.4.7: the table above is recounted
from the rows. The table before it read 0.4.5 9/1/8, 0.4.6 49/45/4, 0.4.7
21/21/0, deferred 4/4/0, already repaid 1/0/1, total 84/71/13. 0.4.7 shipped
with every row it held still open; no row was repaid in 0.4.6 after
2026-09-27 or in 0.4.7. One row is repaid by bookkeeping — D-13, repaid on
`21cf1ef8` two days before this ledger had columns and carried open since —
eleven were narrowed, four widened, and every open row now names 0.4.8 (a
ticket of the 0.4.8 plan), 0.5 (with its reason), deferred, or *unassigned*,
which means no 0.4.8 plan ticket takes it and the coordinator decides. D-64,
0.4.5's one open row, moved with the others. Of the 70 open rows: 11 narrowed,
59 untouched, of which 4 widened. The evidence, row by row, is the section
*The 2026-10-08 reconciliation* below.)

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
no part of D-64 or D-65 is repaid. By the 0.4.5 tickets: ticket 49 repaid D-46
whole (§5.3 row 14: the window's title has one writer to the OS, throttled to
one write a frame) and with it that row's part of D-2; it added no row. Ticket
48 repaid D-45 whole (§5.3 row 13: one reading of the window's place per turn,
taken at its head, plus one per attention delivery between turns, because no
turn runs inside Windows' modal move/size loop) and that row's part of D-2; it
added no row. Ticket 50 repaid D-37 whole (§5.3 row 5: the machine's font
collection is walked only on the font lane, by a numbered request, and the face
`settings.json` names is found by a lookup of that one family) and that row's
part of D-2; it added no row, because the lookup it leaves on the window thread
measured under one frame (2.6–3.4 ms cold on the development machine). The font
lane now numbers its requests and answers, the request identity D-33 asks of
every lane; it does so in its own slot, not through a shared shape, so D-33 is
not advanced beyond that.
added no row. Ticket 51 repaid D-38 whole (§5.3 row 6: a changed search reads
one slice of history on the keystroke's frame and one per turn after it, on the
window thread, no ownership moved) and that row's part of D-2; it added no row.
Ticket 55 repaid D-61 and D-62 whole (the two defects the Mac build carried:
a `webnav` test that asked a Windows question on every machine, and three
Windows-only constants compiled where nothing read them); it added no row.
Ticket 62 added D-68 and repaid it whole in the same commit (the taskbar's
auto-hide state is asked on `taskbar_lane`, a lane of its own, and the window
thread reads the latest answer from a numbered slot) and with it D-2's part for
the probe ticket 48 left inside §5.3 row 13.
Ticket 63 added D-69 and repaid it whole (§5.3 row 22: the input method's caret
area is one wanted value, told to the system by one road at most once a turn
and only when it moved, and at once on `Ime::Enabled`) and with it that row's
part of D-2.

Ticket 54 narrowed D-64 and repaid none of it: the environment is asked for once on
an idle turn after startup, which takes the creation call (8.5–39 ms) out of the
first page's gesture, but the environment starts no runtime process (a windowless
probe: no descendant process, an empty profile folder), so §5.3 row 21's residual —
the first controller and the pump dispatch after it — stays, and so does D-2's part
for that row. It added no row.

Ticket 60 narrowed D-64 again and repaid none of it (owner's ruling 2026-09-25,
option A): a profile that has opened a page gets one spare controller made on a
quiet idle turn and handed to its first eligible page, which then pays the rehost
walk and a navigate (146 ms median in spike 59, against 2318 ms cold). The row
stays open, narrowed, for 0.4.6: a profile's first-ever page, a page that arrives
before the spare has landed, and every page after the spare is used still pay the
per-page controller. It repaid a defect it found (a late controller dropped
without `Close()`; DESIGN 2026-09-25) and added no row.

0.4.6 ticket A5 repaid no row. It advanced D-33 to "contract and harness
landed, exceptions listed": `bt-app::lane` states the lane contract once and
`lane_contract_tests` holds the hand-off, font, taskbar and computation lanes to
it, and every claim a lane fails is declared with the row that repairs it — the
seven rows it added, D-70…D-76. It moved D-33 from 0.4.5 to 0.4.6 (the note's
R8).

0.4.6 ticket A1a repaid no row. It advanced D-2's inventory: the window thread's
waits are one registry, `crates/bt-app/src/window_waits.tsv`, from which
`docs/ARCHITECTURE.md` §5.3 is generated, and each owner-thread wait is a door type
of `bt_platform::admission::doors` held equal to it; no door takes its token until
A1d, so nothing is admitted yet. It added D-77 (§5.3 row 23, the first window's GPU
opened with a blocking wait, found by the thread-door note's revision (e)2). The
two version notes the budget note's R8 asks of the first ticket to touch the
ledger: D-33's was made by A5; D-42's is made here (independent of D-41).

0.4.6 ticket A1b repaid no row and added none. It advanced D-2: the thread door
lends every worker a `WorkerCtx`, and the one worker-only door that exists, the
hand-off, takes it (the seven verbs are private behind `ShellThread`). D-33's
version note: unchanged at 0.4.6 — the hand-off lane's executor is now made from
the capability the door lends (`HandoffLane::start`'s `make_executor` takes
`&WorkerCtx`), its contract adapter drives it the same way, and no row of
`lane::EXPECTED_FAILURES` moved.

0.4.6 ticket A1c repaid the thread door's half of D-16 and added no row. The
eighteen bare spawns of `bt-app` and `bt-platform` start through the door at the
band each had, so every thread those crates start is a `Worker` by its name, and
the three door processes' main threads wait as workers through
`enter_standalone_main`. D-16 stays open for the enumeration lane and the
observation threads' band; no band changed (RULES 53's 0.4.7 ticket). It advanced
D-2: every thread that runs Folio's code outside `bt-pty` and the resample pool
now has a role.

0.4.6 ticket A1d repaid no row and added none. It advanced D-2: every owner-thread
door of the registry takes its token, so each of the window thread's listed waits
happens only inside an admission — on the window thread, in its phases, measured —
and nowhere else; no wait moved. D-33's version note: unchanged at 0.4.6 — no lane
changed, and the lane contract's rows did not move. D-42's version note: unchanged
at 0.4.6 — row 10 (device recovery's rebuild) is not converted, as the note's
revision (c)3 defers it to B9. D-43 and D-44: rows 11 and 12 are admitted where they
stand, through `bt-app::pty_door` (`spawn_shell` minted in `create_leaf_session`,
`resize` minted in `commit_leaf_resize`) — their location is recorded, they did not
move. D-77: row 23 is admitted through `bt-app::gpu_door::open_first_window`, where it stands.

0.4.6 ticket A1e repaid no row. It added D-78…D-82: the `Drop` exception rows of the
thread-door note's (e)3 that had no ledger row (the trace writer's, the endpoints',
the video engines' and seats', the shell's), and one its check found (WinHTTP's
`http::Request`, the note's revision (g)2); `DirWatch`'s two rows are D-40's. Each is
a row of the closed `Drop` inventory the source guard holds, and each is owed a
repayment by a named 0.4.7 ticket. It advanced D-2: the escapes the compiler cannot
see around the doors — `unsafe`, a second constructor, a writer called where no
transition was ruled, a lowered lint, a macro or FFI construct outside its owner, a
door that returns its effect, a `Drop` that waits — are fenced by one source guard.

0.4.6 ticket A4 repaid no row. It advanced D-2: each turn fixes one
`TurnAllowance` from the earliest next frame of the windows on the glass whose
clocks are running, and the search walk's slice and the idle calls (the web
engine's warm-up ask, the spare controller's making and its drain) ask it before
each unit and yield when it is spent (the budget note's §R-B, Codex's Q2). It
added D-84, the part of aggregate scheduling the allowance does not reach (§R-G).

### The 2026-10-08 reconciliation (0.4.8 plan J4)

Every row was read against what 0.4.6 and 0.4.7 landed after this ledger's
last pass (2026-09-27, `34e16355`, to `v0.4.7-preview` = `e87f556c` and main
`6a3f0eb6`): the commits, the dated `docs/DESIGN.md` entries, `docs/RULES.md`,
`docs/ARCHITECTURE.md`, the committed inventories (`window_waits.tsv`,
`window-thread-bare-sites.tsv`, `MIGRATION-DEBT.tsv`, `lane::EXPECTED_FAILURES`)
and the manifests. No commit of that range names a ledger row by its ID — the
`D-n` in the U-42 and U-SMALL-047 messages number rehearsal findings, not these
rows — so each finding below is a fact of the tree, with the commit that made
it. *Repaid* needs a commit and a stated rule; where the code is repaired and
the rule is stated nowhere, the row is *narrowed* with that doubt on it.

**Repaid.**
- D-13 — `21cf1ef8` (2026-09-21): the width probe moved to `bt-corpus`, and
  `bt-pty` names `bt-term` only as a dev-dependency; the rule is ARCHITECTURE
  §3.2's *Done 2026-09-21* and the DESIGN entry of that day, *a PTY transport
  … must owe terminal policy nothing*.

**Narrowed** (the row's own words for what remains are in its status cell).
- D-2 — T-ENV-REFRESH round 4 (row 11's birth on `bt-pty-birth`);
  T-INTEGRATION-INJECT-1's second follow-up `c7a604ab` (an unlisted join of the
  script worker removed, the scanner taught its shape); bare sites 248 → 232.
  A2 has not landed.
- D-3 — T-PROBE-CHILD and T-PROBE-BORN-IN-JOB (`spawn_probe`, ARCHITECTURE
  §2.2: *a machine probe is not in that set: no descendant may outlive its
  probe owner*); T-ENV-REFRESH round 4 (the PSReadLine probe's five-second
  deadline, asked again after failure).
- D-5 — `5bdc1a5a`: RULES row 36, the update check, folded.
- D-20 — T-GATES-047 `7c2a4a36` (as recorded on 2026-09-30).
- D-23, D-24 — rows retired with the readers they named (`c7a604ab`,
  `addfbaed`, `e5d56789`); none migrated.
- D-34 — T-INTEGRATION-INJECT-1 and -4: the `$PROFILE` install runs on
  `powershell-profile-install`.
- D-43 — T-ENV-REFRESH round 4: ARCHITECTURE §5, *A shell birth has one
  short-lived worker*.
- D-52 — T-INTEGRATION-INJECT-1: fact 19's enable writer retired.
- D-60 — `64bcebdb` (2026-09-13): the tests answer from a loopback listener;
  no sentence states the rule.
- D-83 — T-GUARDS-BLIND `5f7ba1fe` and `1d914d1a`: core-macos runs `bt-app`'s
  updater and uninstall modules.

**Widened** (open, and larger than the row says).
- D-14 — `ae514613`: `bt-term` imports `bt_platform::host_names`.
- D-32 — 203 methods in `main.rs`'s two `impl Runtime<'_>` blocks, 117 of them
  unassigned.
- D-33 — three answer roads of 0.4.7 outside the lane contract.
- D-82 — the WinHTTP request now has a product caller, on the update job's
  worker.

**Touched, not narrowed.** D-39 (contained, still no deadline); D-54
(B-EXPLORER-CLAIM rules a new fact of the class, none of its four).

**Untouched.** Every other open row. Its status cell says so and gives the
version it had (*was …*).

## The ledger

"§" alone means a section of `docs/ARCHITECTURE.md`. "Split prep" is
`docs/plans/bt-app-split-prep.md`. "Survey Part 4" is the 2026-09-21 process and
thread survey's list of twenty-two facts with more than one owner, which
§4.2 sorts into five classes; the fact-to-class assignment in D-51…D-55 is this
ledger's.

| ID | what | source | ticket | version | status |
|---|---|---|---|---|---|
| D-1 | session state has no owner independent of the window | structure review C-1 · K-1 | none yet | 0.5 — **0.5 precondition** (the 2026-09-21 structure review): the session/document/view ownership note is 0.4.8 J5 (T-05-CONTRACTS, design only); the first slice and the session registry are 0.5; the backend stays 0.6 | open; **2026-10-08 (J4):** untouched; 0.4.7 added session facts on the session's own thread (T-PANE-IDENTITY's `current_frame`, `screen_fence_state`; T-PANE-COLUMNS' `foreground_program`), no owner beside the window; was 0.4.7 |
| D-2 | the window thread's blocking set is a list, not a budget | C-2 · K-6 | through D-33…D-47 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — A2 (the bare-site lint), the step its closure waits for, has no ticket; G4 takes the waits #29 and #31 | open — §5.3 row 1 repaid on `2657e5e3`; rows 13 and 14 repaid by tickets 48 and 49 (D-45, D-46); row 5 repaid by ticket 50 (D-37); row 6 repaid by ticket 51 (D-38); the taskbar probe left inside row 13 repaid by ticket 62 (D-68); row 22 repaid by ticket 63 (D-69); the list is one registry with a generated §5.3, each owner-thread wait a door type held to it (A1a, 2026-09-26: advanced, not repaid); the thread door lends every worker a `WorkerCtx` and the hand-off door takes it (A1b, 2026-09-26: advanced, not repaid); every owner-thread door takes its token and each listed wait happens only admitted (A1d, 2026-09-26: advanced, not repaid); the escapes the compiler cannot see are fenced by one source guard, and every `Drop` that may wait is a row of a closed inventory (A1e, 2026-09-27: advanced, not repaid); every raw effect outside a door is a row of `docs/plans/window-thread-bare-sites.tsv`, which only shrinks — 248 sites, seeded at 263 on `2cc59a83` — and the configuration the lint will need is fenced and its probe proven per target (A2a, 2026-09-26: advanced, not repaid); every turn is accounted and every admitted call measured per call, with a budget line for each of the four triggers and an exit summary from the run's atomics (A3, 2026-09-27: advanced, not repaid — A1 and A3 have landed, and by the owner's ruling of 2026-09-25 D-2 closes when A2 lands too); deferrable work yields to the earliest window's deadline — the search walk's slice and the idle calls ask one `TurnAllowance` a turn (A4, 2026-09-27: advanced, not repaid — A1, A3 and A4 have landed; what the allowance leaves of aggregate scheduling is D-84); **2026-10-08 (J4):** **narrowed**: row 11's process birth runs on the `bt-pty-birth` worker and the window thread joins it (T-ENV-REFRESH round 4); an unlisted window-thread join of the PowerShell script worker was found and removed, and the scanner taught its shape (`c7a604ab`); row 29 added, ruled to stay (the update's exit guard, 2026-09-28); bare sites 248 → 232; A2 has not landed; was 0.4.6 |
| D-3 | ten one-shot probes with no common contract | K-9 · C-2 | none yet | 0.4.8 — B4 (T-PROBE-NO-CACHED-FAILURE) and B3 (T-FRESH-FACTS: the probes as facts re-asked); G1 for the two named containment gaps | open — the update job's worker `bt-update-job` added to the list (U-20, 2026-09-27; its two drivers U-27 and U-20); **2026-10-08 (J4):** **narrowed**: every machine probe starts through one door, `bt_platform::spawn_probe` / `probe_output`, which contains its whole process tree and ends it on deadline, wait, drop and unwind (T-PROBE-CHILD, T-PROBE-BORN-IN-JOB; ARCHITECTURE §2.2); the three PowerShell probes share one five-second deadline and a failed answer is asked again at the next reader edge (T-ENV-REFRESH round 4, T-INTEGRATION-INJECT-3 round 2). Still open: `copilot --version` and the macOS locale probe have no deadline, and each probe keeps its own slot, wake and latch (`profile_runtime::REMOVAL` has no in-flight latch); was 0.4.6 |
| D-4 | controlled failure loses dirty preview edits | C-3 · K-8 | G7-SWEEP-048 | 0.4.8 — G7 (census #28: an internal stop loses unsaved preview edits, this row's controlled-failure road); the emergency half is D-56 (0.5) | **narrowed to the emergency half (D-56)** — **2026-10-09 (G7):** the controlled road is repaid: `FolioApp::fail`'s twelve sites and `exiting` leave by one road, `stop_every_window`, which keeps every dirty preview buffer first (the quit's judged write, else a copy in the data directory's `recovered` folder, each said in `diagnostics.log`) and is held to its tables by `restore_app_tests::failure_road`; the panic hook still loses them — D-56; was 0.4.6 |
| D-5 | the rules existed only as history — 35 `docs/RULES.md` rows not yet folded | K-2 · C-4 | the ticket that depends on each row | 0.4.8 — rolling: each 0.4.8 ticket folds the `docs/RULES.md` row it depends on in its own commit; what is unfolded at 0.4.8's end goes to 0.5 with its subsystem | open — 19 folded; row 28's wheel half folded by ticket 37 (the press half is not); row 25's font-list half folded by ticket 50 (the glyph atlas half is not); **2026-10-08 (J4):** **narrowed**: row 36 (the update check) folded on `5bdc1a5a` (0.4.6); RULES has 55 rows, 24 folded in whole or part, 31 `not yet folded`; was 0.4.6 |
| D-6 | cross-crate chains are visible nowhere | K-10 · C-4 | through D-48…D-50 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — printed path written; its hand-off hop updated on `2657e5e3` and `5d4c7aff`; **2026-10-08 (J4):** untouched: §7.2's chains are still first-pass lines (T-IMAGE-N added a hop to §7.1's recognition and verdict rows); was 0.4.7 |
| D-7 | source-reading guards are the architecture document; their rules owe prose | K-14 · C-4 | with D-28 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — it closes with D-28 | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-8 | the asking/telling family has no taxonomy | K-3 · C-4 | census-5a (note) and census-5b, after the ruling | 0.5 — the owner rules the table before 0.5's notification model | open — 28 surfaces inventoried and seven kinds proposed for the ruling in `docs/plans/design/ownership-census-2026-09-25.md` §3–§4 (2026-09-25); **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-9 | configuration entrances: the fourth row | K-4 · C-4 | none yet | 0.5 — the fourth entrance is 0.5's outward interface | open — §9 table written; export ruled not an entrance on `5f433943`; **2026-10-08 (J4):** untouched; 0.4.8 F8 (T-LAUNCH-ENV) adds an explicit launch input and must say whether it is a row of §9's table; was 0.4.7 |
| D-10 | diagnostics have plumbing but no event model | K-5 · C-4 | none yet | 0.5 — the operation vocabulary, with the 0.5 self-report | open; **2026-10-08 (J4):** untouched: 0.4.6 and 0.4.7 added `diagnostics.log` lines (U-39, U-42a…e) each in its own format; was 0.4.6 |
| D-11 | the split fixes file size, not coupling — the ownership census | K-7 · C-4 | census-1 | 0.4.6, with D-32 | repaid (census-1) — the census is `bt_source::FieldCensus` over `bt-app`; inventory and site rows are query reports under `target/`, while the owner annotations (census-1's proposals, until the owner rules) and the shrink-only unknown row multiset are committed and held by `bt-source`'s `census` test and `scripts/ci/check-census-unknowns.ps1`, so a fact with an unknown is `incomplete`, never single-writer. D-32 (census-7) is what the census was taken for, and stays open |
| D-12 | `bt-platform` is a drawer | K-11 | none yet | 0.5 — `bt-platform` is not split in 0.4.8 (plan K, *Not in 0.4.8*); J2's small boundary crate for D-14 is the first extraction | open; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-13 | the `bt-pty → bt-term` edge | K-12 · C-4 · split prep P21 | P21 | — | repaid on `21cf1ef8` (2026-09-21) — the ConPTY width probe lives in `bt-corpus` and `bt-pty`'s manifest names `bt-term` only under `[dev-dependencies]` (ARCHITECTURE §3.2, *Done 2026-09-21*); carried open by mistake from 2026-09-23 to 2026-10-08 (J4). Not this row: `bt-pty`'s optional edge to `bt-platform` behind its test-only `test-shell` feature and `bt-term`'s dev-dependency on `bt-pty` (T-TEST-SHELL-HYGIENE, T-INTEGRATION-INJECT-4 round 6), for J1 to classify |
| D-14 | `bt-term → bt-platform` is broader than its manifest | C-4 · K-11 | CC-3 (`bt-effects`), CC-4 | 0.4.8 — J2 (T-WRONG-EDGES), after J1; the boundary crate is also I1's prerequisite | open; **2026-10-08 (J4):** **widened**: a fourth product import surface, `bt_platform::host_names` (`inline_image`'s local-host set, `ae514613`, 0.4.6); was 0.4.6; **2026-10-08 (CC-3):** **narrowed**: the read ledger is `bt-effects`', and three surfaces remain for CC-4 (thread priority, `resolved_for_a_door`, `host_names`); **repaid on `744cc02a` (CC-4; the product edge went in `0479c1ca`, the last, dev, edge in `744cc02a`)**: `bt-term`'s manifest names `bt-platform` nowhere (not as a dev-dependency either); the host names and the resample pool's thread-start hook are installed by the host (`bt_term::install_host_names`, `install_pool_thread_start`; `bt-app`'s `host_answers::install`), and `verify_path` takes the door-ready resolver as a parameter |
| D-15 | `bt-term → bt-math` is real coupling | C-4 · K-11 | recorded by D-27; CC-5, CC-6b, CC-7 (design T-COMPOSE-CRATE §3.4) | 0.4.8 — J2 (T-WRONG-EDGES): the design decides; J1's allow-list records it until then | open (recorded debt); **2026-10-08 (J4):** untouched; was 0.4.7; **2026-10-08 (CC-5):** **narrowed**: the four math data types are `bt_doc::math`'s (re-exported by `bt-math`); what remains is execution — `MathEngine`/`key_for_em_px` in `session.rs` (CC-6b), `rasterize_svg_document` in `inline_image` and the `MathEngine` re-export (CC-7); **2026-10-08 (CC-6a):** untouched; the receiving crate exists — `bt-compose`, layer 6, which `typeset` moves into with CC-6b |
| D-16 | the door pattern: the enumeration lane and the thumbnail thread's band | K-13 | A1c (the thread door's bypass) | 0.5 — **0.5 precondition**: the side-effect contract's admission half; its note is 0.4.8 J5 (T-05-CONTRACTS); the enumeration lane and the observation threads' band are built in 0.5 | open — rule stated; the thread door's bypass (`folio-web-thumb` and five unnamed spawns) repaid by A1c in 0.4.6; the enumeration lane and the observation threads' band (`folio-web-thumb` among them, RULES 53's 0.4.7 ticket) remain; **2026-10-08 (J4):** untouched: every thread 0.4.7 added comes through the door (T-PROBE-CHILD's reader, T-KEYBOARD-CTRLALT's `folio-layout-tables`, T-UNINSTALL-UX's remover pipe); `folio-web-thumb` still stands at `Normal`; RULES 53's 0.4.7 ticket was never cut; was 0.4.7 |
| D-17 | preview selections have no revisioned mapping to the document | C-4 | none yet | 0.5 — with D-1's document owner, after J5's note | open; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-18 | the census reads a query's argument as a file-bound subject | split prep, 2026-09-22 | census-2 (the census note's revision (b)) | 0.4.6 — D-29…D-32 need a true census | repaid (census-2, 0.4.6) — each subject is item-bound or file-bound by how the test reads it (`ITEM_QUERIES` and the helpers derived from it); only a file-bound subject read out of `main.rs` is a reader 2a must retarget; `--self-check` holds the fixture |
| D-19 | MIGRATION-DEBT class P0 — the documentation generators (3 rows) | `docs/plans/MIGRATION-DEBT.tsv`; split prep §6 | P0 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-20 | MIGRATION-DEBT class P10 — the platform-file walk (1 row) | same | P10 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — narrowed by T-GATES-047: the script twin and agreement pin are gone; the one Rust directory walk remains on MIGRATION-DEBT; **2026-10-08 (J4):** no further change; was 0.4.6 |
| D-21 | MIGRATION-DEBT class P12 — `bt-platform`'s walkers and the `stand_in` guard (5 rows) | same | P12 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-22 | MIGRATION-DEBT class P13 — the remaining source-text walks (3 rows) | same | P13 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-23 | MIGRATION-DEBT class P14 — named-body pins (192 rows) | same | P14 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** **narrowed**: 192 → 190 rows, two readers retired with the code they read (T-INTEGRATION-INJECT-1, `c7a604ab`), none migrated; was 0.4.6 |
| D-24 | MIGRATION-DEBT class P16 — ledger keys naming a file (63 rows), and the ten text `#[cfg(test)]` splits | same | P16 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** **narrowed**: 63 → 57 rows, six keys retired with their readers (B-EXPLORER-CLAIM `e5d56789`; T-INTEGRATION-INJECT-1 `addfbaed`), none migrated; was 0.4.6 |
| D-25 | MIGRATION-DEBT class P17 — cross-crate and script readers (13 rows) | same | P17 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-26 | the ~110 `[..].concat()` needle halves written whole | split prep §6 | P18 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-27 | the CI dependency-direction guard, and `bt-term → bt-math` recorded | split prep §8.4–§8.5 | P19 | 0.4.8 — J1 (T-DEP-DIRECTION-GUARD) | open — `bt-workbench`'s entry is already written for it in `docs/ARCHITECTURE.md` §3.3 (census-3, 2026-09-25); **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-28 | MIGRATION-DEBT to zero and deleted; the allowlist final | split prep §7.2 | P20 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-29 | the unmoved topic `launch` (4 methods) | split prep Appendix C | none yet | 0.4.8 — K2 (T-SPLIT-2B), after its reader fix; K2's plan line names free functions and types, so the coordinator confirms it takes the unmoved topics | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-30 | the unmoved topic `settings` (31 methods) | split prep Appendix C | none yet | 0.4.8 — K2 (T-SPLIT-2B), after its reader fix; as D-29 | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-31 | the unmoved topic `focus` (51 methods) | split prep Appendix C | none yet | 0.4.8 — K2 (T-SPLIT-2B), after its reader fix; as D-29 | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-32 | the unassigned `Runtime` methods still in `main.rs` (112 at the move, 115 today) | split prep §7.1, Appendix C | census-7 (a frozen list, after census-1 and census-2) | 0.4.8 — J3 (T-RUNTIME-DEP-MAP) drafts the destinations, K2 moves them | open; **2026-10-08 (J4):** **widened**: `main.rs`'s two `impl Runtime<'_>` blocks hold 203 methods (201 at the 2026-09-23 count): 117 unassigned plus D-29…D-31's 86; was 0.4.6 |
| D-33 | the lane contract as one shape, wrapping the existing lanes | §5.4 step 1, §5.1 | A5 (contract and harness); the exception rows D-70…D-76 | 0.5 — **0.5 precondition**: the side-effect contract's execution and completion half; its note is 0.4.8 J5 (T-05-CONTRACTS); the row closes when D-70…D-76 close | open — first instance, `handoff_lane`, on `2657e5e3`; the font lane (`settings::MonospaceFamilySlot`) numbers its requests since ticket 50; **contract and harness landed, exceptions listed** (A5, 2026-09-25): `bt-app::lane` declares the hand-off, font, taskbar and computation lanes' policies, `lane_contract_tests` runs eight claims on each through its real admission, publication and acceptance, and each failure is a row of `lane::EXPECTED_FAILURES` naming D-70…D-76. Not yet adapted: path verification (on `MathWorker`'s shared answer sender), the ten probes (D-3), and the files, preview, index and git workers; **2026-10-08 (J4):** **widened**: three answer roads 0.4.7 added stand outside `bt-app::lane` — `folio-layout-tables` (T-KEYBOARD-CTRLALT, which states its own two terminal answers and a bound of eight), the foreground-program worker (T-PANE-COLUMNS E8) and the PowerShell parse questions on `powershell-script-prepare` (T-INTEGRATION-INJECT-3); `lane::EXPECTED_FAILURES` unchanged; was 0.4.6 |
| D-34 | §5.3 row 2 — the marks lock's install half on the window thread | §5.3 | none yet | 0.4.8 — G6 (T-INTEGRATION-INJECT-2: Folio's own PSReadLine per pane retires the module install that takes this lock); the agent-hook installs (`attention_ownership::record`) have no plan ticket and are listed for the coordinator | open — the wait behind our own writer repaid on `fbfab1ff`; ticket 56 put the PSReadLine upgrade on the same thread at launch (`psreadline::upgrade_recorded` from `Runtime::create`, before the first window and before the profile migration's worker): it takes the lock only on a launch that replaces Folio's own older module, and the same move to a worker repays it; **2026-10-08 (J4):** **narrowed**: the `$PROFILE` install half runs on a worker, `powershell-profile-install` (T-INTEGRATION-INJECT-1 and -4; DESIGN 2026-10-04, 2026-10-05); the PSReadLine install and upgrade (`psreadline::install_recorded` from `apply_psreadline`, the launch's `upgrade_recorded`, `App::release_trial_writes`) and the agent-hook installs still take the marks lock on the window thread; `window_waits.tsv` row 2 still names the retired `spend_powershell_intent`; was 0.4.6 |
| D-35 | §5.3 row 3 — `psreadline::apply_recorded`, nine files under the lock | §5.3 | none yet | 0.4.8 — G6 (T-INTEGRATION-INJECT-2) retires the installed module this row writes | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-36 | §5.3 row 4 — `psreadline::installed_copy`'s recursive walk | §5.3 | none yet | 0.4.8 — G6 (T-INTEGRATION-INJECT-2) retires the installed module this row walks | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-37 | §5.3 row 5 — the machine's whole font collection enumerated inline | §5.3 | 50 | 0.4.5 — the traced frozen gear | repaid (ticket 50) |
| D-38 | §5.3 row 6 — the find box re-scans every frozen line per keystroke | §5.3 | 51 | 0.4.5 — a per-keystroke cost | repaid (ticket 51) |
| D-39 | §5.3 row 7 — macOS locale children on the pane-birth road | §5.3 | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** touched, not narrowed: both commands now lead a contained process group (`probe_output`, T-PROBE-CHILD) but still have no deadline and stay on the pane-birth road; was 0.4.6 |
| D-40 | §5.3 row 8 — macOS `DirWatch` start and drop wait without a bound | §5.3 | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — B7 was never cut | open — both platforms' `DirWatch::drop` are rows of the closed `Drop` inventory the source guard holds (A1e, 2026-09-27); B7 repays them; **2026-10-08 (J4):** untouched; 0.4.8 B1 (T-WINDOWS-ALL) gives every window the git watch and the document re-reads, which multiplies this row's start and drop sites unless B1 repays them; was 0.4.6 |
| D-41 | §5.3 row 9 — presentation on the window thread; the present mode has no owner | §5.3; §5.4 step 4 | none yet | deferred → unassigned — construction awaits the owner's measurement-based decision after the self-inflicted waits are fixed and measured (owner, 2026-09-24; `docs/plans/design/window-thread-budget-2026-09-25.md` §R-E, replacing §4); a release is assigned only after that ruling, and this deferral does not repay D-41 | open — since ticket 37 a presented picture is a pair (frame and metrics: `SeatSignature::metrics`, `LeafSession::presented_metrics`), and the lane must carry both; **2026-10-08 (J4):** untouched |
| D-42 | §5.3 row 10 — device recovery blocks and sleeps on the window thread | §5.3; §5.4 step 4 | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — B9 was never cut | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-43 | §5.3 row 11 — PTY birth on the window thread | §5.3; §5.4 step 5 | none yet | deferred → 0.5 toward 0.6 — needs D-1's session owner to keep input and resize order | open — admitted where it stands through `pty_door::spawn_shell`, minted in `create_leaf_session` (A1d, 2026-09-26: its door, not its move); **2026-10-08 (J4):** **narrowed**: process birth (`CreatePseudoConsole`, `CreateProcessW`, the folder `stat`, the fresh account environment) runs on the `bt-pty-birth` worker and the window thread joins it through `PtyBirth` (T-ENV-REFRESH round 4; ARCHITECTURE §5, *A shell birth has one short-lived worker*); the join stays until the session owner (D-1) |
| D-44 | §5.3 row 12 — the synchronous `ResizePseudoConsole` round trip | §5.3; §5.4 step 5 | none yet | deferred → 0.5 toward 0.6 — as D-43 | open — admitted where it stands through `pty_door::resize`, minted in `commit_leaf_resize`, one admission per leaf (A1d, 2026-09-26: its door, not its move); **2026-10-08 (J4):** untouched |
| D-45 | §5.3 row 13 — `sample_window_place` resampled at three sites for one instant | §5.3 | 48 | 0.4.5 — one site is `drain_pty` | repaid (ticket 48) |
| D-46 | §5.3 row 14 — `Window::set_title` at five sites with no throttle | §5.3 | 49 | 0.4.5 — one site is `drain_pty` | repaid (ticket 49) |
| D-47 | §5.3 row 20 — renames, the preserving save and store writes on the window thread | §5.3 | none yet | 0.4.8 — B6 (T-SETTINGS-REREAD): its storage transaction (revision preconditions, field-level merge, durable owed edits) is this row's storage lane for settings, keybindings, profiles and pins; renames and the preview save have no plan ticket and are listed for the coordinator | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-48 | §7.2 chain stub — attention ingress | §7.2 | census-4 | 0.5 — written with D-57's census-4, when the attention core moves | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-49 | §7.2 chain stub — resize | §7.2 | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-50 | §7.2 chain stub — paste convergence | §7.2 | none yet | 0.4.8 — A1 (the Linux series, #19…#27): PR #17's survey makes Linux's lane-owned paste a second paste architecture, this row's decision | open; **2026-10-08 (J4):** untouched on main; was 0.4.6 |
| D-51 | §4.2 class — observations of external state (survey facts 1, 5, 6, 7, 12) | §4.2; survey Part 4 | none yet | 0.4.8 — B (freshness): B2 and B3 re-ask facts 5, 6 and 12 on the environment broadcast, B6 re-reads fact 7; fact 1 (the verdict ledger) has no plan ticket | open; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-52 | §4.2 class — asynchronous publication and competing operations (facts 4, 10, 13, 19, 22) | §4.2; survey Part 4 | none yet | 0.4.8 — G7 (#30, the Enable/Undo race in two windows, a competing operation of this class); facts 4, 10, 13 and 22 have no plan ticket and are listed for the coordinator | open — fact 10 touched by U-25, not widened (the start's renewal rides the existing probe and latch); **2026-10-08 (J4):** **narrowed**: fact 19's enable writer retired with the `$PROFILE` enable road (T-INTEGRATION-INJECT-1), so `REMOVAL` has one writer kind; two presses still start two threads with no in-flight latch; was 0.4.6 |
| D-53 | §4.2 class — durability and external transactions (facts 8, 9, 11) | §4.2; survey Part 4 | U-6 | 0.4.8 — B6 (T-SETTINGS-REREAD)'s storage transaction for the store files; facts 8 (the session snapshot) and 9 (the marks record) go with it or to J5's contract, the coordinator decides | open — **narrowed by U-6 (0.4.6)**: fact 11's part repaid — the update check's memory, file and claim have one owner, `update::OfferState`, whose one lock is held across every read-modify-write; facts 8 and 9 remain; **2026-10-08 (J4):** untouched: the update journal and the transaction adapter (U-13…U-42, U-41a1) are new facts of this class with one owner each from birth (§4.2); facts 8 and 9 unchanged; was 0.4.6 |
| D-54 | §4.2 class — identity, admission and lifecycle (facts 2, 3, 14, 20) | §4.2; survey Part 4 | none yet | 0.5 — with D-1's session registry, which issues the epochs this class needs | open; **2026-10-08 (J4):** touched, not narrowed: B-EXPLORER-CLAIM (`e5d56789`, 0.4.6) ruled who may take the data directory's claim, a new fact of this class (§4.2); facts 2, 3, 14 and 20 unchanged; was 0.4.7 |
| D-55 | §4.2 class — projections, delivery and loss (facts 15, 16, 17, 18, 21) | §4.2; survey Part 4 | none yet | 0.5 — each publication declares its kind, under J5's side-effect contract | open; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-56 | §11 emergency termination — a journal and a defined recoverable revision | §11 | none yet | 0.5 — the journal, with D-4's policy and J5's contract | open; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-57 | §12.1 — `bt-workbench` is born | §12.1 | census-3 and census-4 | 0.5 — census-4 opens the 0.5 workbench | open — day-one contents, order, public surface and guard entry in `docs/plans/design/ownership-census-2026-09-25.md` §5 (2026-09-25); **narrowed by census-3** (2026-09-25, branch `feat/bt-workbench-born`): the crate exists holding the ledger, `attention::expiry` (`WAIT_TTL`, `WaitClock`), `is_consumed` and `Places` (private counter, compile-fail doctest), `bt-app` its only dependent, `bt-layout` its only dependency; what remains is census-4 — the reach rule and its `notify` tests move, and D-48; **2026-10-08 (J4):** untouched: `bt-workbench` changed only by U-44's dead-code sweep; was 0.4.6 |
| D-58 | `profile_runtime`'s two tests failing `WouldBlock` on a slow CI disk | `docs/DESIGN.md`, 2026-09-23 | 34 | — | repaid on `fbfab1ff` |
| D-59 | `bt-render`'s two atlas soaks, ignored under protest | `scripts/ci/ignored-tests.txt` | none yet | deferred → the version that gains a CI runner with a real graphics adapter; whether to provision one is decided in 0.4.6 | open — ticket 37 added a CI-runnable mixed-size stress (`mixed_size_seats_share_the_atlas_and_get_their_text_back`, WARP, texture ceiling 512) and no ignore; the soaks were not extended; **2026-10-08 (J4):** untouched |
| D-60 | two macOS `http` tests that reach the network | `docs/plans/port/m4-7/transcript.md` | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — the code is repaired; the row waits for its rule sentence | open; **2026-10-08 (J4):** **narrowed**: both tests answer from a loopback listener (`answering`, `ask_this_machine`) since `64bcebdb` (2026-09-13, before this row was written); the rule that a transport test never reaches the network is stated in no DESIGN, RULES or ARCHITECTURE sentence, so the row is not marked repaid; was 0.4.6 |
| D-61 | a Mac-only red test in `webnav` | ticket 13's report | 55 | 0.4.5 — small; the Mac CI job (D-63) is in 0.4.6 | repaid (ticket 55) |
| D-62 | `bt-render` fails clippy on macOS: three unused constants | ticket 13's report | 55 | 0.4.5 — small; the Mac CI job (D-63) is in 0.4.6 | repaid (ticket 55) |
| D-63 | the macOS CI job tests none of `bt-app`, `bt-term`, `bt-render` and lints only `bt-platform` | `.github/workflows/ci.yml`, `core-macos` | 72 | 0.4.6 | repaid (ticket 72) — the job lints the workspace and tests `bt-render` and `bt-corpus`; the `bt-app` and `bt-term` suites are not portable yet and are D-78 |
| D-64 | opening a web page holds the window thread for seconds: WebView2 environment and controller creation and `drive_web_page`'s install burst, unprobed inside `window_event` | the 2026-09-23 investigation of hover cards, float drag and web-open stutter, §3; ticket 43 | 43, 54 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — narrowed by ticket 43: the phases are named in the stall self-report; the remaining cost is the engine's own thread-affine work (§5.3 row 21) ruled 2026-09-24: warm the engine at a quiet moment — follow-up warm-up, ticket 54; narrowed by ticket 54: the environment call is taken at an idle turn, but the environment starts no runtime process (measured), so the first page's `request_controller` and pump dispatch remain; ruling owed; ruled 2026-09-25 (option A) and **narrowed by ticket 60**: a profile that has opened a page gets a spare controller made at idle for its first eligible page (2318 → 146 ms median, spike 59); **open for 0.4.6** — the residual is a profile's first-ever page, a page that arrives before the spare has landed, and every page after the spare is used; **2026-10-08 (J4):** untouched; was 0.4.5 |
| D-65 | overlay fades are folded per primitive and blended in linear light: a fading surface shows its text before its plate, and translucent inks differ from the CSS mock | the 2026-09-23 fade audit, §0–§2 and §7; ticket 46 | 46; the L variant none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — the L variant precedes the 0.5 restyle | open; **2026-10-08 (J4):** untouched; the 0.5 design tokens are written (`docs/plans/design/ui-05-tokens-2026-09-27.md`); was 0.4.7 |
| D-66 | a fading surface's translucent pixels step at the landing frame: composited on encoded bytes while it fades, blended in linear light at rest — on the light theme the tip's shadow lightens at its darkest pixel from about `#DB` to `#EE` as the fade lands | ticket 46's report (Findings); the 2026-09-23 fade audit, §7 | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — with D-65 | open; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-67 | `bt-app` fails clippy on macOS: nine app-build and five test-build unused items and ignored results | ticket 55's report | 72 | 0.4.6 — beside D-63, since widening the Mac CI job hits it | repaid (ticket 72) |
| D-68 | the taskbar's auto-hide state asked of Explorer on the window thread: `SHAppBarMessage(ABM_GETSTATE)` inside `sample_window_place`, at every turn's head and again for a delivery between turns | the owner's next93 stall report, 2026-09-25; ticket 62 | 62 | 0.4.5 — a wait on another process inside the typing turn | repaid (ticket 62) |
| D-69 | §5.3 row 22 — `Window::set_ime_cursor_area` called at every offer of a caret: twice in one turn (15 + 85 ms) and once for 3,138 ms under load on the owner's next93 | the owner's next93 stall reports, 2026-09-25; ticket 63 | 63 | 0.4.5 — typing stability | repaid (ticket 63) |
| D-70 | the hand-off lane: a worker that dies leaves the requests it had accepted with no terminal outcome — `answers()` reads a disconnected channel as empty, `LANE_GONE` answers only a later submission, and the ids stay owed in each window's `Pending` | A5 (`lane::EXPECTED_FAILURES`: Handoff × a dead worker is observable) | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-71 | the hand-off lane: the answers held for an undrained consumer are unbounded — the answer channel is an unbounded `mpsc::channel` and `turned_away` a `Vec` | A5 (Handoff × answers held are bounded) | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-72 | the font lane: a coalesced or superseded request gets no outcome of its own, and a walk that dies leaves `ScanState::running` set for ever — no fault, no later walk; the wake has no failure road | A5 (Font × every request ends exactly once; a dead worker is observable) | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-73 | the taskbar lane: the requests between two served ones get no outcome, and a worker that dies leaves `Asks::worker` set — requests are counted and never served, and nothing says so | A5 (Taskbar × every request ends exactly once; a dead worker is observable) | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-74 | the computation lane (`MathWorker`): admission and answers are unbounded `mpsc::channel`s, each way | A5 (Computation × a full lane answers without waiting; answers held are bounded) | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-75 | the computation lane: no request identity — an answer carries only its question, so one question asked twice gives two answers nobody can tell apart | A5 (Computation × every request has its own identity) | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-76 | the computation lane: the decoration thread's death is invisible while the scaling and path-verification threads hold clones of the one answer sender; the drain sees a disconnection only when all three have gone | A5 (Computation × a dead worker is observable) | none yet | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open; **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-77 | §5.3 row 23 — the first window's GPU is opened with `pollster::block_on(GpuContext::open(…))` in `Runtime::create`, on the window thread, a wait no row listed | thread-door note revision (e)2; A1a | B9 (device recovery rests on deadlines and rebuilds on a worker) | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — it moves with D-42 | open — registered as row 23 `pending`, door `admission::doors::GpuOpen`; it stays on the window thread until then (coordinator, 2026-09-26); admitted where it stands through `gpu_door::open_first_window` (A1d, 2026-09-26); **2026-10-08 (J4):** untouched; was 0.4.6 |
| D-78 | `trace_sink::Shutdown`'s `Drop` flushes the trace — a bounded wait, through its admitted door — when `fn main` returns early from a loop that could not be built | thread-door note (c)4, (e)3, (g)1; A1e | none yet — *The trace writer is retired through its admitted flush door, never by a drop* | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — a row of the closed `Drop` inventory (A1e, 2026-09-27); **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-79 | `AttentionPipe` and `LaunchPipe`, on both platforms, join their listener thread in `Drop` | thread-door note (c)4, (e)3; A1e | none yet — *An endpoint is retired through an explicit door, not by its drop* | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — a row of the closed `Drop` inventory (A1e, 2026-09-27); no product reach: the endpoints live in statics; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-80 | the video engines (`video::engine::Engine`, `macos_player::Engine`) and `VideoSeat` and `VideoSeats` shut the engine down — a bounded poll and a join — in `Drop` | thread-door note (c)4, (e)3, (f)2; A1e; scope extended by the note's revision (m) (2026-09-27): the direct closes of `window_waits.tsv` row 25 and the media session's reader quiescence and `MFShutdown` of row 27 | none yet — *A video engine is shut down through an explicit door, not by its drop* | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — rows of the closed `Drop` inventory (A1e, 2026-09-27); **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-81 | `PtySession`'s `Drop` finishes the input dump (a write, two `sync_data`) and runs `shutdown` (a bounded reap, a bounded join) | thread-door note (c)4, (e)3; A1e | none yet — *A shell is taken apart only through `retire_within`, never by a drop on the window thread* | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — a row of the closed `Drop` inventory (A1e, 2026-09-27); on `pty-retirement`, and on the caller only when that thread cannot start; **2026-10-08 (J4):** untouched; was 0.4.7 |
| D-82 | WinHTTP's `http::Request` waits up to `CLOSE_WAIT` (5 s) on a `Condvar` in `Drop` for its handle's closing callback | thread-door note (g)2; A1e | none yet — *A download's request is closed through its own bounded door, not by its drop* | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — a row of the closed `Drop` inventory (A1e, 2026-09-27); no product caller of `https_download` yet; **2026-10-08 (J4):** **widened**: `https_download` has a product caller now, the update job (`update_job`, U-18…U-20), so the drop's wait runs on the `bt-update-job` worker, never the window thread; was 0.4.7 |
| D-83 | the `bt-app` and `bt-term` suites are not portable: on macOS 294 of `bt-app`'s 4,600 tests (282 after ticket 72) and 43 of `bt-term`'s fail, each asserting a Windows fact on every host | ticket 72's report (Mac mini, 2026-09-26) | none yet | 0.4.8 — H1 (T-D83), then H7 | open; **2026-10-08 (J4):** **narrowed**: core-macos runs `bt-app`'s updater modules (T-GUARDS-BLIND, `5f7ba1fe`) and its uninstall tests (`1d914d1a`); the rest of `bt-app` and all of `bt-term` are checked, not tested; was 0.4.7 |
| D-84 | aggregate turn scheduling beyond deferrable work: a turn's deadline is one number shared by every window, and only deferrable work is scheduled against it | budget note §R-B, §R-G (Codex's Q2; the owner's ruling of 2026-09-25, 1); A4 | none yet — owed a 0.4.7 ticket, *The window thread's turn is scheduled across windows and sources*, after B4–B9 | unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) | open — opened by A4 (2026-09-27); **2026-10-08 (J4):** untouched: its ticket was to follow B4–B9, none of which was cut; was 0.4.7 |
| D-85 | the program-walk lane: the requests made while a walk is out are answered by the next walk and get no outcome of their own; the line `diagnostics.log` gets when that walk ends is the only record of them | A5 (Programs × every request ends exactly once); T-PROGRAMS-REFRESH (2026-10-08) | none yet | unassigned — listed for the coordinator (2026-10-08) | open |

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

**Ledger.** source: structure review C-1 · K-1 · ticket: none yet · version: 0.5 — **0.5 precondition** (the 2026-09-21 structure review): the session/document/view ownership note is 0.4.8 J5 (T-05-CONTRACTS, design only); the first slice and the session registry are 0.5; the backend stays 0.6 · status: open; **2026-10-08 (J4):** untouched; 0.4.7 added session facts on the session's own thread (T-PANE-IDENTITY's `current_frame`, `screen_fence_state`; T-PANE-COLUMNS' `foreground_program`), no owner beside the window; was 0.4.7.

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

**Ledger.** source: C-2 · K-6 · ticket: through D-33…D-47 · version: unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — A2 (the bare-site lint), the step its closure waits for, has no ticket; G4 takes the waits #29 and #31 · status: open — §5.3 row 1 repaid on `2657e5e3`; rows 13 and 14 repaid by tickets 48 and 49 (D-45, D-46); row 5 repaid by ticket 50 (D-37); row 6 repaid by ticket 51 (D-38); the taskbar probe left inside row 13 repaid by ticket 62 (D-68); row 22 repaid by ticket 63 (D-69); the list is one registry with a generated §5.3, each owner-thread wait a door type held to it (A1a, 2026-09-26: advanced, not repaid); the thread door lends every worker a `WorkerCtx` and the hand-off door takes it (A1b, 2026-09-26: advanced, not repaid); every owner-thread door takes its token and each listed wait happens only admitted (A1d, 2026-09-26: advanced, not repaid); the escapes the compiler cannot see are fenced by one source guard, and every `Drop` that may wait is a row of a closed inventory (A1e, 2026-09-27: advanced, not repaid); every raw effect outside a door is a row of `docs/plans/window-thread-bare-sites.tsv`, which only shrinks — 248 sites, seeded at 263 on `2cc59a83` — and the configuration the lint will need is fenced and its probe proven per target (A2a, 2026-09-26: advanced, not repaid); every turn is accounted and every admitted call measured per call, with a budget line for each of the four triggers and an exit summary from the run's atomics (A3, 2026-09-27: advanced, not repaid — A1 and A3 have landed, and by the owner's ruling of 2026-09-25 D-2 closes when A2 lands too); deferrable work yields to the earliest window's deadline — the search walk's slice and the idle calls ask one `TurnAllowance` a turn (A4, 2026-09-27: advanced, not repaid — A1, A3 and A4 have landed; what the allowance leaves of aggregate scheduling is D-84); **2026-10-08 (J4):** **narrowed**: row 11's process birth runs on the `bt-pty-birth` worker and the window thread joins it (T-ENV-REFRESH round 4); an unlisted window-thread join of the PowerShell script worker was found and removed, and the scanner taught its shape (`c7a604ab`); row 29 added, ruled to stay (the update's exit guard, 2026-09-28); bare sites 248 → 232; A2 has not landed; was 0.4.6.

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

**Added to the list (0.4.6 ticket U-20, 2026-09-27).** The update job's
worker, `bt-update-job` — one per press, started by the Windows Prepare
(`update_prepare_windows`, U-20) and the macOS Prepare (`update_prepare_macos`,
U-27) — is one more of the set: its own report inbox and wake
(`update_job::Poster`, `install_progress_wake`, its own `AppEvent` and
station), its own cancel flag, and no deadline of its own beyond the download
door's and the archive expansion's. The common contract has not landed, so the
worker keeps this shape until it does; it is a mutation of the install folder's
`.folio-update\`, not a probe, which is the depth review's point above.

**Version.** 0.4.4. **Status.** open.

**Ledger.** source: K-9 · C-2 · ticket: none yet · version: 0.4.8 — B4 (T-PROBE-NO-CACHED-FAILURE) and B3 (T-FRESH-FACTS: the probes as facts re-asked); G1 for the two named containment gaps · status: open — the update job's worker `bt-update-job` added to the list (U-20, 2026-09-27; its two drivers U-27 and U-20); **2026-10-08 (J4):** **narrowed**: every machine probe starts through one door, `bt_platform::spawn_probe` / `probe_output`, which contains its whole process tree and ends it on deadline, wait, drop and unwind (T-PROBE-CHILD, T-PROBE-BORN-IN-JOB; ARCHITECTURE §2.2); the three PowerShell probes share one five-second deadline and a failed answer is asked again at the next reader edge (T-ENV-REFRESH round 4, T-INTEGRATION-INJECT-3 round 2). Still open: `copilot --version` and the macOS locale probe have no deadline, and each probe keeps its own slot, wake and latch (`profile_runtime::REMOVAL` has no in-flight latch); was 0.4.6.

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

**Status.** narrowed to the emergency half, D-56 (2026-10-09, G7-SWEEP-048). The
controlled road takes the depth review's exit as the coordinator ruled it for G7:
every dirty buffer goes through the quit's judged write — the file when it can be
written, its conflict check kept — and otherwise into a copy in the data
directory's `recovered` folder, never over the file; the outcome of each is said
in `diagnostics.log` before the windows close. The panic hook is untouched.

**Ledger.** source: C-3 · K-8 · ticket: G7-SWEEP-048 · version: 0.4.8 — G7 (census #28: an internal stop loses unsaved preview edits, this row's controlled-failure road); the emergency half is D-56 (0.5) · status: narrowed to the emergency half (D-56); **2026-10-09 (G7):** the controlled road keeps every unsaved preview edit (`FolioApp::stop_every_window`); **2026-10-08 (J4):** untouched; was 0.4.6.

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

**Ledger.** source: K-2 · C-4 · ticket: the ticket that depends on each row · version: 0.4.8 — rolling: each 0.4.8 ticket folds the `docs/RULES.md` row it depends on in its own commit; what is unfolded at 0.4.8's end goes to 0.5 with its subsystem · status: open — 19 folded; row 28's wheel half folded by ticket 37 (the press half is not); row 25's font-list half folded by ticket 50 (the glyph atlas half is not); **2026-10-08 (J4):** **narrowed**: row 36 (the update check) folded on `5bdc1a5a` (0.4.6); RULES has 55 rows, 24 folded in whole or part, 31 `not yet folded`; was 0.4.6.

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

**Ledger.** source: K-10 · C-4 · ticket: through D-48…D-50 · version: unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) · status: open — printed path written; its hand-off hop updated on `2657e5e3` and `5d4c7aff`; **2026-10-08 (J4):** untouched: §7.2's chains are still first-pass lines (T-IMAGE-N added a hop to §7.1's recognition and verdict rows); was 0.4.7.

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

**Ledger.** source: K-14 · C-4 · ticket: with D-28 · version: unassigned — no 0.4.8 plan ticket takes it; listed for the coordinator (2026-10-08) — it closes with D-28 · status: open; **2026-10-08 (J4):** untouched; was 0.4.6.

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
| 2026-09-24, ticket 37 | persistent pane state — a terminal pane's text size while it is not 100 % | non-urgent | non-modal | the existing top-right controls: left of the pane's `⌄` (`seats::text_size_mark_beside`); a click resets it | ruled 2026-09-24 (owner): the same rule in a headless pane's corner, left of its `⌄`; no room, no mark |

**Ledger.** source: K-3 · C-4 · ticket: census-5a (note) and census-5b, after the ruling · version: 0.5 — the owner rules the table before 0.5's notification model · status: open — 28 surfaces inventoried and seven kinds proposed for the ruling in `docs/plans/design/ownership-census-2026-09-25.md` §3–§4 (2026-09-25); **2026-10-08 (J4):** untouched; was 0.4.6.

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

**Ledger.** source: K-4 · C-4 · ticket: none yet · version: 0.5 — the fourth entrance is 0.5's outward interface · status: open — §9 table written; export ruled not an entrance on `5f433943`; **2026-10-08 (J4):** untouched; 0.4.8 F8 (T-LAUNCH-ENV) adds an explicit launch input and must say whether it is a row of §9's table; was 0.4.7.

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

**Ledger.** source: K-5 · C-4 · ticket: none yet · version: 0.5 — the operation vocabulary, with the 0.5 self-report · status: open; **2026-10-08 (J4):** untouched: 0.4.6 and 0.4.7 added `diagnostics.log` lines (U-39, U-42a…e) each in its own format; was 0.4.6.

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

**Ledger.** source: K-7 · C-4 · ticket: census-1 · version: 0.4.6, with D-32 · status: repaid (census-1) — the census is `bt_source::FieldCensus` over `bt-app`; inventory and site rows are query reports under `target/`, while the owner annotations (census-1's proposals, until the owner rules) and the shrink-only unknown row multiset are committed and held by `bt-source`'s `census` test and `scripts/ci/check-census-unknowns.ps1`, so a fact with an unknown is `incomplete`, never single-writer. D-32 (census-7) is what the census was taken for, and stays open.

**Repaid (2026-09-26, census-1).** The census the smallest change asked for is a query, `bt_source::FieldCensus`, over `bt-app`'s product items. Every `self.` access resolves to `Runtime`, `WindowRuntime`, or through `Deref` to `TabState` by stated rules (the census note's revision (b)2 §3); every field of `App`, `WindowRuntime`, `TabState` and `LeafSession` has a committed inventory row, with proven writers, mutable access, hub membership and inner mutability in separate columns, and every site the rules cannot resolve is listed with its item and reason. `bt-source`'s `census` test is the diff gate. What it is for — drafting the unassigned methods' destinations against owners rather than name clusters — is D-32, census-7.

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

**Ledger.** source: K-11 · ticket: none yet · version: 0.5 — `bt-platform` is not split in 0.4.8 (plan K, *Not in 0.4.8*); J2's small boundary crate for D-14 is the first extraction · status: open; **2026-10-08 (J4):** untouched; was 0.4.7.

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

**Repaid (2026-09-21, `21cf1ef8`; recorded 2026-10-08 by J4).** The first of the
three alternatives was taken the same day as the review: the probe moved to
`crates/bt-corpus`, byte for byte, and `bt-pty`'s manifest names `bt-term` under
`[dev-dependencies]` only, with a comment that no code the crate ships names
`bt_term` (DESIGN, 2026-09-21; ARCHITECTURE §3.2, *Done 2026-09-21*). The ledger
gained its columns two days later from the review's list and carried the row
open until this reconciliation.

**Ledger.** source: K-12 · C-4 · split prep P21 · ticket: P21 · version: — · status: repaid on `21cf1ef8` (2026-09-21) — the ConPTY width probe lives in `bt-corpus` and `bt-pty`'s manifest names `bt-term` only under `[dev-dependencies]` (ARCHITECTURE §3.2, *Done 2026-09-21*); carried open by mistake from 2026-09-23 to 2026-10-08 (J4). Not this row: `bt-pty`'s optional edge to `bt-platform` behind its test-only `test-shell` feature and `bt-term`'s dev-dependency on `bt-pty` (T-TEST-SHELL-HYGIENE, T-INTEGRATION-INJECT-4 round 6), for J1 to classify.

---

## D-14 — `bt-term → bt-platform` is broader than its manifest says

*C-4 (refinement) · K-11* · **Class:** wrong layer.

**Evidence.** The manifest comment calls it one call. There are three product
import surfaces: `inline_image::resample_pool` sets a thread priority through
`bt-platform`, `session::verify_path` calls `handoff::resolved_for_a_door`, and
`inline_image::read_and_decode_local_image` goes through the read ledger.

**Narrowed 2026-10-08 (CC-3).** The read ledger moved to `bt-effects`, with the
admission vocabulary (`docs/ARCHITECTURE.md` §3.1), and `bt-term` names it from
there; `bt-term`'s manifest comment now names what is left. **What CC-4 still
owes**, counted in `bt-term`'s source on the CC-3 commit: product
`inline_image.rs` `resample_pool` (`set_current_thread_priority`,
`ThreadPriority`), `inline_image.rs` `local_host_names` (`host_names`) and
`session.rs` `verify_path` (`resolved_for_a_door`); tests `session.rs`
(`quiet_command` twice, `host_names` once), which need the platform only as a
dev-dependency once the product edge goes. The exemption row
(`scripts/ci/crate-edge-exemptions.tsv`) stays until CC-4 removes the edge.

**Repaid 2026-10-08 (CC-4: the product edge in `0479c1ca`, the last, dev, edge in `744cc02a`).** `bt-term`'s manifest names `bt-platform`
nowhere, not as a dev-dependency either; `cargo tree -p bt-term -i bt-platform`
prints nothing, and the exemption row is deleted. The three remaining surfaces
are the host's (`docs/ARCHITECTURE.md` §3.2): the machine's names and the
resample pool's thread-start hook are process-wide facts the host installs
(`bt_term::install_host_names`, `bt_term::install_pool_thread_start`; `bt-app`
installs both in `host_answers::install`, before its event loop), and
`verify_path` takes the door-ready resolver as a parameter, which its one product
caller fills with `bt_platform::resolved_for_a_door` — the function did not move
and the verdicts were compared byte for byte on a fixture set before the old call
was deleted. The tests that needed the platform left with it: the real-shell
tests went to `bt-pty/tests`, and the host-name pin's real-producer half to
`bt-app`.

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
forbids. **Status.** repaid on `744cc02a`.

**Ledger.** source: C-4 · K-11 · ticket: CC-3 (`bt-effects`), CC-4 · version: 0.4.8 — J2 (T-WRONG-EDGES), after J1; the boundary crate is also I1's prerequisite · status: open; **2026-10-08 (J4):** **widened**: a fourth product import surface, `bt_platform::host_names` (`inline_image`'s local-host set, `ae514613`, 0.4.6); was 0.4.6; **2026-10-08 (CC-3):** **narrowed**: the read ledger is `bt-effects`', three surfaces remain for CC-4; **repaid on `744cc02a` (CC-4)**.

---

## D-15 — `bt-term → bt-math` is real coupling

*C-4 (refinement) · K-11* · **Class:** wrong layer — **recorded debt, not a task**.

**Evidence.** `session.rs` typesets through `MathEngine` and `key_for_em_px` in
product code; `inline_image::decode_svg_bytes` rasterises through
`rasterize_svg_document`; and `crates/bt-term/src/lib.rs` re-exports the engine.
The math data types (`MathRenderKey`, `MathRaster`, `MathRenderError`,
`MathFailureStage`) are `bt_doc::math`'s since CC-5, and `bt-term` names them
there. (The binary target that
used it too, `bt-repaint-oracle`, moved to `bt-corpus` with CC-4.) Hiding the
dependency behind re-exports changes nothing.

**If left.** The terminal crate carries decoration policy. Accepted for now.

**Smallest change.** The composition design's three steps (T-COMPOSE-CRATE
§3.4 D-15): the data types to `bt-doc` (CC-5, done), math execution to
`bt-compose` (CC-6b), the SVG codec installed by the host (CC-7), after which
`bt-term` has no `bt-math` edge.
**This row exists so that the edge is recorded rather than rediscovered.**

**Version.** 0.5 at the earliest. **Status.** decided — recorded as debt.

**Ledger.** source: C-4 · K-11 · ticket: recorded by D-27; CC-5, CC-6b, CC-7 (design T-COMPOSE-CRATE §3.4) · version: 0.4.8 — J2 (T-WRONG-EDGES): the design decides; J1's allow-list records it until then · status: open (recorded debt); **2026-10-08 (J4):** untouched; was 0.4.7; **2026-10-08 (CC-5):** **narrowed**: the data types moved to `bt-doc`; execution remains (`typeset`'s engine calls for CC-6b, the SVG codec and the `MathEngine` re-export for CC-7); **2026-10-08 (CC-6a):** untouched; the receiving crate `bt-compose` exists (layer 6).

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
stated in `docs/ARCHITECTURE.md` §6 and `docs/RULES.md` row 52; the thread door's
bypass is repaid (0.4.6, A1c: every thread `bt-app` and `bt-platform` start comes
through the door, held by a source guard); the enumeration lane and the thumbnail
thread's band are open, the band with the other observation threads' in RULES 53's
0.4.7 ticket.

**Ledger.** source: K-13 · ticket: A1c (the thread door's bypass) · version: 0.5 — **0.5 precondition**: the side-effect contract's admission half; its note is 0.4.8 J5 (T-05-CONTRACTS); the enumeration lane and the observation threads' band are built in 0.5 · status: open — rule stated; the thread door's bypass (`folio-web-thumb` and five unnamed spawns) repaid by A1c in 0.4.6; the enumeration lane and the observation threads' band (`folio-web-thumb` among them, RULES 53's 0.4.7 ticket) remain; **2026-10-08 (J4):** untouched: every thread 0.4.7 added comes through the door (T-PROBE-CHILD's reader, T-KEYBOARD-CTRLALT's `folio-layout-tables`, T-UNINSTALL-UX's remover pipe); `folio-web-thumb` still stands at `Normal`; RULES 53's 0.4.7 ticket was never cut; was 0.4.7.

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

**Ledger.** source: C-4 · ticket: none yet · version: 0.5 — with D-1's document owner, after J5's note · status: open; **2026-10-08 (J4):** untouched; was 0.4.7.

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
  timeout; a hung probe thread is never reclaimed. *(Half fixed: the PSReadLine
  probe has the five-second PowerShell deadline since T-ENV-REFRESH round 4;
  the Copilot probe's tree is contained since T-PROBE-CHILD but still has no
  deadline — D-3.)*
- `folio-web-thumb` stands at `Normal`, breaking the band rule (it goes
  through the thread door since A1c, at the band it had), and **panics on
  spawn failure**.
  `folio-video-canplay` holds the video stack's last unbounded join.
  `bt-dir-watch` has an unbounded receive in its start and an unbounded join in
  its drop.
- `crates/bt-pty/Cargo.toml`'s comment claims the library depends on the
  terminal crate; production code never names it. *(Fixed by `21cf1ef8`, with
  D-13.)*
- `crates/bt-term/Cargo.toml`'s "one call" comment is stale — there are three
  import surfaces (D-14).
- `bt-term::session::opening_it_would_run_it` has a doc comment saying a platform
  helper answers the question off Unix, while that arm returns a constant and the
  helper is never called in that crate.
- `the_shell_page_is_gone` walks the crate's source non-recursively, so a future
  `runtime/` directory silently escapes a whole-program prohibition guard (D-7).
- `crates/bt-platform/src/lib.rs` contains an embedded NUL byte; the file is not
  clean UTF-8. *(Fixed by T-GUARDS-BLIND, `5f7ba1fe`: the bytes are written as
  escapes, and a gate refuses a NUL byte in any tracked text file.)*
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

**Repaid by census-2 (0.4.6).** The census tells two kinds of subject apart by how the test reads. A literal in the item-naming position of a `bt-source` query — the constructors listed in the script's `ITEM_QUERIES` (`ItemQuery::{function, method, type_item, field, variant}`, `Scope::Impls`), or a test helper or closure the script derives from them (`method_body(owner, name)`, `method_body(name)`, `let body = |name| method_body("Runtime", name)`, a `for` over a literal array fed to one) — is **item-bound**: its witnesses are resolved by the query's own kind and owner type, and when 2a moves it the row says *subject moves: follows the item*, not *retarget*. Every other literal stays **file-bound** with today's reading; a literal spelled both ways is both. A test whose only source reading is an item query is now a census row (it was invisible before, so a migrated pin dropped out of the inventory the moment it stopped naming a file). Ten of the eleven hand-written overrides were removed: each audited a `main.rs` text reading that no tree the script runs on still makes. Evidence: the script's `--self-check` fixture, and the relocation `7edfd12a` (census-2's report).

**Ledger.** source: split prep, 2026-09-22 · ticket: census-2 (the census note's revision (b)) · version: 0.4.6 — D-29…D-32 need a true census · status: repaid (census-2, 0.4.6) — each subject is item-bound or file-bound by how the test reads it (`ITEM_QUERIES` and the helpers derived from it); only a file-bound subject read out of `main.rs` is a reader 2a must retarget; `--self-check` holds the fixture.

---

## D-19…D-63 — the rows added on 2026-09-23

One line each: what it is, its anchor by name, and why its version.
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
  0.4.6, because D-29…D-32 regenerate the inventory through them.
- **D-20 · P10 (1 row, narrowed).** `bt_app::platform_gate_tests`' Rust
  directory walk remains. The portable-core script's former duplicate reader
  and their agreement test are retired; the script now checks only portable
  crates as a cheap local gate.
- **D-21 · P12 (5 rows).** `bt-platform`'s three directory walkers and the
  `stand_in` guard, one ticket because they share a file. 0.4.6: CI-only
  work.
- **D-22 · P13 (3 rows).** The remaining source-text walks — one directory walk,
  two `include_str!`. 0.4.6.
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
  `uninstall_tests`' hand-supplied file list, `context_menu` and `msix`. 0.4.6:
  `uninstall_tests` reads a module graph rather than a list before settings
  moves.
- **D-26 · P18 (not on the list).** The ~110 `[..].concat()` needle halves,
  written whole now that `bt-source`'s provenance prevents self-match. A
  readability change with no coverage consequence; 0.4.6.
- **D-27 · P19.** The CI dependency-direction guard of split prep §8.4 over
  `cargo metadata` plus a per-target scan, with the exception set compared
  against the merge base; and the line recording `bt-term → bt-math` (D-15).
  0.4.6: cheap, independent of `bt-source`, and it keeps D-13's repair from
  drifting back.
- **D-28 · P20.** MIGRATION-DEBT at zero and deleted; `bt_source::FileScoped`
  final at four entries or fewer, each with a reason; the tripwire
  (`crates/bt-source/tests/tripwire.rs`) green. 0.4.6: it is the class sum.

### The relocation's residue

Step 2a moved 25 of 28 topics (1,195 methods) into `crates/bt-app/src/runtime/`.
Each unmoved topic is held by a reader the move turns red — by split prep §6.0
the reader's defect, not the move's. **0.4.6 for all three**: each is one
reader fix plus a pure move, beside the rest of the split's second half.

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
  the order `flush_pending_pty_resize` represents is the contract. 0.4.7.
- **D-50 · paste convergence.** `Runtime::prepare_clipboard_paste`,
  `paste_text`, `bt-term`'s `input::paste_bytes`, and the clipboard read on
  the window thread. 0.4.6: tickets 02 and 03 changed `deliver_paste` and the
  hops are fresh.

### `docs/ARCHITECTURE.md` — ownership (§4)

§4.1's three-owner split is D-1; §4.3's `Deref` trap is D-11's census. §4.2
rules that the survey's twenty-two multi-owner facts are five problems with
one rule each; none of the five rules is yet a shared shape in the code.
Collapsing a class means its facts follow its one rule through one contract,
with each fact's differing policy kept. D-52 and D-53 are 0.4.6, with the
lanes they name; D-51, D-54 and D-55 are 0.4.7.

- **D-51 · observations of external state** — facts 1 (the printed-path
  verdict ledger), 5 (`profiles::title`'s cache, keyed on one of its two
  inputs), 6 (PSReadLine's three copies), 7 (settings, profiles and pins as
  read), 12 (`shell_integration::PROFILE_ANSWERS`, never re-asked).
- **D-52 · asynchronous publication and competing operations** — facts 4
  (`schemes::CATALOGUE` and `REVISION`), 10 (Explorer registration's three
  copies; **touched by U-25, not widened** — the start's renewal is one more
  trigger of the same probe under the same `BUSY` latch, not a new copy or a
  new writer thread), 13 (the font slots, two writers), 19 (`profile_runtime::REMOVAL`,
  one slot, two writers, no latch), 22 (the generation check re-derived three
  times: `window.background_decode`, `window.clipboard_picture`, the web host).
- **D-53 · durability and external transactions** — facts 8 (the session
  snapshot's three copies), 9 (the marks record), 11 (the update check's
  memory, file and claim). Fact 11's part repaid by U-6 (0.4.6):
  `update::OfferState`.
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
  may be lost. D-4 is the controlled-failure half. 0.4.7: the journal is new
  storage and follows the storage lane of 0.4.6.
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
  **Narrowed (2026-10-08, J4):** both tests ask a loopback listener
  (`answering`, `ask_this_machine`) since `64bcebdb` (2026-09-13) — the local
  server was there before this row was written. What keeps it open is the
  rule: no sentence in DESIGN, RULES or ARCHITECTURE says a transport test
  never reaches the network.

The other entries of `scripts/ci/ignored-tests.txt` are probes, one writer and
two privilege-bound fixtures, each with its reason there; they are not debt.

### macOS

- **D-61 · `webnav::tests::a_local_file_is_shown_and_typed_as_a_path_and_loaded_as_a_uri`**
  is red on a Mac: `file_url_of_local_path` receives a `D:\…` path there and
  answers `None`. The test states a Windows fact on every platform. 0.4.5: small.
  **Repaid by ticket 55:** showing a `file:` URL as a path stays a string
  question on every machine, but taking a typed path back is a question about
  this machine (`Path::is_absolute`), so the test now walks both spellings —
  `D:\…` and `/Users/…` — and requires the one this machine calls absolute to
  come back as its URL and the other to be refused.
- **D-62 · `bt-render` clippy on macOS.** `CJK_FALLBACK_FAMILIES`,
  `CJK_FALLBACK_FONT_FILES` and `CHROME_SANS_FONT_FILES` are never used there,
  so `cargo clippy -p bt-app` stops in `bt-render`. 0.4.5: small.
  **Repaid by ticket 55:** all three move under `#[cfg(target_os = "windows")]`,
  the `cfg` every one of their readers (`FolioFallback`, the Windows
  `terminal_font_system` and `load_chrome_sans_family`, and the Windows-gated
  tests) already carries. A Mac should not read them: it has its own
  `MACOS_CJK_FALLBACK_FAMILIES` and `MACOS_CHROME_SANS_FAMILIES` and names no
  font files.
  The row undercounted: clippy stopped at the library, and behind it the test
  target carried fifteen more dead-code warnings on a Mac: test helpers and
  fixtures, three of them `#[cfg(test)]` items of the library itself, whose
  only readers are `#[cfg(target_os = "windows")]` tests (`shape_chrome_labels`, `GpuContext::lose_the_device_on_purpose`,
  `ByteLru::is_empty`, `HanStream`, `ChineseFourK`, `stress_label`, the
  `a_lost_device` module's `FORMAT`, `ink_pixels`,
  `a_sentence_this_window_keeps` and `one_frame`, and four more). Ticket 55
  gives each the same `cfg` as its readers, as `chinese_four_k_frame` already
  had it, so `cargo clippy -p bt-render --all-targets -- -D warnings` is clean
  on a Mac.
- **D-63 · the macOS CI job's reach.** `core-macos` checks `bt-app`, `bt-term`
  and `bt-render` but tests none of them, and runs clippy on `bt-platform`
  only — which is why D-61 and D-62 were found by a person. 0.4.6, after D-61
  and D-62 make widening it green.
  **Repaid by ticket 72 (2026-09-26).** `core-macos` runs `logic`'s clippy line
  word for word (`--workspace --all-targets --exclude mitex --exclude
  mitex-parser -- -D warnings`), plants a macOS-only dead item and demands that
  line refuse it, and adds `bt-render` (295 + 7 tests, green on the Mac mini's
  Metal device) and `bt-corpus` (16 tests) to its test line. `bt-app` and
  `bt-term` stay checked and not tested, because their suites are red on macOS
  by construction (D-83); that part of the row moves there rather than staying
  open here.

---

## D-64…D-67 — the rows added on 2026-09-24

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
  **Ticket 43, 2026-09-24.** The phases are stations of the stall self-report
  now, and the pump between handlers is one of them (`message pump`). Measured
  headless on the development machine (three runs, a message-only parent):
  the first page's `request_controller` 88–269 ms synchronous with ~4,500 page
  faults, then one engine dispatch of 70–303 ms on the pump; later pages ~3 ms
  and ~50 ms; the environment 10–38 ms; the install burst 2–6 ms, under the
  frame budget, so it is not split. The environment cannot be made on a lane:
  WebView2 answers `0x802A000C` ("can only be called from the thread that
  created the object") to an environment made on another thread, for both
  `BrowserVersionString` and `CreateCoreWebView2CompositionController`. What is
  left is §5.3 row 21, and the row stays open until a ruling chooses how to
  move it. **Ruled 2026-09-24 (coordinator):** the environment, and with it the
  runtime's processes, is warmed on the window thread during an idle turn after
  startup, before the first page is asked for; the controller is still made per
  page; hosting WebView2 on its own UI thread is not taken (it changes §5.2).
  Follow-up: the warm-up, ticket 54.
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
  (0.4.7)** — the whole overlay pass in encoded space (glyphon `ColorMode::Web`),
  so the 0.5 restyle compares like with like. The row closes with the second.
- **D-66 · a fading surface steps at the frame it lands on.** Added by ticket 46
  (`bt_render::OverlayGroup`), which composites a fading surface on encoded bytes
  and draws it straight onto the frame, in linear light, once it is at rest — so
  the byte-identical-at-rest guarantee holds. Opaque pixels meet at the landing;
  translucent ones (shadows, antialiased edges) do not, because the two blends
  disagree over an unknown ground (the audit's §7 table). On the dark theme the
  step is at most about 1.5 ΔL* and invisible; on the light theme the tip's shadow
  at its darkest pixel goes from about `#DB` on the last fade frame to `#EE` at
  rest, a visible lightening as the fade lands. Owed: the L variant — the whole
  overlay pass in encoded space — after which the fade and the rest are one
  arithmetic. 0.4.6, by the coordinator's ruling on ticket 46.
- **D-67 · `bt-app` clippy on macOS.** Found by ticket 55: once D-62 let
  `cargo clippy -p bt-app --all-targets -- -D warnings` get past `bt-render` on
  a Mac, `bt-app` stopped on errors of its own. The bin `folio` build has nine:
  `attention_copilot.rs` (associated functions `parse` and `leading_number`
  never used), `explorer_menu.rs` (variants `Absent`, `Current`, `Elsewhere`
  never constructed; function `classify` never used), `psreadline.rs`
  (`PROBE_COMMAND`, `parse_probe_output` never used), `shell_integration.rs`
  (`PROFILE_COMMAND`, `parse_profile_answer` never used), and two ignored
  `#[must_use]` results — `bt_platform::hide_every_window_of_this_process` in
  `main.rs` and `bt_platform::hotkey::allow_foreground_for` in
  `launch_wire.rs`. The bin `folio` test build has five, among them
  `psreadline.rs`'s `PROBE_COMMAND` and `shell_integration/profile_marks.rs`'s
  `apply` never used. The shape is D-62's: items whose only readers are
  Windows-gated, and seams whose answer only Windows reads. 0.4.6, beside D-63,
  since widening the Mac CI job hits it.
  **Repaid by ticket 72 (2026-09-26).** Ten distinct findings behind the fourteen
  counts (the test build's five repeat four of the bin's nine). Each dead item takes
  its readers' gate — `#[cfg(windows)]` for `PROBE_COMMAND` and
  `PROFILE_COMMAND`, `#[cfg(any(windows, test))]` for `Version::parse`/
  `leading_number`, `parse_probe_output` and `parse_profile_answer`;
  `profile_marks::apply`, read only by two Windows tests, becomes their
  `#[cfg(windows)]` helper in `shell_integration`'s test module.
  `explorer_menu::read_state` loses its platform arms instead of gaining a gate:
  `bt_platform::msix::registered` exists on every platform and `supported()`
  answers `Unsupported` off Windows first, so the one portable reading
  constructs `Absent`/`Current`/`Elsewhere` and calls `classify` everywhere,
  with the same answer on a Mac. The two ignored answers are dropped by name
  with the reason at the call (`let _ =`), and the Windows arms of both doors
  gain the `#[must_use]` the portable arms already had, so the two signatures
  agree and a caller that drops the answer is told so on every platform.

## D-68 — the row added on 2026-09-25

- **D-68 · the taskbar's state asked on the window thread.** The owner's stall
  self-report on next93 (`1ce9e187`, stall #3 at session age 124 s) held the
  window thread 535 ms, and inside it `sample_window_place` spent 99 ms and 92 ms
  in `taskbar_is_auto_hidden` — once under the wake fold and once under the
  application turn. The call is `SHAppBarMessage(ABM_GETSTATE)`, a message to
  Explorer's taskbar that waits while Explorer is busy. Ticket 48 had made the
  place one reading per turn (D-45) but left every probe in it on the window
  thread. **Repaid by ticket 62 in the same commit**: `bt-app::taskbar_lane`
  asks on its own `BelowNormal` thread — at launch, every
  `taskbar_lane::REFRESH_INTERVAL` (5 s) while a window is on a screen, and on
  `AppEvent::SystemPreferencesChanged` — and publishes into a
  `bt_platform::TaskbarState`, a numbered slot that takes only newer answers; the
  window thread reads it with one atomic load. The station `taskbar reading`
  (`Station::PlaceTaskbar`) now measures that read.

## D-70…D-76 — the rows added on 2026-09-25 by the lane contract (A5)

0.4.6 ticket A5 (`docs/plans/design/window-thread-budget-2026-09-25.md` §R-D)
put one contract over four lanes: `bt-app::lane` declares each lane's policy —
replacement, execution order, delivery order, bounds, cancellation — and
`lane_contract_tests` runs eight claims on the hand-off, font, taskbar and
computation lanes through adapters that drive each lane's own admission,
publication and acceptance, with only the executor (held, or ended on purpose)
and the loop's wake standing in. A claim a lane fails is a row of
`lane::EXPECTED_FAILURES`, with the exact kind of failure and one of the rows
below; the suite is red on an unexpected pass, a different failure, a skipped
lane or claim, and a claim that exercised no request. **No lane's behaviour
changed**: the font lane's three functions became methods of
`settings::FontLane` and `MathWorker::spawn` takes its wake as a closure, so the
adapters run the product's own code.

**What is mitigating today.** Every "dead worker" row (D-70, D-72, D-73, D-76)
is reachable only by a worker's thread ending while the process lives. A panic
on any thread runs `install_panic_log_hook`'s fatal road and ends the process,
so today a dead worker is a dead Folio; the rows are the contract debt for the
day a lane must survive its worker (the storage lane of B4 is the first that
will be asked to). The unbounded rows (D-71, D-74) are bounded in practice only
by how fast presses and decoration requests arrive and how often the loop
drains; neither lane states a bound, and the contract asks for one.

- **D-70 · hand-off, dead worker.** Sequence: a hand-off is executing, two
  more are queued, the worker ends. `HandoffLane::answers` extends
  `turned_away` with `answers.try_iter()`, which stops at a disconnected
  channel as it stops at an empty one; `LANE_GONE` is answered only when a new
  submission meets `TrySendError::Disconnected`. The three ids stay in the
  window's `Pending` with no outcome. Owed: a terminal outcome for every
  accepted id when the worker ends (a lane-level fault the drain reports once,
  which each window turns into its surfaces' refusals).
- **D-71 · hand-off, answers held.** `start` builds the answer side as an
  unbounded `mpsc::channel`, and `turn_away` pushes to an unbounded `Vec`; a
  consumer that does not drain holds every answer (40 of 40 in the suite).
  Owed: a declared bound on answers held, and what a full mailbox does.
- **D-72 · font.** `claim_scan` folds every request made during a walk into
  one `again` round; the requests between are answered by nothing of their own.
  A walk that ends without `finish_scan` leaves `running` set, so every later
  request only sets `again`, and no walk ever runs again; the picker keeps its
  last list and nothing reports it. `MONOSPACE_WAKE` is called and has no
  failure road. Owed: a superseded outcome, a fault the slot can report, and a
  running flag a dead walk cannot leave behind. The target stays the
  application-wide slot.
- **D-73 · taskbar.** `TaskbarLane::serve` answers the newest request
  standing; the ones between are answered by nothing of their own. A worker that
  ends leaves `Asks::worker` set, so `ask_locked` only signals a condition
  variable nobody waits on; the reading stays at its last answer for the rest of
  the run. Owed: as D-72.
- **D-74 · computation, bounds.** `MathWorker::spawn` makes all four channels
  with `mpsc::channel`. Owed: declared bounds for admission and answers, per
  job, and what each job does when full (the per-session queue's drop-oldest is
  the existing policy one level up).
- **D-75 · computation, identity.** The lane mints nothing; an answer carries
  its question (the task, a path, a key), and the same question asked twice is
  two answers the consumer cannot tell apart. Owed: a request id and a lane
  incarnation, or a ruling that per-question identity is this lane's contract
  (the note's `PerQuestion` replacement) with the duplicate case stated.
- **D-76 · computation, death.** The three threads share one answer sender
  (`scale_result_tx` and `path_result_tx` are clones), so `drain_math_answers`
  sees `Disconnected` — and `disable_math_worker_state` raises its notice —
  only when all three have ended; a dead decoration thread's queued questions
  are lost silently while the other two live. Owed: a death signal per thread,
  or one lane per thread (D-2's "three jobs share one result type").

## D-77 — the row added on 2026-09-26 by A1a

- **D-77 · the first window's GPU is a wait no row listed.** `Runtime::create`
  (`main.rs`, from `FolioApp::resumed`) opens the first window's adapter, device and
  surface with `pollster::block_on(GpuContext::open(…))`, on the window thread.
  `pollster::block_on` is in the budget note's vocabulary, and the call was on no
  row of §5.3, whose own rule makes that a defect. The thread-door note's revision
  (e)2 found it while completing the door inventory, and A1a registers it as row 23
  with status `pending` and door `GpuOpen` (DESIGN, 2026-09-26, *every thread that runs Folio's code has a role, the window thread has a phase, and each owner-thread wait is a door the registry lists*).
  **It stays where it is for now** (coordinator, 2026-09-26): A1d admits it on the
  window thread, and it moves when device recovery rebuilds on a worker (B9,
  D-42), which is the same request run for the first device. Owed: the first
  window's device asked for off the window thread, with the window shown only when
  it lands.

## D-78…D-82 — the rows added on 2026-09-27 by A1e

The `Drop`s that may reach the window-thread vocabulary, from the thread-door
note's closed inventory ((c)4, (e)3, (f)2, (g)). Each is held body by body by
`hang_watch::window_waits_tests::every_door_is_where_the_registry_says`: its
first-party edges in order and its effects by call site, so a new wait under a
row is red. None is ruled to stay: each wait can reach a thread that must not
wait on some road, and none has a ruling that it may.

- **D-78 · the trace writer's drop.** `trace_sink::Shutdown::drop` admits the
  `TraceFlush` door and flushes: a polled close, a bounded `recv_timeout`, a
  polled finish and a join. Reached only on `fn main`'s early return from a loop
  that could not be built, which says `exiting()` first. Owed: the writer retired
  through its admitted flush door, never by a drop.
- **D-79 · the endpoints' drops.** `AttentionPipe` and `LaunchPipe`, on both
  platforms, signal their listener and join it in `Drop`. No product reach today —
  the endpoints live in statics for the process's life — but a public-API and a
  test destruction path exist. Owed: an explicit retirement door.
- **D-80 · the video engines' drops.** `video::engine::Engine` and
  `macos_player::Engine` shut down in `Drop` (a 2 ms poll under `SHUTDOWN_BUDGET`,
  then a join); `VideoSeat` and `VideoSeats` reach the same `shutdown` from their
  own drops, on the window thread when a pane or a window closes. Owed: an explicit
  shutdown door, repaying both layers together.
- **D-81 · the shell's drop.** `PtySession::drop` finishes the input dump (one
  write, two `sync_data`) and then runs `shutdown` (a bounded reap and a bounded
  join). It runs on `pty-retirement`, and on the caller only when that thread
  cannot be started. Owed: a shell taken apart only through `retire_within`, with
  the thread-refused road still tearing it down.
- **D-82 · a download's request.** WinHTTP's `http::Request::drop` (U-7) closes
  its handle and waits up to `CLOSE_WAIT` on a `Condvar` for the closing callback.
  `https_download` has no product caller yet; the update's download will call it
  on a worker. Found by A1e's check, not by a review (the note's (g)2). Owed: the
  request closed through its own bounded door, not by its drop.
## D-83 — the row added on 2026-09-26 by ticket 72

- **D-83 · the `bt-app` and `bt-term` suites assert Windows facts on every
  host.** Measured on the owner's Mac mini at `a7939601` (`cargo test -p <crate>
  --no-fail-fast`): `bt-app`'s bin suite ran 4,600 tests, 4,298 passed and 294
  failed; ticket 72 made the twelve `uninstall::tests` among them pass (their
  sandbox sat under `$TMPDIR`, and `/var` is a link), which leaves 282. The rest
  are fixtures written as Windows facts: drive-letter and `\`-joined paths
  (`profiles` 82, the flat `tests` module 51, `shell_integration` 43,
  `preview` 14, `printed_path_provenance_tests` 10, `launch_wire`, `cli`,
  `git_graph`, `file_peek`), Ctrl where macOS reads Cmd (`shortcuts` 34,
  `input` 6, `preview_edit`), Windows-only rows and folders (`webhost` 12,
  `settings` 7, `explorer_menu`), and the environment-named config folders of
  the three agent installers. `bt-term`'s suite has 43 of the same kind (12 of
  `inline_image::tests`' 47, 22 of `session::tests`' 347, and 9 in the
  `notifications`, `shell_integration_bash`, `shell_integration_cmd`,
  `shell_integration_script` and `shell_integration_wsl` binaries). Neither is
  a product defect by itself — the webnav test D-61 repaired was the same
  shape — but until they pass, `core-macos` cannot test either crate, and a
  macOS-only regression in them is found by a person. Owed: each test asks the
  question its claim is about on the host it runs on, or states its platform
  with a gate, the D-61 way; then the two crates join `core-macos`'s test line.
  0.4.7, proposed by ticket 72.

## D-84 — the row added on 2026-09-27 by A4

- **D-84 · aggregate turn scheduling beyond deferrable work.** A4 gave the window
  thread's turn one shared deadline, `TurnAllowance` (`hang_watch::accounting`):
  the earliest next frame boundary of the windows on the glass whose `FrameClock`
  is running, less `PRESENT_RESERVE` (2 ms), else `TURN_BUDGET` (16 ms) from the
  turn's start. The search walk's slice and the idle calls ask it and yield; that
  is the whole of what is scheduled. Still open, each named so the next ticket
  can take it: **per-window fairness** — the deadline is one number, so a window
  walking a search yields to another window's frame and to every other window's
  work in the same turn, and no window has an allowance of its own; **the phase
  of several animating windows** — only the earliest boundary is read, so a second
  window on another phase can miss its frame with nothing recorded against it;
  **the scheduling delay by source** — A3 measures it against the loop's own
  `WaitUntil`, the fold of every obligation, and does not say whether the wake was
  owed to input, a frame clock or a timer; **a ruled bound per registry row** —
  every row's per-call bound is `WAIT_ALLOWANCE` (4 ms) until the registry has a
  bound column, its generator and its guard; and **the non-deferrable terms** of
  §R-B's inequality — events, the drain's fixed `DRAIN_TURN_BUDGET` (at 120 Hz it
  consumes the frame; a finding, not a scaling rule), preparation and admitted
  waits — which are observed by A3 and scheduled by nothing. Owed: a 0.4.7 ticket,
  *The window thread's turn is scheduled across windows and sources*, drafted
  after B4–B9 have moved their waits. Opened by A4 per §R-G; by the owner's ruling
  of 2026-09-25 it is one of the two rows D-2 leaves behind when it closes.

## D-85 — the row added on 2026-10-08 by T-PROGRAMS-REFRESH

- **D-85 · program walk.** `ProgramsLane::serve` answers the newest request
  standing; a request replaced before its walk started is answered by the walk
  that served its successor, which a request made later always is — nothing is
  lost, but no request has an outcome of its own, which is D-72's and D-73's
  shape. Unlike them, a walk that unwinds is observable (`WalkOut` marks the
  worker gone and reports the walk that died, and the next request starts a new
  worker), so only the outcome half is owed: a "superseded by walk N" outcome the
  drain can report once per request, if a consumer ever needs one.
