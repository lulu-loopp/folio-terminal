//! The keyboard shortcut registry (P2-7 audit, user ruling 2026-08-10 "plan A").
//!
//! One constant table maps every bindable [`Action`] to its default [`Chord`] and the [`Scope`] it
//! is in force in. Event dispatch is a lookup into this table, never a scattered chain of modifier
//! `if`s: the future shortcut-editing panel edits exactly this data, so a binding that is not
//! expressible here is a binding the panel could never show — which is why the preview's `Ctrl+S`
//! arrived as a third column (ruling 9, 2026-08-12) rather than as an `if` at the dispatch site.

use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Everything the window can be asked to do from the keyboard.
///
/// `JumpAttention` and `CommandPalette` are real rows with no machine behind them yet (the
/// attention queue is P1-8, the palette is P1-9). They are listed, bound, and dispatched to an
/// explicit no-op rather than omitted: the audit decided the key belongs to us, so nothing else may
/// claim it and nothing may leak to the shell in the meantime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    NewTab,
    ClosePane,
    NextTab,
    PrevTab,
    /// 1-based tab ordinal, always within `1..=9`; out-of-range targets are ignored at dispatch.
    GotoTab(u8),
    ReopenClosed,
    JumpAttention,
    CommandPalette,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChordKey {
    Character(&'static str),
    Named(NamedKey),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

/// One row of the table: what it does, what it is pressed with, and where it is
/// in force.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub(crate) action: Action,
    pub(crate) chord: Chord,
    pub(crate) scope: Scope,
}

impl Binding {
    /// A row in force everywhere.
    const fn window(action: Action, chord: Chord) -> Self {
        Self {
            action,
            chord,
            scope: Scope::Window,
        }
    }

    /// A row in force only while the preview seat holds the focus.
    const fn preview(action: Action, chord: Chord) -> Self {
        Self {
            action,
            chord,
            scope: Scope::Preview,
        }
    }

    /// A row in force only on a terminal showing its primary screen.
    const fn terminal_primary(action: Action, chord: Chord) -> Self {
        Self {
            action,
            chord,
            scope: Scope::TerminalPrimary,
        }
    }

    /// A row in force only while the search capsule is up.
    const fn search_open(action: Action, chord: Chord) -> Self {
        Self {
            action,
            chord,
            scope: Scope::SearchOpen,
        }
    }
}

/// The default binding table. This is the single source of truth for shortcut keys.
///
/// Every **window** action wears Shift alongside Ctrl because bare `Ctrl+letter` is the shell's
/// control-code alphabet, and no row uses `Ctrl+Alt`: Windows reports AltGr as exactly that pair,
/// so binding it would steal a character from every layout that composes with AltGr. The one bare
/// `Ctrl+letter` in the table is scoped instead of shifted — see [`Scope`].
pub(crate) const BINDINGS: &[Binding] = &[
    Binding::window(Action::NewTab, Chord::new(CTRL_SHIFT, character("n"))),
    Binding::window(Action::ClosePane, Chord::new(CTRL_SHIFT, character("w"))),
    Binding::window(
        Action::NextTab,
        Chord::new(CTRL, ChordKey::Named(NamedKey::Tab)),
    ),
    Binding::window(
        Action::PrevTab,
        Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::Tab)),
    ),
    Binding::window(Action::GotoTab(1), Chord::new(CTRL_SHIFT, character("1"))),
    Binding::window(Action::GotoTab(2), Chord::new(CTRL_SHIFT, character("2"))),
    Binding::window(Action::GotoTab(3), Chord::new(CTRL_SHIFT, character("3"))),
    Binding::window(Action::GotoTab(4), Chord::new(CTRL_SHIFT, character("4"))),
    Binding::window(Action::GotoTab(5), Chord::new(CTRL_SHIFT, character("5"))),
    Binding::window(Action::GotoTab(6), Chord::new(CTRL_SHIFT, character("6"))),
    Binding::window(Action::GotoTab(7), Chord::new(CTRL_SHIFT, character("7"))),
    Binding::window(Action::GotoTab(8), Chord::new(CTRL_SHIFT, character("8"))),
    Binding::window(Action::GotoTab(9), Chord::new(CTRL_SHIFT, character("9"))),
    Binding::window(Action::ReopenClosed, Chord::new(CTRL_SHIFT, character("t"))),
    Binding::window(
        Action::JumpAttention,
        Chord::new(CTRL_SHIFT, character("a")),
    ),
    Binding::window(
        Action::CommandPalette,
        Chord::new(CTRL_SHIFT, character("p")),
    ),
    Binding::window(
        Action::SplitHorizontal,
        Chord::new(ALT_SHIFT, character("-")),
    ),
    Binding::window(Action::SplitVertical, Chord::new(ALT_SHIFT, character("="))),
    Binding::window(
        Action::DuplicatePaneSplit,
        Chord::new(CTRL_SHIFT, character("d")),
    ),
    // **`Ctrl+Shift+B`, and pointedly not the mock-up's `Ctrl+B`.**
    //
    // The mock-up binds a bare `Ctrl+B` (6126-6134), which discipline ① above
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
    Binding::window(Action::FilesPane, Chord::new(CTRL_SHIFT, character("b"))),
    // **`Ctrl+Shift+G`** (R28, 2026-08-15) — the chord VS Code has spent a
    // decade teaching, for the surface it taught it on.
    //
    // It wears Shift for `Ctrl+Shift+B`'s reason and not merely for its company:
    // a bare `^G` is readline's "abort the current command", which is the very
    // key someone reaches for when a shell has them halfway into something they
    // want out of, and discipline ① does not let this table take it.
    //
    // The row it works is the column's page, so it does nothing at all when the
    // window has no files column and nothing when the Git panel is switched off —
    // a chord for a surface that is not there is not an error, it is a chord with
    // nothing to say.
    Binding::window(Action::GitPage, Chord::new(CTRL_SHIFT, character("g"))),
    Binding::window(Action::OpenSettings, Chord::new(CTRL, character(","))),
    // **The one scoped row** (ruling 9, 2026-08-12). It is the mock-up's chord
    // verbatim — bare `Ctrl+S`, from any focus state *inside the preview*, so a
    // buffer can be saved after flipping to the rendered view or clicking
    // elsewhere in the pane — and it is claimed nowhere else, which is what
    // leaves `^S` with the shell. The audit's ① discipline forbids taking a bare
    // control letter *from the terminal*; it does not forbid a chord in a place
    // where there is no terminal to take it from.
    Binding::preview(Action::SavePreview, Chord::new(CTRL, character("s"))),
    // **`Ctrl+Shift+↑/↓`, and pointedly not the mock-up's `Ctrl+Alt+↑/↓`** (user
    // ruling 2026-08-16, inventory D-1).
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
        Action::PrevCommandMark,
        Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::ArrowUp)),
    ),
    Binding::terminal_primary(
        Action::NextCommandMark,
        Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::ArrowDown)),
    ),
    // **`Ctrl+F`, and it is an exception to discipline ① written down as one**
    // (user ruling, 2026-08-16, inventory D-2).
    //
    // Discipline ① — bare `Ctrl+letter` is the shell's control-code alphabet and
    // is never taken — is the reason every window row above wears Shift, and it
    // is why the same audit moved `Ctrl+B` to `Ctrl+Shift+B` two months ago. The
    // ruling that overturns it here is narrow and gives its reasons: what ①
    // protects is a control code with an *owner*, and `^F` has none on Windows
    // that a terminal user meets — readline's forward-one-character is the same
    // key as `→` and nobody presses it; the reference product for this surface
    // (VS Code's integrated terminal) takes `Ctrl+F` for exactly this box; and
    // the row is scoped, so the moment a full-screen program is on the glass the
    // chord is not in the table at all and `less` keeps its page-forward.
    //
    // That last clause is doing the real work. This is the same shape as
    // `Ctrl+S` in a preview (ruling 9): the discipline forbids taking a bare
    // control letter *from a terminal*, and here it is taken only where the
    // scrollback the search reads actually exists.
    Binding::terminal_primary(Action::OpenSearch, Chord::new(CTRL, character("f"))),
    // **The alias**, bound in the same breath by the same ruling.
    //
    // Two rows and not a special case in the matcher, because two rows is what
    // the shortcut-editing panel can show and edit: a reader whose muscles know
    // `Ctrl+Shift+F` from another product finds the box, and a reader who wants
    // `^F` back for their shell can take this row's chord and delete the other's
    // without the panel having to explain that one of them was a hidden twin.
    Binding::terminal_primary(Action::OpenSearch, Chord::new(CTRL_SHIFT, character("f"))),
    // `F3` / `Shift+F3` (B81) — the walk that works while the terminal still has
    // the keyboard. Bare, because a function key is not a control code and the
    // discipline has nothing to say about it, and scoped so that a shell which
    // uses `F3` (`cmd.exe` recalls its last command with it) keeps the key
    // whenever there is no search to walk.
    Binding::search_open(
        Action::NextMatch,
        Chord::new(ModifiersState::empty(), ChordKey::Named(NamedKey::F3)),
    ),
    Binding::search_open(
        Action::PrevMatch,
        Chord::new(ModifiersState::SHIFT, ChordKey::Named(NamedKey::F3)),
    ),
];

const fn character(text: &'static str) -> ChordKey {
    ChordKey::Character(text)
}

/// Resolve a key press to an action.
///
/// `logical` is the key winit produced (Shift already applied); `base` is the same physical key
/// with every modifier stripped, from `KeyEventExtModifierSupplement::key_without_modifiers`.
/// `focus` is what the window's keyboard focus looks like, which is what decides whether a scoped
/// row is in force at all — a row out of scope is not "found and ignored", it is not in the table
/// for this press, so the key falls through to the encoder exactly as an unbound one does.
pub(crate) fn lookup_action(
    logical: &Key,
    base: &Key,
    modifiers: ModifiersState,
    focus: Focus,
) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|binding| {
            binding.scope.holds(focus) && binding.chord.matches(logical, base, modifiers)
        })
        .map(|binding| binding.action)
}

impl Chord {
    fn matches(&self, logical: &Key, base: &Key, modifiers: ModifiersState) -> bool {
        // Exact, not "contains": a superset such as the retired Ctrl+Alt+Shift dev keys must miss
        // the table entirely rather than land on the Ctrl+Shift row underneath it.
        if modifiers != self.modifiers {
            return false;
        }
        match self.key {
            ChordKey::Named(named) => {
                let named_matches = |key: &Key| matches!(key, Key::Named(other) if *other == named);
                named_matches(logical) || named_matches(base)
            }
            // Shift folding: a US keyboard reports Shift+1 as "!" with a bare key of "1", while a
            // layout that puts the digit behind Shift reports the reverse. Accepting either end
            // binds the key the user sees printed on it in both cases.
            ChordKey::Character(text) => {
                let text_matches = |key: &Key| matches!(key, Key::Character(produced) if produced.eq_ignore_ascii_case(text));
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
        lookup_action(&key, &key, modifiers, Focus::default())
    }

    /// The same press with the preview seat holding the focus.
    fn press_in_preview(key: Key, modifiers: ModifiersState) -> Option<Action> {
        lookup_action(
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
        lookup_action(
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
        lookup_action(
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

    /// Every row of the ruled table, asserted one binding at a time.
    #[test]
    fn every_ruled_binding_resolves_to_its_action() {
        assert_eq!(press(character("n"), CTRL_SHIFT), Some(Action::NewTab));
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
            Some(Action::OpenSearch),
            "the alias reaches the same verb"
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
    /// Five promises, one assertion each: the chord opens the capsule on a
    /// scrollback; the alias reaches the same verb; the alternate screen keeps
    /// both, so `less` still pages forward; and a document is not a scrollback.
    ///
    /// MUTATIONS:
    /// (1) give the rows `Scope::Window` — the alternate-screen assertions go
    ///     red, and so does `bare_control_letters_stay_with_the_terminal`, which
    ///     is discipline (1) noticing;
    /// (2) drop the alias row — the second assertion goes red;
    /// (3) move the row to `CTRL_SHIFT` alone — the first goes red, which is the
    ///     ruling refusing to be renegotiated by a later tidy-up.
    #[test]
    fn control_f_opens_the_search_on_a_scrollback_and_passes_through_on_the_alternate_screen() {
        assert_eq!(
            press_on_primary_screen(character("f"), CTRL),
            Some(Action::OpenSearch)
        );
        assert_eq!(
            press_on_primary_screen(character("f"), CTRL_SHIFT),
            Some(Action::OpenSearch)
        );
        assert_eq!(
            press(character("f"), CTRL),
            None,
            "on the alternate screen ^F is the program's page-forward and reaches it untouched"
        );
        assert_eq!(
            press(character("f"), CTRL_SHIFT),
            None,
            "and so is the alias - the whole row is out of the table there"
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
        // 19 single actions plus GotoTab(1..=9), plus one alias row: `Ctrl+Shift+F`
        // is a second chord for `OpenSearch` and is the only place in this table
        // where two rows name one verb (user ruling 2026-08-16).
        assert_eq!(BINDINGS.len(), 29);

        // Two rows may share a chord only if no focus state has both in force —
        // which is what a scope is *for*, and also the one way scopes could
        // quietly reintroduce the ambiguity the flat table forbade.
        //
        // Every focus the window can actually be in, and no impossible one: the
        // keyboard is on a preview, on a terminal's scrollback, or on neither
        // (a terminal running a full-screen program, a files column, a menu).
        // "Both at once" is not a state a window has.
        for (index, binding) in BINDINGS.iter().enumerate() {
            for other in BINDINGS.iter().skip(index + 1) {
                let overlap = [
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
                    // The capsule is only ever up over a terminal on its primary
                    // screen, so this is the fourth state and not a fifth: there
                    // is no "search open over a preview".
                    Focus {
                        preview: false,
                        terminal_primary: true,
                        search_open: true,
                    },
                ]
                .into_iter()
                .any(|focus| binding.scope.holds(focus) && other.scope.holds(focus));
                assert!(
                    binding.chord != other.chord || !overlap,
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
        }
    }

    /// Every action the enum can name must actually be reachable from the table.
    #[test]
    fn every_action_has_a_default_chord() {
        let mut expected = vec![
            Action::NewTab,
            Action::ClosePane,
            Action::NextTab,
            Action::PrevTab,
            Action::ReopenClosed,
            Action::JumpAttention,
            Action::CommandPalette,
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

        for action in expected {
            assert!(
                BINDINGS.iter().any(|binding| binding.action == action),
                "{action:?} has no default chord"
            );
        }
    }

    /// Each table row must be reachable through the same lookup dispatch uses —
    /// **inside its own scope**, which is the only place a scoped row claims to
    /// be reachable at all.
    #[test]
    fn every_table_row_round_trips_through_lookup() {
        for binding in BINDINGS {
            let key = match binding.chord.key {
                ChordKey::Character(text) => Key::Character(text.into()),
                ChordKey::Named(named) => Key::Named(named),
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
                lookup_action(&key, &key, binding.chord.modifiers, focus),
                Some(binding.action),
                "{:?} is in the table but unreachable",
                binding.action
            );
        }
    }
}
