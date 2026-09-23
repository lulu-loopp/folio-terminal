//! `frame` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    BrokerAim, Drag, FILES_NOTICE_DWELL, FOOT_REVEAL_FEEDBACK, Fading, FrameImageReferences,
    FrameTraces, GhostFace, HyperlinkActivation, PaneDraw, PointerTarget, PresentIntent, Runtime,
    STARTUP_PTY_POLL_INTERVAL, TabPress, a_bare_redraw_still_owes_a_present,
    advance_periodic_deadline, apply_hover_marks, attention_ledger_deadline, chrome_over,
    chrome_tick_reuses_picture, dispatch_tab_decoration_tasks, earliest_named_deadline, files,
    frame_matches_grid, git, hang_watch, hyperlink_activation, ime_outbound, mark_leaf_painted,
    math_copy_window, native_window, present_diagnostics, present_gate, preview,
    pty_drain_says_nothing_new, pty_frame_is_unchanged, seats, settings, settling,
    startup_poll_delay, take_math_worker_notice, trace_sink, trace_unchanged_present, webhost,
};
use anyhow::Context;
use anyhow::{Result, ensure};
use bt_layout::SeatId;
use bt_render::{
    FrameSource, FrameTrigger, PresentOutcome, compose_preedit, frame_content_digest,
    frame_is_alternate_screen,
};
use bt_viewport::{HyperlinkHit, ViewportFrame};
use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;

impl Runtime<'_> {
    /// **What the platform draws in this window's title bar** (M3-3, owner
    /// ruling 2026-09-12).
    ///
    /// The one capability read the window chrome makes, and the only one it is
    /// allowed to make: every caller below — the caption run, the strip's own
    /// origin, the hit test, the drag band, the picture — takes its answer from
    /// here, so the buttons that are drawn and the buttons that are clickable
    /// cannot come from two different opinions about whose title bar this is.
    ///
    /// **Not a `cfg`, and deliberately not.** `main.rs` is one of the eleven
    /// files §4.3 of the port plan lets name a platform, and naming one here
    /// would still be wrong: the answer is a fact about *this window*, measured
    /// off its own standard window buttons when the frame was installed, so a
    /// window with no native title bar to keep gets the same answer on macOS
    /// that every window gets on Windows. `first_run`'s rows are ruled the same
    /// way (M3-6), and the pin is
    /// `the_caption_run_is_decided_by_one_capability_read`.
    ///
    /// **One answer per window *state*, not one for the window's life** (§13.48).
    /// The measurement is still taken once at `install` and still asked of the
    /// window afterwards; what changed is that macOS takes this window's buttons
    /// off it in full screen and gives them back on the way out, so the frame
    /// measures again on those two transitions. Nothing here has to know that —
    /// the frame answers what it last measured — and the one thing that does is
    /// [`App::adopt_platform_chrome`], which asks this and re-draws the bar.
    pub(crate) fn platform_chrome(&self) -> bt_platform::PlatformChrome {
        self.window.custom_window_frame.platform_chrome()
    }

    /// Rebuild the chrome quads and labels from the current solve. Returns
    /// whether anything visible changed.
    pub(crate) fn refresh_chrome(&mut self) -> bool {
        self.refresh_chrome_with_overlay(true)
    }

    pub(crate) fn publish_frame(&mut self, trigger: FrameTrigger) -> Result<()> {
        let skip_unchanged = matches!(trigger.source, FrameSource::PtyOutput);
        self.publish_frame_inner(trigger, skip_unchanged)
            .map(|_| ())
    }

    /// A frame owed by something that lives in retained renderer state.
    ///
    /// The strip's ring, the caret's blink phase, a pane in flight: each of
    /// them has already written what it changed into the renderer — chrome
    /// quads through `set_chrome`, the blink through `set_cursor_blink_visible`,
    /// the transform through the tween the redraw samples — and what is left
    /// owing is a *present*, not a terminal picture. When the picture on the
    /// glass is already the newest one anybody composed, that is all this asks
    /// for; otherwise it falls through to the ordinary whole-window publish,
    /// which is what the caller did unconditionally before.
    pub(in crate::runtime) fn publish_chrome_frame(&mut self, now: Instant) -> Result<()> {
        if chrome_tick_reuses_picture(self.picture_on_glass()) {
            let bodies = self.pane_draws(now);
            let (seat_ids, seats) = Self::retained_seats(
                &self.window.tabs[self.window.active_tab],
                self.window
                    .last_presented_frame
                    .as_ref()
                    .filter(|_| self.focused().is_some()),
                &bodies,
                self.focused_leaf,
                self.window.renderer.seat_viewport(),
                self.keyboard_owner_is_a_shell(),
            );
            let signature = self.present_signature(&seat_ids, &seats);
            if self.app.gpu.device_loss().is_none()
                && self
                    .window
                    .present_gate
                    .unchanged(&signature, self.present_conditions(FrameSource::Expose))
            {
                trace_unchanged_present(
                    self.app.trace_perf,
                    &mut self.window.present_gate,
                    FrameSource::Expose,
                    self.window.last_presented_frame.as_ref(),
                    self.window.pending_frames.overwrites(),
                );
                return Ok(());
            }
            self.window.chrome_present_pending = true;
            hang_watch::during(hang_watch::Station::WindowRedraw, || {
                self.window.window.request_redraw()
            });
            return Ok(());
        }
        self.publish_frame(FrameTrigger {
            occurred_at: now,
            source: FrameSource::Expose,
        })
    }

    pub(in crate::runtime) fn publish_frame_inner(
        &mut self,
        trigger: FrameTrigger,
        skip_unchanged: bool,
    ) -> Result<bool> {
        hang_watch::during(hang_watch::Station::Present, || {
            // **Whatever asked for this frame, it carries every journey that is
            // running** (review 2026-09-18, P1). Above everything that reads a
            // height, a layer or a quad, because that is the whole of what it is
            // for: the alternative is a frame built from a sample some earlier tick
            // left behind, and a pane printing every five milliseconds composes
            // enough of those to hold a band still for as long as it keeps printing.
            // Costs a window with nothing moving one `bool` and one `Option` read.
            // See [`Self::carry_live_journeys`].
            self.carry_live_journeys(Instant::now());
            // **Every publish settles the PTY's debt**, and this is the one door they all come
            // through — a keystroke's frame, an expose, a resize's, the drain's own. What has been
            // fed is about to be on the glass, or is about to be found no different from what
            // already is, and either answers the question the wait was asking. Cleared here rather
            // than at each caller so that a publication path added later cannot forget: an armed
            // deadline nobody disarms is the failure [`coalesce::Pending`] is written against.
            self.window.pty_coalesce.settle();
            // **The tripwire.** A frame is about to describe a tab whose seat set
            // may have changed; if it changed without anyone carrying the change to
            // the shells, this is the moment the two pictures diverge and the last
            // moment anything can say so. Two `u64`s, in debug only — the cost of
            // the third instance of a bug that has now happened twice.
            debug_assert_eq!(
                self.window.shells_settled_revision,
                self.seats.structure_revision(),
                "a seat arrived or left without `settle_seat_set_change`: the panes \
             are about to be drawn at their new widths while their shells still \
             believe the old ones"
            );
            // **A tab with no shell composes no terminal picture** (§7.1.6h), and
            // that is the whole of what this branch says. Everything below composes,
            // holds, decorates and diffs *one shell's grid*; a folder tab has no
            // grid, so there is nothing here for it to do and nothing it could be
            // given a default of.
            //
            // It is not a frame that is skipped, though: what such a tab shows —
            // its column, its preview, the chrome around them — is retained renderer
            // state, and the debt it owes the glass is a **present**. So it takes the
            // door the strip's ring and the caret's blink already take, which is the
            // same reading [`Self::publish_chrome_frame`] makes about the same debt.
            // Answering `false` is then literally true: no picture entered the slot.
            if self.focused().is_none() {
                self.window.chrome_present_pending = true;
                hang_watch::during(hang_watch::Station::WindowRedraw, || {
                    self.window.window.request_redraw()
                });
                return Ok(false);
            }
            // Real-machine decoration-state trace (`BT_DECOR_TRACE=<path>`). Runs on every frame trigger
            // — including held/skipped frames — so a persistent stuck-source block is captured even when
            // the presented frame does not change. Zero cost when the variable is unset.
            self.shell_mut().session.trace_decorations();
            if matches!(trigger.source, FrameSource::Keyboard) {
                self.shell_mut()
                    .session
                    .release_presentation_hold_for_user_input();
            }
            // **The search re-asks itself here, before anything is projected** — so
            // a line that has just frozen, or a character the shell has just echoed,
            // is inside the hit set of the very frame that is about to draw it.
            //
            // Not forced: this is the rebuild *output* caused, so the match the
            // reader is standing on keeps its place (B58's first half), and the two
            // planes that did not move are not re-scanned at all (see
            // [`SearchScanCache`]). A frame that changed nothing about the search
            // costs one comparison of a fifty-row scan.
            self.refresh_search(false)?;
            let active = self.window.active_tab;
            let tasks = self.app.math_worker.tasks.clone();
            let scale_tasks = self.app.math_worker.scale_tasks.clone();
            let path_tasks = self.app.math_worker.path_tasks.clone();
            let window = self.window_id();
            dispatch_tab_decoration_tasks(
                window,
                &mut self.window.tabs[active],
                &tasks,
                &scale_tasks,
                &path_tasks,
                &mut self.app.math_worker_running,
                &mut self.app.math_worker_notice_pending,
            );
            self.window.composed_terminal_frames =
                self.window.composed_terminal_frames.saturating_add(1);
            let asked_about_paths;
            // **What the projection cost this frame** (`BT_PERF_TRACE projection`,
            // T-MATH-TOGGLE-STUTTER). The gesture that made this worth printing is
            // the formula source toggle: it changes the set of suppressed ids, which
            // is the one thing that sends `project` down its rebuild road, and the
            // question a person at the machine has to be able to answer is whether
            // that rebuild measured the lines that changed or the whole scrollback.
            // `lines_measured` is exactly that number. Off unless the switch is set,
            // and the reading costs one `u64` load when it is not.
            let trace_perf = self.app.trace_perf;
            let measurements_before = self.shell().projection.line_text_measurements();
            // **Which road the projection took**, beside how many lines it measured
            // (T-MATH-MARKS-IN-SOURCE-FACE). A frame of a change of face measures no
            // lines at all either way — the cache answers for every one of them — so
            // `lines_measured` alone cannot tell a band moved where it stands from a
            // band that made the whole document be pushed through two trees again.
            let rebuilds_before = self.shell().projection.rebuilds();
            let bands_moved_before = self.shell().projection.bands_moved();
            let mut terminal_frame = {
                // Bound once, to the focused leaf: `session` and `projection` are
                // two fields of one shell, and reaching each through its own deref
                // would be two borrows of the tab rather than one borrow of the
                // leaf.
                let leaf = self.window.tabs[active].shell_mut();
                let projection_started_at = trace_perf.then(Instant::now);
                leaf.session.refresh_projection(&mut leaf.projection);
                let refresh_us =
                    projection_started_at.map(|started_at| started_at.elapsed().as_micros());
                let frame = leaf
                    .session
                    .viewport_frame(&mut leaf.projection)
                    .context("project terminal grid into viewport frame")?;
                // **Written after the frame, not before it.** The three scroll numbers
                // are what this frame decided — where the window stands, how far it
                // could stand, and how much of that the blank tail under the prompt is
                // spending — and a line printed before `viewport_frame` would report
                // the frame before this one, which is the answer to a different
                // question than the one the reader is holding the log for. The timing
                // is still the refresh's own, taken above.
                if let Some(refresh_us) = refresh_us {
                    trace_sink::stderr_line(format!(
                        "BT_PERF_TRACE projection source={:?} refresh_us={refresh_us} lines_measured={} projected_lines={} rebuilt={} band_moved={} scroll_offset_subpixels={} scroll_extent_subpixels={} bottom_relief_subpixels={}",
                        trigger.source,
                        leaf.projection
                            .line_text_measurements()
                            .saturating_sub(measurements_before),
                        leaf.projection.projected_line_count(),
                        leaf.projection.rebuilds().saturating_sub(rebuilds_before),
                        leaf.projection
                            .bands_moved()
                            .saturating_sub(bands_moved_before),
                        leaf.projection.scroll_offset_subpixels(),
                        leaf.projection.scroll_extent_subpixels(),
                        leaf.projection.bottom_relief_subpixels(),
                    ));
                }
                // The frame that drew the names is the frame that discovered which of them nobody has
                // answered for (§7.1.5j). Taken here, one step after the projection wrote them down,
                // because the projection is what walks every line and the session is what owns a
                // worker.
                asked_about_paths = leaf
                    .session
                    .absorb_printed_path_probes(&mut leaf.projection)
                    != 0;
                frame
            };
            // State-driven frame hold. Review displacement holds a vanished scroll anchor during a
            // resize reprint. Independently, an unmatched off-band stale-pending DPI record holds the
            // previous complete formula frame while a proven primary reprint is between clear and exact
            // source re-anchor. Both release through projection/session facts (re-anchor, explicit user
            // takeover, or hard lifecycle retirement), never a timer.
            if self.shell().projection.presentation_hold()
                && self.window.last_presented_frame.is_some()
            {
                if self.app.trace_perf {
                    trace_sink::stderr_line(format!(
                        "BT_PERF_TRACE hold=presentation source={:?} review={} exact_source={}",
                        trigger.source,
                        u8::from(self.shell().projection.review_hold()),
                        u8::from(self.shell().projection.exact_source_reprint_hold()),
                    ));
                }
                return Ok(false);
            }
            if hang_watch::during(hang_watch::Station::DetectionPass, || {
                self.shell_mut()
                    .session
                    .schedule_visible_artifacts(&terminal_frame)
            }) != 0
                || asked_about_paths
            {
                dispatch_tab_decoration_tasks(
                    window,
                    &mut self.window.tabs[active],
                    &tasks,
                    &scale_tasks,
                    &path_tasks,
                    &mut self.app.math_worker_running,
                    &mut self.app.math_worker_notice_pending,
                );
            }
            // This frame's own references, scanned once for the whole of this frame's life on screen.
            // The hover upgrade below, the Ctrl+click verb and the peek all read this one list, so no
            // two of them can disagree about where a reference is or whether it is one. The session
            // scanned the same frame once more when it painted the resting dots inside `viewport_frame`;
            // collapsing the two would mean hanging the scan on the session as state, which is the very
            // kind of thing this affordance was rebuilt to be rid of.
            //
            // Kept on the leaf rather than on the window, because "this frame" is one pane's frame.
            let scan = FrameImageReferences {
                columns: terminal_frame.columns.get(),
                references: self.shell().session.frame_image_references(&terminal_frame),
            };
            self.shell_mut().frame_image_references = scan;
            // The pointer's marks, but only if the pointer is standing in *this* pane. The link's
            // underline and the verified reference's solid underline both follow the pointer
            // immediately; only what a hover *reveals* — a tooltip there, a thumbnail here — waits out
            // the 300ms settle. When the pointer is in another pane, that pane wears them and this one
            // is left alone, which `redraw` sees to.
            if self.window.hover_pane == Some(self.focused_leaf) {
                // **And a frame redrawn under a pointer that has not moved asks too** (closure
                // review B-1). This is the one place "what is under the pointer" is recomputed
                // without a `CursorMoved`: a wheel scroll and a fresh line of output both arrive
                // as a frame, and either can slide a link under a resting hand. One hit test and
                // one map lookup, and only while the pointer is over this pane.
                self.ask_about_the_link_under_the_pointer();
                let hovered_reference = self.hovered_image_reference();
                apply_hover_marks(
                    &mut terminal_frame,
                    &self.window.hyperlink_hover,
                    hovered_reference.as_ref().map(|(_, reference)| reference),
                );
            }
            // Asked first, because it answers a gesture the user has just made and
            // is waiting on, while the two below announce a background loss that
            // happened whenever it happened.
            //
            // **Held for a dwell rather than taken once.** The two notices below are
            // one-shots and can afford to be: they report a thread that died, and a
            // sentence about that is worth exactly one frame whenever it lands. This
            // one is an *answer* — the user double-clicked something and this is what
            // came back — and an answer that is gone before the next repaint is a
            // gesture that looks like it did nothing at all. The mock-up's own
            // click-feedback (B24) holds its word for 1300ms for the same reason.
            if let Some((notice, shown_at)) = self.window.files_notice.clone() {
                if Instant::now().saturating_duration_since(shown_at) < FILES_NOTICE_DWELL {
                    terminal_frame.status_text = Some(notice);
                } else {
                    self.window.files_notice = None;
                }
            }
            if terminal_frame.status_text.is_none()
                && let Some(notice) =
                    take_math_worker_notice(&mut self.app.math_worker_notice_pending)
            {
                terminal_frame.status_text = Some(notice.to_owned());
            }
            // The same one-shot, for the same kind of loss. Asked second so that a
            // window unlucky enough to lose both threads at once still says
            // something rather than nothing — the formula notice wins this frame and
            // this one keeps its flag for the next, because `take` only clears the
            // one it actually read.
            if terminal_frame.status_text.is_none()
                && let Some(notice) =
                    files::take_files_worker_notice(&mut self.app.files_worker_notice_pending)
            {
                terminal_frame.status_text = Some(notice.to_owned());
            }
            // The third of the same one-shot, in the same queue and last for the
            // same reason: each `take` clears only the flag it read, so a window
            // that loses all three threads says all three sentences, one per frame.
            if terminal_frame.status_text.is_none()
                && let Some(notice) =
                    preview::take_preview_worker_notice(&mut self.app.preview_worker_notice_pending)
            {
                terminal_frame.status_text = Some(notice.to_owned());
            }
            // The fourth, in the same queue and last for the same reason.
            if terminal_frame.status_text.is_none()
                && let Some(notice) =
                    git::take_git_worker_notice(&mut self.app.git_worker_notice_pending)
            {
                terminal_frame.status_text = Some(notice.to_owned());
            }
            let composed = if ime_outbound::enabled() {
                let (composed, written) =
                    bt_render::compose_preedit_traced(&terminal_frame, self.shell_preedit())
                        .context("reject non-rectangular frame before IME composition")?;
                self.trace_ime_frame(&terminal_frame, written);
                composed
            } else {
                compose_preedit(&terminal_frame, self.shell_preedit())
                    .context("reject non-rectangular frame before IME composition")?
            };
            if skip_unchanged
                && pty_drain_says_nothing_new(
                    pty_frame_is_unchanged(
                        self.window.pending_frames.pending_frame(),
                        self.window.last_presented_frame.as_ref(),
                        &composed.frame,
                    ),
                    self.window.unpainted_pane_output,
                )
            {
                if self.app.trace_perf {
                    let digest_started = Instant::now();
                    let digest = frame_content_digest(&composed.frame);
                    let alternate_screen = frame_is_alternate_screen(&composed.frame);
                    let digest_elapsed = digest_started.elapsed();
                    trace_sink::stderr_line(format!(
                        "BT_PERF_TRACE skip=unchanged source={:?} content_fnv={:016x} alt={} digest_us={} present_unchanged={} slot_overwrites={}",
                        trigger.source,
                        digest.content_fnv,
                        u8::from(alternate_screen),
                        digest_elapsed.as_micros(),
                        self.window.present_gate.unchanged,
                        self.window.pending_frames.overwrites(),
                    ));
                }
                return Ok(false);
            }
            // The frame is offered rather than read: this is the only moment the
            // grid's caret exists, and [`Runtime::offer_ime_caret`] decides whether
            // the grid is the rung that owns it.
            self.offer_ime_caret(Some(&composed.frame));
            self.shell_mut()
                .session
                .record_published_frame(&composed.frame, trigger.occurred_at);
            self.flush_resize_trace();
            // **The one line a picture becomes newer than the glass.** Everything
            // above this can hold, skip or decide the frame says nothing new; only
            // a frame that actually enters the slot moves the revision the
            // chrome-only path compares against.
            self.window.terminal_content_revision =
                self.window.terminal_content_revision.saturating_add(1);
            self.window
                .pending_frames
                .publish(composed.frame, trigger)
                .context("reject non-rectangular frame at publish boundary")?;
            hang_watch::during(hang_watch::Station::WindowRedraw, || {
                self.window.window.request_redraw()
            });
            Ok(true)
        })
    }

    /// **How far the Advanced group would be drawn open this frame**, quantised
    /// — [`Self::drawn_focus_reveal`]'s opposite number for the disclosure.
    ///
    /// `None` whenever no clock is on a group, which is every frame of every
    /// window that is not opening or shutting one, and every frame at all under
    /// reduced motion.
    pub(crate) fn drawn_advanced_reveal(
        &self,
        now: Instant,
    ) -> Option<(settings::SettingsCategory, u16)> {
        let (page, tween) = self.window.advanced_reveal?;
        let (reveal, moving) = tween.sample(now, self.app.motion);
        moving.then(|| (page, (reveal.clamp(0.0, 1.0) * 1000.0).round() as u16))
    }

    /// **How solid each pane's hover-revealed head furniture is this frame** —
    /// the `⌄`, the folder, the pop-out, the `×` and a preview head's tools.
    ///
    /// One number per seat, because the whole run reveals on one predicate
    /// ([`seats::head_run_revealed`]) and the design's own requirement is that it
    /// reveals *together*. Sent down as a slice; `seats` is given the predicate
    /// as well and falls back to it, so a caller who forgets this map draws the
    /// run exactly as it was drawn before there was a fade.
    ///
    /// **The list is every pane showing a run plus whatever is still leaving.**
    /// A hand crossing from one pane to the next leaves two entries for ninety
    /// milliseconds — one coming up and one going down — which is why this is a
    /// list at all and not the one seat under the pointer. Since 裁4 two can be
    /// up at once for a second reason: a menu standing on one head while the
    /// hand is in the pane beside it.
    pub(in crate::runtime) fn settled_head_ink(&mut self, now: Instant) -> Vec<(SeatId, f32)> {
        let motion = self.app.motion;
        // **Both arms of the predicate** (裁4, 2026-08-26). The register eases
        // toward whatever `seats::head_run_revealed` says, and it has to be the
        // same sentence the paint and the hit test read — easing toward
        // `pane_hover` alone would run the ninety-millisecond fade *out* the
        // instant the hand reached the list the head had just opened.
        let showing = self.head_run();
        let mut seats: Vec<SeatId> = self
            .window
            .settling
            .held()
            .into_iter()
            .filter_map(|key| match key {
                Fading::HeadRun(seat) => Some(seat),
                _ => None,
            })
            .collect();
        for seat in [showing.hovered, showing.raised].into_iter().flatten() {
            if !seats.contains(&seat) {
                seats.push(seat);
            }
        }
        seats
            .into_iter()
            .map(|seat| {
                let ink = self.window.settling.settle(
                    &Fading::HeadRun(seat),
                    0.0,
                    settling::Toward::eased(
                        f32::from(u8::from(seats::head_run_revealed(showing, seat))),
                        bt_render::HOVER_CHROME_FADE,
                    ),
                    now,
                    motion,
                );
                (seat, ink)
            })
            .collect()
    }

    pub(crate) fn publish_pty_drain_frame(&mut self, now: Instant, force: bool) -> Result<()> {
        let keyboard_at = self.pending_keyboard_at;
        let published = self.publish_frame_inner(
            FrameTrigger {
                occurred_at: keyboard_at.unwrap_or(now),
                source: if keyboard_at.is_some() {
                    FrameSource::Keyboard
                } else {
                    FrameSource::PtyOutput
                },
            },
            !force,
        )?;
        // **The hold belongs to the focused leaf; the panes beside it hold
        // nothing.** `publish_frame_inner` returns before the slot while a
        // presentation hold stands, so a sibling that has just spoken would be
        // frozen behind a decision that was never about it. Same shape as the
        // wheel bug [`Self::repaint_pane_change`] was written for, and the same
        // repair: run the compositor over the picture already on the glass, which
        // rebuilds every unfocused pane from its own projection.
        if !published && self.window.unpainted_pane_output {
            self.represent_on_screen_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Expose,
            })?;
        }
        // No shell, no synchronized-update window to be inside — the keyboard
        // debt is simply cleared, which is what `sync_open == false` already says
        // (§7.1.6h).
        let sync_open = self
            .focused()
            .is_some_and(|leaf| leaf.session.synchronized_update_deadline().is_some());
        if published || !sync_open {
            self.pending_keyboard_at = None;
        } else if self.app.trace_perf {
            trace_sink::stderr_line("BT_PERF_TRACE defer=synchronized-update".to_owned());
        }
        Ok(())
    }

    pub(in crate::runtime) fn frame_hit(&self) -> Option<bt_render::GridHit> {
        self.pane_frame_hit().map(|(_, hit)| hit)
    }

    /// Spend a click on the hyperlink run it landed on (§7.1.5g).
    ///
    /// `control` is the press's own modifier, carried here by the drag rather
    /// than read from the keyboard again. `click_no_drag` is `true` because that
    /// gate is already behind us: [`Self::finish_local_selection`] has
    /// established that the release came up on the cell the press went down on.
    /// It stays in the signature so the table can be tested with it false, which
    /// is what pins "a drag is only ever a selection".
    pub(in crate::runtime) fn activate_hyperlink(
        &mut self,
        seat: SeatId,
        hyperlink: HyperlinkHit,
        control: bool,
    ) -> Result<()> {
        let namespace = self.seat_path_namespace(seat);
        let namer = bt_transcript::paths::PathNamer::Pane(&namespace);
        let activation = hyperlink_activation(control, true, &hyperlink.uri, namer, &|path| {
            self.seat_path_verdict(seat, path)
        });
        // Which arm of the routing table this address fell into, the address it
        // fell there with, and — for a `file:` — what the address actually named
        // on this disk (`BT_MOUSE_TRACE`).
        //
        // The whole hit goes in, anchors included, because a link an application
        // broke across three printed rows is a link whose *identity* is the
        // question: a `uri` that arrives here truncated at a row boundary parses
        // to a path that is not there, and the arm it then takes is `None` —
        // which from outside the window is indistinguishable from the click
        // never having arrived at all.
        //
        // **The diagnostic asks the ledger, not the disk** (audit 3 C-2). It used to stat the
        // target inside the closure, on the argument that an unset gate asks nothing — true, and
        // beside the point once the gate *is* set: a diagnostic must not be the thing that stalls
        // the loop on a cold share, and it is the same rule the table one line up now obeys.
        // `verdict=?` is a name nobody has answered for, which the table reads as "not a link".
        self.mouse_trace(|| {
            let named = bt_platform::file_uri_to_path(&hyperlink.uri).map_or_else(
                || "path=unparsed".to_owned(),
                |path| match self.seat_path_verdict(seat, &path) {
                    None => format!("path={} verdict=?", path.display()),
                    Some(verdict) => format!(
                        "path={} exists={} dir={} exec={}",
                        path.display(),
                        u8::from(verdict.exists),
                        u8::from(verdict.directory),
                        u8::from(verdict.executable),
                    ),
                },
            );
            format!(
                "activate_hyperlink control={} uri={:?} arm={activation:?} {named} hit={hyperlink:?}",
                u8::from(control),
                hyperlink.uri,
            )
        });
        match activation {
            HyperlinkActivation::None => {}
            // **Through the one hand-off, like every other address that leaves**
            // (R1-26). This arm used to call `shell_execute` itself, which is
            // the raw bridge: it takes whatever string it is given and asks the
            // machine what is registered for the scheme. The door in front of
            // it — [`Self::hand_url_to_the_browser`] — is what puts the address
            // through `webnav::address_bar` first, and that is the same
            // judgement the seat's half of this row already makes. A refusal is
            // said out loud here for the reason the page arm below says one: a
            // press asked for something.
            HyperlinkActivation::Browser => {
                if !self.hand_url_to_the_browser(&hyperlink.uri)? {
                    self.window.hyperlink_hover.show_blocked(hyperlink);
                    self.publish_interaction_frame()?;
                }
            }
            // **The same address, kept in this window** (§7.1.5g ①). One door
            // and not a second: `webnav::address_bar` is what an address typed
            // into the head, restored from a session or read out of `pins.json`
            // goes through, and `open_web_page`'s own contract is that every
            // caller has passed it first. A link printed in the terminal is a
            // string from outside exactly as those are, so it is judged by the
            // same rule and lands on the same seat — which is also what keeps
            // the terminal and the address field from giving two answers about
            // one string.
            //
            // **The door itself moved out of this arm on 2026-08-29**
            // (§7.1.5g ⑦) when a link written into a document turned out to be
            // asking the same thing: it is [`Self::open_web_address_here`], and
            // both surfaces call it rather than each spelling one judgement.
            HyperlinkActivation::Page(url) => {
                // **A refusal is said out loud, because a request was made.**
                // The silence this whole slice is about was a plain click that
                // asked for something and produced nothing at all; a plain click
                // that asks for an address this window will not load owes the
                // reader the same sentence `Ctrl` has always got — and on this
                // surface that sentence is the hover line, under the very cells
                // the address is printed in.
                if !self.open_web_address_here(&url)? {
                    self.window.hyperlink_hover.show_blocked(hyperlink);
                    self.publish_interaction_frame()?;
                }
            }
            // The files column's own road, entered at the same door
            // ([`Self::open_preview`]): a picture down the decode lane, everything
            // else through the tab's pool, and what even that cannot read gets the
            // "no preview" card whose one button is the system's handler. A share
            // arrives here too and meets §7.1.3's own refusal, which is a card
            // this window already has words for.
            // **The one door out of this window that takes a *path*** — the same one an
            // unpreviewable file's card offers, and the same one that will not start a program.
            // A page leaves through it too: the handler this machine has registered for `.html`
            // is its browser, so the ruling's "system browser" and the table's "system handler"
            // are one call and not two.
            //
            // **Fed by the ledger** (audit 3 C-2 + owner ruling 2026-09-21). The verb is main's;
            // what changed is that the three questions the door used to ask a disk on this thread
            // — is it there, is it a folder, would opening it run it — were answered by a worker
            // and travel here. On Windows the door asks no disk anyway; on a Mac it canonicalised
            // and stat-ed, which is a stall on a mounted share.
            HyperlinkActivation::External(path) => {
                // **The printed spelling, exactly as `main` handed it over.** The Windows door
                // asks `names_a_program` of the name it is given and `main` gave it this one; the
                // resolved name rides on the target, which is where the macOS door — the one that
                // *did* resolve — takes it from.
                let facts = self.verified_target(seat, &path);
                self.open_local_path_verified(&path, facts);
            }
            HyperlinkActivation::Preview(path, at) => self.open_preview_at(path, at)?,
            // **Shown where it lives, whatever it is** (audit 3 C-4). A folder took this arm from
            // the beginning; a file takes it since the day `ShellExecuteW`'s `open` verb turned
            // out to be an interpreter for half a dozen extensions nobody had listed. Nothing
            // here starts a program on either platform — Explorer selects the row, Finder selects
            // the icon — so a label an attacker chose buys a window opening and not a process.
            // [`Runtime::open_local_path`] is one gesture away, from the surfaces where the
            // *user* picked the file.
            HyperlinkActivation::Reveal(path) => {
                // **And it is revealed off the ledger, not off the disk** (closure review of
                // audit 3 C-2). The asking door stats the target and resolves its spelling on
                // this thread; the ledger already answered both — the name is there, and it is a
                // folder or it is not — and a `Ctrl`+click on a path under a junction into a dead
                // share must not stall the window inside a call this branch exists to remove.
                let facts = self.verified_target(seat, &path);
                self.reveal_verified(&path, facts);
            }
            // The folder's own road, and the one that stays in this window: the
            // column this tab already has is pointed at it, and a tab without one
            // gets the column `Ctrl+Shift+B` would have opened.
            // **The soft verb since 2026-08-25**, which is not a change to this
            // arm's meaning but to what "point the column at this folder" means
            // everywhere: a folder printed by a program is very often inside the
            // tree the column is already rooted at — a prompt's own cwd always
            // is — and re-rooting on it threw a whole repository away to show a
            // subtree of it. Outside the tree, `locate_folder_in_files_column`
            // is the old verb exactly.
            HyperlinkActivation::FilesColumn(path) => self.locate_folder_in_files_column(&path)?,
            HyperlinkActivation::Blocked => {
                self.window.hyperlink_hover.show_blocked(hyperlink);
                self.publish_interaction_frame()?;
            }
        }
        Ok(())
    }

    pub(in crate::runtime) fn publish_interaction_frame(&mut self) -> Result<()> {
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// Put the frame that is **already on the glass** back into the presentation
    /// slot, so that [`Self::redraw`] runs and rebuilds every *other* pane from
    /// that pane's own projection.
    ///
    /// The focused pane's pixels are unchanged and stay unchanged — whatever
    /// declined to compose a new picture for it is honoured to the letter — but
    /// the compositor pass is the only thing an unfocused pane is ever drawn by,
    /// and it runs for a frame in the slot and for nothing else. So this is how a
    /// pane that is not the keyboard's gets to the screen when the keyboard's
    /// pane has nothing to say: not by inventing a picture for the held pane, but
    /// by running the pass its neighbours are painted in.
    pub(in crate::runtime) fn represent_on_screen_frame(
        &mut self,
        trigger: FrameTrigger,
    ) -> Result<()> {
        // Not while a resize present is outstanding: that gate admits only the
        // newly projected grid, and the frame on screen is the previous one.
        if self.pending_resize_present.is_none()
            && self.window.pending_frames.pending_frame().is_none()
            && let Some(frame) = self.window.last_presented_frame.clone()
        {
            self.window
                .pending_frames
                .publish(frame, trigger)
                .context("re-present the on-screen frame for an unfocused pane")?;
        }
        hang_watch::during(hang_watch::Station::WindowRedraw, || {
            self.window.window.request_redraw()
        });
        Ok(())
    }

    /// The docked chrome under the pointer, or `None` where a window is standing.
    ///
    /// [`Self::pointer_target_at`] read by everything that speaks
    /// [`seats::ChromeTarget`] and nothing else — which is most of this window:
    /// the hover, the press, the release, every "is the pointer on *that*
    /// button" question. A point a float has claimed is `None` here because a
    /// window has no `ChromeTarget` to give: its head, its grip and its `DOCK`
    /// are [`float::FloatPart`]'s, and the one thing this answer must never be
    /// is the chrome the window is covering.
    pub(crate) fn chrome_target_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<seats::ChromeTarget> {
        match self.pointer_target_at(position)? {
            PointerTarget::Chrome(target) => Some(target),
            PointerTarget::Float(..) => None,
        }
    }

    /// Put the frame already on screen back in the slot so a pure chrome change
    /// reaches the glass. Chrome lives beside the frame, exactly as the peek
    /// flyout does, so `redraw` would otherwise find nothing queued and skip.
    pub(crate) fn present_chrome_change(&mut self) -> Result<()> {
        if self.pending_resize_present.is_none()
            && self.window.pending_frames.pending_frame().is_none()
            && let Some(frame) = self.window.last_presented_frame.clone()
        {
            self.window
                .pending_frames
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::Expose,
                    },
                )
                .context("re-present the on-screen frame for a seat chrome change")?;
        }
        hang_watch::during(hang_watch::Station::WindowRedraw, || {
            self.window.window.request_redraw()
        });
        Ok(())
    }

    /// **Tell the broker where the hand is and what it is carrying** — the
    /// source's whole half of a cross-window gesture (multiwindow slice F2).
    ///
    /// The label and the arriving subtree are refreshed on every move rather than
    /// captured once, and both for the same reason the local ghost re-reads its
    /// tab every frame: a tab renamed under a gesture in flight says its new name
    /// over the target's glass too, and a pane whose sibling closed is a
    /// different subtree than it was a moment ago.
    ///
    /// `ours` is passed in rather than asked again: [`Self::drive_drag`] has
    /// already spent that syscall on this very pointer, and asking twice invites
    /// two answers about one instant.
    pub(in crate::runtime) fn publish_to_broker(
        &mut self,
        drag: &Drag,
        position: PhysicalPosition<f64>,
        ours: bool,
    ) {
        let face = self
            .drag_label(&drag.source, bt_render::chrome_palette())
            .map(|(mark, mark_logical, colour, text)| GhostFace {
                mark,
                mark_logical,
                colour,
                text,
            });
        // The arriving shape, for the one question only the target can answer.
        // A pane is a one-leaf subtree — §7.1.6k′'s own finding, that a foreign
        // pane was always `DropCargo::Layout` and never a third arm.
        let cargo_tree = drag.source.pane().and_then(|leaf| {
            self.tab_state(leaf.tab)
                .and_then(|tab| tab.seats.tree().find_seat(leaf.seat).cloned())
                .map(bt_layout::LayoutNode::seat)
        });
        let screen = self.to_screen(position);
        let Some(broker) = self.app.drag_broker.as_mut() else {
            return;
        };
        broker.face = face;
        broker.cargo_tree = cargo_tree;
        if let Some(screen) = screen {
            broker.pointer = screen;
        }
        // Coming home is stated here and now, so that the frame drawn on the
        // pointer move that re-entered this window is already the ordinary one.
        // Leaving is *not* stated here: which other window the hand went to is a
        // question only the application can answer, and it answers it before this
        // turn ends.
        if ours {
            broker.aim_at(BrokerAim::Home, Instant::now());
        }
    }

    /// Whether a surface that covers the **whole window** stands over the panes.
    ///
    /// Only the four that draw a scrim. A menu or a popup is drawn as an overlay
    /// layer *after* the hole and therefore covers it correctly on its own
    /// rectangle; a scrim covers every rectangle, and a hole punched through one
    /// would be a page read clearly through a dimmed window.
    /// **The boxes of this window's own chrome standing over a page** (M4-3) -
    /// what `bt_platform::Compositor::set_page_cover` is told, and nothing but a
    /// reading of two things this frame already decided.
    ///
    /// `above` is where the page's hole is punched ([`bt_render::WebHole::above`]):
    /// the layers at and below it are drawn *under* the page and the ones after
    /// it are drawn *over* it, which is the same sentence the renderer paints by.
    /// `None` is a docked page, whose hole stands under the whole stack, so every
    /// layer is over it; `Some(level)` is a floated page, whose hole is punched
    /// above its own float's face precisely so that the page is not covered by
    /// the pane it is in.
    ///
    /// **Why this exists at all.** On Windows the question never arises: a
    /// WebView2 composed into a visual is in no hit-test order, every press in
    /// the window is Folio's, and Folio forwards to the page the ones over it. A
    /// `WKWebView` is a real view in the window's own hierarchy and AppKit routes
    /// presses to it directly, so a search capsule, a `⌄` menu or a tooltip drawn
    /// across a page would be chrome nothing could press. The macOS compositor
    /// hands these to the page's slot; the Windows one drops them with a line
    /// saying why.
    ///
    /// Only the layers that actually **overlap** the page are sent: a menu on the
    /// other side of the window is not a hole in this page, and a list of every
    /// layer in the stack would be a per-frame allocation for nothing.
    pub(in crate::runtime) fn chrome_over(
        &self,
        above: Option<usize>,
        body: Option<[f32; 4]>,
    ) -> Vec<[f32; 4]> {
        chrome_over(&self.window.overlay_bounds, above, body)
    }

    pub(in crate::runtime) fn begin_present_attempt(
        &mut self,
        source: FrameSource,
        retained: bool,
        owed: bool,
    ) -> present_diagnostics::Attempt {
        let owed = owed || self.picture_is_owed();
        let now = present_diagnostics::timestamp(Instant::now());
        let state = &mut self.window.present_diagnostics;
        state.observe(owed, now, present_diagnostics::progress());
        state.sequence += 1;
        let window = u64::from(self.window.window.id());
        let generation = self.window.renderer.surface_generation();
        hang_watch::present_attempt(window, generation, state.sequence);
        present_diagnostics::Attempt::new(
            window,
            generation,
            state.sequence,
            source,
            retained,
            self.app.trace_perf,
            now,
        )
    }

    pub(in crate::runtime) fn finish_present_attempt(
        &mut self,
        mut attempt: present_diagnostics::Attempt,
        result: &Result<()>,
    ) {
        attempt.phase(None);
        hang_watch::end_present_attempt();
        let instant = attempt.landed_at.unwrap_or_else(Instant::now);
        let now = present_diagnostics::timestamp(instant);
        if result.is_err() && attempt.outcome == present_diagnostics::Outcome::NoPicture {
            attempt.outcome = present_diagnostics::Outcome::FailedRender;
        }
        let state = &mut self.window.present_diagnostics;
        state.attempted(&attempt);
        if self.app.trace_perf {
            hang_watch::during(hang_watch::Station::DiagnosticWrite, || {
                let native = native_window(&self.window.window)
                    .ok()
                    .map(bt_platform::native_present_facts)
                    .unwrap_or_default();
                trace_sink::stderr_line(
                    attempt.line(
                        native,
                        self.window.window_shown,
                        (
                            self.window.window_exposed,
                            self.window
                                .attention_sampled_at
                                .map(|at| instant.saturating_duration_since(at).as_micros() as u64),
                        ),
                        self.window.renderer.present_configuration(&self.app.gpu),
                        state.last_present.map_or(0, |at| now.saturating_sub(at)),
                        state.age(now),
                    ),
                );
            });
        }
        // Check before updating the landing timestamp: a single blocked attempt
        // can cross the threshold and land before the next event-loop turn.
        self.check_picture_freshness(instant, attempt.landed_at.is_some());
        if attempt.landed_at.is_some() {
            let state = &mut self.window.present_diagnostics;
            state.last_present = Some(now);
            state.last_landed = (attempt.generation, attempt.sequence);
            state.observe(false, now, present_diagnostics::progress());
        }
    }

    /// **What one present cost, printed once for every present this window
    /// makes** (`BT_PERF_TRACE present`).
    ///
    /// The number a person feels — how long the event that asked for this
    /// picture waited before the picture was handed to the compositor — beside
    /// the two ratios that say whether the window is *doing* redundant work:
    /// frames composed against frames presented (the slot's own overwrite count
    /// is the difference), and notches taken from the platform against notches
    /// routed.
    ///
    /// **Called from both present sites, and that is the whole of why it is a
    /// function.** A window has two ways to put a picture on the glass —
    /// [`Self::redraw`] presents a terminal frame somebody composed, and
    /// [`Self::present_retained_picture`] presents the picture already there with
    /// whatever the renderer has been told since — and **a tab with no shell only
    /// ever takes the second** (§7.1.6h): `publish_frame_inner` composes no
    /// terminal picture for it, so every frame it has is a retained present.
    /// While this line was printed at the first site alone, the instrument said
    /// such a tab drew *nothing at all*, and §7.10 ④″ had to write that down as a
    /// known blind spot. It is also what hid §7.10 ④‴: a tab whose gestures
    /// really were producing no frames read, in the trace, exactly like a tab
    /// whose frames were merely not being printed.
    ///
    /// `retained=` is the field that tells the two apart; every other field is
    /// what it always was. **`last_present_at` is written whichever kind of
    /// present this was, and outside the trace gate** — `since_previous_us` is
    /// about the gap between pictures reaching the glass, and a gap measured
    /// against only some of them is not that gap.
    pub(in crate::runtime) fn trace_present(
        &mut self,
        source: FrameSource,
        receipt: bt_render::PresentReceipt,
        retained: bool,
    ) {
        let presented_at = Instant::now();
        let since_previous = self
            .window
            .last_present_at
            .map_or(Duration::ZERO, |previous| presented_at - previous);
        self.window.last_present_at = Some(presented_at);
        if !self.app.trace_perf {
            return;
        }
        let Ok(latency) = receipt.latency() else {
            return;
        };
        // Derived from the receipt's two existing boundaries. The stall ledger
        // is the authority for station time; this field only saves a trace
        // reader from subtracting two timestamps the renderer already returns.
        let submit_to_present = latency
            .event_to_present_call
            .saturating_sub(latency.event_to_submit);
        // **The clock the line itself is measured on** — see
        // [`WindowRuntime::perf_trace_us`]. It starts before the `format!`,
        // because building the fields is part of what a trace costs a frame,
        // and stops once the sink has the line.
        let trace_started = Instant::now();
        trace_sink::stderr_line(format!(
            "BT_PERF_TRACE present source={source:?} retained={} event_to_present_us={} event_to_submit_us={} submit_to_present_us={} since_previous_us={} composed={} slot_overwrites={} wheel_events={} wheel_routings={} pace_interval_us={} pace_skipped={} trace_us={}",
            u8::from(retained),
            latency.event_to_present_call.as_micros(),
            latency.event_to_submit.as_micros(),
            submit_to_present.as_micros(),
            since_previous.as_micros(),
            self.window.composed_terminal_frames,
            self.window.pending_frames.overwrites(),
            self.window.wheel_events,
            self.window.wheel_routings,
            // **What the pacer is doing, on the line the cadence is read from**
            // (owner's report 2026-09-18). `pace_interval_us` is the display's
            // own frame as this window understands it, and `pace_skipped` is how
            // many turns the gate turned away since the previous present — so a
            // recording says both what the rate *should* be and that the loop
            // was genuinely being held to it, rather than leaving the reader to
            // infer the second from the gaps.
            self.window.frame_clock.interval().as_micros(),
            self.window.frame_clock.take_skipped(),
            self.window.perf_trace_us,
        ));
        self.window.perf_trace_us = trace_started.elapsed().as_micros();
    }

    pub(in crate::runtime) fn present_conditions(
        &self,
        source: FrameSource,
    ) -> present_gate::PresentConditions {
        present_gate::PresentConditions {
            visible: self.window.window_shown
                && self.window.window.is_visible() == Some(true)
                && !self.window.window_hidden
                && self.window.window_exposed,
            resize_pending: self.pending_resize_present.is_some()
                || matches!(source, FrameSource::Resize),
            skirt_pending: self.window.compositor.skirt_covers_anything(),
        }
    }

    /// The signature is gathered only after pane_draws sampled the transforms
    /// and placed previews. Equality includes the complete terminal projection,
    /// so selection, hover marks and both viewport origins advance a seat's
    /// picture revision even when its terminal bytes did not change.
    pub(in crate::runtime) fn present_signature(
        &self,
        seat_ids: &[SeatId],
        seats: &[bt_render::SeatFrame<'_>],
    ) -> present_gate::PresentSignature {
        debug_assert_eq!(seat_ids.len(), seats.len());
        let tab = &self.window.tabs[self.window.active_tab];
        let seats = seat_ids
            .iter()
            .zip(seats)
            .map(|(id, seat)| {
                let owner = (tab.id.0, id.0);
                let same_picture = tab
                    .sessions
                    .get(id)
                    .and_then(|leaf| leaf.last_presented_frame.as_ref())
                    .is_some_and(|previous| present_gate::pictures_match(previous, seat.frame));
                present_gate::SeatSignature {
                    owner,
                    picture_revision: self
                        .window
                        .present_gate
                        .picture_revision(owner, same_picture),
                    viewport: seat.seat,
                    clip: seat.clip,
                    focused: seat.focused,
                }
            })
            .collect();
        let size = self.window.window.inner_size();
        present_gate::PresentSignature {
            renderer: self.window.renderer.present_state(),
            window_visible: self.window.window_shown
                && self.window.window.is_visible() == Some(true),
            seats,
            native_pages: self
                .window
                .web
                .iter()
                .map(|(leaf, page)| present_gate::NativePageSignature {
                    owner: (leaf.tab.0, leaf.seat.0),
                    state: page.present_state(),
                })
                .collect(),
            window_size: (size.width, size.height),
            dpi: self.window.window.scale_factor().to_bits(),
        }
    }

    pub(crate) fn redraw(&mut self) -> Result<()> {
        let Some((frame, trigger)) = self.window.pending_frames.take() else {
            // Nothing composed. If an animation — or a tab that has no other
            // kind of frame — owes the glass a present of what is already
            // there, this is where that debt is paid.
            if a_bare_redraw_still_owes_a_present(
                self.window.chrome_present_pending,
                self.focused().is_some(),
            ) {
                return self.present_retained_picture();
            }
            let attempt = self.begin_present_attempt(FrameSource::Expose, false, false);
            self.finish_present_attempt(attempt, &Ok(()));
            return Ok(());
        };
        let mut attempt = self.begin_present_attempt(trigger.source, false, true);
        let result = (|| {
            // A composed frame supersedes any chrome-only debt: it is about to be
            // presented, chrome and all.
            let validation_parent = hang_watch::enter(hang_watch::Station::RedrawValidate);
            self.window.chrome_present_pending = false;
            if let Some(expected) = self.pending_resize_present {
                ensure!(
                    frame_matches_grid(&frame, expected),
                    "resize presentation requires the newly projected grid: expected {}x{}, got {}x{}",
                    expected.columns,
                    expected.rows,
                    frame.columns,
                    frame.grid_rows
                );
            }
            let has_text = frame
                .cells
                .iter()
                .take(
                    frame
                        .drawable_rows()
                        .saturating_mul(frame.columns.get() as usize),
                )
                .any(|cell| !cell.text.trim().is_empty());
            // The other panes of this tab. The focused leaf's frame came out of the
            // slot above, with every presentation-hold and resize contract the slot
            // exists to enforce still attached to it; the panes the user is not
            // typing in hold nothing, so they are projected here, at the moment they
            // are drawn.
            //
            // A lone terminal leaf produces exactly one entry, whose rectangle is
            // the same `pane_body_viewport` answer `resolve_seat_layout` already
            // handed the renderer — so the slice is N = 1 and the command stream is
            // the one that was always issued. The loop is not a second path; it is
            // the same path counted.
            hang_watch::at(validation_parent);
            let focused_leaf = self.focused_leaf;
            // U8 — one instant for the whole frame, as everywhere else a tween is
            // read: two panes of one present sampled at two times would be two
            // frames of the same animation composited together.
            let now = Instant::now();
            let bodies =
                hang_watch::during(hang_watch::Station::RedrawLayout, || self.pane_draws(now));
            let active = self.window.active_tab;
            // The pointer's marks belong to the pane the pointer is in, focused or not, so an
            // unfocused pane that is being hovered is projected *and then decorated* — the same two
            // steps `publish_frame_inner` takes for the focused one. Resolved before the loop because
            // it is a question about the whole window and the loop holds a leaf.
            let hovered_reference = self.hovered_image_reference();
            let hover_pane = self.window.hover_pane.filter(|seat| *seat != focused_leaf);
            let mut unfocused_frames: Vec<(PaneDraw, ViewportFrame)> = Vec::new();
            // **And what those panes owe the typesetting engine** (review row R5-4).
            // A settled `$$…$$` block becomes work for the engine when a frame is
            // projected over it, and until this walk asked, the only frame that ever
            // asked was the focused leaf's — so a block printed in the pane beside it
            // stayed as its own LaTeX until somebody clicked into that pane. This is
            // the road that already visits every visible leaf, so it is where the
            // question belongs; the answer is gathered here and dispatched once
            // below, because the dispatcher wants the tab and this loop is holding a
            // leaf of it.
            let mut owes_the_engine = false;
            let projection_parent = hang_watch::enter(hang_watch::Station::RedrawProjection);
            hang_watch::counters(0, 0, bodies.len());
            for pane in &bodies {
                if pane.seat == focused_leaf {
                    continue;
                }
                let hyperlink_hover = &self.window.hyperlink_hover;
                let Some(leaf) = self.window.tabs[active].sessions.get_mut(&pane.seat) else {
                    continue;
                };
                leaf.session.refresh_projection(&mut leaf.projection);
                let mut projected = leaf
                    .session
                    .viewport_frame(&mut leaf.projection)
                    .context("project an unfocused pane's grid into a viewport frame")?;
                // A pane nobody has the keyboard in still draws paths, and still owes them an answer.
                owes_the_engine |= leaf
                    .session
                    .absorb_printed_path_probes(&mut leaf.projection)
                    != 0;
                owes_the_engine |= hang_watch::during(hang_watch::Station::DetectionPass, || {
                    leaf.session.schedule_visible_artifacts(&projected)
                }) != 0;
                if hover_pane == Some(pane.seat) {
                    apply_hover_marks(
                        &mut projected,
                        hyperlink_hover,
                        hovered_reference
                            .as_ref()
                            .filter(|(seat, _)| *seat == pane.seat)
                            .map(|(_, reference)| reference),
                    );
                }
                unfocused_frames.push((*pane, projected));
            }
            hang_watch::at(projection_parent);
            let dispatch_parent = hang_watch::enter(hang_watch::Station::RedrawDispatch);
            if owes_the_engine {
                let tasks = self.app.math_worker.tasks.clone();
                let scale_tasks = self.app.math_worker.scale_tasks.clone();
                let path_tasks = self.app.math_worker.path_tasks.clone();
                let window = self.window_id();
                dispatch_tab_decoration_tasks(
                    window,
                    &mut self.window.tabs[active],
                    &tasks,
                    &scale_tasks,
                    &path_tasks,
                    &mut self.app.math_worker_running,
                    &mut self.app.math_worker_notice_pending,
                );
            }
            hang_watch::at(dispatch_parent);
            let assembly_parent = hang_watch::enter(hang_watch::Station::RedrawSeatFrames);
            let focused_body = bodies
                .iter()
                .find(|pane| pane.seat == focused_leaf)
                .copied()
                .unwrap_or_else(|| {
                    let viewport = self.window.renderer.seat_viewport();
                    PaneDraw {
                        seat: focused_leaf,
                        viewport,
                        clip: viewport,
                    }
                });
            // **The frame this present draws owns both formula overlay lanes**
            // (2026-09-20, T-MARKS-FRAME-IN-HAND). Every unfocused pane has now
            // been projected, and nothing has reached the present funnel yet. Hand
            // those exact frames to both lookups, then rebuild the overlay only when
            // a lit or travelling band can make the answer visible.
            let mut math_band_trace = None;
            if self.formula_overlay_is_active() {
                let frame_for = |seat| {
                    if seat == focused_leaf {
                        return Some((focused_body.viewport, &frame));
                    }
                    unfocused_frames
                        .iter()
                        .find(|(pane, _)| pane.seat == seat)
                        .map(|(pane, projected)| (pane.viewport, projected))
                };
                let trace = self.math_band_trace_for_present(frame_for);
                let placement = hang_watch::during(hang_watch::Station::RedrawOverlay, || {
                    self.math_tool_placement(frame_for)
                });
                let toggle_layers = hang_watch::during(hang_watch::Station::RedrawOverlay, || {
                    self.formula_toggle_layers(now, frame_for)
                });
                hang_watch::during(hang_watch::Station::RedrawOverlay, || {
                    self.refresh_formula_overlay_for_present(now, placement, toggle_layers)
                });
                math_band_trace = self
                    .app
                    .trace_perf
                    .then(|| self.math_band_trace_line(now, trace));
            }
            let table_sources = Self::table_sources(
                std::iter::once(&frame).chain(unfocused_frames.iter().map(|(_, it)| it)),
            );
            hang_watch::during(hang_watch::Station::RedrawTables, || {
                self.refresh_table_paints(&table_sources)
            });
            let mut seat_frames = Vec::with_capacity(unfocused_frames.len() + 1);
            seat_frames.push(bt_render::SeatFrame {
                seat: focused_body.viewport,
                clip: focused_body.clip,
                frame: &frame,
                // **The owner, not the focus** (user report + ruling, 2026-08-13).
                // This flag is read by `seat_caret` alone, and what it is asked
                // there is "is this the caret typing would land in" — see
                // [`Self::keyboard_owner_is_a_shell`] for why the answer stopped
                // being "yes, it is the focused pane".
                focused: self.keyboard_owner_is_a_shell(),
            });
            for (pane, projected) in &unfocused_frames {
                seat_frames.push(bt_render::SeatFrame {
                    seat: pane.viewport,
                    clip: pane.clip,
                    frame: projected,
                    focused: false,
                });
            }
            let seat_ids: Vec<_> = std::iter::once(focused_leaf)
                .chain(unfocused_frames.iter().map(|(pane, _)| pane.seat))
                .collect();
            let signature = hang_watch::during(hang_watch::Station::RedrawSignature, || {
                self.present_signature(&seat_ids, &seat_frames)
            });
            let conditions = self.present_conditions(trigger.source);
            hang_watch::at(assembly_parent);
            match Self::present_seats_and_commit(
                &mut self.app.gpu,
                &mut self.window.renderer,
                &self.window.compositor,
                &self.window.window,
                FrameTraces {
                    attempt: &mut attempt,
                    gate: &mut self.window.present_gate,
                    trace_perf: self.app.trace_perf,
                    slot_overwrites: self.window.pending_frames.overwrites(),
                    conditions,
                    preview: &mut self.window.preview_trace_echo,
                    census: &mut self.window.glyph_census_echo,
                },
                &seat_frames,
                PresentIntent { trigger, signature },
            )
            .context("render terminal frame")?
            {
                outcome @ (Some(PresentOutcome::Presented(_)) | None) => {
                    let commit_parent = hang_watch::enter(hang_watch::Station::RedrawCommit);
                    let receipt = outcome.and_then(|outcome| match outcome {
                        PresentOutcome::Presented(receipt) => Some(receipt),
                        _ => unreachable!(),
                    });
                    // A whole frame ends whatever textless run was going, which is
                    // what makes the next refusal a new question rather than the
                    // continuation of an old one ([`WindowRuntime::may_ask_again`]).
                    self.window.textless_frames = 0;
                    // And it ends a device-loss episode for the same kind of reason:
                    // a device that has drawn is a device this process is willing to
                    // lose again ([`DeviceLossPilot::a_frame_reached_the_glass`]).
                    if receipt.is_some() {
                        self.app.device_loss_pilot.a_frame_reached_the_glass();
                    }
                    // The glass now holds the newest picture anyone composed. This
                    // is the equality [`chrome_tick_reuses_picture`] reads as its
                    // licence to answer the next animation tick from the screen.
                    self.window.presented_picture_revision = self.window.terminal_content_revision;
                    // And every pane that could be drawn was just projected afresh
                    // and put on the glass with it, so nothing is owed. See
                    // [`WindowRuntime::unpainted_pane_output`] for why this is one bit
                    // squared here rather than a debt kept per pane.
                    self.window.unpainted_pane_output = false;
                    if self.window.window_shown && !self.window.first_visible_present_dpi_checked {
                        self.window.first_visible_present_dpi_checked = true;
                        self.reconcile_authoritative_dpi("first-present")?;
                    }
                    let latency = receipt.map(|receipt| receipt.latency());
                    if let Some(receipt) = receipt {
                        self.trace_present(trigger.source, receipt, false);
                        self.trace_math_band(math_band_trace);
                    }
                    if self.app.trace_startup
                        && matches!(trigger.source, FrameSource::Resize)
                        && let Some(Ok(latency)) = latency
                    {
                        trace_sink::stderr_line(format!(
                            "BT_RESIZE present={}us columns={} rows={}",
                            latency.event_to_present_call.as_micros(),
                            frame.columns,
                            frame.grid_rows
                        ));
                    }
                    if has_text && !self.window.first_text_presented {
                        self.window.first_text_presented = true;
                        if self.app.trace_startup {
                            let text_visible = self.app.startup_started.elapsed();
                            self.window.first_text_visible = Some(text_visible);
                            trace_sink::stderr_line(format!(
                                "BT_STARTUP first_text_present={}ms",
                                text_visible.as_millis()
                            ));
                        }
                    }
                    // Each pane keeps the frame it just drew, so a pointer question
                    // asked over it can be answered by its own cells. The focused
                    // leaf's copy is `WindowRuntime::last_presented_frame` as well, which
                    // is what the presentation-hold and scroll contracts read.
                    //
                    // And each pane's ledger is squared here, in the same breath and
                    // for the same reason: this is the moment its cells reached the
                    // glass, which is the only moment [`seen_revision`] recognises as
                    // being looked at. Every painted pane, not the focused one —
                    // three panes on screen are three panes the user can read, and a
                    // tab that lit up for a sibling of the pane holding the keyboard
                    // was the bug this pass was written to end.
                    let active = self.window.active_tab;
                    for (pane, projected) in unfocused_frames {
                        if let Some(leaf) = self.window.tabs[active].sessions.get_mut(&pane.seat) {
                            leaf.last_presented_frame = Some(projected);
                            mark_leaf_painted(leaf);
                        }
                    }
                    if let Some(leaf) = self.window.tabs[active].sessions.get_mut(&focused_leaf) {
                        leaf.last_presented_frame = Some(frame.clone());
                        mark_leaf_painted(leaf);
                    }
                    self.window.last_presented_frame = Some(frame);
                    // The pane under the pointer just drew new cells, so the list its pointer verbs
                    // read is re-derived from them. Only that pane: a reference scan answers a
                    // question nobody is asking of the panes the pointer is not in, and the resting
                    // dotted affordance they do wear was painted by the session itself.
                    if let Some(seat) = self.window.hover_pane.filter(|seat| *seat != focused_leaf)
                    {
                        self.rescan_pane_references(seat);
                    }
                    self.pending_resize_present = None;
                    hang_watch::at(commit_parent);
                }
                // The frame is still owed, so it goes back in the slot and the
                // window asks for another turn. `PresentedWithoutText` joins the two
                // swapchain refusals here rather than the arm above it: a picture
                // that lost its characters is not the picture that was composed, and
                // recording it as presented would leave `presented_picture_revision`
                // claiming the glass holds a frame it does not — which is the licence
                // the animation path reads before answering a tick from the screen.
                //
                // The frame a window that is off screen composed goes back in the
                // slot with the rest of them, and waits there: `may_ask_again` is
                // what declines to ask for the turn, not this arm. See
                // [`ask_again_after`].
                Some(
                    outcome @ (PresentOutcome::PresentedWithoutText(_)
                    | PresentOutcome::Skipped
                    | PresentOutcome::SkippedNotVisible
                    | PresentOutcome::Reconfigure),
                ) => {
                    self.window
                        .pending_frames
                        .publish(frame, trigger)
                        .context("reject non-rectangular frame during redraw retry")?;
                    if self.window.may_ask_again(&outcome) {
                        hang_watch::during(hang_watch::Station::WindowRedraw, || {
                            self.window.window.request_redraw()
                        });
                    }
                }
            }
            Ok(())
        })();
        self.finish_present_attempt(attempt, &result);
        result
    }

    /// **One turn of the event loop, for one window** (multiwindow slice C).
    ///
    /// Every clock this window keeps, read once, and the earliest moment it
    /// needs to be woken again — which the caller takes the minimum of over
    /// every open window. Nothing here changed when the loop learned to route:
    /// this is the body `about_to_wait` always had, with the window it was
    /// implicitly about now named by the `self` it is called on.
    ///
    /// `application_clocks` is the one thing a window cannot decide for itself.
    /// The three watches and the session debounce belong to the application, so
    /// turning them once per window would be a folder re-read per window and a
    /// debounce that fires N times; exactly one window in the process is given
    /// the job, and it is the one that opened first — see [`FolioApp::order`].
    pub(crate) fn turn(
        &mut self,
        now: Instant,
        application_clocks: bool,
    ) -> Result<Option<Instant>> {
        self.check_picture_freshness(now, false);
        // **The frame debt belongs to the turn that incurs it** (owner's report
        // 2026-09-18). Opened here rather than cleared wherever it is paid,
        // because "an animation asked to draw and the glass was not ready" is a
        // fact about *this* pass of the loop: whatever the gate refuses below is
        // what the deadline fold at the foot of this method books, and a turn
        // that refuses nothing books nothing and lets the window go idle. And
        // the answer itself is decided here, once, for every animation this
        // turn will run — see [`pace::FrameClock::open`] for why it cannot be
        // the live question. See [`crate::pace`].
        let last_present = self.window.last_present_at;
        self.window.frame_clock.open(last_present, now);
        // First, because everything below it is allowed to assume the window is
        // where the hand last left it. This is the door the coalescing is *for*:
        // the queue has just run dry, so whatever the wheel collected while the
        // loop was away is one burst and gets one frame. See [`WheelBurst`].
        self.flush_wheel()?;
        // **And the drop beside it, for the same reason and at the same door**
        // (GitHub issue #1 ②). This is the boundary the batch is defined by: the
        // queue has just run dry, so every `DroppedFile` of one drop has arrived
        // and the paths in hand are that drop and no other.
        self.flush_dropped_files()?;
        // **Directly after it, and before anything that reads a cell** (§7.50).
        // A DPI change the drag wrote down is a window whose font is still
        // measured for the display it left; every clock below this line that
        // publishes a frame would publish that one. The hand has let go by the
        // time this returns true, so nothing here is done to a window that is
        // still moving.
        //
        // **And it says its own name from here** (T-STATION-SPLIT): the stretch
        // between the wheel's door and the drain used to be charged to
        // `flush_wheel`, which is the one call in it that had nothing to do with
        // a display move.
        hang_watch::at(hang_watch::Station::DpiSettle);
        self.settle_deferred_dpi()?;
        // **And directly after that one**, for the half of a DPI change that is
        // owed to a rectangle rather than to a hand (T-CARD-ANCHOR-DPI). Every
        // message `WM_DPICHANGED` produced has been delivered by the time a turn
        // comes round, so this is the first moment at which the rectangle in
        // hand is the one the window is keeping. It is a no-op on every road
        // where a `Resized` did arrive, which is most of them.
        self.settle_dpi_rectangle()?;
        // The three answers a platform modal can have left behind, under their
        // own name (T-STATION-SPLIT) — see [`hang_watch::Station::Pickers`].
        hang_watch::at(hang_watch::Station::Pickers);
        self.apply_math_context_menu_result();
        self.apply_folder_pick_result()?;
        self.apply_image_pick_result()?;
        self.drain_pty()?;
        if !self.window.first_text_presented && now >= self.window.startup_poll_at {
            advance_periodic_deadline(
                &mut self.window.startup_poll_at,
                now,
                STARTUP_PTY_POLL_INTERVAL,
            );
        }
        // **Directly after the drain**, because the two facts it reads are what
        // the drain has just moved: whether a shell has spoken for the first
        // time, and whether an OSC 133 has landed. Polled here rather than
        // pushed from the probe's thread for the reason the invitation below is:
        // this changes a pane's height, and the panes are this thread's.
        //
        // **The drain's name stops here** (T-STATION-SPLIT). Everything from
        // this line to the page below used to be charged to `drain_pty`, which
        // is the one call in the run that is about a shell's output.
        hang_watch::at(hang_watch::Station::PaneRows);
        self.settle_pane_notices()?;
        // **And the row under a preview head, on the same terms** (user ruling
        // 2026-08-24). It is polled beside the notice strip because it is the
        // same kind of fact and costs the same kind of change: a page that has
        // just committed its first address, a buffer that has just arrived on a
        // seat, a pane whose document was swapped — each of them can add or
        // retire a row, and a row is twenty-eight pixels of somebody's document.
        self.settle_preview_rails()?;
        // Polled here rather than pushed from the probe's thread: raising a
        // modal is a change to the window, and the window is this thread's.
        //
        // **The first-run card is asked first**, because it is the one surface
        // that can be owed on a launch where nothing has happened yet, and
        // because its own gate closes the moment it goes up.
        //
        // **The clock run begins here** — see [`hang_watch::Station::Clocks`].
        // Every entry is a deadline or an edge, never a filesystem/PATH poll.
        hang_watch::at(hang_watch::Station::Clocks);
        hang_watch::during(hang_watch::Station::ClockRaiseFirstRunIfDue, || {
            self.raise_first_run_if_due()
        })?;
        hang_watch::during(hang_watch::Station::ClockRaisePsreadlineInviteIfDue, || {
            self.raise_psreadline_invite_if_due()
        })?;
        hang_watch::during(hang_watch::Station::ClockAdvanceCursorBlinkIfDue, || {
            self.advance_cursor_blink_if_due(now)
        })?;
        if application_clocks {
            // A poll of the world outside this process, put back on the clock
            // run's name the moment it returns (T-STATION-SPLIT).
            let leaving = hang_watch::enter(hang_watch::Station::Watches);
            hang_watch::during(hang_watch::Station::ClockAdvanceSchemeWatch, || {
                self.advance_scheme_watch(now)
            })?;
            hang_watch::during(hang_watch::Station::ClockAdvanceStorageWatch, || {
                self.advance_storage_watch(now)
            })?;
            hang_watch::at(leaving);
        }
        hang_watch::during(hang_watch::Station::ClockAdvanceRenameBlinkIfDue, || {
            self.advance_rename_blink_if_due(now)
        })?;
        // The preview seats' own watch (W2 slice 5). This window's and not the
        // application's - see [`WindowRuntime::preview_watch`] - and beside the
        // two above because it is the same shape: a clock that asks for a
        // wake-up only while it is holding news, over subscriptions that are
        // brought level with what the seats are showing on the same turn.
        hang_watch::at(hang_watch::Station::Watches);
        hang_watch::during(hang_watch::Station::ClockAdvancePreviewWatch, || {
            self.advance_preview_watch(now)
        })?;
        // And the file trees', beside it and on the same terms: this window's
        // and not the application's, because which folders are on the glass is a
        // question only a window can answer.
        hang_watch::during(hang_watch::Station::ClockAdvanceFilesWatch, || {
            self.advance_files_watch(now)
        })?;
        // The hosted page's own clock: the wait for a browser process to say it
        // has gone. **The only backstop on the graceful path**, where
        // `ProcessFailed` does not arrive at all and `BrowserProcessExited`
        // arrived late in one of eight measured shutdowns and never in another.
        self.advance_web_page(now)?;
        // Ahead of the strip's own animation tick: paying the press's promise
        // activates a tab, and the strip that is redrawn afterwards should be
        // the one the switch produced.
        hang_watch::at(hang_watch::Station::Clocks);
        hang_watch::during(hang_watch::Station::ClockAdvanceTabPressIfDue, || {
            self.advance_tab_press_if_due(now)
        })?;
        // **The pictures are serviced before anything can be refused** (closure
        // review O4, 2026-09-18). A decoder pumped and a decoded animation's
        // clock walked are not this window asking the glass for a frame — they
        // are state being brought up to `now` — and the tick below can be turned
        // away by any neighbouring pane that is printing. See
        // [`Self::service_pictures`], which the head of every compose calls too.
        hang_watch::during(hang_watch::Station::ClockServicePictures, || {
            self.service_pictures(now)
        });
        hang_watch::during(hang_watch::Station::ClockAdvanceStripAnimation, || {
            self.advance_strip_animation(now)
        })?;
        // A screenful of held-back output arriving at once, under its own name
        // rather than the web page's — see [`hang_watch::Station::SyncUpdate`].
        hang_watch::at(hang_watch::Station::SyncUpdate);
        hang_watch::during(
            hang_watch::Station::ClockFinishSynchronizedUpdateIfDue,
            || self.finish_synchronized_update_if_due(now),
        )?;
        // The other road on which a pane's picture moves with no byte passing through the drain:
        // the bounded wait for the rest of a burst, run out. Beside the sync-update release
        // because it is the same kind of thing and must not be forgotten on any turn — see
        // [`coalesce::Pending`].
        hang_watch::during(hang_watch::Station::ClockFinishPtyCoalesceIfDue, || {
            self.finish_pty_coalesce_if_due(now)
        })?;
        // The watcher's own clock (R31's D), beside the rest of this window's:
        // it asks for a wake-up only while it is holding news, and the
        // subscriptions it keeps level with the screen are dropped here the turn
        // after the page they were for went away.
        //
        // The application's, not this window's, since slice B raised it: the set
        // it follows is the union of every window's pages, and one clock over a
        // union is one clock. So one window turns it, as with the two watches
        // above, and the news it produces reaches the pages through the caches
        // those pages own.
        if application_clocks {
            hang_watch::at(hang_watch::Station::Watches);
            hang_watch::during(hang_watch::Station::ClockAdvanceGitWatch, || {
                self.advance_git_watch(now)
            })?;
            // `flush_if_due` stamps [`hang_watch::Station::Autosave`] itself,
            // and since T-STATION-SPLIT that name covers the file it writes and
            // nothing else: the clock run below says its own name on the very
            // next line.
            self.app.session_store.flush_if_due(now);
        }
        // **The one rule, in the one place it can be kept.** Whichever rung holds
        // the keyboard publishes its caret here, at the tail of every pass —
        // which is what "every frame the caret can move" means without anybody
        // having to enumerate the ways it moves: typing, scrolling, resizing and
        // a capsule relaid all wake this loop, and the throttle below turns a
        // rectangle that did not move into no call at all. The terminal's rung
        // says nothing here and everything in `publish_frame_inner`, because its
        // caret is a property of a frame and there is no frame at this point.
        //
        // **And before the caret is published, whether there is still a field
        // for it** (§7.1.5a″): a composition whose field has gone is cancelled
        // here, on the identical argument — every way the keyboard can move has
        // already happened by the time this line runs, so nothing has to
        // enumerate them.
        //
        // **And the run this line opens says its own name** (T-STATION-SPLIT).
        // Every clock from here to the pane resize below used to be charged to
        // `session flush_if_due`, the last station entered above — a lane that
        // writes one small file and had nothing to do with any of them.
        hang_watch::at(hang_watch::Station::Clocks);
        hang_watch::during(hang_watch::Station::ClockSettleCompositionOwner, || {
            self.settle_composition_owner()
        })?;
        hang_watch::during(hang_watch::Station::ClockOfferImeCaret, || {
            self.offer_ime_caret(None)
        });
        hang_watch::during(hang_watch::Station::ClockFlushImeCursorArea, || {
            self.flush_ime_cursor_area(now)
        });
        hang_watch::during(hang_watch::Station::ClockFinishResizeIfQuiescent, || {
            self.finish_resize_if_quiescent(now)
        })?;
        hang_watch::during(hang_watch::Station::ClockFinishPreviewScaleIfQuiet, || {
            self.finish_preview_scale_if_quiet(now)
        })?;
        hang_watch::during(hang_watch::Station::ClockAdvanceLiveMathIfDue, || {
            self.advance_live_math_if_due(now)
        })?;
        hang_watch::during(
            hang_watch::Station::ClockActivateHyperlinkHoverIfDue,
            || self.activate_hyperlink_hover_if_due(now),
        )?;
        hang_watch::during(hang_watch::Station::ClockActivatePeekIfDue, || {
            self.activate_peek_if_due(now)
        })?;
        // Who holds the glass **before** this pass's clocks run, so that the
        // tail below can tell "nothing was hovering" from "something was and its
        // grace just ran out". See [`Self::rearm_hover_intents`].
        let glass_was = self.hover_float_up();
        // Both `⌄` clocks, and the pane menu's own two. Above the tip's, on the
        // layout peek's precedent and for its reason: a menu that has matured is
        // already on screen when the tip asks whether it has anything to explain.
        hang_watch::during(hang_watch::Station::ClockAdvanceChevrons, || {
            self.advance_chevrons(now)
        })?;
        hang_watch::during(hang_watch::Station::ClockAdvancePaneMenu, || {
            self.advance_pane_menu(now)
        })?;
        // And the terminal menu's, which since §7.1.6i's floor owns the same two
        // clocks in the same one slot. The two menus are never up together (E61:
        // the opener closes the others), so this is a second reader of the same
        // rule rather than a second rule.
        hang_watch::during(hang_watch::Station::ClockAdvanceTermMenu, || {
            self.advance_term_menu(now)
        })?;
        // And a tab menu's, which owns the same two clocks in the same one slot
        // for the same reason. A third reader of one rule rather than a third
        // rule.
        hang_watch::during(hang_watch::Station::ClockAdvanceTabMenu, || {
            self.advance_tab_menu(now)
        })?;
        // And the drag's own rest, beside them because it is the same shape and
        // the same quarter second (§7.1.6k). Below them rather than above: a
        // spring takes a whole tab off the screen, and the two menus above have
        // already been retired by the drag that armed this one.
        hang_watch::during(hang_watch::Station::ClockAdvanceDragSpring, || {
            self.advance_drag_spring(now)
        })?;
        // And the drag's other clock, which is the same shape once more: a hand
        // that has stopped at the edge of a list (缺陷 #188). Below the spring
        // rather than above it, and the order is load-bearing: a spring that
        // matures on this frame has changed which tab is on screen, and the
        // auto-scroll's own re-survey should be struck against the stage the
        // window is actually showing rather than the one it was showing a line
        // ago.
        hang_watch::during(hang_watch::Station::ClockServiceDragAutoscroll, || {
            self.service_drag_autoscroll(now)
        })?;
        // **And what the pointer is on, re-read against the picture that is
        // actually on the glass** (owner's report 2026-09-18). The band under a
        // resting hand can stop being under it without the hand doing anything,
        // and this is the only thing that notices. **It is also now the whole of
        // the exit** (owner's ruling of that afternoon): the half-second clock
        // that used to be spent on the next line is gone, and the door this
        // reaches — `update_math_hover` — ends the hover on the spot, so a band
        // that stopped being under a motionless pointer is let go on the turn
        // that noticed rather than half a second later. See
        // [`Self::refresh_math_hover_against_the_picture`].
        hang_watch::during(
            hang_watch::Station::ClockRefreshMathHoverAgainstThePicture,
            || self.refresh_math_hover_against_the_picture(now),
        )?;
        // And the band that is changing face, **ahead** of the marks: the
        // journey this pays for moves the block's own rectangle, and the marks
        // ride that rectangle's right-hand midline (§7.1.5p ⑨ ii, ⑪) — so the
        // picture they follow should be the one this turn has just asked for
        // rather than the one before it.
        hang_watch::during(hang_watch::Station::ClockAdvanceMathToggleIfDue, || {
            self.advance_math_toggle_if_due(now)
        })?;
        // And the band's marks, beside the clock that takes them down: the fade
        // they are climbing, and the second look they owe the picture that lit
        // the band they belong to (owner's report 2026-09-14).
        hang_watch::during(hang_watch::Station::ClockAdvanceMathToolsIfDue, || {
            self.advance_math_tools_if_due(now)
        })?;
        // Ahead of the tip's own promotion, so §6's ordering is structural and
        // not merely a consequence of 350 being less than 380: on the frame both
        // came due, the peek is already showing when the tip asks.
        hang_watch::during(hang_watch::Station::ClockAdvanceLayoutPeekIfDue, || {
            self.advance_layout_peek_if_due(now)
        })?;
        hang_watch::during(hang_watch::Station::ClockAdvanceTooltipIfDue, || {
            self.advance_tooltip_if_due(now)
        })?;
        // The hint card's 800ms and its fade, beside the tip's and after it:
        // the two are the same shape — a clock armed by a hand that has stopped
        // — and this one is the outer of the pair, so a window where both came
        // due on one frame has already shown the tip when the card is asked.
        //
        // **This turn is also where a hold that became answerable is answered.**
        // The offer can change with no keyboard event at all — a menu closing, a
        // press on the settings row — so `note_key_hint` is asked on every turn
        // and not only from `ModifiersChanged`; it re-arms a hold that was
        // refused and takes down a card whose window has stopped offering.
        hang_watch::during(hang_watch::Station::ClockNoteKeyHint, || {
            self.note_key_hint(now)
        })?;
        hang_watch::during(hang_watch::Station::ClockAdvanceKeyHintIfDue, || {
            self.advance_key_hint_if_due(now)
        })?;
        // The Cards column's own one-time bubble, beside the hint card because
        // the two are the same kind of surface — and asked on every turn for the
        // same reason: the thing that raises it is the *column being on screen*,
        // which is a fact about the posture rather than an event, and a window
        // that opened straight into Cards never sent one.
        hang_watch::during(hang_watch::Station::ClockNoteCardHint, || {
            self.note_card_hint(now)
        })?;
        hang_watch::during(hang_watch::Station::ClockAdvanceCardHint, || {
            self.advance_card_hint(now)
        })?;
        // The notices' three clocks, beside the tip's and after it: a card that
        // has just left frees the pixels the tip may be about to be laid over.
        hang_watch::during(hang_watch::Station::ClockAdvanceToasts, || {
            self.advance_toasts(now)
        })?;
        // The jump's row flash, beside the notices' clocks: it is the same shape —
        // a fade that runs itself out with the pointer and the keyboard both
        // still, and that nothing else in the window would wake the loop to draw.
        hang_watch::during(hang_watch::Station::ClockAdvanceCommandFlash, || {
            self.advance_command_flash(now)
        })?;
        // And the rail's own three, beside the flash because they are the same
        // shape: a hand resting on a rail moves nothing and no other clock in this
        // window would wake the loop to finish the crest it started.
        hang_watch::during(hang_watch::Station::ClockAdvanceCommandRails, || {
            self.advance_command_rails(now)
        })?;
        // The scroll thumb's rest and fade, beside the rail's clocks because it
        // is the same shape once more, and on the same pane's own edge (P2-9
        // slice 1).
        hang_watch::during(hang_watch::Station::ClockAdvanceTerminalThumbs, || {
            self.advance_terminal_thumbs(now)
        })?;
        // The glance card's own 350ms, beside the layout peek's and for the same
        // reason it stands where it does: it is a *peek*, and a peek that has
        // matured is already on screen when everything above it asks.
        hang_watch::during(hang_watch::Station::ClockAdvanceFilePeek, || {
            self.advance_file_peek(now)
        })?;
        // The float's two clocks and its entrance. After the tip's, because a
        // maturing intent can *open* a window over whatever the tip was about to
        // explain, and the frame both are due on should show that in the order
        // they will actually be drawn.
        hang_watch::during(hang_watch::Station::ClockAdvanceFloat, || {
            self.advance_float(now)
        })?;
        // **The glass came free while the hand stood still.** Below every clock
        // that can retire a hover panel and above nothing that arms one, because
        // that is exactly the instant the refusals handed out on the last
        // pointer move stopped being true. Costs nothing on a pass where nobody
        // was hovering, which is almost every pass.
        if glass_was.is_some() && self.hover_float_up().is_none() {
            hang_watch::during(hang_watch::Station::ClockRearmHoverIntents, || {
                self.rearm_hover_intents(now)
            })?;
        }
        // And the foot's own 1300ms, whichever strip is wearing it. Its own step
        // because the two feet are drawn into two different surfaces — see
        // `advance_foot_reveal`.
        hang_watch::during(hang_watch::Station::ClockAdvanceFootReveal, || {
            self.advance_foot_reveal(now)
        })?;
        // And a page's magnification, on the same clock and for the same reason
        // it is its own step: what has to be rebuilt is the pane the page is in.
        hang_watch::during(hang_watch::Station::ClockAdvancePageFootClocks, || {
            self.advance_page_foot_clocks(now)
        })?;
        // And the preview's own acknowledgement, on the same 1300ms clock.
        hang_watch::during(hang_watch::Station::ClockAdvancePreviewNotice, || {
            self.advance_preview_notice(now)
        })?;
        hang_watch::during(hang_watch::Station::ClockAdvancePreviewRefusal, || {
            self.advance_preview_refusal(now)
        })?;
        // Service the PTY gate after every other due task that can mutate session state, then carry
        // the deadline derived from that exact sample into the control-flow decision below.
        let pty_resize_deadline = self.flush_pending_pty_resize(now)?;
        // **And the round trip's name stops there** (T-STATION-SPLIT): the
        // deadline arithmetic below is this window reading its own clocks, not
        // conhost answering.
        hang_watch::at(hang_watch::Station::Deadlines);
        // **What is mid-flight in this window, told to the frame clock once a
        // turn** (review 2026-09-18 round 2, P1). Here rather than at the gate
        // because a journey being alive is a *fact*, and the gate only ever sees
        // a question: four of the advancers above ask it before they have
        // established that they hold anything at all, so a window with an empty
        // tip host and an empty float host answered "running" on every turn for
        // ever — and every keystroke after that rebuilt the whole interface.
        //
        // The two walks the report shares with the fold are made once and
        // answer both questions. See [`AnimationWork`], [`Self::running_journeys`]
        // and [`Self::carry_live_journeys`].
        let strip_animation = self.strip_animation_work(now);
        let terminal_thumbs = self.terminal_thumb_work(now);
        let strip_animation_deadline = strip_animation.deadline;
        let terminal_thumb_deadline = terminal_thumbs.deadline;
        let running = self.running_journeys(now, strip_animation.moving, terminal_thumbs.moving);
        self.window.frame_clock.note_running(running);
        let startup_deadline = startup_poll_delay(self.window.first_text_presented)
            .map(|_| self.window.startup_poll_at);
        // Every leaf, not every tab's focused leaf: an unfocused pane runs its own resize
        // transaction and owes its own PSReadLine repair, so its quiescence is its own deadline to
        // wake for. Reading this through the tab's deref asked only the pane holding the keyboard,
        // and the panes beside it had to wait for some unrelated event to carry them over the line.
        let resize_finish_deadline = self
            .window
            .tabs
            .iter()
            .flat_map(|tab| tab.leaves())
            .filter_map(|(_, leaf)| leaf.session.resize_finish_deadline())
            .min();
        // Every leaf, for the reason the line above gives about resizes: a DEC
        // 2026 block is a property of one screen, and
        // `finish_synchronized_update_if_due` has always walked every leaf to
        // time them out. Reading the deadline through the tab's deref asked only
        // the pane holding the keyboard, so an unfocused pane that opened a block
        // and then went quiet had nothing to wake the loop on its behalf — its
        // own timeout could only fire if something else happened to wake the
        // window first.
        let synchronized_update_deadline = self
            .window
            .tabs
            .iter()
            .flat_map(|tab| tab.leaves())
            .filter_map(|(_, leaf)| leaf.session.synchronized_update_deadline())
            .min();
        // Every leaf of the tab on screen, for the reason the line above gives:
        // a stability window is a property of one pane's rows, and the pane that
        // printed the block is often not the one holding the keyboard. See
        // [`Self::live_stability_deadline`].
        let live_stability_deadline = self.live_stability_deadline();
        const DEADLINE_OWNERS: [&str; 49] = [
            "startup poll",
            "IME cursor",
            "shell caret",
            "tab press",
            "rename caret",
            "strip animation",
            "web teardown",
            "PTY resize",
            "resize finish",
            "synchronized update",
            "live stability",
            "PTY coalesce",
            "attention credential",
            "tooltip",
            "key hint",
            "Cards hint",
            "toast",
            "command flash",
            "command rails",
            "terminal thumbs",
            "layout peek",
            "file peek",
            "file-peek close grace",
            "file-peek dwell",
            "float",
            "revealed foot",
            "web zoom acknowledgement",
            "web dialog acknowledgement",
            "preview save notice",
            "preview refusal",
            "chevrons",
            "pane menu",
            "terminal menu",
            "tab menu",
            "drag spring",
            "drag auto-scroll",
            "hyperlink hover",
            "peek hover",
            "formula tools",
            "formula toggle",
            "formula-copy acknowledgement",
            "preview resample",
            "session save",
            "schemes watch",
            "storage watch",
            "git watch",
            "preview watch",
            "files watch",
            "refused frame",
        ];
        let deadlines = [
            startup_deadline,
            self.window.ime_cursor_throttle.deadline(),
            // Only while a shell holds the keyboard: a frozen caret owes no
            // wake-up at all (ruling 2026-08-13).
            self.keyboard_owner_is_a_shell()
                .then(|| self.window.cursor_blink.deadline())
                .flatten(),
            // The press's own 180ms, and only while it still owes one — a press
            // that has been paid or has slipped reports nothing, so a held
            // button costs no wake-ups at all.
            self.window
                .tab_press
                .as_ref()
                .and_then(TabPress::wake_deadline),
            // The rename caret blinks only while there is a rename.
            self.window
                .rename
                .is_some()
                .then(|| self.window.rename_blink.deadline())
                .flatten(),
            strip_animation_deadline,
            // Only while a page is waiting for its browser to go — a window with
            // no page, and a page nobody is closing, ask for no wake-ups at all.
            self.window
                .web
                .values()
                .filter_map(webhost::WebSeat::next_deadline)
                .min(),
            pty_resize_deadline,
            resize_finish_deadline,
            synchronized_update_deadline,
            live_stability_deadline,
            // **The bounded wait for the rest of a burst**, and the wake that keeps
            // [`coalesce::Pending`]'s invariant true: a deferred publication is never `Some`
            // without this line booking the turn that pays it. A window whose panes are not
            // mid-burst reports nothing and costs no wake-ups at all, which is every window
            // almost always.
            self.window.pty_coalesce.until,
            // A standing credential's ten minutes, and only while one is standing. A window whose
            // panes are asking for nothing reports nothing and costs no wake-ups at all — which is
            // every window, almost always.
            attention_ledger_deadline(&self.window.tabs),
            // The tip's 380ms while one is settling, and the fade's own frames
            // until it lands. A window with no tip under the pointer reports
            // nothing and costs no wake-ups at all.
            self.tooltip_deadline(now),
            // The hint card's 800ms while a hold is settling, and the fade's
            // own frames until it lands. A window whose hands are empty — or
            // whose hand has already pressed something — reports nothing and
            // costs no wake-ups at all (§7.1.5e′).
            self.key_hint_deadline(now),
            // The Cards bubble's four seconds, and the nudge's frames while the
            // card it points at is still travelling. A reader who has already
            // been told — which is every reader from their second visit onwards
            // — reports nothing, and under reduced motion even a window that is
            // being told reports one instant and then sleeps through it (§7.21).
            self.card_hint_deadline(now),
            // A notice's entrance landing, its life running out, its exit
            // finishing — and nothing at all while one is held under the pointer,
            // because a stopped clock owes no wake-ups (2026-08-16).
            self.toast_deadline(now),
            // The jump flash's own 950ms, at the animation's rate and only while
            // one is running. A window that has not jumped asks for nothing.
            self.command_flash_deadline(now),
            // `width .1s, background .14s, opacity .12s` on the ticks of whichever
            // rail the pointer is on — and nothing at all once the longest of the
            // three has landed, which is what makes a rail under a still hand cost
            // no wake-ups.
            self.command_rail_deadline(now),
            // A terminal thumb's rest and the fade after it — and nothing at all
            // for a pane with no scrollback, a pane parked in history (its bar
            // is standing, not fading) or a pane whose fade has landed.
            terminal_thumb_deadline,
            // The peek's 350ms while one is settling, and nothing afterwards:
            // it has no fade, so a schematic on screen is finished and asks for
            // no frames at all.
            self.window.layout_peek.deadline(),
            // The glance's 350ms while one is settling, the fade's own frames
            // until it lands (owner's ruling 2026-09-13) and — since it became a
            // card a hand can walk into (2026-08-14) — the grace that takes it
            // down again once the pointer has left its corridor. A card standing
            // still under a pointer that is inside it has no clock running at
            // all once its 90ms has landed, and asks for no wake-ups, which is
            // the same silence it kept when it could not be touched.
            self.file_peek_deadline(now),
            //
            // **Both of the card's own two clocks below are clamped to the
            // window's frame** (review 2026-09-18). They are spent by
            // `advance_file_peek`, which the frame gate can turn away; a raw
            // deadline in front of a gated advance is a wake-up that arrives,
            // is refused, and re-arms itself at the same instant — the spin a
            // schedule's clothes are the usual disguise for.
            self.window
                .file_peek
                .as_ref()
                .and_then(|peek| peek.closing_at)
                .map(|closing| self.clamp_animation_deadline(closing)),
            // And the dwell's own 350ms, which is the one clock in this window
            // that has to fire under a pointer that is not moving at all — a
            // hand resting on another row sends no events, so without this wake
            // "停留即换" would only ever happen on the next thing to twitch.
            self.window
                .file_peek
                .as_ref()
                .and_then(|peek| peek.dwell.as_ref())
                .and_then(|dwell| dwell.clock.due())
                .map(|due| self.clamp_animation_deadline(due)),
            // The intent's 180ms, the grace's 220/420, and the entrance's own
            // frames until it lands. A window with no float and no hovered
            // trigger reports nothing and costs no wake-ups at all.
            self.float_deadline(now),
            // The foot's confirmation owes exactly one wake-up: the instant it is
            // due to turn back into a path. One entry for both feet, because
            // there is one clock.
            self.window
                .revealed_foot
                .map(|(_, at)| at + FOOT_REVEAL_FEEDBACK),
            // And a page's magnification, on that same clock. One entry for
            // however many pages are on screen — the earliest is the only one
            // that needs a wake-up, and the pass it wakes finds the rest.
            self.window
                .web
                .values()
                .filter_map(|web| web.zoom_said().map(|(_, at)| at + FOOT_REVEAL_FEEDBACK))
                .min(),
            // And a message the page was refused, on that same clock and owing
            // the same single wake-up (R1-21).
            self.window
                .web
                .values()
                .filter_map(|web| web.dialog_said().map(|at| at + FOOT_REVEAL_FEEDBACK))
                .min(),
            // The preview's "Saved", on the same clock and owing the same single
            // wake-up: the instant it is due to go away.
            self.preview_notice_deadline(),
            // And the reason a refused edit is floating, on its own two seconds
            // (owner's ruling 2026-09-12). One entry because there is one
            // reader: the pill names one surface, and a second attempt anywhere
            // replaces it and sets this clock again.
            self.preview_refusal_deadline(),
            // The two chevrons' 250/150, and nothing at all while the pointer is
            // not on one and no menu one opened is up (2026-08-16).
            self.window.chevrons.deadline(),
            // And the pane menu's own clock: the heading's rest while the
            // submenu is shut, the safety triangle's cap while it is open.
            self.pane_menu_deadline(),
            // And the terminal menu's, which is the same clock on the same
            // heading behind the second door (§7.1.6i).
            self.term_menu_deadline(),
            // And a tab menu's, which is that same clock on that same heading
            // behind the third door (丙2).
            self.tab_menu_deadline(),
            // And the spring's 250 (§7.1.6k) — the one clock in this window that
            // fires under a hand that has deliberately stopped moving. Absent for
            // every drag that is not resting a pane on somebody else's tab, which
            // is every drag most of the time.
            self.drag_spring_deadline(),
            // And the auto-scroll's frame, which is the other clock that fires
            // under a hand that has deliberately stopped — this one because it
            // has stopped *at an edge* (缺陷 #188). Absent for every drag that is
            // not in a band, and absent again the moment the list reaches the end
            // it was running towards, so a hand parked at the foot of a
            // fully-scrolled column asks for nothing.
            self.drag_autoscroll_deadline(now),
            self.window.hyperlink_hover.show_at,
            self.window.peek_hover.show_at,
            // **And nothing at all for the band's own exit** (owner's ruling
            // 2026-09-18). This used to be `math_hover_clear_at`, the
            // half-second a band was held for after the pointer left it; the
            // hover now ends on the pointer's own event, like every other hover
            // in this window, so there is no clock left to wake for. What the
            // marks still owe — the ninety milliseconds they leave over — is the
            // next entry's, exactly as it was.
            // The band's marks while either of their journeys is still running —
            // the fade in, the fade out, or a move to the boxes a block that
            // changed shape has just published — and nothing once both have
            // landed: a pointer resting on a formula costs no wake-ups at all
            // (owner's ruling 2026-09-14 ②, and its report that evening).
            self.math_tools_deadline(now),
            // And the band that is changing face, while its ninety milliseconds
            // are still running — one entry, because one hand presses one mark,
            // and none at all once it has landed or under `Motion::Reduced`,
            // which never starts a journey at all (§7.1.5p ⑪).
            self.math_toggle_deadline(now),
            // And the copy tick's one wake-up: the instant it is due to turn
            // back into a pair of sheets. One entry because there is one
            // clipboard and one clock.
            //
            // **Through `math_copy_window`, so a spent tick asks for nothing**
            // (audit 2026-09-15, RB-1). This used to be `*at + FOOT_REVEAL_FEEDBACK`
            // unconditionally, and nothing ever cleared the field — so from 1300ms
            // after a copy this handed `ControlFlow::WaitUntil` an instant already
            // in the past, on every turn, for the life of the window. That is the
            // 100%-CPU failure `about_to_wait`'s empty-registry branch names, and
            // it was reached by a window with a formula somebody had copied.
            // `advance_math_tools_if_due` above has already retired a spent
            // acknowledgement by the time this is read, so on the ordinary road
            // this filter never fires; it is here because a deadline that can be
            // in the past must be impossible rather than merely unreached.
            math_copy_window(self.window.math_copied.as_ref().map(|(_, at)| at), now),
            self.preview_resample_deadline(),
            application_clocks
                .then(|| self.app.session_store.deadline())
                .flatten(),
            // The schemes folder's own debounce, on the same terms: absent for
            // every window whose folder nobody is editing.
            application_clocks
                .then(|| self.app.scheme_watch.deadline())
                .flatten(),
            // And the storage folder's, on exactly the same terms — absent for
            // every window in which nothing at all has been written lately.
            application_clocks
                .then(|| self.app.storage_watch.deadline())
                .flatten(),
            // The debounce a change notification started (R31's D). Absent —
            // and therefore costing nothing — for every window that is not
            // currently holding unanswered news about a repository, which is
            // every window most of the time.
            application_clocks
                .then(|| self.app.git_watch.deadline())
                .flatten(),
            // And the preview seats', on the same terms and without the
            // application gate: absent for every window that is not currently
            // holding unanswered news about a file it has open, which is every
            // window most of the time.
            self.window.preview_watch.deadline(),
            // And the file trees', on exactly the same terms: absent for every
            // window not currently holding unanswered news about a folder it is
            // showing, which is every window most of the time.
            self.window.files_watch.deadline(),
            // **And the frame an animation asked for and was refused** (owner's
            // report 2026-09-18). Every entry above says when a clock is next
            // worth reading; this one says that a clock already read wanted to
            // draw and the glass was not ready for it, which is the only kind of
            // debt in this fold that nothing else would ever come back for — a
            // band whose shape changed under a motionless pointer owes a frame
            // and has no clock of its own to ask for one. Absent on every turn
            // that refused nothing, which is every turn of a window with nothing
            // moving in it. See [`Runtime::animation_frame_is_due`].
            self.window
                .frame_clock
                .owes_a_frame()
                .then(|| self.next_animation_deadline())
                .flatten(),
        ];
        let wake = earliest_named_deadline(DEADLINE_OWNERS, deadlines);
        if self.app.trace_perf
            && !running.any()
            && !self.window.frame_clock.owes_a_frame()
            && self.window.pending_frames.pending_frame().is_none()
            && !self.window.chrome_present_pending
            && !self.window.pictures_owe_a_frame
            && !self.window.cards.owes_frame()
            && let Some((owner, at)) = wake
            && at.saturating_duration_since(now) < Duration::from_millis(100)
        {
            trace_sink::stderr_line(format!(
                "BT_PERF_TRACE idle_wake owner={owner:?} in_us={}",
                at.saturating_duration_since(now).as_micros(),
            ));
        }
        let wake_deadline = wake.map(|(_, at)| at);
        self.reap_exited_tabs()?;
        Ok(wake_deadline)
    }
}
