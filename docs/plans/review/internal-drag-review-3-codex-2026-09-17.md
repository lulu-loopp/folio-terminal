# Internal file-row drag review, round 3 — 2026-09-17

Reviewed `ba7aadffce3caf44935f8def95f282e240cc3722` against round 2 at `8baa023e`.
Read the round-2 report from `docs/internal-drag-review-2` and the round-1 report at `5d7184fd`.
References below are at ba7aadff; `main.rs` means `crates/bt-app/src/main.rs`, and platform
paths are relative to `crates/bt-platform/src/`. Product code remained read-only.

**Verdict: merge into 0.4.2. No must-fix found in this round's requested ownership attack.**
The bar remains no PTY write to a pane the user did not aim at; the previous blocker is closed.

## Round-2 P1: closed

The closing line is `main.rs:90510`, `self.glass_here(released_at)`: the point is freshly
queried at `:90503`, surveyed at `:90508`, and checked through the shared `to_screen` conversion
(`:89938`, `:89985`) used by the drag's ownership check (`:89966`, `:90114`).
`paste_offer_is_kept` (`:29597`) requires ownership, equal offers and a fitting plan, returning
the accepted target. `:29585` accepts only `Ours`; `:90583` refuses before `:90587` can write.
The source pin (`:171598`, `:171630`) checks the fresh calls and forbids `drag.pointer`,
`broker` and `pointer_position` in `paste_offer_kept`; it is a source check, not event replay.

## Ownership attack

**Windows.** `lib.rs:7113` calls `WindowFromPoint`, then `GetAncestor(hit, GA_ROOT)` at `:7114`.
Hidden/disabled windows are skipped by [WindowFromPoint](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-windowfrompoint).
For an always-on-top `WS_EX_LAYERED | WS_EX_TRANSPARENT` overlay, mouse input passes through
to the window beneath ([layered-window hit testing](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows)).
Writing beneath that click-through overlay is correct: the click would reach us too.
Do not generalize that combination to `WS_EX_TRANSPARENT` alone or every visually translucent window.
For an enabled, non-click-through hit on a child of another process's window, GA_ROOT still
names that foreign top-level parent: it follows the parent chain, without a process restriction,
and does not follow owners ([GetAncestor](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getancestor)).
Thus a foreign root becomes `Theirs`; failure becomes `Unknown`; both refuse the local write.

**macOS.** `macos_impl.rs:719` passes 0 to `windowNumberAtPoint_belowWindowWithWindowNumber`.
[Apple's contract](https://developer.apple.com/documentation/appkit/nswindow/windownumber(at:belowwindowwithwindownumber:))
selects the frontmost mouse-down recipient, can return another application's window, and excludes
transparent points/windows ignoring mouse events. Zero excludes no upper portion of z-order.
There is no normal-window-level filter here: mouse-receiving menu-bar/overlay windows participate
under that contract (inference, not a native layer-by-layer measurement); decorative click-through
overlays may be skipped, consistently with the Windows case.
The number is resolved through this app's `windowWithWindowNumber` (`:724`), then its content view
(`:725`); either missing value returns None. Apple explicitly gives the menu bar as an example of
a number lacking an app-owned object ([window lookup](https://developer.apple.com/documentation/appkit/nsapplication/window(withwindownumber:))).
That is `Unknown` at `main.rs:89994`, never `Ours`. There is no search behind the foreign hit.

**This window, not the process.** `main.rs:89988` gets `native_window(&self.window.window)`;
`:89992` requires exact equality with the hit. `NativeWindow` derives equality over its handle
(`lib.rs:41`); the raw handle is this HWND (`main.rs:118669`) or NSView (`:118672`).
A second window in this process has a different handle/content view, so becomes `Theirs` and
cannot write here, even if a cached broker aim stays Home. A second Folio process is likewise
`Theirs` on Windows or `Unknown` on macOS; neither application name nor process membership admits it.
Unknown stays home only for pane-moving drops (`:29563`), never for text (`:29585`).

## Validation and limits

At ba7aadff, all six permitted commands passed: **18 test executions, 17 distinct tests, 0 failures**.
Every command was `cargo test -p bt-app --bin folio <filter> -j 4`:
`clipboard_path_tests` (12), `row_splits` (2), `a_rows_centre_says` (1),
`a_pane_offers_one_middle_and_four_bands_at_every_scale` (1),
`a_content_drop_into_a_window_below_its_minimum_is_refused` (1), and the explicitly requested
`a_stale_aim_is_refused_however_it_went_stale` (1, also included in the clipboard filter).
The stale-aim regression's row ⑤ (`main.rs:171744`) loops over `Theirs` and `Unknown` while
keeping the offer and fitting plan identical; its `Ours` control returns the promised target.
It also covers changed/absent offers, session replacement, no promise and a refused plan.
Platform conclusions above combine source tracing with API contracts, not native reproductions.
No application was launched, no process was terminated, and no scratch tests were added.
Only this review document is committed; no broader test suite or heavier build was run.
