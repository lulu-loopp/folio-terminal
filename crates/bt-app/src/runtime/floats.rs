//! `floats` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    CardWords, DropLanding, FileMenuState, FileMenuTarget, FileMenuTreeRow, HoverFloat, Layered,
    LeafId, MenuPaint, OwnTrigger, PAGE_SPINNER_STROKE_LOGICAL_PX, PointerTarget, PopoverTrigger,
    Popup, PopupOwner, PopupsUp, PreviewDocument, PreviewHeadFrame, PreviewImageState, PreviewPane,
    PreviewRailFrame, PreviewSurface, RevealedFoot, RowActivation, RowHost, Runtime, TabRename,
    TabSurface, TermMenuState, card_trace, chevron_turn_target, display_title, dump_chrome_frame,
    favicon, file_menu_powers, files, files_float_content_height, files_row_activation,
    files_row_menu_subject, float, float_viewport_rect, git, git_graph, git_panel, hang_watch,
    i18n, indeterminate_start_milliturns, marks, menubar, page_keepsake_icon, popup_owner, preview,
    preview_select, profiles, refused_preview_card, restore, revealable_preview_file, risen_frame,
    seats, shown_address, update, webhost, webnav,
};
use anyhow::Result;
use bt_layout::SeatId;
use bt_render::Travel;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::MouseScrollDelta;

impl Runtime<'_> {
    /// Rebuild chrome while leaving the overlay to a present that already owes
    /// a current-frame formula rebuild.
    pub(crate) fn refresh_chrome_without_overlay(&mut self) -> bool {
        self.refresh_chrome_with_overlay(false)
    }

    pub(in crate::runtime) fn refresh_chrome_with_overlay(
        &mut self,
        include_overlay: bool,
    ) -> bool {
        // **The one heavy call in this window that borrowed whichever name
        // happened to be standing** (T-STATION-SPLIT). It is reached from two
        // hundred-odd doors — a keystroke, a hover, a press, a probe's answer,
        // every frame that changes the furniture — and it rebuilds the whole of
        // the chrome: the strip, the rail, every pane head, every preview card's
        // measured verb, and the focus column's thumbnails. None of those doors
        // stamped a station, so a hold in here was reported as the last call
        // before it, which on the owner's typing recording was `flush_wheel`.
        //
        // `enter` and not `at`, on that function's own rule: this stands inside
        // somebody else's function every time, so the caller's name is handed
        // back at the end rather than kept. There is no early return in this
        // body — the bracket is exact.
        let leaving_station = hang_watch::enter(hang_watch::Station::Chrome);
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, _) = self.window.renderer.presentation_geometry().swapchain_size;
        // **The window's own drag handler is told what the bar is wearing now**
        // (§13.11 ⑥). It is handed the boxes Folio draws up there, not a
        // boundary: on the title bar every pixel that is not one of those boxes
        // is a place the window may be picked up by, and a single `x` cannot say
        // that about a bar whose controls do not fill the space in front of
        // them. The same list answers the press on the host where the press
        // reaches this application instead of the frame.
        //
        // The posture and not the preference, for the reason R3 left behind:
        // focus mode takes the strip away in *either* tab layout and the
        // vertical layouts put the tab list down the side, so a bar asked about
        // the stored preference would go on reserving the boxes of a strip
        // nobody is drawing — which is how the whole top bar came to answer
        // `HTCLIENT` and the window could not be dragged at all.
        //
        // **Ceiled and not rounded**, because the frame asks with whole pixels:
        // for an integer `x`, `x >= left && x < right` on the solved rectangle is
        // `x >= left.ceil() && x < right.ceil()` exactly, so the boxes the frame
        // holds claim precisely the pixel columns the boxes here do.
        let folio_boxes: Vec<[i32; 4]> = seats::title_bar_folio_boxes(
            width as f32,
            scale,
            self.platform_chrome(),
            self.window.tabs.len(),
            self.window.tab_scroll,
            self.rail_posture(),
            self.is_quake_window(),
        )
        .into_iter()
        .map(|rect| rect.map(|edge| edge.ceil() as i32))
        .collect();
        self.window
            .custom_window_frame
            .set_title_bar_boxes(&folio_boxes);
        // The badge's box is a function of the number in it, and only the font
        // knows how wide a number is — so the measuring happens here, where the
        // renderer is, and the strip is handed the answer rather than a font.
        let now = Instant::now();
        // B22, before anything is built: the cards are a function of whether a
        // divider is being held, and this is the one choke point every path that
        // grabs, releases or cancels one already goes through.
        self.sync_resizing_cards(now);
        let palette = bt_render::chrome_palette();
        let renaming = self.window.rename.as_ref().and_then(TabRename::tab);
        // Only a tab drag lifts a tab out of the strip; a pane in the air leaves
        // the strip exactly as it was.
        let carried = self
            .window
            .drag
            .as_ref()
            .and_then(|drag| drag.tab_carry().map(|carry| carry.offset));
        // The panes this window has pages on **and the icon each page's site
        // wears**, threaded into the strip so that a tab whose identity pane is
        // one of them wears it (§7.7 ②) — the same drawing the head under it
        // wears, through the same `pane_mark`.
        let page_seats: BTreeMap<LeafId, Option<favicon::FaviconId>> = {
            let favicons = self.app.favicons.borrow();
            self.window
                .web
                .iter()
                .map(|(leaf, web)| (*leaf, favicons.of_url(&web.page().url)))
                .collect()
        };
        // Which tabs are making a sound (§7.23 ⑩) — one walk of the page map
        // for the whole strip, beside the one that reads their icons.
        let audible = self.audible_tabs();
        let grabbed = self.window.drag.as_ref().and_then(|drag| {
            let tab = drag.tab()?;
            self.window
                .tabs
                .iter()
                .position(|candidate| candidate.id == tab)
        });
        // **§7.1.6k — the tab a carried pane is aimed at wears the landing wash.**
        //
        // The very drawing [`Runtime::strip_stand_in`] dresses its stand-in in
        // (`.drop-preview` and `@keyframes tab-land`'s `from` are one pair of
        // declarations in the mock-up: an accent wash behind an inset accent
        // ring), and the reuse is the argument rather than a saving. The slot a
        // drop will fill, the tab that has just been filled, and the tab that is
        // *about* to be handed a pane are one picture of one event seen at three
        // moments. A fourth mark invented here would be a second vocabulary for
        // "this is where it goes".
        //
        // **F2 — and a tab another window's hand is resting on wears it too.**
        // Read off [`ForeignDrag`] rather than off the drag this window does not
        // have, and folded into the same expression rather than drawn beside it:
        // the two are never both set (the pointer is over one window), and one
        // `Option<TabId>` is the whole of what the strip needs to know.
        let aimed = self
            .window
            .drag
            .as_ref()
            .and_then(|drag| drag.landing)
            .or_else(|| self.window.foreign.as_ref().and_then(|visit| visit.landing))
            .and_then(|landing| match landing {
                DropLanding::StripAdopt { tab } => Some(tab),
                _ => None,
            });
        let tabs = self
            .window
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let pane_count = tab.seats.pane_count();
                (
                    tab.display_title(),
                    pane_count,
                    tab.tab_mark(&page_seats),
                    tab.mark_state(
                        index == self.window.active_tab,
                        now,
                        self.app.motion,
                        &palette,
                    ),
                    seats::TabTrailer {
                        pinned: tab.pinned,
                        reveal: tab.pin_reveal.sample(now, self.app.motion).0,
                        files: tab.seats.is_lone_terminal(),
                        files_lit: tab.files_lit.sample(now, self.app.motion).0,
                        audible: audible.contains(&tab.id),
                    },
                    tab.drawn_offset(
                        now,
                        self.app.motion,
                        carried.filter(|_| grabbed == Some(index)),
                    ),
                    // **How far through a flight this tab is** (§7.1.6b″), read
                    // off the same clock and the same hand as the offset beside
                    // it, and deliberately not derived from it — see
                    // [`TabState::drawn_flight`].
                    tab.drawn_flight(
                        now,
                        self.app.motion,
                        carried.filter(|_| grabbed == Some(index)),
                    ),
                    // The wash the tween is running, or a full one while this tab
                    // is the one under a carried pane — whichever is louder, so a
                    // tab that has only just landed and is now being aimed at
                    // does not dim on the way.
                    tab.landing
                        .sample(now, self.app.motion)
                        .0
                        .max(if aimed == Some(tab.id) { 1.0 } else { 0.0 }),
                    // The layer under the override, which is exactly what the
                    // editor's placeholder shows: `autoName(s)` is `displayName`
                    // with the manual name taken out (mock-up 2605-2606).
                    //
                    // Only a tab with a shell can be renaming — `open_rename`
                    // turns the other two shapes away through
                    // [`seed::Seed::can_be_named`] — so the `and_then` is the
                    // shape of that fact rather than a case being handled: no
                    // shell, no editor, no placeholder to compute.
                    (renaming == Some(tab.id))
                        .then(|| tab.focused())
                        .flatten()
                        .map(|leaf| {
                            display_title(
                                None,
                                leaf.announced_title(),
                                leaf.standing_in(),
                                tab.focused_profile_title(),
                                &tab.focused_announcement_set(),
                            )
                        }),
                )
            })
            .collect::<Vec<_>>();
        // One character of the mini transcript's face, measured on the same beat
        // as the badges — see [`WindowRuntime::focus_mini_advance`].
        let focus_mini_advance = {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            renderer.measure_chrome_mono_text(
                gpu,
                // A digit rather than a letter: in a monospaced face every
                // character has the same advance, and a digit is the one that is
                // present in every face that could ever answer `Monospace`.
                "0",
                bt_render::FOCUS_MINI_TERM_FONT_LOGICAL_PX * scale,
            )
        };
        self.window.focus_mini_advance = focus_mini_advance;
        // And one of the window's own face at the files size — see
        // [`WindowRuntime::focus_mini_face_advance`] for why this is a second
        // number and why a digit is the honest stand-in for a proportional face.
        let focus_mini_face_advance = {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            renderer.measure_chrome_text(
                gpu,
                "0",
                bt_render::FOCUS_MINI_FILES_FONT_LOGICAL_PX * scale,
            )
        };
        self.window.focus_mini_face_advance = focus_mini_face_advance;
        self.refresh_focus_thumbnails(now, scale, card_trace::why::FRAME);
        let badge_font_px = bt_render::WINDOW_TAB_BADGE_FONT_LOGICAL_PX * scale;
        let mut tabs = tabs
            .into_iter()
            .map(
                |(
                    title,
                    pane_count,
                    mark_kind,
                    mark,
                    trailer,
                    offset,
                    flight,
                    landing,
                    placeholder,
                )| {
                    seats::TabContent {
                        mark_kind,
                        badge_text_width: if pane_count > 1 {
                            self.window.renderer.measure_chrome_text(
                                &mut self.app.gpu,
                                &pane_count.to_string(),
                                badge_font_px,
                            )
                        } else {
                            0.0
                        },
                        // Filled in below, once the strip's own geometry has said how
                        // much room the box has: the editor scrolls to keep its caret
                        // in sight, and "in sight" is a width this loop has not
                        // computed yet.
                        edit: placeholder.map(|placeholder| seats::TabEdit {
                            placeholder,
                            ..seats::TabEdit::default()
                        }),
                        title,
                        pane_count,
                        mark,
                        trailer,
                        offset,
                        flight,
                        landing,
                    }
                },
            )
            .collect::<Vec<_>>();
        self.measure_open_rename(&mut tabs, scale, width as f32);
        // **K124 — the stand-in goes into the run.** Inserted after the rename
        // editor has been measured, because the editor is a fact about a real tab
        // and the indices it was measured against are the strip's own; the
        // stand-in is a guest that takes a slot for one gesture and then leaves.
        let mut active_tab = self.window.active_tab;
        let mut grabbed = grabbed;
        let mut strip_preview = None;
        if let Some((slot, stand_in)) = self.strip_stand_in() {
            tabs.insert(slot, stand_in);
            strip_preview = Some(slot);
            // Everything the strip indexes by position moves over with it. A
            // stand-in inserted before the active tab does not make its
            // *neighbour* active, and neither does it hand the grab to someone
            // else.
            active_tab += usize::from(active_tab >= slot);
            grabbed = grabbed.map(|index| index + usize::from(index >= slot));
        }
        // The caption of **every** preview seat. It comes from whichever door
        // filled that seat — two doors, one caption, and only one of them can be
        // open at a time (P36) — and it is asked per seat because a collapsed bar
        // names the file its own pane is showing, exactly as its head would.
        let preview_titles: Vec<(SeatId, String)> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| {
                let surface = self.preview_here(seat);
                let title = match self
                    .preview_pane(surface)
                    .and_then(|pane| pane.image.as_ref())
                {
                    Some(image) => image.title(),
                    None => self.preview_buffer_on(surface)?.name.clone(),
                };
                Some((seat, title))
            })
            .collect();
        // C28, per leaf: every terminal pane head names its *own* shell. Resolved
        // here rather than in `seats`, which knows nothing about sessions (L1).
        let terminal_names = self.terminal_names();
        // **A page's seat wears the globe** (§7.7 ②, W2 slice ④). Added here
        // rather than inside `leaf_marks`, because that function belongs to the
        // tab and a page belongs to the window — but it goes into the *same*
        // map, so the head, the strip row above it, the drag ghost and the
        // collapse bar cannot end up drawing four glyphs for one seat. The map's
        // meaning is "which content is this leaf holding", which is the one
        // thing `seats::pane_mark` cannot work out from a `SeatKind`.
        let mut leaf_marks = self.leaf_marks();
        // **Only the tab in front's pages**, because that is whose leaves this
        // map is about. The window's map spans every tab and seat numbers
        // restart at one in each of them (F1b′), so a page open on a background
        // tab used to hand its globe to whichever pane of the tab on the glass
        // happened to share its number — a terminal wearing a browser's mark.
        let front = self.id;
        for (leaf, web) in self.window.web.iter().filter(|(leaf, _)| leaf.tab == front) {
            let page = web.page();
            // **While a navigation is in flight the mark spins in place**
            // (§7.7 ②): no progress hairline, no second row, and no second
            // glyph — the seat gains nothing for a state that lasts a second,
            // and the arc it borrows is the one this window already turns on a
            // working tab. `stroke_px` is the mock-up's own 1.4.
            leaf_marks.insert(
                leaf.seat,
                match page.loading_since.filter(|_| page.loading) {
                    Some(since) => marks::ChromeMark::ProgressRing {
                        start_milliturns: indeterminate_start_milliturns(
                            now.saturating_duration_since(since),
                            self.app.motion,
                        ),
                        sweep_milliturns: (bt_render::WINDOW_TAB_RING_INDETERMINATE_TURNS * 1000.0)
                            .round() as u16,
                        stroke_px: (PAGE_SPINNER_STROKE_LOGICAL_PX * scale).round().max(1.0) as u32,
                    },
                    // **And where the site has an icon, the icon** (§7.7 ②:
                    // 「站点有图标画 favicon」). Asked with the URL the engine
                    // last committed, so a page that navigates to another server
                    // changes what this mark means in the same frame the address
                    // changes — the store is keyed by site and this is a lookup,
                    // not a copy, so there is nothing to invalidate. A site with
                    // no icon answers `None` and the globe is drawn, which is
                    // both halves of §7.7 ② in one expression.
                    None => marks::ChromeMark::Globe {
                        favicon: self.app.favicons.borrow().of_url(&page.url),
                    },
                },
            );
        }
        // B14, per leaf: every files pane head names its *own* root. Resolved
        // beside the terminal names for the same reason and by the same rule.
        let files_names = self.files_names();
        // C28-C43, per leaf: the rows each files column shows this frame. Walked
        // here, beside the names, because the walk needs the directory cache and
        // `seats` holds neither content nor a filesystem (L1).
        let mut files_trees = self.files_trees(now);
        // R2 乙案, before the rows are laid out: the painter believes the stored
        // scroll now, so a list that got shorter or a pane that got taller has to
        // be answered here rather than by the layout quietly disagreeing with the
        // number it was handed.
        self.heal_files_scroll(scale, &mut files_trees);
        // And the strip along the bottom of each of them, whose caption is cut to
        // the room it has — which only something holding a font can decide.
        self.dress_files_feet(scale, now, &mut files_trees);
        // And the box, when a row of one of them is being typed into — after the
        // scroll has been healed, because where a row *is* is what the caret is
        // measured against.
        self.dress_files_tree_editor(scale, &mut files_trees);
        // The `Files | Git` switch and, for a column standing on the second of
        // them, the page itself. Both are empty when the master switch is off,
        // which is how "the page does not exist" reaches the painter: not as a
        // flag it has to remember to check, but as a column that is not in the
        // map.
        let files_views = self.files_views(scale);
        let git_pages = self.git_pages(scale, &files_views);
        // A column torn out of the tree takes its foot's memory with it — see
        // [`Self::sweep_foot_phrases`], which is `sweep_command_rails`' own
        // sentence one strip along.
        self.sweep_foot_phrases();
        // **The head run's ninety milliseconds**, settled here — before the
        // borrows below — because it is the last thing in this pass that needs
        // the window mutably. See [`Self::settled_head_ink`].
        let head_ink = self.settled_head_ink(now);
        // And the tab strip's own ninety, for the same reason and in the same
        // place. See [`Self::settled_tab_ink`].
        let active_ink = self.settled_tab_ink(now);
        // And the card column's, which is the same ninety on the third surface.
        // See [`Self::settled_card_ink`].
        let card_ink = self.settled_card_ink(now);
        // Kept for the hit test, which is `&self` by construction and cannot
        // measure a string — the same reason `files_name_widths` is a field. The
        // press then lands on the row that was drawn, because it *is* the row
        // that was drawn.
        self.window.git_pages_shown = git_pages.clone();
        // And the hover healed against that very list, before anything is drawn
        // from it: the map above has just been rebuilt under a pointer that did
        // not move, and the hover is an index into it. See
        // [`Self::heal_git_hover`] — `heal_files_scroll`'s twin, one state along.
        self.heal_git_hover(scale);
        let git_graphs = self.git_graphs(scale);
        // Kept for the hit test, for `git_pages_shown`'s reason exactly.
        //
        // **The seats' half of the map, written without touching the other.** A
        // floating graph is built and recorded a pass later, in `float_layer`,
        // and an assignment here would take the window's entry away between the
        // two passes — a press landing on a row the window is still drawing.
        self.window
            .git_graphs_shown
            .retain(|surface, _| !matches!(surface, PreviewSurface::Seat(_)));
        self.window.git_graphs_shown.extend(
            git_graphs
                .iter()
                .map(|(surface, content)| (*surface, content.clone())),
        );
        // **The tab half of every graph key, spent here** (§7.12 ⓑ). The build
        // above is keyed by surface so that one map answers for the panes and
        // for the floats at once; what the chrome pass paints is one tab, and
        // inside one tab a seat number is a whole name. Narrowing here is what
        // lets the painter go on knowing nothing about tabs — and what stops a
        // graph standing on a background tab from being drawn into the pane of
        // the same number in front of you.
        let here = self.id;
        let graph_bodies: BTreeMap<SeatId, &git_graph::GraphContent> = git_graphs
            .iter()
            .filter_map(|(surface, content)| match surface {
                PreviewSurface::Seat(leaf) if leaf.tab == here => Some((leaf.seat, content)),
                PreviewSurface::Seat(_) | PreviewSurface::Float(_) | PreviewSurface::Peek => None,
            })
            .collect();
        self.window.files_view_widths = files_views
            .iter()
            .map(|(seat, content)| (*seat, content.widths))
            .collect();
        // **One sentence per preview pane, and each about its own body.**
        //
        // This was one message spread over every `SeatKind::Preview` placement
        // until slice 7's restore put two waiting panes on screen at once and
        // both of them said "Loading README.md…" while one was waiting on
        // `todo.txt` — the same bug the head and the foot were cured of one
        // slice earlier, in the one piece of the pane that had been left
        // singular.
        let preview_messages_owned: Vec<(SeatId, String)> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| {
                let surface = self.preview_here(seat);
                let message = match (
                    self.preview_pane(surface)
                        .and_then(|pane| pane.image.as_ref()),
                    self.preview_buffer_on(surface),
                ) {
                    (Some(preview), _) => preview.message(),
                    // A document on its way: the same sentence a picture uses,
                    // because it is the same wait. And, after it arrives, the
                    // one line a *composed* document prints instead of a body —
                    // a diff with nothing in it, or a repository that would not
                    // answer (G-3, [`preview::PreviewBuffer::body_notice`]).
                    (None, Some(buffer)) => {
                        if buffer.load == preview::PreviewLoad::Pending {
                            Some(i18n::preview_loading(&buffer.name))
                        } else {
                            buffer.body_notice().map(str::to_owned)
                        }
                    }
                    // An open pane with nothing chosen invites rather than sits
                    // mute.
                    (None, None) => Some(i18n::Text::PreviewEmptyState.text().to_owned()),
                }?;
                Some((seat, message))
            })
            .collect();
        // The "no preview" card, when the buffer on a seat has said it will
        // never have a body. Measured here beside the files names, and for the
        // same reason: only something holding a font can size a button, and the
        // hit test reads the number this frame stored.
        //
        // Per seat for the reason above, and here it cost more than a wrong
        // word: the paint draws a body notice only where there is no card, so
        // one refused file used to blank the "Loading …" of every other preview
        // pane on screen.
        // **The five failure cards, and the one card that was already here**
        // (§7.7 ④, W2 slice ④). One drawing, and the words are owned rather
        // than borrowed because a fault's sentence is built from a host or a
        // scheme — `web_fail_did_not_respond`, `web_fail_blocked_scheme` — and
        // a `&'static str` cannot carry one.
        //
        // Four of the five *are* the seat's content and are drawn as pane
        // chrome; the fifth stands over a page that is still there and is an
        // overlay (`websheet`), because the transparency hole is punched over
        // everything the seats draw (§7.8 ②).
        let preview_open_label = self.preview_open_button_label(now);
        let preview_card_notices: Vec<(SeatId, CardWords)> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| {
                if let Some(fault) = self
                    .web_on(seat)
                    .and_then(webhost::WebSeat::fault)
                    .filter(|fault| !fault.stands_over_the_page())
                {
                    return Some((
                        seat,
                        CardWords {
                            notice: fault.say(),
                            // **Through the one spelling** (user ruling
                            // 2026-08-25). Three of the four cards carry an
                            // address on this line, and a card naming a local
                            // file in the URI form while the row above it names
                            // the same file as a path is the split the ruling
                            // came to close — the two screenshots behind it were
                            // exactly two surfaces disagreeing about one disk.
                            // Everything that is not a local file this window
                            // minted goes through untouched.
                            detail: shown_address(fault.detail().unwrap_or_default()),
                            detail_lines: Vec::new(),
                            verb: Some(fault.verb_text().text().to_owned()),
                            // **The class's mark and never the site's**, the
                            // same ruling `websheet` states at length: §7.7 ④
                            // says a failure card wears 「一枚本类的记号」, and
                            // what these four cards are about is this window's
                            // report on a page that is not there. A server's own
                            // icon over that sentence would read as the server
                            // having said it.
                            mark: marks::ChromeMark::Globe { favicon: None },
                            fault: true,
                            width: 0.0,
                        },
                    ));
                }
                let here = self.preview_here(seat);
                // **A picture this pane refused to draw is a card too** (owner's
                // ruling 2026-09-12). It used to be a sentence alone in the
                // middle of an empty pane — `Preview failed: this picture has
                // too many pixels` and no way on — while the file beside it that
                // nothing here reads at all got a mark, a sentence and a button.
                // The two are the same page: this window cannot show it, and the
                // machine may be able to. Asked before the buffer below because
                // a picture *is* not one (`preview_chrome_on`), so the two
                // cannot both answer.
                if let Some(refusal) = self
                    .preview_picture(here)
                    .and_then(PreviewImageState::refusal)
                {
                    return Some((
                        seat,
                        refused_preview_card(
                            refusal.notice.clone(),
                            refusal.offers_the_default_app,
                            preview_open_label,
                        ),
                    ));
                }
                // **The button belongs to the refusals it is true of**
                // (R1-12): a card that cannot read *this content* offers the
                // machine's own handler, and a card refusing a share or
                // reporting a disk that said no offers nothing, because there
                // is nothing it could honestly offer.
                let refusal = self.preview_buffer_on(here)?.refusal()?;
                Some((
                    seat,
                    refused_preview_card(
                        refusal.notice().to_owned(),
                        refusal.offers_the_default_app(),
                        preview_open_label,
                    ),
                ))
            })
            .collect();
        // The head's own run and the strip along the bottom, both measured here
        // beside the card and for the card's reason: only something holding a
        // font can say how wide a name is drawn, and the hit test reads the
        // number this frame stored rather than measuring for itself.
        // **One head and one foot per preview seat, and both named by it.**
        //
        // A head names the file its own body is showing. Dressing `preview()`
        // alone and letting the paint use that answer for every preview
        // placement was a caption belonging to whichever pane happened to be
        // first in the tree — real-machine capture caught exactly that, two panes
        // showing two files under one file's name, one file's path and one pin.
        // The content plane went plural in this slice; the chrome that describes
        // it has to go plural in the same one.
        let preview_frames: Vec<(SeatId, PreviewHeadFrame)> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| Some((seat, self.dress_preview_head(seat, scale)?)))
            .collect();
        let preview_feet: Vec<(SeatId, seats::FootWords)> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| Some((seat, self.dress_preview_foot(seat, scale, now)?)))
            .collect();
        // The row under each head (user ruling 2026-08-24), dressed the same way
        // and for the same reason: one per seat, each naming the content its own
        // body is showing.
        let preview_rail_frames: Vec<(SeatId, PreviewRailFrame)> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| {
                Some((
                    seat,
                    self.dress_preview_rail(self.preview_here(seat), scale)?,
                ))
            })
            .collect();
        // **The foot no longer lends this row a phrase** (owner's ruling
        // 2026-09-12). It used to: the standing fact and the flashed
        // confirmation were taken off the foot this frame had already dressed
        // and hung on the rail's right hand, because the breadcrumb had retired
        // the strip along the bottom and the 2026-08-24 ruling kept the foot's
        // other duties. The two have gone their separate ways since — the fact
        // is a padlock this row's own dressing decides
        // (`dress_preview_rail`), and the confirmation is a pill over the
        // document — so there is nothing left to copy across and no second place
        // for one file's facts to be derived.
        // **Each card's own verb, measured** (§7.7 ④). The five failure cards
        // do not share a caption with `Open in default app`, so a single stored
        // width would size every button to whichever card was drawn last — and
        // the hit test reads that number.
        let mut preview_card_notices = preview_card_notices;
        for (seat, words) in &mut preview_card_notices {
            words.width = match &words.verb {
                Some(verb) => self.window.renderer.measure_chrome_text(
                    &mut self.app.gpu,
                    verb,
                    seats::PREVIEW_CARD_BUTTON_FONT_LOGICAL_PX * scale,
                ),
                None => 0.0,
            };
            // **And the fact, wrapped to the seat it will be drawn in** (§7.43).
            // Gate 5 photographed the alternative on a 479-pixel seat: a
            // 92-character `CreateCoreWebView2EnvironmentWithOptions failed: …`
            // drawn as one centred line and cut off at both ends, so neither
            // half of the sentence a reader was meant to copy was on screen.
            // `restore::wrap_anywhere` and not a second wrapper, because the
            // fact is one token as often as it is a sentence.
            words.detail_lines = if words.detail.is_empty() {
                Vec::new()
            } else {
                let Some(body) =
                    seats::preview_seat_body_rect(&self.seats, &self.seat_layout, *seat, scale)
                else {
                    continue;
                };
                let font = seats::preview_card_detail_font_px(scale);
                let width = seats::preview_card_detail_width(body, scale);
                let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
                restore::wrap_anywhere(&words.detail, width, |text| {
                    renderer.measure_chrome_text(gpu, text, font)
                })
            };
        }
        self.window.preview_button_width = self.window.renderer.measure_chrome_text(
            &mut self.app.gpu,
            preview_open_label,
            seats::PREVIEW_CARD_BUTTON_FONT_LOGICAL_PX * scale,
        );
        self.window.preview_card_verbs = preview_card_notices
            .iter()
            .map(|(seat, words)| {
                (
                    *seat,
                    seats::PreviewCardButton {
                        offers: words.verb.is_some(),
                        text_px: words.width,
                        detail_lines: words.detail_lines.len(),
                        fault: words.fault,
                    },
                )
            })
            .collect();
        let preview_cards: Vec<(SeatId, seats::PreviewCardContent<'_>)> = preview_card_notices
            .iter()
            .map(|(seat, words)| {
                (
                    *seat,
                    seats::PreviewCardContent {
                        notice: &words.notice,
                        detail: &words.detail_lines,
                        mark: words.mark,
                        fault: words.fault,
                        button: words.verb.as_deref(),
                        button_text_px: words.width,
                        // The hover already named a seat; now the card it lights
                        // is that seat's, so two "Open in default app" buttons
                        // side by side no longer light together.
                        button_hovered: self.window.seat_pointer.hover
                            == Some(if words.fault {
                                seats::ChromeTarget::PreviewFaultVerb(*seat)
                            } else {
                                seats::ChromeTarget::PreviewOpenButton(*seat)
                            }),
                    },
                )
            })
            .collect();
        // U8 — sampled here, on the same `now` every other animated value in
        // this build reads, and handed over as numbers. `seats` has an explicit
        // invariant that nothing in it knows what time it is, and a `PaneMotion`
        // passed down whole would be a clock in the one module that must not
        // hold one.
        let pane_transforms = self.pane_transforms(now);
        let resizing_cards = self.resizing_cards_frame(now);
        // **Each terminal pane not at 100 %, read off the pane for this frame** (ticket 37) — the
        // pane head's text-size mark. A reading handed down, like `terminal_names`, never kept.
        let text_sizes = self.pane_text_sizes();
        // Q173, one row per window on screen — a list since 浮窗多开.
        let float_shown = self.float_shown_tabs();
        // Measured into the runtime rather than into a local, because the hit
        // test needs the very same numbers and cannot measure: it is `&self` by
        // construction (a pointer moving is not a reason to touch a renderer).
        // Storing them here, where the picture is built from them, is what makes
        // "the button you can press is the button you can see" true by
        // construction rather than by two functions agreeing.
        self.window.files_name_widths = self.measure_files_names(&files_names);
        // Borrowed off the owned frames above, the same two-step the heads take:
        // the paint looks a placement up in this list rather than being handed
        // one answer for every preview seat.
        // **And each one broken to the pane it will stand in** (user ruling
        // 2026-08-29; `docs/DESIGN.md` §7.43 ① and ⑤, on the third surface that
        // draws "整页只剩一句话"). A body notice is shaped with `Wrap::None` and
        // clipped to its own box, so a sentence wider than the pane loses both
        // its ends and the reader gets its middle — which is exactly what
        // `Loading <a real file name>…` did in a narrow column, and what the
        // empty pane's invitation would do the moment it named the underline it
        // is talking about. Wrapped here because this is the side holding a
        // font, and against `pane_notice_width` because that is the box the
        // paint will lay it out in.
        let mut preview_messages: Vec<(SeatId, Vec<String>)> = Vec::new();
        for (seat, message) in &preview_messages_owned {
            let font = bt_render::SEAT_TITLE_FONT_LOGICAL_PX * scale;
            let lines = match seats::pane_notice_width(&self.seat_layout, *seat, scale) {
                Some(width) => {
                    let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
                    restore::wrap_anywhere(message, width, |text| {
                        renderer.measure_chrome_text(gpu, text, font)
                    })
                }
                // A seat the solver has no rectangle for is a seat with no
                // column to break against; the sentence stands whole rather
                // than being broken to a width nobody measured.
                None => vec![message.clone()],
            };
            preview_messages.push((*seat, lines));
        }
        // The two lists the paint looks its placement up in, borrowed off the
        // owned frames above — the same two-step the head's `name` and `count`
        // have always taken, once per seat instead of once.
        let preview_heads: Vec<(SeatId, seats::PreviewHeadContent<'_>)> = preview_frames
            .iter()
            .map(|(seat, head)| {
                (
                    *seat,
                    seats::PreviewHeadContent {
                        name: &head.name,
                        count: &head.count,
                        edit: head.edit.as_ref().map(|edit| seats::PreviewNameEdit {
                            text: &edit.text,
                            caret_px: edit.caret_px,
                            selection: edit.selection,
                            caret_lit: edit.caret_lit,
                            refused: head.refused,
                        }),
                        ..head.content
                    },
                )
            })
            .collect();
        // Borrowed off the owned frames above, the same two-step everything else
        // on a preview seat takes.
        let preview_rails: Vec<(SeatId, seats::PreviewRailContent<'_>)> = preview_rail_frames
            .iter()
            .map(|(seat, frame)| {
                (
                    *seat,
                    seats::PreviewRailContent {
                        measure: &frame.measure,
                        address: &frame.address,
                        segments: &frame.segments,
                        open: i18n::Text::PreviewRailOpen.text(),
                        meta: &frame.meta,
                        flip_to_source: frame.flip_to_source,
                        web: frame.web,
                        edit: frame.edit.as_ref().map(|edit| seats::PreviewNameEdit {
                            text: &edit.text,
                            caret_px: edit.caret_px,
                            selection: edit.selection,
                            caret_lit: edit.caret_lit,
                            refused: frame.refused,
                        }),
                    },
                )
            })
            .collect();
        let preview_feet: Vec<(SeatId, seats::FootStrip<'_>)> = preview_feet
            .iter()
            .map(|(seat, words)| {
                (
                    *seat,
                    seats::FootStrip {
                        path: &words.lead,
                        path_width: words.lead_width,
                        revealed: words.flashing,
                        dissolved: words.dissolved,
                        notice: &words.notice,
                        notice_width: words.notice_width,
                        web: self.seat_holds_a_page(*seat),
                        // The head's own answer, asked the same way, so that the
                        // band and the head above it cannot name one page with
                        // two drawings — see [`seats::FootStrip::favicon`].
                        favicon: self
                            .web_on(*seat)
                            .and_then(|web| self.app.favicons.borrow().of_url(&web.page().url)),
                    },
                )
            })
            .collect();
        let preview_titles: Vec<(SeatId, &str)> = preview_titles
            .iter()
            .map(|(seat, title)| (*seat, title.as_str()))
            .collect();
        // **The pane in the hand, as the one-leaf tree the card it is about to
        // become would hold** (缺陷 #189). Owned here, above the list it is
        // borrowed into, because a stand-in is a picture of a tab that does not
        // exist yet and so there is no tree in this window to point at: the
        // subtree is cloned out of the tab the pane is still filed under —
        // exactly as [`Runtime::publish_to_broker`] clones it for the other
        // window — and a card of one seat is what a torn-out pane becomes.
        let stand_in_pane: Option<(LeafId, bt_layout::LayoutNode)> =
            self.stand_in_pane().and_then(|leaf| {
                self.tab_state(leaf.tab)
                    .and_then(|tab| tab.seats.tree().find_seat(leaf.seat).cloned())
                    .map(|seat| (leaf, bt_layout::LayoutNode::seat(seat)))
            });
        // One slot per tab, in the strip's order, borrowed off the budget that
        // just ran (§7.1.6b′ F2). A tab with no entry is a card the budget did
        // not project — off screen, or in a window where the mode is off — and
        // its slot is `None`.
        let mut focus_thumbnails: Vec<Option<seats::FocusThumbnail<'_>>> = self
            .window
            .tabs
            .iter()
            .map(|tab| {
                Some(seats::FocusThumbnail {
                    tree: tab.seats.tree(),
                    focused: tab.focused_leaf,
                    seats: self.window.focus_thumbs.seats(tab.id)?,
                    // The grid those rows are drawn on, which is the grid they
                    // were cut to — see [`seats::FocusThumbnail::mono_cell`].
                    // Measured a few lines up, on this window's own face at this
                    // window's own scale, which is the face and the scale every
                    // card in this column is about to be shaped in.
                    mono_cell: focus_mini_advance,
                })
            })
            .collect();
        // **K124's stand-in is a guest in `tabs`, so it is a guest here too**
        // (the card column's tear-out, user ruling 2026-08-29). This list and
        // that one are walked by one index — `focus_card_mini_chrome` is handed
        // `thumbnails[index]` — so a stand-in inserted into one and not the
        // other would hand every card below the slot its neighbour's tree, and
        // the stand-in itself a picture of a tab that is not the one it is
        // standing in for.
        //
        // **And it is the pane's own picture that goes in it** (缺陷 #189, user
        // ruling). What stood here was `None` — "what the stand-in draws is the
        // *slot*, and the pane's own picture is the ghost under the pointer" —
        // and the ghost is a *label*: a mark and a name riding the cursor, which
        // is the same two things the card's own head is already saying. So the
        // slot drew a body of bare `--termbg` on a panel of nearly that value,
        // and the user's report is the consequence — 「看不到即将落座的那张替身
        // 卡」. Every other card in this column is its picture; the one the hand
        // is aiming at may not be the exception.
        //
        // The projection is the seat's own, out of the cache the pane's home
        // card is drawn from, so the two are one reading of one shell.
        //
        // **And a visitor's stand-in draws the pane too** (B2, 2026-09-01). What
        // stood here was "this window has never drawn that pane, and inventing
        // something for it is the one thing worse than a blank" — true of the
        // second half and false of the first. It rested on §7.1.6b⁗'s 「那枚
        // pane 属于另一个进程」, and there is no other process: the broker exists
        // because one `FolioApp` holds both windows. So nothing is invented — the
        // window that *does* hold the pane sends its own projection across, the
        // same one its home card is drawn from, and this window draws what it
        // was handed ([`ForeignPane`]). `None` survives as the honest answer for
        // a payload nobody has looked at yet, and it is now the exception.
        if let Some(slot) = strip_preview {
            let guest = match self.window.foreign.as_ref() {
                Some(foreign) => foreign.pane.as_ref().map(|pane| seats::FocusThumbnail {
                    tree: &pane.tree,
                    focused: pane.focused,
                    seats: &pane.seats,
                    // **This window's cell, for a projection the other window
                    // cut.** The rows crossed the broker as text; the grid is a
                    // fact about the face they are about to be *drawn* in, and
                    // that face is this window's. A visitor's card whose columns
                    // were cut wider than this card holds is clipped at this
                    // card's edge exactly as it was before — that half is the
                    // clip's, and it is unchanged.
                    mono_cell: focus_mini_advance,
                }),
                None => stand_in_pane.as_ref().and_then(|(leaf, tree)| {
                    Some(seats::FocusThumbnail {
                        tree,
                        // The card is a tab of exactly this one pane, and in
                        // that tab this pane is the one holding the keyboard.
                        focused: leaf.seat,
                        seats: self.window.focus_thumbs.seats(leaf.tab)?,
                        mono_cell: focus_mini_advance,
                    })
                }),
            };
            focus_thumbnails.insert(slot.min(focus_thumbnails.len()), guest);
        }
        let chrome = seats::build_chrome_for_tabs(
            &self.seats,
            &self.seat_layout,
            scale,
            // E53's `body.dragging` is derived here rather than mirrored into a
            // field at every place a drag starts and ends: it is a fact about
            // the runtime, and the way for a mirror of it to go wrong is for one
            // of those places to be added later and forget.
            seats::ChromePointer {
                other_drag_in_flight: self.window.drag.is_some(),
                ..self.window.seat_pointer
            },
            seats::ChromeContent {
                // **Read here rather than in `seats.rs`** — that file draws what
                // this one has decided is true, and "is there a newer release"
                // is a fact about a file on the disk. The owner answers from
                // memory — a lock and a clone of four small fields, never the
                // disk; it is on the frame path for the reason `i18n::current()`
                // is, and like that one it answers the same thing all frame.
                update_mark: update::gear_mark_is_lit(),
                // **Which caption run this window wears** (§7.54e ②) — the one
                // window whose `×` hides rather than closes, and which therefore
                // has no second button that means the same thing.
                summoned: self.is_quake_window(),
                // **What AppKit still draws in this window's bar** (M3-3) — the
                // one capability read, made here and handed on, so the caption
                // run that is painted is the caption run that is hit-tested.
                chrome: self.platform_chrome(),
                tabs: &tabs,
                head_ink: seats::HeadInk::new(&head_ink),
                active_ink: seats::TabInk::new(&active_ink),
                card_ink: seats::TabInk::new(&card_ink),
                active_tab,
                grabbed,
                strip_preview,
                tab_scroll: self.window.tab_scroll,
                // Sampled on this frame's own `now`, like the chevron beside it:
                // the two scalars are what the rail *looks like* this instant,
                // and `seats` holds no clock to work them out for itself.
                rail: self.sampled_rail(now),
                rail_scroll: self.window.rail_scroll,
                focus_reveal: self.window.focus_reveal.sample(now, self.app.motion).0,
                // Sampled here, on the frame it is drawn, beside the reveal it
                // sits next to — and answered as `0.0` by the host itself for
                // every window that is not being shown the bubble and every
                // window whose reader asked for less motion.
                focus_card_nudge_rows: self.window.card_hint.nudge_rows(now, self.app.motion),
                focus_thumbnails: &focus_thumbnails,
                preview_titles: &preview_titles,
                float_shown: &float_shown,
                terminal_names: &terminal_names,
                leaf_marks: &leaf_marks,
                files_names: &files_names,
                files_name_widths: &self.window.files_name_widths,
                files_root_open: self.window.root_menu.seat(),
                files_trees: &files_trees,
                files_views: &files_views,
                git_pages: &git_pages,
                git_graphs: &graph_bodies,
                preview_messages: &preview_messages,
                preview_feet: &preview_feet,
                preview_heads: &preview_heads,
                preview_rails: &preview_rails,
                preview_cards: &preview_cards,
                fit_overflow: self.seat_overflow,
                profile_menu_open: self.window.profile_menu.is_open(),
                chevron_turn: self.window.chevron_turn.sample(now, self.app.motion).0,
                pane_motion: seats::PaneMotionFrame::new(&pane_transforms),
                resizing_cards,
                text_sizes: &text_sizes,
                // §7.1.6i: the two facts a lone pane's corner ghost is a
                // function of, and neither of them is `Seats`'s to know — which
                // pane the search capsule is standing on (it takes the ghost's
                // corner and its lane, and the instrument that was asked for out
                // loud wins), and which pane's menu is up (the ghost stays lit
                // under its own list).
                search_seat: self.window.search.seat(),
                head_raised: self.head_that_raised_a_layer(),
            },
        );
        let seats::WindowChrome {
            seats,
            rail,
            flight,
        } = chrome;
        dump_chrome_frame(&seats);
        // **With the quads under them** (0.4.4 ticket 09): a site's icon is asked about the head,
        // the strip or the bar it is drawn on, and those are what the quads are.
        let mut icons = self
            .window
            .chrome_marks
            .resolve_on(&seats.sprites, &seats.quads, &palette);
        // **And the last frame of every page a modal is standing over** (§7.8
        // ⑩), which joins the same channel the marks just went down and is drawn
        // in the same pass — under every overlay layer, and therefore under the
        // scrim, which is the whole point: the page's pixels are dimmed by the
        // dialog exactly as its neighbour pane's letters are.
        //
        // Not rasterized and not a mark: it is pixels that already exist, so it
        // is appended to the resolved list rather than passed through
        // `chrome_marks`, which is a cache of drawings this window makes.
        icons.extend(self.page_keepsake_icons());
        let chrome_changed = self
            .window
            .renderer
            .set_chrome(seats.quads, seats.labels, icons);
        // The rail floats over the panes, so it is handed on to the overlay
        // stack instead of being drawn in the same run as them — see
        // [`seats::WindowChrome`]. Kept here rather than rebuilt down there
        // because it is a *product* of this build: everything it needs was
        // sampled on this frame's `now`, and asking for it again a few lines
        // later would be asking a second clock.
        self.window.rail_chrome = rail;
        // **And whatever is in motion, on the level above both** (§7.1.6b″).
        // Kept beside the rail's and for the rail's reason: it is a product of
        // this build, sampled on this frame's `now`.
        self.window.flight_chrome = flight;
        // From the same geometry, on the same beat: what the strip draws is what
        // can be tipped, and both are decided here or neither is.
        self.rebuild_tooltip_anchors(scale, width as f32, now);
        // The overlay is rebuilt from the same choke point as the chrome under
        // it, so every path that already knew to repaint on a resize, a DPI
        // change or a theme switch carries the dialog with it for free.
        let overlay_changed = include_overlay && self.refresh_overlay();
        hang_watch::at(leaving_station);
        chrome_changed || overlay_changed
    }

    /// **One of the five menus that has a level to itself**, through its own
    /// passage and its child's.
    ///
    /// Exhaustive on the six rather than taking a closure, because which
    /// builder draws which surface is the one thing that differs, and a seventh
    /// with a band of its own must not reach this without saying so.
    pub(in crate::runtime) fn stage_menu(&mut self, popup: Popup, now: Instant) -> marks::Band {
        let paint = match popup {
            Popup::File => self.file_menu_layer(),
            Popup::Pane => self.pane_menu_layer(),
            Popup::GitMenu => self.git_menu_layer(),
            Popup::TermMenu => self.term_menu_layer(),
            Popup::Tab => self.tab_menu_layer(),
            Popup::Palette => self.palette_layer(),
            // The other four are drawn by the modal chain, which passes them
            // itself: see [`ModalBand`].
            Popup::Profile | Popup::Root | Popup::GraphFilter | Popup::Preview => MenuPaint::none(),
        };
        let mut layers = self.stage(
            Layered::Popup(popup),
            paint.menu.into(),
            Some(paint.travel),
            now,
        );
        if let Some((submenu, travel)) = paint.child {
            let child = self.stage(Layered::Submenu(popup), submenu.into(), Some(travel), now);
            layers.append(child);
        }
        layers
    }

    /// Rebuild the blended layer over the chrome. Returns whether anything
    /// visible changed.
    ///
    /// One layer, because there is only ever one thing in it: the scrim outranks
    /// every popup, so a modal that is up owns the layer outright, and the
    /// picker is closed the moment the dialog opens.
    pub(crate) fn refresh_overlay(&mut self) -> bool {
        // One clock for the whole build, for the reason `tab_trailers` reads one:
        // the drawing and the tween that fades it must not disagree about what
        // time it is, and two `Instant::now()` calls in one frame can.
        let now = Instant::now();
        // Formula geometry belongs to the frame a present is about to draw. An
        // ordinary state rebuild has no such frame in hand, so it keeps drawing
        // the follow already placed by the last present and leaves the source
        // face to `refresh_formula_overlay_for_present` at the glass door.
        let formula_tools = self.formula_tool_layers(now);
        self.refresh_overlay_with_formula(now, formula_tools)
    }

    /// **E61 — the opener closes the others**, in the one place that knows what
    /// "the others" are.
    ///
    /// Called by every opener before it raises its own, and never after: an
    /// opener that toggled first would close the menu it had just opened. The
    /// popup being raised is passed in and skipped, which is what lets a
    /// *toggle* go through the same door as an open.
    ///
    /// Nothing is repainted here. Each caller repaints once after it has raised
    /// its own popup, and a repaint in the middle of the chain would be a frame
    /// showing a window with every menu shut — a flicker on every single press
    /// that opens one.
    ///
    /// **Both `⌄` clocks stop too** (2026-08-16). Whatever ran this has answered
    /// the question the clocks were asking, and a rest that matured a frame later
    /// would re-open a menu the press had just put away.
    pub(in crate::runtime) fn close_popups_except(&mut self, keep: Popup) {
        for popup in keep.others() {
            self.close_popup(popup);
        }
        self.window.chevrons.clear();
        // **And every hover panel with them** (user report, 2026-08-19). A menu
        // is [`HoverFloat::Menu`], and the list's one ordering says a menu
        // outranks anything a hover raised: the press has just answered the
        // question the flyout or the glance was still asking. Nothing here can
        // put a menu away — that is this function's own loop, above — so a hover
        // surface can never take one down, which is the other half of the
        // ordering and the reason `keep` is not a parameter.
        self.close_hover_floats_except(HoverFloat::Menu);
    }

    /// **Put one popup away.** The single arm every closer walks, so that
    /// "closing the pane menu" is one piece of knowledge and not one per caller
    /// — E61's whole finding, which was six hand-copied runs of `self.x = None`
    /// and no two of them alike.
    ///
    /// Exhaustive on [`Popup`] on purpose: a tenth popup does not compile until
    /// it says here how it goes away.
    pub(crate) fn close_popup(&mut self, popup: Popup) {
        match popup {
            Popup::Profile => {
                // P133's rule, owed by every closer and paid by only one of
                // them until now: the arrow turns back when the list goes.
                if self.window.profile_menu.close() {
                    self.window.chevrons.menu_gone(popup);
                    self.start_chevron_turn();
                }
            }
            Popup::Root => {
                self.window.root_menu.close();
            }
            // **A menu a `⌄` governs takes its gate's pin with it** (owner
            // ruling 2026-09-23) — only when it was actually up, so that a
            // closer walking every popup does not stop a rest that is running
            // on a button whose menu was never raised.
            Popup::File => {
                if self.window.file_menu.take().is_some() {
                    self.window.chevrons.menu_gone(popup);
                }
            }
            Popup::Pane => {
                if self.window.pane_menu.take().is_some() {
                    self.window.chevrons.menu_gone(popup);
                }
            }
            Popup::GraphFilter => self.window.graph_filter_menu = None,
            Popup::GitMenu => self.window.git_menu = None,
            Popup::TermMenu => self.window.term_menu = None,
            Popup::Tab => self.window.tab_menu = None,
            Popup::Preview => {
                self.window.preview_menu.close();
            }
            // Its layout goes with it, on `search_layout`'s own reasoning: the
            // press router is `&self` and cannot lay anything out, so it tests
            // the box that was actually drawn — and a box that is not up must
            // not leave one behind for it to test.
            Popup::Palette => {
                self.window.palette = None;
                self.window.palette_layout = None;
            }
        }
        // **And the rail is asked again, because its answer just changed under a
        // pointer that did not move** (§7.1.6e″ ②). A popup the sidebar grew
        // holds the panel out for as long as it is up; the frame it goes away in
        // is therefore the frame the panel has to be re-asked about, and nothing
        // else will ask — a hand that clicked into a pane to dismiss the list is
        // already where it is going to be, and the panel would stand out over
        // the terminal until it happened to move again. The same door
        // `drive_rail_zone` is already asked at after a float opens and after a
        // drag ends, for word-for-word the same reason.
        //
        // Only for a popup that was actually the rail's: `aim_rail_at` would
        // no-op for the rest, and asking anyway would say this function knows
        // something about the stage's menus that it does not.
        if popup_owner(popup, self.tab_surface_now()) == PopupOwner::Tabs(TabSurface::Rail) {
            self.drive_rail_zone(self.window.pointer_position);
        }
    }

    /// **Every popup, with nothing kept** (user ruling 2026-08-25, B10).
    ///
    /// [`Self::close_popups_except`] is what an *opener* owes: it spares the one
    /// it is raising, because a toggle has to reach its own popup through the
    /// same door as an open. This is what a *departure* owes, and there is
    /// nothing to spare — the surface every one of them was raised over has just
    /// stopped being on the glass.
    ///
    /// The hover panels go with them for `close_popups_except`'s own reason, and
    /// the `⌄` clocks are cleared so that a rest maturing a frame later cannot
    /// re-open a menu onto the tab that has just arrived.
    pub(in crate::runtime) fn close_every_popup(&mut self) {
        for popup in Popup::ALL {
            self.close_popup(popup);
        }
        self.window.chevrons.clear();
        self.close_hover_floats_except(HoverFloat::Menu);
    }

    /// **Which popups this window has raised**, read off the window once.
    ///
    /// The one place the eight are listed against the state that holds them;
    /// see [`PopupsUp`] for why there is exactly one such place.
    ///
    /// **A popup counts only while it is one you can see** (P137, generalised
    /// 2026-08-25). The preview switcher has said this about itself since it
    /// was written — "a switcher whose pane is on a tab you are not looking at
    /// is not a menu you can see, it draws nothing, and it must swallow
    /// nothing" — and the sentence was never about switchers. Two popups hang
    /// off a pane the way that one does and had no such scope: a files column's
    /// root menu, whose `root_menu_layout` folds to `None` the moment its seat
    /// is not a files column on the tab in front, and the commit graph's branch
    /// filter, whose `graph_filter_menu_layout` folds the same way. Either one
    /// left open behind a tab switch was a menu drawing nothing at all while
    /// the window went on believing the keyboard was its — which, now that
    /// belief also decides where the keystroke goes, would be a whole window
    /// typing into nothing.
    ///
    /// **Stated as a rule and given one owner each, 2026-09-21** (closure review
    /// of `fix/pane-head-knows-the-zoom-mark`): *a menu whose layout is `None`
    /// is not up.* Transcribing a layout's fold conditions here is what let the
    /// third instance in — a preview head too narrow for its switcher dropped
    /// the pill, `preview_menu_layout` folded on it, and this line went on
    /// saying the popup was up. So the three popups whose anchor lives inside a
    /// pane now ask the same `&self` question their own layout folds on —
    /// [`Self::root_menu_stand`], [`Self::graph_filter_menu_stand`],
    /// [`Self::preview_menu_stand`] — and there is nothing left here to
    /// disagree with. The other seven are raised at a *point* (a right press, a
    /// chord), so they have no anchor that can leave the glass under them.
    pub(crate) fn popups_up(&self) -> PopupsUp {
        PopupsUp {
            profile: self.window.profile_menu.is_open(),
            root: self
                .window
                .root_menu
                .seat()
                .is_some_and(|seat| self.root_menu_stand(seat).is_some()),
            file: self.window.file_menu.is_some(),
            pane: self.window.pane_menu.is_some(),
            graph_filter: self
                .window
                .graph_filter_menu
                .as_ref()
                .is_some_and(|menu| self.graph_filter_menu_stand(menu.surface).is_some()),
            preview: self
                .preview_menu_seat()
                .is_some_and(|seat| self.preview_menu_stand(seat).is_some()),
            git_menu: self.window.git_menu.is_some(),
            term_menu: self.window.term_menu.is_some(),
            // **Not scoped to a tab that still exists**, unlike the three above
            // it. A tab menu holds a `TabId` and is anchored at a point, so it
            // goes on drawing — and goes on taking the keyboard — even after the
            // tab it names has been moved to another window under it. That is
            // the honest state: the reader is still looking at a list, and every
            // verb in it already answers "that tab is gone" by doing nothing.
            tab_menu: self.window.tab_menu.is_some(),
            palette: self.window.palette.is_some(),
        }
    }

    /// Aim the arrow at wherever the list now is.
    ///
    /// Called from both verbs and reading the menu rather than being told which
    /// way to go, so the two can never disagree: whatever put the list up or
    /// down, the arrow's target is one lookup away from the truth. The
    /// *repaint* is the caller's, which is why this only sets the target — a
    /// turn that has begun is finished by `advance_strip_animation` off the
    /// deadline `strip_animation_work` asks for.
    pub(in crate::runtime) fn start_chevron_turn(&mut self) {
        let now = Instant::now();
        self.window.chevron_turn.retarget(
            chevron_turn_target(
                self.window.profile_menu.is_open(),
                self.sampled_rail(now).profile_menu_side(),
            ),
            now,
            self.app.motion,
        );
    }

    /// **A minted page onto a float's own engine** (§7.39) — the door a page card
    /// takes when its head is carried out into a window.
    ///
    /// The one thing a float does that a pane does not: it keeps a page of its
    /// own. A float *is* a browser, carried by a leaf that is in no layout tree,
    /// which is exactly the shape `pop_out_preview` leaves a popped-out page in
    /// (§7.14a). So a detached leaf is minted on this float's tab, the float is
    /// told to carry it, and the engine is opened on it through the same gate
    /// every navigation passes ([`Self::open_minted_page_on`]).
    ///
    /// The leaf's seat is minted from the tab's own monotonic counter
    /// ([`seats::Seats::mint_detached_seat`]) and never reused, so it cannot come
    /// to name another pane's page — the guarantee [`float::FloatPreview::page`]
    /// is written against.
    pub(in crate::runtime) fn open_minted_page_on_float(
        &mut self,
        id: float::FloatId,
        mint: webnav::Mint,
    ) -> Result<()> {
        // A detached leaf on this float's tab for the float's own engine — the
        // seat a popped-out page keeps, minted fresh because this page was never
        // in a pane to leave one behind.
        let leaf = LeafId {
            tab: self.id,
            seat: self.seats.mint_detached_seat(),
        };
        // A pane at the float's surface, so the chassis has one to hand back.
        // `DOCK` lifts the float's pane and lands it, and a float carrying a page
        // but holding no pane would give `dock_preview_float` nothing to remove
        // and dock nothing. It is the same empty pane a popped-out page's carried
        // one becomes once its buffer has left it — the engine, not the pane, is
        // what the window shows.
        *self.preview_panes.entry(PreviewSurface::Float(id)) = PreviewPane::default();
        // Carried before the engine is asked for, so `page_carried_by` names it
        // the moment the web seat lands: `open_web_page_on` inserts the seat
        // synchronously, so both facts are true by the end of this call.
        if let Some(win) = self.window.float.live_mut(id)
            && let float::FloatTenant::Preview(preview) = &mut win.tenant
        {
            preview.page = Some(leaf);
        }
        self.open_minted_page_on(leaf, mint)
    }

    /// **Everything one file menu draws that its subject cannot carry** — the
    /// folded levels' names, and the mark a `New terminal here` wears.
    ///
    /// The names are the caller's because they belong to the *menu that is up*
    /// (`FileMenuState::crumbs`) and this is asked from three places that each
    /// already hold them; the mark is read here because "which profile is the
    /// default" is a setting and [`Self::default_profile`] is the one reader of
    /// it this window has (user ruling 2026-08-25).
    ///
    /// **And it is stripped of its colours on the way out** (the same day's
    /// second ruling, [`marks::ChromeMark::in_line`]). Here rather than in
    /// `profiles`, because this is the seam between "which shell is the
    /// default", which is a setting, and "what does a row of this menu look
    /// like", which is the menu's own business — and the menu's business is a
    /// column of thin monochrome glyphs.
    pub(in crate::runtime) fn file_menu_look<'a>(
        &self,
        subject: profiles::FileMenuSubject,
        powers: profiles::FileMenuPowers,
        crumbs: &'a [String],
    ) -> profiles::FileMenuLook<'a> {
        profiles::FileMenuLook {
            subject,
            powers,
            crumbs,
            terminal: profiles::mark(self.default_profile()).in_line(),
        }
    }

    /// Raise the menu on one tree row.
    ///
    /// **Both kinds of node, since the user ruling of 2026-08-25.** K143 refused
    /// a directory here — mock-up 8122's "directory rows etc. keep the native
    /// menu for now" — and the refusal was sound while the menu was three verbs
    /// that were all about a file. The ruling ends that: two of the three were
    /// always answerable over a folder's path, and a folder has two verbs of its
    /// own that no other surface offers *at the row*. Which rows a subject gets
    /// is [`profiles::file_menu`]'s answer and not this function's.
    ///
    /// **What is still refused is stated in one place each.** A row that leads
    /// nowhere and shows nothing raises no menu ([`files_row_menu_subject`]), and
    /// a column with no root raises none either: [`RowActivation::Nowhere`] means
    /// there is no path to put on a clipboard, and a menu whose every verb does
    /// nothing is worse than no menu.
    pub(in crate::runtime) fn open_file_menu(
        &mut self,
        target: FileMenuTarget,
        point: [f32; 2],
    ) -> Result<()> {
        if target.activation.path().is_none() {
            return Ok(());
        }
        // **A menu with no rows is not a menu** (user ruling 2026-08-25). The
        // one subject that can be empty is the `…` chip's, and it can only be
        // empty by arriving from a rail that is not folded — which is a bug
        // upstream rather than a state to draw. Refusing here rather than
        // drawing an empty frame is also what lets `profiles::file_menu_step`
        // index its own list: the keyboard cannot be handed a menu with nothing
        // in it.
        if profiles::file_menu(target.subject, file_menu_powers(target.row.as_ref()))
            .rows
            .is_empty()
        {
            return Ok(());
        }
        // E61: the opener closes the others. Not the float — that is a place, not
        // a popup, and this menu is very often *about a row inside it*.
        self.close_popups_except(Popup::File);
        // **A file menu already up is replaced, which is its going away** (owner
        // ruling 2026-09-23): the new one starts unpinned, and a press that
        // raised it pins it on its own way out.
        self.window.chevrons.menu_gone(Popup::File);
        self.window.file_menu = Some(FileMenuState {
            point,
            row: target.row,
            activation: target.activation,
            subject: target.subject,
            crumbs: target.crumbs,
            hover: None,
            trigger: target.trigger,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Raise the file menu on a row named by the keyboard rather than by the
    /// pointer (K143's "可键盘化").
    ///
    /// It hangs from the row's own bottom-left corner, which is where a menu
    /// belonging to a list item belongs when no pointer said otherwise — the
    /// same placement the root menu takes under its button. The rectangle is
    /// re-derived from the live layout at the moment of the press, through the
    /// one function the hit test also uses, so a menu raised by a key and a menu
    /// raised by a click on the same row cannot come up in two places.
    pub(in crate::runtime) fn raise_file_menu_on_row(
        &mut self,
        seat: SeatId,
        key: &str,
    ) -> Result<()> {
        let now = Instant::now();
        let scale = self.window.renderer.scale_factor() as f32;
        let trees = self.files_trees(now);
        let Some((index, kind)) = trees.get(&seat).and_then(|tree| {
            tree.rows
                .iter()
                .position(|row| row.key == key)
                .map(|index| (index, tree.rows[index].kind))
        }) else {
            return Ok(());
        };
        let Some(subject) = files_row_menu_subject(kind) else {
            return Ok(());
        };
        let Some(geometry) = seats::files_tree_geometry_of(
            &self.seat_layout,
            &trees,
            scale,
            self.git_panel_on(),
            seat,
        ) else {
            return Ok(());
        };
        let rect = geometry.row_rect(index);
        let root = self.window.tabs[self.window.active_tab]
            .files_state(seat)
            .root;
        self.open_file_menu(
            FileMenuTarget {
                row: Some(FileMenuTreeRow {
                    host: RowHost::Column(seat),
                    key: key.to_owned(),
                }),
                activation: files_row_activation(&root, key),
                subject,
                crumbs: Vec::new(),
                trigger: None,
            },
            [rect[0], rect[3]],
        )
    }

    /// The file menu's own level of the overlay stack, or nothing when none is
    /// up.
    fn file_menu_layer(&mut self) -> MenuPaint {
        let Some(layout) = self.file_menu_layout() else {
            return MenuPaint::none();
        };
        let Some(menu) = self.window.file_menu.as_ref() else {
            return MenuPaint::none();
        };
        let (subject, hover) = (menu.subject, menu.hover);
        let powers = file_menu_powers(menu.row.as_ref());
        let crumbs: Vec<String> = menu.crumbs.iter().map(|level| level.name.clone()).collect();
        let look = self.file_menu_look(subject, powers, &crumbs);
        let travel = layout.travel();
        MenuPaint::plain(profiles::file_menu_build(&layout, &look, hover), travel)
    }

    pub(in crate::runtime) fn close_file_menu(&mut self) -> Result<bool> {
        if self.window.file_menu.take().is_none() {
            return Ok(false);
        }
        // The pill's gate goes with it, on [`Self::close_profile_menu`]'s note: a
        // grace still running against a menu that has already gone would fire a
        // second close on an empty state, and a rest maturing a frame later would
        // raise a menu the thing that closed this one had just answered. And its
        // pin goes with the menu (owner ruling 2026-09-23).
        self.window.chevrons.menu_gone(Popup::File);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// What the pane can answer for, as of now.
    ///
    /// Read once, when the menu is raised, and carried in [`TermMenuState`] from
    /// then on — see that struct for why a snapshot rather than a live read: the
    /// shell under an open menu goes on printing, and a row that greyed itself
    /// out from under a descending hand would be worse than one that was greyed
    /// from the start.
    fn term_menu_subject(&self, seat: SeatId) -> profiles::TermMenuSubject {
        profiles::TermMenuSubject {
            // The same question `Copy` will ask when it runs: not "is there a
            // pair of anchors" but "is there text between them", which is what
            // `write_selection_text` refuses on. A collapsed selection — a bare
            // click that never travelled — has anchors and no bytes, and a
            // `Copy` offered for it would put nothing on the clipboard and say
            // nothing about having done so.
            has_selection: self
                .sessions
                .get(&seat)
                .and_then(|leaf| leaf.session.selection_text())
                .is_some_and(|text| !text.is_empty()),
            restart_in_flight: self.window.restarting == Some(seat),
            // The same question `Find…` will ask when it runs, asked of the same
            // function: one door, so a greyed row and a declined verb can never
            // disagree about which panes a search can be addressed to.
            can_search: self.seat_can_search(seat),
        }
    }

    /// Raise the menu a right press asked for.
    ///
    /// E61 first: the opener closes every other popup, which for this one is the
    /// whole of what keeps a right press inside a pane from dropping a menu on
    /// top of the pane-head menu already hanging over it.
    pub(in crate::runtime) fn open_term_menu_at(
        &mut self,
        seat: SeatId,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        self.close_popups_except(Popup::TermMenu);
        // §7.1.6i's floor: the pane-verb segment is drawn exactly when the head
        // that would otherwise carry those verbs is not on screen. Asked of the
        // same predicate the head itself is drawn from, so "the head is here" and
        // "the menu repeats the head" can never be two answers.
        let lone = !self.seats.seat_wears_head(bt_layout::SeatKind::Terminal);
        self.window.term_menu = Some(TermMenuState {
            point: [position.x as f32, position.y as f32],
            seat,
            pane: profiles::TermMenuPane::Shell,
            subject: self.term_menu_subject(seat),
            hover: None,
            lone,
            submenu_open: false,
            pointer_was: None,
            submenu_hold_until: None,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **The same menu, raised on a rendered page** (user report 2026-08-28).
    ///
    /// [`Self::open_term_menu_at`]'s twin and deliberately not a second machine:
    /// what differs between the two is the row list and the two verbs behind it,
    /// which is exactly what [`profiles::TermMenuPane`] carries. The segment is
    /// off — those are the pane's verbs and this seat always wears the head that
    /// holds them — and `Copy` is greyed unless there are bytes to write, which
    /// is the terminal's own question asked of this surface's own selection.
    pub(in crate::runtime) fn open_page_menu_at(
        &mut self,
        seat: SeatId,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        self.close_popups_except(Popup::TermMenu);
        let has_selection = self
            .preview_selected_text(self.preview_here(seat))
            .is_some();
        self.window.term_menu = Some(TermMenuState {
            point: [position.x as f32, position.y as f32],
            seat,
            pane: profiles::TermMenuPane::Page,
            subject: profiles::TermMenuSubject {
                has_selection,
                ..profiles::TermMenuSubject::default()
            },
            hover: None,
            lone: false,
            submenu_open: false,
            pointer_was: None,
            submenu_hold_until: None,
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The terminal menu's own level of the overlay stack.
    fn term_menu_layer(&mut self) -> MenuPaint {
        let Some((look, seat)) = self.window.term_menu.as_ref().map(|menu| {
            (
                profiles::TermMenuLook {
                    pane: menu.pane,
                    subject: menu.subject,
                    hover: menu.hover,
                    lone: menu.lone,
                    submenu_open: menu.submenu_open,
                },
                menu.seat,
            )
        }) else {
            return MenuPaint::none();
        };
        let Some(layout) = self.term_menu_layout() else {
            return MenuPaint::none();
        };
        let travel = layout.travel();
        // The child's own direction, or the parent's for the frames where there
        // is no child to have one — an empty band reads neither.
        let child_travel = layout.submenu_travel().unwrap_or(Travel::Right);
        // The profile the pane is *running*, which is what the child marks —
        // `pane_menu_layer`'s own sentence, read at this menu's door: a pane you
        // split from a Git Bash is a Git Bash, and a child that ticked PowerShell
        // on it would be telling you about the window rather than about the pane.
        // `position_of` and not `index_of_id`: the tick names a row of the table
        // being drawn, and a pane whose profile has been deleted has no row to
        // tick rather than the fallback's.
        let current = self
            .sessions
            .get(&seat)
            .and_then(|leaf| profiles::position_of(&leaf.profile));
        let programs = &self.app.profile_programs;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        let mut layers = profiles::term_menu_build(&layout, &look, current, programs, &mut measure);
        // The seam `push_submenu` states: the menu on the first layer, the child
        // on the ones after it. Splitting there is what gives the two their own
        // clocks.
        let child = layers.split_off(1);
        MenuPaint {
            menu: layers,
            travel,
            child: Some((child, child_travel)),
        }
    }

    pub(in crate::runtime) fn close_term_menu(&mut self) -> Result<bool> {
        if self.window.term_menu.take().is_none() {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Spend an entry.
    ///
    /// The menu is **taken** first, so every verb below runs with it already
    /// gone: a row that raises a gate would otherwise leave a menu standing
    /// behind the scrim, and a row that spawns a shell would leave one hanging
    /// over a pane that is being rebuilt underneath it.
    ///
    /// **The segment's rows go straight to the pane menu's own runner**
    /// (§7.1.6i, §7.1.6e's rule): one verb, two doors, one implementation. A
    /// `Duplicate pane` reached by right-clicking a lone pane and one reached
    /// from a head's `⌄` are the same call with the same seed, and there is
    /// nowhere for them to drift apart.
    pub(in crate::runtime) fn run_term_menu_row(
        &mut self,
        hit: profiles::TermMenuHit,
    ) -> Result<()> {
        // The menu's own padding, either rule, a greyed row. A press there is the
        // menu swallowing it — decided in `chrome_mouse_input` — so there is
        // nothing to spend and, in particular, no menu to take away.
        if hit == profiles::TermMenuHit::Surface {
            return Ok(());
        }
        // Resolved while the child still exists — see the `Submenu` arm below,
        // and [`Runtime::run_pane_menu_row`], which does the same thing for the
        // same reason one menu over.
        let submenu = match hit {
            profiles::TermMenuHit::Submenu(at) => {
                match self.term_menu_layout().and_then(|l| l.submenu_row(at)) {
                    Some(of) => Some(of),
                    None => return Ok(()),
                }
            }
            _ => None,
        };
        let Some(menu) = self.window.term_menu.take() else {
            return Ok(());
        };
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        let seat = menu.seat;
        let row = match hit {
            profiles::TermMenuHit::Row(profiles::TermMenuEntry::Term(row)) => row,
            profiles::TermMenuHit::Row(profiles::TermMenuEntry::Pane(row)) => {
                return self.run_pane_verb(seat, profiles::PaneMenuHit::Row(row));
            }
            // The child's row counts rows on the glass since B9, so the profile
            // it is about is asked of the layout — and asked *before* the take
            // above, which is why `submenu` is resolved at the top of this
            // function rather than here.
            profiles::TermMenuHit::Submenu(_) => {
                let Some(profile) = submenu else {
                    return Ok(());
                };
                return self.run_pane_verb(seat, profiles::PaneMenuHit::Submenu(profile));
            }
            profiles::TermMenuHit::Surface => return Ok(()),
        };
        // **A page's two verbs answer about the page**, not about a shell that
        // is not under this menu. Split here rather than by a second runner, so
        // that everything above — the take, the frame, the segment's door — is
        // said once for both.
        if menu.pane == profiles::TermMenuPane::Page {
            let surface = self.preview_here(seat);
            return match row {
                // The clipboard door copy-on-select uses, so the menu and the
                // drag write the same bytes and both leave the selection
                // standing — the terminal's row's own rule, one surface over.
                profiles::TermMenuRow::Copy => {
                    self.copy_preview_text_selection(surface);
                    Ok(())
                }
                profiles::TermMenuRow::SelectAll => {
                    let selection = match &self.preview_pane(surface).map(|pane| &pane.doc) {
                        Some(PreviewDocument::Markdown { blocks, .. }) => {
                            preview_select::select_all(&preview_select::pieces(blocks))
                        }
                        _ => None,
                    };
                    if selection.is_some() {
                        self.preview_pane_mut(surface).md_select = selection;
                        self.repaint_preview()?;
                    }
                    Ok(())
                }
                // Nothing else is on a page's list — see
                // [`profiles::TermMenuPane::Page`].
                _ => Ok(()),
            };
        }
        match row {
            // **Copy-on-select's own door**, so that the menu and the drag put
            // the same bytes on the clipboard and both leave the selection
            // standing. `Ctrl+C`'s door is the other one (`copy_selection`),
            // which clears the selection afterwards — right for a keystroke that
            // ends a gesture, wrong for a menu row that was reached by pointing
            // at what is still highlighted.
            profiles::TermMenuRow::Copy => {
                self.copy_selection_on_release(seat);
                Ok(())
            }
            profiles::TermMenuRow::Paste => self.paste_from_clipboard_into(seat),
            profiles::TermMenuRow::SelectAll => self.select_all_in_pane(seat),
            profiles::TermMenuRow::Find => self.open_search(seat),
            profiles::TermMenuRow::ClearScreen => self.clear_pane_screen(seat),
            // **The gate, and the verb behind it** — `GitDiscard`'s shape: the
            // question stands in front of the deletion rather than behind an
            // interrupted one, so it is asked here and answered in
            // [`Runtime::answer_dirty_gate`]. A pane with nothing in its
            // scrollback raises no gate (`gate_dirty_names` is empty) and the
            // clear goes straight through, which is right: there is nothing to
            // ask about when there is nothing to lose.
            profiles::TermMenuRow::ClearScrollback => {
                if !self
                    .raise_dirty_gate(restore::GateRequest::ClearScrollback(seat))?
                    .proceeds()
                {
                    return Ok(());
                }
                self.clear_pane_scrollback(seat)
            }
            profiles::TermMenuRow::RestartShell => self.restart_shell(seat),
        }
    }

    /// Open or shut this menu's `Split with` child, and report whether anything
    /// moved. [`Runtime::set_pane_submenu`]'s twin, on the second door.
    pub(in crate::runtime) fn set_term_submenu(&mut self, open: bool) -> Result<bool> {
        let Some(menu) = self.window.term_menu.as_mut() else {
            return Ok(false);
        };
        if menu.submenu_open == open || (open && !menu.lone) {
            return Ok(false);
        }
        menu.submenu_open = open;
        menu.submenu_hold_until = None;
        // The highlight follows the surface it is on: opening lands on the first
        // profile, closing takes it back to the heading it came from, so `←`
        // leaves the keyboard somewhere rather than nowhere.
        menu.hover = Some(if open {
            profiles::TermMenuHover::Submenu(0)
        } else {
            profiles::TermMenuHover::Row(profiles::TermMenuEntry::Pane(
                profiles::PaneMenuRow::SplitWith,
            ))
        });
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// The terminal menu's own clocks, matured —
    /// [`Runtime::advance_pane_menu`]'s twin, and two clocks in one slot for its
    /// reason: a menu cannot be both waiting to open its child and holding it
    /// open against the rows.
    pub(in crate::runtime) fn advance_term_menu(&mut self, now: Instant) -> Result<()> {
        let Some(menu) = self.window.term_menu.as_ref() else {
            return Ok(());
        };
        let Some(due) = menu.submenu_hold_until else {
            return Ok(());
        };
        if now < due {
            return Ok(());
        }
        if menu.submenu_open {
            self.set_term_submenu(false)?;
            if let Some(position) = self.window.pointer_position {
                self.drive_term_menu_hover(position)?;
            }
            return Ok(());
        }
        self.set_term_submenu(true)?;
        Ok(())
    }

    /// The terminal menu's next wake-up, for the loop's set.
    pub(in crate::runtime) fn term_menu_deadline(&self) -> Option<Instant> {
        self.window.term_menu.as_ref()?.submenu_hold_until
    }

    /// Which files flyout — if any — this point would raise.
    ///
    /// Its own function because two callers ask it now: the pointer's own move,
    /// and [`Self::rearm_hover_intents`] asking again once the glass came free.
    /// A second copy of the match would be a second opinion about which chrome
    /// targets are triggers.
    ///
    /// `&mut self` for [`Self::layout_peek_target_at`]'s reason: the trigger is
    /// read through the float-aware router, so a `Files` button a window is
    /// standing on arms nothing.
    pub(in crate::runtime) fn float_trigger_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<float::FloatTrigger> {
        match self.chrome_target_at(position) {
            Some(seats::ChromeTarget::TabFiles(index)) => self
                .window
                .tabs
                .get(index)
                .map(|tab| float::FloatTrigger::Tab(tab.id)),
            Some(seats::ChromeTarget::PaneFiles(seat)) => Some(float::FloatTrigger::Pane(LeafId {
                tab: self.window.tabs[self.window.active_tab].id,
                seat,
            })),
            // **And a folder named in the output** (user ruling 2026-08-27,
            // §7.29). Last, for [`Self::row_under`]'s reason: the chrome is
            // drawn over the panes, so a cell is what is left when nothing on
            // the glass has claimed the point.
            _ => self.folder_reference_trigger(),
        }
    }

    /// **Both `⌄` clocks, told where the pointer is** (user ruling, 2026-08-16).
    ///
    /// Called from the one place every pointer move passes through, above the
    /// routing that returns early: a chevron's rest has to keep accumulating
    /// while the pointer is over a *menu* — that is precisely the state the leave
    /// grace exists to distinguish — so it cannot be observed from inside a
    /// branch that a menu's own hover handler returns out of.
    ///
    /// Where the pointer is, for each chevron, is three questions asked of three
    /// geometries and answered once here rather than at each gate: the button's
    /// own box, the menu's frame (and its submenu's), and everything else.
    ///
    /// **`None` is a place, and it is `Away`** (user report, 2026-08-21). The
    /// title bar's drag band — everything right of the `⌄` and left of the
    /// caption run — is `HTCAPTION`, which is to say Win32's and not this
    /// window's: `bt_platform::custom_frame_hit_test` answers `Caption` there,
    /// so the pointer moving into it produces one `WindowEvent::CursorLeft` and
    /// then **no motion at all** for as long as the hand rests there. A gate
    /// that only ever hears about the pointer through `CursorMoved` is a gate
    /// that never learns the hand has gone, and the menu it opened stands until
    /// something else takes it down. So the *absence* of a position is fed in as
    /// the answer it is — nothing app-owned lies outside the client rect, so the
    /// pointer is on neither button and on neither menu — and the same 150ms
    /// grace runs from it as from any other departure. Nothing here special-cases
    /// leaving: `None` simply fails both hit tests, exactly as a point in the
    /// middle of a pane does.
    pub(in crate::runtime) fn observe_chevrons(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
        now: Instant,
    ) {
        // **A drag owns the pointer outright**, and a gesture in flight is not a
        // hover — the same rule the tip, the layout peek and the float's own
        // intent already live by (`rebuild_tooltip_anchors` empties its whole
        // list for the length of a drag). A menu dropped under a pane being
        // carried across the window would be a menu nobody asked for, standing
        // in the way of the drop.
        if self.window.drag.is_some() || self.window.float_drag.is_some() {
            self.window.chevrons.clear();
            return;
        }
        // **And no rest is accumulated underneath another hover panel** (user
        // report, 2026-08-19). The flyout's closing grace is 420ms leftward and
        // this rest is 250, so a hand walking off a flyout onto the `⌄` beside
        // it used to open a menu *over* a window that had not finished leaving —
        // two hover panels on the glass at once, which is the screenshot. The
        // clock is not merely refused a maturity here, it never starts: an
        // intent that ran invisibly and fired the instant the other panel went
        // would be the same overlap one frame later. It is offered again by
        // [`Self::rearm_hover_intents`] the moment the glass is free.
        //
        // A menu already up is not "another panel" — this gate is what keeps it
        // open — so the whole of [`Popup`] is excluded from the question.
        if !self.hover_float_free(HoverFloat::Menu) {
            self.window.chevrons.clear();
            return;
        }
        let profile_open = self.window.profile_menu.is_open();
        let pane_open = self.window.pane_menu.is_some();
        // **One walk of the hit test for all three clocks**, and it is the very
        // walk the press router makes: what the pointer is standing on is asked
        // as a *trigger* ([`PopoverTrigger`]), so a rest on the `Open ⌄` and a
        // press on the `Open ⌄` cannot come to disagree about which button that
        // is. The two clocks below want a plain chrome target, which is the
        // `Chrome` arm of the same answer; the rail's wants the pill.
        let standing_on = position.and_then(|position| self.popover_trigger_at(position));
        let target = match standing_on {
            Some(PopoverTrigger::Chrome(target)) => Some(target),
            _ => None,
        };
        let on_profile_button = matches!(target, Some(seats::ChromeTarget::NewTabMenu));
        let pane_seat = self.window.pane_menu.as_ref().map(|menu| menu.seat);
        // A rest on *any* pane head's chevron arms that head's menu — including
        // a second head's while the first head's menu is up, which is how a hand
        // walks a menu across a split without clicking. The gate is told
        // `Button` for that case with `open` false, because the menu that is up
        // is not this button's.
        let on_pane_button = match target {
            Some(seats::ChromeTarget::PaneMenu(seat)) => Some(seat),
            _ => None,
        };
        let profile_where = if on_profile_button {
            profiles::ChevronPointer::Button
        } else if profile_open
            && position.is_some_and(|position| {
                self.profile_menu_layout()
                    .is_some_and(|layout| layout.contains(position.x as f32, position.y as f32))
            })
        {
            profiles::ChevronPointer::Surface
        } else {
            profiles::ChevronPointer::Away
        };
        // **The menu's whole region, not its two rectangles** — see
        // [`profiles::PaneMenuLayout::holds`]. `contains` answers where the
        // pointer *is*; the grace needs to know whether the menu still has this
        // hand, and a hand cutting diagonally toward a child row that hangs
        // below the parent's own bottom edge is over neither box while it is
        // plainly still dealing with the menu.
        //
        // `pointer_was` is last move's position, which is what makes the
        // triangle a statement about direction — and it is still last move's
        // here because this runs at the head of `pointer_moved`, above
        // `drive_pane_menu_hover`, which is where it is re-seated. Asking after
        // that would hand the triangle `from == to`, and a degenerate triangle
        // answers "yes" to everything.
        let pane_was = self
            .window
            .pane_menu
            .as_ref()
            .and_then(|menu| menu.pointer_was);
        let pane_at = position.map(|position| [position.x as f32, position.y as f32]);
        let pane_on_surface = pane_open
            && pane_at.is_some_and(|pane_at| {
                self.pane_menu_layout()
                    .is_some_and(|layout| layout.holds(pane_was, pane_at))
            });
        let (pane_where, pane_owner_open) = match (on_pane_button, pane_on_surface) {
            (Some(seat), _) => (
                profiles::ChevronPointer::Button,
                pane_seat == Some(seat) && pane_open,
            ),
            (None, true) => (profiles::ChevronPointer::Surface, pane_open),
            (None, false) => (profiles::ChevronPointer::Away, pane_open),
        };
        // **And the preview rail's `Open ⌄`, by the same three questions** (user
        // ruling 2026-09-10: 「展开菜单的控件 hover 就开,执行动作的控件必须点」).
        //
        // `on_rail_pill` names a *surface* where the pane's names a seat, and
        // `rail_menu` is the surface whose pill raised the menu that is up — so
        // the `open` flag handed to the gate is again "the menu that is up is
        // this button's", and a hand walking from a docked rail's pill onto a
        // float's arms the float's exactly as it arms a second pane head's.
        //
        // The menu's own region is its frame: this face has no submenu, so
        // [`profiles::file_menu_hit`] answering at all is the whole of "the hand
        // is still on the menu" — no safety triangle, because there is no child
        // to cut a corner towards.
        let rail_menu = self.window.file_menu.as_ref().and_then(FileMenuState::rail);
        let rail_open = rail_menu.is_some();
        let on_rail_pill = match standing_on {
            Some(PopoverTrigger::Rail(surface, seats::PreviewRailPart::OpenWith)) => Some(surface),
            _ => None,
        };
        let rail_on_surface = rail_open
            && position.is_some_and(|position| {
                self.file_menu_layout().is_some_and(|layout| {
                    profiles::file_menu_hit(&layout, position.x, position.y).is_some()
                })
            });
        let (rail_where, rail_owner_open) = match (on_rail_pill, rail_on_surface) {
            (Some(surface), _) => (profiles::ChevronPointer::Button, rail_menu == Some(surface)),
            (None, true) => (profiles::ChevronPointer::Surface, rail_open),
            (None, false) => (profiles::ChevronPointer::Away, rail_open),
        };
        self.window.chevrons.observe(
            (profile_where, profile_open),
            (pane_where, pane_owner_open),
            (rail_where, rail_owner_open),
            now,
        );
    }

    /// **Which trigger the pointer is standing on**, if it is standing on one
    /// (owner's report 2026-09-13; user ruling 2026-09-10 for the pill it grew
    /// out of).
    ///
    /// The one place a point becomes a [`PopoverTrigger`], which is what makes
    /// the comparison the rule is built on a comparison of like with like: a
    /// button the hover clock is watching and a button a press lands on are read
    /// off the same walk, and the canonical spelling of each control is decided
    /// here and nowhere else.
    ///
    /// Both hosts through [`Self::pointer_target_at`], which already keeps
    /// [`Self::file_row_under`]'s order: a window is drawn over the panes, so a
    /// point inside a float belongs to the float — and a float that covers a
    /// docked pill without offering one of its own is an answer of "no pill",
    /// not a fall-through to the pane underneath.
    ///
    /// **A float answers for two controls and no more.** The rail's band and the
    /// graph's toolbar are the only furniture a torn-off window draws that can
    /// raise a popover; the rest of its chassis — its head, its grip, its `DOCK`
    /// — raises none, and naming them here would be inventing triggers for
    /// popovers that do not exist.
    pub(in crate::runtime) fn popover_trigger_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<PopoverTrigger> {
        match self.pointer_target_at(position)? {
            PointerTarget::Float(id, part) => {
                let surface = PreviewSurface::Float(id);
                match part {
                    float::FloatPart::Rail(part) => Some(PopoverTrigger::Rail(surface, part)),
                    float::FloatPart::GraphTool(tool) => {
                        Some(PopoverTrigger::GraphTool(surface, tool))
                    }
                    _ => None,
                }
            }
            PointerTarget::Chrome(target) => Some(self.docked_popover_trigger(target)),
        }
    }

    /// **What one popup hangs from** — the register, and the only place any of
    /// these six answers is written down (owner's report 2026-09-13).
    ///
    /// A `match` over [`Popup`] rather than a field on each state, so that a
    /// popup added to that list has to say here whether it has a trigger: the
    /// compiler asks the question, which is the whole reason the register is
    /// shaped like this and not like six `if let`s in the router.
    ///
    /// **`None` is a real answer and the commonest one.** Four of the ten are
    /// raised by a gesture — a right press on a row, on a pane, on a tab — or by
    /// a chord, and a popover with no button cannot be closed by pressing its
    /// button. Every press outside those four dismisses them and then goes on
    /// being the press it was, which is how a second right press moves a context
    /// menu from one row to another.
    pub(in crate::runtime) fn popup_trigger(&self, popup: Popup) -> Option<OwnTrigger> {
        let button = |control| {
            Some(OwnTrigger {
                control,
                keeps_the_press: false,
            })
        };
        match popup {
            Popup::Profile => self
                .window
                .profile_menu
                .is_open()
                .then_some(PopoverTrigger::Chrome(seats::ChromeTarget::NewTabMenu))
                .and_then(button),
            Popup::Root => self
                .window
                .root_menu
                .seat()
                .map(|seat| PopoverTrigger::Chrome(seats::ChromeTarget::FilesRoot(seat)))
                .and_then(button),
            Popup::File => self
                .window
                .file_menu
                .as_ref()
                .and_then(|menu| menu.trigger)
                .and_then(button),
            Popup::Pane => self
                .window
                .pane_menu
                .as_ref()
                .map(|menu| PopoverTrigger::Chrome(seats::ChromeTarget::PaneMenu(menu.seat)))
                .and_then(button),
            Popup::GraphFilter => self
                .window
                .graph_filter_menu
                .as_ref()
                .map(|menu| PopoverTrigger::GraphTool(menu.surface, git_graph::GraphTool::Filter))
                .and_then(button),
            // **The one control that keeps its press** — see
            // [`OwnTrigger::keeps_the_press`]. The head's name is a title, a drag
            // handle and the rename door as well as this menu's button, and the
            // second click of a pair belongs to the editor (P136).
            Popup::Preview => self.preview_menu_seat().map(|seat| OwnTrigger {
                control: PopoverTrigger::Chrome(seats::ChromeTarget::PreviewName(seat)),
                keeps_the_press: true,
            }),
            // Raised by a gesture or by a chord, and therefore by no button.
            Popup::GitMenu | Popup::TermMenu | Popup::Tab | Popup::Palette => None,
        }
    }

    /// **The gate of a `⌄` menu that is up**, or `None` when `popup` is not one
    /// of the three a `⌄` governs or is not up (owner ruling 2026-09-23).
    ///
    /// The strip's picker and the pane head's menu are governed whenever they
    /// are up; the file menu only while it is the one a preview rail's
    /// `Open ⌄` raised — the same menu under a tree row's right press, or under
    /// the breadcrumb's `…` chip, is not a chevron's and is never pinned.
    pub(in crate::runtime) fn chevron_menu_up(
        &mut self,
        popup: Popup,
    ) -> Option<&mut profiles::ChevronGate> {
        let up = match popup {
            Popup::Profile => self.window.profile_menu.is_open(),
            Popup::Pane => self.window.pane_menu.is_some(),
            Popup::File => self
                .window
                .file_menu
                .as_ref()
                .is_some_and(|menu| menu.rail().is_some()),
            Popup::Root
            | Popup::GraphFilter
            | Popup::Preview
            | Popup::GitMenu
            | Popup::TermMenu
            | Popup::Tab
            | Popup::Palette => false,
        };
        if up {
            self.window.chevrons.gate(popup)
        } else {
            None
        }
    }

    /// **A press on a closed `⌄` pins the menu it opened** (owner ruling
    /// 2026-09-23: 「直接点按钮=钉住」) — called by each button's press arm right
    /// after its opener. The rest-open in [`Self::advance_chevrons`] goes
    /// through the same openers and never through here, which is the whole
    /// difference between a peek and a pinned menu.
    ///
    /// Nothing is pinned when the opener raised nothing (a pane with no shell,
    /// a pill with no path): the pin belongs to a menu that is up.
    pub(in crate::runtime) fn pin_the_chevron_menu_a_press_opened(&mut self, popup: Popup) {
        if let Some(gate) = self.chevron_menu_up(popup) {
            gate.pin();
        }
    }

    /// **The three hover-open clocks, matured** — the one place any of these
    /// menus is opened or closed by time rather than by a press.
    ///
    /// Every gate is read through [`profiles::ChevronGate::due`] and acted on by
    /// the same two verbs each button already has, which is what makes the
    /// ruling's "两处 ⌄ 语义完全对齐" — and the rail pill's later enrolment in it
    /// — a property of the code rather than a coincidence of three
    /// implementations.
    pub(in crate::runtime) fn advance_chevrons(&mut self, now: Instant) -> Result<()> {
        if let Some(action) = self.window.chevrons.profile.due(now) {
            self.window.chevrons.profile.clear();
            match action {
                profiles::ChevronAction::Open => {
                    if !self.window.profile_menu.is_open() {
                        self.toggle_profile_menu()?;
                    }
                }
                profiles::ChevronAction::Close => {
                    self.close_profile_menu()?;
                }
            }
        }
        if let Some(action) = self.window.chevrons.pane.due(now) {
            self.window.chevrons.pane.clear();
            match action {
                profiles::ChevronAction::Open => {
                    // Which head is under the pointer is asked again here rather
                    // than remembered with the clock: the rest is 250ms long and
                    // a layout can change inside it.
                    if let Some(position) = self.window.pointer_position
                        && let Some(seats::ChromeTarget::PaneMenu(seat)) =
                            self.chrome_target_at(position)
                    {
                        self.toggle_pane_menu(seat)?;
                    }
                }
                profiles::ChevronAction::Close => {
                    self.close_pane_menu()?;
                }
            }
        }
        if let Some(action) = self.window.chevrons.rail.due(now) {
            self.window.chevrons.rail.clear();
            match action {
                profiles::ChevronAction::Open => {
                    // Which pill is under the pointer is asked again here, for
                    // the pane head's reason one arm up — and through the very
                    // door a press on it goes through, so the menu a rest raises
                    // and the menu a click raises are one menu in one place.
                    if let Some(position) = self.window.pointer_position
                        && let Some(PopoverTrigger::Rail(surface, seats::PreviewRailPart::OpenWith)) =
                            self.popover_trigger_at(position)
                    {
                        self.open_preview_rail_menu(surface)?;
                    }
                }
                // **Only the menu a pill raised is closed by a pill's grace.**
                // A file menu a right press put on a tree row is not this
                // clock's, and the clock never runs against one — but the
                // ownership is re-asked here rather than trusted across the
                // 150ms, exactly as the head under the pointer is above.
                profiles::ChevronAction::Close => {
                    if self
                        .window
                        .file_menu
                        .as_ref()
                        .is_some_and(|menu| menu.rail().is_some())
                    {
                        self.close_file_menu()?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Do what one row of the file menu says, and put the menu away.
    ///
    /// The menu closes *first* in every case, on the mock-up's own order
    /// (8094): two of these verbs move the keyboard somewhere else, and a menu
    /// still on screen after the focus has left it is a menu the next Esc will
    /// be spent on.
    pub(in crate::runtime) fn run_file_menu_row(
        &mut self,
        row: profiles::FileMenuRow,
    ) -> Result<()> {
        let Some(menu) = self.window.file_menu.take() else {
            return Ok(());
        };
        self.window.chevrons.menu_gone(Popup::File);
        let (activation, tree_row, crumbs) = (menu.activation, menu.row, menu.crumbs);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        // **A folded level is answered from the menu's own list and not from the
        // target's path** (user ruling 2026-08-25). Every other row here acts on
        // one path, which is what `activation` carries; this face has as many
        // paths as it has rows, and each row carries its own — see
        // [`FoldedLevel`]. Taken before the single-path gate below, because a
        // row of this face is not about that path at all.
        if let profiles::FileMenuRow::Crumb(at) = row {
            let Some(level) = crumbs.get(at) else {
                return Ok(());
            };
            let folder = level.folder.clone();
            return self.locate_folder_in_files_column(&folder, None);
        }
        let Some(path) = activation.path().map(Path::to_path_buf) else {
            return Ok(());
        };
        match row {
            // The same two doors the double click walks through, reached through
            // the same match rather than through a second copy of it: the menu
            // row and the double click are one verb with two ways of asking for
            // it, and a window in which they could ever disagree would be a
            // window where the menu's own wording had become a lie.
            profiles::FileMenuRow::Open => match activation {
                RowActivation::Preview(path) => self.open_preview(path)?,
                // The one door out of this window, and the same one the head's
                // arrow used before the 2026-08-24 ruling moved the verb into
                // this list — `bt_platform`'s own refusal for the executable
                // list included, because the rule belongs to the door.
                RowActivation::DefaultApp(path) => {
                    self.open_local_path(&path);
                }
                RowActivation::Nowhere => {}
            },
            // **Through `open_local_path`, which is this window's one door out
            // to the shell for a path the user pointed at** (user ruling
            // 2026-08-25). It is the same function the preview head's `↗`
            // presses, and that is the point: "open this with whatever the
            // machine has registered for it" is one verb, so the breadcrumb's
            // `Open ⌄` and a tree row's second door must not grow two
            // implementations of it — which is exactly why they are one row
            // wearing each face's preposition rather than two rows. The door's
            // own refusal for the executable list comes along —
            // `bt_platform::open_local_path` reads `PATHEXT` and says no, which
            // is §7.1.3's "the tree is a way of looking at files, and the thing
            // next to it that runs programs is the terminal" enforced at the
            // bridge rather than at each call site.
            profiles::FileMenuRow::OpenWith => {
                self.open_local_path(&path);
            }
            // Only a tree row has a fold, and only a tree row carries the host
            // and key a fold needs. A breadcrumb's menu has neither and does not
            // offer the row — `file_menu`'s `Document` arm — so the absence is
            // the honest answer rather than a guard against a caller.
            profiles::FileMenuRow::Fold => {
                if let Some(tree_row) = tree_row {
                    self.fold_files_row(tree_row.host, &tree_row.key)?;
                }
            }
            profiles::FileMenuRow::NewTerminal => self.new_terminal_in_folder(&path)?,
            // **The row opens a field in the tree rather than a dialog** (0.3),
            // which is `Rename`'s own answer one row down and for its own
            // reason: the name is typed where the name will be. Only a folder
            // row offers these — `file_menu`'s own lists — and only a tree row
            // carries the host and the key a box is placed by, so the absence is
            // the honest answer rather than a guard against a caller.
            profiles::FileMenuRow::NewFile | profiles::FileMenuRow::NewFolder => {
                if let Some(tree_row) = tree_row {
                    let folder = matches!(row, profiles::FileMenuRow::NewFolder);
                    self.open_files_row_new(tree_row.host, &tree_row.key, folder)?;
                }
            }
            // **Nothing is asked first, because the bin is the undo** — see
            // [`profiles::FileMenuRow::Delete`], which carries the argument. The
            // verb itself is `Runtime::delete_files_row`, and it goes through
            // `bt_platform::recycle` and never through `remove_file`.
            profiles::FileMenuRow::Delete => {
                if let Some(tree_row) = tree_row {
                    self.delete_files_row(tree_row.host, &tree_row.key)?;
                }
            }
            // **B5 — the row opens the editor rather than asking a dialog for a
            // name** (user ruling 2026-08-25). The name is typed where the name
            // is, which is the answer this window already gives on a tab, on a
            // preview head and on an address; a modal that asked for a string
            // would be the one surface in the product that renamed something
            // somewhere other than where it is written.
            //
            // Only a tree row has a row to put a box on, and only a tree row
            // carries the host and key that box is keyed by — `file_menu`'s
            // `Document` and `FoldedPath` arms do not offer this row at all, so
            // the absence is the honest answer rather than a guard.
            profiles::FileMenuRow::Rename => {
                if let Some(tree_row) = tree_row {
                    self.open_files_row_rename(tree_row.host, &tree_row.key)?;
                }
            }
            profiles::FileMenuRow::CopyPath => self.copy_path_to_clipboard(&path)?,
            profiles::FileMenuRow::InsertPath => self.insert_path_into_terminal(&path)?,
            // **The same audited door the three feet go through**
            // (`Runtime::reveal_in_explorer`), which already answers both halves
            // of what this row means: a file is selected inside its folder and a
            // folder is opened as itself, decided by `bt_platform::
            // reveal_arguments` and not by this call site knowing which kind of
            // row raised the menu.
            profiles::FileMenuRow::Reveal => {
                self.reveal_in_explorer(&path);
            }
            // Answered above, before the single-path gate, because a row of this
            // face is about its own folder and not about the menu's path. The
            // arm is written out rather than wildcarded so that a fifth face
            // reaching here is a compiler error and not a silence.
            profiles::FileMenuRow::Crumb(_) => {}
        }
        Ok(())
    }

    /// Which float the pointer is over and what part of it, if any.
    ///
    /// **Top to bottom**, and the first window to claim the point owns it: a
    /// float is opaque, so the pointer never reaches what is behind one. That is
    /// [`float::FloatHost::hit_order`]'s order, which is the reverse of the paint
    /// order and stated once, there.
    ///
    /// A *dismissed* float answers nothing — `pointer-events: none` on
    /// `.closing`, which is what stops a window that is on its way out from
    /// swallowing the click you aimed at what is behind it. `hit_order` filters
    /// those out, so a closing window is transparent to this by construction.
    pub(in crate::runtime) fn float_hit_at(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<(float::FloatId, float::FloatPart)> {
        let scale = self.window.renderer.scale_factor() as f32;
        // Measured once for the whole sweep: the caption is the same width in
        // every window, and asking the renderer inside the loop would borrow it
        // against the host we are walking.
        let dock_label = self.float_dock_label_width(scale);
        let now = Instant::now();
        // **The no-preview card's button, measured once for the sweep** (§7.39).
        // Its caption is the same in every window — `Open in default app`, or the
        // acknowledgement that briefly replaces it — so it is asked here for
        // `dock_label`'s reason exactly, and `preview_card_geometry` centres a
        // button of this width in whichever window is refused below.
        let open_label = self.preview_open_button_label(now);
        let open_button_px = self.window.renderer.measure_chrome_text(
            &mut self.app.gpu,
            open_label,
            seats::PREVIEW_CARD_BUTTON_FONT_LOGICAL_PX * scale,
        );
        let (x, y) = (position.x as f32, position.y as f32);
        // The identities first, then the windows one at a time: the head's tools
        // are a question about the *content* plane, which is `&self`, and asking
        // it inside a loop that is already holding the host would be two borrows
        // of one window.
        let ids: Vec<float::FloatId> = self.window.float.hit_order().map(|win| win.epoch).collect();
        for id in ids {
            let tools = self.float_head_tools(id);
            // The row under the head, as it was drawn: the widths were measured
            // where a font was, and a band resolved against anything else would
            // answer about a row nobody saw (§7.7 ⑩ 欠账).
            let rail = self.rail_geometry(PreviewSurface::Float(id), scale);
            let Some(win) = self.window.float.drawn().find(|win| win.epoch == id) else {
                continue;
            };
            let geometry = float::float_geometry(
                risen_frame(win.frame, self.float_fade_of(win, now, scale)),
                win.mode,
                scale,
                dock_label,
                tools,
            );
            // **Whichever list this window is showing.** A buffer's own *text* is
            // asked by the preview's pointer path, which resolves its surface for
            // itself; a tree, a Git page and a commit graph are all lists in this
            // rectangle, and which of them answers is the one question the paint
            // has already decided — each of the three records what it drew, or
            // records nothing.
            let page = self.window.float_git_pages_shown.get(&id).cloned();
            let tree = win.files().filter(|_| page.is_none()).map(|files| {
                let rows = files::tree_view(&files.files, &files.cache).rows.len();
                seats::files_tree_geometry(geometry.body, rows, files.cache.scroll_px, scale)
            });
            let git = page.as_ref().map(|page| {
                (
                    git_panel::git_panel_geometry(geometry.body, page, scale),
                    page,
                )
            });
            // The graph this window drew, if it drew one. Asked of the same map
            // the docked graphs are recorded in, so "the row you can press is
            // the row you can see" is one derivation on both hosts.
            let graph = self
                .window
                .git_graphs_shown
                .get(&PreviewSurface::Float(id))
                .cloned();
            // **The no-preview card's button, where the paint put it** (§7.39).
            // A refused document draws the pane's card into this body, and the
            // button is centred by the same `preview_card_geometry` — so the hit
            // lands on the rectangle that was drawn rather than on a second guess
            // at it. `None` for every window that is not refused, which is a body
            // with no button to take the press.
            //
            // **Whichever refusal this window is standing on** (owner's ruling
            // 2026-09-12): the document nothing here reads, or the picture this
            // window would not draw. One question, asked through the same
            // `float_refusal_words` the paint asked, so a button that was drawn
            // is a button that can be pressed.
            let card_button = self
                .float_refusal_words(PreviewSurface::Float(id), open_label)
                .filter(|words| words.verb.is_some())
                .and_then(|_| {
                    seats::preview_card_geometry(geometry.body, Some(open_button_px), 0, scale)
                        .button
                });
            if let Some(part) = float::float_hit(&geometry, x, y, rail.as_ref(), |x, y| {
                let (x, y) = (x + geometry.body[0], y + geometry.body[1]);
                if let Some(button) = card_button
                    && x >= button[0]
                    && x < button[2]
                    && y >= button[1]
                    && y < button[3]
                {
                    return Some(float::FloatPart::CardButton);
                }
                if let Some(graph) = graph.as_ref() {
                    // The order — toolbar, then the open block's parts, then the
                    // row — is `git_graph::graph_hit`'s and not this host's, so
                    // the window and the pane cannot part company about it.
                    return git_graph::graph_hit(geometry.body, graph, scale, x, y).map(|hit| {
                        match hit {
                            git_graph::GraphHit::Tool(tool) => float::FloatPart::GraphTool(tool),
                            git_graph::GraphHit::Detail { index, part } => {
                                float::FloatPart::GraphDetail { index, part }
                            }
                            git_graph::GraphHit::Row(index) => float::FloatPart::GraphRow(index),
                        }
                    });
                }
                if let Some((geometry, page)) = git.as_ref() {
                    let index = geometry.row_at(x, y)?;
                    let row = page.rows.get(index)?;
                    // **The verbs before the row they stand on**, which is the
                    // docked page's own order (`hit_git_panel`): first match
                    // wins, and a row asked first would swallow every button
                    // inside it.
                    let hovered = matches!(
                        self.window.float_hover,
                        Some((on, float::FloatPart::Row(at) | float::FloatPart::GitAct { index: at, .. }))
                            if on == id && at == index
                    );
                    let boxes = git_panel::act_boxes(row, geometry.row_rect(index), scale, hovered);
                    if let Some((act, _)) = boxes.into_iter().find(|(_, rect)| {
                        x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
                    }) {
                        return Some(float::FloatPart::GitAct { index, act });
                    }
                    return Some(float::FloatPart::Row(index));
                }
                tree.as_ref()?.row_at(x, y).map(float::FloatPart::Row)
            }) {
                return Some((id, part));
            }
        }
        None
    }

    /// Advance both of the float's clocks and its animation.
    pub(in crate::runtime) fn advance_float(&mut self, now: Instant) -> Result<()> {
        // **Everything down to the fade is state, and state is never paced**
        // (closure review O4, 2026-09-18). Behind the gate, a hover intent that
        // matured while a neighbouring pane was printing opened no window, a
        // grace that ran out closed none, a finished exit was never swept, and
        // the two questions at the foot — which the note on them says are asked
        // "on every turn of the loop" — were asked on no turn at all. See
        // [`Self::animation_frame_is_due`] for the three things an advancer does.
        let scale = self.window.renderer.scale_factor() as f32;
        let mut changed = false;
        // A *transient* peek whose trigger has gone — the tab closed, the split
        // collapsed, the pane it hung from stopped being a terminal — has nothing
        // left to hang from, and `PeekHost::retain`'s rule applies: a popup whose
        // subject died must not be left on screen. A **pinned** window is exempt
        // by ruling (§7.1.2): it was torn off, and the death of the header it came
        // from is not on its list of closers.
        if let Some((id, origin)) = self
            .window
            .float
            .peek()
            .and_then(|win| Some((win.epoch, win.origin?)))
            && self.trigger_rect(origin).is_none()
        {
            self.window.float.dismiss(id, now);
            changed = true;
        }
        if let Some(trigger) = self.window.float.take_due(now) {
            // The intent matured. The trigger is looked up **again, by identity**
            // (G86): the strip may have been rebuilt during those 180ms, and an
            // intent that had remembered a rectangle would be pointing at one
            // that no longer exists.
            if self.trigger_rect(trigger).is_some() {
                self.open_float(trigger, float::FloatMode::Peek)?;
            }
            changed = true;
        }
        if self.window.float.grace_expired(now)
            && let Some(id) = self.window.float.peek_id()
        {
            self.dismiss_float(id)?;
            changed = true;
        }
        if self.window.float.sweep(now, self.app.motion, scale) {
            self.forget_dead_float_gestures();
            // A finished exit is where a window stops being drawn, and therefore
            // where it stops being a preview surface.
            self.sweep_preview_panes();
            changed = true;
        }
        // `height: auto` while nobody has taken hold of it: the window grows as
        // its rows arrive and stops at the cap, which is what the mock-up's CSS
        // does and what a tree read on a worker cannot get by measuring once.
        if self.resize_floats_to_content() {
            changed = true;
        }
        // A living float asks the worker for whatever it has not yet been told.
        // Above the gate with the rest of the service: these are idempotent and
        // they are how a window that has just opened gets its rows at all.
        self.ask_float_directories();
        self.ask_git_for_floats();
        // **And now the one paced thing**: a float in the middle of its entrance
        // owes a frame; one standing still owes nothing, which is why the
        // deadline reports nothing then. A turn on which the clocks above
        // changed something has news of its own and goes through.
        if !changed && !self.animation_frame_is_due() {
            return Ok(());
        }
        let animating = self
            .window
            .float
            .drawn()
            .any(|win| win.fade(now, self.app.motion, scale).moving);
        if (changed || animating) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The directories the floats are showing but have never read.
    ///
    /// The docked columns' own walk, for the trees that are not columns: each
    /// view says what it wants, its cache is marked before the send so a second
    /// walk on the same frame does not ask twice, and **each window's own** epoch
    /// rides along, so a late answer is matched to the view that asked rather
    /// than to whichever float is in front when it lands.
    fn ask_float_directories(&mut self) {
        // Nothing open asks nothing, and it asks it without allocating (closure
        // review 2, 2026-09-18): this is on the per-turn road.
        if self.window.float.is_empty() {
            return;
        }
        let window = self.window_id();
        let mut asks = Vec::new();
        for win in self.window.float.live_windows_mut() {
            let epoch = win.epoch;
            let Some(files) = win.files_mut() else {
                continue;
            };
            let root = files.files.root.clone();
            for key in files::tree_view(&files.files, &files.cache).wanted {
                files.cache.mark_pending(&key);
                asks.push(files::DirRequest {
                    window,
                    host: files::FilesHost::Float(epoch),
                    path: files::full_path(&root, &key),
                    key,
                });
            }
        }
        for ask in asks {
            if !self.app.files_worker.request(ask) {
                self.disable_files_worker();
                break;
            }
        }
    }

    /// Turn one commit's file list over, in a floating Git page (R15).
    pub(in crate::runtime) fn expand_float_commit(
        &mut self,
        id: float::FloatId,
        hash: &str,
    ) -> Result<()> {
        let tab = self.window.tabs[self.window.active_tab].id;
        let mut question = None;
        if let Some(files) = self
            .window
            .float
            .live_mut(id)
            .and_then(float::FloatWin::files_mut)
        {
            let opened = git_panel::toggled_expansion(files.files.git_expanded.as_deref(), hash);
            files.files.git_expanded.clone_from(&opened);
            // Shutting one asks nothing: the answer stays in the cache, so
            // pressing the same commit again draws its files without a second
            // subprocess.
            if let Some(hash) = opened {
                question = files.git.begin_commit_files(&hash);
            }
        }
        if let Some(question) = question
            && !self.app.git_worker.request(git::GitRequest {
                window: self.window_id(),
                host: git::GitHost::Float { id, tab },
                question,
            })
        {
            self.disable_git_worker();
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The next fifty commits, for a floating page (R16).
    pub(in crate::runtime) fn load_more_float_commits(&mut self, id: float::FloatId) -> Result<()> {
        let tab = self.window.tabs[self.window.active_tab].id;
        let Some(question) = self
            .window
            .float
            .live_mut(id)
            .and_then(float::FloatWin::files_mut)
            .and_then(|files| files.git.more_commits())
        else {
            return Ok(());
        };
        if !self.app.git_worker.request(git::GitRequest {
            window: self.window_id(),
            host: git::GitHost::Float { id, tab },
            question,
        }) {
            self.disable_git_worker();
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Show the float's root in the OS file manager (B24, the float's foot).
    ///
    /// The confirmation is the mock-up's: the folder becomes a tick, the path
    /// becomes a sentence, and both go back 1300ms later. It says the thing a
    /// launched process cannot — Explorer may open behind this window, or focus a
    /// window that was already open somewhere else, and without a word here the
    /// click reads as having done nothing.
    pub(in crate::runtime) fn reveal_float_root(&mut self, id: float::FloatId) -> Result<()> {
        let Some(root) = self
            .window
            .float
            .live(id)
            .and_then(float::FloatWin::files)
            .map(|files| files.files.root.clone())
        else {
            return Ok(());
        };
        if root.is_empty() {
            return Ok(());
        }
        // Through the same audited door the other two feet use, rather than
        // spawning `explorer.exe` itself as this once did: one verb printed on
        // three strips is one verb, and a bridge that only one of the three
        // goes around is a bridge with a hole in it.
        let handed = self.reveal_in_explorer(Path::new(&root));
        self.when_handed_over(
            handed,
            crate::handoff_lane::OnAccepted::Revealed(RevealedFoot::Float(id)),
        );
        Ok(())
    }

    /// Show the **file** a preview float is holding in the OS file manager.
    ///
    /// [`Self::reveal_float_root`]'s opposite number, and the same division the
    /// two feet already make one surface over: a foot prints what its window is
    /// showing, so a tree's points at a folder and a buffer's points at the file
    /// itself (the docked preview foot's own ruling, 2026-08-13 — "the directory
    /// it is in" is not an answer to "where is this file").
    pub(in crate::runtime) fn reveal_float_file(&mut self, id: float::FloatId) -> Result<()> {
        let surface = PreviewSurface::Float(id);
        // A file's door, for [`Self::reveal_preview_file`]'s reason — including
        // the working-tree file a git document is a reading of.
        let Some(path) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| revealable_preview_file(&buffer.source))
            .or_else(|| {
                self.preview_pane(surface)
                    .and_then(|pane| pane.image.as_ref())
                    .map(|image| image.path.clone())
            })
        else {
            return Ok(());
        };
        let handed = self.reveal_in_explorer(&path);
        self.when_handed_over(
            handed,
            crate::handoff_lane::OnAccepted::Revealed(RevealedFoot::Float(id)),
        );
        Ok(())
    }

    /// Put the float back inside a viewport that has just changed shape.
    ///
    /// The two regimes of `M2-tiny-window-priority.md` §3.2, side by side:
    /// TRANSIENT dissolves on any geometry change, PINNED is translated home and
    /// keeps its size. The asymmetry is the promise: nobody was told the peek
    /// would stay, and the pinned window is there because somebody asked for it,
    /// so taking it away would be a silent undo of that choice.
    /// Both halves run now, where the old shape ran one *or* the other: a peek
    /// and pinned windows can be on screen together, so dissolving the transient
    /// one is no longer a reason to leave the permanent ones un-clamped.
    pub(in crate::runtime) fn reclamp_float(&mut self) {
        if self.window.float.wipe_peek().is_some() {
            // Every gesture aimed at the window that just went dies with it —
            // including a header press still waiting to become one, which if it
            // outlived its peek would keep whatever opened next.
            self.forget_dead_float_gestures();
            self.sweep_preview_panes();
        }
        let scale = self.window.renderer.scale_factor() as f32;
        let viewport = self.float_viewport();
        // Every live window, whoever is inside it: the preview float inherits
        // P67's translate-home for free because the chassis is shared, and a
        // second re-clamp of its own would be a second answer to one question.
        for win in self.window.float.live_windows_mut() {
            win.frame = float::clamp_pinned(win.frame, viewport, scale);
        }
    }

    /// Drop every gesture and hover aimed at a window that is no longer live.
    ///
    /// One place for it rather than three lines after each of the four ways a
    /// window can go (`×`, Esc, Dock, dissolve): with more than one float on
    /// screen the old blanket `float_drag = None` would cancel a *drag of another
    /// window* every time any window closed, which is the bug a list invites and
    /// the reason this is asked by identity.
    pub(in crate::runtime) fn forget_dead_float_gestures(&mut self) {
        if let Some(drag) = self.window.float_drag
            && self.window.float.live(drag.win).is_none()
        {
            self.window.float_drag = None;
        }
        if let Some(press) = self.window.float_head_press
            && self.window.float.peek_id() != Some(press.win)
        {
            self.window.float_head_press = None;
        }
        if let Some((id, _)) = self.window.float_hover
            && self.window.float.live(id).is_none()
        {
            self.window.float_hover = None;
        }
        // **And the glance one of its rows raised** (user ruling 2026-09-07,
        // §7.58). It is a gesture on this window in exactly the sense the three
        // above are: it was placed against the window's frame, it names a file the
        // window's tree listed, and it is drawn over the top of it. A card left
        // standing where its window used to be is a card about a place the reader
        // can no longer see — and it would have no anchor to be re-placed against
        // on the next frame that drew it.
        if let Some(RowHost::Float(id)) = self.window.file_peek.as_ref().map(|peek| peek.host)
            && self.window.float.live(id).is_none()
        {
            self.hide_file_peek();
        }
    }

    /// The float's next appointment.
    pub(in crate::runtime) fn float_deadline(&self, now: Instant) -> Option<Instant> {
        let scale = self.window.renderer.scale_factor() as f32;
        let owner = self.window.float.deadline(
            now,
            self.app.motion,
            scale,
            self.window.frame_clock.interval(),
        )?;
        if self.window.float.is_animating(now, self.app.motion, scale) {
            self.next_animation_deadline().map(|frame| frame.min(owner))
        } else {
            Some(owner)
        }
    }

    /// Which of the head's optional controls **this window** is asking for.
    ///
    /// A tree asks for none of them and gets the head it always had. A buffer
    /// asks for the dot's slot unconditionally and for whichever of the two verbs
    /// its content class earns (P57) — the slot is reserved occupied or not,
    /// because reserving it is the whole of what makes "the dot appearing shoves
    /// nothing" true (P16), and a head laid out without room for a button it is
    /// then drawn with is a button standing on the name (D4).
    pub(in crate::runtime) fn float_head_tools(&self, id: float::FloatId) -> float::FloatHeadTools {
        let is_preview = self
            .window
            .float
            .drawn()
            .find(|win| win.epoch == id)
            .is_some_and(|win| win.preview().is_some());
        if !is_preview {
            return float::FloatHeadTools::default();
        }
        let surface = PreviewSurface::Float(id);
        let buffer = self.preview_buffer_on(surface);
        // **The row under the head** (§7.7 ⑩ 欠账, 2026-08-25), asked with the
        // very predicate a docked seat is asked with: what a window wears is
        // decided by what is *in* it, and a torn-off page that stopped showing
        // its address the moment it left the tree was this rule being asked of
        // the wrong thing.
        let rail = self.preview_rail_kind(surface);
        float::FloatHeadTools {
            // **And no band for the disk notice since 2026-09-12** (owner's
            // ruling). The chassis reserves nothing for it: a window's news is a
            // pill floating over the bottom edge of its body, laid out by
            // `notice_layers` off `seats::news_pill_box` and costing this
            // geometry no rows at all.
            dirty: true,
            save: self.preview_is_editable(surface),
            // **And the flip goes down to that row when there is one**, which is
            // `preview_head_tools`'s own clause travelling across unchanged
            // (user ruling 2026-08-24): a markdown with a breadcrumb wears its
            // `</>` down there, and one without keeps it upstairs. Two flips on
            // one window would be the head and the row each claiming the verb.
            flip: buffer.is_some_and(|buffer| buffer.ftype == preview::PreviewFtype::Markdown)
                && rail.is_none(),
            rail: rail.is_some(),
        }
    }

    /// How far through its entrance or exit this window is.
    ///
    /// **A preview float has neither** (P49 ③). The mock-up gives `flyIn`/
    /// `flyOut` to `#files-flyout` alone: a tree flies out of the header it was
    /// summoned from, and a buffer is *carried* out of a pane by hand — a
    /// gesture that has already shown where the window came from, and one an
    /// entrance played on top of would only blur. So it is simply there, at full
    /// strength, from the frame it opens to the frame it is wiped.
    pub(in crate::runtime) fn float_fade_of(
        &self,
        win: &float::FloatWin,
        now: Instant,
        scale: f32,
    ) -> float::FloatFade {
        if win.preview().is_some() {
            return float::FloatFade {
                opacity: 1.0,
                rise: 0.0,
                moving: false,
            };
        }
        win.fade(now, self.app.motion, scale)
    }

    /// Just the body of a float, asked **without a font**.
    ///
    /// [`Self::float_geometry_of`] has to borrow the renderer, because the head's
    /// `DOCK` button is sized to a measured caption and only the face knows how
    /// wide that is. The body is not: it is the frame less its border, its head
    /// and its foot, and none of those depend on what the head has room for. So
    /// every `&self` question about where a float's *content* is — which is all
    /// of the preview's pointer arithmetic — is answered here rather than by
    /// taking a mutable borrow of the whole window to describe controls it is
    /// never going to look at.
    /// **Which float, if any, is holding this page** (§7.14a, user report
    /// 2026-08-25).
    ///
    /// The inverse of [`float::FloatPreview::page`] and the only way back: a
    /// popped-out page's seat is closed out of the tree, so every question the
    /// per-frame placement asks about a docked page — where is your pane, which
    /// tab is it on, is it in front — has no answer for this one, and this is
    /// what supplies a different one.
    ///
    /// **Asked of `drawn` and not of `live`**, so that a page keeps its rectangle
    /// through the float's own closing fade: a window on its way out is still on
    /// the glass, and a page taken off it a frame early would leave a hole in a
    /// window still drawing its border. When the fade ends the float leaves this
    /// list, `sync_web_page` finds no rectangle and `advance_web_page` retires
    /// the browser — which is the same sentence a closed pane already gets.
    pub(crate) fn float_holding_the_page(&self, leaf: LeafId) -> Option<float::FloatId> {
        self.window
            .float
            .drawn()
            .find_map(|win| (win.preview()?.page == Some(leaf)).then_some(win.epoch))
    }

    /// **Which overlay layer this float's face is** — the index a page it is
    /// carrying spells its hole against (§7.14c).
    ///
    /// Read off [`WindowRuntime::float_hole_level`], which the chrome pass wrote
    /// when it last assembled the stack. `None` is the honest answer for a float
    /// this frame's stack has no layer for — a window whose fade has ended
    /// between the two passes — and a page with no level is a page with no hole,
    /// which is the window simply being there.
    pub(in crate::runtime) fn float_hole_level(&self, id: float::FloatId) -> Option<usize> {
        self.window.float_hole_level.get(&id).copied()
    }

    /// **The rectangle a float's content fills** — its body, inset off the
    /// rounded corners and the grip (§7.39).
    ///
    /// The one box every tenant of the chassis derives from, and it is why they
    /// agree about where the window's floor is: a floated page's engine bounds,
    /// the transparency hole punched for it and — through
    /// [`webhost::WebSeat::shown_at`] — the region [`Self::web_page_at`] forwards
    /// the pointer into; a document's text and the scrollbar down its right edge;
    /// the pointer arithmetic every gesture on it does. Inset in this one place,
    /// all of them follow: the grip a page used to cover is uncovered, hittable
    /// and told to resize, the scrollbar a document drew over the curve stops
    /// above it, and the two corners either squared off are the float's own
    /// rounded face again. A resize re-solves this rectangle from the float's new
    /// frame every frame, so the engine's bounds and the scrollbar both follow
    /// the drag with nothing else asked ([`float::FloatGeometry::content_body`]).
    pub(in crate::runtime) fn float_body_rect(
        &self,
        id: float::FloatId,
        scale: f32,
    ) -> Option<[f32; 4]> {
        Some(self.float_chassis(id, scale)?.body)
    }

    /// **The rectangle a float's content is inset into for the two tenants that
    /// cannot be drawn over the window's floor** — its engine and its scrollbar
    /// ([`float::FloatGeometry::content_body`], §7.39, user report 2026-08-28).
    ///
    /// The body itself reaches the window's own floor since the ruling of
    /// 2026-09-12 — that is what "no reserved foot" means on this chassis, and
    /// it is why a document, a picture and a recording all now run to the bottom
    /// edge with the resize grip drawn over them. Two things still may not:
    ///
    /// * **a page**, which is a composition-hosted WebView2 — one opaque
    ///   rectangle that paints its own square corners, that no click can pass
    ///   through, and that would bury the grip;
    /// * **a document's scrollbar**, whose rule rides the body's right edge down
    ///   to its floor and was photographed drawn out over the rounded corner.
    ///
    /// Neither is a *band* and neither costs the document a pixel of height:
    /// this is one inset, on two tenants that are drawn inside the same body
    /// everything else fills.
    pub(in crate::runtime) fn float_inset_body_rect(
        &self,
        id: float::FloatId,
        scale: f32,
    ) -> Option<[f32; 4]> {
        Some(self.float_chassis(id, scale)?.content_body(scale))
    }

    /// **This float's chassis, asked without a font** — the derivation
    /// [`Self::float_body_rect`] and [`Self::float_inset_body_rect`] both read.
    ///
    /// One derivation and not two, for `seats::pane_notice_strip`'s own stated
    /// reason one host over: the band and the body it was taken out of are the
    /// two halves of one subtraction, and a strip arrived at by adding a bar
    /// back onto a body would disagree with it wherever a clamp bit — a
    /// disagreement that shows as a seam and puts every press one row off.
    ///
    /// `dock_label_px` is zero here on purpose: the caption's width moves the
    /// `DOCK` button inside the head and moves nothing below it, so every
    /// `&self` question about where this window's *content* is can be answered
    /// without borrowing the face to measure a word.
    fn float_chassis(&self, id: float::FloatId, scale: f32) -> Option<float::FloatGeometry> {
        let win = self.window.float.drawn().find(|win| win.epoch == id)?;
        let fade = self.float_fade_of(win, Instant::now(), scale);
        Some(float::float_geometry(
            risen_frame(win.frame, fade),
            win.mode,
            scale,
            0.0,
            self.float_head_tools(id),
        ))
    }

    /// The floats' own level of the overlay stack — **one layer per window,
    /// bottom to top**.
    ///
    /// The stack slot was always a `Vec`, so 浮窗多开 needed nothing of it: the
    /// z-order inside the family is [`float::FloatHost::drawn`]'s order, stated
    /// there and merely obeyed here.
    ///
    /// `below` is how many layers the stack already holds under this group — see
    /// [`OverlayStack::below_the_floats`] — and it is handed in rather than
    /// looked up because it is the *stack's* arithmetic and not a window's: what
    /// this function knows is which window is where inside its own family, and
    /// adding the two here is what makes [`WindowRuntime::float_hole_level`] an
    /// index into the one flattened list.
    pub(in crate::runtime) fn float_layer(&mut self, now: Instant, below: usize) -> marks::Band {
        let ids: Vec<float::FloatId> = self.window.float.drawn().map(|win| win.epoch).collect();
        // Rebuilt from nothing on every pass, exactly as `git_pages_shown` is:
        // it is a record of what this frame drew, and a stale entry in it is a
        // press landing on a row that is not there.
        self.window.float_git_pages_shown.clear();
        // And the ledger a floated page's hole is spelled against, on the same
        // terms and for the same reason (§7.14c).
        self.window.float_hole_level.clear();
        // And the recording's, which is a *different* height in the same stack
        // — see the note beside the push.
        self.window.float_video_level.clear();
        // **The floating graphs, on the same terms** — and only the floating
        // half of that map, because the seats' entries were written by the
        // chrome pass and are not this pass's to throw away.
        self.window
            .git_graphs_shown
            .retain(|surface, _| !matches!(surface, PreviewSurface::Float(_)));
        let scale = self.window.renderer.scale_factor() as f32;
        let mut layers = marks::Band::default();
        for id in ids {
            let Some(window) = self.float_window(id, now) else {
                continue;
            };
            // **The window's fade, as one surface** (ticket 46): every layer
            // this window puts down below — its face, the slot for a recording,
            // its scroll bars, the ▶, the recording's bar and the disk notice —
            // drawn whole and put back once, rather than each wearing the fade in
            // its own opacity. Its rise stays in the frame it was built at
            // (`risen_frame`), which every reader of where this window is — the
            // press router, the page's hole, the recording's box — reads too.
            let opacity = self
                .window
                .float
                .drawn()
                .find(|win| win.epoch == id)
                .map_or(1.0, |win| self.float_fade_of(win, now, scale).opacity);
            let mut surface = marks::Band::default();
            // **The window's own scroll bar, on the layer directly above it**
            // (user ruling, 2026-08-14). Not further up, because a window in
            // front has to cover this one whole — bar included; and not on the
            // window's own layer, because a layer paints its quads *before* the
            // document it carries (see [`scroll_bar_layer`]).
            //
            // It fades with the window it belongs to, for the reason the tenant
            // does: the window's `opacity` is the CSS declaration on the element
            // the whole surface is, and a bar that stayed at full strength
            // through an entrance would be a scrollbar arriving before its
            // window.
            //
            // **Where this window lands in the one flattened list** — written
            // before the push, because that is the slot the push is about to
            // take. A page this float is carrying has its hole punched directly
            // above this layer and under the bar beside it (§7.14c).
            self.window
                .float_hole_level
                .insert(id, below + layers.len() + surface.len());
            surface.layers.push(window);
            // **And a slot of its own for a recording, directly over that
            // face** (route B slice ②; §7.44 ③, found on the machine
            // 2026-08-28).
            //
            // A page's hole and a recording's picture are *not* the same
            // height, and writing them at one index is what put a float's video
            // underneath the window that was supposed to be showing it. The
            // renderer draws `VideoStage::Overlay(n)` between layer `n`'s ground
            // and layer `n`'s fills — which is right for a hole, because a hole
            // is punched under the marks that legitimately stand over a page,
            // and wrong for a picture, because a float's body well is one of
            // those fills. Photographed: the window drew its face, the fills
            // covered the recording, and the picture only appeared for the
            // moment the chassis was on its way out.
            //
            // An empty layer, and that is the whole of the mechanism: a stage
            // index has to name a layer the z-order loop actually reaches, and
            // what this one is for is being reached. It costs no buffer, no
            // draw call and no quad — see `marks::OverlayLayer::default`.
            self.window
                .float_video_level
                .insert(id, below + layers.len() + surface.len());
            surface.layers.push(marks::OverlayLayer::default());
            // **And the recording's control bar, on the same terms** (route B
            // slice ②; §7.44 ②). Above this window's own layer for the scroll
            // bar's reason exactly — a layer paints its quads before the
            // picture it carries, and the picture here is a video drawn over
            // this layer's ground — and below the next window for its other
            // reason: a float in front covers this one whole.
            let bar = self.video_bar_layer(PreviewSurface::Float(id));
            // And the ▶ over a recording this window is *not* playing — the same
            // disc a docked pane wears, because a float showing a video and
            // offering no way to start it would be the surface that knows least
            // about what it is holding.
            let play = self.video_play_mark_layer(PreviewSurface::Float(id));
            // **And the disk notice's band** (B1, 2026-09-01), on the scroll
            // bar's terms exactly: above this window's own layer, because a
            // layer paints its quads before whatever it carries, and below the
            // next window, because a float in front covers this one whole.
            let notice = self.float_notice_layer(id);
            surface
                .layers
                .extend(self.preview_float_bar_layers(id).into_iter().chain(play));
            // The recording's bar keeps its own fade inside the window's — the
            // two multiply, where writing the window's over the bar's (as this
            // did, the audit's F6) gave the bar's letters the window's fade and
            // threw its own away.
            if let Some(bar) = bar {
                surface.append(bar);
            }
            surface.layers.extend(notice);
            layers.append(surface.faded(opacity, [0.0, 0.0]));
        }
        layers
    }

    /// **The refusal this window is standing on**, whichever lane it came down
    /// (owner's ruling 2026-09-12).
    ///
    /// The pane's card builder asks these two questions in this order and for
    /// this reason: a picture is not a buffer (`preview_chrome_on`), so the two
    /// cannot both answer, and asking the picture first is what lets a window
    /// showing a refused `.png` wear a card at all.
    pub(in crate::runtime) fn float_refusal_words(
        &self,
        surface: PreviewSurface,
        open_label: &str,
    ) -> Option<CardWords> {
        if let Some(refusal) = self
            .preview_picture(surface)
            .and_then(PreviewImageState::refusal)
        {
            return Some(refused_preview_card(
                refusal.notice.clone(),
                refusal.offers_the_default_app,
                open_label,
            ));
        }
        let refusal = self.preview_buffer_on(surface)?.refusal()?;
        Some(refused_preview_card(
            refusal.notice().to_owned(),
            refusal.offers_the_default_app(),
            open_label,
        ))
    }

    /// The rectangle a float is allowed to occupy, in physical pixels: **the
    /// whole client area except the title bar** (user ruling 2026-08-12).
    ///
    /// The rail and the pane heads are ordinary ground — a float may cover them
    /// outright, and the overlay stack already draws it over them, so nothing
    /// beneath prints through. What the ruling keeps is one strip: the caption
    /// row, whose three buttons and drag band must be reachable at every moment,
    /// because a window you cannot move, minimise or close is a different order
    /// of problem from a sidebar you cannot see. So the guarantee is not "chrome
    /// is never covered" but "the window's own handle is never covered", and it
    /// is structural for the same reason the old rule was: the strip is removed
    /// from the space rather than defended by a runtime check.
    ///
    /// This overturns `M2-tiny-window-priority.md` §3.3, which clamped into the
    /// solved content box precisely so that no chrome could ever be covered; see
    /// the dated annotation there.
    pub(in crate::runtime) fn float_viewport(&self) -> [f32; 4] {
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        float_viewport_rect(width, height, self.window.renderer.scale_factor() as f32)
    }

    /// Summon a float at a trigger. **Always** — a trigger has one answer.
    ///
    /// # 同根去重: instituted and repealed the same day (2026-08-12)
    ///
    /// The morning's 浮窗多开 ruling put a dedup here: when the root a trigger
    /// named was already in a pinned window, this raised that window instead of
    /// opening a second, by analogy with a browser focusing the tab that already
    /// holds the page. The afternoon's ruling **removes it**, and the four
    /// reasons are worth keeping where the temptation to re-add it lives:
    ///
    /// * **The precedent is the other way.** Explorer and Finder both open a
    ///   second window on a folder you already have open — comparing two places
    ///   in one tree is what a file manager is *for*.
    /// * **Two windows on one root are not duplicates.** Each carries its own
    ///   throwaway `{root, open, sel}` (G81) and its own scroll, so they can show
    ///   entirely different parts of the same tree.
    /// * **It made the hover unpredictable.** A peek that comes up over most
    ///   triggers and silently fails over the ones whose root you have already
    ///   opened is a gesture nobody can model — and it fails in exactly the shape
    ///   of the bug 浮窗多开 was built to fix. It is the same disease that
    ///   retired 「re-click 关闭」 in the morning ruling, caught one step later.
    /// * **The costs are lopsided.** An unwanted window costs one click on `×`;
    ///   a refused one costs the user the thing they asked for and says nothing
    ///   about why.
    ///
    /// Raising is not gone, it is merely no longer the *trigger's* job: pressing
    /// a window brings it forward ([`float::FloatHost::raise`], from
    /// [`Self::press_float`]), which is where a stacking gesture belongs.
    pub(in crate::runtime) fn open_float(
        &mut self,
        trigger: float::FloatTrigger,
        mode: float::FloatMode,
    ) -> Result<()> {
        let Some(anchor) = self.trigger_rect(trigger) else {
            return Ok(());
        };
        let root = self.trigger_root(trigger);
        // A window summoned by a trigger is a **peek at a folder**, so it opens
        // on the tree: there is no page for it to have been standing on. The
        // Git page reaches a float only by being carried there — a column that
        // was on it when it popped out (`undock_files_column`).
        let state = seats::FilesLeafState {
            root,
            ..seats::FilesLeafState::default()
        };
        self.place_float(
            mode,
            Some(trigger),
            float::FloatFiles {
                files: state,
                width: bt_layout::FILES_W,
                ..float::FloatFiles::default()
            },
            Some(anchor),
        )
    }

    /// Every origin a live window already stands at — what the cascade steps
    /// clear of.
    ///
    /// The caller's own list rather than the host's, because
    /// [`float::cascade_origin`] must not assume the newcomer is going into this
    /// host: a pop-out computes its frame before its window exists.
    pub(in crate::runtime) fn taken_float_origins(&self) -> Vec<[f32; 2]> {
        self.window
            .float
            .live_windows()
            .map(|win| [win.frame[0], win.frame[1]])
            .collect()
    }

    /// Put a float on screen, sized to its content and placed against `anchor`.
    ///
    /// The one door every way of opening one goes through — hover, click, a
    /// second trigger, and a column popping out — because the four differ only in
    /// what they hand it, and four doors would be four places to forget the epoch
    /// or the clamp.
    pub(in crate::runtime) fn place_float(
        &mut self,
        mode: float::FloatMode,
        origin: float::FloatOrigin,
        tenant: float::FloatFiles,
        anchor: Option<[f32; 4]>,
    ) -> Result<()> {
        let float::FloatFiles {
            files: ref state,
            ref cache,
            ..
        } = tenant;
        // E61: the opener closes what it must not coexist with. A float is a
        // window rather than a menu, so it does not join the menus' mutually
        // exclusive chain — but a menu hanging off a control that is about to be
        // covered is a menu pointing at nothing.
        if self.window.profile_menu.close() {
            self.window.chevrons.menu_gone(Popup::Profile);
        }
        self.window.root_menu.close();
        let scale = self.window.renderer.scale_factor() as f32;
        let viewport = self.float_viewport();
        let git_page = self.git_panel_on() && state.view == seats::FilesView::Git;
        let size = float::float_opening_size(
            files_float_content_height(state, cache, git_page, viewport, scale),
            viewport,
            scale,
            float::FloatSizing::files(),
        );
        let placed = match anchor {
            Some(anchor) => float::float_placement(anchor, size, viewport, scale),
            // No anchor is the pop-out case with a head that has already gone;
            // the middle of the content area is the honest place for a window
            // that has no particular place to be.
            None => {
                let left = ((viewport[0] + viewport[2]) / 2.0 - size[0] / 2.0).round();
                let top = ((viewport[1] + viewport[3]) / 2.0 - size[1] / 2.0).round();
                [left, top, left + size[0], top + size[1]]
            }
        };
        // The cascade, between the placement and the clamp: a window landing on
        // one already standing at that origin steps clear of it, so the thing
        // that obviously happened is visible. The ruling is about *floats*, not
        // about previews, which is why it is here in the shared door.
        let placed = float::cascade_origin(placed, &self.taken_float_origins(), viewport, scale);
        let frame = float::clamp_pinned(placed, viewport, scale);
        self.window.float.open(
            mode,
            origin,
            float::FloatTenant::Files(Box::new(tenant)),
            frame,
            anchor,
            Instant::now(),
        );
        // Only the gestures whose window this opening took away — a new peek
        // replaces the old one, and a new pinned window dismisses a live peek —
        // and not a carry of some *other* float that is still perfectly in hand.
        self.forget_dead_float_gestures();
        // And the content plane of the window this opening replaced, on the same
        // terms: an opening is a closing for whoever was in the peek slot.
        self.sweep_preview_panes();
        // A window that opens under a stationary pointer inherits none of the
        // last one's shape.
        self.apply_pointer_cursor();
        // The rail's business may have just begun (G102). Re-asked here rather
        // than waiting for the next pointer move, because the answer changed
        // without the pointer moving — which is the whole reason the rail's zone
        // is a function and not a `:hover`.
        self.drive_rail_zone(self.window.pointer_position);
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Begin one float's exit (§7.1.2's remaining closers, and only those).
    pub(in crate::runtime) fn dismiss_float(&mut self, id: float::FloatId) -> Result<bool> {
        if !self.window.float.dismiss(id, Instant::now()) {
            return Ok(false);
        }
        self.forget_dead_float_gestures();
        // **The view goes; the buffer stays** (P60). A preview float's pane is
        // retired the moment the window stops existing, but the buffer it was on
        // belongs to the *tab*'s pool (§7.1.3) and is untouched: closing a window
        // you tore off is not a way to lose an edit, and the tab-close gate is
        // still the thing that asks about one.
        self.sweep_preview_panes();
        // The window can go while the pointer stands still — Esc closes it from
        // the keyboard — so the hand it was shaping has to be given back here
        // rather than at the next move that may never come.
        self.apply_pointer_cursor();
        // ...and it may have just ended.
        self.drive_rail_zone(self.window.pointer_position);
        self.refresh_chrome();
        self.present_chrome_change()?;
        Ok(true)
    }

    /// Esc's float rung: close the **frontmost** window and only that one.
    ///
    /// One press, one window, which is what a stack of windows makes Esc mean
    /// everywhere else: it takes the thing in front of you, and pressing it again
    /// takes the next. Closing them all would make one keystroke undo an
    /// arbitrary amount of work the user asked for.
    pub(in crate::runtime) fn dismiss_top_float(&mut self) -> Result<bool> {
        let Some(id) = self.window.float.top().map(|win| win.epoch) else {
            return Ok(false);
        };
        self.dismiss_float(id)
    }

    /// Wheel over a floating tree's body — the same `overflow-y: auto`, in a
    /// window instead of a column.
    ///
    /// The float's scroll lives in its own `DirCache` (`FloatFiles::cache`),
    /// which is where the painter and the hit test already read it from
    /// (`row_geometry(RowHost::Float)`); this is the one writer a wheel gets,
    /// and it clamps at both ends the way the column's does (R2 乙案) — the
    /// body it clamps against is the very body the painter lays the rows into.
    pub(in crate::runtime) fn scroll_float_tree(
        &mut self,
        id: float::FloatId,
        delta: MouseScrollDelta,
    ) -> Result<()> {
        let Some(geometry) = self.row_geometry(RowHost::Float(id)) else {
            return Ok(());
        };
        let body = geometry.viewport;
        let travel = self.vertical_wheel_travel(delta, body[3] - body[1]);
        let scale = self.window.renderer.scale_factor() as f32;
        let Some(files) = self
            .window
            .float
            .live_mut(id)
            .and_then(float::FloatWin::files_mut)
        else {
            return Ok(());
        };
        let rows = files::tree_view(&files.files, &files.cache).rows.len();
        let scrolled = seats::clamp_files_scroll(body, rows, files.cache.scroll_px - travel, scale);
        if scrolled == files.cache.scroll_px {
            return Ok(());
        }
        files.cache.scroll_px = scrolled;
        // The rows under a still pointer changed, so the hover row did too.
        if let Some(position) = self.window.pointer_position {
            self.window.float_hover = self.float_hit_at(position);
        }
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **One row of the application menu that is this product's and has no row
    /// in the shortcut table** (M3-2; the clipboard pair is
    /// T-MAC-EDIT-CLIPBOARD, `docs/DESIGN.md` §13.26 ⑨).
    ///
    /// Help leaves through the same door every other address in this window
    /// leaves through, and it is a **row** — pressed with a target of Folio's
    /// own, like every verb row.
    ///
    /// The other two are not rows at all. They reach this window through the
    /// application delegate's `copy:` / `paste:`, which is the **last** rung of
    /// AppKit's responder chain — so everything that is a real text responder
    /// has already had the press and kept it: the page in a web pane answers
    /// with WebKit's own copy, a field in a sheet with the field's. What is left
    /// is a surface AppKit has never heard of, which on this platform is every
    /// pane Folio draws itself, and [`menubar::clipboard_seat`] is the one place
    /// that decides which.
    ///
    /// **Each clipboard arm is the door the keystroke already uses**, never a
    /// second one: a second copy path would be a second answer to what a
    /// selection is worth on the pasteboard, and a second paste path would be a
    /// second place for the sanitising, the bracketing and the multi-line policy
    /// to be decided.
    pub(crate) fn run_an_application_menu_verb(
        &mut self,
        action: bt_platform::menu::AppMenuAction,
    ) -> Result<()> {
        let copying = match action {
            bt_platform::menu::AppMenuAction::Help => {
                // The address is a constant of this build and always navigable;
                // the hand-off's `false` is for a typed address and is not a
                // verdict on this one (this is what the landing arm did before
                // the verbs moved here).
                self.hand_url_to_the_browser(update::RELEASES_PAGE)?;
                return Ok(());
            }
            bt_platform::menu::AppMenuAction::CopySelection => true,
            bt_platform::menu::AppMenuAction::PasteIntoFocus => false,
        };
        match menubar::clipboard_seat(self.clipboard_focus()) {
            // A surface of this window's is holding the keyboard and answers
            // its own keys; see `ClipboardSeat::Nobody` for why doing nothing
            // is the answer rather than the absence of one.
            menubar::ClipboardSeat::Nobody => Ok(()),
            menubar::ClipboardSeat::PreviewDocument => {
                if copying {
                    self.copy_preview_selection();
                    Ok(())
                } else {
                    self.paste_into_preview()
                }
            }
            menubar::ClipboardSeat::Terminal => {
                if copying {
                    self.copy_selection()
                } else {
                    self.paste_from_clipboard()
                }
            }
        }
    }

    /// The same picture for a page a float is carrying, on that window's layer.
    pub(in crate::runtime) fn float_page_keepsake_icon(
        &self,
        float: float::FloatId,
    ) -> Option<bt_render::ChromeIcon> {
        let keepsake = self
            .window
            .page_keepsakes
            .iter()
            .find(|keepsake| keepsake.float == Some(float))?;
        let picture = self.window.web_thumbs.frame(keepsake.leaf)?;
        Some(page_keepsake_icon(picture, keepsake.rect))
    }

    /// **Bring the float carrying this page to the front and give it the
    /// window's preview focus** (§7.14c).
    ///
    /// The first two statements of [`Self::press_float`] exactly, and they are
    /// the same two statements for the same reason: a press inside a floating
    /// window raises it (user ruling 2026-08-12, rule ⑤) and a raise is a frame
    /// debt. What this cannot be is a call *to* `press_float` — that function
    /// then goes on to act on the part that was pressed, and the part pressed
    /// here is not the window's at all.
    pub(in crate::runtime) fn raise_the_float_holding(&mut self, leaf: LeafId) -> Result<()> {
        let Some(id) = self.float_holding_the_page(leaf) else {
            return Ok(());
        };
        if self.window.float.raise(id) && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }
}
