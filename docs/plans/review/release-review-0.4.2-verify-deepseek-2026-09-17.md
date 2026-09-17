STATUS COMPLETE

# Verification of release-review-0.4.2-codex findings X-1, X-2, X-4, X-5, X-6, X-7, X-8, X-10

Read-only check against commit 29e728c8. No build, no test, no launch, no edits to
tracked files. All line references are at 29e728c8.

## X-1 — CONFIRMED (must-fix)

Re-opened: crates/bt-app/src/main.rs:715, main.rs:97694-97707, main.rs:97783-97808,
crates/bt-app/src/seats.rs:453-477, crates/bt-layout/src/tree.rs:13.

Claim: an async picture paste stores only a generation + `SeatId`, and adoption resolves
that seat against the *currently active tab*, so a paste can land in another tab.

What the code does — it matches exactly. The answer carries no tab identity:
`struct ClipboardPictureAnswer { generation: u64, seat: SeatId, result: ... }` (main.rs:715).
`adopt_clipboard_picture` hands that seat to `paste_paths_into(landed.seat, ...)` (main.rs:97860),
which resolves it in the active tab: `let active = self.window.active_tab; ... self.window.tabs[active].sessions.get(&seat)`
(main.rs:97700-97701). `SeatId(pub u64)` is per-tree (bt-layout/src/tree.rs:13), and each new
single-pane tab re-mints from 1: `pub fn lone_seat(seat: &Seat) -> (Self, SeatId) { let id = SeatId(1); ... }`
(seats.rs:453-454). The window-scoped `generation` only changes on another paste (`withdraw`, main.rs:741-744),
not on tab activation, so the mismatch survives the generation check.

Reachability: paste a screenshot in tab A, switch to B before the PNG encode returns. The encode
is worker-side (main.rs:97792-97808) and explicitly may span "tens of milliseconds" (main.rs:97774).
Visible consequence: tab B's first pane (also `SeatId(1)`) receives the path. Replacing the shell in
the same seat between ask and adopt produces the same defect with no tab switch, because the session
is re-resolved at adoption time.

Fix: correct. Capture tab + seat + session incarnation at accept time and deliver only to the live
origin (or drop the obsolete answer). The proposed test (completion after tab switch) is the right one.

Missed nothing material. Note the identical resolution bug also affects the drop path through
`paste_paths_into` (main.rs:97694), but X-10 covers the drop side's own, different defect.

## X-2 — CONFIRMED (must-fix)

Re-opened: crates/bt-app/src/clipboard_picture.rs:158-182, :253-266, :274-282; main.rs:97792-97796.

Claim: two workers (two windows or two instances) enumerating the same shared folder in the same
second compute the same next name and both `fs::write` the same path, overwriting each other.

What the code does — matches. `plan` computes the index purely from a listing
(`mine.iter().filter(|(at,_,_)| *at == stamp).map(...).max().map_or(1, ...)`, clipboard_picture.rs:168-171)
and `save` then does `fs::write(&path, &bytes)` (clipboard_picture.rs:261) — a truncating, non-exclusive
write. Nothing reserves the name between `plan` and `write`; `names_in` even swallows a failed
`read_dir` (clipboard_picture.rs:274-281) so a brand-new folder yields index 1 for every racer. Both
windows of one process share `clipboard_picture::directory()` (main.rs:97791), and separate instances
share the same `%TEMP%/folio/clipboard`.

Reachability: two windows (or two Folio instances) pasting a picture in the same wall-clock second.
Visible consequence: one PNG silently replaces the other; the pasted path can name the wrong image.

Fix: correct in direction. Smallest correct form is exclusive creation with retry
(`OpenOptions::new().create_new(true).write(true)`, on `AlreadyExists` re-`plan` with the bumped index),
plus write-then-rename so a file is never visible half-written. "Atomic replacement alone does not
prevent two requests choosing the same name" is the precise point.

Missed nothing. The same-second test at clipboard_picture.rs:355-380 does exercise `plan` only, never
the write, exactly as the finding states.

## X-4 — CONFIRMED (must-fix)

Re-opened: crates/bt-app/src/clipboard_picture.rs:231-241; crates/bt-platform/src/windows_clipboard.rs:173-191;
image-0.25.10 src/codecs/bmp/decoder.rs:64-65, :619-642; src/io/free_functions.rs:305-318.

Claim: a tiny DIB header with large valid dimensions reaches `DynamicImage::from_decoder` with no
application cap, and the pinned `image` crate allocates the whole output before reading pixels.

What the code does — matches. `png_from_dib` calls `DynamicImage::from_decoder(BmpDecoder::new_without_file_header(...))`
with no limits (clipboard_picture.rs:232-235). Acquisition copies each offered encoding uncapped:
`let bytes = std::slice::from_raw_parts(pointer, size).to_vec();` (windows_clipboard.rs:188). In image
0.25.10, `MAX_WIDTH_HEIGHT: i32 = 0xFFFF` (decoder.rs:65) and `check_for_overflow` (decoder.rs:642)
only guards integer overflow, not practical size. `DynamicImage::from_decoder` → `decoder_to_image` →
`decoder_to_vec`, which does `let mut buf = vec![...; total_bytes / size_of::<T>()];` **before**
`read_image` (free_functions.rs:316-317); its only guard is `total_bytes > isize::MAX`. A 32768² 24-bit
header therefore requests ~3 GiB before the truncated body can fail.

Reachability: any other app puts such a DIB on the clipboard; user presses Ctrl+V. The worker thread
(main.rs:97792) is in-process, so an allocation failure aborts the whole application — the worker does
not contain it.

Fix: correct. Bounding decoded dimensions / total bytes (or setting `image`'s `Limits` on the decoder)
before `from_decoder`, and capping `global_bytes`, is the smallest correct form. The reviewer's
admission that no OOM probe was run is accurate and does not weaken the source-level conclusion.

Missed nothing. (The 8-byte-PNG-signature acceptance noted in the review's own crash-safety paragraph,
clipboard_picture.rs:213-218, is a separate, already-recorded item.)

## X-5 — CONFIRMED (must-fix)

Re-opened: crates/bt-app/src/main.rs:22246-22259, :95755, :96514-96558, :96045-96057.

Claim: the Mac wheel fix rewrites *any* Shift + pure-horizontal report `(x,0)` to `(0,x)` on every
platform, so genuine horizontal gestures are misclassified.

What the code does — matches. `upright_wheel` matches `LineDelta(x,y) if y == 0.0 && x != 0.0 => LineDelta(0.0, x)`
and the same for `PixelDelta` (main.rs:22251-22256); it is plain Rust with no `cfg` gate, applied at
`let delta = upright_wheel(reported, self.window.modifiers.shift_key())` (main.rs:95755). The terminal
column path then flips sign: `let notches = if sideways { f64::from(x) } else { -f64::from(y) };`
(main.rs:96544-96548) — a rewritten `(0,x)` makes `wheel_points_sideways` false and yields `-x`, the
reverse of the pre-rewrite `x`. The web branch reads `(x, y)` straight off the delta (main.rs:96048-96053),
so a rewritten report loses its horizontal axis and gains vertical.

Reachability: ordinary — a tilt-wheel's horizontal scroll on Windows with Shift held, or a two-finger
horizontal trackpad swipe with Shift on macOS. The code's own safety argument ("a genuine sideways
gesture ... is made without Shift", main.rs:22236-22238) is the unsound assumption. Visible consequence:
horizontal scrolling reverses direction in the terminal, and surfaces that only read `y` (first-run at
main.rs:95987, settings, web page) scroll vertically from a horizontal gesture.

Fix: direction correct, not fully "smallest". Gating `upright_wheel` to macOS (`#[cfg(target_os="macos")]`)
is the smallest fix for the cross-platform regression but still misclassifies a genuine Shift+horizontal
Mac trackpad gesture; the finding's call to normalize "only a known platform-translated gesture" is the
right general fix but needs provenance the current event does not carry. Both should be stated.

Missed nothing. The test `(x,0) → (0,x)` is indeed a codification of the regression, not a guard against it.

## X-6 — CONFIRMED (should-fix)

Re-opened: crates/bt-detect/src/lib.rs:842-860, :905-907, :968-971; crates/bt-term/src/session.rs:11231-11240,
:11340-11344, :11409-11419.

Claim: the across-rows detector accepts a closing row whose first `$` closes a split even when a complete
pair follows it, but the frozen scan window's predicate requires a lone-dollar census, so the opening row
is never included.

What the code does — matches, and the predicate asymmetry is exact. `detect_inline_math_across_rows`
uses `let close = first_inline_closer(text)?` (lib.rs:791), and `first_inline_closer` is
`let close = text.find('$')?; closes_a_row_split(text, close)` (lib.rs:968-971) — the *first* `$`,
whatever follows. The frozen window's gate is `may_close_row_split_inline_math`, which requires
`matches!(dollar_census(text), DollarCensus::Lone(close) if ...)` (lib.rs:905-907), and
`dollar_census` returns `Pair` as soon as a second `$` exists (lib.rs:850-859). The SPLIT_TAIL row
(three dollars, lib.rs:3806-3807) is therefore accepted by the former and rejected by the latter.
`frozen_inline_join_window_start` returns `None` on that rejection (session.rs:11236-11238), so
`join_start` is `None` in `schedule_scan` (session.rs:11340) and the scan window is never extended to
the line above; with no open display block the fallback (session.rs:11409-11419) covers only the
candidate. The reviewer's temporary regression test result (pair detection succeeded, frozen-window
predicate false) is consistent with this.

Reachability: freeze the two rows into history, then require detection at the closing row (e.g. after a
resize or re-scan). Visible consequence: the split quadratic formula is not typeset — its opening `$x`
fragment is omitted from the join.

Fix: correct. Make join-window discovery recognize a valid first closer even when complete formulas
follow it on the row (i.e. use `first_inline_closer` semantics, not the lone census, in the window gate).

Missed nothing. The direct-scanner tests genuinely bypass this (they call `detect_inline_math_across_rows`
with both rows, lib.rs:3809-3816), so the session-path interaction is untested, as stated.

## X-7 — CONFIRMED (must-fix)

Re-opened: crates/bt-app/src/trace_sink.rs:285-287, :349-355; crates/bt-app/src/hang_watch.rs:2040-2049;
crates/bt-app/src/persist.rs:274-277; main.rs:111857, :97813.

Claim: a trace writer blocked in `stderr.write_all` holds Rust's shared stderr lock; the watchdog prints
slow holds before polling, and UI diagnostics use `eprintln!`, so all can block behind that writer.

What the code does — matches. The sink writer's body ends in `put`, which does
`let _ = stderr.write_all(batch.as_bytes());` (trace_sink.rs:353), using `std::io::stderr()` (trace_sink.rs:286)
whose `Write` impl serializes on a shared lock. The watchdog prints **before** its decision:
`for hold in slow { eprintln!("{}", hold.line()); }` then `match watch.poll(...)` (hang_watch.rs:2041-2049).
UI-side diagnostics go through `eprintln!` too: `report_save_did_not_finish` (persist.rs:275-277), the
quit-write error (main.rs:111857) and the clipboard-worker start failure (main.rs:97813) — the last two on
the window thread. If a stderr consumer stops reading, the sink blocks inside the OS write while holding
the lock, and each of these call sites blocks behind it.

Reachability: run under `BT_PERF_TRACE` with stderr redirected to a pipe/file whose reader stops consuming.
Visible consequence: the watchdog never reaches the new two-second decision, and a UI error path can stall
the window thread.

Fix: correct. The smallest correct move is to make the watchdog's decision loop independent of the console
(do the `poll`/report-file capture before any `eprintln!`, or route diagnostics through the same nonblocking
queue). The finding's note that "moving trace writes alone does not eliminate the shared-output dependency"
is accurate — the writers share the same `stderr`.

Missed nothing.

## X-8 — CONFIRMED (must-fix)

Re-opened: crates/bt-app/src/persist.rs:409-428; main.rs:111851-111869; crates/bt-app/src/quit.rs:192-238,
:359-368; CHANGELOG.md:61-69.

Claim: a save that times out enters `Phase::Abandoned`, not `Retiring`, so the advertised "closes after
three seconds" is not implemented.

What the code does — matches. `wait_for` returns `Err(save_did_not_finish())` on timeout (persist.rs:425-428).
The quit driver then treats it as a plain failure: `if let Err(error) = &landed { eprintln!(...); ... toast(QuitSessionNotWritten) }`
followed by `self.report_to_quit(|quit| quit.written(landed.is_ok()));` (main.rs:111856-111869). `written(false)`
sets `Phase::Abandoned` (quit.rs:363-366), which maps to `QuitStep::Abandon` (quit.rs:238), not `Retire`;
the comment is explicit that an abandoned quit is one "the application carries on past" (quit.rs:271-273).
CHANGELOG.md:61-69 promises "gives the save three seconds ... and closes".

Reachability: quit while the session filesystem stalls (>3 s). Visible consequence: instead of closing and
keeping the last completed save, Folio shows "session not written" and stays open — the opposite of the
release note, though the unbounded hang is gone.

Fix: correct. Distinguish timeout from ordinary write failure in the quit transaction and wire the timeout
to the retire path (preserving the dirty sentinel). The "valid last-completed snapshot, not immutability"
caveat is accurate.

Missed nothing.

## X-10 — CONFIRMED (should-fix)

Re-opened: main.rs:113135-113137, :95811-95828, :95896-95909; crates/bt-platform/src/macos_impl.rs:652-669.

Claim: a file drop queues only paths; the drop target is inferred from a cached or later-sampled pointer
position, not the position at drop time.

What the code does — matches. `WindowEvent::DroppedFile(path) => { runtime.window.dropped_files.push(path); ... }`
(main.rs:113135-113137) keeps only the path (winit 0.30 provides no position). `flush_dropped_files`
asks `dropped_files_point()` at flush time (main.rs:95823), and that function prefers the cached
`self.window.pointer_position` — `let live = self.window.pointer_position; ... Some(_) => None` skips the
native query (main.rs:95897-95903) — falling back to `bt_platform::pointer_position_in_window`, which on
macOS reads `NSEvent::mouseLocation()` at query time (macos_impl.rs:659), i.e. the *current* cursor, not
the drop point. `pointer_position` is only cleared on `CursorLeft` (main.rs:85362) and set on `CursorMoved`
(main.rs:86784), neither of which an external drag delivers (main.rs:95853-95854).

Reachability: drag a file from Explorer/Finder onto a pane after previously hovering a different pane (the
cache may be stale), or move the cursor after release while queued output delays the flush. Visible
consequence: the path is pasted into the wrong pane.

Fix: correct, but not "small" in this codebase — winit discards the drop point, so capturing it requires a
backend change (or platform-specific `IDropTarget`/`NSView` handling) to carry the drop coordinates/target
to the batch. The finding's framing ("the promise is stronger than the event data retained") is fair.

Missed nothing.

## Ranked summary

1. X-4 — CONFIRMED: unchecked decode/allocation can abort the app from a malicious DIB header (must-fix).
2. X-8 — CONFIRMED: session-save timeout abandons the quit instead of closing as advertised (must-fix).
3. X-1 — CONFIRMED: delayed picture paste can land in another tab via active-tab seat resolution (must-fix).
4. X-5 — CONFIRMED: `upright_wheel` rewrites genuine Shift+horizontal gestures on every platform (must-fix).
5. X-2 — CONFIRMED: concurrent pastes can overwrite the same PNG through un-reserved names (must-fix).
6. X-7 — CONFIRMED: blocked trace writer can stall the watchdog and UI diagnostics on shared stderr (must-fix).
7. X-6 — CONFIRMED: frozen join-window predicate rejects a valid split closer, omitting the opening row (should-fix).
8. X-10 — CONFIRMED: file-drop target is inferred from cached/later cursor state, not the drop point (should-fix).

STATUS COMPLETE
