//! **`input`, as the application drives it.** Tests whose first assertion is about
//! `input`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::a_shell;
use winit::keyboard::{Key, NamedKey};

#[test]
fn keyboard_mapping_carries_the_layouts_letters_and_preserves_terminal_controls() {
    assert_eq!(
        input::legacy_bytes(
            &Key::Character("hello".into()),
            ModifiersState::empty(),
            false
        ),
        Some(b"hello".to_vec())
    );
    assert_eq!(
        input::legacy_bytes(&Key::Named(NamedKey::Enter), ModifiersState::empty(), false),
        Some(vec![b'\r'])
    );
    assert_eq!(
        input::legacy_bytes(
            &Key::Named(NamedKey::Backspace),
            ModifiersState::empty(),
            false
        ),
        Some(vec![0x7f])
    );
    assert_eq!(
        input::legacy_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty(), false),
        Some(vec![b' '])
    );
    assert_eq!(
        input::legacy_bytes(&Key::Character("c".into()), ModifiersState::CONTROL, false),
        Some(vec![0x03])
    );
    // Every bare `Ctrl+letter` is the shell's and is sent as its control
    // code (2026-08-17: `^X` used to be swallowed here, and with it `^B`,
    // `^L`, `^R` — the whole readline alphabet the shortcut table promised
    // to leave alone).
    assert_eq!(
        input::legacy_bytes(&Key::Character("x".into()), ModifiersState::CONTROL, false),
        Some(vec![0x18])
    );
    // **And a character outside ASCII is bytes like any other** (M1-7, X-3
    // §4 ⑤). This case asserted `None` until 2026-09-12, and what it was
    // really encoding was a guard (`text.is_ascii()`) that was supposed to
    // stop a *composed* character being typed twice. It did not need to: a
    // composition never arrives as a key event on either platform — winit
    // drops an IME's `WM_CHAR` here because no key event stands under it, and
    // on macOS a commit arrives as `Ime::Commit` with no `KeyboardInput` at
    // all (X-3 measured `你好` once, six bytes). What the guard did instead
    // was swallow `ü ä ö ß` on a German layout, on both platforms, whole.
    assert_eq!(
        input::legacy_bytes(&Key::Character("中".into()), ModifiersState::empty(), false),
        Some("中".as_bytes().to_vec())
    );
    assert_eq!(
        input::legacy_bytes(&Key::Character("ü".into()), ModifiersState::empty(), false),
        Some("ü".as_bytes().to_vec())
    );
    assert_eq!(
        input::legacy_bytes(
            &Key::Named(NamedKey::Process),
            ModifiersState::CONTROL,
            false
        ),
        None
    );
}

#[test]
fn bracketed_paste_follows_vendor_decset_and_normalizes_crlf() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(2).unwrap());

    session.feed(b"\x1b[?2004h").unwrap();
    assert_eq!(
        input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
        b"\x1b[200~one\rtwo\r\x1b[201~"
    );

    session.feed(b"\x1b[?2004l").unwrap();
    assert_eq!(
        input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
        b"one\rtwo\r"
    );
}

#[test]
fn a_tracked_pane_keeps_a_press_on_an_ordinary_cell_and_shift_still_takes_it_back() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
    let hit = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    let forwarded = route_forwarded_mouse_button(
        &mut route,
        ElementState::Pressed,
        input::MouseProtocolButton::Left,
        hit,
        session.terminal_modes(),
        ModifiersState::CONTROL,
        PressedCellTarget::Ordinary,
        a_shell(),
    );
    assert!(forwarded.is_some());
    assert!(matches!(route, Some(MouseRoute::Forward { .. })));

    let mut shifted_route = None;
    assert!(
        route_forwarded_mouse_button(
            &mut shifted_route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
            PressedCellTarget::Ordinary,
            a_shell(),
        )
        .is_none()
    );
    assert!(shifted_route.is_none());
}
