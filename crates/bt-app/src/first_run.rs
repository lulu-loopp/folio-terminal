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
    explorer_menu::ExplorerPlace,
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
/// (`Text::DescExplorerMenuNoPackage`).
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
    /// Which line this shape's row reads.
    ///
    /// **The line, and no sentence under it** (v4): where the entry lands is
    /// the only thing that differs between the two shapes, so it is the only
    /// thing the two strings differ about.
    #[must_use]
    pub fn line(self) -> Text {
        match self {
            Self::FirstPageAndClassic => Text::FirstRunRowExplorer11,
            Self::ClassicOnly => Text::FirstRunRowExplorer10,
        }
    }

    /// **The highest place this machine can honour**, which is what the card's
    /// one switch has always meant (user ruling 2026-09-07).
    ///
    /// **Not its own answer** — [`crate::explorer_menu::place_when_on`] is, and
    /// the Settings row's switch calls the same function. The two surfaces ask
    /// one question, so one function answers it and "the card and the row mean
    /// the same thing" is a fact about the code rather than two tables somebody
    /// has to keep in step.
    ///
    /// It never answers [`ExplorerPlace::Off`]: a row left off spends nothing at
    /// all, and `applications` is what says so.
    #[must_use]
    pub fn place(self) -> ExplorerPlace {
        crate::explorer_menu::place_when_on(self == Self::FirstPageAndClassic)
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
    /// **The wider gap that stands above this row**, when the row opens the
    /// agent group.
    ///
    /// v3 hung a caps heading here — `AGENTS FOUND ON THIS MACHINE` — and v4
    /// deleted it rather than restyling it (user ruling 2026-09-06): a group of
    /// three rows set apart from the three above them does not need a label to
    /// say it is a group, and the heading was the one string on the card that
    /// described our own act of looking. v4 put a hairline there instead, and
    /// **that hairline is gone too** (user ruling 2026-09-07): the settings page
    /// draws no line between its rows and this card is six rows of that same
    /// shape, so the break is air and nothing else. Hung on the row and not on
    /// the group, for the heading's own reason: a machine with no agents opens
    /// no gap, because there is no row for it to stand above.
    pub group_break_above: bool,
    /// The row's one line, and it is the result the reader gets.
    pub line: Text,
    /// What the pointer resting on the row says: the mechanism, and the address
    /// of the reader's own file where there is one.
    pub tip: Text,
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
/// three agents is found the group break and its rows are both absent, and the
/// card never says that it looked and found nothing.
#[must_use]
pub fn rows(machine: &Machine) -> Vec<Row> {
    let mut rows = vec![Row {
        kind: RowKind::Update,
        group_break_above: false,
        line: Text::FirstRunRowUpdate,
        tip: Text::FirstRunTipUpdate,
        on: true,
    }];
    rows.push(Row {
        kind: RowKind::Explorer,
        group_break_above: false,
        line: explorer_shape(machine).line(),
        tip: Text::FirstRunTipExplorer,
        on: false,
    });
    if !machine.powershell_integration_installed {
        rows.push(Row {
            kind: RowKind::PowerShell,
            group_break_above: false,
            line: Text::FirstRunRowPowerShell,
            tip: Text::FirstRunTipPowerShell,
            on: false,
        });
    }
    let agents = [
        (
            RowKind::Claude,
            machine.claude_found && machine.claude_installable,
            Text::FirstRunRowClaude,
            Text::FirstRunTipClaude,
        ),
        (
            RowKind::Codex,
            machine.codex_found && machine.codex_installable,
            Text::FirstRunRowCodex,
            Text::FirstRunTipCodex,
        ),
        (
            RowKind::Copilot,
            machine.copilot_found && machine.copilot_installable,
            Text::FirstRunRowCopilot,
            Text::FirstRunTipCopilot,
        ),
    ];
    let mut opened = false;
    for (kind, offered, line, tip) in agents {
        if !offered {
            continue;
        }
        rows.push(Row {
            kind,
            group_break_above: !opened,
            line,
            tip,
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
    /// **Where Folio's verb goes in Explorer's menu** — one press since the rows
    /// merged (user ruling 2026-09-07), carrying the highest place this machine
    /// can honour.
    ///
    /// It was two applications until then, and the pair is what the one place
    /// still means: [`crate::explorer_menu::ExplorerPlace::FirstPage`] is the
    /// package **and** the classic entry. Never
    /// [`crate::explorer_menu::ExplorerPlace::Off`] — a row left off asks for
    /// nothing, and off is already the factory state.
    Explorer(ExplorerPlace),
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
        // **`choice` again since the switch** (user ruling 2026-09-07): the row
        // is `[true, false]` like every other switch in the dialog, and what a
        // press of `On` reaches is the machine's business rather than the
        // index's. `Off` is spent by nobody here — `applications` never mints an
        // `Explorer(Off)`, because a row left off asks for nothing — so the card
        // presses `On` and the dialog reads back the place this machine can give.
        Application::Explorer(place) => choice(SettingsRow::ContextMenu, place.on()),
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
            RowKind::Explorer if row.on => spent.push(Application::Explorer(shape.place())),
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
            Target::Row(index) => Some(Focus::Switch(index)),
            Target::Later => Some(Focus::Later),
            Target::Done => Some(Focus::Done),
            Target::Panel => self.focus,
        };
    }

    /// A key the card **acts on**: the ring comes on, wherever it was.
    ///
    /// Not every key that arrives (user ruling 2026-09-07). A card that is up
    /// swallows everything it does not act on, and a ring lit by a swallowed
    /// key — a bare modifier, a letter — claims a keyboard nobody has used on
    /// it, which is the same claim [`Self::open`] refuses to make.
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

/// What `Not now`, `Esc` and shutting the window spend.
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
    Later,
    Done,
}

/// The focus order, as a ring: every switch in visual order, then the two
/// verbs. The card is modal, so focus never leaves it.
///
/// **`Open settings` is not a stop any more** (v4 §5): the footer is the two
/// buttons and one faint line, and a line that states a fact is not something
/// a keyboard can land on.
#[must_use]
pub fn focus_order(switches: usize) -> Vec<Focus> {
    (0..switches)
        .map(Focus::Switch)
        .chain([Focus::Later, Focus::Done])
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

/// `min(440px, 92%)`.
///
/// **440 and not v3's 480** (v4 §2, user ruling 2026-09-06). A card of one-line
/// rows does not need 480 of measure: v3 bought its width for a *sentence*
/// under every title, and with the sentence moved to the row's tooltip the 354
/// of text column left here holds every line in both languages with room over.
pub const MAX_WIDTH_LOGICAL_PX: f32 = 440.0;
const WIDTH_RATIO: f32 = 0.92;

/// How much of the window's height the card may take. The head and the foot are
/// pinned inside whatever is left and the body between them scrolls.
const SURFACE_MARGIN_LOGICAL_PX: f32 = 34.0;

/// 20 and not `.restore`'s 22: the mark's line box is taller than a bare title,
/// so the optical top of the card is a couple of pixels lower than the metric
/// one.
const PADDING_TOP_LOGICAL_PX: f32 = 20.0;
const PADDING_X_LOGICAL_PX: f32 = 22.0;
const PADDING_BOTTOM_LOGICAL_PX: f32 = 16.0;

/// The Folio mark on the header line — `design/assets/app-icon/folio.ico`, the
/// icon `build.rs` links into `folio.exe` as group 1.
const MARK_LOGICAL_PX: f32 = 22.0;
const MARK_GAP_LOGICAL_PX: f32 = 10.0;

const TITLE_FONT_LOGICAL_PX: f32 = 15.0;
const TITLE_LINE_LOGICAL_PX: f32 = 21.0;
/// **18, and then the first row.** v3's 16 under a bare title; the header is one
/// line taller in feel now that a mark stands on it, so the step under it grows
/// with it.
const HEADER_MARGIN_BOTTOM_LOGICAL_PX: f32 = 18.0;

/// **One 13px line, vertically centred in 42** (v4 §2). The extra height per row
/// is what "looser rhythm" buys once the second line is gone.
const ROW_HEIGHT_LOGICAL_PX: f32 = 42.0;
const ROW_FONT_LOGICAL_PX: f32 = 13.0;
/// How far the row's band runs past the content column on each side.
///
/// v4 drew this: eight pixels of `--hover` beyond the type, so that a row under
/// the pointer read as a band rather than as a box round the words. Nothing is
/// drawn there now (user ruling 2026-09-07), and the eight pixels stay because
/// the band was never only a fill — it is what a press reaches and what the
/// tooltip hangs off, and a target that stopped at the first glyph would be
/// smaller than the row a reader is aiming at.
const ROW_BAND_BLEED_LOGICAL_PX: f32 = 8.0;
/// **The whole of what separates Folio's own rows from the agent rows** (user
/// ruling 2026-09-07): air, and nothing drawn in it.
///
/// v4 spent 9 + hairline + 9 here and a `--border-soft` hairline between every
/// other pair of rows. The settings page — the page this card borrows its row
/// shape from, and the page every one of these six rows also lives on — draws no
/// line between rows at all, and the reader asked for the same here. So rows of
/// one group now abut, and the group break is this gap: wider than the air a row
/// already carries around its own line, so it reads as a break, and not a mark,
/// so there is nothing on the card to be consistent with.
const GROUP_GAP_LOGICAL_PX: f32 = 16.0;

/// `.aswitch`, to the pixel.
const SWITCH_WIDTH_LOGICAL_PX: f32 = 30.0;
const SWITCH_HEIGHT_LOGICAL_PX: f32 = 18.0;
const SWITCH_KNOB_LOGICAL_PX: f32 = 14.0;
const SWITCH_KNOB_INSET_LOGICAL_PX: f32 = 2.0;
const SWITCH_KNOB_SHADOW_LOGICAL_PX: f32 = 3.0;
const SWITCH_KNOB_SHADOW_INK: [u8; 3] = [0, 0, 0];
const SWITCH_KNOB_SHADOW_ALPHA: f32 = 0.25;
/// The gap between the line's column and the switch.
const SWITCH_GAP_LOGICAL_PX: f32 = 12.0;

/// **16, and no hairline** (v4 §2, and now the card's own rule as well): there
/// is no line anywhere on this card, so a line under the last row would be the
/// only one and would read as a row boundary with nothing under it.
const FOOT_GAP_LOGICAL_PX: f32 = 16.0;
const SETTINGS_LINE_FONT_LOGICAL_PX: f32 = 11.0;
const SETTINGS_LINE_LINE_LOGICAL_PX: f32 = 15.0;
const SETTINGS_LINE_MARGIN_BOTTOM_LOGICAL_PX: f32 = 14.0;

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
    /// **The whole row, switch included** (v4 §2, §8).
    ///
    /// Not the switch alone, which is what v3 hit-tested. A row carries a
    /// tooltip about itself end to end, and this window has a standing rule
    /// about exactly that shape: something that answers a hover and not a click
    /// is the window lying about what it drew (§7.1.5f, quoted again in
    /// §7.1.5g). So the band that speaks is the band that answers, and what it
    /// answers is the switch drawn in it. **The band is no longer filled**
    /// (user ruling 2026-09-07) — the settings page paints nothing under the
    /// pointer either — and that changes what is drawn, not what is reached.
    Row(usize),
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

/// One row, as the card draws it.
///
/// **No `description_lines`, and that is the whole of v4**: what a row says is
/// one line by construction, so nothing here has to be wrapped and nothing here
/// can grow a second line under somebody's font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowContent {
    /// The gap that separates Folio's rows from the agent rows stands above
    /// this one.
    pub group_break_above: bool,
    pub line: String,
    /// What the pointer resting on this row says.
    pub tip: String,
    pub on: bool,
}

/// Everything the card draws that had to be measured with a real font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Content {
    pub title: String,
    pub rows: Vec<RowContent>,
    /// The faint line above the two verbs, already broken to lines that fit.
    pub settings_lines: Vec<String>,
    pub later: String,
    pub later_width: f32,
    pub done: String,
    pub done_width: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct RowRects {
    /// The whole band — the content column plus the eight pixels it runs past
    /// that column on each side. **What answers a press and what the tooltip
    /// hangs off, and it is one rectangle for both** because it is one
    /// rectangle to the reader. Nothing is painted in it (user ruling
    /// 2026-09-07).
    band: [f32; 4],
    line: (String, [f32; 4]),
    tip: String,
    switch: [f32; 4],
    on: bool,
}

/// Every rectangle the card draws and hit-tests.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    scale: f32,
    frame: [f32; 4],
    /// The Folio mark, square, centred on the title's line box.
    mark: [f32; 4],
    title: (String, [f32; 4]),
    /// The clip the scrolling body is seen through — the content column.
    viewport: [f32; 4],
    /// The same two horizontal lines, out to the card's own edge.
    ///
    /// A row's lit band runs wider than the text column by design, so the thing
    /// that bounds it is the card and not the column; what bounds it vertically
    /// is still the scroller, and those are the two facts this rectangle is.
    body_clip: [f32; 4],
    /// How tall the body is in full, so a caller can clamp a scroll.
    body_height: f32,
    scroll: f32,
    rows: Vec<RowRects>,
    thumb: Option<[f32; 4]>,
    track: Option<[f32; 4]>,
    settings_line: Vec<(String, [f32; 4])>,
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

    /// The scroll that brings a row fully into view, or the one already in force
    /// when it is there.
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
        let ring = FOCUS_RING_TIGHT_OFFSET_LOGICAL_PX * self.scale + FOCUS_RING_WIDTH_LOGICAL_PX;
        let (top, bottom) = (row.band[1], row.band[3]);
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

    /// **What every row offers the tooltip host**: the band the pointer has to
    /// be in, and the sentence it earns.
    ///
    /// The band is cut to the body, so a row scrolled under the fade is not a
    /// row anything can be said about — the same rule [`hit`] applies to a
    /// press, stated once for both.
    #[must_use]
    pub fn tips(&self) -> Vec<(usize, [f32; 4], String)> {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                clipped(row.band, self.body_clip).map(|shown| (index, shown, row.tip.clone()))
            })
            .collect()
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

    let head = px(PADDING_TOP_LOGICAL_PX + TITLE_LINE_LOGICAL_PX + HEADER_MARGIN_BOTTOM_LOGICAL_PX);
    let button_height =
        2.0 * border + px(2.0 * BUTTON_PADDING_Y_LOGICAL_PX + BUTTON_LINE_LOGICAL_PX);
    let foot = px(FOOT_GAP_LOGICAL_PX)
        + content.settings_lines.len().max(1) as f32 * px(SETTINGS_LINE_LINE_LOGICAL_PX)
        + px(SETTINGS_LINE_MARGIN_BOTTOM_LOGICAL_PX)
        + button_height
        + px(PADDING_BOTTOM_LOGICAL_PX);

    let body_height = body_extent(content, scale);
    let room = (surface_height - 2.0 * px(SURFACE_MARGIN_LOGICAL_PX) - 2.0 * border - head - foot)
        .max(px(ROW_HEIGHT_LOGICAL_PX));
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
    let title_box = [
        content_left + px(MARK_LOGICAL_PX + MARK_GAP_LOGICAL_PX),
        cursor,
        content_right,
        cursor + px(TITLE_LINE_LOGICAL_PX),
    ];
    // Centred on the title's line box, left edge on the card's own padding.
    let mark_top = (cursor + (px(TITLE_LINE_LOGICAL_PX) - px(MARK_LOGICAL_PX)) / 2.0).round();
    let mark = [
        content_left.round(),
        mark_top,
        content_left.round() + px(MARK_LOGICAL_PX),
        mark_top + px(MARK_LOGICAL_PX),
    ];
    let title = (content.title.clone(), title_box);
    cursor = title.1[3] + px(HEADER_MARGIN_BOTTOM_LOGICAL_PX);

    let viewport = [
        content_left,
        cursor,
        content_right,
        cursor + viewport_height,
    ];
    let mut rows = Vec::with_capacity(content.rows.len());
    let mut walk = viewport[1] - scroll;
    for row in &content.rows {
        // **Rows of one group abut; the group break is air** (user ruling
        // 2026-09-07). Nothing is drawn between two rows, so there is nothing
        // here to measure a rectangle for — only a walk.
        if row.group_break_above {
            walk += px(GROUP_GAP_LOGICAL_PX);
        }
        let band = [
            content_left - px(ROW_BAND_BLEED_LOGICAL_PX),
            walk,
            content_right + px(ROW_BAND_BLEED_LOGICAL_PX),
            walk + px(ROW_HEIGHT_LOGICAL_PX),
        ];
        let switch_top = (band[1] + band[3] - px(SWITCH_HEIGHT_LOGICAL_PX)) / 2.0;
        let switch = [
            content_right - px(SWITCH_WIDTH_LOGICAL_PX),
            switch_top,
            content_right,
            switch_top + px(SWITCH_HEIGHT_LOGICAL_PX),
        ];
        walk = band[3];
        rows.push(RowRects {
            band,
            // **Every row's line starts on the same x** (user ruling
            // 2026-09-06): the three agent rows carried a coloured silhouette
            // in v4's first drawing and no longer do, so there is one text
            // column on this card and not two.
            line: (
                row.line.clone(),
                [content_left, band[1], text_right, band[3]],
            ),
            tip: row.tip.clone(),
            switch,
            on: row.on,
        });
    }

    // **In the right padding, not in the line's column**: the bar is the one
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

    let cursor = viewport[3] + px(FOOT_GAP_LOGICAL_PX);
    let settings_line: Vec<(String, [f32; 4])> = content
        .settings_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let line_top = cursor + index as f32 * px(SETTINGS_LINE_LINE_LOGICAL_PX);
            (
                line.clone(),
                [
                    content_left,
                    line_top,
                    content_right,
                    line_top + px(SETTINGS_LINE_LINE_LOGICAL_PX),
                ],
            )
        })
        .collect();
    let cursor = cursor
        + content.settings_lines.len().max(1) as f32 * px(SETTINGS_LINE_LINE_LOGICAL_PX)
        + px(SETTINGS_LINE_MARGIN_BOTTOM_LOGICAL_PX);

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
        mark,
        title,
        viewport,
        body_clip: [
            frame[0] + border,
            viewport[1],
            frame[2] - border,
            viewport[3],
        ],
        body_height,
        scroll,
        rows,
        thumb,
        track,
        settings_line,
        later: (content.later.clone(), later_rect),
        done: (content.done.clone(), done_rect),
    }
}

/// How tall the body is in full, before any of it is hidden.
fn body_extent(content: &Content, scale: f32) -> f32 {
    let px = |value: f32| value * scale;
    let mut height = 0.0;
    for row in &content.rows {
        if row.group_break_above {
            height += px(GROUP_GAP_LOGICAL_PX);
        }
        height += px(ROW_HEIGHT_LOGICAL_PX);
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
    // A row scrolled out of the viewport is not a row anybody can press: the
    // clip is where it stops being drawn, so it is where it stops answering.
    for (index, row) in layout.rows.iter().enumerate() {
        if contains(row.band, x, y) && contains(layout.body_clip, x, y) {
            return Target::Row(index);
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

// ── the Folio mark ─────────────────────────────────────────────────────────

/// The shipped icon's own bytes.
///
/// **The mark on this card is the mark on the taskbar, and there is one copy of
/// it** (v4 §3, user ruling 2026-09-06 — `folio.ico` is the final icon and not
/// a placeholder). `crates/bt-app/build.rs` reads this exact path and links it
/// into `folio.exe` as icon group 1; drawing anything else at the top of the
/// first thing a new reader ever sees would make the first impression disagree
/// with the icon they just double-clicked. Restating the drawing's geometry in
/// Rust would be a second source free to drift from the file, so the file is
/// what is read.
const FOLIO_ICO: &[u8] = include_bytes!("../../../design/assets/app-icon/folio.ico");

/// One entry of `folio.ico`, decoded.
struct MarkEntry {
    side: u32,
    rgba: std::sync::Arc<[u8]>,
}

/// Every uncompressed entry of `folio.ico`, smallest first.
///
/// **The classic-DIB entries only** — 16 through 64. `.ico` stores 128 and 256
/// as PNG, and this card never wants them: the mark is 22 logical pixels, so
/// even a 300% monitor asks for 66, and a decoder for two entries nothing on
/// this card can use would be a dependency bought for nothing.
fn mark_entries() -> &'static [MarkEntry] {
    static ENTRIES: std::sync::OnceLock<Vec<MarkEntry>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| {
        let mut entries = Vec::new();
        let count = u16::from_le_bytes([FOLIO_ICO[4], FOLIO_ICO[5]]) as usize;
        for index in 0..count {
            let at = 6 + 16 * index;
            let length =
                u32::from_le_bytes(FOLIO_ICO[at + 8..at + 12].try_into().unwrap()) as usize;
            let offset =
                u32::from_le_bytes(FOLIO_ICO[at + 12..at + 16].try_into().unwrap()) as usize;
            let image = &FOLIO_ICO[offset..offset + length];
            // A PNG entry opens with the signature; a DIB entry opens with the
            // 40-byte `BITMAPINFOHEADER`. That first word is the whole test.
            if u32::from_le_bytes(image[0..4].try_into().unwrap()) != 40 {
                continue;
            }
            let side = i32::from_le_bytes(image[4..8].try_into().unwrap()) as u32;
            let depth = u16::from_le_bytes(image[14..16].try_into().unwrap());
            if depth != 32 {
                continue;
            }
            // BGRA, bottom-up, with the mandatory AND mask after it — which a
            // 32-bit icon's transparency does not use and this does not read.
            let pixels = &image[40..];
            let mut rgba = vec![0_u8; (side * side * 4) as usize];
            let stride = (side * 4) as usize;
            for row in 0..side {
                let source = ((side - 1 - row) * side * 4) as usize;
                let target = (row * side * 4) as usize;
                rgba[target..target + stride].copy_from_slice(&pixels[source..source + stride]);
                // BGRA to RGBA: blue and red change places, and the two the
                // swap does not touch are already where they belong.
                for column in 0..side as usize {
                    rgba.swap(target + column * 4, target + column * 4 + 2);
                }
            }
            entries.push(MarkEntry {
                side,
                rgba: rgba.into(),
            });
        }
        entries.sort_by_key(|entry| entry.side);
        entries
    })
}

/// The entry to draw a mark this many physical pixels wide.
///
/// The smallest one that is at least as big, so the sampler only ever shrinks —
/// Windows' own rule for picking an icon, and the reason `make-folio-ico.py`
/// solves nine sizes instead of one.
fn mark_entry(side_px: f32) -> Option<&'static MarkEntry> {
    let entries = mark_entries();
    entries
        .iter()
        .find(|entry| entry.side as f32 >= side_px)
        .or_else(|| entries.last())
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
    let mut images = Vec::new();

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

    // **The mark keeps its tile on a dark card** (user ruling 2026-09-06). The
    // icon's ground is `#202027` and the dark card's is `#202020`: at seven
    // levels apart the tile is gone and only the paper sheet inside it is
    // visible, which is a headless mark at the top of the first thing a reader
    // ever sees. It is given the card's own edge — one hairline of `--border`,
    // at the tile's own corner radius — so the tile ends where the card says
    // its own surfaces end. Nothing is drawn in the light theme, where a
    // graphite tile on white needs no help and an edge would be noise.
    if !bt_render::background_is_light(palette.dialog_surface) {
        quads.extend(rounded_overlay_halo(
            layout.mark,
            px(MARK_LOGICAL_PX) * MARK_TILE_RADIUS_UNITS,
            border,
            palette.menu_border,
            alpha(palette.menu_border_alpha),
        ));
    }
    if let Some(entry) = mark_entry(layout.mark[2] - layout.mark[0]) {
        images.push(bt_render::ChromeIcon {
            key: format!("first-run-folio-mark-{}", entry.side),
            rect: layout.mark,
            rgba: std::sync::Arc::clone(&entry.rgba),
            width_px: entry.side,
            height_px: entry.side,
            opacity: 1.0,
            clip: None,
            above_text: false,
        });
    }

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
    // **A row under the pointer is not painted** (user ruling 2026-09-07). v4
    // filled the whole band with `--hover`; the settings page, whose row shape
    // this card borrows, paints nothing when the pointer crosses a row — only
    // the control at its right end answers a hover — and the reader asked for
    // the same here. What the pointer still earns is the row's tooltip, and
    // what a press still reaches is the whole band: the rectangle did not
    // change, only what is drawn in it.
    for (index, row) in layout.rows.iter().enumerate() {
        if let Some(shown) = clipped(row.line.1, viewport) {
            labels.push(ChromeLabel {
                mono: false,
                text: row.line.0.clone(),
                rect: row.line.1,
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
        if clipped(row.switch, viewport).is_some() {
            push_switch(&mut quads, row.switch, row.on, scale, palette, viewport);
            if focus == Some(Focus::Switch(index)) {
                // **Cut to the card's own edge, not to the text column** (user
                // ruling 2026-09-07: the ring on the first switch came up with
                // its right-hand side sliced off). The switch's right edge is
                // the content column's right edge, and the ring stands one
                // pixel outside the control and is two more wide — so a ring
                // cut to the column loses exactly the three logical pixels that
                // are on the far side of it. What it may not cross is the
                // card's own edge, and what it may not escape is the scroller,
                // and `body_clip` is both of those facts.
                quads.extend(clip_quads(
                    focus_ring(
                        row.switch,
                        scale,
                        FOCUS_RING_TIGHT_OFFSET_LOGICAL_PX,
                        palette.accent,
                    ),
                    layout.body_clip,
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

    for (text, rect) in &layout.settings_line {
        labels.push(ChromeLabel {
            mono: false,
            text: text.clone(),
            rect: *rect,
            font_size_px: px(SETTINGS_LINE_FONT_LOGICAL_PX),
            color: palette.dialog_muted_text,
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
            images,
            ..Default::default()
        },
        OverlayLayer {
            quads: over,
            ..Default::default()
        },
    ]
}

/// The tile's corner radius as a fraction of its side — `GROUND_RADIUS` in
/// `design/assets/app-icon/make-folio-ico.py`, which is the file that draws
/// `folio.ico`.
const MARK_TILE_RADIUS_UNITS: f32 = 0.22;

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
            knob_face(palette, on),
            1.0,
        ),
        clip,
    ));
}

/// What the knob is made of.
///
/// **`--menu`, except on a dark card with the switch off** (user ruling
/// 2026-09-06). `.aswitch i` is `--menu` in both themes with no dark override,
/// and in the dark that is `#2A2A2A` on an `--active` track that resolves to
/// `#343434`: six off switches, each with a hole punched in the left end of it,
/// on the loudest surface this program has ever put them on. Fluent's own dark
/// toggle answers this the other way round from its light one — the knob is the
/// *ink*, dark on the light theme's light track and light on the dark theme's
/// dark one — so the off knob takes `--ink`, the card's own strongest ink and
/// the colour of the very line beside it. **No new colour, and the light theme
/// is untouched**: there, `--menu` is white on a near-white track and the
/// knob's `0 1px 3px` lift is what separates them, exactly as the mock-up has
/// it. A knob that is *on* stays `--menu` in both themes, which is also
/// Fluent's answer: the dark theme's accent is a pale blue, and a pale knob on
/// it would be the switch with its state rubbed out.
fn knob_face(palette: bt_render::ChromePalette, on: bool) -> [u8; 3] {
    if on || bt_render::background_is_light(palette.dialog_surface) {
        palette.menu_surface
    } else {
        palette.dialog_title_text
    }
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
/// them. A row's own line is never measured: it is one line by construction,
/// and the switch beside it is placed against the row rather than against it.
pub const MEASURED_SETTINGS_LINE_FONT_LOGICAL_PX: f32 = SETTINGS_LINE_FONT_LOGICAL_PX;
pub const MEASURED_BUTTON_FONT_LOGICAL_PX: f32 = BUTTON_FONT_LOGICAL_PX;

/// The width the faint line above the buttons is wrapped to — the card's whole
/// content column, since v4 took the `Open settings` link off the end of it.
#[must_use]
pub fn settings_line_width(surface_width: f32, scale: f32) -> f32 {
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    card_width(surface_width, scale) - 2.0 * border - 2.0 * PADDING_X_LOGICAL_PX * scale
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
            offered.iter().all(|row| !row.group_break_above),
            "the card is opening a group gap with nothing in the group"
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
        assert!(
            rows(&configured)[3].group_break_above,
            "the gap moved off the first agent row that is actually shown"
        );
        assert!(
            rows(&configured)[..3]
                .iter()
                .all(|row| !row.group_break_above),
            "a group gap is being opened inside Folio's own three rows"
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
        assert_eq!(explorer_shape(&both).line(), Text::FirstRunRowExplorer11);
        let classic = Machine {
            explorer_first_page_available: false,
            ..both
        };
        assert_eq!(explorer_shape(&classic), ExplorerShape::ClassicOnly);
        assert_eq!(explorer_shape(&classic).line(), Text::FirstRunRowExplorer10);
        assert!(
            rows(&classic)
                .iter()
                .any(|row| row.kind == RowKind::Explorer),
            "a machine with no package lost the entry it could have had"
        );
    }

    /// RED (user ruling 2026-09-07) — **the card's one switch asks for the
    /// highest place this machine can honour, and never for `Off`.**
    ///
    /// What that switch has always meant is "as much of this as this machine can
    /// do", which is [`ExplorerPlace::FirstPage`] where the first page is
    /// reachable and the classic entry everywhere else. **Since the second
    /// ruling of that day the Settings row's switch means exactly the same
    /// thing, through the same function** — [`ExplorerShape::place`] is a call to
    /// [`crate::explorer_menu::place_when_on`] and so is the row's `On`, which is
    /// why the two surfaces cannot drift.
    ///
    /// `Off` is not among the answers a card can spend at all: a row left off
    /// spends nothing, because off is the factory state and a card that pressed
    /// it would take away an entry a previous install left behind.
    ///
    /// MUTATION: answer `FirstPage` for both shapes — stop calling the shared
    /// function — and a Windows 10 reader's `Done` asks to register a package
    /// Windows there has no page for.
    #[test]
    fn the_cards_switch_asks_for_the_highest_place_this_machine_can_reach() {
        assert_eq!(
            ExplorerShape::FirstPageAndClassic.place(),
            ExplorerPlace::FirstPage
        );
        assert_eq!(
            ExplorerShape::ClassicOnly.place(),
            ExplorerPlace::ShowMoreOptions
        );
        let mut all_on = rows(&every_row());
        for row in &mut all_on {
            row.on = true;
        }
        for shape in [
            ExplorerShape::FirstPageAndClassic,
            ExplorerShape::ClassicOnly,
        ] {
            assert!(
                !applications(&all_on, shape).contains(&Application::Explorer(ExplorerPlace::Off)),
                "{shape:?}: a card asked for the answer that removes things"
            );
            assert!(
                applications(&all_on, shape).contains(&Application::Explorer(shape.place())),
                "{shape:?}: the card spent a place its machine was not opened with"
            );
        }
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
        // **One press for Explorer since 2026-09-07**, and since that day's
        // second ruling it is `On` — the row is a switch and what `On` reaches is
        // the machine's answer. So the card's shape and the dialog's `On` land on
        // the same place by construction: the card presses `On`, the dialog reads
        // the same machine fact, and neither spells a place of its own.
        for (shape, offered) in [
            (ExplorerShape::FirstPageAndClassic, true),
            (ExplorerShape::ClassicOnly, false),
        ] {
            assert_eq!(
                settings::explorer_place_requested(
                    target(Application::Explorer(shape.place())),
                    offered
                ),
                Some(shape.place()),
                "{shape:?}: the card's switch and the row's switch are one switch"
            );
        }
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
                // **One application since 2026-09-07**, carrying the place the
                // Settings row now spells. The pair the two applications used to
                // be is what `FirstPage` means.
                Application::Explorer(ExplorerPlace::FirstPage),
                Application::PowerShellOffer(true),
                Application::PowerShellIntent,
                Application::ClaudeHooks,
                Application::CodexNotify,
                Application::CopilotHooks
            ]
        );
        assert!(
            applications(&all_on, ExplorerShape::ClassicOnly)
                .contains(&Application::Explorer(ExplorerPlace::ShowMoreOptions)),
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

    /// PIN (§7.56 §6, v4 §5) — **the focus order is every switch and then the
    /// two verbs, and it is a ring.**
    ///
    /// The card is modal, so focus never leaves it. `↑`/`↓` move between
    /// switches only, because the list is a list. **There is no third stop in
    /// the foot**: v4 took the `Open settings` link off the card, and what
    /// stands there now is a statement of fact rather than an affordance.
    ///
    /// MUTATIONS:
    /// ① let `Tab` fall off either end and the keyboard leaves a modal card,
    ///    landing in a shell behind a scrim;
    /// ② let the arrows walk onto the buttons and `↓` from the last row presses
    ///    nothing while looking as though it might;
    /// ③ leave the link in the ring and `Tab` stops on a faint line that
    ///    nothing can press.
    #[test]
    fn the_focus_walks_the_card_in_a_ring_and_the_arrows_stay_in_the_list() {
        assert_eq!(
            focus_order(3),
            [
                Focus::Switch(0),
                Focus::Switch(1),
                Focus::Switch(2),
                Focus::Later,
                Focus::Done
            ]
        );
        assert_eq!(stepped(Focus::Switch(2), 3, true), Focus::Later);
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
        assert_eq!(arrowed(Focus::Later, 3, true), Focus::Switch(0));
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
        // registrations, so an Explorer row that is on asks for the top place.
        card.flip(1);
        assert!(
            card.done()
                .contains(&Application::Explorer(ExplorerPlace::FirstPage)),
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
                Application::Explorer(ExplorerPlace::ShowMoreOptions),
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

    /// The window `card-v4-*.png` were composed on: a 1133 × 548 logical window
    /// at 1.5×.
    const SURFACE: (f32, f32) = (1699.0, 822.0);
    /// A window short enough that six rows do not fit in it. **v4's body does
    /// not scroll at the reference window** — that is the one measurable side
    /// effect of taking the sentences off the rows (v4 §0) — so the machinery
    /// has to be exercised where it actually fires.
    const SHORT_SURFACE: (f32, f32) = (1699.0, 560.0);
    const SCALE: f32 = 1.5;

    fn measured(rows: &[Row]) -> Content {
        Content {
            title: Text::FirstRunTitle.text().to_owned(),
            rows: rows
                .iter()
                .map(|row| RowContent {
                    group_break_above: row.group_break_above,
                    line: row.line.text().to_owned(),
                    tip: row.tip.text().to_owned(),
                    on: row.on,
                })
                .collect(),
            settings_lines: vec![Text::FirstRunSettingsLine.text().to_owned()],
            later: Text::FirstRunLater.text().to_owned(),
            later_width: 50.0,
            done: Text::FirstRunDone.text().to_owned(),
            done_width: 40.0,
        }
    }

    /// Relative luminance, near enough for "which of these two is lighter".
    fn luma(colour: [u8; 3]) -> f32 {
        0.2126 * f32::from(colour[0])
            + 0.7152 * f32::from(colour[1])
            + 0.0722 * f32::from(colour[2])
    }

    /// PIN (§7.56 §9) — **the head and the foot are pinned and the body between
    /// them scrolls.**
    ///
    /// The pinned foot is why the scroll is safe: `Done`, `Not now` and the one
    /// line that says every row is in Settings are never the thing that
    /// scrolled away.
    ///
    /// MUTATIONS:
    /// ① let the card grow past the window and the two verbs are off the bottom
    ///    of the screen on a short window, with no way to answer;
    /// ② put the foot inside the scroller and the reader has to find the buttons
    ///    before they can press them.
    #[test]
    fn the_body_scrolls_and_the_two_verbs_never_do() {
        let content = measured(&rows(&every_row()));
        let tall = layout(&content, SHORT_SURFACE.0, SHORT_SURFACE.1, SCALE, 0.0);
        assert!(
            tall.scrolls(),
            "six rows did not scroll in a window 373 logical pixels tall"
        );
        assert!(
            tall.frame[3] - tall.frame[1] <= SHORT_SURFACE.1,
            "the card is taller than the window it is drawn in"
        );
        let scrolled = layout(
            &content,
            SHORT_SURFACE.0,
            SHORT_SURFACE.1,
            SCALE,
            tall.scroll_extent(),
        );
        assert_eq!(
            scrolled.later.1, tall.later.1,
            "`Not now` moved when the body was scrolled"
        );
        assert_eq!(scrolled.done.1, tall.done.1);
        assert_eq!(scrolled.title.1, tall.title.1);
        assert_eq!(
            scrolled.mark, tall.mark,
            "the product's own mark scrolled off the top of its own card"
        );
        assert_eq!(
            scrolled.settings_line[0].1, tall.settings_line[0].1,
            "the one line that says where these rows live scrolled away"
        );
        assert!(
            scrolled.rows[0].band[1] < tall.rows[0].band[1],
            "the body did not move"
        );
    }

    /// PIN (§7.56, v4 §0) — **at the reference window the card does not
    /// scroll.**
    ///
    /// This is what taking the sentences off the rows bought, and it is worth a
    /// gate of its own: v3 needed 376 logical pixels of body in 315 of room and
    /// clipped every reader on their first launch, on the very card that is
    /// meant to be the first impression.
    ///
    /// MUTATION: put a sentence back under any row — or give a row a second
    /// line under somebody's font — and the first thing a new machine shows is
    /// a scrollbar.
    #[test]
    fn six_rows_fit_the_reference_window_without_a_bar() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        assert!(
            !placed.scrolls(),
            "the card that is a machine's first impression opens already clipped"
        );
        assert!(
            build(&placed, SURFACE, None, None)
                .iter()
                .all(|layer| layer.quads.iter().all(|quad| quad.color != SCROLLBAR_INK)),
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
        let content = measured(&rows(&every_row()));
        let placed = layout(&content, SHORT_SURFACE.0, SHORT_SURFACE.1, SCALE, 0.0);
        let last = placed.rows.len() - 1;
        let to = placed.scroll_showing(Focus::Switch(last));
        assert!(
            to > 0.0,
            "the ring walked onto a row that is under the fade and nothing moved"
        );
        let after = layout(&content, SHORT_SURFACE.0, SHORT_SURFACE.1, SCALE, to);
        assert!(
            after.rows[last].band[3] <= after.viewport[3],
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

    /// PIN (§7.56, v4 §2 / §8) — **the whole row answers a press, and a row
    /// scrolled out of the body answers nothing.**
    ///
    /// A hovered row is filled end to end with `--hover` and carries a tooltip
    /// about itself; this window's standing rule is that a mark which answers a
    /// hover and not a click is the window lying about what it drew (§7.1.5f,
    /// §7.1.5g). So the band that lights is the band that answers, and the clip
    /// is where both stop.
    ///
    /// MUTATIONS:
    /// ① hit-test the switch alone and five sixths of a lit row does nothing
    ///    when it is pressed;
    /// ② drop the viewport check and a click on the faint line flips whichever
    ///    row happens to be under the foot.
    #[test]
    fn a_press_anywhere_on_a_row_is_a_press_on_its_switch_and_the_fade_ends_it() {
        let content = measured(&rows(&every_row()));
        let placed = layout(&content, SHORT_SURFACE.0, SHORT_SURFACE.1, SCALE, 0.0);
        let middle = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        let first = &placed.rows[0];
        let (x, y) = middle(first.switch);
        assert_eq!(hit(&placed, x, y), Target::Row(0));
        // The far left of the same band — where the line's first glyph is, and
        // as far from the switch as the row goes.
        assert_eq!(
            hit(
                &placed,
                f64::from(first.band[0] + 1.0),
                f64::from((first.band[1] + first.band[3]) / 2.0)
            ),
            Target::Row(0),
            "the lit band answered a hover and not a press"
        );
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
        assert_eq!(
            hit(&placed, 4.0, 4.0),
            Target::Panel,
            "the card is modal, so a press on its scrim is still its own"
        );
    }

    /// PIN (v4, user ruling 2026-09-06 — 「六行统一左对齐，agent 行不画标记」)
    /// — **every row's line starts on the same x, and nothing is drawn in front
    /// of any of them.**
    ///
    /// v4's first drawing put a 16px coloured silhouette at the head of each of
    /// the three agent rows, which indented those three lines by 26 pixels and
    /// made the card two text columns instead of one. The marks are struck: the
    /// row names its agent in words, the break above the group already says the
    /// three belong together, and one repeated silhouette in three colours gave
    /// a colour-blind reader nothing at all.
    ///
    /// MUTATIONS:
    /// ① indent the agent rows past a mark and the six lines stand on two
    ///    different lefts, which is the thing that was reported;
    /// ② push the marks back as sprites or images and the card carries seven
    ///    pictures where it should carry one.
    #[test]
    fn every_row_line_starts_on_one_x_and_the_only_picture_is_the_folio_mark() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        let left = placed.rows[0].line.1[0];
        for (index, row) in placed.rows.iter().enumerate() {
            assert!(
                (row.line.1[0] - left).abs() < 0.01,
                "row {index}'s line starts at {} while the first starts at {left} — the card has \
                 grown a second text column",
                row.line.1[0]
            );
        }
        let layers = build(&placed, SURFACE, None, None);
        assert!(
            layers.iter().all(|layer| layer.sprites.is_empty()),
            "the card is drawing marks it has none of"
        );
        let images: Vec<_> = layers
            .iter()
            .flat_map(|layer| layer.images.iter())
            .collect();
        assert_eq!(
            images.len(),
            1,
            "the card draws {} pictures; the Folio mark is the only one it has",
            images.len()
        );
        assert_eq!(
            images[0].rect, placed.mark,
            "the one picture on the card is not the mark in the header"
        );
    }

    /// PIN (§7.56 ⑦, user ruling 2026-09-07) — **a row under the pointer is
    /// not painted.**
    ///
    /// v4 filled the whole band with `--hover`. The settings page — whose row
    /// shape this card borrows — paints nothing when the pointer crosses a row;
    /// only the control at the row's right end answers a hover. What the
    /// pointer still earns here is the row's tooltip, and what a press still
    /// reaches is the whole band, so the executable form of the ruling is that
    /// a hovered card and an unhovered one draw **the same picture**.
    ///
    /// MUTATION: push the `--hover` fill back and the two pictures differ by
    /// the band's own quads, which is the report.
    #[test]
    fn a_row_under_the_pointer_is_not_painted() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        let cold = build(&placed, SURFACE, None, None);
        for index in 0..placed.rows.len() {
            let hovered = build(&placed, SURFACE, Some(Target::Row(index)), None);
            assert_eq!(
                quads_of(&hovered),
                quads_of(&cold),
                "the pointer on row {index} changed what the card draws"
            );
        }
    }

    /// PIN (§7.56 ⑧, user ruling 2026-09-07) — **the ring waits for the
    /// keyboard, and when it comes it is not sliced off at the text column.**
    ///
    /// Two halves of one report. The card opens with the focus on its first
    /// switch and the ring put away, because a ring drawn before anybody has
    /// used a keyboard claims one; and the ring `Tab` then lights stands one
    /// pixel outside a switch whose right edge **is** the content column's, so
    /// a ring cut to that column loses its whole right-hand side.
    ///
    /// MUTATIONS:
    /// ① light the ring on the way into the key handler — on a bare `Shift`, or
    ///    on any key the card swallows — and `focus_ring` answers before a
    ///    keyboard has moved anything;
    /// ② cut the ring to the viewport instead of to the card and its outermost
    ///    quad stops exactly on the switch's own right edge, which is the
    ///    photograph.
    #[test]
    fn the_ring_waits_for_the_keyboard_and_is_not_cut_when_it_comes() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        let mut card = Card::default();
        card.open(rows(&every_row()), ExplorerShape::ClassicOnly);
        assert_eq!(
            card.focus(),
            Some(Focus::Switch(0)),
            "the card does not open on its first switch"
        );
        assert!(
            card.focus_ring().is_none(),
            "the card opened with a ring on a control nobody has walked to"
        );
        let dark = build(&placed, SURFACE, None, card.focus_ring());

        // What `Tab` does, and nothing else.
        let stepped_to = stepped(card.focus().expect("a focus"), card.rows().len(), true);
        card.move_focus(stepped_to);
        let Some(Focus::Switch(index)) = card.focus_ring() else {
            panic!("Tab left the ring away");
        };
        let lit = build(&placed, SURFACE, None, card.focus_ring());

        let ring = added(&lit, &dark);
        assert!(!ring.is_empty(), "Tab drew no ring at all");
        let clip = placed.body_clip;
        for quad in &ring {
            assert!(
                quad.rect[0] >= clip[0] - 0.01
                    && quad.rect[1] >= clip[1] - 0.01
                    && quad.rect[2] <= clip[2] + 0.01
                    && quad.rect[3] <= clip[3] + 0.01,
                "a ring quad at {:?} is outside the clip {clip:?} it was cut with",
                quad.rect
            );
        }
        // The ring stands `offset + width` outside the control on every side,
        // and the switch's right edge is the content column's — so this is the
        // assertion the old clip failed.
        let switch = placed.rows[index].switch;
        let reach = (FOCUS_RING_TIGHT_OFFSET_LOGICAL_PX + FOCUS_RING_WIDTH_LOGICAL_PX) * SCALE;
        let right = ring
            .iter()
            .fold(f32::MIN, |far, quad| far.max(quad.rect[2]));
        assert!(
            (right - (switch[2] + reach)).abs() < 0.51,
            "the ring reaches {right} where the control's own edge is {} and the ring should \
             stand {reach} beyond it",
            switch[2]
        );
    }

    /// Every quad the card draws, in the order it draws them.
    fn quads_of(layers: &[OverlayLayer]) -> Vec<OverlayQuad> {
        layers
            .iter()
            .flat_map(|layer| layer.quads.iter().copied())
            .collect()
    }

    /// The quads `lit` draws that `dark` does not — a multiset difference, so a
    /// quad drawn twice in both is not reported as new.
    fn added(lit: &[OverlayLayer], dark: &[OverlayLayer]) -> Vec<OverlayQuad> {
        let mut rest = quads_of(dark);
        let mut extra = Vec::new();
        for quad in quads_of(lit) {
            if let Some(at) = rest.iter().position(|other| *other == quad) {
                rest.remove(at);
            } else {
                extra.push(quad);
            }
        }
        extra
    }

    /// PIN (§7.56 ⑦, user ruling 2026-09-07 — 「要不要这里也不分隔保持一致」) —
    /// **nothing is drawn between two rows, and the group break is air.**
    ///
    /// v4 drew a `--border-soft` hairline between every pair of rows and a
    /// `--border` one above the agent group. The settings page — whose row shape
    /// this card borrows, and where every one of these six rows also lives —
    /// draws no line between its rows at all, and the reader asked for the same
    /// here. So rows of one group abut, and what separates the two groups is
    /// `GROUP_GAP_LOGICAL_PX` of nothing.
    ///
    /// **How "no separator" is stated so a machine can check it.** The card's
    /// face and its scrim both cross the body, but neither is *inside* it: they
    /// run from above the first row to below the last. A hairline between two
    /// rows is the other shape — it fits inside the body and it crosses the
    /// column the six lines are written in. The switches do not: they live out
    /// at the card's right-hand padding, past where any line ends.
    ///
    /// MUTATIONS:
    /// ① push the group's `--border` rule back and the one quad inside the body
    ///    that crosses the text column is exactly it;
    /// ② push the `--border-soft` ones back between rows of one group and the
    ///    same assertion catches four more;
    /// ③ give the group break its v4 spacing — air, hairline, air — and the gap
    ///    assertion reports 19 logical pixels where 16 were asked for.
    #[test]
    fn no_line_is_drawn_between_two_rows_and_the_group_break_is_air() {
        let px = |value: f32| value * SCALE;
        let content = measured(&rows(&every_row()));
        let placed = layout(&content, SURFACE.0, SURFACE.1, SCALE, 0.0);
        let (text_left, text_right) = (placed.rows[0].line.1[0], placed.rows[0].line.1[2]);
        for layer in build(&placed, SURFACE, None, None) {
            for quad in layer.quads {
                let inside_body = quad.rect[1] >= placed.viewport[1] - 0.01
                    && quad.rect[3] <= placed.viewport[3] + 0.01;
                let crosses_the_column = quad.rect[0] < text_right && quad.rect[2] > text_left;
                assert!(
                    !(inside_body && crosses_the_column),
                    "a quad at {:?} lies inside the body and crosses the column the rows are \
                     written in — the card has grown a separator",
                    quad.rect
                );
            }
        }
        for (index, pair) in placed.rows.windows(2).enumerate() {
            let gap = pair[1].band[1] - pair[0].band[3];
            let want = if content.rows[index + 1].group_break_above {
                px(GROUP_GAP_LOGICAL_PX)
            } else {
                0.0
            };
            assert!(
                (gap - want).abs() < 0.01,
                "the gap above row {} is {gap} and should be {want}",
                index + 1
            );
        }
    }

    /// PIN (v4 §1, user ruling 2026-09-06) — **the header is the shipped icon
    /// and the greeting, and the greeting stands clear of the mark.**
    ///
    /// `design/assets/app-icon/folio.ico` is what `build.rs` links into
    /// `folio.exe`, so the first thing a reader sees agrees with the icon they
    /// just double-clicked. There is no third line: v4 dropped the muted
    /// sentence under the title (user ruling), so the header is two things.
    ///
    /// MUTATIONS:
    /// ① set the title on the card's own padding and it is drawn over the mark;
    /// ② hand the sampler an entry smaller than the box and the mark on a 200%
    ///    monitor is an upscale of a 24-pixel drawing.
    #[test]
    fn the_header_is_the_shipped_mark_and_the_greeting_beside_it() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        let side = placed.mark[2] - placed.mark[0];
        assert!(
            (side - MARK_LOGICAL_PX * SCALE).abs() < 0.51,
            "the mark is {side} physical pixels wide, not 22 logical"
        );
        assert!(
            (placed.mark[3] - placed.mark[1] - side).abs() < 0.01,
            "not square"
        );
        assert!(
            placed.title.1[0] >= placed.mark[2],
            "the greeting is set over the mark"
        );
        assert!(
            (placed.title.1[0] - placed.mark[2] - MARK_GAP_LOGICAL_PX * SCALE).abs() < 0.51,
            "the gap between the mark and the greeting is not the one the spec names"
        );
        // The mark's own line box is the title's, so the two share a centre.
        assert!(
            ((placed.mark[1] + placed.mark[3]) / 2.0
                - (placed.title.1[1] + placed.title.1[3]) / 2.0)
                .abs()
                < 1.01,
            "the mark is not centred on the greeting's line"
        );
        let entry = mark_entry(side).expect("folio.ico carries no uncompressed entry");
        assert!(
            entry.side as f32 >= side,
            "the mark is being upscaled from a {}px entry into a {side}px box",
            entry.side
        );
        assert_eq!(
            entry.rgba.len(),
            (entry.side * entry.side * 4) as usize,
            "the decoded entry is not a square of RGBA"
        );
        // Decoded the right way up and the right way round: `make-folio-ico.py`
        // puts the graphite tile in the corners and the paper across the middle.
        let at = |x: u32, y: u32| {
            let index = ((y * entry.side + x) * 4) as usize;
            [
                entry.rgba[index],
                entry.rgba[index + 1],
                entry.rgba[index + 2],
            ]
        };
        let middle = at(entry.side / 2, entry.side / 2);
        let corner = at(entry.side / 2, 2);
        assert!(
            luma(middle) > luma(corner) + 40.0,
            "the mark decoded to {middle:?} in the middle and {corner:?} at the top, which is not \
             a pale sheet on a graphite tile"
        );
    }

    /// PIN (v4 §3, user ruling 2026-09-06 — the dark card's tile) — **on a dark
    /// plane the mark is given the card's own edge.**
    ///
    /// The icon's ground is `#202027` and the dark card's is `#202020`: seven
    /// levels apart, so the tile disappears and only the paper sheet inside it
    /// is left standing on nothing. One hairline of `--border`, at the tile's
    /// own corner radius, puts the edge back where the drawing has one. Nothing
    /// is drawn in the light theme, where a graphite tile on white needs no
    /// help.
    ///
    /// MUTATION: draw the edge in both themes and the light card carries a ring
    /// round a mark that already had one.
    #[test]
    fn a_dark_card_gives_the_mark_the_edge_its_own_ground_takes_away() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        // The edge is `--border` ink laid inside the mark's own box, which no
        // other quad on this card is: the rows' rules run the whole content
        // width and the card's own border is drawn round the frame.
        let edge = |palette: bt_render::ChromePalette| {
            let mut quads = Vec::new();
            if !bt_render::background_is_light(palette.dialog_surface) {
                quads.extend(rounded_overlay_halo(
                    placed.mark,
                    MARK_LOGICAL_PX * SCALE * MARK_TILE_RADIUS_UNITS,
                    (FLOAT_WINDOW_BORDER_LOGICAL_PX * SCALE).max(1.0),
                    palette.menu_border,
                    f32::from(palette.menu_border_alpha) / 255.0,
                ));
            }
            quads
        };
        assert!(
            !edge(bt_render::DARK_CHROME).is_empty(),
            "the dark card's mark has no edge, so its tile is the card"
        );
        assert!(
            edge(bt_render::LIGHT_CHROME).is_empty(),
            "the light card is ringing a mark that stands out on its own"
        );
        // And the edge is actually lighter than both grounds it stands between,
        // which is the whole point of taking `--border` rather than a shadow.
        let dark = bt_render::DARK_CHROME;
        let over = |ink: [u8; 3], alpha: f32, ground: [u8; 3]| {
            let mix = |a: u8, b: u8| f32::from(b) + (f32::from(a) - f32::from(b)) * alpha;
            luma([
                mix(ink[0], ground[0]).round() as u8,
                mix(ink[1], ground[1]).round() as u8,
                mix(ink[2], ground[2]).round() as u8,
            ])
        };
        let edge_luma = over(
            dark.menu_border,
            f32::from(dark.menu_border_alpha) / 255.0,
            dark.dialog_surface,
        );
        assert!(
            edge_luma > luma(dark.dialog_surface) + 8.0,
            "the edge {edge_luma} is not distinguishable from the card it is drawn on"
        );
    }

    /// PIN (v4 §3 / §8, user ruling 2026-09-06) — **every row hands the tooltip
    /// its own mechanism sentence, over the whole band, and a row under the
    /// fade hands it nothing.**
    ///
    /// This is where v3's explanations went. The reader is owed the address of
    /// their own files — v3 §10.1 — and owed is not the same as shown unasked;
    /// the row line is the decision and the tooltip is the receipt.
    ///
    /// MUTATIONS:
    /// ① hang the tip on the switch and the sentence is unreachable from five
    ///    sixths of the band that lights up to promise it;
    /// ② hand every row the same string and the card names one file six times;
    /// ③ skip the clip and a row scrolled under the foot answers a hover that
    ///    landed on the buttons.
    #[test]
    fn every_row_hands_the_tooltip_the_file_that_row_writes() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        let tips = placed.tips();
        assert_eq!(tips.len(), placed.rows.len(), "a row has nothing to say");
        for (index, rect, text) in &tips {
            assert_eq!(
                *rect, placed.rows[*index].band,
                "row {index}'s tip is hung on something narrower than the band that lights up"
            );
            assert!(!text.trim().is_empty());
        }
        // Each of the four rows that writes a file the reader owns names that
        // file, and no two rows name the same one.
        for (row, named) in [
            (2_usize, "$PROFILE"),
            (3, "~/.claude/settings.json"),
            (4, "~/.codex/config.toml"),
            (5, "~/.copilot/hooks/folio.json"),
        ] {
            assert!(
                tips[row].2.contains(named),
                "the row that writes {named} does not name it: {:?}",
                tips[row].2
            );
        }
        let said: std::collections::BTreeSet<&str> =
            tips.iter().map(|(_, _, text)| text.as_str()).collect();
        assert_eq!(said.len(), tips.len(), "two rows say the same sentence");
        // Under the fade there is nothing to hover, so there is nothing to say.
        let short = layout(
            &measured(&rows(&every_row())),
            SHORT_SURFACE.0,
            SHORT_SURFACE.1,
            SCALE,
            0.0,
        );
        assert!(short.scrolls());
        assert!(
            short.tips().len() < short.rows.len(),
            "a row scrolled out of the body is still offering a tooltip"
        );
        for (_, rect, _) in short.tips() {
            assert!(
                rect[1] >= short.viewport[1] - 0.01 && rect[3] <= short.viewport[3] + 0.01,
                "a tip is hung on a box that runs outside the body it is drawn in"
            );
        }
    }

    /// PIN (§7.56 §3) — **the switch is 30 × 18 with a 14 × 14 knob two pixels
    /// in, its right edge on the card's padding, centred on its row.**
    ///
    /// Settings' own row shape: the sentence on the left, the control on the
    /// right. A reader handed six rows of that shape must meet the same shape
    /// when they next open Settings to change one.
    ///
    /// MUTATIONS:
    /// ① let the line's column run under the switch and every long line
    ///    collides with it;
    /// ② hang the switch off the row's top and it stops sitting on the line it
    ///    is about.
    #[test]
    fn the_switch_is_settings_own_control_in_settings_own_row_shape() {
        let placed = layout(
            &measured(&rows(&every_row())),
            SURFACE.0,
            SURFACE.1,
            SCALE,
            0.0,
        );
        let row = &placed.rows[0];
        let px = |value: f32| value * SCALE;
        assert!((row.switch[2] - row.switch[0] - px(SWITCH_WIDTH_LOGICAL_PX)).abs() < 0.01);
        assert!((row.switch[3] - row.switch[1] - px(SWITCH_HEIGHT_LOGICAL_PX)).abs() < 0.01);
        let switch_middle = (row.switch[1] + row.switch[3]) / 2.0;
        let band_middle = (row.band[1] + row.band[3]) / 2.0;
        assert!(
            (switch_middle - band_middle).abs() < 0.51,
            "the switch is not centred on its own row"
        );
        assert!(
            row.line.1[2] <= row.switch[0],
            "the line's column runs under the control"
        );
        let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * SCALE).max(1.0);
        assert!(
            (row.switch[2] - (placed.frame[2] - border - px(PADDING_X_LOGICAL_PX))).abs() < 0.01,
            "the switch's right edge is not on the card's own padding"
        );
        assert!(
            (row.band[3] - row.band[1] - px(ROW_HEIGHT_LOGICAL_PX)).abs() < 0.01,
            "a row is not 42 logical pixels tall"
        );
    }

    /// PIN (v4 §7 ⑦, user ruling 2026-09-06 — 「关着的开关在暗色下像个洞」) —
    /// **a knob that is off stands against its own track in both themes.**
    ///
    /// `.aswitch i` was `--menu` in both, and in the dark that is `#2A2A2A` on
    /// an `--active` track that resolves to `#343434`: six off switches with a
    /// hole punched in the left end of each, on the loudest surface this
    /// program has ever put them on. Fluent answers this the other way round
    /// from its light theme — the knob is the *ink* — so the dark off knob
    /// takes `--ink`, which is the colour of the very line beside it and not a
    /// new one.
    ///
    /// MUTATIONS:
    /// ① give the dark theme `--menu` back and the knob is darker than the
    ///    track it sits in, which is the report;
    /// ② give the light theme `--ink` and a white card grows six black pills;
    /// ③ light the *on* knob as well and the dark theme's pale accent carries a
    ///    pale knob, with the switch's state rubbed out.
    #[test]
    fn a_knob_that_is_off_stands_against_its_track_in_both_themes() {
        // **The two themes are separated by different things, and that is the
        // ruling.** In the light theme `--menu` is white on a near-white track
        // — eighteen levels apart — and what makes it a control is the
        // `0 1px 3px` under it, which the spec says in as many words and
        // `the_knob_s_shadow_falls_off_and_leaves_the_rest_of_the_track_alone`
        // is the gate for. In the dark theme the same shadow is invisible
        // against a dark track, so the ink has to do it, and there the gap is
        // the fact.
        let dark = bt_render::DARK_CHROME;
        let (track, knob) = (luma(dark.float_row_selected), luma(knob_face(dark, false)));
        assert!(
            knob > track + 24.0,
            "a dark off knob at {knob} on a track at {track} is a hole in the switch rather \
             than a control standing in it"
        );
        assert_eq!(
            knob_face(bt_render::LIGHT_CHROME, false),
            bt_render::LIGHT_CHROME.menu_surface,
            "the light theme's knob moved, and nothing was reported about it"
        );
        for palette in [bt_render::LIGHT_CHROME, bt_render::DARK_CHROME] {
            assert_eq!(
                knob_face(palette, true),
                palette.menu_surface,
                "a knob that is on left `--menu`, so the accent under it is carrying a knob of \
                 its own brightness"
            );
        }
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
        let mut content = measured(&rows(&every_row()));
        // Both answers, on rows clear of either end of the viewport, so that
        // half a shadow cut off by the clip is not the shape under test.
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

    /// PIN (v4 §2, user ruling 2026-09-06) — **440 logical, clamped to 92% of a
    /// window narrower than that.**
    ///
    /// MUTATION: keep v3's 480 and the card is wider than the sentences it now
    /// holds, which is the measurement the width was retuned from.
    #[test]
    fn the_card_is_four_hundred_and_forty_logical_or_the_window_s_own_share() {
        // **The number, not the constant.** Asserting against
        // `MAX_WIDTH_LOGICAL_PX` would be the constant agreeing with itself.
        assert!(
            (card_width(2000.0, 1.0) - 440.0).abs() < 0.51,
            "the width v4's one-line rows were measured to is not the width the card takes"
        );
        assert!(
            (card_width(2000.0, 2.0) - 880.0).abs() < 0.51,
            "the cap is in logical pixels, so it doubles with the monitor's scale"
        );
        assert!(
            (card_width(400.0, 1.0) - 368.0).abs() < 0.51,
            "a window narrower than the card did not hand it 92%"
        );
    }

    /// PIN (user ruling 2026-09-06 — 「PowerShell 那一行要带上整合的名字」) —
    /// **the PowerShell row names the integration, in both languages, before it
    /// says what the reader gets.**
    ///
    /// Every other row on this card says only a result, and this one may not:
    /// the reader who later goes to Settings to change it has to know what the
    /// thing is called, and the card is the only place they will ever be told.
    ///
    /// **The English column puts the benefit first from 2026-09-07** (copy
    /// audit, user-approved the same day; §7.56 ⓪″). The ruling above is
    /// untouched — the row still names the thing, so the Settings row it
    /// belongs to is still findable — and only the order moved, which is that
    /// audit's own "benefit before mechanism". The Chinese column did not
    /// change and still opens with the name.
    ///
    /// MUTATIONS:
    /// ① drop the name and the row is one anonymous result among several, and
    ///    the Settings row it belongs to is unfindable;
    /// ② put the Chinese benefit first and its half goes red, because that
    ///    column was not part of the English audit.
    #[test]
    fn the_powershell_row_carries_the_integration_s_name_in_both_languages() {
        for (lang, name, first) in [
            (crate::i18n::Lang::English, "PowerShell", false),
            (crate::i18n::Lang::Chinese, "PowerShell 整合", true),
        ] {
            let line = Text::FirstRunRowPowerShell.in_lang(lang);
            assert!(
                line.contains(name),
                "the PowerShell row does not carry {name:?}: {line:?}"
            );
            assert!(
                !first || line.starts_with(name),
                "the name is not the first thing the line says: {line:?}"
            );
            assert!(
                line.len() > name.len(),
                "the line is the name and nothing else, so it says what the switch is and not \
                 what it gets you: {line:?}"
            );
        }
    }
}
