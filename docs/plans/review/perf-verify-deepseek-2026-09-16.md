STATUS COMPLETE

# Verification of P-1, P-2, P-3 (perf-review-2026-09-16) against source

Target: 2a261ebfb84409f3ab196a2dd71574ba7f776aee, worktree D:/Developer/bt-wt/perf-audit.
Method: read-only source inspection (grep/read only; no cargo, no launch, no edit). Only P-1/P-2/P-3 were re-checked.

## P-1 — synchronous acquire/submit/present on the event thread

**Verdict: CONFIRMED** — re-opened crates/bt-render/src/lib.rs:9632,9635,10479,10446,4506 and crates/bt-app/src/main.rs:99927,100337,100139,111446 at 2a261ebf.

**Claim vs. code.** The finding says acquire, submit and present all synchronously occupy the winit owner thread. The code does exactly that, in one synchronous function:

- `crates/bt-render/src/lib.rs:9632` `gpu.queue.submit([encoder.finish()]);`
- `crates/bt-render/src/lib.rs:9635` `AcquiredFrame::Swapchain(texture) => gpu.queue.present(texture),`
- `crates/bt-render/src/lib.rs:10479` `FrameTarget::Surface(surface) => match surface.get_current_texture() {` (the acquire, inside `fn acquire`, which is blocking on `get_current_texture`)
- `crates/bt-render/src/lib.rs:10446` `surface.configure(&gpu.device, &self.config)` (reconfigure, when `configured_size != config` per :10432)
- `crates/bt-render/src/lib.rs:4506` `config.desired_maximum_frame_latency = 1;`

The timestamps the report's measurement depends on are real: `submitted_at` at :9633 and `present_called_at` at :9640 bracket the present call. The whole sequence lives in `present_frame`, which `present_seats_and_commit` calls synchronously (`renderer.present_frame(gpu, seat_frames, trigger)?` at crates/bt-app/src/main.rs:99927). `present_seats_and_commit` is reached from both `redraw` (main.rs:100337) and `present_retained_picture` (main.rs:100139), and `redraw` is the winit handler `WindowEvent::RedrawRequested => runtime.redraw()` (main.rs:111446). So acquire+submit+present run inline on the owner thread for every composed and retained present.

**Is the cost real on the felt paths?** Yes. Typing reaches it through Keyboard→`redraw` after echo; mouse reaches it through Expose (chrome-hover/retained `present_seats_and_commit`). It is a blocking/synchronous call occupying the event thread — not row composition, not a worker. The finding's caveat that this measures *call duration, not physical scanout* is correct and I confirm it (the receipt stops at `present_called_at`, before `compositor.commit()` at main.rs:99963).

**Fix.** The proposed "capacity-one latest-frame renderer handoff with resize generations" is the correct shape, and its two stated constraints are real: `surface.configure`/`get_current_texture` must stay serialized against present (the renderer's `configured_size` at :10469 is the resize generation), and `compositor.commit()` (main.rs:99963, DirectComposition/COM) must remain on the owner thread. It is not the *smallest* step (a bounded render thread taking acquire/submit/present while the owner keeps configure/commit is the minimum), but it is a faithful general fix, not a shim. "Latency=1 alone" being only an experiment is accurate.

**Missed.** Nothing material. One minor boundary worth naming: `compositor.commit()` and `compositor.set_covered_size()` (main.rs:99958-99965) are also synchronous COM work on the owner thread *after* the present receipt; the finding already flags them as "outside the receipt", but they are part of the same event-thread present stall and should move with the same isolation plan, not be left on the owner while the GPU lane is handed off.

## P-2 — every present re-shapes chrome/preview and recreates GPU geometry

**Verdict: CONFIRMED** — re-opened crates/bt-render/src/lib.rs:11892-11941,8771,8518-8573,8690-8702,10013 and crates/bt-app/src/main.rs:100139 at 2a261ebf.

**Claim vs. code.** The finding says `shape_chrome_labels` builds a fresh cosmic-text Buffer and shapes each label, preview paragraphs are re-shaped, per-seat vertex buffers are recreated (empty layers allocate dummy buffers), and icon lists are cloned twice. The code matches:

- `crates/bt-render/src/lib.rs:11930` `let mut buffer = Buffer::new(font_system, Metrics::new(label.font_size_px, line_height));` (per label), then `:11940` `set_chrome_label_text(...)` and `:11941` `buffer.shape_until_scroll(font_system, false);`
- `crates/bt-render/src/lib.rs:8771` `preview_text_layouts.extend(shape_preview_body(&mut gpu.font_system, body, 1.0));`
- `crates/bt-render/src/lib.rs:8518-8573` four `gpu.device.create_buffer_init(...)` calls per seat (ground, rect, status, math-overlay), each falling back to `empty_rect.as_slice()` when the layer is empty — i.e. empty layers still allocate a buffer.
- `crates/bt-render/src/lib.rs:8695-8698` `.iter().cloned().partition(|icon| !icon.above_text)` then `:8700/:8702` two `prepare_chrome_icon_draws` calls, each of which starts `let icons = icons.to_vec();` at `:10013`. So the icon list is cloned once by partition and once more by `to_vec` per partition half.
- Retained redraws go through the same funnel: `present_retained_picture` → `present_seats_and_commit` (main.rs:100139), so an unchanged terminal picture with a hover/fade/caret change still re-shapes and reallocates.

**Is the cost real?** Yes on typing, Expose, PtyOutput and retained presents alike: it is O(drawable chrome/preview text + visible geometry) with fresh CPU allocation and GPU buffer creation per frame, on the event thread. The report's own caveat — `rectangles_us` (:9694) is a broad interval (math-prepared→rectangles-prepared), not shaping-only — is correct and I confirm it.

**Fix.** Retain shaped chrome/preview text keyed by text/font/width/layout revision, reuse growable instance/vertex buffers with range uploads, and omit empty layers. This is the right general fix and it names the real invalidation axes (DPI/font/content/width; sharing composed and retained caches). It is not over-broad: the two text lanes share the same `shape_*` helpers, so one revision-keyed cache serves both. The minimum correct first step would be narrower (skip shaping when the chrome-label/preview set is byte-identical to last frame), but the finding's version is a superset, not a hack.

**Missed.** The per-seat `status_rect_buffer`/`math_overlay_buffer` dummy allocations (:8545-8573) are the cheapest form of the same defect, but the finding groups them correctly. One point not stated: `prepare_chrome_icon_draws` re-uploads missing tiles via `gpu.upload_rgba_tiles` inside the hot path (:10019-10020) — for a chrome icon whose texture is already resident this is a hash lookup, but a cold icon uploads synchronously during the present, which the "retain/reuse" fix should also cover by ensuring icon keys persist.

## P-3 — whole-screen image/path detection repeats on feed and twice on publication

**Verdict: CONFIRMED** — re-opened crates/bt-term/src/session.rs:2891,3003,9694-9708,9845-9868,9892-9918,9934,2641-2666,2731-2812,7576,7618,2854 and crates/bt-app/src/main.rs:64840,64908,34112,95930 at 2a261ebf.

**Claim vs. code.** The finding says (a) each nonempty PTY feed re-scans the whole live screen for image paths then for OSC-8 links, and (b) each publication scans frame references twice. Both are true.

- Feed path: `feed_at` (session.rs:2891) runs, on `result.is_ok()`, `self.reconcile_live_image_paths(false, &vec![false; self.live_rows.len()]);` (session.rs:3003). Its only gate is `if !self.math_layout_options.detect_image_paths { return; }` (:9695). The band gate `if band_gates && !inline_image_bands_admitted(...)` (:9704-9706) never fires because `INLINE_IMAGE_BANDS = false` (session.rs:687) — so the alternate-screen exclusion does **not** disable this work. `detected_live_image_paths` (:9892) walks `for row in 0..self.live_rows.len() as u32` via `for_each_live_logical_line` (:9848), building a joined `logical_text` String per logical line and calling the detector, then `detected.extend(self.detected_live_image_links(stable));` (:9916) scans all rows again for OSC-8 `file://` targets (:9936-9949).
- Publication path: `viewport_frame` (:7576) calls `self.decorate_image_reference_affordance(&mut frame)` (:7618), which calls `frame_image_references(frame)` (:2855). `frame_image_references` (:2641) reconstructs text/cell maps row×column (:2743-2746) and detects paths (:2809). The app then calls it **again** for hover storage: `references: self.shell().session.frame_image_references(&terminal_frame)` (main.rs:64840), and the code comment at main.rs:64833-64835 literally says "The session scanned the same frame once more when it painted the resting dots inside `viewport_frame`". This second scan happens *before* the unchanged gate at main.rs:64908, so a frame later discarded as unchanged already paid for it.
- Enabled in this build: the struct default is `detect_image_paths: false` (session.rs:189) but the session builder overrides it — `detect_image_paths: true` (main.rs:34112).

**Is the cost real?** Yes, and it is O(visible cells/text), not O(document/scrollback): both `for_each_live_logical_line` and `frame_image_references` bound their walks to live rows / `frame.drawable_rows()`. It is pure CPU on the event thread (string/vec allocation + `detect_peek_image_candidates` per logical line), with zero required output when the screen shows no image references — i.e. it runs even in the traced alternate-screen, no-image case. The estimate of "two live traversals + two frame-reference traversals per feed-and-publish cycle" is accurate.

**Fix.** The core of the proposed fix — "compute frame references once and share the result between decoration and hover" — is the smallest correct change, and it is directly the split the code comment at main.rs:64833-64835 declines ("collapsing the two would mean hanging the scan on the session as state"). A correct version threads the reference list out of `viewport_frame` (or stores it once on the leaf next to `frame_image_references`), keyed by the frame it describes, invalidated on reflow/screen switch/hyperlink change and worker verification verdicts (the `verified` flag at session.rs:2803). The finding's additional "key live path results by logical-line content/CWD/settings revision, scan damaged lines only" is a further, still-general optimization, but it is optional, not the minimum. The `verify`/`image_path_is_verified` dependency is the one ownership constraint: decoration reads worker state, so any cache must be invalidated when a verdict lands, exactly as the finding states.

**Missed.** The feed-time re-seat scan (session.rs:3003) runs even though `create_and_retire=false` — it recomputes the full `detected_live_image_paths` just to re-anchor existing occurrences, with the `stable` slice passed as all-`false`. A content-keyed cache would let the "no new candidates" case return without re-walking; the finding's fix covers this, but the "re-seat only" semantics of the `false` call are worth a dedicated note because they are the cheapest to eliminate safely (re-anchor by stored anchors rather than re-detecting).

## Ranked summary

1. **P-1 — CONFIRMED (highest impact):** acquire/submit/present run synchronously on the winit owner thread (lib.rs:9632/9635/10479, main.rs:111446); this is the dominant measured stall and the fix must move the GPU lane, not just change latency flags.
2. **P-2 — CONFIRMED:** every present re-shapes chrome/preview text and recreates per-seat vertex buffers including dummy empty layers (lib.rs:11930/11941/8518-8573/10013); real 4 ms-median recurring CPU+GPU allocation on the event thread.
3. **P-3 — CONFIRMED:** each successful feed re-scans the live screen for paths and OSC-8 links (session.rs:3003/9694/9916) and each publication scans frame references twice (session.rs:7618 + main.rs:64840), all O(visible) but unconditional even with no images and on alternate screen.

STATUS COMPLETE
