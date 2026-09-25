# A spare web controller for the first page — design note, 2026-09-25

Ticket 60 (0.4.5), D-64, `ARCHITECTURE.md` §5.3 row 21. No code; anchors by name only.

**Rulings honoured.** 2026-09-24: warm the engine at a quiet moment; the controller is per page;
no WebView2 thread (§5.2 unchanged). 2026-09-25: option A, gated on a persisted "has used web panes
before" flag; the first pane adopts the spare. 2026-09-25: two-day window, else 0.4.6. No new UI
string.

**Evidence (spike 59, clean VM, 2 vCPU, runtime 153.0.4234.48).** First page: cold median 2318 ms
(n = 8), new controller over a live runtime 608 ms, spare handed over by `WebHost::rehost` then
navigated **146 ms** (n = 11; rehost 11–73 ms, then no hold over 0.6 ms). `put_ParentWindow`
between two hidden windows: 11/11 `Moved`. Making the spare: 2.1 s wall (1.3–4.2), 375 ms busy,
longest hold median 202 ms, **max 590 ms** (the synchronous
`CreateCoreWebView2CompositionController` on the first run after boot). Idle price: 6 processes,
**91 MB private** (87–94), ~250 MB working set with shared pages counted twice. The browser exits
0.5–17 s after its last controller closes, so the runtime is up exactly as long as a controller
lives.

## 1. The flag

**Where.** `settings.json`, as a fourth *receipt about this machine* beside `first_run_card`,
`powershell_install_pending` and `cards_gesture_hint_offer` (RULES row 33; `settings_bundle`'s
module header): `SettingsV1::web_pages_used: WebPagesUsedV1 { Never, Used }`, two values and not a
`bool` for `FirstRunCardV1`'s reason, schema **v38 → v39**, the migration writing `Never` (the
v13–v16 way: nothing in an old file says whether a page was ever opened, and the first page after
the upgrade writes the truth). Not `session.json`: that file is the photograph of windows and a
process's own statics, rewritten from scratch on every save, and "this profile has opened a page" is
neither. Not the marks record (`integration-marks.json`): that is the ledger of writes *outside*
`%APPDATA%\Folio`, taken under the marks lock. The store is `persist::SettingsStore`, the write its
ordinary undebounced `store` (the same one `note_card_hint` spends its receipt with).

**Never a UI setting.** No `SettingsRow`, no `visible_rows` entry, no Export/Import effect:
`settings_bundle::plan_settings` binds it `web_pages_used: _` with the other three receipts, and
the RULES row 33 sentence becomes "four keys".

**Who writes it.** One door: the `WebOutcome::Committed` arm of `Runtime::apply_web_outcomes` calls
`App::note_a_web_page_committed`, which stores `Used` when the loaded value is `Never` and does
nothing otherwise. `Committed` is emitted only when `WebMachine::recoverable_url` moves, i.e. on a
successful non-blank top-level navigation, so a refused address, an error page and the spare's own
`about:blank` never write it. In practice it is written once per profile.

**Who reads it.** Only the warm-up clock's spare stage (§2), live from `settings_store.loaded()`. A
scratch profile has no `settings.json`, so the flag is `Never`: tests, the VM smoke and every
first launch make no spare.

## 2. The spare's lifecycle

**The state** is a portable generic beside `EnvironmentSlot`, in
`bt-platform/src/web_environment.rs`: `SpareSlot<S>` with `SparePhase { None, Creating, Ready,
Adopted, Retired }`, tested on every platform. **One instance, one owner:** `App::web_spare:
SpareSlot<web_spare::Spare>` on the window thread (the `App` is the process's), where
`bt-app/src/web_spare.rs`'s `Spare` is `{ seat: webhost::WebSeat, parent: bt_platform::SpareParent
}`. `SpareParent` (Windows) is the never-shown `WS_POPUP`, `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`
window of the spike, promoted from `webview.rs`'s test-only `HiddenWindow`, plus its own
`Compositor`; it destroys the window on drop. `bt_platform::spare_parent()` answers `None` on macOS
(WKWebView is made on the spot) and on the portable arm, so there the stage is `NothingToWarm`.

| phase | entered by | holds | leaves by |
|---|---|---|---|
| `None` | process start | nothing | the clock's spare stage → `Creating` |
| `Creating` | `SpareParent` made, `WebSeat::open(page = the parent's own PageVisual, url = BLANK_PAGE, Mint::Blank, scale and scheme of the window that turns the clocks)` | seat + parent | install acknowledged → `Ready`; any retirement cause → `Retired` |
| `Ready` | the seat's `InstallEvents` acknowledged (its `about:blank` navigation issued) | seat + parent | `take()` by the first page → `Adopted`; any retirement cause → `Retired` |
| `Adopted` | §3 | nothing (the seat is the pane's, the parent is dropped) | terminal |
| `Retired` | `retire()` | the closing seat + parent, until the seat answers `Gone` | terminal; the teardown drops both |

**Made** by the existing clock, not a second one. `web_warmup::WebWarmup`'s `done: bool` becomes a
stage `Environment → Spare → Done`. The spare stage fires on the first turn that meets ticket 54's
conditions (grace after the first frame, `WEB_ENGINE_WARMUP_QUIET` since the last stir, restore card
down) **counted afresh from the environment turn**, so the two never share a turn, and also:
`web_pages_used == Used`, and no window's `web` map holds a page. A spare stage that finds the
flag `Never` or a page open goes to `Done`: **at most one spare per process, attempted once, never
replenished.** `EngineDoor` gains `make_spare`, so the clock is still driven headless. The seat is
built by the same `WebSeat::open` every pane uses, so its gates, guards, recovery machine and
install burst are the pane's own; `request_environment` is answered on the spot (the environment is
`Ready` from the warm-up) or joins the call in flight.

**Driven** in `FolioApp::user_event`'s `WebPageSpoke` arm after the windows' pages
(`app.web_spare.drive()`), and ticked in the application-clocks window's turn for its deadlines
(`ENGINE_START_DEADLINE`, `BROWSER_EXIT_DEADLINE`), one more wake owner, `"spare web controller"`
(`DEADLINE_OWNERS` 52 → 53). Its outcomes reach no window: `Fault` is one `diagnostics::note` line
and a retirement; nothing else it can say has a reader.

**Retired** — one predicate, `web_spare::must_retire`, read after every drive: a `Fault`, or
`present_state()` answering a generation other than the one it became ready at, a state other than
`Ready`, or no controller. That covers the engine failing to start, the browser process failing or
exiting (the seat's own machine would rebuild; a spare never rebuilds), a new browser version (the
old browser cannot exit while any controller over it lives — the pages' rebuild would otherwise
wait out `BROWSER_EXIT_DEADLINE` behind the spare), and `forget_web_environment` from another seat's
rebuild (`holds_the_process_environment` then refuses the handoff, §3). **GPU device loss does not
retire it**: the spare has no wgpu surface, its `Compositor` is built on a null rendering device,
and a pane's page survives `recovered_from_a_lost_device` the same way (Q2). **DPI change** does
nothing to it: `SetShouldDetectMonitorScaleChanges(false)` makes the window the authority, and
adoption hands it the adopting window's scale. **Window close** of a window that is not the last:
nothing. **The last window and quit:** retired with the run, in `FolioApp::close`'s `ending` branch
beside `retire_the_summon_with_the_run` and in the quit transaction's `Retire` step. Retirement is
`WebSeat::close` on the parent's compositor, so the seat walks §7.35's road — controller closed,
`AwaitBrowserExitBeforeCleanup`, `Gone` on `BrowserProcessExited` or the deadline — and **the run
may not end until the spare has let go**: `FolioApp::every_page_has_gone` and the empty-registry
exit in `reap_leaving_windows` both ask `app.web_spare.has_let_go(now)`, bounded by
`quit::PAGE_TEARDOWN_DEADLINE`. §7.35 measured why this is not optional: an `HWND` with the
engine's child window under it cannot be destroyed by a thread that stopped pumping, and the
process rundown hangs. A spare that was never adopted therefore keeps a run whose windows are gone
alive for the browser's exit (0.5–17 s measured, bounded at 12 s) — invisible, and the same wait
every closed page already costs.

## 3. Adoption

**The step.** `Runtime::open_web_page_on`, the arm where `self.window.web` has no seat for the leaf —
today the one `webhost::WebSeat::open(` call. It becomes: `match self.app.web_spare.take()`
(`Ready → Adopted`, answering the `Spare`); `Some` → `Runtime::adopt_spare_web_page`, `None` → the
existing `WebSeat::open` road, unchanged. That is where a pane "would call `request_controller`":
its state machine never reaches `EnvironmentPending`, so it never produces `CreateController` or
`InstallEvents`. A second pane finds the slot `Adopted` and builds its own controller as today.

**What adoption does, in order**, under one new station `WebAdopt`
(`"adopt_spare_web_controller"`):
1. `WebSeat::rehost(from = spare.parent's compositor, to = &self.window.compositor, SeatAddress {
   page = the pane's PageVisual, window = this window }, take_focus = false, &mut outcomes)` — the
   tear-out walk exactly (`WebHost::rehost`: hide, clear target, commit source, `put_ParentWindow`,
   set target, commit target, bounds, presence, notify), ending in the seat's private `adopt` →
   `take_address`, which clears `presence`, `bounded`, `rastered` and `placed`. `Moved`: the parent is dropped (no controller is
   parented to it any more; the VM smoke checks `IsWindow` is false and the drop holds nothing).
   `KeptSource`: the spare goes to `Retired` and this pane takes the `WebSeat::open` road — the
   ordinary answer to a door that refused, not a second policy. `Lost`: the seat is already the
   pane's and rebuilds in the target through `WebMachine::on_rehost_lost`, as a tear-out does; the
   parent waits out the closed controller's browser-exit wait before it is dropped.
2. The per-page settings a spare cannot know: `set_device_scale(this window's scale)` and
   `set_color_scheme(web_color_scheme_in_force())` (a no-op when unchanged; the spare was told the
   scheme at birth and a theme flip since then walked only the windows' seats). Everything else the
   install burst set is per controller and identical for every seat (`WEB_SETTINGS`, raw-pixel
   bounds mode, detection off, every event), so it is **not** re-applied.
3. The seat is inserted into `self.window.web` (the `a_pane_with_no_engine_has_one_built_for_it`
   line) and placed by the one placement writer, `Runtime::sync_web_page`.
4. **Then** `WebSeat::go(url, minted, compositor)`. A new machine entry `WebMachine::adopt(url,
   sized)` keeps the rule `InstallEvents` keeps ("the engine is given its size before its URL",
   `WebSeat::wanted`): it answers `Navigate` when the seat already has bounds, and otherwise records
   `desired_url` and navigates from `apply_presence` on the first bounds — never against zero by
   zero.

**The link filter and the allowed-file state.** What is fixed at creation is the *registration*:
`attach_events` adds the `WebResourceRequested` filter over every context and
`FrameNavigationStarting`, and `WebHost::new` captures the two gate closures. What they *consult* is
the seat's `mint: Rc<RefCell<Mint>>`, read per request. Because the spare is a `WebSeat` and the
whole seat moves into the pane, the adopting pane's allowed-file state **is** that cell: the
`Navigate` arm writes `*self.mint.borrow_mut() = minted` (and `set_request_rules`, a no-op on
Windows) before `navigate`, as for every page. `guards` travel with the seat from the spare's
install report, so `WebSeat::issue` still refuses a local file on a controller that lacks one.
Nothing is re-applied. One fix rides along: the gates capture `page` for their `BT_WEB_TRACE`
label, which after adoption would name the parent's seat; the label becomes a cell written by
`take_address`, which also corrects it after a tear-out.

**What adoption leaves for the stall self-report.** The first page's gesture turn names
`adopt_spare_web_controller` (the rehost walk, 11–73 ms in the VM) instead of `request_environment`,
`request_controller` and a pump dispatch; its navigation is `WebHost::navigate` as today. One
`BT_WEB_TRACE` line, `adopt <seat> from=spare outcome=<Moved|KeptSource|Lost>`. The slot stays
`Adopted` for the process, so a later reader can tell "the first page had a spare" from "it had
none".

## 4. Cost and bounds

The creation is split across turns wherever the API has a seam; the one call it cannot split is
WebView2's own synchronous controller call.

| turn | work | measured hold | station |
|---|---|---|---|
| A (quiet) | environment ask, ticket 54 | 8.5–39 ms | `warm_web_engine` |
| B (quiet again, ≥ 1 s after A; flag; no page) | `SpareParent` (window + `Compositor::new`) and `WebSeat::open` | not measured; its station measures it | `make_spare_web_controller` (new, `WebSpare`) |
| C (the spoke B's answer raises) | `request_controller` | 2–4 ms typical, **590 ms worst** (first run after boot) | `make_spare_web_controller` › `request_controller` |
| — | the engine's own dispatches | median longest 202 ms | `message pump` (the engine's) |
| D (controller spoke) | install burst, `about:blank` navigate | 2–6 ms | › `WebHost::install`, `WebHost::navigate` |

C is not re-gated on quiet: the environment answer arrives before its call returns (ticket 54), so
C follows B within the same quiet stretch; a gate would add a state for no measured gain. A
keystroke that lands during C waits for it — that is the price of the ruling, paid once per process
by flag holders only, on a turn that followed at least a second of stillness. The idle price is
**~91 MB private in six processes, paid only by profiles that have opened a page**, for as long as
the spare waits. The two-builds caveat (ticket 40, `0x8007139F`) is unchanged: a second build with
other options gets no environment, so its spare retires with one diagnostics line and its pages
fail exactly as before.

## 5. Tests red on BASE (headless; the real controller only in the VM smoke)

In the repo's shape (`/// RED (60) — **claim.**`, why, `/// MUTATION:`), with recorded requests
through the real `EnvironmentSlot`/`SpareSlot` and a recording `EngineDoor`:
1. `a_profile_that_has_never_opened_a_page_gets_no_spare` — the clock asks for the environment
   once and never for a spare. MUTATION: drop the flag condition.
2. `the_spare_is_made_once_on_a_quiet_turn_after_the_environment_turn` — not on turn A, a stir
   between A and B delays it by the quiet stretch, never twice. MUTATION: fire both stages on one
   turn; drop the once rule.
3. `a_spare_is_not_made_while_a_page_is_open` — MUTATION: drop the "no page" input.
4. `the_first_page_takes_the_ready_spare_and_the_second_finds_none` (`SpareSlot`) — and nothing is
   taken while `Creating`. MUTATION: `take` leaves `Ready`.
5. `an_adopted_controller_navigates_without_asking_for_one` (`WebMachine::adopt`) — the effects
   are one `Navigate`; no `CreateController`, no `InstallEvents`. MUTATION: route `adopt` through
   `request`.
6. `an_adopted_page_is_given_its_size_before_its_address` — unsized: `Ignore`, then `Navigate` on
   the first bounds. MUTATION: navigate at once.
7. `an_engine_that_goes_away_retires_the_spare` (`must_retire`) over the recorded states: fault,
   generation moved, not `Ready`, no controller; and `retire` from `Creating`/`Ready` hands the
   seat back, after which `take` answers `None`. MUTATION: ignore the generation.
8. `the_run_waits_for_the_spare_to_let_go` — `has_let_go` false while `Retired` and not `Gone`,
   true at `Gone` or at the bound. MUTATION: answer true on `Retired`.
9. `the_first_committed_page_writes_the_receipt_once` — a real `SettingsStore` in a temp folder,
   two commits, one write, `Used` on disk. MUTATION: write on every commit. With it
   `the_receipt_is_no_row_and_no_import_moves_it` (`plan_settings` and `visible_rows`) and the
   v38 → v39 migration test.
10. Source guards through `bt_source`: `open_web_page_on` takes the spare only in the no-seat arm;
    `every_page_has_gone` and the empty-registry exit ask `web_spare`. The two station labels join
    the web-phase vocabulary test.

## 6. Docs and ledger

- **CHANGELOG**, Unreleased / Changed: *"Once you have opened a web page in Folio, the first one you
  open after launch appears in a fraction of a second: Folio gets it ready while it is idle."*
- **DESIGN**: `### 2026-09-2x — The first web pane adopts a spare controller made on a quiet turn,
  for profiles that have opened a page before` — the flag and where it lives, the lifecycle table,
  the adoption order, the split and its numbers, the idle price, the §7.35 teardown.
- **ARCHITECTURE §5.3 row 21** → **narrowed by ticket 60**: for a profile with the flag, the first
  page's window-thread cost is the rehost walk (11–73 ms VM) and a navigate; the controller call
  (≤ 590 ms) moves to a quiet turn under `make_spare_web_controller`; a profile's first-ever page
  and every later page still pay the per-page controller (608 ms median warm, VM). §6 gains a row:
  *creating a native window outside the framework* — `bt_platform::SpareParent`, the one
  `CreateWindowExW` in product code, held by a source guard.
- **D-64** → narrowed, with the numbers above (2318 → 146 ms median for flag holders); status per Q1.
- **RULES**: row 33 (four receipts), row 31 (v39), row 43 (the spare is retired with the run and
  held by the same browser-exit wait), row 49 (a trailing entry for the 2026-09-25 ruling).

## 7. Architecture impact

- **(a) facts touched.** New: the spare's lifecycle, owner `App::web_spare` (`SpareSlot`), window
  thread, one writer road (clock → `Creating` → `Ready` → `take`/`retire`). A seat's controller
  (owner `WebSeat`/`WebHost`): a second source, see (c′). `settings.json`'s new receipt, owner
  `SettingsStore`, one writer (`note_a_web_page_committed`). The process's `EnvironmentSlot`: one
  more asker, the same `ask`. The run's end (`every_page_has_gone`, `reap_leaving_windows`) reads
  the spare. `hang_watch`: `WebSpare = 208`, `WebAdopt = 209`, `STATION_COUNT` 208 → 210. The wake
  fold: 52 → 53 owners.
- **(b) doors.** WebView2's environment, controller, install, navigate and rehost calls on the
  window thread, no door (§5.2, as today). One new kind of effect — a native window created outside
  winit — gets its door first: `bt_platform::SpareParent`, listed in §6. One `settings.json` write
  through `SettingsStore::store` (§5.3 row 20's class, once per profile). `diagnostics::note`
  lines. No thread, child process, file read or hand-off.
- **(c) debt.** D-64 narrowed (§5.3 row 21). None added; `MIGRATION-DEBT.tsv` unchanged.
- **(c′) new trigger sources.** A seat's controller gains a second source, adoption, beside its
  own `request_controller`. Readers that assumed a controller was made for the seat's window:
  `holds_the_process_environment` (holds: one environment); the `WebHost::new` gate closures'
  trace label (fixed, §3); `the_controller_has_been_told_nothing`'s caches (cleared by
  `take_address`); `tell_web_pages_their_color_scheme`, which walks windows only (answered at
  adoption); the new-version rebuild's wait, which assumed every controller over the old browser
  is a page's (the spare closes on the same event); the §7.35 teardown and the quit wait, which
  counted only windows' pages (now the spare too); `ENGINE_START_DEADLINE`'s card, which a spare
  has no pane to draw (its fault is a line and a retirement). The environment gains a third asker
  whose failure road is the seat's own.
- **(d) ownership change: no.** No existing fact moves or splits. A controller moves whole from one
  owner to another through the road tear-out already uses between two windows' `web` maps; the
  spare is a new fact born with one owner.

## 8. Open questions for the owner

- **Q1.** D-64 after ticket 60: close it with the residual accepted (first-ever page and later
  pages ~0.6–2.3 s, a wait with holds ≤ ~0.2 s), or keep it open for 0.4.6 (replenish the spare
  after each "no pages open" period)?
- **Q2.** GPU device loss does not retire the spare (it has no wgpu surface; pages survive it too) —
  agreed, rather than retiring it on every loss?
- **Q3.** An upgrading profile starts at `Never` and pays one cold first page before the flag is
  written — or should the migration read `Used` from a `session.json`/Recent that holds a web page?

---

## Appendix — ticket 60 brief

# 60 — The first web pane adopts a spare controller made on a quiet turn (0.4.5, D-64)

| | |
|---|---|
| **target version** | **0.4.5**, two-day window (owner 2026-09-25): not CI-green by 2026-09-27 → 0.4.6, then it stays on its branch until the 0.4.5 tag (standing rules, release window) |
| branch / worktree | `feat/spare-web-controller` · `D:\Developer\bt-wt\spare-web` |
| size | M |
| who | **Opus** |
| lane | local compile lane (name-filtered tests); the real controller only in the VM smoke; macOS arm CI-checked |
| shape | one commit, mergeable alone, CI green |
| depends on | 43, 40, 54 (merged); spike 59 (`680b3093`, evidence only). BASE = the coordinator's sha at dispatch |
| repays or narrows | D-64 = §5.3 row 21 |

**Read first.** This note in full; `reports/59.md`, `54.md`, `43.md`; §7.35 of `docs/DESIGN.md`.
Anchors (rg on BASE, symbols only): `web_warmup::{WebWarmup, EngineDoor, ThisProcess}`,
`Runtime::warm_web_engine`, `bt_platform::{EnvironmentSlot, warm_web_environment, WebHost::{new,
request_controller, install, rehost}}`, `webhost::{WebMachine, WebSeat::{open, go, rehost, take_address,
set_device_scale, set_color_scheme, close, tick, present_state}}`,
`Runtime::{open_web_page_on, apply_web_outcomes, sync_web_page}`, `FolioApp::{close,
every_page_has_gone, reap_leaving_windows, user_event}`, `quit::PAGE_TEARDOWN_DEADLINE`,
`settings_bundle::plan_settings`, `SETTINGS_MIGRATIONS`, `webview.rs`'s test `HiddenWindow`.

**Goal.** §1–§4 of this note, as written. No new UI string, no new setting row, no notice.

**Tests.** §5, each red on BASE and red again with only its product line reverted; name filters
`web`, `webhost`, `warm`, `spare`, `environment`, `station`, `stall`, `settings`, `quit`, with the
count each selected.

**VM smoke (the real controller).** Clean VM, scratch profile. Launch 1: open a local page, check
`web_pages_used` is `Used`, quit. Launch 2: wait 12 s, count `msedgewebview2` (the VM's six system
ones excluded by creation time), open the same page, read the stall/`BT_WEB_TRACE` lines
(`adopt … outcome=Moved`, no `request_controller` on the gesture turn), time the first page, check
the parent window is gone, open a second page (it requests its own controller), close the last
window and time the process's exit (≤ `PAGE_TEARDOWN_DEADLINE`). Launch 3 with the flag set, close
before any page: the process exits within the bound and leaves no `msedgewebview2` of its own.

**Docs in the same commit.** §6.

**Architecture impact.** §7; the report restates it as built.

**Standing rules.** `_standing-rules.md` in full.

**Report.** `reports\60.md`: anchors that moved, the stations' measured holds on the VM (turn B
especially), the first-page times, gate tails, the red/green table, the head sha.

---

## Revision 2026-09-25 (b), after the Codex review

Review: `spare-web-controller-review-codex-2026-09-25.md` (Codex, static, against `bb473f4c`):
**adopt with changes**. All three P1 findings and the four P2 findings are **adopted**, and none
is refuted. I re-read the code for each one on this branch, and what I found is cited below. This
section supersedes the parts of §1–§8 and of the appendix that it names. Those parts stay above
unedited, because this file only ever grows.

**The coordinator's answers to §8 are now constraints:**
- Q1: D-64 stays **open, narrowed, for 0.4.6**.
- Q2: a GPU (wgpu) device loss on its own **does not** retire the spare.
- Q3: the v39 upgrade **infers `Used`** from pages in the saved session (SW-4).

### Verified in code before folding
- **Nothing is woken once the registry is empty.** `FolioApp::about_to_wait_inner` handles an
  empty registry (after `reap_leaving_windows`) with `ControlFlow::Wait` and returns. The quit's
  retirement branch returns before any window turns. `FolioApp::advance_retirement` ticks only
  each window's own `web` map (SW-2).
- **A closed seat cannot count on an exit notification.** `WebHost::close` walks
  `CloseStep::EnvironmentEvents`, which removes `BrowserProcessExited` and
  `NewBrowserVersionAvailable`. A closing seat therefore ends on `BROWSER_EXIT_DEADLINE` through
  `WebSeat::tick` unless an event was already queued (SW-2).
- **Recovery runs before the proposed predicate could look.** `WebMachine::on_browser_process_failed`,
  `on_browser_process_exited` (under a live seat) and `on_new_browser_version_available` bump the
  generation and return `RebuildFromScratch` / `RebuildForNewVersion`. `WebSeat::drive` applies
  them inside the same call, and the first of these runs `forget_web_environment` and
  `start_environment` (SW-1).
- **An existing leak, also reachable from pages.** When `close_pending_controller` runs before the
  controller callback has arrived, it drops the holder slot. The callback still holds a clone of
  it, stores its controller there, and pushes `Controller`. The seat, now `Closing`, answers
  `CloseOrphanController`. That calls `close_pending_controller` again, finds nothing, and the
  controller is dropped without `Close()`. A spare retired while `Creating` makes this common, so
  the fix is part of ticket 60 (SW-1).
- **`fail` closes and drops at once.** `FolioApp::fail` runs `close_window(true)` for every window
  and then `windows.clear()`, `App::finish` and `event_loop.exit()`, with no wait. That is how pages
  are treated today on this path (SW-2).
- **Where the saved-page evidence lives.** `persist::SessionStore::open` runs before
  `SettingsStore::open` in `FolioApp::create`. Settings migrations are `MigrationStep = fn(Value) ->
  Value`. A page is recorded, typed, in four places: `PreviewPaneV1::cur_source ==
  PreviewSourceV1::Url`, `PreviewPoolEntryV1::source == Url`, `RecentSeedV1::Preview { source: Url,
  .. }`, and `RecentPreviewV1::Page` (SW-4).

### SW-1 — retirement knows the phase and takes the event before recovery does (P1, adopted)

**1. `WebSeat` gets a recovery policy.** `RecoveryPolicy { Page, Parked }` is set by `open`: `Page`
for panes, `Parked` for the spare. It is switched to `Page` by adoption and never switched back.
One pure function sits between `digest` and `apply` in `WebSeat::drive`:
- `recovery_under(Parked, effect)` maps `RebuildFromScratch`, `RebuildForNewVersion` and `Reload`
  to **`Retire`**. It maps a transition into `Failed` (the engine did not start, or the controller
  did not arrive) to `Retire` as well.
- `Retire` runs `machine.close()`'s `AwaitBrowserExitBeforeCleanup` through the existing `step`,
  plus one `WebOutcome::Retired(reason)`.
- `recovery_under(Page, e)` is the identity.

**So a parked seat never forgets the environment, never asks for it again, and never rebuilds.** It
no longer matters whether the pages or the spare hear a shared browser event first. The executor
(`step`) stays the one shared implementation.

**2. What "normal" means in each phase.** While `Creating`, the normal states are
`EnvironmentPending`, then `ControllerPending`, then `Ready`. The spare retires only on a
`Retired(..)` from the policy, on `ENGINE_START_DEADLINE`, or on the owner's own causes (items 3
and 4). The §2 predicate "any state other than `Ready`" is **withdrawn**.

**3. The environment's epoch is checked where the spare is owned.** `EnvironmentSlot` gains `epoch:
u64`, bumped by every `forget` and by every `arrived(Ok)`, and read through
`bt_platform::web_environment_epoch()` (`0` where there is no process-wide environment). The spare
records the epoch its controller was made under. Each advance of the spare compares it: a different
epoch means another seat's rebuild forgot the environment, so the spare retires. `take` compares it
again (item 5).

**4. Retiring is idempotent, and late controller answers are closed.**
- `SpareSlot::retire` from `Retiring`, `Adopted` or `Retired` is a no-op.
- `WebHost::close_pending_controller` keeps a holder that has not been answered in
  `orphans: Vec<(u64, holder)>`. The `Controller` completion for an orphan's generation calls
  `Close()` on whatever it delivered, instead of dropping it.
- A retiring seat keeps being advanced until its orphans are empty or its deadline passes. No late
  completion can install anything or bring the spare back.
- This also fixes the pages' form of the same leak. It is recorded as a finding and repaid in the
  same commit.

**5. Taking the spare re-checks that it is still fit.** `SpareSlot::begin_adoption` (which
replaces §2's `take`) succeeds only when all of these hold:
- the phase is `Parked`;
- the seat is `Ready` with a controller and `RecoveryPolicy::Parked`;
- the epoch is unchanged;
- the blank page has landed (SW-5).

Otherwise it answers `None` **and** moves the spare to `Retiring`.

### SW-2 — an exit clock owned by the application (P1, adopted)

- **`App::web_spare.advance(now) -> Option<Instant>`**, in this order:
  1. Drain the retiring or parked seat (`WebSeat::drive` on its parent's compositor).
  2. Run `WebSeat::tick(now, …)`.
  3. When the seat answers `Gone` (or its orphans are closed and its deadline has passed), drop
     the seat and then the parent.
  4. Answer the next deadline.

  **No step waits for a notification.** The seat's own `BROWSER_EXIT_DEADLINE`, set by
  `AwaitBrowserExitBeforeCleanup`, is what ends the wait (see "Verified in code").
- **It is called from three places:**
  - the turn of the window that runs the application clocks (while windows are open);
  - `FolioApp::advance_retirement` (the quit branch), with its deadline folded into `waking`;
  - `FolioApp::about_to_wait_inner`'s empty-registry arm, before choosing the control flow. That
    arm sets `ControlFlow::WaitUntil(spare deadline)` in place of `Wait` whenever the spare has
    not let go.
- **The run may end only when the spare has let go.** `reap_leaving_windows`' exit check becomes
  `a_run_ends_with_its_last_visible_window(len) && app.web_spare.has_let_go(now)`, and
  `every_page_has_gone` asks the same thing.
- **One absolute bound per run.** `App::run_retiring_until` is set **once**, when retirement starts
  (in `FolioApp::close`'s `ending` branch, or in the quit's `Retire` step), to `now +
  quit::PAGE_TEARDOWN_DEADLINE`. Polling never restarts it.
  - Pages and the spare retire **at the same moment**: the spare's `retire()` is called in the
    same branch that closes the last window's pages.
  - At the bound, the spare takes the abandonment road: one `diagnostics::note` line (`the spare
    web page's browser outlived the run's bound`), controller references dropped, and the HWND left
    to process exit through the existing `leave_process`.
  - `has_let_go` is true after that. The spike measured browser tails of up to 17 s, so the note
    promises **a bounded lifecycle, not an absence of browser processes at the bound**. Any tail is
    recorded separately.
- **An orderly failure (`FolioApp::fail`) and `exiting` do not wait.** The spare is handled as
  `fail` already handles pages: `app.web_spare.abandon()` calls `Close()` on its controller and on
  any orphans at once, and does not wait. The parent HWND is **not** destroyed by a thread that has
  stopped pumping: it is moved into `App::abandoned_parents` and left to process exit.
  - Ordering: after `report_frame_shape_stop` and the windows' `close_window(true)`, and before
    `App::finish`.
  - So the stop is reported just as quickly, the sentinel is finalised, no frame is published, no
    question is asked, and no bound changes.
  - `exiting` takes the same abandonment when it finds a spare still standing.
- **The new invisible cost is stated honestly.** A profile that has used pages, and that opens none
  this run, now waits for the spare's browser after its last window closes. That is invisible and
  lasts up to `PAGE_TEARDOWN_DEADLINE` (12 s). That run had no page of its own that would have
  paid it.

### SW-3 — adoption is one transaction, and the spare's owner keeps what fails (P1, adopted)

**1. `begin_adoption` does not take the resources away.** It moves the slot to **`Adopting`**; the
slot still owns the seat and the parent. `Runtime::adopt_spare_web_page` then works on the slot's
seat in place:
1. `park_for_handoff`: `wanted = Hidden`, `wanted_bounds = None` (SW-5).
2. `WebSeat::rehost(from = parent compositor, to = window compositor, address, take_focus =
   false)`.

**2. What each result leaves, and who owns it:**
- **`SourceKept`**: the slot moves to `Retiring` and still owns the seat and the parent, which it
  retires through SW-2's clock. The pane **falls back to building its own controller** through the
  unchanged `WebSeat::open` road. That is a real controller request, recorded as such.
- **`Moved`**:
  - The slot first gives up the seat: `finish_adoption()` answers the seat and moves to
    `Adopted`.
  - It drops the parent, which now holds no controller.
  - The seat enters the window through **the pane bookkeeping tail that already exists**, factored
    out of `open_web_page_on`'s `Ok` arm as `Runtime::seat_a_web_page(leaf, web, index,
    outcomes)`: insert into `window.web`, `leave_preview_buffer_in`, `clear_preview_image_in`, and
    `apply_web_outcomes`. Both roads use it, so nothing is copied.
  - Only then does it apply `set_device_scale` and `set_color_scheme`. Their errors become
    `WebOutcome::Fault` lines on a seat that is already the pane's, which is how every other seat
    reports a setter failure. **No `?` can return with the seat outside every owner.**
- **`Lost`**:
  - `WebHost::rehost` has already closed the controller, and `on_rehost_lost` has started a rebuild
    in the target window.
  - The seat enters the window the same way (`seat_a_web_page`), and `go(url, minted)` records the
    URL and mint for the rebuild.
  - The slot moves to **`Adopted { old_parent: Some((parent, released_at)) }`**, with `released_at
    = now + BROWSER_EXIT_DEADLINE`. The old parent is released at that moment, or at the run's
    bound, whichever comes first, and never waits on the new page's lifetime.
  - The slot owns the parent, the window owns the seat, and no resource has two owners.

**3. Setter order after `Moved` (keeps §3 step 4).** Take the target's scale and scheme, clear the
caches (`take_address`), then placement (`sync_web_page`), then `go`. Only **target** bounds
release the URL (SW-5).

### SW-4 — the receipt, the upgrade, and what is in memory versus on disk (P2, adopted)

- **The upgrade.** The structural step `migrate_settings_v38_to_v39` stays pure and writes `Never`.
  **`App::reconcile_web_pages_used(&SessionV1)`** runs in `FolioApp::create`, in the window process
  only, after both stores are open and before the restore choice can rewrite the session:
  - If the receipt is `Never` and the session parsed as a current or migrated document, it looks
    for typed page records across every window, tab, pane, pool entry, Recent seed and Recent
    preview (the four places listed above). If it finds one, it writes `Used` through the receipt
    writer.
  - A session that is missing, corrupt or from a newer build is **no evidence**: the receipt stays
    `Never`.
  - An HTML file name is not a page. It is never matched by searching strings.
  - It runs on every launch while the receipt is `Never`, so it costs one walk until the first
    `Used`. It never writes `Never`.
- **The writer.** `App::note_a_web_page_committed` hands `store()` a copy set to `Used` on every
  `Committed`, and lets `SettingsStore::store`'s `wants_write` decide. That fixes the retry after a
  failed first write, which the "do nothing if already `Used`" check in §1 would have blocked.
  Door processes never construct the writer.
- **"One writer", restated.** There is one writer function with two callers: the `Committed` arm
  and the startup reconciliation.
- **Who pays, restated precisely** (this replaces "scratch profiles never warm"):
  - A profile whose history has no page (receipt `Never`, and no typed page saved) makes **zero
    controller requests**. Only ticket 54's environment warm-up (~2 MB, no processes) runs.
  - A copied profile that holds `Used`, or saved pages, qualifies.
  - A fresh, isolated scratch profile (`APPDATA` **and** `LOCALAPPDATA`) does not warm.
  - A reused one that opened a page does warm on its next launch.

### SW-5 — "warmed" means the blank page has landed, with real geometry (P2, adopted)

- **The spare gets geometry through the existing placement path.** Once created, it is placed
  through `WebSeat::place` on its parent's compositor: scale first (the scale of the window that
  runs the clocks), then a nonzero rectangle (800 × 600 physical, the parent's client size), then
  `Shown` inside the never-shown parent, then a compositor commit. That is the spike's harness state.
- **`Parked` means the blank page has landed.** The seat gains `landed_on_blank()`, set when
  `NavigationCompleted` succeeds for `about:blank` on the current generation. Being installed is no
  longer enough, so the 146 ms median keeps describing the state it was measured in.
- **A page that arrives before that** finds no spare (`begin_adoption` answers `None` without
  retiring it) and builds its own controller. The spare stays for **the first eligible page**,
  which is the wording from here on.
- **The deferred address is released once.** It goes through the common `Navigate` / mint / guard
  path, from `apply_presence` on the first *target* bounds. Stale spare bounds were cleared by
  `park_for_handoff`.

### SW-6 — the controller call waits for a quiet turn (P2, adopted)

- **While `Creating`, the spare is advanced only on a turn the clock calls quiet**, through
  `WebWarmup::quiet_at(now, restore_card_up)`. It is not advanced on the `WebPageSpoke` arm.
  - `WebHost::has_events()` tells the clock that something is waiting, so the spare's wake joins
    the fold as `"spare web controller"`.
  - Each advance **drains before it ticks**, so an answer that has arrived is never read as silence
    by `ENGINE_START_DEADLINE`.
  - The deadline itself counts only once the environment answer has been read.
- A page's own requests are never held behind the spare. All three askers share one
  `EnvironmentSlot::ask`.
- A run that is ending never advances a `Creating` spare. It retires it.

### SW-7 — the native acceptance covers the whole adoption contract (P2, adopted)

This is listed under "VM smoke" below.

### Lesser points folded

- **Two builds sharing one profile folder.** The mechanism is unchanged, but **the exposure is
  wider**. A spare in build A keeps A's browser, and its options, alive before any visible page, so
  build B meets `0x8007139F` in more of the day. The fix is still never to kill B's browser or reset
  the folder.
- **A new browser version, or a crash, after adoption** is ordinary page recovery (`Page` policy).
- **The GPU device, per Q2.** After a wgpu device loss has been recovered, the spare is still
  adoptable. An actual browser failure is SW-1.
- **A window closing that is not the last** leaves the spare to the application. The spare holds no
  reference to any window's compositor, only its parent's.
- **Two pages opened in one turn.** The first `begin_adoption` moves the slot to `Adopting` before
  any platform call, so the second finds it not `Parked` and builds its own controller.
- **The trace labels** of the gate closures become a cell written by `take_address`, as in §3.
- **The CHANGELOG line** is reworded: *"The first web page you open after launch can appear in a
  fraction of a second: once you have opened web pages in Folio, it gets one ready while it is
  idle."*
- **D-64**: open, narrowed, for 0.4.6. The residual is a profile's first-ever page, pages that
  arrive before the spare has landed, and every page after the spare is used.

### The lifecycle, restated (supersedes §2's table)

Phases: **None → Creating → Parked → Adopting → Adopted**, with **Retiring → Retired** from
any live phase. An event reaches **the seat's `digest` first**. Then **`recovery_under(policy)`**
filters the effect, and **the slot** (`App::web_spare`) acts on the resulting outcome or on its own
causes.

| phase \ event | handled first by | environment / controller answer | browser exit or failure, new version | epoch changed (another seat forgot the environment) | start deadline | a page asks (`begin_adoption`) | window closes (not last) | last window closes, or quit `Retire` | `fail` / `exiting` |
|---|---|---|---|---|---|---|---|---|---|
| **None** | clock | — | — | — | — | `None`: the pane builds its own | — | → Retired (nothing to wait for) | nothing |
| **Creating** | seat (drained on quiet turns only) | ordinary progress: `CreateController`, `InstallEvents`, then placement and the blank navigation | policy → `Retire` → **Retiring** | slot → **Retiring** | seat → `Failed` → policy → **Retiring** | `None`: the pane builds its own; the spare continues | nothing | slot → **Retiring**, exit clock | `abandon` |
| **Parked** (blank landed) | seat, on any spoke | — (late or orphan answers are `Close`d) | policy → **Retiring**, before any forget or rebuild | slot → **Retiring** | — | re-check (SW-1 item 5) → **Adopting**, or → Retiring and `None` | nothing | slot → **Retiring**, exit clock | `abandon` |
| **Adopting** (inside one call) | `adopt_spare_web_page` | — | — (synchronous; there is no pump turn inside it) | checked on entry | — | a second page finds it not `Parked`: builds its own | — | — | — |
| ↳ `Moved` | slot | → **Adopted**; the seat is in the window (`Page` policy); the parent is dropped | | | | | | | |
| ↳ `SourceKept` | slot | → **Retiring** (owns the seat and the parent); the pane builds its own | | | | | | | |
| ↳ `Lost` | slot | → **Adopted { old_parent }**; the seat rebuilds in the window | | | | | | | |
| **Adopted** | window (the seat) | ordinary page road | ordinary page recovery | — | — | `None` | — | releases `old_parent` at its time or at the run's bound | `abandon` on `old_parent` |
| **Retiring** | slot's `advance` | orphans `Close`d | ignored (already closing) | ignored | → `ReleaseUserDataFolder` at `BROWSER_EXIT_DEADLINE`, with no notification | `None` | nothing | continues, bounded by `run_retiring_until` | `abandon` |
| **Retired** | — | — | — | — | — | `None` | — | `has_let_go` = true | nothing |

The clock's spare stage runs at most once per process (§2), so no path leads from Retired or
Adopted back to Creating.

### Tests, re-specified as sequences of events (supersedes §5)

All are headless, and all drive the real `WebMachine`, `recovery_under`, `EnvironmentSlot`,
`SpareSlot` and `WebWarmup` through recorded doors, with **delayed callbacks**. Callbacks are
queued and delivered on a later simulated turn, never inside the call that caused them. Each test
records the environment creations, forgets, controller requests, closes, orphan closes and the
parent's lifetime.
The native executor's COM calls are verified only in the VM.

1. **`a_profile_whose_history_has_no_page_never_asks_for_a_controller`.** Fresh profile; upgraded
   without pages; a failed or blank navigation; an import in each direction. Zero controller
   requests and zero receipt writes. MUTATION: drop the receipt condition.
2. **`the_spare_waits_for_a_quiet_turn_even_when_the_environment_answers_late`.** Environment
   answer delayed past a stir: the controller request comes only on the next quiet turn, and once.
   MUTATION: drive the spare on the spoke.
3. **`a_page_opened_while_the_spare_is_being_made_neither_waits_nor_takes_it`.** One environment
   creation for both. The page makes its own controller request. The spare later parks for the
   next eligible page. MUTATION: `begin_adoption` from `Creating`.
4. **`a_page_and_the_spare_hearing_one_browser_exit_rebuild_once`.** One environment forget and
   one new environment creation (the page's). The spare's effects contain no forget and no ask, and
   it is `Retiring`. The same holds for a new version and for either delivery order. MUTATION:
   `recovery_under` as the identity.
5. **`a_spare_retired_while_its_controller_is_pending_closes_the_late_controller`.** Retire during
   `ControllerPending`, then deliver the completion: one orphan `Close`, no install. MUTATION: the
   old `close_pending_controller`.
6. **`another_seat_forgetting_the_environment_retires_a_parked_spare`.** Epoch bump, then advance:
   `Retiring`, and `begin_adoption` answers `None`. MUTATION: drop the epoch comparison.
7. **`every_handoff_result_leaves_each_resource_one_owner`.** With a recorded handoff door returning
   `Moved`, `SourceKept`, `Lost`, and `Moved` plus a failing setter, the test asserts the owners of
   the seat, the parent and the old parent after each. For `SourceKept`: exactly one new controller
   request (the pane's). For `Moved`: none. MUTATION: take the resources in `begin_adoption`.
8. **`an_adopted_page_navigates_only_on_the_targets_bounds_and_only_once`.** Stale spare bounds; a
   target with no layout yet; a hidden/background target; repeated placement. Exactly one
   `Navigate`, after the first target bounds. MUTATION: release on any bounds.
9. **`two_pages_in_one_turn_share_one_spare`.** Two `begin_adoption` calls: one adopts, one builds
   its own. MUTATION: move to `Adopting` after the handoff.
10. **`the_run_ends_after_the_spare_lets_go_with_no_window_left_to_wake_it`.**
    - Last-window close and quit are driven through the empty-registry and retirement arms'
      functions, extracted as pure `next_control_flow(...)`, with **no exit notification** and an
      advancing fake clock.
    - The run ends at `BROWSER_EXIT_DEADLINE` from retirement, and never later than
      `run_retiring_until`.
    - The bound is not restarted by polls. Pages and the spare retire concurrently.
    - MUTATION: `ControlFlow::Wait` in the empty arm (the test hangs on its deadline), or a
      restarted bound.
11. **`an_orderly_stop_abandons_the_spare_at_once`.** `abandon` makes no wait, closes the
    controller and orphans, and holds the parent for process exit. MUTATION: route it through
    `retire`.
12. **Receipt tests.**
    - `the_receipt_is_written_by_a_real_commit_and_retried_after_a_failed_write`: a real
      `SettingsStore` in a temp folder, the first write failing, then a successful retry.
    - `the_upgrade_reads_typed_saved_pages_and_nothing_else`: each of the four record kinds is
      `Used`; an `.html` file preview, a corrupt session and a future session are `Never`.
    - The import test in both directions.
13. **Guards through `bt_source`**, kept only as wiring pins beside the behaviour tests above:
    `seat_a_web_page` is the one bookkeeping tail; `SpareParent` is the one `CreateWindowExW` in
    product code; the station vocabulary includes the new stations.

Report the number of tests each name filter selected and ran.

### VM smoke list (supersedes the appendix's)

This is one extended session: a clean VM, an isolated `APPDATA` and `LOCALAPPDATA`, and
`BT_WEB_TRACE`. Each launch **waits for a `spare parked` trace line**, never for a fixed number of
seconds.

1. **A `Never` launch.** Zero `msedgewebview2` processes of Folio's own after 30 s.
2. **Seed `Used`, launch, wait for `parked`.** Change the theme, move the window to a second monitor
   at a different DPI (if the VM cannot provide two DPIs, record that this condition is still
   open), then open a local page that reports CSS size, `devicePixelRatio` and
   `prefers-color-scheme`. Assert:
   - the `adopt … outcome=Moved` line, and no `request_controller` on the gesture turn;
   - the controller's parent is the pane's window, and `IsWindow(hidden parent)` is false;
   - nonzero bounds, the pane's origin, clipping, and the first visible frame correct before any
     corrective resize; then change DPI again;
   - the times: first page, and `NavigationCompleted`.
3. **Input on the adopted page.** Address-bar typing, click to focus, Tab out, a Folio shortcut, and
   IME composition with its candidate window placed at the caret.
4. **File gates on the adopted page.** An allowed local navigation, a navigation that is currently
   refused, and an iframe/resource request where today's policy differs. The trace lines name the
   target pane, and change again after a tear-out of the adopted page.
5. **A second page** makes its own controller request.
6. **Closing and quitting.** Close while `Creating`, and close while `Parked` before any page, each
   with the time to process exit, the sentinel removed and the run footer written. Quit (Ctrl+Shift+Q)
   with a parked spare. Last-window close with an adopted page. For each, record the hidden
   parent's HWND lifetime and every browser PID's exit time **separately** from the bound.
7. **An orderly stop.** The frame-validator's orderly stop with a parked spare, against a baseline
   of the same stop with no spare: the report, the session finalisation, the footer and the exit
   time must match.
8. **Background adoption.** Open the page into a background pane, then bring it forward.

The rare branches (`SourceKept`, `Lost`, a failing setter, a new version, a crash while parked) are
covered by the headless fault injection of tests 4–7, not left to chance in the VM.

### Architecture impact, restated (supersedes §7 where it differs)

- **(a) Facts.** New: the spare's lifecycle, with phases None, Creating, Parked, Adopting, Adopted
  and Retiring/Retired, one owner (`App::web_spare`), and the run's retirement bound
  (`App::run_retiring_until`, one writer). Changed: a seat's recovery policy (`WebSeat`, written by
  `open` and by adoption); `EnvironmentSlot` gains an epoch; `WebHost` gains the pending orphans.
  The receipt has one writer function with two callers.
- **(b) Doors.** As before, plus the `SpareParent` window door. No new thread, process or file read.
  The startup reconciliation reads the already-loaded `SessionV1` and does no new disk read.
- **(c) Debt.** D-64 open, narrowed, for 0.4.6. The orphan-controller leak is a defect repaid in
  this commit (a DESIGN entry, and no ledger row needed).
- **(c′) Readers to add to §7's list:** the empty-registry arm's control flow;
  `FolioApp::advance_retirement`; `fail` and `exiting`; the restore/session road (it now feeds the
  receipt); `ENGINE_START_DEADLINE` (it now counts from the answer being read, for the spare only).
- **(d) Ownership change: still no.** Every resource has exactly one owner at every return of the
  transaction (table above). The slot keeps any resource the transaction fails to move.

---

## Appendix (b) — ticket 60 brief, superseding the appendix above

# 60 — The first eligible web page adopts a spare controller made on a quiet turn (0.4.5 if green in two days, else 0.4.6; D-64 narrowed)

| | |
|---|---|
| **target version** | **0.4.5 within the owner's two-day window** (dispatch day + 2). Not CI-green **and** VM-evidenced by then → **0.4.6, whole**: no partial merge, no weakened test, no extended window. It then stays on its branch until the 0.4.5 tag. |
| branch / worktree | `feat/spare-web-controller` · `D:\Developer\bt-wt\spare-web` |
| size | **M, at its top edge.** It is honestly M only because every mechanism is reused: the seat, the machine, the step executor, rehost, placement, the teardown deadline and the settings store. The new code is the slot, one policy filter, an epoch, the orphan list, the exit clock at two call sites, the reconciliation and the transaction. |
| who | **Opus** |
| lane | local compile lane (name-filtered tests); **VM session on day 1 afternoon**, not at the end |
| shape | one commit, mergeable alone; CI green on Windows, macOS and Linux; VM evidence attached |
| depends on | 43, 40, 54 (merged). BASE = the coordinator's sha. |
| repays or narrows | D-64 narrowed (it stays open, for 0.4.6); repays the orphan-controller leak |

**Read first.** This note, including Revision (b); the Codex review; `reports/59.md`, `54.md` and
`43.md`; DESIGN §7.35.

**Goal.** §1–§4 as amended by Revision (b): the receipt and the upgrade's reconciliation (SW-4);
the lifecycle table; the parked recovery policy, the epoch and the orphans (SW-1); the exit clock
and the orderly-stop abandonment (SW-2); the adoption transaction and `seat_a_web_page` (SW-3); the
landed-blank readiness and the target-bounds release (SW-5); quiet-only advancing while Creating
(SW-6). No UI string, no setting row, no notice.

**Not in this ticket.** Replenishing the spare. Any change to shutdown or recovery beyond the
spare's own road. A WebView2 thread. Any change to existing bounds or timeouts.

**Tests.** Revision (b)'s list, 1–13. Each is red on BASE by behaviour (a missing symbol does not
count as red), and red again with only its product line reverted. Name filters: `web`, `webhost`,
`warm`, `spare`, `environment`, `station`, `stall`, `settings`, `quit`, `migrat`, with the count
each selected.

**The two-day plan, and what is cut if it slips.**
- **Day 1, morning:** the slot, the policy, the epoch, the orphans and tests 1–9.
- **Day 1, afternoon:** the exit clock, abandonment and tests 10–11, then **the VM session's items
  2, 6 and 7** (adoption, closing and quitting, the orderly stop).
- **Day 2:** the receipt and the upgrade (test 12), guards, docs, the rest of the VM list, CI.
- **Stop rule:** if at the end of day 1 test 10, or VM item 6, is not green, **stop and report**.
  The ticket moves to 0.4.6 whole. The one prerequisite that may be split off for 0.4.6 on its own
  is the orphan-controller fix (it stands alone and helps pages too). Nothing of SW-1–SW-3 is
  optional, and no part of the feature may ship enabled without them.
- **If the VM cannot give two DPIs,** the cross-DPI item is reported as open. The coordinator
  decides whether it blocks the merge. The ticket does not.

**Docs in the same commit.**
- CHANGELOG: the reworded line from Revision (b).
- DESIGN: one entry for the ruling and design, one for the orphan-controller defect.
- ARCHITECTURE: §5.3 row 21 narrowed, with the numbers; a §6 door row for `SpareParent`.
- structural-debt: D-64 open, narrowed, for 0.4.6, with the residual named.
- RULES: rows 31, 33, 43 (the spare shares the run's bounded retirement; the orderly stop abandons
  without waiting) and 49.

**Architecture impact.** Revision (b)'s restatement. The report restates it as built.

**Standing rules.** `_standing-rules.md` in full.

**Report.** `reports\60.md`:
- the anchors that moved;
- the red-before / green-after table;
- every VM item, with its evidence or marked open;
- the stations' holds (turn B, and the quiet-gated controller call);
- adopted and unadopted exit times, with browser tails recorded separately;
- the gate tails and the head sha.
