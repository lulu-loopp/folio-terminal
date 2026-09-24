//! `attention` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    AttentionDelivery, NoticeHost, NoticeStrip, PreviewSurface, Runtime, UserInputKind,
    WindowRuntime, answer_attention_in, attention, attention_codex, attention_copilot,
    attention_hooks, attention_trace, emit_attention_lines, float, hang_watch, i18n, marks,
    native_window, next_attention_stop, notice, notify, profiles, seats, toast,
};
use anyhow::Result;
use bt_layout::SeatId;
use std::time::Instant;
use winit::dpi::PhysicalPosition;

impl Runtime<'_> {
    /// Raise a notice (user ruling, 2026-08-16).
    ///
    /// The one door. Everything that wants to say "this just happened" comes
    /// through here, so that the timing, the cap and the z-order are decided
    /// once — and so that the thing raising it has to name the **surface** it is
    /// about, which is what the ruling is: a notice appears where the attention
    /// already is, not in a corner of the window.
    pub(crate) fn toast(
        &mut self,
        kind: toast::ToastKind,
        anchor: toast::ToastAnchor,
        title: Option<String>,
        body: impl Into<String>,
    ) -> Result<()> {
        self.window.toasts.raise(
            kind,
            anchor,
            title,
            body,
            None,
            self.app.motion,
            Instant::now(),
        );
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        // A card born under a still pointer is a card the pointer is on: the
        // real machine raised one over the very row that was clicked, drew it
        // with its `×` lit — the paint reads the pointer afresh — and then let
        // its clock run, because the hold is a *hover* and no hover event had
        // arrived. Asked here, the way `drive_rail_zone` is asked after a float
        // opens: the answer changed without the pointer moving.
        if let Some(position) = self.window.pointer_position {
            self.drive_toast_hover(position)?;
        }
        Ok(())
    }

    /// Where a notice's surface is this frame, or `None` when it has gone.
    ///
    /// Resolved afresh every time it is asked, exactly as a tip's text is: a
    /// column can be dragged wider and a seat can be split while a card is
    /// standing on it, and a card holding the rectangle it was born with would be
    /// a confident sentence over somewhere else. `None` is what sends it to the
    /// window's corner — the fallback, and only that.
    fn toast_anchor_rect(&self, anchor: toast::ToastAnchor) -> Option<[f32; 4]> {
        let scale = self.window.renderer.scale_factor() as f32;
        match anchor {
            // The column's page body, whichever page it is showing. A notice
            // about what this column was asked to do does not move to the corner
            // because the reader glanced at the tree.
            toast::ToastAnchor::FilesColumn(seat) => {
                let rect = seats::files_pane_rect(&self.seat_layout, seat)?;
                Some(seats::files_pane_geometry(rect, scale, self.git_panel_on()).body)
            }
            // **Its own tab's pane, or nowhere** (§7.12 ⓑ). A notice outlives a
            // tab switch and the strip is the only thing on the glass that
            // changed, so a card anchored to a pane on some other tab is a
            // confident sentence pointing at a stranger. `None` is the corner,
            // which is exactly what "the surface this was about is not in front
            // of you" means.
            toast::ToastAnchor::PreviewSeat(leaf) => (leaf.tab == self.id)
                .then(|| {
                    seats::preview_seat_body_rect(&self.seats, &self.seat_layout, leaf.seat, scale)
                })
                .flatten(),
            toast::ToastAnchor::Window => None,
        }
    }

    /// The notices' own layer, and the boxes the pointer will be tested against.
    ///
    /// Both come out of one call for [`Self::tooltip_layer`]'s reason: the text
    /// is measured here, beside the renderer, because only the font can say how
    /// wide a line is — and measuring it twice is how the drawn `×` and the
    /// pressable `×` drift apart.
    pub(in crate::runtime) fn toast_layer(&mut self) -> Vec<marks::OverlayLayer> {
        // Recorded at the end and only on the path that paints, so the debt is
        // against what is *on screen* — [`Self::tooltip_layer`]'s own note.
        self.window.toasts_drawn = Vec::new();
        self.window.toast_layouts = Vec::new();
        self.window.toast_pointer_drawn = toast::ToastPointer::default();
        if self.window.toasts.is_empty() {
            return Vec::new();
        }
        let now = Instant::now();
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        // The rectangles first, while nothing is borrowed: `place` needs both the
        // anchor resolver (which reads the layout) and the measurer (which holds
        // the renderer), and the two cannot both borrow `self`.
        let anchors: Vec<(toast::ToastAnchor, Option<[f32; 4]>)> = self
            .window
            .toasts
            .toasts()
            .iter()
            .map(|toast| (toast.anchor(), self.toast_anchor_rect(toast.anchor())))
            .collect();
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let layouts = toast::place(
            self.window.toasts.toasts(),
            |anchor| {
                anchors
                    .iter()
                    .find(|(id, _)| *id == anchor)
                    .and_then(|(_, rect)| *rect)
            },
            (width as f32, height as f32),
            scale,
            &mut |run, size| renderer.measure_chrome_text(gpu, run, size),
        );
        let pointer = self.toast_pointer(&layouts);
        let palette = bt_render::chrome_palette();
        let motion = self.app.motion;
        let layers = toast::build(
            &layouts,
            &self.window.toasts,
            pointer,
            &palette,
            scale,
            now,
            motion,
        );
        self.window.toasts_drawn = self.window.toasts.frame_state(now, motion);
        self.window.toast_pointer_drawn = pointer;
        self.window.toast_layouts = layouts;
        layers
    }

    /// Raise a card that carries a verb, and answer which card it is.
    ///
    /// **A deletion is immediate and its way back is a button on the notice**
    /// (plan §2.3, user ruling 2026-08-17 Q3): this dialog has no dirty gate to
    /// route a confirmation through and every choice in it is written the instant
    /// it is made, so what an irreversible one is owed is not a second question
    /// but an undo — the register `Ctrl+Shift+T` already struck in this product.
    /// A confirmation would be its first modal over a modal.
    ///
    /// The id comes back because the caller has to remember which card its undo
    /// belongs to: a second deletion while the first card is still standing
    /// replaces the pending undo, and a verb pressed on a card that is no longer
    /// the one holding it must do nothing rather than undo the wrong thing.
    pub(in crate::runtime) fn toast_with_verb(
        &mut self,
        kind: toast::ToastKind,
        anchor: toast::ToastAnchor,
        body: impl Into<String>,
        verb: &str,
    ) -> Result<toast::ToastId> {
        let id = self.window.toasts.raise(
            kind,
            anchor,
            None,
            body,
            Some(verb.to_owned()),
            self.app.motion,
            Instant::now(),
        );
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(id)
    }

    /// Which card, which `×` and which verb the pointer is on, for the reveal
    /// ladder.
    fn toast_pointer(&self, layouts: &[toast::ToastLayout]) -> toast::ToastPointer {
        let Some(position) = self.window.pointer_position else {
            return toast::ToastPointer::default();
        };
        match toast::at(layouts, position.x as f32, position.y as f32) {
            Some(toast::ToastHit::Close(id)) => toast::ToastPointer {
                card: Some(id),
                close: Some(id),
                action: None,
            },
            Some(toast::ToastHit::Action(id)) => toast::ToastPointer {
                card: Some(id),
                close: None,
                action: Some(id),
            },
            Some(toast::ToastHit::Card(id)) => toast::ToastPointer {
                card: Some(id),
                close: None,
                action: None,
            },
            None => toast::ToastPointer::default(),
        }
    }

    /// Whether the cards on screen differ from the cards last painted.
    fn toasts_owe_frame(&self, now: Instant) -> bool {
        self.window.toasts_drawn != self.window.toasts.frame_state(now, self.app.motion)
    }

    /// When this window next has notice work: an entrance landing, a life running
    /// out, an exit finishing — or this instant, when a frame is already owed.
    pub(in crate::runtime) fn toast_deadline(&self, now: Instant) -> Option<Instant> {
        if self.toasts_owe_frame(now) || self.window.toasts.is_animating(now, self.app.motion) {
            return self.next_animation_deadline();
        }
        self.window.toasts.deadline(now, self.app.motion)
    }

    /// Move every card's clock on, and pay the frames the movement owes.
    pub(in crate::runtime) fn advance_toasts(&mut self, now: Instant) -> Result<()> {
        // **The clocks first, and never paced** (closure review O4,
        // 2026-09-18). A notice's life running out and a departed card being
        // dropped are *state* rather than a frame of animation — see
        // [`Self::animation_frame_is_due`] for the three things an advancer
        // does — and behind the gate they are a notice that a neighbouring pane
        // printing every five milliseconds can keep on the reader's screen for
        // as long as it keeps printing.
        let moved = self.window.toasts.advance(now, self.app.motion);
        // **And then the fade's own frame** (owner's report 2026-09-18): a turn
        // inside the frame the glass is already showing has nothing it could put
        // on it, and the refusal books the turn that has. A turn on which the
        // service changed something is not that turn — what it has to say is new
        // — so it goes through.
        if !moved && !self.animation_frame_is_due() {
            return Ok(());
        }
        if (moved || self.toasts_owe_frame(now)) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Note which card the pointer is on — the hover that holds a card's clock,
    /// and the one that lights its `×`. Returns whether the press should stop
    /// here.
    pub(in crate::runtime) fn drive_toast_hover(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let hit = toast::at(
            &self.window.toast_layouts,
            position.x as f32,
            position.y as f32,
        );
        let card = hit.map(|hit| match hit {
            toast::ToastHit::Close(id)
            | toast::ToastHit::Card(id)
            | toast::ToastHit::Action(id) => id,
        });
        // Two questions, and both have to be asked: the clock stops for the card
        // under the pointer, and the ladder's rung changes when the pointer
        // crosses into the `×` *without* changing which card it is on.
        let moved =
            self.toast_pointer(&self.window.toast_layouts) != self.window.toast_pointer_drawn;
        let held = self.window.toasts.hover(card, Instant::now());
        if (held || moved) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(hit.is_some())
    }

    /// A press on a card: the `×` sends it away, and anywhere else is swallowed.
    ///
    /// **Swallowed, not ignored.** A toast stands over a list of files with a
    /// verb on every row; a press that fell through it would stage whatever
    /// happened to be under the card you were reaching for.
    pub(in crate::runtime) fn press_toast(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(hit) = toast::at(
            &self.window.toast_layouts,
            position.x as f32,
            position.y as f32,
        ) else {
            return Ok(false);
        };
        // The verb first: pressing it does the thing and sends the card away,
        // because a card whose verb has been taken is a card whose sentence is
        // no longer true.
        if let toast::ToastHit::Action(id) = hit {
            self.take_profile_undo(id)?;
            self.take_checkout_undo(id)?;
            if self
                .window
                .toasts
                .dismiss(id, Instant::now(), self.app.motion)
                && self.refresh_overlay()
            {
                self.present_chrome_change()?;
            }
            return Ok(true);
        }
        if let toast::ToastHit::Close(id) = hit
            && self
                .window
                .toasts
                .dismiss(id, Instant::now(), self.app.motion)
            && self.refresh_overlay()
        {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// What one host's strip is saying, or nothing when it wears none.
    ///
    /// **Three kinds of host and each answers with one of two facts**: a
    /// terminal seat from its shell's offer, a preview seat and a preview float
    /// from the buffer under them. A window can never answer with the first —
    /// it is torn off a preview — which is why its arm asks only the second.
    pub(crate) fn notice_on(&self, host: NoticeHost) -> Option<notice::Notice> {
        let NoticeHost::Seat(seat) = host else {
            return self.preview_disk_notice_on(self.notice_surface(host));
        };
        if let Some(leaf) = self.sessions.get(&seat) {
            return leaf.integration_offer.as_ref()?.showing(
                leaf.output_revision > 0,
                leaf.session.shell_integration_seen(),
            );
        }
        self.preview_disk_notice_on(self.preview_here(seat))
    }

    /// **Which preview surface a notice host is about.**
    ///
    /// The one translation between the two vocabularies, so that every verb a
    /// strip offers a document — `Reload`, `Keep`, the `×` that means the same
    /// as `Keep` — is written once and reaches the buffer whichever host was
    /// pressed. A terminal seat resolves to its tab's preview leaf, which holds
    /// no buffer, and the preview verbs it can never show therefore find none.
    pub(in crate::runtime) fn notice_surface(&self, host: NoticeHost) -> PreviewSurface {
        match host {
            NoticeHost::Seat(seat) => self.preview_here(seat),
            NoticeHost::Float(id) => PreviewSurface::Float(id),
        }
    }

    /// Every strip's own level of the overlay stack, and the rectangles a press
    /// is tested against — stored here for the capsule's reason: the box you can
    /// press is the box you can see.
    ///
    /// **The one pass that settles the band, for every host** (B1, 2026-09-01).
    /// It returns the *panes'* layers, because they are drawn in the pane lane;
    /// a float's strip is drawn in that window's own lane, above its face and
    /// under any window in front of it, and is built from the row this pass
    /// wrote ([`Self::float_notice_layer`]). Measuring it a second time over
    /// there would be a second answer to "where is this row and how much of the
    /// sentence fits", which is the disagreement between what is drawn and what
    /// is pressed that this map exists to prevent.
    pub(in crate::runtime) fn notice_layers(&mut self) -> Vec<marks::OverlayLayer> {
        let scale = self.window.renderer.scale_factor() as f32;
        let palette = bt_render::chrome_palette();
        let font = notice::FONT_LOGICAL_PX * scale;
        // Both kinds of seat, because both kinds wear this band since the
        // 2026-08-29 ruling, and every preview float, because a window torn off
        // a preview wears it too. `notice_on` is what tells them apart, and a
        // host that wears none answers `None` and costs one lookup.
        let now = Instant::now();
        // **Two kinds of wearer and two shapes** (owner's ruling 2026-09-12). A
        // shell's offer is a *band* across the top of its pane's body, because a
        // terminal is a column of rows with nothing to float over. Everything a
        // preview has to say is a *pill* over the bottom edge of its document,
        // because a document is a surface — and because the band it used to be
        // stood there empty for every hour it had nothing in it, which is the
        // report this ruling answers.
        let wearers: Vec<(NoticeHost, bool)> = self
            .seats
            .terminals()
            .into_iter()
            .map(|seat| (NoticeHost::Seat(seat), false))
            .chain(
                self.seats
                    .preview_seats()
                    .into_iter()
                    .map(|seat| (NoticeHost::Seat(seat), true)),
            )
            .chain(
                self.preview_float_ids()
                    .into_iter()
                    .map(|id| (NoticeHost::Float(id), true)),
            )
            .collect();
        let mut layouts = std::collections::BTreeMap::new();
        let mut layers = Vec::new();
        for (host, floats) in wearers {
            // The words first, because a pill's are not always a `Notice`: a
            // confirmation has no verbs and a refusal's sentence comes off the
            // buffer (see [`Self::preview_pill_say`]).
            let floating = floats
                .then(|| self.preview_pill_say(self.notice_surface(host), now))
                .flatten();
            let standing = (!floats).then(|| self.notice_on(host)).flatten();
            let saying = match (&floating, standing) {
                (Some((words, verbs)), _) => notice::NoticeSay::pill(words, verbs),
                (None, Some(state)) => notice::NoticeSay::band(state),
                (None, None) => continue,
            };
            let verbs = saying.verbs;
            // Measured against the font that will draw them, which is what keeps
            // a Chinese verb from being given an English word's box. **Before
            // the rectangle and not after it since the owner's ruling of
            // 2026-09-12**: a pill is as wide as what it says, so what it says
            // has to be measured before there is anywhere to say it.
            let widths: Vec<f32> = verbs
                .iter()
                .map(|verb| {
                    self.window
                        .renderer
                        .measure_chrome_text(&mut self.app.gpu, verb.text(), font)
                })
                .collect();
            // Each host's own rectangle: a terminal's is the complement of the
            // subtraction that made room for it, and a preview's is cut out of
            // the body it floats over, takes nothing from it, and is no wider
            // than the sentence and the words inside it.
            let strip = if floats {
                let sentence =
                    self.window
                        .renderer
                        .measure_chrome_text(&mut self.app.gpu, saying.text, font);
                let content = notice::pill_content_width(sentence, &widths, scale);
                self.preview_surface_body_rect(self.notice_surface(host), scale)
                    .and_then(|body| seats::news_pill_box(body, scale, content))
            } else {
                match host {
                    NoticeHost::Seat(seat) => {
                        seats::pane_notice_strip(&self.seats, &self.seat_layout, seat, scale)
                    }
                    // A float is never a terminal, so a window never wears the
                    // band: the one notice a torn-off preview can carry is its
                    // own document's, and that is a pill.
                    NoticeHost::Float(_) => None,
                }
            };
            let Some(strip) = strip else {
                continue;
            };
            let bar = notice::lay_out(strip, saying, &widths, scale);
            // **And the sentence, cut to the row it was left** (§7.43). The
            // words keep their whole boxes and the prose is what gives way, so
            // what is drawn is the longest prefix of it that fits with a `…` —
            // `settings::ellipsized`, which is the one answer in this window to
            // "how much of this fits", and not a second count of characters.
            let say = {
                let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
                notice::sentence(saying, &bar, font, &mut |text, size| {
                    renderer.measure_chrome_text(gpu, text, size)
                })
            };
            // A pane's band is drawn here; a window's is drawn in its own lane
            // from the same row, so only the panes' layers go back.
            if let NoticeHost::Seat(_) = host {
                let hover = self
                    .window
                    .notice_hover
                    .filter(|(hovered, _)| *hovered == host)
                    .map(|(_, element)| element);
                layers.push(notice::build(&bar, &say, hover, &palette, scale));
            }
            layouts.insert(host, NoticeStrip { bar, say });
        }
        self.window.notice_layouts = layouts;
        layers
    }

    /// **One floating window's strip, drawn** (B1, 2026-09-01).
    ///
    /// Built from the row [`Self::notice_layers`] settled earlier in this same
    /// frame rather than measured again, which is that pass's own note: the box
    /// you can press is the box you can see, and two measurements are two boxes.
    ///
    /// A layer of its own, chained after this window's face for
    /// [`Self::preview_float_bar_layers`]'s reason exactly — above the window it
    /// belongs to, and under any window standing in front of it, because a float
    /// in front covers this one whole. It is not passed through the notice
    /// band's own passage (`Layered::Notice`, the downward arrival a pane's
    /// strip is staged with): a float's chassis has no passages at all (P49 ③ —
    /// a preview window is simply there at full strength), and a band inside a
    /// window's layer cannot arrive on a clock the window itself does not keep.
    pub(in crate::runtime) fn float_notice_layer(
        &mut self,
        id: float::FloatId,
    ) -> Option<marks::OverlayLayer> {
        let scale = self.window.renderer.scale_factor() as f32;
        let palette = bt_render::chrome_palette();
        let host = NoticeHost::Float(id);
        let strip = self.window.notice_layouts.get(&host)?.clone();
        let hover = self
            .window
            .notice_hover
            .filter(|(hovered, _)| *hovered == host)
            .map(|(_, element)| element);
        Some(notice::build(
            &strip.bar, &strip.say, hover, &palette, scale,
        ))
    }

    /// Which strip's control the pointer is on, and whether it is on a strip at
    /// all.
    ///
    /// The `bool` is what the routing above it reads, for
    /// [`Self::drive_search_hover`]'s reason: a pointer standing on the strip is
    /// not standing on a cell, and the pane must not be told about it.
    pub(in crate::runtime) fn drive_notice_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<bool> {
        // **A pill with nothing to press is not hovered** (owner's ruling
        // 2026-09-12), on `press_notice`'s own reason one gesture along: a
        // confirmation floating over a document must not take the pointer away
        // from the words under it.
        let hover = position.and_then(|at| {
            self.window.notice_layouts.iter().find_map(|(host, strip)| {
                (!strip.bar.verbs.is_empty() || strip.bar.close.is_some())
                    .then(|| {
                        notice::hit(&strip.bar, at.x as f32, at.y as f32)
                            .map(|element| (*host, element))
                    })
                    .flatten()
            })
        });
        if self.window.notice_hover != hover {
            self.window.notice_hover = hover;
            if self.refresh_overlay() {
                self.present_chrome_change()?;
            }
        }
        Ok(hover.is_some())
    }

    /// A press on a strip.
    ///
    /// **One door for both hosts** (B1, 2026-09-01). The three shell verbs are
    /// asked of a seat because only a seat can show them — a window is torn off
    /// a preview and its band can only ever say what a document's file did — and
    /// the two document verbs are asked of the *surface*, which both hosts have.
    pub(in crate::runtime) fn press_notice(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let hit = self.window.notice_layouts.iter().find_map(|(host, strip)| {
            notice::hit(&strip.bar, position.x as f32, position.y as f32).map(|it| {
                (
                    *host,
                    it,
                    strip.bar.verbs.is_empty() && strip.bar.close.is_none(),
                )
            })
        });
        let Some((host, element, verbless)) = hit else {
            return Ok(false);
        };
        // Which seat, when the host is one. `None` is a float, and every arm
        // below that needs a seat is an arm a float's strip cannot show.
        let seat = match host {
            NoticeHost::Seat(seat) => Some(seat),
            NoticeHost::Float(_) => None,
        };
        match element {
            notice::NoticeElement::Close => self.close_pane_notice(host)?,
            notice::NoticeElement::Verb(notice::NoticeVerb::Add) => {
                if let Some(seat) = seat {
                    self.add_to_profile(seat)?;
                }
            }
            notice::NoticeElement::Verb(notice::NoticeVerb::Never) => {
                self.apply_powershell_integration_offer(false)?;
            }
            notice::NoticeElement::Verb(notice::NoticeVerb::Restart) => {
                // The strip goes first: `restart_shell` builds a whole new leaf
                // for this seat, and the offer that one carries is decided from
                // the file as it now stands.
                self.close_pane_notice(host)?;
                if let Some(seat) = seat {
                    self.restart_shell(seat)?;
                }
            }
            notice::NoticeElement::Verb(notice::NoticeVerb::ReloadFromDisk) => {
                self.reload_preview_from_disk(host)?;
            }
            notice::NoticeElement::Verb(notice::NoticeVerb::KeepMyEdits) => {
                self.close_pane_notice(host)?;
            }
            // **The one way out of a page this window will not edit** (owner's
            // ruling 2026-09-12) — the refused card's own verb, on the pill
            // that answers a reader who has just tried to type into it.
            notice::NoticeElement::Verb(notice::NoticeVerb::OpenExternally) => {
                self.open_preview_externally_on(self.notice_surface(host))?;
            }
            // The strip's own width. It takes the press and answers nothing — a
            // bar with a hole in it lets a click through onto a cell that is
            // nowhere near the pointer.
            //
            // **A pill with no verb in it does not take the press at all**
            // (owner's ruling 2026-09-12). A band is chrome a reader did not ask
            // for and stands in its own row, so swallowing a stray click is the
            // honest thing; a confirmation floating over a document is *the
            // document's* surface for the second it is up, and a `Saved` that
            // ate a click into the paragraph under it would be this window
            // charging the reader for having been told.
            notice::NoticeElement::Body => {
                if verbless {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// **Re-read all three agent configurations**, on the one edge that can afford it.
    ///
    /// The same three reads the launch makes, and the same pair of facts out of each: whether this
    /// copy's marks are in that file, and — when the file is one this build will not edit — the
    /// reason the row says instead of `Off`. A refused row takes no press at all, so this is the
    /// only way a machine that has been put right becomes a machine Folio will act on again
    /// without a relaunch.
    pub(crate) fn refresh_agent_rows(&mut self) {
        (
            self.app.claude_hooks_installed,
            self.app.agent_config_refusals[0],
        ) = attention_hooks::row_state();
        (
            self.app.codex_notify_installed,
            self.app.agent_config_refusals[1],
        ) = attention_codex::row_state();
        (
            self.app.copilot_hooks_installed,
            self.app.agent_config_refusals[2],
        ) = attention_copilot::row_state();
    }

    /// Whether one of the built-in agent profiles has its program on this
    /// machine.
    ///
    /// **The same lookup the picker greys a row with**, and deliberately not a
    /// second one: `ProfilePrograms` already asked the machine, once, where
    /// `claude.cmd`, `codex.cmd` and `copilot.cmd` are, and a card that answered
    /// the question a different way could offer a row for a program the picker
    /// says is not installed.
    pub(in crate::runtime) fn agent_is_on_this_machine(&self, id: &str) -> bool {
        // `position_of` and not `index_of_id`: that one must answer with *some*
        // profile because a pane has to start something, and "this table has no
        // such row" is exactly the answer this question needs.
        profiles::has_id(id) && self.app.profile_programs.is_available(id)
    }

    pub(crate) fn apply_terminal_notifications(&mut self, enabled: bool) {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.terminal_notifications = enabled;
        self.app.settings_store.store(settings);
    }

    /// Whether the end of a turn may reach the desktop (`attention` plan §11.7).
    ///
    /// [`Self::apply_terminal_notifications`]'s shape and every one of its three reasons, on the
    /// row beneath it — with one difference worth stating because it is where the two rows part.
    /// That switch is read **where a toast would be raised**; this one is read **at the door**,
    /// before the ledger evaluates anything, because §11.7 requires that with it off the lane is
    /// not walked at all: no evaluation, no trace line, and no bit set. A switch that only
    /// suppressed the delivery would leave the ledger having decided this turn's ending was dealt
    /// with, so switching the row back on mid-turn would find a turn already marked answered —
    /// half a state left behind by a preference being turned off and on again.
    ///
    /// **Nothing inside the window moves either way.** The bell dot, the unread dot and the
    /// queue's own badge are the ledger's, and this row is about the desktop outside — which is
    /// the same division the row above is filed under and the one the ledger's predicate is kept
    /// clear of.
    pub(crate) fn apply_turn_end_notification(&mut self, enabled: bool) {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.turn_end_notification = enabled;
        self.app.settings_store.store(settings);
    }

    /// **Do what the ledger's answer says**, for every interruption it allowed this turn
    /// (`attention` plan §11.7, slice C3).
    ///
    /// **Not a gate, and that is red line 12.** Both doors have already evaluated the one
    /// condition either of them is allowed to; what arrives here is a decision with a
    /// [`attention::Reach`] attached, and this is the three-armed consumer of it. Nothing here asks
    /// again whether the interruption was owed — a second opinion on this side is exactly the
    /// second gate the red line forbids, and it would be one taken from a reading of the window
    /// made after the decision rather than at it.
    ///
    /// * **`Nothing`** — the reader is looking at the pane. It is not deferred and not queued; a
    ///   notification held for later is a notification about something they finished doing.
    /// * **`Flash`** — the shell is asked to call the eye to this window's taskbar button. **Once
    ///   per turn however many panes spoke**, because the button is the window's and flashing it
    ///   twice is not twice as much of anything. On a window that has the keyboard this is a
    ///   documented no-op and the arm is written knowing it — see [`notify::desktop_reach`]. It
    ///   never arrives on a desktop whose taskbar hides itself: there is no button to flash there,
    ///   and the reach said so (user ruling 2026-08-28).
    /// * **`Toast`** — the window is out of reach of the reader's eyes, so the desktop is what is
    ///   left.
    ///
    /// **The three arms are [`notify::interruption`]'s and the `match` here only performs them.**
    /// The pairing of a reach with what it does is a claim — that a toast never flashes and a
    /// flash never reaches the desktop — and it lives in a function so a test can hold it. This
    /// method owns the parts a test cannot: which window's handle, which route, which sentence.
    ///
    /// **`terminal_notifications` gates the toast and not the flash**, and the split is the row's
    /// own sentence: it governs whether a program may *put a message on the desktop*. A taskbar
    /// button calling for attention is not a message and leaves nothing behind.
    ///
    /// That is the *last* gate a delivery passes and not the first: which of the two rows decides
    /// whether an announcement is made at all is read at the door, by
    /// [`attention::NotificationSwitches`], because a row that is off requires that its lane not be
    /// evaluated at all — no trace, no bit set. The two are not a double negative. This one asks
    /// "may a message be put on the desktop"; the door asks "is there an announcement to make".
    /// A `Reach::Flash` that the door let through still flashes with `Notifications` off, which is
    /// the whole of "a taskbar button is not a message".
    /// **The two rows that can stop an announcement, read together** (§7.1.5o ③′, user ruling
    /// 2026-08-26).
    ///
    /// Read on the turn rather than cached, for the reason every other settings read on this path
    /// is: a row toggled in the settings panel has to take effect on the next arrival and not on
    /// the next restart. Read as a *pair* rather than one at a time, because the door's whole job
    /// is to choose between them, and a caller that fetched only the one it expected to need would
    /// be making that choice itself, in a second place, from a guess about the transport.
    pub(crate) fn notification_switches(&self) -> attention::NotificationSwitches {
        let settings = self.app.settings_store.loaded();
        attention::NotificationSwitches {
            turn_end: settings.turn_end_notification,
            desktop_messages: settings.terminal_notifications,
        }
    }

    pub(crate) fn raise_attention(&mut self, raised: Vec<AttentionDelivery>) -> Result<()> {
        if raised.is_empty() {
            return Ok(());
        }
        let enabled = self.app.settings_store.loaded().terminal_notifications;
        let window = u64::from(self.window.window.id());
        let mut flashed = false;
        let mut refusal = None;
        for delivery in raised {
            match notify::interruption(delivery.reach, enabled) {
                notify::Interruption::Nothing => {}
                notify::Interruption::FlashTheTaskbarButton => {
                    if !flashed && let Ok(native) = native_window(&self.window.window) {
                        flashed = true;
                        bt_platform::flash_window(native);
                    }
                }
                notify::Interruption::PutItOnTheDesktop => {
                    let route = notify::NotificationRoute {
                        window,
                        tab: delivery.tab.0,
                        seat: delivery.seat.0,
                    };
                    let body = delivery.body.unwrap_or_else(|| {
                        match delivery.why {
                            attention::Why::Awaiting => i18n::Text::ToastWaitingForYou,
                            attention::Why::TurnEnd => i18n::Text::ToastTurnFinished,
                        }
                        .text()
                        .to_owned()
                    });
                    if let Err(error) =
                        self.app
                            .notifications
                            .show(&delivery.title, &body, &route.launch())
                    {
                        refusal = Some(error);
                    }
                }
            }
        }
        match refusal {
            Some(error) => self.raise_notification_refusal(&error),
            None => Ok(()),
        }
    }

    /// **Say in the window that the desktop would not take it** (user ruling 2026-08-25).
    ///
    /// The one failure this block could otherwise have and nobody could detect: the reader is not
    /// notified, and is not notified that they were not notified. A line of `stderr` goes to a log
    /// file the reader has no reason to open, so the refusal is said where they are — as a card of
    /// the family every other refusal in this window already wears.
    ///
    /// **Once per session**, and it costs nothing to arrange: [`NotificationDesk`] latches its own
    /// refusal, so every later message answers `Ok(())` and no second card is raised. That is the
    /// right number — the sentence is "this machine cannot raise notifications", which is true once
    /// and does not become truer.
    ///
    /// **In the corner and not on a pane.** The window that could not raise a toast is by
    /// definition one the reader was not looking at, so there is no pane under their eye for the
    /// card to hang off; and what the card is about is the *channel*, which belongs to the window
    /// rather than to whichever shell happened to speak first.
    fn raise_notification_refusal(&mut self, error: &str) -> Result<()> {
        self.toast(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            Some(i18n::Text::NotifyRefusedTitle.text().to_owned()),
            format!("{} {error}", i18n::Text::NotifyRefusedBody.text()),
        )
    }

    /// Answer a clicked toast: this window forward, that tab on screen, that pane holding the
    /// keyboard.
    ///
    /// All three, and in that order, because a notification's promise is the pane and not the
    /// window: raising a window onto the wrong tab would be the click half-honoured, and the
    /// order matters because the tab switch clears pointer and frame state that a later window
    /// activation would otherwise race.
    ///
    /// **Every step is allowed to find nothing.** A tab can be closed, a pane can be closed or
    /// split away, and the toast that named them can sit in the notification centre for as long
    /// as the reader leaves it there. Each miss stops the walk where it is rather than falling
    /// back to something nearby — a click that lands on a pane the notification was not about is
    /// worse than one that lands on the window and stops.
    ///
    /// **One `open` line per click** (`BT_ATTENTION_TRACE`, §11.1.5's station list). The whole of
    /// this walk used to be silent, and silence here is what left the C slice's own report saying
    /// the path "was not verified on a real machine": a click that landed correctly and a click
    /// that was swallowed by a closed tab looked exactly alike from outside the window — and a
    /// toast banner is gone in five seconds, so there is nothing on screen to go back to either.
    /// Every arm below writes its own, `leave=` naming what it did not find, on the same
    /// one-line-per-decision rule the rest of the file keeps.
    pub(crate) fn open_from_notification(
        &mut self,
        route: notify::NotificationRoute,
    ) -> Result<()> {
        let launch = route.launch();
        let Some(index) = self
            .window
            .tabs
            .iter()
            .position(|tab| tab.id.0 == route.tab)
        else {
            attention_trace::line(|| format!("open route={launch} leave=no-tab"));
            return Ok(());
        };
        // Un-minimised first: `focus_window` on an iconified window brings it forward without
        // restoring it on some configurations, and a restored window that never came forward is
        // the same click going unanswered twice.
        let restored = self.window.window.is_minimized() == Some(true);
        if restored {
            self.window.window.set_minimized(false);
        }
        hang_watch::during(hang_watch::Station::WindowFocus, || {
            self.window.window.focus_window()
        });
        self.activate_tab(index, false)?;
        let seat = route.seat_id();
        if !self.window.tabs[index].sessions.contains_key(&seat) {
            attention_trace::line(|| {
                format!(
                    "open tab={index} seat={seat:?} id={} by=toast restored={} leave=no-seat",
                    route.tab,
                    u8::from(restored)
                )
            });
            return Ok(());
        }
        // `tab=` is the **index**, which is what every other station in this file
        // means by the word (`attention::Site`); `id=` is the `TabId` the toast
        // was written with. Both, because they are two different numbers and the
        // whole point of this line is being able to line it up against the
        // `toast` line that raised it.
        attention_trace::line(|| {
            format!(
                "open tab={index} seat={seat:?} id={} by=toast restored={}",
                route.tab,
                u8::from(restored)
            )
        });
        if self.window.tabs[index].focused_leaf != seat {
            self.window.tabs[index].focused_leaf = seat;
            // The slot holds the pane that *was* focused; leaving it would let the next present
            // assert a stale grid against the new one (`focus_pane_at`'s own reason).
            self.window.last_presented_frame = None;
        }
        if self.seats.set_focus(seat) {
            self.apply_window_min_inner_size()?;
            self.commit_seat_geometry()?;
            self.mark_session_dirty(Instant::now());
        }
        Ok(())
    }

    /// **回答才消费** — a user action **with a source**, put into this seat's shell, answers what
    /// it was asking (`attention` plan §10.3.2, §11.3; user ruling 2026-08-25).
    ///
    /// The door out of the queue, and it is a *write into that shell* rather than a glance at it.
    /// §7.1.5b is explicit about why: looking at a blocked agent does not unblock it, so a queue
    /// that emptied on sight would be a queue that forgot exactly the things it exists to remember.
    ///
    /// **It used to be `Enter` alone, and that was the third of the four defects.** Answering a
    /// permission prompt in Claude Code is `1`, `y`, `Esc` or `↓`; you replied to the agent and the
    /// badge stayed lit. The door is now "you put a byte in its mouth on purpose", which is the only
    /// honest converse of "looking does not count" — a look produces no bytes.
    ///
    /// **The kind is the caller's, and it is a kind rather than a byte string.** Six of the seven
    /// answer; the seventh is a forwarded pointer *sweep*, which is not an answer and cannot be
    /// spelled as one ([`UserInputKind::answer_kind`]). The terminal's own replies — a
    /// device-attributes answer, a colour report, the focus reports of A0.5, the PSReadLine repair —
    /// are not writes of a *kind* at all and never reach this door; they are structurally out of
    /// reach rather than excluded by a list (red line 10).
    ///
    /// Silent when the seat has nothing unanswered, which is the ordinary case: every keystroke in
    /// every shell comes through here, and almost none of them is answering anything.
    pub(in crate::runtime) fn answer_attention(&mut self, seat: SeatId, by: UserInputKind) {
        let index = self.window.active_tab;
        let reach = notify::desktop_reach(true, self.window.place());
        // Split rather than reached through `self`: the place is drawn from the window's own serial
        // while the account it lands in is a field of one of that window's tabs.
        let WindowRuntime {
            tabs,
            attention_next_place,
            ..
        } = &mut self.window;
        answer_attention_in(
            &mut tabs[index],
            index,
            seat,
            by,
            reach,
            attention_next_place,
            Instant::now(),
            attention_trace::global(),
        );
    }

    /// **A tab was looked at, offered to every account it holds** (`attention` plan §10.9).
    ///
    /// The arrival with nothing to say, and saying it is the point: [`TabState::mark_seen`] one
    /// line up retires this tab's bells and failure codes, and a reader of these two lines has to be
    /// able to see that the standing requests were *offered the same event and kept*. A rule that
    /// held only because nobody had written the call is a rule one edit away from not holding.
    pub(in crate::runtime) fn mark_attention_seen(&mut self, index: usize) {
        let reach = notify::desktop_reach(true, self.window.place());
        // A look is not a program speaking, so this instant stamps nothing (`attention` plan
        // §11.10.4 — [`attention::Event::is_the_programs_voice`] answers `MarkSeen` with `false`).
        // It is read here rather than threaded down from a frame because a tab switch is not part
        // of one: there is no animation being sampled, and the moment the switch happens is the
        // only moment this arrival is about.
        let now = Instant::now();
        let WindowRuntime {
            tabs,
            attention_next_place,
            ..
        } = &mut self.window;
        for (seat, leaf) in tabs[index].leaves_mut() {
            let at = attention::Site {
                tab: index,
                seat: *seat,
            };
            let lines = leaf
                .attention
                .apply(
                    at,
                    reach,
                    attention::Event::MarkSeen,
                    attention_next_place,
                    now,
                )
                .lines;
            emit_attention_lines(attention_trace::global(), lines);
        }
    }

    /// `Ctrl+Shift+A` — go to the session that has been waiting longest, and put
    /// the keyboard in it (§7.1.5b P1-8).
    ///
    /// **One transaction, and it is the card's own.** The tab switch is
    /// [`Runtime::activate_tab`] — the very call `release_tab_press` makes when a
    /// card in the focus column is clicked, reached through the same
    /// `ChromeTarget::Tab(index)` the column answers with. There is no second way
    /// to bring a tab up, in focus mode or out of it, which is what makes
    /// "聚焦态下跳转 = 换上舞台" true by construction rather than by a branch: the
    /// mode changes what the stage is drawn beside, and this verb never mentions
    /// it.
    ///
    /// **The pane, and not only the tab.** A jump that landed on the tab and left
    /// the keyboard in whichever pane it was last in would be a jump that puts
    /// the answer in the wrong shell. The move is guarded on the tab actually
    /// holding a session at that seat — the same guard every focus move in this
    /// window carries — so a stop whose seat is not a terminal lands the tab and
    /// stops there, which is §7.1.5b's own answer for a non-terminal stage: 落
    /// tab,由卡片橙框指路.
    ///
    /// **Nothing is consumed here.** Arriving is looking, and looking is not
    /// answering; the place stands until [`Runtime::answer_attention`] retires it — or until the
    /// program takes back what it asked. That is what makes a second press walk on to the next one
    /// instead of finding the queue one shorter than it was.
    pub(in crate::runtime) fn jump_to_attention(&mut self) -> Result<()> {
        let queue: Vec<((usize, SeatId), u64)> = self
            .window
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(index, tab)| {
                tab.sessions.iter().filter_map(move |(seat, leaf)| {
                    leaf.attention
                        .ticket()
                        .map(|ticket| ((index, *seat), ticket))
                })
            })
            .collect();
        let active = self.window.active_tab;
        let standing_on = self.window.tabs[active]
            .sessions
            .get(&self.focused_leaf)
            .and_then(|leaf| leaf.attention.ticket());
        let waiting = queue.len();
        let Some((tab, seat)) = next_attention_stop(&queue, standing_on) else {
            attention_trace::line(|| format!("jump queue={waiting} from=none to=none"));
            return Ok(());
        };
        attention_trace::line(|| {
            let ticket = queue
                .iter()
                .find(|(place, _)| *place == (tab, seat))
                .map(|(_, ticket)| *ticket)
                .unwrap_or_default();
            let from =
                standing_on.map_or_else(|| String::from("none"), |ticket| ticket.to_string());
            format!("jump queue={waiting} from={from} to=tab={tab},seat={seat:?},ticket={ticket}")
        });
        self.activate_tab(tab, false)?;
        if self.window.tabs[tab].sessions.contains_key(&seat) {
            self.focus_seat(seat)?;
        }
        // The tab switch published a frame with the focus where it *was*; the
        // move above happened after it. `set_focus_mode`'s own tail, for the same
        // reason — a verb that changes what the chrome says has to say so.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }
}
