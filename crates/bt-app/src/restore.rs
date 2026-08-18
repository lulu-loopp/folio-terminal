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
    profiles::{self, index_of_id},
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
pub fn title_text() -> &'static str {
    crate::i18n::Text::RestoreTitle.text()
}

// ── `.restore .sub` ────────────────────────────────────────────────────────
/// `font-size: 12.5px`.
pub const SUB_FONT_LOGICAL_PX: f32 = 12.5;
/// `line-height: 1.5` — declared, so it is arithmetic rather than a measurement:
/// 12.5 × 1.5.
const SUB_LINE_HEIGHT_RATIO: f32 = 1.5;
const SUB_LINE_LOGICAL_PX: f32 = SUB_FONT_LOGICAL_PX * SUB_LINE_HEIGHT_RATIO;
/// `margin: 0 0 14px`.
const SUB_MARGIN_BOTTOM_LOGICAL_PX: f32 = 14.0;
/// The paragraph (mock-up line 2227, less its closing aside).
///
/// **It states what happens and stops** (user ruling, 2026-08-17). The mock-up's
/// line went on "— the output is not ours to keep", and that clause is the
/// product talking about itself in the first person to excuse something it is
/// not doing; "as new shells" has already told the reader the output is gone.
/// It wraps, so it is drawn as lines — see [`wrap`], which had to learn how
/// Chinese breaks before this line could be shown in it.
pub fn sub_text() -> &'static str {
    crate::i18n::Text::RestoreSub.text()
}

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
pub fn decline_text() -> &'static str {
    crate::i18n::Text::RestoreDecline.text()
}
pub fn restore_text() -> &'static str {
    crate::i18n::Text::RestoreAccept.text()
}

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
            } => {
                let profile = index_of_id(profile_id);
                (
                    profiles::mark(profile),
                    // **This seed's own profile** is the name's last layer, and
                    // this row is where getting it wrong shows worst: a restore
                    // row has no program title at all (the program left with the
                    // tab), so a shell that never reported a folder falls
                    // straight through to it. Under the old hard-coded
                    // `"PowerShell"` the prompt listed three tabs wearing the
                    // Ubuntu, Git and Command Prompt marks and captioned every
                    // one of them `PowerShell` — the mark and the word, side by
                    // side, naming two different shells.
                    crate::display_title(
                        manual_name.as_deref(),
                        None,
                        Some(Path::new(cwd)),
                        &profiles::display_title(profile),
                    ),
                    cwd.clone(),
                )
            }
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
    /// [`sub_text()`], already broken to lines that fit [`content_width`]. See
    /// [`wrap`].
    pub sub_lines: Vec<String>,
    /// [`decline_text()`] at [`BUTTON_FONT_LOGICAL_PX`] × scale.
    pub decline_text_width: f32,
    /// [`restore_text()`] at the same size.
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
/// Greedy over [`break_pieces`], which is what CSS `overflow-wrap: normal` does
/// in a browser that knows about CJK: a Latin word that cannot fit alone gets a
/// line of its own and overruns it, because the alternative — breaking inside a
/// word — is a thing the design never asked for and would put half a path on
/// each of two lines; a run of ideographs breaks between any two of them,
/// because that is where Chinese has its opportunities and it has no others.
///
/// The whitespace that separated two pieces is put back when they end up on one
/// line and dropped when they do not, which is why this joins with a space
/// rather than slicing the original: a line must not end in the space that
/// happened to precede the word that did not fit.
#[must_use]
pub fn wrap(text: &str, max_width: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for piece in break_pieces(text) {
        if line.is_empty() {
            line.push_str(piece.text);
            continue;
        }
        let candidate = if piece.space_before {
            format!("{line} {}", piece.text)
        } else {
            format!("{line}{}", piece.text)
        };
        if measure(&candidate) <= max_width {
            line = candidate;
        } else {
            lines.push(std::mem::take(&mut line));
            line.push_str(piece.text);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// One atom of the paragraph, and whether a space stood in front of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BreakPiece<'a> {
    text: &'a str,
    space_before: bool,
}

/// Cut `text` into the smallest runs a line may end after.
///
/// **Whitespace first, then inside each word.** The first cut is the one this
/// function has always made and is the whole of what English needs. The second
/// exists because Chinese has no spaces at all: `SUB_TEXT` in Chinese is one
/// forty-character "word", and a wrapper that only knew about spaces would draw
/// it as a single line running off both edges of a 400px dialog. That is the one
/// place in the whole string table where translation is not enough and the
/// rendering path itself has to learn something (§A15 of the string inventory
/// flagged it as such before the words existed).
///
/// **A character-class rule, not UAX#14.** The real algorithm has thirty-odd
/// classes, a pair table and tailoring, and it exists to serve arbitrary text in
/// any script; what is being wrapped here is one paragraph of this product's own
/// prose, in two languages, at one measure. Three rules cover it, and each is
/// listed because a rule that is not written down is a rule the next person
/// deletes:
///
/// 1. **Break between two adjacent characters when either is CJK.** Ideographs,
///    kana and Hangul are written without spaces and a line may end after any of
///    them; the Latin/CJK boundary is a break opportunity too, so `Folio时` may
///    split even though nothing separates them.
/// 2. **Never break *before* closing punctuation** — `，。、！？；：）》」』` and
///    their Latin equivalents. A line that began with `。` would be the single
///    most obviously wrong thing this could do.
/// 3. **Never break *after* opening punctuation** — `（《「『` and `([{`. The
///    mirror of rule 2, and it is a separate rule rather than a symmetry because
///    the two sets are not each other's mirror image in Unicode.
///
/// 4. **Break *after* a path separator** — `\\` and `/`, in both languages.
///    Added with the PSReadLine invitation (§7.1.6c-3b), which is the first of
///    these sentences to name a place on disk: a Windows path is one
///    space-less token forty or eighty characters long, and rule 1 does not
///    reach it because nothing in it is CJK. Without this the paragraph draws a
///    line that runs out of both edges of a 400px dialog — which is exactly the
///    failure Chinese had, arriving in a second script. *After* and not before,
///    because a line ending in `\\` reads as "continues", and a line beginning
///    with one reads as a UNC share.
///
/// What is knowingly given up: no line-break class for the numeric and unit
/// runs (`64 KB` is protected only by having a space in it, as it is in
/// English), no tailoring for the two Chinese conventions about `·`, and no
/// hyphenation in either language. None of the three is reachable from the
/// sentences this function actually wraps.
fn break_pieces(text: &str) -> Vec<BreakPiece<'_>> {
    let mut pieces = Vec::new();
    for (index, word) in text.split_whitespace().enumerate() {
        let mut start = 0usize;
        let mut previous: Option<char> = None;
        for (offset, character) in word.char_indices() {
            if let Some(previous) = previous
                && breaks_between(previous, character)
            {
                pieces.push(BreakPiece {
                    text: &word[start..offset],
                    space_before: index > 0 && start == 0,
                });
                start = offset;
            }
            previous = Some(character);
        }
        pieces.push(BreakPiece {
            text: &word[start..],
            space_before: index > 0 && start == 0,
        });
    }
    pieces
}

/// Whether a line may end between these two characters — see [`break_pieces`]
/// for the three rules and for what they deliberately leave out.
fn breaks_between(before: char, after: char) -> bool {
    // Rule 4, ahead of the CJK gate because it is not about script: a path
    // separator is a break opportunity in an English sentence as much as in a
    // Chinese one.
    if matches!(before, '\\' | '/') && !matches!(after, '\\' | '/') {
        return true;
    }
    if !is_cjk(before) && !is_cjk(after) {
        return false;
    }
    !no_break_before(after) && !no_break_after(before)
}

/// The scripts written without spaces, plus the punctuation that belongs to
/// them.
///
/// The ranges are blocks rather than a property lookup, which is the same trade
/// `bt-unicode` makes for width: the answer only has to be right for text this
/// product writes, and every one of these blocks is unambiguous.
fn is_cjk(character: char) -> bool {
    matches!(character,
        '\u{1100}'..='\u{11ff}'      // Hangul Jamo
        | '\u{2e80}'..='\u{2eff}'    // CJK radicals
        | '\u{3000}'..='\u{303f}'    // CJK symbols and punctuation
        | '\u{3040}'..='\u{30ff}'    // Hiragana, Katakana
        | '\u{3130}'..='\u{318f}'    // Hangul compatibility jamo
        | '\u{3400}'..='\u{4dbf}'    // CJK extension A
        | '\u{4e00}'..='\u{9fff}'    // CJK unified ideographs
        | '\u{a960}'..='\u{a97f}'    // Hangul jamo extended A
        | '\u{ac00}'..='\u{d7ff}'    // Hangul syllables
        | '\u{f900}'..='\u{faff}'    // CJK compatibility ideographs
        | '\u{fe30}'..='\u{fe4f}'    // CJK compatibility forms
        | '\u{ff00}'..='\u{ff60}'    // Fullwidth forms
        | '\u{ffe0}'..='\u{ffe6}'    // Fullwidth signs
        | '\u{20000}'..='\u{3ffff}'  // CJK extensions B and beyond
    )
}

/// Punctuation a line may never begin with.
fn no_break_before(character: char) -> bool {
    matches!(
        character,
        '，' | '。'
            | '、'
            | '！'
            | '？'
            | '；'
            | '：'
            | '·'
            | '…'
            | '‥'
            | '）'
            | '〕'
            | '】'
            | '》'
            | '〉'
            | '」'
            | '』'
            | '〗'
            | '〙'
            | '〛'
            | '〞'
            | '＂'
            | '％'
            | '℃'
            | 'ー'
            | '～'
            | '〜'
            | ','
            | '.'
            | '!'
            | '?'
            | ';'
            | ':'
            | ')'
            | ']'
            | '}'
            | '%'
    )
}

/// Punctuation a line may never end with.
fn no_break_after(character: char) -> bool {
    matches!(
        character,
        '（' | '〔'
            | '【'
            | '《'
            | '〈'
            | '「'
            | '『'
            | '〖'
            | '〘'
            | '〚'
            | '〝'
            | '('
            | '['
            | '{'
    )
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
        text: title_text().to_owned(),
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
        decline_text(),
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
        restore_text(),
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
    push_button_enabled(
        quads, labels, rect, text, primary, hovered, true, scale, border, palette,
    );
}

/// `.btn`, with the third state the invitation needs: **offered but dark**.
///
/// A disabled button is drawn as a plain `.btn` with muted ink and never in the
/// accent, and it does not light under the pointer — which is the whole of what
/// "no dead button" asks for on this surface. It stays on screen rather than
/// disappearing because the sentence beside it explains a state of the machine,
/// and a reason with nothing to point at is a reason about nothing.
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn push_button_enabled(
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    rect: [f32; 4],
    text: &str,
    primary: bool,
    hovered: bool,
    enabled: bool,
    scale: f32,
    border: f32,
    palette: bt_render::ChromePalette,
) {
    let px = |value: f32| value * scale;
    let radius = px(BUTTON_RADIUS_LOGICAL_PX);
    let primary = primary && enabled;
    let hovered = hovered && enabled;
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
        } else if enabled {
            palette.dialog_title_text
        } else {
            palette.title_text_muted
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

// ── the dirty-buffer gate (P123-P125, `DESIGN.md` §7.1.3) ───────────────────
//
// "Hidden dirty state never evaporates silently: closing the LAST preview pane,
// closing the tab, or shutting the app all confirm every dirty buffer by name"
// (P117). The mock-up asks all three with the browser's own `confirm()`, which is
// a modal with two buttons and a sentence — so this is that, drawn in the
// restore prompt's own craft rather than as a fourth kind of window: the same
// [`push_float_window`] face, the same `.btn` pair, the same padding, the same
// wrap. What differs is what the two surfaces *are*, and the difference is one
// field: this one is a **gate**, so it dims and it owns the keyboard, and the
// prompt above is not, so it does neither.

/// What a press on the gate answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateAnswer {
    /// Throw the unsaved edits away and carry on with what was asked.
    Discard,
    /// Change nothing. **The default**, and Esc's answer: a gate that took
    /// silence for consent would be the thing §7.1.3 exists to forbid.
    Cancel,
}

/// Something on the gate the pointer can be over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateTarget {
    /// The dialog itself, away from either button. A press here does nothing.
    Panel,
    Cancel,
    Discard,
}

/// `Discard` — the destructive answer, and therefore **not** the focused one.
pub const GATE_DISCARD_TEXT: &str = "Discard";
pub const GATE_CANCEL_TEXT: &str = "Cancel";
/// The button Enter answers, which is the one that changes nothing.
pub const GATE_FOCUSED_ANSWER: GateAnswer = GateAnswer::Cancel;
/// The title of the gate that guards unsaved preview edits.
///
/// **The gate has more than one question to ask now**, so the title travels with
/// the request rather than being baked into the drawing (see
/// [`GateRequest::title`]). This is the sentence for the three the gate was built
/// for; the fourth — a working-tree discard — asks something different and says
/// so.
pub const GATE_TITLE_TEXT: &str = "Discard unsaved changes?";
/// The title of the gate in front of a working-tree discard (R14).
///
/// It says *changes*, not *unsaved changes*, and the difference is the whole
/// point: the file on disk is saved. What is about to go is the difference
/// between it and what git has, which no amount of saving would bring back.
pub const GATE_GIT_DISCARD_TITLE: &str = "Discard changes?";
/// And the title when the file is untracked, where "discard" means *delete*.
///
/// A gate that said "discard changes" over a file git has never seen would be
/// describing the smaller of two acts. The button still says `Discard`, because
/// that is the word the page's own verb uses; the question says what it does.
pub const GATE_GIT_DELETE_TITLE: &str = "Delete this file?";
/// The word on the destructive button when the thing going is a **name** rather
/// than a file's contents (v2 ④).
///
/// The button carries the row's own verb, which for the two ref deletions is
/// `Delete` — a gate headed "Delete branch?" whose only other button said
/// `Discard` would be two words for one act, and the reader would be entitled to
/// wonder which of them the button actually does.
pub const GATE_DELETE_TEXT: &str = "Delete";
/// The question over a `git branch -d` (v2 ④).
///
/// It does not promise the branch will go, and that is deliberate: `-d` is the
/// merged-only spelling, so git refuses a branch whose commits are nowhere else.
/// The sentence under it says so in one line, and git's own refusal arrives on a
/// card if it comes to that.
pub const GATE_GIT_DELETE_BRANCH_TITLE: &str = "Delete this branch?";
/// And over a `git tag -d`, which git never refuses.
pub const GATE_GIT_DELETE_TAG_TITLE: &str = "Delete this tag?";
/// The question over `Clear scrollback…` — the mock-up's own first line
/// (8250), word for word.
pub const GATE_CLEAR_SCROLLBACK_TITLE: &str = "Clear scrollback?";
/// The word on the button that goes through with it.
///
/// The row's own verb, which is the rule [`GATE_DELETE_TEXT`] states: a gate
/// headed "Clear scrollback?" whose only other button said `Discard` would be
/// two words for one act.
pub const GATE_CLEAR_TEXT: &str = "Clear";

/// The gate's own sentence — the mock-up's `Discard unsaved changes to a.txt,
/// b.md?` (3600), split into a title and a list because a `confirm()` string has
/// nowhere else to put either.
///
/// **By name, always** (§7.1.3). A gate that said "some files have unsaved
/// changes" would be asking you to guess what you are about to lose, which is the
/// same silence it exists to break, one sentence further on.
#[must_use]
pub fn gate_message(names: &[String]) -> String {
    format!("Discard unsaved changes to {}?", names.join(", "))
}

/// What the gate is asking about, and which control the pointer is on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirtyGate {
    open: Option<GateRequest>,
    hover: Option<GateTarget>,
}

/// What the window was in the middle of doing when the gate stopped it.
///
/// The gate carries the *intention*, not a callback, so the answer is spent by
/// re-running one of three verbs that already exist. A closure would put the same
/// three verbs behind a type nothing can print, and a gate is exactly the place a
/// wrong verb must be impossible to reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateRequest {
    /// Closing the tab's last preview pane (P123).
    ClosePane(bt_layout::SeatId),
    /// Closing a tab (P124), by index in the strip.
    CloseTab(usize),
    /// Shutting the window (P125).
    Shut,
    /// Throwing a file's working-tree changes away, or deleting an untracked
    /// file (R14).
    ///
    /// **The fourth question this machine asks, and the first that is not about
    /// an unsaved buffer.** It is here rather than in a second gate of its own
    /// because everything a confirmation *is* — a scrim, a modal that owns the
    /// keyboard, Esc and Enter both answering "change nothing", a destructive
    /// button that is deliberately not the focused one — is already here and
    /// correct, and a second copy of it would be a second chance to get the
    /// default answer the wrong way round. What had to be generalized is only the
    /// vocabulary: the title now comes from the request.
    GitDiscard {
        seat: bt_layout::SeatId,
        /// Repo-relative, in git's grammar — and the name the gate says out loud.
        path: String,
        /// Whether git has ever seen this file, which decides both the sentence
        /// and the command.
        untracked: bool,
    },
    /// Deleting a local branch from a context menu (v2 ④) — `git branch -d`.
    ///
    /// **`GitDiscard`'s sibling and not its cousin**: it is here for the same
    /// reason, which is that everything a confirmation *is* already lives in this
    /// machine and correct, and a second copy would be a second chance to get the
    /// default answer the wrong way round.
    ///
    /// Behind the gate even though `-d` refuses an unmerged branch, because the
    /// case it does *not* refuse is still a name disappearing off a list at the
    /// end of a two-item pointer gesture — and "it was on the menu under the
    /// pointer" is not consent.
    ///
    /// Keyed by **root** and not by seat, unlike the discard above. A branch is a
    /// fact about a repository and the menu that offers this can be raised in a
    /// graph — which is a document keyed by root and belongs to no column — so a
    /// request naming a seat would be a request that could not be made from half
    /// the places that make it.
    GitDeleteBranch {
        root: std::path::PathBuf,
        name: String,
    },
    /// Deleting a tag from the graph's own pill (v2 ④) — `git tag -d`.
    GitDeleteTag {
        root: std::path::PathBuf,
        name: String,
    },
    /// **Deleting one pane's transcript** — `Clear scrollback…` (ticket #62).
    ///
    /// Here for the reason [`Self::GitDiscard`] is here: everything a
    /// confirmation *is* already lives in this machine and is correct, and the
    /// mock-up's own `window.confirm("Clear scrollback?\nPast output is deleted
    /// — search over it will find nothing.")` is exactly a title and a sentence
    /// with a destructive button under them.
    ///
    /// It is the only request on this list whose subject is **not a document**,
    /// and that is the whole of why §7.1.6 puts it behind a gate at all: what
    /// goes is not a file you could open again but a record that exists nowhere
    /// else on the machine — bt-app never writes a transcript to disk
    /// (`docs/M2-persistence-schema-v1.md` §0), so the only copy of what a shell
    /// said this session is the one this row deletes.
    ClearScrollback(bt_layout::SeatId),
}

impl GateRequest {
    /// The question this gate is asking.
    #[must_use]
    pub fn title(&self) -> &'static str {
        match self {
            Self::ClosePane(_) | Self::CloseTab(_) | Self::Shut => GATE_TITLE_TEXT,
            Self::GitDiscard {
                untracked: false, ..
            } => GATE_GIT_DISCARD_TITLE,
            Self::GitDiscard {
                untracked: true, ..
            } => GATE_GIT_DELETE_TITLE,
            Self::GitDeleteBranch { .. } => GATE_GIT_DELETE_BRANCH_TITLE,
            Self::GitDeleteTag { .. } => GATE_GIT_DELETE_TAG_TITLE,
            Self::ClearScrollback(_) => GATE_CLEAR_SCROLLBACK_TITLE,
        }
    }

    /// The word on the button that goes through with it.
    ///
    /// Carried on the request for [`Self::title`]'s reason: the gate is one
    /// machine asking several questions now, and the verb the reader pressed to
    /// get here is part of the question.
    #[must_use]
    pub fn answer_text(&self) -> &'static str {
        match self {
            Self::ClosePane(_) | Self::CloseTab(_) | Self::Shut | Self::GitDiscard { .. } => {
                GATE_DISCARD_TEXT
            }
            Self::GitDeleteBranch { .. } | Self::GitDeleteTag { .. } => GATE_DELETE_TEXT,
            Self::ClearScrollback(_) => GATE_CLEAR_TEXT,
        }
    }

    /// The sentence under it — **by name, always** (§7.1.3).
    ///
    /// `names` is what the caller collected: the dirty buffers for the three
    /// original requests, and the one path for a discard. A gate that named
    /// nothing would be asking you to guess what you are about to lose, which is
    /// the same silence it exists to break.
    #[must_use]
    pub fn message(&self, names: &[String]) -> String {
        match self {
            Self::ClosePane(_) | Self::CloseTab(_) | Self::Shut => gate_message(names),
            Self::GitDiscard {
                untracked: false, ..
            } => format!(
                "{} goes back to the last staged or committed version. This cannot be undone.",
                names.join(", ")
            ),
            Self::GitDiscard {
                untracked: true, ..
            } => format!(
                "{} is deleted. git has no copy of it, so this cannot be undone.",
                names.join(", ")
            ),
            // **Short and honest** (ticket wording, v2 ④). It says what the
            // command is going to be — `-d` — without saying the word, because
            // "git will refuse if it is not merged" is what `-d` *means* and is
            // the one thing a reader needs in order to press the button without
            // being surprised by what comes back.
            Self::GitDeleteBranch { .. } => format!(
                "Delete branch {}? git will refuse if it is not merged.",
                names.join(", ")
            ),
            Self::GitDeleteTag { .. } => format!(
                "Delete tag {}? The commit it names stays where it is.",
                names.join(", ")
            ),
            // **By count, because there is no name** (§7.1.3's "by name, always"
            // read for a subject that has none): every other request on this list
            // names a file or a ref, and what this one deletes is a pane's own
            // past, which is not called anything. The number is the honest
            // substitute — it is the one thing about the transcript a reader
            // cannot see from where they are standing, since the whole of what
            // makes this row dangerous is the part that has scrolled out of
            // sight. The second sentence is the mock-up's own (8250).
            Self::ClearScrollback(_) => format!(
                "{} of past output is deleted. Search over it will find nothing.",
                names.join(", ")
            ),
        }
    }
}

impl DirtyGate {
    pub fn request(&self) -> Option<&GateRequest> {
        self.open.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn open(&mut self, request: GateRequest) {
        self.open = Some(request);
        self.hover = None;
    }

    /// Put it away and hand back what it was asking about.
    pub fn take(&mut self) -> Option<GateRequest> {
        self.hover = None;
        self.open.take()
    }

    pub fn set_hover(&mut self, hover: Option<GateTarget>) -> bool {
        let hover = self.open.is_some().then_some(hover).flatten();
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(&self) -> Option<GateTarget> {
        self.hover
    }
}

/// The answer a press on `target` gives, if it gives one at all.
#[must_use]
pub fn gate_answer(target: GateTarget) -> Option<GateAnswer> {
    match target {
        GateTarget::Discard => Some(GateAnswer::Discard),
        GateTarget::Cancel => Some(GateAnswer::Cancel),
        GateTarget::Panel => None,
    }
}

/// Everything the gate draws that had to be measured with a real font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GateContent {
    /// What the gate is asking — [`GateRequest::title`].
    ///
    /// Carried rather than read from a constant at the draw, because the gate now
    /// asks four questions and only three of them are about an unsaved buffer.
    pub title: &'static str,
    /// [`GateRequest::message`], already broken to lines that fit
    /// [`content_width`].
    pub message_lines: Vec<String>,
    /// The word on the destructive button — [`GateRequest::answer_text`].
    ///
    /// Beside its width rather than derived from it, because the two travel
    /// together: what the button says and how wide the box has to be are one
    /// measurement made once, and a layout that carried only the number would be
    /// a layout the painter had to guess the word for.
    pub discard_text: &'static str,
    pub cancel_text_width: f32,
    pub discard_text_width: f32,
}

/// Every rectangle the gate draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct GateLayout {
    scale: f32,
    frame: [f32; 4],
    title: [f32; 4],
    /// The words in [`Self::title`]'s box, carried from the request.
    title_text: &'static str,
    message: Vec<(String, [f32; 4])>,
    cancel: [f32; 4],
    discard: [f32; 4],
    /// The words in [`Self::discard`]'s box, carried for [`Self::title_text`]'s
    /// reason.
    discard_text: &'static str,
}

/// Where every part of the gate lands in a window this size.
#[must_use]
pub fn gate_layout(
    content: &GateContent,
    surface_width: f32,
    surface_height: f32,
    scale: f32,
) -> GateLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let width = dialog_width(surface_width, scale);
    let button_height =
        2.0 * border + px(2.0 * BUTTON_PADDING_Y_LOGICAL_PX + BUTTON_LINE_LOGICAL_PX);
    let height = (2.0 * border
        + px(DIALOG_PADDING_TOP_LOGICAL_PX)
        + px(TITLE_LINE_LOGICAL_PX + TITLE_MARGIN_BOTTOM_LOGICAL_PX)
        + content.message_lines.len() as f32 * px(SUB_LINE_LOGICAL_PX)
        + px(SUB_MARGIN_BOTTOM_LOGICAL_PX)
        + button_height
        + px(DIALOG_PADDING_BOTTOM_LOGICAL_PX))
    .round();

    let left = ((surface_width - width) / 2.0).round();
    let top = ((surface_height - height) / 2.0).round();
    let frame = [left, top, left + width, top + height];

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
    let message = content
        .message_lines
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
    cursor += content.message_lines.len() as f32 * px(SUB_LINE_LOGICAL_PX)
        + px(SUB_MARGIN_BOTTOM_LOGICAL_PX);

    // `justify-content: flex-end`, and **the destructive answer is the one on
    // the right without being the primary one**: it is where the eye goes last,
    // it is not the button Enter presses, and it is not painted in the accent —
    // an accent-filled `Discard` would be the window recommending the one action
    // it cannot undo.
    let button_width =
        |text_width: f32| 2.0 * border + 2.0 * px(BUTTON_PADDING_X_LOGICAL_PX) + text_width;
    let discard = [
        content_right - button_width(content.discard_text_width),
        cursor,
        content_right,
        cursor + button_height,
    ];
    let cancel = [
        discard[0] - px(ACTIONS_GAP_LOGICAL_PX) - button_width(content.cancel_text_width),
        cursor,
        discard[0] - px(ACTIONS_GAP_LOGICAL_PX),
        cursor + button_height,
    ];
    GateLayout {
        scale,
        frame,
        title,
        title_text: content.title,
        message,
        cancel,
        discard,
        discard_text: content.discard_text,
    }
}

/// What a point is over. **Always an answer**, unlike the restore prompt's: this
/// one is modal, so a press outside it is still the gate's and is swallowed.
#[must_use]
pub fn gate_hit(layout: &GateLayout, x: f64, y: f64) -> GateTarget {
    let (x, y) = (x as f32, y as f32);
    if contains(layout.cancel, x, y) {
        return GateTarget::Cancel;
    }
    if contains(layout.discard, x, y) {
        return GateTarget::Discard;
    }
    GateTarget::Panel
}

/// The gate as one overlay layer, **scrim and all**.
///
/// The scrim is the difference between this and the prompt above it, and it is
/// the honest one: a restore prompt floats over a window that already works, and
/// a gate stands in front of an action that is about to happen. Everything else
/// on this surface is that module's craft, unchanged.
#[must_use]
pub fn gate_build(
    layout: &GateLayout,
    surface: (f32, f32),
    hover: Option<GateTarget>,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let mut quads = vec![OverlayQuad {
        rect: [0.0, 0.0, surface.0, surface.1],
        color: palette.modal_scrim,
        alpha: alpha(palette.modal_scrim_alpha),
    }];
    let mut labels = Vec::new();

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
        text: layout.title_text.to_owned(),
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
    for (text, rect) in &layout.message {
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
    push_button(
        &mut quads,
        &mut labels,
        layout.cancel,
        GATE_CANCEL_TEXT,
        false,
        hover == Some(GateTarget::Cancel),
        scale,
        border,
        palette,
    );
    push_button(
        &mut quads,
        &mut labels,
        layout.discard,
        layout.discard_text,
        false,
        hover == Some(GateTarget::Discard),
        scale,
        border,
        palette,
    );
    vec![OverlayLayer {
        quads,
        labels,
        ..Default::default()
    }]
}

// ── the PSReadLine invitation (§7.1.6c-3b) ──────────────────────────────────
//
// The third dialog on this surface and the third of one craft: the same
// `push_float_window` face, the same `.btn` pair, the same padding, the same
// wrap. It lives here rather than in `psreadline.rs` for the reason the gate
// lives here rather than in `preview_edit.rs` — the twelve geometry constants
// above are this module's, `push_button` and `push_float_window` are private to
// it, and a fourth surface that copied them into another file is a fourth
// surface that would drift the first time one of them moved. `psreadline.rs`
// owns everything this dialog is *about*: the probe, the trigger table, the
// bytes and the two verbs.
//
// What differs from the gate is one thing, and it is the reason this is not a
// fifth `GateRequest`: the affirmative answer can be **unavailable**. A gate's
// destructive answer is always pressable and never recommended; this dialog's
// constructive answer is recommended, and on a machine whose execution policy
// would refuse the module it is dark with a sentence saying why. `GateRequest`
// has nowhere to put either fact.

/// What a press on the invitation answers.
///
/// `Panel` for the dialog's own face **and** for a disabled Install, which is
/// deliberate: a control that cannot act must not report that it was pressed,
/// and routing it to the same nothing the face routes to is how that is spelled
/// once instead of at every call site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InviteTarget {
    Panel,
    Decline,
    Install,
}

/// Everything the invitation draws that had to be measured with a real font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InviteContent {
    pub title: &'static str,
    /// The body, already broken to lines that fit [`content_width`].
    pub message_lines: Vec<String>,
    /// Why Install is dark, wrapped the same way. **Empty when it is not.**
    ///
    /// A separate field and not a last paragraph of the body, because the two
    /// are different kinds of sentence: the body is about the offer and is true
    /// on every machine, and this is about *this* machine and appears with the
    /// state it describes.
    pub reason_lines: Vec<String>,
    pub decline_text: &'static str,
    pub install_text: &'static str,
    pub install_enabled: bool,
    pub decline_text_width: f32,
    pub install_text_width: f32,
}

/// Every rectangle the invitation draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct InviteLayout {
    scale: f32,
    frame: [f32; 4],
    title: [f32; 4],
    title_text: &'static str,
    message: Vec<(String, [f32; 4])>,
    reason: Vec<(String, [f32; 4])>,
    decline: [f32; 4],
    install: [f32; 4],
    decline_text: &'static str,
    install_text: &'static str,
    install_enabled: bool,
}

/// Where every part of the invitation lands in a window this size.
#[must_use]
pub fn invite_layout(
    content: &InviteContent,
    surface_width: f32,
    surface_height: f32,
    scale: f32,
) -> InviteLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let width = dialog_width(surface_width, scale);
    let button_height =
        2.0 * border + px(2.0 * BUTTON_PADDING_Y_LOGICAL_PX + BUTTON_LINE_LOGICAL_PX);
    let body_lines = content.message_lines.len() + content.reason_lines.len();
    let height = (2.0 * border
        + px(DIALOG_PADDING_TOP_LOGICAL_PX)
        + px(TITLE_LINE_LOGICAL_PX + TITLE_MARGIN_BOTTOM_LOGICAL_PX)
        + body_lines as f32 * px(SUB_LINE_LOGICAL_PX)
        + px(SUB_MARGIN_BOTTOM_LOGICAL_PX)
        + button_height
        + px(DIALOG_PADDING_BOTTOM_LOGICAL_PX))
    .round();

    let left = ((surface_width - width) / 2.0).round();
    let top = ((surface_height - height) / 2.0).round();
    let frame = [left, top, left + width, top + height];

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

    let stack = |lines: &[String], cursor: &mut f32| -> Vec<(String, [f32; 4])> {
        let placed = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let line_top = *cursor + index as f32 * px(SUB_LINE_LOGICAL_PX);
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
        *cursor += lines.len() as f32 * px(SUB_LINE_LOGICAL_PX);
        placed
    };
    let message = stack(&content.message_lines, &mut cursor);
    let reason = stack(&content.reason_lines, &mut cursor);
    cursor += px(SUB_MARGIN_BOTTOM_LOGICAL_PX);

    // `justify-content: flex-end`, and here the **constructive** answer is the
    // one on the right and the one in the accent — the mirror of the gate, and
    // for the same reason read the other way: the window may recommend the
    // action that can be undone from a settings row, and must not recommend the
    // one that cannot.
    let button_width =
        |text_width: f32| 2.0 * border + 2.0 * px(BUTTON_PADDING_X_LOGICAL_PX) + text_width;
    let install = [
        content_right - button_width(content.install_text_width),
        cursor,
        content_right,
        cursor + button_height,
    ];
    let decline = [
        install[0] - px(ACTIONS_GAP_LOGICAL_PX) - button_width(content.decline_text_width),
        cursor,
        install[0] - px(ACTIONS_GAP_LOGICAL_PX),
        cursor + button_height,
    ];
    InviteLayout {
        scale,
        frame,
        title,
        title_text: content.title,
        message,
        reason,
        decline,
        install,
        decline_text: content.decline_text,
        install_text: content.install_text,
        install_enabled: content.install_enabled,
    }
}

/// What a point is over. **Always an answer**, like the gate's: this one is
/// modal, so a press outside it is still the invitation's and is swallowed.
#[must_use]
pub fn invite_hit(layout: &InviteLayout, x: f64, y: f64) -> InviteTarget {
    let (x, y) = (x as f32, y as f32);
    if contains(layout.decline, x, y) {
        return InviteTarget::Decline;
    }
    if layout.install_enabled && contains(layout.install, x, y) {
        return InviteTarget::Install;
    }
    InviteTarget::Panel
}

/// The invitation as one overlay layer, **scrim and all**.
#[must_use]
pub fn invite_build(
    layout: &InviteLayout,
    surface: (f32, f32),
    hover: Option<InviteTarget>,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let mut quads = vec![OverlayQuad {
        rect: [0.0, 0.0, surface.0, surface.1],
        color: palette.modal_scrim,
        alpha: alpha(palette.modal_scrim_alpha),
    }];
    let mut labels = Vec::new();

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
        text: layout.title_text.to_owned(),
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
    for (text, rect) in &layout.message {
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
    // A rung quieter than the body, because it is an aside about this machine
    // and the body is the offer.
    for (text, rect) in &layout.reason {
        labels.push(ChromeLabel {
            text: text.clone(),
            rect: *rect,
            font_size_px: px(SUB_FONT_LOGICAL_PX),
            color: palette.title_text_muted,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
    push_button(
        &mut quads,
        &mut labels,
        layout.decline,
        layout.decline_text,
        false,
        hover == Some(InviteTarget::Decline),
        scale,
        border,
        palette,
    );
    push_button_enabled(
        &mut quads,
        &mut labels,
        layout.install,
        layout.install_text,
        true,
        hover == Some(InviteTarget::Install),
        layout.install_enabled,
        scale,
        border,
        palette,
    );
    vec![OverlayLayer {
        quads,
        labels,
        ..Default::default()
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window the mock-up was measured in, and the shape every geometry
    /// claim below is stated against.
    const SURFACE: (f32, f32) = (1440.0, 756.0);

    /// PIN (P123-P125, `DESIGN.md` §7.1.3) — **the gate names every buffer it is
    /// about, and its default answer changes nothing.**
    ///
    /// Two rulings, one surface. The sentence is the mock-up's own
    /// `Discard unsaved changes to a.txt, b.md?`, and it is by name because a
    /// gate that said "some files" would be asking you to guess what you are
    /// about to lose — the same silence it exists to break, one sentence on. And
    /// the focused button is `Cancel`, which is what makes Enter and Esc safe: on
    /// a question about losing work, the answer that is easiest to give by
    /// accident must be the one that costs nothing.
    ///
    /// MUTATIONS:
    /// ① make `GATE_FOCUSED_ANSWER` `Discard` — the second assertion goes red,
    ///    and a stray Enter throws the buffers away;
    /// ② give `Discard` the accent fill `push_button`'s `primary` draws — the
    ///    last assertion goes red, which is the window recommending the one
    ///    action it cannot undo;
    /// ③ have `gate_hit` return `None` off the dialog — the gate stops being
    ///    modal, and a press behind the scrim reaches the thing it is guarding.
    #[test]
    fn the_dirty_gate_names_what_it_is_about_and_defaults_to_keeping_it() {
        assert_eq!(
            gate_message(&["a.txt".to_owned(), "b.md".to_owned()]),
            "Discard unsaved changes to a.txt, b.md?"
        );
        assert_eq!(
            gate_message(&["only.rs".to_owned()]),
            "Discard unsaved changes to only.rs?"
        );
        assert_eq!(GATE_FOCUSED_ANSWER, GateAnswer::Cancel);
        assert_eq!(gate_answer(GateTarget::Cancel), Some(GateAnswer::Cancel));
        assert_eq!(gate_answer(GateTarget::Discard), Some(GateAnswer::Discard));
        assert_eq!(
            gate_answer(GateTarget::Panel),
            None,
            "the dialog's own body answers nothing"
        );

        let content = GateContent {
            title: GATE_TITLE_TEXT,
            message_lines: vec!["Discard unsaved changes to a.txt?".to_owned()],
            discard_text: GATE_DISCARD_TEXT,
            cancel_text_width: 40.0,
            discard_text_width: 48.0,
        };
        let layout = gate_layout(&content, SURFACE.0, SURFACE.1, 1.0);
        let centre = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let (x, y) = centre(layout.cancel);
        assert_eq!(
            gate_hit(&layout, f64::from(x), f64::from(y)),
            GateTarget::Cancel
        );
        let (x, y) = centre(layout.discard);
        assert_eq!(
            gate_hit(&layout, f64::from(x), f64::from(y)),
            GateTarget::Discard
        );
        // **Modal**: a press anywhere else is still the gate's.
        assert_eq!(gate_hit(&layout, 1.0, 1.0), GateTarget::Panel);
        // `Discard` sits to the right of `Cancel` and is not the primary button —
        // it is where the eye goes last, and it wears no accent.
        assert!(layout.cancel[2] <= layout.discard[0]);
        let palette = chrome_palette();
        let layers = gate_build(&layout, SURFACE, None);
        assert!(
            !layers[0]
                .quads
                .iter()
                .any(|quad| quad.color == palette.accent),
            "nothing on this dialog recommends the destructive answer"
        );
        assert!(
            layers[0]
                .quads
                .iter()
                .any(|quad| quad.color == palette.modal_scrim && quad.rect[2] >= SURFACE.0),
            "and it dims, because it stands in front of something already happening"
        );
    }

    /// PIN — the gate holds one question at a time and hands it back whole.
    ///
    /// Mutation: make `take` clone rather than take, and the verb it interrupted
    /// re-raises the gate it just answered — forever.
    #[test]
    fn the_gate_holds_one_question_and_gives_it_up_when_answered() {
        let mut gate = DirtyGate::default();
        assert!(!gate.is_open());
        assert_eq!(gate.take(), None);
        gate.open(GateRequest::CloseTab(2));
        assert!(gate.is_open());
        assert_eq!(gate.request(), Some(&GateRequest::CloseTab(2)));
        assert!(gate.set_hover(Some(GateTarget::Discard)));
        assert_eq!(gate.hover(), Some(GateTarget::Discard));
        assert_eq!(gate.take(), Some(GateRequest::CloseTab(2)));
        assert!(!gate.is_open());
        assert_eq!(gate.hover(), None, "a shut gate is over nothing");
        // A hover cannot be set on a gate that is not up.
        assert!(!gate.set_hover(Some(GateTarget::Cancel)));
    }

    /// PIN — **a restore row is named by its own profile**, not by the default
    /// one, whenever nothing else named it.
    ///
    /// Red gate, and it was caught on a real machine rather than reasoned about:
    /// the prompt came back listing three tabs wearing the Ubuntu, the Git and
    /// the Command Prompt marks, and captioned every one of them `PowerShell`.
    /// The mark and the word sat side by side on one row naming two different
    /// shells — half an identity contradicting the other half, which is exactly
    /// what `docs/UI-UX.md` §126-137 says a session's identity may never do.
    ///
    /// This row is where the last layer of the name chain is most exposed, and
    /// the reason is structural rather than bad luck: a restore row has **no**
    /// program title by construction (mock-up 7480 — the program's title left
    /// with the program), so a shell that never reported a folder falls straight
    /// through manual name and OSC 2 and OSC 7 to the profile. Three of the four
    /// profiles ship without shell integration, so "never reported a folder" is
    /// their ordinary state rather than an edge case.
    #[test]
    fn a_restore_row_falls_back_to_its_own_profile_s_name_and_never_the_default_s() {
        for profile in profiles::shipped() {
            let row = RestoreRow::from_seed(
                &Seed::Term {
                    profile_id: profile.id.clone(),
                    // The case the bug lived in: no folder was ever reported, so
                    // there is nothing under the profile to catch the name.
                    cwd: String::new(),
                    manual_name: None,
                },
                1,
            );
            assert_eq!(row.mark, profile.mark);
            assert_eq!(
                row.label, profile.display_title,
                "a {} row must not be captioned with another profile's name",
                profile.id
            );
        }

        // The layers above it still win, and still for every profile: a folder
        // names the row when the shell reported one, and your own name beats
        // both. Otherwise "fall back to the profile" would quietly become
        // "always show the profile".
        let git = profiles::id(index_of_id("gitbash"));
        assert_eq!(
            RestoreRow::from_seed(
                &Seed::Term {
                    profile_id: git.clone(),
                    cwd: r"C:\work\repo".to_owned(),
                    manual_name: None,
                },
                1,
            )
            .label,
            "repo",
            "the folder it stood in outranks the profile"
        );
        assert_eq!(
            RestoreRow::from_seed(
                &Seed::Term {
                    profile_id: git.clone(),
                    cwd: r"C:\work\repo".to_owned(),
                    manual_name: Some("build".to_owned()),
                },
                1,
            )
            .label,
            "build",
            "and your own name outranks the folder"
        );
    }

    /// The two rows and the three-line paragraph the measurement was taken with,
    /// at 1x. The widths are the mock-up's own renderer's, so a rectangle
    /// computed from them is comparable with the one it reported.
    ///
    /// The paragraph is input rather than a claim about where [`wrap`] breaks —
    /// what the stack below it depends on is that there are *three* lines, and
    /// that survived the rename: `Folio` is nine characters shorter than the name
    /// it replaced, which moves both breaks and leaves the count where it was
    /// (135 characters over a line that holds about 58 is three lines under any
    /// greedy rule). Had it become two, the 232.75 the height pin reports would
    /// have had to move with it.
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
                "These were open when you last closed Folio. They come back".to_owned(),
                "in the folders you left them, as new shells — the output".to_owned(),
                "is not ours to keep.".to_owned(),
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
        assert_eq!(title_text(), "Reopen your other tabs?");

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
        assert_eq!(decline_text(), "No thanks");
        assert_eq!(restore_text(), "Restore");
        assert_eq!(
            sub_text(),
            "These were open when you last closed Folio. They come back \
in the folders you left them, as new shells."
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
        assert_eq!(
            named.mark,
            profiles::mark(0),
            "a profile's icon is its mark"
        );
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
        assert_eq!(stranger.mark, profiles::mark(0));
    }

    /// PIN (i18n slice, 2026-08-17) — **Chinese wraps, and it wraps between
    /// ideographs.**
    ///
    /// Red gate: this paragraph in Chinese contains exactly four spaces and is
    /// forty characters long, so the wrapper that only knew about whitespace
    /// returned it as one line four hundred units wide and drew it straight off
    /// both edges of a 400px dialog. Every assertion below fails against that
    /// wrapper.
    ///
    /// The ruler is ten units a character so the arithmetic is visible: at 100
    /// units a line holds ten characters, and the interesting question is only
    /// ever *where* the tenth one ends.
    #[test]
    fn a_chinese_paragraph_breaks_between_ideographs_and_never_onto_a_full_stop() {
        let ruler = |text: &str| text.chars().count() as f32 * 10.0;

        let lines = wrap("重新打开你的其他标签", 40.0, ruler);
        assert_eq!(
            lines,
            vec!["重新打开", "你的其他", "标签"],
            "a run of ideographs breaks between any two of them"
        );

        // Rule 2: no line may begin with closing punctuation. 签 and 。 are one
        // atom, so the greedy fill stops six characters in rather than putting a
        // lone full stop at the head of the second line.
        let lines = wrap("重新打开你的标签。", 60.0, ruler);
        assert_eq!(
            lines,
            vec!["重新打开你的", "标签。"],
            "the full stop stays with the character it follows"
        );
        for line in &lines {
            assert!(
                !line.starts_with(['。', '，', '、', '！', '？']),
                "{line:?} begins with punctuation that has nothing to close"
            );
        }

        // Rule 3: no line may end with opening punctuation.
        let lines = wrap("先看这个（新建标签）好吗", 50.0, ruler);
        for line in &lines {
            assert!(
                !line.ends_with(['（', '《', '「', '『']),
                "{line:?} ends with punctuation whose subject is on the next line"
            );
        }

        // Nothing is lost and nothing is invented: the concatenation is the
        // paragraph again, because a Chinese break inserts no space.
        let paragraph = crate::i18n::Text::RestoreSub.in_lang(crate::i18n::Lang::Chinese);
        let lines = wrap(paragraph, 260.0, ruler);
        assert!(
            lines.len() > 1,
            "the restore prompt's own sentence wraps at the dialog's measure"
        );
        for line in &lines {
            assert!(
                ruler(line) <= 260.0,
                "{line:?} is {} wide and the box is 260",
                ruler(line)
            );
        }
        let rejoined: String = lines
            .iter()
            .flat_map(|line| line.chars())
            .filter(|character| !character.is_whitespace())
            .collect();
        let expected: String = paragraph
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        assert_eq!(
            rejoined, expected,
            "wrapping loses no character and adds none"
        );
    }

    /// PIN — a Latin word is still atomic, even standing next to Chinese.
    ///
    /// The failure this guards is the over-eager fix: a rule that broke wherever
    /// the script changed would also break `Folio` in half the moment it was
    /// long enough to matter, and the product's own name arriving as `Fol` /
    /// `io` is worse than the line it was trying to fit.
    #[test]
    fn a_latin_word_beside_chinese_is_still_broken_only_at_its_edges() {
        let ruler = |text: &str| text.chars().count() as f32 * 10.0;
        let lines = wrap("重启Folio以切换", 50.0, ruler);
        assert_eq!(
            lines,
            vec!["重启", "Folio", "以切换"],
            "the name moves to a line of its own rather than being cut"
        );
        for line in &lines {
            assert!(
                !line.contains("Fol") || line.contains("Folio"),
                "{line:?} carries a fragment of a Latin word"
            );
        }
    }

    /// PIN — the paragraph breaks at spaces and only at spaces, greedily, which
    /// is what `overflow-wrap: normal` does. Measured with a ruler of exactly
    /// ten units a character so the breaks are arithmetic rather than a font.
    ///
    /// **Unchanged by the CJK pass**, which is the half of that pass worth
    /// pinning here: English has no break opportunity inside a word, so every
    /// line below is the line this function drew before it had ever seen an
    /// ideograph.
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
            wrap(sub_text(), 4000.0, ruler).len(),
            1,
            "a wide enough box takes it whole"
        );
        // And the sentence really is three lines at the mock-up's own width:
        // 354 logical px at 12.5px, which this ruler stands in for by measuring
        // the same words the browser did.
        let lines = wrap(sub_text(), 530.0, ruler);
        assert!(lines.len() > 1, "at a realistic width it wraps");
        assert_eq!(
            lines.join(" "),
            sub_text(),
            "wrapping loses no word and adds none"
        );
    }

    /// PIN (§7.1.6c-3b) — **a path breaks after its separators, and only
    /// after them.**
    ///
    /// The PSReadLine invitation is the first of this window's sentences to name
    /// a place on disk, and a Windows module path is one space-less token
    /// eighty characters long with no CJK in it — so rule 1 does not reach it
    /// and the paragraph drew a line running out of both edges of a 400px
    /// dialog. It is the same failure Chinese had, arriving in a second script.
    ///
    /// MUTATIONS: (1) break *before* the separator instead and the second line
    /// starts with a backslash, which reads as a UNC share; (2) allow a break
    /// between two adjacent separators and a UNC root is cut in half; (3) drop
    /// the rule and the first assertion is one 39-character line.
    #[test]
    fn a_path_may_end_a_line_after_a_separator_and_never_before_one() {
        let ruler = |text: &str| text.chars().count() as f32 * 10.0;
        assert_eq!(
            wrap(r"C:\Users\me\Documents\PSReadLine", 130.0, ruler),
            vec![r"C:\Users\me\", r"Documents\", "PSReadLine"],
            "greedy, and every line ends on the separator rather than starting on one"
        );
        assert_eq!(
            wrap(r"\\server\share\folio.ps1", 130.0, ruler),
            vec![r"\\server\", r"share\", "folio.ps1"],
            "a UNC root is two separators and is never split between them — the \
             line ends after the pair, never inside it"
        );
        assert_eq!(
            wrap("a/b/c", 100.0, ruler),
            vec!["a/b/c"],
            "the rule is a break *opportunity*; a path that fits is one line"
        );
        assert_eq!(
            wrap("one two three four", 100.0, ruler),
            vec!["one two", "three four"],
            "and prose with no separator in it is untouched by the new rule"
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

        let title = label(title_text());
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
            .find(|label| label.text == restore_text())
            .expect("the primary button is named");
        assert_eq!(
            restore.color, BUTTON_PRIMARY_INK,
            ".btn.primary color: #fff"
        );
        assert!(restore.align_center, "a button's caption is centred on it");
        let decline = rest
            .labels
            .iter()
            .find(|label| label.text == decline_text())
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

    /// PIN (v2 ④) — **the two ref deletions go through this gate**, and the gate
    /// says what it is about in the words of the thing it is about.
    ///
    /// Three facts, and each of them is a way a confirmation goes wrong:
    /// ① the title names the *kind* of thing, so a reader who reached the dialog
    ///    by accident can tell a branch from a file at a glance;
    /// ② the sentence names the thing itself and says what git will do — `-d`
    ///    refuses an unmerged branch, and a gate that hid that would be
    ///    promising an outcome it does not control;
    /// ③ the button carries the row's own verb, because a dialog headed "Delete
    ///    this branch?" whose only other button says `Discard` is two words for
    ///    one act.
    ///
    /// MUTATION: answer `GATE_DISCARD_TEXT` for either new request and ③ goes
    /// red; drop the "will refuse" clause and ② does.
    #[test]
    fn a_ref_deletion_asks_before_it_happens_and_says_what_git_will_do() {
        let root = std::path::PathBuf::from(r"D:\repo");
        let branch = GateRequest::GitDeleteBranch {
            root: root.clone(),
            name: "goner".to_owned(),
        };
        let tag = GateRequest::GitDeleteTag {
            root,
            name: "v0.9".to_owned(),
        };
        assert_eq!(branch.title(), GATE_GIT_DELETE_BRANCH_TITLE);
        assert_eq!(tag.title(), GATE_GIT_DELETE_TAG_TITLE);
        let names = vec!["goner".to_owned()];
        let sentence = branch.message(&names);
        assert!(sentence.contains("goner"), "by name, always: {sentence}");
        assert!(
            sentence.contains("not merged"),
            "and it says what -d does: {sentence}"
        );
        assert!(tag.message(&["v0.9".to_owned()]).contains("v0.9"));
        assert_eq!(branch.answer_text(), GATE_DELETE_TEXT);
        assert_eq!(tag.answer_text(), GATE_DELETE_TEXT);
        assert_eq!(
            GateRequest::Shut.answer_text(),
            GATE_DISCARD_TEXT,
            "and the three the gate was built for are unchanged"
        );
        assert_eq!(
            GateRequest::GitDiscard {
                seat: bt_layout::SeatId(1),
                path: "a.txt".to_owned(),
                untracked: false,
            }
            .answer_text(),
            GATE_DISCARD_TEXT
        );
        // **Enter still changes nothing**, whichever question is being asked —
        // the one rule a gate may never get wrong.
        assert_eq!(GATE_FOCUSED_ANSWER, GateAnswer::Cancel);

        // And the word travels into the layout, so the button that appears is
        // the button the request asked for.
        let content = GateContent {
            title: branch.title(),
            message_lines: vec![sentence],
            discard_text: branch.answer_text(),
            cancel_text_width: 40.0,
            discard_text_width: 44.0,
        };
        let layout = gate_layout(&content, SURFACE.0, SURFACE.1, 1.0);
        let layer = gate_build(&layout, SURFACE, None)
            .into_iter()
            .next()
            .expect("the gate draws one layer");
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == GATE_DELETE_TEXT),
            "the button says Delete: {:?}",
            layer.labels.iter().map(|l| &l.text).collect::<Vec<_>>()
        );
        assert!(
            layer
                .labels
                .iter()
                .all(|label| label.text != GATE_DISCARD_TEXT),
            "and never Discard beside it"
        );
    }
}
