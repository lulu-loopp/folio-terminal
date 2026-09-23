//! `web` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    AppEvent, LeafId, PageKeepsake, PreviewSurface, RenameExit, Runtime, TabRename, WebHeadVerb,
    WebPlacement, a_page_is_off_the_glass, a_page_still_has_a_pane, a_page_was_replaced,
    a_retirement_happens_on_this_turn, hang_watch, hole_for, input, marks, native_window, preview,
    preview_image_placement, restore, revived_page_of, seats, shown_address, web_mouse_button,
    web_trace, webhost, webnav, websheet,
};
use anyhow::Result;
use bt_layout::SeatId;
use std::collections::BTreeSet;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};

impl Runtime<'_> {
    /// **The page on one seat of the tab in front, and the one door to it** —
    /// every reader of a hosted page goes through this pair.
    ///
    /// It is a pair of one-line functions on purpose: it was written when
    /// `WindowRuntime::web` was one page, because `BT_WEB_DEV` was the only way
    /// to reach one, and every caller already asked "the page on *this* seat".
    /// Slice ③ made the field a map keyed by seat, F1b′ re-keyed it by
    /// [`LeafId`], and on both days the lookup changed here and in no other
    /// place — which is what the pair was for.
    pub(in crate::runtime) fn web_on(&self, seat: SeatId) -> Option<&webhost::WebSeat> {
        self.window.web.get(&self.leaf_here(seat))
    }

    pub(in crate::runtime) fn web_on_mut(&mut self, seat: SeatId) -> Option<&mut webhost::WebSeat> {
        let leaf = self.leaf_here(seat);
        self.window.web.get_mut(&leaf)
    }

    /// **The download sheet, when a page is showing one** (§7.7 ④, W2 slice ④).
    ///
    /// Its rectangle is stored beside the layer for the search capsule's own
    /// reason: the press router is `&self` and cannot lay anything out, so the
    /// box you can press has to be the box that was drawn.
    pub(in crate::runtime) fn web_sheet_layers(&mut self) -> Vec<marks::OverlayLayer> {
        self.window.web_sheet_layouts.clear();
        // **Every page of *this tab* that is showing one** (user ruling
        // 2026-09-06). A sheet is drawn over the body of the page it came from,
        // so the walk is this tab's own preview seats: the window's map spans
        // every tab, and a page on a tab that is not on the glass has no body
        // rectangle here to stand on. Two pages side by side are two cards, each
        // in its own pane — the same sentence the pane heads, the switchers and
        // the search capsules already say once per seat.
        let standing: Vec<(SeatId, String, String, String)> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| {
                let fault = self.web_on(seat)?.fault()?;
                fault.stands_over_the_page().then(|| {
                    (
                        seat,
                        fault.say(),
                        fault.detail().unwrap_or_default().to_owned(),
                        fault.verb_text().text().to_owned(),
                    )
                })
            })
            .collect();
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let palette = bt_render::chrome_palette();
        let mut layers = Vec::new();
        for (seat, say, detail, verb) in standing {
            let Some(body) =
                seats::preview_seat_body_rect(&self.seats, &self.seat_layout, seat, scale)
            else {
                continue;
            };
            let width = self.window.renderer.measure_chrome_text(
                &mut self.app.gpu,
                &verb,
                websheet::verb_font_px(scale),
            );
            // **The sentence is wrapped before the card is laid out**, because
            // how many lines it takes is what decides how tall the card is. The
            // wrap is `restore::wrap` — the one in this window that knows a Latin
            // word may not be broken and a run of ideographs may — because a
            // second one would be a second answer about where a line ends.
            let say_font = websheet::say_font_px(scale);
            let say_width = websheet::say_width(body, scale);
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let say_lines = restore::wrap(&say, say_width, |text| {
                renderer.measure_chrome_text(gpu, text, say_font)
            });
            let layout = websheet::lay_out(body, width, say_lines.len(), !detail.is_empty(), scale);
            layers.push(websheet::build(
                &layout,
                websheet::SheetContent {
                    say: &say_lines,
                    detail: &detail,
                    verb: &verb,
                    verb_hovered: self.window.seat_pointer.hover
                        == Some(seats::ChromeTarget::PreviewFaultVerb(seat)),
                    close_hovered: self.window.seat_pointer.hover
                        == Some(seats::ChromeTarget::PreviewSheetClose(seat)),
                },
                &palette,
            ));
            self.window.web_sheet_layouts.push((seat, layout));
        }
        layers
    }

    /// [`Self::web_on`] for a surface — the pair's second half, moved across.
    pub(in crate::runtime) fn web_of(&self, surface: PreviewSurface) -> Option<&webhost::WebSeat> {
        self.window.web.get(&self.rail_page(surface)?)
    }

    /// **Open one web address on this window's own seat**, or report that the
    /// door would not have it (§7.1.5g ⑥).
    ///
    /// One door and not a second: `webnav::address_bar` is what an address typed
    /// into the head, restored from a session or read out of `pins.json` goes
    /// through, and [`Self::open_web_page`]'s own contract is that every caller
    /// has passed it first. A link printed in the terminal and a link written
    /// into a document are strings from outside exactly as those are, so they
    /// are judged by the same rule and land on the same seat — which is also
    /// what keeps two surfaces from giving two answers about one string.
    ///
    /// `false` means the door refused, and the *caller* says so: the two callers
    /// have two mouths — the terminal's hover line under the cells the address
    /// is printed in, and a notice standing on the preview surface — and neither
    /// can speak for the other's surface. What must not differ, and does not, is
    /// the judgement.
    ///
    /// `Search` is unreachable from a string that already carries a scheme —
    /// `webnav::check` only offers one where there is none — and is folded in
    /// with the refusal rather than given a verb of its own, so that an arm
    /// which can never run cannot grow a behaviour nobody meant.
    pub(in crate::runtime) fn open_web_address_here(&mut self, url: &str) -> Result<bool> {
        match webnav::address_bar(url) {
            webnav::Decision::Navigate(url) => {
                self.open_web_page(&url)?;
                Ok(true)
            }
            webnav::Decision::Refuse(_) | webnav::Decision::Search(_) => Ok(false),
        }
    }

    /// Put the page where the seat is — and punch the hole that lets it be
    /// seen — or take both away.
    ///
    /// **The two halves are one decision and are made here together.** The page
    /// is composed *under* wgpu's visual, so it is visible exactly where this
    /// surface is transparent; a frame that moved one and not the other would be
    /// a page peeping out beside its own pane, and there is no third place the
    /// two could be reconciled.
    pub(in crate::runtime) fn sync_web_page(&mut self, now: Instant) {
        // **A station inside a frame** (see [`hang_watch::Station::WebPlace`]).
        // This runs from `pane_draws`, which is inside whatever the frame's own
        // station is, so the one being left is put back below rather than the
        // rest of the frame being charged here. The empty-window arm returns
        // before it, because a window with no page makes no call worth timing.
        if self.window.web.is_empty() {
            self.window.renderer.set_web_holes(Vec::new());
            // A window with no page has no page holding its keyboard, and the
            // edge is asked here too: the last page in a window can be closed
            // while it is the one being typed into, and the keys have to come
            // back to a window that no longer has anywhere to ask.
            self.settle_the_web_keyboard();
            return;
        }
        let leaving_station = hang_watch::enter(hang_watch::Station::WebPlace);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let motion = self.app.motion;
        let obstructed = self.a_modal_covers_the_window();
        // Every page, and its answer read off the tab that holds it — a page on a
        // tab nobody is looking at has no placement in *this* tab's layout, and
        // `preview_image_placement` says so by answering `None`, which is the
        // same `Hidden` a page under a modal gets.
        let pages: Vec<LeafId> = self.window.web.keys().copied().collect();
        let active = self.window.active_tab;
        let mut placements: Vec<WebPlacement> = Vec::with_capacity(pages.len());
        // **And the pages a modal is standing over**, gathered on the same walk
        // because the answer is made of the same five facts (§7.8 ⑩). Empty on
        // every frame no dialog is open.
        let mut keepsakes: Vec<PageKeepsake> = Vec::new();
        for leaf in pages {
            let seat = leaf.seat;
            let transform = self.window.pane_motion.transform_of(seat, now, motion);
            // **Each page measured against its own tab's tree**, not against
            // whichever tab is in front. A page on a background tab still has a
            // rectangle, and the engine still has to be told its size — see
            // `webhost::WebSeat::apply_presence`. Asking the active tab would
            // answer `None` for every page but one, which is how a restored
            // window's second page came up against zero by zero and never loaded.
            //
            // The page names its own tab since F1b′, so the tab is found by id
            // rather than by asking every tree which one holds a seat with this
            // number — a question two of them could answer yes to.
            let Some(index) = self.window.tabs.iter().position(|tab| tab.id == leaf.tab) else {
                placements.push(WebPlacement {
                    leaf,
                    presence: webhost::WebPresence::Hidden,
                    rect: None,
                    above: None,
                    // A page with no rectangle has nothing standing over it.
                    cover: Vec::new(),
                });
                continue;
            };
            // **A seat that has been asked to go is already gone**, whatever
            // rectangle its pane still has: its visual left the composition tree
            // with `host.close()`, and a hole cut for it would be the desktop
            // showing through the document that replaced it - for as long as ten
            // seconds, which is what the browser-exit wait can be.
            if self
                .window
                .web
                .get(&leaf)
                .is_some_and(webhost::WebSeat::is_closing)
            {
                placements.push(WebPlacement {
                    leaf,
                    presence: webhost::WebPresence::Hidden,
                    rect: None,
                    above: None,
                    // A page with no rectangle has nothing standing over it.
                    cover: Vec::new(),
                });
                continue;
            }
            // **A page that has been popped out is measured against its float**
            // (§7.14a). Its seat left the tree when the window opened, so the
            // layout has no rectangle for it and never will again; the float's
            // body is where it lives now. And a float floats across tab switches
            // — that is what §7.1.2 made it for — so the page inside one is not
            // hidden by the tab behind it changing.
            let floated = self.float_holding_the_page(leaf);
            // **And whether this pane is reading the page's source instead**
            // (§7.7 ⑭). Asked of the surface the page is drawn on, which is the
            // float when one is carrying it — the flip is the *view's*, and a
            // page carried into a window keeps the face it was turned to.
            let sourced = self
                .page_source_shown_on(match floated {
                    Some(id) => PreviewSurface::Float(id),
                    None => PreviewSurface::Seat(leaf),
                })
                .is_some();
            let tab = &self.window.tabs[index];
            let body = match floated {
                // **The page's own rectangle, off the corners and the grip**
                // (§7.39). A float is the page's pane, and its rounded face and
                // its resize grip are the two things a single opaque engine
                // rectangle would otherwise cover; `float_body_rect` is the body
                // raised clear of both, and it is the bounds, the hole and the
                // pointer region all at once.
                Some(id) => self.float_inset_body_rect(id, scale),
                None => {
                    preview_image_placement(&tab.seats, &tab.seat_layout, seat, scale, transform)
                        .map(|placement| {
                            [
                                placement.seat.x as f32,
                                placement.seat.y as f32,
                                (placement.seat.x + placement.seat.width) as f32,
                                (placement.seat.y + placement.seat.height) as f32,
                            ]
                        })
                }
            };
            // **A seat showing a card has no page to show** (§7.7 ④, W2 slice
            // ④). Four of the five failures replace the seat's content, and the
            // card is drawn as ordinary pane chrome — which the transparency
            // hole would erase, since it is punched over everything the seats
            // draw (§7.8 ②). So the page goes first and the hole with it, which
            // is also the honest account: a WebView that is dead, absent or
            // refused has nothing to put there. Asked of *this* seat's own
            // fault, because a card is a seat's state and not the window's.
            let carded = self
                .window
                .web
                .get(&leaf)
                .and_then(webhost::WebSeat::fault)
                .is_some_and(|fault| !fault.stands_over_the_page());
            // The size comes off the rectangle whatever is standing over it; the
            // presence is the second question, and a page on a tab nobody is
            // looking at answers it the same way a page under a modal does.
            let rect = webhost::web_presence(body, false).bounds();
            // **The four reasons that are not the modal**, asked separately
            // because the difference is what a pane draws (§7.8 ⑩): a page the
            // dialog alone took away is a page the reader was looking at a
            // moment ago and expects to still be there under the scrim, and one
            // of the other four is a page that has no business on this glass at
            // all.
            let elsewhere =
                a_page_is_off_the_glass(false, floated.is_some(), index == active, carded, sourced);
            let presence = webhost::web_presence(body, obstructed || elsewhere);
            if obstructed
                && !elsewhere
                && let webhost::WebPresence::Shown(bounds) = webhost::web_presence(body, false)
            {
                keepsakes.push(PageKeepsake {
                    leaf,
                    rect: bounds.as_rect(),
                    float: floated,
                });
            }
            // **And where in the stack the hole for it is punched** (§7.14c). A
            // float is not a surface standing *over* this page — it is the pane
            // the page is in — so the hole has to be punched above the float's
            // own face, which is drawn a whole overlay pass after the seats are.
            // A docked page keeps the older answer, `None`, which is under the
            // entire stack.
            let above = floated.and_then(|id| self.float_hole_level(id));
            // **One line per decision, and none while the answer stands still**
            // — `BT_WEB_TRACE`'s fourth station, and the one that separates the
            // ways a page comes up empty: it was never given a rectangle, it was
            // given one and hidden, it was hidden by a card, it was placed and
            // the hole was never cut, or — the fifth, and the one the 2026-08-25
            // report cost a whole ticket to see — the hole was cut in a place
            // something drew over afterwards. `above=` is that last one: the
            // level the hole stands on, `-` for the seats' own place under the
            // whole stack. Change-gated against what the seat was last asked, so
            // a still page is silent.
            if self
                .window
                .web
                .get(&leaf)
                .is_some_and(|web| web.wanted() != presence)
            {
                web_trace::line(|| {
                    format!(
                        "place tab={} seat={} floated={} body={} presence={presence:?} \
                         above={} obstructed={} carded={} front={} sourced={}",
                        leaf.tab.0,
                        seat.0,
                        floated.map_or_else(|| String::from("-"), |id| id.to_string()),
                        body.map_or_else(|| String::from("none"), |rect| format!("{rect:?}")),
                        above.map_or_else(|| String::from("-"), |level| level.to_string()),
                        u8::from(obstructed),
                        u8::from(carded),
                        u8::from(index == active),
                        u8::from(sourced),
                    )
                });
            }
            let cover = self.chrome_over(above, body);
            placements.push(WebPlacement {
                leaf,
                presence,
                rect,
                above,
                cover,
            });
        }
        // **A page holds the keyboard only while it is what typing goes into**
        // (§7.7 ④, W2 slice ④, generalised 2026-08-24). Asked once a frame
        // because most of the ways a page stops being it are silent: a scrim
        // rises, a card replaces the seat, the reader presses the pane next door,
        // a field opens on the page's own head. The engine reports the focus it
        // *takes* and nothing at all about the focus it should no longer have.
        self.settle_the_web_keyboard();
        let window = &mut *self.window;
        let mut holes = Vec::new();
        for placement in placements {
            // **The placement is what answers the hole**, so it is asked for its
            // answer rather than only for its failure: a page that could not be
            // placed has no floor, and a refusal that only reached `stderr`
            // while the hole was cut anyway is the shape of the defect this
            // whole slice is about.
            let floored = match window.web.get_mut(&placement.leaf) {
                Some(web) => {
                    match web.place(
                        &window.compositor,
                        placement.presence,
                        placement.rect,
                        &placement.cover,
                    ) {
                        Ok(floored) => floored,
                        Err(error) => {
                            eprintln!("BT_WEB place failed: {error}");
                            false
                        }
                    }
                }
                None => false,
            };
            holes.extend(hole_for(placement.presence, floored, placement.above));
        }
        window.renderer.set_web_holes(holes);
        self.keep_what_the_modal_covers(keepsakes, now);
        hang_watch::at(leaving_station);
    }

    /// **Take the keyboard back from a page that has stopped being what typing
    /// goes into** (§7.7 W2 片④ ⑧, extended by the user report of 2026-08-24).
    ///
    /// The falling edge, and only the falling edge. Nothing here ever *gives* a
    /// page the keyboard: that is a press inside it ([`Self::press_web_page`]),
    /// which is the one door it has ever had, and a rising edge that focused the
    /// engine on its own account would be this window typing into a document
    /// nobody clicked on.
    fn settle_the_web_keyboard(&mut self) {
        let Some(held) = self.window.web_keyboard else {
            return;
        };
        if self.page_is_the_typing_target(held) {
            return;
        }
        self.window.web_keyboard = None;
        if let Ok(native) = native_window(&self.window.window)
            && let Err(error) = bt_platform::take_keyboard_focus(native)
        {
            eprintln!("BT_WEB focus return failed: {error}");
        }
    }

    /// Read everything this window's pages have said, and do it.
    pub(crate) fn drive_web_page(&mut self) -> Result<()> {
        if self.window.web.is_empty() {
            return Ok(());
        }
        let window = &mut *self.window;
        let outcomes: Vec<(LeafId, Vec<webhost::WebOutcome>)> = window
            .web
            .iter_mut()
            .map(|(leaf, web)| (*leaf, web.drive(&window.compositor)))
            .collect();
        for (leaf, outcomes) in outcomes {
            self.apply_web_outcomes(leaf, outcomes)?;
        }
        // **And the pictures the worker finished.** Read on the same beat the
        // engine's own words are read on, because the shrinker wakes the loop
        // through the very same event: a card with a new frame is a page having
        // spoken, one thread further out.
        self.collect_page_pictures();
        // **Everything the head, the foot and the cards read arrives here**
        // (§7.7 ②, ③, ④, W2 slice ④): the title, the address, the two history
        // flags, the hover line, the failure. Slice ① needed no repaint because
        // no chrome read the page; every one of those is chrome now, and an
        // engine that speaks between frames would otherwise be a head that says
        // what was true a navigation ago.
        //
        // Asked of `refresh_chrome` rather than tracked per field: it already
        // answers "did anything move", which is the same question and one
        // answer.
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **What one page said, paid** — addressed by the pane it was said from.
    ///
    /// A [`LeafId`] and not a seat number since F1b′, because an engine answers
    /// from whatever tab it lives on: the window's map spans them all, and a
    /// number read out of one tab names a different pane in the next.
    pub(crate) fn apply_web_outcomes(
        &mut self,
        leaf: LeafId,
        outcomes: Vec<webhost::WebOutcome>,
    ) -> Result<()> {
        for outcome in outcomes {
            match outcome {
                // The chord the page was not allowed to see. It runs on the
                // ordinary door, so that a verb pressed over a web seat and the
                // same verb pressed over a terminal are the same verb.
                webhost::WebOutcome::Run(action) => self.run_shortcut(action)?,
                // The Tab contract's far edge: inside a page `Tab` walks the
                // page's own controls, and at the end of them the keyboard comes
                // back to this window. One call, because a window with one
                // `HWND` and no Win32 controls of its own has exactly one place
                // for the keyboard to come back *to* — and once it is there the
                // next `Tab` is Folio's, which is what "handed back" has to mean
                // before there is a pane order to hand it into.
                webhost::WebOutcome::FocusLeftThePage { .. } => {
                    let native = native_window(&self.window.window)?;
                    if let Err(error) = bt_platform::take_keyboard_focus(native) {
                        eprintln!("BT_WEB focus return failed: {error}");
                    }
                }
                webhost::WebOutcome::Cursor(id) => {
                    self.window.web_cursor = Some(id);
                    self.apply_pointer_cursor();
                }
                // **A sound started or stopped, so the strip is drawn again**
                // (user ruling 2026-08-27; §7.23 ⑩). The bit itself is already
                // on the seat and the strip reads it there — what travels is the
                // change, because a mark that appeared only on the next frame
                // something else happened to repaint would be a mark that
                // sometimes arrived a minute late.
                webhost::WebOutcome::PlayingAudioChanged => {
                    self.refresh_chrome();
                    self.present_chrome_change()?;
                }
                // **A page asked for a dialog and was answered** (R1-21). The
                // moment is already on the seat and the foot reads it there;
                // what travels is the repaint, for the sound's reason exactly —
                // a page answered while nobody is moving the mouse would
                // otherwise say nothing until something else redrew the band.
                webhost::WebOutcome::DialogDismissed => {
                    self.refresh_chrome();
                    self.present_chrome_change()?;
                }
                // **The engine took the keyboard; whether it may keep it is this
                // window's answer, not its own** (§7.7 W2 片④ ⑧, user report
                // 2026-08-24). A controller focuses itself on its own account —
                // one made visible again restores the focus it had when it was
                // hidden — and this event is the only notice of it, arriving a
                // message pump after whatever caused it. Ignoring it is how a
                // page came to be holding every key while the surface this window
                // was drawing the caret in was the address field on that page's
                // own head: opened, selected, and impossible to type in, escape
                // from or click away.
                //
                // `LostFocus` is the same fact from the other side and is where
                // the receipt is torn up, because the keyboard leaving a page is
                // not always this window taking it.
                webhost::WebOutcome::PageFocus(focused) => {
                    if focused {
                        // The receipt is written first either way — it names the
                        // page that has the keys *right now* — and the one place
                        // that decides whether it may keep them decides here too.
                        self.window.web_keyboard = Some(leaf);
                        self.settle_the_web_keyboard();
                    } else if self.window.web_keyboard == Some(leaf) {
                        self.window.web_keyboard = None;
                    }
                }
                // **The page committed, so this seat now has an identity** (slice
                // ③). One row in the tab's pool, keyed by `webnav::switcher_key`
                // of the URL the engine actually loaded — which is why a redirect
                // leaves one row and not two, and why a navigation that never
                // committed leaves none at all.
                webhost::WebOutcome::Committed => {
                    // **A different page is a different picture** (§7.11). Said
                    // here because this is the one outcome that means the seat's
                    // identity moved, and a card that went on showing the last
                    // page's frame would be the only surface in the column
                    // saying something that is not true of the tab it names.
                    self.window.web_thumbs.invalidate(leaf);
                    // And a page that has arrived somewhere is not a door
                    // anybody can still take back (§7.7 ⑨). Said at the two
                    // outcomes that end a blank page's life — this one and
                    // `Gone` — as well as where the address was taken, because a
                    // switcher row and a restore can move a seat without any
                    // field being open.
                    self.forget_a_blank_page(leaf);
                    self.commit_web_page(leaf)?;
                }
                webhost::WebOutcome::Gone => {
                    self.forget_a_blank_page(leaf);
                    self.window.web.remove(&leaf);
                    self.window.web_cursor = None;
                    let live: BTreeSet<LeafId> = self.window.web.keys().copied().collect();
                    self.window.web_thumbs.retain(&live);
                    if self.window.web.is_empty() {
                        self.window.renderer.set_web_holes(Vec::new());
                    }
                }
                // **A capture came back.** The bytes go to the worker; nothing
                // is decoded on this thread, because a page-sized PNG measured
                // 18 ms to decode and resample and the whole projection budget
                // is 3.0 (gate 11).
                webhost::WebOutcome::Captured { png, source } => {
                    let url = self
                        .window
                        .web
                        .get(&leaf)
                        .and_then(webhost::WebSeat::identity)
                        .unwrap_or_default()
                        .to_owned();
                    if let Some(job) = self.window.web_thumbs.arrived(leaf, &url, png, source) {
                        self.page_shrinker().send(job);
                    }
                }
                // Still said out loud, and now also drawn where §7.7 ④ says: the
                // seat keeps the refusal as its own card and this line stays,
                // because the difference between "the policy stopped it" and "it
                // went and came back" is not otherwise visible from outside the
                // process.
                webhost::WebOutcome::Refused(uri) => eprintln!("BT_WEB refused {uri}"),
                // What no card covers. The five §7.7 ④ states are drawn by the
                // seat itself; this is the residue, and it goes where `BT_DPI`
                // goes for its reason — a fact with nowhere to be drawn is still
                // a fact.
                webhost::WebOutcome::Fault(text) => eprintln!("BT_WEB {text}"),
                webhost::WebOutcome::FindMatches { count, active } => {
                    self.web_find_reported(count, active)?;
                }
                // **What one page learned about its site's icon, filed for the
                // whole application** (the favicon slice, `docs/DESIGN.md` §7.13, §7.7 ②).
                //
                // Filed here and drawn nowhere: every surface that draws a page
                // already asks the store while it builds its marks, so what this
                // owes is the store and one repaint. The repaint is asked for
                // only when something actually changed — an icon re-announced
                // unchanged, which is what a reload does, must not cost a frame.
                //
                // Decoding happens inside `learn`, on this thread. It is a
                // 32-pixel PNG arriving once per site, not the 1146-pixel
                // photograph §7.11 ③ ⓑ had to move off the render thread; the
                // measurement is in §7.13.
                webhost::WebOutcome::Favicon { site, png } => {
                    let changed = match png {
                        Some(png) => self.app.favicons.borrow_mut().learn(&site, &png),
                        None => self.app.favicons.borrow_mut().forget(&site),
                    };
                    // This window is repainted by `drive_web_page`'s own tail,
                    // which already asks "did anything move" once for every
                    // fact a page can change. What that tail cannot reach is
                    // **the other windows**: the store is the application's, so
                    // a second window standing on the same server is now wearing
                    // a different icon and has no other reason to redraw.
                    self.app.favicons_changed |= changed;
                }
            }
        }
        Ok(())
    }

    /// The clock the browser-exit deadline is hung on, the seat's own mortality,
    /// and the chord list the focus keeps changing.
    pub(in crate::runtime) fn advance_web_page(&mut self, now: Instant) -> Result<()> {
        hang_watch::at(hang_watch::Station::WebPage);
        if self.window.web.is_empty() {
            return Ok(());
        }
        // **A page goes when the seat stops being a page** - either because the
        // seat has left the tree, or because something else has landed on it.
        // Asked of every tab and not of the active one: a web seat on a tab
        // nobody is looking at is still a web seat.
        // The page names its own tab since F1b′, so "is this pane still a page"
        // is asked of that one tab rather than of whichever tab happens to hold a
        // seat with the same number.
        let orphaned: BTreeSet<LeafId> = self
            .window
            .web
            .keys()
            .copied()
            .filter(|leaf| {
                // **A page that has been popped out has not lost its pane; it
                // has taken it with it** (§7.14a). Its seat is closed out of the
                // tree by design, so the tree answers "no pane" for the one case
                // where the browser must *not* be closed — and that answer was
                // the whole of why a popped-out page came up empty.
                let floated = self.float_holding_the_page(*leaf);
                let surface = match floated {
                    Some(id) => PreviewSurface::Float(id),
                    None => PreviewSurface::Seat(*leaf),
                };
                !self
                    .window
                    .tabs
                    .iter()
                    .filter(|tab| tab.id == leaf.tab)
                    .any(|tab| {
                        a_page_still_has_a_pane(
                            floated.is_some(),
                            tab.seats.preview_seats().contains(&leaf.seat),
                            tab.preview_panes.get(surface).is_some_and(|pane| {
                                a_page_was_replaced(
                                    pane.image.as_ref().map(|image| image.path.as_path()),
                                    pane.buffer.as_ref(),
                                )
                            }),
                        )
                    })
            })
            .collect();
        // **Which of them is being asked to go on *this* turn** (§7.10 ④‴), read
        // before the loop below tells any of them, because the loop is what makes
        // the answer false. See [`a_retirement_happens_on_this_turn`] for what
        // asking it every turn instead cost.
        let retiring = a_retirement_happens_on_this_turn(
            orphaned
                .iter()
                .filter_map(|leaf| self.window.web.get(leaf))
                .map(webhost::WebSeat::is_closing),
        );
        let focus = self.shortcut_focus();
        let window = &mut *self.window;
        let mut outcomes: Vec<(LeafId, Vec<webhost::WebOutcome>)> = Vec::new();
        for (leaf, web) in &mut window.web {
            let mut theirs = Vec::new();
            // The chords the page may not keep change with the focus, and the
            // focus changes without anything telling the engine so.
            web.set_claims(&self.app.shortcuts, focus);
            if orphaned.contains(leaf) {
                // **The one synchronous call into the browser on this path.**
                // Everything else in this function is arithmetic over maps and
                // a deadline comparison; `WebSeat::close` reaches
                // `ICoreWebView2Controller::Close`, which runs the page's own
                // teardown in the browser process while this thread waits. It
                // is stationed on its own so that a hold the ledger blames on
                // this turn names four lines of code rather than a hundred —
                // see [`hang_watch::Station::WebRetire`].
                let leaving = hang_watch::enter(hang_watch::Station::WebRetire);
                theirs.extend(web.close(&window.compositor));
                hang_watch::at(leaving);
            }
            theirs.extend(web.tick(now, &window.compositor));
            outcomes.push((*leaf, theirs));
        }
        // **The other half of this turn's cost, stationed apart from the
        // first** — see [`hang_watch::Station::WebOutcomes`] for the 659 ms
        // measurement that made the two worth telling apart. A controller
        // arriving is configured and navigated from here, on this thread.
        let leaving = hang_watch::enter(hang_watch::Station::WebOutcomes);
        for (leaf, theirs) in outcomes {
            self.apply_web_outcomes(leaf, theirs)?;
        }
        hang_watch::at(leaving);
        // **A retirement owes a frame** (W2 slice 5, found on the machine). The
        // hole a page is seen through is punched while a frame is being composed
        // (`sync_web_page`), and this loop runs at the tail of a turn — *after*
        // the redraw. So the frame that drew the document which replaced a page
        // still carried the page's hole, and with the window then idle there was
        // no next frame to take it out: the desktop showed through the pane
        // until something else happened to ask for one.
        //
        // `present_chrome_change` and not a bare `request_redraw`, for the
        // reason written on that function: with no frame queued a redraw finds
        // nothing to draw and skips, and this is precisely a change that belongs
        // to the chrome rather than to a shell's output.
        //
        // **On the turn the retirement happens and not on every turn after it**
        // — see `retiring` above for what the difference cost.
        if retiring {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// `BT_WEB_DEV=<url>` — open a preview seat and put that page on it.
    ///
    /// The whole of slice 1's entry, and deliberately the whole of it: no
    /// address field, no verb, no menu row, because those are slice 4's and
    /// putting a provisional one in front of a person is how a provisional one
    /// ships.
    pub(crate) fn open_development_web_page(&mut self) -> Result<()> {
        let Some(url) = webhost::development_target() else {
            return Ok(());
        };
        self.open_web_page(url)
    }

    /// **Go to a page in this tab** — the one door, whatever asked.
    ///
    /// `BT_WEB_DEV` at launch, a switcher row, a pin, a restored session: all of
    /// them arrive here, and every one of them passes `webnav::address_bar`
    /// first, at the call site, because **a pin is not a permission** (`plan.md`
    /// §3) and neither is a session file. What this function owns is the *seat*:
    /// the tab's existing page if it has one, and a landing preview seat if it
    /// has not.
    fn open_web_page(&mut self, url: &str) -> Result<()> {
        self.open_web_page_with(url, webnav::Mint::Nothing)
    }

    /// [`Self::open_web_page`] carrying the host's own note about this target.
    ///
    /// `Mint::Nothing` for every ordinary address, which is what makes the two
    /// spellings one function: an address was gated by `webnav::address_bar` at
    /// its call site and has nothing further to declare, while a controlled file
    /// entry carries the one `file:` URL it minted all the way to the engine.
    pub(crate) fn open_web_page_with(&mut self, url: &str, minted: webnav::Mint) -> Result<()> {
        // **The landing rule and nothing else** (user ruling 2026-09-06). A page
        // is a preview buffer (§7.9), so where a new one goes is the question
        // `preview_landing_surface` already answers for every other kind: the
        // first unlocked preview pane, or a freshly split one when this tab has
        // none. The arm that used to stand here — "whichever pane of this tab
        // already holds a page" — is what made a second page unreachable.
        let Some(PreviewSurface::Seat(leaf)) = self.preview_landing_surface() else {
            eprintln!("BT_WEB no preview seat could be opened for {url}");
            return Ok(());
        };
        let seat = leaf.seat;
        self.open_web_page_on(self.leaf_here(seat), url, minted)?;
        self.focus_seat(seat)
    }

    /// **Go to a page on one named pane**, without choosing the pane and without
    /// taking the focus.
    ///
    /// [`Self::open_web_page`]'s other half, split off for the door that has both
    /// answers already: a restored session names the pane *and* the page (see
    /// [`Self::revive_web_pages`]), and a restore that moved the focus would land
    /// a window on whichever tab happened to hold a page.
    ///
    /// An engine that is already on this pane is *navigated*; one that is not is
    /// built. Both are the same sentence to the recovery machine — `desired_url`,
    /// last write wins, nothing loaded until the events are installed — which is
    /// why the two arms differ only in whether there is a machine yet.
    ///
    /// **A [`LeafId`] and not a seat number, and this line is the defect F1b′
    /// repaired.** The re-check below spans the whole window, so with a bare seat
    /// number the second tab to open a page found the first tab's engine and
    /// *navigated it*: two tabs on one `.html`, one browser, and an address bar
    /// steering somebody else's page.
    pub(crate) fn open_web_page_on(
        &mut self,
        leaf: LeafId,
        url: &str,
        minted: webnav::Mint,
    ) -> Result<()> {
        let seat = leaf.seat;
        // **This page's own tab, found once.** Every caller builds the leaf out
        // of a tab of this window, so the `None` arm is a tab that closed between
        // the naming and here — nothing to open a page on, and nothing to undo.
        let Some(index) = self.window.tabs.iter().position(|tab| tab.id == leaf.tab) else {
            return Ok(());
        };
        if self.window.web.contains_key(&leaf) {
            let window = &mut *self.window;
            let outcomes = window
                .web
                .get_mut(&leaf)
                .map(|web| web.go(url, minted, &window.compositor))
                .unwrap_or_default();
            return self.apply_web_outcomes(leaf, outcomes);
        }
        let native = native_window(&self.window.window)?;
        let proxy = self.app.event_proxy.clone();
        match webhost::WebSeat::open(
            bt_platform::PageVisual {
                tab: leaf.tab.0,
                seat: seat.0,
            },
            native,
            url,
            minted,
            // **This window's scale factor, at birth** (§7.8 ⑨). Every number a
            // composition-hosted controller holds about the display it is on is
            // a number this window gave it, and the first of them has to be
            // right: a page opened on the second display lays itself out before
            // anything else happens to it.
            self.window.renderer.metrics().scale_factor,
            Box::new(move || {
                let _ = proxy.send_event(AppEvent::WebPageSpoke);
            }),
        ) {
            // **A seat, even when the engine refused to start** (§7.36). The
            // second value is what the engine said on the way in — nothing at
            // all on a machine that has one, and on a machine that has not, the
            // fault the seat is already wearing, on its way to `stderr` like
            // every other. See `webhost::WebSeat::open`.
            Ok((web, engine_said)) => {
                self.window.web.insert(leaf, web);
                // The pane stops showing whatever document it was on the moment
                // it becomes a page: one seat shows one thing, and a buffer left
                // pointed at from underneath a browser is a switcher row claiming
                // to be current while a page covers it.
                //
                // **Except the buffer that is the page itself**, which a restore
                // has already put there and which the first commit will put there
                // again: clearing it would blank the head and the foot for as long
                // as the engine takes to come up, which on a cold profile is the
                // better part of a second.
                //
                // **On this page's own tab, and named as one** (F1b′, found on
                // the machine; §7.12 ⓑ made the name carry it). What that cost,
                // measured: a window restored with a page in each of two tabs
                // cleared the first tab's buffer twice and left the second tab's
                // file buffer standing under its own engine, so
                // `advance_web_page` read that buffer as "something else landed
                // here" and closed the page it had just opened. One of the two
                // pages was gone within a frame of arriving. It was cured here
                // first, by reaching the tab through `leaf.tab` while
                // `PreviewSurface::Seat` still held a bare number; the surface
                // now carries the whole leaf, so the two lines below say it
                // rather than work around it.
                let surface = PreviewSurface::Seat(leaf);
                let showing_a_page = self.window.tabs[index]
                    .preview_panes
                    .get(surface)
                    .and_then(|pane| pane.buffer.as_ref())
                    .is_some_and(|source| source.web_url().is_some());
                if !showing_a_page {
                    self.leave_preview_buffer_in(index, surface);
                }
                // **And the picture with it** — unconditionally again, since
                // route B (2026-08-28; §7.44 ④).
                //
                // This carried an exception while a video was played by a page:
                // a player shell was the one navigation whose pane had to keep
                // its `PreviewImageState`, because that state was the pane's
                // whole account of the recording the browser was playing. There
                // is no such navigation any more. A recording is played by an
                // engine this window drives and drawn on its own glass, and it
                // never travels through this door at all — so every page that
                // reaches here is a page, and every page replaces what the pane
                // was showing.
                self.clear_preview_image_in(index, surface);
                self.apply_web_outcomes(leaf, engine_said)?;
            }
            // What is left here is the one failure that is not the engine's: no
            // `%LOCALAPPDATA%`, so there is no profile for any engine to use and
            // no seat to hang a card on.
            Err(error) => eprintln!("BT_WEB {error}"),
        }
        Ok(())
    }

    /// **Every page a restored tab was on, put back on the engine** (`plan.md`
    /// §5 片③ 恢复; §4's state machine does the rest).
    ///
    /// `create_tab_state` brings the *buffer* back — that is what makes the head,
    /// the foot, the switcher row and the tab's name right on the first frame —
    /// and this is what makes it a page again rather than a picture of one. The
    /// two are deliberately separate: a buffer is content and belongs to the tab,
    /// while an engine is a controller on a window's `HWND`, and only this side of
    /// the restore has a window.
    ///
    /// **A session file is not an authorisation either.** The URL goes through
    /// `webnav::address_bar` exactly as a pin does and for the pin's own reason:
    /// the document is editable, may have been written by an older build, and may
    /// name a target this policy no longer allows. What comes back is the
    /// *normalised* answer, so a restore navigates where a fresh open would.
    ///
    /// Asked by index, like [`Self::request_revived_previews`] beside it, because
    /// a restore builds every tab before it activates any of them.
    /// [`Self::revive_web_pages`] for every tab this window opened holding.
    ///
    /// A loop and not a call on the active tab, for the reason the head reads
    /// beside it are a loop: a page on a tab nobody is looking at is still a
    /// page, and a window that only revived the tab it opened on would leave the
    /// others showing a head and a foot over a hole.
    pub(crate) fn revive_all_web_pages(&mut self) -> Result<()> {
        for index in 0..self.window.tabs.len() {
            self.revive_web_pages(index)?;
        }
        Ok(())
    }

    /// Hand one address to whatever this machine opens `https` with.
    ///
    /// **Through the address door first, without exception.** Every string that
    /// reaches here came from somewhere — a download the engine started, the
    /// page's own committed URL, a constant of this build — and 「钉不是授权」
    /// is the same sentence about all of them: the check is at the point of use,
    /// not at the point of storage. `shell_execute` is what hands an arbitrary
    /// scheme to whatever the machine registered for it, and `address_bar` is
    /// what makes sure the scheme is not arbitrary.
    /// **And only from a press** (R1-16). Every caller is a gesture: a card's
    /// one button, a foot's one verb, a settings row's one press, a `Ctrl`
    /// click on a printed address. There is deliberately no caller that is an
    /// *event* — a page that starts a download reaches the reader through a
    /// card whose button is this call, rather than through this call directly,
    /// so one address leaving this window is always one press that asked for
    /// it.
    ///
    /// Answers the hand-off's id when the address passed the door and went to the
    /// OS hand-off lane, and `None` when the door refused it — so a caller that owes
    /// the reader a refusal can say one now, and one for the system's refusal with
    /// [`Self::if_refused`] when the lane answers (2026-09-22).
    pub(crate) fn hand_url_to_the_browser(
        &mut self,
        url: &str,
    ) -> Result<Option<crate::handoff_lane::HandoffId>> {
        let webnav::Decision::Navigate(target) = webnav::address_bar(url) else {
            return Ok(None);
        };
        Ok(Some(self.hand_off(
            bt_platform::Handoff::Address(target),
            crate::handoff_lane::Refusal {
                stderr: Some((
                    "recoverable web hand-off failure",
                    "hand a page's address to the system browser",
                )),
                program_notice: false,
            },
        )))
    }

    /// **`Ctrl+L`, and the double click on the name cell** — the address
    /// field's two doors (§7.7 ②).
    ///
    /// One function for both, which is what "the second door onto the same
    /// room" means here: the editor, its seeding and its selection are decided
    /// once, so a URL typed after a double click and one typed after the chord
    /// cannot be seeded differently.
    pub(in crate::runtime) fn open_web_address(&mut self) -> Result<()> {
        // **The page holding the keyboard, and not the seat holding it**
        // (§7.7 ⑩ 欠账, 2026-08-25; user report: `Ctrl+L` did nothing over a
        // torn-off page). `focused_web_seat` is docked-only *by design* — it
        // exists to hang chrome off a pane head, and a floated page has no head
        // in the layout — but this chord is not asking where to hang anything.
        // It is asking which browser the letters are about, and
        // `page_with_the_keyboard` has answered that for both hosts since
        // §7.14c. The field itself was already keyed by leaf and needed no
        // change at all: what was missing was a door that could name one.
        let Some(leaf) = self.page_with_the_keyboard() else {
            return Ok(());
        };
        self.open_web_address_on(leaf)
    }

    pub(crate) fn open_web_address_on(&mut self, leaf: LeafId) -> Result<()> {
        // **Whatever field was open leaves first, and the page is asked for
        // after it has** (§7.7 ⑨). Closing an address field is one of the two
        // ways a blank page is taken back, so the page this was called about can
        // stop existing between the call and this line — and a field opened over
        // a pane that has just gone is a caret nobody can see.
        // Reading the address afterwards is also simply the more truthful of the
        // two orders: what the box is seeded with is what the pane is showing
        // now.
        self.finish_rename(RenameExit::Blur)?;
        let Some(url) = self.window.web.get(&leaf).map(|web| web.page().url.clone()) else {
            return Ok(());
        };
        // **Seeded with what the row is showing** (found on a real window,
        // 2026-08-24). A seat whose one navigation was refused has no committed
        // URL, so this used to open an empty box over a row printing the address
        // in full — the reader would have had to retype what was in front of
        // them to correct one character of it. The two strings are now the same
        // pair read in the same order, which is `dress_preview_rail`'s own.
        let url = if url.is_empty() {
            self.window
                .web
                .get(&leaf)
                .and_then(webhost::WebSeat::fault)
                .and_then(webhost::WebFault::refused_address)
                .unwrap_or_default()
        } else {
            url
        };
        // **Seeded in the spelling the row is showing** (user ruling
        // 2026-08-25): a local file is a path here too, and `WebSeat::go_to`
        // mints it back into a `file:` URL on the way out. A field that opened
        // on a different spelling of what is printed above the caret would be
        // asking the reader to accept a substitution they never made.
        let url = shown_address(&url);
        // **The field takes the keyboard back from the page, and it has to**
        // (found on the machine, 2026-08-22: `Ctrl+L` opened a field nothing
        // could be typed into). A page keeps every key this window's table does
        // not claim, and §7.8 ④ measured the shape of that exactly: **a bare
        // printable key never enters `AcceleratorKeyPressed` at all**. So a
        // field raised over a page that still holds the keys is a field that can
        // be opened, selected and dismissed, and never typed in.
        //
        // The same sentence the download sheet and a modal already make, said at
        // the third surface this window stands over a page with: whatever is
        // asking for keys takes them.
        if let Ok(native) = native_window(&self.window.window) {
            let _ = bt_platform::take_keyboard_focus(native);
        }
        self.window.rename = Some(TabRename::open_address(leaf, &url));
        self.window
            .rename_blink
            .reset(Instant::now(), self.app.motion);
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// `F12`, and the head's `</>` tool.
    /// **Addressed by leaf and not by seat** (§7.14c). Every other verb in
    /// [`Scope::WebPage`]'s neighbourhood hangs chrome off a pane head and so
    /// needs a seat; this one opens a window of the engine's own and needs
    /// nothing of the layout, so it answers for a page carried into a float
    /// exactly as it does for a docked one.
    pub(in crate::runtime) fn open_web_dev_tools(&mut self) -> Result<()> {
        let Some(leaf) = self.page_with_the_keyboard() else {
            return Ok(());
        };
        self.open_web_dev_tools_at(leaf)
    }

    fn open_web_dev_tools_at(&mut self, leaf: LeafId) -> Result<()> {
        if let Some(web) = self.window.web.get(&leaf)
            && let Err(error) = web.open_dev_tools()
        {
            eprintln!("BT_WEB {error}");
        }
        Ok(())
    }

    /// One of the head's three navigation buttons, or the `</>` beside them.
    pub(in crate::runtime) fn run_web_head_verb(
        &mut self,
        surface: PreviewSurface,
        verb: WebHeadVerb,
    ) -> Result<()> {
        let Some(leaf) = self.rail_page(surface) else {
            return Ok(());
        };
        if verb == WebHeadVerb::DevTools {
            return self.open_web_dev_tools_at(leaf);
        }
        let Some(web) = self.window.web.get_mut(&leaf) else {
            return Ok(());
        };
        let outcome = match verb {
            WebHeadVerb::Back => web.walk_history(false),
            WebHeadVerb::Forward => web.walk_history(true),
            WebHeadVerb::Reload => web.reload_or_stop(),
            WebHeadVerb::DevTools => Ok(()),
        };
        if let Err(error) = outcome {
            eprintln!("BT_WEB {error}");
        }
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// The one verb on the failure card this seat is showing (§7.7 ④).
    pub(in crate::runtime) fn run_web_fault_verb(&mut self, seat: SeatId) -> Result<()> {
        let Some(verb) = self
            .web_on(seat)
            .and_then(|web| web.fault())
            .map(webhost::WebFault::verb)
        else {
            return Ok(());
        };
        match verb {
            webhost::WebFaultVerb::DownloadTheRuntime => {
                self.hand_url_to_the_browser(webhost::RUNTIME_DOWNLOAD_PAGE)?;
                Ok(())
            }
            // **Not the head's reload** (user ruling 2026-08-25): the head's
            // three buttons talk to a host that exists, and this card is on
            // screen precisely because none does. It goes back to
            // `CreateCoreWebView2Environment` through the same effect a browser
            // crash takes, so the one road to a new engine stays one road.
            webhost::WebFaultVerb::RestartTheEngine => {
                let leaf = self.leaf_here(seat);
                let window = &mut *self.window;
                let outcomes = window
                    .web
                    .get_mut(&leaf)
                    .map(|web| web.restart_engine(&window.compositor))
                    .unwrap_or_default();
                self.apply_web_outcomes(leaf, outcomes)?;
                self.refresh_chrome();
                self.present_chrome_change()
            }
            webhost::WebFaultVerb::Reload => {
                let surface = self.preview_here(seat);
                self.run_web_head_verb(surface, WebHeadVerb::Reload)
            }
            webhost::WebFaultVerb::CopyAddress(address) => {
                self.copy_text_to_clipboard(&address);
                Ok(())
            }
            // The page is still there — that is the whole reason this card is a
            // sheet — so what is handed over is the address it is standing on.
            webhost::WebFaultVerb::OpenPageInBrowser => {
                let page = self
                    .web_on(seat)
                    .map(|web| web.page().url.clone())
                    .unwrap_or_default();
                self.hand_url_to_the_browser(&page)?;
                Ok(())
            }
            // **The press a cancelled download now waits for** (R1-16). The
            // address is the one the card is showing, and this is the only way
            // it leaves the window.
            webhost::WebFaultVerb::OpenDownloadInBrowser(target) => {
                self.hand_url_to_the_browser(&target)?;
                Ok(())
            }
        }
    }

    /// A press anywhere on the download sheet. Returns whether it landed there.
    pub(in crate::runtime) fn press_web_sheet(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let (x, y) = (position.x as f32, position.y as f32);
        // **The card the press landed in, and no other.** With a card per page
        // the scrim is a card's own and stops at that page's body, so a press is
        // asked of each in the order they were drawn and spent by the first that
        // covers it.
        let Some((seat, layout)) = self
            .window
            .web_sheet_layouts
            .iter()
            .find(|(_, layout)| websheet::covers(layout, x, y))
            .cloned()
        else {
            return Ok(false);
        };
        match websheet::hit(&layout, seat, x, y) {
            Some(seats::ChromeTarget::PreviewSheetClose(_)) => {
                self.dismiss_web_sheet_on(seat)?;
            }
            Some(_) => self.run_web_fault_verb(seat)?,
            None => {}
        }
        Ok(true)
    }

    /// Take one page's download sheet away — the `×` on the card itself, which
    /// names the page it is standing on.
    fn dismiss_web_sheet_on(&mut self, seat: SeatId) -> Result<bool> {
        let dismissed = self
            .web_on_mut(seat)
            .is_some_and(webhost::WebSeat::dismiss_sheet);
        if dismissed {
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        Ok(dismissed)
    }

    /// Take the download sheets away. The one card with an Escape, and the
    /// reason is that there is a page under it to come back to.
    ///
    /// Asked of this tab's pages, on `web_sheet_layers`' own argument: the sheet
    /// an Escape can be about is a sheet that is on the glass, and the window's
    /// map spans tabs nobody is looking at.
    ///
    /// **Every card standing in this tab, and not one of them** (user ruling
    /// 2026-09-06). Escape has no pointer to name a card with, and there is no
    /// keyboard focus on a card either — so the key means what it means to every
    /// other layer of this window: take away what is standing in front of the
    /// thing I am looking at. Pressing it twice to clear two cards would be a
    /// hidden order nobody can see; the `×` on each card is the way to spend
    /// them one at a time, and it names its own page.
    pub(in crate::runtime) fn dismiss_web_sheet(&mut self) -> Result<bool> {
        let seats = self.seats.preview_seats();
        let mut dismissed = false;
        for seat in seats {
            dismissed |= self
                .web_on_mut(seat)
                .is_some_and(webhost::WebSeat::dismiss_sheet);
        }
        if dismissed {
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        Ok(dismissed)
    }

    pub(in crate::runtime) fn revive_web_pages(&mut self, index: usize) -> Result<()> {
        let Some(tab) = self.window.tabs.get(index) else {
            return Ok(());
        };
        let id = tab.id;
        let pages: Vec<(LeafId, String, webnav::Mint)> = tab
            .preview_panes
            .iter()
            .filter_map(|(surface, pane)| {
                let PreviewSurface::Seat(leaf) = surface else {
                    return None;
                };
                debug_assert_eq!(leaf.tab, id, "a tab's panes are filed under its own leaves");
                let (url, minted) = revived_page_of(pane.buffer.as_ref()?)?;
                Some((leaf, url, minted))
            })
            .collect();
        for (leaf, url, minted) in pages {
            self.open_web_page_on(leaf, &url, minted)?;
        }
        Ok(())
    }

    /// **A page loaded, so the seat has an identity and the pool has a row.**
    ///
    /// The identity is `webnav::switcher_key` of what the engine actually
    /// committed — `webhost::WebSeat::identity`, which is
    /// `WebMachine::recoverable_url`, which is the *one* ledger (see
    /// [`webhost::WebOutcome::Committed`]). Three consequences fall out rather
    /// than being arranged: a redirect leaves one row because only the committed
    /// address is ever keyed; a failed navigation leaves none because the machine
    /// did not move; and re-visiting an address finds the row already there,
    /// because `PreviewPool::open` finds before it makes.
    ///
    /// The row is listed under the page's *title* once slice ④ reads one; until
    /// then it is listed under its site, which is what a Recent row and an
    /// unopened pin already print (`webnav::site_label`).
    fn commit_web_page(&mut self, leaf: LeafId) -> Result<()> {
        let Some(url) = self
            .window
            .web
            .get(&leaf)
            .and_then(webhost::WebSeat::identity)
        else {
            return Ok(());
        };
        let key = webnav::switcher_key(url);
        let source = preview::PreviewSource::Web(key.clone());
        let surface = PreviewSurface::Seat(leaf);
        // **This page's own tab, which is not always the one in front.** A page
        // commits on whatever tab it lives on, so the row goes in that tab's
        // pool: a navigation that commits while another tab is up would otherwise
        // put its buffer in that other tab's pool, and the row would appear in a
        // switcher belonging to a tab that has never been to it. Measured on the
        // real window — a page opened at launch under the restore prompt,
        // committing after the prompt had put a restored tab in front of it, left
        // its row in the restored tab and none in its own.
        //
        // Named by the key rather than searched for by seat number since F1b′:
        // the page carries its own tab, so there is nothing to look up and no
        // second tab that could answer to the same number.
        let Some(index) = self
            .window
            .tabs
            .iter()
            .position(|state| state.id == leaf.tab)
        else {
            return Ok(());
        };
        let tab = &mut self.window.tabs[index];
        let shown = tab.preview_panes.showing();
        // The name it already had where it has been here before, and its site
        // where it has not. A title is slice ④'s to read.
        let name = tab
            .preview_pool
            .get(&source)
            .map_or_else(|| webnav::site_label(&key), |buffer| buffer.name.clone());
        tab.preview_pool.open(source.clone(), name, &shown);
        // No caret and no scroll to file or restore: a page's view is the
        // engine's, and this window has never held one.
        tab.preview_panes.entry(surface).buffer = Some(source);
        self.mark_session_dirty(Instant::now());
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **Which page a point is inside**, as the pages stand on the glass this
    /// frame — and `None` for a point that is on none of them.
    ///
    /// Asked of what each seat was actually *told* (`shown_at`) rather than of
    /// the layout, which is the same rule the single-page version kept: a page
    /// hidden behind a modal is not somewhere a click can land, and the engine's
    /// own bounds are the only account of that which cannot be a frame stale.
    ///
    /// **Less whatever this window has standing on it** (§7.7 ②, ④, W2 slice
    /// ④). Two of this window's own surfaces are drawn *over* a page — the
    /// search capsule in the pane's top-right corner, and the download sheet —
    /// and both are overlay layers, which is above the transparency hole and
    /// therefore above the page (§7.8 ②). One subtraction here rather than four
    /// guards, because the press, the wheel, the hover and the cursor all ask
    /// this same question: a pointer that is over the capsule is not over the
    /// page, whatever the rectangles say. The subtraction is at this door and
    /// not at [`Self::point_is_on_the_web_page`]'s, so that the forwarders which
    /// ask *which* page also get it — a click on the capsule that is forwarded
    /// to the page underneath is the same bug read the other way round.
    ///
    /// **And the tab list is a third such surface** (user report, next23 #211).
    /// An icon rail *opens over* the panes rather than reflowing them — the
    /// stage keeps only `--railpark` clear (Q179,
    /// [`seats::RailState::terminal_inset_logical_px`]) — so the panel's whole
    /// trailing run, the `×` on every row included, stands inside the rectangle
    /// a page in the tab underneath was given. The press ladder asks the page
    /// before it asks the chrome router, so every one of those pixels was being
    /// handed to the engine: a preview-only tab's row could be hovered and lit
    /// and its `×` pressed, and the click went into the document. Nothing about
    /// the close verb was broken — `close_tab` never sees the press at all.
    ///
    /// This is [`seats::ChromeTarget::RailBody`]'s own sentence said to the one
    /// surface that is not drawn by wgpu: *a surface drawn over another surface
    /// answers for every pixel it covers*. It was already true of the files tree
    /// the rail covers; a page is the same case with a different painter.
    pub(in crate::runtime) fn web_page_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<LeafId> {
        let (x, y) = (position.x as f32, position.y as f32);
        // **And a hand that is already carrying something is not a hand the page
        // can have** (user report, 0.2.2: a pane dragged over a page could not be
        // let go of).
        //
        // First of the subtractions, because it does not depend on where the
        // pointer is at all. A gesture of this window's is a press that has not
        // finished: the release that ends it is owed to whatever the press began
        // on, and every one of those endings — the drop, the divider, the
        // scrubber, the thumb, the selection — is answered *below* the ladder's
        // page arm in [`Self::mouse_input`]. So a page that answered here
        // swallowed the release and the gesture stayed latched to the hand: the
        // landing was drawn, the pane never moved, and the next pointer move went
        // on dragging something nobody was holding any more.
        //
        // It is one subtraction and not a guard at each of the four askers for
        // the reason the three below it are one: the press, the wheel, the hover
        // and the cursor all ask this same question, and a page that is not under
        // the hand must not be under any of them either — a link lighting up
        // beneath a carried pane, or the page's own I-beam standing in for the
        // closed hand, are the same answer read on a different instrument.
        if self.a_gesture_holds_the_pointer() {
            return None;
        }
        if self.tab_list_target_at(position).is_some() {
            return None;
        }
        if self
            .window
            .web_sheet_layouts
            .iter()
            .any(|(_, layout)| websheet::covers(layout, x, y))
        {
            return None;
        }
        let inside = |box_: [f32; 4]| x >= box_[0] && x < box_[2] && y >= box_[1] && y < box_[3];
        if self
            .window
            .search_layout
            .is_some_and(|capsule| self.window.search.is_open() && inside(capsule.frame))
        {
            return None;
        }
        self.window.web.iter().find_map(|(leaf, web)| {
            let bounds = web.shown_at()?;
            (position.x >= f64::from(bounds.x)
                && position.y >= f64::from(bounds.y)
                && position.x < f64::from(bounds.x + bounds.width as i32)
                && position.y < f64::from(bounds.y + bounds.height as i32))
            .then_some(*leaf)
        })
    }

    /// Whether a point is inside any page this frame.
    pub(in crate::runtime) fn point_is_on_the_web_page(
        &self,
        position: PhysicalPosition<f64>,
    ) -> bool {
        self.web_page_at(position).is_some()
    }

    /// Forward one mouse event to a page, in the window's own coordinates.
    pub(in crate::runtime) fn send_to_web_page(
        &mut self,
        leaf: LeafId,
        event: bt_platform::WebMouseEvent,
        position: PhysicalPosition<f64>,
    ) {
        let now = Instant::now();
        let Some(web) = self.window.web.get_mut(&leaf) else {
            return;
        };
        if let Err(error) = web.send_mouse(event, (position.x as i32, position.y as i32), now) {
            eprintln!("BT_WEB {error}");
        }
    }

    /// The same, for the page a point is over — and nothing at all when it is
    /// over none.
    fn send_to_web_page_at(
        &mut self,
        event: bt_platform::WebMouseEvent,
        position: PhysicalPosition<f64>,
    ) {
        if let Some(leaf) = self.web_page_at(position) {
            self.send_to_web_page(leaf, event, position);
        }
    }

    /// The pointer, forwarded to the page — and the page told when it left.
    ///
    /// The second half is not symmetrical with the first because the engine will
    /// not take a `LEAVE` at all (`w0p-evidence.md` §1 gate 3): what tells a page
    /// the pointer has gone is a move to a point outside its rectangle, which is
    /// what the position already is on the frame the pointer crosses out.
    pub(in crate::runtime) fn drive_web_pointer(&mut self, position: PhysicalPosition<f64>) {
        if self.window.web.is_empty() {
            return;
        }
        let inside = self.web_page_at(position);
        if inside.is_none() && !self.window.web_pointer_inside {
            return;
        }
        // **The page the pointer just left hears about it too**, and it hears the
        // only thing the engine will accept: a move to somewhere it is not. A
        // window with two pages has a boundary between them, and a page that was
        // never told the hand had gone keeps a hover lit under a page it is no
        // longer under.
        let leaving: Vec<LeafId> = self
            .window
            .web
            .keys()
            .copied()
            .filter(|leaf| Some(*leaf) != inside)
            .collect();
        for leaf in leaving {
            self.send_to_web_page(
                leaf,
                bt_platform::WebMouseEvent::Move,
                PhysicalPosition::new(-1.0, -1.0),
            );
        }
        self.window.web_pointer_inside = inside.is_some();
        if let Some(leaf) = inside {
            self.send_to_web_page(leaf, bt_platform::WebMouseEvent::Move, position);
        } else {
            // The page's cursor stops being the answer the moment the pointer is
            // out of its rectangle, and the answer has to be re-asked to say so.
            self.window.web_cursor = None;
        }
        self.apply_pointer_cursor();
    }

    /// A button, over the page.
    pub(in crate::runtime) fn press_web_page(
        &mut self,
        state: ElementState,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        let down = state == ElementState::Pressed;
        if down {
            // **Blur, before the page takes the keyboard** (user report
            // 2026-08-24). `chrome_mouse_input`'s own guard says "a press
            // anywhere else — the page below very much included — is a blur that
            // commits", and it was telling the truth about everything it could
            // reach: this branch returns two rungs above it, so a press inside a
            // page ran no blur at all. What that left behind is the worst shape
            // a keyboard surface can be in — the field still standing on the
            // head, and every key from here on going into the engine, so the
            // field could not be typed in, escaped from, or clicked away.
            //
            // Before `web_page_at` is asked, not after: closing an address field
            // can take a blank page back with it (§7.7 ⑨), and a leaf read
            // before that is a pane this press would then be forwarded into
            // after it had gone.
            self.finish_rename(RenameExit::Blur)?;
        }
        let Some(leaf) = self.web_page_at(position) else {
            return Ok(());
        };
        if down {
            // **A press inside a page is a press inside its pane, and the pane
            // comes forward** — whichever kind of surface that pane is (§7.14c).
            //
            // A docked page's pane is a seat, and it takes the layout focus
            // exactly as any pane does when it is pressed (D40). A page carried
            // into a float has no seat at all: the float *is* its pane, and
            // `press_float`'s rule ⑤ — "a press anywhere inside a window brings
            // it to the front" — never reached it, because a float carrying a
            // page has no body of its own to press and this branch returns two
            // rungs above that function. Without the raise no float is ever the
            // focused preview surface, so `page_with_the_keyboard` has nothing
            // to answer with and the settling loop takes the keys straight back.
            // Focusing the pane *under* the window would be worse than nothing:
            // it is the layout focus moving to a pane nobody pressed.
            if self.float_holding_the_page(leaf).is_some() {
                self.raise_the_float_holding(leaf)?;
            } else {
                self.focus_pane_at(position)?;
            }
            // **A station, because this is a `return` nobody can see from
            // outside** (`BT_MOUSE_TRACE`'s own rule). Which seat the press
            // landed the focus on is the whole of whether `Ctrl+L` and `F12`
            // are claimed from the page a moment later.
            self.mouse_trace(|| {
                format!(
                    "press_web_page focus={:?} page_leaf={:?} holds={}",
                    self.seats.focus(),
                    Some(leaf),
                    self.page_holds_the_keyboard()
                )
            });
            if let Some(web) = self.window.web.get(&leaf) {
                if let Err(error) = web.focus_page() {
                    eprintln!("BT_WEB focus failed: {error}");
                }
                // **The receipt is written where the keyboard is handed over**,
                // and this is the only place this window ever hands it over.
                // `GotFocus` says the same thing again a message pump later; it
                // is the engine's own account and arrives for focus this window
                // never gave, which is why both are read — but the press is what
                // makes the sentence true, so the press is where it is recorded.
                self.window.web_keyboard = Some(leaf);
            }
        }
        let Some(event) = web_mouse_button(button, down) else {
            return Ok(());
        };
        self.send_to_web_page(leaf, event, position);
        Ok(())
    }

    /// A wheel notch, over the page.
    ///
    /// **Not the window's scroll**: the pane a page sits in has no document of
    /// its own to move, and the two axes are the page's exactly as they are in
    /// any other browser.
    pub(in crate::runtime) fn scroll_web_page(
        &mut self,
        position: PhysicalPosition<f64>,
        x: f32,
        y: f32,
    ) {
        // **`Ctrl`+wheel zooms the page** (方案 §0's five extras).
        //
        // Nothing is being taken from anything: this product has no type-size
        // zoom bound to a wheel at all — a picture zooms on the *bare* wheel —
        // so `Ctrl`+wheel is empty everywhere else in this window, which is what
        // makes it free to be the browser gesture here.
        //
        // The notch is not forwarded as well. A page that received both would
        // scroll while it zoomed, which is the one combination no browser does.
        //
        // **And it is `⌘`+wheel on a Mac** (§13.45 ①), asked of the same
        // function a click on a link asks: the gesture is "this notch is for
        // Folio", and the key that says so is the platform's. Control could not
        // stay there either — on that desk it is the system's own screen-zoom
        // modifier, and it is the one this window has just handed the secondary
        // click.
        if input::pointer_chord_held(self.window.modifiers_held) && y != 0.0 {
            // The page under the pointer, which is the page the notch was aimed
            // at — the same question every other wheel event on this path asks.
            if let Some(leaf) = self.web_page_at(position)
                && let Some(web) = self.window.web.get_mut(&leaf)
            {
                match web.zoom_by(y > 0.0) {
                    // **The page moved, so the foot says where to** (user ruling
                    // 2026-08-25): the seat recorded the factor the engine
                    // settled on, and this is the frame that has to show it.
                    // `Ok(None)` is a notch at the end of the ladder — nothing
                    // moved, so there is nothing to confirm, which is also what
                    // keeps a wall of `300%` off the glass under a held wheel.
                    Ok(Some(_)) => {
                        if self.refresh_chrome() {
                            // The wheel is not on a `?` path, and a failure to
                            // present a confirmation is not worth taking the
                            // gesture down over.
                            let _ = self.present_chrome_change();
                        }
                    }
                    Ok(None) => {}
                    Err(error) => eprintln!("BT_WEB {error}"),
                }
            }
            return;
        }
        // `WHEEL_DELTA`, which is what a page's own `deltaY` is derived from.
        let notch = |value: f32| (value * 120.0).round().clamp(-32768.0, 32767.0) as i16;
        if y != 0.0 {
            self.send_to_web_page_at(bt_platform::WebMouseEvent::Wheel(notch(y)), position);
        }
        if x != 0.0 {
            self.send_to_web_page_at(
                bt_platform::WebMouseEvent::HorizontalWheel(notch(x)),
                position,
            );
        }
    }
}
