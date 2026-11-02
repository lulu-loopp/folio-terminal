//! **A press inside the open name editor puts the caret where it is, and a drag selects**
//! (0.4.9 plan row 179), on headless windows: a tab is renamed in the sidebar and on the strip,
//! the window is drawn, and the pointer is pressed at seams measured the way the painter measures
//! them — the box's left edge is where the caret stands at the draft's start, and the draft is
//! shaped in the face the tab list sets its names in.

use super::*;
use crate::headless::{Headless, HeadlessFolio};
use crate::test_support::cross_tab;
use crate::text_field::TextMove;
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
        folio.draw();
        let (left, top, bottom, advances) = {
            let runtime = folio.runtime();
            let [left, top, _, bottom] = runtime
                .window
                .rename_caret_line
                .expect("the open editor is drawn with its caret");
            let scale = runtime.window.renderer.scale_factor() as f32;
            let font = match layout {
                seats::TabLayoutMode::Vertical => bt_render::RAIL_TAB_FONT_LOGICAL_PX,
                seats::TabLayoutMode::Horizontal => bt_render::WINDOW_TAB_FONT_LOGICAL_PX,
            } * scale;
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
/// MUTATION: return at the top of `Runtime::press_rename_draft` — every caret stays at 0.
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
/// MUTATION: answer `Ok(false)` from `Runtime::drag_rename_selection` — the selection is empty.
#[test]
fn a_drag_inside_the_name_selects_what_it_crossed() {
    let mut renaming = Renaming::open("rename-caret-drag", seats::TabLayoutMode::Vertical, NAME);
    renaming.folio.press(renaming.at(1, 1.0));
    assert_eq!(held(&renaming.folio.runtime()), Some("RenameSelection"));
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
/// MUTATION: pass `false` for `extend` in `Runtime::press_rename_draft` — the selection is empty.
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
/// MUTATION: record `from: 0` in `TabRename::fit`'s `RenameDrawn` — the caret goes to the head.
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
