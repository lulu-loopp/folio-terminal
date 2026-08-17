//! The settings dialog — the modal the gear opens.
//!
//! Spec authority is `design/ui-mockup.html`'s `#settings-overlay` block: the
//! `.overlay` scrim, the `.settings` dialog on it, its `header`/`h1`/`.dlg-close`,
//! and the `.content` rows inside. Every number below is that stylesheet's own,
//! and the ones that are line boxes rather than declarations were measured in the
//! mock-up's own renderer rather than guessed from a line-height factor — a box
//! model derived from *our* font would drift the moment the font did, and the
//! design is the thing being reproduced.
//!
//! Two rulings from `docs/DESIGN.md` §7.1.5 shape the module:
//!
//! * **A modal means MODAL.** [`hit`] never returns "nothing" — every point in
//!   the window belongs to the overlay while it is up, so nothing behind the
//!   scrim can be dragged, clicked, selected or hovered. The mock-up says the
//!   same thing twice: the scrim is a real element at `z-index: 70`, and its
//!   keyboard guards are commented "a modal means MODAL: no layout edits behind
//!   the scrim".
//! * **The scrim outranks every popup and float.** That is a draw-order fact and
//!   lives in `bt-render`; what lives here is that the dialog is built out of
//!   [`bt_render::OverlayQuad`]s, which blend, because a scrim and a hairline
//!   over an unknown surface cannot be pre-composited.
//!
//! **The dialog's own keyboard** (2026-08-16) is the third: `InputOwner::Dialog`
//! says keys come *here*, and [`SettingsPanel::focus`] says where in here they
//! land. Until this slice the answer was nowhere — the modal swallowed every key
//! but Esc — and every surface the Settings block is about to grow (a category
//! rail, a shortcut recorder, a profile page) needs one before it can be
//! reached at all. The order is [`focus_order`], the walk is
//! [`SettingsPanel::key`], and the ring is the mock-up's own
//! `button:focus-visible` shown under the web's own `:focus-visible` rule.
//!
//! Nothing here is layout: the dialog is not a seat, takes no space from the
//! solver, and is never persisted (a dialog does not survive a restart).

use bt_persist::{BackgroundFitV1, LanguageV1, SplitDirectionV1, ThemeModeV1};
use bt_render::{
    ChromeLabel, ChromeLabelWeight, CursorStyle, FLOAT_WINDOW_BORDER_LOGICAL_PX,
    FLOAT_WINDOW_RADIUS_LOGICAL_PX, FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad,
    WINDOW_CAPTION_GLYPH_LOGICAL_PX, chrome_palette, rounded_overlay_fill, rounded_overlay_shadow,
};

use crate::i18n::Text;
use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::profiles;
use crate::seats::{RailMode, TabLayoutMode};

// ── `.settings`, `.overlay` ────────────────────────────────────────────────
/// `.settings { width: min(720px, 92%) }` — the cap and the share.
///
/// **480 until the category rail arrived** (user ruling Q2 = A, 2026-08-17). The
/// mock-up's own note beside the groups said the rail waits "rather than before
/// there is enough here to navigate", and the day it arrives 480 stops working
/// arithmetically rather than aesthetically: a 168px column plus the 22px
/// gutters plus a 118px picker leaves 150px for a title and the line that
/// explains it, and the line that explains it is the longest string in the
/// dialog. 720 is the width the industry's settings dialogs sit at, it keeps
/// the *page* (720 − 168 = 552) close to the 480 every row in here was measured
/// against, and it stays a centred dialog rather than becoming a whole-window
/// surface — which is the other half of the ruling: this is still a modal you
/// dismiss, not a place you go.
const DIALOG_MAX_WIDTH_LOGICAL_PX: f32 = 720.0;
const DIALOG_WIDTH_RATIO: f32 = 0.92;
/// `.settings { margin: 54px auto 0 }` — the drop from the window's top, and
/// `auto` on both sides, which is what centres it.
const DIALOG_TOP_LOGICAL_PX: f32 = 54.0;
/// `.settings { max-height: calc(100% - 72px) }`, of which 54 is the margin
/// above; this is what the rule leaves below.
const DIALOG_BOTTOM_LOGICAL_PX: f32 = 18.0;

// ── `.settings header` ─────────────────────────────────────────────────────
/// `padding: 16px 12px 10px 22px` around a 30px `.dlg-close`: 16 + 30 + 10.
const HEADER_HEIGHT_LOGICAL_PX: f32 = 56.0;
const HEADER_PADDING_TOP_LOGICAL_PX: f32 = 16.0;
const HEADER_PADDING_LEFT_LOGICAL_PX: f32 = 22.0;
/// 12 rather than 22 so the *icon* inside the 30px button ends 22 from the
/// dialog's edge — the mock-up says so in as many words.
const HEADER_PADDING_RIGHT_LOGICAL_PX: f32 = 12.0;
/// `.settings header h1 { font-size: 16px; font-weight: 600 }`. Weight is not
/// expressible through a chrome label and is noted as a deviation.
const HEADER_TITLE_FONT_LOGICAL_PX: f32 = 16.0;

// ── `.dlg-close` ───────────────────────────────────────────────────────────
/// `width: 30px; height: 30px`. Deliberately not the caption run's 46x40: the
/// mock-up rules that a dialog's close is not a caption button, because a
/// caption button's hard-edged rectangle is owed to the window edge it fills.
const CLOSE_SIDE_LOGICAL_PX: f32 = 30.0;
/// `border-radius: 6px`.
const CLOSE_RADIUS_LOGICAL_PX: f32 = 6.0;

// ── `.settings .content` ───────────────────────────────────────────────────
const CONTENT_PADDING_X_LOGICAL_PX: f32 = 22.0;
const CONTENT_PADDING_TOP_LOGICAL_PX: f32 = 2.0;
const CONTENT_PADDING_BOTTOM_LOGICAL_PX: f32 = 18.0;

// ── `.group-label` ─────────────────────────────────────────────────────────
const GROUP_LABEL_FONT_LOGICAL_PX: f32 = 11.0;
/// The 11px line box, measured in the mock-up.
const GROUP_LABEL_LINE_LOGICAL_PX: f32 = 13.0;
/// `margin: 10px 0 2px` — and now the *only* top margin a heading takes.
///
/// **`margin-top: 16px` retired with the rail** (2026-08-17). The mock-up used
/// to override the top of every heading after the first with an inline
/// `style="margin-top:16px"`, and the extra six pixels were what separated one
/// group from the next rather than one row from the next. With one category per
/// page there is no next group to separate: a page draws exactly one heading, at
/// the base margin, and the six pixels have nothing left to say. The mock-up is
/// written back with the same retirement, so the constant is not being dropped
/// ahead of its authority.
const GROUP_LABEL_MARGIN_TOP_LOGICAL_PX: f32 = 10.0;
const GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX: f32 = 2.0;
/// `letter-spacing: .05em` at 11px.
const GROUP_LABEL_TRACKING_EM: f32 = 0.05;

// ── `.row` ─────────────────────────────────────────────────────────────────
const ROW_PADDING_Y_LOGICAL_PX: f32 = 11.0;
const ROW_PADDING_X_LOGICAL_PX: f32 = 2.0;
const ROW_GAP_LOGICAL_PX: f32 = 16.0;
const ROW_TITLE_FONT_LOGICAL_PX: f32 = 13.5;
/// The 13.5px line box, measured in the mock-up.
const ROW_TITLE_LINE_LOGICAL_PX: f32 = 16.5;
const ROW_DESC_FONT_LOGICAL_PX: f32 = 12.0;
/// The 12px line box, measured in the mock-up.
const ROW_DESC_LINE_LOGICAL_PX: f32 = 14.5;
const ROW_DESC_MARGIN_TOP_LOGICAL_PX: f32 = 1.0;

// ── `.combo` ───────────────────────────────────────────────────────────────
const COMBO_MIN_WIDTH_LOGICAL_PX: f32 = 118.0;
/// 5 + the 13px line box (15.5) + 5 + two 1px borders.
const COMBO_HEIGHT_LOGICAL_PX: f32 = 27.5;
const COMBO_RADIUS_LOGICAL_PX: f32 = 6.0;
const COMBO_PADDING_LEFT_LOGICAL_PX: f32 = 12.0;
const COMBO_PADDING_RIGHT_LOGICAL_PX: f32 = 10.0;
const COMBO_FONT_LOGICAL_PX: f32 = 13.0;
/// The size a picker item's label is drawn at, and therefore the size it has to
/// be measured at. Public so the one caller that owns a font cannot measure the
/// popup's text at a size the popup does not draw it at.
pub const MENU_ITEM_FONT_LOGICAL_PX: f32 = COMBO_FONT_LOGICAL_PX;
/// `.combo .chev { font-size: 8.5px }`.
///
/// The chevron's own column is reserved at this same number: `▼` at 8.5px inks
/// 7.33px wide in the mock-up, so its em box is the tightest bound that cannot
/// cut the glyph, and a bound is all the value's own rectangle needs.
const COMBO_CHEVRON_FONT_LOGICAL_PX: f32 = 8.5;
// ── `.slider` (§7.1.6c-4b) ─────────────────────────────────────────────────
// The dialog's second control form, and the first added since it was built. It
// occupies the combo's column EXACTLY: 76 of track + 8 of gap + 34 of number is
// 118, which is `COMBO_MIN_WIDTH_LOGICAL_PX`, so the right edge of every control
// in the dialog is still one line. `the_slider_occupies_the_combos_column`
// holds the three to that sum rather than trusting three literals to keep
// agreeing.
const SLIDER_TRACK_WIDTH_LOGICAL_PX: f32 = 76.0;
const SLIDER_TRACK_HEIGHT_LOGICAL_PX: f32 = 4.0;
const SLIDER_THUMB_LOGICAL_PX: f32 = 12.0;
const SLIDER_GAP_LOGICAL_PX: f32 = 8.0;
const SLIDER_VALUE_WIDTH_LOGICAL_PX: f32 = 34.0;
const SLIDER_VALUE_FONT_LOGICAL_PX: f32 = 12.5;
/// `.slider .track { border-radius: 2px }` — half its own height, so the ends
/// are round rather than clipped.
const SLIDER_TRACK_RADIUS_LOGICAL_PX: f32 = SLIDER_TRACK_HEIGHT_LOGICAL_PX / 2.0;
/// What one press of `←`/`→` is worth.
///
/// Five and not one, because the value is a percentage of a picture's presence
/// and one percent of that is not a thing anybody can see — while twenty presses
/// across the whole range is a control that can actually be driven from the
/// keyboard. It is also the granularity the number beside the track can show
/// without ever looking like it failed to move.
pub const SLIDER_STEP_PERCENT: u8 = 5;
/// `.slider:hover .thumb { transform: scale(1.15) }`.
const SLIDER_THUMB_HOVER_SCALE: f32 = 1.15;

/// `.combo > button { gap: 10px }` — between the value and the chevron.
const COMBO_GAP_LOGICAL_PX: f32 = 10.0;
/// The mock-up's chevron is the character, not the `#i-chev` symbol: a solid
/// down-pointing triangle set as text beside the value.
const COMBO_CHEVRON: &str = "\u{25bc}";
/// What `text-overflow: ellipsis` puts at the cut — the single character, not
/// three periods, because that is the glyph the property is named after and the
/// one the face draws at a width three periods do not have.
const ELLIPSIS: &str = "\u{2026}";

// ── `.combo-menu` / `.combo-item` ──────────────────────────────────────────
/// `border-radius: 8px` — a popup menu's own round, and deliberately not the
/// 10px every *window* that floats shares.
const MENU_RADIUS_LOGICAL_PX: f32 = 8.0;
const MENU_PADDING_LOGICAL_PX: f32 = 4.0;
/// `top: calc(100% + 4px)`.
const MENU_OFFSET_LOGICAL_PX: f32 = 4.0;
/// The mock-up's own flip test: `menu.bottom > clip.bottom - 8`.
const MENU_CLEARANCE_LOGICAL_PX: f32 = 8.0;
const ITEM_HEIGHT_LOGICAL_PX: f32 = 27.5;
const ITEM_RADIUS_LOGICAL_PX: f32 = 5.0;
const ITEM_PADDING_X_LOGICAL_PX: f32 = 10.0;
const ITEM_GAP_LOGICAL_PX: f32 = 8.0;
/// `.combo-item .tick { width: 14px; font-size: 11px }` — a fixed column, so
/// every item's text starts at the same x whether or not it is the selected one.
const TICK_WIDTH_LOGICAL_PX: f32 = 14.0;
const TICK_FONT_LOGICAL_PX: f32 = 11.0;
const TICK: &str = "\u{2713}";

// ── `button:focus-visible` (mock-up 2205) ──────────────────────────────────
/// `outline: 2px solid var(--accent)` — the mock-up's one global focus ring,
/// declared for every `button` on the page and therefore for this dialog's `×`
/// and every row's picker button alike.
const FOCUS_RING_WIDTH_LOGICAL_PX: f32 = 2.0;
/// `outline-offset: 1px` — the gap between the control's own edge and the ring.
const FOCUS_RING_OFFSET_LOGICAL_PX: f32 = 1.0;
/// `border-radius: 6px`. The same round the controls themselves wear, which is
/// what keeps the ring concentric with the thing it names rather than a
/// rectangle around a rounded box.
const FOCUS_RING_RADIUS_LOGICAL_PX: f32 = 6.0;

// ── the category rail (`.settings-nav`, born 2026-08-17) ───────────────────
//
// **There is no mock-up CSS for this.** The mock-up carried the rail as a
// commented promise and nothing else — "deliberately NOT built yet: it waits for
// the Settings extension block, and it will be born the same day as the
// shortcut-editing panel" — so every number below is derived from tokens the
// dialog already uses rather than transcribed, and the mock-up is written back
// to in the same change so it goes on being the authority.
//
// The derivations, each named where it comes from:
//   * the column's width is the one free number, chosen so the longest category
//     word (`Rendered blocks`) sits on one line at 12.5px with the gutters the
//     rest of the dialog uses;
//   * an item's height is the `.row` rhythm at a smaller type size, its round is
//     the 6px every control in here wears, and its left padding is the picker
//     button's own 12;
//   * the selected ground is `--hover` (`dialog_hover`), which is the one lit
//     state this dialog has; a *hovered* word that is not the selected one wears
//     the same ground at half strength, so "where am I" and "what is under my
//     finger" stay two readings without a second colour — **no accent bar**
//     (user ruling 2026-08-17: in this house a selection is a ground and a
//     brighter word, never a coloured stroke; the files tree and the tab strip
//     say so already, and a bar would be a second vocabulary).

/// The rail's own column, header excluded.
const NAV_WIDTH_LOGICAL_PX: f32 = 168.0;
/// Above the first item — the `.group-label`'s own first `margin-top`, so the
/// rail's first word sits on the same line as the page's first heading.
const NAV_PADDING_TOP_LOGICAL_PX: f32 = 10.0;
/// Below the last, matching `.content`'s own bottom padding.
const NAV_PADDING_BOTTOM_LOGICAL_PX: f32 = 18.0;
/// The rail's own gutters, inside which the item pills sit.
const NAV_PADDING_X_LOGICAL_PX: f32 = 10.0;
const NAV_ITEM_HEIGHT_LOGICAL_PX: f32 = 30.0;
const NAV_ITEM_GAP_LOGICAL_PX: f32 = 2.0;
/// The round every control in this dialog wears.
const NAV_ITEM_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `.combo > button`'s own left padding, so the rail's words and the page's
/// pickers start their text the same distance inside their boxes.
const NAV_ITEM_PADDING_LEFT_LOGICAL_PX: f32 = 12.0;
/// Between `.group-label`'s 11 and `.row .title`'s 13.5: a rail is read at a
/// glance like a heading and chosen from like a row.
const NAV_ITEM_FONT_LOGICAL_PX: f32 = 12.5;
/// How strongly a hovered word that is *not* the page shows the ground the
/// selected word wears at full strength — the pointer's question, half as loud
/// as the answer.
const NAV_HOVER_GROUND_ALPHA: f32 = 0.5;

// ── the shortcut page (born 2026-08-17) ────────────────────────────────────
//
// Also without mock-up CSS, and derived the same way: a shortcut line *is* a
// `.row` — a title with a muted line under it on the left, controls on the
// right — so it keeps `.row`'s padding, gap and two type sizes exactly, and only
// the things `.row` has never held get numbers of their own.

/// What a chord's caps are given, right of the title column.
///
/// A reserved width rather than a measured one, because the geometry here is a
/// pure function of numbers and only the renderer knows how wide a glyph is
/// (the same division `widest_option` already runs on). 168 holds the widest
/// chord this table can produce — three caps and a range — with room to spare;
/// the caps are laid out right to left inside it, beside the button that
/// changes them.
const SHORTCUT_CAPS_WIDTH_LOGICAL_PX: f32 = 168.0;
/// One key cap: `.combo`'s hairline-and-face recipe at a smaller round.
const CAP_HEIGHT_LOGICAL_PX: f32 = 20.0;
const CAP_RADIUS_LOGICAL_PX: f32 = 4.0;
const CAP_PADDING_X_LOGICAL_PX: f32 = 6.0;
const CAP_GAP_LOGICAL_PX: f32 = 4.0;
const CAP_FONT_LOGICAL_PX: f32 = 11.5;
/// A cap is never narrower than it is tall: a single letter in a box a third of
/// its height wide reads as a sliver, not as a key.
const CAP_MIN_WIDTH_LOGICAL_PX: f32 = CAP_HEIGHT_LOGICAL_PX;

/// The `Record` button — `.btn`'s grammar (mock-up 2000-2008) at the picker's
/// own height, so a row's right-hand control is the same object it is on every
/// other page.
const SHORTCUT_RECORD_WIDTH_LOGICAL_PX: f32 = 84.0;
/// The `↺` beside it: a 24px square, the smallest hover verb this house draws.
const SHORTCUT_RESTORE_SIDE_LOGICAL_PX: f32 = 24.0;
/// Between the two right-hand controls.
const SHORTCUT_CONTROL_GAP_LOGICAL_PX: f32 = 8.0;
/// The glyph the restore verb wears. `↺` and not a word, because it stands
/// beside a button that is already a word and a second one would read as a
/// choice between two verbs rather than as an undo of one.
const RESTORE_GLYPH: &str = "\u{21ba}";
/// Above the page's own `Restore all defaults`, which stands off the last row
/// the way a later heading stands off the group above it.
const SHORTCUT_FOOT_MARGIN_TOP_LOGICAL_PX: f32 = 14.0;
const RESTORE_ALL_WIDTH_LOGICAL_PX: f32 = 152.0;
const BUTTON_HEIGHT_LOGICAL_PX: f32 = 27.5;
const BUTTON_RADIUS_LOGICAL_PX: f32 = 6.0;
const BUTTON_FONT_LOGICAL_PX: f32 = 13.0;

// ── every word this dialog puts in front of a person ───────────────────────
//
// Constants for `shortcuts.rs`' own reason: the i18n sweep lands next slice and
// collects a table by *finding* these, and a literal buried at a call site is a
// string that never gets translated.

/// The header, and the gear's own tooltip — [`Text::Settings`], which the gear
/// reads too, because they are one word for one thing.
const RECORD_BUTTON_LABEL: &str = "Record";
/// What the button says while it is listening.
const RECORD_LISTENING_LABEL: &str = "Press…";
/// What the muted line says while a row is listening.
///
/// **Three verbs and no preamble**, because the button beside it already says
/// `Press…` and this column is the narrowest in the dialog — it gives up 168px
/// to the caps that a settings row keeps. A sentence that started by repeating
/// the button would have spent the room it had on the half a reader already
/// knew, and arrived at the three things they do not cut off.
const RECORD_PROMPT: &str = "Enter keeps it · Esc cancels · Del clears";
/// What it says when a key arrives that no file could hold.
const RECORD_UNUSABLE_HINT: &str = "That key cannot be written down";
const RESTORE_ALL_LABEL: &str = "Restore all defaults";

// ── the one picker whose items carry a mark (mock-up 7647) ─────────────────
/// `.profile-item .ticon { width: 14px }` (mock-up 1023) — the same column the
/// `˅` menu gives a profile mark, because it is the same `.ticon` class.
const OPTION_ICON_COLUMN_LOGICAL_PX: f32 = 14.0;
/// `.pmark { width: 15px }` (mock-up 246). Wider than its 14px column by one
/// pixel, exactly as in the picker: a flex box centres a child that overflows it.
const OPTION_MARK_LOGICAL_PX: f32 = 15.0;
/// What a `.ticon` costs an item that has one: the column, and the flex gap
/// after it. Zero for every other row, which is what keeps their popups the
/// width they have always been.
fn option_icon_advance(row: SettingsRow, scale: f32) -> f32 {
    if row.option_mark(0).is_some() {
        (OPTION_ICON_COLUMN_LOGICAL_PX + ITEM_GAP_LOGICAL_PX) * scale
    } else {
        0.0
    }
}

/// `.ticon-wrap.dead .ticon { opacity: .35; filter: grayscale(1) }` (mock-up
/// 314), quoted through `profiles`' own reading of it — the picker greys an
/// unstartable profile exactly this way, and a second spelling of the same state
/// is how two surfaces come to disagree about what grey means.
const UNAVAILABLE_MARK_OPACITY: f32 = 0.35;

/// The persisted theme modes, in the mock-up's own picker order (2500-2502:
/// `Light`, `Dark`, then the selected `System`).
///
/// The two named modes first and the follow-the-OS one last, which is the order
/// every system picker in Windows uses and the order the mock-up draws: `System`
/// is not a third colour, it is the answer "ask somebody else", and an answer
/// about the question belongs after the answers to it. This list used to open
/// with `System` — the enum's own declaration order, arrived at by writing the
/// constant off `ThemeModeV1` instead of off the picker it draws.
pub const THEME_OPTIONS: [ThemeModeV1; 3] =
    [ThemeModeV1::Light, ThemeModeV1::Dark, ThemeModeV1::System];
pub const CURSOR_OPTIONS: [CursorStyle; 3] =
    [CursorStyle::Bar, CursorStyle::Block, CursorStyle::Underline];
pub const TAB_LAYOUT_OPTIONS: [TabLayoutMode; 2] =
    [TabLayoutMode::Horizontal, TabLayoutMode::Vertical];
pub const SIDEBAR_OPTIONS: [RailMode; 2] = [RailMode::Expanded, RailMode::Icons];
/// On first, which is the order every On/Off picker in the mock-up uses
/// (`data-combo="wrap"`, `data-combo="attnchip"`) and the order a reader expects
/// when the affirmative is the default.
pub const FORMULA_OPTIONS: [bool; 2] = [true, false];
/// What the Background image row's two items mean.
///
/// A named pair rather than a `bool`, because neither of them is the *value* of
/// the row: the value is a path, and these two are the two things a person does
/// to it. `Choose…` is a verb that opens the system's chooser and may come back
/// with nothing; `None` is the only way to clear a picture once one is set, and
/// giving it an item is what saves the row from needing a second control beside
/// its button to undo itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageSource {
    None,
    Choose,
}

/// `None` first, which is [`FORMULA_OPTIONS`]' order for its reason: the state
/// the row is born in reads first, and the verb reads second.
pub const IMAGE_SOURCE_OPTIONS: [ImageSource; 2] = [ImageSource::None, ImageSource::Choose];

/// The three ways a picture meets a window that is not its shape, in the
/// mock-up's order (Stretch, Fill, Tile) — least to most of the picture's own
/// geometry kept.
pub const IMAGE_FIT_OPTIONS: [BackgroundFitV1; 3] = [
    BackgroundFitV1::Stretch,
    BackgroundFitV1::Fill,
    BackgroundFitV1::Tile,
];

/// The three answers to "which way does a split with no direction of its own
/// cut" (user ruling, 2026-08-16), with the historical behaviour first — which
/// is both the product default and the order a reader expects when the first
/// item is what they already had.
pub const SPLIT_DIRECTION_OPTIONS: [SplitDirectionV1; 3] = [
    SplitDirectionV1::Auto,
    SplitDirectionV1::Right,
    SplitDirectionV1::Down,
];
/// The three answers to "which language is this window written in", in the order
/// [`THEME_OPTIONS`] established and for its reason: the two named answers first,
/// and the one that means "ask somebody else" after them.
///
/// **中文 first among the two named ones**, which is the one place this list is
/// not simply Theme's shape copied. A picker's job here is to be findable by
/// somebody who cannot read the dialog it is in: a user who opened Settings
/// *because* the window is in a language they do not read is looking for their
/// own language's name, and 中文 above English puts it where a Chinese reader's
/// eye goes first while costing an English reader nothing — they can read every
/// row on the page.
pub const LANGUAGE_OPTIONS: [LanguageV1; 3] =
    [LanguageV1::Chinese, LanguageV1::English, LanguageV1::System];

/// The sizes the grid's face can be drawn at, in logical pixels.
///
/// **A list and not a spinner**, which is the same ruling every other picker in
/// this dialog is built on: a control that accepts any number owes the user a
/// validator, a clamp and an answer to what happens at 4 and at 400, and none of
/// those is a question a font size is worth asking.
///
/// One-pixel steps through the range anybody reads at, then two-pixel steps
/// above it, because at 20 logical pixels a single pixel is a 5% change nobody
/// picking from a list is trying to express. 16 is the default and the number
/// every frame before this row was drawn at.
pub const FONT_SIZE_OPTIONS: [u8; 11] = [10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24];

/// The numerals the size picker draws.
///
/// A parallel array of literals rather than a formatted `String`, because
/// [`SettingsRow::option_label`] returns `&'static str` and a number is not a
/// translatable string — the i18n table's own header lists quantities among the
/// things that stay put in both languages. The two arrays are pinned to the same
/// length and the same numbers by
/// `every_font_size_has_exactly_its_own_numeral`.
const FONT_SIZE_LABELS: [&str; FONT_SIZE_OPTIONS.len()] = [
    "10", "11", "12", "13", "14", "15", "16", "18", "20", "22", "24",
];

/// This machine's monospaced families, enumerated once and kept for the life of
/// the process.
///
/// **The `OnceLock` is what makes `option_label`'s `&'static str` honest.** A
/// family name is a runtime string, and every other picker in this dialog hands
/// back a literal; leaking one `Vec<String>` into a `static` gives the same
/// lifetime without a `Box::leak` per row, without allocating on a dialog that
/// redraws on hover, and without changing the signature of nine functions and
/// their thirty call sites. The list cannot change under it, because the answer
/// is only wanted while a picker is open and a font installed mid-session is a
/// case every other program answers with "restart" too.
///
/// Enumerated lazily — the first time a caller actually asks — rather than at
/// startup, because opening a system font collection is exactly the cost
/// `bt_render::terminal_font_system` refuses to pay on every launch. The Settings
/// dialog is the only thing that asks.
static MONOSPACE_FAMILIES: std::sync::OnceLock<Vec<bt_platform::MonospaceFamily>> =
    std::sync::OnceLock::new();

/// Every monospaced family this machine has, in the order the picker draws them.
#[must_use]
pub fn monospace_families() -> &'static [bt_platform::MonospaceFamily] {
    MONOSPACE_FAMILIES.get_or_init(|| {
        #[cfg(windows)]
        {
            bt_platform::monospace_font_families()
        }
        #[cfg(not(windows))]
        {
            bt_platform::order_monospace_families(Vec::new())
        }
    })
}

/// Which row of the family picker a stored family name is.
///
/// A name the machine does not have resolves to the default's row, which is the
/// same degradation `bt_render::GpuContext::set_terminal_font` performs on the
/// face itself: what the picker shows and what the grid is drawn in are then one
/// answer, rather than a tick on a family nothing is rendering.
#[must_use]
pub fn family_index(name: &str) -> usize {
    monospace_families()
        .iter()
        .position(|family| family.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            monospace_families().iter().position(|family| {
                family
                    .name
                    .eq_ignore_ascii_case(bt_platform::DEFAULT_MONOSPACE_FAMILY)
            })
        })
        .unwrap_or(0)
}

/// The names one scheme picker draws, in its own order, as `&'static str`.
///
/// Two `OnceLock`s and not one list filtered per call, for
/// [`monospace_families`]'s reason taken one step further: `option_label`
/// returns `&'static str`, and the *order* has to be stable across the measure
/// pass, the hit test and the draw within one frame. The catalogue behind them
/// is itself read once per process — see `crate::schemes::catalogue` — so these
/// are two views of one enumeration rather than two enumerations.
#[must_use]
pub fn scheme_labels(light: bool) -> &'static [&'static str] {
    static LIGHT: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    static DARK: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    let slot = if light { &LIGHT } else { &DARK };
    slot.get_or_init(|| crate::schemes::catalogue().names_for(light).collect())
}

/// Which row of a scheme picker a stored name is.
///
/// A name this build does not hold — a file deleted, or one a newer build
/// bundled — resolves to the default's row, which is
/// `crate::schemes::Catalogue::resolve`'s answer asked as an index: the tick and
/// the colours on screen are one resolution, not two that agree most of the
/// time.
#[must_use]
pub fn scheme_index(name: &str, light: bool) -> usize {
    let labels = scheme_labels(light);
    labels
        .iter()
        .position(|label| *label == name)
        .or_else(|| {
            let fallback = crate::schemes::catalogue().default_name(light);
            labels.iter().position(|label| *label == fallback)
        })
        .unwrap_or(0)
}

/// Which row of the size picker a stored size is, or the default's row for a
/// size this build's list does not offer.
///
/// `bt_persist` deliberately does not clamp the stored size — a file written by
/// a newer build may name 17 — so the degradation happens here, where the list
/// that does not contain it lives.
#[must_use]
pub fn font_size_index(size: u8) -> usize {
    FONT_SIZE_OPTIONS
        .iter()
        .position(|option| *option == size)
        .unwrap_or_else(|| {
            FONT_SIZE_OPTIONS
                .iter()
                .position(|option| *option == bt_persist::DEFAULT_TERMINAL_FONT_SIZE)
                .unwrap_or(0)
        })
}

/// The label a theme wears in the picker, matching the mock-up's own casing.
fn theme_label(theme: ThemeModeV1) -> &'static str {
    match theme {
        ThemeModeV1::System => Text::OptionSystem.text(),
        ThemeModeV1::Light => Text::OptionLight.text(),
        ThemeModeV1::Dark => Text::OptionDark.text(),
    }
}

/// The label a language wears in the picker.
///
/// **The two named ones are endonyms and are not translated**: `English` reads
/// `English` in a Chinese dialog and `中文` reads `中文` in an English one, which
/// is what every operating system's own language picker does and for the reason
/// this row exists at all — the person who most needs to find their language is
/// the person who cannot read the words around it. Only `System` is a word about
/// the question rather than a name, so only `System` is translated, and it is
/// [`Text::OptionSystem`] — the same string the Theme row uses, because it is the
/// same promise.
fn language_label(language: LanguageV1) -> &'static str {
    match language {
        LanguageV1::System => Text::OptionSystem.text(),
        LanguageV1::English => "English",
        LanguageV1::Chinese => "中文",
    }
}

fn cursor_label(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::Bar => Text::OptionCursorBar.text(),
        CursorStyle::Block => Text::OptionCursorBlock.text(),
        CursorStyle::Underline => Text::OptionCursorUnderline.text(),
    }
}

fn tab_layout_label(layout: TabLayoutMode) -> &'static str {
    match layout {
        TabLayoutMode::Horizontal => Text::OptionHorizontal.text(),
        TabLayoutMode::Vertical => Text::OptionVertical.text(),
    }
}

fn image_source_label(source: ImageSource) -> &'static str {
    match source {
        ImageSource::None => Text::OptionImageNone.text(),
        ImageSource::Choose => Text::OptionImageChoose.text(),
    }
}

fn image_fit_label(fit: BackgroundFitV1) -> &'static str {
    match fit {
        BackgroundFitV1::Stretch => Text::OptionFitStretch.text(),
        BackgroundFitV1::Fill => Text::OptionFitFill.text(),
        BackgroundFitV1::Tile => Text::OptionFitTile.text(),
    }
}

/// Which row of the fit picker is ticked, for a stored fit.
///
/// A free function beside the other `*_index` resolvers, and public for their
/// reason: the caller holds `bt_persist`'s value and the dialog speaks in
/// indices, and doing the conversion in two places is how the ticked row comes
/// to disagree with the window.
#[must_use]
pub fn image_fit_index(fit: BackgroundFitV1) -> usize {
    IMAGE_FIT_OPTIONS
        .iter()
        .position(|candidate| *candidate == fit)
        .unwrap_or(0)
}

/// The mock-up's own word for both states of every On/Off picker it draws.
fn on_off_label(enabled: bool) -> &'static str {
    if enabled {
        Text::OptionOn.text()
    } else {
        Text::OptionOff.text()
    }
}

/// User ruling 2026-08-10: the mode's name, not a sentence about it.
///
/// The mock-up used to spell this option out as "Icons, expand on hover" so the
/// picker would name the hover behaviour; the ruling cut it back to "Icons" and
/// the mock-up moved with it (line 2381). A picker item is a name, and the one
/// place with no room to explain a behaviour is the line the user reads while
/// choosing it. The rail still expands on hover — the label just stops saying so.
fn sidebar_label(mode: RailMode) -> &'static str {
    match mode {
        RailMode::Expanded => Text::OptionExpanded.text(),
        RailMode::Icons => Text::OptionIcons.text(),
    }
}

/// What a split direction is called in the picker.
///
/// `Auto` says what it does in the parenthesis, and that is deliberate against
/// the ruling `sidebar_label` above records: "a picker item is a name, not a
/// sentence about it". The two are not in conflict, because `Auto` on its own is
/// not a name of anything — every other item here names a direction, and the one
/// that names a *rule* has to say which rule or it names nothing. `Right` and
/// `Down` are bare for exactly the reason the ruling gives.
fn split_direction_label(direction: SplitDirectionV1) -> &'static str {
    match direction {
        SplitDirectionV1::Auto => Text::OptionSplitAuto.text(),
        SplitDirectionV1::Right => Text::OptionSplitRight.text(),
        SplitDirectionV1::Down => Text::OptionSplitDown.text(),
    }
}

/// The mark a split direction wears in the picker.
///
/// The second row in this dialog whose items carry one, and it earns it the way
/// `Default profile` earns its profile marks (`UI-UX.md:115`): the difference
/// between "beside" and "below" is a *shape*, and this build already draws that
/// shape twice — `#i-split-right` and `#i-split-down`, the pair the pane menu
/// used to caption in words before the picker replaced them with a diagram. A
/// picker that named two axes in text alone would be the one place in the
/// product where they are not marks.
///
/// `Auto` wears the `⊞` the pane head used to: it is the glyph for "split, and
/// do not ask me which way", which is precisely what the option means.
fn split_direction_mark(direction: SplitDirectionV1) -> ChromeMark {
    match direction {
        SplitDirectionV1::Auto => ChromeMark::Split,
        SplitDirectionV1::Right => ChromeMark::SplitRight,
        SplitDirectionV1::Down => ChromeMark::SplitDown,
    }
}

/// **One page of the dialog, and one word in the rail beside it** (user ruling
/// Q2/Q3 = A, 2026-08-17).
///
/// This is what the mock-up's group labels grew into. Until today they were
/// headings inside one scroll — "GROUPS, not a flat list", with the rail
/// deliberately postponed "rather than before there is enough here to navigate"
/// — and the day the shortcut editor arrives there is more than enough: an
/// editor is a page, not a run of rows, and a scroll that ran a thirty-line
/// table on under the last picker would have been a dialog nobody could find
/// the top of. So a group became a category, a category owns a page, and the
/// heading it used to draw is now that page's title.
///
/// **The rail is derived from the rows, exactly as the headings were.** A
/// category with nothing to show draws no rail item, costs no width and cannot
/// be navigated to — which is the same sentence as "a group with no rows draws
/// no heading and costs no height", one level up, and it is what settles the
/// placeholder question: `Profiles` and `Language` are not listed with a page
/// saying "coming next", they simply arrive on the day their rows do. A door
/// onto an empty room is a worse promise than no door.
///
/// The order here is the rail's order and it is declared rather than derived,
/// because it is a reading order and not a fact about the rows: `General` first
/// because it is where a dialog opens, `Shortcuts` last because a table is a
/// place you go on purpose. Within a page, the rows keep the order
/// [`visible_rows`] gives them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsCategory {
    /// The catch-all, and the page the dialog opens on.
    ///
    /// It is where the mock-up's `Startup` group and this build's own `Files`
    /// group have gone. Both held exactly one row, and two headings over two
    /// rows is a table of contents for a paragraph; a reader looking for "what
    /// opens on a new tab" or "does this thing read my repository" is looking
    /// for the page that is not about a *look*, and that page is this one.
    #[default]
    General,
    Appearance,
    /// Mock-up 2555. **Born with the PSReadLine row** (§7.1.6c-3b): the variant
    /// and its heading existed for two slices with nothing to put under them,
    /// and `nav_items` derives the rail from what has content, so the page
    /// appeared the moment it had one. Line wrapping is still the row the
    /// mock-up drew here first, and it still has no setting behind it.
    Terminal,
    RenderedBlocks,
    /// The one category whose page is not a list of rows.
    ///
    /// It is never empty — the shortcut table is a constant of this build — so
    /// it is always in the rail, and it is last because it is the page a reader
    /// goes to deliberately rather than one they scroll past.
    Shortcuts,
}

impl SettingsCategory {
    /// Every category this build knows, in the rail's own order.
    pub const ALL: [Self; 5] = [
        Self::General,
        Self::Appearance,
        Self::Terminal,
        Self::RenderedBlocks,
        Self::Shortcuts,
    ];

    /// The page's own heading, drawn in the `.group-label` grammar the groups
    /// this replaced were drawn in.
    ///
    /// Upper-cased at the source, as `"APPEARANCE"` always was: the chrome text
    /// path has no `text-transform`, and the mock-up's own words are
    /// "Appearance" (2492), "Terminal" (2555), "Rendered blocks" (2570) and
    /// "Startup" (2601).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::General => Text::CategoryGeneral.text(),
            Self::Appearance => Text::CategoryAppearance.text(),
            Self::Terminal => Text::CategoryTerminal.text(),
            Self::RenderedBlocks => Text::CategoryRenderedBlocks.text(),
            Self::Shortcuts => Text::CategoryShortcuts.text(),
        }
    }

    /// The word in the rail.
    ///
    /// **A second string and not the heading lower-cased**, and the reason is
    /// the i18n ruling rather than typography: this file upper-cases at the
    /// source precisely because "大写是内容不是样式" — a `to_uppercase` here
    /// would be a rule that has to be *unlearned* the day the same word arrives
    /// in Chinese, where there is no case to raise. Two literals side by side in
    /// one match is a reader seeing both at once; a transformation is a rule
    /// somebody has to remember does not apply.
    #[must_use]
    pub fn nav_label(self) -> &'static str {
        match self {
            Self::General => Text::NavGeneral.text(),
            Self::Appearance => Text::NavAppearance.text(),
            Self::Terminal => Text::NavTerminal.text(),
            Self::RenderedBlocks => Text::NavRenderedBlocks.text(),
            Self::Shortcuts => Text::NavShortcuts.text(),
        }
    }
}

/// The span of whole percentages one slider row runs over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SliderRange {
    pub min: u8,
    pub max: u8,
}

impl SliderRange {
    /// This range's own clamp — the one place a value is forced into it.
    #[must_use]
    pub const fn clamp(self, value: u8) -> u8 {
        if value < self.min {
            self.min
        } else if value > self.max {
            self.max
        } else {
            value
        }
    }

    /// How far along the track a value sits, `0.0..=1.0`.
    #[must_use]
    pub fn fraction(self, value: u8) -> f32 {
        let span = f32::from(self.max.saturating_sub(self.min));
        if span <= 0.0 {
            return 0.0;
        }
        f32::from(self.clamp(value) - self.min) / span
    }

    /// The value a fraction of the way along the track, rounded to the nearest
    /// whole percentage.
    ///
    /// Rounded and not truncated, because this is what a drag produces: truncation
    /// would make the thumb lag the pointer by up to a whole step on the way up
    /// and sit exactly under it on the way down, which reads as the control
    /// sticking.
    #[must_use]
    pub fn value_at(self, fraction: f32) -> u8 {
        let span = f32::from(self.max.saturating_sub(self.min));
        let offset = (fraction.clamp(0.0, 1.0) * span).round();
        self.min.saturating_add(offset as u8).min(self.max)
    }

    /// One press of an arrow key, from `value` in the direction of `delta`.
    ///
    /// Stepping is from the value's own position and not from a grid: a file that
    /// says 63 steps to 68, not to 65. Snapping to multiples would silently
    /// rewrite a number the user may have typed into `settings.json` on purpose,
    /// and the first arrow press is exactly when they would not be looking.
    #[must_use]
    pub fn stepped(self, value: u8, delta: i16) -> u8 {
        let value = i16::from(self.clamp(value)) + delta * i16::from(SLIDER_STEP_PERCENT);
        self.clamp(value.clamp(0, 255) as u8)
    }
}

/// Which form a row's control takes.
///
/// Two, and the dialog intends to stay at two: a combo answers a question with a
/// small named set, a slider answers one with a percentage, and every row in
/// this product is one of those two questions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsControl {
    Combo,
    Slider(SliderRange),
}

impl SettingsControl {
    #[must_use]
    pub const fn range(self) -> Option<SliderRange> {
        match self {
            Self::Combo => None,
            Self::Slider(range) => Some(range),
        }
    }
}

/// Where a slider's parts land inside the control column.
///
/// **One derivation, four readers** — the paint, the hit test, the drag and the
/// keyboard's own idea of where the thumb now is. The track is the thing that
/// matters: a thumb is 12px wide and a track is 76, so a rule that measured the
/// press against the *thumb's* travel rather than the track's length would put
/// 100% six pixels short of the right-hand end, which is exactly far enough to
/// be unreachable and not far enough to look like a bug.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderGeometry {
    /// The whole control column — what a press has to land in to count.
    pub band: [f32; 4],
    /// The groove, full width, at its own 4px height.
    pub track: [f32; 4],
    /// The filled part of the groove, left edge to the thumb's centre.
    pub fill: [f32; 4],
    /// The thumb's own square, centred on its position along the track.
    pub thumb: [f32; 4],
    /// Where the percentage is written.
    pub value: [f32; 4],
}

/// Solve one slider's geometry inside the box the row gave its control.
///
/// `combo` is the row's control rectangle — the same 118-wide box a picker would
/// have had, which is what keeps the two forms in one column.
#[must_use]
pub fn slider_geometry(
    combo: [f32; 4],
    scale: f32,
    range: SliderRange,
    value: u8,
) -> SliderGeometry {
    let px = |logical: f32| logical * scale;
    let track_width = px(SLIDER_TRACK_WIDTH_LOGICAL_PX);
    let track_height = px(SLIDER_TRACK_HEIGHT_LOGICAL_PX);
    let thumb = px(SLIDER_THUMB_LOGICAL_PX);
    let middle = (combo[1] + combo[3]) / 2.0;
    let track = [
        combo[0],
        middle - track_height / 2.0,
        combo[0] + track_width,
        middle + track_height / 2.0,
    ];
    let centre = track[0] + track_width * range.fraction(value);
    SliderGeometry {
        band: combo,
        track,
        fill: [track[0], track[1], centre.max(track[0]), track[3]],
        thumb: [
            centre - thumb / 2.0,
            middle - thumb / 2.0,
            centre + thumb / 2.0,
            middle + thumb / 2.0,
        ],
        value: [
            track[2] + px(SLIDER_GAP_LOGICAL_PX),
            combo[1],
            track[2] + px(SLIDER_GAP_LOGICAL_PX + SLIDER_VALUE_WIDTH_LOGICAL_PX),
            combo[3],
        ],
    }
}

/// The value a press or drag at `x` is asking for.
///
/// Measured against the **track**, clamped at both ends, so that grabbing the
/// thumb and dragging past the end of the dialog lands on the end of the range
/// rather than wherever the arithmetic ran out.
#[must_use]
pub fn slider_value_at(combo: [f32; 4], scale: f32, range: SliderRange, x: f32) -> u8 {
    let track_width = SLIDER_TRACK_WIDTH_LOGICAL_PX * scale;
    if track_width <= 0.0 {
        return range.min;
    }
    range.value_at((x - combo[0]) / track_width)
}

/// One line of the dialog's `.content`: a title, a description and a picker.
///
/// An enumeration and not a set of named fields, because a row now exists in
/// four places — the dialog's height, the stack's offsets, the hit test and the
/// draw — and one of them is *conditional* (see [`visible_rows`]). Named fields
/// make each of those four teach itself the condition separately, and the first
/// one that is not taught is a control the user can click but cannot see.
///
/// **Order is Theme, Cursor, Tab layout, Sidebar, Display formulas.** The
/// mock-up's binding constraint is that Sidebar reads as a dependent of Tab
/// layout — its row only exists while Tab layout is Vertical — so it has to sit
/// immediately under the row it depends on. Cursor is a row the mock-up does not
/// have at all, so it keeps the position it already shipped in rather than being
/// moved for a reason the mock-up never states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsRow {
    Theme,
    /// **The palette a Light window wears** (§7.1.6c-4a), and its dark twin
    /// below.
    ///
    /// Directly under `Theme` because the three are one decision read in
    /// sequence: `Theme` says *which* canvas, and these two say what that canvas
    /// looks like. A scheme never overrules the mode — a window on `Dark` stays
    /// dark whatever is chosen here — which is exactly why the two rows exist
    /// instead of one: the pair a user is not currently looking at has to be
    /// settable too, or switching the theme would mean coming back here.
    ///
    /// **Each row lists only the schemes whose canvas is its own.** That is not
    /// tidiness: the chrome picks its palette from the luma of the background
    /// actually painted, so a dark scheme in this row would paint a dark canvas
    /// and then be dressed in the *other* row's chrome. See
    /// `crate::schemes::Catalogue::names_for`.
    LightScheme,
    /// The palette a Dark window wears. Its description is where the pair says
    /// once where a user's own scheme files go.
    DarkScheme,
    /// **The picture behind the window** (§7.1.6c-4b), and the five rows under
    /// it that finish the same sentence.
    ///
    /// Directly under the scheme pair because that is where the ground is being
    /// described: the schemes say what colour it is, these say whether there is
    /// a picture on it, how it meets a window that is not its shape, how much of
    /// it comes through, how much of the *desktop* comes through behind it, and
    /// whether Windows blurs that. The last row of the six is a window posture
    /// rather than a ground, and it is here because it is the one place a reader
    /// looking for "how this window behaves" will already be.
    ///
    /// **A two-item picker and not a new control form.** `None` and `Choose…`
    /// are the two things a person does to this row, the second opens the
    /// system's own chooser, and the button carries the chosen file's name.
    /// Clearing therefore has a first-class home — a browse button alone would
    /// have needed a second control beside it to undo itself.
    BackgroundImage,
    /// How that picture meets a window that is not its shape: Stretch, Fill,
    /// Tile. Three answers and not a number, because each is a different
    /// sentence about what may be lost — the aspect ratio, the edges, or
    /// nothing.
    ImageFit,
    /// **The dialog's first slider**, and the reason the form exists: this row's
    /// answer is a percentage, and a percentage in a picker is a list of
    /// twenty-one items nobody wants to scroll to find 65 in.
    ImageOpacity,
    /// How much of the window's ground is there at all — the row that lets the
    /// desktop through. Floored at
    /// `bt_persist::MINIMUM_BACKGROUND_OPACITY`, and its description states
    /// what does *not* fade, because "background opacity" on a terminal reads
    /// as "everything".
    ///
    /// One of the two rows in this dialog that can be greyed **whole**: it needs
    /// a window composited with premultiplied alpha, and where the renderer did
    /// not get one there is nothing here to offer.
    BackgroundOpacity,
    /// Windows' own blur behind the ground (`DWMWA_SYSTEMBACKDROP_TYPE`).
    ///
    /// The second row that can be greyed whole, and the one that actually is on
    /// a real machine: the attribute arrived in Windows 11 22H2, and every older
    /// Windows refuses it. The refusal is asked of DWM rather than of a build
    /// number — see `bt_platform::system_backdrop_available`.
    Acrylic,
    /// Whether the window stays above other windows.
    ///
    /// The one row of the six that is visible with a picture, a blur and an
    /// opacity all switched off, and the only one that is not about drawing at
    /// all.
    AlwaysOnTop,
    Cursor,
    TabLayout,
    Sidebar,
    /// User ruling 2026-08-10. Last, because it is the only row of the second
    /// group; it used to sit above the conditional Tab layout/Sidebar pair to
    /// keep it from sliding up and down the dialog, and being in a group of its
    /// own underneath them serves the same end.
    Formulas,
    /// The sibling switch for `$…$` runs inside command output, directly under
    /// the block one it reads as a variant of. Two switches and not one because
    /// the two carry different risk — a `$$` pair is a whole-line delimiter,
    /// while a lone `$` is the most overloaded byte a shell prints — so a user
    /// who wants typeset blocks with every `$` in a log left alone has to be
    /// able to say exactly that.
    InlineFormulas,
    /// The Git panel's master switch (user ruling, 2026-08-15) — the Files
    /// group's only row.
    ///
    /// **The one row in this dialog whose Off costs nothing and buys silence.**
    /// Every other switch here changes what the product draws; this one changes
    /// what it *does*: off, no `git` process is ever started, because the page
    /// that would ask for one does not exist. That is why it is a switch in the
    /// settings and not a fold on the panel — a control that only hid the drawing
    /// would leave the reading in place, which is the half a user turning this off
    /// is actually asking about.
    GitPanel,
    /// **Which way a split with no direction of its own cuts** (user ruling,
    /// 2026-08-16).
    ///
    /// Filed under `Appearance` beside `Tab layout`, which is where the two
    /// "where does a thing go" rows belong together — and deliberately not in a
    /// group of its own. A `Panes` heading over one row would be a category the
    /// dialog has to grow into rather than one it has, and the row a user goes
    /// looking for after meeting the pane menu's picker is the row next to the
    /// one about where tabs go.
    ///
    /// It governs only the verbs whose sentence stops at "split" — see
    /// `bt_persist::SettingsV1::split_direction`, which is where the scope is
    /// argued, and `Runtime::settings_split_axis`, which is the one place it is
    /// read.
    SplitDirection,
    /// Mock-up 2464-2474, the Startup group's only row — and the first picker in
    /// this dialog whose items carry a mark (7645-7648).
    ///
    /// **The one row whose options can be unavailable.** Every other picker here
    /// offers choices this program can always honour; this one offers four
    /// shells, and whether a machine has Git Bash is not the product's to decide.
    /// A greyed item is the same sentence the `˅` menu's greyed row speaks, in
    /// the same words, because it is the same fact.
    DefaultProfile,
    /// **Which language the window writes in** (user ruling 2026-08-10, shipped
    /// 2026-08-17) — `General`'s first row, above the two that were already here.
    ///
    /// First because it is the row that decides how every other row reads. A
    /// user who has opened this dialog to change the language has opened it
    /// unable to read it, and the one kindness available is to put the row where
    /// the eye lands first rather than under two sentences they cannot parse.
    ///
    /// **The only row in the dialog whose answer does not take effect where you
    /// can see it.** Its description says so and choosing a value raises a card
    /// that says it again; [`crate::i18n`]'s header argues why, and the short of
    /// it is that this window caches measured widths in a dozen places and has no
    /// language revision to invalidate them with.
    Language,
    /// **The face the grid is drawn in** (§7.1.6c-3b) — never the window's own.
    ///
    /// Under `Appearance` and next to the size it travels with, because they are
    /// one decision made in two halves and a reader who has changed one is about
    /// to look for the other.
    ///
    /// The interface font is a **stated left-out** of this slice rather than an
    /// oversight, and there are two named blockers: `GpuContext`'s
    /// `chrome_cap_height_ratio`, resolved once because a face cannot change,
    /// and `terminal_font_system`'s deliberate refusal to enumerate `Fonts/` at
    /// startup — which the sans loader's two-file stack depends on. Both are
    /// argued in `docs/DESIGN.md` §7.1.6c-3b.
    TerminalFont,
    /// How large that face is drawn, in logical pixels.
    ///
    /// Hot, like the family: the whole DPI path already exists to re-measure a
    /// grid and tell every shell its new cell size, and a font size change is
    /// the same event arriving through another door.
    FontSize,
    /// **The Terminal page's first row**, and therefore the row that puts that
    /// page in the rail (§7.1.6c-3b).
    ///
    /// An On/Off picker and not a pair of buttons, and the choice is worth
    /// stating because the ticket left it open. What the user is deciding is a
    /// state — *is Folio's patched PSReadLine installed on this machine* — and a
    /// state with two values is what every other picker in this dialog already
    /// draws. A button row would have needed a tenth row function, a new hit
    /// target, a new focus stop and a new drawing, to express the same two
    /// answers less clearly.
    ///
    /// Both items can be unavailable, which is the second reason the combo fits:
    /// `On` is dark under an execution policy that would refuse the module, and
    /// `Off` is dark unless the copy on disk is byte for byte the one Folio
    /// wrote. That is [`Self::DefaultProfile`]'s greyed-item machinery, already
    /// spelled once for both the hit test and the draw, and the description
    /// carries the reason.
    PsReadLine,
}

impl SettingsRow {
    /// Which page this row is on, and therefore which word in the rail leads to
    /// it.
    ///
    /// Stated once, per row, and read by everything — the rail's items, the
    /// page's rows, its height, its hit test, its draw and its heading are all
    /// derived by walking [`visible_rows`] and reading this answer, so there is
    /// no second list of categories to keep in step and no way to show a page
    /// under a name that does not describe it. **Six readings of one
    /// derivation** now, which is R4's lesson taken as far as it goes: the first
    /// reading nobody teaches is a control the user can reach and cannot see.
    #[must_use]
    pub fn category(self) -> SettingsCategory {
        match self {
            Self::Theme
            | Self::LightScheme
            | Self::DarkScheme
            | Self::Cursor
            | Self::TabLayout
            | Self::Sidebar
            | Self::SplitDirection
            | Self::TerminalFont
            | Self::FontSize
            // The window's ground and the window's postures (§7.1.6c-4b). All
            // six are Appearance, including `Always on top`: it is not a look,
            // but it is not a language, a shell or a block either, and the page
            // a reader hunting "how this window behaves" opens first is this
            // one — the same judgement `Split direction` was filed under.
            | Self::BackgroundImage
            | Self::ImageFit
            | Self::ImageOpacity
            | Self::BackgroundOpacity
            | Self::Acrylic
            | Self::AlwaysOnTop => SettingsCategory::Appearance,
            // The row the mock-up's `Terminal` page was waiting for. It is about
            // what a shell does rather than what the window looks like, which is
            // the line that page's name draws.
            Self::PsReadLine => SettingsCategory::Terminal,
            // The mock-up files what typesetting does to a block under "Rendered
            // blocks" (2570), beside that page's own Maximum height row.
            Self::Formulas | Self::InlineFormulas => SettingsCategory::RenderedBlocks,
            // Both were headings over one row apiece — `FILES` and `STARTUP` —
            // and both are the same kind of question: not a look, not a block, no
            // page of their own to fill. See [`SettingsCategory::General`].
            //
            // Language joins them rather than `Appearance`, and the distinction
            // is the one that page's own name makes: appearance is what the
            // window *looks* like, and a language is what it *says*. A reader
            // hunting for it under a heading about looks would be hunting for the
            // wrong noun.
            Self::GitPanel | Self::DefaultProfile | Self::Language => SettingsCategory::General,
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Theme => Text::RowTheme.text(),
            Self::Cursor => Text::RowCursor.text(),
            Self::Formulas => Text::RowFormulas.text(),
            Self::InlineFormulas => Text::RowInlineFormulas.text(),
            Self::GitPanel => Text::RowGitPanel.text(),
            // Mock-up 2360.
            Self::TabLayout => Text::RowTabLayout.text(),
            // Mock-up 2374.
            Self::Sidebar => Text::RowSidebar.text(),
            Self::SplitDirection => Text::RowSplitDirection.text(),
            // Mock-up 2467.
            Self::DefaultProfile => Text::RowDefaultProfile.text(),
            Self::Language => Text::RowLanguage.text(),
            Self::TerminalFont => Text::RowTerminalFont.text(),
            Self::FontSize => Text::RowFontSize.text(),
            Self::LightScheme => Text::RowLightScheme.text(),
            Self::DarkScheme => Text::RowDarkScheme.text(),
            Self::PsReadLine => Text::RowPsReadLine.text(),
            Self::BackgroundImage => Text::RowBackgroundImage.text(),
            Self::ImageFit => Text::RowImageFit.text(),
            Self::ImageOpacity => Text::RowImageOpacity.text(),
            Self::BackgroundOpacity => Text::RowBackgroundOpacity.text(),
            Self::Acrylic => Text::RowAcrylic.text(),
            Self::AlwaysOnTop => Text::RowAlwaysOnTop.text(),
        }
    }

    /// The line under a row's title — **a function of the row's current value**.
    ///
    /// The mock-up varies two of these with the picker beside them: `wrap-desc`
    /// says "Long lines fold at the pane's edge" on and "Long lines run on and
    /// the pane scrolls sideways" off (2561, 7897), and `blockmax-desc` swaps
    /// between "Rendered blocks show in full" and "Blocks taller than this
    /// scroll inside themselves" (2576, 7904). A description that describes the
    /// *setting* rather than the *state* is the one line a user reads to find
    /// out what the switch they are looking at is currently doing, so the value
    /// has to be in scope here.
    ///
    /// Neither of those two rows exists yet — there is no line-wrapping and no
    /// block-height setting behind them (`bt_persist::SettingsV1` holds neither),
    /// and this build does not invent a row for a preference nothing reads. What
    /// arrives now is the *shape*: the parameter, so that the slice which adds
    /// those rows adds two match arms rather than changing every call site of a
    /// method with nine of them.
    ///
    /// Still `&'static str`, which is the i18n ruling's own constraint
    /// (2026-08-13): the language table is a `match lang` returning one of two
    /// literals, and a description that varied by value *and* by language is
    /// still a choice among literals. A `String` here would allocate on every
    /// frame of a dialog that redraws on hover.
    #[must_use]
    pub fn description(self, values: SettingsValues) -> &'static str {
        match self {
            // Mock-up 2496, word for word. It used to read "Light or dark" with
            // a note claiming the mock-up's line named a third option this build
            // did not have; System shipped, and the note outlived the fact —
            // leaving the one line a user reads to find out what the picker
            // offers naming two of its three items.
            Self::Theme => Text::DescTheme.text(),
            Self::Cursor => Text::DescCursor.text(),
            // What Off does and, just as much, what it does not do: the line has
            // to say "source" or a reader will expect the formula to vanish.
            Self::Formulas => Text::DescFormulas.text(),
            // Says "in command output" because that limit is the feature, not a
            // caveat about it: a `$…$` on the prompt or input line is never
            // typeset, and a user who reads only this line should not go away
            // expecting one to be.
            Self::InlineFormulas => Text::DescInlineFormulas.text(),
            // Says what Off *does* rather than what it hides, because what it
            // does is the reason to reach for it: no page, no chord, and no `git`
            // process started on your behalf.
            Self::GitPanel => Text::DescGitPanel.text(),
            // Mock-up 2361.
            Self::TabLayout => Text::DescTabLayout.text(),
            // Mock-up 2375.
            Self::Sidebar => Text::DescSidebar.text(),
            // Says *which* splits, because the scope is the setting. A line
            // reading "which way a pane splits" would promise to override the
            // two chords that draw their own rule and the picker's four zones,
            // and a user who then found `Alt+Shift+-` still stacking panes would
            // conclude the switch was broken.
            Self::SplitDirection => Text::DescSplitDirection.text(),
            // Mock-up 2468, word for word. It is also the *scope* of the setting
            // and the reason `profiles::index_of_id` does not read it: a tab and
            // a launch are the two things it answers for, and a pane coming back
            // off disk is neither.
            Self::DefaultProfile => Text::DescDefaultProfile.text(),
            // The one line in this dialog that describes *when* rather than
            // what, because "when" is the surprising half. See the variant.
            Self::Language => Text::DescLanguage.text(),
            // Says what it does *not* move, because "font" on a terminal's
            // settings page reads as "all the text" and the chrome keeps its
            // own face.
            Self::TerminalFont => Text::DescTerminalFont.text(),
            Self::FontSize => Text::DescFontSize.text(),
            // The folder is named on the dark half alone — see the two strings.
            Self::LightScheme => Text::DescLightScheme.text(),
            Self::DarkScheme => Text::DescDarkScheme.text(),
            // The one description in the dialog that reports a fact about the
            // machine rather than about the setting: which PSReadLine is
            // installed, and what that costs today. It is also where the reason
            // a greyed item is grey is written — see the variant.
            Self::PsReadLine => crate::psreadline::row_description(values.psreadline),
            Self::BackgroundImage => Text::DescBackgroundImage.text(),
            Self::ImageFit => Text::DescImageFit.text(),
            Self::ImageOpacity => Text::DescImageOpacity.text(),
            // The two rows that can be greyed whole say *why* on this line
            // instead of saying what they do — `psreadline::row_description`'s
            // ruling, generalised: there is one muted line under a title, and a
            // row that cannot act has exactly one thing worth putting on it. It
            // states the fact and does not apologise for it (user ruling
            // 2026-08-17: UI copy carries no editorial).
            Self::BackgroundOpacity => {
                if values.translucency_available {
                    Text::DescBackgroundOpacity.text()
                } else {
                    Text::DescBackgroundOpacityUnavailable.text()
                }
            }
            Self::Acrylic => {
                if values.acrylic_available {
                    Text::DescAcrylic.text()
                } else {
                    Text::DescAcrylicUnavailable.text()
                }
            }
            Self::AlwaysOnTop => Text::DescAlwaysOnTop.text(),
        }
    }

    /// Which of the dialog's two control forms this row answers with.
    ///
    /// **One answer, read by the layout, the hit test, the draw and the
    /// keyboard** — [`option_enabled`](Self::option_enabled)'s ruling applied to
    /// the control rather than to an item inside one. A row whose geometry said
    /// "slider" while its hit test said "picker" is a track you can drag and a
    /// menu that opens under your finger.
    #[must_use]
    pub fn control(self) -> SettingsControl {
        match self {
            Self::ImageOpacity => SettingsControl::Slider(SliderRange {
                // A picture can be turned all the way off without being
                // forgotten, which is what distinguishes this floor from the
                // ground's: somebody comparing two wallpapers wants 0 to mean
                // "show me the window without it", not "unset the row".
                min: 0,
                max: 100,
            }),
            Self::BackgroundOpacity => SettingsControl::Slider(SliderRange {
                min: bt_persist::MINIMUM_BACKGROUND_OPACITY,
                max: 100,
            }),
            _ => SettingsControl::Combo,
        }
    }

    /// Whether this machine can honour this row at all.
    ///
    /// `false` greys the row **whole** — title, sentence and control — and turns
    /// its description into the reason (see [`description`](Self::description)).
    /// That is the Shortcuts page's `reserved` line one surface over, and it is
    /// the same sentence: this is a thing you are being shown and cannot have.
    ///
    /// The row is still drawn and still a focus stop. Drawn, because the reason
    /// is the point — a row that vanished on Windows 10 would leave a reader
    /// hunting for a feature they had read about. A focus stop, because a ring
    /// is not an action: what the greying forbids is a control that *appears to
    /// act*, and both the hit test and `activate` refuse this one.
    #[must_use]
    pub fn available(self, values: SettingsValues) -> bool {
        match self {
            Self::Acrylic => values.acrylic_available,
            Self::BackgroundOpacity => values.translucency_available,
            _ => true,
        }
    }

    /// Where the ring goes and what a press lands on, for this row's control.
    #[must_use]
    pub fn control_target(self) -> SettingsTarget {
        match self.control() {
            SettingsControl::Combo => SettingsTarget::Combo(self),
            SettingsControl::Slider(_) => SettingsTarget::Slider(self),
        }
    }

    /// What a slider row currently reads, as a whole percentage — `None` for
    /// every row that is not a slider.
    #[must_use]
    pub fn slider_value(self, values: SettingsValues) -> Option<u8> {
        let range = self.control().range()?;
        let value = match self {
            Self::ImageOpacity => values.background_image_opacity,
            Self::BackgroundOpacity => values.background_opacity,
            _ => return None,
        };
        // Clamped on the way out, because a hand-edited `settings.json` is
        // allowed to say 7 (`bt_persist` deliberately stores what it was given)
        // and a thumb outside its own track is a thumb somewhere else entirely.
        Some(range.clamp(value))
    }

    /// How many items this row's picker holds.
    #[must_use]
    pub fn option_count(self) -> usize {
        match self {
            Self::Theme => THEME_OPTIONS.len(),
            Self::Cursor => CURSOR_OPTIONS.len(),
            Self::Formulas | Self::InlineFormulas | Self::GitPanel | Self::PsReadLine => {
                FORMULA_OPTIONS.len()
            }
            Self::TerminalFont => monospace_families().len(),
            Self::FontSize => FONT_SIZE_OPTIONS.len(),
            Self::LightScheme => scheme_labels(true).len(),
            Self::DarkScheme => scheme_labels(false).len(),
            Self::TabLayout => TAB_LAYOUT_OPTIONS.len(),
            Self::Sidebar => SIDEBAR_OPTIONS.len(),
            Self::SplitDirection => SPLIT_DIRECTION_OPTIONS.len(),
            Self::Language => LANGUAGE_OPTIONS.len(),
            // The picker is built from the same list the `˅` menu is built from
            // (mock-up 7645: "the default-profile picker is built from the same
            // list the ⌄ menu uses"). Not a copy of it — the same table — so a
            // fifth profile appears in both surfaces or in neither.
            Self::DefaultProfile => profiles::PROFILES.len(),
            Self::BackgroundImage => IMAGE_SOURCE_OPTIONS.len(),
            Self::ImageFit => IMAGE_FIT_OPTIONS.len(),
            Self::Acrylic | Self::AlwaysOnTop => FORMULA_OPTIONS.len(),
            // A slider has no items. Zero rather than a refusal, because
            // `option_labels` is what the layout measures the control column
            // against, and the honest measurement of a control that never opens
            // a menu is no words at all.
            Self::ImageOpacity | Self::BackgroundOpacity => 0,
        }
    }

    /// Every word this row's picker will draw, for a caller that has to measure
    /// them before the geometry can be solved. The draw reads `option_label` for
    /// the same indices, so the set measured and the set drawn cannot drift.
    pub fn option_labels(self) -> impl Iterator<Item = &'static str> {
        (0..self.option_count()).filter_map(move |index| self.option_label(index))
    }

    /// The word an item wears, or `None` past the end of the row's options.
    fn option_label(self, index: usize) -> Option<&'static str> {
        match self {
            Self::Theme => THEME_OPTIONS.get(index).copied().map(theme_label),
            Self::Cursor => CURSOR_OPTIONS.get(index).copied().map(cursor_label),
            Self::Formulas | Self::InlineFormulas | Self::GitPanel | Self::PsReadLine => {
                FORMULA_OPTIONS.get(index).copied().map(on_off_label)
            }
            // `&'static str` out of a runtime string, without a leak per call:
            // the list is enumerated once into a `static`, which outlives every
            // caller by construction. See [`monospace_families`].
            Self::TerminalFont => monospace_families()
                .get(index)
                .map(|family| family.name.as_str()),
            Self::FontSize => FONT_SIZE_LABELS.get(index).copied(),
            // The same `OnceLock` answer the family picker gives, for the same
            // reason: a scheme's name is a runtime string and this signature is
            // `&'static str`. See [`scheme_labels`].
            Self::LightScheme => scheme_labels(true).get(index).copied(),
            Self::DarkScheme => scheme_labels(false).get(index).copied(),
            Self::TabLayout => TAB_LAYOUT_OPTIONS.get(index).copied().map(tab_layout_label),
            Self::Sidebar => SIDEBAR_OPTIONS.get(index).copied().map(sidebar_label),
            Self::SplitDirection => SPLIT_DIRECTION_OPTIONS
                .get(index)
                .copied()
                .map(split_direction_label),
            Self::Language => LANGUAGE_OPTIONS.get(index).copied().map(language_label),
            Self::DefaultProfile => {
                (index < profiles::PROFILES.len()).then(|| profiles::title(index))
            }
            Self::BackgroundImage => IMAGE_SOURCE_OPTIONS
                .get(index)
                .copied()
                .map(image_source_label),
            Self::ImageFit => IMAGE_FIT_OPTIONS.get(index).copied().map(image_fit_label),
            Self::Acrylic | Self::AlwaysOnTop => {
                FORMULA_OPTIONS.get(index).copied().map(on_off_label)
            }
            Self::ImageOpacity | Self::BackgroundOpacity => None,
        }
    }

    /// The mark an item wears, for the one row whose items have one.
    ///
    /// Mock-up 7647: `<span class="tick">✓</span><span class="ticon">${p.icon}</span>${p.title}`
    /// — and it is the only combo item in the whole dialog with a `.ticon`, which
    /// is why this returns `Option` from a row rather than being a field every
    /// option list has to fill in with `None`.
    ///
    /// It is the profile's own mark and not a generic shell glyph, for the reason
    /// `UI-UX.md:115` gives about every other surface: you recognise PowerShell by
    /// that blue, not by reading a word. A picker that named four shells in text
    /// alone would be the one place in the product where they are not marks.
    #[must_use]
    pub fn option_mark(self, index: usize) -> Option<ChromeMark> {
        match self {
            Self::DefaultProfile => profiles::PROFILES.get(index).map(|profile| profile.mark),
            Self::SplitDirection => SPLIT_DIRECTION_OPTIONS
                .get(index)
                .copied()
                .map(split_direction_mark),
            _ => None,
        }
    }

    /// Whether this item can be chosen on this machine.
    ///
    /// **Answered once, and read by both the hit test and the draw** — the ruling
    /// `profiles::hit` already made for the `˅` menu's greyed rows, and it is the
    /// same ruling because it is the same failure: a rule spelled only at the
    /// click lights the row under the pointer and then does nothing; a rule
    /// spelled only at the draw leaves the item dark and still selectable.
    #[must_use]
    pub fn option_enabled(self, index: usize, values: SettingsValues) -> bool {
        match self {
            Self::DefaultProfile => values
                .profile_available
                .get(index)
                .copied()
                .unwrap_or(false),
            // The second row whose items can be unavailable, and the only one
            // where choosing an unavailable item would *do* something: `On`
            // under a refusing execution policy writes files no shell will load,
            // and `Off` on a module Folio did not write deletes somebody else's
            // directory. Answered once, read by the hit test and the draw alike.
            Self::PsReadLine => match FORMULA_OPTIONS.get(index).copied() {
                Some(true) => values.psreadline_install_available,
                Some(false) => values.psreadline_remove_available,
                None => false,
            },
            _ => index < self.option_count(),
        }
    }

    /// Which of this row's options the app is currently in.
    fn selected_index(self, values: SettingsValues) -> Option<usize> {
        match self {
            Self::Theme => THEME_OPTIONS.iter().position(|it| *it == values.theme),
            Self::Cursor => CURSOR_OPTIONS.iter().position(|it| *it == values.cursor),
            Self::Formulas => FORMULA_OPTIONS
                .iter()
                .position(|it| *it == values.display_formulas),
            Self::InlineFormulas => FORMULA_OPTIONS
                .iter()
                .position(|it| *it == values.inline_formulas),
            Self::GitPanel => FORMULA_OPTIONS
                .iter()
                .position(|it| *it == values.git_panel),
            Self::TabLayout => TAB_LAYOUT_OPTIONS
                .iter()
                .position(|it| *it == values.tab_layout),
            Self::Sidebar => SIDEBAR_OPTIONS.iter().position(|it| *it == values.sidebar),
            Self::SplitDirection => SPLIT_DIRECTION_OPTIONS
                .iter()
                .position(|it| *it == values.split_direction),
            Self::Language => LANGUAGE_OPTIONS
                .iter()
                .position(|it| *it == values.language),
            Self::TerminalFont => Some(values.terminal_font),
            Self::FontSize => Some(values.font_size),
            Self::LightScheme => Some(values.light_scheme),
            Self::DarkScheme => Some(values.dark_scheme),
            // The *state of the machine*, not a stored preference: what the row
            // shows ticked is whether the module is on disk, which is what a
            // reader is actually asking. `settings.json`'s own field records
            // whether they were asked, which is a different question and has no
            // picker.
            Self::PsReadLine => FORMULA_OPTIONS.iter().position(|it| {
                *it == (values.psreadline == crate::psreadline::RowState::InstalledByFolio)
            }),
            // The *resolved* default, which is why the caller hands over an index
            // rather than the stored id. **Mock-up bug not copied** (2471): its
            // combo button is born with the literal text `PowerShell` and only
            // ever updates when the user picks something, so a default that was
            // not index 0 showed stale words until touched. Reading state is the
            // whole of the fix, and it is free here because there is no second
            // place holding the button's caption.
            Self::DefaultProfile => Some(values.default_profile),
            // Ticked on what the window is actually doing: `None` is ticked
            // while no picture is named, and neither item is ticked once one
            // is — because the answer then is the file itself, and the button is
            // carrying its name.
            Self::BackgroundImage => (!values.background_image)
                .then(|| {
                    IMAGE_SOURCE_OPTIONS
                        .iter()
                        .position(|it| *it == ImageSource::None)
                })
                .flatten(),
            Self::ImageFit => Some(values.background_fit),
            Self::Acrylic => FORMULA_OPTIONS.iter().position(|it| *it == values.acrylic),
            Self::AlwaysOnTop => FORMULA_OPTIONS
                .iter()
                .position(|it| *it == values.always_on_top),
            Self::ImageOpacity | Self::BackgroundOpacity => None,
        }
    }
}

/// Which rows the dialog holds while the tabs run on this axis, in the order
/// they are stacked — grouped, with each group's rows together.
///
/// **The one place the conditional row is stated** (mock-up 5644:
/// `$("row-railmode").style.display = state.layoutMode === "vertical" ? "" :
/// "none"`). The height, the stacking, the hit test, the draw *and the headings*
/// all read the list this returns, so none of them carries a copy of the rule and
/// none of them can be forgotten when it changes.
///
/// Rows of one category must stay contiguous here: the heading is derived from
/// where [`SettingsRow::category`] changes as a page's list is walked, so a row
/// filed out of order would head its page twice.
#[must_use]
pub fn visible_rows(tab_layout: TabLayoutMode) -> Vec<SettingsRow> {
    let mut rows = vec![
        SettingsRow::Theme,
        SettingsRow::LightScheme,
        SettingsRow::DarkScheme,
        // The window's ground, immediately under the pair that says what colour
        // it is (user ruling 2026-08-17, §7.1.6c-4b) — the rest of the same
        // sentence, read downwards: is there a picture, how does it meet the
        // window, how much of it comes through, how much of the desktop comes
        // through behind it, is that blurred, and does the window stay in front.
        //
        // Unconditional, unlike `Sidebar`: the two rows a machine may not be
        // able to honour are greyed rather than dropped, because the reason is
        // what a reader came for — see `SettingsRow::available`.
        SettingsRow::BackgroundImage,
        SettingsRow::ImageFit,
        SettingsRow::ImageOpacity,
        SettingsRow::BackgroundOpacity,
        SettingsRow::Acrylic,
        SettingsRow::AlwaysOnTop,
        SettingsRow::Cursor,
        SettingsRow::TabLayout,
    ];
    if tab_layout == TabLayoutMode::Vertical {
        rows.push(SettingsRow::Sidebar);
    }
    rows.push(SettingsRow::SplitDirection);
    rows.push(SettingsRow::TerminalFont);
    rows.push(SettingsRow::FontSize);
    rows.push(SettingsRow::Formulas);
    rows.push(SettingsRow::InlineFormulas);
    rows.push(SettingsRow::Language);
    rows.push(SettingsRow::GitPanel);
    rows.push(SettingsRow::DefaultProfile);
    rows.push(SettingsRow::PsReadLine);
    rows
}

/// **Everything the dialog is holding this frame**: the rows it could show, and
/// the shortcut table's own lines.
///
/// One parameter instead of two everywhere, and a borrow instead of a copy,
/// because the shortcut lines are strings derived from a table that changes as
/// the user edits it. It is handed to the layout, the focus walk and the draw
/// alike so that all three read one description of the dialog's contents — the
/// same reason [`SettingsValues`] is passed in rather than fetched.
#[derive(Clone, Copy, Debug)]
pub struct SettingsContent<'a> {
    /// Every row the dialog would show across all its pages, in order.
    pub rows: &'a [SettingsRow],
    /// The shortcut page's own lines, folded and tagged by `shortcuts.rs`.
    pub shortcuts: &'a [crate::shortcuts::ShortcutRow],
}

impl SettingsContent<'_> {
    /// The rows of one page, in the order [`visible_rows`] put them.
    #[must_use]
    pub fn page_rows(&self, category: SettingsCategory) -> Vec<SettingsRow> {
        self.rows
            .iter()
            .copied()
            .filter(|row| row.category() == category)
            .collect()
    }

    /// **The rail**, derived: every category with something on its page, in
    /// [`SettingsCategory::ALL`]'s order.
    ///
    /// This is where the placeholder ruling actually lives, and `Terminal` is
    /// the proof it works: it had a variant, a heading and a page for two slices
    /// and did not appear, because it had no rows — and it appeared the moment
    /// the PSReadLine row gave it one, with nothing else in this file touched.
    /// `Shortcuts` is always here because its page is a table this build ships,
    /// never empty.
    #[must_use]
    pub fn nav_items(&self) -> Vec<SettingsCategory> {
        SettingsCategory::ALL
            .into_iter()
            .filter(|category| self.has_content(*category))
            .collect()
    }

    /// Whether a category has anything to show.
    #[must_use]
    pub fn has_content(&self, category: SettingsCategory) -> bool {
        match category {
            SettingsCategory::Shortcuts => !self.shortcuts.is_empty(),
            other => self.rows.iter().any(|row| row.category() == other),
        }
    }

    /// The category a dialog opened now would land on: the first the rail holds.
    ///
    /// Asked rather than assumed, because `General` is only the answer while
    /// `General` has rows — and the whole point of deriving the rail is that a
    /// build which took the last one away must not open onto a blank page.
    #[must_use]
    pub fn first_category(&self) -> SettingsCategory {
        self.nav_items()
            .first()
            .copied()
            .unwrap_or(SettingsCategory::Shortcuts)
    }
}

/// What every row in the dialog currently reads.
///
/// Passed in rather than fetched, so `build` is a function of its arguments and
/// a row's drawn value cannot disagree with the value the caller is about to
/// persist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsValues {
    pub theme: ThemeModeV1,
    pub cursor: CursorStyle,
    pub tab_layout: TabLayoutMode,
    pub sidebar: RailMode,
    pub display_formulas: bool,
    pub inline_formulas: bool,
    /// Whether the Files column offers its Git page at all.
    pub git_panel: bool,
    /// Which way a split with no direction of its own cuts.
    pub split_direction: SplitDirectionV1,
    /// Which language the window writes in — **the stored mode**, not the
    /// resolved language.
    ///
    /// The row has to show what the file says: a user who picked `System` on a
    /// Chinese Windows must see the tick on `System`, not on the Chinese item.
    /// The resolved answer lives in [`crate::i18n::current`] and is a different
    /// question.
    pub language: LanguageV1,
    /// The resolved default profile — an index into `profiles::PROFILES`, never
    /// the stored id.
    ///
    /// The dialog shows what will actually happen. If the file names a profile
    /// this machine no longer has, `profiles::default_profile` has already
    /// degraded it and the tick sits on the shell the `+` would really start —
    /// which is what the row's own description promises. The *stored* id is left
    /// alone by that degradation, so nothing here can quietly consume a choice
    /// the user made before they uninstalled something.
    pub default_profile: usize,
    /// Which row of the family picker is ticked — an index, resolved against
    /// this machine's list, never the stored name.
    ///
    /// [`SettingsValues::default_profile`]'s ruling for the same reason: the
    /// dialog shows what is actually being drawn, so a family that has been
    /// uninstalled since it was chosen ticks the face the grid really has. The
    /// stored name is left alone by that resolution.
    pub terminal_font: usize,
    /// Which row of the size picker is ticked. An index for `terminal_font`'s
    /// reason — a size this build's list does not offer resolves to the default
    /// rather than leaving the combo blank.
    pub font_size: usize,
    /// Which row of the light scheme picker is ticked — an index into
    /// [`scheme_labels(true)`](scheme_labels), never the stored name.
    ///
    /// [`SettingsValues::terminal_font`]'s ruling a third time: a scheme whose
    /// file has been deleted since it was chosen ticks the scheme the window is
    /// actually drawn in, which is Folio's own. The stored name is left alone by
    /// that resolution, so moving a file out of the folder and back does not
    /// consume the choice.
    pub light_scheme: usize,
    /// The same for the dark picker.
    pub dark_scheme: usize,
    /// What the PSReadLine row is describing, reconciled from the out-of-band
    /// probe and the module actually on disk.
    pub psreadline: crate::psreadline::RowState,
    /// Whether the row's `On` item can be chosen on this machine.
    pub psreadline_install_available: bool,
    /// Whether its `Off` item can be — true only for a module Folio wrote.
    pub psreadline_remove_available: bool,
    /// Which profiles this machine can start, in table order.
    ///
    /// An array rather than a borrowed `ProfilePrograms`, so this type stays
    /// `Copy` and `build`/`hit` can keep taking it by value. Four bools is the
    /// whole of what those two need to know about the filesystem.
    pub profile_available: [bool; profiles::PROFILES.len()],
    /// Whether a picture is named — **not which one**. The name is a path and
    /// this struct is `Copy` + `Eq`; the button's caption comes through
    /// [`build`]'s own `background_image` argument, which is where a borrowed
    /// string belongs.
    pub background_image: bool,
    /// Which row of the fit picker is ticked — an index, never the stored enum,
    /// for `terminal_font`'s reason.
    pub background_fit: usize,
    /// How much of the picture reaches the window, 0–100 whole percent.
    pub background_image_opacity: u8,
    /// How much of the ground reaches the window, 30–100 whole percent.
    pub background_opacity: u8,
    pub acrylic: bool,
    pub always_on_top: bool,
    /// Whether this Windows knows what a system backdrop is
    /// (`bt_platform::system_backdrop_available`). `false` greys the Acrylic row
    /// whole and turns its sentence into the reason.
    pub acrylic_available: bool,
    /// Whether this window's surface is composited with premultiplied alpha —
    /// the thing a translucent ground is made of (`docs/DESIGN.md` §2.3 A2).
    ///
    /// Read from the renderer's own `alpha_report`, not assumed: the row's state
    /// has to be a fact about the surface that was actually configured, and the
    /// day a second window target appears is the day an assumption here would
    /// draw a slider that moves and changes nothing.
    pub translucency_available: bool,
}

#[cfg(test)]
impl SettingsValues {
    /// A representative reading of every row, for a test that is about the
    /// dialog's geometry or its scrim rather than about a value.
    ///
    /// Here rather than in this module's `mod tests` because [`hit`] now takes
    /// one, so a test in *another* module that presses on the dialog needs one
    /// too — and a second hand-written literal over there is a second place to
    /// forget a field when a row is added.
    #[must_use]
    pub fn sample() -> Self {
        Self {
            theme: ThemeModeV1::Dark,
            cursor: CursorStyle::Bar,
            tab_layout: TabLayoutMode::Horizontal,
            sidebar: RailMode::Expanded,
            display_formulas: true,
            inline_formulas: true,
            git_panel: true,
            split_direction: SplitDirectionV1::Auto,
            language: LanguageV1::System,
            default_profile: profiles::FALLBACK_PROFILE,
            terminal_font: 0,
            font_size: font_size_index(bt_persist::DEFAULT_TERMINAL_FONT_SIZE),
            light_scheme: scheme_index(bt_persist::DEFAULT_LIGHT_SCHEME, true),
            dark_scheme: scheme_index(bt_persist::DEFAULT_DARK_SCHEME, false),
            psreadline: crate::psreadline::RowState::Outdated,
            psreadline_install_available: true,
            psreadline_remove_available: false,
            // A fully equipped machine, so a geometry test is not quietly also a
            // test of what is installed on the one running it.
            profile_available: [true; profiles::PROFILES.len()],
            background_image: false,
            background_fit: image_fit_index(BackgroundFitV1::default()),
            background_image_opacity: bt_persist::DEFAULT_BACKGROUND_IMAGE_OPACITY,
            background_opacity: bt_persist::DEFAULT_BACKGROUND_OPACITY,
            acrylic: false,
            always_on_top: false,
            // Both capabilities present, for `profile_available`'s reason: a
            // geometry test must not quietly become a test of which Windows the
            // suite is running on. The tests that are *about* the greying inject
            // `false` themselves.
            acrylic_available: true,
            translucency_available: true,
        }
    }
}

/// Whether the dialog is up, and what is open inside it.
///
/// App state and nothing else: it is not a seat, so the solver never sees it;
/// it is not an intent, so the session file never sees it. A dialog that
/// survived a restart would be a window that opens with a question on it.
/// **Not `Copy` since the recorder arrived**, which is worth a line because
/// every caller used to take it by value. A capture in progress carries the caps
/// it is showing and the sentence it is refusing with, and both are text; a
/// `Copy` state would have meant keeping the recorder's words somewhere else,
/// which is two places holding one moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsPanel {
    open: bool,
    /// **Which page is up** (user ruling Q3 = A, 2026-08-17). One page per
    /// category, so this is the whole of "where am I" — and it is state on the
    /// panel rather than on the runtime because the panel is what a key press
    /// moves, and a key press that changed a page recorded somewhere else would
    /// be a walk with two owners.
    ///
    /// It is not persisted, for the reason nothing else here is: a dialog does
    /// not survive a restart, and a dialog that reopened on the page you left is
    /// a dialog that opens somewhere you have to read before you can act.
    category: SettingsCategory,
    /// The row the recorder is listening for, and what it has heard.
    recording: Option<Recording>,
    /// Which row's picker is open. Nested state, because Esc unwinds one layer
    /// per press (§7.1.5) and "the menu is open" is the top layer.
    menu: Option<SettingsRow>,
    hover: Option<SettingsTarget>,
    /// **Which control the keyboard is on** — the dialog's own focus, and the
    /// half of `InputOwner::Dialog` that did not exist until this slice.
    ///
    /// The window-level owner (`main.rs`'s `KeyboardOwner`/`ImeOwner::Modal`)
    /// answers "do keys go to the dialog"; it never answered "and where in it",
    /// so until now the dialog swallowed every key but Esc and had nowhere for
    /// one to land. This field is the answer to the second question and lives
    /// here rather than up there because it is a fact about *this* dialog's
    /// contents, which is also what makes it testable without a window.
    ///
    /// `None` while the dialog is shut. Opening it seats the focus (see
    /// [`Self::toggle`]), so an open dialog always has one.
    focus: Option<SettingsTarget>,
    /// **Whether the ring is drawn** — the web's `:focus-visible` heuristic,
    /// which this dialog owes a native equivalent of because the mock-up's ring
    /// (`button:focus-visible`, 2205) is spelled in exactly those terms.
    ///
    /// The rule that heuristic encodes: a ring answers "where will the next key
    /// go", which is a question only somebody using the keyboard is asking.
    /// Ringing a button somebody just *clicked* tells them where their own
    /// finger was. So a pointer press moves the focus with the ring off, and any
    /// key press turns it on until the next pointer press.
    focus_visible: bool,
}

/// A capture in progress: which line is listening, what it is showing, and what
/// it is refusing.
///
/// **The chord is held pending rather than taken on the spot** (S64's shape,
/// worked through). A recorder that bound the first complete chord it saw would
/// have no state left in which to *refuse* one — and refusing is the point: the
/// AltGr zone and the shell's control alphabet are both cases where the user
/// pressed something real and must be told why it cannot be theirs, without
/// their old chord having been thrown away in the meantime. So a press produces
/// a candidate, a refusal replaces the candidate with a reason, and `Enter`
/// commits whatever is standing.
///
/// The cost is written down where it is paid, in `shortcuts::RecordedKey`: bare
/// `Esc`, `Backspace`, `Delete` and `Enter` are this state's own verbs and
/// cannot be recorded bare.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Recording {
    /// Which line of the shortcut page is listening, by its index in
    /// [`SettingsContent::shortcuts`].
    row: usize,
    /// The caps the box is showing — modifiers alone while the keys are going
    /// down, the whole chord once one has arrived.
    caps: Vec<String>,
    /// The chord `Enter` would take, if there is one.
    candidate: Option<crate::shortcuts::Chord>,
    /// Why the last press was refused, if it was.
    hint: Option<String>,
}

impl SettingsPanel {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn menu(&self) -> Option<SettingsRow> {
        self.menu
    }

    /// Which page is up.
    #[must_use]
    pub fn category(&self) -> SettingsCategory {
        self.category
    }

    /// Which line of the shortcut page is listening for a chord, if one is.
    #[must_use]
    pub fn recording_row(&self) -> Option<usize> {
        self.recording.as_ref().map(|capture| capture.row)
    }

    /// **Which** row is listening, what it is showing, and the reason the last
    /// press was refused.
    ///
    /// The row is carried here rather than read off the focus, and that is not
    /// redundancy — it is a bug found on the real window: a capture started by a
    /// *pointer* has no ring at all (`:focus-visible` puts it away), so a draw
    /// that asked the ring which row was listening left a box swallowing every
    /// key while looking exactly like one that was not.
    #[must_use]
    pub fn recording_state(&self) -> Option<(usize, &[String], Option<&str>)> {
        self.recording.as_ref().map(|capture| {
            (
                capture.row,
                capture.caps.as_slice(),
                capture.hint.as_deref(),
            )
        })
    }

    /// Turn to a page, answering whether that changed anything.
    ///
    /// The focus lands on the page's first control rather than staying on the
    /// rail, and that is deliberate: choosing a category is choosing to look at
    /// what is on it — no. It stays on the rail item, because the rail is where
    /// the arrows just were and a focus that jumped out of the list a user is
    /// walking would make the second `↓` land somewhere they were not looking.
    /// `→`, `Tab` and `Enter` are the three ways to say "now take me in".
    pub fn select_category(&mut self, category: SettingsCategory) -> bool {
        if self.category == category {
            return false;
        }
        self.category = category;
        self.menu = None;
        self.recording = None;
        self.focus = Some(SettingsTarget::Nav(category));
        true
    }

    /// The gear: open when shut, shut when open. Closing takes the menu with it.
    ///
    /// **Opening seats the focus on the first row's control, not on the `×`.**
    /// The visual order puts the close first (it is in the header, above
    /// everything), and Tab walks that order — but the row a user came for is
    /// the first row, and a dialog that opens with the keyboard parked on
    /// "leave" has spent its one free position on the verb nobody opened it to
    /// reach. The ring is off, because a gear pressed with the mouse is not a
    /// keyboard interaction; the first key turns it on.
    ///
    /// **And it opens on the first page**, which is `General` while `General`
    /// has rows — not on the page it was left on. A dialog that reopened where
    /// you left it is one you have to read before you can act, and the point of
    /// a rail is that the way back is one word away.
    pub fn toggle(&mut self, content: SettingsContent<'_>) {
        self.open = !self.open;
        self.menu = None;
        self.hover = None;
        self.recording = None;
        self.focus_visible = false;
        self.category = content.first_category();
        self.focus = self
            .open
            .then(|| page_order(content, self.category).first().copied())
            .flatten();
    }

    /// Close the top-most open layer and report whether there was one — the Esc
    /// route of §7.1.5, which unwinds exactly one layer per press.
    ///
    /// The picker is a rung of its own here as it is in the mock-up's own Esc
    /// ladder (6184-6187: `.combo.open`, then the dialog), and closing it hands
    /// the keyboard back to the button it hangs under rather than dropping the
    /// focus on the floor.
    ///
    /// **A capture is the newest rung and it is above the picker**, because it
    /// is the innermost thing that can be open: while the recorder is listening
    /// the whole keyboard is its, so the one key a user has left to say "not
    /// that" must undo the listening and nothing else. §7.1.5's ladder gains a
    /// step rather than growing a second ladder.
    pub fn close_one_layer(&mut self) -> bool {
        if let Some(capture) = self.recording.take() {
            self.hover = None;
            self.focus = Some(SettingsTarget::Record(capture.row));
            return true;
        }
        if let Some(row) = self.menu {
            self.menu = None;
            self.hover = None;
            self.focus = Some(SettingsTarget::Combo(row));
            return true;
        }
        if self.open {
            self.close();
            return true;
        }
        false
    }

    /// Shut everything, whatever was open.
    pub fn close(&mut self) {
        self.open = false;
        self.menu = None;
        self.hover = None;
        self.recording = None;
        self.focus = None;
        self.focus_visible = false;
    }

    /// Shut whichever picker is open, leaving the dialog up — what choosing an
    /// item does, and the only direction the runtime ever asks for.
    pub fn close_menu(&mut self) {
        if let Some(row) = self.menu.take() {
            self.focus = Some(SettingsTarget::Combo(row));
        }
    }

    pub fn toggle_menu(&mut self, row: SettingsRow) {
        self.menu = (self.menu != Some(row)).then_some(row);
    }

    /// Returns whether the hover changed, so a caller can skip a repaint.
    pub fn set_hover(&mut self, hover: Option<SettingsTarget>) -> bool {
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(&self) -> Option<SettingsTarget> {
        self.hover
    }

    /// Where the keyboard is, whether or not the ring is showing.
    #[must_use]
    pub fn focus(&self) -> Option<SettingsTarget> {
        self.focus
    }

    /// Where the **ring** is drawn, which is the focus only while it was reached
    /// by keyboard. The one reader is the draw, so the heuristic is stated once.
    #[must_use]
    pub fn focus_ring(&self) -> Option<SettingsTarget> {
        self.focus_visible.then_some(self.focus).flatten()
    }

    /// A pointer press: the focus goes to what was pressed, with the ring off.
    ///
    /// Every press the dialog takes comes through here, including the ones that
    /// close it — the scrim and the panel body move the focus nowhere, but they
    /// are still pointer interaction and still put the ring away, which is the
    /// half of `:focus-visible` a naive "set focus on click" would miss.
    pub fn press(&mut self, target: SettingsTarget) {
        self.focus_visible = false;
        // **A capture ends where the finger goes**, unless the finger is on the
        // very button that is listening. The recorder takes every key while it
        // is open, so a capture the pointer had walked away from would be a
        // dialog silently eating the keyboard on behalf of a row that has
        // stopped saying it is listening — the state and what is drawn come
        // apart, and only one of them is visible.
        if self
            .recording
            .as_ref()
            .is_some_and(|capture| target != SettingsTarget::Record(capture.row))
        {
            self.recording = None;
        }
        match target {
            SettingsTarget::Close
            | SettingsTarget::Combo(_)
            | SettingsTarget::Slider(_)
            | SettingsTarget::Nav(_)
            | SettingsTarget::Record(_)
            | SettingsTarget::RestoreRow(_)
            | SettingsTarget::RestoreAll => self.focus = Some(target),
            // An item's own row is what the keyboard lands on: the menu is about
            // to close, and a focus naming an item of a shut picker names
            // nothing.
            SettingsTarget::Choice(row, _) | SettingsTarget::Menu(row) => {
                self.focus = Some(SettingsTarget::Combo(row));
            }
            SettingsTarget::Scrim | SettingsTarget::Panel => {}
        }
    }

    /// Put the focus back on something the dialog still holds.
    ///
    /// The row list is conditional ([`visible_rows`]), so choosing `Horizontal`
    /// in the Tab layout picker deletes the Sidebar row — and the focus may be
    /// standing on it. Called by the runtime after any press that could change
    /// the list, and again at the top of [`Self::key`], because a focus naming a
    /// row that is gone draws no ring and answers no key: the dialog would look
    /// like it had swallowed the keyboard.
    pub fn keep_focus_reachable(&mut self, content: SettingsContent<'_>) {
        if !self.open {
            return;
        }
        // A page that has lost every row goes with them: the rail derives itself
        // from the rows, so a category the dialog is standing on can stop
        // existing under it.
        if !content.has_content(self.category) {
            self.category = content.first_category();
            self.menu = None;
            self.recording = None;
            self.focus = None;
        }
        let rows = content.page_rows(self.category);
        // **An open picker's own item is a legal place for the focus and is not
        // in the Tab order**, which is a list of the dialog's controls: while a
        // picker is up the keyboard is *inside* one of them. Checked against the
        // picker that is actually open, so an item of a shut one is still
        // unreachable.
        if let (Some(menu), Some(SettingsTarget::Choice(row, index))) = (self.menu, self.focus)
            && menu == row
            && rows.contains(&row)
            && index < row.option_count()
        {
            return;
        }
        let order = focus_order(content, self.category);
        if self.focus.is_some_and(|focus| order.contains(&focus)) {
            return;
        }
        // A picker open on a row that just vanished goes with it, and so does a
        // capture on a line that has.
        if self.menu.is_some_and(|row| !rows.contains(&row)) {
            self.menu = None;
        }
        if self
            .recording
            .as_ref()
            .is_some_and(|capture| capture.row >= content.shortcuts.len())
        {
            self.recording = None;
        }
        self.focus = order.first().copied();
    }
}

/// One key press, in the dialog's own vocabulary.
///
/// The runtime maps winit's keys onto this so that the whole focus model is a
/// pure function of an enum — the same division `hit` runs on, and the only way
/// "Shift+Tab from the first control wraps to the last" is a property rather
/// than something you check by opening the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsKey {
    /// Tab, or Shift+Tab when `backwards`.
    Tab {
        backwards: bool,
    },
    Down,
    Up,
    /// The two that cross between the rail and the page.
    ///
    /// A rail is a list you walk with `↑`/`↓` and step *out of* sideways — the
    /// tablist shape every settings dialog on this platform uses, and the reason
    /// the rail is one Tab stop rather than five. `←` and `→` are what make that
    /// true of the keyboard: without them the only way back to the rail from the
    /// middle of a page would be to Tab through everything under it.
    Left,
    Right,
    Home,
    End,
    /// Enter or Space — "press the thing the ring is on".
    Activate,
    Escape,
    /// Every other key. It does nothing and is still swallowed, but it is a
    /// keyboard interaction, so it turns the ring on.
    Other,
}

/// Which of a slider's four keys was pressed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SliderKey {
    /// `←` or `→`, one [`SLIDER_STEP_PERCENT`] in that direction.
    Step(i16),
    /// `End` (`true`) or `Home` (`false`) — the ends of the range, which for the
    /// ground's slider is its floor and not zero.
    End(bool),
}

/// What a key press did, for a runtime that has to repaint, scroll or persist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsKeyVerdict {
    /// Nothing moved. The press is consumed all the same: a modal owns the
    /// keyboard, and a key it has no verb for is a key that must not reach the
    /// terminal behind the scrim.
    Inert,
    /// The focus, the ring or the open picker changed — repaint, and bring the
    /// focused row into view.
    Moved,
    /// The focused item was chosen. The runtime runs exactly the side effects a
    /// *press* on this target runs, because it is the same press arriving by a
    /// different road.
    Chose(SettingsTarget),
    /// The last layer unwound and the dialog is now shut.
    Closed,
    /// A slider was driven from the keyboard to a new whole percentage
    /// (§7.1.6c-4b).
    ///
    /// Its own verdict rather than `Chose`, because the two are different
    /// events: `Chose` names a *target* the runtime presses, and this names a
    /// *value* the runtime stores. Folding a slider into `Chose(Choice(row, n))`
    /// would mean inventing an option index for a control that has no options,
    /// and every reader of that index would then have to know it was a
    /// percentage in disguise.
    Adjusted(SettingsRow, u8),
}

impl SettingsKeyVerdict {
    /// `Moved` or `Inert`, from whether anything actually moved.
    const fn from_moved(moved: bool) -> Self {
        if moved { Self::Moved } else { Self::Inert }
    }
}

impl SettingsPanel {
    /// Drive the dialog from one key.
    ///
    /// **The Esc ladder is one rung deeper than the pointer's** and that is the
    /// mock-up's own order (6184-6187): a press closes the open picker and
    /// leaves the dialog up, the next closes the dialog. `close_one_layer` is
    /// still the thing that does it, so there is one ladder rather than two.
    ///
    /// Inside an open picker the arrows move the *option* and skip the ones this
    /// machine cannot honour — the same [`SettingsRow::option_enabled`] answer
    /// the hit test and the draw read, because an item the pointer is refused is
    /// an item the keyboard must be refused too. Outside one they move the
    /// focus, which is the same walk Tab makes: one order, two spellings.
    pub fn key(
        &mut self,
        key: SettingsKey,
        content: SettingsContent<'_>,
        values: SettingsValues,
    ) -> SettingsKeyVerdict {
        if !self.open {
            return SettingsKeyVerdict::Inert;
        }
        self.keep_focus_reachable(content);
        // Every key press is keyboard interaction, including the ones with no
        // verb here: `:focus-visible`'s heuristic is about the *input device*,
        // not about whether the key happened to be bound.
        let ring_appeared = !self.focus_visible;
        self.focus_visible = true;
        let moved = match key {
            SettingsKey::Escape => {
                return if self.close_one_layer() {
                    if self.open {
                        SettingsKeyVerdict::Moved
                    } else {
                        SettingsKeyVerdict::Closed
                    }
                } else {
                    SettingsKeyVerdict::Inert
                };
            }
            SettingsKey::Activate => return self.activate(content, values),
            SettingsKey::Tab { backwards } => {
                // Tab leaves an open picker rather than walking inside it — a
                // `<select>`'s own behaviour, and the only reading that keeps
                // "one popup at a time" (mock-up 5102) true of the keyboard.
                self.close_menu();
                self.step_focus(content, if backwards { -1 } else { 1 })
            }
            // **A focused slider owns `←`/`→` and `Home`/`End`.** Everywhere
            // else in this dialog `←` means "back to the rail" and `→` means
            // "into the page", and that stays true — but a track under the ring
            // is a control whose whole vocabulary is those four keys, and a
            // slider that could only be dragged would be the one control here
            // the keyboard cannot reach. It is the same shape the open picker
            // already has: while you are *inside* a control, the arrows are the
            // control's. Tab and Shift+Tab still leave, which is why nothing
            // becomes unreachable.
            SettingsKey::Left => match self.slider_key(values, SliderKey::Step(-1)) {
                Some(verdict) => return verdict,
                None => self.step_out_of_page(content),
            },
            SettingsKey::Right => match self.slider_key(values, SliderKey::Step(1)) {
                Some(verdict) => return verdict,
                None => self.step_into_page(content),
            },
            SettingsKey::Home => match self.slider_key(values, SliderKey::End(false)) {
                Some(verdict) => return verdict,
                None => self.jump(content, values, false),
            },
            SettingsKey::End => match self.slider_key(values, SliderKey::End(true)) {
                Some(verdict) => return verdict,
                None => self.jump(content, values, true),
            },
            SettingsKey::Down => self.step(content, values, 1),
            SettingsKey::Up => self.step(content, values, -1),
            SettingsKey::Other => false,
        };
        if moved || ring_appeared {
            SettingsKeyVerdict::Moved
        } else {
            SettingsKeyVerdict::Inert
        }
    }

    /// Whether the ring is standing on a rail item.
    fn on_nav(&self) -> bool {
        matches!(self.focus, Some(SettingsTarget::Nav(_)))
    }

    /// One of the four keys a focused slider owns, or `None` when the ring is
    /// not on a slider and the key means what it always meant.
    ///
    /// `Some(Inert)` and not `None` for a value that did not move: the key was
    /// the slider's, it simply had nowhere further to go, and letting it fall
    /// through would tip the focus out of a control the user is still driving
    /// the moment they reached its end.
    fn slider_key(&self, values: SettingsValues, key: SliderKey) -> Option<SettingsKeyVerdict> {
        if self.menu.is_some() {
            return None;
        }
        let Some(SettingsTarget::Slider(row)) = self.focus else {
            return None;
        };
        let range = row.control().range()?;
        if !row.available(values) {
            return Some(SettingsKeyVerdict::Inert);
        }
        let current = row.slider_value(values)?;
        let next = match key {
            SliderKey::Step(delta) => range.stepped(current, delta),
            SliderKey::End(true) => range.max,
            SliderKey::End(false) => range.min,
        };
        Some(if next == current {
            SettingsKeyVerdict::Inert
        } else {
            SettingsKeyVerdict::Adjusted(row, next)
        })
    }

    /// `→` from the rail: into the page, onto its first control.
    ///
    /// It answers nothing anywhere else. `→` on a picker button is not "open it"
    /// — that is Enter's, and a second spelling would make `←` ambiguous between
    /// "close the picker" and "go back to the rail".
    fn step_into_page(&mut self, content: SettingsContent<'_>) -> bool {
        if !self.on_nav() || self.menu.is_some() {
            return false;
        }
        let Some(landing) = page_order(content, self.category).first().copied() else {
            return false;
        };
        self.focus = Some(landing);
        true
    }

    /// `←` from anywhere on the page: back to the rail item that opened it.
    ///
    /// Back to *this page's* item and not to wherever the rail was last, because
    /// the rail's selection and the page on screen are one fact — the rail is
    /// what says which page this is.
    fn step_out_of_page(&mut self, content: SettingsContent<'_>) -> bool {
        if self.on_nav() || self.menu.is_some() {
            return false;
        }
        if !content.has_content(self.category) {
            return false;
        }
        let landing = SettingsTarget::Nav(self.category);
        let changed = self.focus != Some(landing);
        self.focus = Some(landing);
        changed
    }

    /// Enter or Space on whatever the ring is on.
    fn activate(
        &mut self,
        content: SettingsContent<'_>,
        values: SettingsValues,
    ) -> SettingsKeyVerdict {
        match self.focus {
            Some(SettingsTarget::Close) => {
                self.close();
                SettingsKeyVerdict::Closed
            }
            // Enter on a rail item is the third spelling of "take me in", beside
            // `→` and Tab. The page it names is already the page on screen — the
            // arrows switch as they walk — so there is nothing to select, only
            // somewhere to go.
            Some(SettingsTarget::Nav(_)) => {
                SettingsKeyVerdict::from_moved(self.step_into_page(content))
            }
            // A row this machine cannot honour refuses Enter, exactly as a
            // greyed picker item does — the ring may stand on it and read its
            // reason, and nothing more.
            Some(SettingsTarget::Combo(row) | SettingsTarget::Slider(row))
                if !row.available(values) =>
            {
                SettingsKeyVerdict::Inert
            }
            // Enter on a slider has nothing to open and nothing to choose: the
            // value is already what the arrows have made it. Inert rather than
            // a no-op fallthrough, so that the reason is written down.
            Some(SettingsTarget::Slider(_)) => SettingsKeyVerdict::Inert,
            Some(SettingsTarget::Combo(row)) => {
                // "Opens its menu with the current value focused": the picker
                // opens where the user already is, not at its top, so the first
                // arrow press moves one step from what they have rather than
                // teleporting them to an option they did not ask about.
                self.menu = Some(row);
                self.focus = Some(SettingsTarget::Choice(
                    row,
                    row.selected_index(values).unwrap_or(0),
                ));
                SettingsKeyVerdict::Moved
            }
            Some(target @ SettingsTarget::Choice(row, index)) => {
                if !row.option_enabled(index, values) {
                    return SettingsKeyVerdict::Inert;
                }
                self.close_menu();
                SettingsKeyVerdict::Chose(target)
            }
            // The recorder opens here rather than through a verdict, because
            // starting to listen changes nothing outside this dialog: no file is
            // written until a chord is confirmed.
            Some(SettingsTarget::Record(row)) => {
                if content
                    .shortcuts
                    .get(row)
                    .is_none_or(|line| !line.recordable)
                {
                    return SettingsKeyVerdict::Inert;
                }
                self.begin_recording(row);
                SettingsKeyVerdict::Moved
            }
            Some(target @ (SettingsTarget::RestoreRow(_) | SettingsTarget::RestoreAll)) => {
                SettingsKeyVerdict::Chose(target)
            }
            _ => SettingsKeyVerdict::Inert,
        }
    }

    /// One step of the arrows: through the open picker's options if there is
    /// one, along the rail if the ring is on it, otherwise through the page's
    /// own controls.
    ///
    /// **Three regions, one spelling.** `↑`/`↓` never cross between the rail and
    /// the page — a list you are walking should not tip you out of its bottom
    /// into the first control of whatever it was pointing at — and that is what
    /// makes `←`/`→` and Tab the ways across rather than one of five behaviours
    /// of the same key.
    fn step(&mut self, content: SettingsContent<'_>, values: SettingsValues, delta: isize) -> bool {
        match (self.menu, self.focus) {
            (Some(menu), Some(SettingsTarget::Choice(row, index))) if menu == row => {
                self.step_option(row, values, index, delta)
            }
            (_, Some(SettingsTarget::Nav(_))) => self.step_nav(content, delta),
            _ => self.step_in(&page_order(content, self.category), delta),
        }
    }

    /// One step along the rail — **and the page turns with it**.
    ///
    /// Automatic activation, which is what a rail whose items are pages wants:
    /// the tab pattern's own rule for a list where showing a section costs
    /// nothing. A rail that needed Enter after every arrow would make the
    /// keyboard walk twice as long as the pointer's one click.
    fn step_nav(&mut self, content: SettingsContent<'_>, delta: isize) -> bool {
        let items = content.nav_items();
        if items.is_empty() {
            return false;
        }
        let at = items
            .iter()
            .position(|item| *item == self.category)
            .unwrap_or(0) as isize;
        let landing = items[(at + delta).rem_euclid(items.len() as isize) as usize];
        self.select_category(landing)
    }

    /// Home/End: the first or last option of an open picker, else the two ends
    /// of whichever region the ring is in.
    fn jump(&mut self, content: SettingsContent<'_>, values: SettingsValues, last: bool) -> bool {
        if let (Some(menu), Some(SettingsTarget::Choice(row, _))) = (self.menu, self.focus)
            && menu == row
        {
            // Stepped *onto* the end from just outside it, so Home and End land
            // on the first choosable item at that end rather than on a greyed
            // one — the same skip the arrows make, and not a second rule.
            return if last {
                self.step_option_from(row, values, row.option_count() as isize, -1)
            } else {
                self.step_option_from(row, values, -1, 1)
            };
        }
        if self.on_nav() {
            let items = content.nav_items();
            let landing = if last { items.last() } else { items.first() };
            return landing
                .copied()
                .is_some_and(|item| self.select_category(item));
        }
        let order = page_order(content, self.category);
        let landing = if last { order.last() } else { order.first() };
        let Some(landing) = landing.copied() else {
            return false;
        };
        let changed = self.focus != Some(landing);
        self.focus = Some(landing);
        changed
    }

    fn step_option(
        &mut self,
        row: SettingsRow,
        values: SettingsValues,
        index: usize,
        delta: isize,
    ) -> bool {
        self.step_option_from(row, values, index as isize, delta)
    }

    /// Walk the picker from `start` in `delta`, wrapping, and stop at the first
    /// item this machine can honour. Answers `false` when there is no such item
    /// to move to, which leaves the focus exactly where it was.
    fn step_option_from(
        &mut self,
        row: SettingsRow,
        values: SettingsValues,
        start: isize,
        delta: isize,
    ) -> bool {
        let count = row.option_count() as isize;
        if count == 0 {
            return false;
        }
        for step in 1..=count {
            let index = (start + delta * step).rem_euclid(count) as usize;
            if row.option_enabled(index, values) {
                let landing = SettingsTarget::Choice(row, index);
                let changed = self.focus != Some(landing);
                self.focus = Some(landing);
                return changed;
            }
        }
        false
    }

    /// One step along the dialog's Tab order, wrapping at both ends.
    fn step_focus(&mut self, content: SettingsContent<'_>, delta: isize) -> bool {
        self.step_in(&focus_order(content, self.category), delta)
    }

    /// One step along any list of stops, wrapping at both ends.
    fn step_in(&mut self, order: &[SettingsTarget], delta: isize) -> bool {
        if order.is_empty() {
            return false;
        }
        let at = self
            .focus
            .and_then(|focus| order.iter().position(|entry| *entry == focus))
            .unwrap_or(0) as isize;
        let landing = order[(at + delta).rem_euclid(order.len() as isize) as usize];
        let changed = self.focus != Some(landing);
        self.focus = Some(landing);
        changed
    }
}

// ── the recorder ───────────────────────────────────────────────────────────

/// One press, already judged, on its way to a listening row.
///
/// **The judging happens in `shortcuts.rs` and the state machine happens here**,
/// and the split is the point: this dialog draws boxes and walks a focus, and
/// what counts as a chord — which keys can be written into a file, which
/// modifier pairs a German keyboard produces by accident, which row already has
/// this one — is the shortcut table's own knowledge. A recorder that decided any
/// of it would be a second answer to a question `apply_overrides` also asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordInput {
    /// Only modifiers are down. The box shows them and goes on waiting.
    Modifier { caps: Vec<String> },
    /// A complete chord, with the reason it cannot be taken if there is one.
    Candidate {
        caps: Vec<String>,
        chord: crate::shortcuts::Chord,
        refusal: Option<String>,
    },
    /// Bare `Esc`.
    Cancel,
    /// Bare `Backspace` or `Delete`.
    Unbind,
    /// Bare `Enter`.
    Confirm,
    /// A key no file could hold.
    Unusable,
}

/// What a press did to a capture, for a runtime that has to repaint or write a
/// file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordVerdict {
    /// The box changed and nothing else did.
    Moved,
    /// Capture ended with nothing to persist.
    Ended,
    /// Give the line at this index this chord — or, with `None`, take its chord
    /// away — and write the file.
    Commit(usize, Option<crate::shortcuts::Chord>),
}

impl SettingsPanel {
    /// Start listening on one line of the shortcut page.
    pub fn begin_recording(&mut self, row: usize) {
        self.menu = None;
        self.recording = Some(Recording {
            row,
            ..Recording::default()
        });
        self.focus = Some(SettingsTarget::Record(row));
    }

    /// Drive a capture from one judged press.
    ///
    /// A refusal **leaves the capture open**, which is S64's real requirement:
    /// the point of refusing at record time rather than after is that the user
    /// is still holding the keyboard and can simply press something else. A
    /// refusal that shut the box would make the second attempt cost another
    /// click on the button they are already looking at.
    pub fn record(&mut self, input: RecordInput) -> RecordVerdict {
        let Some(capture) = self.recording.as_mut() else {
            return RecordVerdict::Ended;
        };
        match input {
            RecordInput::Modifier { caps } => {
                capture.caps = caps;
                capture.candidate = None;
                capture.hint = None;
                RecordVerdict::Moved
            }
            RecordInput::Candidate {
                caps,
                chord,
                refusal,
            } => {
                capture.caps = caps;
                match refusal {
                    Some(reason) => {
                        capture.candidate = None;
                        capture.hint = Some(reason);
                    }
                    None => {
                        capture.candidate = Some(chord);
                        capture.hint = None;
                    }
                }
                RecordVerdict::Moved
            }
            RecordInput::Unusable => {
                capture.candidate = None;
                capture.hint = Some(RECORD_UNUSABLE_HINT.to_owned());
                RecordVerdict::Moved
            }
            RecordInput::Cancel => {
                let row = capture.row;
                self.recording = None;
                self.focus = Some(SettingsTarget::Record(row));
                RecordVerdict::Ended
            }
            RecordInput::Unbind => {
                let row = capture.row;
                self.recording = None;
                self.focus = Some(SettingsTarget::Record(row));
                RecordVerdict::Commit(row, None)
            }
            RecordInput::Confirm => match capture.candidate.take() {
                // Nothing standing: `Enter` with no candidate is not "bind
                // Enter" and it is not "give up" either — the box is still
                // waiting, and saying so costs nothing.
                None => RecordVerdict::Moved,
                Some(chord) => {
                    let row = capture.row;
                    self.recording = None;
                    self.focus = Some(SettingsTarget::Record(row));
                    RecordVerdict::Commit(row, Some(chord))
                }
            },
        }
    }
}

/// **The dialog's Tab order**: the close, the rail (one stop, the selected item),
/// then the page's own controls.
///
/// **The rail is one Tab stop and not five**, which is the tablist pattern every
/// settings dialog on this platform uses and the reason `↑`/`↓` and `←`/`→`
/// exist above: Tab crosses *between* the parts of a dialog, and a list you walk
/// with the arrows is one part. A rail that put five stops in the Tab order
/// would make reaching the first row of a page cost six presses.
///
/// Derived from the content for [`visible_rows`]' own reason — the conditional
/// Sidebar row must not be a stop the keyboard can reach while it is not drawn,
/// and a second list stating the order beside the one stating the rows is a
/// second place to forget that. The close is first because it is first on the
/// page; where the focus *starts* is a different question, and
/// [`SettingsPanel::toggle`] answers it.
#[must_use]
pub fn focus_order(
    content: SettingsContent<'_>,
    category: SettingsCategory,
) -> Vec<SettingsTarget> {
    let page = page_order(content, category);
    let mut order = Vec::with_capacity(page.len() + 2);
    order.push(SettingsTarget::Close);
    if content.has_content(category) {
        order.push(SettingsTarget::Nav(category));
    }
    order.extend(page);
    order
}

/// Every control on one page, top to bottom.
///
/// The rows page is one picker per row. The shortcut page is, per line, the
/// `Record` button when the line offers one and the `↺` when there is something
/// to undo — **and a `↺` that is not drawn is not a stop**, which is the same
/// rule the conditional Sidebar row taught: a ring on a control nobody can see
/// is a dialog that looks like it has swallowed the keyboard.
#[must_use]
pub fn page_order(content: SettingsContent<'_>, category: SettingsCategory) -> Vec<SettingsTarget> {
    if category == SettingsCategory::Shortcuts {
        let mut order = Vec::new();
        for (index, line) in content.shortcuts.iter().enumerate() {
            if line.recordable {
                order.push(SettingsTarget::Record(index));
            }
            if line.overridden {
                order.push(SettingsTarget::RestoreRow(index));
            }
        }
        if !content.shortcuts.is_empty() {
            order.push(SettingsTarget::RestoreAll);
        }
        return order;
    }
    content
        .page_rows(category)
        .into_iter()
        .map(SettingsRow::control_target)
        .collect()
}

/// Something in the overlay the pointer can be over — **and the same enumeration
/// the keyboard's focus is stated in** (`SettingsPanel::focus`).
///
/// One type for both, because a control is a control: the ring goes where a
/// press would go, and a second enumeration for "focusable things" would be a
/// second place to teach every control this dialog grows. The two of these that
/// are not controls (`Scrim`, `Panel`) are simply never in [`focus_order`].
///
/// There is no `None`: while the dialog is up every point in the window is one
/// of these, which is the whole of what "modal" means here.
///
/// **Extensible on purpose.** The Settings block's own roadmap adds a left
/// category rail, a shortcut-editing panel and a profile page; each of those
/// brings focusable things that are not a row's picker, and each arrives as a
/// variant here and a line in [`focus_order`] — not as a parallel focus type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTarget {
    /// The dimmed world behind the dialog. A press here closes.
    Scrim,
    /// The dialog itself, away from any control. A press here does nothing, and
    /// in particular does not close — the mock-up closes only on the scrim.
    Panel,
    Close,
    /// One word of the category rail.
    ///
    /// It carries the category rather than an index for [`SettingsRow`]'s own
    /// reason one level up: the rail is derived from the rows and a category can
    /// stop being in it, so an ordinal would come to name a different page the
    /// first time one appeared above it.
    Nav(SettingsCategory),
    /// A row's picker button.
    Combo(SettingsRow),
    /// A slider row's control column — the track, its thumb and the number
    /// beside them, which are one target because they are one control (§7.1.6c-4b).
    ///
    /// The whole column and not the thumb alone: a 12px disc is not a thing to
    /// ask somebody to hit, and every slider anybody has used jumps to a press
    /// on its track.
    Slider(SettingsRow),
    /// The open menu's own body, between or around its items.
    Menu(SettingsRow),
    /// One item of a row's open picker, by its index in that row's options.
    Choice(SettingsRow, usize),
    /// The `Record` button on one line of the shortcut page, by that line's
    /// index in [`SettingsContent::shortcuts`].
    ///
    /// **An index here and a string on disk**, and the difference is the whole
    /// division: this is a fact about the list drawn *this frame*, resolved
    /// before the frame ends, exactly as `Choice(row, index)` is a fact about
    /// the options drawn this frame. The id is what crosses into a file, and it
    /// is looked up through the line the index names.
    Record(usize),
    /// The `↺` on one line of the shortcut page.
    RestoreRow(usize),
    /// The shortcut page's own `Restore all defaults`.
    RestoreAll,
}

/// One row's three boxes, and which row they belong to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowLayout {
    pub row: SettingsRow,
    /// `.row`'s own border box — the full `11 + content + 11` band, padding and
    /// all, which is the rectangle "this row is on screen" is a claim about.
    ///
    /// Not derivable from the three boxes below: the padding is outside all of
    /// them, so a scroll that brought their union into view would still leave
    /// the row's rule and its breathing room cut by the content edge.
    pub band: [f32; 4],
    pub title: [f32; 4],
    pub desc: [f32; 4],
    pub combo: [f32; 4],
}

/// The page's own heading, and which category it names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupLayout {
    pub group: SettingsCategory,
    pub label: [f32; 4],
}

/// One word of the rail, and the band it answers in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavLayout {
    pub category: SettingsCategory,
    /// The pill: the ground a selected item wears and the box a press lands in.
    pub band: [f32; 4],
    /// Where the word sits, one picker's-worth of padding inside the pill.
    pub label: [f32; 4],
}

/// One line of the shortcut page, given boxes.
///
/// It is a `.row` and wears `.row`'s boxes — `band`, `title`, `desc` mean
/// exactly what they mean on a settings row — with the right-hand controls that
/// only this page has. `record` and `restore` are `Option` for the same reason
/// [`SettingsLayout::row`] returns one: a line that offers neither is a real
/// state (a reserved row offers nothing at all), and `None` is how the hit test
/// and the draw are told once instead of each working it out.
#[derive(Clone, Debug, PartialEq)]
pub struct ShortcutLineLayout {
    /// Which line of [`SettingsContent::shortcuts`] this is.
    pub index: usize,
    pub band: [f32; 4],
    pub title: [f32; 4],
    pub desc: [f32; 4],
    /// Where the caps are laid out, right to left from this box's right edge.
    pub caps: [f32; 4],
    pub record: Option<[f32; 4]>,
    pub restore: Option<[f32; 4]>,
}

/// Every rectangle the overlay draws and hit-tests, in physical pixels of the
/// whole surface.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsLayout {
    scale: f32,
    surface: [f32; 2],
    /// The dialog's border box.
    frame: [f32; 4],
    /// The header's content box — what `align-items: center` centres things on.
    header_content: [f32; 4],
    close: [f32; 4],
    /// The `.content` padding box, which is also what content is clipped to.
    content: [f32; 4],
    /// The furthest the content may be scrolled, and therefore also the test for
    /// whether it scrolls at all: `0.0` exactly when the whole stack fits.
    ///
    /// The same shape `RailGeometry::max_scroll` carries, for the same reason —
    /// the caller owns the offset, the geometry owns what the offset may legally
    /// be, and neither has to remember the other's arithmetic.
    max_scroll: f32,
    /// The rail's own column, header excluded — the box its words are clipped
    /// to, and the box a hairline is drawn down the right edge of.
    nav: [f32; 4],
    /// The rail's words, top to bottom.
    nav_items: Vec<NavLayout>,
    /// Which page is laid out here.
    category: SettingsCategory,
    /// The page's heading. A `Vec` of one, kept as a list because it is still
    /// derived by walking the rows and noticing where the category changes —
    /// and on a page that holds one category that walk produces exactly one
    /// heading, which is the derivation proving itself rather than a special
    /// case replacing it.
    groups: Vec<GroupLayout>,
    /// The rows this page is holding, top to bottom — the ones
    /// [`SettingsContent::page_rows`] selected, given boxes.
    rows: Vec<RowLayout>,
    /// The shortcut page's own lines. Empty on every other page.
    shortcuts: Vec<ShortcutLineLayout>,
    /// The shortcut page's `Restore all defaults`.
    restore_all: Option<[f32; 4]>,
    /// The open menu's border box and its items, top to bottom in its row's own
    /// option order. Empty when the menu is shut.
    menu: Option<[f32; 4]>,
    items: Vec<[f32; 4]>,
    menu_kind: Option<SettingsRow>,
}

impl SettingsLayout {
    /// Where a row landed, or `None` when the dialog is not holding it.
    ///
    /// The `Option` is the whole point and is why this is the only way to ask:
    /// `rows` is a list whose contents depend on [`visible_rows`], so "where is
    /// the Sidebar row" has an answer that can be "it is not here". Every
    /// geometry claim in this module asks through it rather than indexing, which
    /// is what keeps a pin from silently reading the row below the one it names.
    ///
    /// Drawing and hit-testing walk `rows` in order instead — they want every
    /// row, not a named one — so at the moment the pins are the only callers.
    #[allow(dead_code)]
    #[must_use]
    pub fn row(&self, row: SettingsRow) -> Option<&RowLayout> {
        self.rows.iter().find(|placed| placed.row == row)
    }

    /// What a press or a drag at `x` is asking a slider row for (§7.1.6c-4b).
    ///
    /// **The one door for both**, which is what makes a drag the same gesture as
    /// the press that began it: a press jumps the thumb to the pointer, and every
    /// motion afterwards asks this again with a new `x`. `None` for a row that is
    /// not on this page or is not a slider.
    ///
    /// `y` is deliberately not a parameter. Once a drag has begun, the pointer is
    /// allowed to wander off the track vertically — every slider ever built keeps
    /// following it — and the press that starts one has already been hit-tested
    /// against the row's band.
    #[must_use]
    pub fn slider_at(&self, row: SettingsRow, x: f64) -> Option<u8> {
        let placed = self.row(row)?;
        let range = row.control().range()?;
        Some(slider_value_at(placed.combo, self.scale, range, x as f32))
    }

    /// The furthest this dialog may be scrolled; `0.0` when nothing overflows.
    #[must_use]
    pub fn max_scroll(&self) -> f32 {
        self.max_scroll
    }

    /// The box the content is clipped to — the wheel's own catchment.
    #[must_use]
    pub fn content_box(&self) -> [f32; 4] {
        self.content
    }

    /// **The scroll that brings the focused control into view**, given the one
    /// this layout was built at — or that same number when it is already there.
    ///
    /// *Minimal* movement, which is the whole of the rule: a row one pixel below
    /// the fold rises one pixel, not to the top of the box. Scrolling further
    /// than the fix requires moves rows the user was reading for a reason they
    /// did not ask about, and the browsers this behaviour is borrowed from
    /// (`scrollIntoView({ block: "nearest" })`) settled the same way.
    ///
    /// The row's whole [`RowLayout::band`] is what has to fit, not its control:
    /// a combo brought exactly to the content edge leaves its own title cut off
    /// above it, and the ring drawn round it is then a ring half outside the box
    /// that clips it. `Close` never moves anything — the header does not scroll.
    #[must_use]
    pub fn scroll_to_show(&self, target: SettingsTarget, scroll: f32) -> f32 {
        // The rail does not scroll and neither does the header, so neither moves
        // anything: `Nav` is the second target in this dialog whose answer is
        // "nowhere", and it is the first one that is a control.
        let band = match target {
            SettingsTarget::Combo(row)
            | SettingsTarget::Slider(row)
            | SettingsTarget::Menu(row) => self.row(row).map(|placed| placed.band),
            SettingsTarget::Choice(row, _) => self.row(row).map(|placed| placed.band),
            SettingsTarget::Record(index) | SettingsTarget::RestoreRow(index) => self
                .shortcuts
                .iter()
                .find(|line| line.index == index)
                .map(|line| line.band),
            SettingsTarget::RestoreAll => self.restore_all,
            SettingsTarget::Nav(_)
            | SettingsTarget::Close
            | SettingsTarget::Scrim
            | SettingsTarget::Panel => {
                return scroll;
            }
        };
        let Some(band) = band else {
            return scroll;
        };
        let above = self.content[1] - band[1];
        let below = band[3] - self.content[3];
        // A band taller than the viewport cannot satisfy both ends; its top wins,
        // because that is where a row's title is and reading starts there.
        let travel = if above > 0.0 {
            -above
        } else if below > 0.0 {
            below
        } else {
            0.0
        };
        (scroll + travel).clamp(0.0, self.max_scroll)
    }

    /// Whether a content box landed wholly inside the scroll viewport.
    ///
    /// Partly-visible is not visible enough to press: a combo sliced by the
    /// content edge would take a click aimed at the row above it. The vertical
    /// rail already answers a scrolled list this way (`seats::hit_rail_chrome`),
    /// and a dialog that scrolls is the same list with a different border.
    fn shows(&self, rect: [f32; 4]) -> bool {
        rect[1] >= self.content[1] && rect[3] <= self.content[3]
    }
}

/// The persisted theme mode a press on the overlay asks the process to select,
/// if it asks for one at all.
///
/// A named function rather than a `match` at the call site so the mapping from
/// "what the pointer hit" to the mode the app resolves against the OS is one
/// thing that can be stated, and pinned, without a live window.
#[must_use]
pub fn theme_requested(target: SettingsTarget) -> Option<ThemeModeV1> {
    match target {
        SettingsTarget::Choice(SettingsRow::Theme, index) => THEME_OPTIONS.get(index).copied(),
        _ => None,
    }
}

/// The profile **id** a press asks to become the default, if it asks at all.
///
/// An id and not the index the press carried, and that is the whole reason this
/// function exists rather than the caller reading `Choice(_, index).1`: an index
/// is a fact about today's table and `settings.json` has to survive tomorrow's
/// (`bt_persist::SettingsV1::default_profile`). The one place the index becomes a
/// name is here, at the boundary between the pointer and the file.
#[must_use]
pub fn default_profile_requested(target: SettingsTarget) -> Option<&'static str> {
    match target {
        SettingsTarget::Choice(SettingsRow::DefaultProfile, index) => {
            profiles::PROFILES.get(index).map(|profile| profile.id)
        }
        _ => None,
    }
}

#[must_use]
pub fn cursor_style_requested(target: SettingsTarget) -> Option<CursorStyle> {
    match target {
        SettingsTarget::Choice(SettingsRow::Cursor, index) => CURSOR_OPTIONS.get(index).copied(),
        _ => None,
    }
}

#[must_use]
pub fn display_formulas_requested(target: SettingsTarget) -> Option<bool> {
    match target {
        SettingsTarget::Choice(SettingsRow::Formulas, index) => FORMULA_OPTIONS.get(index).copied(),
        _ => None,
    }
}

#[must_use]
pub fn inline_formulas_requested(target: SettingsTarget) -> Option<bool> {
    match target {
        SettingsTarget::Choice(SettingsRow::InlineFormulas, index) => {
            FORMULA_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// The split direction a press asks for, if it asks at all.
#[must_use]
pub fn split_direction_requested(target: SettingsTarget) -> Option<SplitDirectionV1> {
    match target {
        SettingsTarget::Choice(SettingsRow::SplitDirection, index) => {
            SPLIT_DIRECTION_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// The language a press asks for, if it asks at all.
///
/// **The mode, not the resolved language** — what goes in the file is what the
/// user pointed at, and `System` has to survive as itself so it goes on meaning
/// "ask Windows" on the next machine this file is read on.
#[must_use]
pub fn language_requested(target: SettingsTarget) -> Option<LanguageV1> {
    match target {
        SettingsTarget::Choice(SettingsRow::Language, index) => {
            LANGUAGE_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// The grid's face, as a press on the family picker — the family's **name**,
/// which is what `settings.json` stores and what the renderer resolves.
#[must_use]
pub fn terminal_font_requested(target: SettingsTarget) -> Option<&'static str> {
    match target {
        SettingsTarget::Choice(SettingsRow::TerminalFont, index) => monospace_families()
            .get(index)
            .map(|family| family.name.as_str()),
        _ => None,
    }
}

/// The grid's face size in logical pixels, as a press on the size picker.
#[must_use]
pub fn font_size_requested(target: SettingsTarget) -> Option<u8> {
    match target {
        SettingsTarget::Choice(SettingsRow::FontSize, index) => {
            FONT_SIZE_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// The light canvas's palette, as a press on its picker — the scheme's **name**,
/// which is what `settings.json` stores and what the catalogue resolves.
#[must_use]
pub fn light_scheme_requested(target: SettingsTarget) -> Option<&'static str> {
    match target {
        SettingsTarget::Choice(SettingsRow::LightScheme, index) => {
            scheme_labels(true).get(index).copied()
        }
        _ => None,
    }
}

/// The dark canvas's, on the same terms.
#[must_use]
pub fn dark_scheme_requested(target: SettingsTarget) -> Option<&'static str> {
    match target {
        SettingsTarget::Choice(SettingsRow::DarkScheme, index) => {
            scheme_labels(false).get(index).copied()
        }
        _ => None,
    }
}

/// Whether the patched PSReadLine was asked for or asked to go.
#[must_use]
pub fn psreadline_requested(target: SettingsTarget) -> Option<bool> {
    match target {
        SettingsTarget::Choice(SettingsRow::PsReadLine, index) => {
            FORMULA_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// What a press on the Background image row's picker asks for (§7.1.6c-4b).
///
/// [`ImageSource::Choose`] is a **verb** and not a value: the caller opens the
/// system's chooser and may come back with nothing, so nothing is stored here.
#[must_use]
pub fn background_image_requested(target: SettingsTarget) -> Option<ImageSource> {
    match target {
        SettingsTarget::Choice(SettingsRow::BackgroundImage, index) => {
            IMAGE_SOURCE_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// How the picture meets the window, as a press on its picker.
#[must_use]
pub fn image_fit_requested(target: SettingsTarget) -> Option<BackgroundFitV1> {
    match target {
        SettingsTarget::Choice(SettingsRow::ImageFit, index) => {
            IMAGE_FIT_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// The system backdrop, as a press on its picker.
#[must_use]
pub fn acrylic_requested(target: SettingsTarget) -> Option<bool> {
    match target {
        SettingsTarget::Choice(SettingsRow::Acrylic, index) => FORMULA_OPTIONS.get(index).copied(),
        _ => None,
    }
}

/// The window's z-order posture, as a press on its picker.
#[must_use]
pub fn always_on_top_requested(target: SettingsTarget) -> Option<bool> {
    match target {
        SettingsTarget::Choice(SettingsRow::AlwaysOnTop, index) => {
            FORMULA_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

/// The Git panel's master switch, as a press on its picker.
#[must_use]
pub fn git_panel_requested(target: SettingsTarget) -> Option<bool> {
    match target {
        SettingsTarget::Choice(SettingsRow::GitPanel, index) => FORMULA_OPTIONS.get(index).copied(),
        _ => None,
    }
}

#[must_use]
pub fn tab_layout_requested(target: SettingsTarget) -> Option<TabLayoutMode> {
    match target {
        SettingsTarget::Choice(SettingsRow::TabLayout, index) => {
            TAB_LAYOUT_OPTIONS.get(index).copied()
        }
        _ => None,
    }
}

#[must_use]
pub fn sidebar_mode_requested(target: SettingsTarget) -> Option<RailMode> {
    match target {
        SettingsTarget::Choice(SettingsRow::Sidebar, index) => SIDEBAR_OPTIONS.get(index).copied(),
        _ => None,
    }
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// Intersect a content rectangle with the box that clips it, or drop it.
///
/// Written so that a box already inside the other comes back *unchanged* —
/// `max` of two equal floats is one of them, bit for bit, and so is `min` — which
/// is what lets [`clip_content`] run over a dialog that does not scroll and hand
/// back the values it was given rather than rounding them on the way through.
fn clipped(rect: [f32; 4], clip: [f32; 4]) -> Option<[f32; 4]> {
    let out = [
        rect[0].max(clip[0]),
        rect[1].max(clip[1]),
        rect[2].min(clip[2]),
        rect[3].min(clip[3]),
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

/// Whether `rect` lies wholly inside `clip`.
fn wholly_inside(rect: [f32; 4], clip: [f32; 4]) -> bool {
    rect[0] >= clip[0] && rect[1] >= clip[1] && rect[2] <= clip[2] && rect[3] <= clip[3]
}

/// **`overflow-y` on `.content`, in the three primitives this overlay draws in.**
///
/// The scrolling stack is built at its own full geometry — every row the height a
/// row is, wherever the offset put it — and this is the single sweep that decides
/// how much of it reaches the screen. The division is the whole fix for the bug
/// this function was extracted for: the rows used to be *built from* the crop, so
/// a row half out of the box was drawn as a shorter row rather than as the visible
/// part of a whole one. A combo sliced by the bottom edge came out a squat pill
/// with its bottom corners rounded on the cut, and a title sliced by the top edge
/// re-centred itself in the sliver and rode up against the edge — the compression
/// the user reported at both ends. Geometry translates; only drawing is cut.
///
/// It is [`seats::clip_pane_chrome`](crate::seats)'s rule, said for the overlay's
/// primitives, and the three channels answer it three different ways for the
/// reasons recorded there:
///
/// **Quads** intersect. A flat fill is the one primitive that crops exactly, and
/// the rounded shapes here are already *made of* flat fills — `rounded_overlay_fill`
/// emits whole runs and single coverage-weighted pixels — so cutting the run is
/// cutting the shape, with the corner left wherever the shape's own corner is.
///
/// **Labels** keep their `rect` and gain a [`ChromeLabel::clip`]. Intersecting the
/// layout box instead is exactly what compressed the titles: the box a label is
/// laid out in is also the box it is centred in, so a cropped box re-centres the
/// glyphs inside the crop and the text slides as the offset changes.
///
/// **Sprites** are dropped unless they are wholly inside. A [`ChromeSprite`] is a
/// raster blit at its own size and the pipeline cannot draw part of one; the
/// scrolling stack draws no marks today, and this is the rule the first one added
/// to it will meet rather than a silent escape from the box.
fn clip_content(
    clip: [f32; 4],
    content: OverlayLayer,
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    sprites: &mut Vec<ChromeSprite>,
) {
    let OverlayLayer {
        quads: content_quads,
        labels: content_labels,
        sprites: content_sprites,
        ..
    } = content;
    quads.extend(
        content_quads
            .into_iter()
            .filter_map(|quad| clipped(quad.rect, clip).map(|rect| OverlayQuad { rect, ..quad })),
    );
    labels.extend(content_labels.into_iter().filter_map(|label| {
        // Composed with whatever window the label already carried rather than
        // replacing it: the combo's value is clipped to its own button before the
        // content box ever sees it, and a clip that widened here would let a long
        // value escape the control it names.
        clipped(label.clip.unwrap_or(label.rect), clip).map(|window| ChromeLabel {
            clip: Some(window),
            ..label
        })
    }));
    sprites.extend(
        content_sprites
            .into_iter()
            .filter(|sprite| wholly_inside(sprite.rect, clip)),
    );
}

/// `text-overflow: ellipsis` — `text` if the whole of it fits `max_width`, else
/// the longest prefix that fits with a `…` after it.
///
/// The `…` is the whole reason this needs a `measure`: a prefix is only the right
/// prefix against the font that will draw it, and this module is a pure function
/// of numbers handed to it. So the caller measures — the same division
/// `layout_for_menu`'s `widest_option`, `peek_strip::layout` and `profiles::build`
/// already run on — and the search here is over char boundaries, never bytes, so
/// no prefix can cut a glyph in half.
///
/// Binary search rather than a walk: text width is monotonic in prefix length for
/// the left-to-right chrome face, and shaping a label is not free enough to do
/// once per character on every frame the dialog is up.
pub(crate) fn ellipsized(
    text: &str,
    max_width: f32,
    font_size_px: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> String {
    if measure(text, font_size_px) <= max_width {
        return text.to_owned();
    }
    // Every place a prefix may end, shortest first. `text` itself is not among
    // them: the whole string was just refused, so `char_indices` — which stops
    // before the end — is exactly the candidate set.
    //
    // A `text` that reaches here is non-empty (the empty string measures zero and
    // left above), so index 0 always exists. It is the floor rather than a
    // candidate: when not even a lone `…` fits, a lone `…` is still what CSS
    // draws, and the label's own box clips it.
    let ends: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    let fits = |end: usize, measure: &mut dyn FnMut(&str, f32) -> f32| {
        measure(&format!("{}{ELLIPSIS}", &text[..end]), font_size_px) <= max_width
    };
    let mut best = 0;
    let (mut low, mut high) = (1, ends.len());
    while low < high {
        let middle = low + (high - low) / 2;
        if fits(ends[middle], measure) {
            best = middle;
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    format!("{}{ELLIPSIS}", &text[..ends[best]])
}

/// The same, cut from the **front**: `text` if it fits, else a `…` and the
/// longest *suffix* that fits after it.
///
/// This is B23 — `.foot-path { direction: rtl; text-align: left }` — and it is
/// the right rule for a path rather than a stylistic preference: the end of a
/// path is the part you are actually looking at, and a right-cut
/// `C:\Users\Weiyi\Developer\Bett…` has thrown away the only segment that
/// answers "where am I".
///
/// The mock-up reaches this through `direction: rtl`, and paid for it: `/`, `~`
/// and `:` are bidi-neutral, so a bare RTL paragraph reorders them and the foot
/// showed `bt/x.png/~` for `~/bt/x.png` (user-reported). The fix there was to
/// wrap the path in a `<bdi>`. Here there is no bidi algorithm to fight in the
/// first place — the string is cut and then laid out left to right like every
/// other label — so the bug it records cannot occur, and this is the honest
/// native equivalent rather than a re-implementation of the workaround.
pub(crate) fn ellipsized_left(
    text: &str,
    max_width: f32,
    font_size_px: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> String {
    if measure(text, font_size_px) <= max_width {
        return text.to_owned();
    }
    // Every place a suffix may begin, longest first — the mirror of the prefix
    // search above, and over char boundaries for the same reason.
    let starts: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    let fits = |start: usize, measure: &mut dyn FnMut(&str, f32) -> f32| {
        measure(&format!("{ELLIPSIS}{}", &text[start..]), font_size_px) <= max_width
    };
    // The floor is the empty suffix — a lone `…` — for the same reason the
    // prefix search's is: when nothing fits, that is still what CSS draws.
    let mut best = starts.len();
    let (mut low, mut high) = (0, starts.len());
    while low < high {
        let middle = low + (high - low) / 2;
        if fits(starts[middle], measure) {
            best = middle;
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let start = starts.get(best).copied().unwrap_or(text.len());
    format!("{ELLIPSIS}{}", &text[start..])
}

/// Where every part of the dialog lands in a window this size, or `None` when
/// the window cannot host it.
///
/// `None` is a real answer, not a failure: `max-height: calc(100% - 72px)` can
/// go to nothing, and a scrim over a dialog that is not there would be a window
/// nobody can use. The runtime treats it as "not open", so no input is trapped
/// behind an invisible modal. Since the rail arrived it is also the answer to a
/// window too *narrow*: below a width that can hold the rail, the gutters and a
/// picker, what is left is not a narrower version of this design, it is a
/// different one — and refusing is more honest than drawing a page with no room
/// for the control every row of it ends in.
///
/// `scroll` is how far the page's stack has been pushed up, in physical pixels,
/// and it is clamped here rather than trusted: `max-height` plus `overflow-y`
/// is a pair, and the half that says how far the overflow reaches belongs to
/// whoever measured the stack. The caller keeps the number and reads
/// [`SettingsLayout::max_scroll`] back to clamp its own copy — the same division
/// the tab strip and the rail already run on. **It is the page's scroll and not
/// the dialog's**: one page per category, so a distance measured on one page is
/// nonsense on the next, which is why turning a page resets it (`Q3 = A`).
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "the page's identity is the eighth, and every one of the eight is a \
              fact only the caller has: the surface, the scale, which picker is \
              open, what the dialog is holding, which page is up, how wide the \
              open picker measures, and how far it has been scrolled"
)]
pub fn layout_for_menu(
    surface_width: f32,
    surface_height: f32,
    scale: f32,
    menu_kind: Option<SettingsRow>,
    content_of: SettingsContent<'_>,
    category: SettingsCategory,
    widest_option: f32,
    scroll: f32,
) -> Option<SettingsLayout> {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let rows = content_of.page_rows(category);
    let shortcut_lines: &[crate::shortcuts::ShortcutRow] =
        if category == SettingsCategory::Shortcuts {
            content_of.shortcuts
        } else {
            &[]
        };
    let width = px(DIALOG_MAX_WIDTH_LOGICAL_PX)
        .min(surface_width * DIALOG_WIDTH_RATIO)
        .round();
    let top = px(DIALOG_TOP_LOGICAL_PX).round();
    let available = (surface_height - top - px(DIALOG_BOTTOM_LOGICAL_PX)).round();
    // `.row .text` is two stacked line boxes; the row is as tall as the taller
    // of that column and the control beside it, which is what `align-items:
    // center` on a flex row resolves to. A shortcut line is the same `.row` with
    // a different control, so it is the same height by construction.
    let text_height =
        px(ROW_TITLE_LINE_LOGICAL_PX + ROW_DESC_MARGIN_TOP_LOGICAL_PX + ROW_DESC_LINE_LOGICAL_PX);
    let row_content_height = text_height.max(px(COMBO_HEIGHT_LOGICAL_PX));
    let row_height = 2.0 * px(ROW_PADDING_Y_LOGICAL_PX) + row_content_height;
    // The page is a stack of one heading and its rows in one order, so its
    // height is that same stack measured — not a row count plus a remembered
    // heading. `heading_advance` answers for both the height here and the
    // placement below, which is what keeps a row from being drawn one heading's
    // worth away from where the dialog made room for it.
    let heading_advance = px(GROUP_LABEL_MARGIN_TOP_LOGICAL_PX)
        + px(GROUP_LABEL_LINE_LOGICAL_PX)
        + px(GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX);
    let mut stack_height = 0.0_f32;
    let mut previous_group: Option<SettingsCategory> = None;
    for row in &rows {
        let group = row.category();
        if previous_group != Some(group) {
            stack_height += heading_advance;
            previous_group = Some(group);
        }
        stack_height += row_height;
    }
    if !shortcut_lines.is_empty() {
        stack_height += heading_advance;
        stack_height += shortcut_lines.len() as f32 * row_height;
        stack_height += px(SHORTCUT_FOOT_MARGIN_TOP_LOGICAL_PX) + px(BUTTON_HEIGHT_LOGICAL_PX);
    }
    let page_height =
        px(CONTENT_PADDING_TOP_LOGICAL_PX) + stack_height + px(CONTENT_PADDING_BOTTOM_LOGICAL_PX);
    // The rail does not scroll — five words never overflow a dialog that is
    // already as tall as its window — but it is still part of what the body has
    // to be tall enough for, or a rail with more items than the shortest page
    // has rows would be cut off by a dialog that fitted the page exactly.
    let nav_items = content_of.nav_items();
    let nav_height = px(NAV_PADDING_TOP_LOGICAL_PX)
        + nav_stack_height(nav_items.len(), scale)
        + px(NAV_PADDING_BOTTOM_LOGICAL_PX);
    let body_height = page_height.max(nav_height);
    let header = px(HEADER_HEIGHT_LOGICAL_PX);
    let height = (2.0 * border + header + body_height).min(available).round();
    // Below the header plus its own two borders there is no dialog left to draw,
    // only a lid — and a lid with no body is not the design's dialog. Narrower
    // than the rail plus the gutters plus a picker is the same sentence sideways.
    let narrowest =
        px(NAV_WIDTH_LOGICAL_PX + 2.0 * CONTENT_PADDING_X_LOGICAL_PX + COMBO_MIN_WIDTH_LOGICAL_PX);
    if width < narrowest || height <= 2.0 * border + header {
        return None;
    }
    let left = ((surface_width - width) / 2.0).round();
    let frame = [left, top, left + width, top + height];
    let inner = [
        frame[0] + border,
        frame[1] + border,
        frame[2] - border,
        frame[3] - border,
    ];
    let header_content = [
        inner[0] + px(HEADER_PADDING_LEFT_LOGICAL_PX),
        inner[1] + px(HEADER_PADDING_TOP_LOGICAL_PX),
        inner[2] - px(HEADER_PADDING_RIGHT_LOGICAL_PX),
        inner[1] + px(HEADER_PADDING_TOP_LOGICAL_PX) + px(CLOSE_SIDE_LOGICAL_PX),
    ];
    // Only the dialog's own frame is snapped to the pixel grid; everything
    // inside it is the design's exact geometry off that frame. Rounding a box's
    // two edges independently is how a 27.5px control becomes a 28px one, and
    // the shapes snap themselves at draw time anyway.
    let close_side = px(CLOSE_SIDE_LOGICAL_PX);
    let close = [
        header_content[2] - close_side,
        header_content[1],
        header_content[2],
        header_content[1] + close_side,
    ];
    let body = [
        inner[0],
        inner[1] + header,
        inner[2],
        inner[3].max(inner[1] + header),
    ];
    // **The rail is a column of the body and the page is what is left of it.**
    // Stated in that order because that is the dependency: the rail's width is
    // the design's, and the page takes the remainder — a page that claimed its
    // width first would push the rail off a narrow window rather than the
    // dialog refusing to open, which is the answer the guard above already gave.
    let nav = [
        body[0],
        body[1],
        body[0] + px(NAV_WIDTH_LOGICAL_PX),
        body[3],
    ];
    let content = [nav[2], body[1], body[2], body[3]];
    let placed_nav = place_nav(&nav_items, nav, scale);
    // What the page's stack wanted minus what the box gives it. The dialog
    // already stopped growing at `available`; this is the height that capping
    // refused, and refusing it silently is what cut the last row off before.
    let max_scroll = (page_height - (content[3] - content[1])).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let text_left = content[0] + px(CONTENT_PADDING_X_LOGICAL_PX);
    let text_right = content[2] - px(CONTENT_PADDING_X_LOGICAL_PX);
    let row_left = text_left + px(ROW_PADDING_X_LOGICAL_PX);
    let row_right = text_right - px(ROW_PADDING_X_LOGICAL_PX);
    let combo_width = px(COMBO_MIN_WIDTH_LOGICAL_PX);
    let combo_height = px(COMBO_HEIGHT_LOGICAL_PX);
    // One walk down the same stack the height was measured from. A heading is
    // emitted wherever the category changes and everything after it moves down
    // by exactly what the heading took, so the boxes drawn, the boxes
    // hit-tested and the height reserved are three readings of one derivation
    // rather than three rules that have to be kept in agreement.
    let mut cursor = content[1] + px(CONTENT_PADDING_TOP_LOGICAL_PX) - scroll;
    let mut placed_groups: Vec<GroupLayout> = Vec::new();
    let mut placed_rows: Vec<RowLayout> = Vec::with_capacity(rows.len());
    let mut previous_group: Option<SettingsCategory> = None;
    for row in &rows {
        let group = row.category();
        if previous_group != Some(group) {
            cursor += px(GROUP_LABEL_MARGIN_TOP_LOGICAL_PX);
            let label = [
                text_left,
                cursor,
                text_right,
                cursor + px(GROUP_LABEL_LINE_LOGICAL_PX),
            ];
            cursor = label[3] + px(GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX);
            placed_groups.push(GroupLayout { group, label });
            previous_group = Some(group);
        }
        placed_rows.push({
            let band = [row_left, cursor, row_right, cursor + row_height];
            let top = cursor + px(ROW_PADDING_Y_LOGICAL_PX);
            cursor += row_height;
            let combo_top = top + (row_content_height - combo_height) / 2.0;
            let combo = [
                row_right - combo_width,
                combo_top,
                row_right,
                combo_top + combo_height,
            ];
            // `.row .text` is `flex: 1` beside a `flex: none` control, one gap
            // apart.
            let text_column_right = combo[0] - px(ROW_GAP_LOGICAL_PX);
            let title = [
                row_left,
                top,
                text_column_right,
                top + px(ROW_TITLE_LINE_LOGICAL_PX),
            ];
            let desc = [
                row_left,
                title[3] + px(ROW_DESC_MARGIN_TOP_LOGICAL_PX),
                text_column_right,
                title[3] + px(ROW_DESC_MARGIN_TOP_LOGICAL_PX + ROW_DESC_LINE_LOGICAL_PX),
            ];
            RowLayout {
                row: *row,
                band,
                title,
                desc,
                combo,
            }
        });
    }
    let mut placed_lines: Vec<ShortcutLineLayout> = Vec::with_capacity(shortcut_lines.len());
    let mut restore_all = None;
    if !shortcut_lines.is_empty() {
        cursor += px(GROUP_LABEL_MARGIN_TOP_LOGICAL_PX);
        let label = [
            text_left,
            cursor,
            text_right,
            cursor + px(GROUP_LABEL_LINE_LOGICAL_PX),
        ];
        cursor = label[3] + px(GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX);
        placed_groups.push(GroupLayout {
            group: SettingsCategory::Shortcuts,
            label,
        });
        for (index, line) in shortcut_lines.iter().enumerate() {
            let band = [row_left, cursor, row_right, cursor + row_height];
            let top = cursor + px(ROW_PADDING_Y_LOGICAL_PX);
            cursor += row_height;
            let middle = (band[1] + band[3]) / 2.0;
            // Right to left, which is how the row is actually built: the verbs
            // sit against the edge and the chord sits against the verb that
            // changes it. Laid out the other way the caps would move whenever a
            // row gained or lost a `↺`, and a chord that shifts sideways when
            // you edit a different row is a chord that looks like it changed.
            let restore = line.overridden.then(|| {
                let side = px(SHORTCUT_RESTORE_SIDE_LOGICAL_PX);
                [
                    row_right - side,
                    middle - side / 2.0,
                    row_right,
                    middle + side / 2.0,
                ]
            });
            let record_right = restore.map_or(row_right, |box_| {
                box_[0] - px(SHORTCUT_CONTROL_GAP_LOGICAL_PX)
            });
            let record = line.recordable.then(|| {
                let width = px(SHORTCUT_RECORD_WIDTH_LOGICAL_PX);
                let height = px(BUTTON_HEIGHT_LOGICAL_PX);
                [
                    record_right - width,
                    middle - height / 2.0,
                    record_right,
                    middle + height / 2.0,
                ]
            });
            let caps_right = record.map_or(record_right, |box_| box_[0] - px(ROW_GAP_LOGICAL_PX));
            let caps_left = caps_right - px(SHORTCUT_CAPS_WIDTH_LOGICAL_PX);
            let caps = [caps_left, band[1], caps_right, band[3]];
            let text_column_right = caps_left - px(ROW_GAP_LOGICAL_PX);
            let title = [
                row_left,
                top,
                text_column_right,
                top + px(ROW_TITLE_LINE_LOGICAL_PX),
            ];
            let desc = [
                row_left,
                title[3] + px(ROW_DESC_MARGIN_TOP_LOGICAL_PX),
                text_column_right,
                title[3] + px(ROW_DESC_MARGIN_TOP_LOGICAL_PX + ROW_DESC_LINE_LOGICAL_PX),
            ];
            placed_lines.push(ShortcutLineLayout {
                index,
                band,
                title,
                desc,
                caps,
                record,
                restore,
            });
        }
        cursor += px(SHORTCUT_FOOT_MARGIN_TOP_LOGICAL_PX);
        let width = px(RESTORE_ALL_WIDTH_LOGICAL_PX);
        restore_all = Some([
            row_right - width,
            cursor,
            row_right,
            cursor + px(BUTTON_HEIGHT_LOGICAL_PX),
        ]);
    }
    // A picker hangs off a row's button, so a picker named for a row this page
    // is not holding has nothing to hang from and is not open. That is not a
    // guard against the impossible: switching Tab layout to Horizontal takes the
    // Sidebar row out from under its own open menu, and turning to another page
    // takes every row of this one.
    //
    // A row scrolled out from under its own picker is the same sentence with the
    // same answer. The button is the anchor, and an anchor clipped away by the
    // content edge is one the popup cannot honestly hang from — it would float
    // beside a control nobody can see, over rows it does not belong to.
    let active = menu_kind.and_then(|row| {
        placed_rows
            .iter()
            .find(|placed| placed.row == row)
            .filter(|placed| placed.combo[1] >= content[1] && placed.combo[3] <= content[3])
            .map(|placed| (row, placed.combo))
    });
    let (menu, items) = match active {
        Some((row, combo)) => menu_layout(
            combo,
            surface_width,
            surface_height,
            scale,
            border,
            row.option_count(),
            widest_option + option_icon_advance(row, scale),
        ),
        None => (None, Vec::new()),
    };
    Some(SettingsLayout {
        scale,
        surface: [surface_width, surface_height],
        frame,
        header_content,
        close,
        content,
        max_scroll,
        nav,
        nav_items: placed_nav,
        category,
        groups: placed_groups,
        rows: placed_rows,
        shortcuts: placed_lines,
        restore_all,
        menu,
        items,
        menu_kind: active.map(|(row, _)| row),
    })
}

/// What a rail of this many words costs, pills and the gaps between them.
fn nav_stack_height(items: usize, scale: f32) -> f32 {
    if items == 0 {
        return 0.0;
    }
    let px = |value: f32| value * scale;
    items as f32 * px(NAV_ITEM_HEIGHT_LOGICAL_PX) + (items - 1) as f32 * px(NAV_ITEM_GAP_LOGICAL_PX)
}

/// Give the rail's words their boxes, top to bottom.
fn place_nav(items: &[SettingsCategory], nav: [f32; 4], scale: f32) -> Vec<NavLayout> {
    let px = |value: f32| value * scale;
    let left = nav[0] + px(NAV_PADDING_X_LOGICAL_PX);
    let right = nav[2] - px(NAV_PADDING_X_LOGICAL_PX);
    let mut cursor = nav[1] + px(NAV_PADDING_TOP_LOGICAL_PX);
    items
        .iter()
        .map(|category| {
            let band = [left, cursor, right, cursor + px(NAV_ITEM_HEIGHT_LOGICAL_PX)];
            cursor = band[3] + px(NAV_ITEM_GAP_LOGICAL_PX);
            let label = [
                band[0] + px(NAV_ITEM_PADDING_LEFT_LOGICAL_PX),
                band[1],
                band[2],
                band[3],
            ];
            NavLayout {
                category: *category,
                band,
                label,
            }
        })
        .collect()
}

/// The theme picker's popup: `min-width: 100%` and `right: 0` off the button,
/// one `MENU_OFFSET` below it, flipped above when it would spill.
///
/// What it is measured against is the *window*, not the dialog's `.content`.
/// The mock-up's own rule is "whatever actually clips it — its scroll container
/// if it has one, else the window", and with one row the content does not
/// scroll and therefore clips nothing. Measured against `.content` anyway, this
/// menu flips up into the header and gets its first item cut off — which the
/// prototype does, and which is the artefact that rule exists to avoid.
///
/// `min-width: 100%` is a **floor**, not a size. This read it as an equality for
/// as long as every option happened to be one short word, and an option longer
/// than its button was therefore cropped mid-glyph — the popup, its item pills
/// and the text all inheriting the button's width. `widest_option` is the
/// measured width of the longest label the open row will draw (the caller owns
/// the font, so the caller measures — the same division `peek_strip::layout` and
/// `restore` already use), and the box grows leftward off `right: 0` to hold it.
fn menu_layout(
    combo: [f32; 4],
    surface_width: f32,
    surface_height: f32,
    scale: f32,
    border: f32,
    option_count: usize,
    widest_option: f32,
) -> (Option<[f32; 4]>, Vec<[f32; 4]>) {
    let px = |value: f32| value * scale;
    // Everything an item spends before and after its own glyphs: the menu's two
    // borders and its padding on both sides, then the item's left padding, the
    // fixed tick column, the gap after it, and the item's right padding.
    let chrome = 2.0 * border
        + 2.0 * px(MENU_PADDING_LOGICAL_PX)
        + 2.0 * px(ITEM_PADDING_X_LOGICAL_PX)
        + px(TICK_WIDTH_LOGICAL_PX)
        + px(ITEM_GAP_LOGICAL_PX);
    let width = (combo[2] - combo[0]).max((chrome + widest_option).ceil());
    let height = 2.0 * border
        + 2.0 * px(MENU_PADDING_LOGICAL_PX)
        + option_count as f32 * px(ITEM_HEIGHT_LOGICAL_PX);
    let below = combo[3] + px(MENU_OFFSET_LOGICAL_PX);
    let top = if below + height > surface_height - px(MENU_CLEARANCE_LOGICAL_PX) {
        combo[1] - px(MENU_OFFSET_LOGICAL_PX) - height
    } else {
        below
    };
    // `right: 0`, so a popup wider than its button grows leftward and its right
    // edge stays on the button's. Then the same clamp every other floating box
    // in this app uses (`profiles`, `tooltip`, `peek_strip`): pull it back inside
    // the window rather than let it hang off, and on a window too narrow to hold
    // it at all the `max` wins and it hangs off the right, never off the left.
    let margin = px(MENU_CLEARANCE_LOGICAL_PX);
    let left = (combo[2] - width)
        .min(surface_width - width - margin)
        .max(margin);
    let frame = [left, top, left + width, top + height];
    let item_left = frame[0] + border + px(MENU_PADDING_LOGICAL_PX);
    let item_right = frame[2] - border - px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX);
    let items = (0..option_count)
        .map(|index| {
            let item_top =
                frame[1] + border + px(MENU_PADDING_LOGICAL_PX) + index as f32 * item_height;
            [item_left, item_top, item_right, item_top + item_height]
        })
        .collect();
    (Some(frame), items)
}

/// What the pointer is over. Never `None`: a modal owns every pixel.
///
/// Smallest target first, the same ruling `seats::hit_chrome` follows — an item
/// sits inside a menu, a menu and a close button sit on the dialog, and the
/// dialog sits on the scrim.
#[must_use]
pub fn hit(layout: &SettingsLayout, values: SettingsValues, x: f64, y: f64) -> SettingsTarget {
    let (x, y) = (x as f32, y as f32);
    if let (Some(menu), Some(row)) = (layout.menu, layout.menu_kind) {
        for (index, item) in layout.items.iter().enumerate() {
            if contains(*item, x, y) {
                // **An item this machine cannot honour is menu body**, which is
                // where the greying is enforced — one answer read by the hover
                // and the click alike (`SettingsRow::option_enabled`). The menu
                // stays open under such a press, exactly as it does over the gap
                // between two items, because nothing was chosen.
                return if row.option_enabled(index, values) {
                    SettingsTarget::Choice(row, index)
                } else {
                    SettingsTarget::Menu(row)
                };
            }
        }
        if contains(menu, x, y) {
            return SettingsTarget::Menu(row);
        }
    }
    if contains(layout.close, x, y) {
        return SettingsTarget::Close;
    }
    // The rail before the page, which is the same smallest-target-first ruling
    // read at this scale: a word of the rail sits *on* the dialog, and the
    // dialog sits on the scrim. The rail is not clipped by the page's scroll box
    // and does not ask `shows` — it does not scroll, so there is nothing for a
    // scroll to have cut off.
    for item in &layout.nav_items {
        if contains(item.band, x, y) {
            return SettingsTarget::Nav(item.category);
        }
    }
    for placed in &layout.rows {
        if layout.shows(placed.combo) && contains(placed.combo, x, y) {
            // **A row this machine cannot honour is dialog body**, which is
            // `option_enabled`'s ruling one level up and enforced in the same
            // one place: a rule spelled only at the draw leaves a greyed row
            // that still opens its picker under the pointer.
            return if placed.row.available(values) {
                placed.row.control_target()
            } else {
                SettingsTarget::Panel
            };
        }
    }
    for line in &layout.shortcuts {
        if let Some(record) = line.record
            && layout.shows(record)
            && contains(record, x, y)
        {
            return SettingsTarget::Record(line.index);
        }
        if let Some(restore) = line.restore
            && layout.shows(restore)
            && contains(restore, x, y)
        {
            return SettingsTarget::RestoreRow(line.index);
        }
    }
    if let Some(restore_all) = layout.restore_all
        && layout.shows(restore_all)
        && contains(restore_all, x, y)
    {
        return SettingsTarget::RestoreAll;
    }
    if contains(layout.frame, x, y) {
        return SettingsTarget::Panel;
    }
    SettingsTarget::Scrim
}

/// Every fill, label and mark the overlay draws, bottom layer first.
///
/// Order matters and is not incidental. Inside one layer the renderer draws all
/// of the fills, then all of the marks, then all of the text, so within a layer
/// only a *fill* can cover a fill — a caption pushed after a popup's surface
/// still lands on top of it, because the text channel runs after every fill in
/// the layer. Covering therefore happens between layers and not inside one: the
/// dialog and its rows are the first layer, the open picker is a second one over
/// it, and a row added to the first layer tomorrow cannot reach through.
///
/// `focus` is where the **ring** goes, not where the keyboard is: the caller
/// hands over [`SettingsPanel::focus_ring`], which is `None` while the focus was
/// reached by pointer. Passing the focus itself would draw a ring round whatever
/// the user just clicked, which is the thing `:focus-visible` exists to stop.
#[must_use]
// Eight, and every one of them is a different question the draw has to be able
// to answer: where, what is under the pointer, what is under the ring, what the
// rows read, what the shortcut table holds, what the picture is called, what is
// being recorded, and how wide a string is. Bundling any of them into a struct
// would be a struct whose only reader is this call.
#[allow(clippy::too_many_arguments)]
pub fn build(
    layout: &SettingsLayout,
    hover: Option<SettingsTarget>,
    focus: Option<SettingsTarget>,
    values: SettingsValues,
    shortcuts: &[crate::shortcuts::ShortcutRow],
    // The chosen picture's file name, or empty when there is none — the one
    // caption in this dialog that is a runtime string rather than a table
    // lookup. It arrives here beside `shortcuts` and not inside
    // [`SettingsValues`] for the reason that struct is `Copy`: a path is not.
    background_image: &str,
    recording: Option<(usize, &[String], Option<&str>)>,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let mut quads = Vec::new();
    let mut labels = Vec::new();
    let mut sprites = Vec::new();

    // The scrim: the whole window, dimmed. Not a rounded shape and not inset —
    // it is `position: absolute; inset: 0` on the window's own client area.
    quads.push(OverlayQuad {
        rect: [0.0, 0.0, layout.surface[0], layout.surface[1]],
        color: palette.modal_scrim,
        alpha: alpha(palette.modal_scrim_alpha),
    });

    // The dialog: the floating-window craft — lift, hairline, face — at the
    // 10px round every window that floats shares, with `--win` for its face
    // because a dialog stands on the window's plane rather than a menu's.
    push_float_window(
        &mut quads,
        layout.frame,
        px(FLOAT_WINDOW_RADIUS_LOGICAL_PX),
        border,
        px(FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.dialog_surface,
        palette.menu_shadow,
        alpha(palette.menu_shadow_inner_alpha),
        alpha(palette.menu_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    labels.push(ChromeLabel {
        text: Text::Settings.text().to_owned(),
        rect: [
            layout.header_content[0],
            layout.header_content[1],
            layout.close[0],
            layout.header_content[3],
        ],
        font_size_px: px(HEADER_TITLE_FONT_LOGICAL_PX),
        color: palette.dialog_title_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });

    let close_hovered = hover == Some(SettingsTarget::Close);
    if close_hovered {
        quads.extend(rounded_overlay_fill(
            layout.close,
            px(CLOSE_RADIUS_LOGICAL_PX),
            palette.dialog_hover,
            1.0,
        ));
    }
    if focus == Some(SettingsTarget::Close) {
        quads.extend(focus_ring(layout.close, scale, palette.accent));
    }
    let glyph = px(WINDOW_CAPTION_GLYPH_LOGICAL_PX).round().max(1.0);
    let glyph_left = ((layout.close[0] + layout.close[2]) / 2.0 - glyph / 2.0).round();
    let glyph_top = ((layout.close[1] + layout.close[3]) / 2.0 - glyph / 2.0).round();
    sprites.push(ChromeSprite::new(
        ChromeMark::WindowClose,
        [glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph],
        if close_hovered {
            palette.dialog_title_text
        } else {
            palette.dialog_secondary_text
        },
    ));

    // **The rail, before the page and outside its clip.** It sits in the
    // dialog's own layer rather than in the scrolling stack because it does not
    // scroll: a rail that moved with the page would be a table of contents that
    // walks off the top of the book.
    let seam = [
        layout.nav[2] - border,
        layout.nav[1],
        layout.nav[2],
        layout.nav[3],
    ];
    quads.push(OverlayQuad {
        rect: seam,
        color: palette.menu_border,
        alpha: alpha(palette.menu_border_alpha),
    });
    for item in &layout.nav_items {
        let selected = item.category == layout.category;
        let hovered = hover == Some(SettingsTarget::Nav(item.category));
        // The selected word wears the ground whole; a hovered one that is not
        // the page wears it at half strength. A hover is a question and a
        // selection is an answer, and the two must read apart — but with a
        // ground and a weight, not with a stroke (user ruling 2026-08-17).
        if selected || hovered {
            quads.extend(rounded_overlay_fill(
                item.band,
                px(NAV_ITEM_RADIUS_LOGICAL_PX),
                palette.dialog_hover,
                if selected {
                    1.0
                } else {
                    NAV_HOVER_GROUND_ALPHA
                },
            ));
        }
        if focus == Some(SettingsTarget::Nav(item.category)) {
            quads.extend(focus_ring(item.band, scale, palette.accent));
        }
        labels.push(ChromeLabel {
            text: item.category.nav_label().to_owned(),
            rect: item.label,
            font_size_px: px(NAV_ITEM_FONT_LOGICAL_PX),
            color: if selected || hovered {
                palette.dialog_title_text
            } else {
                palette.dialog_muted_text
            },
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }

    // Everything below the header is clipped to the content box, which is what
    // `max-height` plus `overflow-y` leaves when the window is too short.
    //
    // Built whole into a stack of its own and cut once, at the end, by
    // [`clip_content`]. Every box below is therefore the box the layout placed —
    // full height, at whatever offset the scroll put it — and no piece of this
    // stack asks whether it is on screen. That question has exactly one asker, and
    // a row's own drawing is not it.
    let clip = layout.content;
    let mut content_stack = OverlayLayer::default();
    for headed in &layout.groups {
        content_stack.labels.push(ChromeLabel {
            text: headed.group.label().to_owned(),
            rect: headed.label,
            font_size_px: px(GROUP_LABEL_FONT_LOGICAL_PX),
            color: palette.dialog_muted_text,
            align_right: false,
            align_center: false,
            // A ratio, so it carries no `scale`: the shaper adds it to a
            // glyph's advance before the font size multiplies both.
            letter_spacing_em: GROUP_LABEL_TRACKING_EM,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
    for placed in &layout.rows {
        // A row the machine cannot honour is greyed WHOLE — title, sentence and
        // control — and its sentence has already become the reason (see
        // `SettingsRow::description`). Same ink and same rule as the Shortcuts
        // page's reserved lines and the greyed items in an open picker: this is
        // a thing you are being shown and cannot have.
        let available = placed.row.available(values);
        content_stack.labels.push(ChromeLabel {
            text: placed.row.title().to_owned(),
            rect: placed.title,
            font_size_px: px(ROW_TITLE_FONT_LOGICAL_PX),
            color: if available {
                palette.dialog_title_text
            } else {
                palette.menu_item_hint_text
            },
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
        content_stack.labels.push(ChromeLabel {
            text: placed.row.description(values).to_owned(),
            rect: placed.desc,
            font_size_px: px(ROW_DESC_FONT_LOGICAL_PX),
            color: palette.dialog_muted_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
    // The buttons after every row's text, so a row's own control cannot be
    // covered by the *fill* of a later row's — the same channel ordering the
    // popup's layer exists for, one scale down.
    for placed in &layout.rows {
        let available = placed.row.available(values);
        match placed.row.control() {
            SettingsControl::Combo => {
                // The Background image row's button carries the chosen file's
                // NAME, because that is the row's value; its two items are the
                // two things you do to it, and neither of them is what is
                // currently set. Every other picker's button is its ticked item.
                let value =
                    if placed.row == SettingsRow::BackgroundImage && !background_image.is_empty() {
                        background_image
                    } else {
                        placed
                            .row
                            .selected_index(values)
                            .and_then(|index| placed.row.option_label(index))
                            .unwrap_or_default()
                    };
                push_combo(
                    &mut content_stack.quads,
                    &mut content_stack.labels,
                    placed.combo,
                    available && hover == Some(SettingsTarget::Combo(placed.row)),
                    value,
                    available,
                    scale,
                    border,
                    palette,
                    measure,
                );
            }
            SettingsControl::Slider(range) => {
                push_slider(
                    &mut content_stack.quads,
                    &mut content_stack.labels,
                    slider_geometry(
                        placed.combo,
                        scale,
                        range,
                        placed.row.slider_value(values).unwrap_or(range.min),
                    ),
                    placed.row.slider_value(values).unwrap_or(range.min),
                    available && hover == Some(SettingsTarget::Slider(placed.row)),
                    available,
                    scale,
                    palette,
                );
            }
        }
        // After the control it names, because a ring is a fill and a layer draws
        // its fills in order — pushed before, the control's own face would cover
        // the inner edge of it.
        if focus == Some(placed.row.control_target()) {
            content_stack
                .quads
                .extend(focus_ring(placed.combo, scale, palette.accent));
        }
    }
    push_shortcut_page(
        &mut content_stack,
        layout,
        shortcuts,
        hover,
        focus,
        recording,
        scale,
        border,
        palette,
        measure,
    );
    clip_content(clip, content_stack, &mut quads, &mut labels, &mut sprites);

    // The open picker, on a layer of its own above everything the dialog drew.
    // Not "pushed last": pushed last it covers the fills under it and none of the text,
    // which is the one channel its own face has to cover — the value and chevron
    // of the control it hangs over are captions, and captions draw after every
    // fill in their layer.
    let mut popup = OverlayLayer::default();
    if let (Some(menu), Some(row)) = (layout.menu, layout.menu_kind) {
        push_float_window(
            &mut popup.quads,
            menu,
            px(MENU_RADIUS_LOGICAL_PX),
            border,
            px(FLOAT_WINDOW_SHADOW_LOGICAL_PX),
            palette.menu_surface,
            palette.menu_shadow,
            alpha(palette.menu_popup_shadow_inner_alpha),
            alpha(palette.menu_popup_shadow_outer_alpha),
            palette.menu_border,
            alpha(palette.menu_border_alpha),
        );
        let selected = row.selected_index(values);
        let icon_advance = option_icon_advance(row, scale);
        for (index, item) in layout.items.iter().enumerate() {
            let label = row.option_label(index).unwrap_or_default();
            let is_selected = selected == Some(index);
            let enabled = row.option_enabled(index, values);
            // An item that cannot be chosen is never hovered — `hit` answers
            // `Menu` over it — but the state is read from the same predicate
            // rather than inferred from that, so the two cannot come apart.
            //
            // **The keyboard's option wears the pointer's ink**, and not the
            // `button:focus-visible` ring: that rule is declared for `button`
            // and a `.combo-item` is a `div`, so the mock-up gives an item
            // exactly one lit state and this is it. Two lit states inside one
            // open picker would also be two answers to "which one does Enter
            // take", when there is only ever one.
            let is_hovered = enabled
                && (hover == Some(SettingsTarget::Choice(row, index))
                    || focus == Some(SettingsTarget::Choice(row, index)));
            if is_hovered {
                popup.quads.extend(rounded_overlay_fill(
                    *item,
                    px(ITEM_RADIUS_LOGICAL_PX),
                    palette.menu_item_hover,
                    1.0,
                ));
            }
            let tick_left = item[0] + px(ITEM_PADDING_X_LOGICAL_PX);
            let tick_right = tick_left + px(TICK_WIDTH_LOGICAL_PX);
            if is_selected {
                popup.labels.push(ChromeLabel {
                    text: TICK.to_owned(),
                    rect: [tick_left, item[1], tick_right, item[3]],
                    font_size_px: px(TICK_FONT_LOGICAL_PX),
                    color: palette.accent,
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                    weight: ChromeLabelWeight::Regular,
                    tabular_numerals: false,
                    clip: None,
                });
            }
            let text_left = tick_right + px(ITEM_GAP_LOGICAL_PX) + icon_advance;
            if let Some(mark) = row.option_mark(index) {
                // Centred on its own 14px column, one pixel narrower than the
                // 15px mark in it — `profiles::push_row`'s arithmetic, because it
                // is the same `.ticon` holding the same `.pmark`.
                let column_left = tick_right + px(ITEM_GAP_LOGICAL_PX);
                let column_right = column_left + px(OPTION_ICON_COLUMN_LOGICAL_PX);
                let side = px(OPTION_MARK_LOGICAL_PX).round();
                let left = ((column_left + column_right - side) / 2.0).round();
                let top = ((item[1] + item[3] - side) / 2.0).round();
                let mut sprite =
                    ChromeSprite::new(mark, [left, top, left + side, top + side], palette.accent);
                if !enabled {
                    sprite.opacity = UNAVAILABLE_MARK_OPACITY;
                    sprite.grayscale = true;
                }
                popup.sprites.push(sprite);
            }
            popup.labels.push(ChromeLabel {
                text: label.to_owned(),
                rect: [
                    text_left,
                    item[1],
                    item[2] - px(ITEM_PADDING_X_LOGICAL_PX),
                    item[3],
                ],
                font_size_px: px(COMBO_FONT_LOGICAL_PX),
                // Three inks, unavailable first — the picker menu's own order and
                // its reason: an unavailable item is never hovered, and stating
                // that precedence rather than relying on it is what keeps the two
                // from disagreeing if it ever stops being true.
                color: if !enabled {
                    palette.menu_item_hint_text
                } else if is_selected || is_hovered {
                    palette.menu_item_text_selected
                } else {
                    palette.menu_item_text
                },
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            });
        }
    }

    let content = OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    };
    if popup.is_empty() {
        vec![content]
    } else {
        vec![content, popup]
    }
}

/// The shortcut page's own lines, into the scrolling stack.
///
/// It draws nothing at all on any other page, and that falls out of the layout
/// rather than being asked here: `layout.shortcuts` is empty unless the page is
/// the shortcut page, which is the same shape "a category with no rows draws no
/// heading" already has.
#[allow(clippy::too_many_arguments)]
fn push_shortcut_page(
    stack: &mut OverlayLayer,
    layout: &SettingsLayout,
    shortcuts: &[crate::shortcuts::ShortcutRow],
    hover: Option<SettingsTarget>,
    focus: Option<SettingsTarget>,
    recording: Option<(usize, &[String], Option<&str>)>,
    scale: f32,
    border: f32,
    palette: bt_render::ChromePalette,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) {
    let px = |value: f32| value * scale;
    for placed in &layout.shortcuts {
        let Some(line) = shortcuts.get(placed.index) else {
            continue;
        };
        let listening = recording.map(|(row, _, _)| row) == Some(placed.index);
        // A reserved row is greyed whole — name, note and caps — because the
        // audit did not take the chord and the line exists to say so. It is the
        // same ink an unavailable picker item wears, for the same reason: this
        // is a thing you are being shown and cannot have.
        let title_ink = if line.reserved {
            palette.menu_item_hint_text
        } else {
            palette.dialog_title_text
        };
        stack.labels.push(ChromeLabel {
            text: line.title.to_owned(),
            rect: placed.title,
            font_size_px: px(ROW_TITLE_FONT_LOGICAL_PX),
            color: title_ink,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
        // **While a row is listening, its own muted line is the recorder's.**
        // A hint printed anywhere else would be a second place to look for the
        // answer to a question asked here; a refusal shown in red where the
        // scope tag was is a sentence in the place the eye is already on.
        let (note, note_ink) = match (listening, recording) {
            (true, Some((_, _, Some(hint)))) => (Some(hint.to_owned()), palette.status_err),
            (true, Some((_, _, None))) => {
                (Some(RECORD_PROMPT.to_owned()), palette.dialog_muted_text)
            }
            _ => (
                line.note.as_ref().map(|note| note.to_string()),
                palette.dialog_muted_text,
            ),
        };
        if let Some(note) = note {
            // **Ellipsised, unlike a settings row's own description.** The rule
            // is the combo value's (`a_value_too_wide_for_its_button_is_
            // ellipsised_not_cropped`): a crop mid-glyph reads as a rendering
            // fault, an ellipsis reads as a line too long. It is applied here
            // and not to every row because this column is the one that is
            // narrow by construction — a shortcut line hands 168px to its caps,
            // and the recorder writes a whole sentence into what is left.
            let width = placed.desc[2] - placed.desc[0];
            stack.labels.push(ChromeLabel {
                text: ellipsized(&note, width, px(ROW_DESC_FONT_LOGICAL_PX), measure),
                rect: placed.desc,
                font_size_px: px(ROW_DESC_FONT_LOGICAL_PX),
                color: note_ink,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            });
        }
        let live: Vec<String> = match (listening, recording) {
            (true, Some((_, caps, _))) => caps.to_vec(),
            _ => line.caps.clone(),
        };
        push_caps(
            stack,
            placed.caps,
            &live,
            line.reserved,
            listening,
            scale,
            border,
            palette,
            measure,
        );
        if let Some(record) = placed.record {
            push_button(
                &mut stack.quads,
                &mut stack.labels,
                record,
                if listening {
                    RECORD_LISTENING_LABEL
                } else {
                    RECORD_BUTTON_LABEL
                },
                hover == Some(SettingsTarget::Record(placed.index)) || listening,
                scale,
                border,
                palette,
                measure,
            );
            if focus == Some(SettingsTarget::Record(placed.index)) {
                stack
                    .quads
                    .extend(focus_ring(record, scale, palette.accent));
            }
        }
        if let Some(restore) = placed.restore {
            push_restore_verb(
                stack,
                restore,
                hover == Some(SettingsTarget::RestoreRow(placed.index)),
                scale,
                palette,
            );
            if focus == Some(SettingsTarget::RestoreRow(placed.index)) {
                stack
                    .quads
                    .extend(focus_ring(restore, scale, palette.accent));
            }
        }
    }
    if let Some(restore_all) = layout.restore_all {
        push_button(
            &mut stack.quads,
            &mut stack.labels,
            restore_all,
            RESTORE_ALL_LABEL,
            hover == Some(SettingsTarget::RestoreAll),
            scale,
            border,
            palette,
            measure,
        );
        if focus == Some(SettingsTarget::RestoreAll) {
            stack
                .quads
                .extend(focus_ring(restore_all, scale, palette.accent));
        }
    }
}

/// A chord as key caps, laid out **right to left** from the box's right edge.
///
/// Right to left because the caps belong to the button beside them: a chord that
/// started at a fixed left edge would drift away from its own `Record` as rows
/// gained and lost their `↺`, and a chord that moves when you edit a different
/// row is a chord that looks like it changed.
///
/// An empty chord draws the word for it instead, in the muted ink — never an
/// empty space, because a blank where a chord goes is indistinguishable from a
/// row that failed to draw.
#[allow(clippy::too_many_arguments)]
fn push_caps(
    stack: &mut OverlayLayer,
    box_: [f32; 4],
    caps: &[String],
    reserved: bool,
    listening: bool,
    scale: f32,
    border: f32,
    palette: bt_render::ChromePalette,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) {
    let px = |value: f32| value * scale;
    let font_size_px = px(CAP_FONT_LOGICAL_PX);
    let middle = (box_[1] + box_[3]) / 2.0;
    let height = px(CAP_HEIGHT_LOGICAL_PX);
    if caps.is_empty() {
        // **`Not set` is a fact about the row, not about the moment.** A box
        // that is listening and has nothing down yet is showing an empty hand,
        // and printing the word for "this row has no chord" there would be the
        // panel answering a question nobody asked while the user is mid-press.
        if listening {
            return;
        }
        stack.labels.push(ChromeLabel {
            text: crate::shortcuts::UNBOUND_CAP.to_owned(),
            rect: [box_[0], box_[1], box_[2], box_[3]],
            font_size_px: px(ROW_DESC_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
        return;
    }
    let mut right = box_[2];
    for cap in caps.iter().rev() {
        let width = (measure(cap, font_size_px) + 2.0 * px(CAP_PADDING_X_LOGICAL_PX))
            .max(px(CAP_MIN_WIDTH_LOGICAL_PX));
        let rect = [
            right - width,
            middle - height / 2.0,
            right,
            middle + height / 2.0,
        ];
        right = rect[0] - px(CAP_GAP_LOGICAL_PX);
        // The combo button's own recipe at a smaller round: a hairline, then a
        // face one border in. A key cap is a control-shaped thing that is not a
        // control, and borrowing the shape is what makes it read as a key rather
        // than as a badge.
        stack.quads.extend(rounded_overlay_fill(
            rect,
            px(CAP_RADIUS_LOGICAL_PX),
            palette.menu_border,
            f32::from(palette.menu_border_alpha) / 255.0,
        ));
        stack.quads.extend(rounded_overlay_fill(
            [
                rect[0] + border,
                rect[1] + border,
                rect[2] - border,
                rect[3] - border,
            ],
            px(CAP_RADIUS_LOGICAL_PX) - border,
            if reserved {
                palette.dialog_surface
            } else {
                palette.dialog_hover
            },
            1.0,
        ));
        stack.labels.push(ChromeLabel {
            text: cap.clone(),
            rect,
            font_size_px,
            color: if reserved {
                palette.menu_item_hint_text
            } else {
                palette.dialog_title_text
            },
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
}

/// `.btn` (mock-up 2000-2008): a bordered, rounded box with a word centred in
/// it, at the picker's own height so a row's right-hand control is one object
/// wherever it appears.
#[allow(clippy::too_many_arguments)]
fn push_button(
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    rect: [f32; 4],
    text: &str,
    hovered: bool,
    scale: f32,
    border: f32,
    palette: bt_render::ChromePalette,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) {
    let px = |value: f32| value * scale;
    let radius = px(BUTTON_RADIUS_LOGICAL_PX);
    quads.extend(rounded_overlay_fill(
        rect,
        radius,
        palette.menu_border,
        f32::from(palette.menu_border_alpha) / 255.0,
    ));
    quads.extend(rounded_overlay_fill(
        [
            rect[0] + border,
            rect[1] + border,
            rect[2] - border,
            rect[3] - border,
        ],
        radius - border,
        if hovered {
            palette.dialog_hover
        } else {
            palette.dialog_surface
        },
        1.0,
    ));
    let font_size_px = px(BUTTON_FONT_LOGICAL_PX);
    labels.push(ChromeLabel {
        text: ellipsized(text, rect[2] - rect[0], font_size_px, measure),
        rect,
        font_size_px,
        color: palette.dialog_title_text,
        align_right: false,
        align_center: true,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
}

/// The `↺` beside an overridden row — the house's own three-step reveal, one
/// step short: it is only drawn at all when there is something to undo, so
/// "absent" is already doing the work "invisible until hovered" does elsewhere.
fn push_restore_verb(
    stack: &mut OverlayLayer,
    rect: [f32; 4],
    hovered: bool,
    scale: f32,
    palette: bt_render::ChromePalette,
) {
    let px = |value: f32| value * scale;
    if hovered {
        stack.quads.extend(rounded_overlay_fill(
            rect,
            px(BUTTON_RADIUS_LOGICAL_PX),
            palette.dialog_hover,
            1.0,
        ));
    }
    stack.labels.push(ChromeLabel {
        text: RESTORE_GLYPH.to_owned(),
        rect,
        font_size_px: px(BUTTON_FONT_LOGICAL_PX),
        color: if hovered {
            palette.dialog_title_text
        } else {
            palette.dialog_secondary_text
        },
        align_right: false,
        align_center: true,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
}

/// **`button:focus-visible`** (mock-up 2205) — the ring round a control the
/// keyboard is on.
///
/// A CSS `outline` is drawn *outside* the border box, `outline-offset` away from
/// it, and it follows the box's own round: so the ring's inner edge is the
/// control grown by the offset, its own radius is the control's plus that same
/// offset, and it reaches `FOCUS_RING_WIDTH` further out. Stated that way rather
/// than as four bars because the controls are rounded and four bars meet at
/// square corners.
///
/// [`bt_render::rounded_overlay_halo`] is the exact, uniform ring its own doc
/// promises to stay — the one primitive here that is a stroke rather than a
/// falloff, which is what an outline needs and what a shadow must not be.
fn focus_ring(rect: [f32; 4], scale: f32, accent: [u8; 3]) -> Vec<OverlayQuad> {
    let offset = FOCUS_RING_OFFSET_LOGICAL_PX * scale;
    bt_render::rounded_overlay_halo(
        [
            rect[0] - offset,
            rect[1] - offset,
            rect[2] + offset,
            rect[3] + offset,
        ],
        FOCUS_RING_RADIUS_LOGICAL_PX * scale + offset,
        FOCUS_RING_WIDTH_LOGICAL_PX * scale,
        accent,
        1.0,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_combo(
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    rect: [f32; 4],
    hovered: bool,
    value: &str,
    // `available`: whether this row can act. A greyed button keeps its border
    // and its face — it stays where it is, exactly as a disabled `.btn` does —
    // and only its ink steps back.
    available: bool,
    scale: f32,
    border: f32,
    palette: bt_render::ChromePalette,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) {
    let px = |logical: f32| logical * scale;
    let radius = px(COMBO_RADIUS_LOGICAL_PX);
    quads.extend(rounded_overlay_fill(
        rect,
        radius,
        palette.menu_border,
        f32::from(palette.menu_border_alpha) / 255.0,
    ));
    quads.extend(rounded_overlay_fill(
        [
            rect[0] + border,
            rect[1] + border,
            rect[2] - border,
            rect[3] - border,
        ],
        radius - border,
        if hovered {
            palette.dialog_hover
        } else {
            palette.dialog_surface
        },
        1.0,
    ));
    let chevron_column = px(COMBO_CHEVRON_FONT_LOGICAL_PX + COMBO_GAP_LOGICAL_PX);
    // `.combo > button` is a fixed 118px beside a value that is whatever the
    // chosen option is called, and the two do not negotiate: the button's width is
    // the design's, so it is the *text* that gives way. Cropping it mid-glyph is
    // what a bare clip does and it reads as a rendering fault rather than as a
    // name too long — "Windows PowerShell 5.1" arriving as "Windows Pov" is the
    // report this exists to answer.
    let value_box = [
        rect[0] + border + px(COMBO_PADDING_LEFT_LOGICAL_PX),
        rect[1],
        rect[2] - border - px(COMBO_PADDING_RIGHT_LOGICAL_PX) - chevron_column,
        rect[3],
    ];
    let font_size_px = px(COMBO_FONT_LOGICAL_PX);
    labels.push(ChromeLabel {
        text: ellipsized(value, value_box[2] - value_box[0], font_size_px, measure),
        rect: value_box,
        font_size_px,
        color: if available {
            palette.dialog_title_text
        } else {
            palette.menu_item_hint_text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
    labels.push(ChromeLabel {
        text: COMBO_CHEVRON.to_owned(),
        rect: [
            rect[0],
            rect[1],
            rect[2] - border - px(COMBO_PADDING_RIGHT_LOGICAL_PX),
            rect[3],
        ],
        font_size_px: px(COMBO_CHEVRON_FONT_LOGICAL_PX),
        color: palette.dialog_muted_text,
        align_right: true,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
}

/// The dialog's other control: a track, the part of it that is filled, a thumb
/// and the number (§7.1.6c-4b).
///
/// Painted from [`SliderGeometry`] and nothing else, which is the same rectangle
/// the hit test and the drag measure against — a track drawn from one derivation
/// and hit from another is a thumb that does not land where the pointer is.
///
/// **The thumb is the accent and so is the filled half**, because they are one
/// statement read two ways: the fill says how much, the thumb says where you
/// would take hold. The empty half is the border colour and not a tint of the
/// accent — an accent at low opacity reads as a *disabled* accent, and there is
/// nothing disabled about the part of the range you have not chosen.
#[allow(clippy::too_many_arguments)]
fn push_slider(
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    geometry: SliderGeometry,
    value: u8,
    hovered: bool,
    available: bool,
    scale: f32,
    palette: bt_render::ChromePalette,
) {
    let px = |logical: f32| logical * scale;
    let radius = px(SLIDER_TRACK_RADIUS_LOGICAL_PX);
    let lit = if available {
        palette.accent
    } else {
        palette.menu_item_hint_text
    };
    quads.extend(rounded_overlay_fill(
        geometry.track,
        radius,
        palette.menu_border,
        f32::from(palette.menu_border_alpha) / 255.0,
    ));
    // Only when there is something to fill: a zero-width rounded rectangle is
    // two half-circles overlapping, which draws as a dot at the left-hand end of
    // a track whose value is its minimum.
    if geometry.fill[2] > geometry.fill[0] + radius {
        quads.extend(rounded_overlay_fill(geometry.fill, radius, lit, 1.0));
    }
    // The thumb grows under the pointer rather than changing colour, which is
    // the mock-up's own `transform: scale(1.15)` — a hover that recoloured the
    // one accent shape on the row would read as a state change rather than as
    // "this is the part you can grab".
    let thumb = if hovered {
        let grow = (geometry.thumb[2] - geometry.thumb[0]) * (SLIDER_THUMB_HOVER_SCALE - 1.0) / 2.0;
        [
            geometry.thumb[0] - grow,
            geometry.thumb[1] - grow,
            geometry.thumb[2] + grow,
            geometry.thumb[3] + grow,
        ]
    } else {
        geometry.thumb
    };
    quads.extend(rounded_overlay_fill(
        thumb,
        (thumb[3] - thumb[1]) / 2.0,
        lit,
        1.0,
    ));
    labels.push(ChromeLabel {
        text: format!("{value}%"),
        rect: geometry.value,
        font_size_px: px(SLIDER_VALUE_FONT_LOGICAL_PX),
        color: if available {
            palette.dialog_muted_text
        } else {
            palette.menu_item_hint_text
        },
        align_right: true,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        // Tabular, because the number changes while you drag and a proportional
        // 8 is narrower than a proportional 0 — without this the track would
        // breathe in and out under the thumb.
        tabular_numerals: true,
        clip: None,
    });
}

/// A surface that floats: its lift, its hairline, its face — the three planes
/// the hover-peek flyout is already built from, in the same order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_float_window(
    quads: &mut Vec<OverlayQuad>,
    frame: [f32; 4],
    radius: f32,
    border: f32,
    spread: f32,
    face: [u8; 3],
    shadow: [u8; 3],
    shadow_inner_alpha: f32,
    shadow_outer_alpha: f32,
    hairline: [u8; 3],
    hairline_alpha: f32,
) {
    // **One soft falloff, not two rings** (user report, 2026-08-13). The two
    // rings this used to draw — the whole spread at the fainter alpha, half of it
    // at the darker — are two plateaus of constant alpha, each half the spread
    // wide, and at a card's 28px that is a pair of concentric squares around the
    // box rather than a shadow. `rounded_overlay_shadow` samples the curve
    // instead, in bands a pixel or two wide, anchored on what the old pair
    // composited to right against the box so nothing got lighter where it was
    // darkest. Every surface that floats through this door is the same shape now.
    quads.extend(rounded_overlay_shadow(
        frame,
        radius,
        spread,
        shadow,
        bt_render::overlay_shadow_alpha(shadow_inner_alpha, shadow_outer_alpha),
    ));
    quads.extend(rounded_overlay_fill(
        frame,
        radius,
        hairline,
        hairline_alpha,
    ));
    // Concentric with the box: one border in on every side, so one border less
    // radius — anything else thickens the hairline through the corner.
    quads.extend(rounded_overlay_fill(
        [
            frame[0] + border,
            frame[1] + border,
            frame[2] - border,
            frame[3] - border,
        ],
        radius - border,
        face,
        1.0,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window big enough that nothing is clamped: the shape every geometry
    /// claim below is stated against.
    /// The window every geometry claim below is stated against.
    ///
    /// **1100 and not 800 since §7.1.6c-4b.** The Appearance page took six more
    /// rows that day and 103 + 15×54 = 913 no longer fits under 800's
    /// `max-height: calc(100% - 72px)` — so every "the page decides the height"
    /// pin would have been measuring the *clamp* instead. The clamp has pins of
    /// its own (`a_row_at_the_content_edge_is_cut_and_never_compressed`,
    /// `a_stack_taller_than_the_dialog_scrolls`), and they say what window they
    /// want rather than relying on this one being too small.
    const SURFACE: (f32, f32) = (1280.0, 1100.0);

    /// `.row`'s own height at scale 1: `2 * 11` of padding around the taller of
    /// the two-line text column (16.5 + 1 + 14.5) and the 27.5 control.
    const ROW_HEIGHT: f32 = 54.0;

    /// The dialog's height at scale 1 for a page holding `rows` rows under its
    /// one heading: two hairlines, the header's `16 + 30 + 10`, and
    /// `2 + 10 + 13 + 2 + rows * 54 + 18` of content.
    ///
    /// **One heading, never more** since the rail arrived: a page holds one
    /// category, so the `margin-top: 16px` a later heading used to take has
    /// nothing left to stand off. The `groups` argument went with it.
    fn dialog_height(rows: usize) -> f32 {
        103.0 + ROW_HEIGHT * rows as f32
    }

    /// The rows a dialog holds with the tabs across the top — the state the app
    /// opens in, and what every claim below is stated against unless it says
    /// otherwise.
    fn flat_rows() -> Vec<SettingsRow> {
        visible_rows(TabLayoutMode::Horizontal)
    }

    /// The shortcut table as the panel would show it, for the claims that are
    /// about it.
    fn shortcut_lines() -> Vec<crate::shortcuts::ShortcutRow> {
        crate::shortcuts::Shortcuts::defaults().editor_rows()
    }

    /// A dialog holding these rows and these shortcut lines.
    fn content<'a>(
        rows: &'a [SettingsRow],
        shortcuts: &'a [crate::shortcuts::ShortcutRow],
    ) -> SettingsContent<'a> {
        SettingsContent { rows, shortcuts }
    }

    /// The page every geometry claim that is not about the rail or the shortcut
    /// table is stated against: the one the mock-up's own rows live on.
    const PAGE: SettingsCategory = SettingsCategory::Appearance;

    /// A representative reading for every row, for the tests that are about
    /// geometry rather than about which value is shown.
    fn values() -> SettingsValues {
        SettingsValues::sample()
    }

    fn open(scale: f32, menu_open: bool) -> SettingsLayout {
        let rows = flat_rows();
        layout_for_menu(
            (SURFACE.0 * scale).round(),
            (SURFACE.1 * scale).round(),
            scale,
            menu_open.then_some(SettingsRow::Theme),
            content(&rows, &[]),
            PAGE,
            0.0,
            UNSCROLLED,
        )
        .expect("this window can host the dialog")
    }

    fn open_cursor(scale: f32) -> SettingsLayout {
        open_rows(scale, Some(SettingsRow::Cursor), TabLayoutMode::Horizontal)
    }

    /// The dialog as it stands with the tabs on this axis, with one row's picker
    /// open or none.
    fn open_rows(
        scale: f32,
        menu: Option<SettingsRow>,
        tab_layout: TabLayoutMode,
    ) -> SettingsLayout {
        open_rows_measured(scale, menu, tab_layout, 0.0)
    }

    /// The same dialog, told how wide the open picker's longest label measures.
    /// Zero is the honest reading for every caller above: their options are one
    /// short word each and the popup's floor — the button's own width — wins.
    fn open_rows_measured(
        scale: f32,
        menu: Option<SettingsRow>,
        tab_layout: TabLayoutMode,
        widest_option: f32,
    ) -> SettingsLayout {
        open_page(
            scale,
            menu,
            tab_layout,
            menu.map_or(PAGE, SettingsRow::category),
            widest_option,
        )
    }

    /// One named page of the dialog.
    fn open_page(
        scale: f32,
        menu: Option<SettingsRow>,
        tab_layout: TabLayoutMode,
        category: SettingsCategory,
        widest_option: f32,
    ) -> SettingsLayout {
        let rows = visible_rows(tab_layout);
        let shortcuts = shortcut_lines();
        layout_for_menu(
            SURFACE.0 * scale,
            SURFACE.1 * scale,
            scale,
            menu,
            content(&rows, &shortcuts),
            category,
            widest_option,
            UNSCROLLED,
        )
        .expect("the settings dialog fits")
    }

    /// The page a row lives on, opened.
    fn open_showing(row: SettingsRow) -> SettingsLayout {
        open_page(1.0, None, TabLayoutMode::Vertical, row.category(), 0.0)
    }
    /// A dialog nobody has turned a wheel over — what every claim that is not
    /// about scrolling is stated against.
    const UNSCROLLED: f32 = 0.0;

    /// A row's boxes, which every geometry claim below names rather than
    /// reaching for a field that no longer exists.
    fn row_of(placed: &SettingsLayout, row: SettingsRow) -> RowLayout {
        *placed.row(row).expect("the dialog holds this row")
    }

    fn combo_of(placed: &SettingsLayout, row: SettingsRow) -> [f32; 4] {
        row_of(placed, row).combo
    }

    fn centre(rect: [f32; 4]) -> (f64, f64) {
        (
            f64::from((rect[0] + rect[2]) / 2.0),
            f64::from((rect[1] + rect[3]) / 2.0),
        )
    }

    fn width(rect: [f32; 4]) -> f32 {
        rect[2] - rect[0]
    }

    fn height(rect: [f32; 4]) -> f32 {
        rect[3] - rect[1]
    }

    /// PIN (mock-up geometry): the dialog lands exactly where
    /// `design/ui-mockup.html` puts it — `width: min(480px, 92%)`,
    /// `margin: 54px auto 0`, and a height its own content decides.
    ///
    /// The height is not a guess: it is `1 + 56 + content + 1` — two hairlines,
    /// the header's `16 + 30 + 10`, and a content box of
    /// `2 + 10 + 13 + 2 + rows * (11 + 32 + 11) + 18`, plus `16 + 13 + 2` for
    /// every heading after the first. The mock-up's own renderer reported 211 for
    /// the two rows under one heading this dialog first shipped with, which is
    /// [`dialog_height`] at `(2, 1)` and the number the formula is anchored on.
    ///
    /// Red gate: every term is load-bearing. Drop the `auto` centring and `left`
    /// moves; drop the 54 and `top` moves; use the row's *border* box (55, which
    /// is what it measures when it is not the last child) and every height here
    /// is one pixel per row too big.
    #[test]
    fn the_dialog_lands_where_the_mock_up_puts_it() {
        assert_eq!(
            dialog_height(2),
            211.0,
            "the mock-up's own measurement: two rows under one heading, which is \
             what this dialog first shipped with"
        );
        let placed = open(1.0, false);
        assert_eq!(width(placed.frame), 720.0, "min(720px, 92%) at 1280 wide");
        assert_eq!(placed.frame[1], 54.0, "margin-top: 54px");
        assert_eq!(
            placed.frame[0],
            (SURFACE.0 - 720.0) / 2.0,
            "margin-left/right: auto"
        );
        let page = SettingsCategory::Appearance;
        let rows = flat_rows();
        let on_page = rows.iter().filter(|row| row.category() == page).count();
        assert_eq!(
            height(placed.frame),
            dialog_height(on_page),
            "the page decides the height, its one heading included"
        );

        // The 92% share takes over below 720/0.92 ~= 782.6 logical pixels.
        let narrow = layout_for_menu(
            720.0,
            800.0,
            1.0,
            None,
            content(&rows, &[]),
            page,
            0.0,
            UNSCROLLED,
        )
        .expect("720 wide still hosts the dialog");
        assert_eq!(
            width(narrow.frame),
            (720.0_f32 * DIALOG_WIDTH_RATIO).round(),
            "under the cap the dialog takes 92% of the window"
        );
        assert_eq!(
            narrow.frame[0],
            ((720.0 - width(narrow.frame)) / 2.0).round()
        );
    }

    /// PIN (mock-up geometry): every control inside the dialog is the box the
    /// stylesheet gives it, at the offset the stylesheet puts it at.
    ///
    /// Read against the mock-up's own measurements, taken with the dialog open
    /// at 1280x800: `.dlg-close` 30x30 with its icon ending 22 from the dialog's
    /// edge, `.combo > button` 118x27.5 flush with the content's right padding,
    /// `.row .text` one gap to its left.
    #[test]
    fn every_control_is_the_box_the_stylesheet_gives_it() {
        let placed = open(1.0, false);
        let theme = row_of(&placed, SettingsRow::Theme);
        let cursor = row_of(&placed, SettingsRow::Cursor);
        assert_eq!((width(placed.close), height(placed.close)), (30.0, 30.0));
        assert_eq!(
            placed.frame[2] - placed.close[2],
            1.0 + HEADER_PADDING_RIGHT_LOGICAL_PX,
            "the 30px button ends 12 inside the dialog's hairline, so its 10px \
             icon ends 22 from the edge - the mock-up's own reasoning"
        );
        assert_eq!(
            placed.close[1],
            placed.frame[1] + 1.0 + 16.0,
            "padding-top: 16px"
        );

        assert_eq!(width(theme.combo), 118.0, "min-width: 118px");
        assert_eq!(height(theme.combo), 27.5, "5 + 15.5 + 5 + two borders");
        assert_eq!(
            placed.frame[2] - theme.combo[2],
            1.0 + CONTENT_PADDING_X_LOGICAL_PX + ROW_PADDING_X_LOGICAL_PX,
            "the control is flush with the row's own right edge"
        );
        assert_eq!(
            theme.combo[0] - theme.title[2],
            ROW_GAP_LOGICAL_PX,
            ".row gap: 16px between the text column and the control"
        );
        // `align-items: center`: the 27.5 control is centred on the 32 the two
        // stacked lines take, not top-aligned with them.
        let text_axis = (theme.title[1] + theme.desc[3]) / 2.0;
        let combo_axis = (theme.combo[1] + theme.combo[3]) / 2.0;
        assert!(
            (text_axis - combo_axis).abs() <= 0.5,
            "the row's items share one axis: {text_axis} vs {combo_axis}"
        );
        assert_eq!(
            theme.desc[1] - theme.title[3],
            ROW_DESC_MARGIN_TOP_LOGICAL_PX,
            ".desc margin-top: 1px"
        );
        assert_eq!(width(cursor.combo), width(theme.combo));
        assert_eq!(height(cursor.combo), height(theme.combo));
        assert_eq!(
            cursor.combo[1] - theme.combo[1],
            9.0 * ROW_HEIGHT,
            "Cursor is nine identical rows under Theme — the two scheme rows              (§7.1.6c-4a) and the six the window's ground took (§7.1.6c-4b) sit              between them, and every one of them is the same height"
        );
    }

    /// PIN (DPI): the dialog is one design at every scale — nothing collapses,
    /// nothing keeps a scale-1 size, and every box stays within a rounding pixel
    /// of the scaled design number.
    ///
    /// Red gate: a box built from an unscaled constant passes at 1.0 and fails
    /// at every other entry in this list.
    #[test]
    fn the_dialog_is_one_design_at_every_dpi() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let placed = open(scale, true);
            let near = |want: f32, got: f32, what: &str| {
                assert!(
                    (got - want).abs() <= 1.0,
                    "scale {scale}: {what} is {got}, wanted {want}"
                );
            };
            let combo = combo_of(&placed, SettingsRow::Theme);
            near(720.0 * scale, width(placed.frame), "the dialog's width");
            near(
                dialog_height(
                    flat_rows()
                        .iter()
                        .filter(|row| row.category() == PAGE)
                        .count(),
                ) * scale,
                height(placed.frame),
                "the dialog's height",
            );
            near(54.0 * scale, placed.frame[1], "the dialog's drop");
            near(30.0 * scale, width(placed.close), "the close button");
            near(30.0 * scale, height(placed.close), "the close button");
            near(118.0 * scale, width(combo), "the combo");
            near(27.5 * scale, height(combo), "the combo");
            let menu = placed.menu.expect("the menu is open");
            near(118.0 * scale, width(menu), "the menu");
            near(
                (2.0 + 8.0 + THEME_OPTIONS.len() as f32 * 27.5) * scale,
                height(menu),
                "the menu",
            );
            for item in &placed.items {
                near(27.5 * scale, height(*item), "a menu item");
            }
            assert_eq!(placed.items.len(), THEME_OPTIONS.len());
        }
    }

    /// PIN (modal): while the dialog is up every point in the window belongs to
    /// the overlay, and the points that are *not* the dialog belong to the
    /// scrim — including the caption run the gear itself lives in.
    ///
    /// Red gate: the gear's own box is computed here from the same numbers
    /// `seats::hit_window_chrome` uses, and that function is asserted to still
    /// call it the gear. If the point were not really the gear the claim below
    /// would be vacuous.
    #[test]
    fn the_scrim_swallows_the_caption_run_the_gear_lives_in() {
        let placed = open(1.0, false);
        let (surface_width, scale) = (SURFACE.0, 1.0_f32);
        let button = bt_render::WINDOW_CAPTION_BUTTON_LOGICAL_PX * scale;
        let gear = f64::from(surface_width - 4.0 * button + button / 2.0);
        let y = f64::from(bt_render::WINDOW_TITLE_BAR_LOGICAL_PX * scale / 2.0);
        assert_eq!(
            crate::seats::hit_window_chrome(
                surface_width,
                scale,
                crate::seats::RailState::default(),
                gear,
                y,
            ),
            Some(crate::seats::ChromeTarget::Settings),
            "the point under test really is the gear"
        );
        assert_eq!(
            hit(&placed, values(), gear, y),
            SettingsTarget::Scrim,
            "a modal means MODAL: the gear is behind the scrim like everything else"
        );
    }

    /// PIN (modal): a sweep of the whole window never lands outside the overlay,
    /// and every point that is not the dialog or its menu is the scrim.
    #[test]
    fn no_point_in_the_window_escapes_the_overlay() {
        let placed = open(1.0, true);
        let menu = placed.menu.expect("the menu is open");
        let mut seen_scrim = false;
        let mut seen_panel = false;
        let mut y = 0.0_f32;
        while y < SURFACE.1 {
            let mut x = 0.0_f32;
            while x < SURFACE.0 {
                let target = hit(&placed, values(), f64::from(x), f64::from(y));
                let inside_dialog = contains(placed.frame, x, y);
                let inside_menu = contains(menu, x, y);
                match target {
                    SettingsTarget::Scrim => {
                        assert!(
                            !inside_dialog && !inside_menu,
                            "({x}, {y}) is on the dialog and was called scrim"
                        );
                        seen_scrim = true;
                    }
                    _ => {
                        assert!(
                            inside_dialog || inside_menu,
                            "({x}, {y}) is behind the scrim and was called {target:?}"
                        );
                        seen_panel = true;
                    }
                }
                x += 7.0;
            }
            y += 7.0;
        }
        assert!(seen_scrim && seen_panel, "the sweep must reach both");
    }

    /// PIN (hit test): every control answers where it is drawn, and the dialog's
    /// own body answers "nowhere" rather than "close".
    #[test]
    fn every_control_answers_where_it_is_drawn() {
        let placed = open(1.0, true);
        let (x, y) = centre(placed.close);
        assert_eq!(hit(&placed, values(), x, y), SettingsTarget::Close);
        // Every row's button, on a dialog with nothing open over them — with a
        // picker up the row it hangs over belongs to the picker, which is
        // `a_press_inside_an_open_picker_never_reaches_the_row_beneath_it`.
        let shut = open(1.0, false);
        for placed_row in &shut.rows {
            let (x, y) = centre(placed_row.combo);
            assert_eq!(
                hit(&shut, values(), x, y),
                placed_row.row.control_target(),
                "{:?}'s control must answer for its own row",
                placed_row.row
            );
        }
        for (index, item) in placed.items.iter().enumerate() {
            let (x, y) = centre(*item);
            assert_eq!(
                hit(&placed, values(), x, y),
                SettingsTarget::Choice(SettingsRow::Theme, index),
                "item {index} must answer for its own option"
            );
        }
        // The menu's own padding: inside the popup, on none of its items.
        let menu = placed.menu.expect("the menu is open");
        assert_eq!(
            hit(
                &placed,
                values(),
                f64::from(menu[0] + 1.0),
                f64::from(menu[1] + 1.0)
            ),
            SettingsTarget::Menu(SettingsRow::Theme)
        );
        // The header, left of the title, is the dialog and nothing more.
        assert_eq!(
            hit(
                &placed,
                values(),
                f64::from(placed.frame[0] + 4.0),
                f64::from(placed.frame[1] + 4.0),
            ),
            SettingsTarget::Panel
        );
    }

    /// PIN: the mapping a press on a picker item makes to `set_theme` — the one
    /// thing between "the user clicked Light" and the process being light.
    ///
    /// Red gate: nothing else in the overlay asks for a theme, so a router that
    /// switched on, say, the combo button would fail the second half.
    #[test]
    fn only_a_picker_item_asks_for_a_theme_and_it_asks_for_its_own() {
        let placed = open(1.0, true);
        for (index, item) in placed.items.iter().enumerate() {
            let (x, y) = centre(*item);
            assert_eq!(
                theme_requested(hit(&placed, values(), x, y)),
                Some(THEME_OPTIONS[index])
            );
        }
        assert_eq!(
            THEME_OPTIONS,
            [ThemeModeV1::Light, ThemeModeV1::Dark, ThemeModeV1::System,]
        );
        for target in [
            SettingsTarget::Scrim,
            SettingsTarget::Panel,
            SettingsTarget::Close,
            SettingsTarget::Combo(SettingsRow::Theme),
            SettingsTarget::Menu(SettingsRow::Theme),
        ] {
            assert_eq!(
                theme_requested(target),
                None,
                "{target:?} asks for no theme"
            );
        }
    }

    #[test]
    fn each_cursor_picker_item_maps_to_its_corresponding_set_value() {
        let placed = open_cursor(1.0);
        assert_eq!(placed.items.len(), CURSOR_OPTIONS.len());
        for (index, item) in placed.items.iter().enumerate() {
            let (x, y) = centre(*item);
            let target = hit(&placed, values(), x, y);
            assert_eq!(target, SettingsTarget::Choice(SettingsRow::Cursor, index));
            assert_eq!(cursor_style_requested(target), Some(CURSOR_OPTIONS[index]));
        }
        for target in [
            SettingsTarget::Scrim,
            SettingsTarget::Panel,
            SettingsTarget::Close,
            SettingsTarget::Combo(SettingsRow::Theme),
            SettingsTarget::Menu(SettingsRow::Theme),
            SettingsTarget::Combo(SettingsRow::Cursor),
            SettingsTarget::Menu(SettingsRow::Cursor),
        ] {
            assert_eq!(cursor_style_requested(target), None);
        }
    }

    #[test]
    fn cursor_combo_reuses_theme_combo_geometry_and_menu_craft() {
        let placed = open_cursor(1.0);
        let theme = combo_of(&placed, SettingsRow::Theme);
        let cursor = combo_of(&placed, SettingsRow::Cursor);
        assert_eq!(width(cursor), width(theme));
        assert_eq!(height(cursor), height(theme));
        // Measured against the row immediately above it rather than against
        // Theme: the two scheme rows moved in between (§7.1.6c-4a) and the
        // window's ground took six more (§7.1.6c-4b), and what this pins is the
        // stacking step, not which row happens to be second.
        let above = combo_of(&placed, SettingsRow::AlwaysOnTop);
        assert_eq!(
            cursor[1] - above[1],
            ROW_HEIGHT,
            "the rows stack exactly one row height apart"
        );
        let (x, y) = centre(cursor);
        assert_eq!(
            hit(&placed, values(), x, y),
            SettingsTarget::Combo(SettingsRow::Cursor)
        );
        assert_eq!(placed.items.len(), 3);
        let labels = labels_of(&placed, None, values());
        for label in ["Bar", "Block", "Underline"] {
            assert!(labels.iter().any(|candidate| candidate.text == label));
        }
    }

    /// PIN: the picker's menu opens below its button, and flips above it when
    /// there is no room — measured against the window, which is what actually
    /// clips it (the mock-up's own rule, and its `.content` does not scroll).
    #[test]
    fn the_menu_opens_below_and_flips_up_when_it_would_spill() {
        let tall = open(1.0, true);
        let tall_combo = combo_of(&tall, SettingsRow::Theme);
        let menu = tall.menu.expect("the menu is open");
        assert_eq!(
            menu[1] - tall_combo[3],
            MENU_OFFSET_LOGICAL_PX,
            "top: calc(100% + 4px)"
        );
        assert_eq!(menu[2], tall_combo[2], "right: 0");
        assert_eq!(
            width(menu),
            width(tall_combo),
            "min-width: 100% — with one short word per option the floor is the width"
        );

        // A window whose bottom is right under the combo leaves no room below.
        let rows = flat_rows();
        let short = layout_for_menu(
            1280.0,
            200.0,
            1.0,
            Some(SettingsRow::Theme),
            content(&rows, &[]),
            PAGE,
            0.0,
            UNSCROLLED,
        )
        .expect("200 tall still hosts the dialog");
        let short_combo = combo_of(&short, SettingsRow::Theme);
        let menu = short.menu.expect("the menu is open");
        assert_eq!(
            short_combo[1] - menu[3],
            MENU_OFFSET_LOGICAL_PX,
            "flipped up, the same 4px gap sits above the button"
        );
        assert!(
            menu[1] >= 0.0,
            "the flipped menu still starts inside the window"
        );
        // Items keep their order under the flip: the first option is still the
        // top one, so "the second item is Dark" does not depend on the flip.
        assert_eq!(short.items.len(), THEME_OPTIONS.len());
        assert!(short.items[0][1] < short.items[1][1]);
    }

    /// A settings list of `count` rows, cycling the real ones.
    ///
    /// Real rows and not invented ones: a row carries its own category, and it
    /// is the category changes that put a heading into the stack, so a synthetic
    /// row would be measuring an easier shape than the dialog actually holds.
    /// Twenty of these is the row count this module's growth is pinned against.
    ///
    /// **All from one page**, since a page is what a layout lays out: cycling
    /// rows across two categories would also break the contiguity the heading
    /// derivation depends on, which is a different bug wearing this test's
    /// clothes.
    fn many_rows(count: usize) -> Vec<SettingsRow> {
        let cycle: Vec<SettingsRow> = visible_rows(TabLayoutMode::Vertical)
            .into_iter()
            .filter(|row| row.category() == PAGE)
            .collect();
        (0..count).map(|index| cycle[index % cycle.len()]).collect()
    }

    /// The dialog holding `rows`, scrolled this far, in a normal window.
    fn scrolled(rows: &[SettingsRow], scroll: f32) -> SettingsLayout {
        layout_for_menu(
            SURFACE.0,
            SURFACE.1,
            1.0,
            None,
            content(rows, &[]),
            PAGE,
            0.0,
            scroll,
        )
        .expect("this window hosts the dialog")
    }

    /// PIN (Bug 2): the dialog never grows out of the window it is in.
    ///
    /// `max-height: calc(100% - 72px)` is a cap, and a cap that content is
    /// allowed to overrun is not one. Before the content scrolled, the stack was
    /// laid out from the content box's top with no offset and simply ran past the
    /// bottom edge — at seven rows the Startup group's Default profile row was
    /// drawn below the frame and clipped away, which is exactly what the user saw.
    #[test]
    fn a_stack_taller_than_the_window_leaves_the_dialog_inside_it() {
        let rows = many_rows(20);
        let placed = scrolled(&rows, 0.0);
        assert_eq!(
            placed.frame[1], DIALOG_TOP_LOGICAL_PX,
            "the dialog keeps its fixed top margin"
        );
        assert!(
            placed.frame[3] <= SURFACE.1 - DIALOG_BOTTOM_LOGICAL_PX,
            "20 rows must not push the frame past the window's bottom margin: \
             frame {:?} in a {}px window",
            placed.frame,
            SURFACE.1
        );
        assert!(
            placed.max_scroll() > 0.0,
            "a stack this tall overflows, so it must report somewhere to scroll"
        );
        // The overflow is exactly what the cap refused, so scrolling to the end
        // puts the last row's bottom on the content box's bottom.
        let end = scrolled(&rows, placed.max_scroll());
        let last = end.rows.last().expect("20 rows were placed");
        assert!(
            last.combo[3] <= end.content[3] + f32::EPSILON,
            "scrolled to the end, the last row is inside the content box: \
             combo {:?} content {:?}",
            last.combo,
            end.content
        );
    }

    /// PIN (Bug 2): every row can be brought into reach and pressed.
    ///
    /// The half of "it scrolls" that a height assertion cannot see. A row drawn
    /// inside the content box but still answering `Panel` to a press would be a
    /// dialog that shows a control it will not accept — so the claim is stated
    /// through `hit`, at the row's own combo, and not through the geometry the
    /// draw call happens to read.
    #[test]
    fn every_row_scrolls_into_reach_and_answers_a_press() {
        let rows = many_rows(20);
        let reference = scrolled(&rows, 0.0);
        let max = reference.max_scroll();
        for index in 0..rows.len() {
            // Where this row sits when nothing is scrolled, turned into the
            // offset that puts it at the top of the content box — clamped, which
            // is what makes the first and last rows work without a special case.
            let unscrolled = reference.rows[index].combo;
            let want = (unscrolled[1] - reference.content[1]).clamp(0.0, max);
            let placed = scrolled(&rows, want);
            let combo = placed.rows[index].combo;
            assert!(
                placed.shows(combo),
                "row {index} ({:?}) never comes fully inside the content box: \
                 combo {combo:?} content {:?} scroll {want}",
                rows[index],
                placed.content
            );
            let (x, y) = centre(combo);
            assert_eq!(
                hit(&placed, values(), x, y),
                rows[index].control_target(),
                "row {index} ({:?}) is visible but does not answer a press",
                rows[index]
            );
        }
    }

    /// PIN (Bug 2): a row that has been scrolled out of the box is not pressable.
    ///
    /// The other side of the same rule, and the reason `hit` asks `shows` rather
    /// than testing the combo alone: the boxes still exist off the end of the
    /// content box, and a press landing on one of them would be the dialog acting
    /// on a control that is not on screen.
    #[test]
    fn a_row_scrolled_out_of_the_content_box_takes_no_press() {
        let rows = many_rows(20);
        let placed = scrolled(&rows, 0.0);
        let last = placed.rows.last().expect("20 rows were placed");
        assert!(
            last.combo[3] > placed.content[3],
            "this fixture depends on the last row overflowing"
        );
        let (x, y) = centre(last.combo);
        assert_eq!(
            hit(&placed, values(), x, y),
            SettingsTarget::Scrim,
            "a combo below the dialog's own bottom edge is scrim, not a control"
        );
    }

    /// PIN (Bug 2): a list that fits does not scroll, and cannot be made to.
    ///
    /// The dialog as it stands today, so the wheel is a no-op until the row list
    /// actually outgrows the window — and the clamp is what makes the runtime's
    /// unclamped field safe to keep across a resize.
    #[test]
    fn a_stack_that_fits_reports_nowhere_to_scroll() {
        let rows = visible_rows(TabLayoutMode::Vertical);
        let placed = scrolled(&rows, 0.0);
        assert_eq!(
            placed.max_scroll(),
            0.0,
            "seven rows fit a {}px window with room to spare",
            SURFACE.1
        );
        let shoved = scrolled(&rows, 4_000.0);
        assert_eq!(
            shoved.rows[0].combo, placed.rows[0].combo,
            "an offset a list this short cannot honour is clamped away, not applied"
        );
    }

    /// PIN (Bug 2): the picker lets go of a row that scrolls out from under it.
    ///
    /// A popup is anchored to its button. The same sentence the Sidebar row
    /// already answers when the Tab layout combo takes it out of the list, asked
    /// of the other way a row can leave: the content moving under it.
    #[test]
    fn a_picker_whose_row_scrolls_away_is_not_open() {
        let rows = many_rows(20);
        let anchored = layout_for_menu(
            SURFACE.0,
            SURFACE.1,
            1.0,
            Some(SettingsRow::Theme),
            content(&rows, &[]),
            PAGE,
            0.0,
            UNSCROLLED,
        )
        .expect("this window hosts the dialog");
        assert!(
            anchored.menu.is_some(),
            "the Theme row is on screen unscrolled, so its picker hangs from it"
        );
        let past = layout_for_menu(
            SURFACE.0,
            SURFACE.1,
            1.0,
            Some(SettingsRow::Theme),
            content(&rows, &[]),
            PAGE,
            0.0,
            anchored.max_scroll(),
        )
        .expect("this window hosts the dialog");
        assert!(
            past.menu.is_none(),
            "scrolled to the end, the Theme row is gone and its picker has \
             nothing to hang from"
        );
    }

    /// The value a row's button shows for [`values`], which is the string the
    /// button has to fit — `None` for a row that has no button.
    ///
    /// A slider has no ticked item and no ellipsis to get wrong: its caption is
    /// three characters of tabular digits in a column measured for four, so the
    /// two pins that read a drawn value skip it rather than inventing a claim
    /// about a control that cannot overflow.
    fn shown_value(row: SettingsRow) -> Option<&'static str> {
        if row.control().range().is_some() {
            return None;
        }
        Some(
            row.selected_index(values())
                .and_then(|index| row.option_label(index))
                .expect("every picker row this dialog holds reads something"),
        )
    }

    /// The box a button's value is laid out in at 1x — the button less its two
    /// hairlines, its padding and the chevron's reserved column.
    ///
    /// [`push_combo`]'s own arithmetic, written once here so the two pins that
    /// read a drawn value find it the same way.
    fn combo_value_box(combo: [f32; 4]) -> [f32; 4] {
        [
            combo[0] + 1.0 + COMBO_PADDING_LEFT_LOGICAL_PX,
            combo[1],
            combo[2]
                - 1.0
                - COMBO_PADDING_RIGHT_LOGICAL_PX
                - COMBO_CHEVRON_FONT_LOGICAL_PX
                - COMBO_GAP_LOGICAL_PX,
            combo[3],
        ]
    }

    /// PIN (Bug 1 — the edge rows were compressed): a row that only partly
    /// reaches the content box is **cut**, never shortened.
    ///
    /// The report, on a scrolled dialog: the row at the top edge (Theme) and the
    /// one at the bottom (Default profile) came out *squat* — the combo a shorter
    /// pill with its corners rounded on the cut, the title jammed against the
    /// edge — rather than whole rows with their overflow hidden. "无论上边下边,
    /// 显示不全的框的高度都会被压缩."
    ///
    /// The cause was that every piece of the stack was built **from**
    /// `clipped(box, content)`: the crop was handed in as the geometry, so a
    /// `ChromeLabel` re-laid-out and re-centred its glyphs inside the sliver, and
    /// `rounded_overlay_fill` put a real corner wherever the cut happened to fall.
    /// Clamping a rectangle and clipping a drawing look alike from a distance and
    /// are not the same operation.
    ///
    /// Stated as the law [`clip_content`] installs: **geometry translates, only
    /// drawing is cut.** A label's layout box is the row's own box at every
    /// offset, wearing the crop as a `clip` beside it; the button's fills are the
    /// whole button's fills with whatever lies outside dropped.
    ///
    /// Red gate: hand the crop back to either channel — `rect: clipped(...)` on
    /// the labels, or `push_combo(..., clipped(placed.combo, clip), ...)` on the
    /// fills — and the matching half of this fails on the first cut row.
    #[test]
    fn a_row_at_the_content_edge_is_cut_and_never_compressed() {
        let rows = many_rows(20);
        let reference = scrolled(&rows, 0.0);
        let max = reference.max_scroll();
        assert!(max > 0.0, "this fixture depends on a stack that overflows");
        let palette = chrome_palette();
        let border = FLOAT_WINDOW_BORDER_LOGICAL_PX.max(1.0);
        let mut rows_cut = 0;
        for step in 0..=8 {
            let placed = scrolled(&rows, max * step as f32 / 8.0);
            let content = placed.content_box();
            let labels = labels_of(&placed, None, values());
            let quads = quads_of(&placed, None, values());
            for row in &placed.rows {
                // Text: the row's own boxes are the layout boxes, whatever the
                // crop, and a box with nothing showing draws nothing at all.
                for (box_of, text) in [
                    (row.title, row.row.title()),
                    (row.desc, row.row.description(values())),
                ] {
                    match clipped(box_of, content) {
                        None => assert!(
                            !labels.iter().any(|label| label.rect == box_of),
                            "{:?}: a box wholly outside the content draws nothing",
                            row.row
                        ),
                        Some(window) => {
                            let drawn = labels
                                .iter()
                                .find(|label| label.rect == box_of)
                                .unwrap_or_else(|| {
                                    panic!(
                                        "{:?}: {text:?} is laid out in the box the row was given, \
                                         {box_of:?} — not in the crop. Labels: {:?}",
                                        row.row,
                                        labels.iter().map(|label| label.rect).collect::<Vec<_>>()
                                    )
                                });
                            assert_eq!(
                                drawn.clip,
                                Some(window),
                                "{:?}: {text:?} wears the crop as its clip",
                                row.row
                            );
                        }
                    }
                }
                // Fills: the whole button's fills, with the outside dropped. Built
                // here from the row's *full* combo so the comparison is against
                // the shape the button is, not against the shape the cut left.
                // A slider is not a button and has no fills to compare against
                // a button's; the clipping it owes is the same clipping every
                // other quad in the content stack owes, which the sweep below
                // already covers.
                let Some(button_value) = shown_value(row.row) else {
                    if clipped(row.combo, content).is_some_and(|seen| seen != row.combo) {
                        rows_cut += 1;
                    }
                    continue;
                };
                let mut whole_quads = Vec::new();
                let mut whole_labels = Vec::new();
                push_combo(
                    &mut whole_quads,
                    &mut whole_labels,
                    row.combo,
                    false,
                    button_value,
                    true,
                    1.0,
                    border,
                    palette,
                    &mut measure,
                );
                if clipped(row.combo, content).is_some_and(|seen| seen != row.combo) {
                    rows_cut += 1;
                }
                for whole in whole_quads {
                    let Some(rect) = clipped(whole.rect, content) else {
                        continue;
                    };
                    let cut = OverlayQuad { rect, ..whole };
                    assert!(
                        quads.contains(&cut),
                        "{:?}: the button's fill {:?} reaches the screen as {cut:?} — a run cut \
                         by the content edge, not a shorter button's own run",
                        row.row,
                        whole.rect
                    );
                }
            }
        }
        assert!(
            rows_cut > 0,
            "no offset in this sweep actually cut a row, so the pin proved nothing"
        );
    }

    /// PIN (Bug 1): scrolling **moves** the stack and does nothing else.
    ///
    /// The geometry half of the law, and the stronger statement of the two: not
    /// merely that a row keeps its height at every offset, but that the whole
    /// stack at offset *s* is the stack at rest slid up by exactly *s* — every
    /// heading, every title, every description, every button. A layout that
    /// clamped a rectangle into the content box would fail this before any
    /// drawing was asked for, which is the point of stating it here as well as at
    /// the draw: the two are different places the same mistake can be made, and
    /// [`clip_content`] can only be honest about a stack that was honest first.
    #[test]
    fn scrolling_translates_the_stack_and_changes_nothing_else() {
        let rows = many_rows(20);
        let rest = scrolled(&rows, 0.0);
        let max = rest.max_scroll();
        assert!(max > 0.0, "this fixture depends on a stack that overflows");
        let slid = |rect: [f32; 4], by: f32| [rect[0], rect[1] - by, rect[2], rect[3] - by];
        for step in 0..=16 {
            let by = max * step as f32 / 16.0;
            let placed = scrolled(&rows, by);
            assert_eq!(
                placed.content, rest.content,
                "the box the stack slides behind does not move with it"
            );
            for (moved, still) in placed.groups.iter().zip(&rest.groups) {
                assert_eq!(
                    moved.label,
                    slid(still.label, by),
                    "{:?}'s heading at offset {by}",
                    moved.group
                );
            }
            for (moved, still) in placed.rows.iter().zip(&rest.rows) {
                assert_eq!(
                    [moved.title, moved.desc, moved.combo],
                    [
                        slid(still.title, by),
                        slid(still.desc, by),
                        slid(still.combo, by)
                    ],
                    "{:?} at offset {by} is its resting self, moved",
                    moved.row
                );
            }
        }
    }

    /// PIN (Bug 1, second half): a value too wide for its button ends in a `…`.
    ///
    /// `.combo > button` is a fixed 118px and the value inside it is whatever the
    /// chosen option is called, so the two collide the moment an option has a long
    /// name — which "Windows PowerShell 5.1" is, and which is how the user met
    /// this: the Default profile button read `Windows Pov`, a name cut mid-glyph
    /// by the label's own box. A crop mid-glyph reads as a rendering fault; an
    /// ellipsis reads as a name too long, which is what it is.
    ///
    /// Two claims, and the second is the one that makes the first mean anything:
    /// every button's value fits its box, **and** a value that had to give way is
    /// a prefix of its own option marked with the `…`, never some other string.
    ///
    /// Red gate: drop the [`ellipsized`] call and the Default profile row fails
    /// the width claim with the whole title in a box that cannot hold it.
    #[test]
    fn a_value_too_wide_for_its_button_is_ellipsised_not_cropped() {
        let mut ellipsised = Vec::new();
        let mut placed_rows = Vec::new();
        for category in SettingsCategory::ALL {
            let page = open_page(1.0, None, TabLayoutMode::Vertical, category, 0.0);
            placed_rows.extend(page.rows.iter().map(|row| (page.clone(), *row)));
        }
        for (placed, row) in &placed_rows {
            let labels = labels_of(placed, None, values());
            // Sliders have no value box and nothing that can outgrow one.
            let Some(whole) = shown_value(row.row) else {
                continue;
            };
            let box_of = combo_value_box(row.combo);
            let drawn = labels
                .iter()
                .find(|label| label.rect == box_of)
                .unwrap_or_else(|| panic!("{:?}'s button draws its value", row.row));
            assert!(
                measure(&drawn.text, COMBO_FONT_LOGICAL_PX) <= width(box_of),
                "{:?}: {:?} is drawn in a box only {} wide",
                row.row,
                drawn.text,
                width(box_of)
            );
            if drawn.text == whole {
                continue;
            }
            let drawn = drawn.clone();
            let kept = drawn
                .text
                .strip_suffix(ELLIPSIS)
                .unwrap_or_else(|| panic!("{:?}: {:?} gave way without a …", row.row, drawn.text));
            assert!(
                whole.starts_with(kept) && !kept.is_empty(),
                "{:?}: {kept:?} is a prefix of {whole:?} and not a different word",
                row.row
            );
            ellipsised.push(row.row);
        }
        // **The family picker is excluded from the exact list, and only from
        // it.** Its values are the names of whatever fonts the machine running
        // this test has installed — `Cascadia Mono` fits and
        // `DejaVu Sans Mono for Powerline` does not — so asserting whether it
        // ellipsises would be asserting a fact about a font list this repository
        // does not own. Every mechanical claim above still applies to it: the
        // drawn text fits the box, and if it gave way it gave way with an `…`
        // after a prefix of the real name.
        ellipsised.retain(|row| *row != SettingsRow::TerminalFont);
        ellipsised.sort_by_key(|row| format!("{row:?}"));
        assert_eq!(
            ellipsised,
            vec![SettingsRow::DefaultProfile, SettingsRow::SplitDirection],
            "the long profile title and `Auto (longer edge)` are the two \
             values this build's own tables can produce that cannot fit the \
             118px button, and every other row's option is one short word that \
             must be left alone"
        );
    }

    /// PIN: [`ellipsized`] keeps the **longest** prefix that fits, and cuts only
    /// between characters.
    ///
    /// The rule the drawn pin above rests on, stated where it can be stated
    /// exactly. The "longest" half is what separates a real `text-overflow` from a
    /// truncation that throws away room it was given; the boundary half is what
    /// keeps a multi-byte name from being cut into invalid UTF-8, which a byte
    /// index would do and which no amount of measuring would catch.
    #[test]
    fn the_ellipsis_keeps_the_longest_prefix_that_fits() {
        let font = 10.0;
        let advance = font * TEST_ADVANCE_PER_EM;
        let text = "Windows PowerShell 5.1";
        assert_eq!(
            ellipsized(text, advance * 100.0, font, &mut measure),
            text,
            "a value with room to spare is left alone, ellipsis and all"
        );
        for characters in 1..text.chars().count() {
            let room = advance * characters as f32;
            let cut = ellipsized(text, room, font, &mut measure);
            assert!(
                measure(&cut, font) <= room,
                "{cut:?} does not fit {characters} characters' worth"
            );
            let kept = cut.strip_suffix(ELLIPSIS).expect("a cut value is marked");
            assert!(text.starts_with(kept), "{kept:?} is a prefix of {text:?}");
            // The longest such prefix: one more character would overflow. The `…`
            // itself takes one character's room in this measure, so the prefix is
            // one shorter than the room allows.
            assert_eq!(
                kept.chars().count(),
                characters.saturating_sub(1),
                "{cut:?} left room unused in {characters} characters' worth"
            );
        }
        // Nothing fits, and a `…` is still what CSS draws — the box does the rest.
        assert_eq!(ellipsized(text, 0.0, font, &mut measure), ELLIPSIS);
        // Cut between characters, never inside one: every prefix of a name whose
        // characters are three bytes each is still a name.
        let wide = "文件资源管理器";
        for characters in 1..=wide.chars().count() {
            let cut = ellipsized(wide, advance * characters as f32, font, &mut measure);
            let kept = cut.strip_suffix(ELLIPSIS).unwrap_or(&cut);
            assert!(
                wide.starts_with(kept),
                "{kept:?} is a whole-character prefix of {wide:?}"
            );
        }
    }

    /// PIN (B23) — the *left* cut keeps the longest **suffix** that fits, which
    /// on a path is the part that answers "where am I".
    ///
    /// The mirror of the test above, and the rule both feet are drawn by: a
    /// float's `.fly-foot` and a docked column's `.files-foot` both show a full
    /// path in a strip narrower than one, and a right cut would give
    /// `C:\Users\Weiyi\Developer\Bett…` — every character of which the user
    /// already knew. The user's own screenshot of what they wanted is the second
    /// assertion here: `…ers\Weiyi\Developer\BetterTerminal`.
    #[test]
    fn the_left_ellipsis_keeps_the_longest_suffix_that_fits() {
        let font = 10.0;
        let advance = font * TEST_ADVANCE_PER_EM;
        let text = r"C:\Users\Weiyi\Developer\BetterTerminal";
        assert_eq!(
            ellipsized_left(text, advance * 100.0, font, &mut measure),
            text,
            "a path with room to spare is left whole"
        );
        // The width the user photographed: room for a `…` and the last 34
        // characters, which is exactly where their own screenshot cut.
        assert_eq!(
            ellipsized_left(text, advance * 35.0, font, &mut measure),
            format!("{ELLIPSIS}ers\\Weiyi\\Developer\\BetterTerminal"),
            "the folder you are in survives; the drive letter is what goes"
        );
        for characters in 1..text.chars().count() {
            let room = advance * characters as f32;
            let cut = ellipsized_left(text, room, font, &mut measure);
            assert!(
                measure(&cut, font) <= room,
                "{cut:?} does not fit {characters} characters' worth"
            );
            let kept = cut.strip_prefix(ELLIPSIS).expect("a cut path is marked");
            assert!(text.ends_with(kept), "{kept:?} is a suffix of {text:?}");
            // The longest such suffix, by the same arithmetic as the prefix
            // test: the `…` costs one character's room.
            assert_eq!(
                kept.chars().count(),
                characters.saturating_sub(1),
                "{cut:?} left room unused in {characters} characters' worth"
            );
        }
        assert_eq!(ellipsized_left(text, 0.0, font, &mut measure), ELLIPSIS);
        // And cut between characters: a path with three-byte names in it is
        // still a path after the cut.
        let wide = r"C:\用户\伟毅\开发\终端";
        for characters in 1..=wide.chars().count() {
            let cut = ellipsized_left(wide, advance * characters as f32, font, &mut measure);
            let kept = cut.strip_prefix(ELLIPSIS).unwrap_or(&cut);
            assert!(
                wide.ends_with(kept),
                "{kept:?} is a whole-character suffix of {wide:?}"
            );
        }
    }

    /// PIN: a window that cannot host the dialog answers `None` rather than a
    /// squashed one — and the runtime reads `None` as "shut", so nothing is
    /// trapped behind a modal with nothing on it.
    #[test]
    fn a_window_too_small_to_host_the_dialog_says_so() {
        let rows = flat_rows();
        let sized = |width: f32, height: f32| {
            layout_for_menu(
                width,
                height,
                1.0,
                None,
                content(&rows, &[]),
                PAGE,
                0.0,
                UNSCROLLED,
            )
        };
        assert!(sized(1280.0, 100.0).is_none(), "too short");
        // **The floor moved with the rail** (2026-08-17). It used to be the
        // 118px picker alone; it is now the rail, the two gutters and that
        // picker, because a page with no room for the control every row of it
        // ends in is not a narrower version of this design.
        let floor =
            NAV_WIDTH_LOGICAL_PX + 2.0 * CONTENT_PADDING_X_LOGICAL_PX + COMBO_MIN_WIDTH_LOGICAL_PX;
        assert!(sized(100.0, 800.0).is_none(), "too narrow");
        assert!(
            sized(floor / DIALOG_WIDTH_RATIO - 4.0, 800.0).is_none(),
            "one pixel under the floor is still under it"
        );
        assert!(
            sized(floor / DIALOG_WIDTH_RATIO + 4.0, 800.0).is_some(),
            "and just over it the dialog opens"
        );
        assert!(sized(1280.0, 800.0).is_some(), "a real window");
    }

    /// PIN (Esc): one layer per press — the open menu first, then the dialog,
    /// then nothing, and "nothing" is reported so the key can fall through to
    /// whoever owns it next.
    #[test]
    fn escape_unwinds_one_layer_per_press() {
        let mut panel = SettingsPanel::default();
        let rows = flat_rows();
        let lines = shortcut_lines();
        assert!(!panel.close_one_layer(), "nothing is open yet");
        panel.toggle(content(&rows, &lines));
        panel.toggle_menu(SettingsRow::Theme);
        assert!(panel.close_one_layer());
        assert!(
            panel.is_open() && panel.menu().is_none(),
            "the menu went first"
        );
        assert!(panel.close_one_layer());
        assert!(!panel.is_open(), "the dialog went second");
        assert!(!panel.close_one_layer(), "and then there is nothing left");
    }

    /// PIN: the gear toggles, and shutting the dialog takes the menu with it —
    /// so reopening never shows a picker the user never opened.
    #[test]
    fn the_gear_toggles_and_shutting_takes_the_menu_with_it() {
        let mut panel = SettingsPanel::default();
        let rows = flat_rows();
        let lines = shortcut_lines();
        panel.toggle(content(&rows, &lines));
        assert!(panel.is_open());
        panel.toggle_menu(SettingsRow::Theme);
        panel.toggle(content(&rows, &lines));
        assert!(!panel.is_open() && panel.menu().is_none());
        panel.toggle(content(&rows, &lines));
        assert!(panel.is_open() && panel.menu().is_none());
        panel.close();
        assert!(!panel.is_open());
    }

    /// How wide a chrome label is, for tests — one flat advance per character.
    ///
    /// A stand-in for the renderer's shaper on purpose. Every claim below that
    /// touches text width is a claim about the *rule* — "the whole string, or a
    /// prefix and a `…`, and never more than the box" — and a rule stated against
    /// a real face would be re-stating Segoe UI's metrics, which no assertion
    /// should own and which change with the font the machine has.
    const TEST_ADVANCE_PER_EM: f32 = 0.5;

    fn measure(text: &str, font_size_px: f32) -> f32 {
        text.chars().count() as f32 * font_size_px * TEST_ADVANCE_PER_EM
    }

    /// The overlay as it is drawn, measured by [`measure`].
    fn built(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        values: SettingsValues,
    ) -> Vec<OverlayLayer> {
        built_with(placed, hover, None, values, None)
    }

    /// The same, with a ring and a capture the caller names.
    fn built_with(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        focus: Option<SettingsTarget>,
        values: SettingsValues,
        recording: Option<(usize, &[String], Option<&str>)>,
    ) -> Vec<OverlayLayer> {
        let lines = shortcut_lines();
        build(
            placed,
            hover,
            focus,
            values,
            &lines,
            "",
            recording,
            &mut measure,
        )
    }

    /// Every fill the overlay draws, whatever layer it is on — the question
    /// "does the dialog paint this at all" is not a question about z-order.
    fn quads_of(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        values: SettingsValues,
    ) -> Vec<OverlayQuad> {
        built(placed, hover, values)
            .into_iter()
            .flat_map(|layer| layer.quads)
            .collect()
    }

    fn labels_of(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        values: SettingsValues,
    ) -> Vec<ChromeLabel> {
        built(placed, hover, values)
            .into_iter()
            .flat_map(|layer| layer.labels)
            .collect()
    }

    fn sprites_of(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        values: SettingsValues,
    ) -> Vec<ChromeSprite> {
        built(placed, hover, values)
            .into_iter()
            .flat_map(|layer| layer.sprites)
            .collect()
    }

    /// Whether `inner` lies wholly inside `outer`.
    fn within(inner: [f32; 4], outer: [f32; 4]) -> bool {
        inner[0] >= outer[0] && inner[1] >= outer[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
    }

    /// Whether the two rectangles share any area at all.
    fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
        a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
    }

    /// Which layer the open picker's own surface is drawn on.
    fn popup_layer(layers: &[OverlayLayer]) -> usize {
        let surface = chrome_palette().menu_surface;
        layers
            .iter()
            .position(|layer| layer.quads.iter().any(|quad| quad.color == surface))
            .expect("an open picker draws its own face in --menu")
    }

    /// PIN (z-order): an open picker is a layer of its own, it is the last one,
    /// and nothing but the picker is on it — so every product of every row lands
    /// underneath it, in whichever channel that row drew itself.
    ///
    /// This is the shape of the bug it exists to keep out, caught on screen with
    /// the Theme picker open: the popup hangs over the Cursor row, and the row's
    /// value, its chevron and its tick came back out through the popup's face.
    /// The reason is that the overlay draws a layer's fills, then its marks, then
    /// its text: the popup's surface is a *fill* and the row's value is *text*, so
    /// pushing the popup last covered the row's own fills and nothing else. Only
    /// a later layer covers a later channel.
    ///
    /// Stated as the geometry the screenshot shows: where the popup's rectangle
    /// crosses a row's, the row's drawing product is on a lower layer — never on
    /// the popup's, and never above it.
    ///
    /// Red gate: the second half is what fails while the popup shares the rows'
    /// layer, and it names the row content it found there. The last assertion
    /// keeps the whole test from going vacuous if the picker ever stops
    /// overhanging the row below it — there would then be nothing to cover.
    #[test]
    fn an_open_picker_is_the_last_layer_and_carries_nothing_but_itself() {
        let mut covered_row_products = 0;
        for kind in [
            SettingsRow::Theme,
            SettingsRow::Cursor,
            SettingsRow::TabLayout,
            SettingsRow::Sidebar,
        ] {
            let placed = open_rows(1.0, Some(kind), TabLayoutMode::Vertical);
            let menu = placed.menu.expect("the picker is open");
            let layers = built(&placed, None, values());
            let popup = popup_layer(&layers);
            assert_eq!(
                popup,
                layers.len() - 1,
                "{kind:?}: nothing at all is drawn over an open picker"
            );

            // The popup's layer is the popup and only the popup. Its captions and
            // marks stand inside its own frame; its fills are that frame plus the
            // shadow it casts, which is the one thing it draws outside itself.
            let lift = FLOAT_WINDOW_SHADOW_LOGICAL_PX.ceil() + 1.0;
            let halo = [
                menu[0] - lift,
                menu[1] - lift,
                menu[2] + lift,
                menu[3] + lift,
            ];
            let top = &layers[popup];
            for label in &top.labels {
                assert!(
                    within(label.rect, menu),
                    "{kind:?}: {:?} at {:?} is not the picker's own text and shares its layer",
                    label.text,
                    label.rect
                );
            }
            for sprite in &top.sprites {
                assert!(
                    within(sprite.rect, menu),
                    "{kind:?}: {:?} is a mark on the picker's layer that is not the picker's",
                    sprite.mark
                );
            }
            for quad in &top.quads {
                assert!(
                    within(quad.rect, halo),
                    "{kind:?}: a fill at {:?} is on the picker's layer but outside its shadow",
                    quad.rect
                );
            }

            // Count what the popup is actually covering, for the vacuity guard
            // below: content that crosses the popup's rectangle and is drawn on
            // a layer under it.
            covered_row_products += layers[..popup]
                .iter()
                .flat_map(|layer| &layer.labels)
                .filter(|label| overlaps(label.rect, menu))
                .count();
        }
        // The claim is not vacuous: a picker really does hang over row content
        // that was on top of it before. The Theme picker is the one the
        // screenshot caught — it covers the row below's value and its chevron —
        // and every picker but the last row's does the same thing now.
        assert!(
            covered_row_products >= 2,
            "no picker overhangs any row's text, so this test proves nothing"
        );
    }

    /// PIN (modal, one layer down): a press inside an open picker belongs to the
    /// picker, even where the picker hangs over another row's control — the hit
    /// test reads the same z-order the draw does, so a click on the popup can
    /// never open a second one behind it.
    ///
    /// Red gate: the sweep is over the *intersection* of the popup and the combo
    /// it covers, and it is asserted to be a real rectangle first, so a hit test
    /// that answered the row under the popup would be caught by the sweep rather
    /// than by an empty loop.
    #[test]
    fn a_press_inside_an_open_picker_never_reaches_the_row_beneath_it() {
        let placed = open(1.0, true);
        let menu = placed.menu.expect("the picker is open");
        let covered = clipped(combo_of(&placed, SettingsRow::LightScheme), menu)
            .expect("the Theme picker hangs over the Light scheme row's control");
        let mut swept = 0;
        let mut y = covered[1] + 0.5;
        while y < covered[3] {
            let mut x = covered[0] + 0.5;
            while x < covered[2] {
                let target = hit(&placed, values(), f64::from(x), f64::from(y));
                assert!(
                    matches!(
                        target,
                        SettingsTarget::Menu(SettingsRow::Theme)
                            | SettingsTarget::Choice(SettingsRow::Theme, _)
                    ),
                    "({x}, {y}) is under the open picker and answered {target:?}"
                );
                swept += 1;
                x += 3.0;
            }
            y += 3.0;
        }
        assert!(swept > 0, "the sweep must cover real ground");
    }

    /// Visual PIN: the scrim is the mock-up's own `rgba(15,15,15,.35)` across
    /// the whole window, and it is the *first* thing drawn, so everything else
    /// stands on it rather than under it.
    #[test]
    fn the_scrim_is_the_mock_ups_own_alpha_over_the_whole_window() {
        let placed = open(1.0, false);
        let palette = chrome_palette();
        let scrim = quads_of(&placed, None, values())[0];
        assert_eq!(scrim.rect, [0.0, 0.0, SURFACE.0, SURFACE.1]);
        assert_eq!(scrim.color, [0x0f, 0x0f, 0x0f]);
        assert_eq!(scrim.color, palette.modal_scrim);
        assert!(
            (scrim.alpha - 0.35).abs() < 0.005,
            "the scrim is .35, saw {}",
            scrim.alpha
        );
    }

    /// Visual PIN (float-window craft): the dialog is a lift, a hairline and a
    /// face, and its corners carry partial coverage rather than a staircase —
    /// the same claim `rounded_rect.rs` pins for every window that floats.
    ///
    /// Red gate: nested rectangles put no pixel strictly between 0 and 1, so the
    /// partial-coverage count alone rules them out.
    #[test]
    fn the_dialog_wears_the_float_windows_craft_at_every_dpi() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let placed = open(scale, false);
            let palette = chrome_palette();
            let quads = quads_of(&placed, None, values());
            let border_alpha = f32::from(palette.menu_border_alpha) / 255.0;
            let hairline = quads
                .iter()
                .filter(|quad| quad.color == palette.menu_border)
                .filter(|quad| (quad.alpha - border_alpha).abs() < 1e-4)
                .count();
            assert!(
                hairline > 0,
                "scale {scale}: the dialog wears --border at the mock-up's own alpha"
            );
            let face = quads
                .iter()
                .filter(|quad| quad.color == palette.dialog_surface && quad.alpha == 1.0)
                .count();
            assert!(face > 0, "scale {scale}: the dialog's face is --win");
            let lift = quads
                .iter()
                .filter(|quad| quad.color == palette.menu_shadow)
                .count();
            assert!(
                lift > 0,
                "scale {scale}: the dialog is lifted off the scrim"
            );
            let radius = FLOAT_WINDOW_RADIUS_LOGICAL_PX * scale;
            let partial = quads
                .iter()
                .filter(|quad| quad.alpha > 0.0 && quad.alpha < 1.0)
                .filter(|quad| quad.rect[2] - quad.rect[0] == 1.0)
                .filter(|quad| quad.color == palette.dialog_surface)
                .count();
            assert!(
                partial >= radius as usize,
                "scale {scale}: an antialiased quarter circle spends at least one \
                 partial pixel per row on the face, saw {partial}"
            );
        }
    }

    /// Visual PIN: the picker shows the selected mode, and marks it with the
    /// accent tick when the menu is open — exactly one tick, never two.
    #[test]
    fn the_picker_shows_and_ticks_the_selected_mode() {
        for selected in THEME_OPTIONS {
            let placed = open(1.0, true);
            let combo = combo_of(&placed, SettingsRow::Theme);
            let palette = chrome_palette();
            let labels = labels_of(
                &placed,
                None,
                SettingsValues {
                    theme: selected,
                    ..values()
                },
            );
            let ticks: Vec<_> = labels.iter().filter(|label| label.text == TICK).collect();
            assert_eq!(ticks.len(), 1, "exactly one option is the selected mode");
            assert_eq!(ticks[0].color, palette.accent, "the tick is the accent");
            // The button says what the tick marks.
            let shown = theme_label(selected);
            let on_button = labels
                .iter()
                .find(|label| {
                    label.text == shown && label.rect[1] >= combo[1] && label.rect[3] <= combo[3]
                })
                .expect("the button shows the selected mode");
            assert_eq!(on_button.color, palette.dialog_title_text);
            // And the tick sits in the row of the option it marks.
            let index = THEME_OPTIONS
                .iter()
                .position(|option| *option == selected)
                .expect("the selected mode is one of the options");
            let item = placed.items[index];
            assert!(ticks[0].rect[1] >= item[1] && ticks[0].rect[3] <= item[3]);
        }
    }

    /// Visual PIN: hover paints one thing and only when something is hovered —
    /// the mock-up's `--hover` over the surface the control actually sits on.
    #[test]
    fn hover_paints_the_control_under_the_pointer_and_nothing_else() {
        let placed = open(1.0, true);
        let palette = chrome_palette();
        // **Everything the rail draws is excluded**, and deliberately: the
        // selected word wears `--hover` as its standing ground, which is a
        // *selection* and not a hover. Counting it here would make this test
        // read "one thing is lit when nothing is hovered", which is true and is
        // a different sentence from the one this pins.
        let nav = placed.nav;
        let count = |hover, color| {
            quads_of(&placed, hover, values())
                .iter()
                .filter(|quad| quad.color == color)
                .filter(|quad| quad.rect[0] >= nav[2] || quad.rect[2] <= nav[0])
                .count()
        };
        assert_eq!(
            count(None, palette.dialog_hover),
            0,
            "nothing is lit when nothing is hovered"
        );
        assert!(
            count(Some(SettingsTarget::Close), palette.dialog_hover) > 0,
            "the close button takes --hover over --win"
        );
        assert!(
            count(
                Some(SettingsTarget::Combo(SettingsRow::Theme)),
                palette.dialog_hover
            ) > 0,
            "so does the combo button"
        );
        assert_eq!(
            count(
                Some(SettingsTarget::Menu(SettingsRow::Theme)),
                palette.menu_item_hover
            ),
            0,
            "the menu's own body is not an item"
        );
        assert!(
            count(
                Some(SettingsTarget::Choice(SettingsRow::Theme, 0)),
                palette.menu_item_hover
            ) > 0,
            "an item takes --hover over --menu, which is a different grey"
        );
    }

    /// The group heading is uppercase and tracked, which is the whole of what
    /// makes it read as a heading at 11px rather than as a small sentence.
    #[test]
    fn the_group_heading_is_uppercase_and_tracked() {
        let placed = open(1.0, false);
        let labels = labels_of(&placed, None, values());
        let heading = labels
            .iter()
            .find(|label| label.text == "APPEARANCE")
            .expect("the Appearance group is headed");
        assert_eq!(heading.font_size_px, GROUP_LABEL_FONT_LOGICAL_PX);
        assert!(
            (heading.letter_spacing_em - 0.05).abs() < 1e-4,
            "letter-spacing: .05em, and em is a ratio"
        );
        assert_eq!(heading.color, chrome_palette().dialog_muted_text);
        // Red gate for a real bug this pass shipped and then caught on screen:
        // tracking was multiplied by the scaled font size on the way in, which
        // at 200% asked the shaper for 1.1 *em* per glyph and spelled the
        // heading out letter by letter. A ratio does not carry the DPI scale.
        for scale in [1.0_f32, 2.0] {
            let placed = open(scale, false);
            let labels = labels_of(&placed, None, values());
            let heading = labels
                .iter()
                .find(|label| label.text == "APPEARANCE")
                .expect("the Appearance group is headed");
            assert!(
                (heading.letter_spacing_em - 0.05).abs() < 1e-4,
                "scale {scale}: tracking is the same ratio at every DPI, saw {}",
                heading.letter_spacing_em
            );
        }
    }

    /// Every page is headed by its own category's word, and every row on it is
    /// drawn under that heading.
    ///
    /// The heading is derived by walking the page's rows and noticing where the
    /// category changes, so on a page that holds one category it is exactly one
    /// heading — and a row drawn above it, or under a word that does not name
    /// it, is the derivation having come apart.
    #[test]
    fn every_page_is_headed_by_its_own_word_and_every_row_sits_under_it() {
        for category in SettingsCategory::ALL {
            if category == SettingsCategory::Shortcuts {
                continue;
            }
            let placed = open_page(1.0, None, TabLayoutMode::Vertical, category, 0.0);
            let labels = labels_of(&placed, None, values());
            let headings: Vec<&ChromeLabel> = labels
                .iter()
                .filter(|label| label.text == category.label())
                .collect();
            if placed.rows.is_empty() {
                assert!(
                    headings.is_empty(),
                    "{category:?} has no rows and must draw no heading"
                );
                continue;
            }
            assert_eq!(headings.len(), 1, "{category:?} is headed exactly once");
            let heading = headings[0].rect[1];
            for placed_row in &placed.rows {
                assert!(
                    placed_row.title[1] > heading,
                    "{:?} is drawn above its own page's heading",
                    placed_row.row
                );
            }
            for other in SettingsCategory::ALL {
                if other == category {
                    continue;
                }
                assert!(
                    !labels.iter().any(|label| label.text == other.label()),
                    "{category:?}'s page draws {other:?}'s heading"
                );
            }
        }
    }

    /// Two rows of the Appearance page, named, so the sweep above cannot go
    /// green on a page that happens to hold nothing.
    #[test]
    fn the_appearance_page_holds_the_mock_ups_own_rows() {
        let placed = open_page(
            1.0,
            None,
            TabLayoutMode::Vertical,
            SettingsCategory::Appearance,
            0.0,
        );
        for row in [
            SettingsRow::Theme,
            SettingsRow::Cursor,
            SettingsRow::TabLayout,
            SettingsRow::Sidebar,
        ] {
            assert!(
                placed.row(row).is_some(),
                "{row:?} belongs to the Appearance page"
            );
        }
        assert!(
            placed.row(SettingsRow::Formulas).is_none(),
            "Display formulas belongs to Rendered blocks and is not on this page"
        );
    }

    /// PIN — **a page's heading takes the stylesheet's base `margin-top: 10px`,
    /// and there is no other margin left to take.**
    ///
    /// The mock-up used to say it twice: `.group-label { margin: 10px 0 2px }`
    /// for the first heading and an inline `margin-top:16px` on every later one,
    /// the extra six pixels separating one group from the next. With one
    /// category per page there is no next group on the page, the 16 was retired
    /// with the rail, and what this pins is the half that survived — plus the
    /// fact that it is now the *only* half, which is what stops the six pixels
    /// coming back as a rule nothing reads.
    #[test]
    fn a_pages_heading_takes_the_stylesheets_base_margin_and_stands_alone() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let placed = open_page(
                scale,
                None,
                TabLayoutMode::Vertical,
                SettingsCategory::Appearance,
                0.0,
            );
            let labels = labels_of(&placed, None, values());
            let heading = labels
                .iter()
                .find(|label| label.text == SettingsCategory::Appearance.label())
                .expect("the page is headed")
                .rect;

            let content_top = placed.content[1];
            assert!(
                (heading[1]
                    - (content_top
                        + (CONTENT_PADDING_TOP_LOGICAL_PX + GROUP_LABEL_MARGIN_TOP_LOGICAL_PX)
                            * scale))
                    .abs()
                    < 0.5,
                "scale {scale}: the heading keeps the stylesheet's 10px"
            );
            assert!(
                (heading[3] - heading[1] - GROUP_LABEL_LINE_LOGICAL_PX * scale).abs() < 0.5,
                "scale {scale}: every heading is one line box tall"
            );
            assert_eq!(
                placed.groups.len(),
                1,
                "scale {scale}: a page draws one heading, so there is no later one"
            );
        }
    }

    /// R4's lesson, asked again now that a second heading pushes the rows below it
    /// down: the height, the stack, the hit test and the draw are one derivation,
    /// so a row is never clickable somewhere it is not drawn.
    ///
    /// This is the red gate for the bug that shipped once already — geometry
    /// taught in one place and forgotten in another gives a control the user can
    /// press but cannot see.
    #[test]
    fn every_row_answers_the_pointer_where_its_own_page_put_it() {
        for tab_layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
            for scale in [1.0_f32, 1.25, 2.0] {
                for row in visible_rows(tab_layout) {
                    let placed = open_page(scale, None, tab_layout, row.category(), 0.0);
                    let combo = placed.row(row).expect("a visible row is placed").combo;
                    let centre = (
                        f64::from((combo[0] + combo[2]) / 2.0),
                        f64::from((combo[1] + combo[3]) / 2.0),
                    );
                    assert_eq!(
                        hit(&placed, values(), centre.0, centre.1),
                        row.control_target(),
                        "{tab_layout:?} at {scale}: {row:?}'s control must answer \
                         where it is drawn"
                    );
                }
            }
        }
    }

    /// The dialog is as tall as everything the page it is showing is holding —
    /// its heading included.
    ///
    /// A height that forgot the heading would push the last row past the bottom
    /// of the content box and clip it away, which is the bug that shipped once.
    #[test]
    fn the_dialog_makes_room_for_every_heading_it_draws() {
        for category in SettingsCategory::ALL {
            if category == SettingsCategory::Shortcuts {
                continue;
            }
            let placed = open_page(1.0, None, TabLayoutMode::Vertical, category, 0.0);
            let content = placed.content;
            let Some(last) = placed.rows.last() else {
                continue;
            };
            assert!(
                last.desc[3] + CONTENT_PADDING_BOTTOM_LOGICAL_PX <= content[3] + 0.5,
                "{category:?}: the last row and the content's bottom padding both \
                 fit inside the content box"
            );
        }
    }

    /// Every category the dialog can show holds at least one row, and a
    /// category's rows are contiguous — the order in [`visible_rows`] is what
    /// the page heading is derived from, so a row filed out of order would head
    /// its own page twice.
    #[test]
    fn each_categorys_rows_stand_together_in_the_visible_order() {
        for tab_layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
            let rows = visible_rows(tab_layout);
            let mut seen: Vec<SettingsCategory> = Vec::new();
            for row in rows {
                let group = row.category();
                if seen.last() != Some(&group) {
                    assert!(
                        !seen.contains(&group),
                        "{tab_layout:?}: {group:?} is interrupted by another category"
                    );
                    seen.push(group);
                }
            }
            assert_eq!(
                seen,
                vec![
                    SettingsCategory::Appearance,
                    SettingsCategory::RenderedBlocks,
                    SettingsCategory::General,
                    SettingsCategory::Terminal,
                ],
                "{tab_layout:?}: every category with rows is shown once, its rows \
                 together"
            );
        }
    }

    /// PIN (S4, 2026-08-17) — **every row is on exactly one page, and every page
    /// the rail offers has something on it.**
    ///
    /// The first half is what makes the rail a partition rather than a filter:
    /// a row filed nowhere is a control nobody can reach, and a row filed twice
    /// is a switch that disagrees with itself two words apart.
    ///
    /// The second is the placeholder ruling, stated as a property: the rail is
    /// the categories that **have** rows, in declaration order, and nothing
    /// else. `Terminal` is the proof it works from both sides — it sat in
    /// `ALL` with a heading and a page and stayed out of the rail for two
    /// slices, and it joined the rail the day §7.1.6c-3b's PSReadLine row gave
    /// it content, with no list of rail items edited anywhere. A door onto an
    /// empty room is a worse promise than no door.
    ///
    /// MUTATIONS:
    /// (1) take the PSReadLine row out of `visible_rows` — `Terminal` leaves the
    ///     rail on its own, which is the derivation working;
    /// (2) list the categories in the rail from `ALL` without the filter — a
    ///     category with nothing on it appears and this goes red.
    #[test]
    fn every_row_is_on_exactly_one_page_and_every_page_in_the_rail_has_something_on_it() {
        for tab_layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
            let rows = visible_rows(tab_layout);
            let lines = shortcut_lines();
            let content = content(&rows, &lines);
            for row in &rows {
                let homes: Vec<SettingsCategory> = SettingsCategory::ALL
                    .into_iter()
                    .filter(|category| content.page_rows(*category).contains(row))
                    .collect();
                assert_eq!(homes.len(), 1, "{row:?} is filed on {homes:?}");
                assert_eq!(homes[0], row.category());
            }
            let rail = content.nav_items();
            for category in &rail {
                assert!(
                    content.has_content(*category),
                    "{category:?} is in the rail with nothing on it"
                );
            }
            assert_eq!(
                rail,
                vec![
                    SettingsCategory::General,
                    SettingsCategory::Appearance,
                    SettingsCategory::Terminal,
                    SettingsCategory::RenderedBlocks,
                    SettingsCategory::Shortcuts,
                ],
                "{tab_layout:?}: the rail is the categories with content, in \
                 declaration order"
            );
            // The other direction of the same rule, stated where it can still
            // be stated now that every category has content: strike the rows
            // of any one page and its word must leave the rail on its own.
            for absent in SettingsCategory::ALL {
                let kept: Vec<SettingsRow> = rows
                    .iter()
                    .copied()
                    .filter(|row| row.category() != absent)
                    .collect();
                let thinner = SettingsContent {
                    rows: &kept,
                    shortcuts: if absent == SettingsCategory::Shortcuts {
                        &[]
                    } else {
                        &lines
                    },
                };
                assert!(
                    !thinner.nav_items().contains(&absent),
                    "{absent:?} keeps its word in the rail with nothing on its                      page — a door onto an empty room"
                );
            }
            assert_eq!(
                content.first_category(),
                SettingsCategory::General,
                "the dialog opens on the first word of the rail"
            );
        }
    }

    /// PIN — the Startup row is the picker built from the `˅` menu's own list,
    /// and the only one in this dialog whose items wear a mark.
    ///
    /// Mock-up 7645: "the default-profile picker is built from the same list the
    /// ⌄ menu uses" — the *same* table, so a fifth profile appears in both
    /// surfaces or in neither, and 7647 is the one `.ticon` in any combo item.
    #[test]
    fn the_startup_row_offers_the_pickers_own_profiles_each_under_its_own_mark() {
        assert_eq!(
            SettingsRow::DefaultProfile.title(),
            "Default profile",
            "mock-up 2467"
        );
        assert_eq!(
            SettingsRow::DefaultProfile.description(values()),
            "What opens on a new tab, and when Folio starts",
            "mock-up 2468, word for word but for the product's name"
        );
        assert_eq!(
            SettingsRow::DefaultProfile
                .option_labels()
                .collect::<Vec<_>>(),
            profiles::PROFILES
                .iter()
                .map(|profile| profile.title)
                .collect::<Vec<_>>(),
        );
        for (index, profile) in profiles::PROFILES.iter().enumerate() {
            assert_eq!(
                SettingsRow::DefaultProfile.option_mark(index),
                Some(profile.mark),
                "{} wears its own mark, not a generic shell glyph",
                profile.id
            );
            assert_eq!(
                default_profile_requested(SettingsTarget::Choice(
                    SettingsRow::DefaultProfile,
                    index
                )),
                Some(profile.id),
                "the press asks for a profile by id — an index would not survive \
                 the table being reordered"
            );
        }
        // **One of two**, and the second one arrived for this one's reason
        // (user ruling, 2026-08-16): `Split direction` names two axes, and the
        // difference between "beside" and "below" is a shape this build already
        // draws. Every other picker's items are words, because every other
        // picker's items *are* words — `Light`, `Bar`, `On` name no object.
        for row in visible_rows(TabLayoutMode::Vertical) {
            if matches!(
                row,
                SettingsRow::DefaultProfile | SettingsRow::SplitDirection
            ) {
                continue;
            }
            for index in 0..row.option_count() {
                assert_eq!(row.option_mark(index), None, "{row:?} draws no marks");
            }
        }
        for (index, direction) in SPLIT_DIRECTION_OPTIONS.iter().enumerate() {
            assert_eq!(
                SettingsRow::SplitDirection.option_mark(index),
                Some(split_direction_mark(*direction)),
                "{direction:?} wears the glyph the rest of the product draws it with"
            );
            assert_eq!(
                split_direction_requested(SettingsTarget::Choice(
                    SettingsRow::SplitDirection,
                    index
                )),
                Some(*direction),
            );
        }
        assert_eq!(
            SettingsRow::SplitDirection
                .option_labels()
                .collect::<Vec<_>>(),
            vec!["Auto (longer edge)", "Right", "Down"],
        );
    }

    /// PIN — the combo reads the app's state, and the app's state is the
    /// *resolved* default.
    ///
    /// **The mock-up's own bug, deliberately not copied** (2471): its combo
    /// button is born holding the literal string `PowerShell` and only ever
    /// updates when the user picks something, so a default that was not index 0
    /// showed stale words until touched. `selected_index` reading `values` is the
    /// whole of the fix, and this pins that the button's caption is derived from
    /// it rather than from a constant.
    ///
    /// Said through [`ellipsized`] rather than against the bare title, because
    /// two of these profiles are named longer than 118px of button can hold and
    /// the row would otherwise be asserting that the fit never happens. What the
    /// pin still forbids is the thing it was written for: a caption that does not
    /// come from `chosen` fails here for every other profile in the table.
    #[test]
    fn the_default_profile_combo_shows_the_profile_that_would_actually_start() {
        for chosen in 0..profiles::PROFILES.len() {
            let values = SettingsValues {
                default_profile: chosen,
                ..values()
            };
            assert_eq!(
                SettingsRow::DefaultProfile.selected_index(values),
                Some(chosen)
            );
            let placed = open_showing(SettingsRow::DefaultProfile);
            let combo = placed
                .row(SettingsRow::DefaultProfile)
                .expect("the Startup row is in the dialog")
                .combo;
            let caption = labels_of(&placed, None, values)
                .into_iter()
                .find(|label| label.rect[1] >= combo[1] && label.rect[3] <= combo[3])
                .expect("the closed combo shows its current value");
            let box_of = combo_value_box(combo);
            assert_eq!(
                caption.text,
                ellipsized(
                    profiles::PROFILES[chosen].title,
                    width(box_of),
                    COMBO_FONT_LOGICAL_PX,
                    &mut measure
                ),
                "the button says what the `+` would start"
            );
        }
    }

    /// PIN — a profile this machine cannot start is greyed *and* unclickable, and
    /// the two are one answer.
    ///
    /// The picker menu's ruling (`profiles::hit`) applied to the dialog, because
    /// it is the same fact about the same machine: a rule spelled only at the
    /// click lights the row under the pointer and then does nothing when pressed;
    /// a rule spelled only at the draw leaves the item dark and still selectable.
    /// So the press, the hover and the ink are asserted together.
    #[test]
    fn a_shell_this_machine_lacks_is_greyed_in_the_startup_picker_and_cannot_be_chosen() {
        let missing = profiles::index_of_id("gitbash");
        let mut available = [true; profiles::PROFILES.len()];
        available[missing] = false;
        let lacking = SettingsValues {
            profile_available: available,
            ..values()
        };
        let placed = open_rows(
            1.0,
            Some(SettingsRow::DefaultProfile),
            TabLayoutMode::Horizontal,
        );
        let item = placed.items[missing];
        let centre = (
            f64::from((item[0] + item[2]) / 2.0),
            f64::from((item[1] + item[3]) / 2.0),
        );

        assert_eq!(
            hit(&placed, lacking, centre.0, centre.1),
            SettingsTarget::Menu(SettingsRow::DefaultProfile),
            "the press lands on the menu's body — nothing was chosen, and the \
             menu stays up"
        );
        assert_eq!(
            hit(&placed, values(), centre.0, centre.1),
            SettingsTarget::Choice(SettingsRow::DefaultProfile, missing),
            "and on a machine that has Git Bash the very same point chooses it"
        );

        let palette = bt_render::chrome_palette();
        let mark = sprites_of(&placed, None, lacking)
            .into_iter()
            .find(|sprite| sprite.mark == profiles::PROFILES[missing].mark)
            .expect("the greyed item still draws its mark");
        assert_eq!(mark.opacity, UNAVAILABLE_MARK_OPACITY);
        assert!(mark.grayscale, "and it loses its colours saying so");
        let label = labels_of(&placed, None, lacking)
            .into_iter()
            .find(|label| label.text == profiles::PROFILES[missing].title)
            .expect("the greyed item is still named");
        assert_eq!(
            label.color, palette.menu_item_hint_text,
            "the quietest ink this surface has, which is what it already uses \
             for text that reports rather than offers"
        );

        // A hover that somehow arrived on it lights nothing — the two halves
        // cannot come apart, which is the whole reason `option_enabled` is asked
        // by the draw as well as by the hit test.
        let hovered = SettingsTarget::Choice(SettingsRow::DefaultProfile, missing);
        assert_eq!(
            labels_of(&placed, Some(hovered), lacking)
                .into_iter()
                .find(|label| label.text == profiles::PROFILES[missing].title)
                .map(|label| label.color),
            Some(palette.menu_item_hint_text),
        );
    }

    /// PIN — the marked picker makes room for its marks.
    ///
    /// `min-width: 100%` is a floor and the popup grows leftward to hold its
    /// widest label; an item that also carries a 14px `.ticon` and its 10px gap
    /// needs 24px more than that label, and a popup sized without them crops the
    /// last glyph of every profile name.
    #[test]
    fn the_marked_picker_reserves_its_icon_column_on_top_of_the_widest_label() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let widest = 120.0 * scale;
            let marked = open_rows_measured(
                scale,
                Some(SettingsRow::DefaultProfile),
                TabLayoutMode::Horizontal,
                widest,
            );
            let plain = open_rows_measured(
                scale,
                Some(SettingsRow::Theme),
                TabLayoutMode::Horizontal,
                widest,
            );
            let width = |placed: &SettingsLayout| {
                let menu = placed.menu.expect("the picker is open");
                menu[2] - menu[0]
            };
            assert_eq!(
                width(&marked) - width(&plain),
                ((OPTION_ICON_COLUMN_LOGICAL_PX + ITEM_GAP_LOGICAL_PX) * scale).ceil(),
                "scale {scale}: exactly the column and its gap, and not a pixel \
                 of slack invented here"
            );
        }
    }

    /// The dialog's close affordance is the mock-up's own `#i-close`, and it is
    /// the only mark the overlay draws.
    #[test]
    fn the_close_affordance_wears_the_mock_ups_own_close_symbol() {
        let placed = open(1.0, true);
        let sprites = sprites_of(&placed, None, values());
        assert_eq!(sprites.len(), 1);
        assert_eq!(sprites[0].mark, ChromeMark::WindowClose);
        let glyph = sprites[0].rect;
        assert_eq!(width(glyph), 10.0, "the close icon is 10px");
        assert_eq!(height(glyph), 10.0);
        assert!(
            (((glyph[0] + glyph[2]) / 2.0) - ((placed.close[0] + placed.close[2]) / 2.0)).abs()
                <= 0.5,
            "the icon is centred in its 30px button"
        );
    }

    /// PIN (user screenshot 2026-08-10): `min-width: 100%` is a **floor**.
    ///
    /// The popup used to take the button's width as its size outright, so an
    /// option longer than its button was cropped mid-glyph and the item's own
    /// hover pill was cropped with it. The measured label now sets the width
    /// when it is the larger of the two, and the arithmetic is stated here in
    /// the numbers the stylesheet uses: the box must hold the text *plus* the
    /// chrome around it, which is the menu's two borders and padding, the item's
    /// padding on both sides, the fixed tick column and the gap after it.
    #[test]
    fn a_picker_wider_than_its_button_grows_to_hold_its_longest_option() {
        // Wider than the 118px button can hold, so the floor loses and the text wins.
        const MEASURED: f32 = 200.0;
        let placed = open_rows_measured(
            1.0,
            Some(SettingsRow::Sidebar),
            TabLayoutMode::Vertical,
            MEASURED,
        );
        let menu = placed.menu.expect("the picker is open");
        let combo = combo_of(&placed, SettingsRow::Sidebar);
        let chrome = 2.0 * 1.0
            + 2.0 * MENU_PADDING_LOGICAL_PX
            + 2.0 * ITEM_PADDING_X_LOGICAL_PX
            + TICK_WIDTH_LOGICAL_PX
            + ITEM_GAP_LOGICAL_PX;

        assert!(
            width(menu) > width(combo),
            "a label too wide for the button must widen the popup, not be cut by it"
        );
        assert_eq!(width(menu), chrome + MEASURED);
        assert_eq!(
            menu[2], combo[2],
            "`right: 0` still holds - the extra width is taken on the left"
        );

        // The item, its pill and its text all inherit the new width, which is
        // the half the screenshot actually showed: a cropped pill under cropped
        // words.
        let item = placed.items[0];
        assert_eq!(
            item[2] - item[0],
            width(menu) - 2.0 * 1.0 - 2.0 * MENU_PADDING_LOGICAL_PX
        );
        let labels = labels_of(&placed, None, values());
        // The closed button draws this word too, so the popup's own copy is the
        // one inside the popup — picking the first match finds the button's.
        let drawn = labels
            .iter()
            .find(|label| label.text == "Expanded" && overlaps(label.rect, menu))
            .expect("the option is drawn inside the picker");
        assert!(
            width(drawn.rect) >= MEASURED,
            "the text's own box must not be narrower than the text it holds: {} < {MEASURED}",
            width(drawn.rect)
        );
        assert!(
            within(drawn.rect, menu),
            "and it must stay inside the popup that drew it"
        );
    }

    /// PIN: the widening is conditional, so every picker that shipped before it
    /// is untouched. Theme, Cursor and Tab layout each wear one short word, and
    /// a short word must leave the popup exactly as wide as its button.
    #[test]
    fn a_picker_whose_options_all_fit_keeps_the_button_width_it_always_had() {
        for (row, tab_layout) in [
            (SettingsRow::Theme, TabLayoutMode::Horizontal),
            (SettingsRow::Cursor, TabLayoutMode::Horizontal),
            (SettingsRow::Formulas, TabLayoutMode::Horizontal),
            (SettingsRow::TabLayout, TabLayoutMode::Horizontal),
            (SettingsRow::Sidebar, TabLayoutMode::Vertical),
        ] {
            // 40px of glyphs is roomier than any of these words needs and still
            // fits inside the button, so the floor must win.
            let placed = open_rows_measured(1.0, Some(row), tab_layout, 40.0);
            let menu = placed.menu.expect("the picker is open");
            let combo = combo_of(&placed, row);
            assert_eq!(
                width(menu),
                width(combo),
                "{row:?}: a picker whose options fit must not grow a pixel"
            );
        }
    }

    /// PIN: a popup too wide for the window is pulled back inside it, the same
    /// clamp `profiles`, `tooltip` and `peek_strip` each apply to their own
    /// floating boxes.
    #[test]
    fn a_picker_too_wide_for_the_window_is_clamped_into_it() {
        let rows = flat_rows();
        let placed = layout_for_menu(
            780.0,
            800.0,
            1.0,
            Some(SettingsRow::Theme),
            content(&rows, &[]),
            PAGE,
            4_000.0,
            UNSCROLLED,
        )
        .expect("the dialog still fits this window");
        let menu = placed.menu.expect("the picker is open");
        assert!(
            menu[0] >= 0.0,
            "the popup must not start off the left edge of the window: {menu:?}"
        );
    }

    /// PIN (user ruling 2026-08-10): the Display formulas row, and the one thing
    /// its switch means. Off is a *rendering* choice - the row says so in the
    /// dialog, because a user who reads "off" and expects the formula to vanish
    /// has been told the wrong thing.
    #[test]
    fn the_display_formulas_row_offers_on_and_off_and_says_what_off_does() {
        let placed = open_rows_measured(
            1.0,
            Some(SettingsRow::Formulas),
            TabLayoutMode::Horizontal,
            0.0,
        );
        let labels = labels_of(&placed, None, values());
        for text in [
            "Display formulas",
            "Typeset $$…$$ blocks; off shows the LaTeX source",
            "On",
            "Off",
        ] {
            assert!(
                labels.iter().any(|label| label.text == text),
                "{text:?} is part of the row and is not drawn"
            );
        }
        assert_eq!(SettingsRow::Formulas.option_count(), 2);
        assert_eq!(
            FORMULA_OPTIONS,
            [true, false],
            "On is the first item, as it is in every On/Off picker the mock-up draws"
        );
    }

    /// PIN: clicking an item asks for exactly the value that item stands for,
    /// and clicking anything else asks for nothing.
    #[test]
    fn only_the_formula_rows_items_ask_for_a_formula_setting() {
        assert_eq!(
            display_formulas_requested(SettingsTarget::Choice(SettingsRow::Formulas, 0)),
            Some(true)
        );
        assert_eq!(
            display_formulas_requested(SettingsTarget::Choice(SettingsRow::Formulas, 1)),
            Some(false)
        );
        assert_eq!(
            display_formulas_requested(SettingsTarget::Choice(SettingsRow::Formulas, 2)),
            None,
            "there is no third option to ask for"
        );
        for target in [
            SettingsTarget::Choice(SettingsRow::Theme, 0),
            SettingsTarget::Choice(SettingsRow::Cursor, 0),
            SettingsTarget::Combo(SettingsRow::Formulas),
            SettingsTarget::Scrim,
            SettingsTarget::Panel,
            SettingsTarget::Close,
        ] {
            assert_eq!(display_formulas_requested(target), None, "{target:?}");
        }
    }

    /// PIN: the row draws the value it is given, both ways round. A tick that
    /// ignored the stored value would still look right in one of the two states.
    #[test]
    fn the_formula_rows_tick_follows_the_stored_value() {
        for (display_formulas, expected) in [(true, 0usize), (false, 1usize)] {
            let placed = open_rows_measured(
                1.0,
                Some(SettingsRow::Formulas),
                TabLayoutMode::Horizontal,
                0.0,
            );
            let values = SettingsValues {
                display_formulas,
                ..values()
            };
            assert_eq!(
                SettingsRow::Formulas.selected_index(values),
                Some(expected),
                "display_formulas={display_formulas} must tick item {expected}"
            );
            let labels = labels_of(&placed, None, values);
            let ticks = labels.iter().filter(|label| label.text == TICK).count();
            assert_eq!(ticks, 1, "exactly one item wears the tick");
        }
    }

    /// PIN (§7.1.6c-4a) — the two scheme rows are on Appearance, directly under
    /// Theme, and each offers only the schemes its own canvas can wear.
    ///
    /// The filtering is the load-bearing half: a dark scheme reachable from the
    /// Light row would paint a dark canvas and then be dressed in the *other*
    /// row's chrome, because the chrome picks its palette from the luma of the
    /// background actually painted. See `crate::schemes::Catalogue::names_for`.
    #[test]
    fn the_two_scheme_rows_sit_under_theme_and_each_offers_only_its_own_canvas() {
        let rows = visible_rows(TabLayoutMode::Horizontal);
        let theme = rows
            .iter()
            .position(|row| *row == SettingsRow::Theme)
            .expect("Theme is in the dialog");
        assert_eq!(rows[theme + 1], SettingsRow::LightScheme);
        assert_eq!(rows[theme + 2], SettingsRow::DarkScheme);
        for row in [SettingsRow::LightScheme, SettingsRow::DarkScheme] {
            assert_eq!(row.category(), SettingsCategory::Appearance);
            assert!(row.option_count() > 0, "{row:?} offers something");
            assert_eq!(
                row.option_labels().count(),
                row.option_count(),
                "{row:?}: every item the picker counts is an item it can draw"
            );
        }
        let light: Vec<&str> = SettingsRow::LightScheme.option_labels().collect();
        let dark: Vec<&str> = SettingsRow::DarkScheme.option_labels().collect();
        assert!(light.contains(&"Folio Light"));
        assert!(dark.contains(&"Folio Dark"));
        assert!(dark.contains(&"Solarized Dark"));
        for name in &light {
            assert!(
                !dark.contains(name),
                "{name} cannot be offered by both rows"
            );
        }
    }

    /// PIN — a stored name this build does not hold ticks the scheme the window
    /// is actually drawn in, and the two rows never answer each other's press.
    #[test]
    fn a_missing_scheme_ticks_the_default_and_a_press_answers_one_row_only() {
        let default_light = scheme_index(bt_persist::DEFAULT_LIGHT_SCHEME, true);
        assert_eq!(scheme_index("A Scheme Nobody Wrote", true), default_light);
        assert_eq!(
            scheme_labels(true)[default_light],
            crate::schemes::FOLIO_LIGHT_NAME
        );
        let default_dark = scheme_index("Solarized Light", false);
        assert_eq!(
            scheme_labels(false)[default_dark],
            crate::schemes::FOLIO_DARK_NAME,
            "a light name asked for by the dark row is as gone as one nobody wrote"
        );

        let light = SettingsTarget::Choice(SettingsRow::LightScheme, 0);
        let dark = SettingsTarget::Choice(SettingsRow::DarkScheme, 0);
        assert_eq!(light_scheme_requested(light), Some("Folio Light"));
        assert_eq!(light_scheme_requested(dark), None);
        assert_eq!(dark_scheme_requested(dark), Some("Folio Dark"));
        assert_eq!(dark_scheme_requested(light), None);
        for target in [
            SettingsTarget::Combo(SettingsRow::LightScheme),
            SettingsTarget::Menu(SettingsRow::DarkScheme),
            SettingsTarget::Choice(SettingsRow::Theme, 0),
        ] {
            assert_eq!(light_scheme_requested(target), None);
            assert_eq!(dark_scheme_requested(target), None);
        }
        assert_eq!(
            light_scheme_requested(SettingsTarget::Choice(
                SettingsRow::LightScheme,
                SettingsRow::LightScheme.option_count()
            )),
            None,
            "past the end of the list is not a scheme"
        );
    }

    // ── the window's ground (§7.1.6c-4b) ───────────────────────────────────

    /// PIN — the slider occupies the picker's column exactly, so the right edge
    /// of every control in the dialog is still one line.
    ///
    /// Held as a *sum* and not as three literals: the day somebody widens the
    /// number to fit four digits is the day the track has to give back eight
    /// pixels, and three constants that each look reasonable on their own are
    /// how a column comes to be 126 wide in one row and 118 in the next.
    #[test]
    fn the_slider_occupies_the_combos_column() {
        assert_eq!(
            SLIDER_TRACK_WIDTH_LOGICAL_PX + SLIDER_GAP_LOGICAL_PX + SLIDER_VALUE_WIDTH_LOGICAL_PX,
            COMBO_MIN_WIDTH_LOGICAL_PX
        );
        let placed = open(1.0, false);
        let picker = combo_of(&placed, SettingsRow::Theme);
        for row in [SettingsRow::ImageOpacity, SettingsRow::BackgroundOpacity] {
            let control = combo_of(&placed, row);
            assert_eq!(control[0], picker[0], "{row:?} starts in the same column");
            assert_eq!(control[2], picker[2], "{row:?} ends in the same column");
        }
    }

    /// PIN — a slider's parts come out of one derivation, and the thumb travels
    /// the **track's** full length rather than its own.
    ///
    /// Red gate: measure the travel against `track_width - thumb_width` — the
    /// naive reading — and 100% lands six pixels short of the right-hand end at
    /// scale 1. Far enough to be unreachable by drag, not far enough to look
    /// like a bug.
    #[test]
    fn a_sliders_thumb_travels_the_whole_track_and_its_parts_share_one_derivation() {
        let placed = open(1.0, false);
        let control = combo_of(&placed, SettingsRow::ImageOpacity);
        let range = SliderRange { min: 0, max: 100 };

        let floor = slider_geometry(control, 1.0, range, 0);
        assert_eq!(floor.track[0], control[0]);
        assert_eq!(
            width(floor.track),
            SLIDER_TRACK_WIDTH_LOGICAL_PX,
            "the track is the track's width and not the control's"
        );
        assert_eq!(
            (floor.thumb[0] + floor.thumb[2]) / 2.0,
            floor.track[0],
            "at the floor the thumb is centred on the track's left end"
        );
        assert_eq!(
            floor.fill[2], floor.fill[0],
            "nothing is filled at the floor"
        );

        let ceiling = slider_geometry(control, 1.0, range, 100);
        assert_eq!(
            (ceiling.thumb[0] + ceiling.thumb[2]) / 2.0,
            ceiling.track[2],
            "at the ceiling it is centred on the right end, not a thumb short of it"
        );
        assert_eq!(width(ceiling.fill), width(ceiling.track));

        let half = slider_geometry(control, 1.0, range, 50);
        assert!(
            ((half.thumb[0] + half.thumb[2]) / 2.0 - (half.track[0] + half.track[2]) / 2.0).abs()
                < 0.01
        );
        // The thumb is round and centred on the track's own middle at every
        // value, which is what keeps it from riding up as the row's text column
        // changes height.
        for value in [0, 25, 50, 75, 100] {
            let geometry = slider_geometry(control, 1.0, range, value);
            assert_eq!(
                width(geometry.thumb),
                SLIDER_THUMB_LOGICAL_PX,
                "{value}%: the thumb is one size"
            );
            assert!(
                ((geometry.thumb[1] + geometry.thumb[3]) / 2.0
                    - (geometry.track[1] + geometry.track[3]) / 2.0)
                    .abs()
                    < 0.01,
                "{value}%: the thumb sits on the track's middle"
            );
        }
        // And the number lives to the right of the track, one gap away, ending
        // where the control does.
        assert_eq!(floor.value[0], floor.track[2] + SLIDER_GAP_LOGICAL_PX);
        assert_eq!(floor.value[2], control[2]);
    }

    /// PIN — the hit test reads the same derivation the paint does, at every
    /// scale, and both ends of the track are reachable.
    ///
    /// The DPI sweep is the half with teeth: a `slider_value_at` that forgot to
    /// scale the track would answer 100% a third of the way along at 2x, and the
    /// dialog would look right in every screenshot taken at 1x.
    #[test]
    fn a_press_anywhere_on_a_track_asks_for_the_value_drawn_there() {
        for scale in [1.0_f32, 1.25, 2.0] {
            let placed = open_page(scale, None, TabLayoutMode::Horizontal, PAGE, 0.0);
            let control = combo_of(&placed, SettingsRow::ImageOpacity);
            let range = SliderRange { min: 0, max: 100 };
            let geometry = slider_geometry(control, scale, range, 0);

            assert_eq!(
                placed.slider_at(SettingsRow::ImageOpacity, f64::from(geometry.track[0])),
                Some(0)
            );
            assert_eq!(
                placed.slider_at(SettingsRow::ImageOpacity, f64::from(geometry.track[2])),
                Some(100),
                "scale {scale}: the far end of the track is the top of the range"
            );
            let middle = f64::from((geometry.track[0] + geometry.track[2]) / 2.0);
            assert_eq!(
                placed.slider_at(SettingsRow::ImageOpacity, middle),
                Some(50)
            );

            // A drag that ran off either end asks for the end, not for a value
            // outside the range and not for whatever the arithmetic produced.
            assert_eq!(
                placed.slider_at(
                    SettingsRow::ImageOpacity,
                    f64::from(geometry.track[0]) - 4000.0
                ),
                Some(0)
            );
            assert_eq!(
                placed.slider_at(
                    SettingsRow::ImageOpacity,
                    f64::from(geometry.track[2]) + 4000.0
                ),
                Some(100)
            );

            // The ground's own slider has a floor, and its left-hand end is that
            // floor rather than zero — the one place the two sliders differ.
            let ground = combo_of(&placed, SettingsRow::BackgroundOpacity);
            assert_eq!(
                placed.slider_at(SettingsRow::BackgroundOpacity, f64::from(ground[0])),
                Some(bt_persist::MINIMUM_BACKGROUND_OPACITY),
                "scale {scale}: there is no setting from which this window can be \
                 made unreadable"
            );
            // And a row that is not a slider has no value to ask for.
            assert_eq!(placed.slider_at(SettingsRow::Theme, middle), None);
        }
    }

    /// PIN — a press on a track answers `Slider(row)`, and the two forms live in
    /// one column without confusing the hit test.
    #[test]
    fn a_slider_row_answers_the_pointer_as_a_slider_and_a_picker_row_as_a_picker() {
        let placed = open(1.0, false);
        for row in [SettingsRow::ImageOpacity, SettingsRow::BackgroundOpacity] {
            let (x, y) = centre(combo_of(&placed, row));
            assert_eq!(hit(&placed, values(), x, y), SettingsTarget::Slider(row));
            assert!(row.control().range().is_some());
            assert_eq!(row.option_count(), 0, "a slider has no items to open");
        }
        for row in [
            SettingsRow::BackgroundImage,
            SettingsRow::ImageFit,
            SettingsRow::Acrylic,
            SettingsRow::AlwaysOnTop,
        ] {
            let (x, y) = centre(combo_of(&placed, row));
            assert_eq!(hit(&placed, values(), x, y), SettingsTarget::Combo(row));
            assert!(row.control().range().is_none());
        }
    }

    /// PIN — the four keys a focused slider owns, and the fact that they are
    /// still the page's keys everywhere else.
    ///
    /// Red gate: let `←` fall through to `step_out_of_page` while a slider has
    /// the ring, and the one control in this dialog that cannot be operated
    /// without a mouse is the one whose whole vocabulary is the arrow keys.
    #[test]
    fn a_focused_slider_owns_the_arrows_and_the_ends_and_nothing_else_does() {
        let mut panel = keyboarded();
        // Walk down to the picture's opacity row.
        let mut guard = 0;
        while panel.focus() != Some(SettingsTarget::Slider(SettingsRow::ImageOpacity)) {
            keyed(&mut panel, SettingsKey::Down);
            guard += 1;
            assert!(guard < 64, "the slider is somewhere in the Tab order");
        }

        assert_eq!(
            keyed(&mut panel, SettingsKey::Right),
            SettingsKeyVerdict::Inert,
            "the picture starts at 100%, and its ceiling has nowhere further to go"
        );
        assert_eq!(
            keyed(&mut panel, SettingsKey::Left),
            SettingsKeyVerdict::Adjusted(SettingsRow::ImageOpacity, 95),
            "one press is one step of five, from where the value actually is"
        );
        assert_eq!(
            keyed(&mut panel, SettingsKey::Home),
            SettingsKeyVerdict::Adjusted(SettingsRow::ImageOpacity, 0)
        );
        assert_eq!(
            keyed(&mut panel, SettingsKey::End),
            SettingsKeyVerdict::Inert,
            "already at the top, because `values()` reads 100 whatever the keys did"
        );
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Slider(SettingsRow::ImageOpacity)),
            "none of the four moved the ring out of the control"
        );
        assert_eq!(
            keyed(&mut panel, SettingsKey::Activate),
            SettingsKeyVerdict::Inert,
            "Enter on a slider has nothing to open and nothing to choose"
        );

        // Shift+Tab still leaves, which is what stops the slider from being a
        // place the keyboard cannot get out of.
        assert_eq!(
            keyed(&mut panel, SettingsKey::Tab { backwards: true }),
            SettingsKeyVerdict::Moved
        );
        assert_ne!(
            panel.focus(),
            Some(SettingsTarget::Slider(SettingsRow::ImageOpacity))
        );

        // And on a picker row the four keys mean exactly what they always meant.
        let mut picker = keyboarded();
        assert_eq!(
            picker.focus(),
            Some(SettingsTarget::Combo(SettingsRow::Theme))
        );
        keyed(&mut picker, SettingsKey::Left);
        assert_eq!(
            picker.focus(),
            Some(SettingsTarget::Nav(PAGE)),
            "`←` off a picker is still `back to the rail`"
        );
    }

    /// PIN — a step is taken from the value that is there, never snapped to a
    /// grid, and it stops at both ends.
    ///
    /// A file may say 63 because somebody typed it; the first arrow press is
    /// exactly the moment they are not looking, and a snap to 65 would be this
    /// program quietly rewriting a number it was given.
    #[test]
    fn an_arrow_steps_from_where_the_value_is_and_stops_at_the_ends() {
        let range = SliderRange { min: 30, max: 100 };
        assert_eq!(range.stepped(63, 1), 68);
        assert_eq!(range.stepped(63, -1), 58);
        assert_eq!(range.stepped(100, 1), 100);
        assert_eq!(range.stepped(32, -1), 30, "the floor holds");
        assert_eq!(
            range.stepped(7, 1),
            35,
            "a value under the floor is lifted first"
        );
        assert_eq!(range.stepped(240, -1), 95);
        assert_eq!(
            i16::from(SLIDER_STEP_PERCENT),
            5,
            "twenty presses across the range, and one percent of a picture's \
             presence is not a thing anybody can see"
        );

        // The fraction and the value are each other's inverse at the ends and at
        // the middle, which is what makes a press and the thumb it moves agree.
        assert_eq!(range.fraction(30), 0.0);
        assert_eq!(range.fraction(100), 1.0);
        assert_eq!(range.value_at(0.0), 30);
        assert_eq!(range.value_at(1.0), 100);
        assert_eq!(range.value_at(0.5), 65);
        // Rounded and not truncated: a drag that truncated would leave the thumb
        // a whole step behind the pointer on the way up.
        assert_eq!(range.value_at(0.499), 65);
        assert_eq!(range.clamp(7), 30);
        assert_eq!(range.clamp(200), 100);
    }

    /// PIN — a row this machine cannot honour is greyed WHOLE, says why on its
    /// own line, and refuses both the pointer and Enter.
    ///
    /// The capability is injected rather than read off the machine running the
    /// suite, which is `profile_available`'s ruling: a test about greying that
    /// depended on which Windows CI happened to be on would be green for the
    /// wrong reason on one of them.
    #[test]
    fn a_row_this_machine_cannot_honour_is_greyed_whole_and_says_why() {
        let palette = chrome_palette();
        let placed = open(1.0, false);

        let mut lacking = values();
        lacking.acrylic_available = false;
        lacking.translucency_available = false;

        for row in [SettingsRow::Acrylic, SettingsRow::BackgroundOpacity] {
            assert!(
                row.available(values()),
                "{row:?} is live on a capable machine"
            );
            assert!(!row.available(lacking));
            assert_ne!(
                row.description(lacking),
                row.description(values()),
                "{row:?}'s muted line becomes the reason it cannot act"
            );
            assert!(
                !row.description(lacking).trim().is_empty(),
                "{row:?} greyed without a reason is a dead control"
            );

            // The pointer lands on the dialog's body and never on the control:
            // the picker must not open and the track must not move.
            let (x, y) = centre(combo_of(&placed, row));
            assert_eq!(
                hit(&placed, lacking, x, y),
                SettingsTarget::Panel,
                "{row:?} must not answer a press while it cannot act"
            );
            assert_eq!(hit(&placed, values(), x, y), row.control_target());
        }

        // Every one of the row's three parts steps back to the hint ink, and
        // nothing on a live row does.
        let greyed = labels_of(&placed, None, lacking);
        let live = labels_of(&placed, None, values());
        for row in [SettingsRow::Acrylic, SettingsRow::BackgroundOpacity] {
            let title = row_of(&placed, row).title;
            let drawn = greyed
                .iter()
                .find(|label| label.rect == title)
                .expect("a greyed row still draws its title");
            assert_eq!(
                drawn.color, palette.menu_item_hint_text,
                "{row:?}'s title wears the ink this house says `cannot` in"
            );
            let lit = live
                .iter()
                .find(|label| label.rect == title)
                .expect("a live row draws its title");
            assert_eq!(lit.color, palette.dialog_title_text);
        }

        // The ring may still stand on it — a ring is not an action — and Enter
        // is refused there, which is what "no dead controls" actually forbids.
        let rows = flat_rows();
        let lines = shortcut_lines();
        let mut panel = SettingsPanel::default();
        panel.toggle(content(&rows, &lines));
        panel.select_category(PAGE);
        panel.press(SettingsTarget::Combo(SettingsRow::Acrylic));
        assert_eq!(
            panel.key(SettingsKey::Activate, content(&rows, &lines), lacking),
            SettingsKeyVerdict::Inert,
            "Enter on a row that cannot act does nothing and says nothing"
        );
    }

    /// PIN — the Background image row's button carries the chosen file's NAME,
    /// and `None` while there is none.
    ///
    /// The name and not the path, because 118px of picker cannot hold a path and
    /// one ellipsised from the right shows the half nobody needs.
    #[test]
    fn the_background_image_button_carries_the_pictures_name() {
        let placed = open(1.0, false);
        let box_of = combo_value_box(combo_of(&placed, SettingsRow::BackgroundImage));

        let empty = labels_of(&placed, None, values());
        let drawn = empty
            .iter()
            .find(|label| label.rect == box_of)
            .expect("the row draws a value");
        assert_eq!(drawn.text, Text::OptionImageNone.text());

        let mut chosen = values();
        chosen.background_image = true;
        let lines = shortcut_lines();
        let mut measure = measure;
        let named: Vec<ChromeLabel> = build(
            &placed,
            None,
            None,
            chosen,
            &lines,
            "ridge.jpg",
            None,
            &mut measure,
        )
        .into_iter()
        .flat_map(|layer| layer.labels)
        .collect();
        let drawn = named
            .iter()
            .find(|label| label.rect == box_of)
            .expect("the row draws a value");
        assert_eq!(drawn.text, "ridge.jpg");

        // Neither item is ticked once a picture is named: the answer is the file,
        // and `None` is a verb that would clear it.
        assert_eq!(SettingsRow::BackgroundImage.selected_index(chosen), None);
        assert_eq!(
            SettingsRow::BackgroundImage.selected_index(values()),
            Some(0)
        );
        assert_eq!(
            background_image_requested(SettingsTarget::Choice(SettingsRow::BackgroundImage, 1)),
            Some(ImageSource::Choose)
        );
        assert_eq!(
            background_image_requested(SettingsTarget::Choice(SettingsRow::BackgroundImage, 0)),
            Some(ImageSource::None)
        );
        assert_eq!(
            background_image_requested(SettingsTarget::Combo(SettingsRow::BackgroundImage)),
            None
        );
    }

    /// PIN — every one of the six rows maps its picker items to exactly its own
    /// setting, and to nobody else's.
    #[test]
    fn each_ground_picker_item_maps_to_its_own_setting_and_no_other() {
        for (index, fit) in IMAGE_FIT_OPTIONS.iter().enumerate() {
            let target = SettingsTarget::Choice(SettingsRow::ImageFit, index);
            assert_eq!(image_fit_requested(target), Some(*fit));
            assert_eq!(image_fit_index(*fit), index, "the tick round-trips");
            assert_eq!(acrylic_requested(target), None);
            assert_eq!(always_on_top_requested(target), None);
            assert_eq!(background_image_requested(target), None);
        }
        for (index, on) in FORMULA_OPTIONS.iter().enumerate() {
            let acrylic = SettingsTarget::Choice(SettingsRow::Acrylic, index);
            assert_eq!(acrylic_requested(acrylic), Some(*on));
            assert_eq!(always_on_top_requested(acrylic), None);
            assert_eq!(git_panel_requested(acrylic), None);
            let on_top = SettingsTarget::Choice(SettingsRow::AlwaysOnTop, index);
            assert_eq!(always_on_top_requested(on_top), Some(*on));
            assert_eq!(acrylic_requested(on_top), None);
        }
        // The two rows read what the window is doing, not what was stored about
        // whether it was asked.
        let mut on = values();
        on.acrylic = true;
        on.always_on_top = true;
        assert_eq!(SettingsRow::Acrylic.selected_index(on), Some(0));
        assert_eq!(SettingsRow::AlwaysOnTop.selected_index(on), Some(0));
        assert_eq!(SettingsRow::Acrylic.selected_index(values()), Some(1));
    }

    /// PIN — `sample()` is the settings a fresh install reads, so a geometry
    /// test is never quietly a test of somebody's `settings.json`.
    ///
    /// It is checked against `bt_persist`'s own defaults rather than against
    /// literals, which is what stops the two drifting: the day the floor moves,
    /// this fails here instead of in whichever geometry pin happened to notice.
    #[test]
    fn the_sample_reading_is_a_fresh_installs_ground() {
        let sample = values();
        assert!(!sample.background_image);
        assert_eq!(
            sample.background_fit,
            image_fit_index(bt_persist::BackgroundFitV1::default())
        );
        assert_eq!(
            sample.background_image_opacity,
            bt_persist::DEFAULT_BACKGROUND_IMAGE_OPACITY
        );
        assert_eq!(
            sample.background_opacity,
            bt_persist::DEFAULT_BACKGROUND_OPACITY
        );
        assert!(!sample.acrylic);
        assert!(!sample.always_on_top);
        assert!(sample.acrylic_available && sample.translucency_available);
        assert_eq!(
            SettingsRow::ImageOpacity.slider_value(sample),
            Some(bt_persist::DEFAULT_BACKGROUND_IMAGE_OPACITY)
        );
        assert_eq!(
            SettingsRow::BackgroundOpacity.slider_value(sample),
            Some(bt_persist::DEFAULT_BACKGROUND_OPACITY)
        );
        assert_eq!(SettingsRow::Theme.slider_value(sample), None);

        // A stored value under the floor is lifted where it is read, because
        // `bt_persist` deliberately stores what it was given and a thumb outside
        // its own track is a thumb somewhere else entirely.
        let mut hand_edited = sample;
        hand_edited.background_opacity = 7;
        assert_eq!(
            SettingsRow::BackgroundOpacity.slider_value(hand_edited),
            Some(bt_persist::MINIMUM_BACKGROUND_OPACITY)
        );
    }

    /// PIN — the six rows are in the Appearance page, in the order the ruling
    /// gave them, immediately after the pair that says what colour the ground is.
    #[test]
    fn the_ground_rows_follow_the_schemes_in_the_order_they_are_decided() {
        let rows = visible_rows(TabLayoutMode::Horizontal);
        let ground = [
            SettingsRow::BackgroundImage,
            SettingsRow::ImageFit,
            SettingsRow::ImageOpacity,
            SettingsRow::BackgroundOpacity,
            SettingsRow::Acrylic,
            SettingsRow::AlwaysOnTop,
        ];
        let start = rows
            .iter()
            .position(|row| *row == SettingsRow::BackgroundImage)
            .expect("the ground is in the dialog");
        assert_eq!(&rows[start..start + ground.len()], &ground);
        for row in ground {
            assert_eq!(row.category(), SettingsCategory::Appearance);
        }
        // On the page they are contiguous too, which is what puts them under one
        // heading rather than splitting the Appearance group in two.
        let page: Vec<SettingsRow> = rows
            .into_iter()
            .filter(|row| row.category() == SettingsCategory::Appearance)
            .collect();
        let start = page
            .iter()
            .position(|row| *row == SettingsRow::BackgroundImage)
            .expect("the ground is on the page");
        assert_eq!(&page[start..start + ground.len()], &ground);
        assert_eq!(
            page[start - 1],
            SettingsRow::DarkScheme,
            "the ground follows the pair that says what colour it is"
        );
    }

    /// PIN (Q191, mock-up 5644): `$("row-railmode").style.display =
    /// state.layoutMode === "vertical" ? "" : "none"` — Sidebar is a dependent
    /// of Tab layout and is not in the dialog at all while the tabs run across
    /// the top.
    #[test]
    fn the_sidebar_row_is_only_in_the_dialog_while_the_tabs_run_down_the_side() {
        assert_eq!(
            visible_rows(TabLayoutMode::Horizontal),
            [
                SettingsRow::Theme,
                SettingsRow::LightScheme,
                SettingsRow::DarkScheme,
                SettingsRow::BackgroundImage,
                SettingsRow::ImageFit,
                SettingsRow::ImageOpacity,
                SettingsRow::BackgroundOpacity,
                SettingsRow::Acrylic,
                SettingsRow::AlwaysOnTop,
                SettingsRow::Cursor,
                SettingsRow::TabLayout,
                SettingsRow::SplitDirection,
                SettingsRow::TerminalFont,
                SettingsRow::FontSize,
                SettingsRow::Formulas,
                SettingsRow::InlineFormulas,
                SettingsRow::Language,
                SettingsRow::GitPanel,
                SettingsRow::DefaultProfile,
                SettingsRow::PsReadLine
            ]
        );
        assert_eq!(
            visible_rows(TabLayoutMode::Vertical),
            [
                SettingsRow::Theme,
                SettingsRow::LightScheme,
                SettingsRow::DarkScheme,
                SettingsRow::BackgroundImage,
                SettingsRow::ImageFit,
                SettingsRow::ImageOpacity,
                SettingsRow::BackgroundOpacity,
                SettingsRow::Acrylic,
                SettingsRow::AlwaysOnTop,
                SettingsRow::Cursor,
                SettingsRow::TabLayout,
                SettingsRow::Sidebar,
                SettingsRow::SplitDirection,
                SettingsRow::TerminalFont,
                SettingsRow::FontSize,
                SettingsRow::Formulas,
                SettingsRow::InlineFormulas,
                SettingsRow::Language,
                SettingsRow::GitPanel,
                SettingsRow::DefaultProfile,
                SettingsRow::PsReadLine
            ],
            "Sidebar still lands directly under the row it depends on, Split \
             direction under the pair of them, the two font rows next to each \
             other because they are one decision in two halves, the two formula \
             rows together as the whole of the Rendered blocks group, and the \
             PSReadLine row last because it is the whole of the Terminal page"
        );
    }

    /// PIN (Q191): a row that is not in the list is in none of the four places a
    /// row exists — it costs no height, draws no text, and no point in the
    /// window answers for its control.
    ///
    /// Red gate: the shape this exists to keep out is a row that was taught the
    /// condition in one place and not the others — a control you can click but
    /// cannot see, or a gap where a row used to be.
    #[test]
    fn a_hidden_sidebar_row_costs_no_height_no_ink_and_no_hit_target() {
        let flat = open_rows(1.0, None, TabLayoutMode::Horizontal);
        let railed = open_rows(1.0, None, TabLayoutMode::Vertical);
        assert!(flat.row(SettingsRow::Sidebar).is_none());
        assert!(railed.row(SettingsRow::Sidebar).is_some());
        assert_eq!(
            height(railed.frame) - height(flat.frame),
            ROW_HEIGHT,
            "the dialog is exactly one row shorter without it"
        );

        let mut y = 0.0_f32;
        while y < SURFACE.1 {
            let mut x = 0.0_f32;
            while x < SURFACE.0 {
                let target = hit(&flat, values(), f64::from(x), f64::from(y));
                assert!(
                    !matches!(
                        target,
                        SettingsTarget::Combo(SettingsRow::Sidebar)
                            | SettingsTarget::Menu(SettingsRow::Sidebar)
                            | SettingsTarget::Choice(SettingsRow::Sidebar, _)
                    ),
                    "({x}, {y}) answers {target:?} for a row the dialog does not have"
                );
                x += 3.0;
            }
            y += 3.0;
        }

        let labels = labels_of(&flat, None, values());
        for absent in [
            SettingsRow::Sidebar.title(),
            SettingsRow::Sidebar.description(values()),
        ] {
            assert!(
                !labels.iter().any(|label| label.text == absent),
                "{absent:?} is drawn for a row the dialog does not have"
            );
        }
    }

    /// PIN (mock-up geometry): the rows are one stack at one pitch inside a group,
    /// whatever is in it, and crossing into the next group costs exactly one
    /// heading and not a pixel more.
    ///
    /// Two claims in one, because the pair is what a group *is*: rows that are
    /// evenly spaced say "these belong together" — and since the rail arrived
    /// that is the *only* step a page has, because a page holds one category and
    /// therefore one heading. The larger step this test used to also pin went
    /// with `margin-top: 16px`.
    #[test]
    fn rows_of_a_page_stack_one_row_height_apart() {
        for layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
            for category in SettingsCategory::ALL {
                if category == SettingsCategory::Shortcuts {
                    continue;
                }
                let placed = open_page(1.0, None, layout, category, 0.0);
                let expected: Vec<SettingsRow> = visible_rows(layout)
                    .into_iter()
                    .filter(|row| row.category() == category)
                    .collect();
                assert_eq!(placed.rows.len(), expected.len());
                assert_eq!(
                    placed.groups.len(),
                    usize::from(!expected.is_empty()),
                    "{category:?}: a page draws one heading, or none at all"
                );
                for pair in placed.rows.windows(2) {
                    let (above, below) = (&pair[0], &pair[1]);
                    assert_eq!(
                        below.combo[1] - above.combo[1],
                        ROW_HEIGHT,
                        "{:?} follows {:?}",
                        below.row,
                        above.row
                    );
                    assert_eq!(width(below.combo), width(above.combo));
                    assert_eq!(height(below.combo), height(above.combo));
                    assert_eq!(below.title[1] - above.title[1], ROW_HEIGHT);
                    assert_eq!(below.desc[1] - above.desc[1], ROW_HEIGHT);
                }
            }
        }
    }

    #[test]
    fn each_tab_layout_picker_item_maps_to_its_corresponding_set_value() {
        let placed = open_rows(1.0, Some(SettingsRow::TabLayout), TabLayoutMode::Vertical);
        assert_eq!(placed.items.len(), TAB_LAYOUT_OPTIONS.len());
        for (index, item) in placed.items.iter().enumerate() {
            let (x, y) = centre(*item);
            let target = hit(&placed, values(), x, y);
            assert_eq!(
                target,
                SettingsTarget::Choice(SettingsRow::TabLayout, index)
            );
            assert_eq!(
                tab_layout_requested(target),
                Some(TAB_LAYOUT_OPTIONS[index])
            );
            assert_eq!(sidebar_mode_requested(target), None);
            assert_eq!(theme_requested(target), None);
            assert_eq!(cursor_style_requested(target), None);
        }
        for target in [
            SettingsTarget::Scrim,
            SettingsTarget::Panel,
            SettingsTarget::Close,
            SettingsTarget::Combo(SettingsRow::TabLayout),
            SettingsTarget::Menu(SettingsRow::TabLayout),
        ] {
            assert_eq!(tab_layout_requested(target), None);
        }
    }

    #[test]
    fn each_sidebar_picker_item_maps_to_its_corresponding_set_value() {
        let placed = open_rows(1.0, Some(SettingsRow::Sidebar), TabLayoutMode::Vertical);
        assert_eq!(placed.items.len(), SIDEBAR_OPTIONS.len());
        for (index, item) in placed.items.iter().enumerate() {
            let (x, y) = centre(*item);
            let target = hit(&placed, values(), x, y);
            assert_eq!(target, SettingsTarget::Choice(SettingsRow::Sidebar, index));
            assert_eq!(sidebar_mode_requested(target), Some(SIDEBAR_OPTIONS[index]));
            assert_eq!(tab_layout_requested(target), None);
        }
        for target in [
            SettingsTarget::Scrim,
            SettingsTarget::Panel,
            SettingsTarget::Close,
            SettingsTarget::Combo(SettingsRow::Sidebar),
            SettingsTarget::Menu(SettingsRow::Sidebar),
        ] {
            assert_eq!(sidebar_mode_requested(target), None);
        }
    }

    /// The two new rows wear the mock-up's own words (lines 2358-2390), the
    /// sidebar's second option among them: user ruling 2026-08-10 cut it from
    /// "Icons, expand on hover" down to the mode's own name, and the mock-up
    /// carries the same word.
    #[test]
    fn the_new_rows_wear_the_mock_ups_own_words() {
        let placed = open_rows(1.0, Some(SettingsRow::Sidebar), TabLayoutMode::Vertical);
        let labels = labels_of(&placed, None, values());
        for text in [
            "Tab layout",
            "Choose where tabs appear in the window",
            "Sidebar",
            "How the vertical tab sidebar rests",
            "Expanded",
            "Icons",
        ] {
            assert!(
                labels.iter().any(|label| label.text == text),
                "{text:?} is the mock-up's own line and is not drawn"
            );
        }
        assert!(
            !labels
                .iter()
                .any(|label| label.text == "Icons, expand on hover"),
            "the sentence form is what the ruling removed; drawing it again is the regression"
        );
    }

    // ── the dialog's own keyboard (slice 1, 2026-08-16) ────────────────────

    /// The dialog in a window too short to hold its whole stack, at this scroll.
    fn cramped(scroll: f32) -> SettingsLayout {
        let rows = flat_rows();
        layout_for_menu(
            SURFACE.0,
            320.0,
            1.0,
            None,
            content(&rows, &[]),
            PAGE,
            0.0,
            scroll,
        )
        .expect("a short window still hosts the dialog")
    }

    /// An open dialog with the keyboard already inside a page, ready to be
    /// driven — on the page the mock-up's own rows live on.
    ///
    /// Opening lands on `General` and on the rail's own word for it; every claim
    /// below that is about a *row* wants the page those rows are on and the
    /// keyboard already in it, which is the rail's two steps: choose, then walk
    /// in.
    fn keyboarded() -> SettingsPanel {
        keyboarded_on(PAGE)
    }

    fn keyboarded_on(category: SettingsCategory) -> SettingsPanel {
        let rows = flat_rows();
        let lines = shortcut_lines();
        let mut panel = SettingsPanel::default();
        panel.toggle(content(&rows, &lines));
        panel.select_category(category);
        // Entered with the pointer rather than with `Right`, so the ring starts
        // off exactly as it does on a freshly opened dialog: every claim below
        // about "the first key lights it" would otherwise be starting from a
        // dialog somebody had already pressed a key on.
        if let Some(first) = page_order(content(&rows, &lines), category).first() {
            panel.press(*first);
        }
        panel
    }

    /// The dialog's whole contents, for the claims that drive it.
    fn keyed(panel: &mut SettingsPanel, key: SettingsKey) -> SettingsKeyVerdict {
        let rows = flat_rows();
        let lines = shortcut_lines();
        panel.key(key, content(&rows, &lines), values())
    }

    /// Where the focus is after this run of keys, from a dialog already inside
    /// its page.
    fn after(keys: &[SettingsKey]) -> Option<SettingsTarget> {
        let mut panel = keyboarded();
        for key in keys {
            keyed(&mut panel, *key);
        }
        panel.focus()
    }

    /// The Tab order of the page every keyboard claim below is stated against.
    fn page_focus_order() -> Vec<SettingsTarget> {
        let rows = flat_rows();
        let lines = shortcut_lines();
        focus_order(content(&rows, &lines), PAGE)
    }

    /// PIN: **the Tab order is the page's order** — the close first, because it
    /// is in the header above everything, then every visible row's control top
    /// to bottom. It is derived from the same row list the drawing is, so the
    /// conditional Sidebar row is a stop exactly while it is a row.
    ///
    /// Red gate: state the order beside the rows instead of from them and the
    /// second half goes red — Sidebar is reachable by keyboard in a dialog that
    /// is not drawing it, which is `visible_rows`' own bug one surface along.
    #[test]
    fn the_tab_order_is_the_close_the_rail_then_every_visible_row_in_the_order_it_is_drawn() {
        let flat = flat_rows();
        let lines = shortcut_lines();
        let expected: Vec<SettingsTarget> = [SettingsTarget::Close, SettingsTarget::Nav(PAGE)]
            .into_iter()
            .chain(
                flat.iter()
                    .filter(|row| row.category() == PAGE)
                    // Each row's OWN control, which is a picker for all but the
                    // two sliders — derived rather than assumed, for the reason
                    // the order itself is derived from the rows (§7.1.6c-4b).
                    .map(|row| row.control_target()),
            )
            .collect();
        assert_eq!(focus_order(content(&flat, &lines), PAGE), expected);
        // **The rail is one stop and not five.** Tab crosses between the parts
        // of a dialog; a list you walk with the arrows is one part, and putting
        // every word of it in the Tab order would make reaching a page's first
        // row cost as many presses as the rail is long.
        assert_eq!(
            focus_order(content(&flat, &lines), PAGE)
                .iter()
                .filter(|stop| matches!(stop, SettingsTarget::Nav(_)))
                .count(),
            1,
            "the rail contributes the selected word and no other"
        );

        let down = visible_rows(TabLayoutMode::Vertical);
        assert!(
            focus_order(content(&down, &lines), PAGE)
                .contains(&SettingsTarget::Combo(SettingsRow::Sidebar)),
            "the sidebar row is a keyboard stop while the tabs run down the side"
        );
        assert!(
            !focus_order(content(&flat, &lines), PAGE)
                .contains(&SettingsTarget::Combo(SettingsRow::Sidebar)),
            "and is not one while the dialog is not drawing it"
        );
    }

    /// PIN (S4/Q3, 2026-08-17) — **the rail and the page are two regions, and
    /// the arrows never carry you between them.**
    ///
    /// Four promises: `↑`/`↓` inside the rail move the rail *and turn the page
    /// with it* (the tab pattern's automatic activation, which is what makes a
    /// keyboard walk cost what a click costs); `→`, Tab and Enter are the three
    /// ways in; `←` and Shift+Tab are the ways back; and a page turned resets
    /// nothing but itself — the focus stays on the rail word the user is
    /// walking, because a focus that jumped into the page would make the second
    /// `↓` land somewhere they were not looking.
    ///
    /// MUTATIONS:
    /// (1) let `↓` fall out of the rail into the page — the "still on the rail"
    ///     assertion goes red, and a user walking the rail is tipped into a
    ///     picker;
    /// (2) drop the automatic activation — the page never changes and the first
    ///     assertion goes red;
    /// (3) make `←` unwind the picker as well as leave the page — the last
    ///     assertion goes red, and Esc has a rival.
    #[test]
    fn the_arrows_walk_the_rail_and_the_page_without_ever_crossing_between_them() {
        let rows = flat_rows();
        let lines = shortcut_lines();
        let rail = content(&rows, &lines).nav_items();
        let mut panel = SettingsPanel::default();
        panel.toggle(content(&rows, &lines));
        assert_eq!(panel.category(), rail[0]);

        // Tab from the page's first control goes back to the rail, which is
        // where a user who overshot expects to land.
        assert_eq!(
            panel.focus(),
            page_order(content(&rows, &lines), rail[0]).first().copied(),
            "opening seats the keyboard on the page, as it always has"
        );
        keyed(&mut panel, SettingsKey::Left);
        assert_eq!(panel.focus(), Some(SettingsTarget::Nav(rail[0])));

        // Down the rail: the page turns and the focus stays on the rail.
        for expected in &rail[1..] {
            keyed(&mut panel, SettingsKey::Down);
            assert_eq!(panel.category(), *expected, "the page turned with the walk");
            assert_eq!(
                panel.focus(),
                Some(SettingsTarget::Nav(*expected)),
                "and the keyboard is still on the rail"
            );
        }
        // And it wraps, like every other walk in this dialog.
        keyed(&mut panel, SettingsKey::Down);
        assert_eq!(panel.category(), rail[0]);

        // Three ways in, one way back.
        for way_in in [
            SettingsKey::Right,
            SettingsKey::Tab { backwards: false },
            SettingsKey::Activate,
        ] {
            let mut panel = SettingsPanel::default();
            panel.toggle(content(&rows, &lines));
            keyed(&mut panel, SettingsKey::Left);
            assert_eq!(panel.focus(), Some(SettingsTarget::Nav(rail[0])));
            keyed(&mut panel, way_in);
            assert_eq!(
                panel.focus(),
                page_order(content(&rows, &lines), rail[0]).first().copied(),
                "{way_in:?} takes the keyboard into the page"
            );
        }
        let mut back = keyboarded();
        keyed(&mut back, SettingsKey::Tab { backwards: true });
        assert_eq!(
            back.focus(),
            Some(SettingsTarget::Nav(PAGE)),
            "Shift+Tab from the first control returns to the rail"
        );

        // `←` inside an open picker does nothing: Esc is the one verb that
        // unwinds a layer, and a second one would be a second ladder.
        let mut open_picker = keyboarded();
        keyed(&mut open_picker, SettingsKey::Activate);
        assert!(open_picker.menu().is_some());
        keyed(&mut open_picker, SettingsKey::Left);
        assert!(
            open_picker.menu().is_some(),
            "the picker is Esc's to close, not the left arrow's"
        );
    }

    /// PIN: Tab walks that order, Shift+Tab walks it backwards, and both wrap.
    ///
    /// Wrapping rather than stopping because the dialog is the whole of what the
    /// keyboard can reach while it is up — there is nowhere past the last
    /// control for a focus to go, and a Tab that did nothing at the end would
    /// read as a dead key.
    #[test]
    fn tab_walks_the_order_and_wraps_at_both_ends() {
        let forwards = SettingsKey::Tab { backwards: false };
        let backwards = SettingsKey::Tab { backwards: true };
        let order = page_focus_order();

        // The keyboard is on the page's first control, which is two past the
        // close: the close, the rail's one word, then the rows.
        assert_eq!(after(&[]), Some(order[2]));
        assert_eq!(after(&[forwards]), Some(order[3]));
        assert_eq!(after(&[backwards]), Some(order[1]), "back onto the rail");
        assert_eq!(
            after(&[backwards, backwards]),
            Some(order[0]),
            "and past it onto the close"
        );
        assert_eq!(
            after(&[backwards, backwards, backwards]),
            order.last().copied(),
            "and past that, round to the last row"
        );

        let all_forwards: Vec<SettingsKey> = vec![forwards; order.len()];
        assert_eq!(
            after(&all_forwards),
            Some(order[2]),
            "a full lap comes back to where it started"
        );
    }

    /// PIN: the arrows are the same walk under another name, and Home/End are
    /// its two ends. One order and one lap, not a second navigation model that
    /// has to be kept in step with Tab's.
    #[test]
    fn the_arrows_home_and_end_walk_the_page_and_wrap_inside_it() {
        let order = page_focus_order();
        let first = order[2];
        let last = *order.last().expect("the page has controls");
        assert_eq!(after(&[SettingsKey::Down]), Some(order[3]));
        assert_eq!(
            after(&[SettingsKey::Up]),
            Some(last),
            "up from the first control wraps inside the page, never onto the rail"
        );
        assert_eq!(after(&[SettingsKey::Home]), Some(first));
        assert_eq!(after(&[SettingsKey::End]), Some(last));
        assert_eq!(
            after(&[SettingsKey::End, SettingsKey::Down]),
            Some(first),
            "the arrows wrap at the page's own two ends"
        );
    }

    /// PIN: **opening puts the keyboard on the first row, not on the close**,
    /// and with the ring off.
    ///
    /// The order starts at the close because the page does; where the focus
    /// starts is the other question, and the answer is the row the user opened
    /// the dialog to reach. A dialog that opened with the keyboard parked on
    /// "leave" would have spent its one free position on the verb nobody came
    /// for. The ring is off because a gear pressed with a mouse is not keyboard
    /// interaction — that is `:focus-visible`, not `:focus`.
    #[test]
    fn opening_the_dialog_puts_the_keyboard_on_the_first_row_with_no_ring() {
        let rows = flat_rows();
        let lines = shortcut_lines();
        let mut panel = SettingsPanel::default();
        panel.toggle(content(&rows, &lines));
        assert_eq!(
            panel.category(),
            SettingsCategory::General,
            "and it opens on the first page, not on the one it was left on"
        );
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Combo(SettingsRow::Language)),
            "the first row of the page it opened on - not the rail, and not the close"
        );
        assert_eq!(panel.focus_ring(), None, "opened by pointer, so no ring");

        // Left on another page, it still comes back to the first one.
        panel.select_category(SettingsCategory::Shortcuts);
        panel.toggle(content(&rows, &lines));
        panel.toggle(content(&rows, &lines));
        assert_eq!(panel.category(), SettingsCategory::General);

        let mut shut = keyboarded();
        shut.toggle(content(&rows, &lines));
        assert_eq!(
            shut.focus(),
            None,
            "a shut dialog holds no focus for the next opening to inherit"
        );
    }

    /// PIN (`:focus-visible`): a pointer press moves the focus and puts the ring
    /// away; any key press brings it back.
    ///
    /// Both halves, because either alone is a different rule. Without the first,
    /// clicking a control rings it and tells the user where their own finger
    /// was; without the second, a user who reaches for Tab after clicking gets a
    /// focus that moves invisibly.
    ///
    /// Red gate: draw the ring from `focus` instead of `focus_ring` and the
    /// second assertion goes red.
    #[test]
    fn a_pointer_press_moves_the_focus_and_puts_the_ring_away() {
        let mut panel = keyboarded();
        keyed(&mut panel, SettingsKey::Down);
        assert!(panel.focus_ring().is_some(), "a key lights the ring");

        panel.press(SettingsTarget::Combo(SettingsRow::Cursor));
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Combo(SettingsRow::Cursor)),
            "the focus follows the finger"
        );
        assert_eq!(panel.focus_ring(), None, "and the ring goes away");

        // The rail is pointer-reachable too, and moves the ring the same way.
        panel.press(SettingsTarget::Nav(SettingsCategory::Shortcuts));
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Nav(SettingsCategory::Shortcuts))
        );
        assert_eq!(panel.focus_ring(), None);

        // Even a press that lands on nothing is pointer interaction.
        keyed(&mut panel, SettingsKey::Down);
        assert!(panel.focus_ring().is_some());
        panel.press(SettingsTarget::Panel);
        assert_eq!(panel.focus_ring(), None);
    }

    /// PIN: Enter opens a picker **on the value it is already showing**, the
    /// arrows move from there, and Enter takes the one under them — reported as
    /// the very `Choice` a press on that item would have been, so the runtime
    /// runs one set of side effects rather than two.
    ///
    /// Space is the same verb: a button on the web answers both.
    #[test]
    fn enter_opens_a_picker_on_its_current_value_and_the_arrows_choose_from_there() {
        let mut panel = keyboarded();
        assert_eq!(
            keyed(&mut panel, SettingsKey::Activate),
            SettingsKeyVerdict::Moved
        );
        assert_eq!(panel.menu(), Some(SettingsRow::Theme));
        let selected = SettingsRow::Theme
            .selected_index(values())
            .expect("the theme row always has a value");
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Choice(SettingsRow::Theme, selected)),
            "the picker opens where the user already is"
        );

        keyed(&mut panel, SettingsKey::Down);
        let next = (selected + 1) % SettingsRow::Theme.option_count();
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Choice(SettingsRow::Theme, next))
        );
        assert_eq!(
            keyed(&mut panel, SettingsKey::Activate),
            SettingsKeyVerdict::Chose(SettingsTarget::Choice(SettingsRow::Theme, next)),
            "the same target a click on that item would have carried"
        );
        assert_eq!(panel.menu(), None, "choosing shuts the picker");
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Combo(SettingsRow::Theme)),
            "and hands the keyboard back to the button it hung under"
        );

        // Home and End are the picker's own two ends while it is open.
        let mut ended = keyboarded();
        keyed(&mut ended, SettingsKey::Activate);
        keyed(&mut ended, SettingsKey::End);
        assert_eq!(
            ended.focus(),
            Some(SettingsTarget::Choice(
                SettingsRow::Theme,
                SettingsRow::Theme.option_count() - 1
            ))
        );
        keyed(&mut ended, SettingsKey::Home);
        assert_eq!(
            ended.focus(),
            Some(SettingsTarget::Choice(SettingsRow::Theme, 0))
        );

        // And Tab leaves an open picker rather than walking inside it, which is
        // what keeps "one popup at a time" true of the keyboard.
        let mut tabbed = keyboarded();
        keyed(&mut tabbed, SettingsKey::Activate);
        keyed(&mut tabbed, SettingsKey::Tab { backwards: false });
        assert_eq!(tabbed.menu(), None);
        assert_eq!(
            tabbed.focus(),
            Some(SettingsTarget::Combo(SettingsRow::LightScheme))
        );
    }

    /// PIN (Esc, §7.1.5): one rung per press — the open picker, then the dialog
    /// — and the picker's rung leaves the keyboard on the button it hung under
    /// rather than dropping it.
    ///
    /// Red gate: close both layers on one press and the second assertion names a
    /// dialog that is already gone.
    #[test]
    fn escape_closes_the_picker_first_and_the_dialog_second() {
        let mut panel = keyboarded();
        keyed(&mut panel, SettingsKey::Activate);
        assert_eq!(panel.menu(), Some(SettingsRow::Theme));

        assert_eq!(
            keyed(&mut panel, SettingsKey::Escape),
            SettingsKeyVerdict::Moved
        );
        assert!(panel.is_open(), "the picker went first");
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Combo(SettingsRow::Theme))
        );

        assert_eq!(
            keyed(&mut panel, SettingsKey::Escape),
            SettingsKeyVerdict::Closed
        );
        assert!(!panel.is_open());
        assert_eq!(
            keyed(&mut panel, SettingsKey::Escape),
            SettingsKeyVerdict::Inert,
            "a shut dialog answers no key, so the press falls to whoever owns it next"
        );
    }

    /// PIN: **an option this machine cannot start is skipped by the arrows and
    /// refused by Enter** — the same [`SettingsRow::option_enabled`] answer the
    /// hit test and the draw already read. A greyed item the keyboard could land
    /// on would be the pointer's own bug, arrived at through the other door.
    #[test]
    fn the_keyboard_skips_an_option_this_machine_cannot_start() {
        let flat = flat_rows();
        let lines = shortcut_lines();
        let mut lacking = values();
        // Only the fallback shell is installed.
        lacking.profile_available =
            std::array::from_fn(|index| index == profiles::FALLBACK_PROFILE);
        lacking.default_profile = profiles::FALLBACK_PROFILE;

        let mut panel = keyboarded_on(SettingsRow::DefaultProfile.category());
        panel.key(SettingsKey::End, content(&flat, &lines), lacking);
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Combo(SettingsRow::DefaultProfile))
        );
        panel.key(SettingsKey::Activate, content(&flat, &lines), lacking);
        assert_eq!(panel.menu(), Some(SettingsRow::DefaultProfile));

        // Every arrow press from here lands on the one profile that can start.
        for _ in 0..profiles::PROFILES.len() + 1 {
            panel.key(SettingsKey::Down, content(&flat, &lines), lacking);
            assert_eq!(
                panel.focus(),
                Some(SettingsTarget::Choice(
                    SettingsRow::DefaultProfile,
                    profiles::FALLBACK_PROFILE
                )),
                "the arrows have nowhere else this machine can go"
            );
        }
        panel.key(SettingsKey::Home, content(&flat, &lines), lacking);
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Choice(
                SettingsRow::DefaultProfile,
                profiles::FALLBACK_PROFILE
            )),
            "Home lands on the first *choosable* item, not on a greyed one"
        );
    }

    /// PIN: a key the dialog has no verb for changes nothing, is still swallowed
    /// — the caller never gets a chance to type it into the terminal behind the
    /// scrim — and still turns the ring on, because it is still somebody using a
    /// keyboard.
    #[test]
    fn a_key_the_dialog_has_no_verb_for_is_swallowed_and_still_lights_the_ring() {
        let mut panel = keyboarded();
        let before = panel.focus();
        assert_eq!(
            keyed(&mut panel, SettingsKey::Other),
            SettingsKeyVerdict::Moved,
            "the ring appearing is a change worth repainting"
        );
        assert_eq!(panel.focus(), before, "and nothing moved");
        assert_eq!(panel.focus_ring(), before);
        assert_eq!(
            keyed(&mut panel, SettingsKey::Other),
            SettingsKeyVerdict::Inert,
            "the second one changes nothing at all"
        );
    }

    /// PIN: **the ring is the mock-up's own `button:focus-visible`** (2205):
    /// `outline: 2px solid var(--accent)` at `outline-offset: 1px`, round, and
    /// around the control — never over it.
    ///
    /// Red gate: draw it as a filled rectangle and the last assertion names the
    /// accent quad sitting on top of the value the button is showing.
    #[test]
    fn the_ring_is_the_mock_ups_own_outline_around_the_control_it_names() {
        let placed = open(1.0, false);
        let combo = combo_of(&placed, SettingsRow::Theme);
        let focus = Some(SettingsTarget::Combo(SettingsRow::Theme));
        let accent = chrome_palette().accent;

        // **Off the rail**, because the rail's selected word wears a 2px accent
        // bar as its standing mark and that is a selection, not a ring. This
        // test is about `:focus-visible`; the bar has its own pin.
        let nav = placed.nav;
        // **Outside every control**, which is what a ring is: `FOCUS_RING_OFFSET`
        // puts it beyond the box it names, while the two sliders' own fill and
        // thumb are the accent *inside* theirs (§7.1.6c-4b). Filtering by colour
        // alone would count a track as a ring on a dialog nobody is touching.
        let controls: Vec<[f32; 4]> = placed.rows.iter().map(|row| row.combo).collect();
        let inside_a_control = |quad: &OverlayQuad| {
            controls.iter().any(|control| {
                quad.rect[0] >= control[0] - 0.5
                    && quad.rect[2] <= control[2] + 0.5
                    && quad.rect[1] >= control[1] - 0.5
                    && quad.rect[3] <= control[3] + 0.5
            })
        };
        let unfocused: Vec<OverlayQuad> = quads_of(&placed, None, values())
            .into_iter()
            .filter(|quad| quad.color == accent)
            .filter(|quad| quad.rect[0] >= nav[2])
            .filter(|quad| !inside_a_control(quad))
            .collect();
        assert!(
            unfocused.is_empty(),
            "a dialog nobody is driving by keyboard draws no ring"
        );

        let ring: Vec<OverlayQuad> = built_with(&placed, None, focus, values(), None)
            .into_iter()
            .flat_map(|layer| layer.quads)
            .filter(|quad| quad.color == accent)
            .filter(|quad| quad.rect[0] >= nav[2])
            .filter(|quad| !inside_a_control(quad))
            .collect();
        assert!(!ring.is_empty(), "the focused control wears one");

        let reach = FOCUS_RING_OFFSET_LOGICAL_PX + FOCUS_RING_WIDTH_LOGICAL_PX;
        // The offset box snapped to whole pixels first, which is
        // `rounded_overlay_halo`'s own arithmetic: a ring and the box it
        // surrounds share one grid, or the outline is a pixel thicker on one
        // side than the other. A 27.5px control does not sit on that grid.
        let offset = FOCUS_RING_OFFSET_LOGICAL_PX;
        let width = FOCUS_RING_WIDTH_LOGICAL_PX;
        let outer = [
            (combo[0] - offset).round() - width,
            (combo[1] - offset).round() - width,
            (combo[2] + offset).round() + width,
            (combo[3] + offset).round() + width,
        ];
        // The value's own room: inside the button by the ring's whole reach and
        // its round, which is everything the outline may not touch.
        let clear = FOCUS_RING_RADIUS_LOGICAL_PX + reach;
        let inner = [
            combo[0] + clear,
            combo[1] + clear,
            combo[2] - clear,
            combo[3] - clear,
        ];
        assert!(inner[2] > inner[0] && inner[3] > inner[1], "a real box");
        for quad in &ring {
            assert!(
                within(quad.rect, outer),
                "the outline reaches {reach} past the control and no further: {:?}",
                quad.rect
            );
            assert!(
                !overlaps(quad.rect, inner),
                "an outline is around a control, not over it: {:?}",
                quad.rect
            );
        }
        assert!(
            ring.iter().any(|quad| quad.rect[1] < combo[1]),
            "and it really does stand off the top edge"
        );
    }

    /// PIN: **scrolling follows the focus, by exactly as much as it has to.**
    ///
    /// A row Tab reached below the fold is brought to the fold and no further:
    /// scrolling it to the top of the box instead would move every row the user
    /// was reading for a reason they did not ask about, which is what
    /// `scrollIntoView({ block: "nearest" })` settled on for the same reason. A
    /// row already in view moves nothing at all.
    ///
    /// Red gate: scroll the row to the content box's top instead and the
    /// "exactly at the fold" assertion names the overshoot.
    #[test]
    fn the_focus_scrolls_into_view_by_exactly_what_it_has_to() {
        let top = cramped(0.0);
        assert!(top.max_scroll() > 0.0, "this window really does overflow");

        let first = SettingsTarget::Combo(SettingsRow::Theme);
        assert_eq!(
            top.scroll_to_show(first, 0.0),
            0.0,
            "a row already in view moves nothing"
        );
        assert_eq!(
            top.scroll_to_show(SettingsTarget::Close, 0.0),
            0.0,
            "the header does not scroll"
        );

        let bottom_row = *flat_rows()
            .iter()
            .rfind(|row| row.category() == PAGE)
            .expect("the page has rows");
        let last = SettingsTarget::Combo(bottom_row);
        let scrolled_to = top.scroll_to_show(last, 0.0);
        assert!(scrolled_to > 0.0, "the last row is below the fold");
        let landed = cramped(scrolled_to);
        let band = landed.row(bottom_row).expect("held").band;
        let content = landed.content_box();
        assert!(
            band[1] >= content[1] && band[3] <= content[3],
            "the row is wholly inside the content box"
        );
        assert!(
            (band[3] - content[3]).abs() < 0.5,
            "and exactly at the fold — no further than it had to travel"
        );

        // And back the other way, by the same minimum.
        let back = landed.scroll_to_show(first, scrolled_to);
        assert!(back < scrolled_to);
        let risen = cramped(back);
        let theme = risen.row(SettingsRow::Theme).expect("held").band;
        assert!(
            (theme[1] - risen.content_box()[1]).abs() < 0.5,
            "the first row rose to the top edge and stopped there"
        );
    }

    /// PIN: a row that disappears out from under the focus hands it to one that
    /// is there.
    ///
    /// Choosing `Horizontal` in the Tab layout picker deletes the Sidebar row —
    /// and the Sidebar row may be the one the keyboard is standing on. A focus
    /// naming a row the dialog no longer holds draws no ring and answers no key,
    /// which reads as the dialog having swallowed the keyboard whole.
    #[test]
    fn a_row_that_disappears_under_the_focus_hands_it_back_to_a_row_that_is_there() {
        let down = visible_rows(TabLayoutMode::Vertical);
        let lines = shortcut_lines();
        let mut panel = SettingsPanel::default();
        panel.toggle(content(&down, &lines));
        panel.select_category(PAGE);
        panel.press(SettingsTarget::Combo(SettingsRow::Sidebar));
        panel.toggle_menu(SettingsRow::Sidebar);
        assert_eq!(
            panel.focus(),
            Some(SettingsTarget::Combo(SettingsRow::Sidebar))
        );

        let flat = flat_rows();
        panel.keep_focus_reachable(content(&flat, &lines));
        assert!(
            focus_order(content(&flat, &lines), PAGE)
                .contains(&panel.focus().expect("an open dialog holds a focus")),
            "the focus landed somewhere the dialog is actually drawing"
        );
        assert_eq!(
            panel.menu(),
            None,
            "and the picker that hung off the vanished row went with it"
        );
    }

    /// The mock-up itself, so a claim about "what the mock-up says" is read out
    /// of it rather than transcribed once and left to rot.
    const MOCKUP: &str = include_str!("../../../design/ui-mockup.html");

    /// The `.desc` line and the picker items of one `data-combo`, as the mock-up
    /// writes them.
    fn mockup_combo(kind: &str) -> (String, Vec<String>) {
        let at = MOCKUP
            .find(&format!("data-combo=\"{kind}\""))
            .expect("the mock-up has this combo");
        let head = &MOCKUP[..at];
        let desc_at = head
            .rfind("<div class=\"desc\"")
            .expect("every row in the mock-up carries a description");
        let desc = &head[desc_at..];
        let desc = &desc[desc.find('>').expect("an open tag closes") + 1..];
        let desc = desc[..desc.find("</div>").expect("and the element closes")].to_owned();

        // Every item of this combo and no other: the tail stops at the next one.
        let tail = &MOCKUP[at..];
        let tail = &tail[..tail[1..]
            .find("data-combo=")
            .map_or(tail.len(), |next| next + 1)];
        let items = tail
            .match_indices("class=\"combo-item")
            .map(|(start, _)| {
                let item = &tail[start..];
                let item = &item[..item.find("</div>").expect("an item closes")];
                let text = item.rfind("</span>").map_or(item, |end| &item[end + 7..]);
                text.trim().to_owned()
            })
            .collect();
        (desc, items)
    }

    /// PIN (Q5): **the Theme row is the mock-up's, in the mock-up's order, with
    /// the mock-up's line** — read out of `design/ui-mockup.html` rather than
    /// copied into an assertion.
    ///
    /// Both halves had drifted. The order was the enum's (`System` first) rather
    /// than the picker's, because the constant was written off `ThemeModeV1`
    /// instead of off the thing it draws. And the description read "Light or
    /// dark" under a note explaining that the mock-up's own line named a third
    /// option this build did not have — true when it was written, and false from
    /// the day System shipped, which left the one line a user reads to find out
    /// what the picker offers naming two of its three items.
    ///
    /// Red gate: this is the test that was missing. Change either the order or
    /// the sentence and it reads the mock-up's own answer back.
    #[test]
    fn the_theme_row_wears_the_mock_ups_own_order_and_its_own_line() {
        let (desc, items) = mockup_combo("theme");
        assert_eq!(items, ["Light", "Dark", "System"], "the fixture parsed");
        assert_eq!(
            SettingsRow::Theme.description(values()),
            desc,
            "the row's description is the mock-up's sentence"
        );
        let drawn: Vec<String> = SettingsRow::Theme
            .option_labels()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            drawn, items,
            "and its picker is the mock-up's list, in order"
        );
        assert_eq!(
            THEME_OPTIONS,
            [ThemeModeV1::Light, ThemeModeV1::Dark, ThemeModeV1::System],
            "which is what the press router maps by index"
        );
    }

    /// PIN (the Language row, 2026-08-17) — **the row exists, it is on the page
    /// a reader would look for it on, and it is the first thing there.**
    ///
    /// Red gate: before this slice `visible_rows` had no `Language` at all and
    /// `General` opened onto the Git panel switch. All three assertions fail
    /// against that build, and the third is the one worth stating out loud: a
    /// user who opens this dialog to change the language opens it unable to read
    /// it, so the row goes where the eye lands rather than under two sentences
    /// they cannot parse.
    #[test]
    fn the_language_row_leads_the_general_page() {
        let rows = visible_rows(TabLayoutMode::Horizontal);
        assert!(rows.contains(&SettingsRow::Language));
        assert_eq!(
            SettingsRow::Language.category(),
            SettingsCategory::General,
            "a language is what the window says, not what it looks like"
        );
        let page: Vec<SettingsRow> = rows
            .iter()
            .copied()
            .filter(|row| row.category() == SettingsCategory::General)
            .collect();
        assert_eq!(
            page.first(),
            Some(&SettingsRow::Language),
            "and it is the first row on that page"
        );
    }

    /// PIN — the Language picker is Theme's shape with Theme's own `System`, and
    /// the two named languages are endonyms.
    ///
    /// Every clause is a decision. `System` shared with Theme because it is one
    /// promise said once (T046 of the string inventory asks for the reuse by
    /// name); `System` **last** because that is the order this dialog's other
    /// three-way picker takes; and `中文` / `English` untranslated because the
    /// person who most needs to find their language is the person who cannot
    /// read the words around it — which is what every operating system's own
    /// picker concludes too.
    #[test]
    fn the_language_picker_names_each_language_in_itself_and_shares_theme_s_system() {
        let drawn: Vec<&str> = SettingsRow::Language.option_labels().collect();
        assert_eq!(drawn.len(), 3);
        assert_eq!(drawn[0], "中文");
        assert_eq!(drawn[1], "English");
        assert_eq!(
            drawn[2],
            crate::i18n::Text::OptionSystem.text(),
            "the third item is Theme's own word, not a second spelling of it"
        );
        assert_eq!(
            LANGUAGE_OPTIONS,
            [LanguageV1::Chinese, LanguageV1::English, LanguageV1::System],
            "which is what the press router maps by index"
        );
        assert_eq!(
            *THEME_OPTIONS.last().expect("three themes"),
            ThemeModeV1::System,
            "and the two pickers put the ask-somebody-else answer in the same place"
        );
    }

    /// PIN — the tick sits on the **stored mode**, and a press asks for a mode.
    ///
    /// The trap is the resolved language: on a Chinese Windows, `System` and
    /// `中文` come out at the same place, and a row that ticked the resolved
    /// answer would show `中文` to a user who had chosen `System` — then write
    /// `中文` into their file the next time they touched anything else.
    #[test]
    fn the_language_row_ticks_what_the_file_says_and_asks_for_the_same_thing() {
        for (index, mode) in LANGUAGE_OPTIONS.into_iter().enumerate() {
            let values = SettingsValues {
                language: mode,
                ..values()
            };
            assert_eq!(
                SettingsRow::Language.selected_index(values),
                Some(index),
                "{mode:?} ticks its own item"
            );
            assert_eq!(
                language_requested(SettingsTarget::Choice(SettingsRow::Language, index)),
                Some(mode),
                "and pressing that item asks for {mode:?}"
            );
        }
        assert_eq!(
            language_requested(SettingsTarget::Choice(SettingsRow::Theme, 0)),
            None,
            "another row's press is not a language"
        );
        assert_eq!(language_requested(SettingsTarget::Close), None);
    }

    /// PIN — the row says *when* it takes effect, in whichever language it is
    /// currently saying it.
    ///
    /// The one line in this dialog that describes a moment rather than a value,
    /// and it has to, because this is the one setting whose effect is not on
    /// screen when it is chosen. See [`crate::i18n`]'s header for why there is no
    /// hot switch to describe instead.
    #[test]
    fn the_language_row_promises_the_next_start_in_both_languages() {
        assert_eq!(
            SettingsRow::Language.description(values()),
            crate::i18n::Text::DescLanguage.text()
        );
        assert_eq!(
            crate::i18n::Text::DescLanguage.in_lang(crate::i18n::Lang::English),
            "Applies the next time Folio starts"
        );
        assert_eq!(
            crate::i18n::Text::DescLanguage.in_lang(crate::i18n::Lang::Chinese),
            "下次启动 Folio 时生效"
        );
    }

    /// PIN (Q4): a description is a function of the row **and its values**.
    ///
    /// Today every row's answer is constant in the values — the two the mock-up
    /// varies (`wrap-desc`, `blockmax-desc`) have no setting behind them yet and
    /// no row here. What this pins is the shape: the parameter is in the
    /// signature, it reaches every arm, and the answer is still `&'static str`,
    /// which is the i18n ruling's own constraint on this method.
    ///
    /// Red gate: drop the parameter and this does not compile, which is the
    /// point — the slice that adds Line wrapping adds a match arm, not a
    /// migration of every call site.
    #[test]
    fn a_rows_description_is_a_function_of_its_values() {
        let light = SettingsValues {
            theme: ThemeModeV1::Light,
            ..values()
        };
        let dark = SettingsValues {
            theme: ThemeModeV1::Dark,
            ..values()
        };
        for row in visible_rows(TabLayoutMode::Vertical) {
            let line: &'static str = row.description(light);
            assert!(!line.is_empty(), "{row:?} says something");
            assert_eq!(
                line,
                row.description(dark),
                "{row:?} has no line that follows a value yet, and does not pretend to"
            );
        }
    }

    // ── the category rail and the shortcut page (slice 2, 2026-08-17) ──────

    /// PIN (Q2 = A) — **the rail's own geometry, stated from the tokens it was
    /// derived from.**
    ///
    /// There is no mock-up CSS to quote here, because the rail was a commented
    /// promise until today; what there is instead is a derivation, and this is
    /// the derivation written down: the dialog widened to 720, the rail is a
    /// 168px column of the body, the page is what is left of it, and the two are
    /// separated by the hairline every surface in this file is separated by.
    ///
    /// The last claim is the one that would rot silently: the *page* must still
    /// be able to hold a 118px picker between the mock-up's own 22px gutters,
    /// which is the whole reason 480 stopped working.
    ///
    /// MUTATIONS:
    /// (1) take the rail's width off the page instead of off the dialog — the
    ///     page's own width assertion goes red;
    /// (2) let the rail scroll with the page — `nav` stops spanning the body and
    ///     the "top to bottom" assertion goes red.
    #[test]
    fn the_rail_is_a_column_of_the_dialog_and_the_page_is_what_is_left_of_it() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let placed = open(scale, false);
            let border = (bt_render::FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
            assert!(
                (width(placed.frame) - 720.0 * scale).abs() <= 1.0,
                "scale {scale}: the dialog is the ruling's own 720 wide"
            );
            assert!(
                (width(placed.nav) - 168.0 * scale).abs() < 0.5,
                "scale {scale}: the rail keeps its column"
            );
            assert_eq!(
                placed.nav[0],
                placed.frame[0] + border,
                "scale {scale}: the rail starts at the dialog's own inside edge"
            );
            assert_eq!(
                placed.content_box()[0],
                placed.nav[2],
                "scale {scale}: the page begins where the rail ends"
            );
            assert!(
                (placed.nav[1] - (placed.frame[1] + border + HEADER_HEIGHT_LOGICAL_PX * scale))
                    .abs()
                    < 0.5,
                "scale {scale}: the rail starts under the header, not beside it"
            );
            assert_eq!(
                placed.nav[3],
                placed.content_box()[3],
                "scale {scale}: and runs to the bottom with the page"
            );
            let gutters = 2.0 * CONTENT_PADDING_X_LOGICAL_PX * scale;
            assert!(
                width(placed.content_box()) - gutters >= COMBO_MIN_WIDTH_LOGICAL_PX * scale,
                "scale {scale}: the page still holds a picker between the gutters"
            );
        }
    }

    /// PIN — **every word of the rail answers the pointer where it is drawn**,
    /// top to bottom in the declared order, and the page's own controls still
    /// answer beside it.
    ///
    /// The rail is a sixth reading of the row list (`SettingsRow::category`'s own
    /// doc counts them), and R4's lesson is that the first reading nobody
    /// teaches is a control the user can reach and cannot see.
    #[test]
    fn every_word_of_the_rail_answers_the_pointer_where_it_is_drawn() {
        let rows = flat_rows();
        let lines = shortcut_lines();
        let expected = content(&rows, &lines).nav_items();
        for scale in [1.0_f32, 1.25, 2.0] {
            let placed = open_page(scale, None, TabLayoutMode::Horizontal, PAGE, 0.0);
            assert_eq!(
                placed
                    .nav_items
                    .iter()
                    .map(|item| item.category)
                    .collect::<Vec<_>>(),
                expected,
                "scale {scale}: the rail draws the categories with content, in order"
            );
            for pair in placed.nav_items.windows(2) {
                assert!(
                    pair[1].band[1] > pair[0].band[1],
                    "scale {scale}: the rail runs top to bottom"
                );
                assert!(
                    (pair[1].band[1] - pair[0].band[3] - 2.0 * scale).abs() < 0.5,
                    "scale {scale}: one gap between two words"
                );
            }
            for item in &placed.nav_items {
                let (x, y) = centre(item.band);
                assert_eq!(
                    hit(&placed, values(), x, y),
                    SettingsTarget::Nav(item.category),
                    "scale {scale}: {:?} must answer where it is drawn",
                    item.category
                );
                assert!((height(item.band) - 30.0 * scale).abs() < 0.5);
                assert!(
                    item.label[0] > item.band[0],
                    "the word sits inside its own pill"
                );
            }
        }
    }

    /// Visual PIN — **the page that is up wears the ground whole; the word
    /// under the pointer wears it at half.**
    ///
    /// Two lit states with two strengths and no stroke (user ruling
    /// 2026-08-17): the selection is the answer and reads at full, the hover is
    /// the question and reads at half, and neither is a bar. Mutation: draw the
    /// hover at 1.0 and the two words become indistinguishable — the second
    /// assertion goes red.
    #[test]
    fn the_rail_marks_the_page_that_is_up_and_lights_the_word_under_the_pointer() {
        let placed = open(1.0, false);
        let palette = chrome_palette();
        let grounds_of = |hover| {
            quads_of(&placed, hover, values())
                .into_iter()
                .filter(|quad| quad.color == palette.dialog_hover)
                .filter(|quad| quad.rect[2] <= placed.nav[2])
                .collect::<Vec<_>>()
        };
        let selected = placed
            .nav_items
            .iter()
            .find(|item| item.category == PAGE)
            .expect("the page that is up is in the rail");
        let at_rest = grounds_of(None);
        assert!(!at_rest.is_empty(), "the page that is up is lit");
        // A rounded fill's corner bands carry their own partial alpha, so the
        // strength of a ground is its strongest quad, not every quad.
        assert!(
            (at_rest
                .iter()
                .map(|quad| quad.alpha)
                .fold(0.0_f32, f32::max)
                - 1.0)
                .abs()
                < 0.001,
            "and lit whole"
        );
        assert!(
            at_rest
                .iter()
                .all(|quad| quad.rect[1] >= selected.band[1] - 0.5
                    && quad.rect[3] <= selected.band[3] + 0.5),
            "and only it"
        );
        // No accent stroke anywhere in the rail — the ruling's whole point.
        assert!(
            !quads_of(&placed, None, values())
                .into_iter()
                .filter(|quad| quad.rect[2] <= placed.nav[2])
                .any(|quad| quad.color == palette.accent),
            "no bar"
        );

        let hovered_item = placed
            .nav_items
            .iter()
            .find(|item| item.category != PAGE)
            .expect("a second word in the rail");
        let other = hovered_item.category;
        let with_hover = grounds_of(Some(SettingsTarget::Nav(other)));
        let hover_alpha: Vec<f32> = with_hover
            .iter()
            .filter(|quad| {
                quad.rect[1] >= hovered_item.band[1] - 0.5
                    && quad.rect[3] <= hovered_item.band[3] + 0.5
            })
            .map(|quad| quad.alpha)
            .collect();
        assert!(!hover_alpha.is_empty(), "the hovered word is lit");
        assert!(
            (hover_alpha.iter().copied().fold(0.0_f32, f32::max) - NAV_HOVER_GROUND_ALPHA).abs()
                < 0.001,
            "at half strength, so it cannot be mistaken for the page that is up"
        );
        let selected_alpha: Vec<f32> = with_hover
            .iter()
            .filter(|quad| {
                quad.rect[1] >= selected.band[1] - 0.5 && quad.rect[3] <= selected.band[3] + 0.5
            })
            .map(|quad| quad.alpha)
            .collect();
        assert!(
            (selected_alpha.iter().copied().fold(0.0_f32, f32::max) - 1.0).abs() < 0.001,
            "while the page that is up stays whole"
        );
    }

    /// PIN (Q3 = A) — **one page at a time, and each page measures itself.**
    ///
    /// A page holds one category's rows and nothing else, so the dialog's height
    /// and the distance it can scroll are facts about *that* page — which is why
    /// turning a page resets the scroll rather than carrying it across.
    #[test]
    fn a_page_holds_its_own_rows_and_measures_its_own_scroll() {
        let rows = visible_rows(TabLayoutMode::Vertical);
        let lines = shortcut_lines();
        for category in content(&rows, &lines).nav_items() {
            let placed = open_page(1.0, None, TabLayoutMode::Vertical, category, 0.0);
            let expected: Vec<SettingsRow> = rows
                .iter()
                .copied()
                .filter(|row| row.category() == category)
                .collect();
            assert_eq!(
                placed.rows.iter().map(|row| row.row).collect::<Vec<_>>(),
                expected,
                "{category:?} draws its own rows and no others"
            );
            let shortcut_lines_drawn = placed.shortcuts.len();
            if category == SettingsCategory::Shortcuts {
                assert_eq!(shortcut_lines_drawn, lines.len());
                assert!(placed.restore_all.is_some(), "the page's own verb is there");
            } else {
                assert_eq!(
                    shortcut_lines_drawn, 0,
                    "{category:?} is not the shortcut page"
                );
                assert!(placed.restore_all.is_none());
            }
        }

        // The shortcut table is long and the row pages are short, so the two
        // pages disagree about how far there is to scroll — which is the whole
        // reason a distance cannot be carried between them.
        let short = open_page(1.0, None, TabLayoutMode::Vertical, PAGE, 0.0);
        let long = open_page(
            1.0,
            None,
            TabLayoutMode::Vertical,
            SettingsCategory::Shortcuts,
            0.0,
        );
        assert!(
            long.max_scroll() > short.max_scroll(),
            "the shortcut table overflows a window the Appearance page fits in"
        );

        // And the panel reports the turn, which is the signal the runtime resets
        // its own offset on.
        let mut panel = SettingsPanel::default();
        panel.toggle(content(&rows, &lines));
        assert!(
            panel.select_category(SettingsCategory::Shortcuts),
            "turning a page is a change worth reporting"
        );
        assert!(
            !panel.select_category(SettingsCategory::Shortcuts),
            "and turning to the page already up is not"
        );
    }

    /// PIN (S1/S3/S67) — **the shortcut page draws every line the table gives
    /// it, and each line's controls answer where they are drawn.**
    ///
    /// A `Record` on a line that offers one, a `↺` only where there is something
    /// to undo, and neither on a row the audit declined — the same three answers
    /// the focus order reads, because a control the pointer can press and the
    /// keyboard cannot reach is half a control.
    #[test]
    fn every_shortcut_line_answers_the_pointer_where_its_own_controls_are_drawn() {
        let mut table = crate::shortcuts::Shortcuts::defaults();
        table.set("new-tab", crate::shortcuts::parse_chord("Ctrl+Shift+Y"));
        let lines = table.editor_rows();
        let rows = visible_rows(TabLayoutMode::Horizontal);
        let placed = layout_for_menu(
            SURFACE.0,
            2_400.0,
            1.0,
            None,
            content(&rows, &lines),
            SettingsCategory::Shortcuts,
            0.0,
            UNSCROLLED,
        )
        .expect("a tall window hosts the whole table");
        assert_eq!(placed.max_scroll(), 0.0, "and hosts it without scrolling");
        assert_eq!(placed.shortcuts.len(), lines.len());

        for (line, drawn) in lines.iter().zip(&placed.shortcuts) {
            assert_eq!(drawn.record.is_some(), line.recordable, "{}", line.title);
            assert_eq!(drawn.restore.is_some(), line.overridden, "{}", line.title);
            if let Some(record) = drawn.record {
                let (x, y) = centre(record);
                assert_eq!(
                    hit(&placed, values(), x, y),
                    SettingsTarget::Record(drawn.index)
                );
            }
            if let Some(restore) = drawn.restore {
                let (x, y) = centre(restore);
                assert_eq!(
                    hit(&placed, values(), x, y),
                    SettingsTarget::RestoreRow(drawn.index)
                );
            }
            assert!(
                drawn.caps[2] <= drawn.record.map_or(drawn.band[2], |box_| box_[0]),
                "{}: the caps sit left of the button that changes them",
                line.title
            );
            assert!(
                drawn.title[2] <= drawn.caps[0],
                "{}: and the name sits left of the caps",
                line.title
            );
        }
        let (x, y) = centre(placed.restore_all.expect("the page's own verb"));
        assert_eq!(hit(&placed, values(), x, y), SettingsTarget::RestoreAll);

        // Exactly one line has a `↺`, which is the one row that departs.
        assert_eq!(
            placed
                .shortcuts
                .iter()
                .filter(|line| line.restore.is_some())
                .count(),
            1
        );
    }

    /// Visual PIN — **a chord is drawn as key caps, right to left from the
    /// button that changes it, and an unbound row says so in words.**
    ///
    /// The caps stay inside the room the layout reserved for them, which is what
    /// keeps the longest chord this table can produce from running into its own
    /// row's name.
    #[test]
    fn a_chord_is_drawn_as_caps_that_stay_in_the_room_reserved_for_them() {
        let lines = shortcut_lines();
        let rows = visible_rows(TabLayoutMode::Horizontal);
        let placed = layout_for_menu(
            SURFACE.0,
            2_400.0,
            1.0,
            None,
            content(&rows, &lines),
            SettingsCategory::Shortcuts,
            0.0,
            UNSCROLLED,
        )
        .expect("a tall window hosts the whole table");
        let drawn = build(
            &placed,
            None,
            None,
            values(),
            &lines,
            "",
            None,
            &mut measure,
        );
        let labels: Vec<ChromeLabel> = drawn
            .iter()
            .flat_map(|layer| layer.labels.clone())
            .collect();

        for (line, box_) in lines.iter().zip(&placed.shortcuts) {
            for cap in &line.caps {
                let label = labels
                    .iter()
                    .find(|label| {
                        &label.text == cap
                            && label.rect[1] >= box_.band[1] - 0.5
                            && label.rect[3] <= box_.band[3] + 0.5
                    })
                    .unwrap_or_else(|| panic!("{}: the cap {cap:?} is drawn", line.title));
                assert!(
                    label.rect[0] >= box_.caps[0] - 0.5 && label.rect[2] <= box_.caps[2] + 0.5,
                    "{}: the cap {cap:?} left the room reserved for it: {:?} in {:?}",
                    line.title,
                    label.rect,
                    box_.caps
                );
            }
            if line.caps.is_empty() {
                assert!(
                    labels
                        .iter()
                        .any(|label| label.text == crate::shortcuts::UNBOUND_CAP
                            && label.rect[1] >= box_.band[1] - 0.5
                            && label.rect[3] <= box_.band[3] + 0.5),
                    "{}: a row with no chord says so rather than drawing nothing",
                    line.title
                );
            }
        }
        assert!(
            labels.iter().any(|label| label.text == RESTORE_ALL_LABEL),
            "the page's own verb is drawn"
        );
    }

    /// PIN (S64) — **the recorder's whole state machine: accept, refuse, cancel,
    /// clear, confirm.**
    ///
    /// A refusal leaves the capture open, which is S64's real requirement: the
    /// point of refusing *at record time* rather than afterwards is that the user
    /// is still holding the keyboard and can simply press something else. A
    /// refusal that shut the box would make the second attempt cost another
    /// click on the button they are already looking at.
    ///
    /// MUTATIONS:
    /// (1) commit the candidate on arrival instead of on `Enter` — the refusal
    ///     assertions have nowhere left to stand and the "still listening" ones
    ///     go red;
    /// (2) let `Escape` fall through to `close_one_layer`'s dialog rung — the
    ///     cancel assertion closes the dialog and goes red.
    #[test]
    fn the_recorder_takes_a_chord_refuses_one_and_lets_go_of_both() {
        let chord = |text: &str| crate::shortcuts::parse_chord(text).expect("a chord");
        let caps = |text: &str| crate::shortcuts::chord_caps(&chord(text));

        // Accept, then confirm.
        let mut panel = keyboarded_on(SettingsCategory::Shortcuts);
        panel.begin_recording(0);
        assert_eq!(panel.recording_row(), Some(0));
        assert_eq!(
            panel.record(RecordInput::Modifier {
                caps: vec!["Ctrl".to_owned()]
            }),
            RecordVerdict::Moved
        );
        assert_eq!(
            panel
                .recording_state()
                .map(|(_, caps, hint)| (caps.to_vec(), hint.is_some())),
            Some((vec!["Ctrl".to_owned()], false)),
            "a modifier on its own is shown and waited on"
        );
        assert_eq!(
            panel.record(RecordInput::Candidate {
                caps: caps("Ctrl+Shift+Y"),
                chord: chord("Ctrl+Shift+Y"),
                refusal: None,
            }),
            RecordVerdict::Moved,
            "a chord is held pending, never taken on arrival"
        );
        assert_eq!(panel.recording_row(), Some(0), "and the box is still open");
        assert_eq!(
            panel.record(RecordInput::Confirm),
            RecordVerdict::Commit(0, Some(chord("Ctrl+Shift+Y")))
        );
        assert_eq!(panel.recording_row(), None, "confirming closes the box");
        assert_eq!(panel.focus(), Some(SettingsTarget::Record(0)));

        // Refuse, and stay.
        let mut panel = keyboarded_on(SettingsCategory::Shortcuts);
        panel.begin_recording(1);
        assert_eq!(
            panel.record(RecordInput::Candidate {
                caps: caps("Ctrl+Alt+P"),
                chord: chord("Ctrl+Alt+P"),
                refusal: Some(crate::shortcuts::HINT_ALTGR_ZONE.to_owned()),
            }),
            RecordVerdict::Moved
        );
        assert_eq!(
            panel.recording_state().and_then(|(_, _, hint)| hint),
            Some(crate::shortcuts::HINT_ALTGR_ZONE),
            "the refusal is shown"
        );
        assert_eq!(panel.recording_row(), Some(1), "and the box stays open");
        assert_eq!(
            panel.record(RecordInput::Confirm),
            RecordVerdict::Moved,
            "there is nothing standing for Enter to take"
        );
        assert_eq!(panel.recording_row(), Some(1));

        // Escape cancels the capture and nothing else — the dialog stays up.
        let rows = flat_rows();
        let lines = shortcut_lines();
        let mut panel = keyboarded_on(SettingsCategory::Shortcuts);
        panel.begin_recording(2);
        assert_eq!(
            panel.key(SettingsKey::Escape, content(&rows, &lines), values()),
            SettingsKeyVerdict::Moved
        );
        assert_eq!(panel.recording_row(), None, "the capture went first");
        assert!(panel.is_open(), "and the dialog is still up");
        assert_eq!(panel.focus(), Some(SettingsTarget::Record(2)));

        // Backspace clears the row outright, which is a write and not a cancel.
        let mut panel = keyboarded_on(SettingsCategory::Shortcuts);
        panel.begin_recording(3);
        assert_eq!(
            panel.record(RecordInput::Unbind),
            RecordVerdict::Commit(3, None)
        );
        assert_eq!(panel.recording_row(), None);

        // A key no file could hold says so and goes on waiting.
        let mut panel = keyboarded_on(SettingsCategory::Shortcuts);
        panel.begin_recording(0);
        assert_eq!(panel.record(RecordInput::Unusable), RecordVerdict::Moved);
        assert_eq!(
            panel.recording_state().and_then(|(_, _, hint)| hint),
            Some(RECORD_UNUSABLE_HINT)
        );
        assert_eq!(panel.recording_row(), Some(0));

        // Cancel leaves the row exactly as it was.
        let mut panel = keyboarded_on(SettingsCategory::Shortcuts);
        panel.begin_recording(0);
        panel.record(RecordInput::Candidate {
            caps: caps("Ctrl+Shift+Y"),
            chord: chord("Ctrl+Shift+Y"),
            refusal: None,
        });
        assert_eq!(panel.record(RecordInput::Cancel), RecordVerdict::Ended);
        assert_eq!(panel.recording_row(), None);

        // And a finger that goes anywhere else ends the capture too: a box that
        // swallows every key while nothing on screen says it is listening is the
        // dialog eating the keyboard.
        let mut panel = keyboarded_on(SettingsCategory::Shortcuts);
        panel.begin_recording(0);
        panel.press(SettingsTarget::Record(0));
        assert_eq!(
            panel.recording_row(),
            Some(0),
            "pressing the very button that is listening changes nothing"
        );
        panel.press(SettingsTarget::Panel);
        assert_eq!(panel.recording_row(), None);
    }

    /// Visual PIN — **a listening row says what it is waiting for, and says why
    /// in red when it refuses.**
    ///
    /// The recorder's words land on the row's own muted line, which is where the
    /// scope tag was: a hint printed anywhere else would be a second place to
    /// look for the answer to a question asked here.
    #[test]
    fn a_listening_row_borrows_its_own_muted_line_for_the_recorders_words() {
        let lines = shortcut_lines();
        let rows = visible_rows(TabLayoutMode::Horizontal);
        let placed = layout_for_menu(
            SURFACE.0,
            2_400.0,
            1.0,
            None,
            content(&rows, &lines),
            SettingsCategory::Shortcuts,
            0.0,
            UNSCROLLED,
        )
        .expect("a tall window hosts the whole table");
        let palette = chrome_palette();
        let focus = Some(SettingsTarget::Record(0));

        let waiting = vec!["Ctrl".to_owned(), "Shift".to_owned()];
        let drawn = build(
            &placed,
            None,
            focus,
            values(),
            &lines,
            "",
            Some((0, &waiting, None)),
            &mut measure,
        );
        let labels: Vec<ChromeLabel> = drawn
            .iter()
            .flat_map(|layer| layer.labels.clone())
            .collect();
        // Said through [`ellipsized`], because this column is the narrowest in
        // the dialog and the test's stand-in metric is wider than the real face:
        // what is pinned is that the *prompt* is what the row draws, not that a
        // particular number of its characters survive a fictional font.
        let room = placed.shortcuts[0].desc[2] - placed.shortcuts[0].desc[0];
        let fitted = |text: &str| ellipsized(text, room, ROW_DESC_FONT_LOGICAL_PX, &mut measure);
        assert!(
            labels
                .iter()
                .any(|label| label.text == fitted(RECORD_PROMPT)),
            "the box says what it is waiting for"
        );
        assert!(
            labels
                .iter()
                .any(|label| label.text == RECORD_LISTENING_LABEL),
            "and the button says it is listening"
        );
        for cap in &waiting {
            assert!(
                labels.iter().any(|label| &label.text == cap),
                "the modifiers already down are shown live"
            );
        }

        let refused = build(
            &placed,
            None,
            focus,
            values(),
            &lines,
            "",
            Some((0, &waiting, Some(crate::shortcuts::HINT_ALTGR_ZONE))),
            &mut measure,
        );
        let hint = refused
            .iter()
            .flat_map(|layer| layer.labels.clone())
            .find(|label| label.text == fitted(crate::shortcuts::HINT_ALTGR_ZONE))
            .expect("the refusal is drawn");

        // **A capture started with the pointer has no ring**, and must still
        // draw as listening. Found on the real window: the draw used to ask the
        // *ring* which row was capturing, and `:focus-visible` had just put the
        // ring away — so a box swallowing every key looked exactly like one that
        // was not, and the button under the finger still said `Record`.
        let by_pointer = build(
            &placed,
            None,
            None,
            values(),
            &lines,
            "",
            Some((0, &waiting, None)),
            &mut measure,
        );
        assert!(
            by_pointer
                .iter()
                .flat_map(|layer| layer.labels.clone())
                .any(|label| label.text == fitted(RECORD_PROMPT)),
            "a capture with no ring is still a capture"
        );
        assert_eq!(
            hint.color, palette.status_err,
            "a refusal is written in the ink this house says no in"
        );
        assert_eq!(
            hint.rect, placed.shortcuts[0].desc,
            "and it stands where the row's own muted line does"
        );
    }

    /// PIN — **the shortcut page's focus order is its own controls, and a `↺`
    /// that is not drawn is not a stop.**
    ///
    /// The same rule the conditional Sidebar row taught, met again: a ring on a
    /// control nobody can see is a dialog that looks like it has swallowed the
    /// keyboard.
    #[test]
    fn the_shortcut_pages_focus_order_holds_only_the_controls_it_draws() {
        let table = crate::shortcuts::Shortcuts::defaults();
        let lines = table.editor_rows();
        let rows = visible_rows(TabLayoutMode::Horizontal);
        let order = page_order(content(&rows, &lines), SettingsCategory::Shortcuts);
        assert_eq!(
            order.last(),
            Some(&SettingsTarget::RestoreAll),
            "the page's own verb is last"
        );
        assert!(
            !order
                .iter()
                .any(|stop| matches!(stop, SettingsTarget::RestoreRow(_))),
            "a table nobody has edited offers nothing to restore"
        );
        for (index, line) in lines.iter().enumerate() {
            assert_eq!(
                order.contains(&SettingsTarget::Record(index)),
                line.recordable,
                "{}",
                line.title
            );
        }

        let mut edited = crate::shortcuts::Shortcuts::defaults();
        edited.set("new-tab", crate::shortcuts::parse_chord("Ctrl+Shift+Y"));
        let lines = edited.editor_rows();
        let order = page_order(content(&rows, &lines), SettingsCategory::Shortcuts);
        assert!(
            order.contains(&SettingsTarget::RestoreRow(0)),
            "and an edited row grows the stop that undoes it"
        );
    }

    /// PIN (S4/S73, 2026-08-17) — **the rail was written back to the mock-up on
    /// the day it was born**, and this reads it out of the file rather than
    /// trusting a memory of having done so.
    ///
    /// `design/ui-mockup.html` is the only visual authority this dialog has. A
    /// surface built in the native tree and never written back is a surface with
    /// no authority at all — the next slice would have nothing to check itself
    /// against — so the rail's own words, its ids and the width that made room
    /// for it are asserted here against the file.
    ///
    /// Red gate: revert the mock-up and every assertion below names what is
    /// missing.
    #[test]
    fn the_mock_up_carries_the_rail_this_slice_gave_it() {
        let at = MOCKUP
            .find("id=\"settings-nav\"")
            .expect("the mock-up has the rail");
        let tail = &MOCKUP[at..];
        let rail = &tail[..tail.find("</nav>").expect("the rail closes")];
        let words: Vec<&str> = rail
            .match_indices("class=\"nav-item")
            .map(|(start, _)| {
                let item = &rail[start..];
                let text = &item[item.find('>').expect("the tag closes") + 1..];
                &text[..text.find("</button>").expect("the word closes")]
            })
            .collect();
        assert_eq!(
            words,
            [
                SettingsCategory::General.nav_label(),
                SettingsCategory::Appearance.nav_label(),
                SettingsCategory::Terminal.nav_label(),
                SettingsCategory::RenderedBlocks.nav_label(),
                SettingsCategory::Shortcuts.nav_label(),
            ],
            "every category this build knows is a word in the mock-up's rail, in \
             the enum's own order"
        );
        // The mock-up draws `Terminal` because *it* has a Line wrapping row; the
        // native build has no such setting and therefore no such word. The rail
        // is derived from the rows on both sides, which is the whole ruling —
        // and this is where the two readings are checked against each other.
        assert!(
            MOCKUP.contains("<div class=\"title\">Line wrapping</div>"),
            "the mock-up's Terminal page has the row that earns it"
        );

        assert!(
            MOCKUP.contains("width: min(720px, 92%);"),
            "the dialog widened in the mock-up too, or the rail has no room"
        );
        for id in [
            "class=\"settings-body\"",
            "class=\"settings-nav\"",
            "class=\"settings-page shown\"",
            "class=\"nav-item selected\"",
        ] {
            assert!(MOCKUP.contains(id), "the mock-up is missing {id}");
        }
        assert!(
            !MOCKUP.contains("style=\"margin-top:16px\""),
            "the later-heading margin retired with the flat list"
        );
        // And the shortcut editor, which is the other half of the same day.
        for text in [
            "class=\"cap\"",
            "class=\"sc-restore\"",
            RESTORE_ALL_LABEL,
            "keybindings.json",
        ] {
            assert!(MOCKUP.contains(text), "the mock-up is missing {text:?}");
        }
    }

    /// A fresh panel is shut, with nothing open inside it and nothing hovered.
    #[test]
    fn a_fresh_panel_is_shut() {
        let panel = SettingsPanel::default();
        assert!(!panel.is_open());
        assert!(panel.menu().is_none());
        assert_eq!(panel.hover(), None);
    }
}
