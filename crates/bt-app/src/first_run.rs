//! The first-run card — every question whose answer writes something outside
//! `%APPDATA%\Folio`, asked once, on a machine that has never run Folio.
//!
//! Spec authority is `docs/DESIGN.md` §7.56 (user ruling, 2026-09-06). The four
//! questions are not preferences: theme, font, size, language and layout are
//! wrong for one click and cost nothing while they are wrong, and each of these
//! four leaves a mark on a file or a registry the reader did not open. They are
//! asked **together**, because they are one decision — how much of this machine
//! may Folio touch — that the product otherwise spread across a strip that
//! appears minutes later, three Settings rows nobody visits and a Windows 11 row
//! most readers never learn exists.
//!
//! **What this module owns is the card's own facts**: whether it is due, which
//! rows a machine can honour, what a press or a key does to the answers, where
//! every rectangle lands, and what `Done` spends. What it deliberately does not
//! own is any of the *doing*: every row's answer leaves here as a
//! [`crate::settings::SettingsTarget`] — literally the press the Settings dialog
//! sends — so the card cannot grow a second way to install anything. See
//! [`settings_target`], which is where that is spelled once instead of at six
//! call sites.
//!
//! **The geometry is `restore.rs`'s craft in Settings' row shape.** The same
//! [`crate::settings::push_float_window`] face, the same `.btn` pair, the same
//! `restore::wrap`. What is new here and nowhere else is a **body that
//! scrolls**: six rows of sentence do not fit under a title on a small window,
//! and the two verbs and the one promise are pinned so that the thing which
//! scrolls away is never the way out.

use std::path::PathBuf;

use bt_persist::FirstRunCardV1;
use bt_render::{
    ChromeLabel, ChromeLabelWeight, FLOAT_WINDOW_BORDER_LOGICAL_PX, FLOAT_WINDOW_RADIUS_LOGICAL_PX,
    FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad, chrome_palette, rounded_overlay_fill,
    rounded_overlay_halo, rounded_overlay_shadow,
};

use crate::{
    i18n::Text,
    marks::OverlayLayer,
    settings::{SettingsRow, SettingsTarget, push_float_window},
    shell_integration,
};

// ── when it appears ────────────────────────────────────────────────────────

/// Whether this launch owes the reader the card.
///
/// **Two facts, and both of them have to be true.** The stored state alone would
/// raise the card over a `settings.json` that exists and could not be parsed —
/// a damaged file falls back to defaults, and a default `NotShown` there is not
/// a new machine, it is a reader whose file this build could not read. The
/// missing-file fact alone would raise it again on the next launch of a run that
/// was killed before the first write landed. Together they say what the ruling
/// says: no `settings.json`, and the card has never been up.
#[must_use]
pub fn due(settings_file_was_missing: bool, stored: FirstRunCardV1) -> bool {
    settings_file_was_missing && stored == FirstRunCardV1::NotShown
}

// ── the rows a machine can honour ──────────────────────────────────────────

/// Which of the six questions a row is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowKind {
    /// The update check — the one row that arrives **on**, and the only one
    /// whose answer the card writes into Folio's own settings.
    Update,
    /// Explorer's right-click menu. One switch, and on Windows 11 with the
    /// package beside `folio.exe` it means both registrations.
    Explorer,
    /// The PowerShell integration. The one row whose work happens later.
    PowerShell,
    Claude,
    Codex,
    Copilot,
}

/// What the Explorer switch can mean on this machine.
///
/// Not a row that argues with itself: on Windows 11 with no `folio.msix` beside
/// `folio.exe` the row is still offered, as the classic entry alone, and its
/// sentence is the Windows 10 one. The switch then means what it can mean, and
/// the sentence explaining why the other half is unavailable stays where a
/// sentence of that shape belongs — on the Settings page
/// (`Text::DescExplorerFirstPageNoPackage`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExplorerShape {
    /// The first page **and** the classic entry, both from one switch.
    FirstPageAndClassic,
    /// The classic entry alone — Windows 10, and a Windows 11 with no package
    /// beside the executable.
    #[default]
    ClassicOnly,
}

impl ExplorerShape {
    /// Which sentence this shape reads under its title.
    #[must_use]
    pub fn description(self) -> Text {
        match self {
            Self::FirstPageAndClassic => Text::FirstRunDescExplorer11,
            Self::ClassicOnly => Text::FirstRunDescExplorer10,
        }
    }
}

/// The facts about this machine the card is built from.
///
/// Every field is something asked of the machine, never of `settings.json`:
/// three of the four answers are read off the registry and off the agents' own
/// files, and a card built from stored booleans would be a second copy of a
/// truth free to disagree with the files it is about.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Machine {
    /// Whether Windows shows a first page of its own — and whether the file
    /// that can be registered on it shipped beside `folio.exe`. One field
    /// because the row has one answer, and it is `FirstPageAndClassic` only
    /// when both halves are true.
    pub explorer_first_page_available: bool,
    /// `claude` is somewhere on the path this window searched.
    pub claude_found: bool,
    /// …and its hook is not in `~/.claude/settings.json` yet.
    pub claude_installable: bool,
    pub codex_found: bool,
    pub codex_installable: bool,
    pub copilot_found: bool,
    pub copilot_installable: bool,
    /// Whether the `$PROFILE` this machine has already loads `folio.ps1`.
    ///
    /// Known only once a PowerShell has said where its own profile is, which on
    /// a first launch it usually has not — so the honest default is `false`, and
    /// a reader whose profile turns out to already carry the line has their
    /// recorded intent cleared by the shell that reports it rather than by this
    /// row being withheld.
    pub powershell_integration_installed: bool,
}

/// One row of the card, as offered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    /// The group heading that stands **above** this row, when it opens one.
    pub group: Option<Text>,
    pub title: Text,
    pub description: Text,
    /// Whether the switch is on. The card is the only thing that changes this.
    pub on: bool,
}

/// The rows this machine is offered, in the order they are drawn.
///
/// **Row order is deliberate.** The one row that is already on opens the card,
/// so the reader is first told what Folio *does* and only then asked what it
/// *may* do — and in a body that can scroll, the only row that is already on is
/// guaranteed to be above the fold. Then the two Windows writes, then the
/// agents.
///
/// **A row Folio would have to refuse is not listed at all.** If none of the
/// three agents is found the group label and its rows are both absent, and the
/// card never says that it looked and found nothing.
#[must_use]
pub fn rows(machine: &Machine) -> Vec<Row> {
    let mut rows = vec![Row {
        kind: RowKind::Update,
        group: None,
        title: Text::FirstRunRowUpdate,
        description: Text::FirstRunDescUpdate,
        on: true,
    }];
    rows.push(Row {
        kind: RowKind::Explorer,
        group: None,
        title: Text::FirstRunRowExplorer,
        description: explorer_shape(machine).description(),
        on: false,
    });
    if !machine.powershell_integration_installed {
        rows.push(Row {
            kind: RowKind::PowerShell,
            group: None,
            title: Text::FirstRunRowPowerShell,
            description: Text::FirstRunDescPowerShell,
            on: false,
        });
    }
    let agents = [
        (
            RowKind::Claude,
            machine.claude_found && machine.claude_installable,
            Text::FirstRunRowClaude,
            Text::FirstRunDescClaude,
        ),
        (
            RowKind::Codex,
            machine.codex_found && machine.codex_installable,
            Text::FirstRunRowCodex,
            Text::FirstRunDescCodex,
        ),
        (
            RowKind::Copilot,
            machine.copilot_found && machine.copilot_installable,
            Text::FirstRunRowCopilot,
            Text::FirstRunDescCopilot,
        ),
    ];
    let mut opened = false;
    for (kind, offered, title, description) in agents {
        if !offered {
            continue;
        }
        rows.push(Row {
            kind,
            group: (!opened).then_some(Text::FirstRunAgentGroup),
            title,
            description,
            on: false,
        });
        opened = true;
    }
    rows
}

/// What the Explorer switch means here.
#[must_use]
pub fn explorer_shape(machine: &Machine) -> ExplorerShape {
    if machine.explorer_first_page_available {
        ExplorerShape::FirstPageAndClassic
    } else {
        ExplorerShape::ClassicOnly
    }
}

// ── what `Done` spends ─────────────────────────────────────────────────────

/// One thing pressing `Done` does.
///
/// **Five of the six leave here as a Settings press** ([`settings_target`]), and
/// the sixth cannot: recording an intent about a `$PROFILE` no shell has named
/// yet is not a row anybody can flip, so it has no target and says so.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Application {
    /// The one stored preference on the card, applied **both ways**: it arrives
    /// on, so a reader who turned it off has answered and the answer is `false`.
    UpdateCheck(bool),
    /// The classic entry. Only ever `true`: a row left off asks for nothing, and
    /// off is already the factory state.
    ContextMenu,
    /// The first page, beside it, on a machine whose switch means both.
    ExplorerFirstPage,
    /// Whether a pane with no integration is still offered one, applied **both
    /// ways** — §7.56's table: the card asked, so the strip does not. A row left
    /// on leaves the offer standing, because the line is being installed and the
    /// offer's own gate closes the moment it is there; a row turned off is an
    /// answer, so the strip never asks again.
    PowerShellOffer(bool),
    /// The intent itself, recorded for the first PowerShell that names its own
    /// `$PROFILE`. Not a Settings row, and never one.
    PowerShellIntent,
    ClaudeHooks,
    CodexNotify,
    CopilotHooks,
}

/// **The press the Settings dialog would send for this answer**, or `None` for
/// the one answer that is not a row.
///
/// This is the whole of the card's contract with the rest of the program: it
/// installs nothing itself, it presses rows. A test can therefore hand each of
/// these to the row reader in `crate::settings` and watch it come back as the
/// same `true`, which is the only way to state "the card and the page are one
/// door" in a form that stays true when either side moves.
#[must_use]
pub fn settings_target(application: Application) -> Option<SettingsTarget> {
    // `FORMULA_OPTIONS` is `[true, false]`, so an answer is the index of itself.
    let choice = |row, on: bool| Some(SettingsTarget::Choice(row, usize::from(!on)));
    match application {
        Application::UpdateCheck(on) => choice(SettingsRow::UpdateCheck, on),
        Application::ContextMenu => choice(SettingsRow::ContextMenu, true),
        Application::ExplorerFirstPage => choice(SettingsRow::ExplorerFirstPage, true),
        Application::PowerShellOffer(on) => choice(SettingsRow::PowerShellOffer, on),
        Application::ClaudeHooks => choice(SettingsRow::ClaudeHooks, true),
        Application::CodexNotify => choice(SettingsRow::CodexNotify, true),
        Application::CopilotHooks => choice(SettingsRow::CopilotHooks, true),
        Application::PowerShellIntent => None,
    }
}

/// Everything `Done` does, in the order it does it.
///
/// **A row that is off spends nothing**, with the two exceptions that are not
/// exceptions at all: the update check arrives on, so turning it off *is* an
/// answer, and the PowerShell offer is the one question with a second surface,
/// so leaving its row off is the answer that closes that surface too. Every
/// other row's off is the factory state and needs recording nowhere.
#[must_use]
pub fn applications(rows: &[Row], shape: ExplorerShape) -> Vec<Application> {
    let mut spent = Vec::new();
    for row in rows {
        match row.kind {
            RowKind::Update => spent.push(Application::UpdateCheck(row.on)),
            RowKind::Explorer if row.on => {
                spent.push(Application::ContextMenu);
                if shape == ExplorerShape::FirstPageAndClassic {
                    spent.push(Application::ExplorerFirstPage);
                }
            }
            RowKind::PowerShell => {
                spent.push(Application::PowerShellOffer(row.on));
                if row.on {
                    spent.push(Application::PowerShellIntent);
                }
            }
            RowKind::Claude if row.on => spent.push(Application::ClaudeHooks),
            RowKind::Codex if row.on => spent.push(Application::CodexNotify),
            RowKind::Copilot if row.on => spent.push(Application::CopilotHooks),
            RowKind::Explorer | RowKind::Claude | RowKind::Codex | RowKind::Copilot => {}
        }
    }
    spent
}

/// The card as this window holds it: whether it is up, what its switches say,
/// where the ring is, and how far the body has been scrolled.
///
/// The answers live here and nowhere else until `Done` is pressed, which is what
/// makes the four rows one decision rather than four (§7.56). A switch on the
/// Settings page acts the instant it is flipped; a switch here is read once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    open: bool,
    rows: Vec<Row>,
    shape: ExplorerShape,
    hover: Option<Target>,
    focus: Option<Focus>,
    /// `:focus-visible` — the ring is drawn for a keyboard and put away by a
    /// finger, the same half of the idea `crate::settings` keeps.
    focus_visible: bool,
    scroll: f32,
}

impl Card {
    /// Put it up, with the rows this machine can honour.
    ///
    /// **Focus opens on the first switch**, and the ring is not yet visible: a
    /// card that arrived with a ring already drawn would be claiming a keyboard
    /// nobody has used on it.
    pub fn open(&mut self, rows: Vec<Row>, shape: ExplorerShape) {
        self.open = true;
        self.hover = None;
        self.focus = (!rows.is_empty()).then_some(Focus::Switch(0));
        self.focus_visible = false;
        self.scroll = 0.0;
        self.rows = rows;
        self.shape = shape;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.hover = None;
        self.rows.clear();
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    #[must_use]
    pub fn hover(&self) -> Option<Target> {
        self.hover
    }

    /// Returns whether the drawing has to change.
    pub fn set_hover(&mut self, hover: Option<Target>) -> bool {
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    #[must_use]
    pub fn focus(&self) -> Option<Focus> {
        self.focus
    }

    /// The ring, not the focus: `None` while the keyboard arrived here by way of
    /// somebody's finger.
    #[must_use]
    pub fn focus_ring(&self) -> Option<Focus> {
        self.focus_visible.then_some(self.focus).flatten()
    }

    #[must_use]
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    pub fn scroll_to(&mut self, scroll: f32) -> bool {
        let changed = (self.scroll - scroll).abs() > f32::EPSILON;
        self.scroll = scroll;
        changed
    }

    /// A pointer press: the focus goes to what was pressed, with the ring off.
    pub fn press(&mut self, target: Target) {
        self.focus_visible = false;
        self.focus = match target {
            Target::Switch(index) => Some(Focus::Switch(index)),
            Target::OpenSettings => Some(Focus::OpenSettings),
            Target::Later => Some(Focus::Later),
            Target::Done => Some(Focus::Done),
            Target::Panel => self.focus,
        };
    }

    /// A key: the ring comes on, wherever it was.
    pub fn light_the_ring(&mut self) {
        self.focus_visible = true;
        if self.focus.is_none() && !self.rows.is_empty() {
            self.focus = Some(Focus::Switch(0));
        }
    }

    pub fn move_focus(&mut self, focus: Focus) {
        self.focus = Some(focus);
        self.focus_visible = true;
    }

    /// Flip one switch. The row is the card's own answer until `Done`.
    pub fn flip(&mut self, index: usize) -> bool {
        let Some(row) = self.rows.get_mut(index) else {
            return false;
        };
        row.on = !row.on;
        true
    }

    /// `←` and `→` — off and on for the focused switch, the platform idiom.
    pub fn set(&mut self, index: usize, on: bool) -> bool {
        let Some(row) = self.rows.get_mut(index) else {
            return false;
        };
        let changed = row.on != on;
        row.on = on;
        changed
    }

    /// Everything `Done` spends.
    #[must_use]
    pub fn done(&self) -> Vec<Application> {
        applications(&self.rows, self.shape)
    }
}

/// What `Not now`, `Esc`, `Open settings` and shutting the window spend.
///
/// **Nothing.** The card closes with the factory values — the rows off, the
/// update check on — writes nothing outside `settings.json`, and does not come
/// back. The one difference from pressing `Done` with the card untouched is the
/// PowerShell offer: `Not now` means *nothing was asked*, so the one question
/// that has a second surface keeps it, exactly as it works today.
///
/// **A window shut while the card is up spends this too, and spends it by
/// construction**: `Shown` is written when the card goes up and nothing is
/// applied until `Done` is pressed, so there is no path by which an `Alt+F4`
/// could leave half a decision behind.
#[must_use]
pub fn declined() -> Vec<Application> {
    Vec::new()
}

// ── the intent, and the shell that finally answers it ──────────────────────

/// What a recorded PowerShell intent does when a shell names its own `$PROFILE`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingStep {
    /// Nothing to do: no intent, or no shell has spoken yet.
    Wait,
    /// Write the line into this file.
    Write(PathBuf),
    /// Clear the intent without writing: the profile already loads `folio.ps1`,
    /// which is the reader having the thing they asked for.
    Clear,
}

/// The step a recorded intent takes against one shell's answer.
///
/// `offer` is [`shell_integration::offer_for`]'s reading of that file, which is
/// the same reading the strip is built on — so an intent and a strip can never
/// disagree about whether a profile carries the line.
#[must_use]
pub fn pending_step(pending: bool, profile: Option<&shell_integration::Offer>) -> PendingStep {
    if !pending {
        return PendingStep::Wait;
    }
    match profile {
        Some(shell_integration::Offer::Owed(path)) => PendingStep::Write(path.clone()),
        // Already carries the line — by another route, by hand, or by a copy
        // under a name this build does not recognise. All three are the reader
        // having what they asked for.
        Some(_) => PendingStep::Clear,
        None => PendingStep::Wait,
    }
}

/// What the Terminal page's PowerShell row says while an intent is outstanding.
///
/// `None` while there is none, which is the row's own sentence.
#[must_use]
pub fn pending_row_line(pending: bool) -> Option<Text> {
    pending.then_some(Text::ShellIntegrationPending)
}

// ── the keyboard ───────────────────────────────────────────────────────────

/// What has the ring.
///
/// **Focus opens on the first switch**, not on `Done`: this is a form, and
/// landing on the primary would let a blind `Enter` answer a card nobody has
/// read. (`restore::FOCUSED_ANSWER` puts focus on the verb because that dialog
/// is a question, not a form.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Switch(usize),
    OpenSettings,
    Later,
    Done,
}

/// The focus order, as a ring: every switch in visual order, then the link, then
/// the two verbs. The card is modal, so focus never leaves it.
#[must_use]
pub fn focus_order(switches: usize) -> Vec<Focus> {
    (0..switches)
        .map(Focus::Switch)
        .chain([Focus::OpenSettings, Focus::Later, Focus::Done])
        .collect()
}

/// Where `Tab` (or `Shift+Tab`) goes from here.
#[must_use]
pub fn stepped(focus: Focus, switches: usize, forward: bool) -> Focus {
    let order = focus_order(switches);
    let at = order.iter().position(|entry| *entry == focus).unwrap_or(0);
    let next = if forward {
        (at + 1) % order.len()
    } else {
        (at + order.len() - 1) % order.len()
    };
    order[next]
}

/// Where `↑` or `↓` goes. **The list is a list**: the arrows move between
/// switches and nowhere else, and from a button they enter the list at the end
/// they came from.
#[must_use]
pub fn arrowed(focus: Focus, switches: usize, down: bool) -> Focus {
    if switches == 0 {
        return focus;
    }
    let last = switches - 1;
    match focus {
        Focus::Switch(at) if down => Focus::Switch((at + 1).min(last)),
        Focus::Switch(at) => Focus::Switch(at.saturating_sub(1)),
        _ if down => Focus::Switch(0),
        _ => Focus::Switch(last),
    }
}

// ── geometry ───────────────────────────────────────────────────────────────

/// `min(480px, 92%)` — wider than `.restore`'s 400, which is too narrow on both
/// axes for a form of six rows.
///
/// 480 and not v1's 460: the switch column takes 42 logical pixels out of the
/// text column where a checkbox column cost 25, and 480 hands back exactly the
/// 20 pixels of sentence that moving to Settings' row shape spent.
pub const MAX_WIDTH_LOGICAL_PX: f32 = 480.0;
const WIDTH_RATIO: f32 = 0.92;

/// How much of the window's height the card may take. The head and the foot are
/// pinned inside whatever is left and the body between them scrolls.
const SURFACE_MARGIN_LOGICAL_PX: f32 = 34.0;

const PADDING_TOP_LOGICAL_PX: f32 = 22.0;
const PADDING_X_LOGICAL_PX: f32 = 22.0;
const PADDING_BOTTOM_LOGICAL_PX: f32 = 16.0;

const TITLE_FONT_LOGICAL_PX: f32 = 15.0;
const TITLE_LINE_LOGICAL_PX: f32 = 21.0;
/// **16, and then the first row.** Not `.restore`'s 5 + sub + 17: with nothing
/// between the title and the list, one step is the whole of the relationship.
const TITLE_MARGIN_BOTTOM_LOGICAL_PX: f32 = 16.0;

const ROW_FONT_LOGICAL_PX: f32 = 13.0;
const ROW_LINE_LOGICAL_PX: f32 = 19.0;
const DESC_FONT_LOGICAL_PX: f32 = 11.5;
const DESC_LINE_LOGICAL_PX: f32 = 16.5;
const DESC_MARGIN_TOP_LOGICAL_PX: f32 = 2.0;
const ROW_GAP_LOGICAL_PX: f32 = 13.0;
/// Tighter inside the agent group: three rows about one idea, under one heading.
const GROUP_ROW_GAP_LOGICAL_PX: f32 = 7.0;

const GROUP_LABEL_FONT_LOGICAL_PX: f32 = 11.0;
const GROUP_LABEL_LINE_LOGICAL_PX: f32 = 13.0;
const GROUP_LABEL_TRACKING_EM: f32 = 0.05;
const GROUP_LABEL_MARGIN_TOP_LOGICAL_PX: f32 = 11.0;
const GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX: f32 = 3.0;

/// `.aswitch`, to the pixel.
const SWITCH_WIDTH_LOGICAL_PX: f32 = 30.0;
const SWITCH_HEIGHT_LOGICAL_PX: f32 = 18.0;
const SWITCH_KNOB_LOGICAL_PX: f32 = 14.0;
const SWITCH_KNOB_INSET_LOGICAL_PX: f32 = 2.0;
const SWITCH_KNOB_SHADOW_LOGICAL_PX: f32 = 3.0;
const SWITCH_KNOB_SHADOW_INK: [u8; 3] = [0, 0, 0];
const SWITCH_KNOB_SHADOW_ALPHA: f32 = 0.25;
/// The gap between the sentence column and the switch.
const SWITCH_GAP_LOGICAL_PX: f32 = 12.0;

const HAIRLINE_MARGIN_TOP_LOGICAL_PX: f32 = 16.0;
const HAIRLINE_MARGIN_BOTTOM_LOGICAL_PX: f32 = 12.0;
const FOOTNOTE_FONT_LOGICAL_PX: f32 = 11.5;
const FOOTNOTE_LINE_LOGICAL_PX: f32 = 16.5;
const FOOTNOTE_LINK_GAP_LOGICAL_PX: f32 = 6.0;
const FOOTNOTE_MARGIN_BOTTOM_LOGICAL_PX: f32 = 14.0;
const LINK_UNDERLINE_ALPHA: f32 = 0.45;

const BUTTON_PADDING_X_LOGICAL_PX: f32 = 14.0;
const BUTTON_PADDING_Y_LOGICAL_PX: f32 = 6.0;
const BUTTON_RADIUS_LOGICAL_PX: f32 = 6.0;
const BUTTON_FONT_LOGICAL_PX: f32 = 13.0;
const BUTTON_LINE_LOGICAL_PX: f32 = 15.5;
const BUTTON_GAP_LOGICAL_PX: f32 = 8.0;
const BUTTON_PRIMARY_HOVER_BRIGHTNESS: f32 = 1.07;
const BUTTON_PRIMARY_INK: [u8; 3] = [0xff, 0xff, 0xff];

const SCROLLBAR_WIDTH_LOGICAL_PX: f32 = 3.0;
const SCROLLBAR_TRACK_ALPHA: f32 = 0.07;
const SCROLLBAR_THUMB_ALPHA: f32 = 0.26;
const SCROLLBAR_INK: [u8; 3] = [55, 53, 47];
/// The clipped row fades into the card's own ground rather than being cut, so a
/// reader is never shown half a glyph and never has to wonder whether the
/// sentence ended there.
const FADE_LOGICAL_PX: f32 = 20.0;
const FADE_BANDS: usize = 10;

const FOCUS_RING_WIDTH_LOGICAL_PX: f32 = 2.0;
const FOCUS_RING_BUTTON_OFFSET_LOGICAL_PX: f32 = 2.0;
const FOCUS_RING_TIGHT_OFFSET_LOGICAL_PX: f32 = 1.0;

/// What a press is on.
///
/// **Always an answer**, like the gate's and the invitation's: the card is
/// modal, so a press outside it is still the card's and is swallowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    /// The card's own face, its scrim, and anything else that answers nothing.
    Panel,
    Switch(usize),
    OpenSettings,
    Later,
    Done,
}

/// The width of the card, clamped the way `restore::dialog_width` clamps.
#[must_use]
pub fn card_width(surface_width: f32, scale: f32) -> f32 {
    (MAX_WIDTH_LOGICAL_PX * scale)
        .min(surface_width * WIDTH_RATIO)
        .round()
}

/// The width one row's sentence is wrapped to: the card, less its padding, less
/// the switch column and the gap before it.
#[must_use]
pub fn text_width(surface_width: f32, scale: f32) -> f32 {
    card_width(surface_width, scale)
        - 2.0 * FLOAT_WINDOW_BORDER_LOGICAL_PX.max(1.0 / scale) * scale
        - 2.0 * PADDING_X_LOGICAL_PX * scale
        - (SWITCH_WIDTH_LOGICAL_PX + SWITCH_GAP_LOGICAL_PX) * scale
}

/// One row, measured against a real font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowContent {
    pub group: Option<String>,
    pub title: String,
    /// The sentence, already broken to lines that fit [`text_width`].
    pub description_lines: Vec<String>,
    pub on: bool,
}

/// Everything the card draws that had to be measured with a real font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Content {
    pub title: String,
    pub rows: Vec<RowContent>,
    pub footnote_lines: Vec<String>,
    /// How wide the **last** footnote line is, so the link can finish that line
    /// rather than stand out on the card's own edge with a hole beside it.
    pub footnote_last_line_width: f32,
    pub open_settings: String,
    pub open_settings_width: f32,
    pub later: String,
    pub later_width: f32,
    pub done: String,
    pub done_width: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct RowRects {
    group: Option<(String, [f32; 4])>,
    title: (String, [f32; 4]),
    description: Vec<(String, [f32; 4])>,
    switch: [f32; 4],
    on: bool,
}

/// Every rectangle the card draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    scale: f32,
    frame: [f32; 4],
    title: (String, [f32; 4]),
    /// The clip the scrolling body is seen through.
    viewport: [f32; 4],
    /// How tall the body is in full, so a caller can clamp a scroll.
    body_height: f32,
    scroll: f32,
    rows: Vec<RowRects>,
    thumb: Option<[f32; 4]>,
    track: Option<[f32; 4]>,
    hairline: [f32; 4],
    footnote: Vec<(String, [f32; 4])>,
    link: (String, [f32; 4]),
    later: (String, [f32; 4]),
    done: (String, [f32; 4]),
}

impl Layout {
    /// How far the body can be scrolled before its last line is at the foot.
    #[must_use]
    pub fn scroll_extent(&self) -> f32 {
        (self.body_height - (self.viewport[3] - self.viewport[1])).max(0.0)
    }

    /// Whether the body is taller than the room it has.
    #[must_use]
    pub fn scrolls(&self) -> bool {
        self.scroll_extent() > 0.0
    }

    /// The scroll that brings a switch fully into view, or the one already in
    /// force when it is there.
    ///
    /// **`Tab` and `↓` into a row below the fold scroll it in**, which is the
    /// whole reason this is a function of the layout rather than of the press: a
    /// ring drawn outside the viewport is a focus the reader cannot see.
    #[must_use]
    pub fn scroll_showing(&self, focus: Focus) -> f32 {
        let Focus::Switch(index) = focus else {
            return self.scroll;
        };
        let Some(row) = self.rows.get(index) else {
            return self.scroll;
        };
        let top = row
            .group
            .as_ref()
            .map_or(row.title.1[1], |(_, rect)| rect[1]);
        let bottom = row
            .description
            .last()
            .map_or(row.title.1[3], |(_, rect)| rect[3])
            .max(row.switch[3]);
        let ring = FOCUS_RING_TIGHT_OFFSET_LOGICAL_PX * self.scale + FOCUS_RING_WIDTH_LOGICAL_PX;
        if top - ring < self.viewport[1] {
            return (self.scroll - (self.viewport[1] - (top - ring))).max(0.0);
        }
        if bottom + ring > self.viewport[3] {
            return (self.scroll + ((bottom + ring) - self.viewport[3])).min(self.scroll_extent());
        }
        self.scroll
    }

    /// One wheel notch, one `Page`, one `Home` or one `End`, clamped.
    #[must_use]
    pub fn scrolled_by(&self, delta: f32) -> f32 {
        (self.scroll + delta).clamp(0.0, self.scroll_extent())
    }

    /// The height of one page of the body, for `Page Up` and `Page Down`.
    #[must_use]
    pub fn page(&self) -> f32 {
        self.viewport[3] - self.viewport[1]
    }
}

/// Where every part of the card lands in a window this size.
#[must_use]
pub fn layout(
    content: &Content,
    surface_width: f32,
    surface_height: f32,
    scale: f32,
    scroll: f32,
) -> Layout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let width = card_width(surface_width, scale);

    let head = px(PADDING_TOP_LOGICAL_PX + TITLE_LINE_LOGICAL_PX + TITLE_MARGIN_BOTTOM_LOGICAL_PX);
    let button_height =
        2.0 * border + px(2.0 * BUTTON_PADDING_Y_LOGICAL_PX + BUTTON_LINE_LOGICAL_PX);
    let foot = px(HAIRLINE_MARGIN_TOP_LOGICAL_PX)
        + border
        + px(HAIRLINE_MARGIN_BOTTOM_LOGICAL_PX)
        + content.footnote_lines.len().max(1) as f32 * px(FOOTNOTE_LINE_LOGICAL_PX)
        + px(FOOTNOTE_MARGIN_BOTTOM_LOGICAL_PX)
        + button_height
        + px(PADDING_BOTTOM_LOGICAL_PX);

    let body_height = body_extent(content, scale);
    let room = (surface_height - 2.0 * px(SURFACE_MARGIN_LOGICAL_PX) - 2.0 * border - head - foot)
        .max(px(ROW_LINE_LOGICAL_PX));
    let viewport_height = body_height.min(room);
    let scroll = scroll.clamp(0.0, (body_height - viewport_height).max(0.0));

    let height = (2.0 * border + head + viewport_height + foot).round();
    let left = ((surface_width - width) / 2.0).round();
    let top = ((surface_height - height) / 2.0).round();
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + px(PADDING_X_LOGICAL_PX);
    let content_right = frame[2] - border - px(PADDING_X_LOGICAL_PX);
    let text_right = content_right - px(SWITCH_WIDTH_LOGICAL_PX + SWITCH_GAP_LOGICAL_PX);

    let mut cursor = frame[1] + border + px(PADDING_TOP_LOGICAL_PX);
    let title = (
        content.title.clone(),
        [
            content_left,
            cursor,
            content_right,
            cursor + px(TITLE_LINE_LOGICAL_PX),
        ],
    );
    cursor = title.1[3] + px(TITLE_MARGIN_BOTTOM_LOGICAL_PX);

    let viewport = [
        content_left,
        cursor,
        content_right,
        cursor + viewport_height,
    ];
    let mut rows = Vec::with_capacity(content.rows.len());
    let mut walk = viewport[1] - scroll;
    for (index, row) in content.rows.iter().enumerate() {
        if index > 0 {
            walk += px(gap_above(&content.rows, index));
        }
        let group = row.group.as_ref().map(|label| {
            walk += px(GROUP_LABEL_MARGIN_TOP_LOGICAL_PX);
            let rect = [
                content_left,
                walk,
                content_right,
                walk + px(GROUP_LABEL_LINE_LOGICAL_PX),
            ];
            walk = rect[3] + px(GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX);
            (label.clone(), rect)
        });
        let title_rect = [
            content_left,
            walk,
            text_right,
            walk + px(ROW_LINE_LOGICAL_PX),
        ];
        // **Centred on the row's first title line**, so a row whose sentence
        // runs to three lines does not drag its control down the card.
        let switch_top = (title_rect[1] + title_rect[3] - px(SWITCH_HEIGHT_LOGICAL_PX)) / 2.0;
        let switch = [
            content_right - px(SWITCH_WIDTH_LOGICAL_PX),
            switch_top,
            content_right,
            switch_top + px(SWITCH_HEIGHT_LOGICAL_PX),
        ];
        walk = title_rect[3] + px(DESC_MARGIN_TOP_LOGICAL_PX);
        let description = row
            .description_lines
            .iter()
            .map(|line| {
                let rect = [
                    content_left,
                    walk,
                    text_right,
                    walk + px(DESC_LINE_LOGICAL_PX),
                ];
                walk = rect[3];
                (line.clone(), rect)
            })
            .collect();
        rows.push(RowRects {
            group,
            title: (row.title.clone(), title_rect),
            description,
            switch,
            on: row.on,
        });
    }

    // **In the right padding, not in the sentence column**: the bar is the one
    // piece of chrome this card grows, and it grows into the margin so that no
    // line of type moves when it appears.
    let (track, thumb) = if body_height > viewport_height {
        let lane_left = (content_right
            + (px(PADDING_X_LOGICAL_PX) - px(SCROLLBAR_WIDTH_LOGICAL_PX)) / 2.0)
            .round();
        let lane = [
            lane_left,
            viewport[1],
            lane_left + px(SCROLLBAR_WIDTH_LOGICAL_PX),
            viewport[3],
        ];
        // Thumb = shown ÷ total, with a floor so a very long body still leaves
        // something to see and to aim at.
        let thumb_height = (viewport_height * viewport_height / body_height)
            .max(px(SCROLLBAR_WIDTH_LOGICAL_PX) * 4.0)
            .min(viewport_height);
        let travel = viewport_height - thumb_height;
        let at = scroll / (body_height - viewport_height);
        let thumb_top = viewport[1] + travel * at;
        (
            Some(lane),
            Some([lane[0], thumb_top, lane[2], thumb_top + thumb_height]),
        )
    } else {
        (None, None)
    };

    let mut cursor = viewport[3] + px(HAIRLINE_MARGIN_TOP_LOGICAL_PX);
    let hairline = [content_left, cursor, content_right, cursor + border];
    cursor = hairline[3] + px(HAIRLINE_MARGIN_BOTTOM_LOGICAL_PX);
    let footnote: Vec<(String, [f32; 4])> = content
        .footnote_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let line_top = cursor + index as f32 * px(FOOTNOTE_LINE_LOGICAL_PX);
            (
                line.clone(),
                [
                    content_left,
                    line_top,
                    content_right,
                    line_top + px(FOOTNOTE_LINE_LOGICAL_PX),
                ],
            )
        })
        .collect();
    // Immediately after the sentence rather than out on the card's own edge: it
    // finishes the line it belongs to.
    let last = footnote
        .last()
        .map_or([content_left, cursor, content_left, cursor], |(_, rect)| {
            *rect
        });
    let link_left = (last[0] + content.footnote_last_line_width + px(FOOTNOTE_LINK_GAP_LOGICAL_PX))
        .min(content_right - content.open_settings_width);
    let link = (
        content.open_settings.clone(),
        [
            link_left,
            last[1],
            link_left + content.open_settings_width,
            last[3],
        ],
    );
    cursor = last[3] + px(FOOTNOTE_MARGIN_BOTTOM_LOGICAL_PX);

    let button_width =
        |text_width: f32| 2.0 * border + 2.0 * px(BUTTON_PADDING_X_LOGICAL_PX) + text_width;
    let done_rect = [
        content_right - button_width(content.done_width),
        cursor,
        content_right,
        cursor + button_height,
    ];
    let later_rect = [
        done_rect[0] - px(BUTTON_GAP_LOGICAL_PX) - button_width(content.later_width),
        cursor,
        done_rect[0] - px(BUTTON_GAP_LOGICAL_PX),
        cursor + button_height,
    ];

    Layout {
        scale,
        frame,
        title,
        viewport,
        body_height,
        scroll,
        rows,
        thumb,
        track,
        hairline,
        footnote,
        link,
        later: (content.later.clone(), later_rect),
        done: (content.done.clone(), done_rect),
    }
}

/// The air above the row at `index`.
///
/// **13, and 7 inside the agent group.** A row that opens a group takes the
/// ordinary gap and then its heading's own margins on top of that; a row that
/// merely stands under one sits closer to the row above it, because three rows
/// about one idea under one heading are a block and not three neighbours.
fn gap_above(rows: &[RowContent], index: usize) -> f32 {
    let under_a_heading = rows[..index].iter().rev().any(|row| row.group.is_some());
    if rows[index].group.is_none() && under_a_heading {
        GROUP_ROW_GAP_LOGICAL_PX
    } else {
        ROW_GAP_LOGICAL_PX
    }
}

/// How tall the body is in full, before any of it is hidden.
fn body_extent(content: &Content, scale: f32) -> f32 {
    let px = |value: f32| value * scale;
    let mut height = 0.0;
    for (index, row) in content.rows.iter().enumerate() {
        if index > 0 {
            height += px(gap_above(&content.rows, index));
        }
        if row.group.is_some() {
            height += px(GROUP_LABEL_MARGIN_TOP_LOGICAL_PX
                + GROUP_LABEL_LINE_LOGICAL_PX
                + GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX);
        }
        height += px(ROW_LINE_LOGICAL_PX + DESC_MARGIN_TOP_LOGICAL_PX);
        height += row.description_lines.len() as f32 * px(DESC_LINE_LOGICAL_PX);
    }
    height
}

/// What a point is over.
#[must_use]
pub fn hit(layout: &Layout, x: f64, y: f64) -> Target {
    let (x, y) = (x as f32, y as f32);
    if contains(layout.done.1, x, y) {
        return Target::Done;
    }
    if contains(layout.later.1, x, y) {
        return Target::Later;
    }
    if contains(layout.link.1, x, y) {
        return Target::OpenSettings;
    }
    // A switch scrolled out of the viewport is not a switch anybody can press:
    // the clip is where it stops being drawn, so it is where it stops answering.
    for (index, row) in layout.rows.iter().enumerate() {
        if contains(row.switch, x, y) && contains(layout.viewport, x, y) {
            return Target::Switch(index);
        }
    }
    Target::Panel
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// `rect`, cut down to what `clip` shows of it, or `None` when nothing is left.
fn clipped(rect: [f32; 4], clip: [f32; 4]) -> Option<[f32; 4]> {
    let cut = [
        rect[0].max(clip[0]),
        rect[1].max(clip[1]),
        rect[2].min(clip[2]),
        rect[3].min(clip[3]),
    ];
    (cut[0] < cut[2] && cut[1] < cut[3]).then_some(cut)
}

/// The card as one overlay layer, **scrim and all**.
#[must_use]
pub fn build(
    layout: &Layout,
    surface: (f32, f32),
    hover: Option<Target>,
    focus: Option<Focus>,
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
        mono: false,
        text: layout.title.0.clone(),
        rect: layout.title.1,
        font_size_px: px(TITLE_FONT_LOGICAL_PX),
        color: palette.dialog_title_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: None,
    });

    let viewport = layout.viewport;
    for (index, row) in layout.rows.iter().enumerate() {
        if let Some((text, rect)) = &row.group
            && let Some(shown) = clipped(*rect, viewport)
        {
            labels.push(ChromeLabel {
                mono: false,
                text: text.clone(),
                rect: *rect,
                font_size_px: px(GROUP_LABEL_FONT_LOGICAL_PX),
                color: palette.dialog_muted_text,
                align_right: false,
                align_center: false,
                letter_spacing_em: GROUP_LABEL_TRACKING_EM,
                weight: ChromeLabelWeight::SemiBold,
                tabular_numerals: false,
                clip: Some(shown),
            });
        }
        if let Some(shown) = clipped(row.title.1, viewport) {
            labels.push(ChromeLabel {
                mono: false,
                text: row.title.0.clone(),
                rect: row.title.1,
                font_size_px: px(ROW_FONT_LOGICAL_PX),
                color: palette.dialog_title_text,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: Some(shown),
            });
        }
        for (text, rect) in &row.description {
            let Some(shown) = clipped(*rect, viewport) else {
                continue;
            };
            labels.push(ChromeLabel {
                mono: false,
                text: text.clone(),
                rect: *rect,
                font_size_px: px(DESC_FONT_LOGICAL_PX),
                color: palette.dialog_muted_text,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: Some(shown),
            });
        }
        if clipped(row.switch, viewport).is_some() {
            push_switch(&mut quads, row.switch, row.on, scale, palette, viewport);
            if focus == Some(Focus::Switch(index)) {
                quads.extend(clip_quads(
                    focus_ring(
                        row.switch,
                        scale,
                        FOCUS_RING_TIGHT_OFFSET_LOGICAL_PX,
                        palette.accent,
                    ),
                    viewport,
                ));
            }
        }
    }

    // **The fade and the bar are a layer of their own, and that is not a
    // tidiness.** A layer's fills are finished before its captions are struck,
    // so a fade pushed in here would be laid *under* the very sentence it is
    // meant to take into the ground. A second layer's three channels open after
    // this one's have closed, which is what puts it over the type — and the bar
    // over the fade, which is the order they stand in.
    let mut over = Vec::new();
    if layout.scroll > 0.0 {
        over.extend(fade(
            viewport,
            palette.dialog_surface,
            px(FADE_LOGICAL_PX),
            Edge::Top,
        ));
    }
    if layout.scroll < layout.scroll_extent() {
        over.extend(fade(
            viewport,
            palette.dialog_surface,
            px(FADE_LOGICAL_PX),
            Edge::Bottom,
        ));
    }
    if let (Some(track), Some(thumb)) = (layout.track, layout.thumb) {
        over.extend(rounded_overlay_fill(
            track,
            px(SCROLLBAR_WIDTH_LOGICAL_PX) / 2.0,
            SCROLLBAR_INK,
            SCROLLBAR_TRACK_ALPHA,
        ));
        over.extend(rounded_overlay_fill(
            thumb,
            px(SCROLLBAR_WIDTH_LOGICAL_PX) / 2.0,
            SCROLLBAR_INK,
            SCROLLBAR_THUMB_ALPHA,
        ));
    }

    quads.push(OverlayQuad {
        rect: layout.hairline,
        color: palette.menu_border,
        alpha: alpha(palette.menu_border_alpha),
    });
    for (text, rect) in &layout.footnote {
        labels.push(ChromeLabel {
            mono: false,
            text: text.clone(),
            rect: *rect,
            font_size_px: px(FOOTNOTE_FONT_LOGICAL_PX),
            color: palette.dialog_muted_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
    labels.push(ChromeLabel {
        mono: false,
        text: layout.link.0.clone(),
        rect: layout.link.1,
        font_size_px: px(FOOTNOTE_FONT_LOGICAL_PX),
        color: palette.accent,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
    // The underline a link wears, at the weight the rest of this window's links
    // wear it: present, and not as loud as the word.
    let underline_top = (layout.link.1[3] - px(FOOTNOTE_LINE_LOGICAL_PX) * 0.18).round();
    quads.push(OverlayQuad {
        rect: [
            layout.link.1[0],
            underline_top,
            layout.link.1[2],
            underline_top + border,
        ],
        color: palette.accent,
        alpha: LINK_UNDERLINE_ALPHA,
    });
    if focus == Some(Focus::OpenSettings) {
        quads.extend(focus_ring(
            layout.link.1,
            scale,
            FOCUS_RING_TIGHT_OFFSET_LOGICAL_PX,
            palette.accent,
        ));
    }

    push_button(
        &mut quads,
        &mut labels,
        layout.later.1,
        &layout.later.0,
        false,
        hover == Some(Target::Later),
        scale,
        border,
        palette,
    );
    push_button(
        &mut quads,
        &mut labels,
        layout.done.1,
        &layout.done.0,
        true,
        hover == Some(Target::Done),
        scale,
        border,
        palette,
    );
    for (ring, rect) in [(Focus::Later, layout.later.1), (Focus::Done, layout.done.1)] {
        if focus == Some(ring) {
            quads.extend(focus_ring(
                rect,
                scale,
                FOCUS_RING_BUTTON_OFFSET_LOGICAL_PX,
                palette.accent,
            ));
        }
    }

    vec![
        OverlayLayer {
            quads,
            labels,
            ..Default::default()
        },
        OverlayLayer {
            quads: over,
            ..Default::default()
        },
    ]
}

/// `.aswitch` — a 30 × 18 track with a 14 × 14 knob two pixels in.
fn push_switch(
    quads: &mut Vec<OverlayQuad>,
    rect: [f32; 4],
    on: bool,
    scale: f32,
    palette: bt_render::ChromePalette,
    clip: [f32; 4],
) {
    let px = |value: f32| value * scale;
    let track = rounded_overlay_fill(
        rect,
        px(SWITCH_HEIGHT_LOGICAL_PX) / 2.0,
        if on {
            palette.accent
        } else {
            // `--active` over `--win`: the wash the mock-up gives a switch that
            // is off, pre-mixed over the plane a modal dialog borrows.
            palette.float_row_selected
        },
        1.0,
    );
    quads.extend(clip_quads(track, clip));
    let inset = px(SWITCH_KNOB_INSET_LOGICAL_PX);
    let knob_left = if on {
        rect[2] - inset - px(SWITCH_KNOB_LOGICAL_PX)
    } else {
        rect[0] + inset
    };
    let knob = [
        knob_left,
        rect[1] + inset,
        knob_left + px(SWITCH_KNOB_LOGICAL_PX),
        rect[1] + inset + px(SWITCH_KNOB_LOGICAL_PX),
    ];
    // `box-shadow: 0 1px 3px rgba(0,0,0,.25)` — the knob's lift, offset by its
    // one logical pixel, drawn through the same door every other lift in this
    // window is drawn through.
    //
    // **A falloff, not a stroke** (user report 2026-09-06, a photograph of the
    // card at 150% in the light theme: every switch that is off wearing a grey
    // band round its knob). A blur's three pixels are a gradient — the quarter
    // alpha stands against the knob and is gone by the time it has travelled
    // its reach. `rounded_overlay_halo` puts the whole quarter on all three of
    // them, and three logical pixels is five physical ones at 150%, which is
    // wider than the two that separate the knob from the track's own edge: the
    // ring stood proud of the track at the top and the bottom and read as a
    // second solid shape laid over the switch rather than as a shadow under
    // its knob. `rounded_overlay_halo`'s own doc names which of the two it is
    // — the exact uniform ring an outline needs, and the one thing a shadow
    // must not be — and `rounded_overlay_shadow` is the falloff beside it.
    quads.extend(clip_quads(
        rounded_overlay_shadow(
            [knob[0], knob[1] + px(1.0), knob[2], knob[3] + px(1.0)],
            px(SWITCH_KNOB_LOGICAL_PX) / 2.0,
            px(SWITCH_KNOB_SHADOW_LOGICAL_PX),
            SWITCH_KNOB_SHADOW_INK,
            SWITCH_KNOB_SHADOW_ALPHA,
        ),
        clip,
    ));
    quads.extend(clip_quads(
        rounded_overlay_fill(
            knob,
            px(SWITCH_KNOB_LOGICAL_PX) / 2.0,
            palette.menu_surface,
            1.0,
        ),
        clip,
    ));
}

/// Every quad of `source`, cut to `clip`, with the ones outside it dropped.
fn clip_quads(source: Vec<OverlayQuad>, clip: [f32; 4]) -> Vec<OverlayQuad> {
    source
        .into_iter()
        .filter_map(|quad| clipped(quad.rect, clip).map(|rect| OverlayQuad { rect, ..quad }))
        .collect()
}

/// Which end of the body is cutting a row in half.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Edge {
    Top,
    Bottom,
}

/// The card's ground, ramped over one end of the viewport.
///
/// **Both ends, and the same rule at each.** The spec draws the foot because
/// that is the end a card at rest is clipping; scroll the body and the head is
/// clipping in exactly the same way, and half a glyph is half a glyph whichever
/// end it was cut at.
fn fade(viewport: [f32; 4], ground: [u8; 3], extent: f32, edge: Edge) -> Vec<OverlayQuad> {
    let band = extent / FADE_BANDS as f32;
    (0..FADE_BANDS)
        .map(|index| {
            // Opaque against the clip, transparent against the type.
            let toward_the_edge = (index + 1) as f32 / FADE_BANDS as f32;
            let (top, alpha) = match edge {
                Edge::Bottom => (viewport[3] - extent + index as f32 * band, toward_the_edge),
                Edge::Top => (
                    viewport[1] + index as f32 * band,
                    1.0 - index as f32 / FADE_BANDS as f32,
                ),
            };
            OverlayQuad {
                rect: [viewport[0], top, viewport[2], top + band],
                color: ground,
                alpha,
            }
        })
        .collect()
}

/// `:focus-visible` — 2px of accent, offset off the control it rings.
///
/// The radius follows the control rather than being one number for all three:
/// a ring drawn with a button's 6px corner around a 9px pill would be a square
/// wrapped round a capsule.
fn focus_ring(
    rect: [f32; 4],
    scale: f32,
    offset_logical: f32,
    accent: [u8; 3],
) -> Vec<OverlayQuad> {
    let offset = offset_logical * scale;
    let radius = (BUTTON_RADIUS_LOGICAL_PX * scale).min((rect[3] - rect[1]) / 2.0);
    rounded_overlay_halo(
        [
            rect[0] - offset,
            rect[1] - offset,
            rect[2] + offset,
            rect[3] + offset,
        ],
        radius + offset,
        FOCUS_RING_WIDTH_LOGICAL_PX * scale,
        accent,
        1.0,
    )
}

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
        mono: false,
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

/// The font sizes a caller has to measure with, so nothing else has to know
/// them. The row's own title is never measured: it is one line by construction,
/// and the switch beside it is the thing that is placed against it.
pub const MEASURED_DESC_FONT_LOGICAL_PX: f32 = DESC_FONT_LOGICAL_PX;
pub const MEASURED_FOOTNOTE_FONT_LOGICAL_PX: f32 = FOOTNOTE_FONT_LOGICAL_PX;
pub const MEASURED_BUTTON_FONT_LOGICAL_PX: f32 = BUTTON_FONT_LOGICAL_PX;

/// The width the footnote is wrapped to, which is the sentence's room less what
/// the link beside it takes.
#[must_use]
pub fn footnote_width(surface_width: f32, scale: f32, link_width: f32) -> f32 {
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    card_width(surface_width, scale)
        - 2.0 * border
        - 2.0 * PADDING_X_LOGICAL_PX * scale
        - FOOTNOTE_LINK_GAP_LOGICAL_PX * scale
        - link_width
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings;

    /// A machine with everything: Windows 11 with the package beside the
    /// executable, all three agents on the path, none of them configured yet.
    fn every_row() -> Machine {
        Machine {
            explorer_first_page_available: true,
            claude_found: true,
            claude_installable: true,
            codex_found: true,
            codex_installable: true,
            copilot_found: true,
            copilot_installable: true,
            powershell_integration_installed: false,
        }
    }

    fn kinds(rows: &[Row]) -> Vec<RowKind> {
        rows.iter().map(|row| row.kind).collect()
    }

    /// PIN (§7.56 §2, user ruling 2026-09-06) — **the card appears on a machine
    /// with no `settings.json`, and on no other.**
    ///
    /// Two facts and both of them required, which is the whole of this
    /// function. The stored state alone would raise the card over a
    /// `settings.json` that exists and could not be parsed — that file falls
    /// back to defaults, and a defaulted `NotShown` there is not a new machine.
    /// The missing file alone would raise it a second time on the next launch
    /// after a run that was killed with the card on screen.
    ///
    /// MUTATIONS:
    /// ① drop the file check and a reader whose settings file got damaged is
    ///    asked, on a machine they have configured, to configure it;
    /// ② drop the stored check and the card comes back on the launch after a
    ///    crash, and after every `Not now` until a write happens to land.
    #[test]
    fn the_card_is_due_only_when_there_was_no_settings_file_and_it_has_never_been_up() {
        assert!(due(true, FirstRunCardV1::NotShown));
        assert!(
            !due(false, FirstRunCardV1::NotShown),
            "a file that exists is a machine somebody has already configured, even when this \
             build could not read it"
        );
        assert!(
            !due(true, FirstRunCardV1::Shown),
            "the card came back after the reader had already answered it"
        );
        assert!(!due(false, FirstRunCardV1::Shown));
    }

    /// PIN (§7.56 §4) — **a row is offered only if it can be honoured, and the
    /// agent group vanishes with its rows.**
    ///
    /// The card never says that it looked and found nothing: if none of the
    /// three agents is on this machine the heading is absent too, and the card
    /// is four lines shorter rather than four lines emptier.
    ///
    /// MUTATIONS:
    /// ① list an agent whose program is not on the path and `Done` writes a
    ///    hook for a tool this machine has not got;
    /// ② list one whose configuration already calls Folio and the card asks a
    ///    question the reader answered before it opened;
    /// ③ hang the heading on the group rather than on its first row and a
    ///    machine with no agents shows a heading over nothing.
    #[test]
    fn only_the_rows_this_machine_can_honour_are_offered() {
        assert_eq!(
            kinds(&rows(&every_row())),
            [
                RowKind::Update,
                RowKind::Explorer,
                RowKind::PowerShell,
                RowKind::Claude,
                RowKind::Codex,
                RowKind::Copilot
            ]
        );
        let none = Machine {
            claude_found: false,
            codex_found: false,
            copilot_found: false,
            ..every_row()
        };
        let offered = rows(&none);
        assert_eq!(
            kinds(&offered),
            [RowKind::Update, RowKind::Explorer, RowKind::PowerShell]
        );
        assert!(
            offered.iter().all(|row| row.group.is_none()),
            "the card is showing a heading over a group with nothing in it"
        );
        let configured = Machine {
            claude_installable: false,
            codex_installable: false,
            ..every_row()
        };
        assert_eq!(
            kinds(&rows(&configured)),
            [
                RowKind::Update,
                RowKind::Explorer,
                RowKind::PowerShell,
                RowKind::Copilot
            ],
            "a row is being offered for a configuration that already calls Folio"
        );
        assert_eq!(
            rows(&configured)[3].group,
            Some(Text::FirstRunAgentGroup),
            "the heading moved off the first agent row that is actually shown"
        );
    }

    /// PIN (§7.56 §3) — **the one row that arrives on is the first row, and it
    /// is the only one.**
    ///
    /// The reader is first told what Folio *does* and only then asked what it
    /// *may* do; and in a body that can scroll, the only row that is already on
    /// is guaranteed to be above the fold.
    ///
    /// MUTATION: ship any other row on and a machine gets a registry key or
    /// somebody's `~/.claude/settings.json` edited by a reader who pressed the
    /// only button on the card that looked like agreement.
    #[test]
    fn the_update_check_is_the_only_row_that_arrives_on_and_it_arrives_first() {
        let offered = rows(&every_row());
        assert_eq!(offered[0].kind, RowKind::Update);
        assert!(offered[0].on);
        assert!(
            offered[1..].iter().all(|row| !row.on),
            "a row that writes outside %APPDATA%\\Folio is switched on before anybody asked"
        );
    }

    /// PIN (§7.56 §4.2) — **the Explorer row is offered on every Windows, and
    /// only its sentence changes.**
    ///
    /// On Windows 11 with no `folio.msix` beside `folio.exe` the switch can
    /// only mean the classic entry, so the row says what it can mean and the
    /// sentence explaining the missing half stays on the Settings page, where a
    /// sentence of that shape belongs. No sentence on this card describes an
    /// absence.
    ///
    /// MUTATIONS:
    /// ① withhold the row without a package and a Windows 11 reader whose
    ///    archive was unpacked without the `.msix` loses the classic entry too;
    /// ② keep the Windows 11 sentence and the row promises a first page it
    ///    cannot register.
    #[test]
    fn the_explorer_row_says_only_what_its_switch_can_mean() {
        let both = every_row();
        assert_eq!(explorer_shape(&both), ExplorerShape::FirstPageAndClassic);
        assert_eq!(
            explorer_shape(&both).description(),
            Text::FirstRunDescExplorer11
        );
        let classic = Machine {
            explorer_first_page_available: false,
            ..both
        };
        assert_eq!(explorer_shape(&classic), ExplorerShape::ClassicOnly);
        assert_eq!(
            explorer_shape(&classic).description(),
            Text::FirstRunDescExplorer10
        );
        assert!(
            rows(&classic)
                .iter()
                .any(|row| row.kind == RowKind::Explorer),
            "a machine with no package lost the entry it could have had"
        );
    }

    /// PIN (§7.56 §11, user ruling 2026-09-06 「完成按各行调用与设置页同一函数」)
    /// — **every row the card spends is spent as the press the Settings page
    /// sends.**
    ///
    /// This is the card's whole contract with the rest of the program: it
    /// installs nothing itself. The assertion is per row and it is made against
    /// `crate::settings`'s own reader for that row, so a target that named the
    /// wrong row — or the wrong index into `FORMULA_OPTIONS`, which is the
    /// mistake nothing else would catch — comes back as `None` or as `false`.
    ///
    /// MUTATIONS:
    /// ① swap the index arithmetic (`FORMULA_OPTIONS` is `[true, false]`, so an
    ///    answer is the index of itself) and `Done` turns the update check off
    ///    for a reader who left it on;
    /// ② point any row at a neighbouring `SettingsRow` and the card installs
    ///    something nobody asked for while the row they did ask for stays off;
    /// ③ give the PowerShell intent a target and it becomes a row nobody can
    ///    flip, applied at a moment when no shell has named a file to write to.
    #[test]
    fn every_answer_leaves_this_card_as_the_press_the_settings_page_sends() {
        let target = |application| settings_target(application).expect("a row of its own");
        assert_eq!(
            settings::update_check_requested(target(Application::UpdateCheck(true))),
            Some(true)
        );
        assert_eq!(
            settings::update_check_requested(target(Application::UpdateCheck(false))),
            Some(false)
        );
        assert_eq!(
            settings::context_menu_requested(target(Application::ContextMenu)),
            Some(true)
        );
        assert_eq!(
            settings::explorer_first_page_requested(target(Application::ExplorerFirstPage)),
            Some(true)
        );
        assert_eq!(
            settings::powershell_integration_offer_requested(target(Application::PowerShellOffer(
                false
            ))),
            Some(false)
        );
        assert_eq!(
            settings::powershell_integration_offer_requested(target(Application::PowerShellOffer(
                true
            ))),
            Some(true)
        );
        assert_eq!(
            settings::claude_hooks_requested(target(Application::ClaudeHooks)),
            Some(true)
        );
        assert_eq!(
            settings::codex_notify_requested(target(Application::CodexNotify)),
            Some(true)
        );
        assert_eq!(
            settings::copilot_hooks_requested(target(Application::CopilotHooks)),
            Some(true)
        );
        assert_eq!(
            settings_target(Application::PowerShellIntent),
            None,
            "the one answer that is not a row has been given one"
        );
    }

    /// PIN (§7.56 §8) — **`Done` applies every row that is on, and a row that is
    /// off spends nothing that is not an answer.**
    ///
    /// The two that spend something either way are not exceptions: the update
    /// check arrives on, so turning it off *is* an answer, and the PowerShell
    /// offer is the one question with a second surface, so a row left off has to
    /// close that surface too (§7).
    ///
    /// MUTATIONS:
    /// ① apply an off row as `install = false` and `Done` on an untouched card
    ///    removes an Explorer entry the reader had from a previous install;
    /// ② drop `PowerShellOffer(false)` and the strip asks again minutes later
    ///    about the question the card just asked;
    /// ③ drop `PowerShellIntent` and a row left on installs nothing, ever.
    #[test]
    fn done_spends_the_rows_that_are_on_and_the_two_answers_that_are_answers() {
        let untouched = rows(&every_row());
        assert_eq!(
            applications(&untouched, ExplorerShape::FirstPageAndClassic),
            [
                Application::UpdateCheck(true),
                Application::PowerShellOffer(false)
            ],
            "an untouched card is doing something to this machine other than what it arrived \
             saying it would"
        );
        let mut all_on = untouched;
        for row in &mut all_on {
            row.on = true;
        }
        assert_eq!(
            applications(&all_on, ExplorerShape::FirstPageAndClassic),
            [
                Application::UpdateCheck(true),
                Application::ContextMenu,
                Application::ExplorerFirstPage,
                Application::PowerShellOffer(true),
                Application::PowerShellIntent,
                Application::ClaudeHooks,
                Application::CodexNotify,
                Application::CopilotHooks
            ]
        );
        assert_eq!(
            applications(&all_on, ExplorerShape::ClassicOnly)
                .into_iter()
                .filter(|spent| *spent == Application::ExplorerFirstPage)
                .count(),
            0,
            "a machine with no package is being asked to register one"
        );
        let mut update_off = rows(&every_row());
        update_off[0].on = false;
        assert!(
            applications(&update_off, ExplorerShape::ClassicOnly)
                .contains(&Application::UpdateCheck(false)),
            "the one row that arrived on was turned off and nothing wrote the answer down"
        );
    }

    /// PIN (§7.56 §8) — **`Not now`, `Esc`, `Ctrl+W` and `Alt+F4` spend
    /// nothing.**
    ///
    /// Not "apply the rows as they stand": the card closes with the factory
    /// values, which is what the machine already has, and the one question with
    /// a second surface keeps it. `Shown` was written when the card went up, so
    /// nothing comes back.
    ///
    /// MUTATION: route `Not now` through `applications` and a reader who
    /// declined has `powershell_integration_offer` written `false` — the strip
    /// they were never asked about, silenced by declining to answer.
    #[test]
    fn declining_writes_nothing_outside_the_card_s_own_state() {
        assert!(declined().is_empty());
    }

    /// PIN (§7.56 §4.3) — **the intent is spent by the first shell to name its
    /// own `$PROFILE`, and a profile that already loads the script clears it
    /// without writing.**
    ///
    /// MUTATIONS:
    /// ① write over a `Silent` offer and a reader whose profile already
    ///    dot-sources `folio.ps1` gets a second copy of the line;
    /// ② act with no intent recorded and every launch edits `$PROFILE`;
    /// ③ leave the intent standing on a `Silent` answer and the card's row waits
    ///    for a shell that has already spoken, for ever.
    #[test]
    fn the_recorded_intent_is_spent_by_the_first_shell_that_names_its_profile() {
        let owed = shell_integration::Offer::Owed(PathBuf::from(r"C:\Users\alice\profile.ps1"));
        assert_eq!(
            pending_step(true, Some(&owed)),
            PendingStep::Write(PathBuf::from(r"C:\Users\alice\profile.ps1"))
        );
        assert_eq!(
            pending_step(true, Some(&shell_integration::Offer::Silent)),
            PendingStep::Clear,
            "a profile that already loads the script is about to be given the line twice"
        );
        assert_eq!(
            pending_step(false, Some(&owed)),
            PendingStep::Wait,
            "a $PROFILE is being written on a machine that never recorded an intent"
        );
        assert_eq!(pending_step(true, None), PendingStep::Wait);
    }

    /// PIN (§7.56 §4.3) — **the Terminal page's PowerShell row says which state
    /// it is in while an intent is outstanding.**
    ///
    /// The row is neither "installed" nor "off"; a row that read `Off` over a
    /// write that is coming would be this window disagreeing with itself about
    /// a file.
    ///
    /// MUTATION: return the sentence unconditionally and the row says a shell is
    /// about to be joined on every machine that never asked for it.
    #[test]
    fn the_settings_row_says_the_write_is_waiting_for_a_shell() {
        assert_eq!(
            pending_row_line(true),
            Some(Text::ShellIntegrationPending),
            "a row that asked for the integration reads as though nothing was asked"
        );
        assert_eq!(pending_row_line(false), None);
    }

    /// PIN (§7.56 §6) — **the focus order is every switch, then the link, then
    /// the two verbs, and it is a ring.**
    ///
    /// The card is modal, so focus never leaves it. `↑`/`↓` move between
    /// switches only, because the list is a list.
    ///
    /// MUTATIONS:
    /// ① let `Tab` fall off either end and the keyboard leaves a modal card,
    ///    landing in a shell behind a scrim;
    /// ② let the arrows walk onto the buttons and `↓` from the last row presses
    ///    nothing while looking as though it might.
    #[test]
    fn the_focus_walks_the_card_in_a_ring_and_the_arrows_stay_in_the_list() {
        assert_eq!(
            focus_order(3),
            [
                Focus::Switch(0),
                Focus::Switch(1),
                Focus::Switch(2),
                Focus::OpenSettings,
                Focus::Later,
                Focus::Done
            ]
        );
        assert_eq!(stepped(Focus::Switch(2), 3, true), Focus::OpenSettings);
        assert_eq!(
            stepped(Focus::Done, 3, true),
            Focus::Switch(0),
            "Tab off the last verb left the card"
        );
        assert_eq!(
            stepped(Focus::Switch(0), 3, false),
            Focus::Done,
            "Shift+Tab off the first switch left the card"
        );
        assert_eq!(arrowed(Focus::Switch(0), 3, true), Focus::Switch(1));
        assert_eq!(
            arrowed(Focus::Switch(2), 3, true),
            Focus::Switch(2),
            "the arrows walked out of the list"
        );
        assert_eq!(arrowed(Focus::Switch(0), 3, false), Focus::Switch(0));
        assert_eq!(
            arrowed(Focus::Done, 3, false),
            Focus::Switch(2),
            "an arrow from a verb enters the list at the end it came from"
        );
        assert_eq!(arrowed(Focus::OpenSettings, 3, true), Focus::Switch(0));
    }

    /// PIN (§7.56 §6) — **the card opens with the ring on the first switch and
    /// the ring not yet drawn.**
    ///
    /// Not on `Done`: this is a form, and landing on the primary would let a
    /// blind `Enter` answer a card nobody has read. And not drawn: a card that
    /// arrived with a ring on it would be claiming a keyboard nobody has used.
    ///
    /// MUTATIONS:
    /// ① open on `Focus::Done` and the first `Enter` after launch answers the
    ///    card;
    /// ② draw the ring from the start and every card in this window disagrees
    ///    with every other about what `:focus-visible` means.
    #[test]
    fn the_card_opens_on_its_first_switch_with_the_ring_put_away() {
        let mut card = Card::default();
        card.open(rows(&every_row()), ExplorerShape::FirstPageAndClassic);
        assert!(card.is_open());
        assert_eq!(card.focus(), Some(Focus::Switch(0)));
        assert_eq!(
            card.focus_ring(),
            None,
            "the card came up already claiming the keyboard"
        );
        card.light_the_ring();
        assert_eq!(card.focus_ring(), Some(Focus::Switch(0)));
        card.press(Target::Done);
        assert_eq!(
            card.focus_ring(),
            None,
            "a finger left a keyboard ring standing"
        );
        // The shape it was opened with is kept, and the only thing that can
        // show it is what `Done` would spend: this machine's switch means both
        // registrations, so an Explorer row that is on spends two.
        card.flip(1);
        assert!(
            card.done().contains(&Application::ExplorerFirstPage),
            "the card forgot which shape its Explorer switch was opened with"
        );
    }

    /// PIN (§7.56 §11) — **a switch on this card is an answer held until
    /// `Done`, not an act.**
    ///
    /// `Done` is what makes the four rows one decision rather than four, and it
    /// is what lets `Not now` mean "I answered nothing".
    ///
    /// MUTATION: apply on the flip and a reader who turns a row on and then off
    ///    again has had it installed and removed, with two cards to prove it.
    #[test]
    fn a_switch_changes_the_card_and_nothing_else_until_done() {
        let mut card = Card::default();
        card.open(rows(&every_row()), ExplorerShape::ClassicOnly);
        assert_eq!(
            card.done(),
            [
                Application::UpdateCheck(true),
                Application::PowerShellOffer(false)
            ]
        );
        assert!(card.flip(1));
        assert!(card.rows()[1].on);
        assert_eq!(
            card.done(),
            [
                Application::UpdateCheck(true),
                Application::ContextMenu,
                Application::PowerShellOffer(false)
            ]
        );
        assert!(card.flip(1));
        assert_eq!(
            card.done(),
            [
                Application::UpdateCheck(true),
                Application::PowerShellOffer(false)
            ],
            "a row flipped on and off again left something behind"
        );
        assert!(card.set(1, true));
        assert!(!card.set(1, true), "a second → reported a change");
    }

    // ── geometry ───────────────────────────────────────────────────────────

    /// The window `card-v3-*.png` were composed on: a 1133 × 548 logical window
    /// at 1.5×.
    const SURFACE: (f32, f32) = (1699.0, 822.0);
    const SCALE: f32 = 1.5;

    fn measured(rows: &[Row], lines_each: usize) -> Content {
        Content {
            title: Text::FirstRunTitle.text().to_owned(),
            rows: rows
                .iter()
                .map(|row| RowContent {
                    group: row.group.map(|label| label.text().to_owned()),
                    title: row.title.text().to_owned(),
                    description_lines: vec!["a measured line".to_owned(); lines_each],
                    on: row.on,
                })
                .collect(),
            footnote_lines: vec![Text::FirstRunFootnote.text().to_owned()],
            footnote_last_line_width: 300.0,
            open_settings: Text::FirstRunOpenSettings.text().to_owned(),
            open_settings_width: 90.0,
            later: Text::FirstRunLater.text().to_owned(),
            later_width: 50.0,
            done: Text::FirstRunDone.text().to_owned(),
            done_width: 40.0,
        }
    }

    /// PIN (§7.56 §9) — **the head and the foot are pinned and the body between
    /// them scrolls.**
    ///
    /// The pinned foot is why the scroll is safe: `Done`, `Not now` and the one
    /// sentence that says every row is in Settings are never the thing that
    /// scrolled away.
    ///
    /// MUTATIONS:
    /// ① let the card grow past the window and the two verbs are off the bottom
    ///    of the screen on a short window, with no way to answer;
    /// ② put the foot inside the scroller and the reader has to find the buttons
    ///    before they can press them.
    #[test]
    fn the_body_scrolls_and_the_two_verbs_never_do() {
        let content = measured(&rows(&every_row()), 3);
        let tall = layout(&content, SURFACE.0, SURFACE.1, SCALE, 0.0);
        assert!(
            tall.scrolls(),
            "seven entries of three lines each fitted a 548 logical window without scrolling"
        );
        assert!(
            tall.frame[3] - tall.frame[1] <= SURFACE.1,
            "the card is taller than the window it is drawn in"
        );
        let scrolled = layout(&content, SURFACE.0, SURFACE.1, SCALE, tall.scroll_extent());
        assert_eq!(
            scrolled.later.1, tall.later.1,
            "`Not now` moved when the body was scrolled"
        );
        assert_eq!(scrolled.done.1, tall.done.1);
        assert_eq!(scrolled.title.1, tall.title.1);
        assert_eq!(
            scrolled.footnote[0].1, tall.footnote[0].1,
            "the one sentence that says where these rows live scrolled away"
        );
        assert!(
            scrolled.rows[0].title.1[1] < tall.rows[0].title.1[1],
            "the body did not move"
        );
        let short = measured(&rows(&every_row())[..2], 1);
        assert!(
            !layout(&short, SURFACE.0, SURFACE.1, SCALE, 0.0).scrolls(),
            "a card that fits grew a scrollbar"
        );
    }

    /// PIN (§7.56 §6) — **a ring below the fold is scrolled into view.**
    ///
    /// `Tab` or `↓` into a row under the fade brings it up, because a focus the
    /// reader cannot see is not a focus.
    ///
    /// MUTATION: return the scroll unchanged and `Tab` walks off the bottom of a
    /// card that looks as though nothing happened.
    #[test]
    fn walking_onto_a_row_below_the_fold_brings_it_into_view() {
        let content = measured(&rows(&every_row()), 3);
        let placed = layout(&content, SURFACE.0, SURFACE.1, SCALE, 0.0);
        let last = placed.rows.len() - 1;
        let to = placed.scroll_showing(Focus::Switch(last));
        assert!(
            to > 0.0,
            "the ring walked onto a row that is under the fade and nothing moved"
        );
        let after = layout(&content, SURFACE.0, SURFACE.1, SCALE, to);
        assert!(
            after.rows[last].switch[3] <= after.viewport[3],
            "the row the ring is on is still below the fold"
        );
        assert_eq!(
            after.scroll_showing(Focus::Switch(last)),
            to,
            "a row that is already in view was scrolled anyway"
        );
        assert_eq!(
            after.scroll_showing(Focus::Done),
            to,
            "the pinned foot moved the body"
        );
    }

    /// PIN (§7.56) — **a switch scrolled out of the body cannot be pressed.**
    ///
    /// The clip is where a control stops being drawn, so it is where it stops
    /// answering. Anything else is a press on a row the reader cannot see.
    ///
    /// MUTATION: drop the viewport check in `hit` and a click on the footnote's
    /// own line flips whichever row happens to be under the foot.
    #[test]
    fn a_press_outside_the_body_is_not_a_press_on_a_row() {
        let content = measured(&rows(&every_row()), 3);
        let placed = layout(&content, SURFACE.0, SURFACE.1, SCALE, 0.0);
        let first = placed.rows[0].switch;
        let middle = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        let (x, y) = middle(first);
        assert_eq!(hit(&placed, x, y), Target::Switch(0));
        let last = placed.rows.len() - 1;
        let (x, y) = middle(placed.rows[last].switch);
        assert_eq!(
            hit(&placed, x, y),
            Target::Panel,
            "a switch under the fade answered a press"
        );
        let (x, y) = middle(placed.done.1);
        assert_eq!(hit(&placed, x, y), Target::Done);
        let (x, y) = middle(placed.later.1);
        assert_eq!(hit(&placed, x, y), Target::Later);
        let (x, y) = middle(placed.link.1);
        assert_eq!(hit(&placed, x, y), Target::OpenSettings);
        assert_eq!(
            hit(&placed, 4.0, 4.0),
            Target::Panel,
            "the card is modal, so a press on its scrim is still its own"
        );
    }

    /// PIN (§7.56 §3) — **the switch is 30 × 18 with a 14 × 14 knob two pixels
    /// in, its right edge on the card's padding, centred on the row's first
    /// title line.**
    ///
    /// Settings' own row shape: the sentence on the left, the control on the
    /// right. A reader handed six rows of that shape must meet the same shape
    /// when they next open Settings to change one.
    ///
    /// MUTATIONS:
    /// ① centre the switch on the whole row and a row whose sentence runs to
    ///    three lines drags its control down the card;
    /// ② let the sentence run under the switch and every long line collides with
    ///    it.
    #[test]
    fn the_switch_is_settings_own_control_in_settings_own_row_shape() {
        let content = measured(&rows(&every_row()), 3);
        let placed = layout(&content, SURFACE.0, SURFACE.1, SCALE, 0.0);
        let row = &placed.rows[0];
        let px = |value: f32| value * SCALE;
        assert!((row.switch[2] - row.switch[0] - px(SWITCH_WIDTH_LOGICAL_PX)).abs() < 0.01);
        assert!((row.switch[3] - row.switch[1] - px(SWITCH_HEIGHT_LOGICAL_PX)).abs() < 0.01);
        let switch_middle = (row.switch[1] + row.switch[3]) / 2.0;
        let title_middle = (row.title.1[1] + row.title.1[3]) / 2.0;
        assert!(
            (switch_middle - title_middle).abs() < 0.51,
            "the switch is not centred on the row's first title line"
        );
        assert!(
            row.title.1[2] <= row.switch[0],
            "the sentence column runs under the control"
        );
        let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * SCALE).max(1.0);
        assert!(
            (row.switch[2] - (placed.frame[2] - border - px(PADDING_X_LOGICAL_PX))).abs() < 0.01,
            "the switch's right edge is not on the card's own padding"
        );
        assert!(
            row.description
                .iter()
                .all(|(_, rect)| rect[2] <= row.switch[0]),
            "a description line runs under the control"
        );
    }

    /// Every quad of the card, small enough to be part of a switch and near
    /// `switch`, drawn in the knob's shadow ink.
    ///
    /// The scrim is the same ink and it is the whole window, so size is what
    /// separates them: nothing a switch draws can be wider than its own track
    /// plus the shadow's reach on both sides.
    fn knob_shadow_quads(
        layers: &[OverlayLayer],
        switch: [f32; 4],
        reach: f32,
    ) -> Vec<OverlayQuad> {
        let near = |rect: [f32; 4]| {
            rect[0] < switch[2] + 4.0 * reach
                && rect[2] > switch[0] - 4.0 * reach
                && rect[1] < switch[3] + 4.0 * reach
                && rect[3] > switch[1] - 4.0 * reach
        };
        let small = |rect: [f32; 4]| {
            rect[2] - rect[0] <= (switch[2] - switch[0]) + 4.0 * reach
                && rect[3] - rect[1] <= (switch[3] - switch[1]) + 4.0 * reach
        };
        layers
            .iter()
            .flat_map(|layer| layer.quads.iter())
            .filter(|quad| {
                quad.color == SWITCH_KNOB_SHADOW_INK
                    && quad.alpha > 0.0
                    && near(quad.rect)
                    && small(quad.rect)
            })
            .cloned()
            .collect()
    }

    /// The rect the knob's shadow hangs on — the knob, offset by its one logical
    /// pixel — snapped to the pixel grid the coverage pass snaps it to.
    fn knob_lift(switch: [f32; 4], on: bool) -> [f32; 4] {
        let px = |value: f32| value * SCALE;
        let inset = px(SWITCH_KNOB_INSET_LOGICAL_PX);
        let side = px(SWITCH_KNOB_LOGICAL_PX);
        let left = if on {
            switch[2] - inset - side
        } else {
            switch[0] + inset
        };
        [
            left.round(),
            (switch[1] + inset + px(1.0)).round(),
            (left + side).round(),
            (switch[1] + inset + side + px(1.0)).round(),
        ]
    }

    /// PIN (§7.56 §3, user report 2026-09-06 — 「这里这个 switch 的阴影是不是有
    /// 问题呢」, a screenshot of the card at 150% in the light theme) — **the
    /// knob's lift is a falloff under the knob and nothing else on the track.**
    ///
    /// `box-shadow: 0 1px 3px rgba(0,0,0,.25)` is a *blur*: darkest right
    /// against the knob and gone by the time it has travelled its reach. Drawn
    /// as `rounded_overlay_halo` it is instead a stroke — the whole three
    /// logical pixels at the full quarter-alpha, which at 1.5× is a five-pixel
    /// grey band ringing the knob on every side, wider than the gap between the
    /// knob and the track's own edge, so it spills off the track top and bottom
    /// and reads as a second solid shape sitting on the switch. That is what
    /// the report is a picture of, and `rounded_overlay_halo`'s own doc says as
    /// much: it is the exact uniform ring an outline needs and a shadow must
    /// not be.
    ///
    /// MUTATIONS:
    /// ① draw the lift with `rounded_overlay_halo` again and the profile down
    ///    the knob's flank is flat — every band at the full alpha, which is the
    ///    grey blob that was reported;
    /// ② hang the ring on the track instead of on the knob and the ink runs to
    ///    the far end of a track that should carry nothing but its own fill;
    /// ③ drop the one-pixel offset and the knob sits in a ring rather than over
    ///    a shadow.
    #[test]
    fn the_knob_s_shadow_falls_off_and_leaves_the_rest_of_the_track_alone() {
        let px = |value: f32| value * SCALE;
        let reach = px(SWITCH_KNOB_SHADOW_LOGICAL_PX).round();
        let mut content = measured(&rows(&every_row()), 3);
        // Both answers, on rows that are not the first: the body is clipped to
        // its viewport, the first row's switch stands a pixel or two above its
        // own title line, and half a shadow cut off by that clip is not the
        // shape under test.
        for (index, row) in content.rows.iter_mut().enumerate() {
            row.on = index % 2 == 1;
        }
        let placed = layout(&content, SURFACE.0, SURFACE.1, SCALE, 0.0);
        let layers = build(&placed, SURFACE, None, None);
        let whole = |row: &&RowRects| {
            row.switch[1] - reach >= placed.viewport[1]
                && row.switch[3] + reach <= placed.viewport[3]
        };
        let off = placed
            .rows
            .iter()
            .filter(whole)
            .find(|row| !row.on)
            .expect("no switch that is off stands clear of the fade");
        let on = placed
            .rows
            .iter()
            .filter(whole)
            .find(|row| row.on)
            .expect("no switch that is on stands clear of the fade");

        for row in [off, on] {
            let lift = knob_lift(row.switch, row.on);
            let ink = knob_shadow_quads(&layers, row.switch, reach);
            assert!(
                !ink.is_empty(),
                "the knob carries no shadow at all (on = {})",
                row.on
            );
            // ── it sits on the knob, and on nothing else ──────────────────
            for quad in &ink {
                assert!(
                    quad.rect[0] >= lift[0] - reach - 0.01
                        && quad.rect[1] >= lift[1] - reach - 0.01
                        && quad.rect[2] <= lift[2] + reach + 0.01
                        && quad.rect[3] <= lift[3] + reach + 0.01,
                    "shadow ink at {:?} is outside the knob's own reach {:?} (on = {})",
                    quad.rect,
                    lift,
                    row.on
                );
            }
            // The end of the track the knob is not at carries the track's fill
            // and nothing over it.
            let empty = if row.on {
                [row.switch[0], row.switch[1], lift[0] - reach, row.switch[3]]
            } else {
                [lift[2] + reach, row.switch[1], row.switch[2], row.switch[3]]
            };
            assert!(
                empty[2] - empty[0] >= px(SWITCH_WIDTH_LOGICAL_PX) / 4.0,
                "the shadow's reach covers the track end to end, so an empty end is not a fact \
                 about it"
            );
            assert!(
                !ink.iter().any(|quad| quad.rect[0] < empty[2]
                    && quad.rect[2] > empty[0]
                    && quad.rect[1] < empty[3]
                    && quad.rect[3] > empty[1]),
                "shadow ink lies on the empty end {empty:?} of the track (on = {})",
                row.on
            );

            // ── and it is a blur, not a stroke ────────────────────────────
            //
            // Read straight down the knob's bottom flank, where the ring's
            // coverage is exactly one, so each sample is its band's own alpha.
            let column = (lift[0] + lift[2]) / 2.0;
            let profile: Vec<f32> = (0..reach as usize)
                .map(|distance| {
                    let y = lift[3] + distance as f32 + 0.5;
                    ink.iter()
                        .filter(|quad| {
                            quad.rect[0] <= column
                                && column < quad.rect[2]
                                && quad.rect[1] <= y
                                && y < quad.rect[3]
                        })
                        .map(|quad| quad.alpha)
                        .fold(0.0_f32, f32::max)
                })
                .collect();
            assert!(
                (profile[0] - SWITCH_KNOB_SHADOW_ALPHA).abs() < 0.01,
                "the shadow is not at its full strength against the knob: {profile:?}"
            );
            assert!(
                profile.windows(2).all(|pair| pair[1] < pair[0]),
                "the shadow does not fall off with distance — it is a stroke round the knob \
                 rather than a blur under it: {profile:?} (on = {})",
                row.on
            );
            assert!(
                profile[profile.len() - 1] <= SWITCH_KNOB_SHADOW_ALPHA / 8.0,
                "the shadow has not run out by the end of its reach: {profile:?}"
            );
            // Offset down by its one logical pixel: it reaches further below the
            // knob than above it.
            let above = ink
                .iter()
                .map(|quad| quad.rect[1])
                .fold(f32::INFINITY, f32::min);
            let below = ink
                .iter()
                .map(|quad| quad.rect[3])
                .fold(f32::NEG_INFINITY, f32::max);
            let knob_top = lift[1] - px(1.0);
            let knob_bottom = lift[3] - px(1.0);
            assert!(
                below - knob_bottom > knob_top - above,
                "the shadow is a ring round the knob rather than a lift under it (on = {})",
                row.on
            );
        }
    }

    /// PIN (§7.56 §9) — **480 logical, clamped to 92% of a window narrower than
    /// that.**
    ///
    /// MUTATION: keep `.restore`'s 400 and the three agent sentences each take a
    /// second line, which is the measurement the width was chosen from.
    #[test]
    fn the_card_is_four_hundred_and_eighty_logical_or_the_window_s_own_share() {
        // **The number, not the constant.** Asserting against
        // `MAX_WIDTH_LOGICAL_PX` would be the constant agreeing with itself, and
        // a card retuned to `.restore`'s 400 would walk straight through it —
        // which is exactly the mutation this line was written to catch.
        assert!(
            (card_width(2000.0, 1.0) - 480.0).abs() < 0.51,
            "the width the three agent sentences were measured to fit on one line each is not \
             the width the card takes"
        );
        assert!(
            (card_width(2000.0, 2.0) - 960.0).abs() < 0.51,
            "the cap is in logical pixels, so it doubles with the monitor's scale"
        );
        assert!(
            (card_width(400.0, 1.0) - 368.0).abs() < 0.51,
            "a window narrower than the card did not hand it 92%"
        );
    }
}
