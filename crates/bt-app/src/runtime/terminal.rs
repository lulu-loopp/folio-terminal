//! `terminal` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::PtyTarget;
use crate::TermViewPorts;
use crate::runtime::attention::deliver_osc_attention;
use crate::runtime::clipboard::input_line_needs_a_space_first;
use crate::runtime::clipboard::offer_pty_input;
use crate::runtime::clipboard::paste_text;
use crate::runtime::mouse::drain_whole_units;
use crate::runtime::mouse::link_target_to_ask_about;
use crate::window_seam::prelude::*;
use crate::{
    ApplicationChange, AttentionDelivery, CommandFlash, DrainOutcome, Fading, FilesFocusArrival,
    FlashBand, FormulaSwitches, LeafId, LocalImageActivation, MouseRoute, PasteTarget,
    RailJumpLanding, ReferenceCard, RowHost, Runtime, SelectionDrag, SelectionDragMode,
    ShellSession, Step, TerminalReference, UserInputKind, apply_stored_terminal_font,
    attention_trace, born_in_tab, cmdrail, coalesce, create_leaf_session, cubic_bezier,
    diagnostics, files, hang_watch, input, marks, mouse_trace, presentation_physical_size, quake,
    scrollback_quota, seats, shell_literal, termscroll, toast,
};
use crate::{
    ClickIntent, DRAIN_SLICES_PER_TURN, DRAIN_TURN_BUDGET, HyperlinkActivation, LeafSeed, TabState,
    TextScale, hyperlink_activation, profiles,
};
use crate::{LeafView, TextStep, pane_cell_metrics};
use anyhow::Context;
use anyhow::Result;
use bt_doc::Bias;
use bt_layout::SeatId;
use bt_render::motion::EASE;
use bt_render::{FrameSource, FrameTrigger, background_rgb, foreground_rgb, scheme_in_force};
use bt_term::{TerminalCanvas, TerminalPalette};
use bt_viewport::{HyperlinkHit, ViewSelection};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;

impl<B: Backend> Runtime<'_, B> {
    /// **Scroll one pane so that a command's own row stands at the top of it**,
    /// and flash that row so the eye can find it.
    ///
    /// This is the first consumer of `ViewportProjection::set_scroll_anchor`, and
    /// the point of going through it rather than through `scroll_by_rows` is that
    /// the result is a *position* rather than an act. A row number computed now
    /// would be wrong at the next PTY frame — every line the shell prints pushes
    /// the content up under it — whereas an anchored viewport is re-projected
    /// against the document every frame and keeps naming the same content through
    /// output, reflow, resize and eviction. §7.1.5c wrote it down years before
    /// there was anything to scroll: "跳转=Anchored(逻辑行)".
    ///
    /// A mark whose anchor is gone does nothing at all. That is not a failure
    /// path being tolerated — it is the honest answer to "jump to a command whose
    /// output was deleted", and the alternative (scroll somewhere near where it
    /// used to be) is the class of guess this whole block refuses.
    ///
    /// Answers with the numbers the jump was computed from when there was an
    /// anchor to jump to, for the rail's own trace line one method up.
    pub(in crate::runtime) fn jump_to_command_mark(
        &mut self,
        seat: SeatId,
        mark: bt_term::CommandMarkId,
    ) -> Result<Option<RailJumpLanding>> {
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(leaf) = self.sessions.get_mut(&seat) else {
            return Ok(None);
        };
        let Some(anchor) = leaf
            .session
            .session
            .command_mark(mark)
            .and_then(|mark| leaf.session.session.command_mark_anchor(mark.start))
            .cloned()
        else {
            return Ok(None);
        };
        // `scroll_y = anchor_y(source) + local_offset`, so a negative offset lifts
        // the viewport's top *above* the anchor and the row lands that far down the
        // pane — the mock-up's own `line.offsetTop - 8`.
        let local_offset =
            -((cmdrail::JUMP_TOP_INSET_LOGICAL_PX * scale * bt_viewport::SUBPIXELS_PER_PX as f32)
                .round() as i64);
        leaf.view
            .projection
            .set_scroll_anchor(Some(bt_viewport::ScrollAnchor {
                source: anchor.clone(),
                local_offset,
            }));
        // Read off the frame that is on the glass, because the frame this jump
        // lands on has not been composed yet (the same reason the flash band
        // waits, below). So this is the landing the projection will compute
        // unless the document moves under it first — and it is computed by the
        // clamp the landing itself uses rather than a second opinion about it.
        let landing = mouse_trace::is_on().then(|| {
            let extent_subpixels = leaf.view.projection.scroll_extent_subpixels();
            let unclamped = leaf
                .view
                .projection
                .scroll_y(leaf.session.session.document())
                .ok()
                .flatten();
            RailJumpLanding {
                anchor_y_subpixels: unclamped.map(|top| top.saturating_sub(local_offset)),
                local_offset_subpixels: local_offset,
                window_top_subpixels: unclamped.map(|top| top.clamp(0, extent_subpixels)),
                extent_subpixels,
                relief_subpixels: leaf.view.projection.bottom_relief_subpixels(),
            }
        });
        self.window.command_flash = Some(CommandFlash {
            seat,
            band: FlashBand::Row(anchor),
            started: Instant::now(),
        });
        // A jump is a scroll, and the bar says where the jump landed
        // (P2-9 slice 1). Only the clock is set here, for the same reason the
        // overlay is not rebuilt below: the frame this jump lands on has not
        // been composed yet.
        self.wake_terminal_thumb(seat);
        // The pane's own repaint, and **not** an overlay rebuild beside it. The
        // band is placed out of the frame that is on the glass, and the frame
        // showing this jump has not been composed yet — an overlay built here
        // would light the row the anchor was in *before* the scroll, for one
        // frame. [`Self::command_flash_deadline`] has already asked for the next
        // one, by which time the redraw this line requests has run.
        self.repaint_pane_change(seat)?;
        Ok(landing)
    }

    /// `Ctrl+Shift+↑/↓` — the command before or after the one the viewport is
    /// showing.
    ///
    /// **Relative to the viewport, not to the last jump.** The mock-up computes it
    /// from `scrollTop` every time (4690-4707) and it matters: a walk that
    /// remembered its own cursor would disagree with the screen the moment the
    /// wheel was touched, and the user would press "previous" and go somewhere
    /// they had already scrolled past.
    ///
    /// **No wrap-around at either end**, also the mock-up's (`if (at < 0) return`)
    /// and deliberately the opposite of the search's ring: a rail is a history with
    /// a beginning and an end, and arriving at the oldest command and being thrown
    /// to the newest is not what the key said.
    pub(in crate::runtime) fn step_command_mark(&mut self, step: Step) -> Result<()> {
        let seat = self.focused_leaf;
        let Some(leaf) = self.sessions.get(&seat) else {
            return Ok(());
        };
        let marks = leaf.session.session.command_marks();
        if marks.is_empty() {
            return Ok(());
        }
        // Where the viewport is looking, as a `ContentAnchor` — the projection's
        // own `ScrollAnchor.source` when it has been scrolled, and the newest mark
        // of all when it is resting at the live bottom.
        let here = leaf
            .view
            .projection
            .scroll_anchor()
            .map(|anchor| anchor.source.clone());
        let at = match &here {
            Some(anchor) => leaf.session.session.command_mark_at_or_before(anchor),
            None => marks.last().map(|mark| mark.id),
        };
        let index = at.and_then(|id| marks.iter().position(|mark| mark.id == id));
        let Some(target) =
            stepped_command_mark(marks.len(), index, step).map(|index| marks[index].id)
        else {
            return Ok(());
        };
        // The walk is not the rail, so it writes no rail line: what the jump was
        // computed from is the rail's own forensics.
        self.jump_to_command_mark(seat, target).map(|_| ())
    }

    /// The ghost is gone, and it goes in one frame — see [`Self::drag_ghost_layer`].
    pub(in crate::runtime) fn forget_the_ghost(&mut self) {
        self.window.settling.forget(&Fading::DragGhost);
    }

    /// Point the "Scrollback" row at `lines` per pane (P2-9 slice 2, 2026-08-19).
    ///
    /// [`Self::apply_block_max_height`]'s shape — every pane in every tab, an
    /// immediate write, one frame's cost — with the one difference that this row's
    /// answer can **delete something**, and so is worth saying out loud.
    ///
    /// **A smaller number binds at once, and waiting would not have spared a line.**
    /// The store evicts `len - quota` in a single batch the next time anything is
    /// frozen, so a capacity installed lazily still collapses history — just at
    /// whichever later moment the shell happened to print. Doing it here makes the
    /// deletion happen *because of* the answer that caused it, in the frame the
    /// reader is watching, rather than five minutes later during someone else's
    /// build. It also travels the same road `Clear scrollback` travels
    /// (`DualPlaneSession::set_frozen_quota` → `delete_history`), so anchors,
    /// command marks, inline images and decorations go with the lines instead of
    /// outliving them.
    ///
    /// **A larger number resurrects nothing**, which is the honest half of the same
    /// coin: an evicted line is gone, and only the future can be kept differently.
    /// The two together are why no gate asks first — a confirmation would be a new
    /// interaction for this dialog, where no other picker confirms anything, and the
    /// row's own sentence already says the oldest go first.
    ///
    /// **A pane on the alternate screen is retuned like any other.** What this
    /// changes is the primary transcript, which `vim` is not writing to and which is
    /// waiting for it underneath; skipping those panes would leave a window whose
    /// answer was true of some of its terminals.
    pub(crate) fn apply_scrollback_lines(&mut self, lines: u32) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.scrollback_lines = lines;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        let quota = scrollback_quota(lines);
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                leaf.session.set_frozen_quota(quota);
            }
        }
        // The scroll offset is clamped against a projection whose extent has just
        // shrunk, so a reader parked deep in the history they no longer have is
        // carried to the oldest line they still do rather than left over nothing.
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// **The grid's face and size, changed while the window is up.**
    ///
    /// The DPI path arriving through another door, and it performs the same six
    /// steps in the same order because they answer the same question: the cell
    /// has changed size, so every rectangle derived from it is stale and every
    /// shell has to be told how many columns it now has.
    ///
    /// 1. re-measure and invalidate (`Renderer::apply_terminal_font`),
    /// 2. tell every leaf in every tab its new cell — a font is a fact about the
    ///    window, so no pane anywhere is exempt, exactly as a DPI change is,
    /// 3. re-derive the work area and the window's minimum inner size, because
    ///    the minimum is stated in cells,
    /// 4. re-solve the seat layout into the same surface,
    /// 5. resize every PTY through the existing gate and debounce,
    /// 6. re-key the math layout and publish.
    ///
    /// Returns whether anything changed. No restart card: unlike the Language
    /// row, this one takes effect where the user can see it.
    pub(crate) fn apply_terminal_font(
        &mut self,
        family: String,
        cjk_family: String,
        size: u8,
    ) -> Result<bool> {
        let current = self.app.settings_store.loaded();
        if current.terminal_font_family == family
            && current.terminal_cjk_font_family == cjk_family
            && current.terminal_font_size == size
        {
            return Ok(false);
        }
        let mut settings = current.clone();
        settings.terminal_font_family = family;
        settings.terminal_cjk_font_family = cjk_family;
        settings.terminal_font_size = size;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        // **Every window on this device, not only this one** (multiwindow slice
        // C). `set_terminal_font` moved the font database and the size on the
        // `GpuContext` — one database serves every window, which is exactly why
        // its own doc requires `apply_font_change` per window: a sibling that is
        // never told goes on composing rows in the face it no longer has.
        self.note_application_change(ApplicationChange {
            font: true,
            look: false,
            caret: false,
            option: false,
            paid_by: Some(self.window.window.id()),
        });
        self.adopt_terminal_font()?;
        Ok(true)
    }

    /// **What one window owes a face change** — the six steps
    /// [`Self::apply_terminal_font`] describes, for this window.
    ///
    /// Its own verb because it is owed twice: once by the window whose settings
    /// page was pressed, and once by every other window this application has
    /// open, which never touched a row and is drawing out of the same font
    /// database.
    pub(crate) fn adopt_terminal_font(&mut self) -> Result<()> {
        apply_stored_terminal_font(
            &mut self.app.gpu,
            &mut self.window.renderer,
            self.app.settings_store.loaded(),
        )?;
        // **The base moved; every pane keeps its rung** (ticket 37) and is re-derived from it.
        self.apply_every_leaf_metrics()?;
        let physical = self.window.window.inner_size();
        if physical.width > 0 && physical.height > 0 {
            let render_physical =
                presentation_physical_size(self.window.renderer.presentation_geometry());
            self.refresh_work_area();
            self.apply_window_min_inner_size()?;
            self.resolve_seat_layout(render_physical);
            self.resize_leaves_to_layout(
                Instant::now(),
                "rebuild terminal grid after a font change",
            )?;
        }
        self.sync_math_layout_key();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// One terminal pane's body, or `None` when there is no bar to stand on it.
    ///
    /// **The alternate screen is suppressed here**, the way it is for the search
    /// capsule ([`Self::seat_can_search`], D-5) and for the rail
    /// ([`cmdrail::host_rect`]): §3.2 keeps the two screens in isolated anchor
    /// namespaces, so the primary history's extent says nothing about what
    /// `vim` is drawing, and a lane that took presses over somebody else's
    /// canvas would scroll a document that is not on screen.
    fn terminal_scroll_body(&self, seat: SeatId) -> Option<[f32; 4]> {
        let crate::TerminalPane { session: leaf, .. } = self.sessions.get(&seat)?;
        if leaf.session.terminal_modes().alternate_screen {
            return None;
        }
        let scale = self.window.renderer.scale_factor() as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        Some([
            body.x as f32,
            body.y as f32,
            (body.x + body.width) as f32,
            (body.y + body.height) as f32,
        ])
    }

    // ─────────────────── the terminal pane's own scroll bar ───────────────────
    //
    // P2-9 slice 1. Everything below reads [`termscroll`] and adds nothing to
    // it: the geometry, the visibility rule and both maps a hand can run
    // backwards live there, where they can be asked the questions a hand asks
    // without a window, a shell or a GPU.

    /// One pane's bar, from that pane's own projection.
    ///
    /// The three numbers are read off [`bt_viewport::ViewportProjection`] and
    /// nowhere else — the extent it clamps the wheel by, the page it measures
    /// that against, and where the view currently stands. A bar derived from
    /// anything else would be a second opinion about how far the view can go.
    pub(crate) fn terminal_scroll_bar(
        &self,
        seat: SeatId,
    ) -> Option<termscroll::TerminalScrollBar> {
        let body = self.terminal_scroll_body(seat)?;
        let leaf = self.sessions.get(&seat)?;
        let scale = self.window.renderer.scale_factor() as f32;
        termscroll::bar(
            body,
            leaf.view.projection.scroll_extent_subpixels(),
            leaf.view.projection.viewport_height_subpixels(),
            leaf.view.projection.scroll_offset_subpixels(),
            scale,
        )
    }

    /// One pane's foot bar, from that pane's own axis.
    ///
    /// The three numbers come off [`bt_viewport::horizontal::HorizontalProjection`]
    /// and nowhere else, which is [`Self::terminal_scroll_bar`]'s rule on the
    /// other axis: the extent it clamps by, the window it measures that against,
    /// and where the window stands. **A wrapping pane returns `None` here for
    /// free** — its projection floors the extent at its own width, so the shared
    /// derivation finds no overflow and declines. There is no `line_wrapping`
    /// branch in this function, and there must not be one: two places deciding
    /// whether a pane has a horizontal axis is two places that can disagree.
    pub(crate) fn terminal_column_bar(
        &self,
        seat: SeatId,
    ) -> Option<termscroll::TerminalColumnBar> {
        let body = self.terminal_scroll_body(seat)?;
        let leaf = self.sessions.get(&seat)?;
        let axis = leaf.view.projection.horizontal();
        let scale = self.window.renderer.scale_factor() as f32;
        termscroll::column_bar(
            body,
            axis.content_extent().0,
            axis.viewport_columns(),
            axis.x_origin().0,
            scale,
        )
    }

    /// The pane whose foot mark the pointer is **on**, and that pane's bar.
    ///
    /// Two gates, not one. The geometric one is the mark itself — there is no
    /// lane down here, so a press beside the mark is the terminal's (see
    /// `termscroll::TerminalColumnBar::thumb_holds`). The other is that the mark
    /// is actually drawn: a bar resting invisibly over somebody's prompt must not
    /// answer for a press aimed at the prompt.
    pub(crate) fn terminal_column_bar_under(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(SeatId, termscroll::TerminalColumnBar)> {
        let at = [position.x as f32, position.y as f32];
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        let bar = self.terminal_column_bar(seat)?;
        if !bar.thumb_holds(at) {
            return None;
        }
        matches!(
            self.terminal_column_thumb(seat, Instant::now()),
            termscroll::Thumb::Shown { .. }
        )
        .then_some((seat, bar))
    }

    /// Note that this pane's foot mark has a reason to be up **now**.
    pub(crate) fn wake_terminal_column(&mut self, seat: SeatId) {
        let active = self.window.active_tab;
        if let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) {
            leaf.view.column_awake = Instant::now();
        }
    }

    /// The pane whose lane the pointer is in, and that pane's bar.
    pub(crate) fn terminal_bar_under(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(SeatId, termscroll::TerminalScrollBar)> {
        let at = [position.x as f32, position.y as f32];
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        let bar = self.terminal_scroll_bar(seat)?;
        bar.lane_holds(at).then_some((seat, bar))
    }

    /// Every terminal pane's mark, one layer each.
    ///
    /// The state machine is asked **before** the geometry, which is the right
    /// way round: [`termscroll::visibility`] owns both suppressions, so a pane
    /// with no scrollback and a pane running `vim` are refused by the rule
    /// rather than by a `None` falling out of the arithmetic further down.
    pub(in crate::runtime) fn terminal_bar_layers(&self) -> Vec<marks::OverlayLayer> {
        let palette = bt_render::chrome_palette();
        let now = Instant::now();
        let motion = self.app.motion;
        self.sessions
            .iter()
            .filter_map(|(seat, leaf)| {
                let situation = termscroll::ThumbSituation {
                    has_history: leaf.view.projection.scroll_extent_subpixels() > 0,
                    alternate_screen: leaf.session.session.terminal_modes().alternate_screen,
                    scrolled: leaf.view.projection.is_scrolled(),
                    near: self.terminal_thumb_hover == Some(*seat),
                    held: self
                        .held_terminal_thumb_drag()
                        .copied()
                        .is_some_and(|drag| drag.seat == *seat),
                    since_rest: now.saturating_duration_since(leaf.view.thumb_awake),
                };
                let thumb = termscroll::visibility(situation, motion, |x| cubic_bezier(x, EASE));
                let bar = self.terminal_scroll_bar(*seat)?;
                termscroll::layer(&bar, thumb, &palette)
            })
            .chain(self.sessions.keys().filter_map(|seat| {
                // The foot's mark, on the same layer and out of the same ink.
                // Its rule is its own (`termscroll::column_visibility`), so it is
                // asked through `terminal_column_thumb` — the one derivation the
                // press reads too, which is what stops the two from disagreeing
                // about whether there is anything on the glass to take.
                let bar = self.terminal_column_bar(*seat)?;
                termscroll::column_layer(&bar, self.terminal_column_thumb(*seat, now), &palette)
            }))
            .collect()
    }

    /// The rows one host is showing, and the root they hang off.
    pub(crate) fn host_rows(&mut self, host: RowHost) -> Option<(String, Vec<files::TreeRow>)> {
        match host {
            // The Git page's rows are not tree rows — see [`Self::peek_row`],
            // which is where the three hosts meet.
            RowHost::Git(_) | RowHost::Terminal(_) => None,
            RowHost::Column(seat) => {
                let root = self.files_state(seat).root;
                let trees = self.files_trees(Instant::now());
                let rows = trees.get(&seat)?.rows.clone();
                Some((root, rows))
            }
            RowHost::Float(id) => {
                let files = self.window.float.live(id)?.files()?;
                let view = files::tree_view(&files.files, &files.cache);
                Some((files.files.root.clone(), view.rows))
            }
        }
    }

    /// **The reference the pointer is standing on in a terminal pane**, resolved
    /// from that pane's own last-drawn frame (user ruling 2026-08-27, §7.29).
    ///
    /// One resolution for the three things a glance needs — *which* card, *what*
    /// it is about, and *where* it stands — because they are three readings of
    /// one fact and a card placed beside a rectangle derived separately from the
    /// path it shows is the class of bug [`Self::peek_row_rect`]'s own note is
    /// about, one surface further out.
    ///
    /// **Nothing here decides what a reference is.** The cell either carries a
    /// hyperlink or it does not, and it carries one only because §7.1.5j ①
    /// folded every printed shape of a local file into a `file:` link and
    /// `PrintedPathLinks` refused to emit one for a path no worker has verified.
    /// So "existing file or folder" is not re-asked here — it was answered
    /// before the cell was drawn, and asking again would be the second list that
    /// clause exists to prevent.
    ///
    /// `cell` is the flat index [`RowHost::Terminal`] and
    /// [`float::FloatTrigger::Reference`] both carry. `None` for a cell that is
    /// off this frame, carries no link, or carries one this window has no card
    /// for.
    pub(crate) fn terminal_reference_at(
        &self,
        seat: SeatId,
        cell: u32,
    ) -> Option<TerminalReference> {
        let frame = self.pane_frame(seat)?;
        let columns = frame.columns.get();
        let (row, column) = (cell / columns, cell % columns);
        let hyperlink = frame.hyperlink_at(row, column)?;
        let namespace = self.seat_path_namespace(seat);
        let namer = bt_transcript::paths::PathNamer::Pane(&namespace);
        let card = reference_card(&hyperlink.uri, namer, &|path| {
            self.seat_path_verdict(seat, path)
        })?;
        let cells = frame.hyperlink_cells(&hyperlink);
        // **Identity is the run's first cell and the target together.** The
        // target alone would make one path printed twice one reference, so a
        // hand moving from the first to the second would move no card; the cell
        // alone would make one reference two the moment the pointer slid one
        // column along it, and the 350ms would never mature. The first cell of
        // the run is the one number both hands agree on.
        let key = format!("{}|{}", cells.first()?, hyperlink.uri);
        // The row's own drawn interval, from the frame that drew it — a pane
        // holding a math block has rows of unequal height, and a multiple of the
        // nominal cell height would place the card beside the wrong one.
        let interval = frame
            .selection_span_vertical_interval(&bt_viewport::SelectionSpan {
                row,
                start_column: column,
                end_column: column,
            })
            .ok()?;
        // The cell the card's anchor is measured in is the one the pane's picture was drawn at
        // (ticket 37), not the window's.
        let metrics = self.pane_frame_metrics(seat)?;
        let scale = metrics.scale_factor as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        let subpixels = |value: i64| value as f32 / bt_viewport::SUBPIXELS_PER_PX as f32;
        let rect = reference_run_rect(
            &cells,
            columns,
            row,
            [
                body.x as f32 + metrics.padding_px,
                body.y as f32 + metrics.padding_px,
            ],
            metrics.cell_width_px,
            subpixels(interval.start),
            subpixels(interval.end),
        )?;
        Some(TerminalReference { card, key, rect })
    }

    /// The cell of a terminal pane the pointer is standing on, when the
    /// reference printed there is one this window has a *file* card for.
    ///
    /// Filtered to the file here rather than in [`Self::peek_row`], so that the
    /// glance's intent is never armed over a folder at all: a folder's card is
    /// the files flyout on the float's own clock, and two clocks counting
    /// against one reference would race to put two different windows over it.
    ///
    /// **A pointer that belongs to something else arms nothing**, which is the
    /// same pair of refusals the underline and the picture flyout make three
    /// lines apart in [`Self::pointer_moved`]: a formula owns the cells it is
    /// drawn over, and a route in flight — a selection being pulled, a button
    /// being forwarded to the program, a block being dragged — owns the pointer
    /// outright. Asked of `self` rather than handed in because both are facts
    /// about this instant, and this is the only reader of them here.
    pub(crate) fn terminal_reference_cell(&mut self) -> Option<(RowHost, usize)> {
        if self.held_mouse_route().is_some() || self.math_hit().is_some() {
            return None;
        }
        let (seat, hit) = self.pane_frame_hit()?;
        let cell = self.reference_cell_index(seat, hit)?;
        matches!(
            self.pointer_reference_at(seat, cell)?.card,
            ReferenceCard::File(_)
        )
        .then_some((RowHost::Terminal(seat), usize::try_from(cell).ok()?))
    }

    /// **The one mutation door of a pane's text size** (ticket 37).
    ///
    /// The three keyboard rows (`text-larger`, `text-smaller`, `text-actual-size`), the wheel's
    /// text-size rung and a click on the pane head's indicator all come through here, and nothing
    /// else writes a pane's rung. It moves the rung of `seat` in the tab on screen, derives and
    /// applies the metrics that rung now means ([`step_leaf_text_scale`]), and — only when they
    /// moved — re-solves the panes through the resize road every geometry change already takes:
    /// the local reflow at once when the integer grid changed, the child told at most once, with
    /// the final grid, after the quiet boundary (and not at all when that grid is the one it
    /// already has), and a new picture even when the grid did not change. Keyboard focus does not
    /// move.
    ///
    /// `count` is how many times `step` is taken — one for a key or a click, the whole rungs a
    /// wheel burst spent for the wheel — and the rung moves by all of them before anything is
    /// derived, so a burst of notches is one apply and one re-solve, not one per notch.
    pub(crate) fn step_pane_text_scale(
        &mut self,
        seat: SeatId,
        step: TextStep,
        count: u32,
    ) -> Result<()> {
        let active = self.window.active_tab;
        let (gpu, renderer) = (&mut self.app.gpu, &self.window.renderer);
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        let moved = step_leaf_text_scale(leaf, step, count, |scale| {
            pane_cell_metrics(gpu, renderer, scale)
        })?;
        // The indicator shows the requested rung, which moves even when the clamp keeps the
        // drawn size where it was.
        self.refresh_chrome();
        if !moved {
            return self.present_chrome_change();
        }
        self.resize_leaves_to_layout(Instant::now(), "re-solve a pane at its new text size")?;
        self.sync_math_layout_key();
        self.repaint_pane_change(seat)
    }

    /// `Reset terminal modes` — restore the program-owned protocol state in one
    /// pane. A session effect only: unlike restarting or clearing, it neither
    /// writes to the PTY nor replaces the child. The session's record of what
    /// carries its bytes, told when the pane was spawned, says which modes the
    /// transport keeps (focus reporting under ConPTY).
    pub(in crate::runtime) fn reset_terminal_modes(&mut self, seat: SeatId) -> Result<()> {
        let Some(crate::TerminalPane { session: leaf, .. }) = self.sessions.get_mut(&seat) else {
            return Ok(());
        };
        let transport = leaf.session.pty_transport();
        leaf.session.reset_program_modes(transport)?;
        self.apply_pointer_cursor();
        self.refresh_chrome();
        self.repaint_pane_change(seat)
    }

    /// `Restart shell…` — **the same seat, the same profile, the same folder**
    /// (`docs/M2-restart-shell-contract.md` §1.1).
    ///
    /// The three things the contract says are kept are kept by being *read off
    /// the leaf that is leaving*: its `profile` (the seat's own, decided when it
    /// was created — never "the current default", so a seat restarted twice
    /// starts the same program both times), and
    /// [`ShellSession::seed_place_for_a_new_shell`]: the **last trusted OSC 7 report**,
    /// else the live shell process's folder (T-CWD-FROM-PROCESS, superseding the
    /// contract's "not a reading of the live process" for this verb), else the folder
    /// the shell was put down in. The manual name is kept by not being touched: it is
    /// the tab's (`TabSeed::manual_name`), and nothing here goes near a tab.
    ///
    /// **The tree is not rebuilt** (§1.1, last row): this replaces the runtime
    /// state behind one leaf and moves no rectangle, so pin, tab and position all
    /// survive by construction rather than by being restored.
    ///
    /// The old shell dies when its `ShellSession` is dropped — `PtySession::drop`
    /// runs `shutdown`, which kills the child and joins its reader — and the new
    /// one is born **first**, beside it (`TerminalPane::successor`), so a ConPTY
    /// that cannot be created leaves the pane exactly as it was rather than empty.
    /// That is the reason this cannot half-succeed.
    ///
    /// **What this does not yet do**, and it is written down rather than
    /// forgotten: the transcript is not carried across with a boundary record
    /// (§1.3), the process tree is killed by handle rather than through a Job
    /// Object (§1.2), and the busy confirmation (§1.6) is not asked — that one
    /// needs the busy state machine of §7.1.5b, which this build does not have,
    /// and a confirmation that guessed would be a dialog in front of a fact
    /// nobody measured.
    pub(in crate::runtime) fn restart_shell(&mut self, seat: SeatId) -> Result<()> {
        let Some(leaf) = self.sessions.get(&seat) else {
            return Ok(());
        };
        if leaf.restart_in_flight() {
            return Ok(());
        }
        // **A Restart shell is born in its tab** (coordinator's ruling 2026-10-09): what a launch
        // carried into the tab, it is born with again.
        let seed = born_in_tab(
            restart_seed(
                &leaf.session.launch.profile,
                leaf.session.seed_place_for_a_new_shell(),
            ),
            self.window.tabs[self.window.active_tab]
                .carried_environment
                .as_ref(),
        );
        // **The replacement is born at the old view's rung** (ticket 37): *Restart shell* keeps
        // the pane, so it keeps its text size, and the constructor is handed the rung rather
        // than the new leaf being repaired to it afterwards. Nothing else of the old view is
        // carried — its selection, its frames and its resize debt belong to a transcript that
        // is about to end. A spawn that fails leaves the old leaf exactly as it was.
        let text_scale = leaf.view.text_scale;
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(body) = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)
        else {
            return Ok(());
        };
        let wake = &self.window.pty_wake;
        let formulas = FormulaSwitches::from_settings(self.app.settings_store.loaded());
        let scrollback = scrollback_quota(self.app.settings_store.loaded().scrollback_lines);
        let view = LeafView::at(&mut self.app.gpu, &self.window.renderer, text_scale)?;
        let spawned = create_leaf_session(
            view,
            body,
            LeafId {
                tab: self.window.tabs[self.window.active_tab].id,
                seat,
            },
            wake,
            None,
            &seed,
            &self.app.profile_programs,
            &self.app.settings_store.loaded().default_profile,
            formulas,
            scrollback,
            self.app.settings_store.loaded().line_wrapping,
            &bt_pty::SystemShellEnvironment,
        )?;
        // **A replacement still being born waits beside the shell it replaces** (T-BIRTH-OFF-WINDOW):
        // the old shell goes on being the one typed into until the new one lands
        // (`Runtime::land_births`), and a birth that fails leaves it as it was. A replacement
        // with no shell to wait for replaces it now.
        if spawned.session.life.birth().is_some() {
            if let Some(leaf) = self.sessions.get_mut(&seat) {
                leaf.await_successor(Box::new(spawned));
            }
            self.refresh_chrome();
            return Ok(());
        }
        self.replace_restarted_shell(seat, spawned)
    }

    /// **The restarted shell takes the pane** — the old leaf is dropped here, by the insert:
    /// `PtySession::drop` takes the child with it, and it takes it only once the replacement is
    /// known to exist. From `restart_shell` for a replacement with no shell to wait for, and from
    /// the landing for one that was being born.
    pub(crate) fn replace_restarted_shell(
        &mut self,
        seat: SeatId,
        spawned: crate::TerminalPane,
    ) -> Result<()> {
        self.sessions.insert(seat, spawned);
        // **The window's frame slot held the pane that is gone.** `focus_pane_at`
        // empties it for the same reason when the keyboard moves: leaving a
        // frame there would let the next present assert a grid belonging to a
        // session that no longer exists.
        if seat == self.focused_leaf {
            self.window.last_presented_frame = None;
        }
        // The capsule's hits were cut from a transcript that no longer exists,
        // and so was the cache the next scan would have carried forward: its
        // line ids name lines *inside one transcript*, and the fresh one hands
        // the same ids to different text. **This is the guarantee
        // [`search::scan_history_slice`] names**, and it is why it is discharged
        // here rather than guessed at there — nothing about two windows of ids
        // can tell a reader which transcript minted them. Both go, and a walk
        // in progress goes with the cache (ticket 51).
        self.clear_search_highlights(seat);
        self.window.search_scan = None;
        self.settle_seat_set_change()?;
        self.refresh_chrome();
        self.repaint_pane_change(seat)
    }

    /// Put one row's path into the shell's input line and give it the keyboard
    /// (K144).
    ///
    /// **This is where "insert path" lives** — `DESIGN.md` §7.1.3 records the
    /// 2026-07-17 ruling that took it off the drag (where it made one gesture
    /// mean two different families of thing) and put it here, "explicit,
    /// discoverable, keyboard-reachable".
    ///
    /// **It still lives here now that the drag has it too** (§7.1.1, user ruling
    /// 2026-09-16). What that ruling reversed was the *refusal* on a terminal's
    /// middle, on the finding that the 0717 objection was about a reader who
    /// could not tell the two families apart until the hand had opened — so the
    /// zone came back with the answer written on it. It reversed nothing about
    /// this row: a pointer gesture and a keyboard-reachable menu are the two
    /// halves of one verb, and the difference between them is where they put the
    /// path. **This one goes to the focused terminal and takes the keyboard with
    /// it; the drop goes to the pane under the pointer and takes nothing.**
    ///
    /// **Which terminal.** The one holding the keyboard, which `focused_leaf`
    /// names whenever this tab has a shell at all. The mock-up also searched the
    /// tree for a fallback terminal, because in the mock-up focus could be *on*
    /// the files pane; here layout focus and the keyboard are two different
    /// words (see [`Runtime::focus_pane_at`]), and the keyboard's shell never
    /// stops being a shell.
    ///
    /// **On a folder tab there is no terminal, and the verb does nothing**
    /// (§7.1.6h). This is one of the two places the ruling's "cmdrail/shell
    /// integration 面对无会话 tab 自然缺席,别为它们造空壳" is actually reached: a
    /// files column in a tab of its own still offers `Insert path into terminal`
    /// on its rows, because the row menu is the same menu wherever the column
    /// stands. What it must not do is invent a shell to paste into, or paste
    /// into a shell in *another* tab, which is the only other thing "the focused
    /// terminal" could be made to mean here. Doing nothing is the honest answer
    /// and it is silent because the menu row is not a promise the window made
    /// about this tab — the clipboard row beside it works, and that is the one
    /// that carries a path out of a tab with nothing running in it.
    ///
    /// **It is sent as a paste, not as typing.** The bytes are wrapped by
    /// [`input::paste_bytes`] exactly as a clipboard paste is, so a shell in
    /// bracketed-paste mode is told this arrived as one lump — which is what
    /// distinguishes the paste from typing. The path encoder separately keeps
    /// each representable path in one argument at a fresh argument boundary.
    pub(in crate::runtime) fn insert_path_into_terminal(&mut self, path: &Path) -> Result<()> {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].focused() else {
            return Ok(());
        };
        let seat = self.window.tabs[active].focused_leaf;
        let insertion = shell_literal::paths_text(
            &[path.to_path_buf()],
            &leaf.launch.paste_recipient,
            input_line_needs_a_space_first(&leaf.session),
        );
        if let Some(notice) = shell_literal::refusal_notice(&insertion.refused) {
            self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                notice,
            )?;
        }
        if insertion.text.is_empty() {
            return Ok(());
        }
        let text = insertion.text;

        // The keyboard goes back to the shell before the characters do. The
        // press that raised this menu very likely came from a column that had
        // taken the keyboard, and a path that arrived in a shell the arrow keys
        // still do not reach is a path you cannot edit.
        if self.set_files_keyboard(None, FilesFocusArrival::Pointer) {
            self.refresh_chrome();
        }
        // And layout focus follows, which is the mock-up's `w.focus = leaf.id`:
        // you are being sent somewhere to finish a command, so that is where the
        // window should be pointing.
        if self.seats.set_focus(seat) {
            self.apply_window_min_inner_size()?;
            self.commit_seat_geometry()?;
            self.mark_session_dirty(Instant::now());
        }

        let crate::TerminalPane {
            session:
                ShellSession {
                    life,
                    session,
                    paste_hold,
                    ..
                },
            view: crate::TermView { projection, .. },
            ..
        } = self.window.tabs[active].shell_pane_mut();
        // The person's own gesture, so it waits behind a paste the shell is still owed, in order
        // (`clipboard_lane`), as a keystroke does.
        let target = match paste_hold {
            Some(hold) => PtyTarget::BehindAPaste(hold),
            None => PtyTarget::of(life.pty(), life.birth()),
        };
        paste_text(session, projection, &text, |bytes| {
            write_pty_input(target, bytes, "write an inserted files row path to PTY")
        })?;
        // One user gesture, landing in one named seat — so it answers what that seat was asking
        // (`attention` plan §10.3.2 row 4). The seat is the shell the path went into, which is the
        // tab's own focused leaf and the one `shell_mut` just wrote through.
        let seat = self.window.tabs[active].focused_leaf;
        self.answer_attention(seat, UserInputKind::FilesRow);
        // Review row R1-24 — a path put into the prompt by a row of the column is
        // the reader's own hand, on the same footing as a paste.
        self.note_user_typing(seat);
        self.pending_keyboard_at = Some(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Keyboard,
        })
    }

    pub(crate) fn drain_pty(&mut self) -> Result<()> {
        hang_watch::at(hang_watch::Station::Drain);
        // Lowered first: see [`PtyWakeSignal::accept`].
        hang_watch::during(hang_watch::Station::DrainWake, || {
            self.window.pty_wake.accept()
        });
        // **The panes whose shells have answered land first** (T-BIRTH-OFF-WINDOW), so a shell's
        // first bytes are drained on the turn it is born: its worker woke this window through the
        // pane's own wake, after publishing the answer.
        hang_watch::during(hang_watch::Station::PtyBirth, || self.land_births())?;
        let mut active_changed = false;
        let mut active_uncapped = false;
        let mut active_sync_closed = false;
        let mut active_sync_open = false;
        let mut active_bytes = 0_usize;
        let mut active_changed_off_focus = false;
        let mut chrome_changed = false;
        let mut moved = false;
        let mut rail_began = false;
        let mut command_ends: Vec<PathBuf> = Vec::new();
        let mut raised: Vec<AttentionDelivery> = Vec::new();
        // **The two window-wide bits of [`seat_holds_the_keyboard`], read once**
        // — this window has the desktop's keyboard, and what has it here is a
        // shell rather than a page, a files tree, a search capsule or a menu.
        // Read before the loop because the loop holds the tabs mutably, and
        // read *here*, on every turn, because that is what spares this from
        // being hung off a list of events: `Focused`, a tab switch, a seat
        // switch, a click into a preview and a menu opening are five different
        // messages and one question, and the question is asked again every turn
        // whatever woke it.
        //
        // Handed down as the separate facts they are rather than as a product,
        // so that the conjunction itself is written exactly once — in
        // [`seat_holds_the_keyboard`], where it can be read and pinned.
        //
        // **Asked of the window and not of `window_focused`, and the difference
        // is a measured defect** (2026-08-25, this machine). That field is the
        // strip's and the caret's copy: it is written on *transitions* and
        // seeded, in so many words, on the assumption that "a window is focused
        // when it opens". A window that opens without the keyboard — because the
        // desktop's foreground lock declined to hand it over, which is the
        // ordinary case for a window launched by a script — receives no
        // transition, because nothing transitioned; the assumption then stands
        // uncorrected for the window's whole life. Measured here: `folio.exe`
        // launched while another app held the foreground reported
        // `holds=true subscribed=true` once and never again, so its agent was
        // told `CSI I` and was never told otherwise — the exact belief this whole
        // report exists to correct, arrived at from the other side.
        //
        // The seed is not changed from here: it is the caret's and the strip's
        // and belongs to whoever is answering for those. What this reads instead
        // is winit's own answer, which is kept from `WM_SETFOCUS`/`WM_KILLFOCUS`
        // and starts out false rather than hopeful.
        // **What the marks rail is standing on, before the bytes that can move
        // it** (T-MAC-LIVE, §13.33 ②). The rail is the one thing on the glass
        // whose whole subject is the ledger, and until this ticket nothing in
        // the drain told the overlay that the ledger had moved: a frame
        // published for output presents the *retained* overlay, so a tick that
        // appeared — or turned red — waited for the next unrelated event.
        //
        // A sum rather than a list, and it costs no allocation: a revision
        // bumps and never falls, so a sum over a fixed set of seats moves if
        // and only if one of them moved, and the seats this reads are exactly
        // the ones [`Self::command_rail_layers`] draws a rail for. A seat that
        // arrives or leaves is a change to the tree, which already refreshes the
        // overlay on its own road.
        let marks_before = hang_watch::during(hang_watch::Station::DrainWatermark, || {
            self.command_marks_watermark()
        });
        // **Read, not asked** (ticket 48). The turn's head took this window's place once
        // (the one writer, in `frame.rs`), focus from the window itself for the reason above;
        // the drain decides from that reading and never asks the desktop about the same turn
        // again.
        let place = self.window.observed_place;
        let window_focused = place.focused;
        let switches = self.notification_switches();
        let owner_is_a_shell = self.keyboard_owner_is_a_shell();
        let active_tab = self.window.active_tab;
        // One instant for the whole drain, for the reason a frame takes one: the OSC lane below
        // stamps the panes a program spoke in (`attention` plan §11.10.4), and two tabs of one turn
        // sampled at two times would be two turns as far as that stamp is concerned.
        let now = Instant::now();
        // **Which tabs spoke, for the card column's clock** (§7.1.6b′, T-5). Kept
        // as indices because the question they are asked — is there a card of this
        // tab on screen? — is one only [`Self::focus_rail_geometry_now`] can
        // answer, and that cannot be reached from inside a loop holding the tabs.
        // `Vec::new` allocates nothing, and nothing is pushed at all unless the
        // column is up and its debt is not already standing.
        let mut spoke: Vec<usize> = Vec::new();
        let collect_speakers = self.collecting_card_speakers();
        // **The reading half of the turn, and it is the half that is on a clock**
        // (T-DRAIN-BURST).
        //
        // Every pane is asked for one [`bt_pty::TERM_READ_SLICE`], in tab order,
        // and then — if any of them still had more to say and the turn has spent
        // neither its [`DRAIN_TURN_BUDGET`] nor its [`DRAIN_SLICES_PER_TURN`] —
        // every pane is asked again. Round-robin rather than pane-at-a-time, so
        // that a pane with two hundred bytes to say is not behind a sibling's
        // quarter megabyte; the passes preserve each pane's own byte order
        // because [`bt_pty::OutputRing::try_pop`] is a queue.
        //
        // The clock is read once per pass, between two calls and never inside
        // one: a read already under way cannot be shortened, which is the whole
        // reason the slice is small. So a turn overruns its budget by at most
        // what one pass costs, and that is the number
        // [`bt_pty::TERM_READ_SLICE`] was chosen to bound.
        //
        // **What each tab said is gathered and acted on once**, below, rather
        // than on every pass. Two reasons, and the second is the ticket's:
        // `deliver_osc_attention` stamps the pane a program spoke in with the
        // turn's single `now`, so passes are not turns as far as it is
        // concerned; and `active_changed` is an *assignment* — a later pass that
        // drained nothing would otherwise erase the earlier pass that did. One
        // small allocation per turn buys both, against a turn that already
        // samples the window's placement through the kernel and publishes a
        // frame.
        let mut outcomes = vec![DrainOutcome::default(); self.window.tabs.len()];
        let mut slices_taken = 0_usize;
        // The *last* pass's answer and not the accumulated one: a pane that had
        // a leftover two passes ago and has since gone quiet owes this window
        // nothing, and a wake raised for it would be a turn that drains nothing
        // and publishes a frame nobody asked for.
        let drain_result = in_drain_feed_turn(
            &mut self.window.tabs,
            |tab| {
                for (_, leaf) in tab.leaves_mut() {
                    hang_watch::during(hang_watch::Station::DrainBegin, || {
                        leaf.session.begin_feed_turn()
                    });
                }
            },
            |tab| {
                hang_watch::during(hang_watch::Station::DrainSettle, || {
                    for (_, leaf) in tab.leaves_mut() {
                        leaf.session.end_feed_turn();
                    }
                });
            },
            |tabs| -> Result<bool> {
                let pending = loop {
                    let mut slice_pending = false;
                    for (index, tab) in tabs.iter_mut().enumerate() {
                        let outcome = drain_tab_pty(
                            tab,
                            window_focused,
                            index == active_tab,
                            owner_is_a_shell,
                        )?;
                        slice_pending |= outcome.pending;
                        outcomes[index].merge(outcome);
                    }
                    slices_taken += 1;
                    if !slice_pending || !drain_may_take_another_slice(slices_taken, now.elapsed())
                    {
                        break slice_pending;
                    }
                };
                Ok(pending)
            },
        );
        let pending = drain_result?;
        let drain_parent = hang_watch::enter(hang_watch::Station::DrainOutcomes);
        // **A shell that spoke is a stir for the web engine's warm-up** (ticket 54): the
        // warm-up waits for a stretch in which no window drained anything.
        if outcomes.iter().any(|outcome| outcome.bytes > 0) {
            self.app.web_warmup.stir(now);
        }
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            let outcome = &mut outcomes[index];
            // **The OSC lane's turn, on the turn the bytes arrived.** A standing request a program
            // wrote down its own tty becomes an episode here, a message it wrote beside one lends
            // that request its words, and a message on a pane with nothing standing is an event of
            // its own — see the function for why all of it belongs on this turn and in this order
            // rather than on the animation tick.
            deliver_osc_attention(
                tab,
                index,
                index == active_tab,
                place,
                switches,
                &mut self.window.attention_next_place,
                &outcome.notifications,
                now,
                attention_trace::global(),
                &mut raised,
            );
            if index == active_tab {
                active_changed = outcome.arrived;
                active_uncapped = outcome.arrived_uncapped;
                active_sync_closed = outcome.sync_bracket_closed;
                active_sync_open = outcome.sync_open;
                active_bytes = outcome.bytes;
                active_changed_off_focus = outcome.arrived_off_focus;
                // **Only the tab on screen** (R31): a Git page in a tab nobody is
                // looking at is not a surface looking at a repository, and the
                // first frame after that tab is switched to asks its own
                // questions anyway.
                command_ends = std::mem::take(&mut outcome.command_ends);
            }
            chrome_changed |= outcome.renamed;
            moved |= outcome.moved;
            rail_began |= outcome.rail_began;
            if collect_speakers && outcome.arrived {
                spoke.push(index);
            }
        }
        // **§7.1.6b′ T-5 — a card moves because its pane did, whatever shell is
        // inside it.**
        //
        // `arrived` is bytes that reached a screen, which is the same condition
        // `DualPlaneSession::feed_at` bumps `screen_revision` on — the very number
        // [`focus_thumb::FocusThumbnails`]'s damage gate keys a terminal seat to.
        // So this is the card's own damage key heard at the one place every leaf
        // of every tab passes through, and it is the whole of what schedules a
        // card's refresh from output. What used to carry a card past
        // [`Self::advance_strip_animation`]'s own gate was the shell: the tab-mark
        // breath, which only `OSC 133;C` starts, and the rename a prompt's
        // `OSC 0`/`OSC 7` causes — see [`focus_thumb::CardClock`] for the gate and
        // for what a shell that reports neither was left showing.
        //
        // Said through [`Self::panes_spoke`], which is the one place the judgement
        // lives: the timeout release in `finish_synchronized_update_if_due` moves
        // a pane's picture too, and two copies of "is this card on screen?" is how
        // two roads start disagreeing.
        self.panes_spoke(&spoke, now);
        // **A ring that still holds bytes is a turn this window owes itself.**
        //
        // [`drain_leaf_pty`] takes one slice from a pane and returns, and the
        // loop above stops going round at [`DRAIN_TURN_BUDGET`], so the rest of
        // a burst is answered by coming back — and coming back has to be
        // asked for here, because the thread that would otherwise ask is the
        // reader, and a reader with a full ring is blocked inside
        // [`bt_pty::OutputRing::push`] waiting for this very drain. Left to it,
        // the loop would go to `ControlFlow::Wait` with a pane mid-sentence.
        //
        // Raised *after* the drain and not before: [`PtyWakeSignal::accept`]
        // lowered the bit at the top of this function precisely so that bytes
        // arriving during the drain raise it again, and this is the same
        // sentence said for the bytes that were already there.
        if pending {
            self.window.pty_wake.raise();
        }
        hang_watch::during(hang_watch::Station::DrainRaiseAttention, || {
            self.raise_attention(raised)
        })?;
        // Raised before anything below can publish, because it is what that
        // publish's own gate reads. Only the tab on screen: a pane of a tab
        // nobody is looking at is not painted by any pass, and its backlog is
        // already kept by the ledger the strip's unread dot is drawn from.
        self.window.unpainted_pane_output |= active_changed_off_focus;
        // **A shell that moved is a session that changed.** `cwd` is a field of
        // every terminal leaf in `session.json`, and until now nothing about a
        // shell reporting a new one was a reason to write the file: the value on
        // disk was whatever had been reported at the last *structural* event —
        // a tab opened, a pin toggled, a split resized. For a tab that was
        // itself the last such event that value is the empty string, because the
        // save ran before the shell had said anything, and no later save
        // corrected it. Measured: a Command Prompt tab opened, left alone and
        // then closed persisted `"cwd": ""` while its own strip read `Alice`.
        //
        // This is §5.1's "有意义的变更" by its plain sense — the field is in the
        // file — and the debounce that ruling exists to provide is what makes a
        // per-prompt trigger cheap: reporting is once per prompt, the quiet
        // window is one to two seconds, so a burst of `cd`s still writes once.
        if moved {
            self.mark_session_dirty(Instant::now());
        }
        // **A pane that has just earned its rail makes room for it** (owner,
        // 2026-09-23: decoration never covers text). Once per pane lifetime —
        // `ShellSession::has_rail` never turns back — so this is at most one
        // extra grid change per pane, at its first prompt, when the screen is
        // nearly empty. Carried by the solve every other geometry change takes,
        // which re-derives every leaf's grid and changes only the one whose
        // answer moved: the pane on screen reflows now and tells its child at
        // the quiet boundary, a pane behind another tab does both there.
        //
        // **And the frame in the slot is re-composed at the grid the pane has now** (ticket 47),
        // as it is on every other road that re-solves the panes — a font change, a tab arriving,
        // a DPI correction all publish straight after. A frame composed before the reserve is a
        // frame of a grid the pane no longer has, and the resize present gate reads the pane's
        // grid as it stands: left in the slot for the drain's coalescing wait below, it is the
        // frame a redraw would take and the gate would refuse.
        let focused_grid = self.focused().map(|leaf| leaf.grid);
        if rail_began {
            self.resize_leaves_to_layout(now, "reserve the command rail's room")?;
            if self.focused().map(|leaf| leaf.grid) != focused_grid {
                self.publish_frame(FrameTrigger {
                    occurred_at: now,
                    source: FrameSource::Expose,
                })?;
            }
        }
        // **R31's third invalidation moment, A: a command finished.** A shell
        // standing inside a repository this tab is showing has just done
        // something, and asking git is the only way to find out what — no
        // watcher, no timer, and nothing at all for a command that ended
        // somewhere else. See [`git_surfaces_wanting_reread`].
        if !command_ends.is_empty() {
            hang_watch::during(hang_watch::Station::DrainGit, || {
                self.reread_git_surfaces(Some(&command_ends))
            })?;
        }
        if chrome_changed {
            // Said, not written: the turn writes it once, after every pass that can
            // change it (`Runtime::flush_title`, ticket 49).
            self.want_title();
            self.refresh_chrome();
            if !active_changed {
                self.present_chrome_change()?;
            }
        }
        // **A tick is not a cell** (§13.33 ②). Everything below this line
        // publishes a *terminal frame*, and the rail is not in one: it is an
        // overlay layer, retained in the renderer between frames and rebuilt
        // only by whoever says it has changed. Said here, beside the name
        // change above and on the same terms — the drain is the one place the
        // bytes that move the ledger arrive, and asking on every turn whether
        // the ledger moved is two `u64`s.
        //
        // Until this ticket the only thing that happened to ask was luck: an
        // `OSC 133;C` starts the tab mark's breath, the breath turns
        // `advance_command_rails` every twenty milliseconds, and a `D` that
        // landed while that was still running was drawn at once. A `D` that
        // landed after it had stopped — every command that runs longer than the
        // breath, which is every command anybody watches a rail for — was not
        // drawn until the reader moved the pointer or pressed a key.
        if hang_watch::during(hang_watch::Station::DrainWatermark, || {
            self.command_marks_watermark()
        }) != marks_before
            && self.refresh_overlay()
        {
            self.present_chrome_change()?;
        }
        if active_changed {
            hang_watch::during(hang_watch::Station::DrainPublish, || -> Result<()> {
                let now = Instant::now();
                // **On arrival, and before anything decides when to draw.** Output is the event the
                // caret answers to, so the blink is reset the moment the bytes land, exactly as it
                // was before the wait existed. Doing it in the publish branch instead left the caret
                // dark whenever some other publication — a decoration result, an expose — settled
                // the burst before the wait ran out, because that publication knows nothing about a
                // caret and the release then had nothing left to do. Three milliseconds of drawing
                // is not worth a second field to carry this across.
                let cursor_revealed = self.reset_cursor_blink(now);
                let opened = self.window.pty_coalesce.opened_at(now);
                // **The one branch this rule adds.** Everything downstream of
                // [`Self::publish_pty_drain_frame`] is untouched: what changes is only *when* this
                // turn's picture is composed, and only when the kernel has said there is more of it
                // on the way. See [`crate::coalesce`] for why that is a fact about the transfer and
                // not a guess about the bytes.
                //
                // The vendor parser withholds bytes inside an open DEC 2026 block, so projecting
                // here cannot expose its intermediate state. It can expose ordinary output before a
                // trailing BSU or a completed update before the next BSU; the unchanged-frame gate
                // in publish_frame cheaply suppresses drains containing only still-buffered sync
                // bytes.
                let arrival = coalesce::Arrival {
                    ends_capped: !active_uncapped,
                    ring_pending: pending,
                    sync_open: active_sync_open,
                    sync_closed: active_sync_closed,
                    first_unpublished: Some(opened),
                    // **Nothing supplies one.** A `Fifo` surface does not say when the display will
                    // next take a frame, and this window keeps no frame pacer, so the only bound in
                    // force is the timer. The input stays because it is the shape a pacer will need
                    // the day there is one — not because it is doing anything today.
                    next_display_deadline: None,
                };
                let decision = coalesce::decide(arrival, now, coalesce::COALESCE_WINDOW);
                hang_watch::during(hang_watch::Station::DrainTrace, || {
                    self.trace_drain(slices_taken, active_bytes, &arrival, decision, opened, now);
                });
                match decision {
                    coalesce::Publication::WaitUntil(until) => {
                        self.window.pty_coalesce.until = Some(until);
                    }
                    coalesce::Publication::Now => {
                        self.publish_pty_drain_frame(now, cursor_revealed)?;
                    }
                }
                Ok(())
            })?;
        }
        hang_watch::at(drain_parent);
        Ok(())
    }

    /// **The deferred publication, when its wait runs out.**
    ///
    /// A sibling of [`Self::finish_synchronized_update_if_due`] and driven from the same place
    /// for the same reason: it is the other path on which a pane's picture moves with no byte
    /// passing through the drain. It is also the whole of [`coalesce::Pending`]'s invariant —
    /// this runs on every turn, so a deadline that has passed is published and disarmed on the
    /// next wake whatever else the window has been doing.
    pub(in crate::runtime) fn finish_pty_coalesce_if_due(&mut self, now: Instant) -> Result<()> {
        let Some(until) = self.window.pty_coalesce.until else {
            return Ok(());
        };
        if now < until {
            return Ok(());
        }
        self.window.pty_coalesce.until = None;
        let cursor_revealed = self.reset_cursor_blink(now);
        self.publish_pty_drain_frame(now, cursor_revealed)
    }

    /// **How far every rail on screen has been told its pane's ledger got**
    /// (§13.33 ②) — the one number the drain compares itself against.
    ///
    /// It is the sum of the seats' own revisions, and the arithmetic is honest
    /// rather than clever: [`bt_term::DualPlaneSession::command_marks_revision`]
    /// "bumps on every ledger change and on nothing else", never falls, so a sum
    /// over one set of seats is strictly greater exactly when one of them has
    /// moved. Wrapping because a `u64` of ledger changes is not a number this
    /// program will reach, and a panic in the drain would be a worse answer than
    /// one missed repaint in the year 292277026596.
    ///
    /// The seats are `self.sessions` — the tab on screen — because those are the
    /// seats [`Self::command_rail_layers`] lays a rail out for. A pane in a tab
    /// nobody is looking at owes the glass nothing, and the frame drawn when
    /// that tab is switched to asks its own questions.
    fn command_marks_watermark(&self) -> u64 {
        self.sessions
            .values()
            .fold(0u64, |sum, crate::TerminalPane { session: leaf, .. }| {
                sum.wrapping_add(leaf.session.command_marks_revision())
            })
    }

    pub(in crate::runtime) fn hyperlink_hit(
        &self,
        hit: bt_render::GridHit,
    ) -> Option<HyperlinkHit> {
        // Asked of the pane the pointer is in, so the link that lights up is the
        // link under the hand.
        let (_, _, frame) = self.pane_hit_context()?;
        frame.hyperlink_at(hit.row, hit.column)
    }

    pub(crate) fn clear_selection(&mut self) {
        self.clear_pane_selection(self.focused_leaf);
    }

    /// A tab with no shell has no transcript to scroll: `Shift+PageUp` in a
    /// folder tab moves nothing, because there is nothing behind the column that
    /// scrolls in rows of cells (§7.1.6h).
    pub(in crate::runtime) fn scroll_view(&mut self, rows: i32) -> Result<()> {
        if self.focused().is_none() {
            return Ok(());
        }
        // A band still changing face lands before the view moves under it (§7.1.5p ⑪): two
        // motions on one surface is not what the ruling asked for, and the end state is what a
        // change is carried into a new view as.
        self.settle_math_toggle()?;
        let leaf = self.shell_pane_mut();
        let subpixels =
            i64::from(rows).saturating_mul(leaf.view.projection.cell_height_subpixels().get());
        leaf.view.projection.scroll_by_subpixels(subpixels);
        // `Shift`+`PageUp`/`PageDown` is a scroll like any other, so it lights
        // the bar like any other (P2-9 slice 1).
        self.woke_terminal_thumb(self.focused_leaf)?;
        self.publish_interaction_frame()
    }

    /// Begin a selection in the pane the press landed in — `seat`, and not the
    /// focused leaf, even though D40 has just made them the same pane.
    pub(in crate::runtime) fn begin_local_selection(
        &mut self,
        owner: PasteTarget,
        hit: bt_render::GridHit,
    ) -> Result<()> {
        let seat = owner.seat;
        self.dismiss_peek()?;
        let count = self
            .window
            .click_tracker
            .register(hit.row, hit.column, Instant::now());
        let local_image_path = self.local_image_path_hit(hit);
        let Some(frame) = self.pane_frame(seat) else {
            return Ok(());
        };
        let hyperlink = frame.hyperlink_at(hit.row, hit.column);
        // **The press asks what a pointer move asks** (closure review B-1). A link that arrived
        // under a resting pointer — a wheel scroll, a fresh line of output — was never asked
        // about, so the press found no verdict and the table answered `None`: a click that did
        // nothing, however many times it was repeated. Asked here, on the way *down*, so the
        // worker's answer has a whole click to land in and the release usually acts on it; and if
        // it has not landed, the link is armed and the next click always works. Nothing is read
        // from a disk on this thread to make that true — that is the defect this branch repaired.
        let asked_about = hyperlink.as_ref().map(|link| link.uri.clone());
        // **Both references read the one hand-over modifier** (§13.45 ①) — the
        // link's and the picture's, which is `ClickIntent`'s own argument said
        // one level out: `Ctrl` here, `⌘` on a Mac, and never twice.
        let hand_over = input::pointer_chord_held(self.window.modifiers_held);
        let hyperlink_control = hand_over;
        let local_image_activation =
            local_image_activation(hand_over, true, local_image_path.as_deref());
        // Local hits are clamped to a continuous frame, which supplies anchors for every grid cell.
        let (mode, origin, initial) = match count {
            2 => {
                let selection = frame
                    .word_selection(hit.row, hit.column)
                    .context("reject non-rectangular frame during word selection")?
                    .context("word selection hit has no anchor")?;
                (SelectionDragMode::Word, selection.clone(), Some(selection))
            }
            3 => {
                let selection = frame
                    .line_selection(hit.row)
                    .context("reject non-rectangular frame during line selection")?
                    .context("line selection hit has no anchor")?;
                (SelectionDragMode::Line, selection.clone(), Some(selection))
            }
            _ => {
                let start = frame
                    .anchor_at(hit.row, hit.column, Bias::Before)
                    .context("reject non-rectangular frame during anchor lookup")?
                    .context("selection hit has no start anchor")?;
                let end = frame
                    .anchor_at(hit.row, hit.column, Bias::After)
                    .context("reject non-rectangular frame during anchor lookup")?
                    .context("selection hit has no end anchor")?;
                (
                    SelectionDragMode::Linear,
                    ViewSelection {
                        start: start.clone(),
                        end,
                    },
                    None,
                )
            }
        };
        // A linear press begins a possible drag but owns no selection yet. Only movement creates
        // one, so click-no-drag cannot briefly feed copy-on-select or leave a zero-width selection.
        self.set_pane_view_selection(seat, initial);
        if let Some(uri) = asked_about {
            // **And the press is the click's own re-check** (closure re-review B-1'). A "yes" is
            // never re-asked by the ordinary door, on the written argument that a link which
            // turns out to be gone is caught by the click — an argument that used to be true
            // because the reveal stat-ed the target on this thread. It does not any more, so the
            // press puts the question again even when the pane holds an answer: one per press,
            // and the release acts on whichever of the two came back.
            self.re_ask_the_worker_about_a_link_target(seat, &uri);
        }
        self.latch_selection_route(MouseRoute::Local(Box::new(SelectionDrag {
            mode,
            owner,
            origin_row: hit.row,
            origin_column: hit.column,
            origin,
            hyperlink,
            hyperlink_control,
            local_image_activation,
        })));
        // The route the press armed, and what it promised (`BT_MOUSE_TRACE`).
        // Read back out of the route rather than off the locals, so what the
        // trace reports is what the release will actually find.
        self.mouse_trace(|| {
            let Some(MouseRoute::Local(drag)) = self.held_mouse_route() else {
                return "begin_local_selection route=missing".to_owned();
            };
            format!(
                "begin_local_selection route=local seat={:?} origin={},{} mode={:?} control={} hyperlink={:?} image={:?}",
                drag.owner.seat,
                drag.origin_row,
                drag.origin_column,
                drag.mode,
                u8::from(drag.hyperlink_control),
                drag.hyperlink,
                drag.local_image_activation,
            )
        });
        self.publish_interaction_frame()
    }

    /// Carry the selection to where the pointer is now, in the pane the drag began
    /// in and in no other.
    ///
    /// The cell is re-read here rather than passed in: the caller knows where the
    /// pointer is, but only the drag knows which pane's cells that point has to be
    /// spoken in, and [`Self::drag_hit_in_pane`] is the one place that translation
    /// is written.
    pub(in crate::runtime) fn extend_local_selection(&mut self) -> Result<()> {
        let Some(MouseRoute::Local(drag)) = self.held_mouse_route() else {
            return Ok(());
        };
        // Copied out field by field rather than by cloning the drag whole: this
        // runs on every pointer move of a gesture, and the anchors are the only
        // part of it with anything to clone.
        let (mode, owner, origin_row, origin_column) =
            (drag.mode, drag.owner, drag.origin_row, drag.origin_column);
        let origin = drag.origin.clone();
        // A selection whose shell is gone is let go, not carried on in
        // whatever pane wears its seat now.
        if self.live_paste_target(owner).is_none() {
            self.drop_mouse_route();
            return Ok(());
        }
        let seat = owner.seat;
        let Some(hit) = self.drag_hit_in_pane(seat) else {
            return Ok(());
        };
        if matches!(mode, SelectionDragMode::Linear)
            && hit.row == origin_row
            && hit.column == origin_column
        {
            return Ok(());
        }
        let Some(frame) = self.pane_frame(seat) else {
            return Ok(());
        };
        let current = match mode {
            SelectionDragMode::Linear => ViewSelection {
                start: frame
                    .anchor_at(hit.row, hit.column, Bias::Before)
                    .context("reject non-rectangular frame during drag anchor lookup")?
                    .context("drag hit has no start anchor")?,
                end: frame
                    .anchor_at(hit.row, hit.column, Bias::After)
                    .context("reject non-rectangular frame during drag anchor lookup")?
                    .context("drag hit has no end anchor")?,
            },
            SelectionDragMode::Word => frame
                .word_selection(hit.row, hit.column)
                .context("reject non-rectangular frame during word drag")?
                .context("word drag hit has no anchor")?,
            SelectionDragMode::Line => frame
                .line_selection(hit.row)
                .context("reject non-rectangular frame during line drag")?
                .context("line drag hit has no anchor")?,
        };
        let after_origin = (hit.row, hit.column) >= (origin_row, origin_column);
        let next = if after_origin {
            ViewSelection {
                start: origin.start,
                end: current.end,
            }
        } else {
            ViewSelection {
                start: current.start,
                end: origin.end,
            }
        };
        self.set_pane_view_selection(seat, Some(next));
        self.publish_interaction_frame()
    }

    /// This window's inputs to [`terminal_link_answers_a_press`], read from the
    /// same [`HyperlinkHover::underline_target`] the underline is painted from —
    /// so the shape and the mark cannot come to disagree about which cells are a
    /// link, nor the shape and the verb about what that link would do.
    pub(in crate::runtime) fn terminal_link_grasp(&self) -> bool {
        let namespace = self.hovered_pane_path_namespace();
        let namer = bt_transcript::paths::PathNamer::Pane(&namespace);
        terminal_link_answers_a_press(
            // The finger is a promise about *this* press, so it is struck
            // against the same modifier the press will read (§13.45 ①).
            input::pointer_chord_held(self.window.modifiers_held),
            self.window
                .hyperlink_hover
                .underline_target()
                .map(|hyperlink| hyperlink.uri.as_str()),
            namer,
            &|path| self.hovered_pane_path_verdict(path),
        )
    }

    /// Drive a drag one pointer move. Returns whether the pointer was consumed.
    ///
    /// A drag owns the pointer outright, exactly as a divider drag does: while
    /// one is in flight nothing below hears the move, so no hover lights up, no
    /// tooltip arms and no selection extends underneath it.
    ///
    /// Three steps, in the mock-up's own order (6753-6790): move the ghost, ask
    /// what is under the pointer, then let the answer do its live half. The
    /// ghost moves *first* and unconditionally, because it is the report on where
    /// the hand is and a hand that has moved has moved whether or not anything is
    /// willing to receive it.
    /// This window's client origin in screen physical pixels — the one number a
    /// pointer has to be added to before two windows can talk about it
    /// (multiwindow slice F2).
    ///
    /// `inner_position` and not the outer rectangle: winit reports every pointer
    /// in client coordinates, and this window's `WM_NCCALCSIZE` has made client
    /// and outer the same rectangle anyway ([`bt_platform::CustomWindowFrame`]),
    /// so the two are one origin here and only one of them stays right if that
    /// ever stops being true.
    fn client_origin_on_screen(&self) -> Option<(f64, f64)> {
        let (x, y) = self.window.window.client_origin()?;
        Some((f64::from(x), f64::from(y)))
    }

    /// A pointer of this window's, in screen physical pixels.
    pub(crate) fn to_screen(&self, position: PhysicalPosition<f64>) -> Option<(f64, f64)> {
        let (x, y) = self.client_origin_on_screen()?;
        Some((x + position.x, y + position.y))
    }

    /// A screen point, in this window's client physical pixels.
    pub(crate) fn screen_to_client(&self, point: (f64, f64)) -> Option<PhysicalPosition<f64>> {
        let (x, y) = self.client_origin_on_screen()?;
        Some(PhysicalPosition::new(point.0 - x, point.1 - y))
    }

    /// Let go of a selection drag: settle its last extent, then spend what the
    /// press promised — a click's dismissal, a drag's copy, a Ctrl+click's link.
    ///
    /// Everything here is asked of `drag.owner.seat`. The release's own cell is
    /// read only when the button truly came up inside that pane, because every
    /// question below is "did it come up on the cell it went down on", and a point
    /// in another pane — or on the chrome — cannot answer that yes. Letting a
    /// neighbour's cell answer would let a release two panes away read as a click
    /// on the origin cell and quietly clear the selection the drag just made.
    pub(in crate::runtime) fn finish_local_selection(&mut self, drag: SelectionDrag) -> Result<()> {
        let seat = drag.owner.seat;
        self.extend_local_selection()?;
        let release_hit = self
            .pane_frame_hit()
            .filter(|(at, _)| *at == seat)
            .map(|(_, hit)| hit);
        let release_hyperlink = release_hit.and_then(|hit| self.hyperlink_hit(hit));
        let release_local_image_path = release_hit.and_then(|hit| self.local_image_path_hit(hit));
        let SelectionDrag {
            mode,
            hyperlink,
            hyperlink_control,
            local_image_activation,
            origin_row,
            origin_column,
            ..
        } = drag;
        let (single_click, hyperlink_to_open, local_image_action) =
            if matches!(mode, SelectionDragMode::Linear)
                && release_hit
                    .is_some_and(|hit| (origin_row, origin_column) == (hit.row, hit.column))
            {
                (
                    true,
                    // Borrowed and then cloned on the way out, rather than moved
                    // in: the press's own hit stays readable below, where
                    // `BT_MOUSE_TRACE` reports it beside the release's. The
                    // clone happens only where a link is actually opening.
                    hyperlink
                        .as_ref()
                        .filter(|pressed| release_hyperlink.as_ref() == Some(*pressed))
                        .cloned(),
                    local_image_activation
                        .path()
                        .is_some_and(|pressed| release_local_image_path.as_deref() == Some(pressed))
                        .then(|| local_image_activation.clone()),
                )
            } else {
                (false, None, None)
            };
        // **The station the cross-monitor report is really about**
        // (`BT_MOUSE_TRACE`). Three separate things can turn a click on a link
        // into nothing here, and until this line they were indistinguishable
        // from the outside: the release landing in another pane (`release=none`),
        // the release landing on a different cell (`single_click=0`), and the
        // two hyperlink hits not comparing equal (`same_link=0`) — the last of
        // which includes a re-scan between press and release having minted new
        // anchors for the same address.
        self.mouse_trace(|| {
            format!(
                "finish_local_selection seat={seat:?} origin={origin_row},{origin_column} release={} \
                 single_click={} press_link={:?} release_link={:?} same_link={} opens_link={} control={} image={:?}",
                release_hit.map_or_else(
                    || "none".to_owned(),
                    |hit| format!("{},{}", hit.row, hit.column)
                ),
                u8::from(single_click),
                hyperlink,
                release_hyperlink,
                u8::from(hyperlink.as_ref() == release_hyperlink.as_ref()),
                u8::from(hyperlink_to_open.is_some()),
                u8::from(hyperlink_control),
                local_image_action,
            )
        });
        let copy_on_select = should_copy_on_select_release(
            self.held_mouse_route(),
            single_click,
            self.app.settings_store.loaded().copy_on_select,
        );
        self.drop_mouse_route();
        if single_click {
            self.clear_pane_selection(seat);
            self.publish_interaction_frame()?;
        } else if copy_on_select {
            self.copy_selection_on_release(seat)?;
        }
        if let Some(hyperlink) = hyperlink_to_open {
            self.activate_hyperlink(seat, hyperlink, hyperlink_control)?;
        }
        if let Some(activation) = local_image_action {
            match activation {
                LocalImageActivation::None => {}
                LocalImageActivation::Preview(path) => self.open_preview_image(path)?,
                LocalImageActivation::External(path) => self.activate_local_image_path(&path)?,
            }
        }
        Ok(())
    }

    /// Local pixel-exact wheel consumption: accumulate the event's fractional subpixels and
    /// scroll by the integral part. Positive subpixels move into history, matching the wheel's
    /// upward direction, and residue below one subpixel simply waits for the next event.
    /// Scroll one pane's view by an exact subpixel amount.
    ///
    /// The seat is named rather than assumed because the wheel belongs to the
    /// pane under the pointer, which is not always the pane under the keyboard.
    /// The remainder accumulator stays window-wide: it holds the fraction of a
    /// subpixel one physical notch left over, and a notch is a property of the
    /// mouse, not of the pane it landed on.
    /// Returns whether the clamped view moved, independently of thumb changes.
    pub(in crate::runtime) fn scroll_view_exact_in(
        &mut self,
        seat: bt_layout::SeatId,
        event_subpixels: f64,
    ) -> Result<bool> {
        self.window.local_wheel_subpixel_remainder += event_subpixels;
        let take = drain_whole_units(&mut self.window.local_wheel_subpixel_remainder, 1.0);
        if take == 0 {
            return Ok(false);
        }
        // A notch lands a band that is still changing face, for `scroll_view`'s reason.
        self.settle_math_toggle()?;
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(false);
        };
        let before = leaf.view.projection.scroll_offset_subpixels();
        leaf.view.projection.scroll_by_subpixels(take);
        let moved = leaf.view.projection.scroll_offset_subpixels() != before;
        self.repaint_pane_change_inner(seat, Some(moved))?;
        // A notch is a reason for the bar to be up, and a moved view is a moved
        // thumb: the overlay is built on demand, so a wheel that only
        // republished the pane would slide the text under a mark that stayed
        // where it was (P2-9 slice 1). Publish the view first so the thumb can
        // share that frame; at a clamp a changed fade still owes its own frame.
        self.woke_terminal_thumb(seat)?;
        Ok(moved)
    }

    /// What the foot bar of `seat` is owed on the glass right now.
    ///
    /// **One derivation, three consumers** — the painter, the hover, and the
    /// press. A press that could take a mark the painter had not drawn is the
    /// same defect [`crate::preview::ScrollBar`] exists to prevent, read in time rather
    /// than in space: you cannot grab what is not there. So the visibility rule
    /// is asked once and everything that cares reads the answer.
    fn terminal_column_thumb(&self, seat: SeatId, now: Instant) -> termscroll::Thumb {
        let Some(leaf) = self.sessions.get(&seat) else {
            return termscroll::Thumb::Hidden;
        };
        let situation = termscroll::ThumbSituation {
            // "Has a horizontal axis" is what `has_history` means down here: a
            // pane whose widest line fits across it has nothing to be a picture
            // of, exactly as a pane with no scrollback has nothing above it.
            has_history: leaf.view.projection.horizontal().max_x_origin().0 > 0,
            alternate_screen: leaf.session.session.terminal_modes().alternate_screen,
            scrolled: leaf.view.projection.horizontal().is_scrolled(),
            near: self.terminal_column_hover == Some(seat),
            held: self
                .held_terminal_column_drag()
                .copied()
                .is_some_and(|drag| drag.seat == seat),
            since_rest: now.saturating_duration_since(leaf.view.column_awake),
        };
        termscroll::column_visibility(situation, self.app.motion, |x| cubic_bezier(x, EASE))
    }

    /// **The same question, put again although the pane already holds an answer** (closure
    /// re-review B-1') — the press's door, and the press's alone.
    ///
    /// Everything [`Self::ask_the_worker_about_a_link_target`] refuses, this refuses too: the
    /// lexical gate, and a question already queued or already out with a worker. What it does not
    /// skip is a verdict, because a held "yes" is exactly the thing a press has to re-check now
    /// that no reveal stats its target.
    fn re_ask_the_worker_about_a_link_target(&mut self, seat: SeatId, uri: &str) {
        let namespace = self.seat_path_namespace(seat);
        let Some(path) =
            link_target_to_ask_about(uri, bt_transcript::paths::PathNamer::Pane(&namespace))
        else {
            return;
        };
        if let Some(crate::TerminalPane { session: leaf, .. }) = self.sessions.get_mut(&seat) {
            leaf.session.re_ask_about_link_target(path);
        }
    }

    /// The flat index [`RowHost::Terminal`] carries, for a hit on `seat`'s own
    /// frame — the one place `row * columns + column` is spelled, so the writer
    /// and every reader of that number agree about which way round it goes.
    pub(crate) fn reference_cell_index(
        &self,
        seat: SeatId,
        hit: bt_render::GridHit,
    ) -> Option<u32> {
        let columns = self.pane_frame(seat)?.columns.get();
        hit.row.checked_mul(columns)?.checked_add(hit.column)
    }

    /// Pay the flash's frames, and let it go when it is over.
    pub(crate) fn advance_command_flash(&mut self, now: Instant) -> Result<()> {
        if self.window.command_flash.is_none() {
            return Ok(());
        }
        // On the window's own display frame — see [`Self::animation_frame_is_due`].
        if !self.animation_frame_is_due() {
            return Ok(());
        }
        let Some(flash) = self.window.command_flash.as_ref() else {
            return Ok(());
        };
        if !cmdrail::flash_is_running(now.saturating_duration_since(flash.started)) {
            self.window.command_flash = None;
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether it is worth writing down which tabs spoke on this turn.
    ///
    /// A window outside the mode has no cards to move, and one already owed a
    /// frame is going to draw them anyway — so both gather nothing at all, which
    /// is what keeps this off the price of an ordinary drain.
    pub(crate) fn collecting_card_speakers(&self) -> bool {
        self.window.focus_mode && !self.window.cards.owes_frame()
    }

    /// **The reader's own bytes went into this pane** (review row R1-24).
    ///
    /// Called from the four doors a person's own input reaches a shell through —
    /// the keyboard, a paste, an IME commit and a files-row insert — and from
    /// nowhere else. A wheel forwarded to a full-screen program is not somebody
    /// typing at a prompt, and neither is a terminal protocol reply, a resize
    /// repair chord, or the one line a restored pane puts back on the prompt
    /// itself.
    ///
    /// What it buys is on `CommandMark::typed_by_user`: the command text this
    /// window remembers across a restart is read off the screen, and a program
    /// printing `OSC 133` marks decides what is on the screen. This is the one
    /// fact about a command that such a program cannot produce.
    pub(crate) fn note_user_typing(&mut self, seat: SeatId) {
        let active = self.window.active_tab;
        if let Some(crate::TerminalPane { session: leaf, .. }) =
            self.window.tabs[active].sessions.get_mut(&seat)
        {
            leaf.session.note_user_input();
        }
    }

    /// The same, followed by the rebuild a moved thumb needs.
    ///
    /// The overlay is built on demand rather than every frame, so a scroll that
    /// only republished the pane would move the text under a thumb that stayed
    /// where it was. `present_chrome_change` costs nothing when a frame is
    /// already pending, which is the usual case on the wheel path.
    pub(crate) fn woke_terminal_thumb(&mut self, seat: SeatId) -> Result<()> {
        self.wake_terminal_thumb(seat);
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Note that this pane's bar has a reason to be up **now** — the moment its
    /// rest is later measured from.
    ///
    /// Every gesture that moves the view says so through here: the wheel, the
    /// keyboard's pages and ends, a jump off the rail, a drag of the thumb
    /// itself and the pointer merely being in the lane. One door, because the
    /// one thing that must not happen is a gesture that scrolls the pane and
    /// leaves its picture of the scroll invisible.
    pub(crate) fn wake_terminal_thumb(&mut self, seat: SeatId) {
        let active = self.window.active_tab;
        if let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) {
            leaf.view.thumb_awake = Instant::now();
        }
    }

    /// A tab with no shell has no live view to return to and no selection to
    /// drop: nothing here is true of it, so nothing changed (§7.1.6h).
    pub(crate) fn return_to_live_for_input(&mut self) -> bool {
        if self.focused().is_none() {
            return false;
        }
        let leaf = self.shell_pane_mut();
        let changed = leaf.session.session.view_selection().is_some()
            || leaf.view.projection.is_scrolled()
            || leaf.session.session.port_release_presentation_hold();
        self.clear_selection();
        self.shell_pane_mut().view.projection.scroll_to_bottom();
        changed
    }
}

/// **The one door every byte this window sends a child goes through** (window-thread
/// unbounded-call sweep, 2026-08-24).
///
/// Every caller of this is on the window thread — a keystroke, a mouse report, an IME commit, a
/// terminal reply the program asked for, a paste, the PSReadLine anchor chord. Until this
/// existed each of them reached `PtySession::write` on its own, and that call was a `write_all`
/// straight onto the ConPTY input pipe: it blocks when the pipe is full, and the pipe is full
/// exactly when conhost has stopped draining it, which is the state a flooding pane puts conhost
/// in. `bt_pty::InputRing` is where that wait went and why; what belongs here is the other half
/// of the contract — **what a window does about a child that has stopped listening.**
///
/// Not an error, and the distinction is the whole reason this is a function. A refusal is a fact
/// about one shell; `?`-ing it out of a keystroke handler would reach `App::fail` and end a
/// process holding every other pane in the window. So it is said and the window carries on, with
/// the numbers in the line, because a shell sitting on a megabyte of unread input is a thing the
/// reader may want to know about the shell rather than a thing to be silent about.
///
/// A leaf with no child at all (`BT_PROBE_INPUT`, a restored pane whose shell is gone) is not a
/// refusal and not an error: there is nowhere for the bytes to go and never was.
///
/// A leaf whose shell is being born keeps them for that shell ([`crate::PaneBirth`]).
pub(crate) fn write_pty_input(pty: PtyTarget<'_>, bytes: &[u8], what: &'static str) -> Result<()> {
    offer_pty_input(pty, bytes, what).map(drop)
}

/// How much this shell has *said*, one drain on.
///
/// This is the number "unread" is measured against, and what it counts is the
/// only thing a badge asking for attention can honestly mean: **the program
/// wrote something into this pane.** It is deliberately not the session's
/// `published_revision`, which counts frames put on the glass — a cursor blink,
/// a hovered link, a chrome repaint and a tab switch all publish frames while
/// the shell sits in silence, and every one of them used to read as news.
///
/// The one exclusion is a reprint *we* asked for. Between the moment a leaf is
/// told its new size and the moment its resize transaction goes quiescent, the
/// bytes coming back are the shell answering our question — a prompt redrawn at
/// a new width is not the program speaking. Bounding the exemption by the
/// transaction rather than by a timer is what keeps it from swallowing anything
/// else: outside that window every byte counts again.
///
/// The tradeoff is stated rather than hidden: real output that a background
/// shell happens to print *inside* its own resize window is not counted, so a
/// tab that prints only during those few hundred milliseconds keeps a clean
/// badge. That is the honest price of not lighting every tab on every window
/// resize, and it is the smaller of the two errors — the exemption is bounded by
/// a transaction the user themselves started.
pub(crate) fn output_revision(
    previous: u64,
    output_arrived: bool,
    resize_transaction_open: bool,
) -> u64 {
    if output_arrived && !resize_transaction_open {
        previous.saturating_add(1)
    } else {
        previous
    }
}

/// The colours a program inside any pane of this process is standing on.
///
/// Nineteen colours and the canvas they make, assembled here because this is the
/// only crate that can see both halves: `bt-render` owns the scheme and the one
/// luma threshold that sorts dark from light, `bt-term` owns the escape-code
/// contract that spells the answer. Neither can reach the other, and neither
/// should — a terminal that derived its own dark/light verdict would be a second
/// threshold waiting to disagree with the one the window paints by.
///
/// The background and the ink are read from the *painted* canvas rather than off
/// the scheme, because a `BT_BG` override moves the glass without moving the
/// settings, and what a program asks about is the glass.
fn terminal_palette_in_force() -> TerminalPalette {
    terminal_palette(scheme_in_force(), background_rgb(), foreground_rgb())
}

/// The mapping alone, so it can be pinned without moving process-wide state.
pub(crate) fn terminal_palette(
    scheme: bt_render::ColourScheme,
    background: [u8; 3],
    foreground: [u8; 3],
) -> TerminalPalette {
    TerminalPalette {
        canvas: if bt_render::background_is_light(background) {
            TerminalCanvas::Light
        } else {
            TerminalCanvas::Dark
        },
        background,
        foreground,
        cursor: scheme.cursor,
        ansi: scheme.ansi,
    }
}

/// **Did this pane commit a synchronized update while it was being fed?**
///
/// `before` is [`bt_term::DualPlaneSession::synchronized_update_commits`] read on the near side of
/// the feed. A counter and not the deadline, and the difference is a whole class of block: one
/// that opens *and* commits inside a single `read(2)` — `BSU`, a screenful, `ESU`, all under the
/// pty's 1,024-byte cap — leaves the deadline `None` on both sides of that feed, so a drain
/// sampling the deadline sees nothing happen and makes a completed frame wait. The count cannot
/// fall between two samples.
///
/// A named function rather than a comparison written in place, so that this is one thing a test
/// can put a real session through — see `coalesce::session_tests`.
pub(crate) fn synchronized_update_committed(
    before: u64,
    session: &bt_term::DualPlaneSession,
) -> bool {
    session.synchronized_update_commits() != before
}

/// Drain one shell's pipe into its own screen.
///
/// "Name" and not "title", and the difference was a live bug. This used to watch
/// `window_title` alone, which is one of the two things [`crate::resolve_title`] reads
/// off a shell; the other is the folder it reported over OSC 7, and that is the
/// layer that names most tabs most of the time. A `cd` therefore moved a tab's
/// name without marking the chrome for redrawing, and the strip went on showing
/// the previous folder until some unrelated event — a click, a resize, a tab
/// switch — repainted it. Measured on a Command Prompt pane: `cd` into
/// `C:\Program Files` left the tab reading `BetterTerminal` while the prompt one
/// line below it read `C:\Program Files>`, and it stayed that way indefinitely.
///
/// It was invisible while `window_title` was the layer that named things, and
/// every profile has it: a PowerShell tab is folder-named too. What made it
/// surface now is Command Prompt, whose only report *is* the folder.
///
/// `holds_the_keyboard` is [`seat_holds_the_keyboard`] already answered for this
/// pane — the four bits are the window's and the tab's, and a leaf can supply
/// none of them about itself.
pub(crate) fn drain_leaf_pty(
    leaf: &mut ShellSession,
    holds_the_keyboard: bool,
) -> Result<DrainOutcome> {
    if leaf.life.pty().is_none() {
        return Ok(DrainOutcome::default());
    }
    let name_before = leaf.name_evidence();
    // Told on every turn rather than at each of the half-dozen places a theme,
    // a scheme or a scheme *file* can change, because a list of those places is
    // a list somebody has to remember to extend. The call is a comparison and a
    // nineteen-colour copy; it queues bytes only when the canvas actually moved
    // and only for a pane that asked to hear about it (DEC 2031).
    hang_watch::during(hang_watch::Station::DrainPalette, || {
        leaf.session.set_color_palette(terminal_palette_in_force())
    });
    let mut changed = false;
    // **One slice, and then back to the caller that is holding the clock.**
    //
    // This was `loop { read; if empty { break } ; feed }`, and the exit it named
    // is one a busy pane never reaches. The read is
    // [`bt_pty::OutputRing::try_pop`] over a one-MiB ring that a *reader thread*
    // refills while this runs, so "the ring is empty" is a question about one
    // instant, and while the child prints at least as fast as `feed_at` absorbs
    // it that instant never arrives. Measured on 2026-08-24, release build, one
    // pane running `for(;;){ [Console]::Out.WriteLine('x'*4000) }` and nothing
    // else — no second pane, no page, nobody touching the frame: the window
    // stopped answering **two seconds** after launch and never answered again,
    // one core burned flat, ~35 MB/s of memory on top, `IsHungAppWindow` true
    // and the window thread `Running` — not blocked on anything, simply never
    // returning. That is the whole of the "drag it bigger and the program dies"
    // report: hosting a page takes a frame from 26 ms to 50 ms, so a pane that
    // stays just ahead of the terminal at rest falls behind while the frame is
    // being dragged, and once it is behind the loop stops terminating.
    //
    // The budget is not a number invented here. [`bt_pty::TERM_READ_QUANTUM`] is
    // *already* "the serialized Term actor quantum" of `DESIGN.md` §1.3 — the
    // unit of output one turn is meant to carry — and `try_pop` already takes at
    // most one of them. So the fix is to stop asking for a second one on the
    // same turn, and to say out loud that more is owed.
    //
    // Nothing is dropped and nothing waits on a clock: the leftover is answered
    // on the very next turn, which [`Runtime::drain_pty`] guarantees by raising
    // the wake. A pane that outruns the window is throttled by the ring and the
    // pipe behind it, which is what backpressure is for and what the ring was
    // built to do.
    //
    // **What T-DRAIN-BURST changed is the size of the ask, and nothing else in
    // this function.** A quantum bounds a turn in *bytes*, and the owner's
    // diagnostics log says what that is worth: holds of 4386, 2791, 1862 and
    // 3112 ms charged to `drain_pty`, page faults near zero beside them, while
    // a pane printed a build log. A number of bytes is not a length of time,
    // because the time a quantum costs is the time its *lines* cost — see
    // [`DRAIN_TURN_BUDGET`] for the account. So the read is a
    // [`bt_pty::TERM_READ_SLICE`] now, and the quantum is spent a slice at a
    // time by [`Runtime::drain_pty`], which looks at the clock in between. One
    // read per call is unchanged and load-bearing: the caller cannot stop a call
    // it is already inside, so the loop that repeats this one lives up there,
    // where the deadline is.
    let slice = hang_watch::during(hang_watch::Station::DrainRingRead, || {
        leaf.life
            .pty()
            .expect("PTY mode checked above")
            .read_output_slice()
    });
    let bytes = slice.bytes;
    hang_watch::counters(bytes.len(), 0, 0);
    // **Was this pane's picture under the program's own control across this slice?**
    //
    // A *counter* and not the deadline, and the difference was a defect: a 1,024-byte capped read
    // holding `BSU … ESU` whole opens and commits a block inside one feed, so the deadline is
    // `None` on both sides of it and a completed frame would have waited three milliseconds for
    // nothing. What is compared is how many updates this pane has committed — a question that
    // cannot fall between two samples. [`crate::coalesce`] declines to wait on either answer.
    let sync_commits_before = leaf.session.synchronized_update_commits();
    if !bytes.is_empty() {
        debug_assert!(bytes.len() <= bt_pty::TERM_READ_SLICE.get());
        // The drain brackets all of its slices with begin/end_feed_turn, so a
        // repaint's proven records stay protected until the whole turn settles.
        hang_watch::during(hang_watch::Station::DrainFeed, || {
            leaf.session
                .feed_at(&bytes, Instant::now())
                .context("apply PTY output")
        })?;
        hang_watch::during(hang_watch::Station::DrainReplies, || -> Result<()> {
            for reply in hang_watch::during(hang_watch::Station::DrainChannel, || {
                leaf.session.take_pty_writes()
            }) {
                write_pty_input(
                    leaf.input_target(),
                    &reply,
                    "return terminal protocol reply to PTY",
                )?;
            }
            Ok(())
        })?;
        changed = true;
    }
    // Asked of the ring itself rather than guessed from the chunk's length: a
    // pop that happened to return exactly one quantum may have emptied it, and a
    // pop that returned less may already have been overtaken by the reader.
    let pending = hang_watch::during(hang_watch::Station::DrainRingStats, || {
        leaf.life
            .pty()
            .expect("PTY mode checked above")
            .ring_stats()
            .current_bytes
            > 0
    });
    // **Where the keyboard is, told to whoever subscribed to hear it** (DEC
    // 1004; attention plan slice A0.5). Told on every turn for the palette's
    // reason one screen up — the answer is four facts a window can move in a
    // dozen places — and the call is a comparison that queues bytes only when
    // the answer actually changed and only for a pane that asked.
    //
    // **After the feed and not beside the palette**, because the bit it consults
    // is written by the bytes just fed: ConPTY sends `\e[?1004h` ahead of the
    // first program, so a pane subscribes on the very turn it is born, and a
    // reconciliation that ran before the feed would owe that pane its opening
    // report until something else happened to wake the loop.
    hang_watch::during(hang_watch::Station::DrainKeyboardFocus, || {
        leaf.report_keyboard_focus(holds_the_keyboard)
    });
    // A canvas notification, a focus report, and any colour answer queued behind
    // them, are owed to a child that has said nothing — so the reply channel is
    // drained once more outside the read above, which only feeds when the child
    // speaks.
    hang_watch::during(hang_watch::Station::DrainReplies, || -> Result<()> {
        for reply in hang_watch::during(hang_watch::Station::DrainChannel, || {
            leaf.session.take_pty_writes()
        }) {
            write_pty_input(
                leaf.input_target(),
                &reply,
                "return terminal protocol reply to PTY",
            )?;
        }
        Ok(())
    })?;
    // The one place a shell is heard to speak. Every leaf of every tab passes
    // through here on every turn of the loop, which is what lets a pane nobody
    // is watching keep an honest account of itself — the frame path cannot, and
    // never could: it runs for the focused pane of the tab on screen and for
    // nothing else.
    let outcome_parent = hang_watch::enter(hang_watch::Station::DrainMarks);
    let outcome = (|| {
        leaf.output_revision = output_revision(
            leaf.output_revision,
            changed,
            leaf.session.resize_finish_deadline().is_some(),
        );
        // **And the one line a restored pane is owed, typed at the moment there is a
        // prompt to type it at** (§7.54e ④ and ⑤).
        //
        // Here for the reason the command ledger below is here: this is the one place
        // every leaf of every tab passes through on every turn, so a pane nobody is
        // looking at is served too — and the fact this waits on is read off that very
        // ledger. **An open mark with nothing executed is a shell reading a line**
        // (`OSC 133;B` seen, no `C`), which is precisely "there is a prompt and the
        // cursor is on it"; writing before that would put the command inside a login
        // banner, or hand it to a shell still sourcing its profile.
        //
        // Taken rather than read, so one prompt receives it once.
        if leaf
            .session
            .command_marks()
            .last()
            .is_some_and(|mark| mark.executed.is_none() && mark.finished.is_none())
            && let Some((command, submit)) = leaf.take_typing()
        {
            let bytes = quake::typed_into_a_prompt(&command, submit);
            if !bytes.is_empty() {
                write_pty_input(
                    leaf.input_target(),
                    &bytes,
                    "type a restored command at the prompt",
                )?;
            }
        }
        // **A command ending is heard here or nowhere.** The ledger is the shell's
        // own account of itself (`OSC 133;D`), and this is the one place every leaf's
        // bytes pass through — so a `git commit` finished in a pane behind a Git page
        // is noticed whether or not that pane is the focused one.
        let finished = leaf
            .session
            .command_marks()
            .iter()
            .rev()
            .find(|mark| mark.finished.is_some())
            .map(|mark| mark.id);
        let ended = finished > leaf.last_finished_command;
        if ended {
            leaf.last_finished_command = finished;
        }
        // **And the first mark is heard here too**, for the same reason: it is
        // what gives this pane a rail, and a pane behind another tab earns one at
        // its first prompt exactly as the pane on screen does.
        let rail_began = leaf.hear_first_mark();
        let name_after = leaf.name_evidence();
        // **A keyboard flag a program asked for and Folio does not honour** is written to the
        // log the first time this session sees it (T-KEYBOARD-PROTOCOL, design note §3) — here,
        // because every leaf's bytes pass through here, the pane on screen or not.
        for line in leaf.session.take_keyboard_protocol_notes() {
            diagnostics::note(&line);
        }
        // **A pane whose PSReadLine is not Folio's after its profile ran** says so once, in the
        // log and nowhere else (T-INTEGRATION-INJECT-2-BUILD round 2), on the same road.
        for line in leaf.session.take_line_editor_notes() {
            diagnostics::note(&line);
        }
        Ok(DrainOutcome {
            // Taken here because this is the one place every leaf of every tab
            // passes through on every turn, which is the same sentence
            // `output_revision` and the command ledger above are written under: a
            // pane nobody is watching is exactly the pane whose notification matters.
            // The seat is filled in by `drain_tab_pty`, which is the level that knows
            // it.
            notifications: leaf
                .session
                .take_notifications()
                .into_iter()
                .map(|notification| (SeatId(0), notification))
                .collect(),
            command_ends: leaf
                .session
                .working_directory()
                .filter(|_| ended)
                .map(Path::to_path_buf)
                .into_iter()
                .collect(),
            arrived: changed,
            pending,
            // **Bytes arrived and the transport did not say there was more**, folded here because a
            // slice's read boundary is visible nowhere else — see [`crate::coalesce`]. A pane whose
            // slice ended on a whole capped read is a pane a millisecond ahead of the rest of a
            // burst; a pane whose slice ended anywhere else has nothing to wait for, and says so.
            arrived_uncapped: changed && !slice.ends_capped,
            bytes: bytes.len(),
            // **The program named its own frame's end during this turn.** That frame is complete and
            // must not be made to wait behind a timer — however far inside one read the naming fell.
            sync_bracket_closed: synchronized_update_committed(sync_commits_before, &leaf.session),
            // **A block is open on this pane.** Asked of every pane and not only the one the frame
            // path composes: a sibling holding a block open is a sibling whose bytes the parser is
            // withholding, and a window that waited on its account would be waiting for a program
            // that has already said where its frame ends.
            sync_open: leaf.session.synchronized_update_deadline().is_some(),
            // A leaf cannot know whether it is the one the frame path will compose;
            // that is a fact about the tab, and [`drain_tab_pty`] is where it is
            // known and where this is filled in.
            arrived_off_focus: false,
            renamed: name_after != name_before,
            moved: name_after.1 != name_before.1,
            rail_began,
        })
    })();
    hang_watch::at(outcome_parent);
    outcome
}

/// **Whether one seat's shell is, right now, the true owner of the keyboard** —
/// the thing DEC 1004's `CSI I`/`CSI O` means, and the whole of what slice A0.5
/// reports (attention plan §3.0, §10.10; Q8 ruled 2026-08-25).
///
/// Four bits and a conjunction, and every one of them is a way for the answer to
/// be no:
///
/// - `window_focused` — this window has the desktop's keyboard at all.
/// - `tab_is_active` — this tab is the one on screen. A pane in a background tab
///   is not being typed into however the window stands.
/// - `seat_is_focused_leaf` — of the panes this tab holds, this is the one. A
///   three-seat tab has one keyboard and two agents who ought to know they are
///   not holding it (§3.0's D10: that is why the report is per pane and not per
///   window).
/// - `owner_is_a_shell` — the keyboard is in a *terminal* and not in a preview,
///   a page, a files tree, a search capsule, a rename field or a menu.
///
/// **The fourth bit is the one the first draft was missing** (evidence in
/// §10.10). [`TabState::focused_leaf`] names *the shell the keyboard falls back
/// to* — it is written for seats that are in `sessions`, which is to say for
/// terminals — so it goes on naming a pane while a page beside it is being typed
/// into. Click a files tree and the window's focus has not moved a bit; a
/// predicate written on three bits would say the shell still has the keyboard,
/// and the agent inside it would go on believing you were reading it.
///
/// **Visibility is not one of the bits, deliberately.** A tab becoming visible
/// while something else holds the keyboard sends nothing: the protocol reports
/// input focus, not eyes (§10.10, and the v1 leaning it supplies the reason
/// for). Focus mode adds no bit either — it does not mint a keyboard owner of
/// its own, and a card taking the stage still resolves to `focused_leaf`.
///
/// `sessions.contains_key(seat)` is an address guard rather than a fifth fact
/// about the product, and stays out of here for that reason: a seat with no
/// session has no PTY to be told anything.
pub(crate) fn seat_holds_the_keyboard(
    window_focused: bool,
    tab_is_active: bool,
    seat_is_focused_leaf: bool,
    owner_is_a_shell: bool,
) -> bool {
    window_focused && tab_is_active && seat_is_focused_leaf && owner_is_a_shell
}

/// **The one apply operation** (ticket 37): put freshly derived metrics on one leaf.
///
/// Every road that changes what a pane is drawn at comes through here — a size step, a Settings
/// change, a display change and a transfer to another window — so the things that carry the
/// metrics move together: the view's own record ([`crate::TermView::metrics`]), the session's cell
/// height, width and baseline and its face size (the session's cell-size stage,
/// [`crate::ShellSession::propose_cell`], which re-keys its math through `set_layout_key`),
/// and the projection, refreshed so a wheel or a scroll read after this sees the new rows. All of
/// it happens **even when the integer grid is unchanged**: a larger face on the same grid is a new
/// picture, and the frame it projects carries the new face size in its `layout_key`. The grid
/// itself is the caller's to re-derive afterwards, through the existing resize transaction
/// (`ShellSession::propose_size` → `ShellSession::release_due_size` → `commit_leaf_resize`).
///
/// Answers whether anything moved. When the metrics are the ones the leaf already has — a step
/// the clamp swallowed — it touches nothing and answers `false`, so no reflow, no PTY and no
/// present follow.
///
/// Writers of the rung: `Runtime::step_pane_text_scale` (the door) and construction. Readers:
/// [`pane_cell_metrics`] and the pane head's indicator. Clearers: none — a view that ends takes
/// its rung with it.
pub(crate) fn apply_leaf_metrics(
    leaf: &mut crate::TerminalPane,
    metrics: bt_render::CellMetrics,
) -> bool {
    if leaf.view.metrics == metrics {
        return false;
    }
    leaf.view.metrics = metrics;
    leaf.session.propose_cell(metrics);
    leaf.session
        .session
        .refresh_projection(&mut leaf.view.projection);
    true
}

/// **What a `Restart shell…` spawns** — `docs/M2-restart-shell-contract.md`
/// §1.1, as a function of the leaf that is leaving.
///
/// A free function so the contract can be pinned without a window: the three
/// things §1.1 says are kept are kept *here*, and every one of them is kept by
/// being read off the dying leaf rather than looked up somewhere newer.
///
/// * **The profile is the seat's own**, not the current default. A seat
///   restarted twice runs the same program both times, which is the whole of
///   what "同一 seat 反复重启永远用同一个可执行文件" asks for; reading
///   `settings.default_profile` here would turn a Git Bash pane into a
///   PowerShell the first time somebody changed that setting.
/// * **The directory is where the pane stands**, read by the caller through
///   [`ShellSession::seed_place_for_a_new_shell`]: the last trusted OSC 7 report,
///   else the shell process's current folder, else the seat's initial cwd.
///   Since T-CWD-FROM-PROCESS (2026-10-17) a `cd` the old shell performed
///   without announcing it *is* seen when its process can be read — that
///   supersedes §1.1's 「最后已知」 and not 「当前」 for this verb; a process
///   that cannot be read leaves the old ladder. Until 2026-10-04 only the first
///   rung was read, so a pane opened in a chosen folder whose shell never
///   reported restarted at its profile's default instead of where it was born.
/// * **No folder at all is an absence, not a fallback.** `None` (a pane whose
///   spawn could name no place, or one put down at its shell's own `~`) reaches
///   `profiles::spawn_place`, which answers it with the profile's own starting
///   directory — the same answer a brand-new pane on that profile gets.
///
/// The manual name is not here because it is not a fact about a shell: it is the
/// tab's (`TabSeed::manual_name`), and a restart that never touches a tab keeps
/// it by construction rather than by copying it.
pub(crate) fn restart_seed(profile: &str, standing_in: Option<profiles::SeedPlace>) -> LeafSeed {
    LeafSeed {
        profile: profile.to_owned(),
        // Named or carried as the leaf says (`ShellSession::seed_place_for_a_new_shell`):
        // `profiles::place_for` weighs it.
        cwd: standing_in,
        // A running pane's profile is one this build has, by construction — it
        // started a process from it.
        unknown_profile_id: None,
        // A new shell starts its card at the tail.
        card_skip: 0,
        // A restart is a new shell in the same place; nothing is owed to its prompt.
        prefill: None,
        // Not the seed's to say: a Restart shell is born in its tab, and `born_in_tab` gives it
        // the tab's carried environment (coordinator's ruling 2026-10-09).
        carried_environment: None,
    }
}

/// Whether letting go of a drag-selection writes it to the clipboard.
///
/// `copy_on_select` is the reader's own answer, and it is a parameter rather
/// than a guard at the call site so that the whole policy is one function a
/// test can ask (gesture audit 2026-08-26, 丙4). The audit's finding about this
/// gesture is unlike the other six: the reader **can** perform it — they do,
/// constantly — they simply are not told that they did, and what they were not
/// told is that their clipboard is gone. Windows Terminal's `copyOnSelect`
/// ships **off**, so it is not muscle memory arriving with the reader either.
/// The switch is what gives the behaviour a name; it stays on by default
/// because that is what every build of this window has done.
pub(crate) fn should_copy_on_select_release(
    route: Option<&MouseRoute>,
    single_click: bool,
    copy_on_select: bool,
) -> bool {
    copy_on_select && !single_click && matches!(route, Some(MouseRoute::Local(_)))
}

/// Which command a walk lands on, given how many there are and which one the
/// viewport is showing.
///
/// A function rather than four lines inside [`Runtime::step_command_mark`], for
/// [`super::tabs::stepped_tab`]'s reason one file over: the ends are the whole of the rule and
/// the ends are what a window, a session and a solve make expensive to reach.
///
/// **`at` is `None` when the viewport is above every command** — scrolled into
/// output that predates the oldest mark, or into a transcript whose oldest marks
/// have been evicted. Forwards, the first command is the next one; backwards there
/// is nothing before it, and there is no wrap to throw the walk to the newest.
pub(crate) fn stepped_command_mark(count: usize, at: Option<usize>, step: Step) -> Option<usize> {
    match (step, at) {
        (Step::Forward, None) => (count > 0).then_some(0),
        (Step::Back, None) => None,
        (Step::Forward, Some(index)) => (index + 1 < count).then_some(index + 1),
        (Step::Back, Some(index)) => index.checked_sub(1),
    }
}

/// [`ReferenceCard`] for the target `uri`, or `None` for a reference this window
/// has no card for.
///
/// **It is [`hyperlink_activation`] read for a hand instead of for a click**,
/// and that is the whole of the implementation on purpose. §7.1.5j ① already
/// folded every shape the terminal can print a local file in — a bare path, a
/// `file:` URI, an OSC 8 target — into one `file:` link fed to one routing
/// table, and a *second* table here would be the thing that clause exists to
/// forbid: a hover that decided for itself which references are files would
/// drift from the click that opens them, and the first symptom would be a card
/// over something a press does nothing to.
///
/// So the mapping is the plain half of that table, arm for arm:
///
/// * [`HyperlinkActivation::Preview`] — this window's own answer for a file, so
///   the card is the one that shows a file. A share arrives down this arm too
///   and is *meant* to: §7.1.3's network refusal is what the card then prints,
///   the preview's judgement borrowed exactly as the files column borrows it.
/// * [`HyperlinkActivation::FilesColumn`] — this window's own answer for a
///   folder, so the card is the one that shows a folder.
/// * everything else — **no card**, and each for the reason its own arm gives.
///   `Browser`, `Share`, `Scheme` and `Blocked` are `Ctrl`'s answers and can never be reached with
///   `control: false`; they are listed so that a future arm cannot be added
///   without someone deciding what a hand resting on it should see. `None` is
///   the plain half of a row with no destination inside this window at all — a
///   `mailto:`, a scheme with no arm — and a card over one would be this window
///   offering to show something it cannot fetch. **`Page` is the one arm that
///   acts and still raises nothing**, and that is a fact about the cards this
///   window has rather than about the address: both of them — the glance and
///   the folder flyout — are made of a thing on this disk, and a remote page is
///   not one. Reading it to fill a card would mean fetching it, which is the
///   press's business and not a resting hand's; the hover line already prints
///   the address, which is everything this window knows about it before it goes.
///
/// `control` is deliberately not a parameter. A modifier held during a hover is
/// not a request; the card is what this window has to say about the reference,
/// and `Ctrl` changes where a *press* goes rather than what the thing is.
pub(crate) fn reference_card(
    uri: &str,
    namer: bt_transcript::paths::PathNamer<'_>,
    verdict: &dyn Fn(&Path) -> Option<bt_term::PathVerdict>,
) -> Option<ReferenceCard> {
    match hyperlink_activation(false, true, uri, namer, verdict) {
        HyperlinkActivation::Preview(path, _) => Some(ReferenceCard::File(path)),
        HyperlinkActivation::FilesColumn(path) => Some(ReferenceCard::Folder(path)),
        HyperlinkActivation::None
        | HyperlinkActivation::Browser(_)
        | HyperlinkActivation::Share(_)
        | HyperlinkActivation::Scheme(_)
        | HyperlinkActivation::Page(_)
        | HyperlinkActivation::External(_)
        | HyperlinkActivation::Reveal(_)
        | HyperlinkActivation::Blocked(_) => None,
    }
}

/// **The box a reference's run occupies on one row of a pane's grid** — the
/// rectangle a card raised over it is placed against.
///
/// `cells` is the segment's flat cell indices in reading order, exactly as
/// [`bt_viewport::ViewportFrame::hyperlink_cells`] hands them over and exactly
/// the set the underline is painted on, so the card stands beside what is lit
/// rather than beside a second opinion about where the reference is.
///
/// **One row's worth, and the row is the pointer's.** A segment may cover the
/// tail of one row and the head of the next — `hyperlink_cells`' own note says
/// so — and a rectangle unioned across both would be a box with a hole in it
/// spanning the whole pane's width. The row the hand is on is the row the hand
/// is asking about, which is the same choice a wrapped file row in the tree
/// makes by having only one.
///
/// `top` and `bottom` are that row's own drawn interval rather than a multiple
/// of the cell height, because a pane with a math block in it has rows of
/// unequal height and the frame is the only thing that knows theirs.
pub(crate) fn reference_run_rect(
    cells: &[u32],
    columns: u32,
    row: u32,
    origin: [f32; 2],
    cell_width: f32,
    top: f32,
    bottom: f32,
) -> Option<[f32; 4]> {
    let (mut first, mut last) = (u32::MAX, 0u32);
    for &cell in cells {
        if columns == 0 || cell / columns != row {
            continue;
        }
        let column = cell % columns;
        first = first.min(column);
        last = last.max(column);
    }
    (first <= last).then(|| {
        [
            origin[0] + first as f32 * cell_width,
            origin[1] + top,
            origin[0] + (last + 1) as f32 * cell_width,
            origin[1] + bottom,
        ]
    })
}

/// Which door a click on a verified inline image reference goes through.
///
/// The modifier is read through [`ClickIntent`] rather than by an `if control`
/// of its own, because this arm and [`hyperlink_activation`]'s are the same rule
/// stated about two kinds of reference (§7.1.5g, user ruling 2026-08-20) — and a
/// rule written down twice is a rule that can come to be two.
pub(crate) fn local_image_activation(
    control: bool,
    click_no_drag: bool,
    worker_verified_path: Option<&std::path::Path>,
) -> LocalImageActivation {
    let Some(path) = worker_verified_path.filter(|_| click_no_drag) else {
        return LocalImageActivation::None;
    };
    match ClickIntent::of(control) {
        ClickIntent::Here => LocalImageActivation::Preview(path.to_path_buf()),
        ClickIntent::System => LocalImageActivation::External(path.to_path_buf()),
    }
}

/// Whether the terminal hyperlink under the pointer would answer a press right
/// now — and therefore whether the pointer wears the hand (§7.1.5g).
///
/// **The underline and the hand are two different sentences.** The dotted rest
/// and its solid hover already say "there is a link here", and they say it to a
/// hand that is not holding anything down, because that is a fact about the
/// output and not about the gesture. The pointing finger is the narrower one:
/// "and *this* press, with the keys held the way they are held right now, is one
/// that goes somewhere".
///
/// **So it is not a fact about `Ctrl`; it is the routing table asked** (user
/// ruling 2026-08-20). A plain hover over a link to a readable file wears the
/// hand, because a plain press opens it — and, since the folder ruling of
/// 2026-08-21, so does a plain hover over a folder, because a plain press points
/// the files column at it; since 2026-08-23 so does a page, and since
/// 2026-08-29 so does a **web address**, because a plain press on one now draws
/// it on the seat. The one shape left without the hand plainly is the row with
/// nothing inside this window at all — a `mailto:`, a scheme with no arm — and
/// that is the whole of what the modifier still lights up on its own.
///
/// One expression rather than two agreeing ones, for the reason 7.1.5f wrote
/// down: a pointer that promises a press the release then declines is this window
/// lying about what it can do. `verdict` travels for the same reason it travels
/// into the table, and reaches no disk at all (audit 3 C-2) — so a name nobody
/// has answered for wears no finger, exactly as it is no link.
pub(crate) fn terminal_link_answers_a_press(
    control: bool,
    uri: Option<&str>,
    namer: bt_transcript::paths::PathNamer<'_>,
    verdict: &dyn Fn(&Path) -> Option<bt_term::PathVerdict>,
) -> bool {
    uri.is_some_and(|uri| {
        hyperlink_activation(control, true, uri, namer, verdict) != HyperlinkActivation::None
    })
}

/// **The one mutation door's core** (ticket 37): move one leaf's rung and apply what it now
/// derives to.
///
/// `derive` is [`pane_cell_metrics`] bound to the leaf's window, so the derivation stays the one
/// derivation; a test hands it the same measuring service at a chosen base and scale. Answers
/// whether the pane's metrics moved — `false` both when the step went nowhere (the end of the
/// ladder) and when it went to a rung the clamp draws at the same size, and in both cases nothing
/// else has been touched. The rung itself moves even then: the indicator shows the requested
/// rung, and a pane at the floor of a base-10 face still says it is at 50 %.
pub(crate) fn step_leaf_text_scale(
    leaf: &mut crate::TerminalPane,
    step: TextStep,
    count: u32,
    derive: impl FnOnce(TextScale) -> Result<bt_render::CellMetrics>,
) -> Result<bool> {
    let requested = (0..count).fold(leaf.view.text_scale, |scale, _| scale.stepped(step));
    let metrics = derive(requested)?;
    leaf.view.text_scale = requested;
    Ok(apply_leaf_metrics(leaf, metrics))
}

/// **May the drain go round again?** — the whole of T-DRAIN-BURST's decision, in
/// one function so that it can be put to a test without a window.
///
/// `slices_taken` counts passes over every pane, not reads of one pane: the
/// panes are drained round-robin so that a quiet pane's few hundred bytes are
/// not stuck behind a noisy sibling's quarter megabyte, and a pass is therefore
/// the unit both halves of this budget are spent in.
///
/// Both bounds are "whichever comes first", and each is there for a case the
/// other does not cover. The clock is the one that matters while a pane prints;
/// the slice count is what keeps a turn from carrying more than §1.3 says it
/// may when the output is cheap enough that the clock never runs out —
/// megabytes of blank lines would otherwise be drained in a single turn, and a
/// turn that long is the defect however fast it parsed.
pub(crate) fn drain_may_take_another_slice(slices_taken: usize, elapsed: Duration) -> bool {
    slices_taken < DRAIN_SLICES_PER_TURN && elapsed < DRAIN_TURN_BUDGET
}

/// Bracket the complete slice loop, including its error return, once for every tab.
pub(crate) fn in_drain_feed_turn<T, R>(
    tabs: &mut [T],
    begin: impl Fn(&mut T),
    end: impl Fn(&mut T),
    drain: impl FnOnce(&mut [T]) -> R,
) -> R {
    for tab in &mut *tabs {
        begin(tab);
    }
    let result = drain(tabs);
    for tab in tabs {
        end(tab);
    }
    result
}

/// Drain every shell this tab holds.
///
/// Written as a loop rather than left to the tab's deref because "drain this
/// tab" has always meant *all of it*: a background pane that stops being read
/// fills its pipe and blocks the shell writing into it. The tab's answer is the
/// union of its leaves' — anything arrived anywhere, any title moved anywhere.
///
/// The three bools are the bits of [`seat_holds_the_keyboard`] a leaf cannot
/// answer about itself; this loop supplies the fourth from the seat it is on,
/// and that predicate — not this signature — is where the four are joined.
fn drain_tab_pty(
    tab: &mut TabState,
    window_focused: bool,
    tab_is_active: bool,
    owner_is_a_shell: bool,
) -> Result<DrainOutcome> {
    hang_watch::during_pane(hang_watch::Station::DrainTab, tab.id.0, || {
        // Copied before the loop, which then holds the tab mutably. It is the one
        // fact this loop needs that a leaf cannot supply about itself: whether the
        // pane it just heard is the pane the frame path is going to compose.
        let focused = tab.focused_leaf;
        let mut outcome = DrainOutcome::default();
        for (seat, leaf) in tab.leaves_mut() {
            let holds_the_keyboard = seat_holds_the_keyboard(
                window_focused,
                tab_is_active,
                *seat == focused,
                owner_is_a_shell,
            );
            let pane_parent = hang_watch::enter_pane(hang_watch::Station::DrainPane, seat.0);
            let result = drain_leaf_pty(leaf, holds_the_keyboard);
            hang_watch::counters(0, 0, 1);
            hang_watch::at(pane_parent);
            let mut leaf_outcome = result?;
            outcome.arrived_off_focus |= leaf_outcome.arrived && *seat != focused;
            // The address a leaf cannot write on its own request, stamped by the
            // level that holds the key it is filed under.
            for (address, _) in &mut leaf_outcome.notifications {
                *address = *seat;
            }
            outcome.merge(leaf_outcome);
        }
        Ok(outcome)
    })
}
