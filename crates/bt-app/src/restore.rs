//! The restore prompt — "Reopen your other tabs?", the question a launch asks
//! once and never again.
//!
//! Spec authority is `design/ui-mockup.html`: the `.restore` block (lines
//! 1931-1969) for the surface, its list and its two buttons, the markup and its
//! two design notes at 2219-2233, and `openRestore` / `finishLaunch`
//! (7467-7521) for what a row says and what a press means. Every number below is
//! that stylesheet's own, and the ones that are line boxes rather than
//! declarations were measured in the mock-up's own renderer — a box model
//! derived from *our* font would drift the moment the font did, and the design
//! is the thing being reproduced.
//!
//! **It is not a modal, and that is the whole reason it is allowed to exist.**
//! The mock-up says so above the markup, and `docs/DESIGN.md` §7.1.4 says it
//! again with the word 非模态:
//!
//! > Not modal and it does not dim: your terminal is already open behind it and
//! > already usable. This is a prompt over a working app, not a gate in front of
//! > one.
//!
//! So this module draws **no scrim** and claims **no input it is not standing
//! on**: [`hit`] returns `Option`, exactly like [`crate::profiles`]'s popup
//! contract, and a press beside the dialog belongs to whatever is there. What it
//! borrows from [`crate::settings`] is craft and not contract — the same
//! [`crate::settings::push_float_window`] lift/hairline/face, the same `--win`
//! plane, the same 10px round every floating window shares. A prompt that
//! trapped the pointer would be the settings modal wearing this one's words.
//!
//! Nothing here is layout: the prompt is not a seat, takes no space from the
//! solver, and is never persisted — an unanswered question folds back into
//! `lastSession` (§7.1.4) rather than being written down as a piece of UI.

use std::path::Path;

use bt_render::{
    ChromeLabel, ChromeLabelWeight, FLOAT_WINDOW_BORDER_LOGICAL_PX, FLOAT_WINDOW_RADIUS_LOGICAL_PX,
    FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad, WINDOW_TAB_BADGE_FONT_LOGICAL_PX,
    WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX, WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX,
    WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX, WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX, chrome_palette,
    rounded_overlay_fill,
};

use crate::{
    marks::{ChromeMark, ChromeSprite, OverlayLayer},
    profiles::{PROFILES, index_of_id},
    seed::Seed,
    settings::push_float_window,
};

// ── `.restore` ─────────────────────────────────────────────────────────────
/// `.restore { width: min(400px, 92%) }` — the cap and the share.
///
/// The share is of the **window**, not of the tab strip: `.restore` is
/// `position: fixed`, and the mock-up's own comment above the rule says why —
/// "like the icon it replaces: both are things on the desktop, and neither
/// belongs to a window that does not exist yet".
const DIALOG_MAX_WIDTH_LOGICAL_PX: f32 = 400.0;
const DIALOG_WIDTH_RATIO: f32 = 0.92;
/// `padding: 20px 22px 16px`.
const DIALOG_PADDING_TOP_LOGICAL_PX: f32 = 20.0;
const DIALOG_PADDING_X_LOGICAL_PX: f32 = 22.0;
const DIALOG_PADDING_BOTTOM_LOGICAL_PX: f32 = 16.0;

// ── `.restore h1` ──────────────────────────────────────────────────────────
/// `font-size: 15px; font-weight: 600`.
const TITLE_FONT_LOGICAL_PX: f32 = 15.0;
/// The 15px line box, measured in the mock-up's own renderer.
const TITLE_LINE_LOGICAL_PX: f32 = 18.0;
/// `margin: 0 0 5px`.
const TITLE_MARGIN_BOTTOM_LOGICAL_PX: f32 = 5.0;
pub const TITLE_TEXT: &str = "Reopen your other tabs?";

// ── `.restore .sub` ────────────────────────────────────────────────────────
/// `font-size: 12.5px`.
pub const SUB_FONT_LOGICAL_PX: f32 = 12.5;
/// `line-height: 1.5` — declared, so it is arithmetic rather than a measurement:
/// 12.5 × 1.5.
const SUB_LINE_HEIGHT_RATIO: f32 = 1.5;
const SUB_LINE_LOGICAL_PX: f32 = SUB_FONT_LOGICAL_PX * SUB_LINE_HEIGHT_RATIO;
/// `margin: 0 0 14px`.
const SUB_MARGIN_BOTTOM_LOGICAL_PX: f32 = 14.0;
/// The paragraph, word for word (mock-up line 2227).
///
/// The sentence is the design: it promises the *folders* and refuses the
/// *output* in the same breath, which is 「不存输出历史」 stated as a courtesy
/// instead of an apology. It wraps, so it is drawn as lines — see [`wrap`].
pub const SUB_TEXT: &str = "These were open when you last closed BetterTerminal. \
They come back in the folders you left them, as new shells — the output is not \
ours to keep.";

// ── `.restore-list` ────────────────────────────────────────────────────────
/// `margin: 0 0 16px`.
const LIST_MARGIN_BOTTOM_LOGICAL_PX: f32 = 16.0;
/// `gap: 1px` — a hairline of air, not a divider.
const LIST_GAP_LOGICAL_PX: f32 = 1.0;

// ── `.restore-list li` ─────────────────────────────────────────────────────
/// `padding: 6px 8px`.
const ROW_PADDING_Y_LOGICAL_PX: f32 = 6.0;
const ROW_PADDING_X_LOGICAL_PX: f32 = 8.0;
/// That padding around the tallest thing in the row, which is the 13px label's
/// own line box: 6 + 15.5 + 6.
///
/// The 14px mark and the 15px badge are both shorter than that, so neither
/// decides the row — which is why this is arithmetic and not a max at runtime.
const ROW_HEIGHT_LOGICAL_PX: f32 = 2.0 * ROW_PADDING_Y_LOGICAL_PX + ROW_LINE_LOGICAL_PX;
/// `border-radius: 6px`.
const ROW_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `gap: 8px` — between every pair of things on the row.
const ROW_GAP_LOGICAL_PX: f32 = 8.0;
/// `font-size: 13px`.
pub const ROW_FONT_LOGICAL_PX: f32 = 13.0;
/// The 13px line box, measured in the mock-up.
const ROW_LINE_LOGICAL_PX: f32 = 15.5;
/// `.ticon { width: 15px; height: 15px }` — the column the mark sits in, which
/// is the tab strip's own icon slot and is **not** the mark.
const ROW_MARK_COLUMN_LOGICAL_PX: f32 = 15.0;
/// `.restore-list .pmark { width: 14px; height: 14px }` — R196. One pixel
/// narrower than its column, and centred in it, exactly as the flex box centres
/// it.
const ROW_MARK_LOGICAL_PX: f32 = 14.0;
/// `.restore-list .rcwd { font-size: 11.5px }`.
pub const ROW_CWD_FONT_LOGICAL_PX: f32 = 11.5;
/// The 11.5px line box, measured in the mock-up.
const ROW_CWD_LINE_LOGICAL_PX: f32 = 14.0;

// ── `.restore-actions`, `.btn` ─────────────────────────────────────────────
/// `justify-content: flex-end; gap: 8px`.
const ACTIONS_GAP_LOGICAL_PX: f32 = 8.0;
/// `.btn { padding: 6px 14px }`.
const BUTTON_PADDING_X_LOGICAL_PX: f32 = 14.0;
const BUTTON_PADDING_Y_LOGICAL_PX: f32 = 6.0;
/// `.btn { border-radius: 6px }`.
const BUTTON_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `.btn { font-size: 13px }`, over the same 15.5 line box the rows use.
pub const BUTTON_FONT_LOGICAL_PX: f32 = 13.0;
const BUTTON_LINE_LOGICAL_PX: f32 = 15.5;
/// `.btn.primary:hover { filter: brightness(1.07) }`.
///
/// A plain multiplier over the sRGB bytes, which is what a browser's
/// `brightness()` is. There is no compositing question to get wrong here: the
/// accent is opaque, so the brightened accent is opaque too.
const BUTTON_PRIMARY_HOVER_BRIGHTNESS: f32 = 1.07;
/// `.btn.primary { color: #fff }` — a literal in the design rather than one of
/// its variables, and opaque, so it lands as written.
const BUTTON_PRIMARY_INK: [u8; 3] = [0xff, 0xff, 0xff];
pub const DECLINE_TEXT: &str = "No thanks";
pub const RESTORE_TEXT: &str = "Restore";

/// What a press on the prompt asks the process to do.
///
/// Nothing here says what *happens* to the tabs: appending the revived ones,
/// dropping the placeholder shell and landing on the tab that was active at
/// shutdown are `finishLaunch`'s job (mock-up 7492-7519) and the runtime's. This
/// enum carries the answer and stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreAnswer {
    /// "Restore" — bring the unpinned tabs back.
    Restore,
    /// "No thanks" — keep whatever you are already looking at.
    ///
    /// The mock-up rules the wording as well as the behaviour: it says "No
    /// thanks" rather than "Start fresh" because *fresh is already on the
    /// screen* (7495-7496). Declining takes nothing away.
    NoThanks,
}

/// The button the prompt opens focused (`$("btn-restore").focus()`, mock-up
/// 7490), and therefore what Enter answers.
pub const FOCUSED_ANSWER: RestoreAnswer = RestoreAnswer::Restore;

/// Something on the prompt the pointer can be over.
///
/// There is deliberately no target for a *row*: the list is what the question is
/// about, not a set of controls, and the mock-up hangs no handler on an `li`.
/// Picking tabs one at a time is a different question, and it is not this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreTarget {
    /// The dialog itself, away from either button. A press here does nothing —
    /// and in particular does not answer, because a prompt that took silence
    /// for an answer would be the thing §7.1.4 forbids.
    Panel,
    Decline,
    Restore,
}

/// The answer a press on `target` gives, if it gives one at all.
///
/// A named function rather than a `match` at the call site, so the mapping from
/// "what the pointer hit" to "what the launch was told" is one thing that can be
/// stated, and pinned, without a live window.
#[must_use]
pub fn answer(target: RestoreTarget) -> Option<RestoreAnswer> {
    match target {
        RestoreTarget::Restore => Some(RestoreAnswer::Restore),
        RestoreTarget::Decline => Some(RestoreAnswer::NoThanks),
        RestoreTarget::Panel => None,
    }
}

/// Whether the prompt is up, and which control the pointer is on.
///
/// App state and nothing else: not a seat, so the solver never sees it; not an
/// intent, so the session file never sees it. What *is* persisted is the
/// question — the tabs it is asking about stay in `lastSession` until they are
/// answered for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestorePrompt {
    open: bool,
    hover: Option<RestoreTarget>,
}

impl RestorePrompt {
    pub fn is_open(self) -> bool {
        self.open
    }

    /// Ask the question. Only a launch does this — the prompt has no control
    /// that reopens it, because it has nothing to ask a second time.
    pub fn open(&mut self) {
        self.open = true;
        self.hover = None;
    }

    /// Put it away, and report whether there was anything to put away.
    pub fn close(&mut self) -> bool {
        let was_open = self.open;
        self.open = false;
        self.hover = None;
        was_open
    }

    /// **Esc is not an answer, so the prompt does not take it.**
    ///
    /// Always `false`, and that is the ruling rather than a stub. `docs/DESIGN.md`
    /// §7.1.4 requires that an unanswered prompt fold back into `lastSession`
    /// and never be lost; the settings modal's Esc route (§7.1.5, "unwind one
    /// layer per press") exists because a modal *has* to offer a way out of the
    /// trap it sets. This one sets no trap. The terminal behind it is already
    /// working, so a question you are ignoring costs you nothing, and Esc
    /// belongs to the thing you are actually typing into.
    ///
    /// Making Esc mean "No thanks" would be worse than useless: it would spend
    /// your other tabs on a keystroke you pressed at a shell.
    pub fn consumes_escape(self) -> bool {
        false
    }

    /// Returns whether the hover changed, so a caller can skip a repaint.
    pub fn set_hover(&mut self, hover: Option<RestoreTarget>) -> bool {
        let hover = if self.open { hover } else { None };
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<RestoreTarget> {
        self.hover
    }
}

/// One line of the list: a tab that was open when the window last closed.
///
/// The three `*_text_width` fields are **measured**, in physical pixels, by the
/// caller that owns a font (`Renderer::measure_chrome_text`), for the same
/// reason `seats::TabContent::badge_text_width` is: this module lays out a flex
/// row, a flex row packs its children against each other, and where the cwd ends
/// is a fact about the glyphs in the label before it. [`RestoreRow::from_seed`]
/// builds the row's text; the caller measures it and fills these in.
#[derive(Clone, Debug, PartialEq)]
pub struct RestoreRow {
    /// The tab's profile mark — a folder for a files locus, the profile's own
    /// mark for a terminal. "A profile's icon is its mark" (mock-up 7482).
    pub mark: ChromeMark,
    /// What it will be **called when it comes back** — not what it was called
    /// when it left. "The program's title left with the program; your name did
    /// not" (mock-up 7480-7481).
    pub label: String,
    pub cwd: String,
    /// How many panes the tab held. One draws no badge at all.
    pub pane_count: usize,
    /// [`Self::label`] at [`ROW_FONT_LOGICAL_PX`] × scale.
    pub label_text_width: f32,
    /// [`Self::cwd`] at [`ROW_CWD_FONT_LOGICAL_PX`] × scale. The cwd is the one
    /// thing on the row that may be cut short, so this is its *natural* width
    /// and the layout decides how much of it there is room for.
    pub cwd_text_width: f32,
    /// [`Self::badge_text`] at the tab badge's own font × scale. Ignored when
    /// the tab held one pane.
    pub badge_text_width: f32,
}

impl RestoreRow {
    /// The row a seed describes, with its text filled in and its widths left for
    /// the caller to measure.
    ///
    /// The naming rule is the mock-up's own (7484-7485) and it is deliberately
    /// **not** the tab strip's: a live tab may be wearing a program's `OSC 2`
    /// title, and that title is about a process that is gone. So a terminal is
    /// named by your name for it or by the folder it stood in, which is exactly
    /// [`crate::display_title`] with no program title to offer, and a files
    /// locus is named by its folder.
    #[must_use]
    pub fn from_seed(seed: &Seed, pane_count: usize) -> Self {
        let (mark, label, cwd) = match seed {
            Seed::Term {
                profile_id,
                cwd,
                manual_name,
            } => (
                PROFILES[index_of_id(profile_id)].mark,
                crate::display_title(manual_name.as_deref(), None, Some(Path::new(cwd))),
                cwd.clone(),
            ),
            Seed::Files { root } => (
                ChromeMark::Folder,
                crate::cwd_leaf(Path::new(root)).unwrap_or_else(|| root.clone()),
                root.clone(),
            ),
        };
        Self {
            mark,
            label,
            cwd,
            pane_count,
            label_text_width: 0.0,
            cwd_text_width: 0.0,
            badge_text_width: 0.0,
        }
    }

    /// The pane-count badge's text, or `None` for a tab that wears no badge.
    ///
    /// "How many panes this tab holds — only shown once it holds more than one"
    /// (mock-up 291), and `leaves.length > 1` at 7487.
    #[must_use]
    pub fn badge_text(&self) -> Option<String> {
        (self.pane_count > 1).then(|| self.pane_count.to_string())
    }
}

/// Everything the prompt draws that had to be measured with a real font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RestoreContent {
    /// The tabs it is asking about — the ones you did **not** pin. "The pinned
    /// ones are already open behind this — you answered for them when you
    /// pinned them" (mock-up 2222-2224).
    pub rows: Vec<RestoreRow>,
    /// [`SUB_TEXT`], already broken to lines that fit [`content_width`]. See
    /// [`wrap`].
    pub sub_lines: Vec<String>,
    /// [`DECLINE_TEXT`] at [`BUTTON_FONT_LOGICAL_PX`] × scale.
    pub decline_text_width: f32,
    /// [`RESTORE_TEXT`] at the same size.
    pub restore_text_width: f32,
}

/// The width, in physical pixels, that text inside the dialog has to fit into.
///
/// Public because the paragraph has to be wrapped *before* the layout can know
/// how tall the dialog is, and wrapping needs a font. The caller measures
/// against this and hands the lines back in [`RestoreContent::sub_lines`].
#[must_use]
pub fn content_width(surface_width: f32, scale: f32) -> f32 {
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    dialog_width(surface_width, scale) - 2.0 * (border + DIALOG_PADDING_X_LOGICAL_PX * scale)
}

/// `width: min(400px, 92%)`, snapped to the pixel grid.
fn dialog_width(surface_width: f32, scale: f32) -> f32 {
    (DIALOG_MAX_WIDTH_LOGICAL_PX * scale)
        .min(surface_width * DIALOG_WIDTH_RATIO)
        .round()
}

/// Break `text` into lines no wider than `max_width`, measuring with the
/// caller's own font.
///
/// Greedy over whitespace, which is what CSS `overflow-wrap: normal` does: a
/// word that cannot fit alone gets a line of its own and overruns it, because
/// the alternative — breaking inside a word — is a thing the design never asked
/// for and would put half a path on each of two lines.
#[must_use]
pub fn wrap(text: &str, max_width: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
            continue;
        }
        let candidate = format!("{line} {word}");
        if measure(&candidate) <= max_width {
            line = candidate;
        } else {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// One row's boxes, and the text that goes in them.
#[derive(Clone, Debug, PartialEq)]
struct RowLayout {
    mark: ChromeMark,
    label: String,
    cwd: String,
    badge_text: Option<String>,
    /// The `li`'s own rounded box.
    frame: [f32; 4],
    mark_rect: [f32; 4],
    label_rect: [f32; 4],
    cwd_rect: [f32; 4],
    badge_rect: Option<[f32; 4]>,
}

/// Every rectangle the prompt draws and hit-tests, in physical pixels of the
/// whole surface, together with the text each one carries.
///
/// The text lives here rather than being handed to [`build`] a second time
/// because a layout and the words it was measured for cannot be allowed to
/// disagree: they are one answer to one question.
#[derive(Clone, Debug, PartialEq)]
pub struct RestoreLayout {
    scale: f32,
    /// The dialog's border box.
    frame: [f32; 4],
    title: [f32; 4],
    sub: Vec<(String, [f32; 4])>,
    rows: Vec<RowLayout>,
    decline: [f32; 4],
    restore: [f32; 4],
}

/// Where every part of the prompt lands in a window this size.
///
/// Always an answer, unlike [`crate::settings::layout_for_menu`]: that one can
/// report `None` because `max-height: calc(100% - 72px)` can go to nothing and a
/// scrim over an absent dialog would be a window nobody can use. This prompt has
/// no `max-height`, no scrim, and nothing to trap — a window too small for it
/// shows a dialog running off its edges, which is what the mock-up does, and the
/// terminal underneath stays usable either way.
#[must_use]
pub fn layout(
    content: &RestoreContent,
    surface_width: f32,
    surface_height: f32,
    scale: f32,
) -> RestoreLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let width = dialog_width(surface_width, scale);

    let list_height = if content.rows.is_empty() {
        0.0
    } else {
        content.rows.len() as f32 * px(ROW_HEIGHT_LOGICAL_PX)
            + (content.rows.len() - 1) as f32 * px(LIST_GAP_LOGICAL_PX)
    };
    let button_height =
        2.0 * border + px(2.0 * BUTTON_PADDING_Y_LOGICAL_PX + BUTTON_LINE_LOGICAL_PX);
    let height = (2.0 * border
        + px(DIALOG_PADDING_TOP_LOGICAL_PX)
        + px(TITLE_LINE_LOGICAL_PX + TITLE_MARGIN_BOTTOM_LOGICAL_PX)
        + content.sub_lines.len() as f32 * px(SUB_LINE_LOGICAL_PX)
        + px(SUB_MARGIN_BOTTOM_LOGICAL_PX)
        + list_height
        + px(LIST_MARGIN_BOTTOM_LOGICAL_PX)
        + button_height
        + px(DIALOG_PADDING_BOTTOM_LOGICAL_PX))
    .round();

    // `left: 50%; top: 50%; transform: translate(-50%, -50%)` — centred on the
    // window on both axes, which is the whole of where it goes.
    let left = ((surface_width - width) / 2.0).round();
    let top = ((surface_height - height) / 2.0).round();
    let frame = [left, top, left + width, top + height];

    // Only the dialog's own frame is snapped to the pixel grid; everything
    // inside it is the design's exact geometry off that frame. Rounding a box's
    // two edges independently is how a 27.5px row becomes a 28px one, and the
    // shapes snap themselves at draw time anyway.
    let content_left = frame[0] + border + px(DIALOG_PADDING_X_LOGICAL_PX);
    let content_right = frame[2] - border - px(DIALOG_PADDING_X_LOGICAL_PX);
    let mut cursor = frame[1] + border + px(DIALOG_PADDING_TOP_LOGICAL_PX);

    let title = [
        content_left,
        cursor,
        content_right,
        cursor + px(TITLE_LINE_LOGICAL_PX),
    ];
    cursor = title[3] + px(TITLE_MARGIN_BOTTOM_LOGICAL_PX);

    let sub = content
        .sub_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let line_top = cursor + index as f32 * px(SUB_LINE_LOGICAL_PX);
            (
                line.clone(),
                [
                    content_left,
                    line_top,
                    content_right,
                    line_top + px(SUB_LINE_LOGICAL_PX),
                ],
            )
        })
        .collect();
    cursor +=
        content.sub_lines.len() as f32 * px(SUB_LINE_LOGICAL_PX) + px(SUB_MARGIN_BOTTOM_LOGICAL_PX);

    let rows = content
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let row_top =
                cursor + index as f32 * (px(ROW_HEIGHT_LOGICAL_PX) + px(LIST_GAP_LOGICAL_PX));
            row_layout(
                row,
                [
                    content_left,
                    row_top,
                    content_right,
                    row_top + px(ROW_HEIGHT_LOGICAL_PX),
                ],
                scale,
            )
        })
        .collect();
    cursor += list_height + px(LIST_MARGIN_BOTTOM_LOGICAL_PX);

    // `justify-content: flex-end`: the primary answer is hard against the
    // trailing edge, the plain one a gap to its left.
    let button_width =
        |text_width: f32| 2.0 * border + 2.0 * px(BUTTON_PADDING_X_LOGICAL_PX) + text_width;
    let restore = [
        content_right - button_width(content.restore_text_width),
        cursor,
        content_right,
        cursor + button_height,
    ];
    let decline = [
        restore[0] - px(ACTIONS_GAP_LOGICAL_PX) - button_width(content.decline_text_width),
        cursor,
        restore[0] - px(ACTIONS_GAP_LOGICAL_PX),
        cursor + button_height,
    ];

    RestoreLayout {
        scale,
        frame,
        title,
        sub,
        rows,
        decline,
        restore,
    }
}

/// One `li`: mark, label, cwd and — when the tab held more than one pane — the
/// badge, packed left to right one `gap` apart.
///
/// Nothing here is right-aligned. The row has no `margin-left: auto` on
/// anything, so the badge sits immediately after the cwd rather than against the
/// row's far edge; what keeps it inside the row is that `.rcwd` is the one item
/// that may shrink (`overflow: hidden` gives it a zero automatic minimum, while
/// the label's own minimum is its text). So the cwd absorbs the shortfall, and
/// when it has absorbed all of it the badge lands exactly on the row's trailing
/// padding — which is the same rectangle a right-aligned badge would occupy,
/// arrived at for the reason the design actually gives.
fn row_layout(row: &RestoreRow, frame: [f32; 4], scale: f32) -> RowLayout {
    let px = |value: f32| value * scale;
    let gap = px(ROW_GAP_LOGICAL_PX);
    let inner_left = frame[0] + px(ROW_PADDING_X_LOGICAL_PX);
    let inner_right = frame[2] - px(ROW_PADDING_X_LOGICAL_PX);

    let column_right = inner_left + px(ROW_MARK_COLUMN_LOGICAL_PX);
    let mark = px(ROW_MARK_LOGICAL_PX).round();
    let mark_left = ((inner_left + column_right - mark) / 2.0).round();
    let mark_top = ((frame[1] + frame[3] - mark) / 2.0).round();
    let mark_rect = [mark_left, mark_top, mark_left + mark, mark_top + mark];

    // `align-items: center` — each of the row's texts is centred on the row by
    // its own line box, which is why the 13px label and the 11.5px cwd share a
    // centre line and not an edge.
    let line = |box_height: f32| {
        let top = (frame[1] + frame[3] - px(box_height)) / 2.0;
        (top, top + px(box_height))
    };
    let (label_top, label_bottom) = line(ROW_LINE_LOGICAL_PX);
    let (cwd_top, cwd_bottom) = line(ROW_CWD_LINE_LOGICAL_PX);

    let label_left = column_right + gap;
    let label_rect = [
        label_left,
        label_top,
        label_left + row.label_text_width,
        label_bottom,
    ];

    let badge_text = row.badge_text();
    let badge_width = badge_text.as_ref().map(|_| {
        (row.badge_text_width + 2.0 * px(WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX))
            .max(px(WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX))
    });

    let cwd_left = label_rect[2] + gap;
    let cwd_room = inner_right - cwd_left - badge_width.map_or(0.0, |width| width + gap);
    let cwd_width = row.cwd_text_width.min(cwd_room).max(0.0);
    let cwd_rect = [cwd_left, cwd_top, cwd_left + cwd_width, cwd_bottom];

    let badge_rect = badge_width.map(|width| {
        let height = px(WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX);
        let badge_left = cwd_rect[2] + gap;
        let badge_top = (frame[1] + frame[3] - height) / 2.0;
        [
            badge_left,
            badge_top,
            badge_left + width,
            badge_top + height,
        ]
    });

    RowLayout {
        mark: row.mark,
        label: row.label.clone(),
        cwd: row.cwd.clone(),
        badge_text,
        frame,
        mark_rect,
        label_rect,
        cwd_rect,
        badge_rect,
    }
}

/// What the pointer is over: a button, [`RestoreTarget::Panel`] for the dialog
/// itself, and `None` for anywhere else in the window.
///
/// The `None` is the whole contract. This is [`crate::profiles`]'s popup shape
/// and not [`crate::settings`]'s modal one: a press beside the prompt belongs to
/// whatever is there — the terminal, a tab, the title bar — and goes on about
/// its business. The dialog is a prompt over a working app, so the app keeps
/// working.
#[must_use]
pub fn hit(layout: &RestoreLayout, x: f64, y: f64) -> Option<RestoreTarget> {
    let (x, y) = (x as f32, y as f32);
    if contains(layout.restore, x, y) {
        return Some(RestoreTarget::Restore);
    }
    if contains(layout.decline, x, y) {
        return Some(RestoreTarget::Decline);
    }
    contains(layout.frame, x, y).then_some(RestoreTarget::Panel)
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

/// Every fill, label and mark the prompt draws, as one overlay layer.
///
/// One layer, and **no scrim**. The absence is the design: the mock-up's own
/// note above the markup rules that this thing does not dim, because the
/// terminal behind it is already open and already usable. A quad over the window
/// here would be the settings modal's contract smuggled in under this one's
/// words, and it would take the working app away to ask about it.
#[must_use]
pub fn build(layout: &RestoreLayout, hover: Option<RestoreTarget>) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let mut quads = Vec::new();
    let mut labels = Vec::new();
    let mut sprites = Vec::new();

    // The dialog: the floating-window craft — lift, hairline, face — at the 10px
    // round every window that floats shares, with `--win` for its face because a
    // dialog stands on the window's own plane rather than a menu's.
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
        text: TITLE_TEXT.to_owned(),
        rect: layout.title,
        font_size_px: px(TITLE_FONT_LOGICAL_PX),
        color: palette.dialog_title_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: None,
    });

    for (text, rect) in &layout.sub {
        labels.push(ChromeLabel {
            text: text.clone(),
            rect: *rect,
            font_size_px: px(SUB_FONT_LOGICAL_PX),
            color: palette.dialog_secondary_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }

    for row in &layout.rows {
        // `background: var(--panel)` — the same plane a resting tab wears, which
        // is why the badge on it is the resting tab's badge exactly.
        quads.extend(rounded_overlay_fill(
            row.frame,
            px(ROW_RADIUS_LOGICAL_PX),
            palette.title_bar,
            1.0,
        ));
        sprites.push(ChromeSprite::new(row.mark, row.mark_rect, palette.accent));
        labels.push(ChromeLabel {
            text: row.label.clone(),
            rect: row.label_rect,
            font_size_px: px(ROW_FONT_LOGICAL_PX),
            // `--ink` over `--panel`: the row inherits the body's own ink, and
            // it is sitting on the panel plane rather than on `--win`.
            color: palette.title_text_hover,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
        // The cwd can be squeezed to nothing by a long name and a badge; when it
        // is, it is not drawn at all rather than drawn as a lone ellipsis.
        if let Some(rect) = clipped(row.cwd_rect, row.frame) {
            labels.push(ChromeLabel {
                text: row.cwd.clone(),
                rect,
                font_size_px: px(ROW_CWD_FONT_LOGICAL_PX),
                // `--ink3` over `--panel`.
                color: palette.title_text_muted,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            });
        }
        if let (Some(rect), Some(text)) = (row.badge_rect, row.badge_text.as_ref()) {
            quads.extend(rounded_overlay_fill(
                rect,
                px(WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX),
                palette.tab_badge_on_resting_tab,
                1.0,
            ));
            labels.push(ChromeLabel {
                text: text.clone(),
                rect,
                font_size_px: px(WINDOW_TAB_BADGE_FONT_LOGICAL_PX),
                color: palette.tab_badge_text_on_resting_tab,
                align_right: false,
                align_center: true,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::SemiBold,
                tabular_numerals: true,
                clip: None,
            });
        }
    }

    push_button(
        &mut quads,
        &mut labels,
        layout.decline,
        DECLINE_TEXT,
        false,
        hover == Some(RestoreTarget::Decline),
        scale,
        border,
        palette,
    );
    push_button(
        &mut quads,
        &mut labels,
        layout.restore,
        RESTORE_TEXT,
        true,
        hover == Some(RestoreTarget::Restore),
        scale,
        border,
        palette,
    );

    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

/// `.btn`, and `.btn.primary` when `primary`.
///
/// The primary one is drawn as a single fill and not as a hairline around a
/// face: `border-color: var(--accent)` on `background: var(--accent)` is a
/// border you cannot see, and two concentric fills of one colour is the same
/// rectangle at twice the cost.
#[allow(clippy::too_many_arguments)]
fn push_button(
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    rect: [f32; 4],
    text: &str,
    primary: bool,
    hovered: bool,
    scale: f32,
    border: f32,
    palette: bt_render::ChromePalette,
) {
    let px = |value: f32| value * scale;
    let radius = px(BUTTON_RADIUS_LOGICAL_PX);
    if primary {
        quads.extend(rounded_overlay_fill(
            rect,
            radius,
            if hovered {
                brightened(palette.accent, BUTTON_PRIMARY_HOVER_BRIGHTNESS)
            } else {
                palette.accent
            },
            1.0,
        ));
    } else {
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
    }
    labels.push(ChromeLabel {
        text: text.to_owned(),
        rect,
        font_size_px: px(BUTTON_FONT_LOGICAL_PX),
        color: if primary {
            BUTTON_PRIMARY_INK
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

/// CSS `filter: brightness(f)` — each sRGB channel multiplied, clamped at white.
fn brightened(color: [u8; 3], factor: f32) -> [u8; 3] {
    color.map(|channel| (f32::from(channel) * factor).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window the mock-up was measured in, and the shape every geometry
    /// claim below is stated against.
    const SURFACE: (f32, f32) = (1440.0, 756.0);

    /// The two rows and the three-line paragraph the measurement was taken with,
    /// at 1x. The widths are the mock-up's own renderer's, so a rectangle
    /// computed from them is comparable with the one it reported.
    fn measured_content(scale: f32) -> RestoreContent {
        RestoreContent {
            rows: vec![
                RestoreRow {
                    mark: ChromeMark::Folder,
                    label: "notes".to_owned(),
                    cwd: "C:\\Users\\you\\notes".to_owned(),
                    pane_count: 1,
                    label_text_width: 34.046_875 * scale,
                    cwd_text_width: 104.601_56 * scale,
                    badge_text_width: 0.0,
                },
                RestoreRow {
                    mark: ChromeMark::ProfilePowerShell,
                    label: "build".to_owned(),
                    cwd: "C:\\Users\\you\\repo\\bt".to_owned(),
                    pane_count: 3,
                    label_text_width: 29.906_25 * scale,
                    cwd_text_width: 113.625 * scale,
                    badge_text_width: 6.0 * scale,
                },
            ],
            sub_lines: vec![
                "These were open when you last closed BetterTerminal.".to_owned(),
                "They come back in the folders you left them, as new shells".to_owned(),
                "— the output is not ours to keep.".to_owned(),
            ],
            decline_text_width: 62.164_063 * scale,
            restore_text_width: 46.796_875 * scale,
        }
    }

    fn placed(scale: f32) -> RestoreLayout {
        layout(
            &measured_content(scale),
            (SURFACE.0 * scale).round(),
            (SURFACE.1 * scale).round(),
            scale,
        )
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

    /// The one layer a prompt with nothing floating inside it draws.
    fn one_layer(layers: Vec<OverlayLayer>) -> OverlayLayer {
        let [layer]: [OverlayLayer; 1] = layers
            .try_into()
            .expect("the prompt has no popup of its own");
        layer
    }

    /// PIN — every measured value of `.restore` and its parts (mock-up lines
    /// 1931-1969), nailed to the stylesheet.
    ///
    /// The line boxes are the four that are not declarations: 15px → 18,
    /// 13px → 15.5, 11.5px → 14, all reported by the mock-up's own renderer,
    /// and the paragraph's 18.75 which *is* a declaration (`line-height: 1.5`)
    /// and so is arithmetic rather than a measurement.
    #[test]
    fn the_prompt_measures_what_the_stylesheet_says_it_measures() {
        assert_eq!(DIALOG_MAX_WIDTH_LOGICAL_PX, 400.0, "width: min(400px, 92%)");
        assert_eq!(DIALOG_WIDTH_RATIO, 0.92, "width: min(400px, 92%)");
        assert_eq!(
            (
                DIALOG_PADDING_TOP_LOGICAL_PX,
                DIALOG_PADDING_X_LOGICAL_PX,
                DIALOG_PADDING_BOTTOM_LOGICAL_PX
            ),
            (20.0, 22.0, 16.0),
            "padding: 20px 22px 16px"
        );
        assert_eq!(
            FLOAT_WINDOW_RADIUS_LOGICAL_PX, 10.0,
            "border-radius: 10px — the round every floating window shares"
        );

        assert_eq!(TITLE_FONT_LOGICAL_PX, 15.0, ".restore h1 font-size: 15px");
        assert_eq!(TITLE_LINE_LOGICAL_PX, 18.0, "the 15px line box");
        assert_eq!(TITLE_MARGIN_BOTTOM_LOGICAL_PX, 5.0, "margin: 0 0 5px");
        assert_eq!(TITLE_TEXT, "Reopen your other tabs?");

        assert_eq!(SUB_FONT_LOGICAL_PX, 12.5, ".sub font-size: 12.5px");
        assert_eq!(SUB_LINE_HEIGHT_RATIO, 1.5, ".sub line-height: 1.5");
        assert_eq!(SUB_LINE_LOGICAL_PX, 18.75, "12.5 x 1.5");
        assert_eq!(SUB_MARGIN_BOTTOM_LOGICAL_PX, 14.0, ".sub margin: 0 0 14px");

        assert_eq!(
            LIST_MARGIN_BOTTOM_LOGICAL_PX, 16.0,
            ".restore-list margin: 0 0 16px"
        );
        assert_eq!(LIST_GAP_LOGICAL_PX, 1.0, ".restore-list gap: 1px");
        assert_eq!(ROW_HEIGHT_LOGICAL_PX, 27.5, "6 + the 13px line box + 6");
        assert_eq!(ROW_PADDING_X_LOGICAL_PX, 8.0, "li padding: 6px 8px");
        assert_eq!(ROW_RADIUS_LOGICAL_PX, 6.0, "li border-radius: 6px");
        assert_eq!(ROW_GAP_LOGICAL_PX, 8.0, "li gap: 8px");
        assert_eq!(ROW_FONT_LOGICAL_PX, 13.0, "li font-size: 13px");
        assert_eq!(ROW_LINE_LOGICAL_PX, 15.5, "the 13px line box");
        assert_eq!(ROW_MARK_COLUMN_LOGICAL_PX, 15.0, ".ticon width: 15px");
        assert_eq!(
            ROW_MARK_LOGICAL_PX, 14.0,
            ".restore-list .pmark {{ width: 14px; height: 14px }}"
        );
        assert_eq!(
            ROW_CWD_FONT_LOGICAL_PX, 11.5,
            ".restore-list .rcwd font-size: 11.5px"
        );
        assert_eq!(ROW_CWD_LINE_LOGICAL_PX, 14.0, "the 11.5px line box");

        assert_eq!(ACTIONS_GAP_LOGICAL_PX, 8.0, ".restore-actions gap: 8px");
        assert_eq!(BUTTON_PADDING_X_LOGICAL_PX, 14.0, ".btn padding: 6px 14px");
        assert_eq!(BUTTON_PADDING_Y_LOGICAL_PX, 6.0, ".btn padding: 6px 14px");
        assert_eq!(BUTTON_RADIUS_LOGICAL_PX, 6.0, ".btn border-radius: 6px");
        assert_eq!(BUTTON_FONT_LOGICAL_PX, 13.0, ".btn font-size: 13px");
        assert_eq!(BUTTON_LINE_LOGICAL_PX, 15.5, "the 13px line box");
        assert_eq!(
            BUTTON_PRIMARY_HOVER_BRIGHTNESS, 1.07,
            ".btn.primary:hover {{ filter: brightness(1.07) }}"
        );
        assert_eq!(
            BUTTON_PRIMARY_INK,
            [0xff, 0xff, 0xff],
            ".btn.primary {{ color: #fff }}"
        );
        assert_eq!(DECLINE_TEXT, "No thanks");
        assert_eq!(RESTORE_TEXT, "Restore");
        assert_eq!(
            SUB_TEXT,
            "These were open when you last closed BetterTerminal. They come back \
in the folders you left them, as new shells — the output is not ours to keep."
        );
    }

    /// PIN — the box the mock-up's own renderer reports: 400 x 232.75 at
    /// 1440x756, centred, for two rows and a three-line paragraph.
    ///
    /// The 232.75 is not a guess, it is the stack: `1 + 20 + 18 + 5 + 3x18.75 +
    /// 14 + (2x27.5 + 1) + 16 + 29.5 + 16 + 1`. Every term is load-bearing, and
    /// the internal offsets below are the same renderer's, taken off the frame.
    ///
    /// What lands on screen is that stack snapped to the pixel grid, and only
    /// the frame is snapped — exactly as the settings dialog does it, because a
    /// window's own border box on a half pixel is a hairline drawn twice at half
    /// strength. The quarter pixel the snap adds falls under the buttons, where
    /// there is nothing but padding to lengthen.
    ///
    /// Red gate: drop the borders from the height and it is 230.75; use the
    /// list's *border* box without its `gap` and it is 231.75; forget that the
    /// button carries its own 1px border on each side and it is 227.75.
    #[test]
    fn the_dialog_stacks_to_the_height_the_mock_up_s_own_renderer_reports() {
        let layout = placed(1.0);
        let stack: f32 = 1.0
            + 20.0
            + 18.0
            + 5.0
            + 3.0 * 18.75
            + 14.0
            + (2.0 * 27.5 + 1.0)
            + 16.0
            + 29.5
            + 16.0
            + 1.0;
        assert_eq!(stack, 232.75, "1+20+18+5+56.25+14+56+16+29.5+16+1");
        assert_eq!(width(layout.frame), 400.0, "min(400px, 92%) at 1440 wide");
        assert_eq!(
            height(layout.frame),
            stack.round(),
            "the stack, on the grid"
        );
        assert_eq!(layout.frame[0], 520.0, "(1440 - 400) / 2");
        assert_eq!(layout.frame[1], 262.0, "((756 - 233) / 2).round()");

        // Every offset below is measured off the frame's own top-left, which is
        // what the mock-up reported minus the same origin.
        assert_eq!(
            layout.title[1] - layout.frame[1],
            21.0,
            "1px border + 20px padding"
        );
        assert_eq!(height(layout.title), 18.0, "the 15px line box");
        assert_eq!(
            layout.title[0] - layout.frame[0],
            23.0,
            "1px border + 22px padding"
        );
        assert_eq!(layout.frame[2] - layout.title[2], 23.0);

        assert_eq!(layout.sub.len(), 3);
        assert_eq!(layout.sub[0].1[1] - layout.frame[1], 44.0, "21 + 18 + 5");
        assert_eq!(height(layout.sub[0].1), 18.75);
        assert_eq!(layout.sub[2].1[1] - layout.frame[1], 81.5, "44 + 2 x 18.75");

        assert_eq!(
            layout.rows[0].frame[1] - layout.frame[1],
            114.25,
            "44 + 56.25 + 14"
        );
        assert_eq!(height(layout.rows[0].frame), 27.5);
        assert_eq!(
            layout.rows[1].frame[1] - layout.rows[0].frame[3],
            1.0,
            "gap: 1px"
        );

        assert_eq!(
            layout.restore[1] - layout.frame[1],
            186.25,
            "114.25 + 56 + 16"
        );
        assert_eq!(height(layout.restore), 29.5, "1 + 6 + 15.5 + 6 + 1");
        assert_eq!(
            layout.restore[2], layout.title[2],
            "flex-end on the content box"
        );
        assert_eq!(width(layout.restore), 76.796_875, "2 + 28 + \"Restore\"");
        assert_eq!(width(layout.decline), 92.164_06, "2 + 28 + \"No thanks\"");
        assert_eq!(
            layout.restore[0] - layout.decline[2],
            8.0,
            ".restore-actions gap: 8px"
        );
        // The bottom padding is what is left under the buttons — plus the
        // quarter pixel the frame's grid snap added, which has nowhere else to
        // go and nothing to disturb where it went.
        assert_eq!(
            layout.frame[3] - layout.restore[3],
            17.25,
            "16px padding + 1px border, + the .25 the snap rounded up"
        );
    }

    /// PIN — `left: 50%; top: 50%; translate(-50%, -50%)` on the **window**, and
    /// `min(400px, 92%)` with the 92% actually engaging.
    ///
    /// The clamp turns over at 400 / .92 = 434.8 logical px, so 400 is a window
    /// narrower than the dialog's own cap and 1440 is one wider.
    #[test]
    fn the_dialog_is_centred_on_the_window_and_the_ninety_two_per_cent_clamp_engages() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let wide = placed(scale);
            assert_eq!(
                width(wide.frame),
                (DIALOG_MAX_WIDTH_LOGICAL_PX * scale).round(),
                "scale {scale}: a wide window gets the 400px cap"
            );
            let surface = [(SURFACE.0 * scale).round(), (SURFACE.1 * scale).round()];
            assert!(
                (f32::midpoint(wide.frame[0], wide.frame[2]) - surface[0] / 2.0).abs() <= 0.5,
                "scale {scale}: centred horizontally"
            );
            assert!(
                (f32::midpoint(wide.frame[1], wide.frame[3]) - surface[1] / 2.0).abs() <= 0.5,
                "scale {scale}: centred vertically"
            );

            // 400 logical px of window: 92% of it is narrower than the cap.
            let narrow_surface = (400.0 * scale).round();
            let narrow = layout(
                &measured_content(scale),
                narrow_surface,
                (700.0 * scale).round(),
                scale,
            );
            assert_eq!(
                width(narrow.frame),
                (narrow_surface * DIALOG_WIDTH_RATIO).round(),
                "scale {scale}: the 92% share is what a narrow window gives"
            );
            assert!(
                width(narrow.frame) < (DIALOG_MAX_WIDTH_LOGICAL_PX * scale).round(),
                "scale {scale}: the clamp has to actually engage or this test proves nothing"
            );
            assert_eq!(
                narrow.frame[0],
                ((narrow_surface - width(narrow.frame)) / 2.0).round(),
                "scale {scale}: still centred once clamped"
            );
            assert_eq!(
                content_width(narrow_surface, scale),
                width(narrow.frame)
                    - 2.0 * ((FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0) + 22.0 * scale),
                "scale {scale}: what the paragraph gets to wrap into"
            );
        }
    }

    /// PIN — **the** red gate for this module. It draws no scrim, and a press
    /// beside it passes straight through.
    ///
    /// `docs/DESIGN.md` §7.1.4 and the mock-up's own note above the markup:
    /// "Not modal and it does not dim: your terminal is already open behind it
    /// and already usable. This is a prompt over a working app, not a gate in
    /// front of one — which is the whole reason it is allowed to exist."
    ///
    /// Red gate: built with the settings dialog's shape — a full-window quad and
    /// a `hit` that never says `None` — this prompt would swallow every click,
    /// drag and hover in the window while it stood there, and would dim the
    /// terminal it is promising is still usable. That is the one thing its own
    /// design note forbids, and both halves of it are checked here.
    #[test]
    fn it_draws_no_scrim_and_a_press_beside_it_passes_through() {
        let layout = placed(1.0);
        let palette = chrome_palette();
        let layer = one_layer(build(&layout, None));
        let surface = [SURFACE.0, SURFACE.1];

        assert!(
            !layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.modal_scrim),
            "the scrim's own colour must appear nowhere in this overlay"
        );
        for quad in &layer.quads {
            let covers_window = quad.rect[0] <= 0.0
                && quad.rect[1] <= 0.0
                && quad.rect[2] >= surface[0]
                && quad.rect[3] >= surface[1];
            assert!(
                !covers_window,
                "a quad spanning the window is a scrim whatever it is called: {quad:?}"
            );
        }
        // Nothing it draws reaches outside its own shadow, either — the lift is
        // the widest thing, and it is three logical pixels.
        let spread = FLOAT_WINDOW_SHADOW_LOGICAL_PX;
        for quad in &layer.quads {
            assert!(
                quad.rect[0] >= layout.frame[0] - spread - 0.5
                    && quad.rect[2] <= layout.frame[2] + spread + 0.5,
                "nothing may be painted across the window: {quad:?}"
            );
        }

        // And the pointer: everywhere off the dialog belongs to whoever is there.
        for (x, y) in [
            (4.0, 4.0),
            (f64::from(SURFACE.0) - 4.0, f64::from(SURFACE.1) - 4.0),
            (
                f64::from(layout.frame[0]) - 2.0,
                f64::from(layout.frame[1]) + 20.0,
            ),
            (
                f64::from(layout.frame[2]) + 2.0,
                f64::from(layout.frame[3]) - 20.0,
            ),
            (
                f64::from(layout.frame[0]) + 20.0,
                f64::from(layout.frame[1]) - 2.0,
            ),
        ] {
            assert_eq!(
                hit(&layout, x, y),
                None,
                "({x}, {y}) is not the prompt's and must pass through"
            );
        }
        assert_eq!(
            hit(
                &layout,
                f64::from(layout.frame[0]) + 4.0,
                f64::from(layout.frame[1]) + 4.0
            ),
            Some(RestoreTarget::Panel),
            "the dialog's own body is the dialog's"
        );
    }

    /// PIN — the two buttons are the only things that answer, and the dialog's
    /// own body answers nothing.
    #[test]
    fn only_the_two_buttons_answer_and_the_body_answers_nothing() {
        let layout = placed(1.0);
        let (x, y) = centre(layout.restore);
        assert_eq!(hit(&layout, x, y), Some(RestoreTarget::Restore));
        let (x, y) = centre(layout.decline);
        assert_eq!(hit(&layout, x, y), Some(RestoreTarget::Decline));

        assert_eq!(answer(RestoreTarget::Restore), Some(RestoreAnswer::Restore));
        assert_eq!(
            answer(RestoreTarget::Decline),
            Some(RestoreAnswer::NoThanks)
        );
        assert_eq!(
            answer(RestoreTarget::Panel),
            None,
            "a press on the dialog is not silence taken for an answer"
        );
        assert_eq!(
            FOCUSED_ANSWER,
            RestoreAnswer::Restore,
            "$(\"btn-restore\").focus()"
        );
    }

    /// PIN — §7.1.4: an unanswered prompt must fold back into `lastSession` and
    /// must not be lost, so Esc is not an answer and the prompt does not take it.
    ///
    /// Red gate: wiring Esc to "No thanks" — the reflex, and what the settings
    /// modal's own Esc route would suggest — would spend every unpinned tab on a
    /// keystroke aimed at the shell behind the prompt, which is still focused
    /// and still working, because this thing is not modal.
    #[test]
    fn escape_is_not_an_answer_and_leaves_the_question_standing() {
        let mut prompt = RestorePrompt::default();
        assert!(!prompt.is_open());
        prompt.open();
        assert!(prompt.is_open());
        assert!(
            !prompt.consumes_escape(),
            "Esc belongs to whatever you are typing into"
        );
        assert!(prompt.is_open(), "and it leaves the question standing");
        assert!(prompt.close(), "only an answer puts it away");
        assert!(!prompt.close(), "closing a shut prompt consumes nothing");
    }

    /// PIN — hover is a fact about an open prompt: a stale button cannot stay
    /// lit under a prompt that is no longer there.
    #[test]
    fn hover_belongs_to_an_open_prompt_only() {
        let mut prompt = RestorePrompt::default();
        assert!(!prompt.set_hover(Some(RestoreTarget::Restore)));
        assert_eq!(prompt.hover(), None);
        prompt.open();
        assert!(prompt.set_hover(Some(RestoreTarget::Restore)));
        assert_eq!(prompt.hover(), Some(RestoreTarget::Restore));
        assert!(
            !prompt.set_hover(Some(RestoreTarget::Restore)),
            "no repaint"
        );
        prompt.close();
        assert_eq!(prompt.hover(), None);
    }

    /// PIN — mock-up 291 and 7487: the badge counts panes, and a tab holding one
    /// pane has nothing to count, so it takes no space at all.
    ///
    /// Red gate: drawing a `1` on every row would put a badge on the common case
    /// — one pane — and turn a "this one is different" mark into decoration.
    #[test]
    fn a_tab_with_one_pane_wears_no_badge_and_a_tab_with_several_wears_one() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = placed(scale);
            let palette = chrome_palette();
            let (single, several) = (&layout.rows[0], &layout.rows[1]);
            assert_eq!(single.badge_text, None, "one pane is nothing to count");
            assert_eq!(single.badge_rect, None);
            assert_eq!(several.badge_text.as_deref(), Some("3"));

            let badge = several.badge_rect.expect("three panes wear a badge");
            assert_eq!(
                height(badge),
                WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX * scale,
                "scale {scale}: `.panecount {{ height: 15px }}`"
            );
            assert_eq!(
                width(badge),
                (WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX * scale)
                    .max(6.0 * scale + 2.0 * WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX * scale),
                "scale {scale}: `min-width: 15px; padding: 0 4px`"
            );
            assert!(
                (f32::midpoint(badge[1], badge[3])
                    - f32::midpoint(several.frame[1], several.frame[3]))
                .abs()
                    <= 0.5,
                "scale {scale}: `align-items: center`"
            );

            let layer = one_layer(build(&layout, None));
            let badges = layer
                .quads
                .iter()
                .filter(|quad| quad.color == palette.tab_badge_on_resting_tab)
                .count();
            assert_eq!(
                layer
                    .labels
                    .iter()
                    .filter(|label| label.text == "3")
                    .count(),
                1,
                "scale {scale}: exactly one row is counted"
            );
            assert!(badges > 0, "scale {scale}: and it is drawn on a fill");
        }
    }

    /// PIN — the row is a flex line packed from the left, and the cwd is the one
    /// item on it that gives ground.
    ///
    /// Red gate: right-aligning the badge instead would look identical on a
    /// crowded row and wrong on every short one, where the mock-up puts the
    /// count immediately after the path rather than out at the row's edge.
    #[test]
    fn the_row_packs_left_and_the_cwd_gives_up_the_room() {
        let scale = 1.0;
        let layout = placed(scale);
        let row = &layout.rows[1];
        // 8px of padding, then a 14px mark centred on its 15px column — which
        // puts it half a pixel in, and a raster on a half pixel is a blurred
        // raster, so the mark alone is snapped (the same call `profiles.rs`
        // makes for the same reason).
        assert_eq!(row.mark_rect[0] - row.frame[0], 9.0, "8 + round(0.5)");
        assert_eq!(width(row.mark_rect), 14.0);
        assert_eq!(
            row.label_rect[0] - row.frame[0],
            31.0,
            "8 padding + 15 column + 8 gap"
        );
        assert_eq!(
            row.cwd_rect[0] - row.label_rect[2],
            8.0,
            "one gap after the label's own advance"
        );
        let badge = row.badge_rect.expect("three panes");
        assert_eq!(badge[0] - row.cwd_rect[2], 8.0, "and one after the cwd");
        assert_eq!(width(row.cwd_rect), 113.625, "the cwd fits, so it is whole");
        // The two texts share a centre line, not an edge: the mock-up's own
        // renderer puts the 15.5 line box 6.0 below the row's top and the 14
        // line box 6.75 below it.
        assert_eq!(height(row.label_rect), 15.5, "the 13px line box");
        assert_eq!(row.label_rect[1] - row.frame[1], 6.0);
        assert_eq!(height(row.cwd_rect), 14.0, "the 11.5px line box");
        assert_eq!(row.cwd_rect[1] - row.frame[1], 6.75);

        // Now crowd it: a cwd far too long for the row must shrink to exactly
        // what the badge and the paddings leave, and the badge must land on the
        // row's trailing padding rather than outside it.
        let mut crowded = measured_content(scale);
        crowded.rows[1].cwd_text_width = 4000.0;
        let crowded = layout_row(&crowded, scale);
        let badge = crowded.badge_rect.expect("three panes");
        assert!(
            (badge[2] - (crowded.frame[2] - ROW_PADDING_X_LOGICAL_PX)).abs() <= 0.001,
            "the badge is kept inside the row: {badge:?} in {:?}",
            crowded.frame
        );
        assert!(
            crowded.cwd_rect[2] <= badge[0],
            "the cwd stops where the badge starts"
        );
        assert!(
            width(crowded.cwd_rect) > 0.0,
            "and there is still some of it"
        );
    }

    /// The second row of a content, laid out in the standard window.
    fn layout_row(content: &RestoreContent, scale: f32) -> RowLayout {
        layout(
            content,
            (SURFACE.0 * scale).round(),
            (SURFACE.1 * scale).round(),
            scale,
        )
        .rows
        .remove(1)
    }

    /// PIN — mock-up 7484-7485: a tab comes back under **your** name for it, or
    /// under the folder it stood in. Never under the title the program that has
    /// gone was wearing.
    #[test]
    fn a_row_is_named_by_your_name_for_it_or_by_the_folder_it_stood_in() {
        let named = RestoreRow::from_seed(
            &Seed::Term {
                profile_id: "pwsh".to_owned(),
                cwd: "C:\\Users\\you\\repo".to_owned(),
                manual_name: Some("build".to_owned()),
            },
            2,
        );
        assert_eq!(named.label, "build", "your name for it");
        assert_eq!(named.cwd, "C:\\Users\\you\\repo");
        assert_eq!(named.mark, PROFILES[0].mark, "a profile's icon is its mark");
        assert_eq!(named.badge_text().as_deref(), Some("2"));

        let unnamed = RestoreRow::from_seed(
            &Seed::Term {
                profile_id: "pwsh".to_owned(),
                cwd: "C:\\Users\\you\\notes".to_owned(),
                manual_name: None,
            },
            1,
        );
        assert_eq!(unnamed.label, "notes", "else the folder it stood in");
        assert_eq!(unnamed.badge_text(), None);

        let files = RestoreRow::from_seed(
            &Seed::Files {
                root: "C:\\Users\\you\\docs\\".to_owned(),
            },
            1,
        );
        assert_eq!(files.label, "docs", "a trailing separator is not a name");
        assert_eq!(files.mark, ChromeMark::Folder);

        // A profile this build does not have costs the tab its shell choice and
        // never the tab — §5.4 逐叶降级, which `index_of_id` already rules on.
        let stranger = RestoreRow::from_seed(
            &Seed::Term {
                profile_id: "nushell".to_owned(),
                cwd: "C:\\Users\\you\\notes".to_owned(),
                manual_name: None,
            },
            1,
        );
        assert_eq!(stranger.mark, PROFILES[0].mark);
    }

    /// PIN — the paragraph breaks at spaces and only at spaces, greedily, which
    /// is what `overflow-wrap: normal` does. Measured with a ruler of exactly
    /// ten units a character so the breaks are arithmetic rather than a font.
    #[test]
    fn the_paragraph_breaks_where_the_words_run_out() {
        let ruler = |text: &str| text.chars().count() as f32 * 10.0;
        assert_eq!(
            wrap("one two three four", 100.0, ruler),
            vec!["one two", "three four"],
            "greedy: `one two` is 70, adding `three` would be 130"
        );
        assert_eq!(
            wrap("antidisestablishmentarianism ok", 50.0, ruler),
            vec!["antidisestablishmentarianism", "ok"],
            "a word too long for the line overruns it rather than being cut"
        );
        assert_eq!(wrap("", 100.0, ruler), Vec::<String>::new());
        assert_eq!(
            wrap(SUB_TEXT, 4000.0, ruler).len(),
            1,
            "a wide enough box takes it whole"
        );
        // And the sentence really is three lines at the mock-up's own width:
        // 354 logical px at 12.5px, which this ruler stands in for by measuring
        // the same words the browser did.
        let lines = wrap(SUB_TEXT, 530.0, ruler);
        assert!(lines.len() > 1, "at a realistic width it wraps");
        assert_eq!(
            lines.join(" "),
            SUB_TEXT,
            "wrapping loses no word and adds none"
        );
    }

    /// PIN — the prompt borrows the dialog's ink for its own plane and the tab
    /// strip's for the rows, because that is what the two surfaces are.
    ///
    /// Red gate: the row sits on `--panel`, not on `--win`. Inking its label and
    /// its cwd with the `dialog_*` family — the same `--ink`/`--ink3`
    /// composited over `--win` — is right in the light theme, where `--win` and
    /// `--panel` are both near-white, and a step adrift in the dark one. That is
    /// exactly the class of error a light-theme review passes.
    #[test]
    fn the_prompt_inks_its_dialog_plane_and_its_panel_rows_apart() {
        let layout = placed(1.0);
        let palette = chrome_palette();
        let layer = one_layer(build(&layout, None));
        let label = |text: &str| {
            layer
                .labels
                .iter()
                .find(|label| label.text == text)
                .unwrap_or_else(|| panic!("the prompt says {text:?}"))
        };

        let title = label(TITLE_TEXT);
        assert_eq!(title.color, palette.dialog_title_text, "--ink over --win");
        assert_eq!(title.font_size_px, TITLE_FONT_LOGICAL_PX);
        assert_eq!(
            title.weight,
            ChromeLabelWeight::SemiBold,
            "h1 font-weight: 600"
        );

        let sub = label(&layout.sub[0].0);
        assert_eq!(
            sub.color, palette.dialog_secondary_text,
            "--ink2 over --win"
        );
        assert_eq!(sub.font_size_px, SUB_FONT_LOGICAL_PX);

        assert_eq!(
            label("notes").color,
            palette.title_text_hover,
            "the row's own label is --ink over --panel"
        );
        assert_eq!(
            label("C:\\Users\\you\\notes").color,
            palette.title_text_muted,
            ".rcwd is --ink3 over --panel"
        );
        assert_eq!(
            label("3").color,
            palette.tab_badge_text_on_resting_tab,
            "the badge sits on --panel, exactly as it does on a resting tab"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.title_bar),
            "and the row itself is --panel"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.dialog_surface),
            "while the dialog's face is --win"
        );
        assert!(
            layer
                .sprites
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::Folder),
            "every row wears its tab's mark"
        );
    }

    /// PIN — `.btn` is a hairline around `--win`; `.btn.primary` is the accent
    /// with white on it, and neither is drawn as the other.
    #[test]
    fn the_plain_button_is_a_hairline_and_the_primary_one_is_the_accent() {
        let layout = placed(1.0);
        let palette = chrome_palette();
        let rest = one_layer(build(&layout, None));
        let restore = rest
            .labels
            .iter()
            .find(|label| label.text == RESTORE_TEXT)
            .expect("the primary button is named");
        assert_eq!(
            restore.color, BUTTON_PRIMARY_INK,
            ".btn.primary color: #fff"
        );
        assert!(restore.align_center, "a button's caption is centred on it");
        let decline = rest
            .labels
            .iter()
            .find(|label| label.text == DECLINE_TEXT)
            .expect("the plain button is named");
        assert_eq!(
            decline.color, palette.dialog_title_text,
            ".btn color: var(--ink)"
        );

        assert!(
            rest.quads.iter().any(|quad| quad.color == palette.accent),
            ".btn.primary background: var(--accent)"
        );

        // Hover: `--hover` over `--win` for the plain one, brightness(1.07) on
        // the accent for the primary one.
        let hovered_plain = one_layer(build(&layout, Some(RestoreTarget::Decline)));
        assert!(
            hovered_plain
                .quads
                .iter()
                .any(|quad| quad.color == palette.dialog_hover),
            ".btn:hover {{ background: var(--hover) }}"
        );
        let hovered_primary = one_layer(build(&layout, Some(RestoreTarget::Restore)));
        let lifted = brightened(palette.accent, BUTTON_PRIMARY_HOVER_BRIGHTNESS);
        assert_ne!(lifted, palette.accent, "1.07 has to move something");
        assert!(
            hovered_primary
                .quads
                .iter()
                .any(|quad| quad.color == lifted),
            ".btn.primary:hover {{ filter: brightness(1.07) }}"
        );
        assert_eq!(
            brightened([0xf5, 0x00, 0x80], 1.07),
            [0xff, 0x00, 0x89],
            "and it clamps at white rather than wrapping"
        );
    }
}
