use winit::keyboard::{Key, ModifiersState, NamedKey};

const CSI: &[u8] = b"\x1b[";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MouseProtocolButton {
    Left,
    Middle,
    Right,
    None,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MouseProtocolEvent {
    Press,
    Release,
    Motion,
}

/// **`Ctrl+V` and `Ctrl+Shift+V`, and `Shift+Insert`.**
///
/// The shifted spelling is here because [`is_copy_shortcut`] has always carried
/// its own (gesture audit 2026-08-26, 附 ①). This asked for `modifiers ==
/// CONTROL` *exactly*, so a hand that pressed `Ctrl+Shift+C` to copy and
/// `Ctrl+Shift+V` to paste — the pair Windows Terminal ships — got the copy and
/// then handed the shell `^V` (0x16) for the paste. The predicate is written to
/// mirror its partner rather than to a modifier policy of its own: the two
/// answer the same question about the same hand, and a pair that disagrees
/// about `Shift` is the bug this is fixing, not a second one to introduce.
pub(crate) fn is_paste_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    let ctrl_v = modifiers.control_key()
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("v"));
    let shift_insert =
        modifiers == ModifiersState::SHIFT && matches!(key, Key::Named(NamedKey::Insert));
    ctrl_v || shift_insert
}

/// **`Ctrl+C`, `Ctrl+Shift+C`, and `Ctrl+Insert`.**
///
/// `Ctrl+Insert` is the older of the two Windows clipboard pairs and this window
/// answered only its paste half — `Shift+Insert` — while the copy half was
/// encoded straight through to the child as `\x1b[2;5~` (gesture audit
/// 2026-08-26, 附 ②). It is matched on exact modifiers, the way `Shift+Insert`
/// is: the `Insert` family has no `Shift`-forces-it spelling, and `Ctrl+Alt` on
/// this key is AltGr ground.
///
/// Whether a press that *is* one of these actually copies is
/// [`should_copy_selection`]'s question, not this one's.
pub(crate) fn is_copy_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    let ctrl_c = modifiers.control_key()
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("c"));
    let ctrl_insert =
        modifiers == ModifiersState::CONTROL && matches!(key, Key::Named(NamedKey::Insert));
    ctrl_c || ctrl_insert
}

/// Whether a clipboard-shaped press should copy rather than reach the child.
///
/// `Shift` forces the answer and a selection earns it. That is what splits
/// `Ctrl+Shift+C` (always a copy) from `Ctrl+C` (an interrupt with nothing
/// selected) — and it puts `Ctrl+Insert` on `Ctrl+C`'s side, because the
/// `Insert` family has no shifted spelling of the copy: with a selection it
/// copies, with none the key stays the child's, which is the same trade `^C`
/// makes and the reason a full-screen program that binds `Insert` keeps it.
pub(crate) fn should_copy_selection(
    key: &Key,
    modifiers: ModifiersState,
    has_selection: bool,
) -> bool {
    is_copy_shortcut(key, modifiers) && (modifiers.shift_key() || has_selection)
}

/// One mouse event in SGR 1006, one-based.
///
/// **`row` and `column` are the child's grid coordinates and never this window's**
/// (`docs/plans/horizontal-scroll/plan.md` §5.5). They arrive from
/// `ViewportFrame::live_point_at` by way of `live_viewport_mouse_hit`, which is the one place a
/// drawn cell is turned into a grid cell; a viewport column reaching here would tell a
/// mouse-tracking program the column the pointer was painted in, and every program that draws by
/// coordinate would answer somewhere else.
pub(crate) fn sgr_mouse_bytes(
    button: MouseProtocolButton,
    event: MouseProtocolEvent,
    row: u32,
    column: u32,
    modifiers: ModifiersState,
) -> Vec<u8> {
    let mut code = match button {
        MouseProtocolButton::Left => 0,
        MouseProtocolButton::Middle => 1,
        MouseProtocolButton::Right => 2,
        MouseProtocolButton::None => 3,
        MouseProtocolButton::WheelUp => 64,
        MouseProtocolButton::WheelDown => 65,
    };
    code += 4 * u8::from(modifiers.shift_key())
        + 8 * u8::from(modifiers.alt_key())
        + 16 * u8::from(modifiers.control_key());
    if event == MouseProtocolEvent::Motion {
        code += 32;
    }
    let suffix = if event == MouseProtocolEvent::Release {
        'm'
    } else {
        'M'
    };
    format!("\x1b[<{code};{};{}{suffix}", column + 1, row + 1).into_bytes()
}

/// The same event in whichever encoding the application asked for. `row` and `column` are the
/// child's grid coordinates — see [`sgr_mouse_bytes`].
pub(crate) fn mouse_bytes(
    sgr: bool,
    button: MouseProtocolButton,
    event: MouseProtocolEvent,
    row: u32,
    column: u32,
    modifiers: ModifiersState,
) -> Vec<u8> {
    if sgr {
        return sgr_mouse_bytes(button, event, row, column, modifiers);
    }
    let mut code = if event == MouseProtocolEvent::Release {
        3
    } else {
        match button {
            MouseProtocolButton::Left => 0,
            MouseProtocolButton::Middle => 1,
            MouseProtocolButton::Right => 2,
            MouseProtocolButton::None => 3,
            MouseProtocolButton::WheelUp => 64,
            MouseProtocolButton::WheelDown => 65,
        }
    };
    code += 4 * u8::from(modifiers.shift_key())
        + 8 * u8::from(modifiers.alt_key())
        + 16 * u8::from(modifiers.control_key());
    if event == MouseProtocolEvent::Motion {
        code += 32;
    }
    // X10 coordinates are byte-limited; SGR 1006 is used whenever the application requests it.
    let x = column.saturating_add(1).min(223) as u8 + 32;
    let y = row.saturating_add(1).min(223) as u8 + 32;
    vec![0x1b, b'[', b'M', code + 32, x, y]
}

pub(crate) fn alternate_scroll_bytes(lines: i32, application_cursor_mode: bool) -> Vec<u8> {
    let key = if lines >= 0 {
        NamedKey::ArrowUp
    } else {
        NamedKey::ArrowDown
    };
    let one = keyboard_bytes(
        &Key::Named(key),
        ModifiersState::empty(),
        application_cursor_mode,
    )
    .expect("arrow keys always encode");
    one.repeat(lines.unsigned_abs() as usize)
}

pub(crate) fn is_ime_owned_key(key: &Key, modifiers: ModifiersState) -> bool {
    is_paste_shortcut(key, modifiers)
        || matches!(
            key,
            Key::Named(
                NamedKey::ArrowUp
                    | NamedKey::ArrowDown
                    | NamedKey::ArrowLeft
                    | NamedKey::ArrowRight
                    | NamedKey::Home
                    | NamedKey::End
                    | NamedKey::Delete
                    | NamedKey::Insert
                    | NamedKey::PageUp
                    | NamedKey::PageDown
                    | NamedKey::Backspace
                    | NamedKey::Enter
                    | NamedKey::Escape
                    | NamedKey::Tab
            )
        )
}

pub(crate) fn keyboard_bytes(
    key: &Key,
    modifiers: ModifiersState,
    application_cursor_mode: bool,
) -> Option<Vec<u8>> {
    // Spike 04's hard rule: Process is tested only on logical_key. Physical Backspace/Escape is
    // still present during composition and must never leak into the shell.
    if matches!(key, Key::Named(NamedKey::Process)) || is_paste_shortcut(key, modifiers) {
        return None;
    }
    if modifiers.control_key()
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("c"))
    {
        return Some(vec![0x03]);
    }
    // **The rest of the control alphabet** (user report, 2026-08-17: Claude
    // Code's `Ctrl+B` never arrived). Every `Ctrl+<letter>` the shortcut table
    // leaves alone is the shell's — that is the whole of discipline ①, and it
    // was true of the *table* while the encoder here knew only `^C`: `^B`,
    // `^D`, `^L`, `^R`, `^U`, `^W`, `^Z` all fell to `None` and were dropped on
    // the floor. A terminal that swallows readline's alphabet is not leaving
    // it to the shell. The byte is the ASCII control code (`letter & 0x1f`),
    // the same for upper and lower case as every terminal since the VT100;
    // `Ctrl+@`/`Ctrl+Space` is NUL and `[ \ ] ^ _` give 0x1b–0x1f, and Alt on
    // top prefixes ESC as it does for a plain character. winit may report the
    // key either as the letter or as the control character it produces
    // (`"\u{2}"`), depending on layout and Ctrl handling — both spellings are
    // read here so the answer does not depend on which one arrived.
    if modifiers.control_key()
        && let Key::Character(text) = key
        && let Some(byte) = control_byte(text)
    {
        return Some(meta_prefix(&[byte], modifiers.alt_key()));
    }

    let modifier = xterm_modifier(modifiers);
    match key {
        Key::Named(NamedKey::ArrowUp) => Some(cursor_key(b'A', modifier, application_cursor_mode)),
        Key::Named(NamedKey::ArrowDown) => {
            Some(cursor_key(b'B', modifier, application_cursor_mode))
        }
        Key::Named(NamedKey::ArrowRight) => {
            Some(cursor_key(b'C', modifier, application_cursor_mode))
        }
        Key::Named(NamedKey::ArrowLeft) => {
            Some(cursor_key(b'D', modifier, application_cursor_mode))
        }
        Key::Named(NamedKey::Home) => Some(cursor_key(b'H', modifier, application_cursor_mode)),
        Key::Named(NamedKey::End) => Some(cursor_key(b'F', modifier, application_cursor_mode)),
        Key::Named(NamedKey::Insert) => Some(tilde_key(2, modifier)),
        Key::Named(NamedKey::Delete) => Some(tilde_key(3, modifier)),
        Key::Named(NamedKey::PageUp) => Some(tilde_key(5, modifier)),
        Key::Named(NamedKey::PageDown) => Some(tilde_key(6, modifier)),
        Key::Named(NamedKey::Tab) if modifiers.shift_key() => Some(b"\x1b[Z".to_vec()),
        Key::Character(text)
            if (text.is_ascii() || modifiers.alt_key())
                && text.chars().all(|character| !character.is_control())
                && !modifiers.control_key() =>
        {
            Some(meta_prefix(text.as_bytes(), modifiers.alt_key()))
        }
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Backspace) => Some(vec![0x7f]),
        Key::Named(NamedKey::Tab) => Some(vec![b'\t']),
        Key::Named(NamedKey::Escape) => Some(vec![0x1b]),
        // winit reports the text-producing space key as Named rather than Character.
        Key::Named(NamedKey::Space) if !modifiers.control_key() => {
            Some(meta_prefix(b" ", modifiers.alt_key()))
        }
        _ => None,
    }
}

fn xterm_modifier(modifiers: ModifiersState) -> u8 {
    1 + u8::from(modifiers.shift_key())
        + 2 * u8::from(modifiers.alt_key())
        + 4 * u8::from(modifiers.control_key())
}

fn cursor_key(final_byte: u8, modifier: u8, application_cursor_mode: bool) -> Vec<u8> {
    if modifier == 1 {
        let mut bytes = if application_cursor_mode {
            b"\x1bO".to_vec()
        } else {
            CSI.to_vec()
        };
        bytes.push(final_byte);
        bytes
    } else {
        format!("\x1b[1;{modifier}{}", char::from(final_byte)).into_bytes()
    }
}

fn tilde_key(number: u8, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1b[{number}~").into_bytes()
    } else {
        format!("\x1b[{number};{modifier}~").into_bytes()
    }
}

/// The control byte a `Ctrl`-held character key produces, or `None` for a key
/// that has no VT control code (a digit, punctuation outside `[\\]^_@`, a
/// non-ASCII character — those keep falling through, which for the digits is
/// what xterm does too without `modifyOtherKeys`).
fn control_byte(text: &str) -> Option<u8> {
    let mut chars = text.chars();
    let character = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let byte = match character {
        // Already the control character — some layouts hand the produced code.
        c if (c as u32) < 0x20 => c as u8,
        'a'..='z' | 'A'..='Z' => (character.to_ascii_uppercase() as u8) & 0x1f,
        '@' | ' ' => 0x00,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        '?' => 0x7f,
        _ => return None,
    };
    Some(byte)
}

fn meta_prefix(bytes: &[u8], alt: bool) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(bytes.len() + usize::from(alt));
    if alt {
        encoded.push(0x1b);
    }
    encoded.extend_from_slice(bytes);
    encoded
}

pub(crate) fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let sanitized = sanitize_paste(text);
    if !bracketed {
        return sanitized;
    }

    let mut bytes = Vec::with_capacity(
        BRACKETED_PASTE_START.len() + sanitized.len() + BRACKETED_PASTE_END.len(),
    );
    bytes.extend_from_slice(BRACKETED_PASTE_START);
    bytes.extend_from_slice(&sanitized);
    bytes.extend_from_slice(BRACKETED_PASTE_END.as_bytes());
    bytes
}

fn sanitize_paste(text: &str) -> Vec<u8> {
    // Remove the complete terminator before generic control filtering. Merely removing ESC would
    // leave a misleading printable "[201~" fragment and weakens later policy changes.
    let without_terminators = text.replace(BRACKETED_PASTE_END, "");
    let mut normalized = String::with_capacity(without_terminators.len());
    let mut characters = without_terminators.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push('\r');
            }
            '\n' => normalized.push('\r'),
            '\t' => normalized.push('\t'),
            character if !character.is_control() => normalized.push(character),
            _ => {}
        }
    }
    normalized.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODIFIERS: [ModifiersState; 8] = [
        ModifiersState::empty(),
        ModifiersState::SHIFT,
        ModifiersState::ALT,
        ModifiersState::SHIFT.union(ModifiersState::ALT),
        ModifiersState::CONTROL,
        ModifiersState::SHIFT.union(ModifiersState::CONTROL),
        ModifiersState::ALT.union(ModifiersState::CONTROL),
        ModifiersState::SHIFT
            .union(ModifiersState::ALT)
            .union(ModifiersState::CONTROL),
    ];

    #[test]
    fn cursor_home_end_matrix_covers_decckm_and_every_xterm_modifier() {
        let keys = [
            (NamedKey::ArrowUp, b'A'),
            (NamedKey::ArrowDown, b'B'),
            (NamedKey::ArrowRight, b'C'),
            (NamedKey::ArrowLeft, b'D'),
            (NamedKey::Home, b'H'),
            (NamedKey::End, b'F'),
        ];

        for application_mode in [false, true] {
            for modifiers in MODIFIERS {
                let modifier = xterm_modifier(modifiers);
                for (key, final_byte) in keys {
                    let expected = if modifier == 1 && application_mode {
                        format!("\x1bO{}", char::from(final_byte))
                    } else if modifier == 1 {
                        format!("\x1b[{}", char::from(final_byte))
                    } else {
                        format!("\x1b[1;{modifier}{}", char::from(final_byte))
                    };
                    assert_eq!(
                        keyboard_bytes(&Key::Named(key), modifiers, application_mode),
                        Some(expected.into_bytes()),
                        "key={key:?} application_mode={application_mode} modifiers={modifiers:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn tilde_key_matrix_covers_both_modes_and_every_xterm_modifier() {
        let keys = [
            (NamedKey::Insert, 2),
            (NamedKey::Delete, 3),
            (NamedKey::PageUp, 5),
            (NamedKey::PageDown, 6),
        ];

        for application_mode in [false, true] {
            for modifiers in MODIFIERS {
                let modifier = xterm_modifier(modifiers);
                for (key, number) in keys {
                    // Exact Shift+Insert is the paste command and deliberately wins over encoding.
                    if key == NamedKey::Insert && modifiers == ModifiersState::SHIFT {
                        assert!(is_paste_shortcut(&Key::Named(key), modifiers));
                        continue;
                    }
                    let expected = if modifier == 1 {
                        format!("\x1b[{number}~")
                    } else {
                        format!("\x1b[{number};{modifier}~")
                    };
                    assert_eq!(
                        keyboard_bytes(&Key::Named(key), modifiers, application_mode),
                        Some(expected.into_bytes()),
                        "key={key:?} application_mode={application_mode} modifiers={modifiers:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn tab_meta_and_legacy_controls_have_terminal_encodings() {
        assert_eq!(
            keyboard_bytes(&Key::Named(NamedKey::Tab), ModifiersState::SHIFT, false),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("x".into()), ModifiersState::ALT, false),
            Some(b"\x1bx".to_vec())
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("é".into()), ModifiersState::ALT, false),
            Some("\u{1b}é".as_bytes().to_vec())
        );
        assert_eq!(
            keyboard_bytes(&Key::Named(NamedKey::Space), ModifiersState::ALT, false),
            Some(b"\x1b ".to_vec())
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("c".into()), ModifiersState::CONTROL, false),
            Some(vec![0x03])
        );
    }

    /// PIN (user report, 2026-08-17) — **the whole control alphabet reaches
    /// the shell, not only `^C`.** `Ctrl+B` is Claude Code's "run in
    /// background", `Ctrl+L` clears, `Ctrl+R` searches history, `Ctrl+D` ends
    /// input; the shortcut table leaves every bare `Ctrl+letter` to the shell,
    /// and the encoder must then actually send it. Both spellings winit may
    /// use are read; case does not matter; Alt prefixes ESC.
    #[test]
    fn every_bare_control_letter_is_sent_as_its_control_code() {
        for (letter, code) in [
            ("b", 0x02u8),
            ("B", 0x02),
            ("d", 0x04),
            ("l", 0x0c),
            ("r", 0x12),
            ("z", 0x1a),
            ("a", 0x01),
        ] {
            assert_eq!(
                keyboard_bytes(
                    &Key::Character(letter.into()),
                    ModifiersState::CONTROL,
                    false
                ),
                Some(vec![code]),
                "Ctrl+{letter}"
            );
        }
        // The layout that reports the produced control character.
        assert_eq!(
            keyboard_bytes(
                &Key::Character("\u{2}".into()),
                ModifiersState::CONTROL,
                false
            ),
            Some(vec![0x02])
        );
        // The punctuation with a code, and one without.
        assert_eq!(
            keyboard_bytes(&Key::Character("[".into()), ModifiersState::CONTROL, false),
            Some(vec![0x1b])
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("_".into()), ModifiersState::CONTROL, false),
            Some(vec![0x1f])
        );
        assert_eq!(
            keyboard_bytes(&Key::Character("1".into()), ModifiersState::CONTROL, false),
            None
        );
        // Alt on top prefixes ESC.
        assert_eq!(
            keyboard_bytes(
                &Key::Character("b".into()),
                ModifiersState::CONTROL | ModifiersState::ALT,
                false
            ),
            Some(vec![0x1b, 0x02])
        );
        // Ctrl+V stays the paste door and is not encoded here.
        assert_eq!(
            keyboard_bytes(&Key::Character("v".into()), ModifiersState::CONTROL, false),
            None
        );
    }

    #[test]
    fn paste_shortcuts_are_commands_and_preedit_owns_editing_keys() {
        assert!(is_paste_shortcut(
            &Key::Character("v".into()),
            ModifiersState::CONTROL
        ));
        assert!(is_paste_shortcut(
            &Key::Named(NamedKey::Insert),
            ModifiersState::SHIFT
        ));
        assert!(is_ime_owned_key(
            &Key::Named(NamedKey::ArrowLeft),
            ModifiersState::CONTROL
        ));
        assert!(is_ime_owned_key(
            &Key::Named(NamedKey::Delete),
            ModifiersState::empty()
        ));
        assert!(!is_ime_owned_key(
            &Key::Character("a".into()),
            ModifiersState::empty()
        ));
    }

    #[test]
    fn paste_normalizes_newlines_filters_controls_and_strips_injected_terminators() {
        assert_eq!(
            paste_bytes("one\r\ntwo\nthree\rfour\tend", false),
            b"one\rtwo\rthree\rfour\tend"
        );
        assert_eq!(
            paste_bytes("safe\x1b[201~tail\0\u{0007}", false),
            b"safetail"
        );
    }

    #[test]
    fn bracketed_paste_wraps_only_after_sanitizing_payload() {
        assert_eq!(
            paste_bytes("one\n\x1b[201~two", true),
            b"\x1b[200~one\rtwo\x1b[201~"
        );
        assert_eq!(paste_bytes("one\n", false), b"one\r");
    }

    #[test]
    fn sgr_mouse_encodes_one_based_coordinates_modifiers_motion_and_release() {
        assert_eq!(
            sgr_mouse_bytes(
                MouseProtocolButton::Left,
                MouseProtocolEvent::Press,
                2,
                4,
                ModifiersState::empty(),
            ),
            b"\x1b[<0;5;3M"
        );
        assert_eq!(
            sgr_mouse_bytes(
                MouseProtocolButton::Right,
                MouseProtocolEvent::Motion,
                0,
                0,
                ModifiersState::SHIFT.union(ModifiersState::CONTROL),
            ),
            b"\x1b[<54;1;1M"
        );
        assert_eq!(
            sgr_mouse_bytes(
                MouseProtocolButton::Middle,
                MouseProtocolEvent::Release,
                9,
                7,
                ModifiersState::ALT,
            ),
            b"\x1b[<9;8;10m"
        );
    }

    #[test]
    fn legacy_mouse_fallback_keeps_non_sgr_mouse_modes_operable() {
        assert_eq!(
            mouse_bytes(
                false,
                MouseProtocolButton::Left,
                MouseProtocolEvent::Press,
                2,
                4,
                ModifiersState::empty(),
            ),
            vec![0x1b, b'[', b'M', 32, 37, 35]
        );
    }

    #[test]
    fn alternate_screen_wheel_uses_cursor_mode_arrow_bytes() {
        assert_eq!(alternate_scroll_bytes(2, false), b"\x1b[A\x1b[A");
        assert_eq!(alternate_scroll_bytes(-1, true), b"\x1bOB");
    }

    #[test]
    fn ctrl_c_is_interrupt_without_selection_but_copy_with_selection_or_shift() {
        let key = Key::Character("c".into());
        assert!(!should_copy_selection(&key, ModifiersState::CONTROL, false));
        assert!(should_copy_selection(&key, ModifiersState::CONTROL, true));
        assert!(should_copy_selection(
            &key,
            ModifiersState::CONTROL.union(ModifiersState::SHIFT),
            false,
        ));
        assert_eq!(
            keyboard_bytes(&key, ModifiersState::CONTROL, false),
            Some(vec![0x03])
        );
    }

    /// RED (gesture audit 2026-08-26, 附 ①) — **`Ctrl+Shift+V` pastes, because
    /// `Ctrl+Shift+C` copies.**
    ///
    /// The predicate asked for `modifiers == CONTROL` *exactly*, so the shifted
    /// half of the pair fell past it into the encoder and the child was handed
    /// `^V` (0x16). Half a pair is worse than neither: a hand that learned both
    /// on Windows Terminal presses both, and the failing half looks like a
    /// clipboard that lost the text rather than like a chord this window does
    /// not take.
    ///
    /// MUTATION: put the `==` back and the first assertion goes red.
    #[test]
    fn the_shifted_clipboard_pair_is_whole() {
        let paste = Key::Character("v".into());
        let copy = Key::Character("c".into());
        let ctrl_shift = ModifiersState::CONTROL.union(ModifiersState::SHIFT);
        assert!(is_paste_shortcut(&paste, ctrl_shift));
        assert!(should_copy_selection(&copy, ctrl_shift, false));
        // winit reports the shifted letter in upper case on most layouts.
        assert!(is_paste_shortcut(&Key::Character("V".into()), ctrl_shift));
        // And the shifted paste never reaches the child as `^V`.
        assert_eq!(keyboard_bytes(&paste, ctrl_shift, false), None);
        // The unshifted half is untouched.
        assert!(is_paste_shortcut(&paste, ModifiersState::CONTROL));
    }

    /// RED (gesture audit 2026-08-26, 附 ②) — **`Ctrl+Insert` copies, because
    /// `Shift+Insert` pastes.**
    ///
    /// The older of the two Windows clipboard pairs, and this window answered
    /// only its paste half; `Ctrl+Insert` was encoded straight through as
    /// `\x1b[2;5~` and the word appeared in no user-visible string in the
    /// repository.
    ///
    /// It answers on `Ctrl+C`'s terms and not `Ctrl+Shift+C`'s: **with a
    /// selection it copies, with none it stays the child's.** `Insert` is a key
    /// full-screen programs bind, and a copy of nothing is not a reason to take
    /// it from them — the same trade `Ctrl+C` makes with `^C`.
    ///
    /// MUTATION: drop the `Insert` arm of [`is_copy_shortcut`] and the first
    /// assertion goes red.
    #[test]
    fn ctrl_insert_copies_a_selection_and_stays_the_child_s_otherwise() {
        let insert = Key::Named(NamedKey::Insert);
        assert!(should_copy_selection(
            &insert,
            ModifiersState::CONTROL,
            true
        ));
        assert!(!should_copy_selection(
            &insert,
            ModifiersState::CONTROL,
            false
        ));
        // It is a copy and never a paste — the pair's other half is Shift.
        assert!(!is_paste_shortcut(&insert, ModifiersState::CONTROL));
        assert!(is_paste_shortcut(&insert, ModifiersState::SHIFT));
        assert_eq!(
            keyboard_bytes(&insert, ModifiersState::CONTROL, false),
            Some(b"\x1b[2;5~".to_vec()),
            "with nothing selected the key is still the child's"
        );
    }
}
