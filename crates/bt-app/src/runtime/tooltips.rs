//! `tooltips` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    PreviewSurface, Runtime, WebHeadVerb, git_graph, git_panel, i18n, marks, pane_control_tip,
    seats, tab_surface_tip_boxes, tooltip, tooltip_anchor_for,
};
use anyhow::Result;
use bt_layout::SeatId;
use std::time::Instant;
use winit::dpi::PhysicalPosition;

impl Runtime<'_> {
    /// Rebuild the tooltip's anchor list from the geometry the strip is about to
    /// be drawn with.
    ///
    /// Beside the chrome and from the same numbers, so an anchor cannot describe
    /// a box the strip is not drawing. Order is innermost-first, which is how
    /// [`tooltip::TooltipAnchors`] reproduces the mock-up's `closest()`: the pin
    /// and the mark answer before the tab they sit on.
    ///
    /// Registration is also where every suppression lands, because "do not tip
    /// this" and "this has nothing to say" are the same instruction to a host
    /// that only ever sees a list (M141).
    ///
    /// **It answers to the layout**, which it did not, and the bug that was is
    /// the one `layout_peek_layer`, `profile_menu_layout` and `chrome_target_at`
    /// each already carry a comment about: `tab_strip_geometry` is a pure
    /// function of a width and a trailer list and knows nothing about a rail
    /// being on screen, so in vertical layout it went on handing back boxes for a
    /// strip nobody was drawing — tab 0's body landing over the top bar, where
    /// the sidebar toggle actually lives. First match wins in
    /// [`tooltip::TooltipAnchors`] and the phantom was pushed first, so hovering
    /// the visible toggle answered with some tab's name.
    pub(in crate::runtime) fn rebuild_tooltip_anchors(
        &mut self,
        scale: f32,
        width: f32,
        now: Instant,
    ) {
        // **A modal card owns every anchor there is.** The first-run card is
        // drawn over a scrim that swallows the pointer outright, so a tip about
        // a tab or a pane head under it would be this window explaining
        // something the reader cannot reach — the same instruction "do not tip
        // this" always is here: a list with nothing else in it.
        if self.window.first_run.is_open() {
            self.rebuild_first_run_tip_anchors();
            return;
        }
        let mut anchors = tooltip::TooltipAnchors::default();
        // A drag owns the pointer outright and everything else goes quiet for the
        // length of the gesture — the same rule hover, the peek flyout and the
        // terminal's own selection already live by. An empty list is how that is
        // said here: there is nothing to be over.
        if self.window.drag.is_none() {
            // **First, so it wins.** The picker floats over whichever surface
            // opened it, and first-match-wins is this list's whole ordering rule
            // — a row registered after the chevron under it would never be
            // reached. It is the innermost thing on screen, so it is pushed
            // innermost.
            if let Some(layout) = self.profile_menu_layout() {
                for (row, rect, text) in
                    layout.tips(&self.app.profile_programs, self.app.recent.entries())
                {
                    anchors.push(tooltip::TooltipAnchorId::ProfileRow(row), rect, text);
                }
            }
            // The other popup, on the same terms: its rows show a folder's last
            // segment, so the whole path has to be somewhere, and the tip is
            // where every other cropped caption in this window puts it.
            if let Some(layout) = self.root_menu_layout() {
                let choices = self
                    .window
                    .root_menu
                    .seat()
                    .map(|seat| self.root_choices(seat))
                    .unwrap_or_default();
                for (row, rect, text) in layout.tips(&choices) {
                    anchors.push(tooltip::TooltipAnchorId::RootRow(row), rect, text);
                }
            }
            let strip = seats::tab_strip_geometry(
                width,
                scale,
                self.platform_chrome(),
                &self.tab_trailers(now),
                self.window.active_tab,
                self.window.tab_scroll,
            );
            let rail = self.rail_geometry_now(now);
            let renaming = self.window.rename.as_ref().and_then(|editor| {
                self.window
                    .tabs
                    .iter()
                    .position(|tab| Some(tab.id) == editor.tab())
            });
            // **The same phantom this function's own doc is about, one mode
            // later** (§7.1.6b′). `tab_strip_geometry` is a pure function of a
            // width and a trailer list and knows nothing about focus mode, so
            // under the card column it goes on handing back boxes for a strip
            // nobody is drawing — and first-match-wins would answer a hover over
            // the title bar with some tab's name. In focus mode neither tab
            // surface is on screen, so neither registers.
            //
            // The column's own tips are not here and are not owed by F1: a card
            // states its name in full and the way out says `Exit` on its face.
            if !self.window.focus_mode {
                for (id, rect) in tab_surface_tip_boxes(
                    self.window.rail.layout,
                    &strip,
                    rail.as_ref(),
                    scale,
                    self.window.profile_menu.is_open(),
                    renaming,
                ) {
                    anchors.push(id, rect, self.tab_surface_tip_text(id));
                }
            }
            // The posture and not the preference, for `panel_covers`'s reason:
            // the sidebar toggle is not drawn while the column is up, and a tip
            // on a button nobody drew is a tip on the drag strip.
            for (target, rect) in seats::window_chrome_boxes(
                width,
                scale,
                self.platform_chrome(),
                self.rail_posture(),
                self.is_quake_window(),
            ) {
                let text = match target {
                    // The gear, silenced while the dialog it opens is up — the
                    // chevron's rule, for the same reason.
                    seats::ChromeTarget::Settings if self.window.settings.is_open() => "",
                    seats::ChromeTarget::Settings => i18n::Text::Settings.text(),
                    // `title="Toggle sidebar"` (mock-up 2270), quoted rather than
                    // reworded: the tip is the mock-up's own text and it names the
                    // verb in both directions, which is what a toggle needs.
                    seats::ChromeTarget::PanelToggle => i18n::Text::ToggleSidebar.text(),
                    seats::ChromeTarget::Minimize => i18n::Text::Minimize.text(),
                    seats::ChromeTarget::Maximize => i18n::Text::Maximize.text(),
                    seats::ChromeTarget::CloseWindow => i18n::Text::CloseWindow.text(),
                    _ => "",
                };
                let Some(id) = tooltip_anchor_for(target) else {
                    continue;
                };
                anchors.push(id, rect, text);
            }
        }
        // Every pane head's `⌄` (user ruling, 2026-08-16). Inside the same
        // `drag.is_none()` guard as everything above, because a head under a
        // drag is a head nobody is pointing at — the list is empty for the whole
        // gesture and this would be registering into it.
        //
        // Pushed before the Git page's rows below and after the strip's above,
        // which is this list's innermost-first order read literally: a pane head
        // sits over its own pane and under the popups, and nothing else in the
        // window claims these nineteen pixels.
        if self.window.drag.is_none() {
            // **Every control on every pane's own run** (user ruling
            // 2026-08-27). One walk of `seats::pane_control_boxes`, which
            // answers for whichever layout the pane is in — its head's run, or
            // its corner's two doors — so a control that is on screen is a
            // control this pass has seen.
            //
            // It was the `⌄` and the corner's 🗀 alone. The argument for leaving
            // the head's folder and its `×` out was that both are idioms this
            // product has taught elsewhere; the ruling overturns it, on the
            // ground an idiom cannot say *what* a `×` closes.
            //
            // Silenced on the one control whose own layer is standing — the
            // gear's rule and the strip chevron's, for their reason: a tip
            // explaining what a button opens, standing beside the thing it just
            // opened, is a sentence about a fact already on screen. It is the
            // control and not the whole run, because the rest of the run is
            // saying nothing that is already on screen.
            let raised = self.head_that_raised_a_layer();
            let capsule = self.window.search.seat();
            let seats: Vec<SeatId> = self
                .seat_layout
                .rects
                .iter()
                .map(|placement| placement.id)
                .collect();
            for seat in seats {
                for (verb, box_) in
                    seats::pane_control_boxes(&self.seats, &self.seat_layout, seat, scale, capsule)
                {
                    if raised
                        == Some(seats::PaneLayer {
                            seat,
                            door: Some(verb),
                        })
                    {
                        continue;
                    }
                    anchors.push(
                        tooltip::TooltipAnchorId::PaneControl(seat, verb),
                        box_,
                        pane_control_tip(verb),
                    );
                }
            }
            // A page's hand-off arrow, in the same pass and inside the same
            // guard: it is a pane head's control like the chevron above, and a
            // head under a drag is a head nobody is pointing at. Only the seats
            // whose head actually draws one register — `preview_head_tools`
            // answers `browser: false` for everything else, so the box is `None`
            // and nothing is pushed.
            let previews: Vec<SeatId> = self.seats.preview_seats();
            for seat in previews {
                let Some(rect) = self.preview_browser_box(seat) else {
                    continue;
                };
                anchors.push(
                    tooltip::TooltipAnchorId::PreviewBrowser(seat),
                    rect,
                    i18n::Text::PreviewOpenInBrowser.text(),
                );
            }
            // **Every control on every preview head**, in the same pass and
            // inside the same guard (user ruling 2026-08-27 — 「头/轨上每一枚可
            // 点的东西都必须有 tooltip」).
            //
            // One loop over one list, and that is the fix rather than a tidying.
            // This used to be three loops naming three of the six controls: the
            // padlock had its own, the source flip borrowed the rail's, the
            // developer tools rode the page's navigation loop — and `Save`, the
            // player's `■` and the pop-out `↗` were named by none of them,
            // because a control got words here by somebody remembering to add a
            // fourth loop. `seats::preview_head_tool_boxes` is the run the paint
            // and the hit test walk, so a control that is on screen is a control
            // this pass has already seen.
            let previews: Vec<SeatId> = self.seats.preview_seats();
            for seat in previews {
                let Some(rect) = seats::full_pane_rect(&self.seat_layout, seat) else {
                    continue;
                };
                let head = seats::pane_head_geometry(
                    rect,
                    bt_layout::SeatKind::Preview,
                    self.seat_layout.seat_is_on_stage(seat),
                    scale,
                );
                let geometry =
                    seats::preview_head_geometry(&head, scale, self.preview_head_tools(seat));
                for (tool, box_) in seats::preview_head_tool_boxes(&geometry) {
                    anchors.push(
                        tooltip::TooltipAnchorId::PreviewHeadTool(seat, tool),
                        box_,
                        self.preview_head_tool_tip(seat, tool),
                    );
                }
            }
            // **A page's three navigation buttons**, in the same pass and inside
            // the same guard (§7.7 ②). Only the seats whose rail actually draws
            // them register: `rail_geometry` answers `None` for every other
            // seat, so nothing is pushed. (The developer tools were a fourth
            // entry here until 2026-08-27; they are drawn on the head and
            // register with the rest of that run, above.)
            let previews: Vec<SeatId> = self.seats.preview_seats();
            for seat in previews {
                let loading = self.web_on(seat).is_some_and(|web| web.page().loading);
                for (tool, box_, text) in [
                    (
                        tooltip::WebNavTool::Back,
                        self.preview_web_tool_box(seat, WebHeadVerb::Back),
                        i18n::Text::PreviewWebBack,
                    ),
                    (
                        tooltip::WebNavTool::Forward,
                        self.preview_web_tool_box(seat, WebHeadVerb::Forward),
                        i18n::Text::PreviewWebForward,
                    ),
                    (
                        tooltip::WebNavTool::Reload,
                        self.preview_web_tool_box(seat, WebHeadVerb::Reload),
                        // The tip changes with the button, because the button
                        // changes: one control, two states, and a tip that said
                        // `Reload` over a stop would be the head describing what
                        // it was doing a second ago.
                        if loading {
                            i18n::Text::PreviewWebStop
                        } else {
                            i18n::Text::PreviewWebReload
                        },
                    ),
                ] {
                    let Some(box_) = box_ else {
                        continue;
                    };
                    anchors.push(
                        tooltip::TooltipAnchorId::PreviewWebNav(seat, tool),
                        box_,
                        text.text(),
                    );
                }
            }
            // **The row under each head** (user ruling 2026-08-25). In the same
            // pass and inside the same guard as the head's own controls above,
            // because it is the same kind of surface one band lower — and after
            // them, which is this list's innermost-first order read literally:
            // the two rows do not overlap, so the order between them is only the
            // order they were built in.
            self.preview_rail_tip_anchors(&mut anchors);
            // The search capsule's own controls, pushed after the heads and
            // before the Git page's rows for this list's own innermost-first
            // rule: the capsule stands over one pane's body, so it is inside
            // everything the strip and the heads register and outside nothing.
            self.search_tip_anchors(&mut anchors);
        }
        // The Git page's own tips, from the page that was drawn (R5, and the
        // three teaching headings the mock-up wrote at 4950-4952). Pushed last of
        // the pane-level anchors and inside the same `drag.is_none()` guard as
        // everything above them, because a page under a drag is a page nobody is
        // pointing at.
        if self.window.drag.is_none() {
            let git_scale = scale;
            let pages: Vec<(SeatId, git_panel::GitPanelContent)> = self
                .window
                .git_pages_shown
                .iter()
                .map(|(seat, page)| (*seat, page.clone()))
                .collect();
            for (seat, page) in pages {
                let Some(rect) = seats::files_pane_rect(&self.seat_layout, seat) else {
                    continue;
                };
                let body = seats::files_pane_geometry(rect, git_scale, true).body;
                let geometry = git_panel::git_panel_geometry(body, &page, git_scale);
                // Which row the verbs are showing on — the same predicate the
                // painter uses, because a tip is a promise about something on
                // screen and a hover verb on a resting row is not (see
                // `git_panel::GIT_ACT_REVEAL`).
                let revealed_row = match self.window.seat_pointer.hover {
                    Some(seats::ChromeTarget::GitRow { seat: on, index })
                    | Some(seats::ChromeTarget::GitAct {
                        seat: on, index, ..
                    }) if on == seat => Some(index),
                    _ => None,
                };
                for (index, row) in page.rows.iter().enumerate() {
                    let box_ = geometry.row_rect(index);
                    // A row scrolled out from under the viewport has no tip:
                    // the anchor and the picture are the same rectangle or the
                    // tip arrives beside nothing.
                    if box_[3] <= body[1] || box_[1] >= body[3] {
                        continue;
                    }
                    // The buttons first, so they win the pixels they are on:
                    // first-match-wins is this list's ordering rule, and a row
                    // registered ahead of the verb inside it would swallow it.
                    let untracked = matches!(
                        row,
                        git_panel::GitRow::Change(change) if change.untracked
                    );
                    for (act, act_box) in
                        git_panel::act_boxes(row, box_, git_scale, revealed_row == Some(index))
                    {
                        anchors.push(
                            tooltip::TooltipAnchorId::GitAct(seat, index, act),
                            act_box,
                            act.tooltip(untracked),
                        );
                    }
                    // **The masthead's buttons register here too**, which they
                    // did not until R31's third moment put a second one beside
                    // the first: this loop used to reach the pills and then
                    // `continue`, so the door to the graph had been drawn, lit
                    // and pressable with nothing to say for itself since G24.
                    // The pills follow the buttons, on the ordering rule above —
                    // `pill_boxes` already stops where the buttons begin, so the
                    // two do not overlap and the order is a discipline rather
                    // than a fix.
                    if let git_panel::GitRow::Masthead(head) = row {
                        for (pill, pill_box) in head
                            .pills
                            .iter()
                            .zip(git_panel::pill_boxes(head, box_, git_scale))
                        {
                            anchors.push(
                                tooltip::TooltipAnchorId::GitPill(seat, index),
                                pill_box,
                                &pill.tooltip,
                            );
                        }
                        continue;
                    }
                    if let Some(text) = git_panel::row_tooltip(row) {
                        anchors.push(tooltip::TooltipAnchorId::GitRow(seat, index), box_, &text);
                    }
                }
            }
        }
        // The commit graph's toolbar (T1). Only the toolbar: the rows below it
        // carry a tooltip of their own and have never registered one, because a
        // list whose every row explains itself is a list that flickers.
        let graph_surfaces: Vec<PreviewSurface> =
            self.window.git_graphs_shown.keys().copied().collect();
        for surface in graph_surfaces {
            let Some(rects) = self.graph_toolbar_rects(surface) else {
                continue;
            };
            for (rect, tool) in [
                (rects.search_clear, git_graph::GraphTool::SearchClear),
                (rects.search, git_graph::GraphTool::Search),
                (Some(rects.filter), git_graph::GraphTool::Filter),
                (Some(rects.refresh), git_graph::GraphTool::Refresh),
            ] {
                let Some(rect) = rect else { continue };
                anchors.push(
                    tooltip::TooltipAnchorId::GitGraphTool(surface, tool),
                    rect,
                    tool.tooltip(),
                );
            }
        }
        // **The glance card, as one anchor and not one per tick.**
        //
        // The rail's whole pointer contract is that *the band* takes the pointer
        // and the nearest ordinal answers it (mock 8404-8409): a list of
        // nine-by-two boxes would put the card up only when a hand happened to
        // land on two pixels, which is precisely the target the crest exists
        // because nobody can hit. So the anchor is the crest's own box, its id
        // names the mark rather than the tick's index — the fisheye renumbers
        // indices under a hand that has not moved — and it is registered only
        // while a rail is hot, which is the only time there is anything to glance
        // at. `retain` below therefore takes the card down the moment the pointer
        // leaves the rail, and the wait in front of it is the card's own
        // `PEEK_INTENT_DELAY` (user report, 2026-08-19): the tip's single host and
        // single fade are still shared, only the length of the countdown is not.
        if self.window.drag.is_none()
            && let Some((id, host, text, face)) = self.command_tick_card()
        {
            anchors.push_faced(id, host, text, face);
        }
        // The colour under the pointer, on the pointer's own 380ms and the
        // tip's own surface (§7.1.6c-4c). Pushed after every other content
        // anchor, which is right: it is the innermost thing on screen that is
        // not a popup. The chip below is the only other box registered inside a
        // preview's body, and the two cannot overlap — this one is a token in a
        // source file and that one is a run of a rendered page, which are two
        // faces the same pane is never showing at once.
        if self.window.drag.is_none()
            && let Some(hover) = self.window.preview_hex_hover.as_ref()
        {
            anchors.push_faced(
                tooltip::TooltipAnchorId::PreviewHex(hover.surface, hover.offset),
                hover.host,
                hover.text.clone(),
                tooltip::TipFace::Swatch { rgba: hover.rgba },
            );
        }
        // **A chip's hover card** (§7.1.3k ⑬), on the same host and the same
        // clock as the colour card above it — the pointer is resting inside a
        // document in both, and a second mechanism for the second one would be
        // two ways for this window to say something about a run. In the plain
        // tip face rather than the colour card's swatch: what it has to show is
        // two lines of words.
        //
        // Read off the link the pointer is already over
        // ([`Runtime::note_preview_link_hover`]) rather than hit-tested again
        // here, so the pill that lights and the card that comes up cannot come
        // to disagree about which chip the hand is on.
        if self.window.drag.is_none()
            && let Some((surface, link)) = self.preview_link_hover.as_ref()
            && let Some(chip) = link.chip.as_ref()
        {
            anchors.push(
                tooltip::TooltipAnchorId::PreviewImageChip(*surface, chip.at),
                link.rect,
                chip.text.clone(),
            );
        }
        // A tip whose subject has left the strip has nothing left to say — and a
        // tip still counting down toward a subject that left has nothing to
        // arrive at. Retiring both here, against the list that was just built, is
        // what keeps the host from owing a frame it could never pay.
        self.window.tooltip.retain(|id| anchors.find(id).is_some());
        self.window.tooltip_anchors = anchors;
        // The peek's subject is a tab rather than an anchor, so it is retired
        // against the same predicate that armed it. Sampled into a slice first:
        // the closure cannot read `self` while the host it is retiring is part
        // of `self`.
        let eligible: Vec<bool> = (0..self.window.tabs.len())
            .map(|index| self.layout_peek_eligible(index))
            .collect();
        self.window
            .layout_peek
            .retain(|index| eligible.get(index).copied().unwrap_or(false));
    }

    /// Whether the tip on screen differs from the tip last painted — the strip's
    /// own frame-debt question ([`tab_owes_frame`]), asked about the fade.
    ///
    /// This and not "is it still fading" is what schedules the *landing* frame:
    /// the moment the fade ends there is one more frame owed, carrying the
    /// opacity from wherever the last wake left it up to a solid 1.
    fn tooltip_owes_frame(&self, now: Instant) -> bool {
        self.window.tooltip_drawn_opacity != self.tooltip_opacity(now)
    }

    /// The opacity the tip should be painted at this instant, or `None` when
    /// there is no tip.
    fn tooltip_opacity(&self, now: Instant) -> Option<f32> {
        self.window
            .tooltip
            .active()
            .map(|_| self.window.tooltip.opacity(now, self.app.motion))
    }

    /// When this window next has tooltip work: the settle deadline, or the next
    /// frame of a fade that has not landed.
    pub(in crate::runtime) fn tooltip_deadline(&self, now: Instant) -> Option<Instant> {
        let next_frame = self.next_animation_deadline();
        if self.tooltip_owes_frame(now) {
            return next_frame;
        }
        let owner =
            self.window
                .tooltip
                .deadline(now, self.app.motion, self.window.frame_clock.interval());
        if self.window.tooltip.is_fading(now, self.app.motion) {
            next_frame
        } else {
            owner.map(|deadline| self.clamp_animation_deadline(deadline))
        }
    }

    /// Note what the pointer is over — and in which face it would be answered,
    /// because that is what decides how long the pointer has to hold still
    /// ([`tooltip::TipFace::intent_delay`]) — and repaint if the answer took a tip
    /// down.
    pub(in crate::runtime) fn note_tooltip(
        &mut self,
        anchor: Option<(tooltip::TooltipAnchorId, tooltip::TipFace)>,
    ) -> Result<()> {
        if self.window.tooltip.observe(anchor, Instant::now()) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The anchor under the pointer right now and the face its tip wears, if any.
    pub(in crate::runtime) fn tooltip_anchor_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(tooltip::TooltipAnchorId, tooltip::TipFace)> {
        self.window
            .tooltip_anchors
            .at(position.x as f32, position.y as f32)
            .map(|anchor| (anchor.id, anchor.face))
    }

    /// Show a settled tip, and keep paying the fade's frames until it lands.
    pub(in crate::runtime) fn advance_tooltip_if_due(&mut self, now: Instant) -> Result<()> {
        // **The wait maturing is state, and state is never paced** (closure
        // review O4, 2026-09-18): behind the gate, a tip whose three hundred and
        // eighty milliseconds ran out while a neighbouring pane was printing
        // would not appear at all until the printing stopped. See
        // [`Self::animation_frame_is_due`] for the three things an advancer does.
        let promoted = self.window.tooltip.activate_if_due(now);
        // And then the fade's own frame, which is the only paced thing here — a
        // turn that promoted a tip has something new to say and goes through.
        if !promoted && !self.animation_frame_is_due() {
            return Ok(());
        }
        if (promoted || self.tooltip_owes_frame(now)) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take the tip down — any press, a lost window, a menu opening (M142, I94).
    pub(in crate::runtime) fn hide_tooltip(&mut self) -> Result<()> {
        if self.window.tooltip.hide() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The tip's own layer, or nothing when none is showing.
    ///
    /// The text is read out of this frame's anchors rather than remembered from
    /// the frame the tip appeared on, so a tab renamed under an open tip says its
    /// new name on the next frame — the mock-up rewrites `el.title` on every
    /// paint for the same reason (line 4331).
    pub(in crate::runtime) fn tooltip_layer(&mut self) -> Vec<marks::OverlayLayer> {
        // Recorded at the end and only on the paths that actually paint, so the
        // frame-debt comparison is against what is *on screen*. Recording the
        // intent instead would let a tip that could not be laid out report itself
        // as drawn, and the debt would be settled by a frame nobody ever saw.
        self.window.tooltip_drawn_opacity = None;
        let now = Instant::now();
        let Some(opacity) = self.tooltip_opacity(now) else {
            return Vec::new();
        };
        let Some(anchor) = self
            .window
            .tooltip
            .active()
            .and_then(|id| self.window.tooltip_anchors.find(id))
        else {
            return Vec::new();
        };
        let (text, host, face) = (anchor.text.clone(), anchor.rect, anchor.face);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let font_px = face.font_logical_px() * scale;
        // Only the font knows how wide a line is, so the measuring happens here,
        // beside the renderer, exactly as the badge's and the editor's do — first
        // to bring the text inside the width bound, then to size the box to the
        // lines that came out. Which face measures is the face that draws: a
        // monospace card measured with the sans metrics is a box its own text
        // overflows.
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let gap = tooltip::TIP_GAP_LOGICAL_PX * scale;
        let max_width = (face.max_width_logical_px() * scale).min(width as f32 - 2.0 * gap);
        let (text, widths) = {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let line_height = (font_px * face.line_height()).round();
            let mut measure = |run: &str| -> f32 {
                if face.monospace() {
                    renderer.measure_preview_paragraph_width(
                        gpu,
                        &[bt_render::PreviewRun {
                            text: run.to_owned(),
                            color: [0, 0, 0],
                            mono: true,
                            bold: false,
                            italic: false,
                            font_scale: 1.0,
                            inline_box_px: None,
                        }],
                        font_px,
                        line_height,
                    )
                } else {
                    renderer.measure_chrome_text(gpu, run, font_px)
                }
            };
            // `white-space: pre-line` and a wrap, or `nowrap` and an ellipsis —
            // the two faces answer "too wide" differently and that difference is
            // the point of [`tooltip::TipFace`].
            let text = if face.wraps() {
                tooltip::wrap(&text, max_width, &mut measure).join("\n")
            } else {
                // Per line, because the box is `white-space: pre-line` in both
                // faces: a `\n` in a nowrap tip is a deliberate second line (the
                // glance card's quoted error), and cutting the whole string as
                // one run would swallow it into the first line's ellipsis.
                text.split('\n')
                    .map(|line| tooltip::ellipsize(line, max_width, &mut measure))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let widths: Vec<f32> = text.split('\n').map(&mut measure).collect();
            (text, widths)
        };
        let Some(layout) = tooltip::layout(
            &text,
            host,
            &widths,
            (width as f32, height as f32),
            scale,
            face,
        ) else {
            return Vec::new();
        };
        let palette = bt_render::chrome_palette();
        self.window.tooltip_drawn_opacity = Some(opacity);
        tooltip::build(&layout, &palette, scale, opacity, face)
    }
}
