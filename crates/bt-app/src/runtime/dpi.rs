//! `dpi` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    ResizeReanchor, Runtime, card_trace, cell_width_subpixels, dpi_snapshot, earliest_deadline,
    ensure_metrics_match_authoritative_scale, ensure_swapchain_matches_inner,
    files_float_content_height, float, hang_watch, log_dpi_snapshot, presentation_physical_size,
    release_due_leaf_resize, resize_worth_solving, scale_factors_match, seats,
    take_psreadline_resize_reanchor_input, trace_sink, trace_surface_size_clamp, write_pty_input,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_render::{FrameSource, FrameTrigger, GridSize};
use std::time::Instant;
use winit::dpi::PhysicalSize;

impl Runtime<'_> {
    /// Grow (or shrink) every self-sizing float to the rows it is now showing.
    ///
    /// Returns whether anything moved. It re-runs the *placement* and not merely
    /// the height, because a window that opened above its trigger — flipped for
    /// want of room below — grows upward rather than down, and only the placement
    /// knows which of those this one is.
    ///
    /// Per window, because `height: auto` is a property of a box and not of the
    /// host: two windows can be following their content at once, and a hand on
    /// one of them ends it for that one alone.
    pub(crate) fn resize_floats_to_content(&mut self) -> bool {
        // A window with no float has nothing to grow (closure review 2,
        // 2026-09-18): this runs on every turn, and the read below is a walk and
        // a `Vec`.
        if self.window.float.is_empty() {
            return false;
        }
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let viewport = self.float_viewport();
        let git_panel_on = self.git_panel_on();
        // Read every window's answer first, then write them: the read wants the
        // whole host and each write wants one window of it.
        let grown: Vec<(float::FloatId, [f32; 4])> = self
            .window
            .float
            .live_windows()
            .filter(|win| win.self_sizing)
            .filter_map(|win| {
                // `height: auto` is the tree's; a buffer float is sized by its
                // own opening rule and has no row count to follow.
                let files = win.files()?;
                let size = float::float_opening_size(
                    files_float_content_height(
                        &files.files,
                        &files.cache,
                        git_panel_on && files.files.view == seats::FilesView::Git,
                        viewport,
                        scale,
                    ),
                    viewport,
                    scale,
                    float::FloatSizing::files(),
                );
                let grown = match win.anchor {
                    Some(anchor) => float::float_placement(anchor, size, viewport, scale),
                    None => [
                        win.frame[0],
                        win.frame[1],
                        win.frame[0] + size[0],
                        win.frame[1] + size[1],
                    ],
                };
                let frame = float::clamp_pinned(grown, viewport, scale);
                (frame != win.frame).then_some((win.epoch, frame))
            })
            .collect();
        if grown.is_empty() {
            return false;
        }
        for (id, frame) in grown {
            if let Some(win) = self.window.float.live_mut(id) {
                win.frame = frame;
            }
        }
        true
    }

    /// A diagnostic about a shell resizing. A tab with no shell has no
    /// transaction, no trace and nothing to log (§7.1.6h) — and a trace that
    /// printed a line about one would be a diagnostic inventing its subject.
    pub(crate) fn flush_resize_trace(&mut self) {
        if !self.app.trace_resize || self.focused().is_none() {
            return;
        }
        let transaction = self.shell().session.resize_trace_transaction();
        if transaction != self.window.resize_trace_logged_transaction {
            self.window.resize_trace_logged_transaction = transaction;
            self.window.resize_trace_logged_events = 0;
        }
        let trace = self.shell().session.resize_trace();
        let conpty_source = self
            .shell()
            .pty
            .as_ref()
            .map(|pty| pty.conpty_source().to_string())
            .unwrap_or_else(|| "direct-input".to_string());
        for event in &trace[self.window.resize_trace_logged_events.min(trace.len())..] {
            trace_sink::stderr_line(format!(
                "BT_RESIZE_TRACE conpty_source={conpty_source:?} {event:?}"
            ));
        }
        self.window.resize_trace_logged_events = trace.len();
    }

    pub(crate) fn finish_resize_if_quiescent(&mut self, now: Instant) -> Result<()> {
        let mut active_finished = false;
        let active = self.window.active_tab;
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            // Per leaf: each shell runs its own ConPTY resize transaction and
            // each PSReadLine holds its own anchor, so quiescence is reached one
            // shell at a time.
            for (_, leaf) in tab.leaves_mut() {
                if !leaf
                    .session
                    .finish_resize_if_quiescent(now)
                    .context("finish ConPTY resize transaction")?
                {
                    continue;
                }
                // `[Console]::CursorLeft/Top` in the PSReadLine handler makes ConPTY ask the terminal
                // `CSI 6 n`. Pay the coalesced repair only after the final resize request *and* every
                // child byte it caused have been quiet. A new geometry event re-opens the transaction,
                // so a divider storm cannot install an intermediate commit's still-moving cursor.
                let prompt_the_shell_opened = leaf.session.shell_prompt_opened_in_order();
                let integration = leaf.integration;
                if let Some(reanchor_input) = take_psreadline_resize_reanchor_input(
                    ResizeReanchor {
                        pending: &mut leaf.pending_psreadline_resize_reanchor,
                        integration,
                    },
                    prompt_the_shell_opened,
                ) {
                    write_pty_input(
                        leaf.pty.as_ref(),
                        reanchor_input,
                        "request PSReadLine anchor repair after resize quiescence",
                    )?;
                }
                active_finished |= index == active;
            }
        }
        if active_finished {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    /// **Every leaf of every tab, because every leaf now has a queue.**
    ///
    /// The quiet window used to belong to the pane holding the keyboard, so this drained one
    /// leaf and the wake-up it answered with was one leaf's. Now that a sibling's ConPTY
    /// notification is coalesced by the same 200 ms — which is what stopped a four-pane split
    /// from making three unbounded conhost round trips per `Resized` — its release is this
    /// window's to make and its deadline is this window's to wake for, exactly as
    /// `resize_finish_deadline` already reads every leaf a few lines below in `about_to_wait`.
    ///
    /// A tab with no shell has no pending ConPTY resize to release, because nothing ever
    /// scheduled one: the queue this drains is a `LeafSession` field, and there is no leaf
    /// (§7.1.6h). Answering `None` is answering "nothing is owed and nothing has to be woken
    /// for", which is exactly true.
    pub(crate) fn flush_pending_pty_resize(&mut self, now: Instant) -> Result<Option<Instant>> {
        hang_watch::at(hang_watch::Station::PtyResize);
        // **A gesture nobody is holding any more is over** (Codex review 2026-09-17). Read before
        // the held-hand question below, because that question is the one a stale answer ruins: a
        // divider drag whose capture was taken away with no blur and no button-up would otherwise
        // read as a hand still on the geometry for the rest of this window's life, and no pane of
        // it could release or settle another resize.
        self.end_a_divider_drag_that_lost_its_pointer()?;
        let active = self.window.active_tab;
        let focused_seat = self.window.tabs[active].focused_leaf;
        // **Is a hand still on the geometry** — see [`service_pending_pty_resize`]. The two
        // gestures that move a pane's rectangle while a button is held: this window's own frame in
        // the OS's modal move/size loop, and a divider. §7.50 already reads the first of them for
        // the DPI settlement's sake; the child's notification is the other thing a gesture in
        // flight should not be interrupted to say.
        let hand_on_the_geometry =
            self.window.divider_drag.is_some() || self.window.custom_window_frame.in_size_move();
        // Every `ResizePseudoConsole` this window actually issues, one line each, so a report of
        // the shape "a resize sequence wedged the program in this pane" can be answered with the
        // sequence instead of with an argument about it. `BT_RESIZE_TRACE`, beside the surface
        // clamp trace it is read with.
        let trace = self.app.trace_resize;
        let mut wake_deadline: Option<Instant> = None;
        let mut committed_any = false;
        let mut active_tab_wants_a_frame = false;
        let mut focused_reflow: Option<GridSize> = None;
        // **Any leaf at all, because the key is that leaf's own columns.**
        // `sync_math_layout_key` writes every leaf of every tab a key built out
        // of `leaf.grid`, and this release is where a pane behind another tab
        // moves that field (`LeafOnStage::Behind` defers the reflow to here). A
        // key gated on the focused pane alone would leave a hidden pane's
        // typeset bands rastered for the width it had before the gesture.
        let mut reflowed_any = false;
        // Read before the walk takes a `&mut` of the tab list (`BT_CARD_TRACE`).
        let window = u64::from(self.window.window.id());
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            let tab_id = tab.id;
            for (seat, leaf) in tab.sessions.iter_mut() {
                // The deferred local reflow lands inside this, immediately before the child hears
                // the same size, in the same order every other resize path uses (actor first,
                // then ConPTY, then the vendor reconcile). When nothing was deferred the actor
                // half is a no-op: our grid already moved at the `Resized` that scheduled this.
                //
                // The OSC 133 phase is sampled in there too, before reconciliation mutates
                // terminal geometry, and the repair debt it records is *replaced* rather than
                // accumulated; the send happens in `finish_resize_if_quiescent`, after ConPTY
                // output has also been silent, so a closed input region still writes exactly zero
                // private bytes.
                //
                // The grid this pane's own actor is wearing as the release begins is read first:
                // it is the `grid_before` of the `card pane resized` line below, and this is the
                // only place it can be read, because the release is what moves it
                // (`BT_CARD_TRACE`).
                let grid_before = leaf.grid;
                let (commit, leaf_wake) = release_due_leaf_resize(leaf, now, hand_on_the_geometry)?;
                wake_deadline = earliest_deadline([wake_deadline, leaf_wake]);
                let Some(commit) = commit else {
                    continue;
                };
                // **`card pane resized`, the committed leg** (T-CARD-TRACE).
                // The quiet boundary is where a hidden pane's deferred reflow
                // actually happens and where every pane's child is told, so it
                // is a second place a card's transcript can be re-wrapped under
                // it — and the `commit=` word is what tells the two apart in the
                // file.
                let pane = card_trace::Pane {
                    window,
                    tab: tab_id,
                    seat: *seat,
                };
                let before = (
                    u32::from(grid_before.columns.get()),
                    u32::from(grid_before.rows.get()),
                );
                let after = (
                    u32::from(leaf.grid.columns.get()),
                    u32::from(leaf.grid.rows.get()),
                );
                let stage = if index == active { "shown" } else { "behind" };
                card_trace::line(|| {
                    card_trace::PaneResized {
                        pane,
                        stage,
                        commit: "committed",
                        before,
                        after,
                    }
                    .line()
                });
                if trace {
                    trace_sink::stderr_line(format!(
                        "BT_RESIZE_TRACE conpty tab={index} seat={} cols={} rows={}",
                        seat.0,
                        leaf.conpty_grid.columns.get(),
                        leaf.conpty_grid.rows.get()
                    ));
                }
                committed_any = true;
                reflowed_any |= commit.reflowed;
                if index == active {
                    active_tab_wants_a_frame |= commit.worth_a_frame();
                    if commit.reflowed && *seat == focused_seat {
                        focused_reflow = Some(leaf.grid);
                    }
                }
            }
        }
        if reflowed_any {
            self.sync_math_layout_key();
        }
        // The present gate is the focused pane's alone: it admits the grid the
        // frame this window is about to draw really carries, and a pane nobody
        // is looking at draws no frame.
        if focused_reflow.is_some() {
            self.pending_resize_present = focused_reflow;
        }
        if committed_any {
            // The quiet boundary is also where a resize *ends*, so it is the
            // meaningful change §5.1 asks the session write to be debounced behind.
            // Marking it on every intermediate `Resized` would turn one drag of a
            // window corner into a hundred disk writes.
            self.mark_session_dirty(now);
        }
        if active_tab_wants_a_frame {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Resize,
            })?;
        }
        Ok(wake_deadline)
    }

    pub(crate) fn resize(&mut self, physical: PhysicalSize<u32>) -> Result<()> {
        // **A minimized window has no geometry to solve for**, and the size Win32 hands over with
        // `SIZE_MINIMIZED` is the icon's, not the window's — see [`resize_worth_solving`].
        if !resize_worth_solving(self.window_is_iconic(), physical) {
            return Ok(());
        }
        // A hand on the frame is a hand on the frame whichever edge it took hold
        // of, so the same reader runs on the resizes of a drag as on its moves.
        self.remember_summoned_arrangement();
        // **The first thing done with a rectangle that is the window's, and that is the whole of
        // it** (§7.1.6c-4b, re-judged 2026-08-24; the gate above is the only thing in front of it,
        // and it decides whether there is a rectangle here at all). Everything below this line —
        // the solve, every pane's ConPTY, the frame — happens between the
        // window becoming bigger and the swapchain becoming bigger, and for
        // the whole of that gap the strip the window grew by is a region
        // nothing in the composition tree paints, which on a window DWM
        // honours the alpha of is the desktop. The compositor's own commit
        // owes nothing to a frame, so the window's ground is on the glass in
        // the same composition pass that made the window bigger. Costs one
        // comparison when a `WM_SIZE` settles back onto the same numbers.
        self.window
            .compositor
            .set_window_size(physical.width, physical.height)
            .map_err(|error| anyhow!(error))
            .context("put the window's own ground under the strip a resize opens")?;
        // Before anything solves: a rectangle this process did not ask for is the user's, and
        // from here on their minima are advice (user ruling 2026-08-08).
        self.defer_preview_resample(Instant::now());
        // Both halves of a visible flyout belong to the viewport that produced them: the anchor is
        // a physical point on the old surface, and the raster was sized to the old pane. A resize
        // dissolves it exactly as a wheel notch does; the retained thumbnail is re-derived, and
        // resampled if the new pane asks for another box, on the next settled hover. The frame
        // this handler publishes below is the repaint that drops it, so nothing is queued here:
        // the frame on screen belongs to the old grid and the resize gate would refuse it.
        self.window.peek_hover.clear();
        self.window.renderer.set_peek_overlay(None);
        // The Resized payload is already in physical pixels. Synchronize presentation before any
        // DPI reconciliation can publish a frame, then reconcile once more against inner_size().
        self.window
            .renderer
            .resize(&self.app.gpu, physical.width, physical.height)
            .context("synchronize renderer swapchain with resized physical client")?;
        self.reconcile_authoritative_dpi("resized")?;
        let requested_physical = self.window.window.inner_size();
        if requested_physical.width == 0 || requested_physical.height == 0 {
            return Ok(());
        }
        let presentation = self.window.renderer.presentation_geometry();
        trace_surface_size_clamp(
            self.app.trace_resize,
            "BT_RESIZE_TRACE",
            requested_physical,
            presentation,
        );
        let render_physical = presentation_physical_size(presentation);
        let resize_trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Resize,
        };
        // solve -> seat rects -> cols/rows -> the existing 200ms ConPTY quiet
        // coalescing -> LayoutKey. §4.2 fixes this order and forbids the reverse
        // (red line L10); the solver itself takes no part in the debounce.
        self.resolve_seat_layout(render_physical);
        let observed_at = Instant::now();
        // A `Resized` that settles back onto the grid ConPTY already has — the common shape of
        // the very first delivery after a clean, same-DPI session restore — must not schedule a
        // real ConPTY resize at all; see `coalesce_pty_resize_on_grid_change`.
        //
        // Every leaf, not just the focused one: an OS resize moves every pane's
        // rectangle, and a pane whose shell is never told stays at the width it
        // had before the drag until something else happens to re-solve.
        self.resize_leaves_to_layout(observed_at, "resize terminal actor")?;
        // G93 / `M2-tiny-window-priority.md` §3.2: a **pinned** float is
        // re-clamped and never dissolved. It is asked *after* the layout has been
        // re-solved, because the box it is being clamped into is that layout's
        // output — clamping against the old one would put it back inside a
        // viewport that no longer exists.
        //
        // A **transient** peek takes the other half of that ruling and dissolves,
        // exactly as the hover-peek thumbnail above already does: it was never
        // promised to anyone, and its anchor has just moved.
        self.reclamp_float();
        self.sync_math_layout_key();
        // The grid actually in force, which inside a coalescing window is not yet the one the
        // child has heard. A tab with no shell has no grid and nothing to gate,
        // exactly as in `commit_seat_geometry` (§7.1.6h).
        self.pending_resize_present = self.focused().map(|leaf| leaf.grid);
        self.publish_frame(resize_trigger)?;
        // Windows dispatches Resized from its modal move/size loop. `Renderer::resize` only records
        // the requested swapchain geometry; `present` prepares this newly projected frame first,
        // then performs ResizeBuffers immediately before acquire/submit. Thus the handler exposes
        // no intermediate "new surface + old grid" frame. Until this callback completes, DWM may
        // scale the previous complete back buffer as one image, which is the all-frame fallback.
        self.redraw()
    }

    /// **The scale has arrived and the rectangle that goes with it has not**
    /// (T-CARD-ANCHOR-DPI, user report 2026-09-14).
    ///
    /// Windows sends `WM_DPICHANGED` and winit turns it into this event *before*
    /// it applies the new rectangle: the `SetWindowPos` is the last thing its
    /// handler does, after this returns. `claim_lawful_layout` already says so
    /// in as many words — it claims the next rectangle sight unseen "before the
    /// new rectangle is known" — and everything below that line then works with
    /// `inner_size()`, which for the whole of this call is still the rectangle
    /// of the display the window is leaving.
    ///
    /// That rectangle is the right one for the swapchain and for the frame,
    /// because it is what is on the glass; it is the wrong one for a pane's
    /// grid, because a grid is pixels counted in cells and only one of those two
    /// has changed yet. See [`DpiRectangle`], which is announced here and holds
    /// the whole of that argument.
    pub(crate) fn scale_factor_changed(&mut self) -> Result<()> {
        // A scale change is a display change on every road that produces one, so
        // the rate is re-read here as well as on the move — see
        // [`Self::follow_the_display`]. The two events do not always both
        // arrive, and neither is reliably first.
        self.follow_the_display();
        self.claim_lawful_layout();
        self.window.dpi_rectangle.announced();
        self.defer_preview_resample(Instant::now());
        self.reconcile_authoritative_dpi("scale-factor-changed")?;
        self.resize(self.window.window.inner_size())
    }

    /// **A rectangle from the OS**, and the one road on which a pane's grid may
    /// be derived from one.
    ///
    /// Lowering the flag here rather than inside [`Self::resize`] is the whole
    /// of the distinction: `scale_factor_changed` calls that method too, with a
    /// rectangle Windows has not chosen yet, and a window told "the rectangle
    /// has arrived" by its own call would be told it by the very event that
    /// says it has not.
    pub(crate) fn resized(&mut self, physical: PhysicalSize<u32>) -> Result<()> {
        self.window.dpi_rectangle.arrived();
        self.resize(physical)
    }

    /// **Pay for a DPI change no rectangle followed** (T-CARD-ANCHOR-DPI).
    ///
    /// Windows always performs a `SetWindowPos` after `WM_DPICHANGED`, but it
    /// does not always change the client size with it: a maximized or
    /// full-screen window whose own display's scale was changed underneath it
    /// keeps every physical pixel it had, so no `WM_SIZE` is produced and no
    /// `Resized` ever arrives. Its panes still owe their grids — the pixels
    /// stayed and the cells did not.
    ///
    /// Run from [`Runtime::turn`] for [`Self::settle_deferred_dpi`]'s reason and
    /// directly after it: the loop coming back round is the first moment at
    /// which every message the DPI change produced has been delivered, so the
    /// rectangle in hand by then is the one the window is keeping — whether that
    /// is a new one or the one it already had.
    pub(crate) fn settle_dpi_rectangle(&mut self) -> Result<()> {
        if !self.window.dpi_rectangle.due() {
            return Ok(());
        }
        let physical = presentation_physical_size(self.window.renderer.presentation_geometry());
        if !resize_worth_solving(self.window_is_iconic(), physical) {
            return Ok(());
        }
        self.resolve_seat_layout(physical);
        self.resize_leaves_to_layout(
            Instant::now(),
            "rebuild terminal grid on the rectangle a scale change settled on",
        )?;
        // **The other half of the DPI move** (`BT_CARD_TRACE`, T-CARD-TRACE).
        // The scale arrived at `apply_scale_factor`; the rectangle it was owed
        // arrives here, and a card's height is answerable to both — so the same
        // station is written again with the scale it has now on both sides,
        // which is what makes the pair readable as one move.
        let scale = self.window.renderer.metrics().scale_factor;
        self.trace_card_scale(card_trace::why::RECTANGLE_SETTLED, scale, scale);
        self.sync_math_layout_key();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(())
    }

    /// **Pay for a DPI change the drag deferred** (§7.50, user ruling
    /// 2026-08-31).
    ///
    /// One turn of the loop asks one question: is there a scale change written
    /// down, and has the hand let go? [`DpiSettlement`] holds why the answer is
    /// worth waiting for. Run from [`Runtime::turn`] rather than from an event,
    /// because the end of the OS's modal move/size loop is not one — winit
    /// surfaces neither end of it, and the loop coming back round is the one
    /// thing that is guaranteed to happen after the hand lets go.
    pub(crate) fn settle_deferred_dpi(&mut self) -> Result<()> {
        if !self
            .window
            .dpi_settlement
            .due(self.window.custom_window_frame.in_size_move())
        {
            return Ok(());
        }
        // **No claim is made here**, and that is deliberate. `claim_lawful_layout`
        // says "the next rectangle to arrive is one this program asked for", and
        // this settlement asks for none: the rectangle arrived during the drag
        // and the `ScaleFactorChanged` that started it claimed it then. A second
        // standing claim would swallow the first rectangle of whatever the reader
        // does next, which is the one thing the provenance rule exists to get
        // right (§7.50, and `size_authority_for_rectangle`).
        self.reconcile_authoritative_dpi("size-move-settled")?;
        Ok(())
    }

    pub(crate) fn reconcile_authoritative_dpi(&mut self, stage: &'static str) -> Result<bool> {
        let physical = self.window.window.inner_size();
        // **The second reader of the same rectangle**, held to the same rule
        // ([`resize_worth_solving`]): `inner_size()` on an iconic window is the icon's client
        // area, and every geometry step below — the swapchain, the solve, the per-leaf grids —
        // would take it for the window's. `WindowEvent::Resized` is not the only way in here;
        // `scale_factor_changed` and `settle_deferred_dpi` reach it directly.
        let worth = resize_worth_solving(self.window_is_iconic(), physical);
        if worth {
            self.window
                .renderer
                .resize(&self.app.gpu, physical.width, physical.height)
                .context("reconcile swapchain with physical client size")?;
            ensure_swapchain_matches_inner(&self.window.renderer, physical)?;
        }
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        // Touching the surface is what obliges a solve, not changing the DPI:
        // every seat rectangle is a function of the surface, so the answer that
        // was true of the old one is not yet true of this one. The equal-scale
        // path below returns early and this method publishes a frame, so the
        // solve has to happen here rather than after that branch — otherwise the
        // frame is drawn against a rectangle nobody re-derived, which is how the
        // terminal came to be drawn over its neighbour's seat. `solve` is pure
        // and the tree has not changed, so on the common no-op path this is the
        // same answer arrived at again.
        if worth {
            self.resolve_seat_layout(render_physical);
        }
        let snapshot = dpi_snapshot(&self.window.window)?;
        log_dpi_snapshot(
            stage,
            snapshot,
            Some(self.window.renderer.metrics().scale_factor),
            self.window.renderer.presentation_geometry(),
            physical,
        );
        if scale_factors_match(
            self.window.renderer.metrics().scale_factor,
            snapshot.authoritative_scale,
        ) {
            return Ok(false);
        }
        // **A hand still on the frame is told nothing and asked for nothing**
        // (§7.50). The swapchain and the seat rectangles above are already level
        // with the surface, which is everything this frame needs to be drawn; the
        // font remeasure, the per-leaf cells, the grids and — the one that
        // actually fought the drag — this window's minimum size going back to the
        // OS as a `SetWindowPos` all wait for the hand to let go. Written down
        // once however many times the seam changed its mind, and spent by
        // `settle_deferred_dpi` on the first turn after `WM_EXITSIZEMOVE`.
        if !self
            .window
            .dpi_settlement
            .arrived(self.window.custom_window_frame.in_size_move())
        {
            return Ok(false);
        }

        self.apply_scale_factor(snapshot.authoritative_scale)?;
        if worth {
            // A DPI change is a similarity transform: the same tree solved again
            // on a new rectangle. Red line L5 — not one ratio, not one fixed
            // extent is rewritten on this path, and `resolve_seat_layout` writes
            // none because `solve` is pure.
            self.refresh_work_area();
            self.apply_window_min_inner_size()?;
            self.resolve_seat_layout(render_physical);
            self.resize_leaves_to_layout(
                Instant::now(),
                "rebuild terminal grid after authoritative DPI correction",
            )?;
        }
        self.sync_math_layout_key();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    fn apply_scale_factor(&mut self, scale_factor: f64) -> Result<()> {
        // **The scale the panel's two lists were last measured against**, read
        // before the renderer forgets it — see [`Self::restate_panel_scroll`].
        let measured_at = self.window.renderer.metrics().scale_factor;
        let metrics = self
            .window
            .renderer
            .update_scale_factor(&mut self.app.gpu, scale_factor)
            .context("remeasure terminal font at new DPI")?;
        ensure_metrics_match_authoritative_scale(metrics.scale_factor, scale_factor)?;
        // **And the one number in the panel that is a measurement rather than a
        // solve** (user report 2026-09-12). Every rectangle the rail and the card
        // column stand on is worked out from the window's current scale on the
        // frame it is drawn — and this one is not: it is physical pixels, kept
        // from the display the list was last scrolled on.
        self.restate_panel_scroll(measured_at, scale_factor);
        // Every shell in every tab: a DPI change is a fact about the display, so
        // no screen anywhere in the window is exempt from it.
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                leaf.session
                    .set_cell_height_subpixels(metrics.cell_height_subpixels());
                leaf.session
                    .set_cell_width_subpixels(cell_width_subpixels(metrics));
                leaf.session
                    .set_ascii_baseline_subpixels(metrics.ascii_baseline_subpixels());
                leaf.session
                    .set_font_size_subpixels(metrics.font_size_subpixels());
            }
        }
        // **And every page this window hosts** (§7.8 ⑨). The same sentence, said
        // to the one kind of content in this window that computes its own
        // pixels. A WebView2 controller in composition hosting has no window of
        // its own to hear a display change through, and its own detection —
        // switched off in `bt_platform`'s `configure` — noticed late enough to
        // be photographed: the page arrived on the second display still laid out
        // at the first one's ratio. This is where the number is known, so this
        // is where it is said. A refusal is one line and not a window taken
        // down, exactly as a refused placement is.
        for web in self.window.web.values_mut() {
            if let Err(error) = web.set_device_scale(scale_factor) {
                eprintln!("BT_WEB rasterization scale failed: {error}");
            }
        }
        self.trace_card_scale(
            card_trace::why::SCALE_APPLIED,
            measured_at,
            metrics.scale_factor,
        );
        Ok(())
    }
}
