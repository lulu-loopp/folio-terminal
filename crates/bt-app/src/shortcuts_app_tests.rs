//! **`shortcuts`, as the application drives it.** Tests whose first assertion is about
//! `shortcuts`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use winit::keyboard::Key;

/// N25 — **the scaffold is retired and its chord belongs to nobody.**
///
/// `Ctrl+Alt+Shift+P` opened and closed the preview seat while the block had
/// no verbs that could. Every one of them exists now, so a chord that
/// duplicated them would be a second way into the same state — and it wore
/// Alt, which the shortcut audit rules off limits precisely because AltGr
/// produces `Ctrl+Alt` on the layouts a European user types on.
///
/// Red gate: put the matcher back and `p` under that chord stops reaching
/// the shell. Asserted through the registry rather than against a deleted
/// function, because "no code claims it" is the property, and the registry
/// is the only thing left that could.
#[test]
fn the_retired_preview_chord_reaches_the_shell_like_any_other_key() {
    let key = Key::Character("p".into());
    let table = shortcuts::Shortcuts::defaults();
    let claimed = |modifiers| {
        table.lookup(
            &key,
            &key,
            modifiers,
            shortcuts::Focus {
                preview: false,
                terminal_primary: true,
                terminal: true,
                search_open: false,
                web_page: false,
            },
        )
    };
    let in_preview = |modifiers| {
        table.lookup(
            &key,
            &key,
            modifiers,
            shortcuts::Focus {
                preview: true,
                terminal_primary: false,
                terminal: false,
                search_open: false,
                web_page: false,
            },
        )
    };
    for modifiers in [
        ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SHIFT,
        // A bare Ctrl+P is DLE and must keep reaching the child.
        ModifiersState::CONTROL,
        ModifiersState::CONTROL | ModifiersState::ALT,
    ] {
        assert!(
            claimed(modifiers).is_none() && in_preview(modifiers).is_none(),
            "{modifiers:?}+P is claimed by nothing, in either focus"
        );
    }
    // And the chord the scaffold deliberately stood aside from belongs to
    // the command palette again (DESIGN.md §7.55): its row left `BINDINGS`
    // for the v0.1 preview because the verb behind it did not exist yet, and
    // returned in v0.2 with the verb in hand. The scaffold wore Alt to keep
    // off this chord; what retired the scaffold is that Alt was never free
    // either, and that reasoning is untouched in either direction.
    assert_eq!(
        claimed(ModifiersState::CONTROL | ModifiersState::SHIFT),
        Some(shortcuts::Action::CommandPalette),
        "Ctrl+Shift+P is the palette's - see shortcuts::Action::CommandPalette"
    );
}

/// The macOS menu memo is decided entirely from inputs. An unchanged turn is
/// rejected here, before plan construction and before any AppKit call.
#[test]
fn unchanged_main_menu_inputs_are_silent_before_appkit() {
    let shortcuts = shortcuts::Shortcuts::defaults();
    let focus = Some(shortcuts::Focus::default());
    let memo = MainMenuInputs::new(&shortcuts, focus);
    assert!(memo.matches(&shortcuts, focus));

    let mut rebound = shortcuts.clone();
    rebound.set("new-tab", None);
    assert!(
        !memo.matches(&rebound, focus),
        "a rebound chord rebuilds the menu"
    );
    assert!(
        !memo.matches(&shortcuts, None),
        "a focus change rebuilds the menu"
    );

    let stale_language = MainMenuInputs {
        language_revision: memo.language_revision.wrapping_sub(1),
        ..memo
    };
    assert!(
        !stale_language.matches(&shortcuts, focus),
        "a language revision rebuilds the menu",
    );
}
