//! `frame` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::TabId;
use crate::present_gate::presentation_equivalent;
use crate::runtime::math::math_copy_window;
use crate::wake_owner::WakeOwner;
use crate::window_seam::prelude::*;
use crate::{
    AnimationWork, BrokerAim, DecorationSenders, Drag, FILES_NOTICE_DWELL, FOOT_REVEAL_FEEDBACK,
    Fading, FrameImageReferences, FrameTraces, GhostFace, HyperlinkActivation, HyperlinkHover,
    LeafId, MathWorkerRequest, Motion, PaneDraw, PathWorkerRequest, PictureOnGlass, PointerTarget,
    PresentIntent, PreviewSurface, Runtime, STARTUP_PTY_POLL_INTERVAL, ScaleWorkerRequest,
    SearchRefresh, ShellAddress, TabPress, TabState, cmdrail, computation_lane,
    disable_math_worker_state, files, foreground_program, git, handoff_lane, hang_watch,
    hyperlink_activation, i18n, ime_outbound, lane, notify, pace, present_diagnostics,
    present_gate, preview, seats, settings, settling, taskbar_lane, termscroll, toast, update,
    webhost,
};
use crate::{ShellSession, TermView, TermViewPorts};
use anyhow::Context;
use anyhow::Result;
use bt_layout::SeatId;
use bt_platform::admission::{WaitToken, doors};
use bt_render::{
    FrameSource, FrameTrigger, PresentOutcome, compose_preedit, foreground_rgb,
    frame_content_digest, frame_is_alternate_screen,
};
use bt_term::{DualPlaneSession, SessionDecorationTask};
use bt_viewport::{HyperlinkHit, ViewportFrame};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;
use winit::window::WindowId;

impl<B: Backend> Runtime<'_, B> {
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
                self.shell_mut().session.port_release_presentation_hold();
            }
            // **The search re-asks itself here, before anything is projected** — so
            // a line that has just frozen, or a character the shell has just echoed,
            // is inside the hit set of the very frame that is about to draw it.
            //
            // Not forced: this is the rebuild *output* caused, so the match the
            // reader is standing on keeps its place (B58's first half), and the two
            // planes that did not move are not re-scanned at all (see
            // [`SearchScanCache`]). A frame that changed nothing about the search
            // costs one comparison of a fifty-row scan. **And it reads no slice of a
            // walk in progress** (ticket 51): the keystroke or the turn that caused
            // this publish has read its one, and the next is the next turn's.
            self.refresh_search(SearchRefresh::Output)?;
            let active = self.window.active_tab;
            let senders = self.app.decoration_senders();
            let window = self.window_id();
            dispatch_tab_decoration_tasks(
                window,
                &mut self.window.tabs[active],
                &senders,
                Instant::now(),
                &mut self.app.math_worker_running,
                &mut self.app.math_worker_notice_pending,
            );
            self.window.composed_terminal_frames =
                self.window.composed_terminal_frames.saturating_add(1);
            // **What the projection cost this frame** (`BT_PERF_TRACE projection`,
            // T-MATH-TOGGLE-STUTTER). The gesture that made this worth printing is
            // the formula source toggle: it changes the set of suppressed ids, which
            // is the one thing that sends `project` down its rebuild road, and the
            // question a person at the machine has to be able to answer is whether
            // that rebuild measured the lines that changed or the whole scrollback.
            // `lines_measured` is exactly that number. Off unless the switch is set,
            // and the reading costs one `u64` load when it is not.
            let trace_perf = self.app.trace_perf;
            let measurements_before = self.shell_pane().view.projection.line_text_measurements();
            // **Which road the projection took**, beside how many lines it measured
            // (T-MATH-MARKS-IN-SOURCE-FACE). A frame of a change of face measures no
            // lines at all either way — the cache answers for every one of them — so
            // `lines_measured` alone cannot tell a band moved where it stands from a
            // band that made the whole document be pushed through two trees again.
            let rebuilds_before = self.shell_pane().view.projection.rebuilds();
            let bands_moved_before = self.shell_pane().view.projection.bands_moved();
            let projected = {
                // Bound once, to the focused leaf: `session` and `projection` are
                // two fields of one shell, and reaching each through its own deref
                // would be two borrows of the tab rather than one borrow of the
                // leaf.
                let leaf = self.window.tabs[active].shell_pane_mut();
                let projection_started_at = trace_perf.then(Instant::now);
                // The frame, and the paths it drew that nobody has answered for yet
                // (§7.1.5j): `bt_compose::project` files those as the session's
                // questions in the same step, because the projection is what walks
                // every line and the session is what owns a worker. It schedules
                // nothing: the hold below decides first.
                let projected = bt_compose::project(bt_compose::Pane {
                    session: &mut leaf.session.session,
                    view: &mut leaf.view.projection,
                })
                .context("project terminal grid into viewport frame")?;
                let project_us =
                    projection_started_at.map(|started_at| started_at.elapsed().as_micros());
                // **Written after the frame, not before it.** The three scroll numbers
                // are what this frame decided — where the window stands, how far it
                // could stand, and how much of that the blank tail under the prompt is
                // spending — and a line printed before the projection would report
                // the frame before this one, which is the answer to a different
                // question than the one the reader is holding the log for.
                if let Some(project_us) = project_us {
                    crate::diagnostics::stderr_line(format!(
                        "BT_PERF_TRACE projection source={:?} project_us={project_us} lines_measured={} projected_lines={} rebuilt={} band_moved={} scroll_offset_subpixels={} scroll_extent_subpixels={} bottom_relief_subpixels={}",
                        trigger.source,
                        leaf.view
                            .projection
                            .line_text_measurements()
                            .saturating_sub(measurements_before),
                        leaf.view.projection.projected_line_count(),
                        leaf.view
                            .projection
                            .rebuilds()
                            .saturating_sub(rebuilds_before),
                        leaf.view
                            .projection
                            .bands_moved()
                            .saturating_sub(bands_moved_before),
                        leaf.view.projection.scroll_offset_subpixels(),
                        leaf.view.projection.scroll_extent_subpixels(),
                        leaf.view.projection.bottom_relief_subpixels(),
                    ));
                }
                projected
            };
            let asked_about_paths = projected.path_work_filed;
            let mut terminal_frame = projected.frame;
            // State-driven frame hold. Review displacement holds a vanished scroll anchor during a
            // resize reprint. Independently, an unmatched off-band stale-pending DPI record holds the
            // previous complete formula frame while a proven primary reprint is between clear and exact
            // source re-anchor. Both release through projection/session facts (re-anchor, explicit user
            // takeover, or hard lifecycle retirement), never a timer.
            //
            // **Decided before anything is scheduled**: a held frame is not acted on, so the work
            // it shows is not filed this turn either (design T-COMPOSE-CRATE §3.2, step 4).
            if projected.hold_requested && self.window.last_presented_frame.is_some() {
                if self.app.trace_perf {
                    crate::diagnostics::stderr_line(format!(
                        "BT_PERF_TRACE hold=presentation source={:?} review={} exact_source={}",
                        trigger.source,
                        u8::from(self.shell_pane().view.projection.review_hold()),
                        u8::from(
                            self.shell_pane()
                                .view
                                .projection
                                .exact_source_reprint_hold()
                        ),
                    ));
                }
                return Ok(false);
            }
            if hang_watch::during(hang_watch::Station::DetectionPass, || {
                bt_compose::schedule(&mut self.shell_mut().session, &terminal_frame)
            }) != 0
                || asked_about_paths
            {
                dispatch_tab_decoration_tasks(
                    window,
                    &mut self.window.tabs[active],
                    &senders,
                    Instant::now(),
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
            self.shell_pane_mut().view.frame_image_references = scan;
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
                    crate::diagnostics::stderr_line(format!(
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
            // **The acknowledgment, at the publish boundary**: below the held and the
            // unchanged returns, immediately before the frame enters the slot, and never
            // again for it — a present that fails files the same frame back without one.
            bt_compose::acknowledge(
                &mut self.shell_mut().session,
                &composed.frame,
                trigger.occurred_at,
            );
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
            crate::diagnostics::stderr_line("BT_PERF_TRACE defer=synchronized-update".to_owned());
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
            //
            // Two refusals and one sentence: the address door's, answered here, and the
            // system's, answered when the OS hand-off lane does (2026-09-22) — both under the
            // cells the address is printed in, as they always were.
            HyperlinkActivation::Browser(url) => match crate::runtime::web::browser_ask(&url) {
                Some(ask) => {
                    self.hand_off(
                        ask.if_refused(crate::handoff_lane::OnRefused::HyperlinkRefused(hyperlink)),
                    )?;
                }
                None => {
                    self.window
                        .hyperlink_hover
                        .show_refused(hyperlink, crate::LinkRefusal::of_address(&url));
                    self.publish_interaction_frame()?;
                }
            },
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
                    self.window
                        .hyperlink_hover
                        .show_refused(hyperlink, crate::LinkRefusal::of_address(&url));
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
                self.open_local_path_verified(&path, facts)?;
            }
            HyperlinkActivation::Preview(path, at) => self.open_preview_at(path, at)?,
            // **A share on another machine, handed to the system** (ticket 14, owner ruling
            // 2026-09-21). Nothing was asked about it — not on the hover, not on this press — so
            // it leaves through the door that needs no ledger, on the OS hand-off lane, where a
            // cold `\\server` holds that lane and not this window. The door refuses a program by
            // the list a local file meets, and says so in the files notice.
            HyperlinkActivation::Share(path) => {
                self.open_unverified_reference(&path)?;
            }
            // **Any other scheme, handed to whatever this machine has registered for it** (ticket
            // 14, owner ruling 2026-09-21). A machine with no handler refuses it, and the refusal
            // is said where the web arm's is: under the cells the address is printed in.
            HyperlinkActivation::Scheme(uri) => {
                self.hand_uri_to_the_system(
                    &uri,
                    crate::handoff_lane::OnRefused::HyperlinkRefused(hyperlink),
                )?;
            }
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
                self.reveal_verified(&path, facts)?;
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
            HyperlinkActivation::FilesColumn(path) => {
                self.locate_folder_in_files_column(&path, None)?
            }
            HyperlinkActivation::Blocked(refusal) => {
                self.window.hyperlink_hover.show_refused(hyperlink, refusal);
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
        if !self.resize_present_owed
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
            // A strip covers the chrome under it the way a window does
            // (T-STRIP-HOVER-THROUGH): the point is the strip's, not a target.
            PointerTarget::Notice(..) | PointerTarget::Search(_) => None,
        }
    }

    /// Put the frame already on screen back in the slot so a pure chrome change
    /// reaches the glass. Chrome lives beside the frame, exactly as the peek
    /// flyout does, so `redraw` would otherwise find nothing queued and skip.
    pub(crate) fn present_chrome_change(&mut self) -> Result<()> {
        if !self.resize_present_owed
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
                let native = self.window.window.present_facts();
                crate::diagnostics::stderr_line(
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
        crate::diagnostics::stderr_line(format!(
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
            resize_pending: self.resize_present_owed || matches!(source, FrameSource::Resize),
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
                    .and_then(|leaf| leaf.view.last_presented_frame.as_ref())
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
                    metrics: seat.metrics,
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
            self.admit_resize_present(&frame)?;
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
                let bt_compose::Projected {
                    frame: mut projected,
                    path_work_filed,
                    ..
                } = bt_compose::project(bt_compose::Pane {
                    session: &mut leaf.session.session,
                    view: &mut leaf.view.projection,
                })
                .context("project an unfocused pane's grid into a viewport frame")?;
                // A pane nobody has the keyboard in still draws paths, and still owes them an answer.
                owes_the_engine |= path_work_filed;
                // No hold is read here: a pane the user is not typing in holds nothing, and
                // the hold is the focused leaf's, decided in `publish_frame_inner`.
                owes_the_engine |= hang_watch::during(hang_watch::Station::DetectionPass, || {
                    bt_compose::schedule(&mut leaf.session.session, &projected)
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
                let senders = self.app.decoration_senders();
                let window = self.window_id();
                dispatch_tab_decoration_tasks(
                    window,
                    &mut self.window.tabs[active],
                    &senders,
                    Instant::now(),
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
                // Each frame with the metrics it was just projected at (ticket 37).
                let tab = &self.window.tabs[active];
                let frame_for = |seat| {
                    let metrics = tab.sessions.get(&seat)?.view.metrics;
                    if seat == focused_leaf {
                        return Some((focused_body.viewport, &frame, metrics));
                    }
                    unfocused_frames
                        .iter()
                        .find(|(pane, _)| pane.seat == seat)
                        .map(|(pane, projected)| (pane.viewport, projected, metrics))
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
            // **Both construction paths carry the seat's own metrics** (ticket 37): every frame
            // here was projected this pass from its leaf's session, which was last applied at
            // `TermView::metrics`, so that is what the picture is drawn at — and what the
            // commit below records beside it as `presented_metrics`. One list, read once, so
            // the tables, the seat frames and the commit are handed the same pairs.
            let frame_metrics: Vec<bt_render::CellMetrics> = {
                let sessions = &self.window.tabs[active].sessions;
                let focused_metrics = sessions.get(&focused_leaf).map_or_else(
                    || self.window.renderer.base_metrics(),
                    |leaf| leaf.view.metrics,
                );
                std::iter::once(focused_metrics)
                    .chain(unfocused_frames.iter().map(|(pane, _)| {
                        sessions
                            .get(&pane.seat)
                            .expect("an unfocused frame was projected from a leaf of this tab")
                            .view
                            .metrics
                    }))
                    .collect()
            };
            let table_sources = Self::table_sources(
                std::iter::once(&frame)
                    .chain(unfocused_frames.iter().map(|(_, it)| it))
                    .zip(frame_metrics.iter().copied()),
                self.app.gpu.font_environment_epoch(),
            );
            hang_watch::during(hang_watch::Station::RedrawTables, || {
                self.refresh_table_paints(&table_sources)
            });
            // **The owner, not the focus** (user report + ruling, 2026-08-13): the
            // focused entry's caret flag is "is this the caret typing would land in" —
            // see [`Self::keyboard_owner_is_a_shell`] for why the answer stopped being
            // "yes, it is the focused pane".
            let seat_frames = bt_compose::seat_frames(
                bt_compose::PaneDrawInput {
                    seat: focused_body.viewport,
                    clip: focused_body.clip,
                    frame: &frame,
                    metrics: frame_metrics[0],
                },
                self.keyboard_owner_is_a_shell(),
                unfocused_frames.iter().zip(&frame_metrics[1..]).map(
                    |((pane, projected), metrics)| bt_compose::PaneDrawInput {
                        seat: pane.viewport,
                        clip: pane.clip,
                        frame: projected,
                        metrics: *metrics,
                    },
                ),
            );
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
                        // **The recovered folder is asked about after the first frame and
                        // never before it** (T-RECOVERED-FOLDER): on its own worker, once per
                        // process (`recovered::begin`).
                        crate::recovered::begin(&crate::persist::storage_dir());
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
                        crate::diagnostics::stderr_line(format!(
                            "BT_RESIZE present={}us columns={} rows={}",
                            latency.event_to_present_call.as_micros(),
                            frame.columns,
                            frame.grid_rows
                        ));
                    }
                    if has_text && !self.window.first_text_presented {
                        self.window.first_text_presented = true;
                        // **An update's trial is ready at its first pane text**
                        // (`update_trial`, F-14): its receipt goes to the storage
                        // worker, and this thread goes on drawing. Once per
                        // process; nothing at all outside a trial.
                        if let Some(receipt) = crate::update_trial::receipt_due() {
                            match self.app.session_store.write_receipt(receipt) {
                                Some(answer) => crate::update_trial::receipt_handed(answer),
                                None => crate::diagnostics::stderr_line("BT_UPDATE_TRIAL no storage worker to write the receipt; this trial stays unconfirmed".to_owned()),
                            }
                        }
                        if self.app.trace_startup {
                            let text_visible = self.app.startup_started.elapsed();
                            self.window.first_text_visible = Some(text_visible);
                            crate::diagnostics::stderr_line(format!(
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
                    // Each frame and the metrics it was drawn at, recorded together: the pair is
                    // what every pointer question about the pane reads (ticket 37).
                    for ((pane, projected), metrics) in unfocused_frames
                        .into_iter()
                        .zip(frame_metrics.iter().skip(1))
                    {
                        if let Some(leaf) = self.window.tabs[active].sessions.get_mut(&pane.seat) {
                            leaf.view.commit_presented(projected, *metrics);
                            mark_leaf_painted(&mut leaf.session);
                        }
                    }
                    if let Some(leaf) = self.window.tabs[active].sessions.get_mut(&focused_leaf) {
                        leaf.view.commit_presented(frame.clone(), frame_metrics[0]);
                        mark_leaf_painted(&mut leaf.session);
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
                    self.resize_present_owed = false;
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
    /// **Ask the desktop where this window is, once, and keep the answer for the turn**
    /// (ticket 48; `docs/ARCHITECTURE.md` §5.3 row 13; `attention` plan §5.2).
    ///
    /// The only writer of [`WindowRuntime::observed_place`] and the four fields it is written
    /// with (`window_hidden`, `window_exposed`, `taskbar_reading`, `attention_sampled_at`),
    /// and the only caller of [`sample_window_place`]. Called from three places, and each is
    /// the moment a delivery can be decided:
    ///
    /// * **the head of [`Self::turn`]** — one reading shared by `drain_pty` and
    ///   `advance_strip_animation`, which used to take one each on the same turn (owner's
    ///   stall reports 2026-09-23/24: the same question asked twice and paid for twice);
    /// * **the window's birth** (`dress_new_window`), so the install road's drain, which runs
    ///   before the first turn, reads a real answer;
    /// * **an attention delivery that arrives between turns** (`AppEvent::AttentionSpoke`).
    ///   Not parked for the next turn, because on Windows there may be none for seconds: inside
    ///   the OS's modal move/size loop winit sends no `AboutToWait`, so a message held for a
    ///   turn would wait for the hand to let go of the frame;
    /// * **the re-placing of a taskbar flash** the taskbar lane's newest answer contradicts
    ///   (`Runtime::replace_contradicted_flash`, ticket 62), so the delivery is decided again on
    ///   a reading that carries the answer.
    ///
    /// "Every turn, never cached across them" (user rulings 2026-08-28 and 2026-09-01) holds for
    /// hidden, exposed and focus: a turn's head replaces the reading before anything in that turn
    /// reads it. The taskbar bit is the taskbar lane's latest answer (ticket 62), read here and
    /// asked of the shell only on that lane.
    ///
    /// Focus is the window's own answer (`Window::has_focus`), for the measured reason
    /// `drain_pty` gives; `WindowRuntime::window_focused` keeps its `WM_SETFOCUS` writer and is
    /// not written here.
    pub(crate) fn observe_window_place(&mut self) {
        let window = &self.window.window;
        let previous = self.window.observed_place;
        let (place, taskbar) = hang_watch::during(hang_watch::Station::Place, || {
            let focused =
                hang_watch::during(hang_watch::Station::PlaceFocus, || window.has_focus());
            sample_window_place(window, focused, previous)
        });
        let window = &mut *self.window;
        window.observed_place = place;
        window.window_hidden = place.hidden;
        window.window_exposed = place.exposed;
        window.taskbar_reading = taskbar;
        window.attention_sampled_at = Some(Instant::now());
    }

    /// **Nothing in this window is moving, and nothing is owed to its glass**: no
    /// journey running, no refused animation frame, no frame waiting to be presented,
    /// no chrome present pending, no picture or card owing a frame.
    ///
    /// The idle test the turn's `BT_PERF_TRACE idle_wake` line was written with, named
    /// so that the web engine's warm-up asks the same question (ticket 54) rather than
    /// a second spelling of it.
    fn turn_facts(
        &self,
        running: crate::pace::Lanes,
        travelling: crate::pace::Lanes,
        resize_owed: bool,
    ) -> crate::web_warmup::TurnFacts {
        crate::web_warmup::TurnFacts {
            running,
            travelling,
            repaint_owed: self.window.frame_clock.owes_a_frame()
                || self.window.pending_frames.pending_frame().is_some()
                || self.window.chrome_present_pending
                || self.window.pictures_owe_a_frame
                || self.window.cards.owes_frame(),
            resize_owed,
        }
    }

    pub(crate) fn turn(
        &mut self,
        now: Instant,
        application_clocks: bool,
    ) -> Result<Option<(WakeOwner, Instant)>> {
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
        // **Where the window is, asked once for the whole turn** (ticket 48; `ARCHITECTURE`
        // §5.3 row 13). The drain and the strip tick both decide what the reader is owed from
        // it, and the doors that fire inside the turn read it back; none of them asks again.
        self.observe_window_place();
        // **This window's frame, for the turn's account** (0.4.6 A3): a turn is measured against
        // the shortest frame among the windows on the glass (`T − t₀`); a hidden window owes none.
        if !self.window.window_hidden {
            hang_watch::frame_interval(self.window.frame_clock.interval());
        }
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
        // The four answers a platform modal can have left behind, under their
        // own name (T-STATION-SPLIT) — see [`hang_watch::Station::Pickers`].
        hang_watch::at(hang_watch::Station::Pickers);
        self.apply_math_context_menu_result()?;
        self.apply_folder_pick_result()?;
        self.apply_image_pick_result()?;
        self.apply_save_pick_result()?;
        // **The pastes the window no longer wants, and what its holds can spend**
        // (`clipboard_lane`): on every turn, so a paste's deadline is kept on the turn it falls
        // in. The lane's answers are taken on its wake's turn, for every window at once
        // (`FolioApp::answer_clipboard`), never here.
        hang_watch::during(hang_watch::Station::ClipboardRead, || {
            self.settle_clipboard(now)
        })?;
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
        hang_watch::during(hang_watch::Station::ClockAdvanceCursorBlinkIfDue, || {
            self.advance_cursor_blink_if_due(now)
        })?;
        // **A quit that holds the storage worker** (T-QUIT-DOCUMENTS): while its documents
        // and their copies are on it, nothing else of this run's is queued there — no
        // re-read for the storage watch and no autosave — so the session write goes behind
        // them and behind nothing else. The watch keeps its stamps and the store its debt.
        // An update trial's receipt (first text, below) goes ahead of them, not behind.
        let quit_holds_the_worker = self
            .app
            .quit
            .as_ref()
            .is_some_and(crate::quit::Quit::holds_the_storage_worker);
        // The storage worker's two clocks — the storage watch's re-read and the autosave — run
        // under this one predicate, and the wake fold books them under it too
        // (T-WINDOW-THREAD-UNDER-LOAD): a clock reported while its spender is held is an instant
        // the loop would spin on.
        let storage_clocks = application_clocks && !quit_holds_the_worker;
        if application_clocks {
            // A poll of the world outside this process, put back on the clock
            // run's name the moment it returns (T-STATION-SPLIT).
            let leaving = hang_watch::enter(hang_watch::Station::Watches);
            hang_watch::during(hang_watch::Station::ClockAdvanceSchemeWatch, || {
                self.advance_scheme_watch(now)
            })?;
            if storage_clocks {
                hang_watch::during(hang_watch::Station::ClockAdvanceStorageWatch, || {
                    self.advance_storage_watch(now)
                })?;
            }
            hang_watch::at(leaving);
        }
        // The update check is the one external fact whose semantics is a
        // schedule. The first open window turns it with the other application
        // clocks; this only decides and books the next look, while the existing
        // update worker performs the request.
        let update_check_deadline = application_clocks
            .then(|| update::advance_schedule(now, update::unix_epoch_ms()))
            .flatten();
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
        // **A changed search's history, one slice a turn** (ticket 51, D-38;
        // `ARCHITECTURE` §5.3 row 6). The keystroke read the newest slice; this reads
        // the next, after the drain so the lines the shell froze meanwhile are carried
        // rather than owed. Under the scan's own name, which is what the work is; the
        // fold below books the next turn at once while a walk is still owed.
        hang_watch::during(hang_watch::Station::SearchScan, || {
            self.advance_search_scan()
        })?;
        // **This window's seat in the git watch** (R31's D; T-WINDOWS-ALL): the
        // repositories its drawn Git pages show, and the news it was told about
        // them. Every window, not only the one that turns the application's
        // clocks: the subscriptions and their clocks are the application's, one
        // clock over the union of every window's pages, and they were ripened for
        // every seat before the first turn (`FolioApp::ripen_git_news`), so each
        // window re-reads its own pages on the same pass.
        hang_watch::at(hang_watch::Station::Watches);
        hang_watch::during(hang_watch::Station::ClockAdvanceGitWatch, || {
            self.advance_git_watch()
        })?;
        if storage_clocks {
            // `flush_if_due` stamps [`hang_watch::Station::Autosave`] itself,
            // and since T-STATION-SPLIT that name covers the file it writes and
            // nothing else: the clock run below says its own name on the very
            // next line.
            // **A session that cannot be saved is said once per streak** (`persist`'s
            // `REFUSALS_BEFORE_TOLD`; `docs/M2-persistence-schema-v1.md` §5.3), on the card the
            // start-up announcement uses for it.
            if let Some(fault) = self.app.session_store.autosave(now) {
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    fault,
                )?;
            }
        }
        // **The one rule, in the one place it can be kept.** Whichever rung holds
        // the keyboard publishes its caret here, at the tail of every pass —
        // which is what "every frame the caret can move" means without anybody
        // having to enumerate the ways it moves: typing, scrolling, resizing and
        // a capsule relaid all wake this loop, and the flush at the turn's tail
        // (ticket 63) turns a rectangle that did not move into no call at all.
        // Here it is only wanted. The terminal's rung
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
        // **The window's title, told to the OS once a turn at most** (ticket 49). After
        // the drain and every clock above, because any of them can change which tab is
        // active or what it is called; `want_title` only said so. A title held for
        // its frame books its wake-up in the fold below.
        self.flush_title(now);
        // Service the PTY gate after every other due task that can mutate session state, then carry
        // the deadline derived from that exact sample into the control-flow decision below.
        let pty_resize_deadline = self.flush_pending_pty_resize(now)?;
        // **And the input method's caret area, on the same terms** (ticket 63). Every
        // road above — the drain's frames, the IME events' frames, a preview repainted,
        // the caret offer at the head of the clock run, and the frame a resize the PTY
        // gate just let through publishes — only said which area it wants; this is
        // where the system is told, once, and only if the area moved. An area held for
        // its 60Hz slot books its wake-up in the fold below. **After the PTY gate**
        // (T-CARET-EVERY-LAYOUT, 2026-10-27): the system caret's slot is wanted by every
        // frame, so an area wanted by that resize's frame after this flush was left for
        // the fold to read as owed at once — a wake at the turn's own instant.
        hang_watch::during(hang_watch::Station::ClockFlushImeCursorArea, || {
            self.flush_ime_cursor_area(now)
        });
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
        // **What a gesture set going**, from the same walk (ticket 60, F1): the lanes less
        // Folio's own periodics. See [`crate::web_warmup::TurnFacts::stirs`].
        let travelling =
            self.running_journeys(now, strip_animation.travelling, terminal_thumbs.travelling);
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
            .filter_map(|(_, leaf)| bt_compose::deadlines(&leaf.session).synchronized_update)
            .min();
        // Every leaf of the tab on screen, for the reason the line above gives:
        // a stability window is a property of one pane's rows, and the pane that
        // printed the block is often not the one holding the keyboard. See
        // [`Self::live_stability_deadline`].
        let live_stability_deadline = self.live_stability_deadline();
        // **The web engine's warm-up** (ticket 54, D-64; `ARCHITECTURE` §5.3 row 21).
        // Every window says whether it is at rest — the same test the idle trace below
        // uses, plus a resize still owed to a shell — and a window that is not stirs the
        // clock; the window that turns the application's clocks then asks it, and it
        // fires at most once per process. After the fold's inputs above, because they
        // are what "at rest" is read from, and before the fold, which books its wake.
        let facts = self.turn_facts(
            running,
            travelling,
            pty_resize_deadline.is_some() || resize_finish_deadline.is_some(),
        );
        if facts.stirs() {
            self.app.web_warmup.stir(now);
        }
        if application_clocks {
            hang_watch::during(hang_watch::Station::WebWarmup, || self.warm_web_engine(now));
        }
        // Unlike the process-wide schedule, this clock belongs only to a
        // window actually showing About. It wakes at the next relative-time
        // bucket so “Last checked” ages under a motionless pointer.
        let last_checked_deadline = (self.window.settings.is_open()
            && self.window.settings.category() == settings::SettingsCategory::About)
            .then(|| update::last_checked_deadline(now, update::unix_epoch_ms()))
            .flatten();
        const DEADLINE_OWNERS: [WakeOwner; 56] = [
            WakeOwner::StartupPoll,
            WakeOwner::ImeCursor,
            WakeOwner::ShellCaret,
            WakeOwner::TabPress,
            WakeOwner::RenameCaret,
            WakeOwner::StripAnimation,
            WakeOwner::WebTeardown,
            WakeOwner::PtyResize,
            WakeOwner::ResizeFinish,
            WakeOwner::SynchronizedUpdate,
            WakeOwner::LiveStability,
            WakeOwner::PtyCoalesce,
            WakeOwner::AttentionCredential,
            WakeOwner::Tooltip,
            WakeOwner::KeyHint,
            WakeOwner::CardsHint,
            WakeOwner::Toast,
            WakeOwner::ClipboardPasteDeadline,
            WakeOwner::CommandFlash,
            WakeOwner::CommandRails,
            WakeOwner::TerminalThumbs,
            WakeOwner::LayoutPeek,
            WakeOwner::FilePeek,
            WakeOwner::FilePeekCloseGrace,
            WakeOwner::FilePeekDwell,
            WakeOwner::Float,
            WakeOwner::RevealedFoot,
            WakeOwner::WebZoomAcknowledgement,
            WakeOwner::WebDialogAcknowledgement,
            WakeOwner::PreviewSaveNotice,
            WakeOwner::PreviewRefusal,
            WakeOwner::Chevrons,
            WakeOwner::PaneMenu,
            WakeOwner::TerminalMenu,
            WakeOwner::TabMenu,
            WakeOwner::DragSpring,
            WakeOwner::DragAutoScroll,
            WakeOwner::HyperlinkHover,
            WakeOwner::PeekHover,
            WakeOwner::FormulaTools,
            WakeOwner::FormulaToggle,
            WakeOwner::FormulaCopyAcknowledgement,
            WakeOwner::PreviewResample,
            WakeOwner::SessionSave,
            WakeOwner::SchemesWatch,
            WakeOwner::StorageWatch,
            WakeOwner::GitWatch,
            WakeOwner::UpdateCheck,
            WakeOwner::AboutLastChecked,
            WakeOwner::PreviewWatch,
            WakeOwner::FilesWatch,
            WakeOwner::RefusedFrame,
            WakeOwner::WindowTitle,
            WakeOwner::SearchWalk,
            WakeOwner::WebEngineWarmUp,
            WakeOwner::SpareWebController,
        ];
        let deadlines = [
            startup_deadline,
            // The input method's forms and the system caret: two slots, one flush.
            self.window
                .ime_cursor
                .deadline(now)
                .into_iter()
                .chain(self.window.system_caret.deadline(now))
                .min(),
            // Only while a shell holds the keyboard: a frozen caret owes no
            // wake-up at all (ruling 2026-08-13).
            self.keyboard_owner_is_a_shell()
                .then(|| self.window.cursor_blink.deadline())
                .flatten(),
            // The press's own 180ms, and only while it still owes one — a press
            // that has been paid or has slipped reports nothing, so a held
            // button costs no wake-ups at all.
            self.held_tab_press().and_then(TabPress::wake_deadline),
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
            // **No foreground-program cadence** (T-WINDOW-THREAD-UNDER-LOAD round 3). The cadence
            // is a gate on the publish of a pane's own output (`dispatch_tab_decoration_tasks`
            // asks `Cadence::should_request` with the publish's instant), and no turn spends it:
            // booked here, a frame candidate's five-second instant — in a background tab, or in
            // the tab on screen while nothing printed — came due on a turn that probed nothing,
            // stayed in the past, and the loop spun on it until that tab published.
            // **The bounded wait for the rest of a burst**, and the wake that keeps
            // [`coalesce::Pending`]'s invariant true: a deferred publication is never `Some`
            // without this line booking the turn that pays it. A window whose panes are not
            // mid-burst reports nothing and costs no wake-ups at all, which is every window
            // almost always.
            self.window.pty_coalesce.until,
            // A standing credential's ten minutes, and only while one is standing. A window whose
            // panes are asking for nothing reports nothing and costs no wake-ups at all — which is
            // every window, almost always.
            // Spent by `settle_attention`, which runs behind the strip's tick and the window's frame
            // gate (`advance_strip_animation`), so it is booked no earlier than the turn that gate
            // admits (T-WINDOW-THREAD-UNDER-LOAD): a due credential refused by the gate is an
            // instant the loop would otherwise spin on until the gate opens.
            attention_ledger_deadline(&self.window.tabs).map(|due| {
                self.strip_animation_next_tick(self.window.frame_clock.interval())
                    .map_or(due, |tick| due.max(tick))
            }),
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
            // A paste the clipboard lane still owes this window, withdrawn at its deadline if it is
            // still owed then (`clipboard_lane::PASTE_DEADLINE`); nothing while none is.
            self.window.clipboard.deadline(),
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
            // Under its spender's own predicate (`storage_clocks`, T-WINDOW-THREAD-UNDER-LOAD).
            storage_clocks
                .then(|| self.app.session_store.deadline())
                .flatten(),
            // The schemes folder's own debounce, on the same terms: absent for
            // every window whose folder nobody is editing.
            application_clocks
                .then(|| self.app.scheme_watch.deadline())
                .flatten(),
            // And the storage folder's, on exactly the same terms — absent for
            // every window in which nothing at all has been written lately.
            // Under its spender's own predicate (`storage_clocks`, T-WINDOW-THREAD-UNDER-LOAD).
            storage_clocks
                .then(|| self.app.storage_watch.deadline())
                .flatten(),
            // The debounce a change notification started (R31's D): the
            // application's clock, folded by the window that turns the
            // application's clocks; the next pass ripens it for every window.
            // Absent — and therefore costing nothing — while no repository any
            // window shows has unanswered news, which is most of the time.
            application_clocks
                .then(|| self.app.git_watch.deadline())
                .flatten(),
            update_check_deadline,
            last_checked_deadline,
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
            // **A title held for its frame** (ticket 49): the one wake-up that makes
            // "the last title always reaches the OS" true on a window where nothing
            // else is happening. Absent unless a title is being held.
            self.window.title.deadline(),
            // **A search walk still owed** (ticket 51): the next turn at once, so the
            // rest of a changed question's history is read a slice a turn with no timer
            // and no other event. Absent unless a walk is partial, and a walk ends in
            // at most (history / slice) turns, so this is never a standing wake.
            self.search_walk_deadline(now),
            // **The web engine's warm-up** (ticket 54): the end of the grace or of the
            // quiet stretch, whichever is later, until it has fired once — so an idle
            // window that nothing else wakes still reaches the turn the clock is due
            // on. Absent once it has fired, before the first frame, and while the
            // restore card stands (the gesture that answers it books the next one). On a turn whose
            // deferrable work yielded, not before the frame boundary it yielded to (0.4.6 A4).
            application_clocks
                .then(|| {
                    self.app
                        .web_warmup
                        .deadline(self.web_warmup_waits_for_the_restore_card())
                        .map(hang_watch::deferred_until)
                })
                .flatten(),
            // **The spare web controller** (ticket 60): while it is being made and its engine has
            // spoken, the next quiet instant (absent while the restore card stands); otherwise its
            // own clocks — the engine's start deadline, its browser-exit wait. Absent for a
            // profile that never gets one, which is most turns of most runs. The quiet instant is
            // a deferrable unit's, so on a turn whose deferrable work yielded it is not before the
            // frame boundary it yielded to (0.4.6 A4).
            application_clocks
                .then(|| {
                    let quiet = self
                        .app
                        .web_warmup
                        .quiet_at(self.web_warmup_waits_for_the_restore_card());
                    self.app
                        .web_spare
                        .deadline(quiet.map(|at| hang_watch::deferred_until(at.max(now))))
                })
                .flatten(),
        ];
        let wake = earliest_named_deadline(DEADLINE_OWNERS, deadlines);
        if self.app.trace_perf
            && facts.at_rest()
            && let Some((owner, at)) = wake
            && at.saturating_duration_since(now) < Duration::from_millis(100)
        {
            crate::diagnostics::stderr_line(format!(
                "BT_PERF_TRACE idle_wake owner={:?} in_us={}",
                WakeOwner::name(owner),
                at.saturating_duration_since(now).as_micros(),
            ));
        }
        self.reap_exited_tabs(now)?;
        Ok(wake)
    }

    /// **How solid each card head's hover-revealed run is this frame** — the `×`
    /// and the offer to pin, in Cards mode (`bt_render::HOVER_CHROME_FADE`).
    ///
    /// [`Self::settled_head_ink`] on the card column and [`Self::settled_tab_ink`]
    /// in shape: keyed by [`TabId`] inside for the register's sake and by index
    /// outside because that is what the painter walks, translated here because
    /// this is the one place that can do both on the same frame.
    ///
    /// **The list is whatever is hovered plus whatever is still leaving**, which
    /// is what makes it a list rather than the one card under the pointer: a hand
    /// crossing from one card to the next leaves two entries for ninety
    /// milliseconds, one coming up and one going down.
    ///
    /// **Nothing is pulled toward `1.0` outside Cards mode.** This register is the
    /// card's alone, and a hover in another layout must not leave an entry in it
    /// that the column would then draw with when the mode is turned on. The
    /// *width* half of a card head's reveal is not in here at all: it is
    /// `TabTrailer::reveal`, the same tween the strip and the rail open their
    /// pins with, driven off the very [`Self::hovered_tab`] this function reads
    /// (owner ruling 2026-09-14 — a card head is a tab head, slide included).
    pub(crate) fn settled_card_ink(&mut self, now: Instant) -> Vec<(usize, f32)> {
        let motion = self.app.motion;
        let showing = self.sampled_rail(now).draws_focus_rail();
        let hovered = showing.then(|| self.hovered_tab()).flatten();
        let places: BTreeMap<TabId, usize> = self
            .window
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| (tab.id, index))
            .collect();
        let mut wanted: Vec<TabId> = self
            .window
            .settling
            .held()
            .into_iter()
            .filter_map(|key| match key {
                Fading::CardHead(tab) => Some(tab),
                _ => None,
            })
            .collect();
        // A card whose tab has been closed mid-fade is forgotten rather than
        // eased: there is nothing left on the glass for a fade home to be drawn
        // on — `settled_tab_ink`'s own argument, one surface along.
        for gone in wanted
            .iter()
            .copied()
            .filter(|tab| !places.contains_key(tab))
            .collect::<Vec<_>>()
        {
            self.window.settling.forget(&Fading::CardHead(gone));
        }
        wanted.retain(|tab| places.contains_key(tab));
        if let Some(index) = hovered
            && let Some(tab) = self.window.tabs.get(index).map(|tab| tab.id)
            && !wanted.contains(&tab)
        {
            wanted.push(tab);
        }
        wanted
            .into_iter()
            .map(|tab| {
                let ink = self.window.settling.settle(
                    &Fading::CardHead(tab),
                    0.0,
                    settling::Toward::eased(
                        f32::from(u8::from(hovered == Some(places[&tab]))),
                        bt_render::HOVER_CHROME_FADE,
                    ),
                    now,
                    motion,
                );
                (places[&tab], ink)
            })
            .collect()
    }

    /// While a flash is running, one frame at the animation's own rate; nothing at
    /// all otherwise.
    ///
    /// **Under reduced motion, one wake-up for the whole band and not sixty.**
    /// The band holds one flat alpha for its 950ms ([`cmdrail::flash_alpha`]), so
    /// the only frame it owes is the one that takes it away — asking for the
    /// animation's rate would be paying for a picture that cannot change. This is
    /// the same trade `Passages` makes by holding nothing at all: `Reduced` is a
    /// genuinely idle window rather than a fast one.
    fn command_flash_deadline(&self, now: Instant) -> Option<Instant> {
        let flash = self.window.command_flash.as_ref().filter(|flash| {
            cmdrail::flash_is_running(now.saturating_duration_since(flash.started))
        })?;
        match self.app.motion {
            Motion::Reduced => {
                Some(self.clamp_animation_deadline(flash.started + cmdrail::JUMP_FLASH))
            }
            Motion::Full => self.next_animation_deadline(),
        }
    }

    /// When this window next has Cards-hint work: the nudge's frames while it is
    /// moving, and the end of the four seconds. Nothing at all for a window
    /// whose reader has already been told (§7.21).
    fn card_hint_deadline(&self, now: Instant) -> Option<Instant> {
        let owner = self.window.card_hint.deadline(
            now,
            self.app.motion,
            self.window.frame_clock.interval(),
        )?;
        if self.window.card_hint.nudge_moving(now, self.app.motion) {
            self.next_animation_deadline().map(|frame| frame.min(owner))
        } else {
            Some(owner)
        }
    }

    /// The thumbs' own clock, run once per pass.
    ///
    /// The same shape as [`Self::advance_toasts`] and for the same reason: a bar
    /// left alone runs out its rest and fades with the pointer and the keyboard
    /// both still, and nothing else in this window would wake the loop to take
    /// it off the glass. The latch is what makes the *last* frame happen — the
    /// pass on which the fade has finished is the pass that must draw the pane
    /// without it, and by then there is no fade left to ask about.
    fn advance_terminal_thumbs(&mut self, now: Instant) -> Result<()> {
        let owed = self.terminal_thumb_work(now).deadline.is_some();
        if !owed && !self.terminal_thumb_owed_frame {
            return Ok(());
        }
        // On the window's own display frame — see [`Self::animation_frame_is_due`].
        if !self.animation_frame_is_due() {
            return Ok(());
        }
        self.terminal_thumb_owed_frame = owed;
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// When any terminal thumb next owes a frame, **and whether any of them is
    /// actually fading** (review round 3, 2026-09-18).
    ///
    /// Only the panes that are actually resting: one standing on a reason —
    /// held, hovered, or parked in history — is not going anywhere, and a pane
    /// with no scrollback has nothing on the glass to take off it.
    ///
    /// Two answers from the one walk, for [`AnimationWork`]'s stated reason and
    /// with this host as the example: the deadline is `Some` for the whole of
    /// [`termscroll::THUMB_REST`] as well, because the loop has to be woken when
    /// the nine hundred milliseconds run out — and for those nine hundred
    /// milliseconds the bar is a still picture at full strength. Reading the
    /// deadline as the liveness kept the overlay lane open for every pane that
    /// had been scrolled in the last second.
    fn terminal_thumb_work(&self, now: Instant) -> AnimationWork {
        let motion = self.app.motion;
        let rests = self
            .sessions
            .iter()
            .filter(|(seat, leaf)| {
                leaf.view.projection.scroll_extent_subpixels() > 0
                    && !leaf.session.session.terminal_modes().alternate_screen
                    && !leaf.view.projection.is_scrolled()
                    && self.terminal_thumb_hover != Some(**seat)
                    && self
                        .held_terminal_thumb_drag()
                        .copied()
                        .is_none_or(|drag| drag.seat != **seat)
            })
            .map(|(_, leaf)| leaf.view.thumb_awake)
            .chain(
                // The foot's marks, on the same clock and through the same
                // latch. Their standing reasons are their own — a hover is not
                // one of them — so the filter reads the horizontal axis rather
                // than the vertical one, and a pane whose widest line fits has
                // nothing down there to take off the glass.
                self.sessions
                    .iter()
                    .filter(|(seat, leaf)| {
                        leaf.view.projection.horizontal().max_x_origin().0 > 0
                            && !leaf.session.session.terminal_modes().alternate_screen
                            && !leaf.view.projection.horizontal().is_scrolled()
                            && self
                                .held_terminal_column_drag()
                                .copied()
                                .is_none_or(|drag| drag.seat != **seat)
                    })
                    .map(|(_, leaf)| leaf.view.column_awake),
            );
        let mut work = AnimationWork::default();
        for rest in rests {
            let due = if termscroll::fade_is_moving(rest, now, motion) {
                self.next_animation_deadline()
            } else {
                termscroll::fade_wait_deadline(rest, now)
            };
            if let Some(due) = due {
                work.deadline = Some(work.deadline.map_or(due, |soonest| soonest.min(due)));
            }
            work.moving |= termscroll::fade_is_moving(rest, now, motion);
        }
        // A thumb fades after the scroll that woke it: a gesture's tween.
        work.travelling = work.moving;
        work
    }

    /// [`Self::reveal_in_explorer`] for a path a worker has already answered for — the door a
    /// reference printed in the terminal takes (closure review of audit 3 C-2).
    ///
    /// The same verb and the same bridge; what differs is that the two facts the bridge needs
    /// arrive from the pane's ledger instead of being fetched from a disk on the thread that
    /// paints. A files column's foot keeps the asking door, because a row of the tree is a path
    /// this window enumerated and has no ledger for.
    ///
    /// **It hands over the whole verdict and the bridge refuses on `exists`** (closure re-review
    /// B-1'): the first shape of this door took a lone `bool` and the caller that should have
    /// checked the other half did not.
    /// **What a pane's ledger has to say to a hand-off** — the three facts the doors out of this
    /// window ask, carried instead of fetched (audit 3 C-2; owner ruling 2026-09-21).
    ///
    /// A name with no verdict answers `exists: false`, which every door refuses: the routing table
    /// has already declined to produce an arm for one, and this is the same answer said again
    /// where the shell is actually reached, so no future caller can hand over a name nobody has
    /// seen.
    pub(crate) fn verified_target(&self, seat: SeatId, path: &Path) -> bt_platform::VerifiedTarget {
        verified_target_of(self.seat_path_verdict(seat, path).as_ref())
    }

    /// [`Self::open_local_path`] for a path a worker has already answered for — the door a
    /// reference printed in the terminal takes under the hand-over modifier.
    ///
    /// The refusal is still the door's and still spoken: "the files column does not run programs"
    /// is a rule the reader is entitled to be told once, whichever surface asked.
    fn open_local_path_verified(
        &mut self,
        path: &Path,
        target: bt_platform::VerifiedTarget,
    ) -> Result<()> {
        self.hand_off(handoff_lane::Ask::new(
            bt_platform::Handoff::OpenVerified(path.to_path_buf(), target),
            handoff_lane::Refusal {
                stderr: Some((
                    "recoverable reference open failure",
                    "open a printed reference with its default handler",
                )),
                program_notice: true,
            },
        ))
    }

    /// **Hand a URI of any scheme to whatever this machine has registered for it** — `mailto:`,
    /// `vscode:`, `ssh:` (ticket 14, owner ruling 2026-09-21).
    ///
    /// Not through [`Self::hand_url_to_the_browser`]: that door is `webnav::address_bar`, which
    /// admits the web and nothing else, and the ruling is that `Ctrl` on any scheme is the
    /// reader's consent. What the machine has no handler for, it refuses, and the lane brings the
    /// refusal back to the surface that asked, which says it as `refused` names.
    pub(crate) fn hand_uri_to_the_system(
        &mut self,
        uri: &str,
        refused: handoff_lane::OnRefused,
    ) -> Result<()> {
        self.hand_off(
            handoff_lane::Ask::new(
                bt_platform::Handoff::Address(uri.to_owned()),
                handoff_lane::Refusal {
                    stderr: Some((
                        "recoverable link hand-off failure",
                        "hand a link to its registered handler",
                    )),
                    program_notice: false,
                },
            )
            .if_refused(refused),
        )
    }

    pub(crate) fn reveal_verified(
        &mut self,
        path: &Path,
        target: bt_platform::VerifiedTarget,
    ) -> Result<()> {
        self.hand_off(handoff_lane::Ask::new(
            bt_platform::Handoff::RevealVerified(path.to_path_buf(), target),
            handoff_lane::Refusal {
                stderr: Some((
                    "recoverable reveal failure",
                    "show a verified path in the file manager",
                )),
                program_notice: false,
            },
        ))
    }

    fn finish_synchronized_update_if_due(&mut self, now: Instant) -> Result<()> {
        let mut active_finished = false;
        let mut active_finished_off_focus = false;
        let mut chrome_changed = false;
        let active = self.window.active_tab;
        // Which tabs had a pane's screen move here, on the drain's own terms and
        // for [`Self::panes_spoke`]'s reason: this release is the one way a pane's
        // picture changes without a byte arriving.
        let mut spoke: Vec<usize> = Vec::new();
        let collect_speakers = self.collecting_card_speakers();
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            let focused = tab.focused_leaf;
            // Per leaf: a synchronized update is a property of one screen, and
            // two shells in one tab time out independently.
            for (seat, leaf) in tab.leaves_mut() {
                // The product's "if due, finish": `bt_compose::advance` is the same step for a
                // single-call host (the web road); this loop keeps its own to read the name evidence
                // between the two, for a due leaf only.
                if !bt_compose::deadlines(&leaf.session).synchronized_update_due(now) {
                    continue;
                }
                let name_before = leaf.name_evidence();
                let finished = leaf
                    .session
                    .finish_synchronized_update(now)
                    .context("finish timed-out DEC 2026 synchronized update")?;
                chrome_changed |= leaf.name_evidence() != name_before;
                if collect_speakers && finished && spoke.last() != Some(&index) {
                    spoke.push(index);
                }
                if index == active && finished {
                    active_finished = true;
                    // **A screen that changed with no byte arriving.** The block
                    // this closes was fed to the parser turns ago and withheld;
                    // releasing it moves cells without anything passing through
                    // the drain, so the drain's own account of who has spoken
                    // never hears it. For a sibling pane that is the only notice
                    // there is.
                    active_finished_off_focus |= *seat != focused;
                }
            }
        }
        self.window.unpainted_pane_output |= active_finished_off_focus;
        self.panes_spoke(&spoke, now);
        if chrome_changed {
            self.want_title();
            self.refresh_chrome();
        }
        if active_finished {
            self.publish_pty_drain_frame(now, false)?;
        }
        Ok(())
    }

    /// **What is mid-flight in this window**, recomputed from scratch and told
    /// to the frame clock once a turn (review 2026-09-18 round 2, P1).
    ///
    /// Every entry is a journey's own "am I still moving" — the predicate its
    /// deadline is built from — and never the deadline itself, because several
    /// of those also carry a clock that is not a tween: a tip's 380 ms before it
    /// is shown, a notice's life running out, a glance card's grace. Waiting is
    /// not moving, and a window that treated it as moving would rebuild its
    /// interface on every keystroke for the length of every wait.
    ///
    /// **And every entry is arithmetic on the journey's own epoch** (review
    /// round 3, 2026-09-18; `docs/DESIGN.md` §7.1.5p ⑬). Three of these used to
    /// read a flag instead, and a flag is cleared by code — code that stands
    /// behind the frame gate and that a neighbouring pane printing every five
    /// milliseconds refuses for as long as it keeps printing. The toast's
    /// `leaving` was the one round 3 measured at nine hundred and eighty-three
    /// redundant overlay builds per thousand publishes; the tip's, the hint
    /// card's and the glance card's *drawn* receipts were the same shape one
    /// turn deep, each carrying once past its own landing. The receipts have not
    /// gone anywhere — they are what schedules the landing frame, and they are
    /// read by the deadlines and paid by the advances, which is where a debt
    /// belongs. They are simply not liveness: a journey stops being live at the
    /// instant its clock says so, whether or not anything has run.
    ///
    /// The cost is one pass of cheap predicates per turn, most of them an
    /// `Option` read over an empty host. Two of them — the strip's and the
    /// thumbs' — come from a walk the fold is about to make anyway, and are
    /// handed in rather than walked twice.
    pub(crate) fn running_journeys(&self, now: Instant, strip: bool, thumbs: bool) -> pace::Lanes {
        let motion = self.app.motion;
        let scale = self.window.renderer.scale_factor() as f32;
        pace::Lanes {
            // The strip's ring, the `˅`, a pane in flight, a card breathing, the
            // dock's fade — and the card column's nudge, which lives here and
            // not in the overlay: the bubble is an overlay layer but what the
            // nudge moves is `focus_card_nudge_rows`, a chrome card. The chrome
            // lane covers both, because `refresh_chrome` ends in
            // `refresh_overlay`; the overlay lane would have moved the caption
            // and left the picture (review round 3, 2026-09-18).
            chrome: strip || self.window.card_hint.nudge_moving(now, motion),
            overlay: thumbs
                || self.window.tooltip.is_fading(now, motion)
                || self.window.key_hint.is_fading(now, motion)
                || self.window.toasts.is_animating(now, motion)
                || self.command_flash_is_running(now)
                || self.command_rails_are_moving(now)
                || self.file_peek_is_fading(now)
                || self.window.float.is_animating(now, motion, scale)
                || self.math_tools_owe_frames(now),
        }
    }

    /// Whether the jump's band is still on screen and still worth a frame — the
    /// first half of [`Self::command_flash_deadline`], which is the half that
    /// says whether anything is moving.
    fn command_flash_is_running(&self, now: Instant) -> bool {
        self.app.motion == Motion::Full
            && self.window.command_flash.as_ref().is_some_and(|flash| {
                cmdrail::flash_is_running(now.saturating_duration_since(flash.started))
            })
    }

    /// **The play button, rasterised, for the lane that draws over a picture**
    /// (user ruling 2026-08-27; `docs/DESIGN.md` §7.23 ⑩).
    ///
    /// Empty for every surface that is not a docked seat showing a playable
    /// video with nothing already on it — the same set
    /// [`Self::seats_wearing_a_play_button`] answers, asked of one surface, so
    /// the control that is drawn and the press that is accepted cannot come
    /// apart.
    ///
    /// The two sprites are cut by [`seats::preview_play_button_sprites`], which
    /// is the one place the circle's geometry is written and the very function
    /// [`seats::hit_preview_play`] measures the press against.
    pub(crate) fn play_button_rasters(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) -> Vec<bt_render::ChromeIcon> {
        let PreviewSurface::Seat(leaf) = surface else {
            return Vec::new();
        };
        if leaf.tab != self.id || !self.seats_wearing_a_play_button().contains(&leaf.seat) {
            return Vec::new();
        }
        let hovered =
            self.window.seat_pointer.hover == Some(seats::ChromeTarget::PreviewPlay(leaf.seat));
        let sprites =
            seats::preview_play_button_sprites(body, hovered, scale, &bt_render::chrome_palette());
        self.window.chrome_marks.resolve(&sprites)
    }
}

/// How much of a shell the user has seen, one present on.
///
/// **Painting is seeing.** The ledger is squared against [`super::terminal::output_revision`] at
/// the exact moment this leaf's cells go to the glass, and at no other moment —
/// which makes "seen" a fact about pixels rather than a fact about which tab
/// held the keyboard.
///
/// The rule this replaces was "the active tab's ledger keeps up", advanced once
/// per event-loop turn. It was wrong in both directions at once. Forward: the
/// pass ran *after* the switch had already changed the active tab, so anything
/// published between the previous turn and the switch was stranded behind a tab
/// that had just gone quiet, and the tab the user had been reading lit up behind
/// them for output that never existed. Backward: a pane that is on screen but
/// not holding the keyboard never publishes at all, so a tab could go on saying
/// nothing while a shell inside it talked.
///
/// Both disappear here. A leaf that was painted owes nothing, whichever pane of
/// whichever tab it is; a leaf that was not painted keeps its backlog, including
/// output that arrived in the same turn as the switch away and never reached the
/// glass. That last case is the one worth stating: it is *correctly* unread.
pub(crate) fn seen_revision(previous_seen: u64, output: u64, leaf_was_painted: bool) -> u64 {
    if leaf_was_painted {
        output
    } else {
        previous_seen
    }
}

/// This leaf's cells have just gone to the glass.
///
/// One function so that the two halves of the present — the pane holding the
/// keyboard and every pane beside it — cannot answer the question differently.
/// They are written separately in [`Runtime::redraw`] only because the borrow
/// checker sees one `&mut` tab and a list taken by value; nothing about the rule
/// distinguishes them, and a rule that lived in two places is how a sibling pane
/// ends up owing a debt it paid in plain sight.
///
/// Unlike [`crate::mark_leaf_seen`] this retires no latch. A bell that rings while you
/// are looking is answered by [`crate::attention_is_consumed`], which asks about the
/// *window's* focus as well — being painted into a window nobody has in front of
/// them is not hearing it.
pub(crate) fn mark_leaf_painted(leaf: &mut ShellSession) {
    leaf.mark_seen(seen_revision(
        leaf.last_seen_revision,
        leaf.output_revision,
        true,
    ));
}

/// **The computation lane's request door for a pane's decoration work** (C1): the task goes to
/// the share that runs its kind — a formula or a decode to `bt-math-worker`, a resample to
/// `bt-image-scale-worker`, a printed path to `bt-path-verify-worker` — and is admitted there, or
/// refused at that share's door and handed back as the task it was.
pub(crate) fn dispatch_decoration_task(
    leaf: ShellAddress,
    task: SessionDecorationTask,
    lane: &computation_lane::ComputationLane,
) -> std::result::Result<(), (lane::Refused, SessionDecorationTask)> {
    match task {
        SessionDecorationTask::Math(task) => lane
            .decorate(MathWorkerRequest::Math {
                leaf,
                task,
                foreground_rgb: foreground_rgb(),
            })
            .map(|_| ())
            .map_err(|(refused, request)| (refused, session_task_of(request))),
        SessionDecorationTask::InlineImage(task) => lane
            .decorate(MathWorkerRequest::InlineImage { leaf, task })
            .map(|_| ())
            .map_err(|(refused, request)| (refused, session_task_of(request))),
        SessionDecorationTask::ScaleInlineImage(task) => lane
            .scale(ScaleWorkerRequest::InlineImage { leaf, task })
            .map(|_| ())
            .map_err(|(refused, request)| match request {
                ScaleWorkerRequest::InlineImage { task, .. } => {
                    (refused, SessionDecorationTask::ScaleInlineImage(task))
                }
                _ => unreachable!("the door hands back the request it was given"),
            }),
        // **Its own share** — see [`PathWorkerRequest`]. A stat that blocks for the redirector's
        // timeout must not stand in front of a formula the reader is waiting on.
        SessionDecorationTask::VerifyPath(path) => lane
            .verify(PathWorkerRequest { leaf, path })
            .map(|_| ())
            .map_err(|(refused, request)| {
                (refused, SessionDecorationTask::VerifyPath(request.path))
            }),
    }
}

/// The session's task a pane's formula or decode request was made of, handed back.
fn session_task_of(request: MathWorkerRequest) -> SessionDecorationTask {
    match request {
        MathWorkerRequest::Math { task, .. } => SessionDecorationTask::Math(task),
        MathWorkerRequest::InlineImage { task, .. } => SessionDecorationTask::InlineImage(task),
        _ => unreachable!("the door hands back the request it was given"),
    }
}

/// **Which of the computation lane's shares a pane's task goes to.**
fn share_of(task: &SessionDecorationTask) -> computation_lane::Share {
    match task {
        SessionDecorationTask::Math(_) | SessionDecorationTask::InlineImage(_) => {
            computation_lane::Share::Decoration
        }
        SessionDecorationTask::ScaleInlineImage(_) => computation_lane::Share::Scale,
        SessionDecorationTask::VerifyPath(_) => computation_lane::Share::Path,
    }
}

/// See [`i18n::Text::MathWorkerStopped`] — one of three sentences deliberately
/// cut to one pattern, because a worker dying is a feature going away and all
/// three have to say which half still works.
pub(crate) fn math_worker_stopped_notice() -> &'static str {
    i18n::Text::MathWorkerStopped.text()
}

/// **Hand a pane's queued decoration work to the computation lane.**
///
/// - A share that refuses because it is full stops only its own work for this hand-out: the
///   rest of the pane's tasks for that share are kept in the pane, and the other two shares are
///   still handed theirs — the three bounds exist so that the shares do not block each other.
///   What a full share refused waits at the head of the pane's queues for the next hand-out (the
///   next output, frame or answer).
/// - A share whose thread the door could not start refuses the task, which goes back to the
///   pane, and the pane hands nothing more out until its next new input
///   (`DualPlaneSession::pause_hand_out_until_new_input`): the start is tried again by new input,
///   once, never by every hand-out that finds the queues as they were.
/// - A share whose worker has ended turns the lane off: a one-way feature downgrade, never a
///   terminal/runtime error.
pub(crate) fn dispatch_pending_math_tasks(
    leaf: ShellAddress,
    session: &mut DualPlaneSession,
    lane: &computation_lane::ComputationLane,
    running: &mut bool,
    notice_pending: &mut bool,
) -> bool {
    if !*running {
        return false;
    }
    let mut full = BTreeSet::new();
    let mut kept = Vec::new();
    let mut disabled = false;
    while let Some(task) = session.take_decoration_worker_task() {
        let share = share_of(&task);
        if full.contains(&share) {
            kept.push(task);
            continue;
        }
        match dispatch_decoration_task(leaf, task, lane) {
            Ok(()) => {}
            Err((lane::Refused::Full { .. }, task)) => {
                full.insert(share);
                kept.push(task);
            }
            Err((lane::Refused::NotStarted(_), task)) => {
                kept.push(task);
                session.pause_hand_out_until_new_input();
                break;
            }
            Err((lane::Refused::Faulted(_), task)) => {
                kept.push(task);
                disabled = disable_math_worker_state(running, notice_pending);
                break;
            }
            Err((lane::Refused::TwinEnded, _)) => {
                unreachable!("a request joins only one still waiting in line")
            }
        }
    }
    // Back at the head of their queues, in the order they were taken.
    for task in kept.into_iter().rev() {
        session.give_back_decoration_worker_task(task);
    }
    disabled
}

/// **What a pane's ledger has to say to a hand-off** (audit 3 C-2; owner rulings 2026-09-21).
///
/// A free function so the road from a real file to a real door argument can be driven in a test
/// without a window: `verify_path` → this → the door's own argument builder. That road is what the
/// closure review r6 found broken — every fixture in the suite carried no resolved name, so the
/// doors were only ever exercised on the fallback.
///
/// A name with no verdict answers `exists: false`, which every door refuses.
pub(crate) fn verified_target_of(
    verdict: Option<&bt_term::PathVerdict>,
) -> bt_platform::VerifiedTarget {
    let Some(verdict) = verdict else {
        return bt_platform::VerifiedTarget::absent();
    };
    bt_platform::VerifiedTarget {
        exists: verdict.exists,
        is_directory: verdict.directory,
        executable: verdict.executable,
        resolved: verdict.door_ready.clone(),
    }
}

/// **Whether this window is out of the reader's reach altogether** (`attention` plan §5.2, C1).
///
/// Two facts and one `||`, and the composition is here rather than in `bt-platform` because it is
/// a *policy* — "hidden" is the name this product gives to the union of them — while the two calls
/// underneath are readings. A third condition, "completely covered by another window", used to be
/// deliberately absent from every fact this module held; it is now [`window_is_exposed`]'s, and it
/// is a **separate** bit rather than a third arm of this `||`. Minimised and cloaked are the window
/// telling us where it is; covered is somebody else's window standing in front of one that is
/// perfectly on screen, and [`notify::desktop_reach`] answers the two differently — see
/// [`bt_workbench::attention::Reach::Toast`].
///
/// A window whose handle cannot be got is reported reachable, on
/// [`bt_platform::cloaked_from_attribute`]'s reasoning: under-stating flashes a taskbar button
/// nobody sees, and over-stating puts a toast in front of somebody who is looking straight at the
/// pane it is about.
///
/// **An owner-thread door** (`doors::PlaceHidden`, row 13's residue), minted only in
/// [`sample_window_place`].
pub(crate) fn window_is_hidden(
    token: WaitToken<'_, doors::PlaceHidden>,
    window: &impl WindowHost,
) -> bool {
    let _ = token;
    window.is_iconic() || window.is_cloaked()
}

/// **Whether the window manager puts this window under any of its own sample points** (user ruling
/// 2026-09-01).
///
/// The window-handle half of [`bt_platform::window_is_exposed`], and it is here for
/// [`window_is_hidden`]'s reason: the composition is one line, and where it lives is where the
/// failure policy is stated. **A window whose handle cannot be got is reported exposed**, which is
/// the same direction that function takes and the same direction
/// [`bt_platform::cloaked_from_attribute`] argues for — of the two wrong answers, one leaves the
/// reader with the marks inside a window they are looking at, and the other puts a toast on a
/// desktop they can see.
///
/// **An owner-thread door** (`doors::PlaceExposure`, row 13's residue), minted only in
/// [`sample_window_place`].
pub(crate) fn window_is_exposed(
    token: WaitToken<'_, doors::PlaceExposure>,
    window: &impl WindowHost,
) -> bool {
    let _ = token;
    window.is_exposed()
}

pub(crate) fn take_math_worker_notice(notice_pending: &mut bool) -> Option<&'static str> {
    if std::mem::take(notice_pending) {
        Some(math_worker_stopped_notice())
    } else {
        None
    }
}

/// Collect every pane of the tab on screen, not merely the one holding the keyboard.
///
/// A leaf queues its own decoration work as its own bytes are drained — that half was always per
/// pane. What was not was the collection: dispatching through the tab's `Deref` asked the focused
/// leaf for its queue and left every other pane's work sitting where it was written. Nothing came
/// back for those panes, so a file reference in them was never verified, and an unverified
/// reference wears no resting underline. Hovering the pane opened the file by another road (the
/// peek), which is why the affordance appeared to be a thing hovering granted rather than a thing
/// every pane is owed.
pub(crate) fn dispatch_tab_decoration_tasks(
    window: WindowId,
    tab: &mut TabState,
    senders: &DecorationSenders,
    now: Instant,
    running: &mut bool,
    notice_pending: &mut bool,
) -> bool {
    let tab_id = tab.id;
    let mut disabled = false;
    for (seat, leaf) in tab.leaves_mut() {
        let address = ShellAddress {
            window,
            leaf: LeafId {
                tab: tab_id,
                seat: *seat,
            },
        };
        disabled |= dispatch_pending_math_tasks(
            address,
            &mut leaf.session,
            &senders.computation,
            running,
            notice_pending,
        );
        foreground_program::request(leaf, address, &senders.foreground, now);
    }
    disabled
}

/// The earliest appointment together with the owner that supplied it.
///
/// Keeping the names beside the fold, rather than reconstructing them after the
/// fact, makes the idle probe report evidence: ties retain the first entry, just
/// as [`Iterator::min`] did before the entries were named.
pub(crate) fn earliest_named_deadline<T: Copy, const N: usize>(
    names: [T; N],
    deadlines: [Option<Instant>; N],
) -> Option<(T, Instant)> {
    names
        .into_iter()
        .zip(deadlines)
        .filter_map(|(name, deadline)| deadline.map(|at| (name, at)))
        .min_by_key(|(_, at)| *at)
}

/// One frame's image references, with the row stride they were scanned at.
///
/// The stride travels with the list because a `GridHit` is a row and a column while a reference is
/// a set of cell indices; keeping the frame's own column count beside the scan is what makes the
/// two the same coordinate rather than two coordinates that usually agree.
///
/// One hovered cell resolves to at most one reference: the first that covers it. The scan pushes
/// printed text before link targets, so where a cell carries both — a link whose label spells the
/// path — the answer is what the pointer is actually standing on. Both name the same file anyway.
/// The pointer's own marks, put on the frame of the pane the pointer is standing in.
///
/// One window, one pointer, and therefore at most one pane wearing these: the hovered link's
/// underline and the status line that spells its target, and the solid underline under a reference
/// that has been verified as a picture. Which pane wears them is a question about coordinates and
/// never about which pane holds the keyboard — this function is called for exactly the pane
/// `WindowRuntime::hover_pane` names, whether or not that is the focused leaf.
fn apply_hover_marks(
    frame: &mut ViewportFrame,
    hyperlink_hover: &HyperlinkHover,
    hovered_reference: Option<&bt_term::FrameImageReference>,
) {
    if let Some(hyperlink) = hyperlink_hover.underline_target()
        && frame.underline_hyperlink(hyperlink)
        && hyperlink_hover.active.is_some()
    {
        frame.status_text = hyperlink_hover.status_text(frame.columns.get() as usize);
    }
    if let Some(reference) = hovered_reference {
        frame.underline_cells(&reference.cells, true);
    }
}

/// **Ask the desktop where this window is, on the turn that is about to decide what the reader is
/// owed** (`attention` plan §5.2; user ruling 2026-08-28).
///
/// The one place the four facts of [`notify::WindowPlace`] are *read* rather than remembered, and
/// it exists so that "sampled together, on one turn" is a call and not a convention three passes
/// each keep on their own. **Its one caller is [`Runtime::observe_window_place`]** (ticket 48):
/// the head of every turn, a window's birth, an attention delivery that arrives between turns, and
/// the re-placing of a flash the taskbar lane's answer contradicts (ticket 62). The taskbar bit is
/// the one fact here that is *not* asked: it is the taskbar lane's latest answer (ticket 62).
/// Focus is that caller's, and it asks the window itself (`Window::has_focus`) for the reason
/// `drain_pty` gives.
///
/// Between three and seven syscalls a turn — `IsIconic`, `DwmGetWindowAttribute`, and the
/// `GetWindowRect` plus one to three `WindowFromPoint`/`GetAncestor` pairs the exposure probe
/// costs; the taskbar's `SHAppBarMessage` is asked on `taskbar_lane` and only read here
/// (ticket 62) — against a pass that walks every leaf of every tab. That is why
/// the answers land in `WindowRuntime`'s fields: the drain, the strip tick and the doors that fire
/// *inside* a turn read them back from there rather than asking again about the same instant. **This is the pass
/// that decides a delivery**, which is what the probe is priced against; nothing on the drawing path
/// asks any of these.
///
/// **Each probe is an owner-thread door** (`doors::PlaceHidden`, `doors::PlaceExposure`, row 13's
/// residue): admitted where it is asked, and a refused probe keeps the answer of `previous`, the
/// place this window last observed.
pub(crate) fn sample_window_place(
    window: &impl WindowHost,
    focused: bool,
    previous: notify::WindowPlace,
) -> (notify::WindowPlace, bt_platform::TaskbarReading) {
    // **Each probe under its own station** (ticket 48): two of them go to other processes —
    // the hit tests to whatever window is under each point, `SHAppBarMessage` to the shell's
    // taskbar — and a stall line should say which one waited.
    // The admission's meter enters the probe's station and puts the turn's back, the
    // `enter`/`at` pair this replaced; the fused call keeps the line
    // `present_diagnostics_tests::existing_hidden_callers_keep_the_same_fused_value` reads.
    let hidden = bt_platform::admission::admitted::<doors::PlaceHidden, _>(|token| {
        window_is_hidden(token, window)
    })
    .unwrap_or(previous.hidden);
    // **The taskbar is read, not asked** (ticket 62). The shell is asked on its own lane; this
    // reads the latest answer without waiting, and — while the window is on a screen — asks the
    // lane for a fresh one when the last request is `REFRESH_INTERVAL` old. The reading travels
    // out beside the place, so the window can say which answer its deliveries were decided on.
    let taskbar = hang_watch::during(hang_watch::Station::PlaceTaskbar, || {
        taskbar_lane::observe(Instant::now(), !hidden)
    });
    let place = notify::WindowPlace {
        focused,
        hidden,
        // **Not asked of a window that is on no screen** (user ruling 2026-09-01). While a window
        // is iconic, `GetWindowRect` describes the icon Windows parked off-screen rather than the
        // window — `bt_platform::is_window_minimized` says so in its own doc — so a hit test
        // against that rectangle would be an answer about somewhere the window is not. A cloaked
        // window is on no screen by definition. Either way the honest answer is that nothing of it
        // is showing, and `desktop_reach` tests the hidden bit outermost anyway.
        exposed: !hidden
            && bt_platform::admission::admitted::<doors::PlaceExposure, _>(|token| {
                window_is_exposed(token, window)
            })
            .unwrap_or(previous.exposed),
        taskbar_is_auto_hidden: taskbar.auto_hidden,
    };
    (place, taskbar)
}

/// The next instant any pane in this window owes the loop a wake-up on the ledger's account.
///
/// `None` for a window with nothing standing, which is every window almost always — so an idle
/// Folio asks for no frames at all because of this.
pub(crate) fn attention_ledger_deadline(tabs: &[TabState]) -> Option<Instant> {
    tabs.iter()
        .flat_map(|tab| tab.leaves())
        .filter_map(|(_, leaf): (&SeatId, &ShellSession)| leaf.attention.clock.deadline())
        .min()
}

/// [`Runtime::chrome_over`] with the stack as a value — the whole of the rule,
/// where a test can read it.
///
/// A free function for [`super::web::a_page_is_off_the_glass`]' reason: what it answers is a
/// question about five numbers and a list, and a method would need a window to
/// ask it.
pub(crate) fn chrome_over(
    layers: &[Option<[f32; 4]>],
    above: Option<usize>,
    body: Option<[f32; 4]>,
) -> Vec<[f32; 4]> {
    let Some(body) = body else {
        return Vec::new();
    };
    let first = above.map_or(0, |level| level.saturating_add(1));
    layers
        .iter()
        .skip(first)
        .flatten()
        .filter(|rect| {
            rect[0] < body[2] && rect[2] > body[0] && rect[1] < body[3] && rect[3] > body[1]
        })
        .copied()
        .collect()
}

pub(crate) fn startup_poll_delay(first_text_presented: bool) -> Option<std::time::Duration> {
    (!first_text_presented).then_some(STARTUP_PTY_POLL_INTERVAL)
}

/// Move a periodic owner's absolute appointment past the turn that just spent
/// it. Merely reading the appointment never calls this function.
pub(crate) fn advance_periodic_deadline(next: &mut Instant, now: Instant, interval: Duration) {
    if *next > now {
        return;
    }
    let missed = now.saturating_duration_since(*next).as_nanos() / interval.as_nanos() + 1;
    let steps = u32::try_from(missed).unwrap_or(u32::MAX);
    *next += interval * steps;
}

pub(crate) fn trace_unchanged_present(
    enabled: bool,
    gate: &mut present_gate::PresentGate,
    source: FrameSource,
    frame: Option<&ViewportFrame>,
    slot_overwrites: u64,
) {
    gate.unchanged = gate.unchanged.saturating_add(1);
    if !enabled {
        return;
    }
    let started = Instant::now();
    let content_fnv = frame.map_or(0, |frame| frame_content_digest(frame).content_fnv);
    let alternate = frame.is_some_and(frame_is_alternate_screen);
    crate::diagnostics::stderr_line(format!(
        "BT_PERF_TRACE skip=unchanged source={:?} content_fnv={:016x} alt={} digest_us={} present_unchanged={} slot_overwrites={}",
        source,
        content_fnv,
        u8::from(alternate),
        started.elapsed().as_micros(),
        gate.unchanged,
        slot_overwrites,
    ));
}

pub(crate) fn pty_frame_is_unchanged(
    pending: Option<&ViewportFrame>,
    last_presented: Option<&ViewportFrame>,
    next: &ViewportFrame,
) -> bool {
    pending
        .or(last_presented)
        .is_some_and(|previous| presentation_equivalent(previous, next))
}

/// Whether a PTY drain says nothing the glass is not already showing.
///
/// **Two halves, because a window is not one pane.** `focused_frame_unchanged`
/// is [`pty_frame_is_unchanged`]'s answer, and it is an answer about exactly one
/// pane: the frame [`Runtime::publish_frame_inner`] composed is the focused
/// leaf's, and the frame it was compared against is the focused leaf's.
/// `unpainted_pane_output` is the rest of the window — see
/// [`crate::WindowRuntime::unpainted_pane_output`] for what it costs to leave it out.
///
/// A drain may be dropped only when both halves are quiet. A sibling pane that
/// has spoken makes the window's content new however still the focused pane is,
/// and the composed frame — identical though it may be — is the vehicle every
/// other pane rides to the screen on: `redraw` is where they are projected, and
/// `redraw` runs for a frame that entered the slot and for nothing else.
///
/// Written as a free function taking the two facts rather than as a condition
/// inside the publish, so that the *previous* policy — the focused frame alone —
/// can be run through the same machinery by a test and shown to lose the pane.
pub(crate) fn pty_drain_says_nothing_new(
    focused_frame_unchanged: bool,
    unpainted_pane_output: bool,
) -> bool {
    focused_frame_unchanged && !unpainted_pane_output
}

/// **Whether a redraw request that found nothing composed still owes the glass
/// a present** (§7.10 ④‴).
///
/// [`Runtime::redraw`] used to answer this with `chrome_present_pending` alone,
/// and for a tab with a shell that is the whole truth: everything that changes
/// such a window either publishes a frame (which is then in the slot) or files
/// this debt on its way to asking for a redraw.
///
/// **It is not the whole truth for a tab with no shell** (§7.1.6h), and that is
/// the second half. Three funnels in this window answer a change that lives in
/// retained renderer state by putting the picture that is *already on the glass*
/// back into the slot and asking for a redraw —
/// [`Runtime::present_chrome_change`], [`Runtime::present_peek_overlay`] and
/// [`Runtime::represent_on_screen_frame`]. Each of them can only do the first
/// half when there is a terminal picture to re-queue, and a tab made of a folder
/// or a file has none, ever: `publish_frame_inner` composes none for it, and
/// `activate_tab` empties `last_presented_frame` on the way in. So the redraw
/// those funnels asked for arrived at a slot that was empty and a flag that was
/// false, and was dropped — which on the machine is a preview pane, alone in a
/// tab, whose wheel scrolls the document in memory and never draws it: **ten
/// notches, ten scroll offsets, zero frames** (measured 2026-08-30).
///
/// The reading is the one `present_retained_picture` already writes down in its
/// own words — "for it this is not the fallback path but the **only** path". A
/// window with no shell in front of it has exactly one kind of frame, so every
/// redraw it is asked for is that kind.
///
/// Costs a tab with a shell nothing: it is the same `chrome_present_pending`
/// question it always was, and `present_retained_picture`'s own first guard
/// turns straight back round for a shell that has not presented yet.
#[must_use]
pub(crate) fn a_bare_redraw_still_owes_a_present(
    chrome_present_pending: bool,
    tab_has_a_shell: bool,
) -> bool {
    chrome_present_pending || !tab_has_a_shell
}

/// Whether a chrome animation's tick can be answered from the picture already
/// on the glass instead of composing a new one.
///
/// **This is the whole of the P0 fix, as one question.** The tab strip's
/// indeterminate ring turns at 60Hz for as long as any session has a command
/// running, and every one of those ticks used to publish a whole-window frame:
/// grid capture, viewport projection, inline-reference scan, IME re-anchor,
/// text re-shaping — all to redraw a fifteen-pixel arc one degree further round.
/// Measured at 6-8ms of main-thread CPU per tick, against a design comment that
/// budgeted 16.5µs (see [`pace::DEFAULT_FRAME_INTERVAL`]). A command emitting *zero
/// bytes* cost 69% of a core.
///
/// The licence to reuse rests on one property of this app: **nothing repaints
/// unless something calls [`Runtime::publish_frame`]**. A fresh idle window
/// publishes no frames at all and sits at 0.0% CPU, which is only true because
/// every content change already announces itself with its own publish. So an
/// animation tick that re-presents what is on the glass is never *less* correct
/// than what already happens on the frames where `refresh_chrome` reports
/// nothing moved, which is to present nothing whatsoever.
///
/// The revisions guard the one case that argument does not cover: a picture
/// that was composed but has not reached the glass yet. Then the newest thing
/// anyone has said is not what is being shown, and the tick composes.
pub(crate) fn chrome_tick_reuses_picture(picture: PictureOnGlass) -> bool {
    // A frame is already composed and queued; this tick's redraw request will
    // present it. Composing a second one on top would throw the first away.
    if picture.frame_pending {
        return true;
    }
    // Nothing to reuse. One window, once.
    if !picture.has_presented_frame {
        return false;
    }
    // A held picture is the picture, by decision. Composing under a hold builds
    // a frame the hold discards a few lines later.
    picture.presentation_hold || picture.presented_revision == picture.content_revision
}

impl TermView {
    /// **A pane's presented frame and the metrics it was drawn at, recorded together** — at the
    /// present, by `Runtime::redraw`'s commit, and nowhere else (the ownership contract's
    /// `commit_presented`, M3, ticket O-3).
    ///
    /// The pair is what every pointer question about the pane reads (ticket 37), so the two are
    /// one write: a frame recorded beside another frame's metrics would put the pointer in a cell
    /// nobody can see. The image references the frame draws are not part of it; they are scanned
    /// when the frame is published, which is a different moment.
    pub(crate) fn commit_presented(
        &mut self,
        frame: ViewportFrame,
        metrics: bt_render::CellMetrics,
    ) {
        self.last_presented_frame = Some(frame);
        self.presented_metrics = metrics;
    }
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "wake_fold_tests.rs"]
mod wake_fold_tests;
