//! `mouse` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    ApplicationChange, DividerDrag, DividerGrip, Drag, DragCarry, DragLatch, DragRelease,
    DragSource, DropBatch, DropLanding, Fading, FloatDrag, FloatDragKind, FloatHeadPress,
    HoverFloat, LeafId, MathHoverExit, MouseRoute, OwnPress, PanePress, PointerTarget, Popup,
    PressAfterBlur, PressedCellTarget, PreviewSurface, RenameExit, RenameSubject, RowActivation,
    RowHost, RowPayload, RowPayloadKind, RowPress, Runtime, SpringGate, TabClick,
    TerminalReference, UserInputKind, WebHeadVerb, WheelAxis, WheelBurst, WheelRoute,
    answered_once, button_router_position, crumb_segments, drain_whole_units, files,
    files_row_activation, first_run, float, float_grasp, float_sizing_of, formula_tools,
    glass_allows_a_drop, hang_watch, image_zoom_notch, input, landing_for_aim,
    live_viewport_mouse_hit, marks, mouse_trace, native_window, over_home_ground, palette,
    platform_pointer_of, pointer_cursor, press_after_blur, press_files_node, press_reaches_no_grid,
    press_spends_itself_closing, pressed_row_identity, profiles, protocol_mouse_button, quit,
    recoverable_wheel_scroll_amount, release_verdict, restore, right_press_raises_terminal_menu,
    risen_frame, route_forwarded_mouse_button, route_forwarded_mouse_motion, search, seats,
    settings, settling, toast, tooltip, upright_wheel, web_page_cursor, websheet, wheel_axis,
    wheel_points_sideways, wheel_route, wheel_zoom_notches, write_pty_input,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::SeatId;
use bt_render::{CursorStyle, FrameSource, FrameTrigger, MathHitTarget, set_cursor_style};
use std::path::PathBuf;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

impl Runtime<'_> {
    /// The ghost's own layer, or nothing when nothing is in the hand (J114-J116).
    ///
    /// Nothing is built for a drag whose landing is already showing itself
    /// ([`DropLanding::shows_itself`]) — not built and then hidden, because an
    /// invisible layer still costs a text shaping pass and a raster lookup every
    /// frame the pointer moves, and "not drawn" is the same picture either way.
    pub(in crate::runtime) fn drag_ghost_layer(&mut self) -> Vec<marks::OverlayLayer> {
        // **F2 — the visitor's ghost, laid out at *this* window's scale.**
        //
        // The four facts it says arrived on the broker ([`GhostFace`]); where
        // they sit is this window's own arithmetic, which is the whole of
        // 「混合 DPI 下幽灵尺寸随所在窗」 — one label, measured against the font
        // size the window it is over draws chrome at, so a tab carried from a
        // 100% display onto a 200% one grows as it crosses rather than staying
        // the size of the window it left. It yields to the stand-in on exactly
        // the same terms a local ghost does ([`DropLanding::shows_itself`]).
        let visiting = self.window.foreign.as_ref().and_then(|visit| {
            visit
                .landing
                .is_none_or(|landing| !landing.shows_itself())
                .then(|| {
                    (
                        visit.pointer,
                        visit.face.mark,
                        visit.face.mark_logical,
                        visit.face.colour,
                        visit.face.text.clone(),
                    )
                })
        });
        let (pointer, mark, mark_logical, mark_color, text) = match visiting {
            Some(visiting) => visiting,
            None => {
                let Some(drag) = self
                    .window
                    .drag
                    .as_ref()
                    .filter(|drag| drag.ghost_is_shown())
                else {
                    self.forget_the_ghost();
                    return Vec::new();
                };
                let (pointer, source) = (drag.pointer, drag.source.clone());
                let palette = bt_render::chrome_palette();
                let Some((mark, mark_logical, mark_color, text)) =
                    self.drag_label(&source, palette)
                else {
                    self.forget_the_ghost();
                    return Vec::new();
                };
                (pointer, mark, mark_logical, mark_color, text)
            }
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // Only the font knows how wide a line is, so the measuring happens here,
        // beside the renderer, exactly as the tip's and the badge's do.
        let width = self.window.renderer.measure_chrome_text(
            &mut self.app.gpu,
            &text,
            bt_render::DRAG_GHOST_FONT_LOGICAL_PX * scale,
        );
        // **And whether the hand has left every glass of ours** (丙2). The
        // visitor's ghost is never this: a window drawing somebody else's
        // payload is by definition a window the hand is over.
        let tearing_out = (self.window.foreign.is_none() && self.window.tearing_out).then(|| {
            let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
            seats::TearOutGhost {
                surface: [width as f32, height as f32],
            }
        });
        let layout = seats::drag_ghost_layout(
            [pointer.x as f32, pointer.y as f32],
            mark_logical,
            width,
            scale,
            tearing_out,
        );
        // **The ghost fades in, and it does not fade out** (the animation slice's
        // second half, `bt_render::DRAG_GHOST_FADE`).
        //
        // In, because the frame a press becomes a *carry* is a real event with
        // nothing else to announce it: the pointer crosses the threshold and a
        // card appears under it out of nothing. Ninety milliseconds makes that a
        // moment instead of a flicker.
        //
        // Out, never: the ghost's whole job ends the instant the thing is let go
        // of, and it is handed to the landing that is already animating in its
        // place ([`DropLanding::shows_itself`], and the same `Vec::new()` above
        // for a drag that ended). A ghost lingering over the slot the thing has
        // already dropped into would be the picture disagreeing with the fact —
        // `pane 关闭不为动画持尸` one surface along. So the way down is
        // [`settling::Settling::forget`] and not a fade.
        //
        // And the *position* is untouched by any of this. It is the pointer's, on
        // every frame, with no easing whatever — the plan's own red line.
        let ink = self.window.settling.settle(
            &Fading::DragGhost,
            0.0,
            settling::Toward::eased(1.0, bt_render::DRAG_GHOST_FADE),
            Instant::now(),
            self.app.motion,
        );
        let mut ghost = seats::build_drag_ghost(
            &layout,
            mark,
            mark_color,
            &text,
            scale,
            bt_render::chrome_palette(),
        );
        ghost.opacity *= ink;
        // Off the glass the ghost is held at this window's edge rather than at
        // the hand, so it is drawn back a little to stop it asserting a position
        // it has not got — [`seats::TEAR_OUT_GHOST_OPACITY`].
        if tearing_out.is_some() {
            ghost.opacity *= seats::TEAR_OUT_GHOST_OPACITY;
        }
        vec![ghost]
    }

    /// What the ghost says — `dragLabel(d)`, mock-up 6734-6751.
    ///
    /// "the mark, then the title", and the title is the **short** one: a pane's
    /// head answers "where is this" with the whole path and a label riding the
    /// pointer answers "which one is this" with the last segment alone (C28, and
    /// `seat_short_caption`'s own note). A tab already has exactly one name and
    /// takes it unchanged — `focusedLeaf(tabById(d.wsId))` in the mock-up is how
    /// a tab finds a name at all, and here the tab has been carrying its own
    /// since T1.
    pub(in crate::runtime) fn drag_label(
        &self,
        source: &DragSource,
        palette: bt_render::ChromePalette,
    ) -> Option<(marks::ChromeMark, f32, [u8; 3], String)> {
        match source {
            DragSource::Tab(id) => {
                let tab = self
                    .window
                    .tabs
                    .iter()
                    .find(|candidate| candidate.id == *id)?;
                Some((
                    // The tab's own mark, by the same `identityLeaf` rule the
                    // strip draws it with — a ghost is a picture of the tab it
                    // was lifted out of, so the two must not disagree about what
                    // that tab is.
                    tab.tab_mark(&{
                        let favicons = self.app.favicons.borrow();
                        self.window
                            .web
                            .iter()
                            .map(|(leaf, web)| (*leaf, favicons.of_url(&web.page().url)))
                            .collect()
                    }),
                    bt_render::WINDOW_TAB_MARK_LOGICAL_PX,
                    palette.accent,
                    tab.display_title(),
                ))
            }
            DragSource::Pane(leaf) => {
                // Through the pane's own tab (§7.1.6k) — after a spring the view
                // has moved and the pane has not, and a ghost that went blank at
                // that moment would be the hand emptying halfway through the
                // gesture.
                let tab = self.tab_state(leaf.tab)?;
                let seat = leaf.seat;
                let kind = tab.seats.tree().find_seat(seat)?.kind;
                let (mark, size, colour) = seats::pane_mark(
                    kind,
                    tab.sessions
                        .get(&seat)
                        .map(|leaf| profiles::mark(profiles::index_of_id(&leaf.profile))),
                    palette,
                );
                // The dragged seat's own name, by id — the ghost and the
                // stand-in it hands off to are two pictures of one pane, so they
                // read the same door ([`Runtime::strip_stand_in`]).
                let name = tab.terminal_name(seat);
                let files_name = tab.files_head_name(seat);
                let title = tab.preview_head_name(seat);
                Some((
                    mark,
                    size,
                    colour,
                    seats::seat_short_caption(
                        kind,
                        title.as_deref(),
                        name.as_deref(),
                        files_name.as_deref(),
                    )
                    .to_owned(),
                ))
            }
            // **P87 — a row travels as its identity, and the target decides the
            // verb.** So the ghost says what the thing *is* and never what will
            // become of it: the same label rides the pointer over a preview it
            // will fill, over an edge it will split and over a terminal that
            // will refuse it. `dragLabel`'s own comment (6811-6828) is that
            // sentence, and it is why the mark here is the leaf mark this
            // payload would land as rather than a fourth glyph invented for
            // dragging — a page for a file, a folder for a folder, exactly what
            // the pane it becomes will wear in its head.
            DragSource::Row(payload) => {
                let (mark, size, colour) =
                    seats::pane_mark(payload.kind.leaf_kind(), None, palette);
                Some((mark, size, colour, payload.name.clone()))
            }
        }
    }

    /// **Which hover panel holds the glass**, if one does — [`HoverFloat`]'s
    /// declaration order, highest first.
    ///
    /// Read off the live state rather than remembered: "is a menu up" and "is
    /// the flyout still inside its closing grace" are facts those hosts already
    /// own, and a second copy of them is a copy that goes stale on whichever
    /// path forgets to write it.
    pub(in crate::runtime) fn hover_float_up(&self) -> Option<HoverFloat> {
        HoverFloat::holding(|who| self.hover_float_is_up(who))
    }

    /// Whether one named panel is on the glass. The window's half of
    /// [`HoverFloat::holding`] — this is where the facts live, and the rule
    /// itself lives beside the enum where it can be tested.
    pub(crate) fn hover_float_is_up(&self, who: HoverFloat) -> bool {
        match who {
            HoverFloat::Menu => self.popups_up().any().is_some(),
            // The **peek** only. A pinned float is a place (§7.1.2) and is on
            // nobody's exclusion list.
            HoverFloat::Flyout => self.window.float.peek_id().is_some(),
            // A card with a `due` is an intent, not a panel: nothing is on the
            // glass for those 350ms, so nothing is being covered.
            HoverFloat::Glance => self
                .window
                .file_peek
                .as_ref()
                .is_some_and(|peek| peek.clock.is_shown()),
            HoverFloat::LayoutPeek => self.window.layout_peek.active().is_some(),
        }
    }

    /// Whether `who`'s clock may run right now: the glass is free, or it is
    /// already `who`'s.
    pub(in crate::runtime) fn hover_float_free(&self, who: HoverFloat) -> bool {
        who.free(|other| self.hover_float_is_up(other))
    }

    /// Take down every hover panel but `keep`, and report whether anything went.
    ///
    /// The intents go with the panels. A flyout dismissed while its own
    /// `settling` was still maturing would re-open itself 180ms later under a
    /// pointer that had not moved — G87's bug, and it is exactly as true when
    /// the dismissal comes from this list as when it comes from Esc.
    pub(in crate::runtime) fn close_hover_floats_except(&mut self, keep: HoverFloat) -> bool {
        let mut went = false;
        for who in keep.others() {
            match who {
                // Menus are [`Popup`]'s list and are closed through it. Nothing
                // a hover raises may take one down — see `close_popups_except`,
                // which is the only caller that passes `Menu` and the only door
                // this arm would ever be reached through.
                HoverFloat::Menu => {}
                HoverFloat::Flyout => {
                    self.window.float.disarm();
                    // **A press *inside* the window is not a press against it.**
                    // §7.1.2 and [`Self::open_file_menu`]'s own note: a float is
                    // a place rather than a popup, and the menus raised on one
                    // are very often about a row inside it — dismissing it here
                    // would answer the question by deleting its subject. So the
                    // peek goes on a menu raised somewhere *else*, which is the
                    // ordinary "a press away puts a transient surface away", and
                    // stays under a menu raised on itself.
                    //
                    // A keyboard-raised menu is covered by the same reading and
                    // not by an exception: a peek never takes the keyboard
                    // (§7.1.2), so a menu a key raised is never about one.
                    if let Some(id) = self.window.float.peek_id() {
                        let on_it = self.window.pointer_position.is_some_and(
                            |at| matches!(self.float_hit_at(at), Some((hit, _)) if hit == id),
                        );
                        if !on_it {
                            self.window.float.dismiss(id, Instant::now());
                            went = true;
                        }
                    }
                }
                HoverFloat::Glance => went |= self.hide_file_peek(),
                HoverFloat::LayoutPeek => went |= self.window.layout_peek.hide(),
            }
        }
        went
    }

    /// **Ask every hover intent again, with the hand where it already is.**
    ///
    /// The exclusion above is a rule about *arming*, and a rule about arming has
    /// a debt: the arming happens on `pointermove`, and the thing that was
    /// blocking it goes away on a *clock*. A flyout's 420ms grace runs out under
    /// a pointer resting on the `⌄` beside it, and without this the menu refused
    /// at 0ms would never be offered again — the hand would have to twitch to
    /// buy a second opinion. `UI-UX.md` §十 1b names that failure and its fix in
    /// one line: **判定要在「答案可能变了」的时候做，不是只在「指针动了」的时候
    /// 做**, using the pointer's remembered position.
    ///
    /// Called once, from the tail of the tick, and only on the pass where the
    /// glass actually came free — so a window with nothing hovering pays
    /// nothing.
    pub(in crate::runtime) fn rearm_hover_intents(&mut self, now: Instant) -> Result<()> {
        let Some(position) = self.window.pointer_position else {
            return Ok(());
        };
        self.observe_chevrons(Some(position), now);
        let trigger = self
            .hover_float_free(HoverFloat::Flyout)
            .then(|| self.float_trigger_at(position))
            .flatten();
        self.window.float.observe(trigger, now);
        let row = self.glancing_row_at(position);
        if self.observe_file_peek(row, now) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        let tab = self
            .hover_float_free(HoverFloat::LayoutPeek)
            .then(|| self.layout_peek_target_at(position))
            .flatten();
        self.note_layout_peek(tab)
    }

    /// **Put a caret shape in force for the process** (multiwindow slice E1).
    ///
    /// `CURSOR_STYLE` is one `AtomicU64` for the whole process — there is one
    /// caret shape the way there is one theme, and for the same reason (§2.8).
    /// So this is the third verb whose door is a settings row in one window and
    /// whose effect is every window's, and it goes down the channel the other
    /// two go down rather than publishing a frame for the window the row
    /// happened to be pressed in and leaving its siblings drawing the old
    /// shape. The file is marked dirty here and only here: the caret shape is a
    /// top-level key of `session.json` (§2.7), so it is written once no matter
    /// how many windows redraw for it.
    pub(crate) fn apply_cursor_style(&mut self, style: CursorStyle) -> Result<bool> {
        if !set_cursor_style(style) {
            return Ok(false);
        }
        self.mark_session_dirty(Instant::now());
        self.note_application_change(ApplicationChange {
            font: false,
            look: false,
            caret: true,
            option: false,
            paid_by: Some(self.window.window.id()),
        });
        self.adopt_new_cursor_style()?;
        Ok(true)
    }

    /// **What one window owes a caret change** — a frame, and nothing else.
    ///
    /// Its own verb for [`Self::adopt_terminal_font`]'s reason: it is owed
    /// twice, once by the window whose settings row was pressed and once by
    /// every other window, and one body is what keeps those two from becoming
    /// two answers. The body is a whole-window publish rather than the chrome
    /// present [`Self::publish_chrome_frame`] would take, because the caret is
    /// drawn into the terminal picture: the shape is read at draw time out of
    /// the process static (`bt_render::cursor_pixel_bounds`), so the picture on
    /// the glass is the one thing that is now wrong.
    pub(in crate::runtime) fn adopt_new_cursor_style(&mut self) -> Result<()> {
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    /// Light the foot mark the pointer is on, and put the last one out.
    ///
    /// **No wake.** The vertical bar's hover bumps its clock, because a pointer
    /// in its lane is a standing reason for it to be up; a pointer on the last
    /// row is not, and bumping the clock here would keep an already-fading mark
    /// alive under a hand that had come for the prompt.
    fn note_terminal_column_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<()> {
        let over = position
            .and_then(|position| self.terminal_column_bar_under(position))
            .map(|(seat, _)| seat);
        if over == self.terminal_column_hover {
            return Ok(());
        }
        self.terminal_column_hover = over;
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Arm the six pixels on a tree row, whichever host it is in (P81).
    ///
    /// **It consumes nothing and decides nothing**, which is exactly the
    /// mock-up's arrangement and the reason the threshold is quoted in P81's own
    /// text: the press goes on to select the row, fold the folder and count
    /// towards a double-click, and the latch waits beside all three. A row that
    /// is not a node arms nothing, so a press on "Loading …" cannot become a
    /// drag of a sentence.
    fn arm_row_press(&mut self, host: RowHost, index: usize, position: PhysicalPosition<f64>) {
        self.window.tab_press = None;
        self.window.pane_press = None;
        let Some((_, rows)) = self.host_rows(host) else {
            return;
        };
        let Some(row) = rows.get(index).filter(|row| row.is_node()) else {
            return;
        };
        let key = row.key.clone();
        let Some(rect) = self.row_geometry(host).map(|tree| tree.row_rect(index)) else {
            return;
        };
        self.window.row_press = Some(RowPress {
            host,
            key,
            rect,
            latch: DragLatch::new(position),
        });
    }

    /// The press on a row has travelled six pixels, so the row is in the air
    /// (P81).
    ///
    /// The payload is resolved **here** rather than at the press, and against
    /// the tree as it stands now: a directory that finished loading while the
    /// button was down has moved every row below it, and a payload taken at the
    /// press would name the file that used to be under the pointer.
    fn begin_row_drag(&mut self, press: RowPress, position: PhysicalPosition<f64>) -> Result<()> {
        let Some(payload) = self.row_payload(press.host, &press.key) else {
            return Ok(());
        };
        self.begin_drag(
            DragSource::Row(payload),
            DragCarry::Pane,
            position,
            Some(press.rect),
        )
    }

    /// **The content verbs** (L141/L142) — the drop that moves no pane.
    ///
    /// Two sentences, and each is the verb that surface already has: a file
    /// lands on a preview through the same door the tree's own double-click uses
    /// ([`Self::open_preview_onto`]), and a folder re-roots a column through the
    /// same door the root menu uses ([`Self::reroot_files_column`]). Neither is
    /// re-implemented here, and that is what makes "dropping a file on a preview
    /// is the same as opening it there" true rather than nearly true — the pool
    /// lookup, the view memory and the expansion-set reset all come along
    /// without being remembered a second time.
    pub(in crate::runtime) fn retarget_row_drop(
        &mut self,
        payload: &RowPayload,
        target: SeatId,
    ) -> Result<()> {
        match payload.kind {
            RowPayloadKind::File => {
                self.open_preview_onto(self.preview_here(target), payload.path.clone())
            }
            RowPayloadKind::Folder => {
                let root = payload.path.display().to_string();
                self.reroot_files_column(target, &root)
            }
        }
    }

    /// **[`Self::terminal_reference_at`], answered once per cell per pointer event** (audit 3
    /// C-2).
    ///
    /// One `CursorMoved` puts the same question to three surfaces —
    /// [`Self::folder_reference_trigger`], [`Self::terminal_reference_cell`] and
    /// [`Self::peek_target`] — and each of them resolved the cell from scratch. Three answers
    /// about one cell is not three facts; it is one fact derived three times, which is what rule 3
    /// of `docs/CONVENTIONS.md` §十 is about, and while the derivation reached the disk it was six
    /// blocking syscalls per motion event.
    ///
    /// **Only a pointer event may read it.** The memo is emptied at the top of
    /// [`Self::pointer_moved`] and nothing between that line and these three reads can change what
    /// a reference resolves to. Every other caller — the float's rectangle, the glance's row
    /// geometry, the card's subject — is answering a *frame* and calls the resolution directly,
    /// because a frame may be composed at any time and an answer from the last pointer event would
    /// be a reference at a place the pointer has left.
    pub(crate) fn pointer_reference_at(
        &self,
        seat: SeatId,
        cell: u32,
    ) -> Option<TerminalReference> {
        answered_once(&self.window.pointer_reference, (seat, cell), || {
            self.terminal_reference_at(seat, cell)
        })
    }

    /// **The terminal menu's hover, with the safety triangle in it** — the pane
    /// menu's own four steps, on the second door.
    ///
    /// Its own function rather than the three lines every other popup's hover
    /// takes, for [`Runtime::drive_pane_menu_hover`]'s reason: since §7.1.6i's
    /// floor this menu can have a *child*, so what the highlight does when the
    /// pointer leaves a row is no longer "the new row takes it" but "the new row
    /// takes it unless the hand is on its way to the child".
    pub(in crate::runtime) fn drive_term_menu_hover(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(layout) = self.term_menu_layout() else {
            return Ok(false);
        };
        let hit = profiles::term_menu_hit(&layout, position.x, position.y);
        let submenu = layout.submenu_frame();
        let to = [position.x as f32, position.y as f32];
        let now = Instant::now();
        let heading = profiles::TermMenuHit::Row(profiles::TermMenuEntry::Pane(
            profiles::PaneMenuRow::SplitWith,
        ));
        let Some(menu) = self.window.term_menu.as_mut() else {
            return Ok(false);
        };
        let hovering_child = layout.on_submenu(to[0], to[1]) || hit == Some(heading);
        let was_open = menu.submenu_open;
        let mut held = false;
        if let Some(submenu) = submenu
            && !hovering_child
        {
            let from = menu.pointer_was.unwrap_or(to);
            if profiles::safe_triangle_holds(from, to, submenu) {
                let until = *menu
                    .submenu_hold_until
                    .get_or_insert(now + profiles::SUBMENU_SAFE_HOLD);
                held = now < until;
            }
            if !held {
                menu.submenu_open = false;
                menu.submenu_hold_until = None;
            }
        } else {
            menu.submenu_hold_until = None;
        }
        if !held {
            menu.pointer_was = Some(to);
        }
        let hovered = match hit {
            Some(profiles::TermMenuHit::Row(entry)) => Some(profiles::TermMenuHover::Row(entry)),
            Some(profiles::TermMenuHit::Submenu(index)) => {
                Some(profiles::TermMenuHover::Submenu(index))
            }
            // The padding, a greyed row, and everywhere outside: nothing is lit.
            // A menu whose last-hovered row stayed lit while the pointer sat in
            // its own margin would be a menu Enter could fire from a place that
            // looks idle.
            Some(profiles::TermMenuHit::Surface) | None => None,
        };
        let mut changed = menu.submenu_open != was_open;
        if !held && menu.hover != hovered {
            menu.hover = hovered;
            changed = true;
        }
        // Resting on the heading opens the child, on the same 250ms every `⌄` in
        // the house takes. Armed here and matured in `advance_term_menu`.
        if hit == Some(heading) && !menu.submenu_open {
            menu.submenu_hold_until
                .get_or_insert(now + profiles::CHEVRON_HOVER_OPEN_DELAY);
        }
        let inside = hit.is_some();
        if changed && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(inside)
    }

    /// **§7.1.6k — the spring, matured: the window goes to the tab the pane has
    /// been resting on, and the pane stays in the air.**
    ///
    /// It has to fire under a pointer that is not moving at all, which is the
    /// whole reason it is a clock in this pass rather than a branch in
    /// [`Runtime::drive_drag`]: a hand resting on a tab sends no events, so a
    /// dwell driven only by pointer moves would spring on the next thing to
    /// twitch and never on stillness. The file peek's own 350ms carries the same
    /// note beside it in the wake set for the same reason.
    ///
    /// **Nothing but the view moves.** No tree is touched, nothing is committed,
    /// and the pane is still filed under the tab it was picked up from — which is
    /// why the tab it came from does not close even when that pane was its last
    /// one. What the switch buys is sight of where you are about to put the
    /// thing; the putting is the release.
    pub(in crate::runtime) fn advance_drag_spring(&mut self, now: Instant) -> Result<()> {
        let Some(drag) = self.window.drag.as_mut() else {
            return Ok(());
        };
        let Some(tab) = drag.spring.due(now) else {
            return Ok(());
        };
        // Spent before the switch is attempted, not after: the rest has been
        // given its answer either way, and a gate left armed because the tab had
        // been reaped would ask again on every frame for the rest of the drag.
        drag.spring.spend(tab);
        let Some(index) = self.tab_slot_of(tab) else {
            return Ok(());
        };
        self.activate_tab(index, false)
    }

    /// The spring's next wake-up, for the loop's set.
    pub(in crate::runtime) fn drag_spring_deadline(&self) -> Option<Instant> {
        self.window.drag.as_ref()?.spring.deadline()
    }

    /// **缺陷 #188 — the list runs under a hand that has reached its edge.**
    ///
    /// The user's report is the shape of the bug: a card column with more cards
    /// than fit, a pane carried down to the column's foot, and nothing under the
    /// pointer but the last card that happened to be on screen — no way to reach
    /// the ones below it and no way to see whether the pane had become a card at
    /// all. The window had no edge auto-scroll on any surface (§7.1.6g's own
    /// note, 2026-08-27: *"本仓纵向也没有这样的东西"*), so this is the furniture
    /// arriving rather than a hole being patched.
    ///
    /// **A clock and not a branch in [`Runtime::drive_drag`]**, for
    /// [`Runtime::advance_drag_spring`]'s reason one function above: a hand held
    /// at the edge of a list is a hand that has *stopped moving*, and a scroll
    /// driven by pointer events alone would advance one step per twitch and stand
    /// still under the very gesture it exists for.
    ///
    /// **Three things happen here and they happen in this order**, because each
    /// is the ground the next stands on:
    ///
    /// 1. **The speed is re-read and the clock is wound or stopped.** A hand out
    ///    of the band, or a list already at the end it was heading for, clears
    ///    the instant outright — see [`Drag::autoscroll_ticked_at`] for why that
    ///    is not the same as leaving it to go stale.
    /// 2. **The list moves.** One integration, `speed × elapsed`, clamped by
    ///    [`seats::autoscroll_step`] to the run's own `max_scroll` — so the
    ///    distance travelled is a function of the time that really passed and
    ///    not of how many frames the machine managed to draw.
    /// 3. **The drag is surveyed again against the viewport that is now on
    ///    screen** (the ruling's ③). The slots moved under a pointer that did
    ///    not, so the landing, the insertion caret and — for a card being
    ///    reordered — the carried card's own offset are all stale by exactly the
    ///    distance the list travelled. [`Runtime::drive_drag`] is the one
    ///    function that re-solves all three, and it is spent whole rather than
    ///    partly copied, which is what makes "松手落在当时可见的槽位" true by
    ///    construction instead of by agreement.
    ///
    /// **A service, and therefore never paced** (closure review O4,
    /// 2026-09-18). This stood behind the window's display gate for one turn of
    /// that work's life, on the reasoning that every clock which moves something
    /// on the glass belongs there. It does not. What the gate decides is *who
    /// may ask for a frame of their own*, and this asks for none: step 2 is an
    /// **integrator**, and step 3 publishes through the gesture's own door. A
    /// refused turn therefore does not cost it a step, it costs it the gesture —
    /// a pane printing every five milliseconds refuses the gate for as long as
    /// it prints, so a hand held at the edge of a full strip would have moved
    /// the list nowhere at all for the length of a build log. It is free when
    /// there is no drag: [`Self::drag_autoscroll_aim`] answers `None`. Its own
    /// wake is [`Self::drag_autoscroll_deadline`], which is clamped to the
    /// display frame — that is where the pacing belongs, and the name of this
    /// method says which side of the line it is on.
    pub(in crate::runtime) fn service_drag_autoscroll(&mut self, now: Instant) -> Result<()> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let motion = self.app.motion;
        let Some((run, scroll, pointer)) = self.drag_autoscroll_aim(now) else {
            return Ok(());
        };
        let travelling =
            seats::autoscroll_speed(&run, scroll, (pointer.x, pointer.y), scale, motion) != 0.0;
        if !travelling {
            if let Some(drag) = self.window.drag.as_mut() {
                drag.autoscroll_ticked_at = None;
            }
            return Ok(());
        }
        // Nothing stands between the reading above and the integration below:
        // see this method's own note on why a service is never paced.
        let Some(last) = self
            .window
            .drag
            .as_ref()
            .and_then(|drag| drag.autoscroll_ticked_at)
        else {
            // The frame the hand arrived on winds the clock and moves nothing: a
            // step is struck between two instants, and there is only one yet. The
            // wake set has already asked for the second, one frame away.
            if let Some(drag) = self.window.drag.as_mut() {
                drag.autoscroll_ticked_at = Some(now);
            }
            return Ok(());
        };
        let Some(moved) = seats::autoscroll_step(
            &run,
            scroll,
            (pointer.x, pointer.y),
            scale,
            motion,
            now.saturating_duration_since(last),
        ) else {
            return Ok(());
        };
        self.set_tab_run_scroll(moved);
        // Wound before the re-survey rather than after it, because
        // `drive_drag` rebuilds the drag from a clone taken at its own door: an
        // instant written afterwards would be written onto the struct this line
        // is about to replace.
        if let Some(drag) = self.window.drag.as_mut() {
            drag.autoscroll_ticked_at = Some(now);
        }
        // The list moved under a stationary pointer, so what it is over changed
        // without the pointer having done anything — [`Runtime::scroll_rail`]'s
        // own line, for its own reason.
        self.window.seat_pointer.hover = self.chrome_target_at(pointer);
        self.drive_drag(pointer)?;
        Ok(())
    }

    /// The auto-scroll's next wake-up, for the loop's set (缺陷 #188).
    ///
    /// Clamped to the window's own display frame on
    /// [`Runtime::next_animation_deadline`]'s terms: asking for anything sooner
    /// would wake the loop to integrate a few microseconds and re-arm the same
    /// deadline, which is a spin wearing a schedule's clothes.
    ///
    /// `None` for every drag that is not currently at an edge — and for every
    /// window with no drag at all — so the ordinary gesture costs no wake-ups.
    /// **And `None` at the ends of the list**, which is what stops a hand parked
    /// at the foot of a fully-scrolled column from waking this window sixty times
    /// a second for as long as it stays there: [`seats::autoscroll_speed`]
    /// answers `0.0` there, and the clock and the deadline read the same answer.
    pub(in crate::runtime) fn drag_autoscroll_deadline(&self, now: Instant) -> Option<Instant> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (run, scroll, pointer) = self.drag_autoscroll_aim(now)?;
        let speed =
            seats::autoscroll_speed(&run, scroll, (pointer.x, pointer.y), scale, self.app.motion);
        if speed == 0.0 {
            return None;
        }
        let tick = self
            .window
            .drag
            .as_ref()
            .and_then(|drag| drag.autoscroll_ticked_at)?
            + self.window.frame_clock.interval();
        Some(self.clamp_animation_deadline(tick))
    }

    /// **Closed by its own trigger** (owner's report 2026-09-13; DESIGN.md
    /// §7.1.6e‴) — the one question every popup's dismissal arm asks, and the one
    /// place the answer is worked out.
    ///
    /// A press that dismisses a popover and lands on that popover's own trigger
    /// is **consumed by the dismissal**: the trigger does not see it. Without
    /// that, one press is answered twice — the arm puts the menu away and the
    /// very same press then reaches the button, which opens it again — and the
    /// button becomes one that cannot shut what it opened. That is what the
    /// owner photographed on the `Open ⌄` pill and on the breadcrumb's `…`.
    ///
    /// **It is the rule and not an exemption.** Five of the six triggered popups
    /// used to spare their own opener here and let it toggle for itself, which
    /// worked only where the opener was a docked `ChromeTarget` and the toggle
    /// was written: a float's branch filter matched neither and re-opened on
    /// every press, exactly as the pill did. Spending the press at the dismissal
    /// makes the trigger's own handler a plain opener again — it is only ever
    /// reached with nothing of its own up.
    ///
    /// Asked **before** the popup is closed, because the answer is about what is
    /// up now; the caller dismisses on [`OwnPress::dismisses`] and then returns
    /// on [`OwnPress::Spent`].
    fn press_on_its_own_trigger(
        &mut self,
        popup: Popup,
        position: PhysicalPosition<f64>,
    ) -> OwnPress {
        let raised = self.popup_trigger(popup);
        let pressed = self.popover_trigger_at(position);
        let verdict = press_spends_itself_closing(raised, pressed);
        if verdict == OwnPress::Spent {
            // **A press on a button is a button press wherever it is answered**
            // (`.files-foot`'s rule). The trigger's own arm breaks these two
            // chains on its way past and is not reached now, so the break is
            // made here: two presses on a `⌄` are two button presses and never
            // half a rename of the tab or the row underneath it.
            self.window.tab_clicks.interrupt();
            self.window.files_row_clicks.interrupt();
        }
        if verdict != OwnPress::Elsewhere {
            self.mouse_trace(|| {
                format!(
                    "popover_own_trigger popup={popup:?} verdict={verdict:?} trigger={raised:?}"
                )
            });
        }
        verdict
    }

    /// Drive the peek's two clocks from a pointer that has moved (G84/H112).
    ///
    /// A pinned window is deliberately not on this path at all: it is closed by
    /// `×`, Esc, Dock or its own trigger, and "the pointer went somewhere else"
    /// is not on that list.
    fn drive_float_hover(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        let hit = self.float_hit_at(position);
        if self.window.float_hover != hit {
            self.window.float_hover = hit;
            self.apply_pointer_cursor();
            if self.refresh_overlay() {
                self.present_chrome_change()?;
            }
        }
        // The clocks below are the *peek*'s alone, and it is asked for by name:
        // with several windows on screen the frame that decides the grace has to
        // be the transient one's, not whichever float is frontmost.
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(frame) = self.window.float.peek().map(|win| win.frame) else {
            return Ok(());
        };
        let reach = float::peek_reach(frame, position.x as f32, position.y as f32, scale);
        // The trigger counts as part of the region, and so does the root menu:
        // the mock-up's own list is `#files-flyout, #root-menu, .tab-files,
        // .pane-files` (3910-3911). Reaching for the thing that summoned it is
        // not leaving it.
        let on_trigger = matches!(
            self.chrome_target_at(position),
            Some(
                seats::ChromeTarget::TabFiles(_)
                    | seats::ChromeTarget::PaneFiles(_)
                    | seats::ChromeTarget::FilesRoot(_)
            )
        ) || self.pointer_is_on_the_peeks_reference(position)
            || self.pointer_is_in_the_peeks_own_glance(position);
        if reach.inside || on_trigger {
            self.window.float.hold();
        } else {
            self.window.float.release(reach.off_left, Instant::now());
        }
        Ok(())
    }

    /// A press inside the float. Returns whether the float consumed it.
    ///
    /// **Nothing is captured until the gesture is committed to** (G92): a press
    /// on `DOCK`, on `×`, on the foot or on a row must reach its own handler, so
    /// only the bare header and the grip start a drag. The mock-up captured the
    /// pointer on every press inside the window and paid for it — the release was
    /// retargeted to the window, so `×` and `DOCK` never saw a click at all.
    fn press_float(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some((id, part)) = self.float_hit_at(position) else {
            return Ok(false);
        };
        // **A press anywhere inside a window brings it to the front** (user
        // ruling 2026-08-12, rule ⑤), before the part is acted on and whatever
        // that part turns out to be. It is what every stacking window manager
        // does and the only thing that makes a buried window reachable — and the
        // raise is a frame debt, so it is paid here rather than left for whatever
        // the branch below happens to repaint.
        if self.window.float.raise(id) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        // Which tenant is in this window, asked once: the three parts below whose
        // verb differs by tenant all read the same answer, and asking three times
        // is three chances for them to disagree about one window.
        let holds_a_buffer = self
            .window
            .float
            .live(id)
            .is_some_and(|win| win.preview().is_some());
        match part {
            float::FloatPart::Close => {
                self.dismiss_float(id)?;
            }
            float::FloatPart::Dock if holds_a_buffer => self.dock_preview_float(id)?,
            float::FloatPart::Dock => self.dock_float(id)?,
            // The foot names what the window is showing, so it reveals what the
            // window is showing: a tree's root folder, a buffer's own file.
            float::FloatPart::Foot if holds_a_buffer => self.reveal_float_file(id)?,
            float::FloatPart::Foot => self.reveal_float_root(id)?,
            float::FloatPart::Save => self.save_preview_on(PreviewSurface::Float(id))?,
            float::FloatPart::Flip => self.flip_preview_source_on(PreviewSurface::Float(id))?,
            // **The news pill is not a part of this chassis** (owner's ruling
            // 2026-09-12). It floats inside the body rather than standing in a
            // row of its own, so there is nothing for `float_hit` to name — and
            // a press on `Reload` still reaches `press_notice` first, because
            // that door is asked above the chrome router entirely
            // (`mouse_input`) off the very rectangle the pill was drawn in. One
            // hit test for one piece of furniture, so `Reload` in a window and
            // `Reload` in a pane are the same press of the same button.
            // **The one way out of a file this window cannot show** (§7.39), the
            // docked card's own verb (`ChromeTarget::PreviewOpenButton`) one
            // surface over. It breaks a click chain for `.files-foot`'s reason: a
            // chain of clicks on a button is a chain of button presses.
            float::FloatPart::CardButton => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.open_preview_externally_on(PreviewSurface::Float(id))?;
            }
            // **A row of whichever page this window is on.** A Git row is not a
            // drag source and never was — the docked page's rows are not either
            // (P150 delegates the *tree's* rows on both hosts) — so the drag arm
            // is the tree's alone.
            float::FloatPart::Row(index) if self.float_shows_git_page(id) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_float_git_row(id, index)?;
            }
            float::FloatPart::GitAct { index, act } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_float_git_act(id, index, act)?;
            }
            // **The graph's three, through the docked graph's own three doors.**
            // A press on a window's commit graph is a press on that graph — the
            // surface it names is this window's, and nothing below the door
            // knows or needs to know which host it came from. Each breaks a
            // click chain for `.files-foot`'s reason: a chain of clicks on a
            // list or a button is not the beginning of a gesture elsewhere.
            float::FloatPart::GraphRow(index) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_graph_row(PreviewSurface::Float(id), index)?;
            }
            float::FloatPart::GraphTool(tool) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_graph_tool(PreviewSurface::Float(id), tool)?;
            }
            float::FloatPart::GraphDetail { part, .. } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_graph_detail(PreviewSurface::Float(id), part)?;
            }
            float::FloatPart::Row(index) => {
                // The float's rows are drag sources on the same terms the
                // column's are — P150's "delegated on both hosts".
                self.arm_row_press(RowHost::Float(id), index, position);
                self.press_float_row(id, index)?;
            }
            float::FloatPart::Head => {
                let frame = self
                    .window
                    .float
                    .live(id)
                    .map(|win| win.frame)
                    .unwrap_or_default();
                let grab = [position.x as f32 - frame[0], position.y as f32 - frame[1]];
                if self.window.float.is_pinned(id) {
                    // A window that is already yours is picked up on the press:
                    // there is nothing left to decide.
                    self.window.float_drag = Some(FloatDrag {
                        win: id,
                        kind: FloatDragKind::Move { grab },
                    });
                } else {
                    // A peek is a moment, and a moment cannot be picked up — yet.
                    // **User ruling 2026-08-12**: dragging its header is how you
                    // say "this one I am keeping", so the press waits at the
                    // latch, and the six pixels that would begin any other drag
                    // promote this window and carry it in the same gesture. Held
                    // still and released, it is the press it always was and means
                    // nothing at all.
                    //
                    // The offset is taken here, at the press, so the promotion
                    // does not move the window under the hand — see
                    // [`FloatHeadPress`].
                    self.window.float_head_press = Some(FloatHeadPress::armed(id, position, frame));
                }
            }
            float::FloatPart::Grip => {
                if self.window.float.is_pinned(id) {
                    self.window.float_drag = Some(FloatDrag {
                        win: id,
                        kind: FloatDragKind::Resize,
                    });
                }
            }
            // A buffer's body is an edit surface: the press puts the caret where
            // the pointer is and arms the drag that selects, exactly as it does
            // in a docked pane.
            //
            // **And its furniture answers before its content, in the docked
            // pane's own order** (user ruling, 2026-08-14): the body's vertical
            // bar rides over the whole document, a wide block's bar rides over
            // that block, and the caret is what is left underneath both. The
            // docked pane states this order in `chrome_mouse_input`; a float
            // cannot borrow that statement, because a press inside a window is
            // claimed *here* — `press_float` is asked before the chrome router
            // ever runs, which is what makes a window opaque to the layout
            // beneath it. So the order is restated, and a bar drawn on a float
            // is a bar a hand can take.
            float::FloatPart::Body if holds_a_buffer => {
                // **A player standing on the picture answers before anything
                // underneath it** (route B slice ②; §7.44 ①, found by
                // photographing the machine 2026-08-28).
                //
                // `press_video_at` states this order for the docked pane, in
                // `chrome_mouse_input` — and a float cannot borrow that
                // statement for the same reason the two bars below cannot: a
                // press inside a window is claimed *here*, above the chrome
                // router, which is what makes a window opaque to the layout
                // beneath it. Left unsaid, a float drew a play disc that lit
                // under the pointer and did nothing when it was pressed, and a
                // control bar no hand could reach — one recording on three
                // surfaces with one of the three unable to start it.
                //
                // The *release* half was never missing: a scrubber let go is
                // answered in `chrome_mouse_input`, which a release does reach.
                if self.press_video_at(position)? {
                    return Ok(true);
                }
                // **And a rendered page's words are drawn across on the same
                // terms** (§7.39, user report 2026-08-28: 「浮窗里的 md 选不中」).
                // The docked pane's order, restated here for `press_float`'s own
                // reason (a press inside a window is claimed *here*, above the
                // chrome router): the two bars stand over the document, the edit
                // surface and the rendered page are alternatives underneath them —
                // a buffer shows its source or its render, never both — and
                // `press_preview_text` is the render's half, reached through the
                // same `preview_surface_at` the docked press uses, so a document
                // in a float selects exactly as it does in a pane and shares one
                // `preview_select` model. A source buffer's `press_preview_body`
                // answers first and a rendered one falls through to it.
                if !(self.press_preview_body_thumb(position)?
                    || self.press_preview_block_thumb(position)?
                    || self.press_preview_body(position)?)
                {
                    self.press_preview_text(position)?;
                }
            }
            // **The row under the head, on the window that grew one** (§7.7 ⑩
            // 欠账). Through the docked row's own ladder and the docked row's own
            // verbs: what a press on this band does is a property of the band,
            // and a window is not a different kind of surface for it to mean
            // something else on.
            float::FloatPart::Rail(part) => {
                self.press_preview_rail(PreviewSurface::Float(id), part)?;
            }
            // The body away from any row: a press lands on the window and goes no
            // further, which is what makes a float opaque to the layout beneath.
            float::FloatPart::Body => {}
        }
        // One place for every branch: two of them put a gesture in the hand and
        // two of them take the whole window away, and in both directions the
        // shape the pointer is wearing was decided by what just changed.
        self.apply_pointer_cursor();
        Ok(true)
    }

    /// A press on one row of the float's tree.
    ///
    /// The docked column's own two verbs (C155): a directory folds or unfolds and
    /// the tree is redrawn; a file is merely selected, and **selecting must not
    /// rebuild the list** — a row node swapped between two clicks is how the
    /// double-click that opens a preview goes silently missing.
    fn press_float_row(&mut self, id: float::FloatId, index: usize) -> Result<()> {
        let motion = self.app.motion;
        let Some(win) = self.window.float.live_mut(id) else {
            return Ok(());
        };
        let epoch = win.epoch;
        let Some(files) = win.files_mut() else {
            return Ok(());
        };
        let rows = files::tree_view(&files.files, &files.cache).rows;
        let Some(row) = rows.get(index) else {
            return Ok(());
        };
        let key = row.key.clone();
        let kind = row.kind;
        let root = files.files.root.clone();
        // The same press as a docked column's (`press_files_node`), because a
        // click is a *toggle* and not a keystroke. This used to be spelled as
        // `TreeCommand::Right`-then-`Left` — but Right on an already-open
        // directory means "select the next row" (the keyboard's own semantics),
        // so the Left that followed acted on the *child* and an open folder
        // could never be shut from a float (user-reported 2026-08-12).
        let opening = press_files_node(&mut files.files, &key, kind);
        if matches!(kind, files::RowKind::Directory { .. }) {
            files.cache.turn_row(&key, opening, Instant::now(), motion);
        }
        if opening {
            // Unfolding is a refresh as well as a disclosure — the kernel says
            // nothing about a folded folder, because `files_watch` dropped its
            // handle with the row — so an opened directory is re-asked exactly
            // as a column's is.
            let request = files::DirRequest {
                window: self.window_id(),
                host: files::FilesHost::Float(epoch),
                path: files::full_path(&root, &key),
                key: key.clone(),
            };
            if !self.app.files_worker.request(request) {
                self.disable_files_worker();
            }
        }
        // K156 on a floating tree too (user report, 2026-08-16): the second
        // press on one file row opens it, on the same counter and the same
        // interval as a column's, keyed by this window.
        //
        // **A folder is still not counted here**, and this is the one place the
        // float and the column now differ. The 2026-08-19 ruling is written
        // about a *column*'s root — "that folder becomes this column's new root,
        // replacing it" — and a float is not a column; it is a window opened at
        // one place, with a foot that says which place. Reading the ruling onto
        // it would be deciding what a float's root means, which nobody has
        // decided, so a folder press here breaks the chain exactly as it did.
        let activating = matches!(kind, files::RowKind::File)
            && self
                .window
                .files_row_clicks
                .register(RowHost::Float(id), &key, Instant::now())
                == TabClick::Double;
        if !matches!(kind, files::RowKind::File) {
            self.window.files_row_clicks.interrupt();
        }
        if activating && let RowActivation::Preview(path) = files_row_activation(&root, &key) {
            self.open_preview(path)?;
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// A press on a peek's header has travelled: keep the window, and carry it
    /// (user ruling 2026-08-12).
    ///
    /// The whole of the gesture's second half, and it deliberately does not move
    /// anything: it promotes and hands the *same* pointer position on to
    /// [`Self::drive_float_drag`], which runs immediately after it in
    /// `cursor_moved`. Doing the first step of the move here as well would mean
    /// two functions that both know how a float follows a pointer, and the one
    /// that ran once per gesture would be the one nobody noticed drifting.
    ///
    /// **The frame does not change on this frame.** Promotion is a change of
    /// *mode*; `promote` is in-place by construction (G91), the grab offset was
    /// measured at the press, and the clamp that could have moved it —
    /// `resize_float_to_content`'s `height: auto` — is switched off by the very
    /// drag step that follows, before any tick can run it.
    fn promote_float_head_press(&mut self, position: PhysicalPosition<f64>) {
        let scale = self.window.renderer.metrics().scale_factor;
        let Some(press) = self.window.float_head_press.as_mut() else {
            return;
        };
        let Some(carry) = press.promoted(position, scale) else {
            return;
        };
        let pressed = press.win;
        self.window.float_head_press = None;
        // A peek that stopped being live while the button was down — dismissed by
        // Esc, wiped by a viewport change, *or replaced by another trigger's* —
        // has nothing this gesture may promote, and the press dies with it rather
        // than keeping a window it was never aimed at.
        if self.window.float.peek_id() != Some(pressed) {
            return;
        }
        let Some(win) = self.window.float.promote() else {
            return;
        };
        // The window keeps its identity across the promotion, so the carry that
        // follows is aimed at the very window the press began on.
        self.window.float_drag = Some(FloatDrag { win, kind: carry });
        // The hand closes on it the instant it becomes carryable, rather than at
        // the next move: this *is* the move, and a frame of open palm over a
        // window already travelling would be the cursor disagreeing with the
        // gesture.
        self.apply_pointer_cursor();
    }

    /// Move or resize the float under a dragged pointer. Returns whether it owned
    /// the event.
    fn drive_float_drag(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(drag) = self.window.float_drag else {
            return Ok(false);
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let viewport = self.float_viewport();
        let pointer = [position.x as f32, position.y as f32];
        let Some(win) = self.window.float.live_mut(drag.win) else {
            self.window.float_drag = None;
            return Ok(false);
        };
        // The grip's floors are the tenant's — a tree is useless below 200×150, a
        // buffer below 260×200 — and the whole dimension set is asked once, at
        // the door, exactly as `FloatSizing` was built for.
        let sizing = float_sizing_of(win);
        win.frame = match drag.kind {
            FloatDragKind::Move { grab } => {
                float::float_dragged_to(win.frame, pointer, grab, viewport, scale)
            }
            FloatDragKind::Resize => {
                float::float_resized_to(win.frame, pointer, viewport, scale, sizing)
            }
        };
        // A hand on the window ends `height: auto`: from here the size and the
        // place are the user's answer, and rows arriving later do not get to
        // move a window somebody has put somewhere.
        win.self_sizing = false;
        // **A window under the hand is a body that moved**, and a picture in it
        // has to be re-fitted to the box the next frame will draw — the float's
        // half of what `commit_seat_geometry` does for a pane. The document beside
        // it has always been rebuilt here for free, because a float's document is
        // built into its layer; a picture is filed on the pane and would
        // otherwise stay behind while its window travelled.
        //
        // The quiet boundary is deferred first, and it is the same one a divider
        // drag uses: a resize by the grip changes the fitted extent on every
        // pointer event, and asking the worker for each of them is the resample
        // storm R2 exists to forbid. A *move* changes no extent, so the
        // deferral costs it nothing and the exact raster it already holds is
        // still exact.
        self.defer_preview_resample(Instant::now());
        self.refresh_preview_for_layout();
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// A press on a trigger — the ladder (G91), now only two rungs.
    ///
    /// A peek from *this* trigger → promote it **in place**, keeping the tree you
    /// have already unfolded, because the gesture is called "keep this" and not
    /// "start again". Anything else → [`Self::open_float`], which opens a window.
    ///
    /// **改判 2026-08-12 — the third rung is gone.** It used to be "already pinned
    /// here → this is you asking for it to go", which is the mock-up's
    /// re-click-closes contract (3742-3751) and one of §7.1.2's four closers. With
    /// several windows reachable at once the trigger's meaning changed under it:
    /// it now says "show me this tree", and a control that means *close this one*
    /// when the root happens to be open and *open one* when it does not is a
    /// control nobody can predict. So the trigger has exactly one answer, every
    /// time, and pressing it again gives you another window on that tree — which
    /// is Explorer's answer and, after the same day's second ruling repealed
    /// 同根去重, this product's too (see [`Self::open_float`]). `×`, Esc and Dock
    /// are untouched, so nothing has become unclosable.
    fn press_float_trigger(&mut self, trigger: float::FloatTrigger) -> Result<()> {
        if self
            .window
            .float
            .peek()
            .is_some_and(|win| win.origin == Some(trigger))
        {
            self.window.float.promote();
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        self.open_float(trigger, float::FloatMode::Pinned)
    }

    /// **Whether a float stands over this point** — the Z-order gate
    /// [`Self::pane_hit_context`] reads before it resolves a pane (§7.39, user
    /// report 2026-08-28).
    ///
    /// The whole frame of every *drawn* float: a pinned window, a transient peek,
    /// and one in mid-drag alike, because all three are in `drawn` and all three
    /// cover what is under them. The frame is risen exactly as it is painted, so
    /// the rectangle the pointer is tested against is the one on the glass rather
    /// than the one the window is settling towards. The card a covered reference
    /// had already raised is retired by the ordinary door the moment this begins
    /// answering for the point — the pointer entering the window reads as the
    /// pointer leaving the reference (P149).
    ///
    /// A float's own body answers for itself through `float_hit_at` and
    /// `web_page_at`, neither of which comes through here; this is only about the
    /// panes underneath.
    pub(in crate::runtime) fn pointer_over_a_float(&self, position: PhysicalPosition<f64>) -> bool {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let now = Instant::now();
        let (x, y) = (position.x as f32, position.y as f32);
        self.window.float.drawn().any(|win| {
            let frame = risen_frame(win.frame, self.float_fade_of(win, now, scale));
            x >= frame[0] && x < frame[2] && y >= frame[1] && y < frame[3]
        })
    }

    /// [`Self::ask_the_worker_about_a_link_target`] for whatever link the pointer is standing on
    /// right now, resolved from the pane it is standing in (closure review B-1).
    ///
    /// The door for the two gestures that carry no hit of their own: the hand-over modifier going
    /// down, and a frame redrawn under a pointer that has not moved. Answers nothing and costs one
    /// hit test plus one map lookup when the pointer is over a link, and one hit test when it is
    /// not.
    pub(crate) fn ask_about_the_link_under_the_pointer(&mut self) {
        let Some((seat, hit)) = self.pane_frame_hit() else {
            return;
        };
        let Some(uri) = self
            .pane_frame(seat)
            .and_then(|frame| frame.hyperlink_at(hit.row, hit.column))
            .map(|hyperlink| hyperlink.uri)
        else {
            return;
        };
        self.ask_the_worker_about_a_link_target(seat, &uri);
    }

    fn forwarded_mouse_hit(&self) -> Option<bt_render::GridHit> {
        let (_, position, frame) = self.pane_hit_context()?;
        let hit = self
            .window
            .renderer
            .metrics()
            .hit_test_frame(frame, position.x, position.y)?;
        Some(live_viewport_mouse_hit(frame, hit))
    }

    /// The forwarded cell under the pointer, refused unless the pointer is in
    /// the pane whose child is about to be told about it.
    ///
    /// [`Self::forwarded_mouse_hit`] answers "where is the pointer", which is
    /// the whole question for a press — the press is what put the pointer's pane
    /// on the receiving end. A wheel notch picks its pane first, on its own
    /// rules, and only then asks for coordinates; the guard is what keeps those
    /// two answers from being about different panes.
    fn forwarded_mouse_hit_in(&self, seat: SeatId) -> Option<bt_render::GridHit> {
        let (hit_seat, position, frame) = self.pane_hit_context()?;
        if hit_seat != seat {
            return None;
        }
        let hit = self
            .window
            .renderer
            .metrics()
            .hit_test_frame(frame, position.x, position.y)?;
        Some(live_viewport_mouse_hit(frame, hit))
    }

    /// Whether the cell a press landed on carries a target this window has verified for itself
    /// — the question [`press_belongs_to_the_window`] asks of the grid (ticket #59).
    ///
    /// One reading of two lists, in the order their verbs are spent: an image reference first
    /// because it is the one that must have been *opened* to count, then the OSC 8 span. Both are
    /// asked of the pane the pointer is standing in and of the frame that pane last drew, which is
    /// the same frame [`Runtime::begin_local_selection`] will read a moment later — so a press that
    /// this says is ours cannot then find nothing to do.
    fn pressed_cell_target(&self, hit: bt_render::GridHit) -> PressedCellTarget {
        if self.local_image_path_hit(hit).is_some() || self.hyperlink_hit(hit).is_some() {
            PressedCellTarget::Ours
        } else {
            PressedCellTarget::Ordinary
        }
    }

    pub(in crate::runtime) fn activate_hyperlink_hover_if_due(
        &mut self,
        now: Instant,
    ) -> Result<()> {
        // The **ledger** is read here, on the settle, and the settle happens once per link —
        // [`HyperlinkHover::activate_if_due`]'s own note. It is the same question
        // [`Self::activate_hyperlink`] asks on the press, so the sentence the reader is shown and
        // the door the press opens are one answer read twice. Neither asks a disk (audit 3 C-2).
        let namespace = self.hovered_pane_path_namespace();
        let namer = bt_transcript::paths::PathNamer::Pane(&namespace);
        // Lifted out of the window for the length of the call so that the ledger — which lives in
        // a pane of the same window — can be read while the hover itself is being written. The
        // hover is `Default`, and nothing between these two lines can observe the gap.
        let mut hover = std::mem::take(&mut self.window.hyperlink_hover);
        let settled =
            hover.activate_if_due(now, namer, &|path| self.hovered_pane_path_verdict(path));
        self.window.hyperlink_hover = hover;
        if settled {
            self.repaint_hovered_pane()?;
        }
        Ok(())
    }

    pub(crate) fn pointer_left(&mut self) -> Result<()> {
        // The page hears it too, and hears it as a move to somewhere it is not:
        // the engine refuses a `LEAVE` outright (`w0p-evidence.md` gate 3), so a
        // point far outside the rectangle is what says the hand has gone.
        if self.window.web_pointer_inside {
            self.window.web_pointer_inside = false;
            self.window.web_cursor = None;
            let pages: Vec<LeafId> = self.window.web.keys().copied().collect();
            for leaf in pages {
                self.send_to_web_page(
                    leaf,
                    bt_platform::WebMouseEvent::Move,
                    PhysicalPosition::new(-1.0, -1.0),
                );
            }
        }
        self.window.pointer_position = None;
        // **Both `⌄` clocks, told the pointer is nowhere** (user report,
        // 2026-08-21) — through the same function every pointer move goes
        // through, handed the same `None` the field above now holds, so the two
        // doors cannot come to two different answers about where a hand that has
        // left is. Owed here for `drive_rail_zone`'s reason one line down and
        // more sharply: the band this window is most often left *for* is the
        // title bar's own drag strip, which is `HTCAPTION` and therefore sends
        // no motion at all, so without this the menu a `⌄` opened would stand
        // over the terminal until something else took it down. The grace is not
        // skipped — a hand that clips the corner of the bar on its way back to
        // the menu is inside 150ms and keeps it.
        self.observe_chevrons(None, Instant::now());
        // R2: a pointer that has left the window is not in the rail, and the rail
        // has to be told — nothing else will move the pointer again to tell it,
        // so an open panel would simply stay open over the terminal forever.
        self.drive_rail_zone(None);
        // And the command marks rail, for the identical reason one line up: a
        // crest still lit after the pointer has left the window is a tick
        // claiming to be under a hand that is not there.
        self.drive_command_rail_hover(None)?;
        // And the capsule's own controls, for the identical reason: a toggle
        // still lit after the pointer has left the window is a button claiming
        // to be under a hand that is not there.
        self.drive_search_hover(None)?;
        // And the strip's, for the identical reason.
        self.drive_notice_hover(None)?;
        // Deliberately *not* a drag cancel, and the reason is measurable rather
        // than stylistic: winit takes the Win32 mouse capture on button-down
        // (`capture_mouse`, its `WM_LBUTTONDOWN` arm), so a held drag keeps
        // receiving motion outside the window and is guaranteed its own
        // button-up. `CursorLeft` here means the pointer crossed the client
        // rect, which during a drag is an ordinary thing to do — the tab strip
        // runs to the window's top edge — and cancelling on it would throw a
        // reorder away for a pixel of overshoot. K129's real cancel is capture
        // loss, and the only capture loss winit surfaces is losing the window.
        // The overlay's own hover goes with it: a `×` still lit after the
        // pointer has left the window is a button claiming to be under a
        // pointer that is not there.
        let settings_hover_cleared = self.window.settings.set_hover(None);
        // **And the pane the hand was standing in, not only the control it was
        // on** (user report with two screenshots, 2026-08-24). The two are
        // separate channels on purpose — [`seats::ChromePointer::pane_hover`]
        // says why — and every pane head's hover run hangs off the second: the
        // `×`, the `⌄` and the folder on a terminal head, and on a page's head
        // the rule, `</>`, the pop-out and the lock. Taking `hover` alone left
        // that whole run lit over a window nobody was pointing at, which is the
        // very sentence the three comments above this one refuse for the `×`.
        // It is loudest during a **resize**: the border being dragged is
        // non-client, so the hand has left for the length of the gesture, and
        // the run therefore looked as though it came and went with the window's
        // width. One door clears both, so they cannot drift apart again.
        if (self.window.seat_pointer.left_the_window() || settings_hover_cleared)
            && self.refresh_chrome()
        {
            self.present_chrome_change()?;
        }
        self.dismiss_peek()?;
        let hyperlink_changed = self.window.hyperlink_hover.clear();
        // **And the band the pointer was on, at once** (owner's ruling
        // 2026-09-18: 「为什么不能和其它 hover 保持一致」). A pointer that has
        // left the window has left the band, and every other hover on the two
        // lines above this one — the run a pane head wears, the `×` on a tab,
        // the settings row, the underline under a link — is taken on this very
        // door with no clock between. The marks still leave over the ninety
        // milliseconds they arrived on; what is gone is the half-second of
        // waiting before that begins. See [`Self::leave_hovered_math`].
        self.leave_hovered_math(Instant::now(), MathHoverExit::PointerLeft)?;
        // Whatever pane the pointer was standing in is standing in none now, and owes the removal
        // of its marks — which, if it was not the focused one, only a redraw can deliver.
        self.window.hover_pane = None;
        if hyperlink_changed {
            self.repaint_hovered_pane()?;
        }
        // The pointer is gone, so `hovered_image_reference` now answers `None`: any solid underline
        // still on screen must fall back to its resting dots.
        self.refresh_image_reference_underline()?;
        Ok(())
    }

    /// Put a pointer gesture's bytes in **one named pane's** shell.
    ///
    /// [`Self::send_user_input`] writes through the focused-leaf deref, which is
    /// the right shell for a keystroke and the wrong one for a wheel notch: the
    /// notch belongs to the pane under the pointer (user ruling, 2026-08-15),
    /// and a report addressed to the keyboard's pane is a row and a column from
    /// one grid delivered into another. Named seat, one write, no deref.
    ///
    /// No return-to-live half, and not because this path happens not to need one
    /// — because a mouse gesture never has one. Every mouse member of
    /// [`UserInputKind`] answers `returns_view_to_live` with false, so the branch
    /// would be dead code standing where a policy looks like it lives.
    fn send_mouse_input_to(
        &mut self,
        seat: SeatId,
        bytes: &[u8],
        context: &'static str,
    ) -> Result<()> {
        // Typing into the stand-in shell is what makes it yours — and a wheel
        // forwarded into a program running there is as much a claim on it as a
        // keystroke. Same sentence as `send_user_input`'s, for the same reason.
        if self.window.placeholder_tab == Some(self.window.tabs[self.window.active_tab].id) {
            self.window.placeholder_tab = None;
        }
        self.pending_keyboard_at = Some(Instant::now());
        let active = self.window.active_tab;
        let pty = self.window.tabs[active]
            .sessions
            .get(&seat)
            .and_then(|leaf| leaf.pty.as_ref());
        write_pty_input(pty, bytes, context)
    }

    pub(crate) fn pointer_moved(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        self.window.pointer_position = Some(position);
        self.window.pointer_last_seen = Some(position);
        // **One resolution of the reference under the pointer, for the three surfaces that ask
        // about it** (audit 3 C-2). Emptied here and filled by the first of them — see
        // [`WindowRuntime::pointer_reference`] for why this line is the whole of its lifetime.
        self.window.pointer_reference.get_mut().take();
        // **The hosted page, before anything returns**, for the reason the two
        // chevron clocks below are told: a page's own hover ends when the
        // pointer is somewhere else, and every branch under this one consumes
        // the move it would have heard that from.
        self.drive_web_pointer(position);
        // **Both `⌄` clocks, before anything returns** (user ruling, 2026-08-16).
        // A chevron's leave grace is running precisely when the pointer is
        // somewhere else, so it has to be told about moves that every branch
        // below consumes — including the ones inside the menu it opened, which
        // is the state the grace exists to tell apart from having left.
        self.observe_chevrons(Some(position), Instant::now());
        // **And the icon rail's zone, before anything returns** — the third
        // instrument with the argument above, and it is the same argument
        // (user report, 2026-08-23: a pane carried towards another tab found a
        // rail of unlabelled icons and no way to make it open).
        //
        // The zone is *not* a hover. It asks where the pointer is, and where the
        // pointer is stays a fact while something is being carried — so it must
        // not be filed below the branches that own the pointer, all of which
        // return before `update_chrome_hover` and the second asker inside it.
        // A drag is only the loudest of them; a divider, a float and a selection
        // all travel across the left edge too.
        //
        // The mock-up says this by construction rather than by a clause:
        // `evalRailZone` is a `document`-level `pointermove` listener, and
        // neither it nor `railBusy` mentions a drag — so the panel rolls out
        // under a laden hand exactly as it does under an empty one, and
        // `stripEl()` then hands the drop engine the rectangle it grew into.
        // Nothing about the landing needs writing for that: `tab_run` measures
        // the rail as it currently stands, so the rows widen with the panel.
        //
        // Two askers and no second answer: [`Self::drive_rail_zone`] re-aims
        // nothing when the target has not moved, and the one inside
        // `update_chrome_hover` is for that function's *other* callers — the
        // doors where the answer changed with no pointer move at all.
        self.drive_rail_zone(Some(position));
        // **A held scrubber owns the pointer, ahead of everything** (route B
        // slice ②; §7.44 ②) — and outside its own bar, for the reason every
        // drag below is asked outside its own box: a scrub that stopped tracking
        // the moment the hand left a thirty-four pixel strip would make the end
        // of a recording a matter of aim.
        if self.drag_video_bar(position)? {
            return Ok(());
        }
        // And every playing surface is told where the pointer is, before any
        // branch below can consume the move — the bar's reveal is armed by a
        // pointer settling on the picture and its dwell is ended by one that has
        // gone somewhere else, and a seat that only heard about moves nothing
        // else wanted would hold its bar up for ever.
        //
        // **It asks for no frame of its own**, and that is deliberate: what a
        // move can change is when the bar is *due* — an intent armed, a dwell
        // restarted — and `strip_animation_work` reads exactly those two
        // through `VideoSeats::bar_deadline` on the turn this move ends. A
        // present forced here would be a repaint on every pointer move anywhere
        // in the window for as long as anything is playing.
        self.note_video_hover(Some(position));
        // The glance card's thumb, ahead of everything: it is the topmost thing
        // on the glass, and a gesture in flight is not a hover. It owns the
        // pointer outside the card too — a drag that let go the moment it left
        // a 300px card would make the last line a matter of aim, and the card
        // would dismiss itself out from under the hand that was using it.
        if self.drag_file_peek_thumb(position)? {
            return Ok(());
        }
        // A selection being drawn across the edit surface owns the pointer, and
        // it owns it *outside* its own body too: a drag that stopped extending
        // the moment it left the pane would make selecting the last line a matter
        // of aim. It is asked before every hover below it for the reason a
        // divider drag is — a gesture in flight is not a hover.
        if self.drag_preview_selection(position)? {
            return Ok(());
        }
        // A selection being drawn across a **rendered** page owns the pointer on
        // exactly those terms, and outside its own body for the same reason.
        if self.drag_preview_text(position)? {
            return Ok(());
        }
        // A picture in hand owns the pointer on exactly the same terms, and
        // outside its own body for the same reason: a pan that stopped the
        // moment the hand crossed the pane's edge would make the last corner of
        // a zoomed screenshot unreachable.
        if self.drag_preview_image(position)? {
            return Ok(());
        }
        // A thumb in hand owns the pointer for the same reason, and outside its
        // own band for the same reason. The body's bar before the block's, the
        // order they are drawn in and the order their presses are taken in.
        if self.drag_preview_body_thumb(position)? {
            return Ok(());
        }
        // A terminal pane's thumb owns the pointer on the same terms and outside
        // its own lane for the same reason: a drag that stopped tracking when
        // the hand wandered eleven pixels inboard would make the far end of a
        // sixty-thousand-line scrollback a matter of aim.
        if self.drag_terminal_thumb(position)? {
            return Ok(());
        }
        // And the foot's mark, on the same terms: a hand that has left the eleven
        // pixels it took hold in is still holding what it took.
        if self.drag_terminal_column_thumb(position)? {
            return Ok(());
        }
        if self.drag_preview_block_thumb(position)? {
            return Ok(());
        }
        // The thumb under the pointer lights up. Asked here rather than among
        // the hovers below because it belongs to the same surface the two
        // gestures above do — and answered `None` while an overlay owns the
        // pointer, so a scrim never leaves a bar lit behind it.
        //
        // **And `None` under a floating window too** (user report 2026-09-12).
        // It is the same sentence the scrim gets and it is asked of the same
        // door: `pointer_target_at` is `None` only where *nothing* on the glass
        // has claimed the point, so a preview window standing over a pane no
        // longer leaves that pane's scrollbar, its link underline or its hex
        // column lit under a hand that is on the window.
        let free = self.settings_layout().is_none()
            && self.dirty_gate_layout().is_none()
            && !self.app.quit.as_ref().is_some_and(quit::Quit::is_asking)
            && self.pointer_target_at(position).is_none();
        self.note_preview_body_hover(free.then_some(position))?;
        // The lane the pointer is in — the fact that lights one pane's mark and
        // holds it on the glass. Answered `None` behind an overlay for the
        // reason the bars above it are: a scrim never leaves a bar lit under it.
        self.note_terminal_thumb_hover(free.then_some(position))?;
        // The foot's mark lights the same way and summons nothing: what is
        // recorded here only changes the ink of a mark that is already drawn.
        self.note_terminal_column_hover(free.then_some(position))?;
        self.note_preview_block_hover(free.then_some(position))?;
        self.note_preview_link_hover(free.then_some(position))?;
        self.note_preview_hex_hover(free.then_some(position));
        // The overlay owns the pointer the way it owns the next click: no chrome
        // hover, no divider, no hyperlink, no peek settle behind the scrim.
        // The invitation takes the pointer outright, scrim included, in the
        // order it is drawn: under the gate, over the dialog.
        // The first-run card first, in the order it is drawn.
        if let Some(layout) = self.first_run_layout() {
            let over = first_run::hit(&layout, position.x, position.y);
            if self.window.first_run.set_hover(Some(over)) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            // **The one overlay that does talk about itself.** Every other
            // modal here answers `None` because it has nothing under the
            // pointer worth a second box; this card's rows each carry the
            // sentence that says which of the reader's files a switch writes,
            // and that sentence lives in the window's own `.tip`.
            let anchor = self.tooltip_anchor_at(position);
            self.note_tooltip(anchor)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        if let Some(layout) = self.psreadline_invite_layout() {
            let over = restore::invite_hit(&layout, position.x, position.y);
            if self.window.psreadline_invite.set_hover(Some(over)) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        if settings::geometry::pointer_moved(self, position.x, position.y)? {
            return Ok(());
        }
        // The quit card takes the pointer outright, scrim and all, in the order
        // it is drawn.
        if let Some(layout) = self.quit_card_layout() {
            let over = restore::quit_hit(&layout, position.x, position.y);
            let moved = self
                .app
                .quit
                .as_mut()
                .is_some_and(|quit| quit.set_hover(Some(over)));
            if moved && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        // The gate takes the pointer outright, on its scrim as well as on its box.
        if let Some(layout) = self.dirty_gate_layout() {
            let over = restore::gate_hit(&layout, position.x, position.y);
            if self.window.dirty_gate.set_hover(Some(over)) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        // The prompt is not modal either. Over its own box the buttons light up;
        // everywhere else the window carries on, because the terminal behind it
        // is still yours to use while the question stands.
        if let Some(layout) = self.restore_layout() {
            let over = restore::hit(&layout, position.x, position.y);
            if over.is_some() {
                if self.window.restore_prompt.set_hover(over) && self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                return Ok(());
            }
            if self.window.restore_prompt.set_hover(None) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
        }
        // **A hand on a notice holds its clock** (user ruling, 2026-08-16), and
        // lights the `×` it is reaching for. Asked unconditionally, because
        // *leaving* a card is what starts its clock again and that happens
        // wherever the pointer goes next; the answer is then whether the card
        // also owns this hover, in which case nothing below it hears about the
        // pointer at all — a card is a surface, and the rows under it are not
        // being pointed at.
        if self.drive_toast_hover(position)? {
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        // The picker is not modal, so it takes the pointer only where it is: over
        // its own box the rows answer, and everywhere else the window carries on.
        if let Some(layout) = self.profile_menu_layout() {
            let over = profiles::hit(
                &layout,
                &self.app.profile_programs,
                self.app.recent.entries(),
                position.x,
                position.y,
            );
            if self.window.profile_menu.set_hover(over.flatten()) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            if over.is_some() {
                // **The picker's own rows may speak, and only they.** This
                // branch returns before the tip is noted further down, which was
                // right for as long as a menu row had nothing to say; now a
                // greyed profile explains its grey and a Recent row carries the
                // path its caption cropped, so the tip has to be answered here or
                // the anchors would be registered and never reached.
                //
                // Filtered rather than taken as it comes: a popup covers what is
                // under it, and `tooltip_anchor_at` would otherwise hand back a
                // tab's tip for a point over the menu's own body, printing a tip
                // about something the pointer cannot see.
                let anchor = self.tooltip_anchor_at(position).filter(|(anchor, _)| {
                    matches!(anchor, tooltip::TooltipAnchorId::ProfileRow(_))
                });
                self.note_tooltip(anchor)?;
                self.update_chrome_hover_target(None)?;
                return Ok(());
            }
        }
        // The root menu takes the pointer the same way and on the same terms.
        if let Some(layout) = self.root_menu_layout() {
            let over = profiles::root_menu_hit(&layout, position.x, position.y);
            if self.window.root_menu.set_hover(over.flatten()) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            if over.is_some() {
                let anchor = self
                    .tooltip_anchor_at(position)
                    .filter(|(anchor, _)| matches!(anchor, tooltip::TooltipAnchorId::RootRow(_)));
                self.note_tooltip(anchor)?;
                self.update_chrome_hover_target(None)?;
                return Ok(());
            }
        }
        // And the preview's switcher, on the same terms as the root menu beside
        // it: it is the same popup with a different list in it.
        if let Some(seat) = self.preview_menu_seat()
            && let Some(layout) = self.preview_menu_layout()
        {
            let items = self.preview_menu_items(seat);
            let over = profiles::preview_menu_hit(&layout, &items, position.x, position.y);
            if self.window.preview_menu.set_hover(over.flatten()) && self.refresh_overlay() {
                self.present_chrome_change()?;
            }
            if over.is_some() {
                self.note_tooltip(None)?;
                self.update_chrome_hover_target(None)?;
                return Ok(());
            }
        }
        // And the file menu the same way again, above both — it is drawn above
        // both. Leaving it *without* clearing the hover would be wrong for the
        // opposite reason to the two above: this is the one menu a keyboard can
        // walk, and a row lit by a key press must not be un-lit by a pointer
        // that merely happens to be resting somewhere else.
        if let Some(layout) = self.file_menu_layout() {
            let over = profiles::file_menu_hit(&layout, position.x, position.y);
            if let Some(row) = over
                && let Some(menu) = self.window.file_menu.as_mut()
                && menu.hover != row
            {
                menu.hover = row;
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
            }
            if over.is_some() {
                self.note_tooltip(None)?;
                self.update_chrome_hover_target(None)?;
                return Ok(());
            }
        }
        // And the git context menu, the same three lines again — and with the
        // same care about a row lit by a key: a keyboard walk must not be undone
        // by a pointer that merely happens to be resting somewhere else.
        if let Some(layout) = self.git_menu_layout() {
            let over = profiles::git_menu_hit(&layout, position.x, position.y);
            if let Some(row) = over
                && let Some(menu) = self.window.git_menu.as_mut()
                && menu.hover != row
            {
                menu.hover = row;
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
            }
            if over.is_some() {
                self.note_tooltip(None)?;
                self.update_chrome_hover_target(None)?;
                return Ok(());
            }
        }
        // And the terminal's own menu — its own function since §7.1.6i's floor
        // gave it a child, for the reason the pane menu's is one: what the
        // highlight does when the pointer leaves a row stops being "the new row
        // takes it" the moment there is a second surface to be on the way to.
        // The one line it keeps from the three it used to be: a row that cannot
        // answer is not hovered, so a pointer over a greyed `Copy` puts the
        // highlight out rather than leaving it on the row the hand has left.
        if self.drive_term_menu_hover(position)? {
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        // And the graph's branch filter, the same three lines a third time.
        if let Some(layout) = self.graph_filter_menu_layout() {
            let over = profiles::git_filter_menu_hit(&layout, position.x, position.y);
            if let Some(row) = over.clone()
                && let Some(menu) = self.window.graph_filter_menu.as_mut()
                && menu.hover != row
            {
                menu.hover = row;
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
            }
            if over.is_some() {
                self.note_tooltip(None)?;
                self.update_chrome_hover_target(None)?;
                return Ok(());
            }
        }
        // And the pane head's menu on the same level. Its own function rather
        // than the file menu's three lines a fourth time, because this is the
        // one popup in the window with a *child*: what the highlight does when
        // the pointer leaves a row is no longer "the new row takes it" but "the
        // new row takes it unless the hand is on its way to the submenu", which
        // is the safety triangle and is not three lines.
        if self.drive_pane_menu_hover(position)? {
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        // And a tab's own menu (丙2), on the same level and by the same
        // argument: it is the fourth popup here with a *child*, so what the
        // highlight does when the pointer leaves a row is "the new row takes it
        // unless the hand is on its way to the window list".
        if self.drive_tab_menu_hover(position)? {
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        // **And the palette** (DESIGN.md §7.55), which needs none of the above
        // machinery because it has no child and no safety triangle: the pointer
        // moves the selection and that is the whole of it.
        if self.drive_palette_hover(position)? {
            self.note_tooltip(None)?;
            self.update_chrome_hover_target(None)?;
            return Ok(());
        }
        // A peek's header, pressed and now travelling: the window is kept and the
        // same gesture becomes the carry (user ruling 2026-08-12). Immediately
        // above the drag it turns into, so the promotion and the first step of the
        // move land on one pointer event and the window never sits still for a
        // frame wondering what it is.
        self.promote_float_head_press(position);
        // **And the glance card's head, on the same six pixels** (user ruling
        // 2026-08-27, §7.29). Beside its twin rather than up beside the card's
        // thumb, because what it turns into is the very drag the next line
        // drives: the promotion and the first step of the carry land on one
        // pointer event, which is what "拖动跟手" means when it is written down.
        if self.promote_file_peek_press(position)? {
            return Ok(());
        }
        // A float being moved or resized owns the pointer outright, above the
        // divider for the reason it is a separate state at all: it is a window
        // over the layout, so while your hand is on it the layout underneath is
        // not being aimed at.
        if self.drive_float_drag(position)? {
            return Ok(());
        }
        // A divider drag owns the pointer outright: while one is in flight the
        // terminal hears nothing, which is the same rule an in-progress
        // selection drag already lives by.
        if self.drive_divider_drag(position)? {
            return Ok(());
        }
        // A press that has travelled past the drag threshold becomes a drag
        // (J112/J113), and a tab press on its way withdraws any activation it was
        // still holding back (J105). Both sources cross the same six pixels
        // through the same [`DragLatch`]; what differs is only what each press
        // was holding on to.
        //
        // J122 is upheld by position rather than by a flag: `drive_divider_drag`
        // has already returned above if a resize is in flight, so neither branch
        // below can be reached while one is — "one gesture owns the pointer at a
        // time", and the ordering is what says so.
        let scale = self.window.renderer.metrics().scale_factor;
        if self
            .window
            .tab_press
            .as_mut()
            .is_some_and(|press| press.travelled(position, scale))
        {
            let press = self
                .window
                .tab_press
                .expect("a press that travelled is a press");
            self.begin_tab_drag(press, position)?;
        } else if self
            .window
            .pane_press
            .as_mut()
            .is_some_and(|press| press.latch.travelled(position, scale))
        {
            let seat = self
                .window
                .pane_press
                .expect("a press that travelled is a press")
                .seat;
            self.begin_pane_drag(seat, position)?;
        } else if self
            .window
            .row_press
            .as_mut()
            .is_some_and(|press| press.latch.travelled(position, scale))
        {
            // P81/J113 — the third source crosses the same six pixels, through
            // the same latch, in the same `else if` chain: one press is in the
            // hand at a time, and which one it is was decided at the press.
            let press = self
                .window
                .row_press
                .clone()
                .expect("a press that travelled is a press");
            self.begin_row_drag(press, position)?;
        }
        // A drag owns the pointer outright, exactly as a divider drag does:
        // hover, the peek flyout, the hyperlink underline and the terminal's own
        // selection all go quiet for the length of the gesture.
        if self.drive_drag(position)? {
            return Ok(());
        }
        self.update_chrome_hover(position)?;
        // H112: the intent is armed by hovering a trigger and disarmed by leaving
        // the region. Asked after the chrome hover has been updated, because it
        // is the chrome hit test that says which trigger — if any — is under the
        // pointer, and the answer has to be this frame's.
        //
        // A drag in flight arms nothing (`armFlyOpen`'s first line, mock-up
        // 3926): every path that owns the pointer has returned above, so the only
        // way to reach here is with a free hand.
        //
        // **And not while another hover panel is on the glass** — one at a time
        // (user report, 2026-08-19), see [`HoverFloat`]. Filtering the *subject*
        // rather than skipping the call is deliberate: `observe(None)` is how
        // this host is told the pointer has left a trigger, and a call skipped
        // instead would leave an intent armed at whatever the hand was over
        // before the menu came up.
        let trigger = self
            .hover_float_free(HoverFloat::Flyout)
            .then(|| self.float_trigger_at(position))
            .flatten();
        self.window.float.observe(trigger, Instant::now());
        self.drive_float_hover(position)?;
        // P146/P150 — the glance's intent, armed by the row under the pointer
        // on **either** host, and disarmed by everything else. Asked here for
        // the flyout intent's own reason one line up: it is the hit tests that
        // say which row — if any — is under the pointer, and the answer has to
        // be this frame's. Every path that owns the pointer has returned above,
        // so a drag, a divider and a float carry cannot arm one.
        //
        // **And the exclusion is asked of the row** (user ruling 2026-09-07),
        // which is the one difference from the flyout's line above: a row of the
        // folder card may raise a glance while that card is on the glass, because
        // the two are one region rather than two panels — see [`glance_may_arm`].
        let row = self.glancing_row_at(position);
        // The card is taken down **here**, on the move that left the row, and the
        // frame it owes is paid here too: the chrome hover above has already
        // presented by the time this runs, so a card retired without its own
        // repaint would stay on the glass until some unrelated event redrew it —
        // which under a hand that has come to rest is never (real-machine
        // capture, 2026-08-13: the glance survived the move onto a folder row).
        if self.observe_file_peek(row, Instant::now()) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        // Below every gesture that owns the pointer and beside the hover it
        // follows: the anchors under a drag or a divider were never reached, and
        // each of those paths returned above having said so. An open picker also
        // returns above, but it answers the tip on its way out — its rows are
        // anchors of their own now.
        // The peek first, and the tip only where the peek is not already
        // answering (§6). A tab that qualifies for neither is untouched by both.
        let peeked = self
            .hover_float_free(HoverFloat::LayoutPeek)
            .then(|| self.layout_peek_target_at(position))
            .flatten();
        self.note_layout_peek(peeked)?;
        // **The rail is a surface standing on the pane's own right edge**, so it
        // takes the pointer before anything the pane itself would answer with: a
        // hyperlink underlined beneath a tick is a link the tick is standing on,
        // and a mouse report sent from under one is a report about a cell nobody
        // pointed at. Every gesture that owns the pointer has already returned
        // above, so this only ever sees a free hand.
        //
        // A selection drag is the one live gesture that reaches this line — it is
        // answered further down, so that a drag pulled across the rail keeps
        // following the hand — and a rail must not light under it.
        //
        // **Above the tip**, which it was not until the glance card arrived: the
        // card is a tip whose anchor is the crest, and the crest is what this line
        // settles. Asking the anchor list first would offer the pointer whatever
        // the *previous* frame's crest was, so a hand walking down a rail would
        // see a card one tick behind it.
        // **The capsule takes the pointer above the rail** — the order the two
        // are drawn in, and the order R5 in the search-block inventory says they
        // have to be answered in: they share the pane's top-right corner, and on
        // a short pane the rail's own block reaches up under the capsule. A hand
        // on a toggle must not also be lighting a tick behind it.
        let on_search =
            self.drive_search_hover(self.window.mouse_route.is_none().then_some(position))?;
        // The strip, beside the capsule: it is a surface inside the pane too, and
        // a hand on one of its words must not also be lighting a tick behind it.
        let on_notice = self.drive_notice_hover(
            (self.window.mouse_route.is_none() && !on_search).then_some(position),
        )?;
        let on_command_rail = self.drive_command_rail_hover(
            (self.window.mouse_route.is_none() && !on_search && !on_notice).then_some(position),
        )?;
        // The glance card wins over anything the pane underneath it would say,
        // for the reason the rail takes the pointer at all: it is the surface on
        // top. `command_tick_anchor` answers only while a rail is hot, so
        // everywhere else this is the tip list exactly as it was.
        let anchor = self
            .command_tick_anchor()
            // The colour card, on the glance card's own terms and for its own
            // reason: its anchor is a token that `note_preview_hex_hover` settled
            // a few lines ago, and the list below still holds the *previous*
            // frame's. Asking the list first would mean a card that never
            // arrives while the hand is still, because a hand that has stopped
            // moving produces no second pointer event to find it with.
            .or_else(|| self.preview_hex_anchor())
            .or_else(|| {
                self.tooltip_anchor_at(position)
                    .filter(|(anchor, _)| !self.layout_peek_suppresses(*anchor))
            });
        self.note_tooltip(anchor)?;
        // Which pane the pointer is in, settled before anything asks a question of it. Everything
        // below — the formula hover, the link's underline, the reference's underline, the flyout —
        // is answered by that pane, with no focus prerequisite and no focus side effect: the
        // window says what it is being pointed at (user ruling 2026-08-10).
        self.observe_hovered_pane()?;
        // `position` stays the window's own coordinates from here down. The flyout is a floating
        // window laid over the surface rather than a tenant of one pane, so its anchor is a point
        // on the window; translating it into some seat's corner is what used to make a peek raised
        // in one pane appear beside another.
        let now = Instant::now();
        let math_hit = self.update_math_hover(now)?;
        let hit = self.frame_hit().filter(|_| !on_command_rail && !on_search);
        let hyperlink = hit
            .filter(|_| {
                math_hit.is_none() && !matches!(self.window.mouse_route, Some(MouseRoute::Local(_)))
            })
            .and_then(|hit| self.hyperlink_hit(hit));
        // **And the pane is asked what the disk says about it** (audit 3 C-2). An `OSC 8` target
        // is the one shape of reference that never went through the printed-path scan — the
        // program declared it and the cell carries it verbatim — so until this line existed
        // nobody had put a question about it anywhere, and the window thread asked on its own
        // behalf, on this very event. It is idempotent: an answered name, a queued one and one a
        // worker is holding all cost a map lookup, which is what makes it safe per motion event.
        if let Some((seat, hit)) = self.window.hover_pane.zip(hyperlink.as_ref()) {
            self.ask_the_worker_about_a_link_target(seat, &hit.uri);
        }
        if self.window.hyperlink_hover.observe(hyperlink, now) {
            // **The hand moved with the modifier; now it moves with the cell**
            // (§7.1.5g, user ruling 2026-08-20). While the finger meant only
            // "`Ctrl` is down over a link", `ModifiersChanged` was the whole of
            // when it could change and applying the shape there was enough. It
            // now means "a press here would answer", which is a fact about *this
            // link* — a plain hover over a readable file wears it and a plain
            // hover over a page does not — so the shape has to be recomputed
            // whenever the link under the pointer changes. Inside `observe`'s own
            // answer because that is exactly when it did: a move within one run,
            // or across the body between two of them, changes nothing to apply.
            self.apply_pointer_cursor();
            self.repaint_hovered_pane()?;
        }
        self.refresh_image_reference_underline()?;
        let peek_path = hit
            .filter(|_| {
                math_hit.is_none() && !matches!(self.window.mouse_route, Some(MouseRoute::Local(_)))
            })
            .and_then(|hit| self.peek_target(hit));
        if self.window.peek_hover.observe(peek_path, position, now) {
            self.present_peek_overlay(None)?;
        }
        if math_hit.is_some() || matches!(self.window.mouse_route, Some(MouseRoute::MathBlock)) {
            return Ok(());
        }
        // Above the "is the pointer over a cell" guard, deliberately: a selection
        // drag is answered in its *origin* pane's cells, which it has whether or
        // not the pointer is over a cell of the pane it is currently crossing.
        // Under the old guard a drag that left its pane simply stopped following
        // the hand until it came back.
        if matches!(self.window.mouse_route, Some(MouseRoute::Local(_))) {
            return self.extend_local_selection();
        }
        if hit.is_none() {
            return Ok(());
        }
        // **The pane under the pointer, its modes and its child** (user report,
        // 2026-08-17). A motion report is a sentence about where the hand is,
        // and it can only be true of the pane the hand is over. This used to
        // measure the cell in the pointer's pane and then read the *focused*
        // pane's modes and write into the *focused* pane's PTY — so with a
        // program tracking the mouse on the right and the pointer over the
        // left, the right-hand program was told about coordinates from a grid
        // it does not have, and lit a row nobody was pointing at. A drag that
        // began as a forwarded press keeps its origin pane through
        // `mouse_route`, which is why the button case still asks the route.
        let Some((seat, _, _)) = self.pane_hit_context() else {
            return Ok(());
        };
        let Some(hit) = self.forwarded_mouse_hit_in(seat) else {
            return Ok(());
        };
        let modes = self.leaf_terminal_modes(seat);
        let Some((sgr, button)) = route_forwarded_mouse_motion(
            self.window.mouse_route.as_ref(),
            modes,
            self.window.modifiers,
        ) else {
            return Ok(());
        };
        let bytes = input::mouse_bytes(
            sgr,
            button,
            input::MouseProtocolEvent::Motion,
            hit.row,
            hit.column,
            self.window.modifiers,
        );
        self.send_mouse_input_to(seat, &bytes, "forward SGR mouse motion to PTY")
    }

    /// **Where the pointer is: the floating windows first, then the chrome
    /// behind them** (user report 2026-09-12, `docs/DESIGN.md` §7.15 ⑩).
    ///
    /// The one door every pointer question in this window goes through, hover
    /// and press alike. `b1cf054` made a float's claim terminal for the press by
    /// writing the rule into [`Self::file_row_under`] — one caller — and the
    /// hover, which does not go through that caller, went on reading the docked
    /// ladder as though the window in front of it were glass: the row hidden
    /// under a preview float lit up, and after the peek delay raised its glance
    /// card on top of the window that was covering it. The rule belongs here,
    /// where both gestures pass.
    ///
    /// **A window's claim is terminal.** [`Self::float_hit_at`] is total inside
    /// a frame — a body its tenant declines comes back
    /// [`float::FloatPart::Body`], anything the named rectangles miss comes back
    /// [`float::FloatPart::Head`] — so there is no answer of "the pointer went
    /// through". Whatever the topmost window says is the whole answer, and the
    /// panes behind it are not asked.
    ///
    /// The sweep is entered only when there is a window to sweep, on
    /// [`Self::popover_trigger_at`]'s note: `float_hit_at` measures two
    /// captions before it looks at anything, and a window with nothing floating
    /// should pay none of it.
    pub(crate) fn pointer_target_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<PointerTarget> {
        if self.window.float.hit_order().next().is_some()
            && let Some((id, part)) = self.float_hit_at(position)
        {
            return Some(PointerTarget::Float(id, part));
        }
        self.docked_chrome_target_at(position)
            .map(PointerTarget::Chrome)
    }

    pub(crate) fn update_chrome_hover(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        // R2's open trigger, asked before the hover is read: opening the rail
        // changes what the pointer is over, and asking in the other order would
        // light up a row one frame after the panel that holds it.
        self.drive_rail_zone(Some(position));
        // **The download sheet answers the hover before the window does**, in
        // the order it is drawn and for the reason its press is claimed above
        // the whole chrome router: it is an overlay standing on one seat's
        // body, so nothing the router can name is in front of it there.
        //
        // Asked here and not inside [`Self::chrome_target_at`] because only the
        // hover is the sheet's to answer this way — the press is
        // [`Self::press_web_sheet`]'s, which swallows the card and the scrim
        // whole, and a router that also returned the sheet's controls would be
        // two doors onto one press.
        let sheet = self
            .window
            .web_sheet_layouts
            .iter()
            .find_map(|(seat, layout)| {
                websheet::hit(layout, *seat, position.x as f32, position.y as f32)
            });
        // One question, asked once, for both halves below.
        let target = self.pointer_target_at(position);
        let hover = sheet.or(match target {
            Some(PointerTarget::Chrome(target)) => Some(target),
            // **A window is opaque to the hover** (user report 2026-09-12): a
            // point inside a float is the float's, so there is no docked chrome
            // under this pointer to light.
            Some(PointerTarget::Float(..)) | None => None,
        });
        // `.pane:hover` is a second question about the same pointer, and it has
        // to be asked here rather than derived from `hover`: over a terminal's
        // body `hover` is `None`, because a terminal is not chrome, and that is
        // most of the pane.
        //
        // **And therefore it needs the rail's sovereignty stated separately.**
        // `hover` gets it from [`seats::ChromeTarget::RailBody`]; this question
        // reads the seat layout, and the seat layout keeps only the *parked*
        // width clear of an icon rail — so under an open one it would go on
        // naming the pane the rail is standing over.
        //
        // **And a float's the same way, and for the same reason the router
        // states once** (2026-09-12): the seat layout is the docked geometry and
        // knows nothing about the windows drawn over it, so a pane a float is
        // covering would go on wearing `.pane:hover` — and on showing the four
        // head controls that come with it — under a hand that is on the window.
        let pane = (!matches!(target, Some(PointerTarget::Float(..)))
            && !self.panel_covers(position))
        .then(|| seats::pane_at(&self.seat_layout, position.x, position.y))
        .flatten();
        self.update_chrome_hover_target_in_pane(hover, pane)
    }

    fn update_chrome_hover_target(&mut self, hover: Option<seats::ChromeTarget>) -> Result<()> {
        // A caller that has decided the pointer belongs to something floating
        // over the panes — an open picker, the restore prompt — is saying the
        // pointer is not in a pane either.
        self.update_chrome_hover_target_in_pane(hover, None)
    }

    /// The pointer wears the shape of what it is over: a diagonal arrow on a
    /// pinned float's grip and an open hand on its header, a resize arrow along
    /// a divider's axis, the ordinary arrow everywhere else. Each is kept for
    /// the whole drag it starts, even when the pointer slips off the band or the
    /// grip it began on.
    pub(crate) fn apply_pointer_cursor(&mut self) {
        let grasp = float_grasp(
            self.window.float_drag.map(|drag| drag.kind),
            self.window.float_hover.map(|(_, part)| part),
            // The window **under the pointer**, not "is any window pinned": a
            // peek standing over a pinned one would otherwise borrow its grab
            // hand, and a header that is not a handle would advertise itself as
            // one (2026-08-12).
            self.window
                .float_hover
                .is_some_and(|(id, _)| self.window.float.is_pinned(id)),
        )
        // **And the glance card's head, which is a header this window draws
        // without a float behind it** (user ruling 2026-08-27, §7.29).
        //
        // Second rather than first, and `or_else` rather than a fourth argument,
        // because the card is *above* every float on the glass: when it is up
        // and the pointer is on it, no float is under the pointer at all, so the
        // two can never both answer. Where they could — a carry already in
        // flight — the float's answer is the one that must win, and asking it
        // first is what makes that true by construction.
        .or_else(|| self.file_peek_head_grasp());
        let divider_axis = self
            .window
            .divider_drag
            .as_ref()
            .map(|drag| drag.dir)
            .or_else(|| match self.window.seat_pointer.hover {
                Some(seats::ChromeTarget::Divider(split)) => self
                    .seats
                    .split_slots(&self.seat_layout)
                    .into_iter()
                    .find(|slot| slot.id == split)
                    .map(|slot| slot.dir),
                _ => None,
            });
        // **Inside the page, the page decides.** A hosted document says what
        // its own rectangle means — a link, a text field, a resize grip — and
        // this window has no way to know any of it. Outside that rectangle the
        // answer is this window's, exactly as it always was, which is why the
        // page's answer is asked for first and only here.
        if let Some(cursor) = self
            .window
            .pointer_position
            .filter(|position| self.point_is_on_the_web_page(*position))
            .and(self.window.web_cursor)
            .and_then(web_page_cursor)
        {
            hang_watch::during(hang_watch::Station::WindowCursor, || {
                self.window.window.set_cursor(cursor)
            });
            return;
        }
        self.window.window.set_cursor(pointer_cursor(
            self.window.drag.is_some(),
            grasp,
            divider_axis,
            self.preview_link_grasp() || self.terminal_link_grasp(),
            self.window.command_rail_hover.is_some(),
            self.image_grasp(),
        ));
    }

    /// **What the auto-scroll is looking at this instant** — the run under the
    /// hand, where its list currently stands, and where the hand is (缺陷 #188).
    ///
    /// Its own function because both the tick and the deadline ask exactly this,
    /// and a clock whose "is anything owed" and "do it" read the geometry two
    /// different ways is a clock that wakes up to be told there was nothing to
    /// do.
    ///
    /// **A file row is offered the band like everything else, since 2026-08-30.**
    /// This used to turn one away, and it said so as §7.1.6b′ ③'s refusal rather
    /// than a second one — *"没有任何 tab 面会接一个文件行,所以也没有任何 tab 面
    /// 有理由为它挪动自己"*. The premise is what changed: every tab surface takes
    /// a row now ([`row_strip_landing`]), so a hand carrying one to the foot of a
    /// long card column has exactly the reason ③ said it did not have. The gate
    /// is gone rather than inverted — this function asks nothing about what is in
    /// the hand any more, which is the shape it had before ③ needed saying.
    fn drag_autoscroll_aim(
        &self,
        now: Instant,
    ) -> Option<(seats::TabRun, f32, PhysicalPosition<f64>)> {
        let drag = self.window.drag.as_ref()?;
        let run = self.tab_run(now)?;
        Some((run, self.tab_run_scroll(), drag.pointer))
    }

    /// Everything a drag does on the way in, whatever it is carrying (J112).
    ///
    /// The three things here are the three the mock-up's `startDrag` does for
    /// every source alike: put away what was explaining the thing you just picked
    /// up, take the pointer, and record the gesture. Anything that varies by
    /// source has already happened in the caller.
    pub(in crate::runtime) fn begin_drag(
        &mut self,
        source: DragSource,
        carry: DragCarry,
        position: PhysicalPosition<f64>,
        home: Option<[f32; 4]>,
    ) -> Result<()> {
        // **A zoomed tab lets the zoom go the moment anything is picked up**
        // (§7.1.6l), before the first landing is offered.
        //
        // The rule zoom lives under is that any verb which changes the tree lets
        // it go, and a drag is the gesture that *aims* at the tree before it
        // changes it: every landing this window offers — an edge of a pane, a
        // rim, a centre to swap with, a preview to open a file into — is a place
        // on a picture a zoomed stage is not showing. Beginning a drag over one
        // pane would ask the hand to aim at panes it cannot see, and the drop
        // preview is computed by the very `solve` the frame ran (M155/D4), so it
        // would be drawn from a tiling nobody is looking at.
        //
        // Here rather than in `begin_pane_drag`, because it is true of every
        // source alike — a tab, a files row and a pane all land in this tree —
        // and this is the one function all of them pass through. A drag that
        // arrives from *another* window needs no counterpart: §7.1.6k lands a
        // foreign pane on this window's tab strip, not in this tab's tree, so
        // there is nothing to aim at here for it to be unable to see.
        if let Some(stage) = self.seats.zoom() {
            self.toggle_pane_zoom(stage)?;
        }
        // `hidePeek()` is the first line of `startDrag` (6482), and L135 is why:
        // a schematic left hanging under a thing that is now moving would be
        // describing where that thing used to be.
        self.hide_layout_peek()?;
        // And the glance card, for the same sentence one surface further in: a
        // row that is now travelling is not a row the pointer is resting on
        // (P145 — "gone on leave/press/scroll/**drag**").
        self.hide_file_peek();
        // **F2 — the application takes the pointer too.** Opened here rather than
        // on the first move that crosses a boundary, because the two numbers F5
        // needs are facts about the *press* and there is no second chance to read
        // them.
        self.open_broker(&source, position);
        self.window.drag = Some(Drag {
            source,
            carry,
            pointer: position,
            landing: None,
            paste_offer: None,
            home,
            spring: SpringGate::default(),
            autoscroll_ticked_at: None,
            seam: None,
        });
        // Hover goes quiet for the whole gesture: while something is in your hand
        // the chrome has nothing to offer the pointer, and a `×` lighting up
        // under a tab that is sliding past is an affordance that cannot be taken.
        //
        // This is also where J117's pinning lands, and deliberately not in a
        // second call of its own: clearing the hover target re-applies the
        // pointer's shape, and the shape is a function of `self.drag` — which was
        // set one line ago. One expression decides it, in one place, for both the
        // taking and the letting go.
        self.update_chrome_hover_target(None)
    }

    /// Where this drag would land if the hand opened now — **the seam U5 plugged
    /// into** (K123-K135).
    ///
    /// Pure: it reads the window and answers a [`DropLanding`], and the live half
    /// of whatever it answers is applied by [`Runtime::drive_drag`] afterwards.
    /// Keeping the survey and the commitment apart is what lets the geometry grow
    /// without the state machine growing with it, and it is why this takes a
    /// source and a position rather than `&self.drag`: the question "what is
    /// under the pointer" has nothing to do with how far a tab has slid.
    ///
    /// **The priority chain, and why it is in this order.**
    ///
    /// 1. **The strip, whatever is in the hand** (K123, 6786-6787). It is asked
    ///    first because the strip is a surface in its own right and sits above
    ///    the layout, not because a tab belongs to it — a *pane* over the strip
    ///    is K124's tearing, and a tab over the layout is N159's merge. Neither
    ///    source is confined to one surface, and reading the source before the
    ///    rectangle is what used to make it look like they were.
    /// 2. **A tab over its own layout is nothing** (K129, 6934). This test only
    ///    ever passes because of the flip in [`Runtime::leave_strip`]: while the
    ///    dragged tab is the one on screen there is no other layout for it to
    ///    join, and the merge would be a tab merging into itself.
    /// 3. **The layout's rim, then a pane's zones** — [`seats::aim_at_layout`],
    ///    which carries K127, K128 and K130-K134 and states the rim-before-pane
    ///    ruling at length.
    /// 4. **Never onto yourself** (K135, 7101): a pane held over its own
    ///    rectangle has no landing at all, in any zone. Applied to the aim rather
    ///    than inside it, because "which pane is this" is a fact about the
    ///    pointer and "is that pane the one in my hand" is a fact about the hand.
    ///
    /// It re-reads the strip's geometry rather than being handed it, and that is
    /// a deliberate cost: a surveyor that depends on what its caller happened to
    /// measure is a surveyor that cannot grow a branch without threading a second
    /// argument through every existing one. The price is one strip solve per
    /// pointer move, on a strip of at most a few dozen tabs.
    pub(in crate::runtime) fn survey_drop(
        &self,
        source: &DragSource,
        home: Option<[f32; 4]>,
        position: PhysicalPosition<f64>,
        seam: &mut Option<usize>,
    ) -> Option<DropLanding> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // **The seam latch is cleared here and set in exactly one place below**
        // (user ruling 2026-09-06). A hand that has left the tab list altogether
        // — for the layout, for another window's glass, for its own home ground
        // — is not standing in any seam, and a latch that survived the departure
        // would give the widened reach of [`seats::TAB_SEAM_RELEASE_LOGICAL_PX`]
        // to a re-entry that has not earned it.
        *seam = None;
        // **P86 — home ground, asked before anything else.** A payload let go
        // over the very rectangle it was picked up from has landed nowhere, in
        // any zone and on any surface. It is K135's sentence for a source that
        // is not a pane, and it is asked first for K135's reason: "is this where
        // I picked it up" is a fact about the hand, and no amount of geometry
        // underneath can make a retraction mean something else.
        if over_home_ground(home, position) {
            return None;
        }
        if let Some(run) = self.tab_run(Instant::now())
            && run.contains(position.x, position.y)
        {
            return self.survey_strip(source, &run, position, seam);
        }
        let showing = self.window.tabs[self.window.active_tab].id;
        // K129 — dragging the active tab onto its own layout is meaningless.
        if *source == DragSource::Tab(showing) {
            return None;
        }
        // **§7.1.6k′ (user ruling 2026-08-23) — the policy record that stood
        // here has been cashed in, so there is nothing left to refuse.**
        //
        // What used to be here was a `return None` for a pane whose tab is not
        // the one on screen, with §7.1.6k's own words beside it: *"离家的 pane
        // 在这张舞台上没有落点,这是政策位不是墙(§7.0 法则③)… 要把那扇门开出来是
        // 它自己的一片(从一棵树里摘下、落进另一棵),现在是未做而不是做不了"*. The
        // user met that state on the machine — spring across to the target tab,
        // aim at the stage to choose a side, and the stage answered nothing at
        // all — and ruled the door open. So a pane the spring left behind in
        // another tab is offered **exactly** the zones a pane of this tab is,
        // and the box is drawn the same way.
        //
        // The type's objection was real and it is answered rather than ignored:
        // `DropCargo::Pane` names a seat **of the tree being planned**, so a
        // foreign pane is not a cargo it can carry. It arrives as
        // `DropCargo::Layout` instead — a one-leaf subtree, renumbered on the
        // way in like any arriving tab's, which is the very route the rim
        // append has taken since §7.1.6k. [`Runtime::plan_inputs_for`] chooses
        // between the two shapes in one place, so "what may a foreign pane land
        // on" has exactly one author and is not re-decided here.
        let aim = seats::aim_at_layout(
            &self.seat_layout,
            self.layout_host_rect(),
            self.seats.pane_count(),
            scale,
            position.x,
            position.y,
        )?;
        landing_for_aim(source, showing, aim)
    }

    /// **Whether the glass under this pointer is this window's own**
    /// (multiwindow slice F2).
    ///
    /// Asked of the window manager and not of a rectangle, and the difference is
    /// the whole of F2's correctness. Two Folio windows overlap all the time: a
    /// pointer resting on B's tab list is *inside A's client rectangle* whenever
    /// B is on top of A, so a source that surveyed its own geometry would light a
    /// slot in A and hand the payload to A when the hand opened, while the reader
    /// watched B. Z-order, minimisation and everybody else's windows decide this,
    /// and none of them is visible from here.
    ///
    /// A window this process cannot ask about answers `true`, which is the
    /// conservative half **for a gesture that moves panes**: with no answer, the
    /// gesture is the ordinary one it always was, in the window that is holding
    /// it. It is not the conservative half for a text write, which is why the
    /// third answer is kept rather than folded away — see [`GlassHere`].
    fn pointer_is_on_our_own_glass(&self, position: PhysicalPosition<f64>) -> bool {
        glass_allows_a_drop(self.glass_here(position))
    }

    fn drive_drag(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(mut drag) = self.window.drag.clone() else {
            return Ok(false);
        };
        // What is in the hand can go away underneath the gesture — a background
        // shell exits and `reap_exited_tabs` closes its tab, or a seat is closed
        // by a verb this window ran for some other reason. There is then nothing
        // left to drag, and the state must not survive the thing it points at.
        if !self.drag_source_lives(&drag.source) {
            self.window.drag = None;
            // F2: and the application's pointer with it — see [`Self::finish_drag`].
            self.app.drag_broker = None;
            self.apply_pointer_cursor();
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(true);
        }
        drag.pointer = position;
        self.leave_strip(&mut drag, position)?;
        // **F2 — the window that is holding the payload offers nothing while the
        // hand is over somebody else's glass.**
        //
        // Asked before the survey rather than filtered out of it, because it is
        // not a fact about geometry at all: the pointer may be squarely inside
        // this window's tab list and still be resting on the window that is
        // sitting on top of it. What the target offers is the *target's* to say,
        // and it says it through [`FolioApp::drive_drag_broker`].
        let ours = self.pointer_is_on_our_own_glass(position);
        // Borrowed out of the clone so the survey can keep `&self`: the seam
        // latch is state of *this gesture* and the runtime holds none of it.
        let mut seam = drag.seam;
        drag.landing = ours
            .then(|| self.survey_drop(&drag.source, drag.home, position, &mut seam))
            .flatten();
        // **And what that landing is promising, if it is promising a text
        // write** (review 2026-09-17 P1-a). Read here, in the same breath as the
        // landing it belongs to, because this is the moment the box on screen is
        // decided — the release then has something taken at a *different* moment
        // to compare its own reading against. See [`PasteOffer`].
        drag.paste_offer = self.paste_offer_at(drag.landing);
        // **A hand on another window's glass is in no seam of this one.** `ours`
        // is false there and the survey never runs, so the latch has to be
        // cleared by the same fact that skipped it — otherwise a hand that left
        // over a seam and came back would re-enter it with the widened reach it
        // had not earned.
        drag.seam = if ours { seam } else { None };
        self.publish_to_broker(&drag, position, ours);
        // **§7.1.6k — the spring is told what the survey answered, not where the
        // pointer is.** One reading of the geometry per move, and the dwell then
        // agrees with the drop by construction: a tab the strip would not hand
        // this pane to is a tab the hand cannot spring to either, and the card
        // column's refusal (②) reaches the spring through the same door without
        // being said twice.
        drag.spring.observe(
            match drag.landing {
                Some(DropLanding::StripAdopt { tab }) => Some(tab),
                _ => None,
            },
            Instant::now(),
        );
        if let (Some(DropLanding::StripReorder { slot }), Some(tab), Some(carry)) =
            (drag.landing, drag.tab(), drag.tab_carry())
        {
            drag.carry = DragCarry::Tab(self.settle_strip_reorder(tab, carry, slot, position));
        }
        self.window.drag = Some(drag);
        // The ghost lives in the overlay rather than in the chrome, and it does
        // not need its own repaint call: `refresh_chrome` rebuilds the overlay
        // from the same choke point and answers `true` if *either* changed. On a
        // pane drag the chrome is identical frame to frame and the overlay is the
        // only thing moving, which is exactly the case that choke point exists
        // for.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Whether the thing this drag is carrying is still in the window.
    fn drag_source_lives(&self, source: &DragSource) -> bool {
        match source {
            DragSource::Tab(tab) => self
                .window
                .tabs
                .iter()
                .any(|candidate| candidate.id == *tab),
            // Asked of the pane's **own** tab (§7.1.6k). Reading the tab on
            // screen was right while a pane drag could not outlive its tab being
            // drawn; with the spring it is the bug that ends the gesture on the
            // first pointer move after a switch, because the seat is of course
            // not in the tree that just arrived.
            DragSource::Pane(leaf) => self
                .tab_state(leaf.tab)
                .is_some_and(|tab| tab.seats.tree().contains(leaf.seat)),
            // A row's payload is a *path*, and a path does not stop existing
            // because the list it was read out of was rebuilt — which is the
            // whole reason the payload is the identity rather than an index
            // (P87). Whether the file is still on the disk is the drop's
            // question and it is asked where it can be answered: the read that
            // fills the buffer, or the walk that fills the column.
            DragSource::Row(_) => true,
        }
    }

    /// Let go (K118, K121, J120, and the mock-up's commit table at 7202-7231).
    ///
    /// One question: what did the last survey answer?
    ///
    /// * **A landing.** It decides. The only landing this slice has is the strip
    ///   reorder, and it decided already — it was applied live, slot by slot, as
    ///   the tab travelled — so there is nothing to commit and nothing to
    ///   rebuild. The tab hands its offset to the settle, the settle runs it down
    ///   to its slot, and the strip that was already on screen carries on being
    ///   the strip (K122).
    /// * **No landing — J120.** The carried thing goes back to the slot the drag
    ///   began in, sliding rather than jumping, displaced neighbours travelling
    ///   home with it.
    ///
    /// **J120 is not a cancel and not a commit, and the distinction is not
    /// pedantry.** It shares [`Runtime::settle_home`] with Esc because the
    /// *motion* is the same one — there is only one way for a thing to go back
    /// where it came from — but nothing else about the two is. A cancel is the
    /// user retracting a gesture; this is the gesture completing and finding
    /// nowhere to be. The difference shows up in what each leaves behind: both
    /// keep the press's activation (J108), and neither writes the session,
    /// because a drag that landed nowhere chose nothing to record.
    fn release_drag(&mut self) -> Result<bool> {
        let Some(drag) = self.window.drag.take() else {
            return Ok(false);
        };
        let now = Instant::now();
        let motion = self.app.motion;
        // A gesture is not a click, and it is not half of one either.
        self.window.tab_press = None;
        self.window.pane_press = None;
        self.window.row_press = None;
        self.window.tab_clicks.interrupt();
        // **F2 — the hand may have opened over another window, or over none.**
        //
        // Asked before the local verdict rather than folded into it, because the
        // two answer about different surfaces: `release_verdict` reads a landing
        // this window surveyed, and there is no such landing to read when the
        // pointer was never over this window's glass. What is left to this window
        // on those paths is J120's settle, which is the same thing it does for a
        // release that landed nowhere — the payload has not moved, and everything
        // that will move it happens at the loop's door.
        if self.hand_over_across_windows(&drag)? {
            return self.finish_drag();
        }
        match release_verdict(drag.landing) {
            DragRelease::Commit => {
                if let (Some(tab), Some(carry)) = (drag.tab(), drag.tab_carry())
                    && let Some(index) = self
                        .window
                        .tabs
                        .iter()
                        .position(|candidate| candidate.id == tab)
                {
                    // K121 as re-ruled: the settle and nothing else. See
                    // [`TabState::settle_into_slot`] for why the wash that used
                    // to be started here belongs to a hand-over alone.
                    self.window.tabs[index].settle_into_slot(carry.offset, now, motion);
                    // The strip's order is the file's order, and a reorder is a
                    // choice the user made rather than a state being explored
                    // (§5.1). A drag that moved nothing decided nothing, and the
                    // activation it did commit has already recorded itself.
                    if carry.moved {
                        self.mark_session_dirty(now);
                    }
                }
            }
            // A drop that was refused between the last survey and the release —
            // the pointer's answer is a function of a tree and a viewport, and
            // both can move under a still hand — has landed nowhere, which is
            // exactly what J120 is for. One outcome, reached two ways.
            DragRelease::Land if self.commit_layout_drop(&drag)? => {}
            // N157, and the same "one outcome, reached two ways" above it: a
            // tear-out the tree refuses between the survey and the release (the
            // pane's sibling closed under a still hand, so G84 now applies) has
            // landed nowhere. A pane going home is a no-op by construction — the
            // tree was never touched — which is why this can be tried and
            // abandoned safely.
            DragRelease::Extract { slot } if self.commit_strip_extract(&drag, slot)? => {}
            // §7.1.6k, and the same "one outcome, reached two ways" a third time:
            // a tab that stopped existing under a still hand, or a tree that
            // stopped fitting, has been landed on by nobody.
            DragRelease::Adopt { tab } if self.commit_strip_adopt(&drag, tab)? => {}
            DragRelease::Extract { .. }
            | DragRelease::Adopt { .. }
            | DragRelease::Land
            | DragRelease::Home => self.settle_home(&drag),
        }
        self.finish_drag()
    }

    /// J119 — "never mind".
    ///
    /// Esc, and the pointer stream ending without a button-up. Everything the
    /// gesture put on screen comes down and **no drop is committed**; the carried
    /// thing goes home by the same route J120 uses.
    ///
    /// **Deviation, recorded.** The mock-up's `cancelDrag` settles the tab into
    /// whatever slot the live reorder last put it in (7153-7165) — it undoes the
    /// *drop*, not the reordering. J120 rules that the native build reads the
    /// mock-up's own sentence literally instead: the reorder is as much a commit
    /// as the drop is, it was made by the same gesture, and a cancel that keeps
    /// half of what it cancelled leaves the user to undo the rest by hand.
    ///
    /// What Esc does *not* undo is the activation (J108): the press chose this
    /// tab, and a cancelled drag does not unchoose it. Nothing here has to say so
    /// — the promise was paid the moment the drag began.
    pub(crate) fn cancel_drag(&mut self) -> Result<bool> {
        let Some(drag) = self.window.drag.take() else {
            return Ok(false);
        };
        self.window.tab_press = None;
        self.window.pane_press = None;
        self.window.row_press = None;
        self.window.tab_clicks.interrupt();
        self.settle_home(&drag);
        self.finish_drag()
    }

    /// What both exits do once the gesture's own business is finished.
    ///
    /// One place, because these are the four things that are true of *ending* a
    /// drag rather than of any particular way of ending one — and the mock-up
    /// puts the same four in both `cancelDrag` and its `pointerup` (7166-7183
    /// against 7189-7201) for exactly that reason.
    /// Answers `true` — a drag that got this far consumed the event that ended
    /// it, which is what both callers report to their own callers.
    fn finish_drag(&mut self) -> Result<bool> {
        // **F2 — and the fifth thing, which is that the application's pointer
        // goes down with the window's.** Every exit passes through here (Esc, the
        // release, and the source's own capture loss), so the broker has one
        // grave and the highlight it was drawing in some other window is taken
        // down by that window's next turn — which is this one.
        self.app.drag_broker = None;
        // **缺陷 #189 — the room the column was holding goes back, and the list
        // comes back with it.**
        //
        // A card column reserves a slot's worth of scroll for the stand-in
        // ([`Self::strip_guests`]), so a hand that ran the list to its end while
        // a pane was in the air left `rail_scroll` one card-pitch past where a
        // column with no guest can go. Where the drop *made* that card the two
        // are already equal and this changes nothing — the list really is a card
        // longer. Where it did not — the pane went into a tab, or the gesture
        // was abandoned — the offset would outlive its reason and the column
        // would stand with a card-tall blank under its last card until somebody
        // turned the wheel.
        //
        // Asked of the geometry rather than of the drag, and asked *here*, which
        // is the one place every ending passes through: the answer is "how far
        // can this column go now", and now is after the drop has been committed
        // and the drag has been taken down.
        if let Some(max_scroll) = self
            .focus_rail_geometry_now(Instant::now())
            .map(|column| column.max_scroll)
        {
            self.window.rail_scroll = self.window.rail_scroll.min(max_scroll);
        }
        // J117 in reverse: the pointer stops being pinned the instant the hand
        // is empty, and takes back the shape of whatever it is now over.
        self.apply_pointer_cursor();
        // Hover was frozen at "nothing" for the whole gesture; the pointer has
        // not moved, but what is under it has.
        if let Some(position) = self.window.pointer_position {
            self.update_chrome_hover(position)?;
        }
        // Taking the ghost down is an overlay change, and on the frame a pane
        // drag ends it is the *only* change there is — which `refresh_chrome`
        // reports, because it owns the overlay's rebuild too.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Route a press onto seat chrome. Returns whether the button was consumed.
    fn chrome_mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        // `BT_MOUSE_TRACE`: the chrome's own answer about this point, read once
        // and only when a trace is open. Reported at every exit that *takes* the
        // event, because "the chrome ate the release" and "which of its twenty
        // arms ate it" are two different findings and only the second one names
        // a place to look.
        let traced_target = mouse_trace::is_on().then(|| self.chrome_target_at(position));
        if button == MouseButton::Middle {
            if state == ElementState::Pressed
                && let Some(seats::ChromeTarget::Tab(index)) = self.chrome_target_at(position)
            {
                // Closing a tab with the wheel is not the first half of anything.
                self.window.tab_clicks.interrupt();
                self.close_tab(index)?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=middle-close-tab state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            let taken = matches!(
                self.chrome_target_at(position),
                Some(seats::ChromeTarget::Tab(_))
            );
            self.mouse_trace(|| format!("chrome_mouse_input taken={} at=middle-on-tab state={state:?} button={button:?} target={traced_target:?}", u8::from(taken)));
            return Ok(taken);
        }
        if button != MouseButton::Left {
            return Ok(false);
        }
        if state == ElementState::Released {
            // **A held scrubber or volume, first of everything** (route B slice
            // ②; §7.44 ②): the fraction it wrote on the way is already the
            // answer, so letting go only puts the dot away and restarts the
            // dwell that will take the bar off the glass.
            if let Some(surface) = self.window.video_bar_drag.take() {
                if let Some(seat) = self.window.video.get_mut(surface) {
                    seat.release(Instant::now());
                }
                self.refresh_chrome();
                self.present_chrome_change()?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-video-bar-track state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // A thumb is let go wherever the hand lets go of it — the offset it
            // wrote on the way is already the answer, so this only puts the
            // accent out. The body's bar first, the order everything else about
            // these two is in.
            if self.preview_body_drag.take().is_some() {
                self.note_preview_body_hover(Some(position))?;
                self.repaint_preview()?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-body-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // The terminal's own thumb, on the same terms: the offset it wrote
            // on the way is the answer, so letting go only settles which ink it
            // wears and restarts the clock that will take it off the glass.
            if let Some(drag) = self.terminal_thumb_drag.take() {
                self.wake_terminal_thumb(drag.seat);
                self.note_terminal_thumb_hover(Some(position))?;
                self.repaint_pane_change(drag.seat)?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-terminal-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // And the foot's, on those same terms.
            if let Some(drag) = self.terminal_column_drag.take() {
                self.wake_terminal_column(drag.seat);
                self.note_terminal_column_hover(Some(position))?;
                self.repaint_pane_change(drag.seat)?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-terminal-column-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            if self.preview_block_drag.take().is_some() {
                self.note_preview_block_hover(Some(position))?;
                self.repaint_preview()?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-block-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // A carried picture is let go wherever the hand lets go of it, and
            // the pan it wrote on the way is already the answer — this only puts
            // the closed hand away.
            if self.preview_image_drag.take().is_some() {
                self.apply_pointer_cursor();
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-image-drag state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // A selection drawn across the edit surface ends wherever the button
            // comes up. The release is consumed because the press was: a gesture
            // belongs to the surface it began on, whatever it is let go over.
            if self.preview_selecting.take().is_some() {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-selecting state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // **And one drawn across a rendered page**, on the same terms and
            // with one more decision to make: a press that never travelled was a
            // click, and a click is the link's or nobody's.
            if self.release_preview_text(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-text state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // Ahead of the press: a gesture that has become a drag answers with
            // its drop, and the press that started it is no longer a click.
            if self.release_drag()? {
                // A press that travelled is not half of a double click, and a
                // pane drag starts on the very head the zoom gesture lives on
                // (J99's rule, at this window's other double-click surface).
                self.window.pane_head_clicks.interrupt();
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-drag-drop state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            if self.window.divider_drag.take().is_some() {
                self.window.seat_pointer.dragging = None;
                self.apply_pointer_cursor();
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                // The end of a drag is a meaningful change (§5.1): the ratio that
                // was being explored is now the ratio the user chose.
                self.mark_session_dirty(Instant::now());
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-divider-drag state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            let target = self.chrome_target_at(position);
            // A pane press that never travelled has nothing to settle: D40 moved
            // the focus on the way down and that is all a press on a head has
            // ever meant. Dropping it is the whole of letting go.
            let pane_press = self.window.pane_press.take();
            let held_pane = pane_press.is_some() | self.window.row_press.take().is_some();
            // **A double-click on a pane head zooms it, and lets it go again**
            // (§7.1.6l, 2026-08-24). This is the seat §7.1.6b′ kept warm.
            //
            // The gesture carried focus mode for one day and was withdrawn on
            // 2026-08-19, on an argument that named its rightful owner in the
            // same breath: a double-click on a title bar means "make this thing
            // bigger" everywhere in this operating system, focus mode made the
            // pane *smaller*, and single-pane zoom is the verb whose shape this
            // is. So the gesture was left empty rather than repurposed, and the
            // layout primitives it needed (`bt-layout`'s `LayoutMode::Focus` /
            // `solve_focused`) were kept unused for exactly this.
            //
            // **Both clicks must land on the same pane's head**, and the pairing
            // is keyed by the seat rather than by a pixel neighbourhood, which is
            // this window's rule at its three other double-click surfaces: the
            // head a re-solve moved between the two clicks is still the same
            // head. A release on anything else breaks the chain (J99), which is
            // the `else` below and not a list of interrupts sprinkled through
            // the press arms — one place decides, so one place can be wrong.
            let doubled = match (pane_press, target) {
                (Some(press), Some(seats::ChromeTarget::PaneHeader(seat)))
                    if seat == press.seat =>
                {
                    self.window.pane_head_clicks.register(seat, Instant::now()) == TabClick::Double
                }
                _ => {
                    self.window.pane_head_clicks.interrupt();
                    false
                }
            };
            if doubled && let Some(press) = pane_press {
                self.toggle_pane_zoom(press.seat)?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-pane-head-zoom state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            if let Some(press) = self.window.tab_press.take() {
                self.release_tab_press(press, target)?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-tab-press state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            if held_pane {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-held-pane state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            let taken = target.is_some();
            self.mouse_trace(|| format!("chrome_mouse_input taken={} at=release-target-is-some state={state:?} button={button:?} target={target:?}", u8::from(taken)));
            return Ok(taken);
        }
        // **Asked again after a blur**, which is why it is not `let`: closing
        // the name editor can change the chrome the press was measured against.
        // The case that made it necessary is a new entry's box (0.3) — its row
        // is in the tree while the box is open and gone the moment the box
        // closes, so every row under it moves up one and a press below the box
        // would land on the row above the one under the pointer. The tab strip
        // and the preview head have a milder version of the same thing (a box
        // sized to its draft, a name that comes back), so the re-ask is
        // unconditional rather than a special case for the tree.
        let mut target = self.chrome_target_at(position);
        // **Whether the row this press named went with the name editor it
        // closed** (D4). Declared out here because the answer is reached inside
        // the rename guard and spent below the blur's other orderings.
        let mut row_gone = false;
        // **A press on a player, before P149 takes the card away** (route B
        // slice ②; §7.44 ②). Two gestures live here and both have to out-rank
        // the dismissal below: the play mark on a float's or a card's picture,
        // and every control on a bar that is up. A card dismissed first would be
        // a card whose own play mark could never be pressed — which is exactly
        // the *「能动的就动」* the ruling asked for, defeated by an ordering.
        if self.press_video_at(position)? {
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-video state={state:?} button={button:?} target={target:?}"));
            return Ok(true);
        }
        // **P149 — the glance is gone on press**, whatever the press turns out to
        // mean and before it means it. A card that survived the button going
        // down would be standing over the pane the press just opened.
        if self.hide_file_peek() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        // A press that lands on something other than a row leaves no row press
        // behind. The arms below that *are* rows arm their own, after this.
        self.window.row_press = None;
        // D40, above the router and consuming nothing: every press inside a pane
        // moves the layout focus there, whatever else the press goes on to mean.
        // Above the rename guard too — clicking into another pane is a blur, and
        // a blur that committed the editor still landed in that pane.
        self.focus_pane_at(position)?;
        // Blur commits, and blur is every press that is not inside the editor
        // (J102; mock-up 5898 `input.addEventListener("blur", () => finish(true))`,
        // which the browser fires on `pointerdown`, before the press does
        // anything else — so this guard stands above the whole router for the
        // same reason).
        //
        // **Ruling.** The editor's own extent is the *whole tab body*, not a
        // sub-box inside it. The mock-up stops propagation on the `<input>`
        // (5899) and the input is only part of the tab; but its central claim is
        // that "the editor is the tab" (376-378), and honouring that means the
        // tab's padding and its mark belong to the editor too. The alternative
        // is a strip of pixels inside the tab you are typing in where a click
        // silently commits, which is the kind of edge nobody discovers on
        // purpose. The `×` and the pin stay buttons — they are the two things in
        // the tab that were never the title.
        if let Some(editor) = self.window.rename.as_ref() {
            // **The editor's own extent**, whichever surface is holding it: the
            // tab body for a tab's name, the head's name run for a file's. A
            // press inside it is consumed whole — no drag armed, no click
            // recorded, no second entry — and a press anywhere else is a blur.
            let editing = match &editor.subject {
                RenameSubject::Tab(_) => self
                    .window
                    .rename
                    .as_ref()
                    .and_then(|editor| {
                        self.window
                            .tabs
                            .iter()
                            .position(|tab| Some(tab.id) == editor.tab())
                    })
                    .map(seats::ChromeTarget::Tab),
                // **This tab's head or no head** (§7.12 ⓑ): the editor lives on
                // the window and the chrome it is drawn in is the front tab's,
                // so a draft left open on a pane of another tab names no target
                // here — exactly as a float's head does not.
                RenameSubject::PreviewName {
                    surface: PreviewSurface::Seat(leaf),
                    ..
                } if leaf.tab == self.id => Some(seats::ChromeTarget::PreviewName(leaf.seat)),
                // A float's head is not chrome, so there is no target that can
                // be inside it and every press out here is a blur.
                RenameSubject::PreviewName { .. } => None,
                // **The address field is its own cell since 2026-08-24**, one
                // row under the name it used to be. The sentence is unchanged
                // and only the target moved: a press inside the field puts a
                // caret, and a press anywhere else — the page below very much
                // included — is a blur that commits.
                //
                // **And not while the page it is about has been torn off**
                // (§7.7 ⑩ 欠账, 2026-08-25). A floated page keeps the `LeafId`
                // of the seat it was popped out of, and that seat number can
                // still exist in this tab wearing something else entirely — so
                // an arm that asked only which tab the leaf named would hand
                // back a rectangle in the tree for a field standing in a window
                // over it. The name cell's arm above says the same thing about
                // the same window: a float is not this tab's chrome, and every
                // press out here is a blur.
                RenameSubject::WebAddress { leaf }
                    if leaf.tab == self.id && self.float_holding_the_page(*leaf).is_none() =>
                {
                    Some(seats::ChromeTarget::PreviewAddress(leaf.seat))
                }
                // A draft left open on a pane of another tab names no target in
                // this tab's chrome, which is the sentence the file name's own
                // arm makes two lines up.
                RenameSubject::WebAddress { .. } => None,
                // **B5 — the crumb's box is a segment of the rail** (user ruling
                // 2026-08-25). The same sentence the two above make: a press
                // inside it puts a caret, a press anywhere else is a blur that
                // commits, and a draft left open on another tab's pane names no
                // target in this tab's chrome.
                RenameSubject::PreviewCrumb {
                    surface: PreviewSurface::Seat(leaf),
                    ..
                } if leaf.tab == self.id => self.window.rename.as_ref().and_then(|_| {
                    let depth = self
                        .preview_rail_path(PreviewSurface::Seat(*leaf))
                        .map(|path| crumb_segments(&path).len())?
                        .checked_sub(1)?;
                    Some(seats::ChromeTarget::PreviewCrumb {
                        seat: leaf.seat,
                        depth,
                    })
                }),
                RenameSubject::PreviewCrumb { .. } => None,
                // **A tree row's box is the row** (B5). `ChromeTarget::FilesRow`
                // is keyed by the row's *index*, and the editor is keyed by the
                // row's stable key, so the two are matched by asking the live
                // tree where that key currently is — the tree can grow under an
                // open box, and an index remembered from the press would be a
                // different row by the time the next press lands.
                RenameSubject::FilesRow { leaf, key } if leaf.tab == self.id => {
                    let (seat, key) = (leaf.seat, key.clone());
                    self.files_trees(Instant::now())
                        .get(&seat)
                        .and_then(|tree| tree.rows.iter().position(|row| row.key == key))
                        .map(|index| seats::ChromeTarget::FilesRow { seat, index })
                }
                RenameSubject::FilesRow { .. } => None,
                // **And the box a new entry is being named in is a row too**
                // (0.3), found the same way and for the same reason — the tree
                // can grow underneath it, so where the pending row *is* has to
                // be asked of the tree that is on the glass rather than
                // remembered.
                RenameSubject::FilesNew { leaf, .. } if leaf.tab == self.id => {
                    let seat = leaf.seat;
                    self.files_trees(Instant::now())
                        .get(&seat)
                        .and_then(|tree| tree.edit.as_ref().map(|edit| edit.at))
                        .map(|index| seats::ChromeTarget::FilesRow { seat, index })
                }
                RenameSubject::FilesNew { .. } => None,
            };
            if editing.is_some() && target == editing {
                // "编辑器内的按下/双击不触发拖拽或再次进入编辑" (J103): the press
                // is consumed whole — no promise armed, no click recorded.
                self.window.tab_clicks.interrupt();
                self.window.preview_name_clicks.interrupt();
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-in-rename-editor state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // **The row is resolved to an identity before the editor is
            // finished, and dispatched to by that identity afterwards** (D4 of
            // the 2026-09-11 review).
            //
            // Re-asking the chrome after the blur is right — the list really is
            // a different list, because the pending row went with the box — but
            // re-asking it *at the same pixels* re-decides the reader's choice:
            // every row under the box moves up one row height, so the press
            // landed on the row that moved into the space, and `press_files_row`
            // selected or unfolded it. The same thing happens on a successful
            // blur-commit that sorts the new row elsewhere, and on a rename that
            // re-sorts.
            //
            // Re-deriving an *index* is necessary. Re-deriving the *choice* is
            // not, and this is the difference between the two.
            let pressed = pressed_row_identity(target, &self.files_tree_contents());
            self.finish_rename(RenameExit::Blur)?;
            match press_after_blur(pressed, &self.files_tree_contents()) {
                PressAfterBlur::AskAgain => target = self.chrome_target_at(position),
                PressAfterBlur::Row(seat, index) => {
                    target = Some(seats::ChromeTarget::FilesRow { seat, index });
                }
                // The row the reader pressed left with the editor. Nothing else
                // is entitled to the press — but it is **spent** below rather
                // than here, so that the blur's own two orderings still run: the
                // pane focus this press moved, and the page it took the keyboard
                // away from.
                PressAfterBlur::Gone => {
                    target = None;
                    row_gone = true;
                }
            }
        }
        // Blur is every press that is not inside the editor — the same sentence
        // the rename guard above makes, and the same reason: a surface that kept
        // the keyboard after you clicked somewhere else is a surface that eats
        // the next thing you type.
        // The surface that holds the keyboard keeps it only while the press is
        // inside *that* surface's body: a press in the preview beside it is a
        // press somewhere else, exactly as a press on the rail is.
        if let Some(focused) = self.preview_edit_focus()
            && self.preview_edit_body(position).map(|(surface, _)| surface) != Some(focused)
        {
            // **And the page it was standing in renders again** (owner's ruling
            // 2026-09-10, reversing T5 ③'s "leaving is `Esc` and nothing else").
            // See [`Self::leave_preview_page`]: this is the third of its three
            // gestures, and the caret stays where it was.
            self.leave_preview_page(focused);
            self.repaint_preview()?;
        }
        // **And now the press the closed editor took with it** (D4). The row it
        // named is not in the list any more; no row that moved into its place
        // inherits it, and neither does the terminal underneath.
        if row_gone {
            self.window.tab_clicks.interrupt();
            self.window.preview_name_clicks.interrupt();
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-row-gone-with-editor state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        let Some(target) = target else {
            // Not on chrome, but possibly not on a terminal either — a press in a
            // preview's body belongs to that seat and must not reach the grid
            // underneath it. Asked of the pane the press actually landed in, so
            // every terminal leaf answers for itself; see [`press_reaches_no_grid`]
            // for the primary-seat version this replaced and what it cost.
            self.window.tab_clicks.interrupt();
            // **The window's own drag handle, before anything under it** (M3-3,
            // owner ruling 2026-09-12, widened 2026-09-13 — §13.11 ⑥). The
            // handle is every pixel of the title bar that is not one of Folio's
            // own boxes, and picking the window up by it is what every title bar
            // on every platform does.
            //
            // It is answered here, in the arm for a press that named no chrome,
            // because that is precisely what it is: the complement of every box
            // this window draws up there. On Windows the press never arrives —
            // the frame answers `HTCAPTION` and the OS starts the move before
            // winit sees anything — so this arm is reached only where the
            // platform hands the press to the application, and the door it calls
            // refuses on the platform where it cannot be right.
            //
            // **A press and not only a drag.** What the door does with it is
            // the platform's business: one click picks the window up, two do
            // whatever the reader has asked a title bar's double click to do.
            //
            // Reported and not propagated: a press the platform would not act
            // on is a window that stayed still, which is worth a line and not a
            // killed press.
            if seats::title_bar_drag_point(
                self.window
                    .renderer
                    .presentation_geometry()
                    .swapchain_size
                    .0 as f32,
                self.window.renderer.metrics().scale_factor as f32,
                self.platform_chrome(),
                self.window.tabs.len(),
                self.window.tab_scroll,
                self.rail_posture(),
                self.is_quake_window(),
                position.x,
                position.y,
            ) {
                if let Err(reason) = self.window.custom_window_frame.press_title_bar() {
                    eprintln!("{reason}");
                }
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-title-bar state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // The body's own bar answers first of all: it is the outermost piece
            // of furniture the pane has, drawn over the document, over every
            // block in it and over every link in those — and a press on it was
            // never a press on what it is standing over.
            if self.press_preview_body_thumb(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-preview-body-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // **And a terminal pane's, on exactly those terms** (P2-9 slice 1).
            // The lane is the outermost band a terminal pane has; a press in it
            // was never a press on the cells it stands beside, and answering it
            // here — above `press_reaches_no_grid` and therefore above both the
            // selection drag and the child's own mouse tracking — is what makes
            // the two true at once: the pane does not select text while the
            // thumb is being dragged, and a full-screen mouse-reporting program
            // does not swallow the bar (§7.1.5f's own ordering: a target this
            // window recognises comes before the program's tracking).
            if self.press_terminal_thumb(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-terminal-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // **And the foot's mark, on the same terms with one clause more.**
            // Standing here buys the same two things it buys the lane above: no
            // selection begins under a drag of it, and a mouse-reporting program
            // does not swallow it. What it must not buy is the rest of the last
            // row, and it does not: the mark answers for the pixels it is drawn
            // on and only while it is drawn on them, so every press this declines
            // — every press on the reader's own prompt — falls through to the
            // grid exactly as it did before there was a bar down here.
            if self.press_terminal_column_thumb(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-terminal-column-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // The bar under a wide block is a scrollbar and answers first: it
            // stands over the block it scrolls, so a press on it was never a
            // press in the content beneath.
            if self.press_preview_block_thumb(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-preview-block-thumb state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // A picture answers before the edit surface does, and answers
            // instead of it: the two are alternatives on one body, and a body
            // showing a picture has nothing to put a caret in.
            if self.press_preview_image(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-preview-image state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // A press inside the edit surface puts the caret where the pointer
            // is and takes the keyboard, which is what a `<textarea>` does and
            // the only way into `InputOwner::PreviewEdit` that does not need a
            // chord.
            if self.press_preview_body(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-preview-body state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            // **And a press inside a rendered page draws a selection across it**
            // (user report 2026-08-28). Below the edit surface because the two
            // are alternatives on one body — a buffer shows its source or its
            // render, never both — and below the picture and the bars for their
            // own reason: everything above this is furniture standing over the
            // document, and a press on furniture was never a press on the words.
            //
            // A markdown **link** is answered from here now rather than from a
            // press of its own three arms up, because a link and the prose it
            // stands in share one button: see [`Self::open_preview_link`].
            if self.press_preview_text(position)? {
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-preview-text state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            return Ok(press_reaches_no_grid(
                &self.seat_layout,
                position.x,
                position.y,
                |seat| self.sessions.contains_key(&seat),
            ));
        };
        match target {
            seats::ChromeTarget::Divider(split) => {
                let Some(slot) = self
                    .seats
                    .split_slots(&self.seat_layout)
                    .into_iter()
                    .find(|slot| slot.id == split)
                else {
                    self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-divider-no-slot state={state:?} button={button:?} target={traced_target:?}"));
                    return Ok(true);
                };
                // **Which seam this is, decided once and from the tree** (F66).
                // A split with a bare fixed column on one side has no ratio worth
                // dragging — its width is pixels — so the grip is chosen here,
                // by the same reading `bt-layout::apply` will use to commit it.
                // Deciding per frame instead would let a mid-gesture re-solve
                // change the verb under the hand.
                let metrics = self.seat_metrics();
                let grip = match self.seats.fixed_column_of_split(&metrics, split) {
                    Some((column, leading)) => DividerGrip::FixedExtent {
                        leading,
                        // The width it is wearing right now, which is what Esc
                        // owes. A column that has never been dragged is sitting
                        // on its kind's opening width, and saying so explicitly
                        // is what makes the rollback total.
                        origin: self
                            .seats
                            .fixed_extent_of(column)
                            .unwrap_or(bt_layout::FILES_W),
                    },
                    None => {
                        let Some(origin) = self
                            .seats
                            .tree()
                            .ratios()
                            .into_iter()
                            .find_map(|(id, ratio)| (id == split).then_some(ratio))
                        else {
                            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-divider-no-ratio state={state:?} button={button:?} target={traced_target:?}"));
                            return Ok(true);
                        };
                        DividerGrip::Ratio(origin)
                    }
                };
                self.window.divider_drag = Some(DividerDrag {
                    split,
                    dir: slot.dir,
                    grip,
                    capture: bt_platform::thread_mouse_capture(),
                });
                self.window.seat_pointer.dragging = Some(split);
                self.apply_pointer_cursor();
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
            }
            seats::ChromeTarget::CollapseBar(seat) => {
                // §2.6.3: clicking a collapsed bar expands it, by promoting it
                // to the focus — W2 then makes it the last seat to fall, and
                // the concession chain gives it the room by itself. Keyboard
                // focus does not move; v1 keeps that on the terminal.
                //
                // §2.6.3 asks for three ways in and this is one of them. The
                // other two are recorded rather than approximated:
                //
                // * *Tab-reachable, Enter to expand.* There is no chrome focus
                //   ring in this build at all — `Tab` is forwarded to the PTY as
                //   a byte (`input.rs`), which is the correct behaviour while the
                //   terminal owns the keyboard. A ring is a window-wide decision
                //   about who owns `Tab` and in what order, not a thing one bar
                //   may invent for itself; it belongs with D45/O173, the
                //   still-open ruling on whether this product has *any* keyboard
                //   route between panes.
                // * *Selectable from the command palette.* There is no command
                //   palette. `Ctrl+Shift+P` is deliberately kept clear for it
                //   (see the dev-only chord below), and O166 already records that
                //   the palette has no pane entries of any kind to be consistent
                //   with.
                //
                // Both are keyboard reach, and a bar that can only be clicked is
                // exactly as reachable as every other pane in this build — which
                // is the honest statement of where the gap is: it is not the
                // collapsed bar's, it is the whole block's.
                if self.seats.set_focus(seat) {
                    self.apply_window_min_inner_size()?;
                    self.commit_seat_geometry()?;
                }
            }
            // D40's focus move is done for every press in the pane by
            // `focus_pane_at` above the router. What the head adds is J118: it is
            // the pane's handle, so the press arms the six pixels and waits.
            //
            // Nothing is consumed and nothing is shown. A press on a head that
            // never travels is a press that meant "put me in this pane", which
            // has already happened — so arming costs the click nothing, and that
            // is exactly the mock-up's shape (`pointerdown` → `startDrag`, with
            // the ordinary click handler left to run, 5835-5840).
            seats::ChromeTarget::PaneHeader(seat) => {
                self.window.tab_press = None;
                self.window.pane_press = Some(PanePress {
                    seat,
                    latch: DragLatch::new(position),
                });
            }
            // I102/I105: one verb for every kind of leaf. A files pane has no
            // session to clear, which is the whole of what `closeFilesPane =
            // closePane` means (mock-up 3579).
            seats::ChromeTarget::PaneClose(seat) => {
                self.window.tab_clicks.interrupt();
                self.close_pane(seat)?;
            }
            // H77: a click on the folder **tears off** a pinned window. The
            // press does not arm the head as a drag handle the way `FilesRoot`
            // does — a control that both opens a window and starts a pane drag
            // is one gesture with two meanings, and the mock-up returns out of
            // `pointerdown` on this button for exactly that reason (5870).
            seats::ChromeTarget::PaneFiles(seat) => {
                self.window.tab_clicks.interrupt();
                let leaf = LeafId {
                    tab: self.window.tabs[self.window.active_tab].id,
                    seat,
                };
                self.press_float_trigger(float::FloatTrigger::Pane(leaf))?;
            }
            seats::ChromeTarget::PaneFloat(seat) => {
                self.window.tab_clicks.interrupt();
                self.undock_files_column(seat)?;
            }
            // The `⌄` (user rulings, 2026-08-15 and 2026-08-16). Its own arm
            // rather than a sub-case of the header's, which is also what keeps
            // it out of the drag: this arm never touches `pane_press`, so the
            // six pixels of travel that would begin a tear-out are never armed
            // here.
            //
            // A **toggle**, which is the second half of the `⌄` grammar: a rest
            // of 250ms has usually already opened this menu by the time a hand
            // that travelled here presses, so a press that only ever opened
            // would be a press that did nothing. The strip's `⌄` has always
            // toggled; this now does too, through the same policy.
            seats::ChromeTarget::PaneMenu(seat) => {
                self.window.tab_clicks.interrupt();
                self.toggle_pane_menu(seat)?;
            }
            seats::ChromeTarget::FilesRow { seat, index } => {
                self.window.tab_clicks.interrupt();
                // P81, in the mock-up's own order: the row is armed as a drag
                // source *and* pressed. Arming first, because `press_files_row`
                // can unfold a directory and rebuild the list under the very
                // index this is about.
                self.arm_row_press(RowHost::Column(seat), index, position);
                self.press_files_row(seat, index)?;
            }
            // B24 — the strip is one button and pressing it hands the folder to
            // Explorer. It breaks a click chain for the same reason `.close` and
            // `.pin` do: a chain of clicks on a button is a chain of button
            // presses, not the beginning of a rename somewhere else.
            seats::ChromeTarget::FilesFoot(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.reveal_files_root(seat)?;
            }
            // The switch. A press on the half you are already on does nothing,
            // which falls out of `set_files_view` rather than being checked here.
            seats::ChromeTarget::FilesSeg { seat, view } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.set_files_view(seat, view)?;
            }
            // A Git row's body, which is a verb as of G-3: a changed file opens
            // its diff in the preview seat (mock-up 7974, `openDiffPreview`), a
            // commit turns its file list over (R15). It breaks a click chain for
            // `.files-foot`'s reason — this is a press on a control, not the
            // beginning of a gesture somewhere else.
            seats::ChromeTarget::GitRow { seat, index } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_git_row(seat, index)?;
            }
            seats::ChromeTarget::GitGraphRow { seat, index } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_graph_row(self.preview_here(seat), index)?;
            }
            // The toolbar's four controls (T1). A click chain is broken for
            // `.files-foot`'s reason: a chain of clicks on a button is a chain of
            // button presses and never the beginning of a gesture elsewhere.
            seats::ChromeTarget::GitGraphTool { seat, tool } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_graph_tool(self.preview_here(seat), tool)?;
            }
            // A part of the open row's detail block (v2 ②). It breaks a click
            // chain for `.files-foot`'s reason — a chain of clicks on a button is
            // a chain of button presses — and it never reaches `press_graph_row`,
            // because a press on a control is not a press on the list.
            seats::ChromeTarget::GitGraphDetail { seat, part, .. } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_graph_detail(self.preview_here(seat), part)?;
            }
            seats::ChromeTarget::GitAct { seat, index, act } => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.press_git_act(seat, index, act)?;
            }
            // The one way out of a file this window cannot show. It breaks a
            // click chain for `.files-foot`'s reason: a chain of clicks on a
            // button is a chain of button presses.
            seats::ChromeTarget::PreviewOpenButton(_) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.open_preview_externally()?;
            }
            // The three tools. Each breaks a click chain for `.files-foot`'s
            // reason — a chain of clicks on a button is a chain of button
            // presses — and none of them arms the head as a drag handle: the
            // mock-up returns out of `pointerdown` on `.pv-tool` (5874) exactly
            // as it does on `.pane-files`, and here that is expressed by simply
            // not arming.
            seats::ChromeTarget::PreviewSave(_) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.save_preview()?;
            }
            // **The seat this button is drawn on, and not whichever pane holds
            // the keyboard** (real-machine defect, 2026-08-26). It threw the id
            // away and asked `preview_keyboard_surface`, which was survivable
            // while `</>` lived on the head of a markdown pane — pressing a
            // pane's head is what gives that pane the keyboard, so the two
            // answers were the same one often enough. A page's row ends that:
            // pressing a button on a browser's address row does not move the
            // keyboard into that seat, so the flip landed on some other
            // surface's pane while the row that was pressed went on drawing
            // `</>`. Every other control on this row already asks
            // `preview_here(seat)` — see `PreviewBrowser` and `PreviewDevTools`
            // directly below — and this is that sentence finished.
            // **Stop, and put the frame back** (user ruling 2026-08-27; §7.23
            // ⑩). See [`Self::stop_video_on`].
            seats::ChromeTarget::PreviewStop(seat) => {
                self.stop_video_on(PreviewSurface::Seat(self.leaf_here(seat)))?;
            }
            seats::ChromeTarget::PreviewFlip(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Flip)?;
            }
            seats::ChromeTarget::PreviewBrowser(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Browser)?;
            }
            // A page's four, and the one verb on the card a failed page shows
            // (§7.7 ②, ④). Each goes through the verb the chord goes through, so
            // a button and a key are two doors onto one room.
            seats::ChromeTarget::PreviewBack(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Back)?;
            }
            seats::ChromeTarget::PreviewForward(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Forward)?;
            }
            seats::ChromeTarget::PreviewReload(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Reload)?;
            }
            // **Not one of the rail's**: the developer tools stayed on the head,
            // in the one verb slot a content class gets (§7.7 ②).
            seats::ChromeTarget::PreviewDevTools(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                let surface = self.preview_here(seat);
                self.run_web_head_verb(surface, WebHeadVerb::DevTools)?;
            }
            seats::ChromeTarget::PreviewFaultVerb(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.run_web_fault_verb(seat)?;
            }
            // **The download sheet's `×` never arrives here**, and the arm is
            // written to say so rather than to route it: the sheet claims every
            // press inside itself in [`Self::press_web_sheet`], which runs
            // above this router, and the target reaches the window only as a
            // hover. Routing it a second time from here would be the two-doors-
            // onto-one-press this module keeps out.
            seats::ChromeTarget::PreviewSheetClose(_) => {}
            seats::ChromeTarget::PreviewLock(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.toggle_preview_lock(seat)?;
            }
            seats::ChromeTarget::PreviewPopOut(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.pop_out_preview(seat)?;
            }
            seats::ChromeTarget::PreviewFoot(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                self.reveal_preview_file(seat)?;
            }
            // ── the rail (user ruling 2026-08-24) ───────────────────────────
            //
            // Each breaks a click chain for `.files-foot`'s reason — a chain of
            // clicks on a button is a chain of button presses — and none of them
            // arms a drag: this row is not a drag handle, so there is nothing to
            // decline.
            //
            // **A press on the address is a caret in it**, and the field it
            // opens is the very one `Ctrl+L` opens: `open_web_address_on` is the
            // one door, so a URL typed after a click and one typed after the
            // chord cannot be seeded differently.
            seats::ChromeTarget::PreviewAddress(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Address)?;
                self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-preview-address state={state:?} button={button:?} target={traced_target:?}"));
                return Ok(true);
            }
            seats::ChromeTarget::PreviewAddressCopy(seat)
            | seats::ChromeTarget::PreviewPathCopy(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Copy)?;
            }
            seats::ChromeTarget::PreviewCrumb { seat, depth } => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Crumb(depth))?;
            }
            seats::ChromeTarget::PreviewCrumbFold(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Fold)?;
            }
            seats::ChromeTarget::PreviewOpenWith(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::OpenWith)?;
            }
            seats::ChromeTarget::PreviewRail(seat) => {
                let surface = self.preview_here(seat);
                self.press_preview_rail(surface, seats::PreviewRailPart::Band)?;
            }
            // The name is a button **and** part of the drag handle, which is
            // `FilesRoot`'s own arrangement and the mock-up's: `.pv-name` is not
            // on the exclusion list at 5874. Arming costs the click nothing, and
            // not arming would make the one pane in the window whose head cannot
            // be grabbed by its name.
            seats::ChromeTarget::PreviewName(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                // **THE SECOND CLICK OF A PAIR OPENS THE EDITOR** (user ruling
                // 2026-08-19). The first has already opened the switcher on a
                // pane holding several files, which is why the editor closes it
                // on the way in — the mock-up's own note, and the reason the
                // pair is counted rather than inferred from what the first click
                // did.
                if self
                    .window
                    .preview_name_clicks
                    .register(seat, Instant::now())
                    == TabClick::Double
                {
                    self.close_preview_menu()?;
                    // **A page's title is not a door any more** (user ruling
                    // 2026-08-24). This cell used to be seeded two ways by one
                    // gesture — a file's name into the rename editor, a page's
                    // title into the address — on the argument that the cell
                    // *was* the address. The ruling retired that: the name is
                    // the document's title and the address has a row of its
                    // own, so there is nothing here for a second click on a page
                    // to open. A title is not editable, and offering to edit it
                    // would be offering to rename somebody else's document.
                    if !self.seat_holds_a_page(seat) {
                        self.open_preview_rename(seat)?;
                    }
                    self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-preview-name-double state={state:?} button={button:?} target={traced_target:?}"));
                    return Ok(true);
                }
                self.window.pane_press = Some(PanePress {
                    seat,
                    latch: DragLatch::new(position),
                });
                // **A single press opens the switcher only when there is one to
                // open.** The name answers the pointer on every head now (it is
                // how the editor is reached), and a pane holding one buffer has
                // nothing to switch to — a menu of one file is a menu about
                // nothing.
                if self.preview_head_tools(seat).switcher {
                    self.toggle_preview_menu(seat)?;
                }
            }
            seats::ChromeTarget::FilesRoot(seat) => {
                self.window.tab_clicks.interrupt();
                self.window.files_row_clicks.interrupt();
                // The head is still armed as a drag handle underneath — B17: the
                // press means "maybe I am about to move this pane", and the
                // click that never travels means "change this folder". Arming
                // costs the click nothing, and not arming would make the one
                // pane in the window whose head cannot be grabbed by its name.
                self.window.pane_press = Some(PanePress {
                    seat,
                    latch: DragLatch::new(position),
                });
                self.toggle_root_menu(seat)?;
            }
            seats::ChromeTarget::Tab(index) => self.press_tab(index, position)?,
            // J99: "`.close`/`.pin` 上的双击不算(那是两次按钮点击)". Neither
            // records a click, so neither can be half of a rename — and both
            // break a chain that was already running.
            seats::ChromeTarget::TabClose(index) => {
                self.window.tab_clicks.interrupt();
                self.close_tab(index)?;
            }
            // F61 — the pin stands in the `×`'s slot, so unpinning is exactly
            // where you already are.
            seats::ChromeTarget::TabPin(index) => {
                self.window.tab_clicks.interrupt();
                self.toggle_pin(index)?;
            }
            // **The speaker takes you to the sound and does not silence it**
            // (user ruling 2026-08-27; §7.23 ⑩). A mute here would be a
            // second, invisible piece of state — a tab that is playing and
            // muted looks exactly like a tab that is paused, and the only way
            // back is to find the same small glyph again. What the reader wants
            // when they hear something they did not expect is to *see* it, and
            // the controls that stop it are the ones under the video, where
            // they have been all along.
            //
            // `interrupt()` for the pin's own reason (J99): two presses on a
            // button in a tab's row are two button presses and never half a
            // rename.
            seats::ChromeTarget::TabSpeaker(index) => {
                self.window.tab_clicks.interrupt();
                self.activate_tab(index, false)?;
            }
            // **The play button, in the middle of a video's first frame**
            // (§7.23 ⑩) — see [`Self::play_video_on`], which is where every
            // reason it can decline is written down.
            seats::ChromeTarget::PreviewPlay(seat) => {
                self.play_video_on(PreviewSurface::Seat(self.leaf_here(seat)))?;
            }
            // H77: the click tears the peek off into a pinned window. **H78 is
            // the `interrupt()`** — the mock-up spells it out at 5800-5802:
            // "double-clicking the peek icon is two pin-toggles, not *and now
            // rename the tab*". It is the same rule `.close` and `.pin` above
            // are held to, and it holds here for the same reason: a chain of
            // clicks on a button is a chain of button presses, and rename lives
            // on the tab body alone.
            seats::ChromeTarget::TabFiles(index) => {
                self.window.tab_clicks.interrupt();
                let Some(id) = self.window.tabs.get(index).map(|tab| tab.id) else {
                    return Ok(false);
                };
                self.press_float_trigger(float::FloatTrigger::Tab(id))?;
            }
            seats::ChromeTarget::NewTab => {
                self.window.tab_clicks.interrupt();
                self.new_tab()?;
            }
            seats::ChromeTarget::NewTabMenu => self.toggle_profile_menu()?,
            // **The gear on the summoned terminal opens its own page** (§7.54e ⑤,
            // user ruling 2026-09-05: 「快捷终端窗上的齿轮直接打开这一栏」). Every
            // other window's gear opens `General`, which is the dialog's own
            // resting page; this window is the one surface in the product whose
            // reader is *inside the thing the page is about*, and a gear that
            // landed them on `General` would be asking them to go and find it.
            //
            // Through `open_settings_on_row`, which is the door the icon's menu
            // used and the door a keyboard walk uses: it opens the dialog if it is
            // shut, turns to the row's own category and scrolls the row into view.
            // A second way of opening this dialog on a page would be a second set
            // of rules about what "open the settings" means.
            seats::ChromeTarget::Settings => {
                if self.is_quake_window() {
                    self.open_settings_on_row(settings::SettingsRow::QuakeHotkey)?;
                } else {
                    self.toggle_settings_panel()?;
                }
            }
            seats::ChromeTarget::PanelToggle => self.toggle_rail_collapsed()?,
            seats::ChromeTarget::Minimize => self.window.window.set_minimized(true),
            seats::ChromeTarget::Maximize => {
                self.window
                    .window
                    .set_maximized(!self.window.window.is_maximized());
            }
            seats::ChromeTarget::CloseWindow => {
                let native = native_window(&self.window.window)?;
                bt_platform::request_window_close(native)
                    .map_err(|error| anyhow!(error))
                    .context("request self-drawn caption close")?;
            }
            // **The rail, on none of its controls.** Nothing happens, and the
            // `Ok(true)` below is the whole point of the arm: the press was
            // *taken*, so it does not fall through to a pane the rail is drawn
            // on top of. See [`seats::ChromeTarget::RailBody`].
            seats::ChromeTarget::RailBody => {}
        }
        self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=press-routed state={state:?} button={button:?} target={target:?}"));
        Ok(true)
    }

    /// **One line into `BT_MOUSE_TRACE`, stamped with this window** — the door
    /// every station of a click's road writes through.
    ///
    /// Behaviourally inert by construction: it takes `&self`, it returns nothing,
    /// and the closure it is handed is never called when the gate is shut (see
    /// [`mouse_trace`]). The window id is part of the prefix rather than of every
    /// caller's format string because a two-window session interleaves two
    /// gestures into one file and there would otherwise be no way to unpick them.
    pub(crate) fn mouse_trace(&self, message: impl FnOnce() -> String) {
        let Some(trace) = mouse_trace::global() else {
            return;
        };
        let window = u64::from(self.window.window.id());
        mouse_trace::emit(Some(trace), || format!("window={window} {}", message()));
    }

    /// What the mouse route is, for a trace line that must not need `Debug` on a
    /// boxed selection drag. The kind is the whole of what the forensics ask:
    /// "was a route armed at all, and is it still there when the button comes
    /// up".
    fn mouse_route_name(&self) -> &'static str {
        match self.window.mouse_route {
            None => "none",
            Some(MouseRoute::Local(_)) => "local",
            Some(MouseRoute::Forward { .. }) => "forward",
            Some(MouseRoute::MathBlock) => "math",
        }
    }

    pub(crate) fn mouse_input(&mut self, state: ElementState, button: MouseButton) -> Result<()> {
        // The road's first station (`BT_MOUSE_TRACE`). Everything a cross-monitor
        // report needs to be settled about the *frame* the click landed in is
        // read here, once, before any router has had the chance to move it: the
        // pointer in physical pixels, the scale the metrics believe, and the two
        // sizes that must agree with each other for a chrome hit rectangle to sit
        // where it is drawn.
        self.mouse_trace(|| {
            let presentation = self.window.renderer.presentation_geometry();
            let inner = self.window.window.inner_size();
            let pointer = self
                .window
                .pointer_position
                .map_or_else(|| "none".to_owned(), |at| format!("{},{}", at.x, at.y));
            format!(
                "mouse_input state={state:?} button={button:?} pointer={pointer} \
                 metrics_scale={} swapchain_size={}x{} inner_size={}x{} route={}",
                self.window.renderer.metrics().scale_factor,
                presentation.swapchain_size.0,
                presentation.swapchain_size.1,
                inner.width,
                inner.height,
                self.mouse_route_name(),
            )
        });
        // **On a Mac, Control+click is the secondary click** (§13.45 ②), and it
        // is made into one here — above every router, below the station that
        // records what the platform actually said. AppKit does not do it for us:
        // a control-click arrives as `mouseDown:` with `buttonNumber` 0, so
        // winit reports a plain left press and the desk's oldest gesture would
        // otherwise begin a selection. One translation at the one door every
        // button event comes through, so that the two ways a Mac makes a
        // secondary press are one press from here on and this window's rule for
        // it is written down once (`right_press_raises_terminal_menu`).
        //
        // Off macOS this is the identity and nothing below it can tell.
        let reported = button;
        let button = input::pressed_button_of_gesture(
            &mut self.window.secondary_press,
            button,
            state,
            self.window.modifiers_held,
            bt_platform::host_platform(),
        );
        if button != reported {
            self.mouse_trace(|| {
                format!("secondary_click state={state:?} reported={reported:?} taken_as={button:?}")
            });
        }
        // M142, and ahead of everything: any press at all takes the tip down.
        // Unconditional — not "a press that hits something", not "a left press" —
        // because a tooltip answers "what is this?" and the act of pressing is
        // you saying you already know. The mock-up's listener is the document's
        // for the same reason.
        if state == ElementState::Pressed {
            // **And a press spends a raised hint card** (§7.1.5e′, user ruling
            // 2026-08-25) — the same sentence one line further out: the card
            // asks whether the hand has forgotten something, and a hand that is
            // pressing a button has answered. The press only: a button coming
            // back up is the end of an answer already given, and the modifiers
            // are very often still down under it.
            self.spend_key_hint()?;
            self.hide_tooltip()?;
            // L135 sends the peek the same way and for the same reason: it is a
            // glance, and pressing is you saying you are done glancing.
            self.hide_layout_peek()?;
            // **A press outside the capsule hands the keyboard back — and leaves
            // the capsule up** (§7.1.5d). Those are two separate facts and both
            // are ruled: the field losing focus is what every text box on this
            // platform does when you click elsewhere, and the capsule *staying*
            // is what makes search "a staying state, not a popup" (B80). What
            // you get is B81's second stance, arrived at with the mouse instead
            // of with `F3`.
            //
            // Here rather than beside the capsule's own press arm, because the
            // router returns early for every surface above that arm: a click on
            // the tab strip has to hand the caret back too, and it never reaches
            // the level the capsule is answered at.
            if self.window.search.is_focused()
                && !self
                    .window
                    .search_layout
                    .zip(self.window.pointer_position)
                    .is_some_and(|(capsule, at)| {
                        search::hit(&capsule, at.x as f32, at.y as f32).is_some()
                    })
            {
                self.window.search.blur();
                self.after_search_change()?;
            }
        }
        // The quit card first, in the order it is drawn: every press is
        // swallowed, its own scrim included, and the answer is the application's
        // whichever window it was pressed in.
        if let (Some(layout), Some(position)) =
            (self.quit_card_layout(), self.window.pointer_position)
        {
            if state == ElementState::Pressed && button == MouseButton::Left {
                let target = restore::quit_hit(&layout, position.x, position.y);
                if let Some(answer) = restore::quit_answer(target) {
                    self.answer_quit_card(answer)?;
                }
            }
            return Ok(());
        }
        // The gate next, and it is the strictest modal the *window* has: every
        // press is swallowed, including the ones that land on its own scrim,
        // because the action it is standing in front of is already under way.
        if let (Some(layout), Some(position)) =
            (self.dirty_gate_layout(), self.window.pointer_position)
        {
            if state == ElementState::Pressed && button == MouseButton::Left {
                let target = restore::gate_hit(&layout, position.x, position.y);
                if let Some(answer) = restore::gate_answer(target) {
                    self.answer_dirty_gate(answer)?;
                }
            }
            return Ok(());
        }
        // The first-run card, in the order it is drawn. Every press is
        // swallowed, its own scrim included; a press on the face answers
        // nothing, and a press on a switch is not an answer to the card, only to
        // that row.
        if let (Some(layout), Some(position)) =
            (self.first_run_layout(), self.window.pointer_position)
        {
            if state == ElementState::Pressed && button == MouseButton::Left {
                let target = first_run::hit(&layout, position.x, position.y);
                self.answer_first_run(target)?;
            }
            return Ok(());
        }
        // The invitation, in the order it is drawn. Every press is swallowed,
        // its own scrim included, and a press on a disabled Install lands on
        // `Panel` and answers nothing.
        if let (Some(layout), Some(position)) = (
            self.psreadline_invite_layout(),
            self.window.pointer_position,
        ) {
            if state == ElementState::Pressed && button == MouseButton::Left {
                let target = restore::invite_hit(&layout, position.x, position.y);
                self.answer_psreadline_invite(target)?;
            }
            return Ok(());
        }
        // **A notice takes its own presses** (user ruling, 2026-08-16), and it
        // takes them **before the modal family** (§7.1.6c-6b, 2026-08-18).
        //
        // The 2026-08-16 ordering put the cards after the modal on one premise —
        // "the two surfaces that are painted over it are surfaces that cannot be
        // open while it is" — and slice 5b makes that premise false: deleting a
        // profile is a verb *of* the settings dialog, and the card it raises
        // carries the only way back. A card painted over the scrim, showing a
        // verb, that answers no press would be worse than the thing the modal
        // rule was protecting: the scrim exists so that nothing *behind* it
        // answers, and a card is not behind it.
        //
        // Nothing else moves. The scrim still swallows every press that lands on
        // it, the caption run included; what changed is that a surface drawn
        // above the scrim is now hit-tested above it too, which is the two
        // saying one thing rather than two.
        if let Some(position) = self.window.pointer_position
            && state == ElementState::Pressed
            && self.press_toast(position)?
        {
            return Ok(());
        }
        // A modal means MODAL. Ahead of the chrome router, so the caption run —
        // the gear included — is behind the scrim like everything else, and no
        // press reaches a divider, a seat, the terminal's selection or a peek.
        if let (Some(layout), Some(position)) =
            (self.settings_layout(), self.window.pointer_position)
        {
            return self.settings_mouse_input(&layout, state, button, position);
        }
        // The prompt takes the press only where it is drawn — it is a prompt over
        // a working app, not a gate in front of one, so a press anywhere else is
        // still the press it always was and reaches the terminal underneath.
        if let (Some(position), Some(layout)) =
            (self.window.pointer_position, self.restore_layout())
            && let Some(target) = restore::hit(&layout, position.x, position.y)
        {
            if state == ElementState::Pressed
                && button == MouseButton::Left
                && let Some(answer) = restore::answer(target)
            {
                self.answer_restore_prompt(answer)?;
            }
            return Ok(());
        }
        // The git context menu, on the file menu's own level and answered by the
        // same three rules — a row runs it, a press outside puts it away and
        // then goes on being the press it was, which is how a second right press
        // moves it from one row to another.
        if let (Some(layout), Some(position)) =
            (self.git_menu_layout(), self.window.pointer_position)
        {
            match profiles::git_menu_hit(&layout, position.x, position.y) {
                Some(row) => {
                    if state == ElementState::Pressed
                        && button == MouseButton::Left
                        && let Some(row) = row
                    {
                        self.run_git_menu_row(row)?;
                    }
                    return Ok(());
                }
                None => {
                    if state == ElementState::Pressed {
                        self.close_git_menu()?;
                    }
                }
            }
        }
        // The terminal's own menu, on the git menu's level and by its three
        // rules — a row runs it, its padding swallows, and a press outside puts
        // it away and then goes on being the press it was, which is how a second
        // right press moves it from one pane to another.
        if let (Some(layout), Some(position)) =
            (self.term_menu_layout(), self.window.pointer_position)
        {
            match profiles::term_menu_hit(&layout, position.x, position.y) {
                Some(hit) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        // The `Split with` heading is the one entry whose press
                        // is not a verb: it opens the child list, which is the
                        // `→` key's job through the same door.
                        if matches!(hit, profiles::TermMenuHit::Row(entry) if entry.has_submenu()) {
                            let open = self
                                .window
                                .term_menu
                                .as_ref()
                                .is_some_and(|menu| menu.submenu_open);
                            self.set_term_submenu(!open)?;
                        } else {
                            self.run_term_menu_row(hit)?;
                        }
                    }
                    return Ok(());
                }
                None => {
                    if state == ElementState::Pressed {
                        self.close_term_menu()?;
                    }
                }
            }
        }
        // The file menu, above the float and above the other two popups, for the
        // reason `refresh_overlay` gives: it is drawn over the floating window
        // because it is very often *about a row inside it*, and a press has to
        // reach whatever is drawn on top.
        if let (Some(layout), Some(position)) =
            (self.file_menu_layout(), self.window.pointer_position)
        {
            match profiles::file_menu_hit(&layout, position.x, position.y) {
                Some(row) => {
                    if state == ElementState::Pressed
                        && button == MouseButton::Left
                        && let Some(row) = row
                    {
                        self.run_file_menu_row(row)?;
                    }
                    return Ok(());
                }
                None => {
                    // A press outside puts it away and then goes on being the
                    // press it always was — including a *second right press*,
                    // which is how a context menu is moved from one row to
                    // another everywhere else, and which the raise below then
                    // completes.
                    //
                    // **Unless it landed on the control that raised it**
                    // (owner's report 2026-09-13), in which case putting it away
                    // is the whole of what the press does. This is the menu the
                    // preview rail's `Open ⌄` pill and its breadcrumb's `…` chip
                    // both raise, and until the rule was written neither could be
                    // shut by pressing it again: the arm closed the menu and the
                    // same press went on to the button, which opened it back up.
                    // See [`Self::press_on_its_own_trigger`].
                    if state == ElementState::Pressed {
                        let own = self.press_on_its_own_trigger(Popup::File, position);
                        if own.dismisses() {
                            self.close_file_menu()?;
                        }
                        if own == OwnPress::Spent {
                            return Ok(());
                        }
                    }
                }
            }
        }
        // The pane head's menu, on the file menu's level and by its three rules:
        // a row runs on a left press, the menu's own padding swallows, and a
        // press outside puts it away and then goes on being the press it always
        // was — including a second right press, which is how a context menu is
        // moved from one head to another.
        //
        // **Except the `⌄` itself**, which is not "outside": a press there is
        // this menu's close and is spent being it — the one rule every triggered
        // popover in this window is under since 2026-09-13. It used to be spelled
        // here as "leave the press alone and let the button toggle for itself",
        // which said the same thing about this menu only, and said nothing at all
        // about the two menus that had no toggle to fall back on.
        if let (Some(layout), Some(position)) =
            (self.pane_menu_layout(), self.window.pointer_position)
        {
            match profiles::pane_menu_hit(&layout, position.x, position.y) {
                Some(hit) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        // The submenu heading is the one entry whose press is not
                        // a verb: it opens the child menu, which is the `→` key's
                        // job through the same door.
                        if let profiles::PaneMenuHit::Row(row) = hit
                            && row.has_submenu()
                        {
                            // **A press on a heading toggles that heading's own
                            // list** (B9), which is the same sentence it always
                            // made and no longer the same as "toggle the child":
                            // pressing `Move to window` while `Split with` is
                            // open opens the one you pressed rather than closing
                            // the one you did not.
                            let open = self.window.pane_menu.as_ref().and_then(|menu| menu.submenu);
                            self.set_pane_submenu((open != Some(row)).then_some(row))?;
                        } else {
                            self.run_pane_menu_row(hit)?;
                        }
                    }
                    return Ok(());
                }
                None => {
                    if state == ElementState::Pressed {
                        let own = self.press_on_its_own_trigger(Popup::Pane, position);
                        if own.dismisses() {
                            self.window.chevrons.clear();
                            self.close_pane_menu()?;
                        }
                        if own == OwnPress::Spent {
                            return Ok(());
                        }
                    }
                }
            }
        }
        // A tab's own menu (丙2), by the pane menu's three rules with none of
        // its exceptions: a row runs on a left press, the menu's own padding
        // swallows, and a press outside puts it away and then goes on being the
        // press it always was — including a second right press, which is how a
        // context menu is moved from one tab to another.
        //
        // There is no opener to spare here the way the pane menu spares its `⌄`:
        // this menu has no button, only a gesture, so every press that misses it
        // really is outside.
        if let (Some(layout), Some(position)) =
            (self.tab_menu_layout(), self.window.pointer_position)
        {
            match profiles::tab_menu_hit(&layout, position.x, position.y) {
                Some(hit) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        // The submenu heading is the one entry whose press is not
                        // a verb: it opens the window list, which is the `→`
                        // key's job through the same door.
                        if matches!(hit, profiles::TabMenuHit::Row(row) if row.has_submenu()) {
                            let open = self
                                .window
                                .tab_menu
                                .as_ref()
                                .is_some_and(|menu| menu.submenu_open);
                            self.set_tab_submenu(!open)?;
                        } else {
                            self.run_tab_menu_row(hit)?;
                        }
                    }
                    return Ok(());
                }
                None => {
                    if state == ElementState::Pressed {
                        self.close_tab_menu()?;
                    }
                }
            }
        }
        // **The palette** (DESIGN.md §7.55), by the same three rules a fifth
        // time: a row runs on a left press, the box's own padding swallows, and
        // a press outside puts it away and then goes on being the press it
        // always was. The mock-up's `pointerdown` handler outside `.palette` is
        // exactly this third rule, and its `click` on a row is the first.
        //
        // The layout is the one the last frame drew rather than one measured
        // here, because this router is `&self` — [`WindowRuntime::palette_layout`].
        if let (Some(layout), Some(position)) = (
            self.window.palette_layout.clone(),
            self.window.pointer_position,
        ) {
            match palette::hit(&layout, position.x, position.y) {
                Some(Some(index)) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        if let Some(palette) = self.window.palette.as_mut() {
                            palette.point_at(index);
                        }
                        self.run_palette_row()?;
                    }
                    return Ok(());
                }
                Some(None) => {
                    if state == ElementState::Pressed {
                        return Ok(());
                    }
                }
                None => {
                    if state == ElementState::Pressed {
                        self.close_command_palette()?;
                    }
                }
            }
        }
        // The graph's branch filter, by the same three rules, with one change:
        // a row does *not* put it away. It is a list of settings and picking two
        // branches means picking one and then the other — see
        // `run_graph_filter_row`.
        if let (Some(layout), Some(position)) = (
            self.graph_filter_menu_layout(),
            self.window.pointer_position,
        ) {
            match profiles::git_filter_menu_hit(&layout, position.x, position.y) {
                Some(row) => {
                    if state == ElementState::Pressed
                        && button == MouseButton::Left
                        && let Some(row) = row
                    {
                        self.run_graph_filter_row(&row)?;
                    }
                    return Ok(());
                }
                None => {
                    // **A press on the button that opened it is not an outside
                    // press** (user report, 2026-08-19). Without it the second
                    // press was two acts in one turn of the loop: this line put
                    // the menu away, and the same press then reached the toolbar
                    // and found no menu open to toggle — so `All branches`
                    // re-opened under the pointer every time it was pressed and
                    // could not be shut by pressing it again.
                    //
                    // The 2026-08-19 fix spared the press instead of spending it,
                    // and named the button by [`seats::ChromeTarget`] — which a
                    // graph drawn in a torn-off window has none of, so the report
                    // stayed true there for another three weeks. The rule is the
                    // window's now and names the tool on either host.
                    if state == ElementState::Pressed {
                        let own = self.press_on_its_own_trigger(Popup::GraphFilter, position);
                        if own.dismisses() {
                            self.close_graph_filter_menu()?;
                        }
                        if own == OwnPress::Spent {
                            return Ok(());
                        }
                    }
                }
            }
        }
        // **The glance card takes its own presses** (user ruling, 2026-08-14),
        // above the float and below the menus — the order it is drawn in.
        //
        // A release is handed on rather than claimed: letting go of a thumb ends
        // the drag, and the same release still has to reach whatever else was
        // waiting for one.
        if state == ElementState::Released {
            self.release_file_peek_thumb()?;
            // The head's other meaning, and it *is* claimed: the press was
            // consumed on the way down so that the hand could still choose, and
            // the release is where the choice is spent.
            if self.release_file_peek_press()? {
                return Ok(());
            }
        } else if self.press_file_peek(button)? {
            return Ok(());
        }
        // **The right press that raises a tab's own menu** (丙2, the gesture
        // audit of 2026-08-26).
        //
        // First of the right-press openers, because the tab list is the first
        // thing `chrome_target_at` asks and for that hit test's own reason
        // (Q179): an open icon rail lies *over* the panes, so a right press on a
        // rail row is over a files column as often as not, and an opener that
        // let the column answer first would raise a file row's menu on a press
        // that landed on a tab.
        //
        // **The target reaches here from all three tab surfaces** — the
        // horizontal strip, the vertical rail and focus mode's column of cards —
        // because all three answer `ChromeTarget::Tab(index)` through the one
        // `chrome_target_at`, which is what makes "right-clicking a tab" one
        // sentence rather than three handlers.
        //
        // **And it moves nothing.** The chrome router below answers only the
        // left button, so this press neither focuses nor activates the tab: the
        // menu's subject is the tab that was pointed at, whichever tab is in
        // front.
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && let Some(position) = self.window.pointer_position
            && let Some(seats::ChromeTarget::Tab(index)) = self.chrome_target_at(position)
            && let Some(tab) = self.window.tabs.get(index).map(|tab| tab.id)
        {
            self.open_tab_menu_at(tab, position)?;
            return Ok(());
        }
        // The right press that raises it (K143/K146). Both hosts, float first
        // because the float is drawn over the columns.
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && let Some(position) = self.window.pointer_position
            && let Some(target) = self.file_row_under(position)
        {
            self.open_file_menu(target, [position.x as f32, position.y as f32])?;
            return Ok(());
        }
        // The right press that raises the pane head's menu (user ruling,
        // 2026-08-15). Below the file rows because a files column's head sits
        // over its own rows and the rows are the more specific thing; above the
        // chrome router below because that router answers only the left button
        // and would otherwise let this press fall all the way through to the
        // shell's own mouse protocol.
        //
        // Every part of the head, its buttons included. A right click on the `×`
        // is not a request to close — the head's whole bar is one context, and
        // an 18-pixel hole in it that silently means something else is exactly
        // the kind of edge a hand finds by accident.
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && let Some(position) = self.window.pointer_position
            && let Some(
                seats::ChromeTarget::PaneHeader(seat)
                | seats::ChromeTarget::PaneClose(seat)
                | seats::ChromeTarget::PaneFiles(seat)
                | seats::ChromeTarget::PaneMenu(seat),
            ) = self.chrome_target_at(position)
        {
            self.open_pane_menu(seat, [position.x as f32, position.y as f32])?;
            return Ok(());
        }
        // The right press that raises a repository row's own menu (v2 ④). Below
        // the two above it because both of those are more specific surfaces
        // standing over the same panes, and above the chrome router below for
        // the pane head's reason: that router answers only the left button and
        // would let this press fall all the way through to the shell.
        //
        // A press on a row with no verbs — a heading, a detail block, the
        // working tree with nothing to compare against — raises nothing and is
        // *not* consumed, so it goes on meaning what it meant before.
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && let Some(position) = self.window.pointer_position
            && self.open_git_menu_at(position)?
        {
            return Ok(());
        }
        // A peek header press that never travelled stops waiting here, and means
        // nothing — rule ① of the 2026-08-12 ruling. It is cleared rather than
        // answered, and the release is deliberately *not* consumed: "pressed and
        // let go without moving" is the absence of a gesture, so whatever that
        // release meant before this feature existed it still means.
        //
        // Ahead of the arming below rather than after it, so the press that arms
        // one is not the event that throws it away.
        self.window.float_head_press = None;
        // A release ends whatever the float was doing, wherever it lands: a
        // gesture that began on the header can finish anywhere, and a window that
        // kept following the pointer after the button came up would be a window
        // stuck to the hand.
        if state == ElementState::Released && self.window.float_drag.take().is_some() {
            // The hand opens onto whatever the release actually left it over,
            // which has to be *asked* rather than assumed: a drag owns the
            // pointer, so the hover underneath is as old as the gesture, and a
            // grip pulled past the 200×150 floor leaves its corner behind — the
            // part the pull began on is exactly the part no longer under the
            // hand. Without this the resize arrow would outlive the resize,
            // until some later move happened to correct it.
            if let Some(position) = self.window.pointer_position {
                self.drive_float_hover(position)?;
            }
            self.apply_pointer_cursor();
            return Ok(());
        }
        // **A float carrying a page has no body of its own to press** (§7.14a).
        // Its head, its grip and its `DOCK` still answer for themselves — those
        // are the window — but the rectangle between them *is* the page, exactly
        // as a docked pane's body is, and a chassis that took the press there
        // would be answering for a document that is not in it.
        //
        // Above `press_float` and nowhere else. A **docked** page that a
        // floating window happens to cover is still covered by it, and the float
        // rightly wins there; what is claimed here is only the page the float is
        // itself carrying.
        if let Some(position) = self.window.pointer_position
            && let Some(leaf) = self.web_page_at(position)
            && self.float_holding_the_page(leaf).is_some()
        {
            self.press_web_page(state, button, position)?;
            return Ok(());
        }
        // The float takes the press where it is drawn, above the layout and below
        // the modals. It is not in the menus' mutually exclusive chain — a menu is
        // dismissed by a press outside it and this deliberately is not (§7.1.2:
        // click-away does not close a pinned window; you click into the terminal
        // to work beside it) — so a press that misses it simply goes on being the
        // press it always was.
        if state == ElementState::Pressed
            && button == MouseButton::Left
            && let Some(position) = self.window.pointer_position
            && self.press_float(position)?
        {
            return Ok(());
        }
        // The picker takes the press only where it is drawn. A press on a row
        // starts that profile's tab; a press on the menu's own padding is the
        // menu's and does nothing; a press anywhere else puts it away and then
        // goes on to be the press it always was.
        if let (Some(layout), Some(position)) =
            (self.profile_menu_layout(), self.window.pointer_position)
        {
            match profiles::hit(
                &layout,
                &self.app.profile_programs,
                self.app.recent.entries(),
                position.x,
                position.y,
            ) {
                Some(row) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        self.close_profile_menu()?;
                        // The row says which door it is. An untagged index here
                        // would have started PowerShell every time somebody
                        // clicked a Recent entry — the same row number means two
                        // different things in two different sections.
                        match row {
                            Some(profiles::MenuRow::Profile(index)) => {
                                // The row's id, taken on the frame the row was
                                // drawn from: the press is the last moment this
                                // position is certainly that profile.
                                self.new_tab_with_profile(&profiles::id(index), None)?;
                            }
                            Some(profiles::MenuRow::Recent(index)) => {
                                self.reopen_recent(index)?;
                            }
                            // The mouse door onto the files column. Every row
                            // above makes a tab; this one gives the tab you are
                            // in a pane, through the same verb `Ctrl+Shift+B`
                            // reaches.
                            Some(profiles::MenuRow::FilesPane) => {
                                self.toggle_files_pane()?;
                            }
                            // The picker's own `close_profile_menu` above is
                            // what §7.1.6e asks for here: the menu is already
                            // shut before a system modal goes up over it.
                            Some(profiles::MenuRow::NewInFolder) => {
                                self.browse_for_new_tab_root();
                            }
                            None => {}
                        }
                    }
                    return Ok(());
                }
                None => {
                    if state == ElementState::Pressed {
                        let own = self.press_on_its_own_trigger(Popup::Profile, position);
                        if own.dismisses() {
                            self.close_profile_menu()?;
                        }
                        if own == OwnPress::Spent {
                            return Ok(());
                        }
                    }
                }
            }
        }
        // The root menu, on exactly the terms the picker above takes its press.
        if let (Some(layout), Some(position)) =
            (self.root_menu_layout(), self.window.pointer_position)
        {
            let choices = self
                .window
                .root_menu
                .seat()
                .map(|seat| self.root_choices(seat))
                .unwrap_or_default();
            match profiles::root_menu_hit(&layout, position.x, position.y) {
                Some(row) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        let seat = self.window.root_menu.seat();
                        // **The pin does not close the menu** (user ruling
                        // 2026-08-19). Every other press here is an answer and
                        // an answer puts the question away; a pin is a change to
                        // the list you are looking at, and the row you just
                        // kept has to be seen arriving at the top of it.
                        match (seat, row) {
                            (
                                _,
                                Some(profiles::RootMenuHit::Pin(profiles::RootMenuRow::Choice(
                                    index,
                                ))),
                            ) => {
                                if let Some(choice) = choices.get(index) {
                                    let path = choice.path.clone();
                                    self.toggle_folder_pin(&path)?;
                                }
                            }
                            (
                                Some(seat),
                                Some(profiles::RootMenuHit::Row(profiles::RootMenuRow::Choice(
                                    index,
                                ))),
                            ) => {
                                self.close_root_menu()?;
                                if let Some(choice) = choices.get(index) {
                                    let path = choice.path.clone();
                                    self.reroot_files_column(seat, &path)?;
                                }
                            }
                            (
                                Some(seat),
                                Some(profiles::RootMenuHit::Row(profiles::RootMenuRow::Browse)),
                            ) => {
                                self.close_root_menu()?;
                                self.browse_for_root(seat);
                            }
                            _ => {
                                self.close_root_menu()?;
                            }
                        }
                    }
                    return Ok(());
                }
                None => {
                    // A press outside puts it away and then goes on to be the
                    // press it always was — except on the button that opened it,
                    // where putting it away is all the press does and it would
                    // otherwise be shut here and re-opened one line later.
                    if state == ElementState::Pressed {
                        let own = self.press_on_its_own_trigger(Popup::Root, position);
                        if own.dismisses() {
                            self.close_root_menu()?;
                        }
                        if own == OwnPress::Spent {
                            return Ok(());
                        }
                    }
                }
            }
        }
        // The preview's switcher, on exactly the terms the two menus above take
        // their press. Its "except the button that opened it" is the name (P136),
        // which would otherwise be shut here and re-opened one line later — and
        // which is also half of a double click, so the press is spent on the
        // close and the chain the name keeps is left alone.
        if let Some(seat) = self.preview_menu_seat()
            && let (Some(layout), Some(position)) =
                (self.preview_menu_layout(), self.window.pointer_position)
        {
            let items = self.preview_menu_items(seat);
            match profiles::preview_menu_hit(&layout, &items, position.x, position.y) {
                Some(row) => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        match row {
                            // The root menu's rule, one menu over: a pin changes
                            // the list you are looking at and leaves it up.
                            Some(profiles::PreviewMenuHit::Pin(index)) => {
                                if let Some(keep) =
                                    items.get(index).and_then(|item| item.keep.clone())
                                {
                                    self.toggle_switcher_pin(seat, &keep)?;
                                }
                            }
                            Some(profiles::PreviewMenuHit::Row(index)) => {
                                self.choose_preview_row(seat, index)?;
                            }
                            None => {
                                self.close_preview_menu()?;
                            }
                        }
                    }
                    return Ok(());
                }
                None => {
                    if state == ElementState::Pressed {
                        let own = self.press_on_its_own_trigger(Popup::Preview, position);
                        if own.dismisses() {
                            self.close_preview_menu()?;
                        }
                        if own == OwnPress::Spent {
                            return Ok(());
                        }
                    }
                }
            }
        }
        // **A press inside a hosted page is the page's** (web preview slice 1).
        //
        // Below every surface that floats over the window — each of those has
        // already returned above — and above the chrome router, because inside
        // that rectangle there is no chrome to route to: the page is what the
        // pane's body *is*.
        if let Some(position) = self.window.pointer_position
            && self.point_is_on_the_web_page(position)
        {
            self.press_web_page(state, button, position)?;
            return Ok(());
        }
        // **The notice strip takes its own press — above the chrome router**
        // (user report on a real machine, 2026-08-29).
        //
        // It used to stand *below* it, and for as long as the only pane that
        // wore a strip was a terminal that was invisible: a terminal's body is
        // cells rather than chrome, so the router looked at the band and passed.
        // A **preview** pane's body is chrome all the way down — the rail, the
        // document, the caret it puts in it — so the router claimed the press
        // and the two words on the strip could be hovered, lit, and never
        // pressed.
        //
        // Above it is also the order the *hover* already used
        // (`drive_notice_hover` runs before every chrome question) and the order
        // the strip is *drawn* in (`Layered::Notice` is above the pane). Three
        // answers that have to agree, and this is the one that did not.
        if state == ElementState::Pressed
            && button == MouseButton::Left
            && let Some(position) = self.window.pointer_position
            && self.press_notice(position)?
        {
            return Ok(());
        }
        // **A button coming up is heard even after the pointer has left this
        // window**, and a button going down is not — the whole of that rule is
        // [`button_router_position`], which is where its reasons are written. A
        // hover is not given the grace either, for the same reason a press is
        // not: it asks where the hand *is*.
        let router_position = button_router_position(
            state,
            self.window.pointer_position,
            self.window.pointer_last_seen,
        );
        if let Some(position) = router_position
            && self.chrome_mouse_input(state, button, position)?
        {
            return Ok(());
        }
        // **The capsule takes its own press** (§7.1.5d), above the rail it
        // shares a corner with and below every surface that floats over the
        // window — the order it is drawn in.
        //
        // *"The capsule is one control: any press hands the caret back"* (B74),
        // which is why even a press on its bare padding is claimed rather than
        // let through: a control you can click a hole in is a control that
        // sometimes types into the shell behind it.
        //
        // The pane underneath has already taken the layout focus, exactly as it
        // has for the rail below: D40 runs above this router and consumes
        // nothing.
        if state == ElementState::Pressed
            && button == MouseButton::Left
            && let Some(position) = self.window.pointer_position
            && self.press_search(position)?
        {
            return Ok(());
        }
        // **The download sheet takes every press inside it** (§7.7 ④), above
        // the capsule and the strip in the order it is drawn. The verb answers;
        // the scrim and the card swallow. **A press on the scrim is not a
        // dismissal**: the one thing a reader must not be able to lose by
        // accident is the only notice saying a file they asked for did not
        // arrive, so Escape is the way out and it is the only one.
        if state == ElementState::Pressed
            && let Some(position) = self.window.pointer_position
            && self.press_web_sheet(position)?
        {
            return Ok(());
        }
        // **The rail takes its own press** — above the pane's cells, below every
        // surface that floats over the window, which is the order it is drawn in.
        //
        // The mock-up binds this in the *capture* phase specifically "to beat the
        // pane click handler wired per-pane" (8488-8503); here that is simply
        // where the arm stands. A press on the band is a jump and never a
        // selection: a column of ticks that also began a drag would turn the tick
        // you missed into a selection nobody asked for.
        //
        // The pane it is on has already taken the layout focus — D40 runs above
        // this router and consumes nothing, so a press on a rail is a press
        // inside a pane like any other. That is the right way round: the rail is
        // *part of* the pane's own edge, not a control floating over it, and a
        // window where clicking a pane focused it except on one nine-pixel strip
        // would have an edge nobody finds on purpose.
        if state == ElementState::Pressed
            && button == MouseButton::Left
            && let Some(position) = self.window.pointer_position
            && self.press_command_rail(position)?
        {
            return Ok(());
        }
        if state == ElementState::Released
            && matches!(self.window.mouse_route, Some(MouseRoute::MathBlock))
        {
            self.window.mouse_route = None;
            // The press ink comes off with the button, wherever it comes up —
            // the gesture belongs to the mark it began on (owner's ruling
            // 2026-09-14 ②).
            if self.window.math_tool_pressed.take().is_some() {
                self.repaint_hovered_pane()?;
            }
            return Ok(());
        }
        if state == ElementState::Pressed
            && let Some((math_seat, math_hit)) = self.math_hit()
            && matches!(button, MouseButton::Left | MouseButton::Right)
        {
            let Some(target) = self.paste_target(math_seat) else {
                return Ok(());
            };
            // Formula pixels are one indivisible presentation object in this slice. Swallowing the
            // complete press/release pair intentionally prevents half-source selections and keeps
            // both local selection and application mouse reporting from seeing synthetic cells.
            self.window.mouse_route = Some(MouseRoute::MathBlock);
            // **A mark that is being held says so** (owner's ruling 2026-09-14
            // ②), and it says so before the verb runs: toggling the source
            // republishes this pane's frame from inside the arm below, and a
            // press ink written afterwards would miss that very frame.
            if button == MouseButton::Left {
                self.window.math_tool_pressed = match math_hit.target {
                    MathHitTarget::ToggleSource => Some((
                        math_hit.anchor.clone(),
                        formula_tools::FormulaTool::ToggleSource,
                    )),
                    MathHitTarget::CopyLatex => Some((
                        math_hit.anchor.clone(),
                        formula_tools::FormulaTool::CopyLatex,
                    )),
                    MathHitTarget::Block | MathHitTarget::Failure => None,
                };
            }
            match (button, math_hit.target) {
                (MouseButton::Left, MathHitTarget::ToggleSource) => {
                    // **And it travels rather than jumping** (owner's ruling
                    // 2026-09-15, §7.1.5p ⑪). The one-frame switch this used to
                    // be still happens — under `Motion::Reduced`, on the live
                    // plane, and wherever the two faces cannot be measured — and
                    // it happens inside this door rather than beside it.
                    self.press_math_toggle(target, &math_hit.anchor)?;
                }
                (MouseButton::Left, MathHitTarget::CopyLatex) => {
                    self.copy_math_latex(target, &math_hit.anchor);
                    // The tick has to reach the glass: nothing else in this
                    // gesture asks for a frame, so without this the
                    // acknowledgement would wait for the next thing to twitch.
                    self.repaint_hovered_pane()?;
                }
                (MouseButton::Right, _) => match self.window.math_context_menu.request() {
                    Ok(true) => {
                        self.window.pending_math_context_anchor =
                            Some((target, math_hit.anchor.clone()));
                    }
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!("recoverable formula context-menu queue failure: {error}");
                    }
                },
                (MouseButton::Left, MathHitTarget::Block) => {
                    if self.shell().session.view_selection().is_some() {
                        self.clear_selection();
                        self.publish_interaction_frame()?;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        // A selection drag already in flight is finished wherever the button comes
        // up — over the pane next door, over the chrome, past the window's edge.
        // The pane it began in owns the whole gesture, so nothing here needs the
        // pointer to be over a cell; under the guard below, a release outside the
        // pane left the route latched and the next move went on selecting.
        if state == ElementState::Released
            && let Some(MouseRoute::Local(drag)) = self.window.mouse_route.as_ref().cloned()
        {
            return self.finish_local_selection(*drag);
        }
        // **A rendered page's own context menu** (user report 2026-08-28),
        // asked before the cell lookup below because a preview seat has no cells
        // and the lookup is where a press on one dies.
        //
        // It is the *terminal's* menu with a page's row list in it
        // ([`profiles::TermMenuPane`]): a right click inside a pane is one
        // gesture, and a second menu machine for two verbs would be a second
        // place for this house's own menu behaviour to drift.
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && let Some(position) = self.window.pointer_position
            && let Some(seat) = seats::pane_at(&self.seat_layout, position.x, position.y)
            && self
                .preview_rendered_surface_at(position)
                .is_some_and(|surface| surface == self.preview_here(seat))
        {
            return self.open_page_menu_at(seat, position);
        }
        let Some((hit_seat, hit)) = self.pane_frame_hit() else {
            // No cell under the pointer at all — the router is out of surfaces
            // and the event ends here (`BT_MOUSE_TRACE`).
            self.mouse_trace(|| "pane_press seat=none".to_owned());
            return Ok(());
        };
        // **The terminal's own context menu** (ticket #62), above the forwarding
        // below it and below every surface that can stand over a pane — which is
        // every arm before this one.
        //
        // The modes are read off **the pane the press landed in** and not off
        // `self.session`, which is the focused leaf: a right press does not move
        // the focus (`chrome_mouse_input` answers only the left button), so
        // asking the keyboard's pane whether *this* pane's program is tracking
        // the mouse would answer a question about the wrong shell — a `vim` in
        // the pane next door would take the menu away from a PowerShell, or fail
        // to keep it away from itself.
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && let Some(position) = self.window.pointer_position
            && let Some(modes) = self
                .sessions
                .get(&hit_seat)
                .map(|leaf| leaf.session.terminal_modes())
            && right_press_raises_terminal_menu(modes, self.window.modifiers)
        {
            return self.open_term_menu_at(hit_seat, position);
        }
        let Some(protocol_button) = protocol_mouse_button(button) else {
            return Ok(());
        };
        // Mouse-protocol forwarding is a shell's mode, so a tab with no shell
        // forwards nothing (§7.1.6h) — the same `return` a pane with no
        // forwardable hit already takes one line down.
        let Some(modes) = self.focused().map(|leaf| leaf.session.terminal_modes()) else {
            return Ok(());
        };
        let Some(forwarded_hit) = self.forwarded_mouse_hit() else {
            return Ok(());
        };
        // Asked of the *pressed* cell rather than the forwarded one: the two are
        // the same cell seen through two maps — `hit` is where the pointer is in
        // this pane's own presented frame, which is the frame the underline was
        // painted on and the frame the scan named its references in, while
        // `forwarded_hit` has already been folded onto the live grid for the
        // child's benefit. The mark and the verb must be read off the same map or
        // a press one row from the underline could claim it.
        let target = self.pressed_cell_target(hit);
        // **The fork the second half of the report turns on** (`BT_MOUSE_TRACE`).
        //
        // Claude Code prints its links with 1000/1002/1003/1006 on, so a press in
        // one of its panes belongs to the child *unless* the cell carries a
        // target this window verified — and a link the application broke across
        // three printed rows is exactly a link the cell scan may fail to name.
        // When that happens the press is forwarded and nothing opens, which is
        // the same nothing every other suspect produces. The cell's own answer
        // and the pressed-cell verdict are both written here so the two can be
        // told apart.
        self.mouse_trace(|| {
            format!(
                "pane_press seat={hit_seat:?} cell={},{} forwarded_cell={},{} pressed_target={target:?} link={:?} image={:?}",
                hit.row,
                hit.column,
                forwarded_hit.row,
                forwarded_hit.column,
                self.hyperlink_hit(hit),
                self.local_image_path_hit(hit),
            )
        });
        if let Some(bytes) = route_forwarded_mouse_button(
            &mut self.window.mouse_route,
            state,
            protocol_button,
            forwarded_hit,
            modes,
            self.window.modifiers,
            target,
        ) {
            self.mouse_trace(|| {
                format!(
                    "pane_press forwarded=1 bytes={} route={}",
                    bytes.len(),
                    self.mouse_route_name()
                )
            });
            // A press or a release forwarded into a program running there is as much a claim on it
            // as a keystroke (user ruling 2026-08-25). Read at this door and not inside
            // `send_user_input`, for that function's own stated reason.
            self.answer_attention(self.focused_leaf, UserInputKind::MouseButton);
            return self.send_user_input(
                &bytes,
                "forward mouse button event to PTY",
                UserInputKind::MouseButton,
            );
        }
        match state {
            ElementState::Pressed if button == MouseButton::Left => {
                self.begin_local_selection(hit_seat, hit)
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn vertical_wheel_travel(&self, delta: MouseScrollDelta, page: f32) -> f32 {
        self.wheel_travel(delta, page, false)
    }

    /// How far one report moves a chrome scroller, in pixels, along the axis it
    /// is being spent on.
    ///
    /// **Positive travels back on both axes** — back up a document, back along a
    /// line toward its start — and the two agree without anything here having to
    /// arrange it. winit reports a wheel turned away from the hand as positive
    /// `y`; its Windows backend negates `WM_MOUSEHWHEEL`, whose own sign is
    /// positive for a tilt to the *right*, so a tilt left arrives as positive
    /// `x`. Both components therefore already mean "back", every caller
    /// subtracts whatever it is given from the offset it keeps, and the one place
    /// a sign could have been introduced is this comment.
    ///
    /// **A notch travels the same distance whichever way it is turned**, which is
    /// why the lines-per-notch multiplier is the same on both axes: a gesture
    /// that changed length depending on the direction of the hand is a distance
    /// the hand has to relearn. `page` is the screenful along *this* axis, so the
    /// "one screen at a time" wheel setting means one screen either way.
    pub(in crate::runtime) fn wheel_travel(
        &self,
        delta: MouseScrollDelta,
        page: f32,
        sideways: bool,
    ) -> f32 {
        match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                let line = self.line_height_subpixels().get() as f32
                    / bt_viewport::SUBPIXELS_PER_PX as f32;
                let amount =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f32 * line,
                        // A page of a scroller is a screenful of it.
                        bt_platform::WheelScrollAmount::Page => page,
                    };
                if sideways { x * amount } else { y * amount }
            }
            MouseScrollDelta::PixelDelta(position) => {
                if sideways {
                    position.x as f32
                } else {
                    position.y as f32
                }
            }
        }
    }

    /// Take one notch from the platform. See [`WheelBurst`] for why this is not
    /// [`Self::mouse_wheel`].
    pub(crate) fn queue_wheel(&mut self, reported: MouseScrollDelta) -> Result<()> {
        self.window.wheel_events = self.window.wheel_events.saturating_add(1);
        // **The wheel road's own first station** (`BT_MOUSE_TRACE`, §7.60): the
        // *raw* report, as the driver sent it, before [`WheelBurst`] merges it
        // into whatever is already held. `mouse_wheel` prints the merged one, so
        // both currencies of the same gesture are in the file; the `events`
        // counter on both lines is what pairs a flush with the reports that fed
        // it, and `carried` is what was already held when this one arrived.
        //
        // It stays the platform's word and not this window's, which is what makes
        // the pair of stations worth having on a Mac: a `Shift` gesture there is
        // reported sideways and stood back up one line below
        // ([`upright_wheel`]), so `raw_delta=lines:3,0` at this station against
        // `flushed=lines:0,3` at the next *is* the rewrite, legible without a
        // debugger.
        let carried = self.window.wheel_burst;
        let events = self.window.wheel_events;
        self.mouse_trace(|| {
            format!(
                "wheel_queue raw_delta={} carried={} events={events}",
                mouse_trace::delta_word(reported),
                carried.map_or_else(
                    || "none".to_owned(),
                    |burst| mouse_trace::delta_word(burst.delta())
                ),
            )
        });
        // **The platform's report becomes this window's here**, and on exactly
        // one desktop that is a change of shape rather than a change of owner.
        // Above the merge, so a burst is accumulated in one currency and every
        // station past it — the math block's pan, the local subpixels, the column
        // arithmetic — reads a report that means what the hand meant.
        //
        // The desktop is asked of `host_platform()` and not of a `cfg!` here, for
        // [`upright_wheel`]'s stated reason and for `first_run`'s: this window
        // reads the `cfg` in one place, and a rule that turns on the platform
        // stays a rule anybody can read from any machine.
        let delta = upright_wheel(
            reported,
            self.window.modifiers.shift_key(),
            bt_platform::host_platform() == bt_platform::HostPlatform::MacOs,
        );
        match self.window.wheel_burst {
            Some(burst) => match burst.plus(delta) {
                Some(merged) => self.window.wheel_burst = Some(merged),
                None => {
                    self.flush_wheel()?;
                    self.window.wheel_burst = Some(WheelBurst::of(delta));
                }
            },
            None => self.window.wheel_burst = Some(WheelBurst::of(delta)),
        }
        Ok(())
    }

    /// Spend whatever the wheel has accumulated, if anything.
    ///
    /// Called from every door that must not run ahead of a notch: the top of
    /// `window_event` for every event that is not itself a notch, and the top of
    /// `about_to_wait`. Free — one `Option` read — for the overwhelming majority
    /// of turns, in which nobody touched the wheel.
    ///
    /// **The station is entered only when there is a notch to spend, and it is
    /// given back on the way out** (T-STATION-SPLIT). It used to be stamped
    /// unconditionally and never restored, and this door stands at the top of
    /// `window_event` — so every keystroke, every press and every resize ran
    /// under the name of a wheel nobody had turned, and the owner's own
    /// recording says so out loud: `held control for 1261 ms — flush_wheel
    /// 1104 ms` on a turn where the hand was typing. `enter`'s own rule
    /// ([`hang_watch::enter`]): a station that stands inside another's function
    /// hands the enclosing one back rather than keeping the rest of it.
    pub(crate) fn flush_wheel(&mut self) -> Result<()> {
        let Some(burst) = self.window.wheel_burst.take() else {
            return Ok(());
        };
        let leaving = hang_watch::enter(hang_watch::Station::Wheel);
        self.window.wheel_routings = self.window.wheel_routings.saturating_add(1);
        let spent = self.mouse_wheel(burst.delta());
        hang_watch::at(leaving);
        spent
    }

    /// **One file of a drop, written down the moment the platform hands it
    /// over** (release review 0.4.2 X-10).
    ///
    /// The dispatcher's `DroppedFile` arm, given a name because of the one thing
    /// it does besides pushing a path: **on the file that opens the batch, and
    /// only then, it asks the platform where the cursor is.** This call runs
    /// inside the delivery of the release itself — `IDropTarget::Drop` on
    /// Windows, `performDragOperation:` on macOS — so the hand is still where it
    /// let go of the file, which is the one instant at which the question has a
    /// true answer.
    ///
    /// **Two guards, one each.** The `is_none` here decides whether the *system*
    /// is called at all, so a drop of forty files crosses into Win32 or AppKit
    /// once rather than forty times; [`DropBatch::collect`]'s own match decides
    /// whose answer the batch keeps, so a point offered later could not win even
    /// if one were taken.
    ///
    /// **`pointer_position` is not consulted, deliberately.** The window's cached
    /// pointer is not the drop point and is not even stale in the ordinary sense:
    /// no pointer event is delivered while another application's drag is over
    /// this window, so what is in it is from before the drag began — a different
    /// gesture entirely, quite possibly over a different pane.
    pub(crate) fn collect_dropped_file(&mut self, path: PathBuf) {
        let opening = self.window.dropped_files.is_none();
        let point = opening.then(|| self.platform_pointer_now()).flatten();
        // **And the shell, named here for the same reason the point is** (X-1):
        // the pane under that point, and the tab and the running program it
        // belongs to, are what the hand was aimed at — facts about this instant
        // and not about the turn that spends them. Only on the opening file, so
        // that a drop of forty does not re-aim thirty-nine times.
        // **And a drop is refused outright while the window is asking something**
        // (review 2026-09-17 P1-b). Asked here, on the file that opens the
        // batch, because this runs inside the platform's delivery of the release
        // — the card the file was let go of over is the card that was on screen
        // when it was let go of. Asked *again* at the flush, because a gate can
        // open between the two. See [`Self::a_modal_holds_the_window`].
        let target = opening
            .then(|| {
                (!self.a_modal_holds_the_window())
                    .then(|| self.dropped_files_seat(point))
                    .flatten()
                    .and_then(|seat| self.paste_target(seat))
            })
            .flatten();
        DropBatch::collect(&mut self.window.dropped_files, path, point, target);
    }

    /// **Where the cursor is, this instant, in this window's own pixels**
    /// (GitHub issue #1 ②, owner's ruling 2026-09-16: a drop lands in the pane
    /// under the cursor).
    ///
    /// **Two readers, and they are the two gestures that put a path on a command
    /// line.** [`Self::collect_dropped_file`] asks it as an external drop
    /// arrives, and [`Self::keep_the_paste_offer`] asks it as an internal drag is
    /// let go of (review 2026-09-17). Both for one reason: there is only one
    /// instant at which "where is the cursor" and "where was this let go of" are
    /// the same question, and it is the one this process is standing in while
    /// the platform delivers the release. Nothing else in this window reads it,
    /// and the name is the platform's rather than either gesture's so that
    /// neither road can grow a second door.
    ///
    /// **The units are `CursorMoved`'s and no conversion happens here**, which
    /// was checked rather than assumed. On Windows a pointer event is
    /// `WM_MOUSEMOVE`'s `lParam` — physical pixels from the client area's
    /// top-left — which is precisely what `GetCursorPos` put through
    /// `ScreenToClient` answers. On macOS winit takes its view's point and
    /// multiplies by the window's backing scale, which is precisely what the
    /// AppKit arm does with `NSEvent.mouseLocation` after the same two
    /// conversions. So the platform's answer is already in the window's physical
    /// pixels and is used as it stands; scaling it again here would square the
    /// factor on every Retina and every 150% display.
    ///
    /// Neither `pointer_position` nor [`WindowRuntime::pointer_last_seen`] is
    /// read: both say where the hand was *before* the drag, which is not where
    /// this drop landed, and a routing built on either would be a guess wearing
    /// a measurement's clothes.
    pub(in crate::runtime) fn platform_pointer_now(&self) -> Option<PhysicalPosition<f64>> {
        platform_pointer_of(
            native_window(&self.window.window)
                .ok()
                .and_then(bt_platform::pointer_position_in_window),
        )
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) -> Result<()> {
        // **The wheel road's opening station** (`BT_MOUSE_TRACE`, §7.60), which
        // is `mouse_input`'s own opening station said in the wheel's words and
        // for its reason exactly: everything a report about a *resized* window
        // needs settled about the frame this notch landed in is read here, once,
        // before any router has had the chance to move it. The pointer is read
        // twice — the live one and the remembered one — because "the hand has
        // not moved since the resize" is a state those two tell apart and
        // nothing else does.
        self.mouse_trace(|| {
            let presentation = self.window.renderer.presentation_geometry();
            let inner = self.window.window.inner_size();
            mouse_trace::WheelEntry {
                pointer: self.window.pointer_position.map(|at| (at.x, at.y)),
                pointer_last_seen: self.window.pointer_last_seen.map(|at| (at.x, at.y)),
                swapchain: presentation.swapchain_size,
                inner: (inner.width, inner.height),
                metrics_scale: self.window.renderer.metrics().scale_factor,
                flushed: delta,
                notches: wheel_zoom_notches(delta),
                events: self.window.wheel_events,
                routings: self.window.wheel_routings,
                alt: self.window.modifiers_held.alt_key(),
                shift: self.window.modifiers_held.shift_key(),
                ctrl: self.window.modifiers_held.control_key(),
                focus_mode: self.window.focus_mode,
            }
            .line()
        });
        // **A notch spends a raised hint card** (§7.1.5e′, user ruling
        // 2026-08-25), and at the very top for `keyboard_input`'s reason: every
        // branch below this line is a surface taking the gesture home, and a
        // card left standing over the thing a notch just did is the failure this
        // surface exists to avoid. The report that settled it is `Ctrl`+wheel
        // zooming a page — a hold that is answered over and over while the card
        // hangs there insisting the hand has forgotten something.
        //
        // It answers nothing and consumes nothing: there is no path from `spend`
        // that can stop a notch, so "the hint never takes a gesture" is
        // structural here exactly as it is on the keyboard.
        self.spend_key_hint()?;
        // **A notch over the card is the card's** (user ruling, 2026-08-14), and
        // it is asked before the dismissal below for the obvious reason: the card
        // is the topmost thing on the glass, and a scroller under the pointer
        // scrolls — the same sentence the strip, the rail, the tree and the
        // preview pane are each answering further down.
        //
        // It goes down `scroll_preview_body`, the pane's own door, so the notch
        // travel, the axis and the clamp are one implementation and not two: a
        // card that scrolled by a different number of pixels per notch than the
        // pane it mirrors would be lying about being the same document.
        if let Some(position) = self.window.pointer_position
            && self.file_peek_holds([position.x as f32, position.y as f32])
            && let Some(body) = self.window.file_peek.as_ref().and_then(|peek| peek.body)
        {
            self.mouse_trace(|| "wheel_route taken=overlay at=hover-card".to_owned());
            return self.scroll_preview_body(PreviewSurface::Peek, body, delta);
        }
        // **P149's `document scroll`, capture: true** — "including the tree's
        // own scrolling". Said once, above every branch below, because that is
        // what a capturing listener on the document *is*: whatever this notch
        // turns out to scroll, the card was placed against a row that is about
        // to be somewhere else.
        if self.hide_file_peek() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        // A notch behind the scrim is nobody's — scrolling the terminal under a
        // modal is the same violation as clicking it. But the dialog's content
        // no longer always fits: `max-height` plus `overflow-y` is a scroller,
        // and a wheel over a scroller scrolls it, which is the same sentence the
        // strip and the rail are already answering below.
        // The first-run card's body is a scroller too, and a notch anywhere over
        // the card is its own — the dialog's rule above, over a card whose foot
        // is pinned so that the two verbs never scroll away (§7.56).
        if self.window.first_run.is_open() {
            let page = self.first_run_page();
            let travel = self.vertical_wheel_travel(delta, page);
            self.mouse_trace(|| "wheel_route taken=overlay at=first-run".to_owned());
            return self.scroll_first_run(travel);
        }
        if let Some(layout) = self.settings_layout() {
            self.mouse_trace(|| "wheel_route taken=overlay at=settings".to_owned());
            return self.scroll_settings(&layout, delta);
        }
        // **A notch over a notice is nobody's** (user ruling, 2026-08-16). The
        // card is not a scroller and it is not transparent: a wheel that fell
        // through it would scroll the list underneath by a notch aimed at
        // something standing over that list. It is swallowed rather than passed
        // on, which is the same answer a press on the card's body gets.
        if let Some(position) = self.window.pointer_position
            && toast::at(
                &self.window.toast_layouts,
                position.x as f32,
                position.y as f32,
            )
            .is_some()
        {
            self.mouse_trace(|| "wheel_route taken=overlay at=toast".to_owned());
            return Ok(());
        }
        // **A notch over the palette is the palette's** (DESIGN.md §7.55 ⑧,
        // user report 2026-09-03: the box was up, the pointer was on its list,
        // and the *pane behind it* scrolled).
        //
        // The box has no scrim, deliberately — the world stays visible because
        // the reader is aiming rather than answering a dialog. What that must
        // not buy is a notch falling through the glass, which is the same
        // violation as a press falling through it, and the press has been
        // answered since the box was built. So the wheel is asked the same
        // question on the same two rectangles, by the same module, and the
        // answer is opaque over the whole frame: `palette::WheelPart`.
        //
        // Here, under the card, the scrim and the toast and above everything
        // else, because that is where the box is drawn: `refresh_overlay` stages
        // it after the five menus and before the toast, and a surface must take
        // the notches over exactly the pixels it covers.
        if let Some(position) = self.window.pointer_position
            && let Some(layout) = self.window.palette_layout.clone()
            && let Some(part) = palette::wheel_part(&layout, position.x, position.y)
        {
            self.mouse_trace(|| format!("wheel_route taken=overlay at=palette part={part:?}"));
            return match part {
                palette::WheelPart::List => self.scroll_palette_list(&layout, delta),
                // The input line is not a scroller, and the box is not
                // transparent — a toast's own answer, one surface over.
                palette::WheelPart::Field => Ok(()),
            };
        }
        // **A notch over a hosted page is the page's** (web preview slice 1).
        //
        // Under the scrim and under a card, on their own arguments above; over
        // every pane, because the pane a page sits in has no document of its own
        // to move and both axes are the page's exactly as they are in any other
        // browser.
        if let Some(position) = self.window.pointer_position
            && self.point_is_on_the_web_page(position)
        {
            let (x, y) = match delta {
                MouseScrollDelta::LineDelta(x, y) => (x, y),
                // A trackpad already speaks pixels, and a notch is 120 of them.
                MouseScrollDelta::PixelDelta(position) => {
                    (position.x as f32 / 120.0, position.y as f32 / 120.0)
                }
            };
            self.mouse_trace(|| "wheel_route taken=page".to_owned());
            self.scroll_web_page(position, x, y);
            return Ok(());
        }
        // A7/A8 — a notch over the tab strip is the strip's. The mock-up gives
        // A floating tree is a scroller too — `#files-flyout .tree {
        // overflow-y: auto }` — and it stands above the panes, so it is asked
        // before any of them (user report, 2026-08-16: neither the peek nor the
        // pinned window would scroll; nothing had ever routed a notch to one).
        // A files float is opaque to the wheel over its whole frame: a notch on
        // its head or foot is spent, not passed to whatever is behind. A buffer
        // float is not answered here — its body is a preview surface and the
        // preview branch below already finds it by that name.
        if let Some(position) = self.window.pointer_position
            && let Some((id, part)) = self.float_hit_at(position)
            && self
                .window
                .float
                .drawn()
                .any(|win| win.epoch == id && win.files().is_some())
        {
            self.mouse_trace(|| format!("wheel_route taken=overlay at=files-float part={part:?}"));
            return match part {
                // `.git-view { overflow-y: auto }` in a window: the second page
                // is a scroller too, and it is the *same* rectangle — which of
                // the two answers a notch depends only on which page this window
                // is on, exactly as it does for a column.
                float::FloatPart::Row(_)
                | float::FloatPart::GitAct { .. }
                | float::FloatPart::Body
                    if self.float_shows_git_page(id) =>
                {
                    self.scroll_float_git_page(id, delta)
                }
                float::FloatPart::Row(_) | float::FloatPart::Body => {
                    self.scroll_float_tree(id, delta)
                }
                _ => Ok(()),
            };
        }
        // `.tabs-inline` `overflow-x: auto`, and a wheel over an overflowing
        // scroller scrolls it; a vertical wheel over a horizontal-only scroller
        // is exactly the case browsers translate into horizontal motion, because
        // most mice have no second axis to offer.
        // R1: the same notch, over whichever axis the tab list is on. A rail is
        // `overflow-y: auto` where the strip is `overflow-x: auto`, and both are
        // "a wheel over a scroller scrolls it".
        if let Some(position) = self.window.pointer_position {
            // §7.1.6b′: the card column is a vertical scroller in **both** tab
            // layouts, so the panel is asked first and the strip only when there
            // is no panel to have been over.
            if self.window.focus_mode || self.window.rail.layout == seats::TabLayoutMode::Vertical {
                // **Asked once and kept** (`BT_MOUSE_TRACE`, §7.60): the line
                // and the branch must read the same answer, and two calls a
                // fraction of a frame apart are two answers.
                let over_the_rail = self.rail_contains(position);
                self.wheel_rail_trace(Instant::now(), position, over_the_rail);
                if over_the_rail {
                    return self.scroll_rail(delta);
                }
            } else if seats::tab_strip_contains(
                self.window
                    .renderer
                    .presentation_geometry()
                    .swapchain_size
                    .0 as f32,
                self.window.renderer.metrics().scale_factor as f32,
                self.platform_chrome(),
                self.window.tabs.len(),
                position.x,
                position.y,
            ) {
                self.mouse_trace(|| "wheel_route taken=tab-strip".to_owned());
                return self.scroll_tab_strip(delta);
            }
        }
        // C28: `.files-tree { overflow-y: auto }` — a notch over a files column
        // is the tree's, which is the same sentence the rail and the strip have
        // just answered. It is asked before the pane router below because that
        // router's answer for a files column is "nobody's", and until this slice
        // gave the column rows that was the truth.
        if let Some(position) = self.window.pointer_position
            && let Some((seat, body)) = seats::files_body_at(
                &self.seat_layout,
                self.window.renderer.metrics().scale_factor as f32,
                self.git_panel_on(),
                position.x,
                position.y,
            )
        {
            // `.git-view { overflow-y: auto }` — the second page is a scroller
            // too, and it is the *same* body: which of the two answers a notch
            // depends only on which page the column is on.
            if self.window.git_pages_shown.contains_key(&seat) {
                self.mouse_trace(|| format!("wheel_route taken=pane at=git-panel seat={seat:?}"));
                return self.scroll_git_panel(seat, body, delta);
            }
            self.mouse_trace(|| format!("wheel_route taken=pane at=files-tree seat={seat:?}"));
            return self.scroll_files_tree(seat, body, delta);
        }
        // A graph is a list before it is a document, and it scrolls like one.
        // Asked before the preview body's own branch because that branch's guard
        // — "the buffer has content" — is false for a graph by construction:
        // there is no text in it to have been read.
        //
        // **On whichever surface the pointer is over.** This narrowed to a seat
        // until 2026-08-20, which was the wheel's share of the blank-window
        // report: a notch over a floating graph fell through to the document
        // branch below and scrolled a body with nothing in it.
        if let Some(position) = self.window.pointer_position
            && let Some((surface, body)) = self.preview_surface_at(position)
            && self.window.git_graphs_shown.contains_key(&surface)
        {
            self.mouse_trace(|| format!("wheel_route taken=pane at=git-graph surface={surface:?}"));
            return self.scroll_git_graph(surface, body, delta);
        }
        // **A notch over a picture is a zoom** (user ruling 2026-08-16, ticket
        // #60). It is asked beside the document's branch and not inside it,
        // because the two are alternatives and not a special case of one
        // another: a body showing a picture has no scroll to spend a notch on,
        // and a body showing a document has no zoom.
        //
        // The card is not reachable here — a notch over it was answered at the
        // very top of this method and went down the document's door — which is
        // half of what keeps the glance at `Fit`; the other half is
        // [`surface_takes_image_zoom`], which would decline anyway.
        if let Some(position) = self.window.pointer_position
            && let Some((surface, _)) = self.preview_surface_at(position)
            && let Some((body, image_px)) = self.preview_image_geometry(surface)
        {
            let notches = wheel_zoom_notches(delta);
            let zoom = image_zoom_notch(
                self.preview_image_zoom(surface),
                notches,
                body,
                image_px,
                [position.x as f32, position.y as f32],
            );
            self.mouse_trace(|| {
                format!("wheel_route taken=pane at=image-zoom surface={surface:?}")
            });
            self.set_preview_image_zoom(surface, zoom)?;
            return Ok(());
        }
        // `.preview-body { overflow: auto }` (mock-up 597) — and the same
        // sentence again. Asked here, beside the tree's, because it is the same
        // kind of answer: a pane whose body is a scroller, which the router
        // below has no vocabulary for.
        if let Some(position) = self.window.pointer_position
            && let Some((surface, body)) = self.preview_surface_at(position)
            && self
                .preview_buffer_on(surface)
                .is_some_and(|buffer| buffer.content.is_some())
        {
            self.mouse_trace(|| {
                format!("wheel_route taken=pane at=preview-body surface={surface:?}")
            });
            return self.scroll_preview_body(surface, body, delta);
        }
        // A notch belongs to the pane it is over. With one terminal that is the
        // terminal or nothing, which is what this guard has always said; with a
        // fleet it is whichever terminal pane the pointer is in, and a notch
        // over a preview showing something that does not scroll is still
        // nobody's.
        //
        // Routing by the pointer rather than by focus is what the rest of the
        // desktop does, and it is the only reading that lets you read a build
        // log in one pane while typing in the other — which is the reason to
        // have two panes at all.
        //
        // **The seat comes back with the story of how it was chosen**
        // (`BT_MOUSE_TRACE`, §7.60). Which of the three ways it was is exactly
        // what separates `terminal-pane` from `focused-leaf-fallback` on the
        // route line below, and it is a fact only this match knows.
        let (target_seat, seat_from) = match self.window.pointer_position {
            Some(position) => match seats::pane_at(&self.seat_layout, position.x, position.y) {
                Some(seat) if self.sessions.contains_key(&seat) => (seat, "pointer"),
                // Over a pane that is not a terminal: nobody's notch.
                Some(seat) => {
                    self.mouse_trace(|| {
                        format!("wheel_route taken=nobody at=not-a-terminal seat={seat:?}")
                    });
                    return Ok(());
                }
                // Off every pane — before the pointer has ever moved, a lone
                // leaf still scrolls exactly as it always has.
                None if self.seats.is_lone_terminal() => (self.focused_leaf, "lone-terminal"),
                None => {
                    self.mouse_trace(|| "wheel_route taken=nobody at=off-every-pane".to_owned());
                    return Ok(());
                }
            },
            None => (self.focused_leaf, "no-pointer"),
        };
        // What a notch that stays in this window is called. The pointer having
        // chosen the pane and the window having fallen back to the leaf it is
        // focused on are the same scroll and two different findings — candidate
        // (c) of the resize report is precisely the second one happening where
        // the first was meant to.
        let local_route = if seat_from == "pointer" {
            "terminal-pane"
        } else {
            "focused-leaf-fallback"
        };
        // **Nobody's notch**, which is the sentence three arms above already
        // write about a pane that is not a terminal — reached here by the two
        // arms that fall back to `focused_leaf`, because on a tab with no shell
        // that seat is a column or a preview (§7.1.6h). Asked once, after the
        // routing, so the fallbacks do not each have to repeat the condition.
        // Every chrome scroller — the strip, the rail, the settings sheet, a
        // floating tree, a preview body — has already had its turn above this,
        // so what is being declined here is only the terminal's own.
        let Some(target_leaf) = self.sessions.get(&target_seat) else {
            self.mouse_trace(|| {
                format!(
                    "wheel_route taken=nobody at=no-shell seat={target_seat:?} \
                     seat_from={seat_from}"
                )
            });
            return Ok(());
        };
        let (cell_subpixels, target_rows) = (
            target_leaf.projection.cell_height_subpixels().get() as f64,
            target_leaf.grid.rows.get() as f64,
        );
        // Scrolling moves the content the flyout was anchored to; the transient peek dissolves.
        self.dismiss_peek()?;
        // One physical event, two currencies. Local routes scroll by exact subpixels (stage C of
        // the pixel-scroll plan); forwarding routes speak whole wheel lines because that is the
        // application protocol. Route is decided first, then only that route's accumulator moves.
        // The notch belongs to the pane under the pointer, so the currency it is
        // converted into is *that* pane's: its cell height, and — for the "one
        // screen at a time" wheel setting — its own row count. Reading the
        // focused leaf's through the deref would scroll a hovered pane by a page
        // of somebody else's window, and the two stop agreeing the moment a
        // split is uneven.
        let event_subpixels = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                let multiplier =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f64,
                        bt_platform::WheelScrollAmount::Page => target_rows,
                    };
                f64::from(y) * multiplier * cell_subpixels
            }
            MouseScrollDelta::PixelDelta(position) => {
                position.y * bt_viewport::SUBPIXELS_PER_PX as f64
            }
        };
        if let Some((_, math_hit)) = self.math_hit() {
            // A hovered math block pans by whole pixels derived from the same exact motion, so
            // trackpads feel identical over blocks and text. The commit is tentative: nothing is
            // taken from the local accumulator until the block actually scrolls, because a
            // non-scrollable block falls through to the ordinary routes with the event intact.
            let mut tentative = self.window.local_wheel_subpixel_remainder + event_subpixels;
            let take_px = drain_whole_units(&mut tentative, bt_viewport::SUBPIXELS_PER_PX as f64);
            let delta_px = i32::try_from(-take_px).unwrap_or(0);
            if delta_px == 0 {
                self.window.local_wheel_subpixel_remainder = tentative;
                self.mouse_trace(|| {
                    format!(
                        "wheel_route taken={local_route} at=math-block-carried \
                         seat={target_seat:?} seat_from={seat_from}"
                    )
                });
                return Ok(());
            }
            let horizontal = if self.window.modifiers.shift_key() {
                delta_px
            } else {
                0
            };
            let vertical = if self.window.modifiers.shift_key() {
                0
            } else {
                delta_px
            };
            // The anchor came from the pane under the pointer, so the block that
            // pans has to be that pane's too — asking the focused session to
            // scroll another pane's block would find no such block, or worse,
            // one that happens to share an anchor.
            let active = self.window.active_tab;
            if self.window.tabs[active]
                .sessions
                .get_mut(&target_seat)
                .is_some_and(|leaf| {
                    leaf.session
                        .scroll_math_block(&math_hit.anchor, horizontal, vertical)
                })
            {
                self.window.local_wheel_subpixel_remainder = tentative;
                self.mouse_trace(|| {
                    format!(
                        "wheel_route taken={local_route} at=math-block seat={target_seat:?} \
                         seat_from={seat_from}"
                    )
                });
                return self.publish_interaction_frame();
            }
        }
        let modes = self.leaf_terminal_modes(target_seat);
        let target_is_scrolled = self.leaf(target_seat).projection.is_scrolled();
        let route = wheel_route(self.window.modifiers.shift_key(), modes, target_is_scrolled);
        // **The last word on this notch** (`BT_MOUSE_TRACE`, §7.60), written
        // above the four arms rather than inside them: what a reader is asking
        // by this point is which *surface* took the gesture home, and all four
        // arms are the terminal's — they differ only in the currency it is
        // spent in, which `route` says on the same line.
        self.mouse_trace(|| {
            let taken = match route {
                WheelRoute::MouseReport | WheelRoute::ArrowKeys => "pty",
                WheelRoute::Local => local_route,
                WheelRoute::Nothing => "nobody",
            };
            format!(
                "wheel_route taken={taken} at=terminal route={route:?} seat={target_seat:?} \
                 seat_from={seat_from}"
            )
        });
        match route {
            WheelRoute::MouseReport => {
                // Mouse-protocol wheel reports are per-notch, never per-system-scroll-line: the
                // application applies its own lines-per-event step, so multiplying by the Windows
                // wheel setting had TUIs (user report 2026-08-01: Claude Code transcript) scrolling
                // three times too far per notch.
                let notches = self.take_forward_wheel_notches(delta);
                if notches == 0 {
                    self.mouse_trace(|| "wheel_pty leave=no-whole-notch".to_owned());
                    return Ok(());
                }
                // The cell under the pointer *in the pane being addressed*. A hit
                // taken from anywhere else would be a row and a column measured in
                // one grid and delivered to another.
                let Some(hit) = self.forwarded_mouse_hit_in(target_seat) else {
                    self.mouse_trace(|| {
                        format!("wheel_pty leave=no-cell-under-the-pointer seat={target_seat:?}")
                    });
                    return Ok(());
                };
                let button = if notches > 0 {
                    input::MouseProtocolButton::WheelUp
                } else {
                    input::MouseProtocolButton::WheelDown
                };
                let one = input::mouse_bytes(
                    modes.sgr_mouse,
                    button,
                    input::MouseProtocolEvent::Press,
                    hit.row,
                    hit.column,
                    self.window.modifiers,
                );
                // On the wire a notch *is* a press (SGR button 64/65, with no release of its own),
                // so "presses count" already covered it. Spelled at this door rather than in the
                // helper, because the helper it shares with a pointer sweep carries two semantics
                // and only one of them is an answer (`attention` plan §11.3).
                self.answer_attention(target_seat, UserInputKind::MouseWheel);
                self.send_mouse_input_to(
                    target_seat,
                    &one.repeat(notches.unsigned_abs() as usize),
                    "forward SGR mouse wheel to PTY",
                )
            }
            WheelRoute::ArrowKeys => {
                let lines = self.take_forward_wheel_lines(target_seat, delta);
                if lines == 0 {
                    self.mouse_trace(|| "wheel_pty leave=no-whole-line".to_owned());
                    return Ok(());
                }
                // The addressed shell's own cursor mode. `ESC O A` and `ESC [ A`
                // are different bytes, and which one a program understands is a
                // fact about *that* program — a hovered vim judged by the focused
                // shell's mode would be sent letters to print.
                let bytes = input::alternate_scroll_bytes(
                    lines,
                    self.leaf_application_cursor_mode(target_seat),
                );
                self.answer_attention(target_seat, UserInputKind::MouseWheel);
                self.send_mouse_input_to(
                    target_seat,
                    &bytes,
                    "forward alternate-screen wheel to PTY",
                )
            }
            WheelRoute::Local => match self.wheel_columns(target_seat, delta) {
                Some(columns) => self.scroll_seat_by_columns(target_seat, columns),
                None => self
                    .scroll_view_exact_in(target_seat, event_subpixels)
                    .map(|_| ()),
            },
            WheelRoute::Nothing => Ok(()),
        }
    }

    /// Whole-line quantization for the alternate-scroll emulation route: arrow-key emulation
    /// mirrors the local scroll distance, so the system lines-per-notch setting applies here.
    /// Fractional motion parks in the forwarding accumulators, never the local subpixel one.
    ///
    /// `seat` is the pane being addressed, because "one screen at a time" is a
    /// number of rows and rows are per pane. Reading the focused leaf's count
    /// would send a hovered half-height pane a full window's worth of arrows.
    fn take_forward_wheel_lines(&mut self, seat: SeatId, delta: MouseScrollDelta) -> i32 {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                let multiplier =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f64,
                        bt_platform::WheelScrollAmount::Page => self.leaf_wheel_rows(seat),
                    };
                self.window.line_wheel_remainder += f64::from(y) * multiplier;
                drain_whole_units(&mut self.window.line_wheel_remainder, 1.0) as i32
            }
            MouseScrollDelta::PixelDelta(position) => {
                self.window.pixel_wheel_remainder += position.y;
                let cell_px = self.window.renderer.metrics().cell_height_px as f64;
                drain_whole_units(&mut self.window.pixel_wheel_remainder, cell_px) as i32
            }
        }
    }

    /// Per-notch quantization for the mouse-protocol route: one wheel report per detent, the
    /// xterm convention every TUI calibrates its own scroll step against. Trackpad pixel deltas
    /// emit one report per accrued cell height of travel.
    fn take_forward_wheel_notches(&mut self, delta: MouseScrollDelta) -> i32 {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                self.window.notch_wheel_remainder += f64::from(y);
                drain_whole_units(&mut self.window.notch_wheel_remainder, 1.0) as i32
            }
            MouseScrollDelta::PixelDelta(position) => {
                self.window.pixel_wheel_remainder += position.y;
                let cell_px = self.window.renderer.metrics().cell_height_px as f64;
                drain_whole_units(&mut self.window.pixel_wheel_remainder, cell_px) as i32
            }
        }
    }

    /// How many **columns** a local notch moves this pane's window, or `None` when
    /// this notch is not the horizontal axis's at all.
    ///
    /// `None` is the answer for every pane that has been drawn by this product
    /// until now, and that is the point: the question [`wheel_axis`] settles is
    /// asked only where there are two axes to settle it between.
    ///
    /// **A notch travels the same distance whichever way it is turned.** The
    /// vertical notch moves the system's lines-per-notch times the cell's
    /// *height*; the same physical distance sideways is that many pixels divided
    /// by the cell's *width*, which on this product's default face is about twice
    /// as many columns as rows. That is `scroll_tab_strip`'s rule quoted where it
    /// belongs — *"a notch that changed length depending on what it was over is a
    /// distance the hand has to relearn at every surface"* — and it is why the
    /// number here is derived from the metrics rather than picked. The "one
    /// screen at a time" setting means one screenful on this axis too, which is
    /// the window's own width in columns.
    ///
    /// The remainder is kept across events for [`WheelBurst`]'s reason: a
    /// high-resolution wheel and a precision touchpad send a detent as a run of
    /// small reports, and rounding each of them alone throws the whole detent
    /// away a sixth at a time.
    fn wheel_columns(&mut self, seat: SeatId, delta: MouseScrollDelta) -> Option<i32> {
        let axis = self.leaf(seat).projection.horizontal();
        // A report the platform put on the x axis says what it means, and
        // [`wheel_points_sideways`] is where this window decides that — the
        // sentence about a hand not being straight now governs both arms of the
        // report rather than the pixel one alone.
        let sideways = wheel_points_sideways(delta);
        // **Rows this gesture is the only way to reach** — [`wheel_axis`]'s first
        // half, read off the pane. On the alternate screen the plain wheel is the
        // program's, so the rows a typeset formula pushed above this pane answer
        // to `Shift` and to nothing else; on the primary screen they are a plain
        // notch away and the key is free to name the other axis.
        let shift_only_rows = self.leaf_terminal_modes(seat).alternate_screen
            && self.leaf(seat).projection.has_displaced_rows();
        if wheel_axis(
            self.window.modifiers.shift_key(),
            sideways,
            shift_only_rows,
            axis.max_x_origin().0 > 0,
        ) != WheelAxis::Columns
        {
            return None;
        }
        let metrics = self.window.renderer.metrics();
        let cell_width = f64::from(metrics.cell_width_px).max(1.0);
        let travel = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                // Turning the wheel away from the hand goes *back* — up a
                // document, and left along a line. A horizontal report already
                // points the way it means and is taken as it stands.
                let notches = if sideways {
                    f64::from(x)
                } else {
                    -f64::from(y)
                };
                let per_notch =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => {
                            (f64::from(lines) * f64::from(metrics.cell_height_px) / cell_width)
                                .max(1.0)
                        }
                        bt_platform::WheelScrollAmount::Page => f64::from(axis.viewport_columns()),
                    };
                notches * per_notch
            }
            MouseScrollDelta::PixelDelta(position) => {
                let pixels = if sideways { position.x } else { -position.y };
                pixels / cell_width
            }
        };
        self.window.local_wheel_column_remainder += travel;
        let take = drain_whole_units(&mut self.window.local_wheel_column_remainder, 1.0);
        // `Some(0)` and not `None`: a report too small to move a column is still
        // this axis's report, and handing it back would spend it on the other one.
        Some(i32::try_from(take).unwrap_or(0))
    }

    /// **Whether a gesture of this window's is holding the pointer** — the one
    /// fact [`Self::web_page_at`] subtracts before it looks at any rectangle.
    ///
    /// "A gesture in flight is not a hover" is already this window's rule for
    /// the chevron clocks, the tooltip and the layout peek; this is the same
    /// sentence said to the one surface this window does not paint. A hosted
    /// page is a document inside a pane, and a document answers the pointer when
    /// the pointer is *over* it — but a pointer with a press still open on it
    /// belongs to whatever that press began on, wherever it has since travelled.
    ///
    /// **Every carry, and not only the drop.** The reported defect was a pane
    /// carried over a page, but the ladder in [`Self::mouse_input`] answers the
    /// divider, the video scrubber, the preview thumbs, the picture pan and the
    /// terminal's own selection *below* its page arm too, so all of them ended
    /// the same way — a release the page ate and a gesture that never finished.
    /// One predicate rather than one clause per gesture is what stops the next
    /// one being written without this line.
    ///
    /// **`MouseRoute::Forward` is deliberately not here.** That press was handed
    /// to the program in the pane, spoken in that pane's cells, and its release
    /// is routed by a cell lookup — a pane holding a page has no cells, so that
    /// gesture cannot end on one however this answers. Its latch is also the one
    /// state here that a release does not always clear (a release with no cell
    /// under it leaves it standing, which it already did before this line
    /// existed), and a predicate that could stick is a predicate that could
    /// switch every page in the window off for good.
    pub(in crate::runtime) fn a_gesture_holds_the_pointer(&self) -> bool {
        self.window.drag.is_some()
            || self.window.divider_drag.is_some()
            || self.window.float_drag.is_some()
            || self.window.video_bar_drag.is_some()
            || self.window.settings_slider_drag.is_some()
            || self.window.settings_menu_bar_drag.is_some()
            || self.preview_block_drag.is_some()
            || self.preview_body_drag.is_some()
            || self.preview_text_drag.is_some()
            || self.preview_image_drag.is_some()
            || self.terminal_thumb_drag.is_some()
            || self.terminal_column_drag.is_some()
            || self
                .window
                .file_peek
                .as_ref()
                .is_some_and(|peek| peek.thumb_grab.is_some())
            || matches!(
                self.window.mouse_route,
                Some(MouseRoute::Local(_) | MouseRoute::MathBlock)
            )
    }
}
