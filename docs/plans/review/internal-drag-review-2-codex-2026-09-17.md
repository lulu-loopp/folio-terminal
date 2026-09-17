# Internal file-row drag review, round 2 — 2026-09-17

Reviewed `8baa023ef224d1876903542410c9f6008d180c68`, including its main merge.
Round 1 was read with `git show 5d7184fd:docs/plans/review/internal-drag-review-codex-2026-09-17.md`.
Line references are at the reviewed commit. `main.rs` and `seats.rs` mean `crates/bt-app/src/`;
platform paths are relative to `crates/bt-platform/src/`. Product code remained read-only.

**Verdict: merge with must-fixes. Do not merge this revision into 0.4.2 unchanged.**
The original four stale-target sequences now refuse correctly, but a release can still paste into
a terminal underneath another window. Fix the release surface check below before 0.4.2;
otherwise hold this feature for 0.4.3. The bar remains no PTY write to a pane the user did not aim at.

## Round-1 disposition

| Finding / sequence | Status | Evidence at 8baa023e |
| --- | --- | --- |
| P1-a: missed final motion from terminal B to terminal A in this window | Closed | `main.rs:90397` reads the platform point; `:90402` surveys again; `:90404` requires equality, so B cannot receive A's release. |
| P1-a: sibling closes, surviving B moves away from the still pointer | Closed | `main.rs:89229` surveys the current seat layout; a different centre or an edge cannot reproduce B's offer (`:46454`, `:90404`). |
| P1-a: target disappears | Closed | `main.rs:46459` requires the seat to exist and be terminal; `:46464` requires a shell identity. No offer means no write. |
| P1-a: active tab closes, neighbour reuses the seat number | Closed | `main.rs:98384` captures active tab, seat and incarnation; whole-offer equality at `:29536` rejects the replacement, including a same-seat shell restart. |
| P1-b: refused content plan still writes | Closed | A fresh plan is built at `main.rs:90431`; `:90404` requires `plan.fits()` before `:90477` writes. |
| P2: contradictory current guidance | Closed | `docs/DESIGN.md:1043` preserves other panes' bands/middles; `:2902` marks the old table historical; invisible-gestures `:104` supersedes file-to-terminal refusal. |

P1-a overall is **partly closed**, because the release's surface ownership remains unchecked below.
The arm matches `RowVerb::PastePath(_)` (`main.rs:90472`) and uses the returned `PasteTarget`,
not the hover-time seat. `paste_paths_into` validates that identity again (`:98367`).
`plan_for` refuses a terminal with no session (`:46337`); the caption requires a fitting plan
(`:46490`), so that case takes the existing outline route rather than silently promising a paste.
The two external-drop comments now acknowledge the internal exception (`:96501`, `:113952`).
`CHANGELOG.md:20`/`:62` and DESIGN `:1043` restrict the execution claim to Folio itself;
DESIGN explicitly leaves the recipient program's interpretation and bracketed-paste mode intact.

## P1 — revalidate the window under the fresh release point

`paste_offer_kept` (`main.rs:90396`) refreshes coordinates but omits the ownership condition that
`drive_drag` applies before surveying (`:90020`). `survey_drop` only reads this window's geometry
(`:89174`, `:89229`); it does not ask which window is actually visible under that point.

Source-traced counterexample, without requiring a focus loss or a layout change:
1. An always-on-top window covers part of terminal B's centre. Hover a still-visible part of B;
   the offer is B and the broker says `Home`.
2. Move into the covered part of that same centre and release without a delivered final motion.
   The source retains mouse capture and focus; the visible destination belongs to the other window.
3. `hand_over_across_windows` consumes the cached broker aim (`main.rs:89736`), so `Home` returns
   to the local path (`:89749`). Even a broker tick queries its cached `broker.pointer`
   (`:112233`, `:112251`), which is updated by delivered motion (`:89979`), not this release.
4. The fresh platform point converts correctly into B's client-space centre. Both offers are B,
   `plan.fits()` is true, and `main.rs:90477` writes into the terminal behind the other window.

This is an inference from the event and call paths, not a native reproduction. Capture permits
delivery over another window ([Win32 SetCapture](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture));
local winit 0.30.13 `windows/event_loop.rs:1797` emits button-up without refreshing cursor motion.
Required fix: check the window under the **fresh** point before accepting the text offer; refuse
foreign or indeterminate ownership. Do not substitute the old broker aim or its old pointer.
Add a regression with unchanged landing/tab/seat/incarnation and a changed surface owner;
the current equality test's positive control would accept exactly that unchanged offer.

## Platform, cancellation, and equality attacks

**Windows coordinates: no extra DPI multiplication belongs here.** `lib.rs:7317` calls
`GetCursorPos`, then `ScreenToClient` using the live HWND; `main.rs:114507` only widens integers.
The client origin is read at release handling, so moving the window does not retain the old origin.
The result is in device units ([Win32 ScreenToClient](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-screentoclient)),
matching winit's direct `WM_MOUSEMOVE`-to-`PhysicalPosition` conversion (`windows/event_loop.rs:1646`).
Winit enables per-monitor awareness (`windows/dpi.rs:20`). Moving to a differently scaled monitor
does not require scaling the queried point again: the survey uses physical seat rectangles and
the renderer's scale for bands (`main.rs:89229`). Scale/resize handlers reconcile metrics and solve
layout (`:99087`, `:99189`, `:98956`). No unit mismatch was found in those paths.
This is a handling-time cursor query, not a timestamped button-up point
([Win32 GetCursorPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getcursorpos));
no native moved-window or mixed-monitor event-order experiment was performed.

**Focus loss and cancelled modal sequence: closed.** `Focused(false)` calls `cancel_drag`
(`main.rs:113997`); Esc calls it before the modal keyboard ladder (`:97370`). Cancellation takes
the drag and clears press state (`:91129`). A subsequent stray mouse-up finds no drag at
`release_drag` (`:90287`), so cannot resurrect this paste. An active quit/gate modal also intercepts
mouse input before chrome routing (`:92613`). This does not protect the focused, captured-window
counterexample above, which needs neither a modal nor cancellation.

**Equality is stable and deliberately coarser than coordinates.** `SeatCentre` contains only
`target: SeatId` (`main.rs:29275`); `PasteTarget` contains tab/seat/incarnation (`:789`). There is
no timestamp, frame id or float position in an accepted offer. A steady gesture can pass; the
positive control tests it. Two distinct physical points surveying to the same seat centre and
same PasteTarget still address the pane the user aimed at, provided it is this window's visible
surface. Exact point equality would be unnecessary. The missing surface condition is the defect.
The older presented-preview concern is also not established by this mechanism: `drive_drag`
stores the offer before `present_chrome_change` merely requests redraw (`:90032`, `:88779`).
Thus offer equality proves agreement with the last survey, not that its box reached the screen.

**Mac coordinates: the same physical client space.** `macos_impl.rs:652` obtains the NSView's
window, converts `NSEvent.mouseLocation` from screen to window to view, handles `isFlipped`, then
multiplies by that window's `backingScaleFactor` (`:659`–`:669`). Local winit 0.30.13
`macos/view.rs:1062`/`:1084` uses the corresponding view conversion and scale for mouse events;
`:591` emits motion before mouse-up. There is no extra desktop-origin flip or second scaling.
The helper rounds to integer physical pixels, so subpixel boundary classifications can differ;
that does not make every unchanged centre refuse. No native Mac run was performed.

## Validation

At `8baa023e`, all permitted filtered runs passed: **17 tests, 0 failures**.
Every command was `cargo test -p bt-app --bin folio <filter> -j 4`:
`clipboard_path_tests` (12, including `a_stale_aim_is_refused_however_it_went_stale` and
`a_rows_centre_verbs_leave_by_three_doors`), `row_splits` (2), `a_rows_centre_says` (1),
`a_pane_offers_one_middle_and_four_bands_at_every_scale` (1), and
`a_content_drop_into_a_window_below_its_minimum_is_refused` (1). No broader suite was run.
The stale-aim test exercises constructed offer pairs (four cases, restart, positive control,
and no-promise). The three-doors test pins source text, not a running Runtime or native events.
The minimum-size test exercises a real content plan. Their passing cannot detect the surface
ownership omission. No application was launched, no process was terminated, and no scratch tests
were added. Only this review document is committed.
