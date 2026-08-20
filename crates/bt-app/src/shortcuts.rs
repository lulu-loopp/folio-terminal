//! The keyboard shortcut registry (P2-7 audit, user ruling 2026-08-10 "plan A").
//!
//! One constant table maps every bindable [`Action`] to its default [`Chord`] and the [`Scope`] it
//! is in force in. Event dispatch is a lookup into this table, never a scattered chain of modifier
//! `if`s: the shortcut-editing panel edits exactly this data, so a binding that is not
//! expressible here is a binding the panel could never show — which is why the preview's `Ctrl+S`
//! arrived as a third column (ruling 9, 2026-08-12) rather than as an `if` at the dispatch site.
//!
//! **Defaults plus overrides equal the effective table** (Settings extension block, slice 2,
//! 2026-08-17). [`BINDINGS`] is the *defaults* and never changes at runtime; [`Shortcuts`] is what
//! dispatch actually reads, and it is [`Shortcuts::defaults`] with whatever `keybindings.json` says
//! laid over it ([`Shortcuts::apply_overrides`]). Every row is named by a **stable id string** and
//! never by its position: the table gains and loses rows between builds, and an ordinal in a file
//! on a user's disk would come to mean a different verb the first time one is inserted above it —
//! the same rule `settings.json`'s `default_profile` already follows for profiles.
//!
//! **One rule, both doors.** A chord may be refused for three reasons — it is in the AltGr
//! forbidden zone, it is a bare `Ctrl+letter` the shell owns, or another row already claims it —
//! and [`chord_verdict`] is the single place all three are decided. The recorder in the settings
//! dialog asks it while the user is still holding the keys down, and `apply_overrides` asks it of
//! every line of a hand-edited file. A file that could consent to `Ctrl+Alt+P` on behalf of a
//! German keyboard would be a second answer to a question with one.

use std::borrow::Cow;
use std::fmt::Write as _;

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::i18n::Text;

/// Everything the window can be asked to do from the keyboard.
///
/// `JumpAttention` and `CommandPalette` are real rows with no machine behind them yet (the
/// attention queue is P1-8, the palette is P1-9). They are listed, bound, and dispatched to an
/// explicit no-op rather than omitted: the audit decided the key belongs to us, so nothing else may
/// claim it and nothing may leak to the shell in the meantime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    NewTab,
    /// **A second window on this application** (multiwindow slice C).
    ///
    /// Its own row and not a mode of [`Self::NewTab`], because the two differ in
    /// the one thing a shortcut row is about: what appears when you press it.
    NewWindow,
    ClosePane,
    NextTab,
    PrevTab,
    /// 1-based tab ordinal, always within `1..=9`; out-of-range targets are ignored at dispatch.
    GotoTab(u8),
    ReopenClosed,
    JumpAttention,
    CommandPalette,
    /// **Turn focus mode on, or off** — one chord for both directions
    /// (§7.1.6b′ ②).
    ///
    /// A toggle and not a pair, and the ruling says why: the mode is **one bit**
    /// and the Appearance row shows which way it is set, so a separate "leave"
    /// key would be a second truth about the same bit. `GitPage`'s reasoning
    /// exactly, one surface up.
    ToggleFocusMode,
    SplitHorizontal,
    SplitVertical,
    DuplicatePaneSplit,
    /// Open this tab's files column, or close the one it already has.
    FilesPane,
    /// Turn a files column's page over: tree to repository, repository to tree.
    ///
    /// A toggle and not two rows, because the switch it works is a segmented pair
    /// of two — `Files | Git` — and a pair of two is a toggle whichever way you
    /// come at it.
    GitPage,
    OpenSettings,
    /// Write the preview seat's buffer back to its file (mock-up 6139-6150).
    ///
    /// The one row in the table that is not the window's everywhere. See
    /// [`Scope`].
    SavePreview,
    /// Walk the command marks rail backwards — the previous command relative to
    /// the one the viewport is showing (§7.1.5c ③).
    ///
    /// **The rail's hover and click must never be the only door.** The mock-up
    /// states it as an iron rule beside the rail's own code (4603-4607: "nothing
    /// is hover-only") and §7.1.5c repeats it, adding that the keyboard walks the
    /// *full* history however the rail has chosen to aggregate itself — a
    /// collapsed bucket is a drawing decision and this is not drawing.
    PrevCommandMark,
    /// The same, forwards.
    NextCommandMark,
    /// Raise the in-pane search capsule on the focused terminal, or — when it is already up —
    /// put the caret back in it with the last query selected (§7.1.5d, B80).
    OpenSearch,
    /// Walk to the next match while the **terminal** still holds the keyboard (B81).
    ///
    /// `Enter` cannot do this and that is the whole reason the function key is in the table: Enter
    /// belongs to the shell the moment the caret leaves the capsule, so `F3` is what covers the
    /// second of the two stances a reader can be in — search open, hands back on the terminal.
    NextMatch,
    /// The same, backwards.
    PrevMatch,
    /// Bring the picture-in-picture terminal in this slot to the front, or send
    /// it away again — one verb per slot, `1..=4`.
    ///
    /// **The slot is a dimension of the action and not a field on a window**,
    /// and that is the whole reason it is here (mock-up 6104-6106): the chord
    /// Ctrl and the backtick key was already taken on the user's own machine in
    /// July 2026, "which is the real
    /// lesson: summon keys MUST be per-slot configurable (P2-7); F9 is only the
    /// prototype's default". A single global `SummonPip` would be a row the
    /// panel could offer exactly one chord for, and the lesson was that one
    /// chord is not enough.
    ///
    /// **All four ship with no chord at all** — see [`Binding::unassigned`] —
    /// and the prototype's `F9` is deliberately not carried over. The mock-up
    /// calls `F9` "only the prototype's default", and this window is not that
    /// prototype: a bare function key here is exactly what the `F3` row two
    /// entries up refuses to take from a shell without a [`Scope`] to hand it
    /// straight back with (`cmd.exe` recalls a command with `F9` as it recalls
    /// one with `F3`), and a summon key cannot be scoped — the whole point of it
    /// is that it works when the thing it summons is *not* on the glass.
    ///
    /// So what ships is the ruling itself rather than a guess at it: four named
    /// slots, four lines in `keybindings.json`, and a panel to fill them in. The
    /// lesson of July was that one key chosen for the user was one key too many.
    ///
    /// A stub row in the sense §7.1.5e means it: named, in the table, dispatched
    /// to an explicit no-op until the picture-in-picture machine arrives.
    SummonPip(u8),
}

/// Where a row is in force.
///
/// **A third column, not a second table** (ruling 9, 2026-08-12). `Ctrl+S` is the
/// binding every editor on this platform has and the one the mock-up asks for
/// (6139-6150), and it is also a bare `Ctrl+letter` — precisely the family the
/// P2-7 audit refused to take from the shell, because `^S` is the terminal's own
/// flow-control stop. Both are true, and what reconciles them is that the chord
/// is only claimed where there is something to save: with the keyboard anywhere
/// but a preview, `^S` still reaches the child untouched.
///
/// It is a column of the data table rather than an `if` at the dispatch site so
/// that the shortcut-editing panel this table exists for can *show* the
/// condition. A binding whose applicability lives in a function body is a
/// binding the panel could only describe by guessing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Scope {
    /// Every focus state — the window's own verbs.
    Window,
    /// Only while the preview seat holds the keyboard focus.
    Preview,
    /// Only while a **terminal** holds the keyboard and is showing its **primary
    /// screen**.
    ///
    /// The second half is the whole reason this is a scope rather than a guard
    /// inside a handler. `Ctrl+Shift+↑` on the alternate screen has to reach the
    /// child untouched — a full-screen program owns its canvas, there is no
    /// scrollback behind it to walk, and §3.2 keeps the two screens in separate
    /// namespaces so there is not even an ordering between a command mark and
    /// what is on the glass. Expressed here, the row is simply *not in the table*
    /// for that press and the key falls through to the encoder exactly as an
    /// unbound one does; expressed as an early return at the dispatch site, the
    /// key would be claimed and then dropped, which is the one outcome that
    /// leaves the child silent.
    ///
    /// It is also the shape the shortcut-editing panel needs. The scrollback keys
    /// (`Shift+PgUp`, `Ctrl+Home`) have carried the same condition since long
    /// before this table existed, in an `if !alternate_screen` ladder inside
    /// `keyboard_input` — which is exactly the "binding whose applicability lives
    /// in a function body" this module's own header says the panel could only
    /// describe by guessing. They are not moved here in this slice, but this is
    /// the column they would move into.
    TerminalPrimary,
    /// Only while the in-pane search capsule is up (§7.1.5d, B81).
    ///
    /// `F3` is a key shells and full-screen programs use — `cmd.exe` recalls its
    /// history with it — so it may only be claimed while there is a search to
    /// walk. Expressed as a scope rather than as a guard inside the handler for
    /// [`Self::TerminalPrimary`]'s reason: out of scope the row is simply not in
    /// the table, so the key reaches the child exactly as an unbound one does,
    /// and the shortcut-editing panel can *show* the condition instead of
    /// guessing at it.
    ///
    /// The capsule only ever opens on a terminal showing its primary screen and
    /// is closed the moment that stops being true, so this scope implies the one
    /// above rather than having to repeat it.
    SearchOpen,
}

/// What the window's focus looks like to the table.
///
/// A struct rather than a bare `bool` because [`Scope`] is expected to grow rows
/// — a files column and a terminal both have chords of their own waiting on the
/// same panel — and a second bool parameter at every call site is how the two
/// would eventually be passed the wrong way round.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Focus {
    /// Whether the focused leaf is a preview seat, its quick edit included.
    pub(crate) preview: bool,
    /// Whether the keyboard is on a terminal that is showing its primary screen.
    ///
    /// Both halves in one bool, because [`Scope::TerminalPrimary`] is one
    /// condition: a preview seat is not a terminal and a terminal running `vim` is
    /// not showing its scrollback, and a row in that scope is out of force for
    /// either reason.
    pub(crate) terminal_primary: bool,
    /// Whether the in-pane search capsule is up — anywhere, on any pane.
    ///
    /// Not "and the caret is in it". The capsule holding the keyboard is a
    /// *different* state, and it is the one this bool is deliberately not about:
    /// `F3` exists precisely for the stance where the search is open and the
    /// hands are back on the shell (B81), so a flag that meant "the field is
    /// focused" would switch the row off exactly when it is wanted.
    pub(crate) search_open: bool,
}

impl Scope {
    /// The tag the shortcut editor prints under a row's name, or `None` for the
    /// scope that is everywhere.
    ///
    /// **[`Self::Window`] shows nothing, and that is the ruling rather than an
    /// omission.** Twenty of the table's rows are in force everywhere; a column
    /// reading "Anywhere" twenty times over is a column that says nothing
    /// twenty times, and the eye stops reading it before it reaches the four
    /// lines that carry a condition. A tag is a *departure* from the ordinary,
    /// so the ordinary wears none — the same reason the `˅` menu marks the
    /// default profile and leaves the other three unmarked.
    #[must_use]
    pub(crate) const fn tag(self) -> Option<Text> {
        match self {
            Self::Window => None,
            Self::Preview => Some(Text::ShortcutScopePreview),
            Self::TerminalPrimary => Some(Text::ShortcutScopeTerminalPrimary),
            Self::SearchOpen => Some(Text::ShortcutScopeSearchOpen),
        }
    }

    /// Whether a row in this scope is in force with the window focused like this.
    const fn holds(self, focus: Focus) -> bool {
        match self {
            Self::Window => true,
            Self::Preview => focus.preview,
            Self::TerminalPrimary => focus.terminal_primary,
            Self::SearchOpen => focus.search_open,
        }
    }
}

/// The key half of a chord.
///
/// `Character` is matched case-insensitively against both the produced logical key and the layout's
/// unmodified key, so `Ctrl+Shift+1` and `Alt+Shift+-` resolve on layouts that reach the digit or
/// the punctuation through Shift as well as on those that reach it directly.
///
/// **`Cow` and not `&'static str`** since the table became editable: a default's character is a
/// literal in this file and costs nothing to borrow, and a recorded one arrives from a keyboard at
/// runtime and has to be owned. One type for both, because a chord out of `keybindings.json` and a
/// chord out of [`BINDINGS`] are the same thing to everybody downstream — a second key type for
/// "the ones the user chose" would be a second matcher, a second renderer and a second place to
/// forget the Shift folding above.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChordKey {
    Character(Cow<'static, str>),
    Named(NamedKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Chord {
    pub(crate) modifiers: ModifiersState,
    pub(crate) key: ChordKey,
}

impl Chord {
    const fn new(modifiers: ModifiersState, key: ChordKey) -> Self {
        Self { modifiers, key }
    }
}

const CTRL: ModifiersState = ModifiersState::CONTROL;
const CTRL_SHIFT: ModifiersState = ModifiersState::CONTROL.union(ModifiersState::SHIFT);
const ALT_SHIFT: ModifiersState = ModifiersState::ALT.union(ModifiersState::SHIFT);

/// One row of the table: what it is called, what it does, what it is pressed
/// with, and where it is in force.
///
/// **`id` is the row's name on disk and it is a string, never a position.** The
/// table gains rows between builds — three arrived in the last week — and an
/// ordinal written into a file on a user's machine would come to mean a
/// different verb the first time one is inserted above it. It is the same rule
/// `settings.json`'s `default_profile` follows, arrived at from the same
/// direction.
///
/// **`chord` is an `Option`, and the `None` is a real state rather than a
/// missing value.** Two things spell themselves that way and they are the same
/// thing seen from either end: a row that ships with no default at all (the
/// three unassigned picture-in-picture slots — see [`Action::SummonPip`]) and a
/// row a user has deliberately taken the chord away from (`"chord": null` in
/// `keybindings.json`). Both mean "this verb has no key today", and a row that
/// vanished from the table instead would be a verb the panel could not offer a
/// key *to*.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    /// The stable id this row is named by in `keybindings.json`.
    pub(crate) id: &'static str,
    /// What this row is called where a person reads it.
    ///
    /// On the row and not on the [`Action`] because one action can hold two
    /// rows: `OpenSearch` has a chord and an alias, deliberately spelled as two
    /// table rows rather than as a special case in the matcher, "because two
    /// rows is what the shortcut-editing panel can show and edit". Two rows
    /// wanting one name would be two lines in the editor a reader could not tell
    /// apart.
    pub(crate) title: Text,
    /// The name of the folded row this one is a member of, when it is one of a
    /// family the editor shows as a single line (`Go to tab 1–9`).
    ///
    /// Members of one family must be contiguous in [`BINDINGS`] — the fold is
    /// derived by walking the table and noticing where this answer changes,
    /// which is the same derivation the settings dialog's own headings use, and
    /// it is here for the same reason: a second list declaring the families
    /// beside the one declaring the rows is a second place to forget one.
    pub(crate) family: Option<Text>,
    pub(crate) action: Action,
    pub(crate) chord: Option<Chord>,
    pub(crate) scope: Scope,
}

impl Binding {
    /// A row in force everywhere.
    const fn window(id: &'static str, title: Text, action: Action, chord: Chord) -> Self {
        Self {
            id,
            title,
            family: None,
            action,
            chord: Some(chord),
            scope: Scope::Window,
        }
    }

    /// The same, as one member of a folded family.
    ///
    /// Spelled out field by field rather than through struct-update syntax, and
    /// every constructor below with it: `..Self::window(…)` would have to *drop*
    /// the fields it overrides, and a `Cow` cannot be dropped in a `const`. The
    /// table is a constant, so its constructors are longhand.
    const fn family(
        id: &'static str,
        title: Text,
        family: Text,
        action: Action,
        chord: Chord,
    ) -> Self {
        Self {
            id,
            title,
            family: Some(family),
            action,
            chord: Some(chord),
            scope: Scope::Window,
        }
    }

    /// A member of a folded family that ships with no chord at all.
    const fn unassigned(id: &'static str, title: Text, family: Text, action: Action) -> Self {
        Self {
            id,
            title,
            family: Some(family),
            action,
            chord: None,
            scope: Scope::Window,
        }
    }

    /// A row in force only while the preview seat holds the focus.
    const fn preview(id: &'static str, title: Text, action: Action, chord: Chord) -> Self {
        Self {
            id,
            title,
            family: None,
            action,
            chord: Some(chord),
            scope: Scope::Preview,
        }
    }

    /// A row in force only on a terminal showing its primary screen.
    const fn terminal_primary(id: &'static str, title: Text, action: Action, chord: Chord) -> Self {
        Self {
            id,
            title,
            family: None,
            action,
            chord: Some(chord),
            scope: Scope::TerminalPrimary,
        }
    }

    /// A row in force only while the search capsule is up.
    const fn search_open(id: &'static str, title: Text, action: Action, chord: Chord) -> Self {
        Self {
            id,
            title,
            family: None,
            action,
            chord: Some(chord),
            scope: Scope::SearchOpen,
        }
    }

    /// The line the editor prints under this row's name: where it is in force,
    /// and whether the verb behind it has arrived.
    ///
    /// Both facts on one line and joined by the product's own separator, because
    /// they are two halves of "when does pressing this do anything" — and a row
    /// that answered only the first would be a row a user presses, sees nothing
    /// from, and concludes is broken (§7.1.5e: "存根行是真实的行").
    fn note(&self) -> Option<Cow<'static, str>> {
        // **Only a row that has a chord can say it is bound.** The
        // picture-in-picture slots are pending *and* unassigned, and printing
        // "Bound; the verb behind it is still to come" over a row reading
        // `Not set` would be the panel contradicting itself across four inches
        // of one line — which is exactly what the real window showed.
        let pending =
            (self.action.is_pending() && self.chord.is_some()).then(|| NOTE_MACHINE_PENDING.text());
        match (self.scope.tag().map(Text::text), pending) {
            (None, None) => None,
            (Some(one), None) | (None, Some(one)) => Some(Cow::Borrowed(one)),
            (Some(scope), Some(pending)) => {
                Some(Cow::Owned(format!("{scope}{NOTE_JOIN}{pending}")))
            }
        }
    }
}

impl Action {
    /// Whether this row's key is claimed but its machine has not arrived.
    ///
    /// §7.1.5e's "存根行是真实的行", stated once and read by the editor: the key
    /// is ours, nothing else may claim it, nothing leaks to the shell in the
    /// meantime — and the panel has to be able to *say* so, or the first user to
    /// press it reports a bug against a decision.
    const fn is_pending(self) -> bool {
        matches!(
            self,
            Self::JumpAttention | Self::CommandPalette | Self::SummonPip(_)
        )
    }
}

/// The default binding table. This is the single source of truth for shortcut keys.
///
/// Every **window** action wears Shift alongside Ctrl because bare `Ctrl+letter` is the shell's
/// control-code alphabet, and no row uses `Ctrl+Alt`: Windows reports AltGr as exactly that pair,
/// so binding it would steal a character from every layout that composes with AltGr. The one bare
/// `Ctrl+letter` in the table is scoped instead of shifted — see [`Scope`].
pub(crate) const BINDINGS: &[Binding] = &[
    Binding::window(
        "new-tab",
        Text::RailNewTab,
        Action::NewTab,
        Chord::new(CTRL_SHIFT, character("n")),
    ),
    // **`Ctrl+Shift+M`, and it is a ruled 2026-08-19 with its reasons written down**
    // (multiwindow slice C, 2026-08-19).
    //
    // The chord this verb *wants* is `Ctrl+Shift+N`: Windows Terminal, WezTerm,
    // Alacritty, kitty and VS Code all open a new window with it, and they can
    // because they put `New tab` on `Ctrl+Shift+T`. This product cannot. The
    // 2026-08-10 audit ruled `Ctrl+Shift+N` = new tab and `Ctrl+Shift+T` = undo
    // close, and both were decided when one window was all there was. Re-pointing
    // `Ctrl+Shift+N` would move a chord out from under a user's fingers to settle
    // a question that user has not been asked, so **the ruling stands and this
    // row takes a free key**. Which key it should really be is a decision for the
    // person whose keyboard it is; the row exists so that the door does, and it
    // is one `keybindings.json` line away from anything else.
    //
    // **Not `Ctrl+Shift+Enter`**, which was tried first and measured on the real
    // window: it never arrives. A modified `Enter` does not reach this
    // application as `NamedKey::Enter` at all — under both a Chinese IME and the
    // US layout, `Ctrl+Shift+Enter` produced no dispatch while `Ctrl+Shift+N` on
    // the same window opened a tab. So no row in this table may be keyed on a
    // modified `Enter` until somebody has found out what Windows and winit
    // between them are doing with it, and this comment is that finding.
    //
    // `M` wears the modifier pair every window verb in this table wears
    // (discipline (1): bare `Ctrl+letter` belongs to the shell), and `^M` — which
    // is what a bare `Ctrl+M` would take — stays with the terminal, where it is
    // Return.
    Binding::window(
        "new-window",
        Text::ShortcutNewWindow,
        Action::NewWindow,
        Chord::new(CTRL_SHIFT, character("m")),
    ),
    Binding::window(
        "close-pane",
        Text::ClosePane,
        Action::ClosePane,
        Chord::new(CTRL_SHIFT, character("w")),
    ),
    Binding::window(
        "next-tab",
        Text::ShortcutNextTab,
        Action::NextTab,
        Chord::new(CTRL, ChordKey::Named(NamedKey::Tab)),
    ),
    Binding::window(
        "prev-tab",
        Text::ShortcutPrevTab,
        Action::PrevTab,
        Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::Tab)),
    ),
    // **Nine rows the editor draws as one line.** They stay nine bindings — the
    // file names each of them, and a user who wants only `Ctrl+Shift+9` moved
    // can move that one by hand — but a panel that spent nine of its lines on
    // nine ordinals of one verb would have buried the twenty other verbs under
    // them. The fold is derived from `family` below, never declared beside the
    // table.
    Binding::family(
        "goto-tab-1",
        Text::ShortcutGotoTab1,
        FAMILY_GOTO_TAB,
        Action::GotoTab(1),
        Chord::new(CTRL_SHIFT, character("1")),
    ),
    Binding::family(
        "goto-tab-2",
        Text::ShortcutGotoTab2,
        FAMILY_GOTO_TAB,
        Action::GotoTab(2),
        Chord::new(CTRL_SHIFT, character("2")),
    ),
    Binding::family(
        "goto-tab-3",
        Text::ShortcutGotoTab3,
        FAMILY_GOTO_TAB,
        Action::GotoTab(3),
        Chord::new(CTRL_SHIFT, character("3")),
    ),
    Binding::family(
        "goto-tab-4",
        Text::ShortcutGotoTab4,
        FAMILY_GOTO_TAB,
        Action::GotoTab(4),
        Chord::new(CTRL_SHIFT, character("4")),
    ),
    Binding::family(
        "goto-tab-5",
        Text::ShortcutGotoTab5,
        FAMILY_GOTO_TAB,
        Action::GotoTab(5),
        Chord::new(CTRL_SHIFT, character("5")),
    ),
    Binding::family(
        "goto-tab-6",
        Text::ShortcutGotoTab6,
        FAMILY_GOTO_TAB,
        Action::GotoTab(6),
        Chord::new(CTRL_SHIFT, character("6")),
    ),
    Binding::family(
        "goto-tab-7",
        Text::ShortcutGotoTab7,
        FAMILY_GOTO_TAB,
        Action::GotoTab(7),
        Chord::new(CTRL_SHIFT, character("7")),
    ),
    Binding::family(
        "goto-tab-8",
        Text::ShortcutGotoTab8,
        FAMILY_GOTO_TAB,
        Action::GotoTab(8),
        Chord::new(CTRL_SHIFT, character("8")),
    ),
    Binding::family(
        "goto-tab-9",
        Text::ShortcutGotoTab9,
        FAMILY_GOTO_TAB,
        Action::GotoTab(9),
        Chord::new(CTRL_SHIFT, character("9")),
    ),
    Binding::window(
        "reopen-closed",
        Text::ShortcutReopenClosed,
        Action::ReopenClosed,
        Chord::new(CTRL_SHIFT, character("t")),
    ),
    Binding::window(
        "jump-attention",
        Text::ShortcutJumpAttention,
        Action::JumpAttention,
        Chord::new(CTRL_SHIFT, character("a")),
    ),
    Binding::window(
        "command-palette",
        Text::ShortcutCommandPalette,
        Action::CommandPalette,
        Chord::new(CTRL_SHIFT, character("p")),
    ),
    // **`Ctrl+Shift+Z`, and the settling of the chord P2-7 left open**
    // (§7.1.6b′ ②, user ruling 2026-08-19).
    //
    // The other candidate was `Ctrl+Shift+F` — freed by the retired find alias
    // two rows up — and it lost on one argument: that chord is Find's muscle
    // memory on every platform, and a window layout that answers to it is a
    // window people stop trusting the first time they press it. `Z` is free in
    // this product, means nothing to a shell, and is outside the `Ctrl+Alt`
    // AltGr zone this table's header rules out. Redo lives on it in some
    // editors; Folio has no Redo of its own to collide with, and the pane below
    // keeps its own keyboard either way.
    //
    // The row is titled with the Appearance row's own name, on `new-tab`'s
    // precedent: the chord and the setting turn one bit, so they are one name.
    Binding::window(
        "focus-mode",
        Text::RowFocusMode,
        Action::ToggleFocusMode,
        Chord::new(CTRL_SHIFT, character("z")),
    ),
    Binding::window(
        "split-horizontal",
        Text::ShortcutSplitHorizontal,
        Action::SplitHorizontal,
        Chord::new(ALT_SHIFT, character("-")),
    ),
    Binding::window(
        "split-vertical",
        Text::ShortcutSplitVertical,
        Action::SplitVertical,
        Chord::new(ALT_SHIFT, character("=")),
    ),
    Binding::window(
        "duplicate-pane-split",
        Text::ShortcutDuplicatePaneSplit,
        Action::DuplicatePaneSplit,
        Chord::new(CTRL_SHIFT, character("d")),
    ),
    // **`Ctrl+Shift+B`, and pointedly not the mock-up's `Ctrl+B`.**
    //
    // The mock-up binds a bare `Ctrl+B` (6126-6134), which discipline (1) above
    // forbids outright: `^B` is readline's "back one character" and tmux's
    // default prefix, and it is exactly the sort of letter the audit refused to
    // take from the shell. `DESIGN.md` §7.1.5 had already flagged the tmux
    // collision and deferred it to the P2-7 audit; the audit closed without a
    // Files row at all, so this is that row arriving late rather than a binding
    // being overturned.
    //
    // Shift is what every other window action wears for the same reason, which
    // leaves `^B` where it belongs and puts this beside `Ctrl+Shift+W` and
    // friends. A toggle, not an opener, matching both the mock-up's own
    // behaviour and VS Code's `Ctrl+B`.
    Binding::window(
        "files-pane",
        Text::ShortcutFilesPane,
        Action::FilesPane,
        Chord::new(CTRL_SHIFT, character("b")),
    ),
    // **`Ctrl+Shift+G`** (R28, 2026-08-15) — the chord VS Code has spent a
    // decade teaching, for the surface it taught it on.
    //
    // It wears Shift for `Ctrl+Shift+B`'s reason and not merely for its company:
    // a bare `^G` is readline's "abort the current command", which is the very
    // key someone reaches for when a shell has them halfway into something they
    // want out of, and discipline (1) does not let this table take it.
    //
    // The row it works is the column's page, so it does nothing at all when the
    // window has no files column and nothing when the Git panel is switched off —
    // a chord for a surface that is not there is not an error, it is a chord with
    // nothing to say.
    Binding::window(
        "git-page",
        Text::ShortcutGitPage,
        Action::GitPage,
        Chord::new(CTRL_SHIFT, character("g")),
    ),
    Binding::window(
        "open-settings",
        Text::Settings,
        Action::OpenSettings,
        Chord::new(CTRL, character(",")),
    ),
    // **The one scoped row** (ruling 9, 2026-08-12). It is the mock-up's chord
    // verbatim — bare `Ctrl+S`, from any focus state *inside the preview*, so a
    // buffer can be saved after flipping to the rendered view or clicking
    // elsewhere in the pane — and it is claimed nowhere else, which is what
    // leaves `^S` with the shell. The audit's (1) discipline forbids taking a
    // bare control letter *from the terminal*; it does not forbid a chord in a
    // place where there is no terminal to take it from.
    Binding::preview(
        "save-preview",
        Text::ShortcutSavePreview,
        Action::SavePreview,
        Chord::new(CTRL, character("s")),
    ),
    // **`Ctrl+Shift` and an arrow, and pointedly not the mock-up's `Ctrl+Alt`
    // and one** (user ruling 2026-08-16, inventory D-1).
    //
    // The mock-up binds the command walk to `Ctrl+Alt+arrow` (6268-6277) and
    // §7.1.5c calls it an iron rule — but the same mock-up's own audit note four
    // lines earlier declares `Ctrl+Alt` a forbidden zone, because Windows reports
    // AltGr as exactly that pair and a German, French, Polish or Portuguese
    // keyboard reaches it by typing an `@` or a `{`. Two rulings in one file, and
    // the later one is the one with a keyboard model behind it.
    //
    // Nothing is actually overturned by choosing Shift: the iron rule asks that
    // the rail have *a* keyboard door — "the rail's hover/click must never be the
    // only door" — and never says which. `Ctrl+Shift+arrow` is not a terminal
    // sequence any shell claims (`Ctrl+arrow` is readline's word movement and is
    // left alone; `Shift+arrow` is selection extension in the applications that
    // have one, and this window's own selection is a pointer gesture), and it
    // wears Shift for the reason every other window row does.
    //
    // Scoped rather than shifted-and-global, because the alternate screen has to
    // keep it — see [`Scope::TerminalPrimary`].
    Binding::terminal_primary(
        "prev-command-mark",
        Text::ShortcutPrevCommandMark,
        Action::PrevCommandMark,
        Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::ArrowUp)),
    ),
    Binding::terminal_primary(
        "next-command-mark",
        Text::ShortcutNextCommandMark,
        Action::NextCommandMark,
        Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::ArrowDown)),
    ),
    // **`Ctrl+F`, and it is an exception to discipline (1) written down as one**
    // (user ruling, 2026-08-16, inventory D-2).
    //
    // Discipline (1) — bare `Ctrl+letter` is the shell's control-code alphabet and
    // is never taken — is the reason every window row above wears Shift, and it
    // is why the same audit moved `Ctrl+B` to `Ctrl+Shift+B` two months ago. The
    // ruling that overturns it here is narrow and gives its reasons: what (1)
    // protects is a control code with an *owner*, and `^F` has none on Windows
    // that a terminal user meets — readline's forward-one-character is the same
    // key as the right arrow and nobody presses it; the reference product for
    // this surface (VS Code's integrated terminal) takes `Ctrl+F` for exactly
    // this box; and the row is scoped, so the moment a full-screen program is on
    // the glass the chord is not in the table at all and `less` keeps its
    // page-forward.
    //
    // That last clause is doing the real work. This is the same shape as
    // `Ctrl+S` in a preview (ruling 9): the discipline forbids taking a bare
    // control letter *from a terminal*, and here it is taken only where the
    // scrollback the search reads actually exists.
    //
    // **The recorder does not inherit the exception**, and that is stated here
    // rather than left to be noticed: [`chord_verdict`] refuses every *new* bare
    // `Ctrl+letter`, this one included if it were being recorded today. The
    // ruling is about one chord, for one surface, with three reasons written
    // down; a recorder that read it as "bare control letters are allowed now"
    // would have turned one argued exception into a policy nobody made.
    Binding::terminal_primary(
        "open-search",
        Text::ShortcutOpenSearch,
        Action::OpenSearch,
        Chord::new(CTRL, character("f")),
    ),
    // **The alias is retired** (user ruling 2026-08-18). `open-search-alias` was
    // a second row naming the same verb, so that a reader whose muscles knew
    // `Ctrl+Shift+F` from another product would find the box — and the cost was
    // a table with two lines for one action, a chord this window took from every
    // shell for a convenience nobody had asked for, and a page where the one
    // duplicated verb had to be explained. The ruling is that somebody who wants
    // that chord records it: the row above is recordable, the recorder writes
    // `keybindings.json`, and one line in a file is a smaller thing than a
    // permanent second default.
    //
    // The id is not reused. A `keybindings.json` still naming it is read, its
    // other rows land, and the retired line is reported by id rather than
    // swallowed — see
    // `a_file_naming_the_retired_alias_still_loads_and_says_which_row_it_lost`.
    // `F3` / `Shift+F3` (B81) — the walk that works while the terminal still has
    // the keyboard. Bare, because a function key is not a control code and the
    // discipline has nothing to say about it, and scoped so that a shell which
    // uses `F3` (`cmd.exe` recalls its last command with it) keeps the key
    // whenever there is no search to walk.
    Binding::search_open(
        "next-match",
        Text::ShortcutNextMatch,
        Action::NextMatch,
        Chord::new(ModifiersState::empty(), ChordKey::Named(NamedKey::F3)),
    ),
    Binding::search_open(
        "prev-match",
        Text::ShortcutPrevMatch,
        Action::PrevMatch,
        Chord::new(ModifiersState::SHIFT, ChordKey::Named(NamedKey::F3)),
    ),
    // **The first rows in this table that exist in order to be configured**
    // (mock-up 6104-6106, §7.1.5e), and the first that ship with nothing in
    // them. See [`Action::SummonPip`] for why the prototype's `F9` is not
    // carried over: a bare function key with no scope to hand it back is the one
    // shape this table has already refused once, four rows up.
    Binding::unassigned(
        "summon-pip-1",
        Text::ShortcutSummonPip1,
        FAMILY_SUMMON_PIP,
        Action::SummonPip(1),
    ),
    Binding::unassigned(
        "summon-pip-2",
        Text::ShortcutSummonPip2,
        FAMILY_SUMMON_PIP,
        Action::SummonPip(2),
    ),
    Binding::unassigned(
        "summon-pip-3",
        Text::ShortcutSummonPip3,
        FAMILY_SUMMON_PIP,
        Action::SummonPip(3),
    ),
    Binding::unassigned(
        "summon-pip-4",
        Text::ShortcutSummonPip4,
        FAMILY_SUMMON_PIP,
        Action::SummonPip(4),
    ),
];

const fn character(text: &'static str) -> ChordKey {
    ChordKey::Character(Cow::Borrowed(text))
}

// ── Every word this table puts in front of a person ────────────────────────
//
// Constants and not literals at their use sites, because the i18n sweep
// (`scratchpad/i18n-string-inventory.md`) collects a language table next slice
// and a string it cannot find is a string that never gets translated. The
// ruling's own constraint applies to all of them: `&'static str`, so a
// `match lang` can keep returning one of two literals without allocating.

/// The name of the row the nine `Ctrl+Shift+digit` bindings fold into.
const FAMILY_GOTO_TAB: Text = Text::ShortcutFamilyGotoTab;
/// The name of the row the four picture-in-picture summon slots fold into.
const FAMILY_SUMMON_PIP: Text = Text::ShortcutFamilySummonPip;

/// What a stub row says about itself (§7.1.5e: the key is claimed, the machine
/// is not here yet).
const NOTE_MACHINE_PENDING: Text = Text::ShortcutNotePending;
/// The product's own separator between two clauses of one muted line.
///
/// Not a table entry: it is a punctuation mark this window uses everywhere it
/// joins two facts on one line, and it is the same mark in both languages.
const NOTE_JOIN: &str = " · ";
/// What a family row says about the chords it folded, in its three states.
///
/// **Three whole sentences and not one with a clause appended**, which is the
/// language table's own ruling on the join: the two halves are separated by a
/// `; ` in English and by a `；` in Chinese, and a formatter that composed them
/// here would have had to know that.
const NOTE_ONE_PER_MEMBER: Text = Text::ShortcutNoteOnePerMember;
const NOTE_NONE_ASSIGNED: Text = Text::ShortcutNoteNoneAssigned;
const NOTE_SOME_UNASSIGNED: Text = Text::ShortcutNoteSomeUnassigned;

/// What the editor prints where the caps would be on a row with no chord.
///
/// A function rather than the constant it was, for the reason every word on
/// this page is now one: the answer depends on the language in force, and
/// `Text::text` reads it at the call.
#[must_use]
pub fn unbound_cap() -> &'static str {
    Text::ShortcutUnbound.text()
}

/// Why the audit listed the `Alt`+arrow families and never took them.
const NOTE_RESERVED_ALT_ARROW: Text = Text::ShortcutReservedAltArrow;

/// The three refusals, in the words the recorder shows.
#[must_use]
pub(crate) fn hint_altgr_zone() -> &'static str {
    Text::ShortcutHintAltGrZone.text()
}

#[must_use]
pub(crate) fn hint_shell_control_letter() -> &'static str {
    Text::ShortcutHintShellControlLetter.text()
}

/// A row the audit **listed and did not take** — no [`Action`], no [`Chord`],
/// and no way to record one.
///
/// The mock-up's own audit block keeps `Alt`+arrow and `Alt+Shift`+arrow in the
/// table with the note "the rows are listed here so the table stays the whole
/// ruling", and the panel is that table's editor, so the panel keeps them too
/// (user ruling Q8, 2026-08-17). They are a separate constant from [`BINDINGS`]
/// rather than rows with a `None` chord, and the difference is the whole point:
/// an unassigned picture-in-picture slot is a chord *waiting for a user*, while
/// these are chords the audit decided **not to ask for** — taking `Alt`+arrow
/// needs its own ruling about what the child loses, and no ruling has been made.
/// A row in `BINDINGS` is one this window claims; these are rows it declines,
/// and the editor greys them to say so.
struct ReservedRow {
    title: Text,
    caps: &'static [&'static str],
}

const RESERVED_ROWS: &[ReservedRow] = &[
    ReservedRow {
        title: Text::ShortcutReservedMoveFocus,
        caps: &["Alt", ARROW_CAPS],
    },
    ReservedRow {
        title: Text::ShortcutReservedResizePane,
        caps: &["Alt", "Shift", ARROW_CAPS],
    },
];

/// The four arrows as one cap, which is what a family of four directions looks
/// like on a line that has room for one key.
const ARROW_CAPS: &str = "← ↑ → ↓";

/// **The effective table** — [`BINDINGS`] with the user's `keybindings.json` laid
/// over it, and the only thing dispatch ever reads.
///
/// Owned rows rather than a borrow of the constant, because an override changes
/// a chord and a chord out of a file is a `String` that has to live somewhere.
/// The rows keep the constant's order and the constant's *set*: an override may
/// change a row's chord or take it away, never add a row or remove one. That is
/// what makes "restore defaults" a deletion rather than a merge, and it is why
/// [`Self::overrides`] can be derived by walking the two tables side by side
/// instead of being remembered separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shortcuts {
    rows: Vec<Binding>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self::defaults()
    }
}

/// One line of `keybindings.json`, in this module's own vocabulary.
///
/// `chord: None` is the file's `null` and means **explicitly unbound** — which
/// is a different sentence from "absent", and the difference is the whole of the
/// format: an absent row is one the user never touched and therefore takes
/// whatever this build's default becomes, while `null` is a user saying "give
/// this key back to my shell" and must survive the day the default changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Override {
    pub(crate) id: String,
    pub(crate) chord: Option<String>,
}

/// A line of `keybindings.json` this build could not honour, in the words a
/// notice would use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OverrideFault {
    pub(crate) id: String,
    pub(crate) reason: String,
}

impl Shortcuts {
    /// The table as this build ships it.
    #[must_use]
    pub(crate) fn defaults() -> Self {
        Self {
            rows: BINDINGS.to_vec(),
        }
    }

    /// Lay a `keybindings.json` over the defaults, reporting every line that
    /// could not be honoured.
    ///
    /// **The same three refusals the recorder makes** ([`chord_verdict`]), asked
    /// at the file's door. A hand-written file is a user's own words and this
    /// build reads them, but "Ctrl+Alt+P" is not a preference — it is a chord a
    /// German keyboard produces by typing `@`, and a file cannot consent to that
    /// on behalf of a layout. A refused line leaves that row at its default and
    /// says which line it was; the rest of the file still lands, because one bad
    /// row is not a reason to throw away nine good ones.
    ///
    /// An id this build does not know is refused the same way and for the
    /// ordinary reason rather than as corruption: a file written by a newer
    /// build, or a row this one has since renamed. §5.4's "逐叶降级" applied to a
    /// table.
    pub(crate) fn apply_overrides(&mut self, overrides: &[Override]) -> Vec<OverrideFault> {
        let mut faults = Vec::new();
        for entry in overrides {
            let Some(index) = self.rows.iter().position(|row| row.id == entry.id) else {
                faults.push(OverrideFault {
                    id: entry.id.clone(),
                    reason: format!("no shortcut is called {:?} in this build", entry.id),
                });
                continue;
            };
            let chord = match &entry.chord {
                None => None,
                Some(text) => {
                    let Some(chord) = parse_chord(text) else {
                        faults.push(OverrideFault {
                            id: entry.id.clone(),
                            reason: format!("{text:?} is not a chord this build can read"),
                        });
                        continue;
                    };
                    match chord_verdict(&self.rows, self.rows[index].id, &chord) {
                        ChordVerdict::Free => {}
                        refused => {
                            faults.push(OverrideFault {
                                id: entry.id.clone(),
                                reason: refused.hint().into_owned(),
                            });
                            continue;
                        }
                    }
                    Some(chord)
                }
            };
            self.rows[index].chord = chord;
        }
        faults
    }

    /// What this table says that the defaults do not — the whole of what is
    /// written back to disk.
    ///
    /// Derived by walking the two tables side by side rather than remembered as
    /// the user goes, so a row set back to its default by hand leaves *no* line
    /// behind: the file holds departures, and a line that says what the default
    /// already says is a line that would freeze today's default into a user's
    /// file for ever.
    #[must_use]
    pub(crate) fn overrides(&self) -> Vec<Override> {
        self.rows
            .iter()
            .zip(BINDINGS)
            .filter(|(row, default)| row.chord != default.chord)
            .map(|(row, _)| Override {
                id: row.id.to_owned(),
                chord: row.chord.as_ref().map(format_chord),
            })
            .collect()
    }

    /// Whether this row says something the defaults do not.
    #[must_use]
    pub(crate) fn is_overridden(&self, id: &str) -> bool {
        self.rows
            .iter()
            .zip(BINDINGS)
            .any(|(row, default)| row.id == id && row.chord != default.chord)
    }

    /// Give one row a chord, or take its chord away.
    pub(crate) fn set(&mut self, id: &str, chord: Option<Chord>) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.chord = chord;
        }
    }

    /// Put one row back the way this build ships it.
    pub(crate) fn restore(&mut self, id: &str) {
        if let Some((row, default)) = self
            .rows
            .iter_mut()
            .zip(BINDINGS)
            .find(|(row, _)| row.id == id)
        {
            row.chord.clone_from(&default.chord);
        }
    }

    /// Put the whole table back, which is the same as deleting the file.
    pub(crate) fn restore_all(&mut self) {
        *self = Self::defaults();
    }

    /// Whether a chord may be given to the row called `id`, and why not when it
    /// may not.
    #[must_use]
    pub(crate) fn verdict_for(&self, id: &str, chord: &Chord) -> ChordVerdict {
        chord_verdict(&self.rows, id, chord)
    }

    /// Resolve a key press to an action.
    ///
    /// `logical` is the key winit produced (Shift already applied); `base` is the same physical key
    /// with every modifier stripped, from `KeyEventExtModifierSupplement::key_without_modifiers`.
    /// `focus` is what the window's keyboard focus looks like, which is what decides whether a
    /// scoped row is in force at all — a row out of scope is not "found and ignored", it is not in
    /// the table for this press, so the key falls through to the encoder exactly as an unbound one
    /// does. A row whose chord is `None` is not in the table for *any* press, which is the whole of
    /// what "unbound" means.
    #[must_use]
    pub(crate) fn lookup(
        &self,
        logical: &Key,
        base: &Key,
        modifiers: ModifiersState,
        focus: Focus,
    ) -> Option<Action> {
        self.rows
            .iter()
            .find(|binding| {
                binding.scope.holds(focus)
                    && binding
                        .chord
                        .as_ref()
                        .is_some_and(|chord| chord.matches(logical, base, modifiers))
            })
            .map(|binding| binding.action)
    }

    /// **The lines the shortcut page draws**, folded, in the table's own order,
    /// with the reserved rows after them.
    ///
    /// Derived by one walk of [`Self::rows`] — a run of rows sharing a `family`
    /// becomes one line — for the reason the settings dialog derives its own
    /// headings the same way: a second list declaring which rows fold would be a
    /// second place to forget one, and the first forgotten fold is a verb that
    /// appears twice or not at all.
    #[must_use]
    pub(crate) fn editor_rows(&self) -> Vec<ShortcutRow> {
        let mut out: Vec<ShortcutRow> = Vec::new();
        let mut index = 0;
        while index < self.rows.len() {
            let head = &self.rows[index];
            let Some(family) = head.family else {
                out.push(ShortcutRow {
                    ids: vec![head.id],
                    title: head.title.text(),
                    note: head.note(),
                    caps: head.chord.as_ref().map(chord_caps).unwrap_or_default(),
                    recordable: true,
                    reserved: false,
                    overridden: self.is_overridden(head.id),
                });
                index += 1;
                continue;
            };
            let end = self.rows[index..]
                .iter()
                .position(|row| row.family != Some(family))
                .map_or(self.rows.len(), |offset| index + offset);
            let members = &self.rows[index..end];
            out.push(ShortcutRow {
                ids: members.iter().map(|row| row.id).collect(),
                title: family.text(),
                note: family_note(head, members),
                caps: fold_caps(members),
                // **A family is shown whole and recorded one slot at a time**,
                // and the recorder takes one chord — so until it learns to ask
                // *which* slot, a folded line offers no Record button. The file
                // still names every member, which is the door that stays open:
                // `keybindings.json` can move `goto-tab-9` on its own today.
                recordable: false,
                reserved: false,
                overridden: members.iter().any(|row| self.is_overridden(row.id)),
            });
            index = end;
        }
        out.extend(RESERVED_ROWS.iter().map(|row| ShortcutRow {
            ids: Vec::new(),
            title: row.title.text(),
            note: Some(Cow::Borrowed(NOTE_RESERVED_ALT_ARROW.text())),
            caps: row.caps.iter().map(|cap| (*cap).to_owned()).collect(),
            recordable: false,
            reserved: true,
            overridden: false,
        }));
        out
    }
}

/// What a folded line says under its name.
fn family_note(head: &Binding, members: &[Binding]) -> Option<Cow<'static, str>> {
    let unassigned = members.iter().filter(|row| row.chord.is_none()).count();
    let counted = if unassigned == 0 {
        NOTE_ONE_PER_MEMBER
    } else if unassigned == members.len() {
        NOTE_NONE_ASSIGNED
    } else {
        NOTE_SOME_UNASSIGNED
    }
    .text();
    match head.note() {
        None => Some(Cow::Borrowed(counted)),
        Some(note) => Some(Cow::Owned(format!("{note}{NOTE_JOIN}{counted}"))),
    }
}

/// The caps a folded line wears.
///
/// A run of members that share their modifiers and differ only by a
/// single-character key collapses to one cap reading `first – last`, which is
/// what `Ctrl+Shift+1 – 9` is. Anything else — a family whose members are not a
/// run, or one where some have no chord at all — shows the first chord it
/// actually has, because a range that skipped its gaps would be a line claiming
/// keys nobody can press.
fn fold_caps(members: &[Binding]) -> Vec<String> {
    let mut bound = members.iter().filter_map(|row| row.chord.as_ref());
    let Some(first) = bound.next() else {
        return Vec::new();
    };
    let all_bound = members.iter().all(|row| row.chord.is_some());
    let runs = all_bound
        && members.iter().all(|row| {
            row.chord.as_ref().is_some_and(|chord| {
                chord.modifiers == first.modifiers
                    && matches!(&chord.key, ChordKey::Character(text) if text.chars().count() == 1)
            })
        });
    if !runs || members.len() < 2 {
        return chord_caps(first);
    }
    let last = members
        .last()
        .and_then(|row| row.chord.as_ref())
        .unwrap_or(first);
    let mut caps = modifier_caps(first.modifiers);
    caps.push(format!(
        "{} – {}",
        key_label(&first.key),
        key_label(&last.key)
    ));
    caps
}

/// One line of the shortcut page.
///
/// A display type and not a borrow of the table, because what the page draws is
/// already a *derivation* of the table — folded, tagged, and with the reserved
/// rows the table deliberately does not hold appended to it. Handing the dialog
/// this instead of `&Shortcuts` is what keeps `settings.rs` from having to know
/// what a `Chord` is: geometry gets strings, and the chord grammar stays in the
/// one module that owns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutRow {
    /// The table rows this line stands for — one, or the whole of a family.
    /// Empty for a reserved row, which stands for no binding at all.
    pub ids: Vec<&'static str>,
    pub title: &'static str,
    /// The muted line under the title: where the row is in force, whether its
    /// verb has arrived, what a fold folded.
    pub note: Option<Cow<'static, str>>,
    /// The chord as key caps, left to right. Empty means no chord.
    pub caps: Vec<String>,
    /// Whether this line offers a Record button.
    pub recordable: bool,
    /// Whether this line is one the audit listed and declined.
    pub reserved: bool,
    /// Whether any of `ids` says something the defaults do not.
    pub overridden: bool,
}

/// Why a chord may not be given to a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChordVerdict {
    /// Nothing stands in its way.
    Free,
    /// Discipline ②: Windows reports AltGr as `Ctrl+Alt`.
    AltGrZone,
    /// Discipline ①: a bare `Ctrl+letter` is the shell's control-code alphabet.
    ShellControlLetter,
    /// Another row in this table already answers to it, in a focus state this
    /// one is also in force in.
    AlreadyUsed(Text),
}

impl ChordVerdict {
    /// The sentence the recorder shows, and the reason a refused file line
    /// carries.
    #[must_use]
    pub(crate) fn hint(&self) -> Cow<'static, str> {
        match self {
            Self::Free => Cow::Borrowed(""),
            Self::AltGrZone => Cow::Borrowed(hint_altgr_zone()),
            Self::ShellControlLetter => Cow::Borrowed(hint_shell_control_letter()),
            Self::AlreadyUsed(title) => {
                Cow::Owned(crate::i18n::shortcut_already_used(title.text()))
            }
        }
    }
}

/// **The one place a chord is judged**, read by the recorder while the keys are
/// still down and by [`Shortcuts::apply_overrides`] at the file's door.
///
/// The three refusals are the audit's own two disciplines plus the flat table's
/// oldest promise, and the third is deliberately asked with
/// [`Binding::conflicts_with`] — the same predicate the red gate
/// `the_table_holds_exactly_the_ruled_rows_and_no_chord_is_claimed_twice` runs
/// on, because a panel that judged conflicts by a second rule would let a user
/// build a table the test forbids.
///
/// **No swap is offered.** A "use it here and take it away from there" would
/// leave a second row silently unbound behind a dialog the user is already
/// looking away from, and a shortcut a user did not ask to lose is a shortcut
/// they will report as broken. The refusal names the row that has it, which is
/// what they need in order to go and clear it themselves.
#[must_use]
pub(crate) fn chord_verdict(rows: &[Binding], id: &str, chord: &Chord) -> ChordVerdict {
    let ctrl_alt = ModifiersState::CONTROL.union(ModifiersState::ALT);
    if chord.modifiers.contains(ctrl_alt) {
        return ChordVerdict::AltGrZone;
    }
    if chord.modifiers == ModifiersState::CONTROL
        && matches!(&chord.key, ChordKey::Character(text)
            if text.chars().count() == 1
                && text.chars().all(|glyph| glyph.is_ascii_alphabetic()))
    {
        return ChordVerdict::ShellControlLetter;
    }
    let Some(subject) = rows.iter().find(|row| row.id == id) else {
        return ChordVerdict::Free;
    };
    let claimed = Binding {
        chord: Some(chord.clone()),
        ..subject.clone()
    };
    rows.iter()
        .filter(|row| row.id != id)
        .find(|row| claimed.conflicts_with(row))
        .map_or(ChordVerdict::Free, |row| {
            ChordVerdict::AlreadyUsed(row.title)
        })
}

impl Binding {
    /// Whether these two rows would answer to one press.
    ///
    /// Two rows may share a chord **only** when no focus state holds both — which
    /// is what a scope is for, and also the one way scopes could quietly
    /// reintroduce the ambiguity the flat table forbade. Stated once here so the
    /// red gate and the recorder cannot drift apart.
    fn conflicts_with(&self, other: &Binding) -> bool {
        let (Some(mine), Some(theirs)) = (&self.chord, &other.chord) else {
            return false;
        };
        mine == theirs
            && REACHABLE_FOCUS
                .iter()
                .any(|focus| self.scope.holds(*focus) && other.scope.holds(*focus))
    }
}

/// Every focus the window can actually be in, and no impossible one.
///
/// The keyboard is on a preview, on a terminal's scrollback, on a scrollback
/// with the capsule up, or on none of those (a terminal running a full-screen
/// program, a files column, a menu). "A preview with a search open" is not a
/// state a window has, and listing it would make the conflict rule refuse pairs
/// that can never meet.
const REACHABLE_FOCUS: [Focus; 4] = [
    Focus {
        preview: false,
        terminal_primary: false,
        search_open: false,
    },
    Focus {
        preview: true,
        terminal_primary: false,
        search_open: false,
    },
    Focus {
        preview: false,
        terminal_primary: true,
        search_open: false,
    },
    Focus {
        preview: false,
        terminal_primary: true,
        search_open: true,
    },
];

// ── The chord's own grammar ────────────────────────────────────────────────
//
// One spelling on the wire and one on the glass, and they are deliberately not
// the same spelling. `keybindings.json` is a file a person edits in an editor
// with an ASCII keyboard, so it says `ArrowUp` and `Escape`; the key caps in the
// dialog are read at a glance beside a name, so they say `↑` and `Esc`. Writing
// the arrow into the file would be asking someone to paste a glyph they cannot
// type.

/// The modifiers, in the order Windows itself writes them.
fn modifier_caps(modifiers: ModifiersState) -> Vec<String> {
    let mut caps = Vec::new();
    if modifiers.control_key() {
        caps.push(MODIFIER_CTRL.to_owned());
    }
    if modifiers.alt_key() {
        caps.push(MODIFIER_ALT.to_owned());
    }
    if modifiers.shift_key() {
        caps.push(MODIFIER_SHIFT.to_owned());
    }
    caps
}

const MODIFIER_CTRL: &str = "Ctrl";
const MODIFIER_ALT: &str = "Alt";
const MODIFIER_SHIFT: &str = "Shift";

/// A chord as the caps the dialog draws, left to right.
#[must_use]
pub(crate) fn chord_caps(chord: &Chord) -> Vec<String> {
    let mut caps = modifier_caps(chord.modifiers);
    caps.push(key_label(&chord.key));
    caps
}

/// What one key is called **on the cap**, which is Q6's ruling made visible.
///
/// A `Character` is printed exactly as the table holds it, and the recorder
/// stores `key_without_modifiers` — so what a user recorded on an AZERTY board
/// comes back as the glyph printed on the key they pressed, not as the glyph
/// Shift made of it. The defaults are this file's own literals and are therefore
/// US-shaped; the matcher accepts either end of the Shift fold, so they still
/// *work* everywhere, and the day a layout API can name a cap is the day this
/// function grows a second reader.
fn key_label(key: &ChordKey) -> String {
    match key {
        ChordKey::Character(text) => text.to_uppercase(),
        ChordKey::Named(named) => named_label(*named).to_owned(),
    }
}

fn named_label(named: NamedKey) -> &'static str {
    match named {
        NamedKey::ArrowUp => "↑",
        NamedKey::ArrowDown => "↓",
        NamedKey::ArrowLeft => "←",
        NamedKey::ArrowRight => "→",
        NamedKey::Escape => "Esc",
        NamedKey::Space => "Space",
        other => named_name(other),
    }
}

/// What one key is called **in the file**.
fn named_name(named: NamedKey) -> &'static str {
    match named {
        NamedKey::Tab => "Tab",
        NamedKey::Enter => "Enter",
        NamedKey::Escape => "Escape",
        NamedKey::Space => "Space",
        NamedKey::Backspace => "Backspace",
        NamedKey::Delete => "Delete",
        NamedKey::Insert => "Insert",
        NamedKey::Home => "Home",
        NamedKey::End => "End",
        NamedKey::PageUp => "PageUp",
        NamedKey::PageDown => "PageDown",
        NamedKey::ArrowUp => "ArrowUp",
        NamedKey::ArrowDown => "ArrowDown",
        NamedKey::ArrowLeft => "ArrowLeft",
        NamedKey::ArrowRight => "ArrowRight",
        NamedKey::F1 => "F1",
        NamedKey::F2 => "F2",
        NamedKey::F3 => "F3",
        NamedKey::F4 => "F4",
        NamedKey::F5 => "F5",
        NamedKey::F6 => "F6",
        NamedKey::F7 => "F7",
        NamedKey::F8 => "F8",
        NamedKey::F9 => "F9",
        NamedKey::F10 => "F10",
        NamedKey::F11 => "F11",
        NamedKey::F12 => "F12",
        // Everything else this window has no way to receive as a chord. A name
        // it cannot parse back is a name it must not write.
        _ => "",
    }
}

/// Every named key this grammar can read, which is exactly the set
/// [`named_name`] can write. One list, walked from both ends, so the round trip
/// is a property rather than two tables that agree today.
const NAMED_KEYS: &[NamedKey] = &[
    NamedKey::Tab,
    NamedKey::Enter,
    NamedKey::Escape,
    NamedKey::Space,
    NamedKey::Backspace,
    NamedKey::Delete,
    NamedKey::Insert,
    NamedKey::Home,
    NamedKey::End,
    NamedKey::PageUp,
    NamedKey::PageDown,
    NamedKey::ArrowUp,
    NamedKey::ArrowDown,
    NamedKey::ArrowLeft,
    NamedKey::ArrowRight,
    NamedKey::F1,
    NamedKey::F2,
    NamedKey::F3,
    NamedKey::F4,
    NamedKey::F5,
    NamedKey::F6,
    NamedKey::F7,
    NamedKey::F8,
    NamedKey::F9,
    NamedKey::F10,
    NamedKey::F11,
    NamedKey::F12,
];

/// A chord as `keybindings.json` writes it: `Ctrl+Alt+Shift+Key`.
#[must_use]
pub(crate) fn format_chord(chord: &Chord) -> String {
    let mut out = String::new();
    if chord.modifiers.control_key() {
        out.push_str("Ctrl+");
    }
    if chord.modifiers.alt_key() {
        out.push_str("Alt+");
    }
    if chord.modifiers.shift_key() {
        out.push_str("Shift+");
    }
    match &chord.key {
        ChordKey::Character(text) => {
            let _ = write!(out, "{text}");
        }
        ChordKey::Named(named) => out.push_str(named_name(*named)),
    }
    out
}

/// Read a chord back, or `None` when the text is not one.
///
/// Modifiers are stripped one prefix at a time rather than by splitting on `+`,
/// so a chord *on* the plus key (`Ctrl++`) reads as the key it is rather than as
/// an empty name between two separators.
#[must_use]
pub(crate) fn parse_chord(text: &str) -> Option<Chord> {
    let mut modifiers = ModifiersState::empty();
    let mut rest = text;
    loop {
        let candidate = [
            ("Ctrl+", ModifiersState::CONTROL),
            ("Alt+", ModifiersState::ALT),
            ("Shift+", ModifiersState::SHIFT),
        ]
        .into_iter()
        .find(|(prefix, _)| rest.starts_with(prefix));
        let Some((prefix, flag)) = candidate else {
            break;
        };
        if modifiers.contains(flag) {
            return None;
        }
        modifiers |= flag;
        rest = &rest[prefix.len()..];
    }
    if rest.is_empty() {
        return None;
    }
    if let Some(named) = NAMED_KEYS
        .iter()
        .copied()
        .find(|named| named_name(*named) == rest)
    {
        return Some(Chord::new(modifiers, ChordKey::Named(named)));
    }
    // A single printed character and nothing longer: a multi-character remainder
    // is a named key this build does not know, and reading it as a "character"
    // would bind a chord no keyboard can produce.
    (rest.chars().count() == 1).then(|| {
        Chord::new(
            modifiers,
            ChordKey::Character(Cow::Owned(rest.to_lowercase())),
        )
    })
}

/// What one press means to a recorder that is listening.
///
/// The recorder is the one surface in this window that wants *every* key, so
/// four of them cannot be chords while it is up. That is a cost and it is
/// written down rather than discovered: bare `Esc`, `Backspace`, `Delete` and
/// `Enter` are the recorder's own verbs and cannot be recorded bare. Wearing any
/// modifier they are ordinary keys again — `Ctrl+Shift+Enter` records — because
/// the verbs are what a person reaches for with one finger while a box is
/// waiting, and nobody cancels a dialog with `Ctrl+Shift+Esc`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecordedKey {
    /// A modifier on its own. The box shows it and goes on waiting — this is
    /// what "shown live" is made of.
    Modifier,
    /// Bare `Esc`: leave the row as it was.
    Cancel,
    /// Bare `Backspace` or `Delete`: take the chord away entirely.
    Unbind,
    /// Bare `Enter`: take the candidate that is showing.
    Confirm,
    /// A chord, spelled with **`key_without_modifiers`** (Q6) so that what comes
    /// back is the glyph printed on the key that was pressed rather than the one
    /// Shift made of it.
    Chord(Chord),
    /// A key this grammar has no name for, so no file could hold it.
    Unusable,
}

/// Classify one press for the recorder.
///
/// `base` is `key_without_modifiers` and not the produced logical key, which is
/// the whole of Q6's ruling: the panel shows what is on the cap, so the cap is
/// what gets stored.
#[must_use]
pub(crate) fn classify_recording(base: &Key, modifiers: ModifiersState) -> RecordedKey {
    if let Key::Named(named) = base
        && is_modifier_key(*named)
    {
        return RecordedKey::Modifier;
    }
    if modifiers.is_empty()
        && let Key::Named(named) = base
    {
        match named {
            NamedKey::Escape => return RecordedKey::Cancel,
            NamedKey::Backspace | NamedKey::Delete => return RecordedKey::Unbind,
            NamedKey::Enter => return RecordedKey::Confirm,
            _ => {}
        }
    }
    match base {
        Key::Named(named) if NAMED_KEYS.contains(named) => {
            RecordedKey::Chord(Chord::new(modifiers, ChordKey::Named(*named)))
        }
        Key::Character(text) if text.chars().count() == 1 => RecordedKey::Chord(Chord::new(
            modifiers,
            ChordKey::Character(Cow::Owned(text.to_lowercase())),
        )),
        _ => RecordedKey::Unusable,
    }
}

const fn is_modifier_key(named: NamedKey) -> bool {
    matches!(
        named,
        NamedKey::Control
            | NamedKey::Shift
            | NamedKey::Alt
            | NamedKey::AltGraph
            | NamedKey::Super
            | NamedKey::Meta
            | NamedKey::Hyper
            | NamedKey::Symbol
            | NamedKey::Fn
            | NamedKey::FnLock
            | NamedKey::CapsLock
            | NamedKey::NumLock
            | NamedKey::ScrollLock
    )
}

/// The modifiers alone, as caps — what the box shows while the keys are still
/// going down.
#[must_use]
pub(crate) fn live_caps(modifiers: ModifiersState) -> Vec<String> {
    modifier_caps(modifiers)
}

impl Chord {
    fn matches(&self, logical: &Key, base: &Key, modifiers: ModifiersState) -> bool {
        // Exact, not "contains": a superset such as the retired Ctrl+Alt+Shift dev keys must miss
        // the table entirely rather than land on the Ctrl+Shift row underneath it.
        if modifiers != self.modifiers {
            return false;
        }
        match &self.key {
            ChordKey::Named(named) => {
                let named_matches = |key: &Key| matches!(key, Key::Named(other) if other == named);
                named_matches(logical) || named_matches(base)
            }
            // Shift folding: a US keyboard reports Shift+1 as "!" with a bare key of "1", while a
            // layout that puts the digit behind Shift reports the reverse. Accepting either end
            // binds the key the user sees printed on it in both cases.
            ChordKey::Character(text) => {
                let text_matches = |key: &Key| matches!(key, Key::Character(produced) if produced.eq_ignore_ascii_case(text.as_ref()));
                text_matches(logical) || text_matches(base)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand for the common case where Shift does not change the produced character.
    ///
    /// Pressed with the keyboard **on a terminal**, which is the focus every
    /// assertion in this module was written against before scopes existed and
    /// the one that has to keep answering the same way.
    ///
    /// Every scope is out of force here, which since 2026-08-16 makes this
    /// specifically *a terminal showing the alternate screen* — the harshest
    /// reading of "on a terminal", and the right one for the assertions that say
    /// what must reach the child.
    fn press(key: Key, modifiers: ModifiersState) -> Option<Action> {
        Shortcuts::defaults().lookup(&key, &key, modifiers, Focus::default())
    }

    /// The same press with the preview seat holding the focus.
    fn press_in_preview(key: Key, modifiers: ModifiersState) -> Option<Action> {
        Shortcuts::defaults().lookup(
            &key,
            &key,
            modifiers,
            Focus {
                preview: true,
                terminal_primary: false,
                search_open: false,
            },
        )
    }

    /// The same press on a terminal that is showing its own scrollback.
    fn press_on_primary_screen(key: Key, modifiers: ModifiersState) -> Option<Action> {
        Shortcuts::defaults().lookup(
            &key,
            &key,
            modifiers,
            Focus {
                preview: false,
                terminal_primary: true,
                search_open: false,
            },
        )
    }

    /// The same press with the search capsule up and the keyboard back on the shell — B81's
    /// second stance, which is the only one `F3` is about.
    fn press_with_search_open(key: Key, modifiers: ModifiersState) -> Option<Action> {
        Shortcuts::defaults().lookup(
            &key,
            &key,
            modifiers,
            Focus {
                preview: false,
                terminal_primary: true,
                search_open: true,
            },
        )
    }

    fn lookup_action(
        logical: &Key,
        base: &Key,
        modifiers: ModifiersState,
        focus: Focus,
    ) -> Option<Action> {
        Shortcuts::defaults().lookup(logical, base, modifiers, focus)
    }

    fn character(text: &str) -> Key {
        Key::Character(text.into())
    }

    const ALL_MODIFIER_COMBINATIONS: [ModifiersState; 8] = [
        ModifiersState::empty(),
        ModifiersState::SHIFT,
        ModifiersState::ALT,
        ModifiersState::ALT.union(ModifiersState::SHIFT),
        ModifiersState::CONTROL,
        ModifiersState::CONTROL.union(ModifiersState::SHIFT),
        ModifiersState::CONTROL.union(ModifiersState::ALT),
        ModifiersState::CONTROL
            .union(ModifiersState::ALT)
            .union(ModifiersState::SHIFT),
    ];

    /// PIN (multiwindow slice C) — **the door onto a second window, and the
    /// chord that did not move to make room for it.**
    ///
    /// Both halves in one test because the second is the reason for the first:
    /// every other terminal on this platform opens a window with
    /// `Ctrl+Shift+N`, and this product cannot, because the 2026-08-10 audit
    /// gave that chord to `New tab` and this slice is not the place to take it
    /// back. Red gate: point `new-window` at `Ctrl+Shift+N` and the second
    /// assertion names the row that was quietly rebound.
    #[test]
    fn a_second_window_has_a_chord_and_new_tab_keeps_its_own() {
        assert_eq!(press(character("m"), CTRL_SHIFT), Some(Action::NewWindow));
        assert_eq!(press(character("n"), CTRL_SHIFT), Some(Action::NewTab));
        // And nothing was taken from the shell to do it: `^M` is Return and stays
        // where Return is.
        assert_eq!(press(character("m"), CTRL), None);
    }

    /// Every row of the ruled table, asserted one binding at a time.
    #[test]
    fn every_ruled_binding_resolves_to_its_action() {
        assert_eq!(press(character("n"), CTRL_SHIFT), Some(Action::NewTab));
        assert_eq!(press(character("m"), CTRL_SHIFT), Some(Action::NewWindow));
        assert_eq!(press(character("w"), CTRL_SHIFT), Some(Action::ClosePane));
        assert_eq!(
            press(Key::Named(NamedKey::Tab), CTRL),
            Some(Action::NextTab)
        );
        assert_eq!(
            press(Key::Named(NamedKey::Tab), CTRL_SHIFT),
            Some(Action::PrevTab)
        );
        assert_eq!(
            press(character("t"), CTRL_SHIFT),
            Some(Action::ReopenClosed)
        );
        assert_eq!(
            press(character("a"), CTRL_SHIFT),
            Some(Action::JumpAttention)
        );
        assert_eq!(
            press(character("p"), CTRL_SHIFT),
            Some(Action::CommandPalette)
        );
        // Door 2 (§7.1.6b′ ②), and the row below it is half the assertion: the
        // chord this one deliberately did **not** take is still Find's.
        assert_eq!(
            press(character("z"), CTRL_SHIFT),
            Some(Action::ToggleFocusMode)
        );
        assert_eq!(
            press(character("f"), CTRL_SHIFT),
            None,
            "Ctrl+Shift+F is nobody's: it is Find's muscle memory, and focus mode \
             took Ctrl+Shift+Z rather than the chord people already aim at Find"
        );
        assert_eq!(
            press(character("-"), ALT_SHIFT),
            Some(Action::SplitHorizontal)
        );
        assert_eq!(
            press(character("="), ALT_SHIFT),
            Some(Action::SplitVertical)
        );
        // R28: the chord a decade of VS Code taught, wearing Shift so that a
        // bare `^G` — readline's "abort" — stays with the shell.
        assert_eq!(press(character("g"), CTRL_SHIFT), Some(Action::GitPage));
        assert_eq!(
            press(character("g"), CTRL),
            None,
            "Ctrl+G is readline's abort and belongs to the terminal"
        );
        assert_eq!(
            press(character("d"), CTRL_SHIFT),
            Some(Action::DuplicatePaneSplit)
        );
        assert_eq!(press(character("b"), CTRL_SHIFT), Some(Action::FilesPane));
        assert_eq!(press(character(","), CTRL), Some(Action::OpenSettings));
        assert_eq!(
            press_on_primary_screen(Key::Named(NamedKey::ArrowUp), CTRL_SHIFT),
            Some(Action::PrevCommandMark)
        );
        assert_eq!(
            press_on_primary_screen(Key::Named(NamedKey::ArrowDown), CTRL_SHIFT),
            Some(Action::NextCommandMark)
        );
        assert_eq!(
            press_on_primary_screen(character("f"), CTRL),
            Some(Action::OpenSearch)
        );
        assert_eq!(
            press_on_primary_screen(character("f"), CTRL_SHIFT),
            None,
            "and the retired alias reaches nothing (user ruling 2026-08-18)"
        );
        assert_eq!(
            press_with_search_open(Key::Named(NamedKey::F3), ModifiersState::empty()),
            Some(Action::NextMatch)
        );
        assert_eq!(
            press_with_search_open(Key::Named(NamedKey::F3), ModifiersState::SHIFT),
            Some(Action::PrevMatch)
        );
    }

    /// PIN (user ruling 2026-08-16, inventory D-2) — **`Ctrl+F` is the one bare
    /// control letter this table takes from a terminal, and it hands it straight
    /// back the moment there is a full-screen program on the glass.**
    ///
    /// Four promises, one assertion each: the chord opens the capsule on a
    /// scrollback; `Ctrl+Shift+F` is nobody's, because the second-chord row was
    /// retired on 2026-08-18 and this window does not take a chord it has no row
    /// for; the alternate screen keeps `^F`, so `less` still pages forward; and
    /// a document is not a scrollback.
    ///
    /// MUTATIONS:
    /// (1) give the row `Scope::Window` — the alternate-screen assertion goes
    ///     red, and so does `bare_control_letters_stay_with_the_terminal`, which
    ///     is discipline (1) noticing;
    /// (2) put the alias row back — the second assertion goes red, which is the
    ///     retirement refusing to be undone by a later tidy-up;
    /// (3) move the row to `CTRL_SHIFT` alone — the first goes red, which is the
    ///     ruling refusing to be renegotiated the same way.
    #[test]
    fn control_f_opens_the_search_on_a_scrollback_and_passes_through_on_the_alternate_screen() {
        assert_eq!(
            press_on_primary_screen(character("f"), CTRL),
            Some(Action::OpenSearch)
        );
        assert_eq!(
            press_on_primary_screen(character("f"), CTRL_SHIFT),
            None,
            "`Ctrl+Shift+F` goes to the shell: the second chord is something \
             a reader records, not something this build takes"
        );
        assert_eq!(
            press(character("f"), CTRL),
            None,
            "on the alternate screen ^F is the program's page-forward and reaches it untouched"
        );
        assert_eq!(
            press_in_preview(character("f"), CTRL),
            None,
            "a document is not a scrollback"
        );
    }

    /// PIN (B81) — **the function key walks the matches, and only while there
    /// are matches to walk.**
    ///
    /// `F3` is `cmd.exe`'s history recall and a dozen programs' help key. The
    /// scope is what keeps it theirs whenever the capsule is down, which is
    /// nearly always.
    ///
    /// MUTATIONS:
    /// (1) give the rows `Scope::TerminalPrimary` — the "no search" assertions
    ///     go red and every shell loses `F3` for good;
    /// (2) drop the Shift from the second row — the two chords collide and
    ///     `the_table_holds_exactly_the_ruled_rows_and_no_chord_is_claimed_twice`
    ///     goes red.
    #[test]
    fn the_function_key_walk_answers_only_while_a_search_is_open() {
        assert_eq!(
            press_with_search_open(Key::Named(NamedKey::F3), ModifiersState::empty()),
            Some(Action::NextMatch)
        );
        assert_eq!(
            press_with_search_open(Key::Named(NamedKey::F3), ModifiersState::SHIFT),
            Some(Action::PrevMatch)
        );
        assert_eq!(
            press_on_primary_screen(Key::Named(NamedKey::F3), ModifiersState::empty()),
            None,
            "with no capsule up F3 is the shell's"
        );
        assert_eq!(
            press_on_primary_screen(Key::Named(NamedKey::F3), ModifiersState::SHIFT),
            None
        );
        assert_eq!(
            press(Key::Named(NamedKey::F3), ModifiersState::empty()),
            None
        );
        // The capsule's own chord still answers while it is open - reopening
        // refocuses it and reselects the query (B80).
        assert_eq!(
            press_with_search_open(character("f"), CTRL),
            Some(Action::OpenSearch)
        );
    }

    /// PIN (user ruling 2026-08-16) — **the rail's keyboard door, and the screen
    /// it is shut on.**
    ///
    /// Three assertions for three separate promises: the walk answers on a
    /// terminal's own scrollback; the chord the mock-up asked for is refused
    /// because `Ctrl+Alt` is how Windows reports AltGr; and on the alternate
    /// screen the row is not in the table at all, so the bytes reach the
    /// full-screen program that owns the canvas.
    ///
    /// MUTATIONS:
    /// ① give the rows `Scope::Window` — the alternate-screen assertions go red,
    ///    and a `vim` user loses a chord to a rail with nothing to walk;
    /// ② put them back on `Ctrl+Alt` — the AltGr assertions go red, and so does
    ///    `the_altgr_family_is_never_claimed`.
    #[test]
    fn the_command_walk_answers_on_a_terminals_scrollback_and_nowhere_else() {
        let ctrl_alt = ModifiersState::CONTROL.union(ModifiersState::ALT);
        for (key, action) in [
            (NamedKey::ArrowUp, Action::PrevCommandMark),
            (NamedKey::ArrowDown, Action::NextCommandMark),
        ] {
            assert_eq!(
                press_on_primary_screen(Key::Named(key), CTRL_SHIFT),
                Some(action)
            );
            assert_eq!(
                press(Key::Named(key), CTRL_SHIFT),
                None,
                "the alternate screen keeps {key:?} — a full-screen program owns its canvas"
            );
            assert_eq!(
                press_on_primary_screen(Key::Named(key), ctrl_alt),
                None,
                "Ctrl+Alt is how Windows reports AltGr"
            );
            // `Ctrl+arrow` is readline's word movement and plain arrows are the
            // child's outright; neither is claimed on either screen.
            assert_eq!(press_on_primary_screen(Key::Named(key), CTRL), None);
            assert_eq!(
                press_on_primary_screen(Key::Named(key), ModifiersState::empty()),
                None
            );
        }
        // A preview seat is not a terminal, so the walk is not in force there
        // either — there is no scrollback in a document.
        assert_eq!(
            press_in_preview(Key::Named(NamedKey::ArrowUp), CTRL_SHIFT),
            None
        );
    }

    /// The Files row wears Shift because the key the mock-up asked for is the
    /// shell's. Red gate: bind `Action::FilesPane` to a bare `CTRL` and
    /// `bare_control_letters_stay_with_the_terminal` goes red on `Ctrl+B`.
    #[test]
    fn the_files_pane_row_leaves_control_b_to_the_shell() {
        assert_eq!(
            press(character("b"), CTRL),
            None,
            "^B is readline's back-one-character and tmux's prefix"
        );
        assert_eq!(press(character("b"), CTRL_SHIFT), Some(Action::FilesPane));
    }

    /// PIN (ruling 9, 2026-08-12) — **`Ctrl+S` is the preview's and nobody
    /// else's.**
    ///
    /// The two halves of the ruling, one assertion each: the chord reaches the
    /// action when the preview holds the focus, and it is not in the table at all
    /// when it does not — so `^S`, the terminal's flow-control stop, still goes
    /// to the shell.
    ///
    /// MUTATIONS:
    /// ① give the row `Scope::Window` — the second assertion goes red, and so
    ///    does `bare_control_letters_stay_with_the_terminal`;
    /// ② make `Scope::holds` return `true` for everything — the same two;
    /// ③ shift the row to `CTRL_SHIFT` "for consistency" — the first assertion
    ///    goes red, which is the ruling refusing to be quietly renegotiated.
    #[test]
    fn control_s_saves_inside_a_preview_and_reaches_the_shell_everywhere_else() {
        assert_eq!(
            press_in_preview(character("s"), CTRL),
            Some(Action::SavePreview)
        );
        assert_eq!(
            press(character("s"), CTRL),
            None,
            "^S is the terminal's flow-control stop wherever there is a terminal"
        );
        // Scoping a row does not make the *window's* rows conditional: they keep
        // answering with the preview focused, because a preview is not a modal.
        assert_eq!(
            press_in_preview(character("n"), CTRL_SHIFT),
            Some(Action::NewTab)
        );
        // And the scope does not smuggle in a shifted variant of its own.
        assert_eq!(press_in_preview(character("s"), CTRL_SHIFT), None);
    }

    #[test]
    fn goto_tab_covers_one_through_nine_and_stops_there() {
        for ordinal in 1..=9u8 {
            let text = ordinal.to_string();
            assert_eq!(
                press(character(&text), CTRL_SHIFT),
                Some(Action::GotoTab(ordinal)),
                "Ctrl+Shift+{ordinal} must select tab {ordinal}"
            );
        }
        assert_eq!(press(character("0"), CTRL_SHIFT), None);
    }

    /// The shifted glyph and the bare key are both accepted, so the binding survives the layout
    /// difference instead of depending on which of the two a keyboard happens to produce.
    #[test]
    fn shift_folded_keys_resolve_from_either_the_glyph_or_the_bare_key() {
        let terminal = Focus {
            preview: false,
            terminal_primary: true,
            search_open: false,
        };
        // US layout: Shift+1 produces "!", the bare key is "1".
        assert_eq!(
            lookup_action(&character("!"), &character("1"), CTRL_SHIFT, terminal),
            Some(Action::GotoTab(1))
        );
        // A layout that reaches the digit through Shift produces "1" with a different bare key.
        assert_eq!(
            lookup_action(&character("1"), &character("&"), CTRL_SHIFT, terminal),
            Some(Action::GotoTab(1))
        );
        // US layout: Shift+- produces "_", Shift+= produces "+".
        assert_eq!(
            lookup_action(&character("_"), &character("-"), ALT_SHIFT, terminal),
            Some(Action::SplitHorizontal)
        );
        assert_eq!(
            lookup_action(&character("+"), &character("="), ALT_SHIFT, terminal),
            Some(Action::SplitVertical)
        );
    }

    #[test]
    fn letter_bindings_ignore_case_of_the_produced_glyph() {
        assert_eq!(press(character("N"), CTRL_SHIFT), Some(Action::NewTab));
        assert_eq!(press(character("W"), CTRL_SHIFT), Some(Action::ClosePane));
    }

    /// A chord fires on its exact modifier set and on no other, so a superset such as the retired
    /// `Ctrl+Alt+Shift` dev keys cannot revive a binding, and AltGr (reported as Ctrl+Alt) can
    /// never reach one.
    #[test]
    fn bindings_require_an_exact_modifier_match() {
        for modifiers in ALL_MODIFIER_COMBINATIONS {
            if modifiers != CTRL_SHIFT {
                assert_eq!(
                    press(character("n"), modifiers),
                    None,
                    "n must not bind under {modifiers:?}"
                );
            }
            // Tab is the one key whose two bindings differ only by Shift, so both are named here
            // and everything else must stay unbound.
            if modifiers != CTRL && modifiers != CTRL_SHIFT {
                assert_eq!(
                    press(Key::Named(NamedKey::Tab), modifiers),
                    None,
                    "Tab must not bind under {modifiers:?}"
                );
            }
        }
    }

    /// AltGr arrives as Ctrl+Alt on Windows; the audit forbids that whole family.
    #[test]
    fn the_altgr_family_is_never_claimed() {
        let ctrl_alt = ModifiersState::CONTROL.union(ModifiersState::ALT);
        let ctrl_alt_shift = ctrl_alt.union(ModifiersState::SHIFT);
        for text in [
            "a", "b", "d", "e", "f", "g", "n", "p", "s", "t", "w", "-", "=", ",", "1", "9",
        ] {
            assert_eq!(press(character(text), ctrl_alt), None, "AltGr+{text}");
            assert_eq!(
                press(character(text), ctrl_alt_shift),
                None,
                "AltGr+Shift+{text}"
            );
            // The scoped row is held to the same discipline: AltGr must not
            // reach it either, wherever the keyboard is.
            assert_eq!(
                press_in_preview(character(text), ctrl_alt),
                None,
                "AltGr+{text} in a preview"
            );
            // And on the screen where the one bare letter in this table *is*
            // claimed: AltGr must not reach `Ctrl+F` either.
            assert_eq!(
                press_on_primary_screen(character(text), ctrl_alt),
                None,
                "AltGr+{text} on a scrollback"
            );
        }
        // The named keys the table claims are held to the same discipline, on
        // every screen: the command walk was the mock-up's one `Ctrl+Alt` row and
        // this is the assertion that keeps it from coming back.
        for key in [NamedKey::ArrowUp, NamedKey::ArrowDown, NamedKey::Tab] {
            assert_eq!(press(Key::Named(key), ctrl_alt), None, "AltGr+{key:?}");
            assert_eq!(
                press_on_primary_screen(Key::Named(key), ctrl_alt),
                None,
                "AltGr+{key:?} on a scrollback"
            );
            assert_eq!(
                press_on_primary_screen(Key::Named(key), ctrl_alt_shift),
                None,
                "AltGr+Shift+{key:?} on a scrollback"
            );
        }
    }

    /// The retired development keys must resolve to nothing at all.
    #[test]
    fn retired_development_keys_are_unbound() {
        let ctrl_alt_shift = ModifiersState::CONTROL
            .union(ModifiersState::ALT)
            .union(ModifiersState::SHIFT);
        assert_eq!(press(character("d"), ctrl_alt_shift), None);
        assert_eq!(press(character("e"), ctrl_alt_shift), None);
        assert_eq!(press(Key::Named(NamedKey::F9), CTRL_SHIFT), None);
    }

    /// Bare and plain-Shift typing belongs to the shell; the table must never intercept it.
    #[test]
    fn unmodified_typing_is_never_intercepted() {
        for text in [
            "a", "b", "f", "g", "n", "w", "t", "d", "p", "s", "-", "=", ",", "1", "9",
        ] {
            assert_eq!(press(character(text), ModifiersState::empty()), None);
            assert_eq!(press(character(text), ModifiersState::SHIFT), None);
            assert_eq!(
                press_in_preview(character(text), ModifiersState::empty()),
                None,
                "typing into a preview is typing"
            );
        }
        assert_eq!(
            press(Key::Named(NamedKey::Enter), ModifiersState::empty()),
            None
        );
        assert_eq!(
            press(Key::Named(NamedKey::Escape), ModifiersState::empty()),
            None
        );
    }

    /// Bare `Ctrl+letter` is the shell's control-code alphabet and stays untouched, which is the
    /// reason every window action above wears Shift.
    ///
    /// Asserted with the keyboard on a terminal, which is the only focus the
    /// sentence is about: `Ctrl+S` inside a preview is a scoped row and there is
    /// no shell there to take it from (see
    /// `control_s_saves_inside_a_preview_and_reaches_the_shell_everywhere_else`).
    #[test]
    fn bare_control_letters_stay_with_the_terminal() {
        for letter in 'a'..='z' {
            assert_eq!(
                press(character(&letter.to_string()), CTRL),
                None,
                "Ctrl+{letter} belongs to the terminal"
            );
        }
        // The sweep above is taken on the alternate screen, where every scoped
        // row is out of force - so it would stay green even if `Ctrl+F` had been
        // given `Scope::Window` by mistake. This is the half that would not:
        // twenty-five of the twenty-six are still the shell's on a scrollback,
        // and the twenty-sixth is named out loud.
        for letter in 'a'..='z' {
            let expected = if letter == 'f' {
                Some(Action::OpenSearch)
            } else {
                None
            };
            assert_eq!(
                press_on_primary_screen(character(&letter.to_string()), CTRL),
                expected,
                "Ctrl+{letter} on a scrollback"
            );
        }
    }

    #[test]
    fn the_table_holds_exactly_the_ruled_rows_and_no_chord_is_claimed_twice() {
        // 21 single actions plus GotoTab(1..=9), plus the four picture-in-picture
        // summon slots (2026-08-17), of which only the first ships with a chord.
        // The twentieth is `new-window` (multiwindow slice C, 2026-08-19) and the
        // twenty-first is `toggle-focus-mode` on Ctrl+Shift+Z (focus F1, same day).
        //
        // **No row names a verb twice** (user ruling 2026-08-18). `Ctrl+Shift+F`
        // used to ride here as a second chord for `OpenSearch`, and it was the
        // only place in the table where two rows meant one thing; it is retired,
        // and anybody who wants it records it onto a row of their own.
        assert_eq!(BINDINGS.len(), 34);
        assert_eq!(
            BINDINGS
                .iter()
                .filter(|binding| binding.action == Action::OpenSearch)
                .count(),
            1,
            "one verb, one row"
        );

        // Two rows may share a chord only if no focus state has both in force —
        // which is what a scope is *for*, and also the one way scopes could
        // quietly reintroduce the ambiguity the flat table forbade. Asked through
        // `conflicts_with`, which is the predicate the recorder and the override
        // loader also ask: a panel judging conflicts by a second rule could build
        // a table this gate forbids.
        for (index, binding) in BINDINGS.iter().enumerate() {
            for other in BINDINGS.iter().skip(index + 1) {
                assert!(
                    !binding.conflicts_with(other),
                    "{:?} reuses a chord already claimed above it in the same focus",
                    other.action
                );
            }
        }

        for binding in BINDINGS {
            assert!(
                !matches!(binding.action, Action::GotoTab(ordinal) if !(1..=9).contains(&ordinal)),
                "GotoTab rows are limited to 1..=9"
            );
            assert!(
                !matches!(binding.action, Action::SummonPip(slot) if !(1..=4).contains(&slot)),
                "PiP slots are limited to 1..=4"
            );
        }
    }

    /// Every id the table names must be its own, because the file names rows by
    /// id and two rows answering to one name is a file with an ambiguous line.
    #[test]
    fn every_row_is_named_once_and_names_itself() {
        for (index, binding) in BINDINGS.iter().enumerate() {
            assert!(!binding.id.is_empty(), "{:?} has no id", binding.action);
            assert!(
                !binding.title.text().is_empty(),
                "{} has no name",
                binding.id
            );
            for other in BINDINGS.iter().skip(index + 1) {
                assert_ne!(binding.id, other.id, "two rows answer to one id");
            }
        }
    }

    /// Every action the enum can name must actually be a row of the table —
    /// **with or without a chord**, because "unbound" is a state a row is in and
    /// not a row that is missing.
    #[test]
    fn every_action_is_a_row_of_the_table() {
        let mut expected = vec![
            Action::NewTab,
            Action::ClosePane,
            Action::NextTab,
            Action::PrevTab,
            Action::ReopenClosed,
            Action::JumpAttention,
            Action::CommandPalette,
            Action::ToggleFocusMode,
            Action::SplitHorizontal,
            Action::SplitVertical,
            Action::DuplicatePaneSplit,
            Action::FilesPane,
            Action::GitPage,
            Action::OpenSettings,
            Action::SavePreview,
            Action::PrevCommandMark,
            Action::NextCommandMark,
            Action::OpenSearch,
            Action::NextMatch,
            Action::PrevMatch,
        ];
        expected.extend((1..=9u8).map(Action::GotoTab));
        expected.extend((1..=4u8).map(Action::SummonPip));

        for action in expected {
            assert!(
                BINDINGS.iter().any(|binding| binding.action == action),
                "{action:?} is not in the table"
            );
        }
        // And the four that ship without one say so rather than being absent.
        for slot in 1..=4u8 {
            let row = BINDINGS
                .iter()
                .find(|binding| binding.action == Action::SummonPip(slot))
                .expect("the slot is a row");
            assert!(
                row.chord.is_none(),
                "slot {slot} ships unassigned - see Action::SummonPip"
            );
        }
    }

    // ── the editable table (Settings extension block, slice 2) ─────────────

    /// PIN — **a chord survives the round trip through the file it is written
    /// to**, for every chord this build ships and every named key the grammar
    /// can spell.
    ///
    /// The file is the thing that outlives the process, so this is the only
    /// property that matters about the two spellings: what `format_chord` writes
    /// is what `parse_chord` reads back, and nothing in between quietly becomes
    /// a different key. The named list is walked from both ends so the writer
    /// and the reader cannot disagree about a name.
    ///
    /// MUTATIONS:
    /// (1) drop a name from `named_name` — its entry writes an empty string and
    ///     the round trip goes red;
    /// (2) split the modifiers on `+` instead of stripping prefixes — the chord
    ///     on the plus key goes red.
    #[test]
    fn every_chord_this_build_writes_is_a_chord_it_can_read_back() {
        for binding in BINDINGS {
            let Some(chord) = &binding.chord else {
                continue;
            };
            let text = format_chord(chord);
            assert_eq!(
                parse_chord(&text).as_ref(),
                Some(chord),
                "{} writes {text:?} and cannot read it back",
                binding.id
            );
        }
        for named in NAMED_KEYS {
            for modifiers in ALL_MODIFIER_COMBINATIONS {
                let chord = Chord::new(modifiers, ChordKey::Named(*named));
                let text = format_chord(&chord);
                assert_eq!(parse_chord(&text), Some(chord), "{text:?}");
            }
        }
        // The two punctuation keys the separator itself lives on.
        for glyph in ["+", "-", "=", ",", "/"] {
            let chord = Chord::new(CTRL_SHIFT, ChordKey::Character(Cow::Borrowed(glyph)));
            let text = format_chord(&chord);
            assert_eq!(parse_chord(&text), Some(chord), "{text:?}");
        }
        assert_eq!(
            format_chord(&Chord::new(CTRL_SHIFT, super::character("n"))),
            "Ctrl+Shift+n"
        );
        assert_eq!(
            format_chord(&Chord::new(
                ModifiersState::CONTROL
                    .union(ModifiersState::ALT)
                    .union(ModifiersState::SHIFT),
                ChordKey::Named(NamedKey::F9)
            )),
            "Ctrl+Alt+Shift+F9",
            "one order on the wire, and it is the one Windows itself writes"
        );
        // Rubbish is refused rather than guessed at.
        for text in ["", "Ctrl+", "Ctrl+Ctrl+n", "Meta+n", "Ctrl+NotAKey"] {
            assert_eq!(parse_chord(text), None, "{text:?}");
        }
    }

    /// PIN (Q6, 2026-08-17) — **the cap the panel draws is the cap on the user's
    /// keyboard**, which is what "display `key_without_modifiers`" means once it
    /// is made of parts.
    ///
    /// Two halves. The recorder stores the *unmodified* key, so a chord recorded
    /// on an AZERTY board where the digit lives behind Shift comes back spelled
    /// with the glyph printed on that key — not with the digit Shift made of it.
    /// And the renderer prints the stored key verbatim, so the two halves are
    /// one decision rather than a store and a guess.
    ///
    /// MUTATIONS:
    /// (1) classify from the produced logical key instead — the AZERTY assertion
    ///     goes red and a French user's panel shows a `1` on a key printed `&`;
    /// (2) upper-case nothing in `key_label` — the cap reads `n` where the key
    ///     says `N`.
    #[test]
    fn a_recorded_chord_wears_the_glyph_printed_on_the_key_that_was_pressed() {
        // US: Shift+1 produces "!", the unmodified key is "1".
        let us = classify_recording(&Key::Character("1".into()), CTRL_SHIFT);
        let RecordedKey::Chord(us) = us else {
            panic!("a digit is a chord");
        };
        assert_eq!(chord_caps(&us), vec!["Ctrl", "Shift", "1"]);

        // AZERTY: the same physical key is printed `&`, and the digit is what
        // Shift makes of it. `key_without_modifiers` is `&`, so that is the cap.
        let azerty = classify_recording(&Key::Character("&".into()), CTRL_SHIFT);
        let RecordedKey::Chord(azerty) = azerty else {
            panic!("a punctuation key is a chord");
        };
        assert_eq!(
            chord_caps(&azerty),
            vec!["Ctrl", "Shift", "&"],
            "the panel shows what is printed on the key, not what Shift made of it"
        );
        assert_eq!(
            format_chord(&azerty),
            "Ctrl+Shift+&",
            "and the file holds the same key"
        );

        // Letters are shown as they are printed on the key, which is upper case.
        let letter = classify_recording(&Key::Character("n".into()), CTRL_SHIFT);
        let RecordedKey::Chord(letter) = letter else {
            panic!("a letter is a chord");
        };
        assert_eq!(chord_caps(&letter), vec!["Ctrl", "Shift", "N"]);

        // Named keys wear the glyph a person reads, not the word a file holds.
        assert_eq!(
            chord_caps(&Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::ArrowUp))),
            vec!["Ctrl", "Shift", "↑"]
        );
        assert_eq!(
            chord_caps(&Chord::new(
                ModifiersState::empty(),
                ChordKey::Named(NamedKey::Escape)
            )),
            vec!["Esc"]
        );
    }

    /// PIN (S64, 2026-08-17) — **the three refusals, decided in one place and
    /// therefore the same at both doors.**
    ///
    /// `Ctrl+Alt` because Windows reports AltGr as exactly that pair; a bare
    /// `Ctrl+letter` because it is the shell's control-code alphabet; and a
    /// chord another row already answers to, judged by the very predicate the
    /// table's own red gate runs on.
    ///
    /// **The `Ctrl+F` exception is not inherited**, and that is asserted rather
    /// than assumed: the ruling that let `Ctrl+F` through was about one chord,
    /// one surface and three reasons, and a recorder that read it as a policy
    /// would have quietly handed the shell's alphabet to anybody who asked.
    ///
    /// MUTATIONS:
    /// (1) drop the `Ctrl+Alt` arm — a German keyboard can bind its own `@`;
    /// (2) judge conflicts by "same chord" without the scope overlap — the
    ///     preview's `Ctrl+S` becomes unbindable from a window row that could
    ///     never meet it;
    /// (3) offer a swap instead of refusing — nothing here goes red, which is
    ///     why the refusal is also written into `chord_verdict`'s own doc.
    #[test]
    fn a_chord_is_refused_for_altgr_for_the_shells_alphabet_and_for_a_row_that_has_it() {
        let table = Shortcuts::defaults();
        let ctrl_alt = ModifiersState::CONTROL.union(ModifiersState::ALT);
        for modifiers in [ctrl_alt, ctrl_alt.union(ModifiersState::SHIFT)] {
            assert_eq!(
                table.verdict_for("new-tab", &Chord::new(modifiers, super::character("p"))),
                ChordVerdict::AltGrZone,
                "Windows reports AltGr as Ctrl+Alt"
            );
        }
        for letter in 'a'..='z' {
            let chord = Chord::new(CTRL, ChordKey::Character(Cow::Owned(letter.to_string())));
            assert_eq!(
                table.verdict_for("new-tab", &chord),
                ChordVerdict::ShellControlLetter,
                "Ctrl+{letter} belongs to the terminal"
            );
        }
        assert_eq!(
            table.verdict_for("open-search", &Chord::new(CTRL, super::character("f"))),
            ChordVerdict::ShellControlLetter,
            "the ruling that let Ctrl+F through was about that chord, not about \
             the family - the recorder refuses to re-derive it"
        );
        assert_eq!(
            table.verdict_for("new-tab", &Chord::new(CTRL_SHIFT, super::character("w"))),
            ChordVerdict::AlreadyUsed(Text::ClosePane),
            "the refusal names the row that has it, which is what a user needs \
             in order to go and clear it"
        );
        // A row may be given the chord it already has: that is not a conflict
        // with itself, it is a no-op the recorder must not refuse.
        assert_eq!(
            table.verdict_for("new-tab", &Chord::new(CTRL_SHIFT, super::character("n"))),
            ChordVerdict::Free
        );
        // Two rows may share a chord when no focus state holds both, which is
        // what a scope is for — and the recorder reads the same predicate.
        assert_eq!(
            table.verdict_for(
                "save-preview",
                &Chord::new(CTRL_SHIFT, super::character("j"))
            ),
            ChordVerdict::Free
        );
        assert_eq!(
            table.verdict_for("new-tab", &Chord::new(CTRL_SHIFT, super::character("j"))),
            ChordVerdict::Free,
            "a chord nobody claims is free"
        );
    }

    /// PIN (N25, restated 2026-08-17) — **the AltGr forbidden zone is empty, and
    /// stays empty however the table is edited.**
    ///
    /// §7.1.5e used to end with "`Ctrl+Alt+Shift+P` (preview seat) is kept,
    /// because it is that feature's only door". It is not kept: the scaffold
    /// retired with N25 when every real door existed, and the sentence outlived
    /// the fact. So there was never a chord to move out of the zone — what this
    /// slice owed was the proof that nothing can get *back into* it, from either
    /// door, which is the assertion below.
    ///
    /// MUTATIONS:
    /// (1) add any `Ctrl+Alt` row to `BINDINGS` — the first loop goes red;
    /// (2) let `apply_overrides` take a `Ctrl+Alt` line — the second does.
    #[test]
    fn no_row_of_the_table_can_ever_be_in_the_altgr_forbidden_zone() {
        let ctrl_alt = ModifiersState::CONTROL.union(ModifiersState::ALT);
        for binding in BINDINGS {
            if let Some(chord) = &binding.chord {
                assert!(
                    !chord.modifiers.contains(ctrl_alt),
                    "{} is in the forbidden zone",
                    binding.id
                );
            }
        }
        let mut table = Shortcuts::defaults();
        let faults = table.apply_overrides(&[Override {
            id: "command-palette".to_owned(),
            chord: Some("Ctrl+Alt+Shift+P".to_owned()),
        }]);
        assert_eq!(faults.len(), 1, "the file's own line is refused too");
        assert_eq!(faults[0].reason, hint_altgr_zone());
        assert_eq!(
            table,
            Shortcuts::defaults(),
            "and the row it named is untouched"
        );
    }

    /// PIN (Q7 = B, 2026-08-17) — **defaults plus overrides is the effective
    /// table, and only the departures are written back.**
    ///
    /// Four claims in one round trip, because they are one mechanism: an
    /// override lands; `null` clears a row outright; a row set back to its
    /// default leaves *no* line behind; and dispatch reads the table it lands
    /// on, so a chord a user just recorded answers on the very next press.
    ///
    /// The last of the four is the one that would be easiest to lose: a table
    /// whose `overrides()` wrote every row would freeze today's defaults into
    /// every user's file for ever, and the next build's retune would reach
    /// nobody.
    ///
    /// MUTATIONS:
    /// (1) write every row from `overrides()` — the "only departures" assertion
    ///     goes red;
    /// (2) treat `None` as "no opinion" instead of "unbound" — `Ctrl+Shift+F`
    ///     answers again after being cleared.
    #[test]
    fn overrides_land_on_the_defaults_and_only_the_departures_come_back() {
        let mut table = Shortcuts::defaults();
        assert!(
            table.overrides().is_empty(),
            "a fresh table departs nowhere"
        );

        let faults = table.apply_overrides(&[
            Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+Y".to_owned()),
            },
            Override {
                id: "next-match".to_owned(),
                chord: None,
            },
        ]);
        assert!(faults.is_empty(), "{faults:?}");

        let key = |text: &str| Key::Character(text.into());
        assert_eq!(
            table.lookup(&key("y"), &key("y"), CTRL_SHIFT, Focus::default()),
            Some(Action::NewTab),
            "dispatch reads the effective table, with no reload in between"
        );
        assert_eq!(
            table.lookup(&key("n"), &key("n"), CTRL_SHIFT, Focus::default()),
            None,
            "and the chord it replaced is nobody's"
        );
        let searching = Focus {
            preview: false,
            terminal_primary: true,
            search_open: true,
        };
        let f3 = Key::Named(NamedKey::F3);
        assert_eq!(
            table.lookup(&f3, &f3, ModifiersState::empty(), searching),
            None,
            "an explicitly cleared row is unbound, not defaulted"
        );
        assert_eq!(
            table.lookup(&f3, &f3, ModifiersState::SHIFT, searching),
            Some(Action::PrevMatch),
            "and its sibling row is untouched"
        );

        assert_eq!(
            table.overrides(),
            vec![
                Override {
                    id: "new-tab".to_owned(),
                    chord: Some("Ctrl+Shift+y".to_owned()),
                },
                Override {
                    id: "next-match".to_owned(),
                    chord: None,
                },
            ],
            "only the two rows that depart, in the table's own order"
        );
        assert!(table.is_overridden("new-tab"));
        assert!(!table.is_overridden("close-pane"));

        // A row given back its own default leaves nothing behind.
        table.set(
            "new-tab",
            Some(Chord::new(CTRL_SHIFT, super::character("n"))),
        );
        assert_eq!(
            table.overrides(),
            vec![Override {
                id: "next-match".to_owned(),
                chord: None,
            }],
            "a line that says what the default says is a line that would freeze it"
        );

        table.restore("next-match");
        assert!(table.overrides().is_empty());
        assert_eq!(table, Shortcuts::defaults());
    }

    /// PIN — **restore, per row and whole.**
    ///
    /// A row restored puts back that row and leaves its neighbours alone; the
    /// whole table restored is the same thing as deleting the file, which is
    /// what makes it one write rather than a walk.
    #[test]
    fn a_row_and_the_whole_table_can_be_put_back_the_way_this_build_ships_them() {
        let mut table = Shortcuts::defaults();
        table.apply_overrides(&[
            Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+Y".to_owned()),
            },
            Override {
                id: "close-pane".to_owned(),
                chord: None,
            },
        ]);
        assert_eq!(table.overrides().len(), 2);

        table.restore("new-tab");
        assert!(!table.is_overridden("new-tab"));
        assert!(
            table.is_overridden("close-pane"),
            "one row back does not take its neighbour with it"
        );

        table.restore_all();
        assert_eq!(table, Shortcuts::defaults());
        assert!(table.overrides().is_empty(), "which is an empty file");
    }

    /// PIN — **a line a build cannot honour is one row degrading, never the
    /// file being thrown away** (§5.4 逐叶降级, applied to a table).
    ///
    /// An id from a newer build, a chord this grammar cannot read, a chord in
    /// the forbidden zone and a bare control letter each cost their own row and
    /// nothing else — the good lines around them still land.
    #[test]
    fn a_line_this_build_cannot_honour_costs_its_own_row_and_no_other() {
        let mut table = Shortcuts::defaults();
        let faults = table.apply_overrides(&[
            Override {
                id: "summon-teleporter".to_owned(),
                chord: Some("Ctrl+Shift+Z".to_owned()),
            },
            Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+Nonsense".to_owned()),
            },
            Override {
                id: "close-pane".to_owned(),
                chord: Some("Ctrl+Alt+Q".to_owned()),
            },
            Override {
                id: "reopen-closed".to_owned(),
                chord: Some("Ctrl+Q".to_owned()),
            },
            Override {
                id: "git-page".to_owned(),
                chord: Some("Ctrl+Shift+Y".to_owned()),
            },
        ]);
        assert_eq!(faults.len(), 4, "{faults:?}");
        assert_eq!(
            faults
                .iter()
                .map(|fault| fault.id.as_str())
                .collect::<Vec<_>>(),
            [
                "summon-teleporter",
                "new-tab",
                "close-pane",
                "reopen-closed"
            ]
        );
        assert_eq!(
            table.overrides(),
            vec![Override {
                id: "git-page".to_owned(),
                chord: Some("Ctrl+Shift+y".to_owned()),
            }],
            "the one good line still landed"
        );
    }

    /// PIN (user ruling 2026-08-18) — **a `keybindings.json` written before the
    /// second-chord row was retired still loads, and the row it lost is named.**
    ///
    /// Retiring a row is the one edit to this table that reaches back into files
    /// people already have. The contract is the one §5.4 states for every stored
    /// document and this table already keeps for an id from a *newer* build: the
    /// line that cannot be honoured costs its own line and nothing else. So the
    /// user's other bindings all land, the retired line is reported by id — the
    /// window puts that sentence in front of them — and the id is not silently
    /// re-bound to the row that kept the verb, because a chord this build gave
    /// away is not a chord it may take back on a user's behalf.
    ///
    /// The stale line also removes itself: `overrides()` is derived by walking
    /// the live table, so the next time anything in this dialog is edited the
    /// file is rewritten without it.
    ///
    /// Red gate: keep `open-search-alias` in `BINDINGS` and the fault
    /// disappears; make `apply_overrides` stop at the first unknown id and the
    /// second assertion goes red.
    #[test]
    fn a_file_naming_the_retired_alias_still_loads_and_says_which_row_it_lost() {
        let mut table = Shortcuts::defaults();
        let faults = table.apply_overrides(&[
            Override {
                id: "open-search-alias".to_owned(),
                chord: Some("Ctrl+Shift+F".to_owned()),
            },
            Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+Y".to_owned()),
            },
        ]);
        assert_eq!(
            faults
                .iter()
                .map(|fault| fault.id.as_str())
                .collect::<Vec<_>>(),
            ["open-search-alias"],
            "the retired row is named, once: {faults:?}"
        );
        let key = |text: &str| Key::Character(text.into());
        assert_eq!(
            table.lookup(&key("y"), &key("y"), CTRL_SHIFT, Focus::default()),
            Some(Action::NewTab),
            "and every other line in the file landed"
        );
        let on_scrollback = Focus {
            preview: false,
            terminal_primary: true,
            search_open: false,
        };
        assert_eq!(
            table.lookup(&key("f"), &key("f"), CTRL_SHIFT, on_scrollback),
            None,
            "the chord the retired row carried is nobody's again, and the file \
             asking for it does not put it back"
        );
        assert_eq!(
            table.lookup(&key("f"), &key("f"), CTRL, on_scrollback),
            Some(Action::OpenSearch),
            "while the row that kept the verb is untouched"
        );
        assert_eq!(
            table.overrides(),
            vec![Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+y".to_owned()),
            }],
            "and the stale line is not written back out"
        );
    }

    /// PIN (S1/S3/S67, 2026-08-17) — **the editor's list is the table, folded,
    /// with the rows the audit declined after it.**
    ///
    /// Every row of `BINDINGS` is on the page exactly once — through a line of
    /// its own or through the family line that folded it — so no verb is missing
    /// and none appears twice. The nine tab ordinals are one line, the four
    /// picture-in-picture slots are another, and the two `Alt`+arrow families
    /// the audit listed and never took are there as well, greyed and offering no
    /// Record button, "so the table stays the whole ruling".
    ///
    /// MUTATIONS:
    /// (1) fold by walking a declared list of families instead of the `family`
    ///     column — a family whose members stop being contiguous silently
    ///     appears twice and the count goes red;
    /// (2) drop the reserved rows — the greyed assertion goes red and the panel
    ///     stops being an editor for the whole ruling;
    /// (3) make a reserved row recordable — the last assertion goes red, and a
    ///     user can bind a chord the audit never decided to take.
    #[test]
    fn the_editor_lists_every_row_of_the_table_once_and_the_declined_ones_after_it() {
        let table = Shortcuts::defaults();
        let lines = table.editor_rows();

        let mut seen: Vec<&str> = Vec::new();
        for line in &lines {
            for id in &line.ids {
                assert!(!seen.contains(id), "{id} is on the page twice");
                seen.push(id);
            }
        }
        for binding in BINDINGS {
            assert!(
                seen.contains(&binding.id),
                "{} is in the table and not on the page",
                binding.id
            );
        }
        assert_eq!(seen.len(), BINDINGS.len());

        let named = |title: &str| {
            lines
                .iter()
                .find(|line| line.title == title)
                .unwrap_or_else(|| panic!("{title} is a line of the page"))
        };
        let tabs = named(FAMILY_GOTO_TAB.text());
        assert_eq!(tabs.ids.len(), 9, "nine bindings, one line");
        assert_eq!(
            tabs.caps,
            vec!["Ctrl", "Shift", "1 – 9"],
            "the fold is derived from the members, not written beside them"
        );
        assert!(!tabs.recordable, "a family is edited a slot at a time");

        let pip = named(FAMILY_SUMMON_PIP.text());
        assert_eq!(pip.ids.len(), 4);
        assert!(
            pip.caps.is_empty(),
            "all four slots ship unassigned - see Action::SummonPip"
        );
        assert!(
            pip.note
                .as_deref()
                .is_some_and(|note| note.contains(NOTE_NONE_ASSIGNED.text())),
            "and the line says so: {:?}",
            pip.note
        );
        assert!(
            pip.note
                .as_deref()
                .is_some_and(|note| !note.contains(NOTE_MACHINE_PENDING.text())),
            "a row with no chord does not also claim to be bound: {:?}",
            pip.note
        );

        // **The alias is gone from the page** (user ruling 2026-08-18). One verb
        // is one line; the second chord is something a reader records.
        assert!(
            !lines.iter().any(|line| line.title.contains("second chord")),
            "the retired alias must not still be listed: {:?}",
            lines.iter().map(|line| line.title).collect::<Vec<_>>()
        );
        let search = named("Find in the terminal");
        assert_eq!(search.ids, vec!["open-search"]);
        assert_eq!(search.caps, vec!["Ctrl", "F"]);
        assert_eq!(
            search.note.as_deref(),
            Some(Text::ShortcutScopeTerminalPrimary.text()),
            "and it wears the scope tag its row carries"
        );

        // A stub row says its machine has not arrived, or a user presses it,
        // sees nothing, and reports a decision as a bug.
        let palette = named("Command palette");
        assert_eq!(palette.note.as_deref(), Some(NOTE_MACHINE_PENDING.text()));

        // A window row wears no tag at all: twenty rows saying "Anywhere" is a
        // column that says nothing twenty times.
        assert_eq!(named("New tab").note, None);

        // And the two the audit declined, at the end, greyed and unrecordable.
        let reserved: Vec<&ShortcutRow> = lines.iter().filter(|line| line.reserved).collect();
        assert_eq!(reserved.len(), RESERVED_ROWS.len());
        for line in &reserved {
            assert!(line.ids.is_empty(), "a declined row binds nothing");
            assert!(!line.recordable, "and cannot be recorded either");
            assert_eq!(line.note.as_deref(), Some(NOTE_RESERVED_ALT_ARROW.text()));
        }
        assert!(
            lines[lines.len() - reserved.len()..]
                .iter()
                .all(|line| line.reserved),
            "they stand after the rows this window actually claims"
        );
    }

    /// PIN — **the recorder's own four verbs, and the cost of having them.**
    ///
    /// Bare `Esc`, `Backspace`, `Delete` and `Enter` cannot be recorded bare,
    /// because a box that is listening for every key needs some key to mean
    /// "stop". Wearing any modifier they are ordinary keys again, which is the
    /// half that keeps the cost small: nobody cancels a dialog with
    /// `Ctrl+Shift+Esc`.
    #[test]
    fn the_recorder_keeps_four_bare_keys_for_itself_and_hands_them_back_modified() {
        assert_eq!(
            classify_recording(&Key::Named(NamedKey::Escape), ModifiersState::empty()),
            RecordedKey::Cancel
        );
        for named in [NamedKey::Backspace, NamedKey::Delete] {
            assert_eq!(
                classify_recording(&Key::Named(named), ModifiersState::empty()),
                RecordedKey::Unbind
            );
        }
        assert_eq!(
            classify_recording(&Key::Named(NamedKey::Enter), ModifiersState::empty()),
            RecordedKey::Confirm
        );
        assert_eq!(
            classify_recording(&Key::Named(NamedKey::Enter), CTRL_SHIFT),
            RecordedKey::Chord(Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::Enter))),
            "modified, it is an ordinary key again"
        );
        // A modifier on its own is what "shown live" is made of.
        for named in [NamedKey::Control, NamedKey::Shift, NamedKey::Alt] {
            assert_eq!(
                classify_recording(&Key::Named(named), ModifiersState::CONTROL),
                RecordedKey::Modifier
            );
        }
        assert_eq!(live_caps(CTRL_SHIFT), vec!["Ctrl", "Shift"]);
        // And a key with no name is refused rather than written down wrong.
        assert_eq!(
            classify_recording(&Key::Named(NamedKey::BrowserBack), ModifiersState::empty()),
            RecordedKey::Unusable
        );
    }

    /// Each table row must be reachable through the same lookup dispatch uses —
    /// **inside its own scope**, which is the only place a scoped row claims to
    /// be reachable at all.
    #[test]
    fn every_table_row_round_trips_through_lookup() {
        for binding in BINDINGS {
            let Some(chord) = &binding.chord else {
                continue;
            };
            let key = match &chord.key {
                ChordKey::Character(text) => Key::Character(text.as_ref().into()),
                ChordKey::Named(named) => Key::Named(*named),
            };
            let focus = match binding.scope {
                Scope::Window => Focus::default(),
                Scope::Preview => Focus {
                    preview: true,
                    terminal_primary: false,
                    search_open: false,
                },
                Scope::TerminalPrimary => Focus {
                    preview: false,
                    terminal_primary: true,
                    search_open: false,
                },
                Scope::SearchOpen => Focus {
                    preview: false,
                    terminal_primary: true,
                    search_open: true,
                },
            };
            assert_eq!(
                lookup_action(&key, &key, chord.modifiers, focus),
                Some(binding.action),
                "{:?} is in the table but unreachable",
                binding.action
            );
        }
    }
}
