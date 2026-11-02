//! **A press inside one of the window's one-line fields puts the caret where it is, and a drag
//! selects** (0.4.9 plan rows 179 and 180), on headless windows: a tab is renamed in the sidebar
//! and on the strip, a page's address field is opened, and the find bar, a commit graph's search,
//! the settings dialog's startup-command field, the palette and a git menu's prompt are each
//! opened by the road the window opens them by; the window is drawn, and the pointer is pressed at
//! seams measured the way each painter measures them — the field's own layout says where its run
//! starts, and the text is shaped in the face that field sets it in.

use super::*;
use crate::headless::{Headless, HeadlessFolio};
use crate::test_support::cross_tab;
use crate::text_field::{TextField, TextMove};
use crate::{TabRename, seats};
use winit::keyboard::ModifiersState;

/// The name the tests rename a tab to: Latin letters around two ideographs, so that a seam a
/// press finds is a byte offset that is not a character count.
const NAME: &str = "ab\u{4e2d}\u{6587}cd";

/// Byte offsets of [`NAME`]'s seams: before `中`, after it, after `文`.
const BEFORE_ZHONG: usize = 2;
const AFTER_ZHONG: usize = 5;
const AFTER_WEN: usize = 8;

/// **A window renaming its one tab to `name`, the caret at the draft's start, drawn**, and
/// where the draft stands on the glass: the box's left edge, the vertical middle of its line,
/// and the draft's boundaries as the tab list's face shapes them.
struct Renaming {
    folio: HeadlessFolio,
    left: f64,
    middle: f64,
    advances: bt_render::ChromeTextAdvances,
}

impl Renaming {
    fn open(tag: &str, layout: seats::TabLayoutMode, name: &str) -> Self {
        let mut folio = HeadlessFolio::with_tabs(tag, vec![cross_tab(1, &["A"])], 0);
        {
            let mut runtime = folio.runtime();
            let rail = seats::RailState {
                layout,
                ..runtime.window.rail
            };
            runtime
                .set_rail_state(rail)
                .expect("the tab list takes its axis");
            let tab = runtime.window.tabs[0].id;
            let mut editor = TabRename::open(tab, Some(name));
            editor.field.step(TextMove::Home, false);
            runtime.window.rename = Some(editor);
            // As `open_rename` does once the editor is in place.
            runtime.refresh_chrome();
        }
        let font = match layout {
            seats::TabLayoutMode::Vertical => bt_render::RAIL_TAB_FONT_LOGICAL_PX,
            seats::TabLayoutMode::Horizontal => bt_render::WINDOW_TAB_FONT_LOGICAL_PX,
        };
        Self::drawn(folio, name, font)
    }

    /// **A window whose page pane has its address field open on `draft`**, the caret at the
    /// draft's start, drawn: the field is the preview rail's own cell, measured by
    /// `dress_preview_address_editor` in the rail's face — another painter, another target
    /// (`ChromeTarget::PreviewAddress`) and another box than a tab's. A headless window hosts no
    /// web engine, so the page is a seat standing shown with none behind it.
    fn address(tag: &str, draft: &str) -> Self {
        let (tab, page) = crate::test_support::tab_with_a_preview(1, Vec::new());
        let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
        {
            let mut runtime = folio.runtime();
            let leaf = crate::LeafId {
                tab: runtime.id,
                seat: page,
            };
            runtime.window.web.insert(
                leaf,
                crate::webhost::WebSeat::shown_without_an_engine(crate::webhost::WebBounds {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 200,
                }),
            );
            let mut editor = TabRename::open_address(leaf, draft);
            editor.field.step(TextMove::Home, false);
            runtime.window.rename = Some(editor);
            runtime.refresh_chrome();
        }
        Self::drawn(folio, draft, seats::PREVIEW_RAIL_FONT_LOGICAL_PX)
    }

    /// The window drawn, and where its open editor's draft stands: the left edge of the caret
    /// line the painter left (the caret is at the draft's start), its vertical middle, and the
    /// draft shaped at `font_logical_px` in the window's scale.
    fn drawn(mut folio: HeadlessFolio, name: &str, font_logical_px: f32) -> Self {
        folio.draw();
        let (left, top, bottom, advances) = {
            let runtime = folio.runtime();
            let [left, top, _, bottom] = runtime
                .window
                .rename_caret_line
                .expect("the open editor is drawn with its caret");
            let font = font_logical_px * runtime.window.renderer.scale_factor() as f32;
            let (gpu, renderer) = (&mut runtime.app.gpu, &mut runtime.window.renderer);
            (
                left,
                top,
                bottom,
                renderer.chrome_text_advances(gpu, name, font),
            )
        };
        assert!(
            advances.width() > 0.0,
            "the headless renderer shapes the draft"
        );
        Self {
            folio,
            left: f64::from(left),
            middle: f64::from(top + bottom) / 2.0,
            advances,
        }
    }

    /// The point `nudge` pixels right of the seam at byte `at`, on the draft's line.
    fn at(&self, at: usize, nudge: f64) -> PhysicalPosition<f64> {
        PhysicalPosition::new(
            self.left + f64::from(self.advances.upto(at)) + nudge,
            self.middle,
        )
    }

    /// The point a fraction of the way across the cluster that runs from byte `from` to `to`.
    fn across(&self, from: usize, to: usize, fraction: f64) -> PhysicalPosition<f64> {
        let width = f64::from(self.advances.between(from, to));
        self.at(from, width * fraction)
    }

    /// The editor's caret and selection, as the draft holds them.
    fn caret(&mut self) -> (usize, std::ops::Range<usize>) {
        let runtime = self.folio.runtime();
        let editor = runtime
            .window
            .rename
            .as_ref()
            .expect("the editor is still open");
        (editor.field.caret(), editor.field.selection())
    }

    /// A press and its release at one point.
    fn click(&mut self, at: PhysicalPosition<f64>) {
        self.folio.press(at);
        self.folio.release(at);
    }
}

/// Which owner holds the window's capture, if one does.
fn held(runtime: &Runtime<'_, Headless>) -> Option<&'static str> {
    runtime
        .app
        .pointer_capture
        .as_ref()
        .map(|capture| capture.owner.name())
}

/// One press at each of three places in a name: right of the seam after `中`, the left half of
/// `中`, its right half — and the caret lands on the nearest seam every time, with no selection.
fn presses_land_on_their_seams(tag: &str, layout: seats::TabLayoutMode) {
    let mut renaming = Renaming::open(tag, layout, NAME);
    renaming.click(renaming.at(AFTER_ZHONG, 1.0));
    assert_eq!(
        renaming.caret(),
        (AFTER_ZHONG, AFTER_ZHONG..AFTER_ZHONG),
        "a press just right of the seam after the first ideograph puts the caret there"
    );
    renaming.click(renaming.across(BEFORE_ZHONG, AFTER_ZHONG, 0.25));
    assert_eq!(
        renaming.caret().0,
        BEFORE_ZHONG,
        "the left half of an ideograph is the seam in front of it"
    );
    renaming.click(renaming.across(BEFORE_ZHONG, AFTER_ZHONG, 0.75));
    assert_eq!(
        renaming.caret().0,
        AFTER_ZHONG,
        "and its right half the seam after it"
    );
}

/// RED (0.4.9 plan row 179; owner's report 2026-11-01) — **a press inside a sidebar tab's name
/// editor puts the caret at the pressed seam**. On main the press was consumed whole by the
/// rename guard and the caret stayed wherever the keyboard had left it.
///
/// MUTATION: return at the top of `Runtime::press_field` — every caret stays at 0.
#[test]
fn a_press_inside_a_sidebar_tabs_name_puts_the_caret_at_the_pressed_seam() {
    presses_land_on_their_seams("rename-caret-rail", seats::TabLayoutMode::Vertical);
}

/// RED (row 179) — **and on the strip**, whose box is another geometry's answer: the press
/// reads the run the strip drew, not the rail's.
#[test]
fn a_press_inside_a_strip_tabs_name_puts_the_caret_at_the_pressed_seam() {
    presses_land_on_their_seams("rename-caret-strip", seats::TabLayoutMode::Horizontal);
}

/// RED (row 179, ruling R1; review F1) — **a press inside a page's address field puts the
/// caret at the pressed seam**: the field is the same editor as a tab's name, drawn by the
/// preview rail's own painter into its own box, and the press reads the run that painter left.
///
/// MUTATION: drop the `drawn_fields.name` record in `Runtime::dress_preview_address_editor` —
/// the press finds no run and the caret stays at 0.
#[test]
fn a_press_inside_a_pages_address_field_puts_the_caret_at_the_pressed_seam() {
    let mut renaming = Renaming::address("rename-caret-address", NAME);
    renaming.click(renaming.at(AFTER_ZHONG, 1.0));
    assert_eq!(
        renaming.caret(),
        (AFTER_ZHONG, AFTER_ZHONG..AFTER_ZHONG),
        "a press just right of the seam after the first ideograph puts the caret there"
    );
    renaming.click(renaming.across(BEFORE_ZHONG, AFTER_ZHONG, 0.25));
    assert_eq!(
        renaming.caret().0,
        BEFORE_ZHONG,
        "and the left half of an ideograph is the seam in front of it"
    );
}

/// RED (row 179, ruling R6) — **a press in the tab body past the name's last letter puts the
/// caret at the name's end, and the editor stays open**: the editor's extent is the whole tab
/// body, so the press is still the editor's and never a blur.
#[test]
fn a_press_past_the_names_end_puts_the_caret_at_its_end() {
    let mut renaming = Renaming::open(
        "rename-caret-past-end",
        seats::TabLayoutMode::Vertical,
        NAME,
    );
    renaming.click(renaming.at(NAME.len(), 6.0));
    assert_eq!(renaming.caret(), (NAME.len(), NAME.len()..NAME.len()));
}

/// RED (row 179, ruling R3) — **a drag inside the name selects what it crossed**: the press puts
/// the anchor, the move carries the caret, and the drag is held as C25 until the button comes up.
///
/// MUTATION: answer `Ok(false)` from `Runtime::drag_field_selection` — the selection is empty.
#[test]
fn a_drag_inside_the_name_selects_what_it_crossed() {
    let mut renaming = Renaming::open("rename-caret-drag", seats::TabLayoutMode::Vertical, NAME);
    renaming.folio.press(renaming.at(1, 1.0));
    assert_eq!(held(&renaming.folio.runtime()), Some("FieldSelection"));
    renaming.folio.move_to(renaming.at(AFTER_ZHONG, 1.0));
    renaming.folio.release(renaming.at(AFTER_WEN, 1.0));
    assert_eq!(
        renaming.caret(),
        (AFTER_WEN, 1..AFTER_WEN),
        "the selection runs from the press to where the button came up"
    );
    assert_eq!(
        held(&renaming.folio.runtime()),
        None,
        "and the release let go of it"
    );
}

/// RED (row 179, ruling R3) — **a `Shift`+press carries the selection's far end to the pressed
/// seam** and leaves its anchor where the caret stood.
///
/// MUTATION: pass `false` for `extend` in `Runtime::press_field` — the selection is empty.
#[test]
fn a_shift_press_carries_the_selection_to_the_pressed_seam() {
    let mut renaming = Renaming::open("rename-caret-shift", seats::TabLayoutMode::Vertical, NAME);
    renaming.click(renaming.at(1, 1.0));
    renaming.folio.runtime().window.modifiers = ModifiersState::SHIFT;
    renaming.click(renaming.at(AFTER_ZHONG, 1.0));
    assert_eq!(renaming.caret(), (AFTER_ZHONG, 1..AFTER_ZHONG));
}

/// RED (row 179) — **a press in a name scrolled along its box counts from the first letter
/// drawn**: with the caret at the end of a name longer than the box, the drawn window starts
/// part-way in, and a press at the box's left edge is the caret at that start — not at the
/// hidden head, and not wherever the keyboard left it.
///
/// MUTATION: record `scroll_px: 0.0` in `TabRename::fit`'s `DrawnRun` — the caret goes to the
/// head.
#[test]
fn a_press_in_a_scrolled_name_counts_from_the_first_letter_drawn() {
    let long = "abcdefghij".repeat(12);
    let mut renaming = Renaming::open(
        "rename-caret-scrolled",
        seats::TabLayoutMode::Vertical,
        &long,
    );
    let start = {
        let mut runtime = renaming.folio.runtime();
        let editor = runtime.window.rename.as_mut().expect("the editor is open");
        editor.field.step(TextMove::End, false);
        runtime.refresh_chrome();
        runtime
            .window
            .rename
            .as_ref()
            .expect("the editor is open")
            .first_visible
    };
    renaming.folio.draw();
    assert!(start > 0, "the name is longer than its box");
    let edge = PhysicalPosition::new(renaming.left + 1.0, renaming.middle);
    renaming.click(edge);
    assert_eq!(renaming.caret(), (start, start..start));
}

/// PIN (row 179, ruling R5) — **a press while a composition is up moves nothing and holds
/// nothing**: the IME is composing at the caret, and the press is still the editor's.
#[test]
fn a_press_while_composing_moves_nothing() {
    let mut renaming = Renaming::open(
        "rename-caret-composing",
        seats::TabLayoutMode::Vertical,
        NAME,
    );
    renaming
        .folio
        .runtime()
        .window
        .rename
        .as_mut()
        .expect("the editor is open")
        .field
        .set_preedit("zh");
    renaming.folio.draw();
    renaming.folio.press(renaming.at(AFTER_WEN, 1.0));
    assert_eq!(held(&renaming.folio.runtime()), None);
    renaming.folio.release(renaming.at(AFTER_WEN, 1.0));
    assert_eq!(renaming.caret(), (0, 0..0));
}

// ── Row 180: every other one-line field of the window ─────────────────────────────────────────

/// **The one-line fields row 180 gives the pointer to**, besides the name editor above: each is
/// opened in a fresh headless window by the road the window opens it by, and drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Chrome {
    FindBar,
    GraphSearch,
    Settings,
    Palette,
    GitPrompt,
}

/// Every [`Chrome`] field, for the tests that hold for all of them.
const EVERY_FIELD: [Chrome; 5] = [
    Chrome::FindBar,
    Chrome::GraphSearch,
    Chrome::Settings,
    Chrome::Palette,
    Chrome::GitPrompt,
];

/// The settings field the tests press in: the summoned terminal's startup command, a field on a
/// page of its own and present on every host.
const QUAKE_COMMAND: settings::SettingsTarget =
    settings::SettingsTarget::Field(settings::SettingsRow::QuakeCommand);

/// **A window with one [`Chrome`] field open on `text`, the caret at its start, drawn**, and
/// where the text stands on the glass: the left edge of the run the field lays its text out in,
/// the vertical middle of the field, and the text's boundaries as the field's own face shapes
/// them — every number read off the field's own layout, never off the run a press reads.
struct Open {
    folio: HeadlessFolio,
    field: Chrome,
    left: f64,
    middle: f64,
    advances: bt_render::ChromeTextAdvances,
}

impl Open {
    fn new(tag: &str, field: Chrome, text: &str) -> Self {
        let root = bt_testpath::temp_path("field-caret-graph 图");
        let (tab, preview_seat) = match field {
            Chrome::GraphSearch => {
                let graph = crate::preview::PreviewBuffer::new(
                    crate::preview::PreviewSource::GitGraph { root: root.clone() },
                    "图 graph".to_owned(),
                );
                let (tab, seat) = crate::test_support::tab_with_a_preview(1, vec![graph]);
                (tab, Some(seat))
            }
            _ => (
                crate::test_support::tab_holding(crate::test_support::leaf_saying("SHELL 外壳")),
                None,
            ),
        };
        let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
        folio.draw();
        {
            let mut runtime = folio.runtime();
            let seat = runtime.seats.identity();
            match field {
                Chrome::FindBar => runtime.open_search(seat).expect("the find bar opens"),
                Chrome::Palette => runtime.open_command_palette().expect("the palette opens"),
                Chrome::Settings => runtime
                    .open_settings_on_row(settings::SettingsRow::QuakeCommand)
                    .expect("the dialog opens on the field"),
                Chrome::GitPrompt => {
                    runtime.window.git_menu = Some(crate::GitMenuState {
                        point: [10.0, 10.0],
                        origin: crate::GitOrigin::Graph(
                            crate::test_support::host_spelling(r"C:\w").into(),
                        ),
                        root: crate::test_support::host_spelling(r"C:\w").into(),
                        target: crate::profiles::GitMenuTarget::LocalBranch {
                            name: "主干 main".to_owned(),
                            current: true,
                        },
                        graph_row: None,
                        hover: None,
                        prompt: Some(crate::GitPromptState {
                            kind: crate::profiles::GitPromptKind::CreateBranch,
                            caption: String::new(),
                            field: TextField::default(),
                            asked_empty: false,
                        }),
                    });
                }
                Chrome::GraphSearch => {
                    // A graph whose history has been read, so its toolbar — and the search
                    // field in it — is drawn: the toolbar stands only over a history.
                    let mut state = crate::git_graph::GraphState::new(root.clone());
                    state.cache.accept(crate::git::GitAnswer::Log {
                        root: root.clone(),
                        skip: 0,
                        outcome: Ok(crate::git::GitLog {
                            skip: 0,
                            commits: Vec::new(),
                            has_more: false,
                        }),
                    });
                    state.sync();
                    let preview_seat = preview_seat.expect("the graph's seat");
                    let tab = runtime.window.tabs[0].id;
                    runtime.window.tabs[0]
                        .git_graphs
                        .insert(root.clone(), state);
                    runtime.seats.set_focus(preview_seat);
                    runtime.window.tabs[0].git_graph_view.insert(
                        crate::test_support::seat_of(tab, preview_seat),
                        crate::GraphView {
                            root: root.clone(),
                            search_focused: true,
                            ..crate::GraphView::default()
                        },
                    );
                }
            }
        }
        {
            let typed = field_of(&mut folio, field);
            typed.select_all();
            typed.insert(text);
            typed.step(TextMove::Home, false);
        }
        folio.runtime().refresh_chrome();
        folio.draw();
        let (left, middle, font) = where_the_text_stands(&mut folio, field);
        let advances = {
            let runtime = folio.runtime();
            let (gpu, renderer) = (&mut runtime.app.gpu, &mut runtime.window.renderer);
            renderer.chrome_text_advances(gpu, text, font)
        };
        assert!(
            advances.width() > 0.0,
            "the headless renderer shapes the text"
        );
        Self {
            folio,
            field,
            left: f64::from(left),
            middle: f64::from(middle),
            advances,
        }
    }

    /// The point `nudge` pixels right of the seam at byte `at`, on the field's line.
    fn at(&self, at: usize, nudge: f64) -> PhysicalPosition<f64> {
        PhysicalPosition::new(
            self.left + f64::from(self.advances.upto(at)) + nudge,
            self.middle,
        )
    }

    /// The point a fraction of the way across the cluster that runs from byte `from` to `to`.
    fn across(&self, from: usize, to: usize, fraction: f64) -> PhysicalPosition<f64> {
        let width = f64::from(self.advances.between(from, to));
        self.at(from, width * fraction)
    }

    /// The field's caret and selection.
    fn caret(&mut self) -> (usize, std::ops::Range<usize>) {
        let field = field_of(&mut self.folio, self.field);
        (field.caret(), field.selection())
    }

    /// A press and its release at one point.
    fn click(&mut self, at: PhysicalPosition<f64>) {
        self.folio.press(at);
        self.folio.release(at);
    }
}

/// The text field `field` is, in the window `folio` holds it open in.
fn field_of(folio: &mut HeadlessFolio, field: Chrome) -> &mut TextField {
    let window = &mut folio.window;
    match field {
        Chrome::FindBar => window.search.field_mut(),
        Chrome::GraphSearch => {
            &mut window.tabs[0]
                .git_graph_view
                .values_mut()
                .next()
                .expect("the graph's view")
                .search
        }
        Chrome::Settings => window
            .settings
            .text_field_mut(QUAKE_COMMAND)
            .expect("the startup command's field"),
        Chrome::Palette => window
            .palette
            .as_mut()
            .expect("the palette is open")
            .field_mut(),
        Chrome::GitPrompt => {
            &mut window
                .git_menu
                .as_mut()
                .and_then(|menu| menu.prompt.as_mut())
                .expect("the prompt is up")
                .field
        }
    }
}

/// **Where `field`'s text stands on the glass, by the field's own layout**: the left edge of the
/// run its text is laid out in, the vertical middle of its line, and the size its face is set at
/// in this window's scale.
fn where_the_text_stands(folio: &mut HeadlessFolio, field: Chrome) -> (f32, f32, f32) {
    let mut runtime = folio.runtime();
    let scale = runtime.window.renderer.scale_factor() as f32;
    match field {
        Chrome::FindBar => {
            let capsule = runtime
                .window
                .search_layout
                .clone()
                .expect("the find bar is drawn");
            let [left, _] = capsule.text_run(scale);
            (
                left,
                (capsule.field[1] + capsule.field[3]) / 2.0,
                crate::search::FIELD_FONT_LOGICAL_PX * scale,
            )
        }
        Chrome::GraphSearch => {
            let surface = *runtime.window.tabs[0]
                .git_graph_view
                .keys()
                .next()
                .expect("the graph's view");
            let rects = runtime
                .graph_toolbar_rects(surface)
                .expect("the graph's toolbar is drawn");
            let toolbar = runtime.window.git_graphs_shown[&surface]
                .toolbar
                .clone()
                .expect("a toolbar");
            let inside = crate::git_graph::graph_search_field(rects, &toolbar, scale)
                .expect("the toolbar draws its search field");
            (
                inside.text[0],
                (inside.text[1] + inside.text[3]) / 2.0,
                crate::git_graph::GRAPH_TOOL_FONT_LOGICAL_PX * scale,
            )
        }
        Chrome::Settings => {
            let layout = runtime.settings_layout().expect("the dialog is drawn");
            let (rect, mono) = layout
                .text_field_box(QUAKE_COMMAND)
                .expect("the field is on the page");
            let ([left, _], font) = settings::field_text_run(rect, mono, layout.scale());
            (left, (rect[1] + rect[3]) / 2.0, font)
        }
        Chrome::Palette => {
            let layout = runtime.palette_layout().expect("the palette is drawn");
            let [left, _] = layout.text_run();
            let line = layout.caret_line();
            (left, (line[1] + line[3]) / 2.0, layout.field_font_px())
        }
        Chrome::GitPrompt => {
            let rects = runtime
                .git_menu_layout()
                .and_then(|layout| layout.prompt_rects())
                .expect("the prompt is drawn");
            let text = rects.text_run(scale);
            (
                text[0],
                (text[1] + text[3]) / 2.0,
                crate::profiles::GIT_PROMPT_FONT_LOGICAL_PX * scale,
            )
        }
    }
}

/// One press at each of three places in [`NAME`] in `field`: right of the seam after `中`, the
/// left half of `中`, its right half — and the caret lands on the nearest seam every time, with no
/// selection; a press past the text's end is its end.
fn presses_land_on_their_seams_in(field: Chrome) {
    let mut open = Open::new(&format!("field-caret-press-{field:?}"), field, NAME);
    open.click(open.at(AFTER_ZHONG, 1.0));
    assert_eq!(
        open.caret(),
        (AFTER_ZHONG, AFTER_ZHONG..AFTER_ZHONG),
        "{field:?}: a press just right of the seam after the first ideograph puts the caret there"
    );
    open.click(open.across(BEFORE_ZHONG, AFTER_ZHONG, 0.25));
    assert_eq!(
        open.caret().0,
        BEFORE_ZHONG,
        "{field:?}: the left half of an ideograph is the seam in front of it"
    );
    open.click(open.across(BEFORE_ZHONG, AFTER_ZHONG, 0.75));
    assert_eq!(
        open.caret().0,
        AFTER_ZHONG,
        "{field:?}: and its right half the seam after it"
    );
    open.click(open.at(NAME.len(), 4.0));
    assert_eq!(
        open.caret(),
        (NAME.len(), NAME.len()..NAME.len()),
        "{field:?}: a press past the text is its end"
    );
}

/// RED (0.4.9 plan row 180; owner's report 2026-11-01) — **a press in the find bar puts the caret
/// at the pressed seam**. On main the press handed the keyboard back to the capsule and left the
/// caret wherever the keyboard had put it.
///
/// MUTATION (`plant-180-press-places-nothing.py`): return `Ok(())` at the top of
/// `Runtime::press_field` — every caret stays at 0, in every field below.
#[test]
fn a_press_in_the_find_bar_puts_the_caret_at_the_pressed_seam() {
    presses_land_on_their_seams_in(Chrome::FindBar);
}

/// RED (row 180) — **and in a commit graph's search**, a field drawn in the graph's own toolbar.
#[test]
fn a_press_in_the_graph_search_puts_the_caret_at_the_pressed_seam() {
    presses_land_on_their_seams_in(Chrome::GraphSearch);
}

/// RED (row 180, ruling 3) — **and in a field of the settings dialog**, whose run is recorded
/// for every field the page draws.
#[test]
fn a_press_in_a_settings_field_puts_the_caret_at_the_pressed_seam() {
    presses_land_on_their_seams_in(Chrome::Settings);
}

/// RED (row 180) — **and in the palette's box**.
#[test]
fn a_press_in_the_palette_box_puts_the_caret_at_the_pressed_seam() {
    presses_land_on_their_seams_in(Chrome::Palette);
}

/// RED (row 180, ruling 4) — **and in a git menu's branch prompt**, whose run the menu's own
/// layer records.
#[test]
fn a_press_in_the_git_prompt_puts_the_caret_at_the_pressed_seam() {
    presses_land_on_their_seams_in(Chrome::GitPrompt);
}

/// The startup command's field, open on a line far longer than its box, the caret at its end and
/// drawn: the box shows the line's tail.
fn a_long_settings_line(tag: &str) -> (Open, String) {
    let long = format!("{NAME}{}", "abcdefghij".repeat(16));
    let mut open = Open::new(tag, Chrome::Settings, &long);
    field_of(&mut open.folio, Chrome::Settings).step(TextMove::End, false);
    open.folio.runtime().refresh_chrome();
    open.folio.draw();
    (open, long)
}

/// How far the startup command's field is pushed left inside its box.
fn quake_scroll(open: &HeadlessFolio) -> f32 {
    open.window.settings.field_scroll(QUAKE_COMMAND)
}

/// RED (row 180, ruling 2) — **a press in a scrolled settings field counts from the first letter
/// drawn, and the letters hold still under it**: with the caret at the end of a line longer than
/// the box, the box shows the tail; a press halfway across it puts the caret on the letter under
/// the pointer, and the frame after it draws every letter where it was.
///
/// MUTATION (`plant-180-settings-scroll-not-sticky.py`): settle the scroll from `0` — the frame
/// after the press scrolls the line back, and the caret is no longer under the pointer.
#[test]
fn a_press_in_a_scrolled_settings_field_counts_from_the_first_letter_drawn() {
    let (mut open, long) = a_long_settings_line("field-caret-settings-scrolled");
    let scroll = quake_scroll(&open.folio);
    assert!(scroll > 0.0, "the line is longer than its box");
    let width = {
        let mut runtime = open.folio.runtime();
        let layout = runtime.settings_layout().expect("the dialog is drawn");
        let (rect, mono) = layout
            .text_field_box(QUAKE_COMMAND)
            .expect("the field is on the page");
        let ([left, right], _) = settings::field_text_run(rect, mono, layout.scale());
        right - left
    };
    let halfway = scroll + width / 2.0;
    let under = crate::text_field::caret_at_x(&long, 0, halfway, |at| open.advances.upto(at));
    let pressed = PhysicalPosition::new(open.left + f64::from(width / 2.0), open.middle);
    open.click(pressed);
    assert_eq!(
        open.caret(),
        (under, under..under),
        "the caret is the seam under the pointer, counted from the first letter drawn"
    );
    open.folio.draw();
    assert_eq!(
        quake_scroll(&open.folio),
        scroll,
        "and the frame after the press draws every letter where it was"
    );
}

/// RED (row 180, ruling 7) — **a press in a settings field without the focus moves the focus and
/// the caret**: the field was drawn from its start while the focus was away, and the press counts
/// from there — and the text it pressed stays where it was drawn.
#[test]
fn a_press_in_a_settings_field_without_the_focus_moves_the_focus_and_the_caret() {
    let (mut open, _) = a_long_settings_line("field-caret-settings-unfocused");
    assert!(quake_scroll(&open.folio) > 0.0, "the line is longer than its box");
    open.folio
        .window
        .settings
        .press(settings::SettingsTarget::Nav(
            settings::SettingsRow::QuakeCommand.category(),
        ));
    open.folio.runtime().refresh_chrome();
    open.folio.draw();
    open.click(open.at(AFTER_ZHONG, 1.0));
    assert_eq!(open.folio.window.settings.focus(), Some(QUAKE_COMMAND));
    assert_eq!(
        open.caret(),
        (AFTER_ZHONG, AFTER_ZHONG..AFTER_ZHONG),
        "the caret is the seam pressed, counted from the field's start"
    );
    open.folio.draw();
    assert_eq!(
        quake_scroll(&open.folio),
        0.0,
        "and the field the focus came back to is drawn from its start"
    );
}

/// RED (row 180) — **a drag in any of the fields selects what it crossed**, held as C25 until the
/// button comes up; and **a `Shift`+press carries the selection's far end** to the pressed seam.
///
/// MUTATION (`plant-180-drag-ignored.py`): answer `Ok(false)` from
/// `Runtime::drag_field_selection` — the selection is empty. (`plant-180-shift-ignored.py`):
/// pass `false` for `extend` in `Runtime::press_field` — the `Shift` half fails.
#[test]
fn a_drag_in_a_chrome_field_selects_what_it_crossed() {
    for field in EVERY_FIELD {
        let mut open = Open::new(&format!("field-caret-drag-{field:?}"), field, NAME);
        open.folio.press(open.at(1, 1.0));
        assert_eq!(
            held(&open.folio.runtime()),
            Some("FieldSelection"),
            "{field:?}: the press holds the drag"
        );
        open.folio.move_to(open.at(AFTER_ZHONG, 1.0));
        open.folio.release(open.at(AFTER_WEN, 1.0));
        assert_eq!(
            open.caret(),
            (AFTER_WEN, 1..AFTER_WEN),
            "{field:?}: the selection runs from the press to where the button came up"
        );
        assert_eq!(
            held(&open.folio.runtime()),
            None,
            "{field:?}: and the release let go of it"
        );

        open.click(open.at(1, 1.0));
        open.folio.runtime().window.modifiers = ModifiersState::SHIFT;
        open.click(open.at(AFTER_ZHONG, 1.0));
        open.folio.runtime().window.modifiers = ModifiersState::empty();
        assert_eq!(
            open.caret(),
            (AFTER_ZHONG, 1..AFTER_ZHONG),
            "{field:?}: a Shift+press carries the selection's far end"
        );
    }
}

/// PIN (row 180, after row 179 R5) — **a press while a composition is up moves nothing and holds
/// nothing**, in every field: the IME is composing at the caret, and the press is still the
/// field's.
#[test]
fn a_press_in_a_composing_chrome_field_moves_nothing() {
    for field in EVERY_FIELD {
        let mut open = Open::new(&format!("field-caret-composing-{field:?}"), field, NAME);
        field_of(&mut open.folio, field).set_preedit("zh");
        open.folio.runtime().refresh_chrome();
        open.folio.draw();
        open.folio.press(open.at(AFTER_WEN, 1.0));
        assert_eq!(held(&open.folio.runtime()), None, "{field:?}");
        open.folio.release(open.at(AFTER_WEN, 1.0));
        assert_eq!(open.caret(), (0, 0..0), "{field:?}");
    }
}

/// RED (row 180, ruling 7) — **a press in the find bar collapses the select-all it reopened
/// with** (B76): the press is a caret put, as a click into any text box is.
#[test]
fn a_press_in_a_reopened_find_bar_collapses_its_select_all() {
    let mut open = Open::new("field-caret-find-select-all", Chrome::FindBar, NAME);
    field_of(&mut open.folio, Chrome::FindBar).select_all();
    open.folio.runtime().refresh_chrome();
    open.folio.draw();
    open.click(open.at(AFTER_ZHONG, 1.0));
    assert_eq!(open.caret(), (AFTER_ZHONG, AFTER_ZHONG..AFTER_ZHONG));
}

/// **The selection wash a field draws**: the span of the quad at the wash's alpha in `layers`.
fn wash_in(layers: &[crate::marks::OverlayLayer]) -> Option<[f32; 2]> {
    layers
        .iter()
        .flat_map(|layer| &layer.quads)
        .find(|quad| quad.alpha == crate::text_field::SELECTION_WASH_ALPHA)
        .map(|quad| [quad.rect[0], quad.rect[2]])
}

/// The wash `field` draws its selection in this frame, as the field's own layer is built.
fn wash_of(open: &mut Open) -> Option<[f32; 2]> {
    let mut runtime = open.folio.runtime();
    match open.field {
        Chrome::FindBar => wash_in(&runtime.search_layers()),
        Chrome::Palette => wash_in(&runtime.palette_layer().menu),
        Chrome::GitPrompt => wash_in(&runtime.git_menu_layer().menu),
        Chrome::GraphSearch => {
            let surface = *runtime.window.tabs[0]
                .git_graph_view
                .keys()
                .next()
                .expect("the graph's view");
            runtime.window.git_graphs_shown[&surface]
                .toolbar
                .as_ref()
                .and_then(|toolbar| toolbar.search_selection)
        }
        Chrome::Settings => unreachable!("the settings dialog always drew its selection"),
    }
}

/// RED (row 180, ruling 1) — **a selection in the find bar, the graph's search, the palette and
/// the git prompt is drawn**, in the settings dialog's wash, between the selection's two seams on
/// the glass. On main only the settings dialog and the name editor drew one; a selection made by
/// the keyboard in these four was there and invisible.
///
/// MUTATION (`plant-180-selection-ink-off.py`): answer `None` from `text_field::selection_span`,
/// the span every one of the four is washed in — no wash in any of them.
#[test]
fn a_selection_in_a_chrome_field_is_drawn() {
    for field in [
        Chrome::FindBar,
        Chrome::GraphSearch,
        Chrome::Palette,
        Chrome::GitPrompt,
    ] {
        let mut open = Open::new(&format!("field-caret-wash-{field:?}"), field, NAME);
        assert_eq!(wash_of(&mut open), None, "{field:?}: a bare caret draws no wash");
        let text = field_of(&mut open.folio, field);
        text.place(BEFORE_ZHONG, false);
        text.place(AFTER_WEN, true);
        open.folio.runtime().refresh_chrome();
        open.folio.draw();
        let [from, to] = wash_of(&mut open).unwrap_or_else(|| panic!("{field:?}: a wash"));
        let near = |drawn: f32, seam: usize| (f64::from(drawn) - open.at(seam, 0.0).x).abs() < 0.5;
        assert!(
            near(from, BEFORE_ZHONG) && near(to, AFTER_WEN),
            "{field:?}: the wash runs from seam to seam: {from}..{to}"
        );
    }
}

/// RED (row 179's review F5, decided in row 180) — **a drag past the left edge of a scrolled
/// name reveals its hidden head**: the press inside the box puts the anchor, a drag carried far
/// left of the box takes the caret to the name's start, and the box scrolls back to show it — as
/// a drag carried past the right edge reveals the tail.
///
/// MUTATION (`plant-180-f5-drag-clamped.py`): clamp `DrawnRun::drag_to` to the run's left edge as
/// `press_at` is — the caret stops at the first letter drawn and the head stays hidden.
#[test]
fn a_drag_past_the_left_edge_of_a_scrolled_name_reveals_its_head() {
    let long = "abcdefghij".repeat(12);
    let mut renaming = Renaming::open(
        "rename-caret-drag-left",
        seats::TabLayoutMode::Vertical,
        &long,
    );
    let start = {
        let mut runtime = renaming.folio.runtime();
        let editor = runtime.window.rename.as_mut().expect("the editor is open");
        editor.field.step(TextMove::End, false);
        runtime.refresh_chrome();
        runtime
            .window
            .rename
            .as_ref()
            .expect("the editor is open")
            .first_visible
    };
    renaming.folio.draw();
    assert!(start > 0, "the name is longer than its box");
    let inside = PhysicalPosition::new(renaming.left + 30.0, renaming.middle);
    renaming.folio.press(inside);
    let anchor = renaming.caret().0;
    let far_left = PhysicalPosition::new(renaming.left - 2000.0, renaming.middle);
    renaming.folio.move_to(far_left);
    renaming.folio.release(far_left);
    assert_eq!(
        renaming.caret(),
        (0, 0..anchor),
        "the drag reached the name's start"
    );
    let shown_from = renaming
        .folio
        .window
        .rename
        .as_ref()
        .expect("the editor is open")
        .first_visible;
    assert_eq!(shown_from, 0, "and the box scrolled back to show it");
}
