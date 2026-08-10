//! The keyboard shortcut registry (P2-7 audit, user ruling 2026-08-10 "plan A").
//!
//! One constant table maps every bindable [`Action`] to its default [`Chord`]. Event dispatch is a
//! lookup into this table, never a scattered chain of modifier `if`s: the future shortcut-editing
//! panel edits exactly this data, so a binding that is not expressible here is a binding the panel
//! could never show.

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
    OpenSettings,
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

/// The default binding table. This is the single source of truth for shortcut keys.
///
/// Every window action wears Shift alongside Ctrl because bare `Ctrl+letter` is the shell's
/// control-code alphabet, and no row uses `Ctrl+Alt`: Windows reports AltGr as exactly that pair,
/// so binding it would steal a character from every layout that composes with AltGr.
pub(crate) const BINDINGS: &[(Action, Chord)] = &[
    (Action::NewTab, Chord::new(CTRL_SHIFT, character("n"))),
    (Action::ClosePane, Chord::new(CTRL_SHIFT, character("w"))),
    (
        Action::NextTab,
        Chord::new(CTRL, ChordKey::Named(NamedKey::Tab)),
    ),
    (
        Action::PrevTab,
        Chord::new(CTRL_SHIFT, ChordKey::Named(NamedKey::Tab)),
    ),
    (Action::GotoTab(1), Chord::new(CTRL_SHIFT, character("1"))),
    (Action::GotoTab(2), Chord::new(CTRL_SHIFT, character("2"))),
    (Action::GotoTab(3), Chord::new(CTRL_SHIFT, character("3"))),
    (Action::GotoTab(4), Chord::new(CTRL_SHIFT, character("4"))),
    (Action::GotoTab(5), Chord::new(CTRL_SHIFT, character("5"))),
    (Action::GotoTab(6), Chord::new(CTRL_SHIFT, character("6"))),
    (Action::GotoTab(7), Chord::new(CTRL_SHIFT, character("7"))),
    (Action::GotoTab(8), Chord::new(CTRL_SHIFT, character("8"))),
    (Action::GotoTab(9), Chord::new(CTRL_SHIFT, character("9"))),
    (Action::ReopenClosed, Chord::new(CTRL_SHIFT, character("t"))),
    (
        Action::JumpAttention,
        Chord::new(CTRL_SHIFT, character("a")),
    ),
    (
        Action::CommandPalette,
        Chord::new(CTRL_SHIFT, character("p")),
    ),
    (
        Action::SplitHorizontal,
        Chord::new(ALT_SHIFT, character("-")),
    ),
    (Action::SplitVertical, Chord::new(ALT_SHIFT, character("="))),
    (
        Action::DuplicatePaneSplit,
        Chord::new(CTRL_SHIFT, character("d")),
    ),
    (Action::OpenSettings, Chord::new(CTRL, character(","))),
];

const fn character(text: &'static str) -> ChordKey {
    ChordKey::Character(text)
}

/// Resolve a key press to an action.
///
/// `logical` is the key winit produced (Shift already applied); `base` is the same physical key
/// with every modifier stripped, from `KeyEventExtModifierSupplement::key_without_modifiers`.
pub(crate) fn lookup_action(
    logical: &Key,
    base: &Key,
    modifiers: ModifiersState,
) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|(_, chord)| chord.matches(logical, base, modifiers))
        .map(|(action, _)| *action)
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
    fn press(key: Key, modifiers: ModifiersState) -> Option<Action> {
        lookup_action(&key, &key, modifiers)
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
        assert_eq!(
            press(character("d"), CTRL_SHIFT),
            Some(Action::DuplicatePaneSplit)
        );
        assert_eq!(press(character(","), CTRL), Some(Action::OpenSettings));
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
        // US layout: Shift+1 produces "!", the bare key is "1".
        assert_eq!(
            lookup_action(&character("!"), &character("1"), CTRL_SHIFT),
            Some(Action::GotoTab(1))
        );
        // A layout that reaches the digit through Shift produces "1" with a different bare key.
        assert_eq!(
            lookup_action(&character("1"), &character("&"), CTRL_SHIFT),
            Some(Action::GotoTab(1))
        );
        // US layout: Shift+- produces "_", Shift+= produces "+".
        assert_eq!(
            lookup_action(&character("_"), &character("-"), ALT_SHIFT),
            Some(Action::SplitHorizontal)
        );
        assert_eq!(
            lookup_action(&character("+"), &character("="), ALT_SHIFT),
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
        for text in ["a", "d", "e", "n", "p", "t", "w", "-", "=", ",", "1", "9"] {
            assert_eq!(press(character(text), ctrl_alt), None, "AltGr+{text}");
            assert_eq!(
                press(character(text), ctrl_alt_shift),
                None,
                "AltGr+Shift+{text}"
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
        for text in ["a", "n", "w", "t", "d", "p", "-", "=", ",", "1", "9"] {
            assert_eq!(press(character(text), ModifiersState::empty()), None);
            assert_eq!(press(character(text), ModifiersState::SHIFT), None);
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
    #[test]
    fn bare_control_letters_stay_with_the_terminal() {
        for letter in 'a'..='z' {
            assert_eq!(
                press(character(&letter.to_string()), CTRL),
                None,
                "Ctrl+{letter} belongs to the terminal"
            );
        }
    }

    #[test]
    fn the_table_holds_exactly_the_ruled_rows_and_no_chord_is_claimed_twice() {
        // 11 single actions plus GotoTab(1..=9).
        assert_eq!(BINDINGS.len(), 20);

        for (index, (_, chord)) in BINDINGS.iter().enumerate() {
            for (other_action, other_chord) in BINDINGS.iter().skip(index + 1) {
                assert_ne!(
                    chord, other_chord,
                    "{other_action:?} reuses a chord already claimed above it"
                );
            }
        }

        for (action, _) in BINDINGS {
            assert!(
                !matches!(action, Action::GotoTab(ordinal) if !(1..=9).contains(ordinal)),
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
            Action::OpenSettings,
        ];
        expected.extend((1..=9u8).map(Action::GotoTab));

        for action in expected {
            assert!(
                BINDINGS.iter().any(|(bound, _)| *bound == action),
                "{action:?} has no default chord"
            );
        }
    }

    /// Each table row must be reachable through the same lookup dispatch uses.
    #[test]
    fn every_table_row_round_trips_through_lookup() {
        for (action, chord) in BINDINGS {
            let key = match chord.key {
                ChordKey::Character(text) => Key::Character(text.into()),
                ChordKey::Named(named) => Key::Named(named),
            };
            assert_eq!(
                lookup_action(&key, &key, chord.modifiers),
                Some(*action),
                "{action:?} is in the table but unreachable"
            );
        }
    }
}
