# 0.4.2 candidate: adversarial release review

Reviewed `v0.4.1-preview..29e728c809cd0d8c37faa646cb9049274d27560d` on 2026-09-16 (report date requested: 2026-09-17). HEAD matched the supplied `RELEASE_REVIEW_HEAD.txt`. Scope was the complete `git log --oneline v0.4.1-preview..HEAD`, the triple-dot diff and its stat (59 files, 11,120 additions, 1,165 deletions), and `CHANGELOG.md:7` Unreleased. References below are candidate lines, before this documentation commit.

This was a source and targeted-test review, not a running-app acceptance test. No application was launched or stopped; no workspace build or product edit was made. macOS conclusions distinguish binding compatibility from runtime behavior. `main.rs` below means `crates/bt-app/src/main.rs`; other shortened module names are qualified on first use.

## Findings

### X-1 — must-fix — An asynchronous picture paste can land in another tab

**Where:** `main.rs:715`, `main.rs:97783`, `main.rs:97844`, `main.rs:97694`; `crates/bt-app/src/seats.rs:453`.

**Scenario:** Paste a screenshot in tab A, then switch to B before conversion finishes. The answer stores only a generation and `SeatId`; adoption calls `paste_paths_into`, which resolves that seat in the *currently active tab*. Each new single-pane tab starts at `SeatId(1)`. Thus B receives A's path. The window-scoped generation changes on another picture paste, not on tab activation. Replacing a shell in the same seat also leaves the old request targeting a new session.

**Fix:** Capture the complete tab/seat identity and a session incarnation when accepting the paste. Deliver to that original live session, or discard an obsolete request. Do not resolve delayed input through `active_tab`. Add a completion-after-tab-switch/session-restart test.

### X-2 — must-fix — Concurrent picture workers can overwrite the same PNG

**Where:** `crates/bt-app/src/clipboard_picture.rs:71`, `:97`, `:158`, `:253`; `main.rs:97792`.

**Scenario:** Two windows paste pictures in the same second. Each worker enumerates the shared clipboard directory, computes the same next suffix, and calls truncating `fs::write` on the same path. A second paste in one window also leaves its superseded worker running. Either consumer can receive the other picture; overlapping writes can expose a partial/corrupt file. The existing same-second test exercises sequential name planning, not name reservation.

**Fix:** Reserve names with exclusive creation and retry, including across application instances; publish only complete files. Coordinate the retention sweep with writers. Atomic replacement alone does not prevent two requests choosing the same name.

### X-3 — should-fix — A stale completion can erase the newest paste

**Where:** `main.rs:733`, `main.rs:755`, `main.rs:97796`.

**Scenario:** Slow request A precedes fast request B. B stores its answer; before the UI consumes it, A unconditionally overwrites the single mailbox slot. `take_current` then removes and rejects A by generation, and B's queued notification finds an empty slot. The most recent paste silently disappears, despite generation checks intended to preserve it.

**Fix:** Check the generation while replacing the slot, using synchronization shared with withdrawal, or give each request its own completion slot. Bound the number of workers. Test reversed completions before either notification is consumed.

### X-4 — must-fix — Clipboard bitmap dimensions can exhaust the process

**Where:** `crates/bt-app/src/clipboard_picture.rs:231`; `crates/bt-platform/src/windows_clipboard.rs:131`, `:173`.

**Scenario:** A clipboard provider offers a tiny DIB header with huge valid dimensions. The new decoder calls `DynamicImage::from_decoder` without an application allocation limit. Pinned `image-0.25.10/src/codecs/bmp/decoder.rs:65,626` permits dimensions through 65,535; `image-0.25.10/src/io/free_functions.rs:305,316` allocates the output before reading its pixels. A 32,768-square RGB header requests about 3 GiB before a truncated body can be rejected. Allocation failure can abort the application; converting on a worker does not contain it. Acquisition also copies each offered encoding without a size cap, and concurrent workers multiply memory pressure.

**Fix:** Bound clipboard bytes, decoded dimensions and total decoded bytes before allocation; apply decoder limits and cap concurrent work. Reject oversized input with the existing toast. Verify with header-only fixtures without actually attempting giant allocations. This review did not run an OOM probe.

### X-5 — must-fix — The Mac wheel fix changes genuine horizontal gestures everywhere

**Where:** `main.rs:22246`, `main.rs:95755`, `main.rs:95985`, `main.rs:96045`, `main.rs:96539`, `main.rs:100726`.

**Scenario:** Hold Shift and use a genuine horizontal wheel/trackpad report (`x != 0, y == 0`). `upright_wheel` converts it to `(0, x)` for both pixel and line deltas, on every platform. The terminal's column-scroll path uses `x` for a horizontal report but `-y` for the rewritten report, reversing this gesture relative to 0.4.1. Settings/first-run can acquire vertical motion. Forwarded web-preview input loses its horizontal axis; with the platform zoom chord it can enter the nonzero-y zoom path instead. A physical horizontal Mac trackpad gesture is also indistinguishable from AppKit's rewritten vertical wheel at this layer.

**Fix:** Preserve raw axes and input provenance. Normalize only a known platform-translated gesture at an appropriate boundary, and retain separate terminal/web/modal routing semantics. Add genuine horizontal Shift-wheel cases on Windows and macOS, including Shift plus the zoom chord. A test asserting `(x,0)` becomes `(0,x)` alone codifies the regression.

### X-6 — should-fix — Frozen split formulas lose the opening row when another formula follows

**Where:** `crates/bt-detect/src/lib.rs:771`, `:905`, `:3805`; `crates/bt-term/src/session.rs:10817`, `:11120`, `:11231`, `:11331`, `:11409`.

**Scenario:** Freeze these two rows into history, then require detection at the closing row:

```text
Use the quadratic formula $x
= \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$, and the density $\varphi(x) = e^{-x^2/2}$.
```

The across-rows detector accepts the pair and returns two runs. But `may_close_row_split_inline_math` accepts only a lone-dollar census, so it rejects the three-dollar closing row. The frozen frontier advances before scheduling; with no earlier open display block, the join-window check fails and the candidate-only fallback omits the opening row. Persisting `InlineMathSite` preserves authority, not the missing text context. Direct scanner tests always supply the pair and miss this interaction.

**Fix:** Make join-window discovery recognize a valid first closer even when complete formulas follow it. Add a live-to-frozen-to-resized session test for this case. A temporary public-API regression test confirmed the mismatch: pair detection returned two runs, while the closing-row predicate was false. This proves the predicate inconsistency; the renderer was not exercised.

### X-7 — must-fix — A blocked trace writer can still block the watchdog and UI diagnostics

**Where:** `crates/bt-app/src/trace_sink.rs:285`, `:349`; `crates/bt-app/src/hang_watch.rs:2040`; `crates/bt-app/src/persist.rs:275`; `main.rs:97851`, `main.rs:111857`.

**Scenario:** Under `BT_PERF_TRACE`, stop consuming redirected stderr. The writer can block inside `Stderr::write_all`, holding Rust's shared stderr lock. The watchdog synchronously prints slow holds *before* calling `watch.poll`; it can block behind that same writer and never reach the new two-second decision. UI error paths still use `eprintln!`, including picture-save errors and the new session-save timeout diagnostic. They can also block before the bounded shutdown flush is reached.

**Fix:** Route UI diagnostics through a nonblocking path too. Keep watchdog polling/report-file capture independent of the console and perform it before any potentially blocking output. Exercise a blocked stderr consumer plus a slow hold and a timeout diagnostic. The queue's `try_lock`/`try_send` is sound; moving trace writes alone does not eliminate the shared-output dependency. Some direct diagnostics predate this batch, but the new writer and two-second promise must account for them together.

### X-8 — must-fix — The three-second timeout abandons quitting instead of closing

**Where:** `crates/bt-app/src/persist.rs:409`, `:425`; `main.rs:111851`; `crates/bt-app/src/quit.rs:359`; `CHANGELOG.md:61`.

**Scenario:** Quit while the session filesystem stalls. `wait_for` marks the writer stalled and returns `Err` after three seconds. The existing transaction then displays `QuitSessionNotWritten` and calls `quit.written(false)`, entering `Phase::Abandoned`, not `Retiring`. The advertised behavior that Folio closes and keeps the previous save is therefore not implemented for this quit path.

**Fix:** Distinguish timeout from ordinary save failure and explicitly wire the intended exit policy through the quit transaction, preserving the dirty-session sentinel. Test the entire write-to-retire transition. A detached in-flight atomic writer may still finish later, so promise a valid last-completed snapshot, not that the earlier snapshot can never change after timeout.

### X-9 — should-fix — Timed-out fallback session writers escape serialization and sentinel tracking

**Where:** `crates/bt-app/src/persist.rs:667`, `:715`, `:844`, `:891`.

**Scenario:** The persistent worker is unavailable and a one-shot fallback starts successfully but stalls. Timeout drops its handle without marking `SessionWriter::stalled`. A later retry starts another independent writer; an older snapshot can finish after a newer one. On close, the store can remove `session.lock` because the normal writer's stalled flag is false, while a fallback is still outstanding. This requires the fallback path, but violates the deadline feature's recovery guarantees precisely when that path matters.

**Fix:** Track fallback work in the same serialized writer/generation state, including timeout status and outstanding handles. Retain the sentinel until outstanding work is accounted for. Test two fallback timeouts/completions in reverse order; helper-level timeout tests do not establish this invariant.

### X-10 — should-fix — File-drop targeting samples a cached or later pointer position

**Where:** `main.rs:113135`, `main.rs:95811`, `main.rs:95896`; `crates/bt-platform/src/macos_impl.rs:652`.

**Scenario:** Drag from an external app into another pane after previously hovering a different pane. `DroppedFile` queues only paths. Flush prefers an existing cached `pointer_position` and queries native position only if it is absent. External drag loops need not deliver the cursor events needed to refresh that cache. Even the fallback queries the pointer at processing time, so moving away after release while the UI is busy can select a different pane. The promise to paste under the drop is stronger than the event data retained.

**Fix:** Carry the native drop coordinates/target through the backend and capture a stable tab/seat destination with the drop batch. Do not infer the release point from later cursor state. Verify external drops across split panes with and without a prior in-window hover and under queued output.

### X-11 — should-fix — Release/design claims exceed the implemented guarantees

**Where:** `CHANGELOG.md:61`, `:144`, `:171`, `:207`; `docs/DESIGN.md:98`, `:99`, `:8579`, `:8581`, `:9964`.

**Scenario:** Users and future maintainers read unconditional promises about closing after three seconds, nonblocking traces, and split-formula rendering, contradicted by X-6 through X-8. The repaint hold now spans one drain turn, not an arbitrarily long repaint stream: the existing drain budget can still divide it between turns (`main.rs:82758`; `crates/bt-term/src/session.rs:3026`). DESIGN's claim that pane shutdown has no unbounded wait also exceeds a bounded reader join: `crates/bt-pty/src/lib.rs:1723` drops the pseudoconsole before that join, and multiple pane/reap budgets are sequential, not one global quit bound.

The Mac narrative at DESIGN:8581 explicitly corrects the preceding explanation, but :9964 still asserts that winit answers NO to `mouseDownCanMoveWindow`. Pinned winit implements `mouseDown:`; the inspected view does not explicitly override `mouseDownCanMoveWindow`. An appended correction leaves two incompatible explanations of the current mechanism.

**Fix:** Update the current-design paragraphs together and limit release claims to behavior actually implemented/tested. State per-operation deadlines, not a global three-second exit. Describe drain-turn scope and conservative formula acceptance. Config backups also retain different policies: attention hooks keep the first dated copy while shell integration finds a free dated suffix (details below); do not imply identical per-edit histories.

### X-12 — note — macOS bindings fit, but blanket nonmovability needs behavioral evidence

**Where:** `crates/bt-platform/src/macos_impl.rs:1189`, `:1751`, `:1785`; `crates/bt-platform/src/macos_picture.rs:38`; `crates/bt-platform/src/macos_clipboard_payload.rs:53`; `Cargo.lock:2387`, `:2412`.

**Scenario:** The tab-drag fix makes the entire NSWindow nonmovable while empty-header dragging still calls `performWindowDragWithEvent` and double-click still invokes zoom/miniaturize. Pinned objc2 0.6.4 / AppKit and Foundation 0.3.2 provide these selectors; the bindings do not establish that explicit dragging overrides `movable=false`. Apple's [isMovable contract](https://developer.apple.com/documentation/appkit/nswindow/ismovable) also changes system display-reconfiguration placement for nonmovable windows. Its [performDrag contract](https://developer.apple.com/documentation/appkit/nswindow/performdrag%28with%3A%29?changes=l_5) requires the original mouse-down event and does not promise that override. This is an unresolved runtime risk, not a demonstrated inability to drag.

**Fix/verification:** Exercise tab reorder/tear-out, empty-header drag and snapping, Spaces, title-bar double-click preferences, and monitor removal/reconnection. If blanket nonmovability breaks these, suppress background dragging at the hit-test/view boundary. Preserve original mouse-down identity. Compiling on CI cannot settle this behavior.

Clipboard review against pinned generated `NSBitmapImageRep.rs:320,559` and `NSPasteboard.rs:459` found compatible argument/return types: `imageRepWithData` returns an optional retained representation; PNG conversion uses the matching property dictionary type; copied NSData bytes and the autorelease pool do not obviously outlive their owners. The change-count check rejects changed clipboard contents. TIFF decompression still deserves the same resource limits as X-4; a successful Objective-C type check does not bound decoder memory.

The cursor query checks the window-thread context, converts screen to window/view coordinates, handles flipped views, and scales to physical pixels. Pinned winit's view is flipped. No definite selector or ownership mismatch was found; cross-display scaling and drop-time correctness remain separate concerns (X-10). The Shift-wheel helper is actually cross-platform Rust, not a Mac-only compilation arm (X-5).

## Present gate: complete drawing-input audit

The root comparison is `crates/bt-app/src/present_gate.rs:5,25,62`; frame equality is `main.rs:117676`. The draw inventory comes from `crates/bt-render/src/lib.rs:8590` (`compose_frame`), not just the signature tests. No concrete missing input was found in the inspected candidate.

| Drawn input | Signature/invalidation path checked |
| --- | --- |
| Terminal cells, colors, text, decorations, row layout and scrolling | Per-seat picture revision from frame equality, including columns, rows, cells, horizontal origin, row maps, layout key and subpixel offset (`main.rs:117676`). |
| Caret position, shape and blink | Cursor in frame; style and sampled blink visibility in `RendererPresentState` (`crates/bt-render/src/lib.rs:4719,7593`). |
| Selection, search and current-search highlights | All three span collections compared by frame equality. |
| Math images/source, hover marks and failure marks | `math_blocks` compared; `pictures_match` additionally compares `math_failures` (`present_gate.rs:5`). Content-addressed render keys are assumed immutable. |
| Tween phase, pane animation and clips | Sampled frame math/offset state plus per-seat viewport/clip/focus in `main.rs:100994`; retained path samples pane draws before comparison (`main.rs:101129`). |
| Notices/status text | Frame `status_text`; window notices are chrome/modal geometry. |
| Wheel thumb and its fade | Sampled chrome quads/layers update the renderer revision; wheel-clamp handling still requests chrome wakeups (`main.rs:96607`). |
| IME preedit | Terminal preedit composed before skip decision (`main.rs:65362`); text-field/modal composition flows through retained layers. |
| Files column, tab strip, tab attention, first-run card and Settings | Chrome quads, labels and icons plus modal layers, compared by setters (`crates/bt-render/src/lib.rs:8200,8232`; `main.rs:40683,45404`). |
| Image peeks/previews, pan/zoom/pointer state, preview document bodies and table blocks | Revision-bearing setters (`crates/bt-render/src/lib.rs:7648,7691,7794,7862,8040`). Table preparation precedes the retained-path gate (`main.rs:101141`). |
| Video frame and native-web holes/controller arrival | Video and web-hole setters (`crates/bt-render/src/lib.rs:7778,7840`), plus native-page state in `main.rs:100994`, even when hole geometry is unchanged. |
| Window focus, font, theme, ground opacity/wallpaper | Direct renderer focus/font/theme state; ground changes bump theme revision (`crates/bt-render/src/ground.rs:165`). |
| DPI, physical dimensions and surface generation | Signature and renderer state (`main.rs:100994`; `crates/bt-render/src/lib.rs:7593,8054`); surface recreation cannot reuse an old stamp. |

Frame `view_generation` is deliberately excluded: publication identity alone is not a different picture. Frame `math_failures` is not lost despite its omission from the older helper. Seat identity includes tab and seat, so this path does not share X-1's identity mistake.

The common funnel invalidates before an attempted render and stamps only a successful `Presented`, after renderer changes (`main.rs:100813,100846,100904`). Textless, failed and reconfigured frames cannot authorize a skip. Chrome-only/shell-less and retained presents use the same gate (`main.rs:65093,101114`). Signature mutation tests are useful but would not discover a future compose input omitted from both signature and tests; keep this inventory beside the gate.

## Requested interactions and daily-use regressions

| Interaction / user action | Trace and assessment |
| --- | --- |
| Drain slicing + turn-wide repaint hold; typing amid output | `main.rs:82758` opens/closes holds around the drain operation, including its returned-error path. `crates/bt-term/src/session.rs:2886,3026` retains DEC synchronized-update semantics instead of forcing a commit per slice. Filtered feed-turn tests passed. No additional concrete typing regression found; a repaint spanning multiple budgeted turns is outside the new guarantee (X-11). |
| Present gate + unchanged wheel publication + retained present; long-history scrolling | `main.rs:96598` compares projection movement; `main.rs:86428` uses skip-unchanged publication. Unpainted siblings remain accounted for at `main.rs:65371`, and thumb wakeups survive clamps. Retained draws are prepared before the gate. No missing-picture case found; real horizontal gestures regress independently (X-5). |
| Trace sink + two-second watchdog | Nonblocking producers do not isolate stderr consumers; X-7. |
| Transcript inline site + across-row formula + resizing history | The site is stored before scheduling (`crates/bt-term/src/session.rs:10794`) and survives frozen history. Its authority cannot repair the omitted predecessor (X-6). The resize adapter's removed speculative fork does not remove the actual terminal state copy. |
| Stable profile IDs + persistence/restore + switching/restarting tabs | `main.rs:10374,16284,31668,34099,34243`; `crates/bt-app/src/profiles.rs:3467,4775`: lifetime-bearing state keeps IDs, resolves current rows/programs, and falls back for a removed profile. No stale-index dereference or session-schema incompatibility found. Ordinary switching is covered by the gate's tab/seat identity; delayed picture paste is not (X-1). |
| Quit deadline + PTY reader bound + session writer; closing panes/windows | The reader wait is bounded after ring/pseudoconsole teardown (`crates/bt-pty/src/lib.rs:1723`), but the quit state machine does not retire on save timeout (X-8), and fallback writes escape its tracking (X-9). These are not one global wall-clock shutdown deadline (X-11). |
| Config backup + shell-integration backup | `crates/bt-app/src/attention_hooks.rs:516,600` distinguishes NotFound from unreadable and refuses mutation on read failure. It follows the physical target and skips an existing dated backup. `crates/bt-app/src/shell_integration.rs:1453,1476` tries dated suffixes. Normal destinations differ; no new shared-path collision found. These pre-existing backup conventions remain different, not a common per-change archival guarantee. |
| Mac tab drag + Shift-wheel + existing drag/zoom | Window nonmovability has unverified whole-window effects (X-12). Shift normalization occurs before per-surface dispatch and can change web zoom/horizontal motion (X-5); neither change validates the other's assumptions. |
| Pasting and dropping during navigation | Plain text still follows the existing paste route; new picture completion and drop routing introduce identity/position races (X-1, X-3, X-10). Picture byte persistence additionally needs X-2/X-4. |
| Settings, first-run card, web preview and ordinary resize | Chrome/modal and surface revisions are represented in the gate. No definite new failure to open/draw these surfaces was found. X-5 affects wheel dispatch into them; native web arrival is explicitly signed. Mac preview/player pools retain owned objects beyond each pool; no specific new use-after-free was found by source inspection. |

## Crash-safety and language pass

`present_gate.rs:71` has a checked revision increment followed by `expect`; exhaustion requires an impractical number of changes and is not a release blocker. Production trace-sink mutex poisoning is recovered and producer contention drops records; its operational problem is X-7, not an unchecked queue index. Clipboard filename parsing uses checked parsing and saturating suffix/count arithmetic; saturation can reuse a name at `u32::MAX`, reinforcing the need for exclusive creation in X-2. The practical crash exposure is unchecked decode allocation (X-4). An eight-byte PNG signature alone is also accepted as a successful PNG (`clipboard_picture.rs:214`), so truncated PNG data bypasses a valid lower-priority encoding; validate it before accepting the preferred representation.

Profile ID lookups at spawn/restore are fallible rather than unchecked table indexing. Drop routing validates the selected seat but does not preserve its historical identity/position. Native cursor calls check failure and thread context; float-to-integer casts do not create a Rust indexing panic. Deadline waits return errors rather than unwrap, but their callers must implement the intended transaction outcome. No `border.rs` exists in this candidate. Test-only unwraps were not counted as production defects.

New English UI text for the Chinese pass: `crates/bt-app/src/i18n.rs:2786` / `Text::PasteClipboardPicture`: **“The clipboard picture could not be saved. Copy again and retry.”** Both language branches currently contain that English sentence; both platform cases are listed as pending at `i18n.rs:5816`. Translate the Chinese branch and remove its pending entries once reviewed. New stderr diagnostics (picture conversion/start failure, session-save timeout, detached reader, trace drops) remain developer-facing English rather than bilingual UI entries.

## Validation and release decision

- `cargo test -p bt-detect row_split -j 4`: 2 selected tests passed.
- `cargo test -p bt-term --lib feed_turn -j 4`: 2 selected tests passed.
- `cargo test -p bt-detect release_review_frozen_join_window -j 4`: the temporary regression test failed as expected for X-6; pair detection succeeded and the frozen-window predicate rejected its closing row. The scratch test was deleted after recording the result.
- No workspace-wide build, GUI smoke run, macOS execution, or destructive/OOM experiment was performed. Source-backed concurrency findings are not presented as reproduced GUI failures.

**Verdict: ship after must-fixes X-1, X-2, X-4, X-5, X-7 and X-8.**
