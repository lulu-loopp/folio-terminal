//! `panes` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    ColumnNotch, CommandFlash, DividerGrip, Drag, DragCarry, DragHandover, DragSource, DropLanding,
    FilesFocusArrival, FlashBand, FolderPick, FormulaSwitches, FrameImageReferences, FrameTraces,
    HandoverInto, LeafId, LeafOnStage, LeafSession, MathHoverExit, MenuPaint, Motion,
    NewWindowPlan, NoticeHost, PaneDraw, PaneMenuState, PaneTransform, PopoverTrigger, Popup,
    PopupOwner, PresentIntent, PreviewSurface, RAIL_TEXT_FADE, RAIL_TEXT_FADE_OPEN_DELAY,
    RAIL_TRANSITION, RevealTween, RowHost, RowPayload, RowPayloadKind, RowVerb, Runtime, SplitSeed,
    TabId, TabState, TabSurface, TearOut, animated_pane_viewports, attention, auto_split_axis,
    card_trace, clamp_into_body, closing_this_pane_closes_the_tab, cmdrail, column_notch,
    create_leaf_session, divider_drag_still_holds_its_pointer, expire_leaf_attention,
    file_menu_powers, files, float, float_dock_label, glyph_trace, hang_watch, host_screen_after,
    leaf_resize_plan, marks, mouse_trace, native_window, notice, notify,
    pages_that_move_with_their_panes, palette, pane_fade_veil_layers, panel_opacity,
    persisted_pane_count, picture_channel_owner, popup_owner, present_diagnostics,
    presentation_physical_size, preview, preview_image_placement, preview_trace, profiles,
    rail_change_strands_its_popups, rail_is_searching, rail_overlay_layer, rail_zone_wants_open,
    restated_scroll, restore, restore_row_seed, risen_frame, row_verb, schedule_leaf_grid_change,
    scrollback_quota, seats, shell_integration, size_authority_for_rectangle, solve_seats,
    solve_tree, trace_sink, trace_unchanged_present, video_seat, webhost,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::{Axis, SeatId, SeatMetrics, SizePolicy};
use bt_render::{
    FrameSource, FrameTrigger, GpuContext, GridSize, PresentOutcome, PresentPhase, Travel,
    WindowRenderer,
};
use bt_viewport::horizontal::ContentColumn;
use bt_viewport::{ViewSelection, ViewportFrame};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::MouseScrollDelta;
use winit::window::{Window, WindowId};

impl Runtime<'_> {
    /// Re-solve the tree against the current surface and place the terminal
    /// seat.
    ///
    /// It used to also *answer* a grid — the one `seats.identity()`'s rectangle
    /// asks for — and every caller handed that answer straight to the focused
    /// leaf, which is a different leaf as soon as a tab holds two. Deriving
    /// cols/rows is [`Self::resize_leaves_to_layout`]'s job now, once per leaf
    /// from that leaf's own body, so there is no single grid for this method to
    /// return and no way for a caller to give one shell another's width.
    ///
    /// The direction is still one-way (red line L10): what comes back out of the
    /// terminal never re-enters here.
    pub(in crate::runtime) fn resolve_seat_layout(&mut self, render_physical: PhysicalSize<u32>) {
        // Provenance is decided here, at the one place a rectangle becomes a layout, and from the
        // very number the solver is handed (user ruling 2026-08-08).
        (self.window.size_policy, self.window.lawful_client_size) = size_authority_for_rectangle(
            self.window.size_policy,
            self.window.lawful_client_size,
            render_physical,
        );
        let (layout, overflow, terminal_seat, viewport) = solve_seats(
            &self.seats,
            &self.window.renderer,
            render_physical,
            self.window.size_policy,
            // **The posture, never the stored preference** (§7.1.6b′). The card
            // column stands in the panel's own place and costs the stage its own
            // width exactly as an expanded rail does, and that width is
            // `RailState::terminal_inset_logical_px` reading `focus`. The bit
            // lives on the window rather than in `window.rail`, so a solve handed
            // the stored preference is a solve told the mode is off — which is
            // the whole of "the column floated over the panes".
            self.rail_posture(),
            self.platform_chrome(),
        );
        // **The one gate the type system cannot hold: the stage was solved against
        // the panel this window is actually wearing.**
        //
        // Every geometry question in this file is supposed to go through
        // [`Self::rail_posture`], and nothing but a reader's care makes that true —
        // `WindowRuntime::rail` is the same type and compiles just as well, which
        // is how the card column came to be drawn floating over the panes on the
        // day it landed. So the agreement is checked where the answer lands: it
        // re-solves the viewport from
        // the posture, and a solve handed anything else stops the debug build on
        // the first frame of the mode rather than showing it to a reader.
        debug_assert_eq!(
            viewport,
            seats::logical_viewport(
                render_physical.width,
                render_physical.height,
                seats::scale_ppm(self.window.renderer.metrics().dpi_milli().get()),
                seats::rail_inset_device_px(
                    self.rail_posture(),
                    seats::scale_ppm(self.window.renderer.metrics().dpi_milli().get()),
                ),
                seats::chrome_band_device_px(
                    seats::scale_ppm(self.window.renderer.metrics().dpi_milli().get()),
                    self.platform_chrome(),
                ),
            ),
            "the stage was laid out against a panel this window is not wearing"
        );
        self.window.seat_viewport = viewport;
        // T230, and the reason the diff is taken *here*: this is the one place a
        // solved layout becomes the layout, so it is the one place that can tell
        // a real change from a rebuild that landed on the same answer. Every
        // cause §3.5 lists — the ladder, a divider drag, a swap, focus mode —
        // passes through it, and none of them has to remember to say so.
        let events = seats::layout_events(&self.seat_layout, &layout);
        self.seat_layout = layout;
        self.seat_overflow = overflow;
        self.publish_layout_events(events);
        self.window.renderer.set_seat_viewport(terminal_seat);
        self.refresh_preview_for_layout();
        self.refresh_chrome();
    }

    /// Every pane's solved box in physical pixels, in the solver's own order.
    ///
    /// The order is `SeatLayout::rects`', which is the in-order walk of the tree
    /// (D2) — red line L8, and the reason [`PaneMotion`] stores a `Vec` at all:
    /// every number in here is geometry, and geometry that depends on iteration
    /// order is geometry nobody chose.
    ///
    /// A seat the solver did not place has no box and is simply absent, which is
    /// the same answer [`PaneMotion::begin`] gives a seat that left the layout.
    pub(in crate::runtime) fn pane_rects(&self) -> Vec<(SeatId, [f32; 4])> {
        self.seat_layout
            .rects
            .iter()
            .filter_map(|placement| {
                let device = placement.device_rect?;
                Some((
                    placement.id,
                    [
                        device.left as f32,
                        device.top as f32,
                        device.right as f32,
                        device.bottom as f32,
                    ],
                ))
            })
            .collect()
    }

    /// What each pane is drawn through right now, for [`seats::PaneMotionFrame`].
    ///
    /// The clock is read once by the caller and handed down, exactly as
    /// [`Self::tab_trailers`] does for the strip: two seats of one chrome build
    /// disagreeing about what time it is would be two seats sampled from two
    /// different frames of the same animation.
    pub(in crate::runtime) fn pane_transforms(&self, now: Instant) -> Vec<(SeatId, PaneTransform)> {
        self.seat_layout
            .rects
            .iter()
            .map(|placement| {
                (
                    placement.id,
                    self.window
                        .pane_motion
                        .transform_of(placement.id, now, self.app.motion),
                )
            })
            .collect()
    }

    /// Hand one commit's geometry changes to whoever is listening (T230).
    ///
    /// See [`WindowRuntime::last_layout_events`] for who that is, and is not, today.
    fn publish_layout_events(&mut self, events: Vec<seats::LayoutEvent>) {
        self.window.last_layout_events = events;
        if self.app.trace_layout_events && !self.window.last_layout_events.is_empty() {
            eprintln!("BT_LAYOUT_EVENTS {:?}", self.window.last_layout_events);
        }
    }

    pub(crate) fn seat_metrics(&self) -> SeatMetrics {
        seats::seat_metrics(self.window.renderer.metrics().dpi_milli().get())
    }

    /// One terminal pane's body, or `None` when the rail has nothing to stand on.
    ///
    /// The rail is drawn on the **primary screen only**, and this is where that is
    /// enforced rather than inside [`cmdrail`]: the ledger already refuses to
    /// record an alternate-screen marker (§3.2's isolated namespace), so a pane
    /// running `vim` has a ledger full of the commands that ran *before* `vim`
    /// started — perfectly true marks that would be drawn over somebody else's
    /// canvas. What the alternate screen suspends is not the data, it is the
    /// picture.
    pub(crate) fn command_rail_body(&self, seat: SeatId) -> Option<[f32; 4]> {
        let leaf = self.sessions.get(&seat)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        cmdrail::host_rect(
            [
                body.x as f32,
                body.y as f32,
                (body.x + body.width) as f32,
                (body.y + body.height) as f32,
            ],
            leaf.session.terminal_modes().alternate_screen,
        )
    }

    /// **The rail's ordinal stack for one pane — "one rail, two sources"**
    /// (§7.1.5d, S4).
    ///
    /// The plain command ledger, unless a capsule is open on this very pane with
    /// something typed in it; then the ledger and the matched lines merged into
    /// one stack in document order. The two sides are put in one line space by
    /// [`bt_viewport::search_address`] — the function the highlighter already uses
    /// to place a cell against a hit — so a command's prompt row and a hit on that
    /// same row are recognisably the same line rather than two ticks a pixel
    /// apart.
    ///
    /// **The hits are deduplicated per line here and not in [`cmdrail::merge`]**,
    /// because this is where the hits are: the search keeps them in document order
    /// grouped by line already, so the dedup is `last()` and not a set. B56's own
    /// asymmetry lands in these four lines — *the rail counts lines, the counter
    /// counts hits* — and it is what stops a `grep` output, where every line
    /// matches several times, from drawing a rail nobody can read.
    fn command_rail_stack(&self, seat: SeatId) -> cmdrail::Stack {
        let Some(leaf) = self.sessions.get(&seat) else {
            return cmdrail::Stack::default();
        };
        let marks = leaf.session.command_marks();
        if self.command_rail_search(seat).is_none() {
            return cmdrail::commands(marks);
        }
        let commands: Vec<cmdrail::CommandLine> = marks
            .iter()
            .map(|mark| cmdrail::CommandLine {
                mark: mark.id,
                line: leaf
                    .session
                    .command_mark_anchor(mark.start)
                    .and_then(bt_viewport::search_address)
                    .map(|(line, _)| line),
                failed: mark.failed(),
            })
            .collect();
        let current = self.window.search.current().map(|hit| hit.line);
        let mut matches: Vec<cmdrail::MatchLine> = Vec::new();
        for (index, hit) in self.window.search.hits().iter().enumerate() {
            if matches.last().is_some_and(|last| last.line == hit.line) {
                continue;
            }
            matches.push(cmdrail::MatchLine {
                line: hit.line,
                hit: index,
                current: current == Some(hit.line),
            });
        }
        cmdrail::merge(&commands, &matches)
    }

    /// What the search contributes to this pane's [`cmdrail::RailKey`], or `None`
    /// when the pane is carrying no search.
    ///
    /// **`None` for an empty field**, which is the one place this parts company
    /// with the prototype: `srchRail()` marks the rail the moment a capsule exists,
    /// so its commands recede at `Ctrl+F` before anything has been asked. An empty
    /// query has asked nothing, and a rail that greys out to answer nothing is a
    /// rail that has told the reader their history went away.
    fn command_rail_search(&self, seat: SeatId) -> Option<cmdrail::RailSearch> {
        if !rail_is_searching(self.window.search.seat(), self.window.search.query(), seat) {
            return None;
        }
        Some(cmdrail::RailSearch {
            query: self.window.search_revision,
            hits: self.window.search.revision(),
            current: self.window.search.current_index(),
        })
    }

    /// Fold every rail that is changing mode this frame — see
    /// [`cmdrail::RailCache::fold_for_mode`].
    fn fold_rails_on_mode_change(&mut self) {
        let modes: Vec<(SeatId, bool)> = self
            .window
            .command_rails
            .keys()
            .map(|seat| (*seat, self.command_rail_search(*seat).is_some()))
            .collect();
        for (seat, searching) in modes {
            if let Some(cache) = self.window.command_rails.get_mut(&seat) {
                cache.fold_for_mode(searching);
            }
        }
    }

    /// Every rail on screen, at whatever temperature its own clocks have reached,
    /// with the jump flash under them.
    ///
    /// The rails themselves come out of [`cmdrail::RailCache`] and are usually not
    /// rebuilt at all: a frame that moved neither a ledger nor a pane nor an
    /// unfolded bucket hands back the layer it handed back last time, which is the
    /// whole reason `command_marks_revision` exists. What the cache cannot hand
    /// back is a rail with a hand on it — `hot` recolours every tick and the crest
    /// travels — so [`cmdrail::RailCache::picture`] paints those afresh and caches
    /// only the resting one.
    pub(in crate::runtime) fn command_rail_layers(&mut self) -> Vec<marks::OverlayLayer> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let palette = bt_render::chrome_palette();
        let motion = self.app.motion;
        let now = Instant::now();
        let flash = self.command_flash_layer(&palette, scale);
        // Before anything is keyed: a rail that is changing mode folds whatever
        // the fisheye had open, because the stack it was open over is about to be
        // replaced. See [`cmdrail::RailCache::fold_for_mode`].
        self.fold_rails_on_mode_change();
        // Pass one, under a shared borrow: every rail that is drawn this frame,
        // and — for the ones whose key has moved — the geometry to draw it with.
        // See [`cmdrail::RailCache::needs_rebuild`] for why the question and the
        // answer are asked in two passes.
        let seats: Vec<SeatId> = self.sessions.keys().copied().collect();
        let plans: Vec<(SeatId, cmdrail::RailKey, Option<cmdrail::Rail>)> = seats
            .into_iter()
            .filter_map(|seat| {
                let body = self.command_rail_body(seat)?;
                let leaf = self.sessions.get(&seat)?;
                let key = cmdrail::RailKey {
                    revision: leaf.session.command_marks_revision(),
                    body,
                    scale,
                    // The pointer's, and the one thing about the pointer a rail's
                    // *geometry* is a function of: a fisheye genuinely moves ticks.
                    expanded: self
                        .window
                        .command_rails
                        .get(&seat)
                        .and_then(|cache| cache.pointer().expanded()),
                    search: self.command_rail_search(seat),
                };
                let stale = self
                    .window
                    .command_rails
                    .get(&seat)
                    .is_none_or(|cache| cache.needs_rebuild(key));
                Some((
                    seat,
                    key,
                    stale.then(|| {
                        cmdrail::lay_out(body, &self.command_rail_stack(seat), scale, key.expanded)
                    }),
                ))
            })
            .collect();
        // Pass two, exclusive: store what changed and hand out the pictures.
        let mut layers: Vec<marks::OverlayLayer> = flash.into_iter().collect();
        for (seat, key, fresh) in plans {
            let cache = self.window.command_rails.entry(seat).or_default();
            if let Some(rail) = fresh {
                cache.install(key, rail, &palette);
            }
            // An empty ledger draws nothing and reports nothing — `cmd.exe` and a
            // PowerShell without the integration script live here (inventory C13).
            if cache.rail().ticks.is_empty() {
                continue;
            }
            layers.push(cache.picture(&palette, now, motion));
        }
        layers
    }

    /// Drop the rails of seats that are no longer terminals of the tab on screen.
    ///
    /// A cache keyed by seat outlives the seat unless something says otherwise,
    /// and a torn-out pane leaves one behind that would be handed back to whatever
    /// seat id the solver reuses next. The sweep runs where the overlay is built,
    /// so it is a function of the same solve everything else that frame is.
    pub(in crate::runtime) fn sweep_command_rails(&mut self) {
        if self.window.command_rails.is_empty() {
            return;
        }
        let live: BTreeSet<SeatId> = self.sessions.keys().copied().collect();
        self.window
            .command_rails
            .retain(|seat, _| live.contains(seat));
        if self
            .window
            .command_rail_hover
            .is_some_and(|(seat, _)| !live.contains(&seat))
        {
            self.window.command_rail_hover = None;
        }
    }

    pub(in crate::runtime) fn drive_command_rail_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<bool> {
        let now = Instant::now();
        let motion = self.app.motion;
        let palette = bt_render::chrome_palette();
        // Pass one, shared: the fisheye and the crest, settled together against
        // the ledger — see [`cmdrail::resolve`].
        let settled = position.and_then(|position| self.settle_command_rail(position));
        let hover = settled
            .as_ref()
            .and_then(|(seat, _, resolved)| resolved.nearest.map(|index| (*seat, index)));
        let mut changed = self.window.command_rail_hover != hover;
        self.window.command_rail_hover = hover;
        // Pass two, exclusive. Every rail the pointer is not on cools and folds
        // whatever it had open — the mock-up's `railReset`, which does exactly
        // these two things in this order.
        let cold: Vec<SeatId> = self
            .window
            .command_rails
            .keys()
            .copied()
            .filter(|seat| hover.map(|(hot, _)| hot) != Some(*seat))
            .collect();
        for seat in cold {
            let Some(cache) = self.window.command_rails.get_mut(&seat) else {
                continue;
            };
            let ticks = cache.rail().ticks.len();
            // Folding is expressed as a key change and paid for by the next
            // [`Self::command_rail_layers`], rather than laid out here: a rail
            // nobody is pointing at can afford to fold on the frame it is drawn.
            changed |= cache.pointer_mut().expand(None);
            cache.pointer_mut().aim(ticks, None, false, now, motion);
        }
        if let Some((seat, key, resolved)) = settled {
            let cache = self.window.command_rails.entry(seat).or_default();
            if cache.needs_rebuild(key) {
                cache.install(key, resolved.rail, &palette);
            }
            let ticks = cache.rail().ticks.len();
            changed |= cache.pointer_mut().expand(resolved.expanded);
            cache
                .pointer_mut()
                .aim(ticks, resolved.nearest, true, now, motion);
        }
        if changed {
            // The finger, and the crest, on the same reading — see
            // [`pointer_cursor`]'s `over_command_tick`.
            self.apply_pointer_cursor();
            // **The chrome and not just the overlay**, because the crest moving
            // changes *what can be tipped* and the tippable list is a product of
            // the chrome build ([`Self::rebuild_tooltip_anchors`], "what the
            // strip draws is what can be tipped, and both are decided here or
            // neither is"). Refreshing only the overlay repainted the rail and
            // left the anchor list as the last chrome build had it — with no
            // `CommandTick` entry in it at all — so `tooltip_layer` settled its
            // 380ms, went looking for the anchor it had been promised, found
            // nothing, and drew no card. Measured on a real window: hovering a
            // tick lit the rail and never produced a glance card unless some
            // unrelated event happened to rebuild the chrome underneath it.
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
        }
        Ok(hover.is_some())
    }

    /// Which rail the pointer is on, what its geometry should be, and which tick
    /// is the crest — all three under a shared borrow, so the answer can be stored
    /// under an exclusive one.
    ///
    /// The band is tested **before** the fisheye is asked anything, and it is
    /// tested at the temperature the rail is currently drawn at (see
    /// [`cmdrail::Rail::hot_bounds`] for why that asymmetry is safe). Asking the
    /// other way round would let a pointer that is not on the rail at all open a
    /// bucket on it.
    fn settle_command_rail(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(SeatId, cmdrail::RailKey, cmdrail::Resolved)> {
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        let body = self.command_rail_body(seat)?;
        let cache = self.window.command_rails.get(&seat)?;
        cmdrail::nearest(
            cache.rail(),
            self.window
                .command_rail_hover
                .is_some_and(|(hot, _)| hot == seat),
            position.x as f32,
            position.y as f32,
        )?;
        let leaf = self.sessions.get(&seat)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let resolved = cmdrail::resolve(
            body,
            &self.command_rail_stack(seat),
            scale,
            cache.pointer().expanded(),
            position.y as f32,
        );
        let key = cmdrail::RailKey {
            revision: leaf.session.command_marks_revision(),
            body,
            scale,
            expanded: resolved.expanded,
            search: self.command_rail_search(seat),
        };
        Some((seat, key, resolved))
    }

    /// Which rail and which tick a point means, in the geometry that is on the
    /// glass.
    ///
    /// Asked of the cache rather than laid out afresh: the band the pointer is
    /// tested against must be the band that was drawn, and one derivation is the
    /// only way to say that. A seat whose rail has not been built this session has
    /// no band and answers nothing.
    fn command_rail_at(&self, position: PhysicalPosition<f64>) -> Option<(SeatId, usize)> {
        let seat = seats::pane_at(&self.seat_layout, position.x, position.y)?;
        self.command_rail_body(seat)?;
        let cache = self.window.command_rails.get(&seat)?;
        cmdrail::nearest(
            cache.rail(),
            self.window
                .command_rail_hover
                .is_some_and(|(hot, _)| hot == seat),
            position.x as f32,
            position.y as f32,
        )
        .map(|index| (seat, index))
    }

    /// While any rail still owes a frame to one of its four clocks, one frame at
    /// the animation's own rate; nothing at all otherwise.
    pub(in crate::runtime) fn command_rail_deadline(&self, now: Instant) -> Option<Instant> {
        self.animating_deadline(self.command_rails_are_moving(now), now)
    }

    pub(crate) fn command_rails_are_moving(&self, now: Instant) -> bool {
        let motion = self.app.motion;
        self.window
            .command_rails
            .values()
            .any(|cache| cache.pointer().is_animating(now, motion))
    }

    /// Pay the rails' frames. The clocks run themselves out — nothing here has to
    /// stop them, because `is_animating` and the paint read the same instants.
    pub(in crate::runtime) fn advance_command_rails(&mut self, now: Instant) -> Result<()> {
        if !self.command_rails_are_moving(now) {
            return Ok(());
        }
        // On the window's own display frame — see [`Self::animation_frame_is_due`].
        if !self.animation_frame_is_due() {
            return Ok(());
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// A press on a rail — **and the one place the rail's two sources part
    /// company** (B40-B41).
    ///
    /// *"Clicking a match tick SELECTS that match (current advances there, count
    /// follows); a plain command tick keeps its normal jump."* Both verbs are
    /// older than this fork: the jump has been here since S1 and
    /// [`search::SearchState::set_current`] since S3, which is why the takeover
    /// costs one `match` and not a second press path. Which of the two a tick
    /// means was decided when the tick was built — see [`cmdrail::Entry::target`]
    /// — so nothing here asks the search whether it is open.
    pub(in crate::runtime) fn press_command_rail(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some((seat, index)) = self.command_rail_at(position) else {
            return Ok(false);
        };
        let Some(target) = self
            .window
            .command_rails
            .get(&seat)
            .and_then(|cache| cache.rail().ticks.get(index))
            .map(|tick| tick.target)
        else {
            return Ok(false);
        };
        match target {
            cmdrail::Target::Command(mark) => {
                let landing = self.jump_to_command_mark(seat, mark)?;
                // **The rail's own station** (`BT_MOUSE_TRACE`). Everything about
                // a press on a tick used to be silent: which tick was hit, which
                // mark it names, and — the question a report of "it jumped to the
                // wrong place" turns on — where the jump could actually land.
                // `window_top == extent` is a jump answered at the ceiling: the
                // mark is inside the last paneful and there is no document below
                // it to scroll, which is not the same sentence as "the rail
                // missed". `relief` is how much of that ceiling the blank rows
                // under the prompt are spending on a formula standing taller than
                // the pane, and it is what moves the ceiling between one press and
                // the next.
                self.mouse_trace(|| {
                    let number = |value: Option<i64>| {
                        value.map_or_else(|| "unresolved".to_owned(), |value| value.to_string())
                    };
                    match landing {
                        Some(landing) => format!(
                            "rail-jump seat={seat:?} tick={index} mark={mark:?} anchor_y={} local_offset={} window_top={} extent={} relief={}",
                            number(landing.anchor_y_subpixels),
                            landing.local_offset_subpixels,
                            number(landing.window_top_subpixels),
                            landing.extent_subpixels,
                            landing.relief_subpixels,
                        ),
                        None => {
                            format!(
                                "rail-jump seat={seat:?} tick={index} mark={mark:?} leave=nothing-to-jump-to"
                            )
                        }
                    }
                });
            }
            cmdrail::Target::Match(hit) => self.select_search_hit(hit)?,
        }
        Ok(true)
    }

    /// **Light the whole pane instead of a row in it** (DESIGN.md §7.55 ⑨).
    ///
    /// The other half of [`Self::jump_to_command_mark`], and deliberately
    /// missing that method's whole first half: **nothing is scrolled**. The two
    /// cases this is drawn for are a command that is still running and a pane
    /// showing an alternate screen, and in both of them a scroll is the wrong
    /// verb — it would take a reader who asked "where is that running" away from
    /// the very output they were being sent to watch, and on an alternate screen
    /// there is no scrollback to move at all.
    ///
    /// The overlay is rebuilt **here**, which is the opposite of the row band's
    /// arrangement one method up and for that arrangement's own reason: the ring
    /// is placed out of the seat layout, which the frame on the glass already
    /// agrees with, so there is no later frame to wait for. The row band waits
    /// because its rectangle is read out of a frame that has not been composed
    /// yet.
    pub(in crate::runtime) fn flash_pane(&mut self, seat: SeatId) -> Result<()> {
        self.window.command_flash = Some(CommandFlash {
            seat,
            band: FlashBand::Pane,
            started: Instant::now(),
        });
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether a seat can hold a capsule at all: a terminal, on its primary screen.
    ///
    /// **The alternate screen is the whole of the second half** (D-5). §3.2 keeps the two screens
    /// in isolated anchor namespaces with no ordering between them, so a search over the primary
    /// history could not place its hits against what `vim` is drawing; and a capsule that stayed up
    /// over a full-screen program would sit there reading `0/0` for as long as the program ran,
    /// which is R3's named failure. `Ctrl+F` is not in the table there either, so the key reaches
    /// the program and means whatever the program says it means.
    ///
    /// **A page is the second host** (§7.7 ②, W2 slice ④). The mock-up's own
    /// selector has read `.term, .pv-web-doc` since W1, and the ruling is that
    /// this is a second host and not a second implementation: the same capsule,
    /// in the same corner, opened with the same chord. What differs is who
    /// counts the matches — see [`Self::refresh_search`].
    pub(in crate::runtime) fn seat_can_search(&self, seat: SeatId) -> bool {
        if self.seat_holds_a_page(seat) {
            return true;
        }
        self.sessions
            .get(&seat)
            .is_some_and(|leaf| !leaf.session.terminal_modes().alternate_screen)
    }

    /// Whether the tab in front holds a page on that seat.
    pub(in crate::runtime) fn seat_holds_a_page(&self, seat: SeatId) -> bool {
        self.web_on(seat).is_some()
    }

    /// Decide which panes owe a strip, and give the answer to the tree.
    ///
    /// **The one place the projection is written.** The facts live on the leaves
    /// — which PowerShell this is, what its own `$PROFILE` says, whether it has
    /// spoken, whether a marker has arrived — and `Seats` holds the answer only
    /// because the pane's body is measured from it. Recomputed whole rather than
    /// edited, so a pane that has gone cannot leave a seat behind in the set.
    ///
    /// A change here is a **layout** change: a strip takes a row off the
    /// terminal, so the seats have to be solved again and the shells told what
    /// they now have. That is why this ends in `commit_seat_geometry` and why
    /// nothing else has to remember to.
    pub(crate) fn settle_pane_notices(&mut self) -> Result<()> {
        let offering = self
            .app
            .settings_store
            .loaded()
            .powershell_integration_offer;
        // **This run's one ask, carried through the loop and put back after it**
        // (user ruling 2026-08-27 — `shell_integration::offer_once_per_run` is
        // the rule). A local because the loop borrows the tab's leaves and the
        // count lives on the application; copying a `bool` in and out is the
        // whole of the reconciliation, and it cannot go stale because nothing
        // between the two lines can reach the field.
        let mut already_asked = self.app.powershell_integration_asked;
        // **The intent the first-run card left behind, and the shell that
        // finally answers it** (§7.56 §4.3). Where a `$PROFILE` is comes from
        // the shell and is never computed here, so a row left on when the card
        // was answered is a row waiting for exactly this moment: the first
        // PowerShell in this process to name its own profile.
        //
        // Not gated on `offering`: the intent is an instruction the reader gave,
        // and whether *strips* are offered is a different question they answered
        // on the same card.
        let pending = self.app.settings_store.loaded().powershell_install_pending;
        let mut named_profile: Option<PathBuf> = None;
        let mut presence = std::collections::BTreeSet::new();
        let mut states: std::collections::BTreeMap<NoticeHost, notice::Notice> =
            std::collections::BTreeMap::new();
        for (seat, leaf) in &mut self.sessions {
            if leaf.integration_offer.is_none() {
                // Not a PowerShell at all, or the machine has not answered where
                // this one's `$PROFILE` is yet. Neither is a decision, so neither
                // is written down: the probe wakes the loop when it lands.
                let asked = leaf
                    .program
                    .as_deref()
                    .filter(|program| offering && shell_integration::is_powershell(program))
                    .and_then(shell_integration::profile_probe)
                    .flatten();
                if let Some(profile) = asked {
                    leaf.integration_offer = Some(shell_integration::offer_once_per_run(
                        &profile,
                        &mut already_asked,
                    ));
                }
            }
            // Asked of every PowerShell pane rather than only of the ones the
            // strip is computed for, because the probe is cached per program and
            // the intent is owed an answer even on a machine whose reader has
            // switched the strip off. The first answer wins; the rest cost a
            // lookup.
            if pending && named_profile.is_none() {
                named_profile = leaf
                    .program
                    .as_deref()
                    .filter(|program| shell_integration::is_powershell(program))
                    .and_then(shell_integration::profile_probe)
                    .flatten();
            }
            let showing = offering
                .then_some(leaf.integration_offer.as_ref())
                .flatten()
                .and_then(|offer| {
                    offer.showing(
                        leaf.output_revision > 0,
                        leaf.session.shell_integration_seen(),
                    )
                });
            if let Some(state) = showing {
                presence.insert(*seat);
                states.insert(NoticeHost::Seat(*seat), state);
            }
        }
        self.app.powershell_integration_asked = already_asked;
        // **The intent is spent here, after the borrow ends**, because writing
        // it down is a write to the settings store and the loop above holds the
        // panes.
        if let Some(profile) = named_profile {
            self.spend_powershell_intent(&profile);
        }
        // **And the previews, on exactly the same terms** (user ruling
        // 2026-08-29). A document whose file moved under unsaved edits, and one
        // whose file is gone, each owe their reader a sentence — and the band
        // that carries it is the one already here rather than a second one of
        // its own, because a reader who meets both must not meet two heights and
        // two ideas of where the `×` is.
        //
        // Recomputed whole beside the shells' answer and not merged into it: the
        // facts come from a *pool* rather than from `sessions`, and a preview
        // seat can never be a terminal seat, so the two passes cannot disagree
        // about one seat.
        //
        // **No `presence` entry since 2026-09-12** (owner's ruling). A preview
        // pane's news is a pill floating over the bottom edge of its document
        // (`seats::news_pill_box`) and takes no row from the layout at all, so
        // the set that makes a body one bar shorter is the shells' alone. That
        // is the whole of "no reserved foot" in this pass: nothing here can make
        // a preview seat wear a band, so `Seats::seat_wears_notice` is false for
        // every one of them and `preview_body_viewport` subtracts nothing.
        for seat in self.seats.preview_seats() {
            let Some(state) = self.preview_disk_notice_on(self.preview_here(seat)) else {
                continue;
            };
            states.insert(NoticeHost::Seat(seat), state);
        }
        // **And the windows torn off them, on exactly the same terms** (B1,
        // 2026-09-01, §7.39's 总则).
        //
        // The news already arrived here — `refresh_preview_file` walks the
        // *pool*, and a floated document reads its tab's pool through
        // `PreviewSurface::Float` like any other surface — so what was missing
        // was never the fact. It was the row: the two loops above are the tree's
        // two kinds of leaf, a float is in neither, and a window whose file had
        // been replaced under unsaved edits went on showing the old body with
        // nothing said about it.
        //
        // No `presence` entry, and that is not an omission: `Seats::set_notices`
        // is what makes a *pane's* body one bar shorter, and a float's body is
        // derived from its frame every frame by `float_head_tools` →
        // `float_geometry`, which asks this same question directly. One answer,
        // two ways of spending it — the tree is told, and the window simply reads.
        for id in self.preview_float_ids() {
            let Some(state) = self.preview_disk_notice_on(PreviewSurface::Float(id)) else {
                continue;
            };
            states.insert(NoticeHost::Float(id), state);
        }
        // **Two changes, not one, and they gate different work.** Whether a seat
        // *wears* a strip decides the pane's height, so a change to that set is a
        // layout change and re-solves. Which strip it wears — `Offer` before the
        // write, `Added` after it — changes only what is drawn in a row that is
        // already there, so a change to the *content* repaints the overlay and
        // moves no rectangle. Folding the two into one set was the bug the live
        // run caught: pressing `Add` turned `Offer` into `Added` without adding
        // or removing a seat, the set was equal, and the strip went on saying
        // "not installed" over a profile it had just written to.
        let geometry_moved = self.seats.set_notices(presence);
        let content_changed = self.window.notice_states != states;
        self.window.notice_states = states;
        if geometry_moved {
            self.commit_seat_geometry()?;
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        if content_changed && self.refresh_overlay() {
            return self.present_chrome_change();
        }
        Ok(())
    }

    /// Take one pane's strip down without deciding anything.
    ///
    /// [`shell_integration::Offer::Closed`] and not `Silent`: nothing was
    /// answered. What that still buys the reader is the *next launch* — this run
    /// has spent its one ask either way (user ruling 2026-08-27,
    /// [`shell_integration::offer_once_per_run`]), while `Don't show again`
    /// writes the setting and ends the asking for good. Before that ruling the
    /// difference was the next PowerShell pane, which is how a reader with four
    /// of them was asked four times.
    pub(in crate::runtime) fn close_pane_notice(&mut self, host: NoticeHost) -> Result<()> {
        if let NoticeHost::Seat(seat) = host
            && let Some(leaf) = self.sessions.get_mut(&seat)
        {
            if leaf.integration_offer.is_none() {
                return Ok(());
            }
            leaf.integration_offer = Some(shell_integration::Offer::Closed);
            return self.settle_pane_notices();
        }
        // **A preview's `×` is `Keep my edits`** and its own verb is the same
        // door (user ruling 2026-08-29): both of them mean "I have read this",
        // and the edits were never in danger from anything but a press on the
        // other word. A deleted file's strip has no other word, so this is the
        // whole of what can be said back to it.
        //
        // **And a torn-off window's `×` is the same sentence** (B1,
        // 2026-09-01): `notice_surface` is what makes the two hosts spend one
        // verb on one buffer.
        let surface = self.notice_surface(host);
        if !self
            .preview_buffer_on_mut(surface)
            .is_some_and(preview::PreviewBuffer::keep_this_body)
        {
            return Ok(());
        }
        self.settle_pane_notices()
    }

    /// The restore prompt's placement, or `None` when it is not asking.
    ///
    /// Every string it draws has to be measured with the real font before the
    /// box that holds them can be sized, which is why the content is built here,
    /// where the renderer is, and handed to a module that knows only numbers.
    pub(in crate::runtime) fn restore_layout(&mut self) -> Option<restore::RestoreLayout> {
        if !self.window.restore_prompt.is_open() || self.app.restore_question.is_empty() {
            return None;
        }
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        // **The application's list, not this window's** (multiwindow slice D,
        // ruling ①). The card names every tab the launch is asking about — this
        // window's, the other open windows', and the windows that have not opened
        // at all — because there is one question. Which window each row goes back
        // to is not read here; it is where the row already lives.
        let rows = self
            .app
            .restore_question
            .iter()
            .map(|tab| {
                let seed = restore_row_seed(tab);
                let mut row =
                    restore::RestoreRow::from_seed(&seed, persisted_pane_count(&tab.root));
                row.label_text_width = measure(&row.label, restore::ROW_FONT_LOGICAL_PX * scale);
                row.cwd_text_width = measure(&row.cwd, restore::ROW_CWD_FONT_LOGICAL_PX * scale);
                row.badge_text_width = row.badge_text().map_or(0.0, |text| {
                    measure(&text, bt_render::WINDOW_TAB_BADGE_FONT_LOGICAL_PX * scale)
                });
                row
            })
            .collect();
        let content = restore::RestoreContent {
            rows,
            sub_lines: restore::wrap(
                restore::sub_text(),
                restore::content_width(width, scale),
                |text| measure(text, restore::SUB_FONT_LOGICAL_PX * scale),
            ),
            decline_text_width: measure(
                restore::decline_text(),
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            restore_text_width: measure(
                restore::restore_text(),
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
        };
        Some(restore::layout(&content, width, height, scale))
    }

    /// The rail's own level of the stack — [`rail_overlay_layer`] with this
    /// window's two answers filled in.
    ///
    /// The fold is sampled *here* rather than inside the lift because it is a
    /// reading of a clock, and a pure function that asks the time is a function
    /// nothing can pin. Reading it once at the layer rather than baking it into
    /// each sprite is also what keeps the fade from compounding with the label
    /// fade the rail already runs (Q183): they are two declarations on two
    /// elements, and CSS multiplies them exactly once each.
    pub(in crate::runtime) fn rail_overlay_layers(&self) -> Vec<marks::OverlayLayer> {
        // **The fold belongs to the ordinary rail, and to nothing else**
        // (§7.1.6b′: the `Sidebar` row's three rest states govern the *ordinary*
        // panel, and the card column is card-width whatever they say). A window
        // whose sidebar was folded away and then entered focus mode kept the
        // fold's `opacity: 0` on the panel layer: the column was solved at its
        // full width, the stage started after it, the hit test answered its
        // cards — and nothing was drawn there. A card-wide hole with live
        // buttons in it,
        // which is worse than either of the two states it was between.
        //
        // `width_logical_px` already refuses the fold for the same reason; this
        // is that refusal on the paint side, which is where it was missing.
        let fold = self
            .window
            .rail_fold
            .sample(Instant::now(), self.app.motion)
            .0;
        rail_overlay_layer(
            &self.window.rail_chrome,
            panel_opacity(self.rail_posture(), fold),
        )
    }

    /// **The pane whose picture the stand-in wears** (缺陷 #189) — the local
    /// half of [`Runtime::strip_stand_in`], as the leaf rather than as a face.
    ///
    /// The stand-in was a *blank* card until this existed, and on a card column
    /// that is very nearly no card at all: the body is `--termbg` on a panel of
    /// almost the same value, so what the reader saw where their pane was about
    /// to land was an accent hairline around nothing. The head has said the
    /// right name since K124 and it was never enough — the whole of a card in
    /// this mode is its picture, and every other card in the column was showing
    /// one.
    ///
    /// **The picture is the one the column already has** (user ruling
    /// 2026-08-29): the seat's own projection, out of the very cache the pane's
    /// home card is drawn from, so the stand-in and the stage cannot be two
    /// readings of one shell. Nothing is projected a second time — see
    /// [`Self::refresh_focus_thumbnails`] for the one thing that *is* arranged,
    /// which is that the home tab goes on being projected while its card is
    /// scrolled out from under the hand.
    ///
    /// `None` for a visitor's stand-in and that is the honest answer, not a gap:
    /// the pane belongs to another window's process, this window has never drawn
    /// it, and what travels on the broker is a label ([`GhostFace`]) and a tree.
    pub(crate) fn stand_in_pane(&self) -> Option<LeafId> {
        // The same door, read in the same order [`Runtime::strip_stand_in`]
        // reads it: a visitor's slot is not this window's pane's.
        if self.window.foreign.is_some() {
            return None;
        }
        let drag = self.window.drag.as_ref()?;
        let DropLanding::StripExtract { .. } = drag.landing? else {
            return None;
        };
        let DragSource::Pane(leaf) = drag.source else {
            return None;
        };
        // Through the pane's **own** tab, by id, for `strip_stand_in`'s reason:
        // the spring may have moved the stage, and the seat this card is a
        // picture of is a fact about the tree it is still filed under.
        self.tab_state(leaf.tab)?
            .seats
            .tree()
            .find_seat(leaf.seat)?;
        Some(leaf)
    }

    /// **The pane this window has in its hand, wherever the hand has gone** (B2,
    /// 2026-09-01).
    ///
    /// [`Self::stand_in_pane`] answers about a stand-in *this* window is
    /// drawing, so it goes quiet the moment the pointer crosses onto another
    /// window's glass — which is exactly when the picture is wanted most. This
    /// is the other half: the payload the broker says this window is holding, as
    /// long as it is holding it and whatever is under the pointer.
    ///
    /// Read off the broker rather than off the drag, because the broker is the
    /// one register of "a gesture is in flight and this window owns it" that
    /// survives the pointer leaving — the same reason `strip_guests` reads
    /// `strip_stand_in` instead of re-deriving it.
    pub(crate) fn carried_pane(&self) -> Option<LeafId> {
        let broker = self.app.drag_broker.as_ref()?;
        if broker.source != self.window_id() {
            return None;
        }
        let leaf = broker.cargo.pane()?;
        // Still in the tree it is filed under: a gesture whose seat was reaped
        // mid-flight carries nothing, which is `stand_in_pane`'s own last line.
        self.tab_state(leaf.tab)?
            .seats
            .tree()
            .find_seat(leaf.seat)?;
        Some(leaf)
    }

    /// **P177 — the veil over every pane that is still arriving**, on the panes
    /// this tab is actually drawing.
    ///
    /// The clock is the one [`Runtime::refresh_overlay`] read for the whole
    /// build, and the rectangles are the live solve rather than anything the
    /// tween remembers: an arriving pane's box is whatever the solver says it is
    /// this frame, including after a window resize that happened mid-fade.
    pub(in crate::runtime) fn pane_fade_veils(&self, now: Instant) -> Vec<marks::OverlayLayer> {
        pane_fade_veil_layers(
            &self.window.pane_motion,
            &self.pane_rects(),
            now,
            self.app.motion,
            bt_render::chrome_palette(),
        )
    }

    /// **What kind of pane a landing is aimed at**, or `None` for the rim and
    /// the strip's three, which are aimed at no pane at all.
    ///
    /// One sentence rather than the same three-line chain at each of the places
    /// that asks it, and the places are the ones the 2026-09-16 ruling made ask:
    /// the caption on the box, the commit behind it, and the plan that decides
    /// whether the box is drawn at all. A row's centre now means one of three
    /// things depending on the pane under it, so "which pane is under it" had
    /// better be one reading.
    fn aimed_seat_kind(&self, landing: DropLanding) -> Option<bt_layout::SeatKind> {
        landing
            .aimed_at()
            .and_then(|seat| self.seats.tree().find_seat(seat))
            .map(|seat| seat.kind)
    }

    /// The dock drawing's layers: the destinations, then the box over them.
    ///
    /// Under every menu and every tip, because the mock-up puts them there
    /// (`#dock-shift` 24 and `#dock-preview` 25 against `.combo-menu`'s 30 and
    /// `.tip`'s 60) — this is a drawing *on* the layout, not a surface floating
    /// over the window.
    pub(in crate::runtime) fn dock_overlay_layers(&self, now: Instant) -> Vec<marks::OverlayLayer> {
        let Some(shown) = self.window.drop_preview.as_ref() else {
            return Vec::new();
        };
        let reveal = shown.reveal.sample(now, self.app.motion).0;
        if reveal <= 0.0 {
            return Vec::new();
        }
        let overlay = seats::dock_overlay(
            &shown.plan,
            &self.seat_layout,
            self.layout_host_rect(),
            shown.inputs.landing.aimed_at(),
            // A refused box carries no word: what it says is said by being
            // dashed and empty, and "Swap panes" printed inside an outline that
            // means "this will not happen" is the box arguing with itself.
            if shown.plan.fits() {
                shown.inputs.landing.caption(
                    &shown.inputs.source,
                    self.aimed_seat_kind(shown.inputs.landing),
                )
            } else {
                ""
            },
            self.window.renderer.metrics().scale_factor as f32,
        );
        let Some(overlay) = overlay else {
            return Vec::new();
        };
        let mut layers = seats::build_dock_overlay(
            &overlay,
            self.window.renderer.metrics().scale_factor as f32,
            bt_render::chrome_palette(),
        );
        for layer in &mut layers {
            layer.opacity = reveal;
        }
        layers
    }

    /// **Whether one of the popups now up is one the sidebar grew** —
    /// §7.1.6e″'s rule ①, as the question the rail's zone asks.
    ///
    /// A popup owned by the rail is part of the rail, so the pointer being in it
    /// is the pointer being in the rail; and because at most one popup is ever
    /// up (E61) the rail stays out for exactly as long as that popup does. That
    /// covers the corridor between the panel and the menu without a grace of its
    /// own — the hand crossing pixels that belong to neither is crossing them
    /// while the menu is still up, which is the same answer at both ends.
    fn rail_grew_a_popup(&self) -> bool {
        let up = self.popups_up();
        let tabs = self.tab_surface_now();
        Popup::ALL.into_iter().any(|popup| {
            up.holds(popup) && popup_owner(popup, tabs) == PopupOwner::Tabs(TabSurface::Rail)
        })
    }

    /// **Put away every popup the sidebar grew** — §7.1.6e″'s rule ③.
    ///
    /// [`Self::close_popup`]'s own loop, filtered by owner: what leaves with the
    /// panel is what the panel raised, and nothing else. Nothing is repainted
    /// here, for [`Self::close_popups_except`]'s reason — the one caller is
    /// mid-way through a posture change that repaints at its end.
    fn close_rail_popups(&mut self) {
        let tabs = self.tab_surface_now();
        for popup in Popup::ALL {
            if popup_owner(popup, tabs) == PopupOwner::Tabs(TabSurface::Rail) {
                self.close_popup(popup);
            }
        }
    }

    /// Turn the Git page on or off for the whole product (user ruling,
    /// 2026-08-15).
    ///
    /// **Off throws away what was read and stops the reading**, in that order.
    /// Dropping every column's [`git::GitCache`] is the honest half of the
    /// promise: a switch that stopped asking but kept a repository's file list in
    /// memory would still be holding what the user just said they did not want it
    /// to hold. Nothing is *asked* again either, because the gate that starts a
    /// question is in `files_tree_walk` and reads this same setting.
    ///
    /// **What is not touched is each column's own page** (see
    /// [`seats::FilesLeafState::view`]). A column that was on Git falls back to
    /// its tree while the switch is off and returns to Git when it is on again —
    /// the switch decides reachability, the column remembers the choice, and
    /// forcing every column back to Files here would silently spend a decision
    /// the user made about something else.
    /// **Which way a direction-less split cuts**, written down (user ruling,
    /// 2026-08-16).
    ///
    /// The shortest `apply_*` in this dialog, and the reason is worth stating:
    /// nothing on screen depends on it. Every other switch here changes what is
    /// drawn or what is running, so its verb has a second half; this one changes
    /// what the *next* split does, and there is no next split until somebody asks
    /// for one. `Runtime::settings_split_axis` reads the store at the moment of
    /// the split, so there is no copy of this value anywhere to keep in step.
    pub(crate) fn apply_split_direction(
        &mut self,
        direction: bt_persist::SplitDirectionV1,
    ) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.split_direction = direction;
        Ok(self.app.settings_store.store(settings))
    }

    /// **Everything that follows a change to the *set* of seats in a tab**, in
    /// the one place every verb that changes it now goes through.
    ///
    /// A function rather than five copies of four lines, because the four lines
    /// have now been forgotten twice in two different verbs and the symptom is
    /// the same both times and legible from nowhere: the tree changes, the
    /// layout re-solves, the panes are *drawn* in their new rectangles — and the
    /// shells beside the arriving pane are never told they have fewer columns,
    /// so the next prompt reprints itself into a width that no longer exists and
    /// loses its middle. `dock_float` was the second (it re-solved the layout and
    /// stopped there); the audit that found it read every call that mutates the
    /// tree, and this is where they all end now.
    ///
    /// The order is the one [`Self::toggle_files_pane`] established:
    ///
    /// 1. **the pointer's picture first**, because every rectangle it named has
    ///    moved and a `pane_hover` naming a seat that is gone keeps a `×` lit on
    ///    a pane that no longer exists;
    /// 2. **the drag**, because a divider in flight was steering a split that
    ///    may not be in the tree any more;
    /// 3. **the window's own minimum**, which is a floor derived from the seats
    ///    and so moves with them;
    /// 4. **the re-solve**, which is the step that actually carries the new
    ///    columns to every shell — and marks the session dirty on its way out,
    ///    which is why nothing here does that a second time.
    ///
    /// What is deliberately *not* here is anything about a particular kind of
    /// pane: a closing preview drops its image, a closing files column drops its
    /// root, a split moves the keyboard. Those belong to their verbs. This is
    /// only the part that is true whenever the count of seats changes at all.
    ///
    /// The preview sweep **is** true whenever the count changes, which is why it
    /// is here and not in a verb: a leaf can stop being a preview surface by any
    /// of a dozen edits (closed, replaced, torn into a tab, restored over), and a
    /// view left behind is a buffer this tab still believes is on screen — which
    /// is precisely what the pool's dirty gates read.
    pub(crate) fn settle_seat_set_change(&mut self) -> Result<()> {
        self.window.seat_pointer = seats::ChromePointer::default();
        self.window.divider_drag = None;
        self.sweep_preview_panes();
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()
    }

    /// `closePane` (mock-up 3558-3578, I102/I103/I105).
    ///
    /// One verb for every kind of leaf: the leaf leaves the tree, its sibling is
    /// promoted, and the run it left is re-balanced — all of which `close_seat`
    /// already does, because G79/G80 rule that leaving is balanced exactly as
    /// joining is.
    ///
    /// The branch that is new here is the last one. `detachLeaf` returns null
    /// when the tree holds a single pane, and the mock-up's answer to that is not
    /// "refuse" but `closeTab(w.id)` — **an empty tab is not a state that
    /// exists** (T226/§2.1), so closing the last pane *is* closing the tab. That
    /// falls through to `close_tab`, which keeps its own rule about the last tab
    /// in the strip: the window does not empty either. The question of *when*
    /// that branch is taken is [`closing_this_pane_closes_the_tab`], and it is
    /// not only about the pane count.
    ///
    /// **One confirmation, and only one pane earns it** (I103's second half, now
    /// that there are unsaved buffers for it to be about). A terminal pane's `×`
    /// kills its shell outright; a preview pane's asks — and only when it is the
    /// *last* preview pane in the tab, because the pool outlives any one pane and
    /// only the last one's closing would strand it (P123).
    pub(in crate::runtime) fn close_pane(&mut self, seat: bt_layout::SeatId) -> Result<()> {
        let kind = self
            .seat_layout
            .get(seat)
            .map(|placement| placement.kind)
            .unwrap_or(bt_layout::SeatKind::Terminal);
        // **Gate ① (P123).** Asked before the pane-count branch below, so the
        // question is put once however the close resolves — a preview pane that
        // is also the tab's last pane would otherwise fall through to `close_tab`
        // and be asked by gate ②, which is the same question with a different
        // subject.
        if self.raise_dirty_gate(restore::GateRequest::ClosePane(seat))? {
            return Ok(());
        }
        if closing_this_pane_closes_the_tab(self.seats.pane_count()) {
            return self.close_tab(self.window.active_tab);
        }
        // **And the hovered formula goes with the pane** (audit 2026-09-15,
        // RB-3). Placed here on purpose: both gates have passed, so the close is
        // certain, and nothing has moved yet — the sweep can still reach the
        // session that is about to be removed, and the frame this publishes is
        // struck against a layout that is still the one on the glass.
        self.leave_hovered_math(Instant::now(), MathHoverExit::BandLeftTheScreen)?;
        let metrics = self.seat_metrics();
        if !self.seats.close_seat(&metrics, seat) {
            return Ok(());
        }
        // A preview seat holds an image the way a terminal holds a session, and
        // the pane going away is the one taking it — **that** pane's, not every
        // preview's: closing one leaf beside a pinned one must leave the pinned
        // one showing what it was showing.
        if kind == bt_layout::SeatKind::Preview {
            let surface = self.preview_here(seat);
            self.clear_preview_image(surface);
            self.clear_preview_view(surface);
        }
        // A terminal seat holds a shell, and the pane going away takes that too.
        // Closing the pane and leaving the ConPTY alive would leak a process
        // with nothing to draw it and nothing to read it — the pipe fills, and
        // the child blocks forever on a write no one will ever drain.
        if kind == bt_layout::SeatKind::Terminal
            && let Some(mut leaf) = self.sessions.remove(&seat)
        {
            // And the place it held in this window's queue, which is one of the three doors out
            // (`attention` plan §4 B4): the program never withdrew and nobody answered, so the
            // ledger says `expire … reason=leaf-gone` and the serial stands where it is.
            let index = self.window.active_tab;
            let reach = notify::desktop_reach(true, self.window.place());
            expire_leaf_attention(
                attention::Site { tab: index, seat },
                reach,
                &mut leaf,
                &mut self.window.attention_next_place,
                Instant::now(),
            );
            // **Taken, and taken apart somewhere else** (T-PANE-CLOSE-OFF-THREAD).
            // The leaf is already out of `sessions`; the session comes out of the
            // leaf, and what happens to it next happens on a thread nobody is
            // waiting for. A click that closes a pane may not be answered with a
            // `ClosePseudoConsole` that returns when the program inside has
            // finished winding down.
            if let Some(pty) = leaf.pty.take() {
                bt_pty::retire_session(pty);
            }
            // Keyboard focus cannot stay on a seat that no longer exists. The
            // rule is [`TabState::refocus_after_losing`]'s, shared with the two
            // cross-boundary gestures that also take a leaf out of a tab.
            self.refocus_after_losing(seat);
        }
        // A files column holds a root the way a terminal holds a shell, and the
        // pane going away takes that too. There is no process to shut down —
        // closing a files pane *is* discarding what it was looking at — but the
        // entry has to go, because seat ids are re-minted from a counter and a
        // state left behind comes back as the next column silently inheriting a
        // root nobody chose for it.
        if kind == bt_layout::SeatKind::Files {
            self.files.remove(&seat);
            // And what it had read. The cache is keyed on the same re-minted
            // seat id, so leaving it behind is the same bug one line up in a
            // more confusing shape: the next column would come up already
            // showing somebody else's directories under its own root.
            self.file_trees.remove(&seat);
            // And what it had learned about the repository under it, for exactly
            // the same reason: a re-minted seat id must not come up already
            // holding another column's branches.
            self.git_trees.remove(&seat);
        }
        debug_assert!(
            self.sessions_match_terminals(),
            "item 6: closing a pane leaves the tab's shells matching its tree"
        );
        debug_assert!(
            self.files_match_files_seats(),
            "A3: closing a pane leaves the tab's files states matching its tree"
        );
        self.settle_seat_set_change()?;
        // And then, uniquely to closing: the pointer is standing still over
        // whatever moved *into* the gap, so the hover it lost above is
        // immediately owed again from the new rectangles.
        if let Some(position) = self.window.pointer_position {
            self.update_chrome_hover(position)?;
        }
        Ok(())
    }

    /// The axis `Alt+Shift+D`'s duplicate cuts along.
    ///
    /// **The setting's answer for the focused pane** (user ruling, 2026-08-16),
    /// which under the default `Auto` is what it always was: across the focused
    /// pane's longer side. A pane the solver has not placed cannot be measured,
    /// and the side-by-side split is the one the dev chord had always used.
    ///
    /// It goes through [`Self::settings_split_axis`] rather than reading the
    /// setting itself, because the ruling is that *every* direction-less split
    /// obeys one answer and a second reader is a second place that can stop
    /// obeying it.
    pub(in crate::runtime) fn duplicate_split_axis(&self) -> Axis {
        self.settings_split_axis(self.focused_leaf)
    }

    /// The axis an "auto" split of **one named pane** cuts along: across its
    /// longer side, so the two halves come out as square as the pane allows.
    ///
    /// Windows Terminal's `duplicatePane` defaults to `split: "auto"` and means
    /// exactly this. The measurement is the solver's own rectangle, never a
    /// guess (red line L10); a pane the solver has not placed cannot be measured
    /// and falls to the side-by-side cut the dev chord always used.
    ///
    /// Named seat rather than the focused leaf, because the pane head's `⊞` is
    /// a button on a *particular* head and the pointer can press it over a pane
    /// that does not hold the keyboard. Reading the focused pane's rectangle
    /// there would cut a tall pane sideways because the wide one next door said
    /// so.
    pub(crate) fn pane_split_axis(&self, seat: SeatId) -> Axis {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)
            .map_or(Axis::Row, |body| auto_split_axis(body.width, body.height))
    }

    /// [`Self::split_focused_terminal`], for a pane named outright, arriving on
    /// this pane's own terms.
    ///
    /// The keyboard's three split chords all mean "the pane I am typing in", and
    /// for seven months that was the only way to ask, so the source was read off
    /// `focused_leaf` inside the verb. The pane head's menu asks about the pane
    /// **under the pointer** (user ruling, 2026-08-15), and while D40 does move
    /// focus into a pane on the way down for a press on its head, leaning on
    /// that would make this verb's subject an accident of routing order rather
    /// than something the call site said. So it is said.
    pub(crate) fn split_terminal_seat(&mut self, source: SeatId, dir: Axis) -> Result<()> {
        self.split_seat(source, dir, false, SplitSeed::Inherit)
    }

    /// **Every split in this window, with the one thing they differ in named.**
    ///
    /// Four call sites and one machine: the chords, the menu's picker, the
    /// menu's four verbs, and the drag that lands a pane on an edge. What varies
    /// between them is the axis, which side the arriving leaf goes on, and what
    /// the arriving shell is — and all three are parameters here rather than
    /// three near-copies of the eighty lines below, which is the shape in which
    /// one of them silently stops re-solving before it spawns.
    ///
    /// `leading` is `Edit::SplitSeat`'s own flag, carried through
    /// `Seats::split_terminal` since the tree existed and passed as `false` by
    /// every caller until the picker's `Left` and `Up` zones needed the other
    /// value. **No layout work was owed for them**: "put the new pane first" is
    /// a tree edit the solver has always been able to make.
    pub(in crate::runtime) fn split_seat(
        &mut self,
        source: SeatId,
        dir: Axis,
        leading: bool,
        seed: SplitSeed,
    ) -> Result<()> {
        let metrics = self.seat_metrics();
        // Ask the solver first. `split_terminal` leaves the tree untouched when
        // it refuses, so there is nothing to undo on this path.
        let Some(arriving) = self.seats.split_terminal(&metrics, source, dir, leading) else {
            return Ok(());
        };
        // Re-solve before spawning: the new pane's shell has to be told how many
        // columns it has, and that answer comes from the solve the split just
        // changed — never invented here (red line L10).
        self.commit_seat_geometry()?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = seats::pane_body_viewport(&self.seats, &self.seat_layout, arriving, scale)
        else {
            // The solver placed no rectangle for the seat it just minted, which
            // would mean the tree and its solve disagree. Undo the split rather
            // than leave a seat nothing can draw.
            self.seats.close_seat(&metrics, arriving);
            self.commit_seat_geometry()?;
            return Ok(());
        };
        // **The arriving pane is the same kind of shell as the pane it came out
        // of, and stands where that pane stands.** Both halves are inherited from
        // the *source leaf* rather than from the tab, which is the whole of what
        // per-leaf profiles buy at this call site: splitting a Git Bash pane used
        // to seat a PowerShell beside it, because a split spawned the default
        // shell whatever the pane was running. Splitting is "another one of
        // these, here", and it is now able to mean it.
        let inherited = self
            .sessions
            .get(&source)
            .map(|leaf| seed.applied(&leaf.profile, leaf.session.working_directory()))
            // A seat with no shell to inherit from has no profile to inherit
            // either, and the fallback is the one answer that is startable by
            // construction. Said here rather than left to `LeafSeed::default()`,
            // whose profile is now the empty id — which names no row and would
            // reach the spawn as a degradation nobody caused.
            .unwrap_or_else(|| seed.applied(profiles::fallback_profile_id(), None));
        let wake = &self.window.pty_wake;
        let formulas = FormulaSwitches::from_settings(self.app.settings_store.loaded());
        let scrollback = scrollback_quota(self.app.settings_store.loaded().scrollback_lines);
        let leaf = create_leaf_session(
            &self.window.renderer,
            body,
            LeafId {
                tab: self.window.tabs[self.window.active_tab].id,
                seat: arriving,
            },
            wake,
            None,
            &inherited,
            &self.app.profile_programs,
            formulas,
            scrollback,
            self.app.settings_store.loaded().line_wrapping,
        )?;
        self.sessions.insert(arriving, leaf);
        debug_assert!(
            self.sessions_match_terminals(),
            "item 6: a split seats a shell for the leaf it minted, and only that"
        );
        // Focus follows the split, keyboard and layout together: you split in
        // order to work in the new pane.
        self.focused_leaf = arriving;
        self.seats.set_focus(arriving);
        self.settle_seat_set_change()?;
        self.refresh_chrome();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// **Which live page one preview surface is showing**, docked or torn off
    /// (§7.7 ⑩ 欠账, 2026-08-25).
    ///
    /// [`Self::web_on`] asked of a surface instead of a seat, and it is the seam
    /// the whole of this row's move across turns on: a docked page is the page
    /// on this tab's leaf, a floated one is the page the window is *carrying*
    /// ([`float::FloatPreview::page`]), and `pop_out_preview` closed the seat
    /// that used to be able to answer for it. A glance card never holds a page —
    /// a hover is a question and a browser is not something a question starts.
    pub(in crate::runtime) fn rail_page(&self, surface: PreviewSurface) -> Option<LeafId> {
        match surface {
            PreviewSurface::Seat(leaf) => self.window.web.contains_key(&leaf).then_some(leaf),
            PreviewSurface::Float(id) => self.page_carried_by(id),
            PreviewSurface::Peek => None,
        }
    }

    /// **The band one preview surface's rail rides on**, in physical pixels.
    ///
    /// The two hosts' one genuine disagreement, and the only thing
    /// [`seats::preview_rail_geometry_in`] has to be told: a docked seat's row is
    /// its pane less its head ([`seats::preview_rail_band`]), a window's is the
    /// strip its own chassis reserved ([`float::FloatGeometry::rail`]).
    /// Everything downstream of this rectangle — where each control stands, what
    /// the pointer is on, where a caret goes — is one derivation for both.
    ///
    /// `None` is "this surface wears no row", which is a real answer for three
    /// different reasons and none of them is a failure: a document with nothing
    /// to point at, a pane on a tab that is not in front, and a window squeezed
    /// past its own floor.
    pub(in crate::runtime) fn rail_band(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<[f32; 4]> {
        match surface {
            PreviewSurface::Seat(leaf) => {
                if leaf.tab != self.id {
                    return None;
                }
                let rect = seats::full_pane_rect(&self.seat_layout, leaf.seat)?;
                Some(seats::preview_rail_band(rect, scale))
            }
            PreviewSurface::Float(id) => {
                let win = self.window.float.drawn().find(|win| win.epoch == id)?;
                let fade = self.float_fade_of(win, Instant::now(), scale);
                // `0.0` for the `DOCK` caption, exactly as `float_body_rect`
                // passes: the row is the frame less its border, its head and its
                // foot, and none of those turn on what the head has room for. It
                // is what keeps this an `&self` question, which every reader of
                // it is.
                float::float_geometry(
                    risen_frame(win.frame, fade),
                    win.mode,
                    scale,
                    0.0,
                    self.float_head_tools(id),
                )
                .rail
            }
            PreviewSurface::Peek => None,
        }
    }

    /// **One preview rail, exactly as it was drawn this frame.**
    ///
    /// The band this surface stands on and the widths the paint stored for it,
    /// put together in the one place — because a tip, a menu and a press that
    /// each built their own would be three rows disagreeing about where a button
    /// is. [`Self::preview_rail_measure`]'s own sentence, with the derivation
    /// finished rather than left to each caller.
    pub(in crate::runtime) fn rail_geometry(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<seats::PreviewRailGeometry> {
        let measure = self.preview_rail_measure(surface)?;
        Some(seats::preview_rail_geometry_in(
            self.rail_band(surface, scale)?,
            scale,
            &measure,
        ))
    }

    /// Put one pane's window at content column `wanted`.
    ///
    /// Expressed through the projection's own mutator so the clamp stays in the
    /// one place that owns it — `HorizontalProjection::new` — and the drag and
    /// the wheel arrive at the same column by the same arithmetic.
    pub(crate) fn scroll_seat_to_column(
        &mut self,
        seat: SeatId,
        wanted: ContentColumn,
    ) -> Result<()> {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        if leaf.projection.horizontal().x_origin() == wanted {
            return Ok(());
        }
        leaf.projection.set_horizontal_origin(wanted);
        self.wake_terminal_column(seat);
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        self.repaint_pane_change(seat)
    }

    /// Move one pane's window `columns` to the right, negative for left.
    pub(in crate::runtime) fn scroll_seat_by_columns(
        &mut self,
        seat: SeatId,
        columns: i32,
    ) -> Result<()> {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        let before = leaf.projection.horizontal().x_origin();
        leaf.projection.scroll_horizontal_by(columns);
        // The wake is unconditional even when the window was already against the
        // end: a reader flicking at a hard stop is still reading sideways, and a
        // mark that went out under their hand would say they had stopped.
        self.wake_terminal_column(seat);
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        if self.leaf(seat).projection.horizontal().x_origin() == before {
            return Ok(());
        }
        self.repaint_pane_change(seat)
    }

    /// Put one pane's view at `wanted` subpixels above the live bottom.
    ///
    /// Expressed as a delta through `scroll_by_subpixels` rather than as a
    /// setter of its own, so the clamp stays in the one place that owns it —
    /// the drag and the wheel end up at the same extent by the same arithmetic.
    pub(crate) fn scroll_seat_to_subpixels(&mut self, seat: SeatId, wanted: i64) -> Result<()> {
        // The bar moves the view like any other scroll, so it lands a band that is still changing
        // face like any other scroll (§7.1.5p ⑪).
        self.settle_math_toggle()?;
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return Ok(());
        };
        let current = leaf.projection.scroll_offset_subpixels();
        if current == wanted {
            return Ok(());
        }
        leaf.projection.scroll_by_subpixels(wanted - current);
        self.woke_terminal_thumb(seat)?;
        self.repaint_pane_change(seat)
    }

    /// Re-solve after a tree edit and carry the consequences to the terminal.
    ///
    /// Deliberately routed through the same coalescer a window resize uses: a
    /// divider drag and an OS resize are the same event as far as ConPTY is
    /// concerned, and §4.2 says the solver does not participate in that
    /// debounce — it answers every frame, and someone else decides when the
    /// child hears about it.
    pub(crate) fn commit_seat_geometry(&mut self) -> Result<()> {
        let trace_started = self.app.trace_perf.then(Instant::now);
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        if render_physical.width == 0 || render_physical.height == 0 {
            return Ok(());
        }
        // A seat rectangle changing is a resize as far as a transient flyout is
        // concerned: its anchor was a physical point on the old pane and its
        // raster was sized to it. tiny-window §3.5 generalises the existing
        // dissolve rule to exactly this case, so the same two lines `resize`
        // already runs run here.
        self.window.peek_hover.clear();
        self.window.renderer.set_peek_overlay(None);
        let now = Instant::now();
        self.defer_preview_resample(now);
        // **U8 — `snapshotPanes()` (mock-up 6557-6562), before the layout moves.**
        //
        // What it measures is `getBoundingClientRect`, which is where each pane
        // *is on screen* — the solved rectangle with whatever transform is still
        // running already applied. That is what makes a second split 80ms into
        // the first one's flight start from the 60% of the way the pane had
        // genuinely travelled, instead of teleporting to a box it never reached.
        // Taken here rather than inside the re-solve because this is the last
        // moment `self.seat_layout` still describes what the user can see.
        let before = self
            .window
            .pane_motion
            .snapshot(&self.pane_rects(), now, self.app.motion);
        self.resolve_seat_layout(render_physical);
        let solved_at = trace_started.map(|_| Instant::now());
        let next_grid =
            self.resize_leaves_to_layout(now, "resize terminal actor for a seat layout change")?;
        let resized_at = trace_started.map(|_| Instant::now());
        // **U8, R5 — only a structural tree change animates.**
        //
        // Every layout-mutating path in this file converges here, and most of
        // them are not structural: a divider drag steers a ratio, a focus change
        // feeds W2's concession ladder, a DPI change re-solves the same tree on
        // a new rectangle. All of them move rectangles; none of them is a pane
        // arriving, leaving or changing places, so "the layout re-solved" cannot
        // be the gate and the tree's own revision is.
        //
        // Nothing is cancelled on the other branch, and that is CSS's answer as
        // much as ours: a transform is expressed *relative* to whatever box the
        // element is currently laid out in, so a flight that is running when the
        // window is resized keeps its remaining offset and decays onto the new
        // rectangle. Both draw seams read the transform against the live solve,
        // which is that behaviour with no extra code.
        if self.window.pane_motion_revision != self.seats.structure_revision() {
            self.window.pane_motion_revision = self.seats.structure_revision();
            let after = self.pane_rects();
            self.window
                .pane_motion
                .begin(&before, &after, now, self.app.motion);
            // The chrome `resolve_seat_layout` built a moment ago was built
            // through the *previous* frame's transforms, because the tweens that
            // decide it did not exist yet. Rebuilt here rather than by moving
            // the solve: only a structural commit pays for it, which is once per
            // split rather than once per divider event.
            self.refresh_chrome();
        }
        self.sync_math_layout_key();
        // The grid actually in force, which inside a coalescing window is not yet the one the
        // child has heard. The present gate admits the grid the frame will really carry, never the
        // one merely solved.
        //
        // A tab with no shell has no grid and nothing to gate: the present it is
        // about to make is chrome and a files column, neither of which is
        // measured in cells (§7.1.6h).
        self.pending_resize_present = self.focused().map(|leaf| leaf.grid);
        self.mark_session_dirty(now);
        self.publish_frame(FrameTrigger {
            occurred_at: now,
            source: FrameSource::Resize,
        })?;
        let published_at = trace_started.map(|_| Instant::now());
        let synchronous_present = self.window.divider_drag.is_none();
        // Pointer motion must stay ahead of the swapchain. `publish_frame` already requested a
        // redraw and `LatestFrameSlot` keeps the newest geometry, so presenting synchronously here
        // would make every divider event wait on GPU acquire/vsync before Windows can deliver the
        // next event. Non-drag seat edits still present immediately; a live drag is frame-paced by
        // RedrawRequested and may coalesce only superseded intermediate positions.
        if synchronous_present {
            self.redraw()?;
        }
        if let (Some(started), Some(solved), Some(resized), Some(published), Some(next_grid)) = (
            trace_started,
            solved_at,
            resized_at,
            published_at,
            next_grid,
        ) {
            trace_sink::stderr_line(format!(
                "BT_PERF_TRACE resize_frame solve_us={} actor_us={} publish_us={} redraw_us={} total_us={} queued={} columns={} rows={}",
                solved.saturating_duration_since(started).as_micros(),
                resized.saturating_duration_since(solved).as_micros(),
                published.saturating_duration_since(resized).as_micros(),
                Instant::now()
                    .saturating_duration_since(published)
                    .as_micros(),
                started.elapsed().as_micros(),
                u8::from(!synchronous_present),
                next_grid.columns,
                next_grid.rows,
            ));
        }
        Ok(())
    }

    /// The row geometry of whichever host this is — **one answer for two
    /// surfaces** (P150).
    ///
    /// Both hosts draw the same list with the same row height and the same
    /// scroll, and both the hit test and the paint already go through
    /// [`seats::files_tree_geometry`]. Asking it once here is what lets the drag
    /// source, the glance card and the press all measure a row the same way; a
    /// second derivation would be a rectangle that disagrees with the one under
    /// the pointer, which is the whole class of bug the hit test's own note is
    /// about.
    pub(in crate::runtime) fn row_geometry(
        &mut self,
        host: RowHost,
    ) -> Option<seats::FilesTreeGeometry> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        match host {
            // A Git page is not a tree and has no tree geometry. Everything that
            // needs a row's rectangle asks [`Self::peek_row_rect`], which knows
            // both; this function answers only for the two hosts that really are
            // one list, and answering `None` here rather than inventing a
            // geometry is what keeps a drag from starting on a page that has no
            // rows to drag.
            // A terminal has no tree geometry either, and for the stronger
            // reason: it has no rows. Its glance is anchored on the run under
            // the pointer ([`Self::terminal_reference_at`]), which is the whole
            // of what a reference has instead of a row rectangle.
            RowHost::Git(_) | RowHost::Terminal(_) => None,
            RowHost::Column(seat) => {
                let trees = self.files_trees(Instant::now());
                let segmented = self.git_panel_on();
                seats::files_tree_geometry_of(&self.seat_layout, &trees, scale, segmented, seat)
            }
            RowHost::Float(id) => {
                let tools = self.float_head_tools(id);
                let dock_label = self.float_dock_label_width(scale);
                let now = Instant::now();
                let win = self.window.float.drawn().find(|win| win.epoch == id)?;
                let files = win.files()?;
                let rows = files::tree_view(&files.files, &files.cache).rows.len();
                let scroll = files.cache.scroll_px;
                let geometry = float::float_geometry(
                    risen_frame(win.frame, self.float_fade_of(win, now, scale)),
                    win.mode,
                    scale,
                    dock_label,
                    tools,
                );
                Some(seats::files_tree_geometry(
                    geometry.body,
                    rows,
                    scroll,
                    scale,
                ))
            }
        }
    }

    /// **The space verb's second half** — the leaf the plan just landed is
    /// empty, and this is what it was landed for.
    ///
    /// Split out from the commit because the *tree* edit and the *content* it
    /// carries are two different failures: a plan that does not fit never
    /// reaches here, and a file that cannot be read still leaves a pane the user
    /// asked for, with the preview's own refusal card in it.
    fn fill_row_leaf(&mut self, payload: &RowPayload, seat: SeatId) -> Result<()> {
        match payload.kind {
            RowPayloadKind::File => {
                self.open_preview_onto(self.preview_here(seat), payload.path.clone())
            }
            RowPayloadKind::Folder => {
                let root = payload.path.display().to_string();
                let active = self.window.active_tab;
                self.window.tabs[active].files.insert(
                    seat,
                    seats::FilesLeafState {
                        root,
                        ..seats::FilesLeafState::default()
                    },
                );
                self.window.tabs[active].file_trees.remove(&seat);
                self.window.tabs[active].git_trees.remove(&seat);
                self.mark_session_dirty(Instant::now());
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
        }
    }

    /// The gate's box this frame, or `None` when nothing is being asked.
    pub(in crate::runtime) fn dirty_gate_layout(&mut self) -> Option<restore::GateLayout> {
        let request = self.window.dirty_gate.request()?.clone();
        let names = self.gate_dirty_names(&request);
        if names.is_empty() {
            return None;
        }
        let lines = request.lines(&names);
        let title = request.title();
        // **`Discard all` when the card is a list, `Discard` when it names one
        // thing** (B1). The word is a fact about the subject, and the subject is
        // what `offers_save` is deciding about too.
        let discard_text = if request.offers_save() {
            restore::gate_discard_all_text()
        } else {
            request.answer_text()
        };
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let room = restore::content_width(width, scale);
        let offers_save = request.offers_save();
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let content = restore::GateContent {
            title,
            // Each line is wrapped on its own, so a file name too long for the
            // dialog runs onto a second line of its own row rather than being
            // run together with the name below it.
            message_lines: lines
                .iter()
                .flat_map(|line| {
                    restore::wrap(line, room, |run| {
                        renderer.measure_chrome_text(gpu, run, restore::SUB_FONT_LOGICAL_PX * scale)
                    })
                })
                .collect(),
            discard_text,
            cancel_text_width: renderer.measure_chrome_text(
                gpu,
                restore::gate_cancel_text(),
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            discard_text_width: renderer.measure_chrome_text(
                gpu,
                discard_text,
                restore::BUTTON_FONT_LOGICAL_PX * scale,
            ),
            save_text_width: offers_save.then(|| {
                renderer.measure_chrome_text(
                    gpu,
                    restore::quit_save_text(),
                    restore::BUTTON_FONT_LOGICAL_PX * scale,
                )
            }),
        };
        Some(restore::gate_layout(&content, width, height, scale))
    }

    /// Where the file menu is, if one is up.
    ///
    /// Unlike [`Runtime::root_menu_layout`] this cannot fold for want of
    /// something to hang from: the anchor is the point the pointer was at, and a
    /// point does not stop existing when the tree behind it is rebuilt — so it is
    /// one of the seven popups [`Runtime::popups_up`] has no stand to ask about.
    /// What the menu *does* fold for is the window changing size under it, which
    /// the caller handles by closing it — a menu is a moment, and a resize ends
    /// the moment.
    pub(in crate::runtime) fn file_menu_layout(&mut self) -> Option<profiles::FileMenuLayout> {
        let menu = self.window.file_menu.as_ref()?;
        let point = menu.point;
        let crumbs: Vec<String> = menu.crumbs.iter().map(|level| level.name.clone()).collect();
        let (subject, powers) = (menu.subject, file_menu_powers(menu.row.as_ref()));
        let look = self.file_menu_look(subject, powers, &crumbs);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::file_menu_layout(
            point,
            (width as f32, height as f32),
            scale,
            &look,
            &mut measure,
        ))
    }

    /// Where the terminal menu is, if one is up.
    ///
    /// [`Runtime::git_menu_layout`]'s twin down to the borrow dance: the look is
    /// a bundle of `Copy` scalars rather than borrows, so unlike the git menu it
    /// needs no owned draw struct — the measure closure can hold the renderer
    /// mutably while the look sits on the stack.
    pub(in crate::runtime) fn term_menu_layout(&mut self) -> Option<profiles::TermMenuLayout> {
        let menu = self.window.term_menu.as_ref()?;
        let (point, look) = (
            menu.point,
            profiles::TermMenuLook {
                pane: menu.pane,
                subject: menu.subject,
                hover: menu.hover,
                lone: menu.lone,
                submenu_open: menu.submenu_open,
            },
        );
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        // **The effective table**, so a rebound chord follows into the menu the
        // same frame it follows into the hint card (gesture audit 2026-08-26,
        // 系统性发现 ②). Borrowed beside the GPU rather than through it: the two
        // are separate fields of `app` and the split borrow is what lets the
        // menu be measured and read from the same expression.
        let shortcuts = &self.app.shortcuts;
        // **The machine**, on the shortcut table's own footing and for the
        // ruling of 2026-09-06: it decides which rows the `Split with ▸` child
        // has, so the menu cannot be measured without it.
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::term_menu_layout(
            point,
            (width as f32, height as f32),
            scale,
            &look,
            shortcuts,
            programs,
            &mut measure,
        ))
    }

    /// `Select all` — **every plane this pane has**, not the screenful in front
    /// of you.
    ///
    /// The anchors come from [`DualPlaneSession::select_all_selection`], which
    /// walks the same three planes a copy is cut from, and they are installed
    /// through [`TabState::set_leaf_selection`] — the one door that also drops
    /// every other pane's selection, because this window holds one.
    pub(in crate::runtime) fn select_all_in_pane(&mut self, seat: SeatId) -> Result<()> {
        let Some(selection) = self
            .sessions
            .get(&seat)
            .and_then(|leaf| leaf.session.select_all_selection())
        else {
            return Ok(());
        };
        self.set_pane_view_selection(seat, Some(selection));
        self.repaint_pane_change(seat)
    }

    /// `Clear screen` — **the screen scrolls away and the row you are typing on
    /// stays**, performed by the terminal on both sides of the pty (§7.1.6,
    /// clear-screen ruling 2026-09-05).
    ///
    /// Nothing is written to the PTY as *input*, which is the whole of the
    /// ruling's first half: `cls` typed at a prompt is a *command*, and a menu
    /// row that typed one for you would land in the middle of whatever
    /// half-finished line was already there, would be refused outright by a
    /// program that is not a shell, and would enter that shell's history.
    ///
    /// **Why the cursor's row is kept, and why the host is told.** The first
    /// draft of this row fed `ESC [ 2 J` `ESC [ H` into this session's own parser
    /// and stopped there. Measured on a real ConPTY (`crates/bt-pty`'s
    /// `clear_screen_keeps_the_prompt_and_the_host_agrees`), that leaves a pane
    /// with no prompt on it and no way to get one: the child's screen lives in
    /// the console host's buffer, ConPTY sends this window only the difference
    /// against what *it* believes is displayed, and a clear it never heard about
    /// gives it no reason to redraw anything. The recorded evidence is that the
    /// child says nothing at all afterwards, and that the next keystroke arrives
    /// as an absolute `CUP` to the row and column the prompt used to end at — a
    /// lone character floating on a blank pane, which is precisely the defect
    /// this rewrite answers.
    ///
    /// So: this window keeps the cursor's row and moves it to the top
    /// (`DualPlaneSession::clear_screen_keeping_cursor_row`), and the host is
    /// asked to do the same to its own buffer
    /// (`PtySession::clear_host_buffer`, `keepCursorRow`). The two then agree —
    /// measured: after the signal the host addresses the kept row as row 1 — so
    /// the prompt is on the screen the moment the menu closes and stays where
    /// both sides think it is.
    ///
    /// The rows above it **scroll out into history the ordinary way**, so
    /// everything cleared is still there to be scrolled back to and still there
    /// to be searched. That is exactly what the row below it is not.
    pub(in crate::runtime) fn clear_pane_screen(&mut self, seat: SeatId) -> Result<()> {
        let Some(leaf) = self.sessions.get_mut(&seat) else {
            return Ok(());
        };
        let alternate = leaf.session.terminal_modes().alternate_screen;
        // **The host is asked first, because its answer is what decides the shape.** Not for
        // tidiness: a window that moved rows and only then discovered it had nobody to tell would
        // have already made the screen the 2026-09-05 report describes.
        //
        // **Not on the alternate screen**: that buffer belongs to the program drawing it, which
        // repaints its own canvas with absolute addresses and needs no help keeping in step.
        let host = if alternate {
            bt_term::HostScreen::Cleared
        } else {
            match leaf.pty.as_ref() {
                // No child, so no second buffer to disagree with this one.
                None => bt_term::HostScreen::Cleared,
                Some(pty) => host_screen_after(pty.clear_host_buffer(true)),
            }
        };
        leaf.session
            .clear_screen_keeping_cursor_row(host)
            .context("clear one pane's screen locally")?;
        // The live selection goes with the screen it was drawn on (§7.1.6). Not
        // because the anchors would dangle — they name rows that are now in
        // history — but because a highlight left standing over cleared cells is
        // the window claiming a selection of what is no longer there.
        self.clear_pane_selection(seat);
        self.repaint_pane_change(seat)
    }

    /// `Clear scrollback` — **the whole of §3.1's ED3 pipeline**, on this pane.
    ///
    /// One escape and not a bespoke teardown, and that is the point: `ESC [ 3 J`
    /// already runs transcript, staging, blocks, indexes, caches, anchor
    /// degradation and tombstones through
    /// `LifecycleDirective::ClearHistoryAndStaging`, and the command marks go
    /// with it through the ledger (`retire_command_marks`) rather than through a
    /// second list that would have to be remembered. A menu row that emptied
    /// some of those by hand would be a second, shorter definition of what
    /// deleting history means — and the first thing to fall off it would be
    /// whichever structure the next slice adds.
    pub(in crate::runtime) fn clear_pane_scrollback(&mut self, seat: SeatId) -> Result<()> {
        let Some(leaf) = self.sessions.get_mut(&seat) else {
            return Ok(());
        };
        leaf.session
            .feed(b"\x1b[3J")
            .context("delete one pane's transcript locally")?;
        // A selection that reached into the history it was cut from is a
        // selection of lines that no longer exist.
        self.clear_pane_selection(seat);
        self.repaint_pane_change(seat)
    }

    /// Where the pane menu is, if one is up. [`Runtime::file_menu_layout`]'s
    /// twin, and anchored the same way: at the point the pointer was at, which
    /// no re-layout can move or destroy.
    pub(in crate::runtime) fn pane_menu_layout(&mut self) -> Option<profiles::PaneMenuLayout> {
        let menu = self.window.pane_menu.as_ref()?;
        let (point, submenu, zoomed) = (menu.point, menu.submenu, menu.zoomed);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let windows = self.other_window_rows();
        let shortcuts = &self.app.shortcuts;
        // **The machine**, on `windows`' own footing (user ruling 2026-09-06):
        // both decide how many rows a child has, not merely how wide it is.
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::pane_menu_layout(
            point,
            (width as f32, height as f32),
            scale,
            submenu,
            zoomed,
            &windows,
            shortcuts,
            programs,
            &mut measure,
        ))
    }

    /// Raise the menu on one pane head.
    ///
    /// **Terminal heads only.** Every verb here is about a shell — four of them
    /// put a second one somewhere, one moves this one, one ends it — and closing
    /// a files column or a preview is what that head's own `×` is for. A menu of
    /// verbs a surface cannot perform is worse than no menu: the same refusal
    /// [`Runtime::open_file_menu`] makes for a directory row, made here for the
    /// same reason and in the same one place rather than at each caller.
    pub(in crate::runtime) fn open_pane_menu(
        &mut self,
        seat: SeatId,
        point: [f32; 2],
    ) -> Result<()> {
        if !self.sessions.contains_key(&seat) {
            return Ok(());
        }
        // E61: the opener closes the others.
        self.close_popups_except(Popup::Pane);
        self.window.pane_menu = Some(PaneMenuState {
            point,
            seat,
            zoomed: self.seats.seat_is_zoomed(seat),
            hover: None,
            submenu: None,
            pointer_was: None,
            submenu_hold_until: None,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The `⌄`'s verb: open the menu under it, or put away the one it opened
    /// (user ruling, 2026-08-16).
    ///
    /// The toggle half is the `⌄` grammar's second door — a click opens what a
    /// rest would have opened, and a click on a button whose menu is already up
    /// closes it. Without it the hover rule would make the button unclickable in
    /// practice: by the time a hand has travelled to a chevron and pressed it,
    /// the rest has usually already opened the menu, and a press that opened it
    /// *again* would be a press that did nothing at all.
    ///
    /// The menu drops from the button's own bottom-left corner rather than from
    /// the pointer, which is where a menu belonging to a *control* hangs — the
    /// strip's `⌄` does the same, and a right click on the head still drops this
    /// same menu at the pointer, because that gesture belongs to the surface
    /// rather than to a button on it.
    pub(in crate::runtime) fn toggle_pane_menu(&mut self, seat: SeatId) -> Result<()> {
        if self
            .window
            .pane_menu
            .as_ref()
            .is_some_and(|menu| menu.seat == seat)
        {
            self.window.chevrons.clear();
            self.close_pane_menu()?;
            return Ok(());
        }
        let Some(anchor) = self.pane_chevron_box(seat) else {
            return Ok(());
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        self.open_pane_menu(
            seat,
            [
                anchor[0],
                anchor[3] + profiles::MENU_OFFSET_LOGICAL_PX * scale,
            ],
        )
    }

    /// The `⌄`'s box on one pane, or `None` when that pane has no room for one.
    ///
    /// **Two layouts, one button** (§7.1.6i): a pane with a head answers with
    /// the chevron in its run, and a lone terminal answers with the corner
    /// ghost. Everything downstream — the menu's anchor, the tooltip's anchor,
    /// the toggle — is written once and works in both.
    ///
    /// Re-derived from the frame the seat is standing in rather than remembered,
    /// which is the `&self`-hit-test discipline every other control in this
    /// window keeps: the rectangle a menu hangs off has to be the rectangle the
    /// button was drawn in, by one derivation and not by two that agree today.
    fn pane_chevron_box(&self, seat: SeatId) -> Option<[f32; 4]> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        seats::pane_chevron_box(
            &self.seats,
            &self.seat_layout,
            seat,
            scale,
            self.window.search.seat(),
        )
    }

    /// The pane menu's own level of the overlay stack, or nothing when none is
    /// up.
    pub(in crate::runtime) fn pane_menu_layer(&mut self) -> MenuPaint {
        let Some(layout) = self.pane_menu_layout() else {
            return MenuPaint::none();
        };
        let Some(menu) = self.window.pane_menu.as_ref() else {
            return MenuPaint::none();
        };
        let travel = layout.travel();
        let child_travel = layout.submenu_travel().unwrap_or(Travel::Right);
        let (hover, seat) = (menu.hover, menu.seat);
        // The profile the pane is *running*, which is what the submenu marks —
        // never the window's default. A pane you split from a Git Bash is a Git
        // Bash, and a submenu that ticked PowerShell on it would be telling you
        // about the window rather than about the pane the menu was raised on.
        // `position_of` and not `index_of_id`: the tick names a row of the table
        // being drawn, and a pane whose profile has been deleted has no row to
        // tick rather than the fallback's.
        let current = self
            .sessions
            .get(&seat)
            .and_then(|leaf| profiles::position_of(&leaf.profile));
        let windows = self.other_window_rows();
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        let mut layers =
            profiles::pane_menu_build(&layout, hover, current, programs, &windows, &mut measure);
        // `push_submenu`'s seam again: the menu first, the child after it.
        let child = layers.split_off(1);
        MenuPaint {
            menu: layers,
            travel,
            child: Some((child, child_travel)),
        }
    }

    pub(in crate::runtime) fn close_pane_menu(&mut self) -> Result<bool> {
        if self.window.pane_menu.take().is_none() {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Open or shut the `Split with` submenu, and report whether anything moved.
    ///
    /// The hold is cleared on both edges: an opening submenu has nothing to
    /// survive yet, and a closing one has nothing left to survive for.
    pub(in crate::runtime) fn set_pane_submenu(
        &mut self,
        open: Option<profiles::PaneMenuRow>,
    ) -> Result<bool> {
        let Some(menu) = self.window.pane_menu.as_mut() else {
            return Ok(false);
        };
        if menu.submenu == open {
            return Ok(false);
        }
        // **The heading is the row that was open, not a name written down here**
        // (B9). It said `SplitWith` while that was the only row with a list, and
        // a closing `Move to window` would have handed the keyboard to a row two
        // above the one it was standing on.
        let was = menu.submenu;
        menu.submenu = open;
        menu.submenu_hold_until = None;
        // The highlight follows the surface it is on. Opening lands on the first
        // row, which is what `→` and a click both mean; closing takes the
        // highlight back to the heading it came from, so `←` leaves the keyboard
        // somewhere rather than nowhere.
        menu.hover = match (open, was) {
            (Some(_), _) => Some(profiles::PaneMenuHover::Submenu(0)),
            (None, Some(heading)) => Some(profiles::PaneMenuHover::Row(heading)),
            (None, None) => menu.hover,
        };
        // **The ring goes out with the list** (B9): a highlighted window with no
        // menu naming it is a window wearing a mark nobody can explain. Armed
        // again by the first hover inside the list that has just opened.
        self.aim_at_window(None);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **The pane menu's hover, with the safety triangle in it** (#53).
    ///
    /// Answers whether the pointer is on either of the menu's two surfaces, so
    /// the caller can stop the pointer stream there exactly as it does for every
    /// other popup.
    ///
    /// Four things happen, in this order and for these reasons:
    ///
    /// 1. **What is under the pointer** is asked once, of the same layout the
    ///    menu was drawn from.
    /// 2. **The triangle is consulted only when it could change the answer** —
    ///    that is, only when a submenu is open and the pointer has moved onto
    ///    something that is neither the submenu nor its heading. Everywhere else
    ///    it has no opinion, and asking it would be a rule with no subject.
    /// 3. **A held highlight does not move**, which is the whole of what the
    ///    triangle buys: the row the hand is crossing does not light up, and the
    ///    submenu it was crossing toward stays open.
    /// 4. **The apex is re-seated** on every move the triangle does *not* hold,
    ///    so a hand that changes direction is measured from where it changed
    ///    rather than from where it started three rows ago.
    pub(in crate::runtime) fn drive_pane_menu_hover(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(layout) = self.pane_menu_layout() else {
            return Ok(false);
        };
        let hit = profiles::pane_menu_hit(&layout, position.x, position.y);
        let submenu = layout.submenu_frame();
        let to = [position.x as f32, position.y as f32];
        let now = Instant::now();
        let Some(menu) = self.window.pane_menu.as_mut() else {
            return Ok(false);
        };
        // **On the child at all**, which is a question about its frame and not
        // about its rows — see [`profiles::PaneMenuLayout::on_submenu`]. Reading
        // it off `hit` alone was the second cause of the 2026-08-19 report: the
        // hit says `Surface` both for the parent's padding and for the child's,
        // so a hand landing in the child's own leading border-and-padding read
        // as "not on the child", the triangle was asked about a pointer already
        // past its base, and the child shut under the hand that had just reached
        // it. The heading keeps its place beside it: a hand back on the row the
        // child hangs from has not left the child either.
        let heading = menu.submenu;
        let hovering_child = layout.on_submenu(to[0], to[1])
            || matches!(hit, Some(profiles::PaneMenuHit::Row(row)) if Some(row) == heading);
        let was_open = menu.submenu;
        let mut held = false;
        if let Some(submenu) = submenu
            && !hovering_child
        {
            // The hold is armed the first time the pointer leaves the child's
            // corridor and is spent by the clock, never by a row: a hand that
            // stops inside the triangle sends no more events, so the cap has to
            // be a deadline rather than a count of moves.
            let from = menu.pointer_was.unwrap_or(to);
            if profiles::safe_triangle_holds(from, to, submenu) {
                let until = *menu
                    .submenu_hold_until
                    .get_or_insert(now + profiles::SUBMENU_SAFE_HOLD);
                held = now < until;
            }
            if !held {
                menu.submenu = None;
                menu.submenu_hold_until = None;
            }
        } else {
            menu.submenu_hold_until = None;
        }
        // The apex only moves when the triangle is not holding. While it holds,
        // the aim is still the one the hand set when it left the heading — a
        // triangle re-drawn from each intermediate position narrows to nothing
        // and the whole rule evaporates halfway across.
        if !held {
            menu.pointer_was = Some(to);
        }
        let hovered = match hit {
            Some(profiles::PaneMenuHit::Zone(zone)) => Some(profiles::PaneMenuHover::Zone(zone)),
            Some(profiles::PaneMenuHit::Row(row)) => Some(profiles::PaneMenuHover::Row(row)),
            Some(profiles::PaneMenuHit::Submenu(index)) => {
                Some(profiles::PaneMenuHover::Submenu(index))
            }
            // The padding, and everywhere outside: nothing is lit. A menu whose
            // last-hovered row stayed lit while the pointer sat in its own margin
            // would be a menu Enter could fire from a place that looks idle.
            Some(profiles::PaneMenuHit::Surface) | None => None,
        };
        // The submenu going is a change even when the highlight does not move —
        // a hold that expires over the row it was already on takes a whole
        // window off the screen, and a repaint skipped because "the hover is the
        // same" would leave that window drawn over nothing.
        let mut changed = menu.submenu != was_open;
        if !held && menu.hover != hovered {
            menu.hover = hovered;
            changed = true;
        }
        // **The ring is the hover, seen from the other window** (B9). Read off
        // the highlight rather than off the hit, so the keyboard's walk lights
        // the same window the pointer's would.
        let ring = match (layout.submenu_kind(), menu.hover) {
            (
                Some(profiles::PaneMenuRow::MoveToWindow),
                Some(profiles::PaneMenuHover::Submenu(at)),
            ) => Some(at),
            _ => None,
        };
        // Resting on the heading opens the child, on the same 250ms the chevrons
        // themselves take (user ruling, 2026-08-16) — one number for "a hand has
        // settled on something that has more behind it". It is armed here and
        // matured in `advance_pane_menu`.
        let resting_on = match hit {
            Some(profiles::PaneMenuHit::Row(row)) if row.has_submenu() => Some(row),
            _ => None,
        };
        if resting_on.is_some() && menu.submenu != resting_on {
            menu.submenu_hold_until
                .get_or_insert(now + profiles::CHEVRON_HOVER_OPEN_DELAY);
        }
        let inside = hit.is_some();
        let aim = ring.and_then(|at| self.other_window_ids().get(at).copied());
        self.aim_at_window(aim);
        if changed && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(inside)
    }

    /// The pane menu's own clocks, matured.
    ///
    /// Two live in the one slot and they are never both meaningful: while the
    /// submenu is shut the deadline is the heading's 250ms rest, and while it is
    /// open it is the safety triangle's 300ms cap. One field because there is one
    /// question — "what does this menu owe at some instant" — and a menu cannot
    /// be both waiting to open its child and holding it open against the rows.
    pub(in crate::runtime) fn advance_pane_menu(&mut self, now: Instant) -> Result<()> {
        let Some(menu) = self.window.pane_menu.as_ref() else {
            return Ok(());
        };
        let Some(due) = menu.submenu_hold_until else {
            return Ok(());
        };
        if now < due {
            return Ok(());
        }
        if menu.submenu.is_some() {
            // The cap ran out with the hand still short of the child. The
            // submenu goes, and the highlight is owed to whatever the pointer is
            // actually over — which is asked of the geometry rather than
            // remembered, because the rows have not moved but the hold was
            // deliberately keeping the answer stale.
            self.set_pane_submenu(None)?;
            if let Some(position) = self.window.pointer_position {
                self.drive_pane_menu_hover(position)?;
            }
            return Ok(());
        }
        // The rest on the heading matured: the child opens, exactly as a rest on
        // a chevron opens its own menu. **Which child is a fact about where the
        // hand is resting** (B9), asked of the same hit test the hover reads.
        let Some(row) = self
            .pane_menu_row_under_pointer()
            .filter(|row| row.has_submenu())
        else {
            return Ok(());
        };
        self.set_pane_submenu(Some(row))?;
        Ok(())
    }

    /// Which row of the pane menu the pointer is on, if it is on one.
    ///
    /// Its own reader because two callers want it at moments when neither has a
    /// layout in hand — a matured rest and a press — and re-deriving it beside
    /// each of them is how one menu grows two hit tests.
    fn pane_menu_row_under_pointer(&mut self) -> Option<profiles::PaneMenuRow> {
        let position = self.window.pointer_position?;
        let layout = self.pane_menu_layout()?;
        match profiles::pane_menu_hit(&layout, position.x, position.y) {
            Some(profiles::PaneMenuHit::Row(row)) => Some(row),
            _ => None,
        }
    }

    /// The pane menu's next wake-up, for the loop's set.
    pub(in crate::runtime) fn pane_menu_deadline(&self) -> Option<Instant> {
        self.window.pane_menu.as_ref()?.submenu_hold_until
    }

    /// One docked chrome target, spelled the way a trigger is spelled.
    ///
    /// The two controls a float can also wear are named by surface and part, so
    /// that one button has one name whichever host drew it; everything else in
    /// the chrome exists on one host only and wears its own target. See
    /// [`PopoverTrigger`] for why a second spelling would be a second button.
    pub(in crate::runtime) fn docked_popover_trigger(
        &self,
        target: seats::ChromeTarget,
    ) -> PopoverTrigger {
        if let Some((seat, part)) = seats::preview_rail_control(target) {
            return PopoverTrigger::Rail(self.preview_here(seat), part);
        }
        if let seats::ChromeTarget::GitGraphTool { seat, tool } = target {
            return PopoverTrigger::GraphTool(self.preview_here(seat), tool);
        }
        PopoverTrigger::Chrome(target)
    }

    /// Do what one entry of the pane menu says, and put the menu away.
    ///
    /// The menu closes *first*, on [`Runtime::run_file_menu_row`]'s order and
    /// for a sharper version of its reason: most of these verbs rebuild the
    /// layout the menu is drawn over, one destroys the pane it was raised on,
    /// and one opens a modal dialog with a message loop of its own.
    ///
    /// The seat comes out of the menu, never from the pointer. Between the press
    /// that raised this and the one that ran it, the pointer has travelled —
    /// down the menu, which is drawn over some *other* pane as often as not.
    pub(in crate::runtime) fn run_pane_menu_row(
        &mut self,
        hit: profiles::PaneMenuHit,
    ) -> Result<()> {
        // **The child's row is resolved while the child still exists** (B9). A
        // `Submenu` hit counts rows on the glass, and what those rows are about
        // is a fact about the layout — which is built out of the menu state the
        // very next line takes away.
        let submenu = match hit {
            profiles::PaneMenuHit::Submenu(at) => {
                let layout = self.pane_menu_layout();
                let kind = layout
                    .as_ref()
                    .and_then(profiles::PaneMenuLayout::submenu_kind);
                let of = layout.as_ref().and_then(|layout| layout.submenu_row(at));
                match (kind, of) {
                    (Some(kind), Some(of)) => Some((kind, of)),
                    // A press on a child that is no longer there. The ordinary
                    // case rather than a fault: a menu can be taken down by
                    // anything between the press and this line.
                    _ => return Ok(()),
                }
            }
            _ => None,
        };
        // Resolved *before* the take, so `other_window_ids` reads the same
        // directory the list was drawn from.
        let bound_for = match submenu {
            Some((profiles::PaneMenuRow::MoveToWindow, of)) => {
                Some(self.other_window_ids().get(of).copied())
            }
            _ => None,
        };
        let Some(menu) = self.window.pane_menu.take() else {
            return Ok(());
        };
        let seat = menu.seat;
        self.window.chevrons.clear();
        // **The ring goes with the menu**, wherever the press lands: a window
        // still wearing the mark after the list that put it there has gone is a
        // window claiming to be a destination nobody is choosing.
        self.aim_at_window(None);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        if let Some(window) = bound_for {
            let Some(window) = window else {
                return Ok(());
            };
            return self.move_pane_to_window(seat, window);
        }
        // Every other child row is the profile list's, and `run_pane_verb` has
        // taken a `PROFILES` index there since the day the submenu existed.
        let hit = match submenu {
            Some((_, of)) => profiles::PaneMenuHit::Submenu(of),
            None => hit,
        };
        self.run_pane_verb(seat, hit)
    }

    /// **Move this pane into a window that is already open** (B9, user ruling
    /// 2026-08-25).
    ///
    /// [`Runtime::move_pane_to_new_window`] with the destination named instead
    /// of made, and it walks the same road F2's cross-window *drag* already
    /// walks — the pane is promoted to a tab where it stands and the tab is
    /// handed over by [`FolioApp::transfer_tab`]. That road is a
    /// [`DragHandover`] because only `FolioApp` can see two windows at once, and
    /// a menu row arrives at one `Runtime` that can see neither: so the row
    /// records the errand and `about_to_wait` spends it, in the same turn and
    /// before any frame.
    ///
    /// **It is the drag's errand and not a second one**, which is what keeps the
    /// promotion, the transfer, the refusal card and the emptied source tab from
    /// growing a menu-shaped copy of themselves.
    ///
    /// The landing is the end of the target's strip. A drag lands where the hand
    /// let go and this row has no hand over that window at all — so the honest
    /// answer is the one every append in this program gives, which is "after
    /// everything that is already there".
    fn move_pane_to_window(&mut self, seat: SeatId, window: WindowId) -> Result<()> {
        if window == self.window_id() {
            return Ok(());
        }
        let slot = self
            .app
            .windows_open
            .iter()
            .find(|open| open.id == window)
            .map_or(0, |open| open.tabs);
        let leaf = LeafId {
            tab: self.window.tabs[self.window.active_tab].id,
            seat,
        };
        self.app.pending_handover = Some(DragHandover {
            cargo: DragSource::Pane(leaf),
            from: self.window_id(),
            into: HandoverInto::Window {
                window,
                landing: DropLanding::StripExtract { slot },
            },
        });
        Ok(())
    }

    /// **One pane verb, wherever it was asked for** (§7.1.6e's rule: one verb,
    /// two doors, never two implementations).
    ///
    /// The pane menu's rows and §7.1.6i's right-click segment both land here, so
    /// `Duplicate pane` means the same split whichever door raised it and the day
    /// the split's seed changes there is one place to change it. The seat is the
    /// caller's, because the two doors carry it differently — one in its own
    /// menu state, one in the terminal menu's — and both took it from the press
    /// that raised them rather than from where the pointer has since travelled.
    pub(in crate::runtime) fn run_pane_verb(
        &mut self,
        seat: SeatId,
        hit: profiles::PaneMenuHit,
    ) -> Result<()> {
        // The pane may have gone while the menu stood open — its shell exited,
        // or another gesture took it. Every verb below needs it to still be
        // there, so it is asked once.
        if !self.sessions.contains_key(&seat) {
            return Ok(());
        }
        match hit {
            // The picker: the one place in this window where the direction is
            // the *gesture*, so the setting is not consulted. `Left` and `Up`
            // are the same two axes with the arriving leaf inserted first, which
            // is `Edit::SplitSeat`'s `leading` flag and has been since the tree
            // existed.
            profiles::PaneMenuHit::Zone(zone) => {
                self.split_seat(seat, zone.axis(), zone.leading(), SplitSeed::Inherit)
            }
            // The row's id, read off the table the submenu was drawn from: the
            // press is the last moment this position is certainly that profile,
            // and the split it seeds may be a frame or a folder chooser later.
            profiles::PaneMenuHit::Submenu(profile) => self.split_seat(
                seat,
                self.settings_split_axis(seat),
                false,
                SplitSeed::Profile(profiles::id(profile)),
            ),
            profiles::PaneMenuHit::Row(row) => match row {
                // The heading is not a verb: pressing it opens the submenu, and
                // that press never reaches here (see `press_pane_menu`). Listed
                // so the match is exhaustive over a closed set rather than over
                // a wildcard that would silently swallow a row added later.
                profiles::PaneMenuRow::Picker
                | profiles::PaneMenuRow::SplitWith
                | profiles::PaneMenuRow::MoveToWindow => Ok(()),
                // §7.1.6l — the row and the double-click on the head are one
                // verb behind two doors, which is what `run_pane_verb` is for.
                profiles::PaneMenuRow::ZoomPane => self.toggle_pane_zoom(seat),
                profiles::PaneMenuRow::NewInFolder => {
                    self.browse_for_split_root(seat);
                    Ok(())
                }
                // The split's own default: same profile, same directory. It is
                // exactly what a bare split already inherits, so this row is the
                // *name* of that behaviour rather than a second implementation
                // of it — which is why it hands over `Inherit` and not a seed it
                // assembled itself.
                profiles::PaneMenuRow::Duplicate => self.split_seat(
                    seat,
                    self.settings_split_axis(seat),
                    false,
                    SplitSeed::Inherit,
                ),
                profiles::PaneMenuRow::MoveToNewTab => self.move_pane_to_new_tab(seat),
                // The row above it and this one are one journey of two lengths,
                // and this one is *made of* that one — see
                // `Runtime::move_pane_to_new_window`, which promotes the pane
                // through the very function the row above spends and then hands
                // the tab to the application's transfer.
                profiles::PaneMenuRow::MoveToNewWindow => self.move_pane_to_new_window(seat),
                // The `×`'s own verb, reached through the `×`'s own door — so the
                // gate a destruction has to pass is passed once and not twice.
                profiles::PaneMenuRow::ClosePane => self.close_pane(seat),
            },
            // The menu's own padding. A press there is the menu swallowing it,
            // which `press_pane_menu` already decided; nothing to run.
            profiles::PaneMenuHit::Surface => Ok(()),
        }
    }

    /// **§7.1.6l — this pane alone on the stage, or back to the tiling.**
    ///
    /// One implementation behind both doors, which is §7.1.6e's rule and the
    /// reason the double-click and the menu row both land here rather than each
    /// doing their own version of it.
    ///
    /// The whole verb is a field and a re-solve. There is no restore path
    /// because nothing was taken apart: `Seats::solve` asks the solver for
    /// `LayoutMode::Focus` while the field is set, red line L13 leaves the tree
    /// alone, and clearing the field puts the tiling back because the tiling
    /// never went anywhere.
    ///
    /// `commit_seat_geometry` is the same one every layout-mutating path in this
    /// file converges on, so the panes that are no longer presented stop being
    /// asked for rectangles, the one that is gets ConPTY's resize, and the FLIP
    /// stays out of it — a zoom bumps no structure revision, and a posture
    /// changing is not a pane arriving.
    ///
    /// The window minimum is re-asked for the ordinary reason: it is the
    /// technical floor and the same for every tree (user ruling 2026-08-08), so
    /// this call cannot change it — and it is made anyway, beside its two
    /// siblings in the collapsed-bar arm and in `set_focus`'s, because a caller
    /// that skips it because it happens to know the answer is a caller that will
    /// be wrong the day the answer changes.
    pub(in crate::runtime) fn toggle_pane_zoom(&mut self, seat: SeatId) -> Result<()> {
        if !self.seats.toggle_zoom(seat) {
            return Ok(());
        }
        self.apply_window_min_inner_size()?;
        self.commit_seat_geometry()?;
        // The posture is not on disk (§7.1.6l) and nothing else about this tab
        // moved, so there is deliberately no `mark_session_dirty` here: a zoom
        // is not a change to the layout the session remembers.
        Ok(())
    }

    /// **`New terminal in folder…`** — ask the system for a folder and split
    /// into it.
    ///
    /// The chooser only gets *queued* here, for [`Runtime::browse_for_root`]'s
    /// reason at length: `IFileDialog::Show` runs a nested message loop that
    /// would re-enter this window's own event handling underneath the `&mut`
    /// borrow that started it. The answer arrives in
    /// [`Runtime::apply_folder_pick_result`].
    ///
    /// **The menu is already gone** — `run_pane_menu_row` took it before this
    /// runs — and that is the ruling's own requirement ("while the modal dialog
    /// is up the menu is closed"). A popup left standing behind a system modal
    /// is a popup nothing can dismiss: it takes no input while the dialog owns
    /// the loop, and the click that dismisses the dialog is spent on the dialog.
    ///
    /// The chooser opens where the pane is standing, by that pane's own OSC 7
    /// report — the same courtesy `browse_for_root` shows a column, and the same
    /// one I88 asks of every arriving shell.
    fn browse_for_split_root(&mut self, seat: SeatId) {
        let start = self
            .sessions
            .get(&seat)
            .and_then(|leaf| leaf.session.working_directory().map(Path::to_path_buf));
        match self.window.folder_picker.request(start.as_deref()) {
            Ok(true) => self.window.folder_pick = Some(FolderPick::SplitInto(seat)),
            // Already queued or already open: a second request while one is up
            // is one dialog, not two, and whoever asked first keeps the answer.
            Ok(false) => {}
            Err(error) => eprintln!("recoverable folder chooser failure: {error}"),
        }
    }

    /// **`Move pane to new window` — the pane leaves this window** (multiwindow
    /// slice F1c; `plan.md` F1c, `DESIGN.md` §2.10).
    ///
    /// **Two doors, one journey, and the journey is composed rather than
    /// copied.** The plan's own sentence is 「pane 拖出窗界 = 先按 `StripExtract`
    /// 升格成 tab 再成窗,一条路」, and this is that sentence in two lines: the
    /// pane becomes a tab through [`Self::extract_pane_into_new_tab`] — the same
    /// function the row above and the drag onto the strip both spend — and the
    /// tab then moves through [`FolioApp::transfer_tab`], which is the whole of
    /// F1b's transaction and the only thing in this program that moves a tab
    /// between windows. Nothing here re-implements either half, which is why a
    /// tear-out can never quietly become a respawn.
    ///
    /// **A lone pane skips the first half rather than failing it.** `close_seat`
    /// will not empty a tree, so the tear-out answers `None` for a pane with no
    /// siblings — and that pane is already the whole tab, so the tab that moves
    /// is the one it is standing in. This is the row §7.1.6i said would catch
    /// the debt `Move pane to new tab` leaves on a lone pane.
    ///
    /// **The window has to exist before the tab can move into it**, and only the
    /// loop's own door may create one, so the errand is written down here and
    /// spent in [`FolioApp::open_pending_window`] — in the same turn, before any
    /// frame is drawn. [`App::pending_new_windows`]'s standing shape and its
    /// standing reason.
    fn move_pane_to_new_window(&mut self, seat: SeatId) -> Result<()> {
        let standing = self.window.tabs[self.window.active_tab].id;
        let slot = self.window.tabs.len();
        let leaf = LeafId {
            tab: standing,
            seat,
        };
        let promoted = self.extract_pane_into_new_tab(leaf, slot)?;
        let errand = TearOut {
            tab: promoted.unwrap_or(standing),
            from: self.window_id(),
            promoted: promoted.is_some(),
            // A menu row names no place — see [`TearOut::at`].
            at: None,
        };
        let like = self.window_id();
        self.app
            .pending_new_windows
            .push(NewWindowPlan::receiving(like, errand));
        Ok(())
    }

    /// One float's chassis, laid out where that window stands this frame.
    ///
    /// The rise is [`risen_frame`]'s, shared with the hit test so the window is
    /// asked about exactly where it is drawn.
    pub(in crate::runtime) fn float_geometry_of(
        &mut self,
        id: float::FloatId,
    ) -> Option<(float::FloatGeometry, float::FloatFade)> {
        let now = Instant::now();
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let tools = self.float_head_tools(id);
        let (mode, frame, fade) = {
            let win = self.window.float.drawn().find(|win| win.epoch == id)?;
            (win.mode, win.frame, self.float_fade_of(win, now, scale))
        };
        let dock_label = self.float_dock_label_width(scale);
        Some((
            float::float_geometry(risen_frame(frame, fade), mode, scale, dock_label, tools),
            fade,
        ))
    }

    /// How wide the `DOCK` caption is — only the font knows, so it is measured
    /// beside the renderer exactly as the tip's and the badge's are.
    ///
    /// **Measured wearing what it is drawn wearing.** `float::build` sets this
    /// label semibold and tracked at `FLOAT_HEAD_TRACKING_EM`, per
    /// `.float-win .fly-head button { font-weight: 600; letter-spacing: .04em }`
    /// (mock-up 720-725). Measuring it as plain regular-weight text made the
    /// button's box a couple of pixels too narrow for its own caption, and the
    /// label's rect *is* that box less its padding — so the `K` was clipped
    /// against the bounds the shortfall had drawn.
    pub(in crate::runtime) fn float_dock_label_width(&mut self, scale: f32) -> f32 {
        let font = float::FLOAT_DOCK_FONT_LOGICAL_PX * scale;
        self.window.renderer.measure_chrome_label(
            &mut self.app.gpu,
            float_dock_label(),
            font,
            bt_render::ChromeLabelWeight::SemiBold,
            float::FLOAT_HEAD_TRACKING_EM,
            false,
        )
    }

    /// `DOCK` — the float becomes a column in the tab you are **looking at**.
    ///
    /// Not the tab it was born in (§7.1.2, user ruling 2026-07-17): a pinned
    /// window floats across tab switches precisely so it can serve you wherever
    /// you are, and sending it home on Dock would be the app deciding for you
    /// where you meant to put it. It also erases the "the tab it came from has
    /// since died" special case, which is the sort of thing a rule earns its
    /// keep by deleting.
    /// **Only the window whose button was pressed is collected**, which is rule
    /// ⑤ of the 2026-08-12 ruling at its plainest: DOCK is one window's control,
    /// and the others stay exactly where they are.
    pub(in crate::runtime) fn dock_float(&mut self, id: float::FloatId) -> Result<()> {
        let Some(win) = self.window.float.wipe(id) else {
            return Ok(());
        };
        self.forget_dead_float_gestures();
        // The window is gone, so whatever content plane it carried goes with it.
        self.sweep_preview_panes();
        // Same reason as dismissal: the window this shape belonged to is gone,
        // and Dock can be reached without the pointer moving afterwards.
        self.apply_pointer_cursor();
        // Docking a *tree* seats a files column. A buffer float's own DOCK lands
        // a preview pane instead, and that door is not open yet.
        let float::FloatTenant::Files(tenant) = win.tenant else {
            return Ok(());
        };
        let metrics = self.seat_metrics();
        let Some(seat) = self.seats.add_files_pane(&metrics, Some(tenant.width)) else {
            return Ok(());
        };
        let active = self.window.active_tab;
        self.window.tabs[active].files.insert(seat, tenant.files);
        self.window.tabs[active]
            .file_trees
            .insert(seat, tenant.cache);
        // **And the repository, the same way and for the same reason** (user
        // ruling, 2026-08-19). `DOCK` is the reverse of the pop-out, so a window
        // that was on its Git page becomes a column on its Git page — with what
        // it had already read, so docking spends no subprocess and the page does
        // not blink back through "Reading the repository…" on the way in.
        self.window.tabs[active].git_trees.insert(seat, tenant.git);
        self.window.tabs[active]
            .git_scroll
            .insert(seat, tenant.git_scroll);
        // `DOCK` is a button and this is the click on it.
        self.set_files_keyboard(Some(seat), FilesFocusArrival::Pointer);
        // **The hole this used to be** (user report, 2026-08-13). A column
        // arriving by this door narrows every pane beside it exactly as one
        // arriving by `Ctrl+Shift+B` does — but this path re-solved the layout
        // and stopped there, and `resolve_seat_layout` moves rectangles without
        // telling a single shell about them. The panes were redrawn narrower
        // while their ConPTYs went on writing at the old width, which is a
        // prompt that reprints itself over its own middle. The keyboard twin
        // never had the bug because it always ran the full ceremony; this one
        // now runs the same one.
        self.settle_seat_set_change()?;
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **The page a docking float was carrying, moved onto the pane it landed
    /// on** (§7.14a) — [`Self::dock_preview_float`]'s one caller.
    ///
    /// A separate function because it is a *transaction on the window's page
    /// map*, and the dock around it is a transaction on the tab's panes: the two
    /// have different failure modes and the failure of this one must not undo
    /// that one. A page that could not be moved is reported and left where it
    /// is; the next frame finds it under a float that is gone, gives it no
    /// rectangle, and [`Self::advance_web_page`] retires it. A pane with a
    /// document under it and no browser is a pane; a pane with a browser
    /// nothing can address is a leak.
    pub(in crate::runtime) fn dock_the_page_of(
        &mut self,
        id: float::FloatId,
        landing: PreviewSurface,
    ) {
        let Some(carried) = self
            .window
            .float
            .live(id)
            .and_then(float::FloatWin::preview)
            .and_then(|preview| preview.page)
        else {
            return;
        };
        let PreviewSurface::Seat(landed) = landing else {
            return;
        };
        let address = webhost::SeatAddress {
            page: bt_platform::PageVisual {
                tab: landed.tab.0,
                seat: landed.seat.0,
            },
            window: match native_window(&self.window.window) {
                Ok(native) => native,
                Err(error) => {
                    eprintln!("BT_WEB {error}");
                    return;
                }
            },
        };
        // **A seat that already holds a page keeps it.** `preview_landing_surface`
        // picks a preview seat, and a preview seat can itself be a page; moving
        // this one on top would drop a live browser without going through the
        // door that waits for its process to exit and gives its profile folder
        // back. So the carried page stays where it is and the next frame retires
        // it the ordinary way — it has neither a float nor a seat any more —
        // which is one page lost and not two.
        if self.window.web.contains_key(&landed) {
            eprintln!(
                "BT_WEB the docked page had nowhere to land: {landed:?} is already showing one"
            );
            return;
        }
        let mut outcomes = Vec::new();
        let report = {
            let window = &mut *self.window;
            let Some(web) = window.web.remove(&carried) else {
                return;
            };
            // Re-keyed under the leaf it is landing on before anything is asked
            // of it, so that every later reader — the placement, the retirement,
            // the keyboard — finds it under one name. `rehost` is given the same
            // compositor twice on purpose: this is a move inside one window, and
            // the visual it is being pointed at is the landing pane's.
            let entry = window.web.entry(landed).or_insert(web);
            entry.rehost(
                &window.compositor,
                &window.compositor,
                address,
                true,
                &mut outcomes,
            )
        };
        if let Some(error) = report.error() {
            eprintln!("BT_WEB the docked page could not follow its pane: {error}");
        }
        if let Err(error) = self.apply_web_outcomes(landed, outcomes) {
            eprintln!("BT_WEB {error}");
        }
    }

    /// **A pane's picture moved, so the card that is a picture of it owes a
    /// frame** (§7.1.6b′ T-5, review row R5-7).
    ///
    /// One function because there are two roads and they must not disagree. The
    /// ordinary one is the drain: bytes reached a screen, which is the same
    /// condition `DualPlaneSession::feed_at` bumps `screen_revision` on — the very
    /// number [`focus_thumb::FocusThumbnails`]'s damage gate keys a terminal seat
    /// to. The other passes through no ring at all: a DEC 2026 block whose
    /// timeout expires is released turns after its bytes arrived, moving cells
    /// with nothing coming through the drain, and the card column was never told.
    ///
    /// **The damage key is checked, not the projection**, and it is checked
    /// against the cards that are actually on screen: a collapsed column, a
    /// window not in the mode, or a tab scrolled out of the rail leaves this at
    /// one rectangle comparison per speaking tab and no frame asked for.
    pub(crate) fn panes_spoke(&mut self, spoke: &[usize], now: Instant) {
        if spoke.is_empty() {
            return;
        }
        if let Some(geometry) = self.focus_rail_geometry_now(now)
            && spoke.iter().any(|index| geometry.card_is_in_view(*index))
        {
            self.window.cards.pane_spoke();
        }
    }

    pub(in crate::runtime) fn drawn_rail(&self, now: Instant) -> (i32, u8) {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let state = self.sampled_rail(now);
        (
            (state.width_logical_px() * scale).round() as i32,
            (state.text_opacity * 255.0).round() as u8,
        )
    }

    /// The dock box's opacity as the overlay would draw it, quantised to the
    /// 1/255 a layer's alpha resolves to — `None` when there is no box.
    pub(in crate::runtime) fn drawn_dock_reveal(&self, now: Instant, motion: Motion) -> Option<u8> {
        self.window.drop_preview.as_ref().map(|shown| {
            let (reveal, _) = shown.reveal.sample(now, motion);
            (reveal.clamp(0.0, 1.0) * 255.0).round() as u8
        })
    }

    /// **Which of this tab's preview seats wear a play button** (user ruling
    /// 2026-08-27; §7.23 ⑩).
    ///
    /// Three facts, settled here so that the painter and the hit test are handed
    /// one answer rather than each deriving a policy
    /// ([`Self::play_button_rasters`] draws by it and
    /// [`seats::hit_preview_play`] presses by it):
    ///
    /// 1. **The pane is showing a video whose spelling can be played** —
    ///    [`preview::path_names_a_video`], which is the playable column
    ///    of the one table. A `.mov` has a face and no player, and what it wears
    ///    instead is a sentence in its fact line, not a button that would refuse.
    /// 2. **Nothing is already on that seat.** A pane that is playing has an
    ///    engine on it, and a play button over a playing video would be a
    ///    control offering what is already happening.
    /// 3. **It is a seat and not a float**, because the verb needs a leaf to put
    ///    an engine on and `pop_out_preview` closes the leaf it tore off.
    ///
    /// **The decoded frame is deliberately not one of them.** A `.webm` carrying
    /// AV1 plays in that engine on a machine whose Media Foundation cannot draw
    /// its first frame (§7.23 ②), and a button gated on the picture would be
    /// missing on exactly the files that most need it — the ones whose pane has
    /// nothing to show until it plays.
    pub(crate) fn seats_wearing_a_play_button(&self) -> std::collections::BTreeSet<SeatId> {
        let tab = match self.window.tabs.iter().find(|tab| tab.id == self.id) {
            Some(tab) => tab,
            None => return std::collections::BTreeSet::new(),
        };
        tab.seats
            .preview_seats()
            .into_iter()
            .filter(|seat| {
                let leaf = LeafId {
                    tab: self.id,
                    seat: *seat,
                };
                // **A seat that is playing wears a pause on its bar, not a play
                // on its picture** (route B slice ②; §7.44 ①).
                //
                // The `is_closing` clause this replaces was route A's whole
                // problem in one line: a browser left the glass when `close` was
                // called and its map entry survived for as long as the process
                // took to end, so a stopped video had no button for most of a
                // second and sometimes for ten. A seat is removed from the map
                // the instant it is stopped — the shutdown is synchronous — so
                // the button comes back in the same frame the picture does, and
                // there is no second state to ask about.
                if self.window.video.get(PreviewSurface::Seat(leaf)).is_some() {
                    return false;
                }
                tab.preview_panes
                    .get(PreviewSurface::Seat(leaf))
                    .and_then(|pane| pane.image.as_ref())
                    .is_some_and(|image| preview::path_names_a_video(&image.path))
            })
            .collect()
    }

    /// Carry the current solve to every shell of the active tab — one rectangle
    /// per leaf, read from one expression.
    ///
    /// **The geometry is the same sentence for all of them**: a leaf's grid is
    /// [`seats::pane_body_viewport`] of *that leaf's* seat, and of nothing else.
    /// **So is the timing.** Every leaf goes through
    /// [`schedule_leaf_grid_change`]: its actor reflows in this turn, and its
    /// child hears the last word of the gesture at the shared 200 ms quiet
    /// boundary. The siblings used to be told at once instead — "they have no
    /// drag to coalesce" was never true of them, only unnoticed, because the
    /// drag being coalesced is the *window's* and it moves every pane's
    /// rectangle at once. A four-pane split therefore made three uncoalesced
    /// `ResizePseudoConsole` round trips on the window thread per OS `Resized`,
    /// each one re-entering conhost and invalidating a PSReadLine anchor.
    ///
    /// The focused leaf used to be sized from [`Self::resolve_seat_layout`]'s
    /// return instead, which is the body of `seats.identity()` — the tab's
    /// *primary* seat. That is a stored field naming one fixed leaf; it moves
    /// only when the leaf it names is closed, and never with focus. In a
    /// lone-terminal tab it is the focused leaf and the mistake was invisible,
    /// which is why it survived U12-B. Split the tab and focus the other pane
    /// and the focused shell was told the primary pane's column count: a narrow
    /// pane stopped wrapping because its shell believed it was wide, and
    /// PSReadLine — whose anchor arithmetic trusts the reported buffer width —
    /// raised "the value must be greater than or equal to zero and less than the
    /// console's buffer size". Taking focus away appeared to cure it because the
    /// unfocused carry below was already reading the right rectangle.
    ///
    /// Without the unfocused half a split pane keeps the width it was born with:
    /// the window resizes, the divider moves, and one pane's shell goes on
    /// believing it has the columns it had at spawn.
    /// Answers the grid the focused leaf's own rectangle solved to — what the
    /// perf trace means by "the size this resize frame was about" — or `None`
    /// when the solver placed no rectangle for it.
    pub(in crate::runtime) fn resize_leaves_to_layout(
        &mut self,
        observed_at: Instant,
        context: &'static str,
    ) -> Result<Option<GridSize>> {
        // **No pane is handed a grid cut from the rectangle the window is
        // leaving** (T-CARD-ANCHOR-DPI; see [`DpiRectangle`], which holds the
        // argument). The gate is here rather than at the two DPI call sites
        // because the sentence is about the rectangle and not about who is
        // asking: a tab activated, a divider let go or a seat layout settled
        // inside the same interval would cut the same phantom grid out of the
        // same pixels.
        //
        // Nothing is queued by the refusal. The rectangle this window ends the
        // DPI change with is re-solved either by the `Resized` that follows or
        // by [`Self::settle_dpi_rectangle`] on the next turn, and both of those
        // re-derive every leaf's grid from the tree as it stands.
        if !self.window.dpi_rectangle.may_cut_a_grid() {
            return Ok(None);
        }
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // **The shells are about to be told what the tree looks like**, which
        // makes this the one honest place to record that they know. See
        // [`WindowRuntime::shells_settled_revision`]: every other reading of "the seat
        // set changed" is a reading of intent, and intent is exactly what the
        // two verbs that forgot the ceremony had.
        self.window.shells_settled_revision = self.seats.structure_revision();
        let plan = leaf_resize_plan(&self.seats, &self.seat_layout, self.focused_leaf, scale);
        let active = self.window.active_tab;
        // Read before the walk takes a `&mut` of this tab's sessions
        // (`BT_CARD_TRACE`).
        let window = u64::from(self.window.window.id());
        let tab_id = self.window.tabs[active].id;
        // The panes without the keyboard, before the focused one, so the pane a
        // gesture is aimed at is the last one this turn touches.
        for target in plan.iter().copied().filter(|target| !target.focused) {
            let (seat, body) = (target.seat, target.body);
            let metrics = self.window.renderer.metrics();
            let physical = PhysicalSize::new(body.width, body.height);
            let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
                continue;
            };
            let next_grid = leaf.grid_for(&metrics, body);
            schedule_leaf_grid_change(
                leaf,
                next_grid,
                physical,
                observed_at,
                LeafOnStage::Shown,
                context,
                card_trace::Pane {
                    window,
                    tab: tab_id,
                    seat,
                },
            )?;
        }
        // And the tabs nobody is looking at, before the focused pane for the
        // reason its siblings go before it.
        self.resize_hidden_leaves_to_layout(observed_at, context)?;
        // A seat the solver could not place has no rectangle, so there is no
        // size to carry and the leaf keeps the one it has until a later solve
        // places it. Substituting some other rectangle here — the window's, the
        // primary seat's — is precisely the defect this method was rewritten to
        // end.
        let Some(target) = plan.iter().copied().find(|target| target.focused) else {
            return Ok(None);
        };
        let body = target.body;
        // A tab with no shell has no mark and so no rail; its grid is only ever
        // traced, and [`Self::schedule_grid_change`] carries it nowhere (§7.1.6h).
        let has_rail = self.window.tabs[active]
            .focused()
            .is_some_and(|leaf| leaf.has_rail);
        let next_grid = cmdrail::terminal_grid_for(&self.window.renderer.metrics(), body, has_rail);
        self.schedule_grid_change(
            next_grid,
            PhysicalSize::new(body.width, body.height),
            observed_at,
            context,
        )?;
        Ok(Some(next_grid))
    }

    /// **Every tab's panes, because a window's rectangle is a fact about the
    /// window** (user report 2026-09-09).
    ///
    /// [`Self::resize_leaves_to_layout`] is the tab on the stage; this is every
    /// other one, and it is here for the reason [`Self::apply_scale_factor`] and
    /// [`Self::sync_math_layout_key`] already walk every leaf of every tab. A
    /// DPI, a face and the rectangle this window has are facts about the
    /// *window*; not one of them is about the pane holding the keyboard, and a
    /// tab that answered them only when somebody looked at it would be a tab
    /// whose shells believe whatever was true the last time it was on stage.
    ///
    /// **What that cost.** A session saved `maximized: true` records the window's
    /// *normal* rectangle beside the flag, so the window opens at that rectangle,
    /// every restored tab is built against it, and `put_the_window_on_the_glass`
    /// asks Windows to maximize only after every shell is already running. The
    /// tab on the stage was put right by the `Resized` that followed; the tabs
    /// behind it were not. On the report's machine 960 logical pixels less the
    /// focus column is too narrow for three panes, so one of them was born a bar
    /// — and stayed one, through the whole of its shell's first prompt, until the
    /// tab was clicked.
    ///
    /// Each tab is solved into [`WindowRuntime::seat_viewport`], the very
    /// rectangle the stage was solved into and stored rather than recomputed for
    /// A12's reason, so "every tab is solved into the same box"
    /// ([`TabState::seat_layout`]) is true of the box as well as of the sentence.
    /// The solve is pure and `O(seats)` and each leaf's notification is coalesced
    /// on its own queue, so a window with a dozen tabs pays a dozen tree walks
    /// per gesture and one `ResizePseudoConsole` per pane per gesture.
    ///
    /// **And one reflow per pane per gesture, which is the half this arrived
    /// without** (user report 2026-09-09). A tree walk is pure and cheap; the
    /// `resize_at` at the end of [`schedule_leaf_grid_change`] is a full vendor
    /// reflow of that pane's grid on the window thread, and taking it per hidden
    /// pane per OS event put the frame the visible pane owes the glass behind
    /// eleven of them on a twelve-tab window. So every leaf here is `Behind`:
    /// the notification and the reflow are both released at the quiet boundary,
    /// which is the same sentence the child was already being told and the same
    /// one [`commit_leaf_resize`] was already written to say. Nothing about
    /// *which* size a hidden tab ends at changes — only when its own actor hears
    /// it, and no frame is drawn from a hidden tab in between.
    fn resize_hidden_leaves_to_layout(
        &mut self,
        observed_at: Instant,
        context: &'static str,
    ) -> Result<()> {
        // Read before any tab is borrowed mutably (`BT_CARD_TRACE`).
        let window = u64::from(self.window.window.id());
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let metrics = seats::seat_metrics(self.window.renderer.metrics().dpi_milli().get());
        let viewport = self.window.seat_viewport;
        let policy = self.window.size_policy;
        let active = self.window.active_tab;
        for index in 0..self.window.tabs.len() {
            if index == active {
                continue;
            }
            let tab = &self.window.tabs[index];
            let (layout, overflow) = solve_tree(&tab.seats, viewport, &metrics, policy);
            // Read while the renderer is still only borrowed, because the tab it
            // is about is borrowed mutably the moment its layout lands on it.
            let cell_metrics = self.window.renderer.metrics();
            let sized: Vec<(SeatId, GridSize, PhysicalSize<u32>)> =
                leaf_resize_plan(&tab.seats, &layout, tab.focused_leaf, scale)
                    .into_iter()
                    .filter_map(|target| {
                        let leaf = tab.sessions.get(&target.seat)?;
                        Some((
                            target.seat,
                            leaf.grid_for(&cell_metrics, target.body),
                            PhysicalSize::new(target.body.width, target.body.height),
                        ))
                    })
                    .collect();
            let tab = &mut self.window.tabs[index];
            tab.seat_layout = layout;
            tab.seat_overflow = overflow;
            let tab_id = tab.id;
            for (seat, next_grid, physical) in sized {
                let Some(leaf) = tab.sessions.get_mut(&seat) else {
                    continue;
                };
                schedule_leaf_grid_change(
                    leaf,
                    next_grid,
                    physical,
                    observed_at,
                    LeafOnStage::Behind,
                    context,
                    card_trace::Pane {
                        window,
                        tab: tab_id,
                        seat,
                    },
                )?;
            }
        }
        Ok(())
    }

    /// The pane the pointer is standing in, and the cell of it under the pointer.
    ///
    /// One lookup for both halves, for the reason [`Self::forwarded_mouse_hit`]
    /// records below: a hit and the pane it belongs to that were fetched
    /// separately can disagree about which pane they mean.
    pub(in crate::runtime) fn pane_frame_hit(&self) -> Option<(SeatId, bt_render::GridHit)> {
        let (seat, position, frame) = self.pane_hit_context()?;
        let hit = self
            .window
            .renderer
            .metrics()
            .hit_test_frame(frame, position.x, position.y)?;
        Some((seat, hit))
    }

    /// The cell under the pointer as the *mouse protocol* names it: the same hit
    /// [`Self::frame_hit`] answers, mapped through the live viewport of the frame it was read
    /// from.
    ///
    /// One lookup, deliberately. The translation used to be a second one — the hit taken from the
    /// pane under the pointer, the frame taken from `WindowRuntime::last_presented_frame` — and two
    /// lookups can disagree about which pane they mean, or about whether there is a frame at all.
    /// Both happen on one gesture: `focus_pane_at` empties that window-level slot the instant
    /// keyboard focus moves, and the press that moved it is still on its way down the router, so
    /// the very next line went looking for a frame that had just been taken away. Reading the
    /// frame beside the hit removes the disagreement by construction rather than by testing for
    /// it — there is no second slot left to be empty.
    /// The frame one pane last drew — the cells a gesture in that pane is made of,
    /// and the anchors a selection set on that pane's session must be read from.
    ///
    /// Asked by seat rather than of the focused leaf. The two are the same pane
    /// for the length of a press, because D40 moves the focus into the pane on the
    /// way down; but "the same by construction" and "the same because of the order
    /// two other things happen in" are different guarantees, and only the first one
    /// survives someone reordering the router.
    ///
    /// The leaf's own copy, never `WindowRuntime::last_presented_frame`. That window-level mirror is
    /// presentation bookkeeping (the projection hold, the unchanged-frame skip) and `focus_pane_at`
    /// empties it the instant keyboard focus moves — while the press that moved it is still on its
    /// way down the router to the very lines that want a frame. `None` means this pane has not
    /// drawn yet, which is the same "there are no cells here" [`Self::pane_hit_context`] already
    /// answers, and the callers already do nothing about.
    pub(crate) fn pane_frame(&self, seat: SeatId) -> Option<&ViewportFrame> {
        self.sessions.get(&seat)?.last_presented_frame.as_ref()
    }

    /// **Which spelling of an absolute path the shell in `seat` prints** — the pane's namespace,
    /// read from the session that was told it at spawn.
    ///
    /// It is what [`bt_transcript::paths::PathNamer::Pane`] carries, and therefore what decides
    /// whether a `\\wsl.localhost\<distro>\…` target printed into *this* pane names a place this
    /// window may read (route D of the untrusted-path audit, 2026-09-08). A seat with no session
    /// behind it — a folder tab, a file tab — prints nothing at all, and `Windows` is the honest
    /// answer for a pane that has no shell to speak another namespace.
    pub(crate) fn seat_path_namespace(
        &self,
        seat: SeatId,
    ) -> bt_transcript::paths::PrintedPathNamespace {
        self.sessions
            .get(&seat)
            .map(|leaf| leaf.session.printed_path_namespace())
            .unwrap_or_default()
    }

    /// [`Self::seat_path_namespace`] for the pane the pointer is standing in.
    ///
    /// The hover's own pane and not the focused one, because a hyperlink hover belongs to whatever
    /// pane the pointer is over ([`Self::hover_pane`]'s whole reason for existing), and the link
    /// under it was printed by *that* pane's shell.
    pub(in crate::runtime) fn hovered_pane_path_namespace(
        &self,
    ) -> bt_transcript::paths::PrintedPathNamespace {
        self.window
            .hover_pane
            .map(|seat| self.seat_path_namespace(seat))
            .unwrap_or_default()
    }

    /// **What this window is allowed to know about a path `seat`'s shell printed** — the ledger
    /// read that stands where a filesystem call used to (audit 3 C-2).
    ///
    /// The whole of the rule is in the body: a `get` on a `BTreeMap` the *worker* filled. The
    /// window thread has no other way to ask, and a pane with no session has no ledger, which is
    /// the honest `None` — a folder tab prints nothing and answers for nothing.
    pub(crate) fn seat_path_verdict(
        &self,
        seat: SeatId,
        path: &Path,
    ) -> Option<bt_term::PathVerdict> {
        self.sessions.get(&seat)?.session.path_verdict(path)
    }

    /// [`Self::seat_path_verdict`] for the pane the pointer is standing in, for
    /// [`Self::hovered_pane_path_namespace`]'s reason: the link under the pointer was printed by
    /// *that* pane's shell, so it is that pane's ledger that answers for it.
    pub(in crate::runtime) fn hovered_pane_path_verdict(
        &self,
        path: &Path,
    ) -> Option<bt_term::PathVerdict> {
        self.seat_path_verdict(self.window.hover_pane?, path)
    }

    /// The cell a selection drag that began in `seat` is over, with the pointer
    /// clamped into that pane's own body.
    ///
    /// Once a drag has begun the pointer is free to wander — into the pane next
    /// door, onto the chrome, past the window's edge — and every one of those
    /// points still has to name a cell of the *origin* pane, because that is the
    /// grid whose anchors this selection is made of. Reading the cell from
    /// whatever pane the pointer happens to be over would apply a neighbour's row
    /// and column to this pane's frame: a selection nobody asked for, at a place
    /// nobody pointed at, in a pane the gesture never belonged to.
    ///
    /// Clamping rather than refusing is also the terminal convention. Dragging
    /// below a pane selects to the end of what is there; it does not stop
    /// selecting at the edge and it does not reach into the pane below.
    pub(in crate::runtime) fn drag_hit_in_pane(&self, seat: SeatId) -> Option<bt_render::GridHit> {
        let position = self.window.pointer_position?;
        let frame = self.pane_frame(seat)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
        let (x, y) = clamp_into_body(body, position.x, position.y);
        self.window
            .renderer
            .metrics()
            .clamped_hit_test_frame(frame, x, y)
    }

    /// Set one pane's selection: the pane the gesture belongs to, named outright.
    ///
    /// Never through the focused-leaf deref, which answers "whichever pane holds
    /// the keyboard *now*" — the right pane during a drag only because nothing
    /// moves focus while a button is held.
    /// Hand a selection to one pane of the tab on screen.
    ///
    /// The window's half of [`TabState::set_leaf_selection`], which is where the
    /// "one selection to a tab" rule lives. Every route that gives a pane a
    /// selection — the drag, the double click, the triple click — comes through
    /// here, so it is the one door that rule has to be written on.
    pub(in crate::runtime) fn set_pane_view_selection(
        &mut self,
        seat: SeatId,
        selection: Option<ViewSelection>,
    ) {
        let active = self.window.active_tab;
        self.window.tabs[active].set_leaf_selection(seat, selection);
    }

    /// The pane the pointer is standing in, that pane's own shell, and the cell under the pointer.
    ///
    /// The subject of every hover verb below, and deliberately never the focused leaf. A pointer
    /// answers about what it is pointing at; a hover that first required a click would be the
    /// window refusing to say what it can already see (user ruling 2026-08-10). Nothing reached
    /// from here writes focus either — reading is not taking.
    pub(in crate::runtime) fn hovered_leaf(
        &self,
    ) -> Option<(SeatId, &LeafSession, bt_render::GridHit)> {
        let (seat, position, frame) = self.pane_hit_context()?;
        let hit = self
            .window
            .renderer
            .metrics()
            .hit_test_frame(frame, position.x, position.y)?;
        Some((seat, self.sessions.get(&seat)?, hit))
    }

    /// Notice the pointer crossing from one pane into another, and pay what the crossing owes.
    ///
    /// Returns the pane the pointer is now in. The pane it entered owes a scan of the frame it
    /// already drew — nothing has published there, so nothing has scanned it, and a hover that
    /// waited for that pane's next PTY byte to answer would answer late or never on a quiet shell.
    /// Both panes owe a repaint, which is how the marks move across with the pointer.
    pub(in crate::runtime) fn observe_hovered_pane(&mut self) -> Result<Option<SeatId>> {
        let hovered = self.pane_hit_context().map(|(seat, _, _)| seat);
        if self.window.hover_pane == hovered {
            return Ok(hovered);
        }
        self.window.hover_pane = hovered;
        if let Some(seat) = hovered {
            self.rescan_pane_references(seat);
        }
        self.repaint_hovered_pane()?;
        Ok(hovered)
    }

    /// Re-derive one pane's image references from the frame that pane last drew.
    ///
    /// Idempotent and cheap to repeat: it reads a frame that is already in hand and asks that
    /// pane's own shell what it draws, which is the same question `publish_frame_inner` asks for
    /// the focused pane on every frame.
    pub(in crate::runtime) fn rescan_pane_references(&mut self, seat: SeatId) {
        let active = self.window.active_tab;
        let Some(leaf) = self.window.tabs[active].sessions.get_mut(&seat) else {
            return;
        };
        let Some(frame) = leaf.last_presented_frame.as_ref() else {
            leaf.frame_image_references = FrameImageReferences::default();
            return;
        };
        leaf.frame_image_references = FrameImageReferences {
            columns: frame.columns.get(),
            references: leaf.session.frame_image_references(frame),
        };
    }

    /// Put the pane under the pointer — and the pane it just left — back on the glass.
    ///
    /// Two doors, because a window with a fleet in it has two ways to redraw a pane. The focused
    /// leaf reaches the glass by publishing a frame, which is the path every existing contract
    /// (presentation hold, scroll anchoring, the unchanged-frame skip) is written against. Every
    /// other pane is re-projected by `redraw` itself, so asking for one is the whole of what it
    /// takes to move its marks. Asking for both is correct and costs one frame: a pointer leaving
    /// the focused pane for a neighbour owes marks to the neighbour and owes their removal to the
    /// pane it left.
    pub(in crate::runtime) fn repaint_hovered_pane(&mut self) -> Result<()> {
        hang_watch::during(hang_watch::Station::WindowRedraw, || {
            self.window.window.request_redraw()
        });
        self.publish_interaction_frame()
    }

    /// Get a change made *to one named pane* onto the glass — including when
    /// that pane is not the one holding the keyboard.
    ///
    /// [`Self::publish_interaction_frame`] builds the **focused** leaf's frame,
    /// and it is allowed to decline: a presentation hold standing on that leaf
    /// returns before it ever reaches `request_redraw`. For the focused pane's
    /// own content that is exactly right — the hold exists to keep the last
    /// complete frame on screen instead of flashing through a resize reprint.
    ///
    /// For **every other pane it is wrong**, and silently so. An unfocused pane
    /// is composed nowhere but inside `redraw`, and `redraw` returns
    /// immediately unless a frame reached the slot, so a hold owned by the
    /// focused leaf freezes panes whose own shells are holding nothing. That is
    /// the shape of the wheel bug: `scroll_by_subpixels` clears the hold on the
    /// leaf it scrolls, so a notch over the focused pane always healed its own
    /// path to the screen while a notch over any other pane moved that pane's
    /// projection and then had no way to draw it.
    ///
    /// The repair is the one this window already uses for a peek overlay
    /// (`present_peek_overlay`): when the focused leaf declines, re-present the
    /// frame **already on screen**. Its pixels are unchanged and stay unchanged
    /// — the hold is honoured to the letter — but putting it back in the slot
    /// runs the compositor, and the compositor rebuilds every other pane from
    /// that pane's own projection. Nothing is invented for the held pane; the
    /// panes that have something new to show simply stop being hostage to it.
    pub(in crate::runtime) fn repaint_pane_change(&mut self, seat: SeatId) -> Result<()> {
        self.repaint_pane_change_inner(seat, None)
    }

    /// A wheel can leave the view at its clamp; other pane changes still owe
    /// their unconditional frame. The focused frame's digest cannot tell us
    /// whether an unfocused view moved, so carry that answer from the scroll.
    pub(in crate::runtime) fn repaint_pane_change_inner(
        &mut self,
        seat: SeatId,
        wheel_view_moved: Option<bool>,
    ) -> Result<()> {
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        };
        if self.publish_frame_inner(trigger, wheel_view_moved.is_some())?
            || seat == self.focused_leaf
            || wheel_view_moved == Some(false)
        {
            return Ok(());
        }
        self.represent_on_screen_frame(trigger)
    }

    /// Drop one pane's selection — the pane named, and not whichever holds the
    /// keyboard. The two differ for a gesture, which belongs to the pane it began
    /// in for as long as the button is down.
    pub(in crate::runtime) fn clear_pane_selection(&mut self, seat: SeatId) {
        let active = self.window.active_tab;
        self.window.tabs[active].clear_leaf_selection(seat);
    }

    /// Advance a divider drag. Returns whether the pointer was consumed.
    ///
    /// Every frame re-reads the split's slot from the current solve and asks
    /// `bt-layout::apply` for the ratio: the clamp, and the refusal when the
    /// clamp is unsatisfiable, are §2.4's and are not re-derived here. Red line
    /// L9 is upheld by the edit itself — `DragDivider`'s focus set is exactly
    /// that one split, so nothing rebalances mid-gesture.
    pub(in crate::runtime) fn drive_divider_drag(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(drag) = self.window.divider_drag else {
            return Ok(false);
        };
        let Some(slot) = self
            .seats
            .split_slots(&self.seat_layout)
            .into_iter()
            .find(|slot| slot.id == drag.split)
        else {
            self.window.divider_drag = None;
            return Ok(false);
        };
        let along = match drag.dir {
            Axis::Row => position.x,
            Axis::Col => position.y,
        };
        let scale_ppm = seats::scale_ppm(self.window.renderer.metrics().dpi_milli().get());
        let metrics = self.seat_metrics();
        // A refusal, and a clamp that changed nothing, both mean "do not
        // re-solve": §2.4 rules that an infeasible drag has zero side effects
        // rather than writing a value the next solve will "correct", which
        // would dress a refusal up as a jitter.
        //
        // The two grips ask the same question of the same slot and differ only
        // in what they write — a proportion or a width — which is exactly the
        // difference §3.4 draws. Both defer their clamp to `bt-layout::apply`,
        // so neither holds an opinion this module could get out of step with.
        let moved = match drag.grip {
            DividerGrip::Ratio(_) => {
                let Some(reserved) = self.seats.split_reserved(&metrics, drag.split) else {
                    return Ok(true);
                };
                let Some((requested, usable)) =
                    seats::requested_ratio(slot, reserved, scale_ppm, along)
                else {
                    return Ok(true);
                };
                self.seats
                    .drag_divider(&metrics, drag.split, requested, usable)
                    == Ok(true)
            }
            DividerGrip::FixedExtent { leading, .. } => {
                let Some((requested, usable)) =
                    seats::requested_fixed_extent(slot, leading, scale_ppm, along)
                else {
                    return Ok(true);
                };
                self.seats
                    .drag_fixed_extent(&metrics, drag.split, requested, usable)
                    == Ok(true)
            }
        };
        if moved {
            // **The second door into sovereignty** (最小值主权, 2026-08-08: "a
            // minimum is law to the program and advice to the user").
            //
            // `Edit::DragDivider` already writes the ratio the hand asked for
            // without consulting a minimum — that half was done the day the
            // ruling landed. What was missing is that the *solve* which turns
            // that ratio into rectangles still ran under `Lawful`, so the
            // concession chain put the layout straight back: measured on a
            // restored 960-logical window holding two preview panes and a
            // terminal, dragging the root divider from 1/3 to 1/2 moved nothing
            // at all — the terminal stayed collapsed at 24px and both previews
            // stayed on their 360 floor. Under `Sovereign` the same ratio gives
            // the terminal 254px and lets the two previews fall to 352 together,
            // which is the ruling's own words: past the floors, the floors give
            // way *in proportion*.
            //
            // Taken here rather than only for the frame the button is down,
            // because "sovereignty, once taken, is returned only by another
            // claim" (`size_authority_for_rectangle`): a layout that snapped
            // back the instant the hand let go would be the refusal arriving one
            // frame late. `claim_lawful_layout` is what hands the minima back
            // their force, and it is the program's own door.
            self.window.size_policy = SizePolicy::Sovereign;
            self.commit_seat_geometry()?;
        }
        Ok(true)
    }

    /// F71/T225: abandon a divider drag and put back the one value it moved.
    ///
    /// "Never mind, and no commit" — the same sentence Esc says to a tab drag,
    /// and it reaches here by the same two doors: the Esc key, and losing the
    /// window, which on Win32 is losing the mouse capture and is this platform's
    /// only `pointercancel` (F72). A drag that ends without either a button-up
    /// or a teardown would keep steering the layout from a pointer nobody is
    /// holding.
    ///
    /// The restore goes back through `Edit::DragDivider` rather than writing the
    /// ratio into the tree directly, and that is the point of it: §2.4's
    /// feasibility judgement and clamp are asked once more, so if the viewport
    /// changed mid-gesture the answer is the *current* legal value nearest the
    /// one we started from, rather than a ratio that was legal for a window that
    /// no longer exists. When nothing changed the clamp is idempotent and the
    /// origin comes back byte for byte.
    ///
    /// Zero side effects when there is nothing to undo: a press that never moved
    /// the ratio restores a value equal to the one already there, `drag_divider`
    /// reports no change, and no re-solve is asked for.
    /// **End a divider drag the system has taken the pointer away from** (Codex review
    /// 2026-09-17), through the same door `Esc` and a blur use — so it restores the one ratio it
    /// was moving, exactly as an unfinished gesture must.
    ///
    /// The whole of the test is [`divider_drag_still_holds_its_pointer`]; what is here is the one
    /// sample it is asked about, taken only while a drag is actually in the air.
    pub(in crate::runtime) fn end_a_divider_drag_that_lost_its_pointer(&mut self) -> Result<bool> {
        let Some(drag) = self.window.divider_drag else {
            return Ok(false);
        };
        if divider_drag_still_holds_its_pointer(drag.capture, bt_platform::thread_mouse_capture()) {
            return Ok(false);
        }
        self.cancel_divider_drag()
    }

    pub(crate) fn cancel_divider_drag(&mut self) -> Result<bool> {
        let Some(drag) = self.window.divider_drag.take() else {
            return Ok(false);
        };
        self.window.seat_pointer.dragging = None;
        let usable = self
            .seats
            .split_slots(&self.seat_layout)
            .into_iter()
            .find(|slot| slot.id == drag.split)
            .map(|slot| slot.slot.extent(slot.dir) - bt_layout::DIVIDER);
        let metrics = self.seat_metrics();
        let restored = match (usable, drag.grip) {
            // The split is gone from the solve, so there is no value of its to
            // put back and nothing to re-solve for.
            (None, _) => false,
            (Some(usable), DividerGrip::Ratio(origin)) => {
                self.seats
                    .drag_divider(&metrics, drag.split, origin, usable)
                    == Ok(true)
            }
            // The pixel half of the same sentence, and it goes back through the
            // same edit for the same reason: a window that changed size under
            // the gesture gets the current legal width nearest the one we
            // started from, rather than a width that fitted a slot that is gone.
            (Some(usable), DividerGrip::FixedExtent { origin, .. }) => {
                self.seats
                    .drag_fixed_extent(&metrics, drag.split, origin, usable)
                    == Ok(true)
            }
        };
        if restored {
            self.commit_seat_geometry()?;
        }
        self.apply_pointer_cursor();
        if let Some(position) = self.window.pointer_position {
            self.update_chrome_hover(position)?;
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// **The docked chrome's own ladder** — everything this window draws *in*
    /// the layout, smallest target first.
    ///
    /// Reached only through [`Self::pointer_target_at`], which asks the floating
    /// windows first, and private to it for the reason the whole of §7.15 ⑩
    /// exists: this ladder knows nothing about floats, so anything that called
    /// it directly would be reading the chrome *behind* a window as though the
    /// window were not there. It keeps `&self` because that is what it is — a
    /// walk of solved rectangles — and the router above it is the one that needs
    /// a face to measure a caption with.
    pub(in crate::runtime) fn docked_chrome_target_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<seats::ChromeTarget> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, _) = self.window.renderer.presentation_geometry().swapchain_size;
        let width = width as f32;
        let rail = self.sampled_rail(Instant::now());
        self.tab_list_target_at(position)
            .or_else(|| {
                seats::hit_window_chrome(
                    width,
                    scale,
                    self.platform_chrome(),
                    rail,
                    self.is_quake_window(),
                    position.x,
                    position.y,
                )
            })
            // Before the pane heads, because the root button lives *inside* one:
            // B17's judgement that a single element can be both a drag handle and a
            // button only holds if the button answers first.
            .or_else(|| {
                seats::hit_files_root(
                    &self.seat_layout,
                    &self.window.files_name_widths,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // And before them for the same reason: the preview head's four controls
            // live inside the drag handle too, and three of them are dead zones in it
            // (the mock-up's `.pane-close, .pane-files, .pv-tool` exclusion, 5874).
            .or_else(|| {
                // Every preview seat's own tools, so the head the walk is standing on
                // is laid out to the name *it* is drawn with. One set for all of them
                // put the second head's buttons wherever the first head's name
                // happened to end.
                let tools: Vec<(SeatId, seats::PreviewHeadTools)> = self
                    .seats
                    .preview_seats()
                    .into_iter()
                    .map(|seat| (seat, self.preview_head_tools(seat)))
                    .collect();
                // The reveal, on [`seats::hit_chrome`]'s own terms and for its own
                // reason: these five are the split head's run in another head.
                seats::hit_preview_head(
                    &self.seat_layout,
                    scale,
                    &tools,
                    self.head_run(),
                    position.x,
                    position.y,
                )
            })
            // And the row under it (user ruling 2026-08-24), on the same terms: its
            // controls stand inside a band the pane head's drag would otherwise
            // claim, and the band itself answers for what its controls leave over —
            // see [`seats::ChromeTarget::PreviewRail`]. Asked after the head only
            // because the two never overlap; either order gives the same answers,
            // and this one reads in the order the rows are drawn.
            .or_else(|| {
                let rails: Vec<(SeatId, seats::PreviewRailMeasure)> = self
                    .seats
                    .preview_seats()
                    .into_iter()
                    .filter_map(|seat| {
                        Some((seat, self.preview_rail_measure(self.preview_here(seat))?))
                    })
                    .collect();
                seats::hit_preview_rail(&self.seat_layout, scale, &rails, position.x, position.y)
            })
            // The lone pane's corner ghost (§7.1.6i), before the pane heads for the
            // same smallest-target-first reason everything above it is asked first:
            // it floats *over* the terminal's own body, and the body is not chrome,
            // so nothing below would ever return it to the head's arm to be
            // out-ranked.
            .or_else(|| {
                seats::hit_pane_ghost(
                    &self.seats,
                    &self.seat_layout,
                    scale,
                    self.window.search.seat(),
                    position.x,
                    position.y,
                )
            })
            .or_else(|| {
                seats::hit_chrome(
                    &self.seats,
                    &self.seat_layout,
                    scale,
                    // **The run that is on screen, not the run the rectangles
                    // allow** (user report, 2026-08-26). A pane head's `×`, `⌄`,
                    // folder and pop-out are drawn only while the hand is inside
                    // that pane, and this hit test used to answer for them whatever
                    // the hand had done — so a press that arrived without a hover
                    // first (a probe's injected click, or the press right after a
                    // resize, which clears `pane_hover` because the border being
                    // dragged is non-client) tore a column out into a floating
                    // window from a head showing nothing but a drag handle.
                    //
                    // It is the *stored* hover and not a fresh `pane_at` of this
                    // very position, which is what makes it the same fact the paint
                    // used: this is asked on the press as well as on the move, and
                    // on the press the only honest question is "what is the reader
                    // looking at".
                    //
                    // And since 裁4 (2026-08-26) it is the *pair*: a head whose own
                    // menu is standing is showing its run even though `pane_hover`
                    // was cleared the instant the hand reached the list, so the
                    // `✕` beside the `⌄` that opened it goes on taking a press.
                    self.head_run(),
                    position.x,
                    position.y,
                )
            })
            // Last, and only for what the pane heads left over: a files column's
            // head carries the same `×` and the same drag handle every other pane
            // has, and the tree lives strictly below it. Asking the rows first would
            // put a row where the close button is on any column scrolled far enough.
            // The foot before the rows, and before the rows can even reach it: the
            // tree's body now stops at the strip, so the two cannot overlap. Asked
            // first anyway, for the reason the root button is asked before the head
            // it lives in — a control's claim on its own rectangle should not depend
            // on a *second* rectangle happening to have been shortened correctly.
            .or_else(|| {
                seats::hit_files_foot(
                    &self.seats,
                    &self.seat_layout,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // The card's button, asked only while the card is up: a control that
            // answers a hit test it is not on screen for is an invisible button.
            .or_else(|| {
                let seat = self.seats.preview()?;
                let button = self.window.preview_card_verbs.get(&seat).copied()?;
                seats::hit_preview_card_button(
                    &self.seats,
                    &self.seat_layout,
                    button,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // **The switch before the page it switches**, and before the tree too:
            // it stands between the head and the body, and a thirty-pixel strip the
            // body also claimed would be a control you cannot press.
            .or_else(|| {
                seats::hit_files_seg(
                    &self.seat_layout,
                    &self.files_seg_widths(),
                    scale,
                    position.x,
                    position.y,
                )
            })
            .or_else(|| {
                seats::hit_git_panel(
                    &self.seat_layout,
                    &self.window.git_pages_shown,
                    scale,
                    position.x,
                    position.y,
                )
            })
            // **The play button, in the preview seat's own body** (user ruling
            // 2026-08-27; §7.23 ⑩) — asked here for the graph's own reason one
            // rung down, and asked *before* it because the two cannot both be on
            // one pane and the cheaper question reads first: a set membership
            // against a solved rectangle.
            //
            // Answering here rather than in `chrome_mouse_input`'s fallback ladder
            // is the ruling: this pane's body is a picture, and every rung of that
            // ladder — the body thumb, a link, a block's scrollbar, the picture's
            // own pan — is a gesture *on* the picture. A control standing on it
            // out-ranks all of them, exactly as the graph's toolbar does.
            .or_else(|| {
                seats::hit_preview_play(
                    &self.seats,
                    &self.seat_layout,
                    &self.seats_wearing_a_play_button(),
                    scale,
                    position.x,
                    position.y,
                )
            })
            // The graph, in the preview seat's own body. Asked here rather than
            // among the preview pane's controls because it *is* the body: nothing
            // else claims that rectangle while a graph is on it, and the document
            // underneath has no text to place a caret in.
            .or_else(|| {
                seats::hit_git_graph(
                    &self.seats,
                    &self.seat_layout,
                    // The docked ones. A window's graph is answered by the float
                    // host's own hit test, which runs before this whole chain.
                    // **And this tab's,** which the key now says rather than the
                    // hit test assuming: `git_graphs_shown` is the window's map and
                    // spans every tab, so before §7.12 ⓑ a graph standing on a
                    // background tab offered its rows to a press on the seat of the
                    // same number in front of you.
                    self.window
                        .git_graphs_shown
                        .iter()
                        .filter_map(|(surface, content)| match surface {
                            PreviewSurface::Seat(leaf) if leaf.tab == self.id => {
                                Some((leaf.seat, content))
                            }
                            PreviewSurface::Seat(_)
                            | PreviewSurface::Float(_)
                            | PreviewSurface::Peek => None,
                        }),
                    scale,
                    position.x,
                    position.y,
                )
            })
            .or_else(|| {
                seats::hit_files_tree(
                    &self.seat_layout,
                    &self.files_tree_contents(),
                    scale,
                    self.git_panel_on(),
                    position.x,
                    position.y,
                )
            })
    }

    pub(in crate::runtime) fn update_chrome_hover_target_in_pane(
        &mut self,
        hover: Option<seats::ChromeTarget>,
        pane: Option<bt_layout::SeatId>,
    ) -> Result<()> {
        if self.window.seat_pointer.hover == hover && self.window.seat_pointer.pane_hover == pane {
            return Ok(());
        }
        // **The play button is not in the chrome pass**, so the ordinary
        // repaint below does not reach it (§7.23 ⑩). It is the one control this
        // window draws in the preview body's own raster lane, and that lane is
        // rebuilt by `refresh_preview_body` — asked here only when the pointer
        // actually crossed this button, because rebuilding every pane's document
        // on every pointer move is the cost that lane exists to avoid.
        let crossed_the_play_button = [self.window.seat_pointer.hover, hover]
            .into_iter()
            .flatten()
            .any(|target| matches!(target, seats::ChromeTarget::PreviewPlay(_)));
        self.window.seat_pointer.hover = hover;
        self.window.seat_pointer.pane_hover = pane;
        if crossed_the_play_button {
            self.refresh_preview_body();
        }
        self.apply_pointer_cursor();
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// J118 — the press on a pane head has travelled 6px, so the pane is in the
    /// air.
    ///
    /// Nothing is measured and nothing is committed. A pane's head is not a
    /// handle the pane hangs from: the tree stays exactly as it was for the whole
    /// gesture, and the only thing that moves is the ghost. That is the mock-up's
    /// shape too — `startDrag(e, { kind: "pane", leafId })` records a leaf and
    /// nothing else (5839) — and it is why a pane drag that comes to nothing
    /// leaves no trace at all.
    ///
    /// The `.pane-close` dead zone (C35, mock-up 5837) needs no guard here: it is
    /// [`seats::ChromeTarget::PaneClose`], a target of its own, so a press on the
    /// `×` never reaches the head's arm of the router in the first place. "The
    /// button is not the bar" is true at the hit test, which is the only place it
    /// can be true once rather than everywhere.
    pub(in crate::runtime) fn begin_pane_drag(
        &mut self,
        seat: SeatId,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        // **No focus-mode guard, and that is a ruling rather than a gap.** The
        // mock-up refuses every pane drag while the mode is on ("focus mode: the
        // tree is parked, no pane drags", 5841) because its focus mode parks the
        // tree behind a stage. Ours does not: §7.1.6b′ ① is that the mode
        // replaces the *tab list* and nothing else — "`render` 里没有聚焦分支,
        // 画的仍是 `renderNode(activeTab().tree)`" — so the stage in front of the
        // column is the ordinary tree, its panes are the ordinary panes, and a
        // drag among them is the ordinary gesture. What the mode does refuse is
        // the tear-out, and that refusal lives where the surface being dropped
        // on is described (`seats::PaneOffers::extract`, ②) rather than at the
        // moment a pane leaves the ground.
        // K135 covers a pane already, and by identity rather than by geometry:
        // a pane held over its own rectangle has no landing however that
        // rectangle has moved since the press. A home rectangle here would be
        // the same rule stated worse.
        // **The tab is recorded here and never re-read** (§7.1.6k). A pane drag
        // can outlive its tab being on screen now — the spring switches the view
        // and leaves the pane where it was picked up — so "which pane" has to be
        // answered once, at the moment the hand closed, rather than by asking
        // which tab happens to be showing later on.
        let leaf = LeafId {
            tab: self.window.tabs[self.window.active_tab].id,
            seat,
        };
        self.begin_drag(DragSource::Pane(leaf), DragCarry::Pane, position, None)
    }

    /// **H93 for §7.1.6k's landing** — whether `target`'s tree can actually take
    /// this pane at its end.
    ///
    /// The same [`seats::Seats::plan_drop`] the release will run, on the same
    /// inputs, asked for the promise instead of the commit. It is *re-asked*
    /// rather than remembered for [`Runtime::commit_layout_drop`]'s reason: a
    /// plan is what letting go would produce, computed, never a remembered
    /// picture — and a survey answered from a cache would go on lighting a tab
    /// up after a window resize made the drop unlawful.
    ///
    /// One tree walk and one solve per pointer move, on a tree of a few leaves,
    /// and only while a pane is actually resting on a foreign tab.
    pub(in crate::runtime) fn pane_adopt_fits(&self, leaf: LeafId, target: TabId) -> bool {
        let Some(from) = self.tab_state(leaf.tab) else {
            return false;
        };
        let Some(travelling) = from.seats.tree().find_seat(leaf.seat).cloned() else {
            return false;
        };
        self.arrival_fits(target, &bt_layout::LayoutNode::seat(travelling))
    }

    /// The layout's own box in device pixels — what every rim distance is
    /// measured from (K128/K130).
    ///
    /// Built from the swapchain and the DPI rather than from the seats inside it,
    /// through the same helper the solver's viewport comes from, so the rim and
    /// the rectangles it competes with cannot disagree about where the layout
    /// begins.
    pub(crate) fn layout_host_rect(&self) -> [f64; 4] {
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let dpi_milli = self.window.renderer.metrics().dpi_milli().get();
        let scale_ppm = seats::scale_ppm(dpi_milli);
        // The same inset `solve_seats` hands `logical_viewport`, through the same
        // helper: these two are twins, and a rim measured one pixel to the left
        // of where the solver put the terminal is a left-edge drop zone that aims
        // into the rail.
        seats::device_viewport(
            width,
            height,
            // The posture, because the solver is handed the posture: a rim
            // measured against the stored preference would aim its left-edge drop
            // zone at the pixels the card column is standing on.
            seats::rail_inset_device_px(self.rail_posture(), scale_ppm),
            // The same bar the solver's viewport starts under, through the same
            // helper and for [`seats::device_viewport`]'s own reason: a rim
            // measured one row above where the seats begin is a top-edge drop
            // zone aimed at the bar.
            seats::chrome_band_device_px(scale_ppm, self.platform_chrome()),
        )
    }

    /// **U7 — let go over the layout** (L136-L140, G81-G83, D43).
    ///
    /// The plan is computed from the drag's own inputs rather than lifted out of
    /// [`WindowRuntime::drop_preview`], and the two are the same object for a reason
    /// that is not a coincidence: `plan_drop` is pure (T223/D2), so the same
    /// inputs give back the same tree to the bit. Reading the cache would be
    /// asking a *remembered* answer to a question the world may have changed
    /// under — the fade that keeps a retired preview alive for its hundred
    /// milliseconds is enough on its own to make the cache older than the
    /// release — and re-asking costs one tree walk on a tree of a few leaves.
    ///
    /// Everything after the adoption is what a tree change costs, in the order
    /// `close_pane` and the preview toggle already pay it: the pointer's picture
    /// is stale because the rectangles moved under it, the window's own minimum
    /// is a function of the tree, and the terminal's columns are a function of
    /// its seat's rectangle. `commit_seat_geometry` marks the session dirty on
    /// the way through, so the strip's order, the tree's shape and the focused
    /// leaf all reach disk through the one channel that already carries them.
    ///
    /// Answers whether the tree changed.
    pub(in crate::runtime) fn commit_layout_drop(&mut self, drag: &Drag) -> Result<bool> {
        let Some(inputs) = self.plan_inputs_for(drag) else {
            return Ok(false);
        };
        let Some(plan) = self.plan_for(&inputs) else {
            return Ok(false);
        };
        // Three things are read off the *old* world, because the adoption below
        // replaces it and none of them can be asked afterwards.
        //
        // `arrived` is the renumbering's own record (N159/D44) and moves out of
        // the plan, which `adopt_drop` consumes. The displaced seat is N161's:
        // `ReplaceSeat` seats the arriving tree where the target pane stood, so
        // by the time the tree is installed there is no such seat to clone, and
        // everything durable about that pane lives on the `Seat` (§5).
        //
        // N160②'s "which pin does the ejected pane inherit" is *not* captured
        // here even though it is the same kind of before-and-after question. It
        // belongs beside the N160① that would overwrite it, which is
        // [`absorb_tab_into_layout`] — where a test can run the two in order.
        // **A row's centre verb is committed before anything is adopted, and
        // there is nothing to adopt.** `plan_for` built the *live* tree for it
        // (see [`seats::Seats::plan_content_drop`]), so `adopt_drop` would
        // install the layout that is already installed and move the focus for a
        // gesture that moved no pane. What the drop actually is, is one sentence
        // about one pane's content — and the verb table decides which.
        if let Some(payload) = drag.source.row() {
            let target_kind = self.aimed_seat_kind(inputs.landing);
            match row_verb(payload.kind, inputs.landing, target_kind) {
                // M147 — the box said so while the hand was open, and the
                // release says the same thing by doing nothing at all.
                RowVerb::Refused => return Ok(false),
                RowVerb::Retarget(target) => {
                    self.retarget_row_drop(payload, target)?;
                    return Ok(true);
                }
                // **The 2026-09-16 ruling's own line, behind the 2026-09-17
                // review's three questions.** A path let go of over a terminal's
                // middle goes through the door an Explorer or Finder drop goes
                // through ([`Runtime::paste_paths_into`]), so the quoting, the
                // `paste_paths_as` spelling, the refusal notice and the delivery
                // are that road's rather than a second one that drifts from it.
                // The keyboard does not move to it, because a drop is a pointer
                // gesture. What it must *not* inherit from the rest of the drag
                // engine is the cached aim — see [`Self::paste_offer_kept`].
                RowVerb::PastePath(_) => {
                    let Some(target) = self.paste_offer_kept(drag, &plan) else {
                        return Ok(false);
                    };
                    let path = payload.path.clone();
                    // **And the keyboard follows the path, strictly afterwards**
                    // (owner's ruling 2026-09-17). Behind the write's own
                    // answer, so a release that reached no shell — a stale
                    // target, a name the shell cannot spell — moves nothing:
                    // the reader is left exactly where they were, which is what
                    // every other refusal on this road already does. And it is
                    // the seat of the `PasteTarget` the two readings agreed on
                    // — never the hover-time seat, and never whichever pane
                    // happens to be holding the keyboard.
                    if self.paste_paths_into(target, vec![path], "write dragged path to PTY")? {
                        self.focus_the_pane_a_path_landed_in(target.seat)?;
                    }
                    return Ok(true);
                }
                RowVerb::Split => {}
            }
        }
        // **§7.1.6k′ — a pane the spring left in another tab is a *cross-tab*
        // move, however much the aim looks like a local one.**
        //
        // Everything above this line edits one tree: `adopt_drop` installs the
        // plan into the tab on screen and the source, if there was one, is a tab
        // giving up its whole self. A foreign pane has to be plucked out of a
        // tree nobody is drawing and filed into this one, its seven tables of
        // content moving with it and its own tab possibly emptying — which is
        // [`Runtime::move_pane_across_tabs`] to the letter, and the very door the
        // tab list's drop already goes through.
        //
        // `plan` is dropped here rather than handed over, and that costs one tree
        // walk and buys the discipline `commit_layout_drop` is built on: the
        // criterion is `plan_for`'s, and `plan_drop` is pure (T223/D2), so the
        // plan the commit rebuilds from the same inputs is the same tree to the
        // bit. What must not be re-decided is the *judgement*, and it is not —
        // a refusal reaches the commit through `plan.fits()` here and through the
        // same [`pane_can_become_a_tab`] there.
        let showing = self.window.tabs[self.window.active_tab].id;
        if let Some(leaf) = drag.source.pane().filter(|leaf| leaf.tab != showing) {
            // M147 — the dashed box already said so while the hand was open, and
            // the release says the same thing by doing nothing at all.
            let Some(aim) = inputs.landing.layout_aim().filter(|_| plan.fits()) else {
                return Ok(false);
            };
            return self.move_pane_across_tabs(leaf, showing, aim);
        }
        let arrived = plan.arrived.clone();
        // **The seat the plan minted for a row**, read before `adopt_drop`
        // consumes the plan. `arrived` is the renumbering's own record and a row
        // arrives as exactly one leaf, so the single pair in it is the identity
        // the payload has to be filed under — derived from the plan rather than
        // re-found in the new tree, which is the same argument N159's remapping
        // already makes.
        let landed_row = drag
            .source
            .row()
            .and_then(|_| arrived.first().map(|(_, landed)| *landed));
        // N161 — a centre with a whole *layout* in the hand evicts the target
        // pane. Only a tab can reach this line holding one: the foreign pane's
        // arm above has already returned, and every other source either arrives
        // as a seat of this tree (a swap, which evicts nobody) or as a leaf the
        // plan minted (a row, which never reaches a centre as a space verb).
        let displaced = match (inputs.landing, &drag.source) {
            (DropLanding::SeatCentre { target }, DragSource::Tab(_)) => {
                self.seats.tree().find_seat(target).cloned()
            }
            _ => None,
        };
        if self.seats.adopt_drop(plan).is_none() {
            return Ok(false);
        }
        if let (Some(payload), Some(seat)) = (drag.source.row(), landed_row) {
            // The pane exists and is empty; filling it is the same verb the
            // ordinary doors run, on a surface named rather than chosen.
            self.settle_seat_set_change()?;
            self.fill_row_leaf(&payload.clone(), seat)?;
            return Ok(true);
        }
        if let Some(source) = drag.tab() {
            self.absorb_tab(source, &arrived, displaced)?;
        }
        debug_assert!(
            self.sessions_match_terminals(),
            "item 6: a layout drop leaves the tab's shells matching its tree"
        );
        self.settle_seat_set_change()?;
        Ok(true)
    }

    /// **N157/K123 — let go of a pane over the strip.**
    ///
    /// [`tear_pane_into_tab`] does the whole of the move and is a free function
    /// so that a test can run it over a constructed `TabState`. What is left here
    /// is the window's: minting the tab id, handing over the solver, and putting
    /// the new entry in the run.
    ///
    /// **The new tab is not activated, and N157 is explicit about it.** You stay
    /// in the layout you were in and the pane waits for you in the strip — the
    /// gesture was "put this over there", not "take me there". So `active_tab`
    /// only moves to keep naming the tab it already named, which the insertion
    /// shifts by one when it lands at or before it.
    ///
    /// Its shell keeps the grid it had until you go and look at it, which is the
    /// same deal a revived tab gets: `activate_tab` re-solves the layout and
    /// schedules the grid change on the way in, and a shell resized to a
    /// rectangle nobody is drawing would be told a column count that never
    /// reaches the screen.
    ///
    /// Answers whether anything moved. `false` leaves the gesture to J120's slide
    /// home, which for a pane is a no-op — the tree was untouched for the whole
    /// drag.
    pub(in crate::runtime) fn commit_pane_extract(
        &mut self,
        drag: &Drag,
        slot: usize,
    ) -> Result<bool> {
        let DragSource::Pane(leaf) = drag.source else {
            return Ok(false);
        };
        // Whether anything moved is the whole of what a drop reports; the id the
        // tear-out now hands back is F1c's caller's, not this one's.
        Ok(self.extract_pane_into_new_tab(leaf, slot)?.is_some())
    }

    /// **§7.1.6k — let go of a pane over one of the strip's own tabs.**
    ///
    /// [`pane_into_tab`] does the whole of the move and is a free function so
    /// that a test can run it over two constructed `TabState`s. What is left here
    /// is the window's: finding the two entries, handing over the solver and the
    /// setting, and — when the tab the pane left is now empty — taking its entry
    /// out of the run.
    ///
    /// **The target is not activated**, and N157 is the precedent rather than an
    /// analogy: you aimed at a tab and said "put this over there", not "take me
    /// there". So the view does not move, and the pane waits for you in the tab
    /// you pointed at. The one exception is the tab you were *looking at* ceasing
    /// to exist because you moved its last pane away — there is no layout left to
    /// stay in, and the honest place to be is the one the pane went to.
    ///
    /// Answers whether anything moved. `false` leaves the gesture to J120's slide
    /// home, which for a pane is a no-op — the trees are untouched on that path.
    pub(in crate::runtime) fn commit_pane_adopt(
        &mut self,
        drag: &Drag,
        target: TabId,
    ) -> Result<bool> {
        let DragSource::Pane(leaf) = drag.source else {
            return Ok(false);
        };
        // **The tab list aims at the end of the tree and always has**, whichever
        // surface asked: *"追加为树末尾分屏"*. It is stated here rather than
        // inside [`Runtime::move_pane_across_tabs`] because it is what this
        // *door* means; the stage's door names a zone instead, and the thing
        // they share is everything after the aim.
        self.move_pane_across_tabs(leaf, target, seats::LayoutAim::Rim(self.append_edge()))
    }

    /// **The rail as it stands this instant** — the stored layout with both of
    /// its clocks read.
    ///
    /// The one place the two tweens become a [`seats::RailState`], so the paint,
    /// the hit test and the terminal's inset cannot be looking at three different
    /// moments of the same animation.
    ///
    /// Outside icon mode the fade is simply not a rule: the mock-up scopes every
    /// `opacity` declaration in that list to `.window.rail-icons` (lines 892-903),
    /// so an expanded rail's words are *there*, full stop, and are not waiting on
    /// a tween that has no reason to have been started.
    /// The fold is sampled on both paths and outside that `if`, because it is the
    /// one clock that runs while the rail is *not* drawing an icon rail — a rail
    /// folding away is collapsing, and `draws_icon_rail()` is false throughout.
    /// Reading it only on the icon path is precisely how the fold would go back to
    /// snapping.
    pub(crate) fn sampled_rail(&self, now: Instant) -> seats::RailState {
        let fold = Some(self.window.rail_fold.sample(now, self.app.motion).0);
        // **The window's posture, and the clocks on top of it.** The join of the
        // two halves is [`Self::rail_posture`]'s one line and is taken from there
        // rather than written a second time here: two spellings of "the rail as
        // this window wears it" is how the paint and the solver came to disagree
        // about whether a card column was on screen.
        //
        // Asked of the state as it *will be* rather than of the stored one: a
        // window in focus mode draws no icon rail whatever `Sidebar` says, so
        // asking the stored rail would sample the opening tween of a panel that
        // is not on screen.
        let resting = self.rail_posture();
        if !resting.draws_icon_rail() {
            return seats::RailState {
                open: 1.0,
                text_opacity: 1.0,
                fold,
                ..resting
            };
        }
        seats::RailState {
            open: self.window.rail_open.sample(now, self.app.motion).0,
            text_opacity: self.window.rail_text.sample(now, self.app.motion).0,
            fold,
            ..resting
        }
    }

    /// The rail's state as the *window* is currently wearing it — the stored
    /// preference with this window's focus-mode bit joined to it.
    ///
    /// [`Self::sampled_rail`] without the clock, for the callers that ask a shape
    /// question outside a frame: "is an icon rail live", "how much width does the
    /// panel cost". Those must answer to the posture and not to the preference,
    /// or a window in focus mode goes on reasoning about a rail nobody is
    /// drawing.
    ///
    /// **The solver is one of those callers, and that is the whole of the fix
    /// this branch carries.** The card column is in the flow exactly as an
    /// expanded rail is: `terminal_inset_logical_px` answers the column's own
    /// width for it, and the viewport `solve_seats` is handed begins there. Passing
    /// [`WindowRuntime::rail`] instead — which it did — is asking the solver to
    /// lay the stage out as though the panel were not on screen, and a panel
    /// drawn over a stage that was never told about it is the occlusion the
    /// screenshots showed.
    /// **And the card height joins here too** (user ruling 2026-08-21).
    ///
    /// `Appearance ▸ Focus card height` is a *file* preference, like the mode's
    /// own bit — one answer for every window — while `window.rail` holds the two
    /// preferences this window happens to be wearing. Joining it at the one
    /// place the posture is assembled is what makes the row take effect at once
    /// and in every window: nothing caches a card height, so the next frame
    /// solves the column at whatever the file now says. A copy kept on
    /// `WindowRuntime` would be a second answer to re-sync on every door.
    pub(crate) fn rail_posture(&self) -> seats::RailState {
        seats::RailState {
            focus: self.window.focus_mode,
            focus_card_body_logical_px: self.app.settings_store.loaded().focus_card_height as f32,
            ..self.window.rail
        }
    }

    /// **A pane that changed its address takes its page with it** (§7.10 ④‴).
    ///
    /// [`move_seat_content`] and [`pane_into_new_tab`] carry the seven tables a
    /// pane can be holding, and a page is not one of them — it cannot be, because
    /// [`WindowRuntime::web`] belongs to the **window** and those two are
    /// functions over a [`TabState`]. So the pane's other half is carried here,
    /// by the three window-level doors that know both ends of the journey:
    /// [`Self::extract_pane_into_new_tab`], [`Self::move_pane_across_tabs`] and
    /// [`Self::absorb_tab`].
    ///
    /// **Without it the browser is not merely misfiled — it is closed.** The map
    /// is keyed by [`LeafId`] and both halves of that key change on a move (the
    /// arriving tree re-mints its seat numbers from one, and the tab is a
    /// different tab), so the page went on answering to a name whose tab no
    /// longer has that seat; `advance_web_page` asks exactly that question of
    /// exactly that tab, called the page orphaned and shut the controller down.
    /// On the machine that is a preview pane dragged onto the tab strip whose
    /// `.html` or `.pdf` goes **blank** while its head and address bar — which
    /// travel with the pane — go on naming the file (user report on `next21`).
    ///
    /// **Taken out of the table before any of them is put back**, which is not
    /// tidiness: a centre *trades* (§7.1.6k′ B4), and the pane that arrives is
    /// filed under the id the pane that left was filed under, so an insert
    /// interleaved with the removes would have one live browser overwrite the
    /// other and lose it without going through the door that waits for its
    /// process to exit.
    ///
    /// [`webhost::WebSeat::rehost`] and not a bare re-key, for the reason written
    /// on it: the seat caches its own address, and the compositor's visual table
    /// is keyed by that address too, so moving only the map entry would leave the
    /// next rebuild asking for a controller under a name nothing answers to. Both
    /// sides are this window's compositor because this is a move inside one
    /// window — the same call the float's re-dock already makes.
    ///
    /// **The keyboard is not taken.** `transfer_tab` passes `take_focus` for the
    /// pane the moved tab is standing on because a person carried that tab into
    /// this window; a tear-out deliberately does not activate the tab it makes
    /// (N157), and a pane dropped on another tab lands wherever the layout put
    /// the focus. Whether the page is what typing goes into is then
    /// `settle_the_web_keyboard`'s answer on the next frame, as it is for every
    /// other page in this window.
    pub(in crate::runtime) fn carry_the_pages_of_moved_panes(
        &mut self,
        moves: &[(LeafId, LeafId)],
    ) -> Result<()> {
        let hosted: BTreeSet<LeafId> = self.window.web.keys().copied().collect();
        let travelling = pages_that_move_with_their_panes(&hosted, moves);
        if travelling.is_empty() {
            return Ok(());
        }
        let native = native_window(&self.window.window)?;
        let carried: Vec<(LeafId, webhost::WebSeat)> = travelling
            .iter()
            .filter_map(|(was, now)| {
                let page = self.window.web.remove(was)?;
                let picture = self.window.web_thumbs.take(*was);
                if let Some(picture) = picture {
                    self.window.web_thumbs.put(*now, picture);
                }
                Some((*now, page))
            })
            .collect();
        for (now, mut page) in carried {
            let mut outcomes = Vec::new();
            let report = page.rehost(
                &self.window.compositor,
                &self.window.compositor,
                webhost::SeatAddress {
                    page: bt_platform::PageVisual {
                        tab: now.tab.0,
                        seat: now.seat.0,
                    },
                    window: native,
                },
                false,
                &mut outcomes,
            );
            if let Some(error) = report.error() {
                eprintln!("BT_WEB the page could not follow its pane: {error}");
            }
            self.window.web.insert(now, page);
            self.apply_web_outcomes(now, outcomes)?;
        }
        Ok(())
    }

    /// **A pane that changed its address takes its recording with it** (user
    /// report on `next22`, defects #202/#204; §7.44 ⑮).
    ///
    /// [`Self::carry_the_pages_of_moved_panes`] said one tab-level table apart
    /// from the seven a pane carries; this is the second, and it is a separate
    /// door rather than a second half of that one because the two tables are
    /// different tables with different keys: [`WindowRuntime::web`] is keyed by
    /// [`LeafId`] and [`WindowRuntime::video`] by [`PreviewSurface`], since a
    /// recording plays on a pane, on a float and on the glance card while a page
    /// only ever plays on a pane. Only the first of those three is a leaf, so
    /// only the first of them can be part of a move at all — which is the whole
    /// of the filter below.
    ///
    /// **Without it the recording does not merely lag — it is shut down.**
    /// [`Runtime::sweep_video_seats`] keeps a seat while the surface it is keyed
    /// by is still about the file it was opened on, and both halves of that key
    /// change on a move; so the seat went on being keyed by an address the
    /// window no longer has, the sweep read it as a surface that has stopped
    /// existing, and the decoder was stopped on the next frame. On the machine
    /// that is a video pane dragged onto the tab strip whose picture goes
    /// **blank** while its head, its control bar and its fact line — which
    /// travel with the pane — go on naming the recording.
    ///
    /// **Taken out of the table before any of them is put back**, which is the
    /// page carrier's reason said again about a different map: a centre *trades*
    /// (§7.1.6k′ B4), so the arriving recording is filed under the key the
    /// departing one was filed under, and an insert interleaved with the removes
    /// would have [`video_seat::VideoSeats::put`] shut down a live engine — the
    /// one thing [`video_seat::VideoSeats::take`] and `put` exist to let this
    /// function avoid.
    ///
    /// No `Result`, unlike its sibling: nothing here can fail. A rehome is one
    /// key changing — the engine is not touched, the decoder does not restart,
    /// the playhead does not go back and the texture keeps its name — so there
    /// is no platform call to report on.
    pub(in crate::runtime) fn carry_the_recordings_of_moved_panes(
        &mut self,
        moves: &[(LeafId, LeafId)],
    ) {
        let hosted: BTreeSet<LeafId> = self
            .window
            .video
            .iter()
            .filter_map(|(surface, _)| match surface {
                PreviewSurface::Seat(leaf) => Some(leaf),
                PreviewSurface::Float(_) | PreviewSurface::Peek => None,
            })
            .collect();
        // The same rule the page table asks, asked of the recording table: a
        // pair whose two halves are equal is not a move and must not be lifted
        // and put back, because the next pair in the transaction may be filing
        // something under that key.
        let travelling = pages_that_move_with_their_panes(&hosted, moves);
        if travelling.is_empty() {
            return;
        }
        let carried: Vec<(PreviewSurface, video_seat::VideoSeat)> = travelling
            .iter()
            .filter_map(|(was, now)| {
                let seat = self.window.video.take(PreviewSurface::Seat(*was))?;
                Some((PreviewSurface::Seat(*now), seat))
            })
            .collect();
        for (now, seat) in carried {
            self.window.video.put(now, seat);
        }
    }

    /// Where the box stands this frame.
    pub(in crate::runtime) fn palette_layout(&mut self) -> Option<palette::PaletteLayout> {
        let state = self.window.palette.as_ref()?;
        let listing = state.listing().clone();
        let scroll = state.scroll();
        let (shown, before, typed) = self.palette_field_look();
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(palette::layout(
            (width as f32, height as f32),
            scale,
            &listing,
            palette::FieldLook {
                shown: &shown,
                before: &before,
                typed,
            },
            scroll,
            &mut measure,
        ))
    }

    /// The rail's trailers and the length of its pinned run, together.
    ///
    /// One call so the seam the rail draws and the rows it hits are measured from
    /// one list — [`seats::pinned_run_len`]'s own argument, one level up.
    pub(in crate::runtime) fn rail_list(&self, now: Instant) -> (Vec<seats::TabTrailer>, usize) {
        let trailers = self.tab_trailers(now);
        let pinned = seats::pinned_run_len(&trailers);
        (trailers, pinned)
    }

    /// The rail's live geometry — [`Runtime::strip_geometry`]'s opposite number.
    ///
    /// `None` exactly when no rail is on screen: a horizontal layout, or a
    /// collapsed one. That is [`seats::rail_geometry`]'s own answer passed
    /// through rather than a second reading of the same question.
    pub(crate) fn rail_geometry_now(&self, now: Instant) -> Option<seats::RailGeometry> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (_, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (trailers, pinned) = self.rail_list(now);
        seats::rail_geometry(
            height as f32,
            scale,
            self.platform_chrome(),
            &trailers,
            pinned,
            self.window.rail_scroll,
            self.sampled_rail(now),
        )
    }

    /// **R2's open trigger, in the smallest form that is usable.**
    ///
    /// The pointer is in the rail's own box, or it is not. That is deliberately
    /// *not* the mock-up's `evalRailZone`, which opens on a reach band wider than
    /// the panel and keeps it open over a wider one still — CSS `:hover` "has no
    /// way to express 'stay open while the pointer is anywhere near', and a zone
    /// with hysteresis does". **The zone, its `RAIL_REACH`/`RAIL_KEEP` widths and
    /// the remembered pointer position that lets a resize re-evaluate it without
    /// a mouse-move are deferred to the sidebar ticket**; what is here is the
    /// half that can be honest on its own.
    ///
    /// It is measured against the width the rail is *aiming* at rather than the
    /// width it currently has, and that is what keeps it from chattering: a test
    /// against the animating width would re-decide the question against a
    /// different boundary on every frame of the answer it just gave.
    pub(in crate::runtime) fn drive_rail_zone(&mut self, position: Option<PhysicalPosition<f64>>) {
        // Asked of the window's posture rather than of the stored preference: in
        // focus mode there is no icon rail to reach for, and a trigger that went
        // on opening one would be aiming a tween at a panel nobody is drawing.
        if !self.rail_posture().draws_icon_rail() {
            return;
        }
        let scale = self.window.renderer.metrics().scale_factor;
        let aiming_open = self.window.rail_open.to > 0.5;
        let target = seats::RailState {
            open: f32::from(u8::from(aiming_open)),
            ..self.window.rail
        };
        let width = f64::from(target.width_logical_px()) * scale;
        // The rail begins at the bar's lower edge, so a pointer up in the
        // caption run is not in the rail however far left it is. **Asked of the
        // window rather than of the constant** (T-MAC-LIGHTS): the header is a
        // fact about this window's chrome, and a trigger measured from anything
        // else would leave the top of the parked rail refusing to open.
        let top = f64::from(seats::window_band_px(scale as f32, self.platform_chrome()));
        // The peek is handed over as **the trigger it hangs from**, not as a
        // yes/no: only a peek hanging off a rail row is the rail's business, and
        // that is a question about identity that no boolean can carry. See
        // [`rail_zone_wants_open`].
        //
        // The popup arrives as a yes/no for the opposite reason: which popup it
        // is has already been decided by [`Self::rail_grew_a_popup`], against a
        // fact — [`popup_owner`] — rather than against a rectangle.
        self.aim_rail_at(rail_zone_wants_open(
            position,
            width,
            top,
            self.window.float.peek().and_then(|win| win.origin),
            self.rail_grew_a_popup(),
        ));
    }

    /// Point both of the rail's clocks at `open`.
    ///
    /// The delay is one-sided, which is [`RAIL_TEXT_FADE_OPEN_DELAY`]'s own doc:
    /// the words wait for the panel on the way out and leave ahead of it on the
    /// way back. Reduced motion is handled where every other tween handles it —
    /// inside [`RevealTween`], which under `Motion::Reduced` simply reports the
    /// target and asks for no frames at all.
    fn aim_rail_at(&mut self, open: bool) {
        let target = f32::from(u8::from(open));
        if self.window.rail_open.to == target {
            return;
        }
        let now = Instant::now();
        let motion = self.app.motion;
        self.window.rail_open.retarget(target, now, motion);
        if open {
            self.window
                .rail_text
                .retarget_after(target, now, motion, RAIL_TEXT_FADE_OPEN_DELAY);
        } else {
            self.window.rail_text.retarget(target, now, motion);
        }
    }

    /// **`.panel-toggle`'s verb (Q178): fold the rail away, or bring it back.**
    ///
    /// R1's real trigger, arriving where the mock-up puts it — `#btn-rail` in the
    /// title bar (2270). The state it writes has been here since Q178 and the
    /// geometry has always answered to it (`RailState::collapsed` is read by
    /// `terminal_inset_logical_px` and by `rail_geometry`); what was missing was
    /// only something a hand could press.
    ///
    /// Through [`Self::set_rail_state`] rather than by assigning the flag,
    /// because collapsing moves the terminal's left edge exactly as changing the
    /// mode does, and every consequence of that — re-solving the panes, the
    /// window's minimum size, the hover under a pointer that did not move — is
    /// already spelled out there once.
    ///
    /// Not persisted, and deliberately: `layout` and `mode` are answers to "where
    /// do I want my tabs", which is a standing preference, while a fold is
    /// "give me the room back for a minute". The session schema carries the first
    /// pair (v5's `tab_layout`/`sidebar_mode`) and has never carried this, so a
    /// window opens with its rail out, which is also what the mock-up's own
    /// `state.railCollapsed` does on reload.
    pub(in crate::runtime) fn toggle_rail_collapsed(&mut self) -> Result<()> {
        self.set_rail_state(seats::RailState {
            collapsed: !self.window.rail.collapsed,
            ..self.window.rail
        })
    }

    /// Put the rail in a posture and pay for it — the one commit path the dev
    /// chord, the settings dialog and the panel toggle take, so none of them can
    /// move the rail without the panes, the window minimum, the hover and the
    /// session file following it.
    pub(crate) fn set_rail_state(&mut self, state: seats::RailState) -> Result<()> {
        let moved = (self.window.rail.layout, self.window.rail.mode) != (state.layout, state.mode);
        // **The panel takes its own popups with it** (§7.1.6e″ ③). A menu the
        // rail grew is part of the rail, so it cannot be left standing over a
        // window whose rail has just folded away or moved house — see
        // [`rail_change_strands_its_popups`] for which changes those are.
        //
        // Ahead of the assignment below, and that is the point of the order:
        // ownership is read off the posture the popup was raised in, not the one
        // that is about to replace it.
        if rail_change_strands_its_popups(self.window.rail, state) {
            self.close_rail_popups();
        }
        // P168 — the fold travels rather than cutting. `.window.rail-collapsed`
        // sets `.rail`'s width to zero and `.rail` carries `width .18s ease`, so
        // the panel is seen to leave instead of blinking out.
        //
        // Aimed only when the flag actually turned over, because
        // `set_rail_state` is also the settings dialog's commit path: choosing a
        // sidebar mode must not restart a transition on an edge that is not
        // moving. Nothing here needs to care about the horizontal layout —
        // `width_logical_px` answers `0.0` for it whatever the fold holds, so a
        // rail that has no business being on screen cannot be eased onto it.
        //
        // Reduced motion is handled where every other tween handles it, inside
        // `RevealTween`: under `Motion::Reduced` the value snaps to its target
        // and asks for no frames, so the fold is instant and the panel-toggle
        // stays as immediate as it was before it could animate.
        if self.window.rail.collapsed != state.collapsed {
            self.window.rail_fold.retarget(
                f32::from(u8::from(!state.collapsed)),
                Instant::now(),
                self.app.motion,
            );
        }
        self.window.rail = state;
        // **The bar this window wears may have just changed, and the platform's
        // own buttons stand in it** (T-MAC-LIGHTS x T-MAC-PILL). Ahead of the
        // solve below, so the first frame drawn against the new band is drawn
        // with the lights already on it.
        self.follow_the_window_band()?;
        eprintln!(
            "BT_RAIL layout={:?} mode={:?} focus={} inset={}px",
            self.window.rail.layout,
            self.window.rail.mode,
            self.window.focus_mode,
            // The inset the stage will actually be solved with, which is the
            // posture's — a trace printed off the preference would report 0 for a
            // window whose panes had just moved 220px.
            self.rail_posture().terminal_inset_logical_px()
        );
        // An icon rail is born parked, whatever the last one was doing, and its
        // words are away with it. A pointer already standing inside it opens it
        // again on the very next move — which is the trigger doing its job, not
        // this deciding for it.
        //
        // **Only when the posture actually moved.** A rail that is merely being
        // folded away by `.panel-toggle` is the same rail in the same mode, and
        // re-parking it would spend the list's scroll position on a gesture that
        // never claimed to move it — fold and unfold, and the row you were
        // looking at is gone. "Born parked" is about a rail that has just become
        // an icon rail; a fold does not make a new one.
        if moved {
            self.window.rail_open = RevealTween::over(RAIL_TRANSITION);
            self.window.rail_text = RevealTween::over(RAIL_TEXT_FADE);
            self.window.rail_scroll = 0.0;
        }
        // The inset changed, so every pane's rectangle did. This is the same
        // path a window resize takes, and it has to be taken here: the rail's
        // *mode* moves the terminal's left edge even though the rail's own
        // opening never does (Q179).
        self.commit_seat_geometry()?;
        self.apply_window_min_inner_size()?;
        if let Some(position) = self.window.pointer_position {
            self.window.seat_pointer.hover = self.chrome_target_at(position);
        }
        // Both halves are layout intent, so they persist exactly as the theme
        // and the cursor shape do — a window that opened with the tabs across
        // the top after the user moved them down the side would be the same
        // broken promise.
        if moved {
            self.mark_session_dirty(Instant::now());
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    pub(in crate::runtime) fn scroll_rail(&mut self, delta: MouseScrollDelta) -> Result<()> {
        let now = Instant::now();
        // **The plain notch is this list's; `Alt` aims the seat under the
        // pointer** ([`column_notch`], user ruling 2026-08-21). The decision is
        // asked here, above everything the list does, because the two readings
        // are alternatives and not a fallback: a bare notch is the column's even
        // when the column has nowhere to go, and a modified one that finds no
        // terminal seat under the pointer is the column's too.
        // **And it is asked of the hand rather than of the keyboard** (§13.33
        // ①). `window.modifiers` has had `Option` taken out of it on a Mac whose
        // reader has left `Option key sends Alt` off, because there it is text;
        // a notch composes no text, so this reads what is actually held.
        let notch = column_notch(self.window.modifiers_held);
        if notch == ColumnNotch::Aim && self.aim_focus_card_window(now, delta)? {
            // The one route word that says the aim answered (`BT_MOUSE_TRACE`,
            // §7.60). `wheel_aim` one line above says what it did with it.
            self.mouse_trace(|| "wheel_route taken=rail-aim".to_owned());
            return Ok(());
        }
        // The two lists share `rail_scroll` because they share the panel; what
        // differs between them is only how long the content is, which is exactly
        // what `viewport` and `max_scroll` carry.
        let Some((viewport, max_scroll)) = self
            .rail_geometry_now(now)
            .map(|geometry| (geometry.viewport, geometry.max_scroll))
            .or_else(|| {
                self.focus_rail_geometry_now(now)
                    .map(|geometry| (geometry.viewport, geometry.max_scroll))
            })
        else {
            self.mouse_trace(|| "wheel_route taken=rail-scroll at=no-geometry".to_owned());
            return Ok(());
        };
        let travel = self.vertical_wheel_travel(delta, viewport[1] - viewport[0]);
        // Wheel-up reveals what lies above, which is a smaller offset.
        let scrolled = (self.window.rail_scroll - travel).clamp(0.0, max_scroll);
        if scrolled == self.window.rail_scroll {
            let held = self.window.rail_scroll;
            self.mouse_trace(|| {
                format!(
                    "wheel_route taken=rail-scroll at=clamped rail_scroll={held} \
                     travel={travel} max_scroll={max_scroll}"
                )
            });
            return Ok(());
        }
        let was = self.window.rail_scroll;
        self.mouse_trace(|| {
            format!(
                "wheel_route taken=rail-scroll at=scrolled rail_scroll={was} to={scrolled} \
                 max_scroll={max_scroll}"
            )
        });
        self.window.rail_scroll = scrolled;
        // The list moved under a stationary pointer, so what it is over changed
        // without the pointer having done anything.
        if let Some(position) = self.window.pointer_position {
            self.window.seat_pointer.hover = self.chrome_target_at(position);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Whether the pointer is over the panel on the left, whichever list it is
    /// holding — the wheel's gate, and `.pane:hover`'s.
    ///
    /// One function over both surfaces because it is one rectangle: the card
    /// column and the rail are the same panel, and a pointer inside it belongs to
    /// it either way. Asking only the rail was right until §7.1.6b′ gave the
    /// panel a second thing to hold.
    pub(in crate::runtime) fn panel_covers(&self, position: PhysicalPosition<f64>) -> bool {
        let now = Instant::now();
        self.rail_geometry_now(now)
            .is_some_and(|rail| rail.covers(position.x, position.y))
            || self
                .focus_rail_geometry_now(now)
                .is_some_and(|column| column.covers(position.x, position.y))
    }

    /// Whether the pointer is over the rail's own box — the wheel's gate.
    pub(in crate::runtime) fn rail_contains(&self, position: PhysicalPosition<f64>) -> bool {
        let now = Instant::now();
        self.rail_geometry_now(now)
            .map(|geometry| seats::rail_run(&geometry))
            .or_else(|| {
                self.focus_rail_geometry_now(now)
                    .map(|geometry| seats::focus_rail_run(&geometry))
            })
            .is_some_and(|run| run.contains(position.x, position.y))
    }

    /// **The rail's own gate, with the column as it is *painted* beside the
    /// column the aim *walks*** (`BT_MOUSE_TRACE`, §7.60).
    ///
    /// Two heights reach [`seats::focus_rail_geometry`] in this program and
    /// nothing had ever compared them: the chrome is built against the bottom of
    /// the lowest pane the solver placed ([`seats::chrome_surface_height`]) and
    /// [`Self::focus_rail_geometry_now`] — which is what `rail_contains` and
    /// [`Self::aim_focus_card_window`] both read — is solved against the
    /// swapchain's. While they agree, a card is where it is drawn; if they ever
    /// part, every card in the column is offset from its own picture and a
    /// gesture aimed at what the reader can see misses. So both are solved here
    /// and printed on one line, with `agree` as the answer.
    ///
    /// **Inside the closure**, so the second solve happens only for a reader who
    /// asked for the file: a diagnostic nobody turned on must cost one atomic
    /// load and nothing else.
    pub(in crate::runtime) fn wheel_rail_trace(
        &self,
        now: Instant,
        position: PhysicalPosition<f64>,
        contains: bool,
    ) {
        self.mouse_trace(|| {
            let aim_height = self
                .window
                .renderer
                .presentation_geometry()
                .swapchain_size
                .1 as f32;
            let paint_height = seats::chrome_surface_height(&self.seat_layout);
            // **The same solver twice, with one number changed.** Anything else
            // would be two columns differing in more than the quantity under
            // investigation, and `agree` would stop meaning what it says.
            let aim = self.focus_rail_geometry_at(now, aim_height);
            let paint = self.focus_rail_geometry_at(now, paint_height);
            mouse_trace::WheelRail {
                contains,
                point: (position.x, position.y),
                rail_scroll: self.window.rail_scroll,
                strip_rail: self.rail_geometry_now(now).map(|geometry| geometry.body),
                aim_height,
                aim: aim.as_ref(),
                paint_height,
                paint: paint.as_ref(),
            }
            .line()
        });
    }

    /// The *program* is about to change the rectangle, so the layout it produces is held to the
    /// minima in full.
    ///
    /// Claims the next rectangle sight unseen rather than recording one now: at this point the
    /// request has been made and the OS has not yet answered it, and predicting the answer is
    /// exactly the guess this mechanism exists to avoid.
    pub(in crate::runtime) fn claim_lawful_layout(&mut self) {
        self.window.size_policy = SizePolicy::Lawful;
        self.window.lawful_client_size = None;
    }

    /// **How far the panel's list is scrolled, said again in the new display's
    /// pixels** (user report 2026-09-12, §7.1.6b′).
    ///
    /// [`WindowRuntime::rail_scroll`] and [`WindowRuntime::tab_scroll`] are the
    /// only numbers the tab panel keeps between frames that are *physical
    /// pixels*. Everything else it stands on — a card's box, its head, its mini
    /// seats, the sticky `+`, the clip box, the hit test — is solved from
    /// `metrics().scale_factor` on the frame it is drawn, so a window carried to
    /// a display of another scale re-derives all of it and needs no help. These
    /// two do not: they were measured against the display the list was last
    /// scrolled on, and left alone they say a different distance there.
    ///
    /// The distance is the fact, and the pixels are how it was written down, so
    /// the restatement is the ratio between the two scales — the same similarity
    /// transform §7.50 already applies to every rectangle on this path ("the
    /// same tree solved again on a new rectangle"). A column standing at its end
    /// on a 200% display stands at its end on a 150% one; a column halfway down
    /// stays halfway down.
    ///
    /// **Both offsets, because the panel has two lists and one of them is not
    /// the cards.** The vertical rail and the card column share `rail_scroll`
    /// and the horizontal strip has `tab_scroll`; all three are read by geometry
    /// that multiplies by the current scale, so all three are stale in exactly
    /// the same way. Restating one and not the others is the next report.
    pub(in crate::runtime) fn restate_panel_scroll(&mut self, measured_at: f64, now_at: f64) {
        self.window.rail_scroll = restated_scroll(self.window.rail_scroll, measured_at, now_at);
        self.window.tab_scroll = restated_scroll(self.window.tab_scroll, measured_at, now_at);
    }

    /// Every terminal pane's rectangle pair on this frame's own clock, and the
    /// preview seat's picture re-placed against the same one.
    ///
    /// Lifted out of [`Self::redraw`] so the chrome-only present can ask the
    /// identical question: a ring turning while a pane is mid-flight has to be
    /// drawn through the transform this instant, and a second copy of this
    /// arithmetic would be a second answer.
    pub(in crate::runtime) fn pane_draws(&mut self, now: Instant) -> Vec<PaneDraw> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let motion = self.app.motion;
        let bodies: Vec<PaneDraw> = self
            .seats
            .terminals()
            .into_iter()
            .filter_map(|seat| {
                let body = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)?;
                let device = self.seat_layout.get(seat)?.device_rect?;
                let pane = [
                    device.left as f32,
                    device.top as f32,
                    device.right as f32,
                    device.bottom as f32,
                ];
                let (viewport, clip) = animated_pane_viewports(
                    body,
                    pane,
                    self.window.pane_motion.transform_of(seat, now, motion),
                );
                Some(PaneDraw {
                    seat,
                    viewport,
                    clip,
                })
            })
            .collect();
        // **U8 — every picture's pane, re-placed on every animated frame.**
        //
        // Their panes FLIP with the rest, and the pair of rectangles each is
        // drawn through is a function of the clock; `refresh_preview_for_layout`
        // runs once per commit and cannot answer for the frames in between. Only
        // the placement is touched here — the raster, its key and the extent it
        // was fitted to belong to the commit, and re-deciding those per frame is
        // the resample storm R2 exists to forbid.
        //
        // **Each against its own pane, and all of them** (§7.1.6k⁵, §7.1.6k⁷).
        // This asked `seats.preview()` — the first preview leaf in the tree —
        // until a tab could hold two, and then it asked which single pane held
        // the one texture lane. Both were the same defect a slice apart: a
        // picture fitted against one pane and placed against another, or not
        // placed at all. The panes of this tab that are holding pictures are the
        // pictures the renderer is holding, so the walk is over them.
        for surface in self.seat_pictures() {
            let PreviewSurface::Seat(leaf) = surface else {
                continue;
            };
            if let Some(placement) = preview_image_placement(
                &self.seats,
                &self.seat_layout,
                leaf.seat,
                scale,
                self.window.pane_motion.transform_of(leaf.seat, now, motion),
            ) {
                self.window.renderer.place_preview_image(
                    picture_channel_owner(surface),
                    placement.seat,
                    placement.clip,
                );
            }
        }
        // And the hosted page, on the same clock and for the same reason: a web
        // seat is a pane, it FLIPs with its neighbours, and a rectangle that is
        // a function of the clock cannot be answered once per commit.
        self.sync_web_page(now);
        bodies
    }

    /// **The page the landing rule would reuse, if that pane holds one** (user
    /// ruling 2026-09-06: 一扇窗允许任意多个网页 pane).
    ///
    /// The singleton rule this replaces (`plan.md` §0 ②「每 tab 单例」) said the
    /// reuse target was *whichever* pane of this tab held a page, first in tree
    /// order. That is what made a second page impossible: a reader who split a
    /// second preview pane and then clicked a path got the **first** pane
    /// navigated and the new pane left standing on its empty placeholder — the
    /// screenshot in the report, and the sentence under it ("一个窗口只能有一个
    /// 网页预览 pane 吗").
    ///
    /// So a page lands where anything else a reader opens lands: on
    /// [`seats::Seats::landing_preview`], the first preview pane that is not
    /// locked. Every consequence the reader already knows follows from a rule
    /// they already know — lock a page and the next one opens beside it, leave it
    /// unlocked and the next one replaces it — and a tab may hold as many pages
    /// as it holds preview panes, each with its own controller on its own visual.
    ///
    /// `None` when the landing pane holds no page, which is what makes the two
    /// arms of the verbs below fork: navigate the page that is there, or make
    /// one.
    pub(crate) fn page_on_the_landing_pane(&self) -> Option<SeatId> {
        self.seats
            .landing_preview()
            .filter(|seat| self.seat_holds_a_page(*seat))
    }

    /// **Which seat `Ctrl+F` opens the capsule on** (§7.7 ②, W2 slice ④).
    ///
    /// `focused_leaf` names the shell the keyboard falls back to and is only
    /// ever written for a seat that is in `sessions` — so on a window whose
    /// keyboard is in a page it still names the terminal beside it, and a
    /// capsule opened through it lands on the wrong pane (found on the machine,
    /// 2026-08-22: `Ctrl+F` over a page opened a search on the shell next door).
    /// The capsule has two hosts now, so the question has two answers and this
    /// is where they are chosen between.
    pub(in crate::runtime) fn search_host_seat(&self) -> SeatId {
        self.focused_web_seat().unwrap_or(self.focused_leaf)
    }

    /// **The one door a frame reaches the screen through.**
    ///
    /// # wgpu presents; the compositor publishes
    ///
    /// Since slice A2 this window's swapchain is the content of a
    /// DirectComposition visual rather than the window's own surface, and wgpu's
    /// dx12 backend calls `SetContent` on that visual without ever calling
    /// `Commit` (`wgpu-hal-30.0.0/src/dx12/mod.rs:1619` — the `Visual` arm,
    /// beside the `VisualFromWndHandle` arm where wgpu owns the composition
    /// device and does commit). Whoever owns the DirectComposition device owns
    /// the commit, and here that is [`WindowRuntime::compositor`]. A present without
    /// its commit is not a dropped frame — it is a screen that never changes
    /// again while every trace in the program reports frames presenting
    /// normally.
    ///
    /// So there is one funnel and it is grep-pinnable: **this is the only call
    /// to `WindowRenderer::present_frame` in `bt-app`**, and the commit is the
    /// statement after it. Two callers reach it — [`Self::redraw`] with a newly
    /// composed frame, and [`Self::present_retained_picture`] with the picture
    /// already on the glass — and a resize present is `redraw` again, not a
    /// third path.
    ///
    /// An associated function rather than a method because both callers hold a
    /// borrow of some other part of `self` across the call: the retained-picture
    /// path is presenting `self.last_presented_frame` itself.
    ///
    /// The commit is skipped when nothing was presented. `Skipped` and
    /// `Reconfigure` both mean the swapchain handed back no image, so there is
    /// nothing new for the compositor to publish and the frame is re-filed by
    /// the caller.
    pub(in crate::runtime) fn present_seats_and_commit(
        gpu: &mut GpuContext,
        renderer: &mut WindowRenderer,
        compositor: &bt_platform::Compositor,
        window: &Window,
        traces: FrameTraces<'_>,
        seat_frames: &[bt_render::SeatFrame<'_>],
        intent: PresentIntent,
    ) -> Result<Option<PresentOutcome>> {
        hang_watch::during(hang_watch::Station::PresentSeats, || {
            let PresentIntent {
                trigger,
                mut signature,
            } = intent;
            let FrameTraces {
                attempt,
                preview,
                census,
                gate,
                trace_perf,
                slot_overwrites,
                conditions,
            } = traces;
            if gpu.device_loss().is_none() && gate.unchanged(&signature, conditions) {
                trace_unchanged_present(
                    trace_perf,
                    gate,
                    trigger.source,
                    seat_frames.first().map(|seat| seat.frame),
                    slot_overwrites,
                );
                attempt.outcome = present_diagnostics::Outcome::Unchanged;
                return Ok(None);
            }
            // Failure, textless presentation, or even a failed commit must not
            // leave an old signature claiming that the surface is still complete.
            gate.invalidate();
            // **What the frame is allowed to cost to measure**, decided at the one
            // funnel because that is the one place every frame passes. Counting a
            // frame's demand on the glyph atlas means rasterizing each distinct
            // glyph a second time to learn its size, so it is off unless
            // `BT_GLYPH_CENSUS` names a file to write it to; setting the flag is a
            // bool store and asking the gate is a `OnceLock` read.
            renderer.set_glyph_census(glyph_trace::wanted());
            let present_parent = hang_watch::enter(hang_watch::Station::RenderCompose);
            attempt.outcome = present_diagnostics::Outcome::FailedRender;
            let outcome =
                renderer.present_frame_with_phases(gpu, seat_frames, trigger, |phase| {
                    attempt.render_phase(phase);
                    hang_watch::phase(match phase {
                        PresentPhase::SurfaceConfigure(_) => hang_watch::Station::SurfaceConfigure,
                        PresentPhase::ComposeEncode => hang_watch::Station::RenderCompose,
                        PresentPhase::TextShaping => hang_watch::Station::TextShaping,
                        PresentPhase::AtlasUpload => hang_watch::Station::AtlasUpload,
                        PresentPhase::Layout => hang_watch::Station::RenderLayout,
                        PresentPhase::SurfaceAcquire => hang_watch::Station::SurfaceAcquire,
                        PresentPhase::QueueSubmit => hang_watch::Station::QueueSubmit,
                        PresentPhase::Present => hang_watch::Station::SwapchainPresent,
                        PresentPhase::Complete => hang_watch::Station::RenderCompose,
                    });
                })?;
            // **What this frame did with the documents on it** — the one funnel is
            // also the one place that can say it, and it says it only when the
            // answer moved (`BT_PREVIEW_TRACE`).
            hang_watch::at(present_parent);
            preview_trace::frame(
                preview_trace::global(),
                preview,
                renderer.preview_text_frame(),
            );
            // **And what it asked the shared glyph atlas for** (`BT_GLYPH_CENSUS`,
            // `docs/DESIGN.md` §7.1.3m) — the ceiling §7.1.3l could only measure by
            // hand, measurable on the machine that meets it.
            glyph_trace::frame(glyph_trace::global(), census, renderer.glyph_census());
            // A textless present is still a present: an image went to the swapchain,
            // so the composition tree has to publish it or the glass keeps showing a
            // picture the swapchain no longer holds. What that frame *owes* is a
            // different question, and it is answered by the callers below.
            if matches!(
                outcome,
                PresentOutcome::Presented(_) | PresentOutcome::PresentedWithoutText(_)
            ) {
                // **The skirt shrinks in the same commit the picture grows in.**
                // The swapchain has just been reconfigured and presented, so this
                // is the first instant its new rectangle is true; publishing the
                // two apart would either leave the strip transparent for one more
                // frame or lay the window's ground under a clear that already
                // carries it, and a 60% window would read at 84% along the strip
                // (§7.1.6c-4b). The present funnel is the only place both facts
                // are in hand at once, which is why it is here and not beside the
                // resize handler.
                attempt.outcome = present_diagnostics::Outcome::FailedCommit;
                attempt.phase(Some(5));
                let (covered_width, covered_height) = renderer.presented_swapchain_size();
                let committed = (|| {
                    hang_watch::during(hang_watch::Station::CompositorSize, || {
                        compositor
                            .set_covered_size(covered_width, covered_height)
                            .map_err(|error| anyhow!(error))
                            .context("tell the window's ground how much of it the swapchain covers")
                    })?;
                    hang_watch::during(hang_watch::Station::CompositorCommit, || {
                        compositor
                            .commit()
                            .map_err(|error| anyhow!(error))
                            .context("publish the presented frame to the window's composition tree")
                    })
                })();
                attempt.phase(None);
                committed?;
                attempt.landed_at = Some(Instant::now());
                // **The frame that withdraws the skirt has to be asked for.** The
                // ground under the strip is sized against the picture of the
                // *previous* present, because a present queues an image rather than
                // showing one, so it can only ever be withdrawn by a later frame —
                // and a window that fell idle the instant a resize ended would have
                // no later frame. Self-terminating: the frame this asks for is the
                // one that makes the answer `false`.
                if compositor.skirt_covers_anything() {
                    window.request_redraw();
                }
            }
            if matches!(outcome, PresentOutcome::Presented(_)) {
                // A successful resize may configure the surface inside this call.
                signature.renderer = renderer.present_state();
                gate.presented(signature);
            }
            attempt.outcome = match &outcome {
                PresentOutcome::Presented(_) => present_diagnostics::Outcome::Presented,
                PresentOutcome::PresentedWithoutText(_) => {
                    present_diagnostics::Outcome::WithoutText
                }
                PresentOutcome::Skipped => present_diagnostics::Outcome::Skipped,
                PresentOutcome::SkippedNotVisible => present_diagnostics::Outcome::NotVisible,
                PresentOutcome::Reconfigure => present_diagnostics::Outcome::Reconfigure,
            };
            Ok(Some(outcome))
        })
    }

    pub(in crate::runtime) fn retained_seats<'a>(
        tab: &'a TabState,
        focused_frame: Option<&'a ViewportFrame>,
        bodies: &[PaneDraw],
        focused_leaf: SeatId,
        viewport: bt_render::SeatViewport,
        focused: bool,
    ) -> (Vec<SeatId>, Vec<bt_render::SeatFrame<'a>>) {
        let mut seat_ids = Vec::with_capacity(bodies.len());
        let mut seat_frames = Vec::with_capacity(bodies.len());
        // **The focused seat frame is pushed only when there is a shell to push
        // one for** (§7.1.6h). On a folder tab the retained window picture is
        // the *previous* tab's, and the fallback rectangle below is the whole
        // viewport — so pushing it would paint another tab's terminal across
        // this one and leave the chrome to cover its own tracks. The list is
        // then simply empty, and `present_frame` composes zero seats and the
        // chrome over them, which is exactly what such a tab is made of.
        if let Some(frame) = focused_frame {
            let focused_body = bodies
                .iter()
                .find(|pane| pane.seat == focused_leaf)
                .copied()
                .unwrap_or(PaneDraw {
                    seat: focused_leaf,
                    viewport,
                    clip: viewport,
                });
            seat_ids.push(focused_leaf);
            seat_frames.push(bt_render::SeatFrame {
                seat: focused_body.viewport,
                clip: focused_body.clip,
                frame,
                focused,
            });
        }
        // Each unfocused pane's own last picture, for the same reason the
        // focused one's is reused: a pane that has not been given anything new
        // to say is showing what it last said, and re-projecting it would be
        // asking it to say the same thing at the price of a whole capture.
        // A pane that has never presented is simply not drawn this present —
        // it has nothing on the glass to keep.
        for pane in bodies {
            if pane.seat == focused_leaf {
                continue;
            }
            let Some(leaf) = tab.sessions.get(&pane.seat) else {
                continue;
            };
            let Some(projected) = leaf.last_presented_frame.as_ref() else {
                continue;
            };
            seat_ids.push(pane.seat);
            seat_frames.push(bt_render::SeatFrame {
                seat: pane.viewport,
                clip: pane.clip,
                frame: projected,
                focused: false,
            });
        }
        (seat_ids, seat_frames)
    }
}
