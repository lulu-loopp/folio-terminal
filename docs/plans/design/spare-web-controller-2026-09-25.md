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
