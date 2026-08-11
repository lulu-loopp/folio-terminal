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
//! Nothing here is layout: the dialog is not a seat, takes no space from the
//! solver, and is never persisted (a dialog does not survive a restart).

use bt_persist::ThemeModeV1;
use bt_render::{
    ChromeLabel, ChromeLabelWeight, CursorStyle, FLOAT_WINDOW_BORDER_LOGICAL_PX,
    FLOAT_WINDOW_RADIUS_LOGICAL_PX, FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad,
    WINDOW_CAPTION_GLYPH_LOGICAL_PX, chrome_palette, rounded_overlay_fill, rounded_overlay_halo,
};

use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::profiles;
use crate::seats::{RailMode, TabLayoutMode};

// ── `.settings`, `.overlay` ────────────────────────────────────────────────
/// `.settings { width: min(480px, 92%) }` — the cap and the share.
const DIALOG_MAX_WIDTH_LOGICAL_PX: f32 = 480.0;
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
const GROUP_LABEL_MARGIN_TOP_LOGICAL_PX: f32 = 10.0;
/// What every heading after the first stands off the group above it.
///
/// The mock-up writes the base `margin: 10px 0 2px` once and then overrides the
/// top on each later heading with an inline `style="margin-top:16px"`
/// (`design/ui-mockup.html:2406, 2421, 2452`) — the extra six pixels are what
/// separate one group from the next rather than one row from the next.
const GROUP_LABEL_MARGIN_TOP_LATER_LOGICAL_PX: f32 = 16.0;
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
/// `.combo > button { gap: 10px }` — between the value and the chevron.
const COMBO_GAP_LOGICAL_PX: f32 = 10.0;
/// The mock-up's chevron is the character, not the `#i-chev` symbol: a solid
/// down-pointing triangle set as text beside the value.
const COMBO_CHEVRON: &str = "\u{25bc}";

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

/// The persisted theme modes, in product order.
pub const THEME_OPTIONS: [ThemeModeV1; 3] =
    [ThemeModeV1::System, ThemeModeV1::Light, ThemeModeV1::Dark];
pub const CURSOR_OPTIONS: [CursorStyle; 3] =
    [CursorStyle::Bar, CursorStyle::Block, CursorStyle::Underline];
pub const TAB_LAYOUT_OPTIONS: [TabLayoutMode; 2] =
    [TabLayoutMode::Horizontal, TabLayoutMode::Vertical];
pub const SIDEBAR_OPTIONS: [RailMode; 2] = [RailMode::Expanded, RailMode::Icons];
/// On first, which is the order every On/Off picker in the mock-up uses
/// (`data-combo="wrap"`, `data-combo="attnchip"`) and the order a reader expects
/// when the affirmative is the default.
pub const FORMULA_OPTIONS: [bool; 2] = [true, false];

/// The label a theme wears in the picker, matching the mock-up's own casing.
fn theme_label(theme: ThemeModeV1) -> &'static str {
    match theme {
        ThemeModeV1::System => "System",
        ThemeModeV1::Light => "Light",
        ThemeModeV1::Dark => "Dark",
    }
}

fn cursor_label(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::Bar => "Bar",
        CursorStyle::Block => "Block",
        CursorStyle::Underline => "Underline",
    }
}

fn tab_layout_label(layout: TabLayoutMode) -> &'static str {
    match layout {
        TabLayoutMode::Horizontal => "Horizontal",
        TabLayoutMode::Vertical => "Vertical",
    }
}

/// The mock-up's own word for both states of every On/Off picker it draws.
fn on_off_label(enabled: bool) -> &'static str {
    if enabled { "On" } else { "Off" }
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
        RailMode::Expanded => "Expanded",
        RailMode::Icons => "Icons",
    }
}

/// A headed run of rows in the dialog's `.content`.
///
/// The mock-up's settings panel is not a flat list — it is group labels with
/// their rows beneath them (`design/ui-mockup.html:2343, 2406, 2421, 2452`), and
/// the user's 2026-08-10 ruling took that literally: groups laid out one after
/// another, with a category rail down the side left for when the panel has grown
/// enough to need one.
///
/// Only the two groups this build has rows for are named. A group with nothing in
/// it draws no heading and costs no height, which falls out of deriving the
/// headings from the row list rather than declaring them beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsGroup {
    Appearance,
    RenderedBlocks,
    /// Last, which is the mock-up's own order: `Appearance` (2355), `Terminal`
    /// (2418), `Rendered blocks` (2433), `Startup` (2464). This build has no
    /// Terminal group, and a group with no rows draws no heading — so the two it
    /// does have keep their places and this one arrives under them.
    Startup,
}

impl SettingsGroup {
    /// The heading as it is drawn.
    ///
    /// Upper-cased at the source, as `"APPEARANCE"` always was: the chrome text
    /// path has no `text-transform`, and the mock-up's own words are
    /// "Appearance" (2355), "Rendered blocks" (2433) and "Startup" (2464).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "APPEARANCE",
            Self::RenderedBlocks => "RENDERED BLOCKS",
            Self::Startup => "STARTUP",
        }
    }
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
    /// Mock-up 2464-2474, the Startup group's only row — and the only picker in
    /// this dialog whose items carry a mark (7645-7648).
    ///
    /// **The one row whose options can be unavailable.** Every other picker here
    /// offers choices this program can always honour; this one offers four
    /// shells, and whether a machine has Git Bash is not the product's to decide.
    /// A greyed item is the same sentence the `˅` menu's greyed row speaks, in
    /// the same words, because it is the same fact.
    DefaultProfile,
}

impl SettingsRow {
    /// Which heading this row is filed under.
    ///
    /// Stated once, per row, and read by the layout — the headings the dialog
    /// draws are derived by walking [`visible_rows`] and noticing where this
    /// answer changes, so there is no second list of groups to keep in step and
    /// no way to show a heading over rows that do not belong to it.
    #[must_use]
    pub fn group(self) -> SettingsGroup {
        match self {
            Self::Theme | Self::Cursor | Self::TabLayout | Self::Sidebar => {
                SettingsGroup::Appearance
            }
            // The mock-up files what typesetting does to a block under "Rendered
            // blocks" (2433), beside that group's own Maximum height row.
            Self::Formulas | Self::InlineFormulas => SettingsGroup::RenderedBlocks,
            Self::DefaultProfile => SettingsGroup::Startup,
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Cursor => "Cursor",
            Self::Formulas => "Display formulas",
            Self::InlineFormulas => "Inline formulas",
            // Mock-up 2360.
            Self::TabLayout => "Tab layout",
            // Mock-up 2374.
            Self::Sidebar => "Sidebar",
            // Mock-up 2467.
            Self::DefaultProfile => "Default profile",
        }
    }

    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            // The mock-up's own line names a third option this build does not
            // have; a description that promises what the picker cannot do is a
            // lie in the one place the user goes to find out what it does.
            Self::Theme => "Light or dark",
            Self::Cursor => "Focused cursor shape",
            // What Off does and, just as much, what it does not do: the line has
            // to say "source" or a reader will expect the formula to vanish.
            Self::Formulas => "Typeset $$…$$ blocks; off shows the LaTeX source",
            // Says "in command output" because that limit is the feature, not a
            // caveat about it: a `$…$` on the prompt or input line is never
            // typeset, and a user who reads only this line should not go away
            // expecting one to be.
            Self::InlineFormulas => "Typeset $…$ in command output; off shows the source",
            // Mock-up 2361.
            Self::TabLayout => "Choose where tabs appear in the window",
            // Mock-up 2375.
            Self::Sidebar => "How the vertical tab sidebar rests",
            // Mock-up 2468, word for word. It is also the *scope* of the setting
            // and the reason `profiles::index_of_id` does not read it: a tab and
            // a launch are the two things it answers for, and a pane coming back
            // off disk is neither.
            Self::DefaultProfile => "What opens on a new tab, and when BetterTerminal starts",
        }
    }

    /// How many items this row's picker holds.
    #[must_use]
    pub fn option_count(self) -> usize {
        match self {
            Self::Theme => THEME_OPTIONS.len(),
            Self::Cursor => CURSOR_OPTIONS.len(),
            Self::Formulas | Self::InlineFormulas => FORMULA_OPTIONS.len(),
            Self::TabLayout => TAB_LAYOUT_OPTIONS.len(),
            Self::Sidebar => SIDEBAR_OPTIONS.len(),
            // The picker is built from the same list the `˅` menu is built from
            // (mock-up 7645: "the default-profile picker is built from the same
            // list the ⌄ menu uses"). Not a copy of it — the same table — so a
            // fifth profile appears in both surfaces or in neither.
            Self::DefaultProfile => profiles::PROFILES.len(),
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
            Self::Formulas | Self::InlineFormulas => {
                FORMULA_OPTIONS.get(index).copied().map(on_off_label)
            }
            Self::TabLayout => TAB_LAYOUT_OPTIONS.get(index).copied().map(tab_layout_label),
            Self::Sidebar => SIDEBAR_OPTIONS.get(index).copied().map(sidebar_label),
            Self::DefaultProfile => {
                (index < profiles::PROFILES.len()).then(|| profiles::title(index))
            }
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
            Self::TabLayout => TAB_LAYOUT_OPTIONS
                .iter()
                .position(|it| *it == values.tab_layout),
            Self::Sidebar => SIDEBAR_OPTIONS.iter().position(|it| *it == values.sidebar),
            // The *resolved* default, which is why the caller hands over an index
            // rather than the stored id. **Mock-up bug not copied** (2471): its
            // combo button is born with the literal text `PowerShell` and only
            // ever updates when the user picks something, so a default that was
            // not index 0 showed stale words until touched. Reading state is the
            // whole of the fix, and it is free here because there is no second
            // place holding the button's caption.
            Self::DefaultProfile => Some(values.default_profile),
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
/// Rows of one group must stay contiguous here: the headings are derived from
/// where [`SettingsRow::group`] changes as this list is walked, so a row filed
/// out of order would head its group twice.
#[must_use]
pub fn visible_rows(tab_layout: TabLayoutMode) -> Vec<SettingsRow> {
    let mut rows = vec![
        SettingsRow::Theme,
        SettingsRow::Cursor,
        SettingsRow::TabLayout,
    ];
    if tab_layout == TabLayoutMode::Vertical {
        rows.push(SettingsRow::Sidebar);
    }
    rows.push(SettingsRow::Formulas);
    rows.push(SettingsRow::InlineFormulas);
    rows.push(SettingsRow::DefaultProfile);
    rows
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
    /// Which profiles this machine can start, in table order.
    ///
    /// An array rather than a borrowed `ProfilePrograms`, so this type stays
    /// `Copy` and `build`/`hit` can keep taking it by value. Four bools is the
    /// whole of what those two need to know about the filesystem.
    pub profile_available: [bool; profiles::PROFILES.len()],
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
            default_profile: profiles::FALLBACK_PROFILE,
            // A fully equipped machine, so a geometry test is not quietly also a
            // test of what is installed on the one running it.
            profile_available: [true; profiles::PROFILES.len()],
        }
    }
}

/// Whether the dialog is up, and what is open inside it.
///
/// App state and nothing else: it is not a seat, so the solver never sees it;
/// it is not an intent, so the session file never sees it. A dialog that
/// survived a restart would be a window that opens with a question on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettingsPanel {
    open: bool,
    /// Which row's picker is open. Nested state, because Esc unwinds one layer
    /// per press (§7.1.5) and "the menu is open" is the top layer.
    menu: Option<SettingsRow>,
    hover: Option<SettingsTarget>,
}

impl SettingsPanel {
    pub fn is_open(self) -> bool {
        self.open
    }

    pub fn menu(self) -> Option<SettingsRow> {
        self.menu
    }

    /// The gear: open when shut, shut when open. Closing takes the menu with it.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.menu = None;
        self.hover = None;
    }

    /// Close the top-most open layer and report whether there was one — the Esc
    /// route of §7.1.5, which unwinds exactly one layer per press.
    pub fn close_one_layer(&mut self) -> bool {
        if self.menu.is_some() {
            self.menu = None;
            self.hover = None;
            return true;
        }
        if self.open {
            self.open = false;
            self.hover = None;
            return true;
        }
        false
    }

    /// Shut everything, whatever was open.
    pub fn close(&mut self) {
        self.open = false;
        self.menu = None;
        self.hover = None;
    }

    /// Shut whichever picker is open, leaving the dialog up — what choosing an
    /// item does, and the only direction the runtime ever asks for.
    pub fn close_menu(&mut self) {
        self.menu = None;
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

    pub fn hover(self) -> Option<SettingsTarget> {
        self.hover
    }
}

/// Something in the overlay the pointer can be over.
///
/// There is no `None`: while the dialog is up every point in the window is one
/// of these, which is the whole of what "modal" means here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTarget {
    /// The dimmed world behind the dialog. A press here closes.
    Scrim,
    /// The dialog itself, away from any control. A press here does nothing, and
    /// in particular does not close — the mock-up closes only on the scrim.
    Panel,
    Close,
    /// A row's picker button.
    Combo(SettingsRow),
    /// The open menu's own body, between or around its items.
    Menu(SettingsRow),
    /// One item of a row's open picker, by its index in that row's options.
    Choice(SettingsRow, usize),
}

/// One row's three boxes, and which row they belong to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowLayout {
    pub row: SettingsRow,
    pub title: [f32; 4],
    pub desc: [f32; 4],
    pub combo: [f32; 4],
}

/// One heading's line box, and which group it names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupLayout {
    pub group: SettingsGroup,
    pub label: [f32; 4],
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
    /// The headings the dialog is drawing, top to bottom. Derived from `rows`,
    /// never declared beside it.
    groups: Vec<GroupLayout>,
    /// The rows the dialog is holding, top to bottom — the list
    /// [`visible_rows`] handed in, given boxes.
    rows: Vec<RowLayout>,
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
fn clipped(rect: [f32; 4], clip: [f32; 4]) -> Option<[f32; 4]> {
    let out = [
        rect[0].max(clip[0]),
        rect[1].max(clip[1]),
        rect[2].min(clip[2]),
        rect[3].min(clip[3]),
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

/// Where every part of the dialog lands in a window this size, or `None` when
/// the window cannot host it.
///
/// `None` is a real answer, not a failure: `max-height: calc(100% - 72px)` can
/// go to nothing, and a scrim over a dialog that is not there would be a window
/// nobody can use. The runtime treats it as "not open", so no input is trapped
/// behind an invisible modal.
#[must_use]
pub fn layout_for_menu(
    surface_width: f32,
    surface_height: f32,
    scale: f32,
    menu_kind: Option<SettingsRow>,
    rows: &[SettingsRow],
    widest_option: f32,
) -> Option<SettingsLayout> {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let width = px(DIALOG_MAX_WIDTH_LOGICAL_PX)
        .min(surface_width * DIALOG_WIDTH_RATIO)
        .round();
    let top = px(DIALOG_TOP_LOGICAL_PX).round();
    let available = (surface_height - top - px(DIALOG_BOTTOM_LOGICAL_PX)).round();
    // `.row .text` is two stacked line boxes; the row is as tall as the taller
    // of that column and the control beside it, which is what `align-items:
    // center` on a flex row resolves to.
    let text_height =
        px(ROW_TITLE_LINE_LOGICAL_PX + ROW_DESC_MARGIN_TOP_LOGICAL_PX + ROW_DESC_LINE_LOGICAL_PX);
    let row_height =
        2.0 * px(ROW_PADDING_Y_LOGICAL_PX) + text_height.max(px(COMBO_HEIGHT_LOGICAL_PX));
    // The content is a stack of headings and rows in one order, so its height is
    // that same stack measured — not a row count plus a remembered number of
    // headings. `heading_advance` answers for both the height here and the
    // placement below, which is what keeps a row from being drawn one heading's
    // worth away from where the dialog made room for it.
    let heading_advance = |first: bool| {
        px(if first {
            GROUP_LABEL_MARGIN_TOP_LOGICAL_PX
        } else {
            GROUP_LABEL_MARGIN_TOP_LATER_LOGICAL_PX
        }) + px(GROUP_LABEL_LINE_LOGICAL_PX)
            + px(GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX)
    };
    let mut stack_height = 0.0_f32;
    let mut previous_group: Option<SettingsGroup> = None;
    for row in rows {
        let group = row.group();
        if previous_group != Some(group) {
            stack_height += heading_advance(previous_group.is_none());
            previous_group = Some(group);
        }
        stack_height += row_height;
    }
    let content_height =
        px(CONTENT_PADDING_TOP_LOGICAL_PX) + stack_height + px(CONTENT_PADDING_BOTTOM_LOGICAL_PX);
    let header = px(HEADER_HEIGHT_LOGICAL_PX);
    let height = (2.0 * border + header + content_height)
        .min(available)
        .round();
    // Below the header plus its own two borders there is no dialog left to draw,
    // only a lid — and a lid with no body is not the design's dialog.
    if width < px(COMBO_MIN_WIDTH_LOGICAL_PX) || height <= 2.0 * border + header {
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
    let content = [
        inner[0],
        inner[1] + header,
        inner[2],
        inner[3].max(inner[1] + header),
    ];
    let text_left = content[0] + px(CONTENT_PADDING_X_LOGICAL_PX);
    let text_right = content[2] - px(CONTENT_PADDING_X_LOGICAL_PX);
    let row_content_height = text_height.max(px(COMBO_HEIGHT_LOGICAL_PX));
    let row_left = text_left + px(ROW_PADDING_X_LOGICAL_PX);
    let row_right = text_right - px(ROW_PADDING_X_LOGICAL_PX);
    let combo_width = px(COMBO_MIN_WIDTH_LOGICAL_PX);
    let combo_height = px(COMBO_HEIGHT_LOGICAL_PX);
    // One walk down the same stack the height was measured from. A heading is
    // emitted wherever the group changes and everything after it moves down by
    // exactly what the heading took, so the boxes drawn, the boxes hit-tested and
    // the height reserved are three readings of one derivation rather than three
    // rules that have to be kept in agreement.
    let mut cursor = content[1] + px(CONTENT_PADDING_TOP_LOGICAL_PX);
    let mut placed_groups: Vec<GroupLayout> = Vec::new();
    let mut placed_rows: Vec<RowLayout> = Vec::with_capacity(rows.len());
    let mut previous_group: Option<SettingsGroup> = None;
    for row in rows {
        let group = row.group();
        if previous_group != Some(group) {
            cursor += px(if previous_group.is_none() {
                GROUP_LABEL_MARGIN_TOP_LOGICAL_PX
            } else {
                GROUP_LABEL_MARGIN_TOP_LATER_LOGICAL_PX
            });
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
                title,
                desc,
                combo,
            }
        });
    }
    // A picker hangs off a row's button, so a picker named for a row the dialog
    // is not holding has nothing to hang from and is not open. That is not a
    // guard against the impossible: switching Tab layout to Horizontal takes the
    // Sidebar row out from under its own open menu.
    let active = menu_kind.and_then(|row| {
        placed_rows
            .iter()
            .find(|placed| placed.row == row)
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
        groups: placed_groups,
        rows: placed_rows,
        menu,
        items,
        menu_kind: active.map(|(row, _)| row),
    })
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
    for placed in &layout.rows {
        if contains(placed.combo, x, y) {
            return SettingsTarget::Combo(placed.row);
        }
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
#[must_use]
pub fn build(
    layout: &SettingsLayout,
    hover: Option<SettingsTarget>,
    values: SettingsValues,
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
        text: "Settings".to_owned(),
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

    // Everything below the header is clipped to the content box, which is what
    // `max-height` plus `overflow-y` leaves when the window is too short.
    let clip = layout.content;
    for headed in &layout.groups {
        if let Some(rect) = clipped(headed.label, clip) {
            labels.push(ChromeLabel {
                text: headed.group.label().to_owned(),
                rect,
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
    }
    for placed in &layout.rows {
        if let Some(rect) = clipped(placed.title, clip) {
            labels.push(ChromeLabel {
                text: placed.row.title().to_owned(),
                rect,
                font_size_px: px(ROW_TITLE_FONT_LOGICAL_PX),
                color: palette.dialog_title_text,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            });
        }
        if let Some(rect) = clipped(placed.desc, clip) {
            labels.push(ChromeLabel {
                text: placed.row.description().to_owned(),
                rect,
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
    }
    // The buttons after every row's text, so a row's own control cannot be
    // covered by the *fill* of a later row's — the same channel ordering the
    // popup's layer exists for, one scale down.
    for placed in &layout.rows {
        let Some(rect) = clipped(placed.combo, clip) else {
            continue;
        };
        let value = placed
            .row
            .selected_index(values)
            .and_then(|index| placed.row.option_label(index))
            .unwrap_or_default();
        push_combo(
            &mut quads,
            &mut labels,
            rect,
            hover == Some(SettingsTarget::Combo(placed.row)),
            value,
            scale,
            border,
            palette,
        );
    }

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
            let is_hovered = enabled && hover == Some(SettingsTarget::Choice(row, index));
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

#[allow(clippy::too_many_arguments)]
fn push_combo(
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    rect: [f32; 4],
    hovered: bool,
    value: &str,
    scale: f32,
    border: f32,
    palette: bt_render::ChromePalette,
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
    labels.push(ChromeLabel {
        text: value.to_owned(),
        rect: [
            rect[0] + border + px(COMBO_PADDING_LEFT_LOGICAL_PX),
            rect[1],
            rect[2] - border - px(COMBO_PADDING_RIGHT_LOGICAL_PX) - chevron_column,
            rect[3],
        ],
        font_size_px: px(COMBO_FONT_LOGICAL_PX),
        color: palette.dialog_title_text,
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
    // The wider, fainter ring first, so the two compose into a falloff rather
    // than a band.
    for (extent, alpha) in [
        (spread, shadow_outer_alpha),
        (spread / 2.0, shadow_inner_alpha),
    ] {
        quads.extend(rounded_overlay_halo(frame, radius, extent, shadow, alpha));
    }
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
    const SURFACE: (f32, f32) = (1280.0, 800.0);

    /// `.row`'s own height at scale 1: `2 * 11` of padding around the taller of
    /// the two-line text column (16.5 + 1 + 14.5) and the 27.5 control.
    const ROW_HEIGHT: f32 = 54.0;

    /// A heading past the first costs `16 + 13 + 2`; the first costs `10 + 13 + 2`
    /// and is already inside the 103 below.
    const LATER_HEADING_HEIGHT: f32 = 31.0;

    /// The dialog's height at scale 1 for a content box holding `rows` rows under
    /// `groups` headings: two hairlines, the header's `16 + 30 + 10`, and
    /// `2 + 10 + 13 + 2 + rows * 54 + 18` of content, plus 31 for every heading
    /// after the first.
    fn dialog_height(rows: usize, groups: usize) -> f32 {
        103.0 + ROW_HEIGHT * rows as f32 + LATER_HEADING_HEIGHT * (groups.saturating_sub(1)) as f32
    }

    /// How many headings a list of rows draws — the same derivation the layout
    /// makes, so a pin cannot count groups a different way than the dialog does.
    fn group_count(rows: &[SettingsRow]) -> usize {
        let mut groups: Vec<SettingsGroup> = Vec::new();
        for row in rows {
            if groups.last() != Some(&row.group()) {
                groups.push(row.group());
            }
        }
        groups.len()
    }

    /// The rows a dialog holds with the tabs across the top — the state the app
    /// opens in, and what every claim below is stated against unless it says
    /// otherwise.
    fn flat_rows() -> Vec<SettingsRow> {
        visible_rows(TabLayoutMode::Horizontal)
    }

    /// A representative reading for every row, for the tests that are about
    /// geometry rather than about which value is shown.
    fn values() -> SettingsValues {
        SettingsValues::sample()
    }

    fn open(scale: f32, menu_open: bool) -> SettingsLayout {
        layout_for_menu(
            (SURFACE.0 * scale).round(),
            (SURFACE.1 * scale).round(),
            scale,
            menu_open.then_some(SettingsRow::Theme),
            &flat_rows(),
            0.0,
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
        layout_for_menu(
            SURFACE.0 * scale,
            SURFACE.1 * scale,
            scale,
            menu,
            &visible_rows(tab_layout),
            widest_option,
        )
        .expect("the settings dialog fits")
    }

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
            dialog_height(2, 1),
            211.0,
            "the mock-up's own measurement: two rows under one heading, which is \
             what this dialog first shipped with"
        );
        let placed = open(1.0, false);
        assert_eq!(width(placed.frame), 480.0, "min(480px, 92%) at 1280 wide");
        assert_eq!(placed.frame[1], 54.0, "margin-top: 54px");
        assert_eq!(
            placed.frame[0],
            (SURFACE.0 - 480.0) / 2.0,
            "margin-left/right: auto"
        );
        assert_eq!(
            height(placed.frame),
            dialog_height(flat_rows().len(), group_count(&flat_rows())),
            "content decides the height, headings included"
        );

        // The 92% share takes over below 480/0.92 ~= 521.7 logical pixels.
        let narrow = layout_for_menu(480.0, 800.0, 1.0, None, &flat_rows(), 0.0)
            .expect("480 wide still hosts the dialog");
        assert_eq!(
            width(narrow.frame),
            (480.0_f32 * DIALOG_WIDTH_RATIO).round(),
            "under the cap the dialog takes 92% of the window"
        );
        assert_eq!(
            narrow.frame[0],
            ((480.0 - width(narrow.frame)) / 2.0).round()
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
            ROW_HEIGHT,
            "Cursor is the next identical row under Theme"
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
            near(480.0 * scale, width(placed.frame), "the dialog's width");
            near(
                dialog_height(flat_rows().len(), group_count(&flat_rows())) * scale,
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
                SettingsTarget::Combo(placed_row.row),
                "{:?}'s button must answer for its own row",
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

    #[test]
    fn theme_picker_orders_system_light_dark() {
        let labels: Vec<_> = THEME_OPTIONS.into_iter().map(theme_label).collect();
        assert_eq!(labels, ["System", "Light", "Dark"]);
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
            [ThemeModeV1::System, ThemeModeV1::Light, ThemeModeV1::Dark,]
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
        assert_eq!(
            cursor[1] - theme[1],
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
        let short = layout_for_menu(
            1280.0,
            200.0,
            1.0,
            Some(SettingsRow::Theme),
            &flat_rows(),
            0.0,
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

    /// PIN: a window that cannot host the dialog answers `None` rather than a
    /// squashed one — and the runtime reads `None` as "shut", so nothing is
    /// trapped behind a modal with nothing on it.
    #[test]
    fn a_window_too_small_to_host_the_dialog_says_so() {
        assert!(
            layout_for_menu(1280.0, 100.0, 1.0, None, &flat_rows(), 0.0).is_none(),
            "too short"
        );
        assert!(
            layout_for_menu(100.0, 800.0, 1.0, None, &flat_rows(), 0.0).is_none(),
            "too narrow"
        );
        assert!(
            layout_for_menu(1280.0, 800.0, 1.0, None, &flat_rows(), 0.0).is_some(),
            "a real window"
        );
    }

    /// PIN (Esc): one layer per press — the open menu first, then the dialog,
    /// then nothing, and "nothing" is reported so the key can fall through to
    /// whoever owns it next.
    #[test]
    fn escape_unwinds_one_layer_per_press() {
        let mut panel = SettingsPanel::default();
        assert!(!panel.close_one_layer(), "nothing is open yet");
        panel.toggle();
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
        panel.toggle();
        assert!(panel.is_open());
        panel.toggle_menu(SettingsRow::Theme);
        panel.toggle();
        assert!(!panel.is_open() && panel.menu().is_none());
        panel.toggle();
        assert!(panel.is_open() && panel.menu().is_none());
        panel.close();
        assert!(!panel.is_open());
    }

    /// Every fill the overlay draws, whatever layer it is on — the question
    /// "does the dialog paint this at all" is not a question about z-order.
    fn quads_of(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        values: SettingsValues,
    ) -> Vec<OverlayQuad> {
        build(placed, hover, values)
            .into_iter()
            .flat_map(|layer| layer.quads)
            .collect()
    }

    fn labels_of(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        values: SettingsValues,
    ) -> Vec<ChromeLabel> {
        build(placed, hover, values)
            .into_iter()
            .flat_map(|layer| layer.labels)
            .collect()
    }

    fn sprites_of(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        values: SettingsValues,
    ) -> Vec<ChromeSprite> {
        build(placed, hover, values)
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
            let layers = build(&placed, None, values());
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
        let covered = clipped(combo_of(&placed, SettingsRow::Cursor), menu)
            .expect("the Theme picker hangs over the Cursor row's control");
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
        let count = |hover, color| {
            quads_of(&placed, hover, values())
                .iter()
                .filter(|quad| quad.color == color)
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

    /// The dialog is a stack of headed groups, and every row sits under the
    /// heading that names it.
    ///
    /// The mock-up's `.content` is four `.group-label`s with their rows beneath
    /// (`design/ui-mockup.html:2343, 2406, 2421, 2452`); this build has the two
    /// the audit assigned. A row drawn under the wrong heading is a row filed
    /// under a word that does not describe it, which is the only thing a group is
    /// for.
    #[test]
    fn every_row_is_drawn_under_the_heading_that_names_it() {
        let placed = open_rows(1.0, None, TabLayoutMode::Vertical);
        let labels = labels_of(&placed, None, values());
        let heading_top = |text: &str| {
            labels
                .iter()
                .find(|label| label.text == text)
                .unwrap_or_else(|| panic!("{text} is headed"))
                .rect[1]
        };
        let appearance = heading_top("APPEARANCE");
        let rendered = heading_top("RENDERED BLOCKS");
        assert!(
            appearance < rendered,
            "Appearance is the first group in the mock-up's own order"
        );

        for row in [
            SettingsRow::Theme,
            SettingsRow::Cursor,
            SettingsRow::TabLayout,
            SettingsRow::Sidebar,
        ] {
            let top = placed.row(row).expect("row is shown").title[1];
            assert!(
                top > appearance && top < rendered,
                "{row:?} belongs to Appearance and must be drawn between the two \
                 headings"
            );
        }
        let formulas = placed
            .row(SettingsRow::Formulas)
            .expect("row is shown")
            .title[1];
        assert!(
            formulas > rendered,
            "Display formulas is what Rendered blocks names"
        );
    }

    /// A heading past the first stands further off what precedes it than a row
    /// does, which is the whole of what makes the stack read as groups.
    ///
    /// The mock-up says it twice: `.group-label { margin: 10px 0 2px }` for the
    /// first (`design/ui-mockup.html:2042`) and an inline `margin-top:16px` on
    /// every later one (2406, 2421, 2452).
    #[test]
    fn a_later_heading_stands_off_the_group_above_it() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let placed = open_rows(scale, None, TabLayoutMode::Vertical);
            let labels = labels_of(&placed, None, values());
            let heading = |text: &str| {
                labels
                    .iter()
                    .find(|label| label.text == text)
                    .unwrap_or_else(|| panic!("{text} is headed"))
                    .rect
            };
            let first = heading("APPEARANCE");
            let later = heading("RENDERED BLOCKS");

            // The first heading takes the base 10px off the content padding.
            let content_top = placed.content[1];
            assert!(
                (first[1]
                    - (content_top
                        + (CONTENT_PADDING_TOP_LOGICAL_PX + GROUP_LABEL_MARGIN_TOP_LOGICAL_PX)
                            * scale))
                    .abs()
                    < 0.5,
                "scale {scale}: the first heading keeps the stylesheet's 10px"
            );

            // The later heading takes 16px off the bottom of the group above it.
            let last_of_first_group = placed
                .row(SettingsRow::Sidebar)
                .expect("vertical tabs show the sidebar row")
                .desc[3];
            let gap = later[1] - last_of_first_group;
            assert!(
                gap > (GROUP_LABEL_MARGIN_TOP_LOGICAL_PX * scale),
                "scale {scale}: a later heading is pushed further than the base \
                 margin, saw {gap}"
            );
            assert!(
                (later[3] - later[1] - GROUP_LABEL_LINE_LOGICAL_PX * scale).abs() < 0.5,
                "scale {scale}: every heading is one line box tall"
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
    fn every_row_answers_the_pointer_where_its_own_group_put_it() {
        for tab_layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
            for scale in [1.0_f32, 1.25, 2.0] {
                let placed = open_rows(scale, None, tab_layout);
                for row in visible_rows(tab_layout) {
                    let combo = placed.row(row).expect("a visible row is placed").combo;
                    let centre = (
                        f64::from((combo[0] + combo[2]) / 2.0),
                        f64::from((combo[1] + combo[3]) / 2.0),
                    );
                    assert_eq!(
                        hit(&placed, values(), centre.0, centre.1),
                        SettingsTarget::Combo(row),
                        "{tab_layout:?} at {scale}: {row:?}'s picker must answer \
                         where it is drawn"
                    );
                }
            }
        }
    }

    /// The dialog is as tall as everything it is holding — both headings included.
    ///
    /// A height that counted one heading while the stack drew two would push the
    /// last group past the bottom of the content box and clip it away.
    #[test]
    fn the_dialog_makes_room_for_every_heading_it_draws() {
        let placed = open_rows(1.0, None, TabLayoutMode::Vertical);
        let content = placed.content;
        let last = placed
            .row(SettingsRow::Formulas)
            .expect("row is shown")
            .desc[3];
        assert!(
            last + CONTENT_PADDING_BOTTOM_LOGICAL_PX <= content[3] + 0.5,
            "the last row and the content's bottom padding both fit inside the \
             content box"
        );
    }

    /// Every group the dialog can show holds at least one row, and a group's rows
    /// are contiguous — the order in [`visible_rows`] is what the headings are
    /// derived from, so a row filed out of order would split its own group in two.
    #[test]
    fn each_groups_rows_stand_together_in_the_visible_order() {
        for tab_layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
            let rows = visible_rows(tab_layout);
            let mut seen: Vec<SettingsGroup> = Vec::new();
            for row in rows {
                let group = row.group();
                if seen.last() != Some(&group) {
                    assert!(
                        !seen.contains(&group),
                        "{tab_layout:?}: {group:?} is interrupted by another group"
                    );
                    seen.push(group);
                }
            }
            assert_eq!(
                seen,
                vec![
                    SettingsGroup::Appearance,
                    SettingsGroup::RenderedBlocks,
                    SettingsGroup::Startup,
                ],
                "{tab_layout:?}: every group is shown, in the mock-up's own order \
                 (2355, 2433, 2464 — its Terminal group at 2418 has no rows here)"
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
            SettingsRow::DefaultProfile.description(),
            "What opens on a new tab, and when BetterTerminal starts",
            "mock-up 2468, word for word"
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
        // The only one. Every other picker's items are words.
        for row in visible_rows(TabLayoutMode::Vertical) {
            if row == SettingsRow::DefaultProfile {
                continue;
            }
            for index in 0..row.option_count() {
                assert_eq!(row.option_mark(index), None, "{row:?} draws no marks");
            }
        }
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
            let placed = open_rows(1.0, None, TabLayoutMode::Horizontal);
            let combo = placed
                .row(SettingsRow::DefaultProfile)
                .expect("the Startup row is in the dialog")
                .combo;
            let caption = labels_of(&placed, None, values)
                .into_iter()
                .find(|label| label.rect[1] >= combo[1] && label.rect[3] <= combo[3])
                .expect("the closed combo shows its current value");
            assert_eq!(
                caption.text,
                profiles::PROFILES[chosen].title,
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
        let placed = layout_for_menu(
            520.0,
            800.0,
            1.0,
            Some(SettingsRow::Theme),
            &flat_rows(),
            4_000.0,
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
                SettingsRow::Cursor,
                SettingsRow::TabLayout,
                SettingsRow::Formulas,
                SettingsRow::InlineFormulas,
                SettingsRow::DefaultProfile
            ]
        );
        assert_eq!(
            visible_rows(TabLayoutMode::Vertical),
            [
                SettingsRow::Theme,
                SettingsRow::Cursor,
                SettingsRow::TabLayout,
                SettingsRow::Sidebar,
                SettingsRow::Formulas,
                SettingsRow::InlineFormulas,
                SettingsRow::DefaultProfile
            ],
            "Sidebar still lands directly under the row it depends on, the two \
             formula rows stay together as the whole of the second group, and \
             Startup's one row is last"
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
            SettingsRow::Sidebar.description(),
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
    /// evenly spaced say "these belong together", and the one larger step says
    /// "and these do not". Every row keeps the identical control either way — a
    /// heading separates rows, it does not restyle them.
    #[test]
    fn rows_stack_one_row_height_apart_within_a_group_and_one_heading_across_one() {
        for layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
            let placed = open_rows(1.0, None, layout);
            assert_eq!(placed.rows.len(), visible_rows(layout).len());
            for pair in placed.rows.windows(2) {
                let (above, below) = (&pair[0], &pair[1]);
                let expected = if above.row.group() == below.row.group() {
                    ROW_HEIGHT
                } else {
                    ROW_HEIGHT + LATER_HEADING_HEIGHT
                };
                assert_eq!(
                    below.combo[1] - above.combo[1],
                    expected,
                    "{:?} follows {:?}",
                    below.row,
                    above.row
                );
                assert_eq!(width(below.combo), width(above.combo));
                assert_eq!(height(below.combo), height(above.combo));
                assert_eq!(below.title[1] - above.title[1], expected);
                assert_eq!(below.desc[1] - above.desc[1], expected);
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

    /// A fresh panel is shut, with nothing open inside it and nothing hovered.
    #[test]
    fn a_fresh_panel_is_shut() {
        let panel = SettingsPanel::default();
        assert!(!panel.is_open());
        assert!(panel.menu().is_none());
        assert_eq!(panel.hover(), None);
    }
}
