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
//!
//! **The third refusal has a way out and the other two have none** (user ruling 2026-08-26). AltGr
//! and the shell's alphabet are facts about a keyboard; a chord another row claims is a fact about
//! this table, and a table is a thing the person in front of it is allowed to change. So the
//! recorder's refusal names the holder *and* offers to take the chord off it
//! ([`ChordVerdict::swap_offer`], [`Shortcuts::take_chord_from`]) — an offer only the recorder can
//! make, because only there is somebody still holding the keys to accept it. At the file's door the
//! same conflict is still a plain refusal.
//!
//! **A file is a set of sentences about the rows it names, not a sequence of edits.** A row the
//! file mentions holds nothing until its own line says so, which is why `apply_overrides` reads in
//! two passes — see it for the launch on which this build refused a file it had itself written a
//! minute earlier.

use std::borrow::Cow;
use std::fmt::Write as _;

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::i18n::{Lang, Text};

/// Everything the window can be asked to do from the keyboard.
///
/// `SummonPip` is the family with no machine behind it yet: four rows, listed, unassigned, and
/// dispatched to an explicit no-op rather than omitted, because the audit decided the *slots*
/// belong to us even though none of their chords does.
///
/// `JumpAttention` was a stub too until 2026-08-20, when the attention queue (P1-8) landed with
/// §7.1.6b′ F3 and gave it [`crate::Runtime::jump_to_attention`]. What the stub bought is exactly
/// what it was for: the chord was already the user's, already documented, and already unable to
/// leak, so the verb arriving changed one arm of one `match` and nothing else.
///
/// **`CommandPalette` was the third, and it left the table on 2026-08-28** — see the variant's
/// own note for the ruling that took its chord back off it for the preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    NewTab,
    /// **A second window on this application** (multiwindow slice C).
    ///
    /// Its own row and not a mode of [`Self::NewTab`], because the two differ in
    /// the one thing a shortcut row is about: what appears when you press it.
    NewWindow,
    /// **The whole application leaving** (multiwindow slice E2, user ruling
    /// 2026-08-20).
    ///
    /// Its own row and not "close every window", because the two differ in the
    /// one thing this table is about: what is true afterwards. Closing windows
    /// one at a time files each of them in Recent and leaves the file describing
    /// the last one; this writes every window into `session.json` as one document
    /// and puts **nothing** in the vault, so the next launch opens what you left.
    /// See [`crate::quit`].
    ///
    /// Scoped to the window like every other row here, and that is not a
    /// contradiction: the *chord* is answered by whichever window has the
    /// keyboard, and what it starts belongs to the process.
    Quit,
    ClosePane,
    NextTab,
    PrevTab,
    /// 1-based tab ordinal, always within `1..=9`; out-of-range targets are ignored at dispatch.
    GotoTab(u8),
    ReopenClosed,
    JumpAttention,
    /// **The verb the preview ships without, and the name it keeps** (user
    /// ruling 2026-08-28).
    ///
    /// It held `Ctrl+Shift+P` from the 2026-08-10 audit until now, on the stub
    /// row's argument: the key is ours, nothing else may claim it, nothing leaks
    /// to the shell in the meantime. That argument is about *this table's*
    /// neighbours, and it was answered honestly for as long as the only readers
    /// were the panel and the shell. The preview has a third reader — a person
    /// deciding whether this product works — and to them a row that draws itself
    /// beside `Bound, but the action behind it is not built yet` is a shipped
    /// feature that does nothing. So for v0.2 the palette gets its chord back;
    /// until then the table does not claim a key it cannot honour, and
    /// `Ctrl+Shift+P` reaches the shell like any other unclaimed chord.
    ///
    /// **The variant stays** rather than being deleted and re-added. The verb is
    /// scheduled, not withdrawn; `main`'s dispatch keeps the arm that answers it
    /// with nothing, so the day the palette lands it is one row of [`BINDINGS`]
    /// and one arm of one `match` — the same shape `JumpAttention` had when it
    /// arrived. Nothing constructs it while it is out of the table, which is
    /// what the attribute below is for and the whole of what it says.
    #[allow(
        dead_code,
        reason = "the v0.2 palette's name, held while its row is out of BINDINGS"
    )]
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
    /// **Put this pane on the stage alone, or put the tiling back** (§7.1.6l,
    /// user ruling 2026-08-25, B7).
    ///
    /// A toggle for [`Self::ToggleFocusMode`]'s reason, one surface down: the
    /// zoom is one field on one tab, the pane head draws which way it is set, and
    /// a separate "restore" key would be a second truth about the same field.
    ZoomPane,
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
    /// Put the search capsule away (§7.7, W2 slice ④).
    ///
    /// **A row of this table, and it did not have to be until a page could hold
    /// the keyboard.** Every other rung of §7.1.5's Escape ladder is raised by
    /// the pointer on this window's own chrome — a menu, a float, a drag — and
    /// pressing chrome takes the keyboard out of whatever pane it was in, so a
    /// page and one of those rungs cannot be up at the same time with the page
    /// still holding the keys. The capsule is the exception, and B81 is why:
    /// `Ctrl+F` raises it **without** taking the keyboard off the surface below
    /// it, which is the whole of its second stance. So it is the one rung that
    /// can stand over a page that still owns Escape — and a page owns every key
    /// this table does not claim.
    ///
    /// On a terminal nothing changes, and the ladder is why: the capsule's rung
    /// answers an Escape long before `Shortcuts::lookup` is asked, so this row
    /// is never reached there and `0x1b` still leaves for the shell the instant
    /// the capsule is gone.
    CloseSearch,
    /// Put the caret in the web seat's address field (§7.7 ②, user ruling
    /// 2026-08-22).
    ///
    /// The second door onto the field the page's title already is; the first is
    /// the double click that renames a file one content class over.
    WebAddress,
    /// **Open an address from anywhere in this window** (§7.7 ⑨, Claude 定
    /// 2026-08-24).
    ///
    /// [`Self::WebAddress`] one scope out, and the two are a pair rather than a
    /// duplicate: `Ctrl+L` is the *page's* own key and answers only where there
    /// is already a page to steer, and this one answers with the hands on a
    /// shell — where there may be no page at all, so the verb has to be able to
    /// make one. What it does is one sentence with a fork inside it: put the
    /// caret in this tab's address, minting the blank page that owns it first if
    /// the tab has not got one.
    ///
    /// **Not a second chord on [`Self::WebAddress`]'s row**, which is the shape
    /// the table already refused once (user ruling 2026-08-18, `open-search`'s
    /// retired alias: "one verb, one row"). It is also not what this is — a row
    /// in force everywhere and a row in force over a page do different things on
    /// a tab with no page, and a single row could only carry one of the two
    /// answers.
    ///
    /// **`Ctrl+Shift+L` and not `Ctrl+L`.** `^L` is readline's clear-screen and
    /// discipline (1) forbids taking it from a terminal; the shifted chord is
    /// this window's own namespace, and it was free.
    WindowAddress,
    /// Open the developer tools on the focused page (§7.7 ②, same ruling).
    WebDevTools,
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
    /// The capsule is closed the moment its host stops being able to hold one,
    /// so this scope implies [`Self::SearchHost`] rather than repeating it.
    SearchOpen,
    /// Only while the keyboard is on a surface the **search capsule** can open
    /// on: a terminal showing its primary screen, or a hosted page (§7.7 ②).
    ///
    /// Two hosts, one capsule, one row. `Ctrl+F` had [`Self::TerminalPrimary`]
    /// while the capsule had one host, and the second host is not a second
    /// instrument — the mock-up's own selector reads `.term, .pv-web-doc`
    /// (14038) and the ruling says「第二个 host,不是第二份实现」.
    ///
    /// **A page makes the scope load-bearing rather than tidy.** With `Ctrl+F`
    /// out of force over a page the chord is not claimed back through
    /// `AcceleratorKeyPressed`, so the engine's *own* find bar opens inside the
    /// seat — a second search box, in a window whose whole search story is that
    /// there is one. The row's scope is what shuts that door, and there is no
    /// other: `AreBrowserAcceleratorKeysEnabled` is not in this build's
    /// bindings, so a key the table does not take is a key the engine keeps.
    SearchHost,
    /// Only while a **hosted page** holds the keyboard (§7.7, W2 slice ④).
    ///
    /// Its own scope and not [`Self::Preview`], because a web seat is a preview
    /// seat and the two rows in it would be wrong on every other one: a markdown
    /// document has no address to put a caret in and no developer tools to open,
    /// and a chord that lands on nothing is a chord the shortcut page would have
    /// to describe with a condition it could not show.
    ///
    /// It is also what lets `Ctrl+L` exist at all. Discipline (1) — a bare
    /// `Ctrl+letter` is the shell's control-code alphabet — forbids taking `^L`,
    /// which is readline's clear-screen and is pressed all day. What the ruling
    /// that put `Ctrl+S` in [`Self::Preview`] settled is that the discipline
    /// forbids taking a control letter **from a terminal**, and there is no
    /// terminal in a page. Out of this scope the row is simply not in the table
    /// and `^L` reaches the child untouched.
    WebPage,
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
    /// Whether the focused leaf is a preview seat **with a page on it**.
    ///
    /// Never true without [`Self::preview`] — a web seat is a preview seat, and
    /// the two flags are a kind and a content class rather than two places.
    pub(crate) web_page: bool,
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
            Self::WebPage => Some(Text::ShortcutScopeWebPage),
            // The tag names the surface the capsule can open on, and the two
            // surfaces have no one word between them — so it names the thing
            // that is true of both: there is something here to search.
            Self::SearchHost => Some(Text::ShortcutScopeSearchHost),
        }
    }

    /// Whether a row in this scope is in force with the window focused like this.
    ///
    /// `pub(crate)` since the web preview: a page holding the focus takes the
    /// window's chords back through `AcceleratorKeyPressed` rather than through
    /// [`Shortcuts::lookup`], and a chord out of force must be left to the page
    /// — so the *same* predicate has to answer at both doors
    /// (`webhost::claimable_chords`).
    pub(crate) const fn holds(self, focus: Focus) -> bool {
        match self {
            Self::Window => true,
            Self::Preview => focus.preview,
            Self::TerminalPrimary => focus.terminal_primary,
            Self::SearchOpen => focus.search_open,
            Self::WebPage => focus.web_page,
            Self::SearchHost => focus.terminal_primary || focus.web_page,
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

    /// A row in force only where the search capsule has somewhere to open.
    const fn search_host(id: &'static str, title: Text, action: Action, chord: Chord) -> Self {
        Self {
            id,
            title,
            family: None,
            action,
            chord: Some(chord),
            scope: Scope::SearchHost,
        }
    }

    /// A row in force only while a hosted page holds the keyboard.
    const fn web_page(id: &'static str, title: Text, action: Action, chord: Chord) -> Self {
        Self {
            id,
            title,
            family: None,
            action,
            chord: Some(chord),
            scope: Scope::WebPage,
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
        self.note_in(crate::i18n::current())
    }

    /// The same line in a named language.
    ///
    /// The language is a parameter and not the process's current one so that
    /// `docs/shortcuts.md` can be written in both columns at once without a test
    /// reaching for the global every other test reads — the same reason
    /// [`Text::in_lang`] exists beside [`Text::text`].
    fn note_in(&self, lang: Lang) -> Option<Cow<'static, str>> {
        // **Only a row that has a chord can say it is bound.** The
        // picture-in-picture slots are pending *and* unassigned, and printing
        // "Bound; the verb behind it is still to come" over a row reading
        // `Not set` would be the panel contradicting itself across four inches
        // of one line — which is exactly what the real window showed.
        let pending = (self.action.is_pending() && self.chord.is_some())
            .then(|| NOTE_MACHINE_PENDING.in_lang(lang));
        match (self.scope.tag().map(|tag| tag.in_lang(lang)), pending) {
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
    ///
    /// **`JumpAttention` left this list on 2026-08-20** (§7.1.6b′ F3). It is the
    /// first row ever to do so, which is the whole point of the mechanism: a
    /// stub row is a promise with a date on it, and the note comes off the row
    /// the day the verb lands rather than being maintained as a second opinion
    /// about which verbs exist.
    ///
    /// **`CommandPalette` left it the other way on 2026-08-28** — not because
    /// the verb arrived but because the row did not, and a row that is not in
    /// [`BINDINGS`] has no key to be claimed *or* pending. The two exits are the
    /// same mechanism read in both directions: this predicate is about rows, so
    /// a name with no row is not one of its answers.
    const fn is_pending(self) -> bool {
        matches!(self, Self::SummonPip(_))
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
    // **`Ctrl+Shift+M`, ruled by the user 2026-08-19** — no longer a placeholder
    // (multiwindow slice C shipped it provisionally the same day).
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
    // **Measured a second time on 2026-08-25, and it still does not arrive.** The
    // `zoom-pane` row below was written on `Ctrl+Shift+Enter` that morning, over
    // this note, on the argument that one machine on one day is a record and not a
    // rule; the user pressed it on the real window and the pane did not zoom. Two
    // measurements six days apart make this a rule rather than a record, so the
    // zoom verb moved to `Ctrl+Shift+X` and **no row of this table is keyed on
    // `Enter` in any combination** — which `the_zoom_verb_has_a_chord_of_its_own`
    // now asserts over the whole table instead of leaving it to whoever writes the
    // next row.
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
    // **`Ctrl+Shift+Q`, ruled by the user 2026-08-20** — and the key was free,
    // which is the whole of why this row needed no argument: `Q` is claimed by
    // nothing in this table, and `^Q` — what a bare `Ctrl+Q` would take — is
    // XON/XOFF's resume and stays with the terminal, exactly as `^M` stays with
    // it one row above. It wears the modifier pair every window verb here wears
    // (discipline (1): bare `Ctrl+letter` belongs to the shell) and it is the
    // chord Windows Terminal, iTerm2 and VS Code all quit on.
    Binding::window(
        "quit",
        Text::ShortcutQuit,
        Action::Quit,
        Chord::new(CTRL_SHIFT, character("q")),
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
    // **`command-palette` stood here until 2026-08-28, and its chord is nobody's
    // now** (user ruling). See [`Action::CommandPalette`] for why the name is
    // kept while the row is not, and `Ctrl+Shift+P` is asserted unclaimed by
    // `every_ruled_binding_resolves_to_its_action` below and by `main`'s
    // `the_retired_preview_chord_reaches_the_shell_like_any_other_key`.
    //
    // Nothing takes the freed key. The chord is the palette's everywhere else on
    // this platform, and a row that moved into it for one release would have to
    // move back out of somebody's fingers in the next.
    //
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
    // **`Ctrl+Shift+X`, ruled by the user 2026-08-25** — the keyboard door
    // §7.1.6l shipped without. The verb had two pointer doors (a double-click on
    // the pane head, the `⌄` menu's row) and no key, which made it the one pane
    // verb in this build a hand on the keyboard could not reach.
    //
    // **It was `Ctrl+Shift+Enter` for half a day, and the machine took it back.**
    // That chord is what Windows Terminal spends on maximising a pane, so the row
    // was written on it over the `new-window` note above on the argument that one
    // measurement on one machine is not a rule. The user pressed it on the real
    // window the same day and nothing happened — the second measurement of the
    // same finding — so the ruling moved the verb to a key that arrives. The note
    // above is now a rule rather than a record, and the test below holds the whole
    // table to it.
    //
    // **`X` and not `C` or `V`.** It is free in this table, it is outside the
    // `Ctrl+Alt` AltGr zone this table's header rules out, and `^X` — what a bare
    // `Ctrl+X` would take — stays with the terminal, where readline spends it on
    // its own prefix. The two letters beside it are not free in the same sense:
    // `Ctrl+Shift+C`/`V` are copy and paste to every hand that has used a terminal,
    // and a window layout answering to either is a window people stop trusting the
    // first time they press it.
    //
    // Titled with the menu row's action face on `focus-mode`'s precedent: the
    // chord and the row turn one field, so they are one name.
    Binding::window(
        "zoom-pane",
        Text::PaneMenuZoom,
        Action::ZoomPane,
        Chord::new(CTRL_SHIFT, character("x")),
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
    //
    // **The scope widened on 2026-08-22 and the argument did not** (§7.7 ②).
    // The row moved from `Scope::TerminalPrimary` to [`Scope::SearchHost`] the
    // day the capsule got its second host, and the three reasons above hold
    // word for word on the new one: `^F` has no owner in a page either, the
    // reference product takes `Ctrl+F` for exactly this box, and out of the
    // scope the row is still not in the table at all. What the widening buys is
    // not convenience — it is the *only* way to keep the engine's own find bar
    // from opening inside the seat, because a key this table does not claim is
    // a key `AcceleratorKeyPressed` hands to the page.
    Binding::search_host(
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
    // **Bare `Escape`, and the one row in this table that exists because of
    // something *below* the keyboard** (§7.7, W2 slice ④).
    //
    // Every rung of §7.1.5's Escape ladder is code in `keyboard_input`, and
    // none of them is a row here — an Escape unwinds one layer per press and
    // then reaches the shell as `0x1b`, which is not a verb a shortcut panel
    // could offer a second chord for. That stayed true until a **page** could
    // hold the keyboard: a page keeps every key this table does not claim, so
    // the ladder is simply not consulted, and the one rung that can be standing
    // over a page whose keyboard is still the page's is the capsule (B81 —
    // `Ctrl+F` raises it *without* taking the keyboard off the surface below).
    //
    // So the ruling is: **Escape belongs to the page, and the window takes it
    // back only where a row of its own table says so.** This is that row and
    // there is exactly one. On a terminal nothing changes at all — `close_search`
    // answers at the capsule's rung long before `lookup` is asked — and with no
    // capsule up the row is not in the table, which is what keeps `0x1b`
    // reaching every shell.
    //
    // Rejected, and written down so the argument is not re-run:
    // ① a `Scope::Window` row — `lookup` sits above the PTY encoder, so it
    //    would swallow the one byte a terminal cannot do without;
    // ② two presses in a row meaning the window — a gesture nobody was told
    //    about, and indistinguishable from "the page ate the first one";
    // ③ `Shift+Esc` for the page — it inverts what page code listens for, and
    //    on this very engine `Shift+Esc` is the browser's task manager.
    //
    // **Not a decorative row** — the 2026-08-26 gesture audit read it as one
    // (「它在设置页里列着,是装饰行」) on the true observation that `lookup`
    // never returns it: the ladder answers first on every host, page included.
    // What that reading misses is that this row's work is done *before* the
    // press is a press. `webhost::claimable_chords` reads this same table to
    // tell the engine which keys to hand back, so with the row gone a focused
    // page keeps `Escape` and the capsule over it has no way to close — see
    // `webhost::tests::the_page_gives_the_search_chord_back_to_the_window`,
    // which asserts exactly that and goes red if the row is withdrawn. The row
    // stays, and it stays on the Shortcuts page, because it is a real chord a
    // reader can rebind and the effect they will see is real.
    Binding::search_open(
        "close-search",
        Text::ShortcutCloseSearch,
        Action::CloseSearch,
        Chord::new(ModifiersState::empty(), ChordKey::Named(NamedKey::Escape)),
    ),
    // **`Ctrl+L` and `F12`** (user ruling 2026-08-22, W1's open question 3).
    //
    // Both are browser conventions and both are doors that do not otherwise
    // exist: the address field has one gesture, a double click, and the
    // developer tools live on a hover tool that is invisible until the pointer
    // arrives. That is what separates them from `Alt+Left` and `Alt+Right`,
    // which this slice deliberately leaves with the engine — those are not a
    // door being added, they are a working door being repossessed, and the
    // engine's answer for them is already this pane's own verb.
    //
    // `Ctrl+L` is a bare `Ctrl+letter`, which discipline (1) forbids taking
    // from a terminal — `^L` is readline's clear-screen. It is taken here on
    // ruling 9's shape exactly: scoped to a surface where there is no terminal
    // to take it from. See [`Scope::WebPage`].
    Binding::web_page(
        "web-address",
        Text::ShortcutWebAddress,
        Action::WebAddress,
        Chord::new(CTRL, character("l")),
    ),
    // **`Ctrl+Shift+L`** (§7.7 ⑨, Claude 定 2026-08-24) — the row above with the
    // scope taken off, and therefore a different verb. See
    // [`Action::WindowAddress`]: the page's own key can assume a page, and a key
    // that answers on a shell cannot, so this one mints the page it needs.
    //
    // The row directly under `web-address` because the two are read as a pair
    // and the editor lists the table in the table's own order. The shifted chord
    // is what a bare `Ctrl+letter` in this window has always had to become the
    // moment it is in force where a terminal is.
    Binding::window(
        "window-address",
        Text::ShortcutWindowAddress,
        Action::WindowAddress,
        Chord::new(CTRL_SHIFT, character("l")),
    ),
    Binding::web_page(
        "web-devtools",
        Text::ShortcutWebDevTools,
        Action::WebDevTools,
        Chord::new(ModifiersState::empty(), ChordKey::Named(NamedKey::F12)),
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
    /// The effective rows, in the table's own order.
    ///
    /// Read by `webhost::claimable_chords`, which needs the *effective* table
    /// and not [`BINDINGS`]: a page that kept handing the window back the
    /// factory chords after somebody rebound one would be a shortcut table with
    /// two answers.
    #[must_use]
    pub(crate) fn rows(&self) -> &[Binding] {
        &self.rows
    }

    /// **The chord a menu prints beside a row that is this verb** (gesture audit
    /// 2026-08-26, 系统性发现 ②).
    ///
    /// The audit's finding was that this window has two doors onto a dozen
    /// verbs and neither door mentions the other: the hint card
    /// ([`crate::keyhint`]) is the keyboard's side and there was nothing at all
    /// on the menus' side, so `Close pane` and `Ctrl+Shift+W`, `Zoom pane` and
    /// `Ctrl+Shift+X`, `Find…` and `Ctrl+F` were each introduced to the reader
    /// twice, as strangers. 「加一列 accel 是把十几条乙升级成『互相教』的最便宜
    /// 的一次改动」.
    ///
    /// **The effective table and not [`BINDINGS`]**, on `claimable_chords`'
    /// reasoning exactly: a menu that went on printing the factory chord after
    /// somebody rebound one would be a shortcut table with two answers. A row
    /// the reader has unbound has no chord, and the menu prints nothing rather
    /// than the word for nothing — a menu is not the shortcuts page and has no
    /// business reporting an absence nobody asked about.
    ///
    /// Joined with `+` rather than drawn as key caps: a menu row's trailing
    /// annotation is small text on this platform and in every editor on it, and
    /// caps in the row would be a second control-shaped thing beside a control
    /// column that is already there.
    #[must_use]
    pub(crate) fn accelerator(&self, action: Action) -> Option<String> {
        self.rows
            .iter()
            .find(|row| row.action == action)
            .and_then(|row| row.chord.as_ref())
            .map(|chord| chord_caps(chord).join("+"))
    }

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
    ///
    /// # A row the file names has no chord but the one its own line gives it
    ///
    /// **The reading is in two passes, and it has to be** (found 2026-08-26 by
    /// the swap the recorder now offers: a file this build had just written was
    /// refused by this build on the next launch). Read line by line into a table
    /// still holding the factory chords, a file is judged against a state that
    /// never existed — `new-tab: Ctrl+Shift+W` is refused because `close-pane`
    /// still has that chord two lines further down, where the file is about to
    /// take it away. Order would decide the answer, and JSON's order is the
    /// order a person typed in.
    ///
    /// So the file is read as what it is: a set of sentences about the rows it
    /// names, in which **a row it names holds nothing until its own line says
    /// so**. Every named row is cleared first, then the chords are dealt out.
    /// Every table that is legal at all can be written down this way — including
    /// a rotation, which no line-at-a-time reading can accept — and the files
    /// that are still refused are exactly the ones describing a table two rows
    /// of which answer to one press.
    ///
    /// A line refused *here* leaves its row at the default after all, offered
    /// through the same judge: the promise above is kept where it can be, and a
    /// default some other line of the same file has since claimed leaves the row
    /// with no chord rather than with a second claim on one.
    pub(crate) fn apply_overrides(&mut self, overrides: &[Override]) -> Vec<OverrideFault> {
        let mut faults = Vec::new();
        // Pass one: the refusals a row can earn on its own, with no reference to
        // any other row — an id nobody answers to, text that is not a chord, and
        // the two disciplines that are about the keyboard rather than about the
        // table. A line refused here never reaches the clear below, which is
        // what leaves its row untouched at the default.
        let mut named: Vec<(usize, Option<Chord>)> = Vec::with_capacity(overrides.len());
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
                    // Only the two keyboard disciplines can answer here: a
                    // conflict is a fact about the table the file describes, and
                    // that table does not exist yet.
                    match chord_discipline(&chord) {
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
            named.push((index, chord));
        }
        for (index, _) in &named {
            self.rows[*index].chord = None;
        }
        // Pass two: deal the chords out. The judge now sees the table the file
        // is building, so what it refuses is one press with two answers in it
        // and never an artefact of which line came first.
        let mut refused_rows: Vec<usize> = Vec::new();
        for (index, chord) in named {
            let Some(chord) = chord else {
                continue;
            };
            match chord_verdict(&self.rows, self.rows[index].id, &chord) {
                ChordVerdict::Free => self.rows[index].chord = Some(chord),
                refused => {
                    faults.push(OverrideFault {
                        id: self.rows[index].id.to_owned(),
                        reason: refused.hint().into_owned(),
                    });
                    refused_rows.push(index);
                }
            }
        }
        for index in refused_rows {
            let Some(default) = BINDINGS[index].chord.clone() else {
                continue;
            };
            if chord_verdict(&self.rows, self.rows[index].id, &default) == ChordVerdict::Free {
                self.rows[index].chord = Some(default);
            }
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

    /// **Move a chord from the row that has it to the row that wants it** (user
    /// ruling 2026-08-26).
    ///
    /// One call and not `set(holder, None)` followed by `set(id, chord)` at the
    /// call site, because the two writes are one edit: a table seen between them
    /// has the chord on neither row, and the one caller that would have written
    /// them apart is the one that also writes the file. Stated here, the file
    /// cannot be written from between them.
    ///
    /// The chord it moves is the one `holder` is actually holding — the caller
    /// hands the chord it recorded, and this asserts nothing about the two being
    /// equal because they always are: `holder` came out of
    /// [`ChordVerdict::AlreadyUsed`], which is how the recorder learned there was
    /// a chord to take.
    pub(crate) fn take_chord_from(&mut self, holder: &str, id: &str, chord: Chord) {
        self.set(holder, None);
        self.set(id, Some(chord));
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

    /// **The lines a hint card shows to a hand holding `modifiers`** (§7.1.5e′).
    ///
    /// The effective table and not [`BINDINGS`], on `webhost::claimable_chords`'s
    /// own reasoning: a card that went on showing the factory chord after
    /// somebody rebound one would be a shortcut table with two answers.
    ///
    /// **The modifiers must match exactly**, and that is a ruling rather than an
    /// optimisation (Claude 定 2026-08-25). The card answers "what does the hand
    /// I am holding do", and a hand holding `Ctrl` cannot press `Ctrl+Shift+N`
    /// without pressing `Shift` first — at which point the card re-answers with
    /// no second wait ([`crate::keyhint::KeyHintHost::observe`] ③). So the rows
    /// one modifier further out are never lost, they arrive the instant that
    /// modifier does; listing them under a bare `Ctrl` would bury the four rows
    /// that hold is actually for under twenty-one that it is not, which is the
    /// wall the whole 800ms wait exists to avoid.
    ///
    /// **A row out of scope is not in the list**, asked with the same
    /// [`Scope::holds`] `lookup` asks — because it is the same question: a row
    /// out of scope is not in the table for this press, so listing it would be
    /// the card promising a verb the very next line of `lookup` will decline.
    ///
    /// **A stub row is not in the list either.** §7.1.5e's 「存根行是真实的行」
    /// is about the chord being *claimed* — nothing else may take it and nothing
    /// leaks to the shell — and the shortcut page says so in as many words on a
    /// line with room for a note. This card has no such room and is not making
    /// that claim: it lists what pressing a key would *do*, and a row dispatched
    /// to an explicit no-op does nothing.
    ///
    /// **A family folds to one line**, derived from `family` exactly as
    /// [`Self::editor_rows`] derives it, and for that method's own reason: nine
    /// lines of one verb would bury the fifteen other verbs under them. The fold
    /// is taken **only when every member of the family is in the list and the
    /// members are a run** — a family half of whose rows have been rebound
    /// elsewhere is listed member by member, because `1 – 9` over eight bound
    /// digits is a line claiming a key nobody can press.
    #[must_use]
    pub(crate) fn hint_lines(&self, modifiers: ModifiersState, focus: Focus) -> Vec<HintLine> {
        if modifiers.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<HintLine> = Vec::new();
        let mut index = 0;
        while index < self.rows.len() {
            let head = &self.rows[index];
            let end = match head.family {
                None => index + 1,
                Some(family) => self.rows[index..]
                    .iter()
                    .position(|row| row.family != Some(family))
                    .map_or(self.rows.len(), |offset| index + offset),
            };
            let members = &self.rows[index..end];
            index = end;
            let listed: Vec<&Binding> = members
                .iter()
                .filter(|row| row.answers_a_hand_holding(modifiers, focus))
                .collect();
            if listed.is_empty() {
                continue;
            }
            match (head.family, folded_key_cap(&listed)) {
                (Some(family), Some(cap)) if listed.len() == members.len() => {
                    out.push(HintLine {
                        key: cap,
                        title: family.text(),
                    });
                }
                _ => out.extend(listed.iter().map(|row| HintLine {
                    key: row_key_cap(row),
                    title: row.title.text(),
                })),
            }
        }
        out
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

/// One line of a hint card: the key to press, and the name of what it does.
///
/// The modifiers are deliberately **not** here — the card's head says them once,
/// and repeating `Ctrl + Shift` down sixteen lines would be sixteen copies of
/// the one thing the reader's own fingers are already telling them.
///
/// A display type and not a borrow of the table, for [`ShortcutRow`]'s reason: the
/// geometry gets strings and the chord grammar stays in the one module that owns
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HintLine {
    /// The key cap, with the held modifiers left off.
    pub(crate) key: String,
    pub(crate) title: &'static str,
}

impl Binding {
    /// Whether this row would answer a press by a hand holding exactly
    /// `modifiers`, with the window's keyboard where `focus` says it is.
    ///
    /// The same three questions [`Shortcuts::lookup`] asks, minus the key itself
    /// — which is the whole of what a hint is: everything about the press except
    /// which key finishes it. Stated once here so the card and the dispatch
    /// cannot drift apart.
    fn answers_a_hand_holding(&self, modifiers: ModifiersState, focus: Focus) -> bool {
        self.scope.holds(focus)
            && !self.action.is_pending()
            && self
                .chord
                .as_ref()
                .is_some_and(|chord| chord.modifiers == modifiers)
    }
}

/// One row's key, as the cap a hint card prints.
fn row_key_cap(row: &Binding) -> String {
    row.chord
        .as_ref()
        .map(|chord| key_label(&chord.key))
        .unwrap_or_default()
}

/// A run of rows that share a family, as **one** cap reading `first – last`, or
/// `None` when the run does not collapse.
///
/// [`fold_caps`]'s own predicate, asked of the key alone: the members have to
/// share their modifiers and each reach a single printed character, or a range
/// between the ends would be a line claiming the keys in the gaps.
fn folded_key_cap(members: &[&Binding]) -> Option<String> {
    let first = members.first()?.chord.as_ref()?;
    let last = members.last()?.chord.as_ref()?;
    if members.len() < 2 {
        return None;
    }
    let runs = members.iter().all(|row| {
        row.chord.as_ref().is_some_and(|chord| {
            chord.modifiers == first.modifiers
                && matches!(&chord.key, ChordKey::Character(text) if text.chars().count() == 1)
        })
    });
    runs.then(|| format!("{} – {}", key_label(&first.key), key_label(&last.key)))
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
    ///
    /// **It carries the holder's id as well as its name**, and the two are not
    /// the same fact: the name is what the sentence says and the id is what the
    /// swap acts on ([`Shortcuts::take_chord_from`]). A refusal that carried only
    /// the title would leave the one caller that can *do* something about it
    /// searching the table by a translated string.
    AlreadyUsed { holder: &'static str, title: Text },
}

impl ChordVerdict {
    /// The sentence the recorder shows, and the reason a refused file line
    /// carries.
    ///
    /// **The file's door and the recorder read the same sentence**, and the
    /// recorder adds one of its own on top ([`Self::swap_offer`]) rather than
    /// having a second copy of this one: a hand-edited line cannot be offered a
    /// swap — there is nobody holding the keyboard to accept it — so the offer
    /// is the part that differs and it is the only part that is written twice.
    #[must_use]
    pub(crate) fn hint(&self) -> Cow<'static, str> {
        match self {
            Self::Free => Cow::Borrowed(""),
            Self::AltGrZone => Cow::Borrowed(hint_altgr_zone()),
            Self::ShellControlLetter => Cow::Borrowed(hint_shell_control_letter()),
            Self::AlreadyUsed { title, .. } => {
                Cow::Owned(crate::i18n::shortcut_already_used(title.text()))
            }
        }
    }

    /// The id of the row that already answers to this chord, when one does.
    #[must_use]
    pub(crate) const fn holder(&self) -> Option<&'static str> {
        match self {
            Self::AlreadyUsed { holder, .. } => Some(holder),
            _ => None,
        }
    }

    /// **The whole sentence the recorder shows on a conflict**: who has the
    /// chord, and what the key still in the user's hand would do about it.
    ///
    /// One clause and not [`Self::hint`] with an offer joined onto it, and the
    /// real window is what settled that (2026-08-26, first smoke run): the
    /// recorder writes into the place the scope tag was, that place is **one
    /// line of the narrowest column in the dialog**, and
    /// `Already used by Close pane · Enter takes it from that row` came back off
    /// the glass as `Already used by Close pane · Enter take…` — cut in exactly
    /// the half that says what to press. A sentence that names the holder *as*
    /// the object of the verb says both facts in thirty characters and needs no
    /// second clause to lose.
    #[must_use]
    pub(crate) fn swap_offer(&self) -> Option<String> {
        let Self::AlreadyUsed { title, .. } = self else {
            return None;
        };
        Some(crate::i18n::shortcut_take_it_from(title.text()))
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
/// **The third refusal now carries an offer** (user ruling 2026-08-26, and it
/// overturns this comment's own first answer). What stood here was "no swap is
/// offered", on the grounds that a "use it here and take it away from there"
/// would leave a second row silently unbound behind a dialog the user has
/// already looked away from. The objection was to the *silence*, and the silence
/// is what changed: the refusal names the row that has the chord **and says, in
/// the same line, that `Enter` will take it from that row** — so the cost is
/// printed before it is paid, by a user who is still holding the keys. What it
/// buys is the case the old answer had no answer for: rebinding onto a chord you
/// already use is the ordinary reason to open this page, and "go and clear the
/// other row yourself, then come back" is four more steps to do the thing the
/// user just asked for.
///
/// The verdict is still a refusal and never a silent success — [`Shortcuts::set`]
/// is not reached until the other row has been cleared, and clearing it is
/// [`Shortcuts::take_chord_from`], which is one call and writes both rows.
#[must_use]
pub(crate) fn chord_verdict(rows: &[Binding], id: &str, chord: &Chord) -> ChordVerdict {
    match chord_discipline(chord) {
        ChordVerdict::Free => {}
        refused => return refused,
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
        .map_or(ChordVerdict::Free, |row| ChordVerdict::AlreadyUsed {
            holder: row.id,
            title: row.title,
        })
}

/// **The two refusals that are about the keyboard and not about the table** —
/// the audit's own disciplines, asked of a chord with nothing else in the room.
///
/// Lifted out of [`chord_verdict`] the day [`Shortcuts::apply_overrides`] needed
/// to ask them *before* it had a table to ask the third one of. They are the two
/// that can be answered that early precisely because they are not about any
/// other row: `Ctrl+Alt` is what a German keyboard sends for `@`, and
/// `Ctrl+letter` is the shell's control-code alphabet, and neither fact changes
/// with what the rest of the table happens to hold.
#[must_use]
fn chord_discipline(chord: &Chord) -> ChordVerdict {
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
    ChordVerdict::Free
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
/// The keyboard is on a preview, on a page inside one, on a terminal's
/// scrollback, on either of those two with the capsule up, or on none of them (a
/// terminal running a full-screen program, a files column, a menu).
///
/// **A preview with a search open used to be impossible and is not any more**
/// (§7.7, W2 slice ④). It was listed as impossible here in as many words while
/// the capsule had one host; the capsule now has two, and the second is a page.
/// A *document* preview still cannot have one — `web_page` is what tells the two
/// apart — so the state that was added is the page's and not the whole kind's.
const REACHABLE_FOCUS: [Focus; 6] = [
    Focus {
        preview: false,
        terminal_primary: false,
        search_open: false,
        web_page: false,
    },
    Focus {
        preview: true,
        terminal_primary: false,
        search_open: false,
        web_page: false,
    },
    Focus {
        preview: true,
        terminal_primary: false,
        search_open: false,
        web_page: true,
    },
    Focus {
        preview: true,
        terminal_primary: false,
        search_open: true,
        web_page: true,
    },
    Focus {
        preview: false,
        terminal_primary: true,
        search_open: false,
        web_page: false,
    },
    Focus {
        preview: false,
        terminal_primary: true,
        search_open: true,
        web_page: false,
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

/// Whether this press is a modifier and nothing else.
///
/// The one question the hint card asks of a key (§7.1.5e′): every *other* press
/// spends the hold, because a hand that has pressed something is no longer a
/// hand that has stopped. It reads [`is_modifier_key`] rather than a list of its
/// own — the recorder and the card have to agree about what a modifier is, or a
/// key one of them thinks is a chord and the other thinks is `Shift` would take
/// a card down while the box beside it went on waiting.
#[must_use]
pub(crate) fn is_a_bare_modifier(key: &Key) -> bool {
    matches!(key, Key::Named(named) if is_modifier_key(*named))
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
                web_page: false,
            },
        )
    }

    /// The same press with a **page** holding the keyboard inside a preview
    /// seat, and the capsule down.
    fn press_on_a_page(key: Key, modifiers: ModifiersState) -> Option<Action> {
        Shortcuts::defaults().lookup(
            &key,
            &key,
            modifiers,
            Focus {
                preview: true,
                terminal_primary: false,
                search_open: false,
                web_page: true,
            },
        )
    }

    /// The same press on a page with the capsule up over it.
    fn press_on_a_page_with_search_open(key: Key, modifiers: ModifiersState) -> Option<Action> {
        Shortcuts::defaults().lookup(
            &key,
            &key,
            modifiers,
            Focus {
                preview: true,
                terminal_primary: false,
                search_open: true,
                web_page: true,
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
                web_page: false,
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
                web_page: false,
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
        // **And the chord that used to be here is nobody's** (user ruling
        // 2026-08-28). The palette's row is out of the table until the verb
        // behind it ships, so `Ctrl+Shift+P` reaches the shell like any other
        // unclaimed chord — see [`Action::CommandPalette`].
        assert_eq!(
            press(character("p"), CTRL_SHIFT),
            None,
            "the palette's row left the table with its verb unbuilt, and took \
             its chord with it"
        );
        // **Focus mode's one chord** (§7.1.6b′ ②), and since the 2026-08-19
        // ruling withdrew the pane-header double-click and the pane menu's row,
        // one of only two ways in — the other is the `Appearance` row that
        // carries this same name. The row below is half the assertion: the chord
        // this one deliberately did **not** take is still Find's.
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

    /// PIN (§7.7 ②, user ruling 2026-08-22) — **the address field and the
    /// developer tools answer over a page and nowhere else.**
    ///
    /// `Ctrl+L` is the whole reason [`Scope::WebPage`] exists. `^L` is
    /// readline's clear-screen, discipline (1) forbids taking it from a
    /// terminal, and the ruling that let `Ctrl+S` into a preview is what lets
    /// this in: there is no terminal in a page to take it from. Out of the scope
    /// the row is not in the table and the byte leaves for the child.
    ///
    /// MUTATIONS:
    /// ① give either row `Scope::Window` — the terminal assertions go red, and
    ///    `bare_control_letters_stay_with_the_terminal` goes red with `Ctrl+L`;
    /// ② give them `Scope::Preview` — the last assertion goes red, which is a
    ///    markdown document being offered an address bar it has not got.
    #[test]
    fn the_address_and_the_developer_tools_answer_only_over_a_page() {
        assert_eq!(
            press_on_a_page(character("l"), CTRL),
            Some(Action::WebAddress)
        );
        assert_eq!(
            press_on_a_page(Key::Named(NamedKey::F12), ModifiersState::empty()),
            Some(Action::WebDevTools)
        );
        assert_eq!(
            press(character("l"), CTRL),
            None,
            "^L is readline's clear-screen and reaches the child untouched"
        );
        assert_eq!(
            press_on_primary_screen(character("l"), CTRL),
            None,
            "a scrollback is still a terminal"
        );
        assert_eq!(
            press(Key::Named(NamedKey::F12), ModifiersState::empty()),
            None,
            "a full-screen program keeps its function keys"
        );
        assert_eq!(
            press_in_preview(character("l"), CTRL),
            None,
            "a document has no address"
        );
        assert_eq!(
            press_in_preview(Key::Named(NamedKey::F12), ModifiersState::empty()),
            None,
            "and no developer tools"
        );
    }

    /// PIN (§7.7 ⑨, Claude 定 2026-08-24) — **`Ctrl+Shift+L` opens an address
    /// from anywhere, and takes nothing from the shell to do it.**
    ///
    /// The whole of the ruling in one test. The window's own address door is in
    /// force in every focus state, because the tab it acts on exists in every
    /// focus state — with the hands on a shell, in a document, over a page. And
    /// the row directly above it in the table is untouched: `^L` is readline's
    /// clear-screen and still reaches the child, which is the pin
    /// `the_address_and_the_developer_tools_answer_only_over_a_page` states from
    /// the other side and the reason this verb had to be a *second* row.
    ///
    /// MUTATIONS:
    /// ① give the row `Scope::WebPage` — the terminal and document assertions go
    ///    red, and the verb becomes a duplicate of the one above it that cannot
    ///    reach the tab it was added for;
    /// ② move it to bare `Ctrl+L` — `conflicts_with` sees the two rows in one
    ///    focus and `the_table_holds_exactly_the_ruled_rows` goes red, and so
    ///    does `bare_control_letters_stay_with_the_terminal`;
    /// ③ replace the row with a second chord on `web-address` — "one verb, one
    ///    row" goes red, and a tab with no page would have nothing to answer.
    #[test]
    fn the_windows_own_address_door_answers_everywhere_and_leaves_control_l_alone() {
        for (where_, action) in [
            ("on a shell", press(character("l"), CTRL_SHIFT)),
            (
                "on a scrollback",
                press_on_primary_screen(character("l"), CTRL_SHIFT),
            ),
            (
                "in a document",
                press_in_preview(character("l"), CTRL_SHIFT),
            ),
            ("over a page", press_on_a_page(character("l"), CTRL_SHIFT)),
        ] {
            assert_eq!(
                action,
                Some(Action::WindowAddress),
                "the window's address door is the window's {where_}"
            );
        }
        // And the row it was added beside is exactly where it was.
        assert_eq!(
            press_on_a_page(character("l"), CTRL),
            Some(Action::WebAddress)
        );
        assert_eq!(
            press(character("l"), CTRL),
            None,
            "^L is readline's clear-screen and reaches the child untouched"
        );
        assert_eq!(
            press_on_primary_screen(character("l"), CTRL),
            None,
            "a scrollback is still a terminal"
        );
    }

    /// PIN (§7.7, W2 slice ④) — **`Escape` puts the capsule away, and is the
    /// shell's the moment there is no capsule.**
    ///
    /// The row exists because a page keeps every key this table does not claim,
    /// and the capsule is the one rung of §7.1.5's ladder that can stand over a
    /// page whose keyboard is still the page's (B81's second stance). What must
    /// not change is the terminal: the ladder answers an Escape at the capsule's
    /// rung long before `lookup` is asked, so this row is never reached there —
    /// and with no capsule up it is not in the table at all, which is what keeps
    /// `0x1b` reaching every shell.
    ///
    /// MUTATIONS:
    /// ① give the row `Scope::Window` — the last two assertions go red, and
    ///    every `vim` in this window loses the one key it cannot do without;
    /// ② give it `Scope::WebPage` — the terminal-with-a-capsule assertion goes
    ///    red, and the table would carry a key that means two things depending
    ///    on which pane raised the capsule.
    #[test]
    fn escape_closes_the_capsule_and_belongs_to_the_shell_otherwise() {
        let escape = Key::Named(NamedKey::Escape);
        assert_eq!(
            press_on_a_page_with_search_open(escape.clone(), ModifiersState::empty()),
            Some(Action::CloseSearch)
        );
        assert_eq!(
            press_with_search_open(escape.clone(), ModifiersState::empty()),
            Some(Action::CloseSearch),
            "one row, both hosts — the ladder simply gets there first on a terminal"
        );
        assert_eq!(
            press_on_a_page(escape.clone(), ModifiersState::empty()),
            None,
            "with no capsule up the page keeps Escape, exactly as a shell does"
        );
        assert_eq!(
            press(escape.clone(), ModifiersState::empty()),
            None,
            "0x1b is the one byte a terminal cannot be asked to do without"
        );
        assert_eq!(
            press_on_primary_screen(escape, ModifiersState::empty()),
            None
        );
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
            web_page: false,
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
        // **Three more on 2026-08-22** (§7.7, W2 slice ④): `Ctrl+L` and `F12`,
        // ruled in by the user, and bare `Escape` — see [`Action::CloseSearch`]
        // for why a page turns a rung of the ladder into a row of the table.
        // **One more on 2026-08-23** (multiwindow slice E2): `quit` on
        // `Ctrl+Shift+Q`, which makes it 22 single actions.
        // **One more on 2026-08-24** (§7.7 ⑨): `window-address` on
        // `Ctrl+Shift+L` — 23 single actions, and the second row in the table
        // that puts a caret in an address. See [`Action::WindowAddress`] for why
        // it is a row rather than a second chord on the first one.
        // **One more on 2026-08-25** (B7): `zoom-pane` on `Ctrl+Shift+X` — 24
        // single actions. It was written on `Ctrl+Shift+Enter` first and moved the
        // same day, because a modified `Enter` does not reach this application;
        // the table is keyed on `Enter` nowhere, and that is now asserted.
        // **One fewer on 2026-08-28** (user ruling): `command-palette` is out,
        // which puts the single actions back to 23 and the table at 39. The
        // name survives in [`Action`] and the row does not — see that variant.
        assert_eq!(BINDINGS.len(), 39);
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

    /// PIN (user ruling 2026-08-25, B7 and its correction the same day) — **the
    /// pane zoom has a chord, and it is `Ctrl+Shift+X`.**
    ///
    /// §7.1.6l shipped the verb with two pointer doors and no keyboard one: a
    /// double-click on the pane head and the `⌄` menu's row. The ruling gives it
    /// the third, and the row is titled with the menu row's own action face on
    /// `focus-mode`'s precedent — the chord and the row turn one bit, so they are
    /// one name. The state face (`Restore pane`) is not this table's business:
    /// a shortcut row says what a key *does*, and this key does the same thing
    /// both ways round, which is what makes it a toggle.
    ///
    /// **The chord was `Ctrl+Shift+Enter` for half a day, and the machine had the
    /// last word.** The `new-window` row's 2026-08-19 note had already measured a
    /// modified `Enter` never arriving; this row was written over that note on the
    /// argument that one measurement on one machine is not a rule. It was measured
    /// again on 2026-08-25 on the real window and it did not arrive then either,
    /// so the ruling moved the verb to a key that does. **The half this test owns
    /// is both directions**: the row answers `X`, and no row of the table is keyed
    /// on `Enter` in any combination at all — a chord this application cannot be
    /// reached by is a row that silently does nothing, and the table may not carry
    /// one.
    ///
    /// Red gate: leave the row out and the lookup falls through to the PTY
    /// encoder, where `Ctrl+Shift+X` is a `^X` the shell already had.
    #[test]
    fn the_zoom_verb_has_a_chord_of_its_own() {
        let row = BINDINGS
            .iter()
            .find(|binding| binding.id == "zoom-pane")
            .expect("the zoom row is a row of the table");
        assert_eq!(row.action, Action::ZoomPane);
        assert_eq!(
            row.chord,
            Some(Chord::new(
                CTRL_SHIFT,
                ChordKey::Character(Cow::Borrowed("x"))
            )),
            "the chord the ruling names"
        );
        assert!(
            press_on_primary_screen(character("x"), CTRL_SHIFT).is_some(),
            "and a press on a terminal is answered by the table, not the encoder"
        );
        assert_eq!(
            press_on_primary_screen(character("x"), ModifiersState::empty()),
            None,
            "a bare x is still the shell's"
        );
        for binding in BINDINGS {
            let Some(chord) = binding.chord.as_ref() else {
                continue;
            };
            assert_ne!(
                chord.key,
                ChordKey::Named(NamedKey::Enter),
                "{}: a modified Enter does not reach this application (measured \
                 2026-08-19 and again 2026-08-25), so no row may be keyed on it",
                binding.id
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
    ///
    /// **`CommandPalette` is the one exception and it is written down rather
    /// than left out** (user ruling 2026-08-28): its row left the table for the
    /// preview while the name stayed, so it is asserted *absent* at the foot of
    /// this test instead of being quietly dropped from the list above — a name
    /// that fell off both would be a verb nobody could tell had been withdrawn
    /// on purpose.
    #[test]
    fn every_action_is_a_row_of_the_table() {
        let mut expected = vec![
            Action::NewTab,
            Action::ClosePane,
            Action::NextTab,
            Action::PrevTab,
            Action::ReopenClosed,
            Action::JumpAttention,
            Action::ToggleFocusMode,
            Action::SplitHorizontal,
            Action::SplitVertical,
            Action::DuplicatePaneSplit,
            Action::ZoomPane,
            Action::FilesPane,
            Action::GitPage,
            Action::OpenSettings,
            Action::SavePreview,
            Action::PrevCommandMark,
            Action::NextCommandMark,
            Action::OpenSearch,
            Action::NextMatch,
            Action::PrevMatch,
            Action::WebAddress,
            Action::WebDevTools,
            Action::WindowAddress,
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
        // **And the one name that is deliberately not a row.** Written as an
        // assertion rather than as an omission, so that putting the row back for
        // v0.2 is a change this test asks for out loud.
        assert!(
            !BINDINGS
                .iter()
                .any(|binding| binding.action == Action::CommandPalette),
            "the palette's row is out of the table until its verb ships - see \
             Action::CommandPalette"
        );
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
    /// (3) drop the holder's id from the verdict — the swap the recorder offers
    ///     (user ruling 2026-08-26) would have to find the row it takes the
    ///     chord off by matching a translated title, which is the one lookup
    ///     that changes answer with the language the window started in.
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
            ChordVerdict::AlreadyUsed {
                holder: "close-pane",
                title: Text::ClosePane,
            },
            "the refusal names the row that has it - by the name a reader sees \
             and by the id the swap acts on"
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
        // The row named here is any row: what is refused is the **chord**, and
        // naming a row that exists is what makes the refusal the zone's rather
        // than the id's. It used to name `command-palette`, whose row left the
        // table on 2026-08-28 — at which point this gate started passing for the
        // wrong reason ("no shortcut is called that in this build"), which is
        // exactly the failure the assertion on `faults[0].reason` catches.
        let faults = table.apply_overrides(&[Override {
            id: "new-tab".to_owned(),
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
            web_page: false,
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
            web_page: false,
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
        let search = named(Text::ShortcutOpenSearch.text());
        assert_eq!(search.ids, vec!["open-search"]);
        assert_eq!(search.caps, vec!["Ctrl", "F"]);
        assert_eq!(
            search.note.as_deref(),
            Some(Text::ShortcutScopeSearchHost.text()),
            "and it wears the scope tag its row carries — which since 2026-08-22              is the one that covers both of the capsule's hosts"
        );

        // **And the palette is not a line at all** (user ruling 2026-08-28).
        // A stub row says its machine has not arrived, which is honest and was
        // enough while the only readers were this panel and the shell. The
        // preview has a reader deciding whether the product works, and to them
        // a listed key that does nothing is a broken feature rather than a
        // dated promise — so the row left the table and this page is one line
        // shorter. See [`Action::CommandPalette`].
        assert!(
            !lines
                .iter()
                .any(|line| line.title == Text::ShortcutCommandPalette.text()),
            "the palette's row is out of the table, so it is off the page: {:?}",
            lines.iter().map(|line| line.title).collect::<Vec<_>>()
        );

        // **And the note comes off the day the verb lands** (§7.1.6b′ F3). This
        // row was a stub for seven weeks beside the palette; the attention queue
        // arrived, so the line under it no longer apologises for it. A stub that
        // kept its note after the machine landed would be the panel telling the
        // reader a working key does nothing.
        assert_eq!(
            named("Jump to the longest waiting pane").note,
            None,
            "P1-8 landed, so the row is an ordinary window row"
        );

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
                    web_page: false,
                },
                Scope::TerminalPrimary => Focus {
                    preview: false,
                    terminal_primary: true,
                    search_open: false,
                    web_page: false,
                },
                Scope::SearchOpen => Focus {
                    preview: false,
                    terminal_primary: true,
                    search_open: true,
                    web_page: false,
                },
                Scope::SearchHost => Focus {
                    preview: false,
                    terminal_primary: true,
                    search_open: false,
                    web_page: false,
                },
                // A page is a preview seat with a page on it, and the capsule
                // can stand over one — which is why the `CloseSearch` row is
                // reachable at all.
                Scope::WebPage => Focus {
                    preview: true,
                    terminal_primary: false,
                    search_open: true,
                    web_page: true,
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

    // ── what a held modifier is told (§7.1.5e′) ────────────────────────────

    /// A terminal showing its own scrollback, which is where most of this
    /// window's rows are in force.
    const ON_A_TERMINAL: Focus = Focus {
        preview: false,
        terminal_primary: true,
        search_open: false,
        web_page: false,
    };

    fn hint_titles(modifiers: ModifiersState, focus: Focus) -> Vec<&'static str> {
        Shortcuts::defaults()
            .hint_lines(modifiers, focus)
            .into_iter()
            .map(|line| line.title)
            .collect()
    }

    /// PIN §7.1.5e′ — **the hold is matched exactly, never as a prefix.**
    ///
    /// Holding `Ctrl` lists the four rows that are `Ctrl` and something, and
    /// none of the twenty that need a `Shift` the reader has not pressed. The
    /// ruling and its argument are on [`Shortcuts::hint_lines`]; this is the
    /// line that makes it a fact.
    ///
    /// Mutation: `chord.modifiers.contains(modifiers)` in
    /// `answers_a_hand_holding`.
    #[test]
    fn a_hold_lists_the_rows_it_is_exactly_and_not_the_ones_it_is_a_prefix_of() {
        let ctrl = hint_titles(CTRL, ON_A_TERMINAL);
        assert!(
            ctrl.contains(&Text::ShortcutNextTab.text()),
            "Ctrl+Tab is a Ctrl row: {ctrl:?}"
        );
        assert!(
            !ctrl.contains(&Text::RailNewTab.text()),
            "Ctrl+Shift+N is not answered by a hand holding only Ctrl: {ctrl:?}"
        );
        let both = hint_titles(CTRL_SHIFT, ON_A_TERMINAL);
        assert!(both.contains(&Text::RailNewTab.text()));
        assert!(
            !both.contains(&Text::ShortcutNextTab.text()),
            "and Ctrl+Tab is not answered by a hand holding Ctrl and Shift: {both:?}"
        );
    }

    /// PIN §7.1.5e′ — **a row out of scope is not in the list**, asked with the
    /// same predicate `lookup` asks.
    ///
    /// `Ctrl+L` is the page's address row and `Ctrl+S` is the preview's save;
    /// with the keyboard on a terminal neither is in the table for a press, so
    /// neither may be in a card that claims to say what a press would do.
    ///
    /// Mutation: drop the `scope.holds(focus)` clause.
    #[test]
    fn a_row_out_of_scope_is_not_in_the_hint() {
        let on_terminal = hint_titles(CTRL, ON_A_TERMINAL);
        assert!(!on_terminal.contains(&Text::ShortcutWebAddress.text()));
        assert!(!on_terminal.contains(&Text::ShortcutSavePreview.text()));
        let on_page = hint_titles(
            CTRL,
            Focus {
                preview: true,
                terminal_primary: false,
                search_open: false,
                web_page: true,
            },
        );
        assert!(
            on_page.contains(&Text::ShortcutWebAddress.text()),
            "and it is there where it is in force: {on_page:?}"
        );
        assert!(
            on_page.contains(&Text::ShortcutSavePreview.text()),
            "a page is a preview seat, so the save row holds too: {on_page:?}"
        );
    }

    /// PIN §7.1.5e′ — **a row whose machine has not arrived is not offered.**
    ///
    /// The shortcut *page* draws a stub row on a line with room for a note; this
    /// card has none. A list of what a press would do may not contain a press
    /// that does nothing.
    ///
    /// **Asked of a row this test builds, and that is not a workaround** (user
    /// ruling 2026-08-28). Every pending row the build now *ships* is a
    /// picture-in-picture slot, and all four ship with no chord at all, so the
    /// shipped table can no longer put a pending row in front of this clause —
    /// `answers_a_hand_holding` would refuse them on the missing chord and the
    /// gate would pass while saying nothing. The property is about a row that is
    /// bound **and** unbuilt, which is the state `command-palette` was in until
    /// its row left the table (see [`Action::CommandPalette`]) and the state the
    /// first assigned summon slot will be in the moment a user records one. So
    /// the subject is constructed here rather than borrowed, and the clause
    /// stays under a gate that can fail.
    ///
    /// Mutation: drop the `!self.action.is_pending()` clause.
    #[test]
    fn a_row_whose_verb_has_not_arrived_is_not_offered() {
        let pending = Binding::window(
            "summon-pip-1",
            Text::ShortcutSummonPip1,
            Action::SummonPip(1),
            Chord::new(CTRL_SHIFT, super::character("0")),
        );
        assert!(
            !pending.answers_a_hand_holding(CTRL_SHIFT, ON_A_TERMINAL),
            "a slot somebody has given a chord still has no machine behind it"
        );
        let arrived = Binding::window(
            "jump-attention",
            Text::ShortcutJumpAttention,
            Action::JumpAttention,
            Chord::new(CTRL_SHIFT, super::character("0")),
        );
        assert!(
            arrived.answers_a_hand_holding(CTRL_SHIFT, ON_A_TERMINAL),
            "and the identical row whose verb exists is offered"
        );
        // The shipped table's own half: the row that stopped being a stub is on
        // the card, and the name whose row left the table is on nothing.
        let held = hint_titles(CTRL_SHIFT, ON_A_TERMINAL);
        assert!(
            held.contains(&Text::ShortcutJumpAttention.text()),
            "the row that stopped being a stub is offered: {held:?}"
        );
        assert!(
            !held.contains(&Text::ShortcutCommandPalette.text()),
            "and the palette, whose row is out of the table entirely, is on \
             nothing: {held:?}"
        );
    }

    /// PIN §7.1.5e′ — **a family is one line, and its cap is the range.**
    ///
    /// Nine ordinals of one verb would bury the fifteen other verbs under them,
    /// which is `editor_rows`' own argument for the same fold.
    ///
    /// Mutation: list the members instead of folding them.
    #[test]
    fn the_nine_tab_ordinals_fold_to_one_line() {
        let lines = Shortcuts::defaults().hint_lines(CTRL_SHIFT, ON_A_TERMINAL);
        let folded: Vec<&HintLine> = lines
            .iter()
            .filter(|line| line.title == Text::ShortcutFamilyGotoTab.text())
            .collect();
        assert_eq!(folded.len(), 1, "one line for the family");
        assert_eq!(folded[0].key, "1 – 9");
        assert!(
            !lines.iter().any(|line| line.key == "5"),
            "and no member is listed beside it: {lines:?}"
        );
    }

    /// PIN §7.1.5e′ — **a family the user has broken up is listed member by
    /// member**, because `1 – 9` over eight bound digits is a line claiming a
    /// key nobody can press.
    ///
    /// Mutation: fold whenever `head.family` is `Some`.
    #[test]
    fn a_family_with_a_member_rebound_elsewhere_is_not_folded() {
        let mut table = Shortcuts::defaults();
        table.set("goto-tab-5", parse_chord("Alt+Shift+5"));
        let lines = table.hint_lines(CTRL_SHIFT, ON_A_TERMINAL);
        assert!(
            !lines.iter().any(|line| line.key == "1 – 9"),
            "the range is a lie once a member has left it: {lines:?}"
        );
        assert!(lines.iter().any(|line| line.key == "4"));
        assert!(
            !lines.iter().any(|line| line.key == "5"),
            "and the one that left is not claimed by this hold: {lines:?}"
        );
    }

    /// PIN §7.1.5e′ — **the hint reads the effective table**, so a chord the
    /// reader recorded is the chord the card shows.
    ///
    /// Mutation: walk `BINDINGS` instead of `self.rows`.
    #[test]
    fn the_hint_shows_the_chord_a_reader_recorded_and_not_the_default() {
        let mut table = Shortcuts::defaults();
        table.set("new-tab", parse_chord("Ctrl+Shift+Y"));
        let lines = table.hint_lines(CTRL_SHIFT, ON_A_TERMINAL);
        let row = lines
            .iter()
            .find(|line| line.title == Text::RailNewTab.text())
            .expect("the row is still in force");
        assert_eq!(row.key, "Y");
    }

    /// PIN §7.1.5e′ — **a row a reader has unbound is not in the list**, which
    /// is the whole of what `chord: None` means, and **an empty hand is asked
    /// nothing.**
    #[test]
    fn an_unbound_row_and_an_empty_hand_are_both_silent() {
        let mut table = Shortcuts::defaults();
        table.set("new-tab", None);
        let lines = table.hint_lines(CTRL_SHIFT, ON_A_TERMINAL);
        assert!(
            !lines
                .iter()
                .any(|line| line.title == Text::RailNewTab.text())
        );
        assert!(
            Shortcuts::defaults()
                .hint_lines(ModifiersState::empty(), ON_A_TERMINAL)
                .is_empty(),
            "no hold, no card"
        );
    }

    /// PIN §7.1.5e′ — **a hold this table claims nothing for says nothing**,
    /// which is what keeps a card off the glass for a `Shift` held while typing
    /// and for anything carrying the Windows key.
    ///
    /// The `Super` half is not an omission being papered over: no row of this
    /// table wears it, so exact matching answers it without a special case, and
    /// this is the line that says the answer is deliberate.
    #[test]
    fn a_hold_this_table_claims_nothing_for_raises_nothing() {
        assert!(
            Shortcuts::defaults()
                .hint_lines(ModifiersState::SHIFT, ON_A_TERMINAL)
                .is_empty(),
            "bare Shift on a terminal with no search open claims nothing"
        );
        assert!(
            Shortcuts::defaults()
                .hint_lines(ModifiersState::ALT, ON_A_TERMINAL)
                .is_empty(),
            "and bare Alt claims nothing, which is why this card changes nothing \
             about what Windows does with the menu key"
        );
        assert!(
            Shortcuts::defaults()
                .hint_lines(
                    ModifiersState::CONTROL.union(ModifiersState::SUPER),
                    ON_A_TERMINAL
                )
                .is_empty(),
            "and nothing in this table wears the Windows key"
        );
    }

    /// RED (user ruling 2026-08-26) — **a conflict names the row that has the
    /// chord twice over: once for the reader and once for the machine**, and
    /// the second one is the whole of what makes a swap possible.
    ///
    /// The title is a translated string and cannot be looked up in the table;
    /// the id is the same word `keybindings.json` uses and is the same in every
    /// language this window starts in.
    ///
    /// MUTATION: return `holder: ""` from `chord_verdict` — the offer still
    /// reads correctly and takes the chord off nothing.
    #[test]
    fn a_conflict_carries_the_id_of_the_row_that_has_the_chord() {
        let table = Shortcuts::defaults();
        let taken = Chord::new(CTRL_SHIFT, super::character("w"));
        let verdict = table.verdict_for("new-tab", &taken);
        assert_eq!(verdict.holder(), Some("close-pane"));
        assert!(
            table.rows().iter().any(|row| row.id == "close-pane"),
            "the id the verdict hands back is a row of the table it judged"
        );
        // A verdict with nobody behind it has no holder and no offer: the two
        // other refusals are final, and a recorder that read an offer off them
        // would be promising to take a chord away from Windows.
        let altgr = Chord::new(
            ModifiersState::CONTROL.union(ModifiersState::ALT),
            super::character("p"),
        );
        assert_eq!(table.verdict_for("new-tab", &altgr).holder(), None);
        assert_eq!(table.verdict_for("new-tab", &altgr).swap_offer(), None);
        assert!(table.verdict_for("new-tab", &taken).swap_offer().is_some());
    }

    /// RED (user ruling 2026-08-26) — **the swap is one edit and it costs
    /// exactly one row its key.**
    ///
    /// The row that had the chord is left *unbound* rather than restored to some
    /// other default, because unbound is what the user chose: they were told
    /// which row it would come off and they pressed `Enter` anyway. It is also
    /// the state `keybindings.json` can say — `"chord": null` — so the swap
    /// survives the next launch, which a table that quietly left the holder on
    /// its default would not.
    ///
    /// MUTATION: make `take_chord_from` skip the `set(holder, None)` — the
    /// table now has one chord on two rows, which is the ambiguity the flat
    /// table has forbidden since it was flat.
    #[test]
    fn taking_a_chord_from_a_row_leaves_that_row_unbound_and_this_one_holding_it() {
        let mut table = Shortcuts::defaults();
        let taken = Chord::new(CTRL_SHIFT, super::character("w"));
        let ChordVerdict::AlreadyUsed { holder, .. } = table.verdict_for("new-tab", &taken) else {
            panic!("Ctrl+Shift+W is close-pane's in this build");
        };
        table.take_chord_from(holder, "new-tab", taken.clone());
        let row = |id: &str| {
            table
                .rows()
                .iter()
                .find(|row| row.id == id)
                .expect("both rows are in the table")
        };
        assert_eq!(row("new-tab").chord.as_ref(), Some(&taken));
        assert_eq!(row("close-pane").chord, None, "one row, one key");
        // And the table it left behind is one the file can hold and the judge
        // agrees with: nothing claims the chord twice.
        assert_eq!(
            table.verdict_for("close-pane", &taken),
            ChordVerdict::AlreadyUsed {
                holder: "new-tab",
                title: Text::RailNewTab,
            },
            "the chord has moved, and the judge now names the row it moved to"
        );
        let departures = table.overrides();
        assert!(
            departures
                .iter()
                .any(|entry| entry.id == "close-pane" && entry.chord.is_none())
        );
        assert!(
            departures.iter().any(
                |entry| entry.id == "new-tab" && entry.chord.as_deref() == Some("Ctrl+Shift+w")
            )
        );
    }

    /// RED (found 2026-08-26) — **a file that moves a chord from one row to
    /// another lands whole, in either order**, and so does one that trades two
    /// chords between two rows.
    ///
    /// This is the defect the swap uncovered on the first launch after it: read
    /// a line at a time into a table still holding the factory chords, the
    /// *user's own file* is refused — `new-tab: Ctrl+Shift+w` conflicts with a
    /// `close-pane` that the very next line is about to clear. The file has to
    /// be read as a set of sentences about the rows it names, which is what
    /// `apply_overrides`' two passes do.
    ///
    /// The trade is the case no line-at-a-time reading can ever accept, in any
    /// order: each of the two lines conflicts with the row the other one is
    /// about. It is here because it is the proof that the two passes are the
    /// *general* answer and not a re-ordering that happens to fix one shape.
    ///
    /// MUTATION: put the reading back to one pass — both halves go red, and the
    /// second stays red however the lines are shuffled.
    #[test]
    fn a_file_that_moves_a_chord_between_rows_lands_whole_in_either_order() {
        let moved = [
            Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+w".to_owned()),
            },
            Override {
                id: "close-pane".to_owned(),
                chord: None,
            },
        ];
        for order in [0, 1] {
            let mut lines = moved.to_vec();
            if order == 1 {
                lines.reverse();
            }
            let mut table = Shortcuts::defaults();
            let faults = table.apply_overrides(&lines);
            assert!(faults.is_empty(), "order {order}: {faults:?}");
            let chord_of = |id: &str| {
                table
                    .rows()
                    .iter()
                    .find(|row| row.id == id)
                    .and_then(|row| row.chord.clone())
            };
            assert_eq!(
                chord_of("new-tab"),
                Some(Chord::new(CTRL_SHIFT, super::character("w")))
            );
            assert_eq!(chord_of("close-pane"), None);
        }

        // The trade: two rows, two chords, each line naming the other's.
        let mut table = Shortcuts::defaults();
        let faults = table.apply_overrides(&[
            Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+w".to_owned()),
            },
            Override {
                id: "close-pane".to_owned(),
                chord: Some("Ctrl+Shift+n".to_owned()),
            },
        ]);
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(
            table.lookup(
                &Key::Character("w".into()),
                &Key::Character("w".into()),
                CTRL_SHIFT,
                Focus::default()
            ),
            Some(Action::NewTab)
        );
        assert_eq!(
            table.lookup(
                &Key::Character("n".into()),
                &Key::Character("n".into()),
                CTRL_SHIFT,
                Focus::default()
            ),
            Some(Action::ClosePane)
        );

        // And a file that really does claim one chord twice is still refused —
        // the second line, and only the second, with the first row left holding
        // the key it asked for.
        let mut table = Shortcuts::defaults();
        let faults = table.apply_overrides(&[
            Override {
                id: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+j".to_owned()),
            },
            Override {
                id: "close-pane".to_owned(),
                chord: Some("Ctrl+Shift+j".to_owned()),
            },
        ]);
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].id, "close-pane");
        assert_eq!(
            table
                .rows()
                .iter()
                .find(|row| row.id == "close-pane")
                .and_then(|row| row.chord.clone()),
            Some(Chord::new(CTRL_SHIFT, super::character("w"))),
            "a refused line leaves its row at the default, when the default is \
             still free to be given back"
        );
    }

    /// RED — **a customised table survives the round trip through
    /// `keybindings.json` and comes back the same table.**
    ///
    /// The whole of what the file has to preserve, in one walk: a moved chord, a
    /// row whose key was taken away on purpose, and a slot that shipped with no
    /// chord and was given one. The third is the one a round trip most easily
    /// loses — `Binding::unassigned`'s rows have `None` as their *default*, so a
    /// writer that thought "absent means unbound" would drop the line that gave
    /// one a key.
    ///
    /// MUTATION: have `overrides()` skip rows whose default chord is `None` —
    /// the picture-in-picture slot comes back empty on the next launch.
    #[test]
    fn a_customised_table_survives_the_round_trip_through_the_file() {
        let mut table = Shortcuts::defaults();
        let taken = Chord::new(CTRL_SHIFT, super::character("w"));
        table.take_chord_from("close-pane", "new-tab", taken);
        table.set(
            "summon-pip-1",
            Some(Chord::new(ALT_SHIFT, ChordKey::Named(NamedKey::F8))),
        );

        // Out through the vocabulary the file is written in, and back in the
        // vocabulary a launch reads it in — the two conversions `persist.rs`
        // makes, with the disk taken out of the middle.
        let file = bt_persist::KeybindingsV1 {
            schema_version: bt_persist::KEYBINDINGS_SCHEMA_VERSION,
            bindings: table
                .overrides()
                .into_iter()
                .map(|entry| bt_persist::BindingOverrideV1 {
                    action: entry.id,
                    chord: entry.chord,
                })
                .collect(),
        };
        let wire = serde_json::to_string(&file).expect("the file serialises");
        let read: bt_persist::KeybindingsV1 = serde_json::from_str(&wire).expect("and reads back");
        let overrides: Vec<Override> = read
            .bindings
            .into_iter()
            .map(|entry| Override {
                id: entry.action,
                chord: entry.chord,
            })
            .collect();

        let mut relaunched = Shortcuts::defaults();
        let faults = relaunched.apply_overrides(&overrides);
        assert!(
            faults.is_empty(),
            "a file this build wrote is a file this build can read: {faults:?}"
        );
        assert_eq!(relaunched, table, "the same table, chord for chord");
    }

    /// RED (§7.1.5e′, and the hot-effect half of the 2026-08-26 slice) — **every
    /// surface that prints a chord reads the effective table, so an edit is on
    /// the glass without anything being told about it.**
    ///
    /// Three readers, one table: the shortcut page's own line, the hint card,
    /// and dispatch itself. There is no revision counter and no invalidation
    /// call anywhere in this module, and that is the design — the caches that
    /// would need one do not exist, because all three are derived per frame from
    /// `Shortcuts::rows`.
    ///
    /// MUTATIONS: fold `hint_lines`' members out of `BINDINGS`, or take
    /// `editor_rows`' caps off `BINDINGS[index]` — each on its own puts the
    /// factory chord back on one surface while the window answers to the other,
    /// which is a shortcut table with two answers.
    #[test]
    fn a_rebound_chord_is_on_every_surface_that_prints_one() {
        let mut table = Shortcuts::defaults();
        let moved = Chord::new(CTRL_SHIFT, super::character("y"));
        table.set("new-tab", Some(moved));

        let line = table
            .editor_rows()
            .into_iter()
            .find(|line| line.ids.first() == Some(&"new-tab"))
            .expect("the page still lists the row");
        assert_eq!(line.caps, vec!["Ctrl", "Shift", "Y"]);
        assert!(
            line.overridden,
            "and marks it as departing from the default"
        );

        let card = table.hint_lines(CTRL_SHIFT, ON_A_TERMINAL);
        assert!(
            card.iter()
                .any(|hint| hint.title == Text::RailNewTab.text() && hint.key == "Y"),
            "the hint card prints the new key: {card:?}"
        );
        assert!(
            !card.iter().any(|hint| hint.key == "N"),
            "and never the one the build shipped"
        );

        let press = |glyph: &str| {
            let key = Key::Character(glyph.into());
            table.lookup(&key, &key, CTRL_SHIFT, ON_A_TERMINAL)
        };
        assert_eq!(press("y"), Some(Action::NewTab));
        assert_eq!(press("n"), None, "and the old chord is nobody's");
    }

    /// PIN §7.1.5e′ — **every cap the card prints is a cap somebody can find on
    /// a keyboard**, and never the empty string a name this grammar cannot
    /// write would leave behind.
    #[test]
    fn every_cap_a_hint_prints_is_a_real_key() {
        for modifiers in [CTRL, CTRL_SHIFT, ALT_SHIFT, ModifiersState::empty()] {
            for focus in REACHABLE_FOCUS {
                for line in Shortcuts::defaults().hint_lines(modifiers, focus) {
                    assert!(
                        !line.key.is_empty(),
                        "{:?} printed an empty cap for {:?}",
                        line.title,
                        modifiers
                    );
                    assert!(!line.title.is_empty());
                }
            }
        }
    }

    /// RED (gesture audit 2026-08-26, 系统性发现 ②) — **a menu row can ask this
    /// table what its verb's chord is, and gets the reader's own answer.**
    ///
    /// The audit's second systemic finding is that this window opens the door
    /// between its two vocabularies from one side only: the hint card teaches
    /// the keyboard's rows, and no menu anywhere prints a chord. This is the
    /// lookup the menus use, and the two things it has to get right are that it
    /// reads the **effective** table (so a rebound chord follows) and that an
    /// unbound row prints nothing at all rather than the word for nothing.
    ///
    /// MUTATION: read [`BINDINGS`] instead of `self.rows` and the rebound
    /// assertion goes red — a menu with the factory chord on it after a rebind
    /// is a second answer to "what key is this".
    #[test]
    fn a_menu_row_reads_its_chord_off_the_effective_table() {
        let table = Shortcuts::defaults();
        assert_eq!(
            table.accelerator(Action::ClosePane).as_deref(),
            Some("Ctrl+Shift+W")
        );
        assert_eq!(
            table.accelerator(Action::ZoomPane).as_deref(),
            Some("Ctrl+Shift+X")
        );
        assert_eq!(
            table.accelerator(Action::OpenSearch).as_deref(),
            Some("Ctrl+F")
        );
        // A row that ships with no chord at all has nothing to print.
        assert_eq!(table.accelerator(Action::SummonPip(1)), None);

        // The reader's own table, and not this build's: a rebind follows, and an
        // unbind takes the annotation away with it.
        let mut rebound = Shortcuts::defaults();
        rebound.apply_overrides(&[
            Override {
                id: "close-pane".to_owned(),
                chord: Some("Ctrl+Shift+J".to_owned()),
            },
            Override {
                id: "zoom-pane".to_owned(),
                chord: None,
            },
        ]);
        assert_eq!(
            rebound.accelerator(Action::ClosePane).as_deref(),
            Some("Ctrl+Shift+J")
        );
        assert_eq!(
            rebound.accelerator(Action::ZoomPane),
            None,
            "a chord the reader gave back to their shell is not a chord a menu may print"
        );
    }

    /// The workspace root, reached from the crate this test is compiled in.
    fn repository_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .expect("this crate sits two directories below the workspace root")
    }

    /// `docs/shortcuts.md` as [`BINDINGS`] would have it, in both languages.
    ///
    /// One renderer with two readers: the gate below compares it against the
    /// file in the tree, and `scripts/generate-shortcuts-table.ps1` writes the
    /// file from the copy every run leaves in `target/`.
    fn shortcuts_document() -> String {
        let mut out = String::from(concat!(
            "# Shortcuts\n",
            "\n",
            "<!-- Written from `BINDINGS` in `crates/bt-app/src/shortcuts.rs`, not by hand.\n",
            "     `scripts/check-shortcuts-table.ps1` turns red when this file and that\n",
            "     table disagree; `scripts/generate-shortcuts-table.ps1` writes it again. -->\n",
        ));
        for lang in Lang::ALL {
            let (heading, lead, columns) = match lang {
                Lang::English => (
                    "## English",
                    "Every key here can be changed on the Shortcuts page in Settings. \
                     Changing one writes `%APPDATA%\\Folio\\keybindings.json`; the last \
                     column is the name a row has in that file.",
                    ["Key", "What it does", "Where it works", "Name in the file"],
                ),
                Lang::Chinese => (
                    "## 中文",
                    "下面每一组键都能在设置的快捷键页里改。改过之后写进 \
                     `%APPDATA%\\Folio\\keybindings.json`，最后一列就是这一行在那个文件里的名字。",
                    ["按键", "作用", "在哪里生效", "文件里的名字"],
                ),
            };
            let _ = write!(out, "\n{heading}\n\n{lead}\n\n");
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                columns[0], columns[1], columns[2], columns[3]
            );
            let _ = writeln!(out, "| --- | --- | --- | --- |");
            for binding in BINDINGS {
                let key = binding.chord.as_ref().map_or_else(
                    || Text::ShortcutUnbound.in_lang(lang).to_owned(),
                    |chord| chord_caps(chord).join("+"),
                );
                let note = binding.note_in(lang).unwrap_or(Cow::Borrowed(""));
                let _ = writeln!(
                    out,
                    "| {key} | {} | {note} | `{}` |",
                    binding.title.in_lang(lang),
                    binding.id
                );
            }
        }
        out
    }

    /// **The table in `docs/shortcuts.md` is this table.**
    ///
    /// The README sends a reader to that file, and a hand-kept copy of forty
    /// rows is a copy that lies on the first row anybody adds — this repository
    /// has already paid for one second list that agreed with the first only on
    /// the day it was written. So the file is rendered from `BINDINGS` and this
    /// gate holds the two together: add a row, retire one, or move a chord
    /// without writing the file again and the workspace goes red.
    ///
    /// Every run also leaves the rendering in `target/shortcuts-table.md`, which
    /// is the file `scripts/generate-shortcuts-table.ps1` copies over the
    /// checked-in one.
    #[test]
    fn docs_shortcuts_md_is_the_bindings_table() {
        let root = repository_root();
        let rendered = shortcuts_document();
        let generated = root.join("target").join("shortcuts-table.md");
        if let Some(parent) = generated.parent() {
            std::fs::create_dir_all(parent).expect("the workspace has a target directory");
        }
        std::fs::write(&generated, rendered.as_bytes()).expect("target/ is writable");
        let checked_in = root.join("docs").join("shortcuts.md");
        let held = std::fs::read_to_string(&checked_in)
            .expect("docs/shortcuts.md is checked in")
            .replace("\r\n", "\n");
        // The first line that differs, and not both documents: a hundred-row
        // table printed twice is a failure a reader scrolls past rather than
        // reads, and the answer to "what drifted" is one line long.
        if let Some((number, held_line, wanted)) = held
            .lines()
            .map(Some)
            .chain(std::iter::repeat(None))
            .zip(rendered.lines().map(Some).chain(std::iter::repeat(None)))
            .take(held.lines().count().max(rendered.lines().count()))
            .enumerate()
            .find(|(_, (held_line, wanted))| held_line != wanted)
            .map(|(index, (held_line, wanted))| {
                (
                    index + 1,
                    held_line.unwrap_or("<end of file>"),
                    wanted.unwrap_or("<end of file>"),
                )
            })
        {
            panic!(
                "docs/shortcuts.md line {number} is not what BINDINGS says — \
                 run scripts/generate-shortcuts-table.ps1\n  \
                 the file:  {held_line}\n  the table: {wanted}"
            );
        }
    }
}
