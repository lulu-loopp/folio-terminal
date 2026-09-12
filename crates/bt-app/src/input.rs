use bt_platform::HostPlatform;
use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState, NamedKey};

// ── The routing rule, and the one sentence it is (M1-7, probe X-3 §4) ───────
//
// **Command is the application's, Control is the terminal's, and no verb wears
// both.** Everything in this module that asks about a modifier asks it through
// the three predicates below, so that the rule is one sentence in one place
// rather than a modifier test at every door. On Windows and on a Unix that is
// not a Mac the application's modifier *is* Control, which is why the same three
// predicates leave that dialect byte for byte where it was: `is_command_chord`
// answers `control_key()` there, and `Ctrl+C` goes on being an interrupt with
// nothing selected exactly as it was in v0.1.

/// **Whether this press wears the modifier that makes a chord the
/// application's** rather than the child's or the text's.
///
/// Control here, Command on a Mac — and the two are not interchangeable in
/// either direction. `^C ^D ^Z` reach a child on macOS untouched (X-3 measured
/// all three), so Control cannot be this window's modifier there; and Command
/// has no control code to take, which is what lets the macOS column of
/// `shortcuts::BINDINGS` claim a bare `Cmd+F` where the Windows column had to
/// argue for one.
#[must_use]
pub(crate) fn is_command_chord(modifiers: ModifiersState) -> bool {
    is_command_chord_on(modifiers, bt_platform::host_platform())
}

/// The same on a named platform, so a test on either machine can ask about the
/// other — `bt_platform::host_platform`'s own argument for being a value.
#[must_use]
pub(crate) fn is_command_chord_on(modifiers: ModifiersState, platform: HostPlatform) -> bool {
    match platform {
        HostPlatform::MacOs => modifiers.super_key(),
        HostPlatform::Windows | HostPlatform::OtherUnix => modifiers.control_key(),
    }
}

/// **Whether this press wears the modifier that belongs to the child** — the
/// other one, and the exact complement of [`is_command_chord`].
///
/// Super here, Control on a Mac. On this platform nothing is behind the Windows
/// key but another program's verb (§7.54d); there it is the whole control-code
/// alphabet. A field that has a verb for the application's modifier asks both
/// questions, because the answer to "is this mine" is not the answer to "is this
/// text".
#[must_use]
pub(crate) fn is_terminal_chord(modifiers: ModifiersState) -> bool {
    is_terminal_chord_on(modifiers, bt_platform::host_platform())
}

/// The same on a named platform. See [`is_command_chord_on`].
#[must_use]
pub(crate) fn is_terminal_chord_on(modifiers: ModifiersState, platform: HostPlatform) -> bool {
    match platform {
        HostPlatform::MacOs => modifiers.control_key(),
        HostPlatform::Windows | HostPlatform::OtherUnix => modifiers.super_key(),
    }
}

/// **The application's modifier held on its own**, which is what a field asks
/// before it runs one of its own verbs (select all, copy, cut, paste).
///
/// Neither Alt nor the other platform's modifier alongside it. Alt is out
/// because AltGr arrives as `Ctrl+Alt` on this platform and is *typing* — the
/// exemption `preview_edit::command` and `rename_key` both already make — and
/// the other modifier is out because a chord wearing both is a chord aimed at
/// neither of the two things this window could do with it.
#[must_use]
pub(crate) fn is_command_chord_alone(modifiers: ModifiersState) -> bool {
    is_command_chord(modifiers) && !modifiers.alt_key() && !is_terminal_chord(modifiers)
}

/// **Whether a press is typing at all**, which is the question every one-line
/// field in this window has to ask before it inserts a character.
///
/// Neither Control nor Super, on either platform and in both directions. X-3
/// called the missing half of this the loudest defect it found and it is not
/// macOS-specific: six fields guarded `ctrl` and `alt` and never `super`, so
/// `Cmd+C` typed a `c` into whatever held the caret on a Mac — and `Win+C` did
/// the same here, which nobody had noticed because the chord usually leaves for
/// the shell first. A field asks this, not `is_command_chord`: the modifier that
/// is not this application's on a given platform is the *terminal's*, and a
/// terminal's chord is no more a character than an application's is.
///
/// Alt is deliberately not in the sentence. It composes text on both platforms —
/// AltGr arrives as `Ctrl+Alt` on Windows and Option is text on a Mac under Q9 —
/// and each field already says for itself what it does with it.
#[must_use]
pub(crate) fn types_a_character(modifiers: ModifiersState) -> bool {
    !modifiers.control_key() && !modifiers.super_key()
}

/// **The modifiers as this window means them**, which on one platform is not
/// quite what winit reported (M1-7, Q9's ruling made into code).
///
/// macOS hands an application *both* halves of the Option key: winit reports the
/// composed character **and** `alt_key()`, so X-3 measured `⌥a` arriving at the
/// pty as `ESC å` — neither Option-as-text nor Option-as-Alt but both at once.
/// `OptionAsAlt::None` at the window constructor settles what the *character*
/// is; this settles what the *modifier* is, and the two have to be settled
/// together or the same press means two things.
///
/// So when Option is text — the shipped answer — Alt is simply not held as far
/// as this window is concerned, and it is taken off here, once, at the door
/// every modifier state in this process comes through
/// (`WindowEvent::ModifiersChanged`). Downstream nothing changes and nothing
/// asks: the encoder does not prefix `ESC`, the search capsule's `Alt`-toggles
/// do not fire, a field inserts the character the layout produced, and the chord
/// table is not consulted about a modifier nobody is holding. With the setting
/// on, winit reports the raw letter instead and the Alt comes through untouched,
/// which is `ESC a` — the other policy, whole.
///
/// Off macOS this is the identity function and says so: Alt is Alt on a keyboard
/// with an Alt key on it.
#[must_use]
pub(crate) fn effective_modifiers(
    reported: ModifiersState,
    option_sends_alt: bool,
    platform: HostPlatform,
) -> ModifiersState {
    if platform == HostPlatform::MacOs && !option_sends_alt {
        reported.difference(ModifiersState::ALT)
    } else {
        reported
    }
}

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
/// The shifted spelling is here because [`is_copy_shortcut_on`] has always carried
/// its own (gesture audit 2026-08-26, 附 ①). This asked for `modifiers ==
/// CONTROL` *exactly*, so a hand that pressed `Ctrl+Shift+C` to copy and
/// `Ctrl+Shift+V` to paste — the pair Windows Terminal ships — got the copy and
/// then handed the shell `^V` (0x16) for the paste. The predicate is written to
/// mirror its partner rather than to a modifier policy of its own: the two
/// answer the same question about the same hand, and a pair that disagrees
/// about `Shift` is the bug this is fixing, not a second one to introduce.
/// **On macOS it is `Cmd+V`, and the `Insert` half is not there** (M1-7, X-3 §4
/// ②). The pair is the platform's, not the product's: no Apple keyboard has an
/// `Insert` key at all, so a spelling for it would be a promise about a key the
/// reader cannot press, and `Ctrl+V` there is `^V` — readline's quoted-insert —
/// and stays the child's.
pub(crate) fn is_paste_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    is_paste_shortcut_on(key, modifiers, bt_platform::host_platform())
}

/// The same on a named platform. See [`is_command_chord_on`] for why the
/// platform is a parameter.
pub(crate) fn is_paste_shortcut_on(
    key: &Key,
    modifiers: ModifiersState,
    platform: HostPlatform,
) -> bool {
    let command_v = is_command_chord_on(modifiers, platform)
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("v"));
    let shift_insert = platform != HostPlatform::MacOs
        && modifiers == ModifiersState::SHIFT
        && matches!(key, Key::Named(NamedKey::Insert));
    command_v || shift_insert
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
///
/// **On macOS it is `Cmd+C`, without the `Insert` half** — see
/// [`is_paste_shortcut`], whose note this one shares for the reason the two
/// predicates share everything: they answer the same question about the same
/// hand.
///
/// **It takes the platform and has no host-reading twin**, which its partner
/// does, and the difference is that nothing outside this module asks it: the one
/// caller is [`should_copy_selection_on`] one function down, which has the
/// platform in its hand already. A wrapper here would be a door with nobody
/// behind it.
pub(crate) fn is_copy_shortcut_on(
    key: &Key,
    modifiers: ModifiersState,
    platform: HostPlatform,
) -> bool {
    let command_c = is_command_chord_on(modifiers, platform)
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("c"));
    let ctrl_insert = platform != HostPlatform::MacOs
        && modifiers == ModifiersState::CONTROL
        && matches!(key, Key::Named(NamedKey::Insert));
    command_c || ctrl_insert
}

/// Whether a clipboard-shaped press should copy rather than reach the child.
///
/// `Shift` forces the answer and a selection earns it. That is what splits
/// `Ctrl+Shift+C` (always a copy) from `Ctrl+C` (an interrupt with nothing
/// selected) — and it puts `Ctrl+Insert` on `Ctrl+C`'s side, because the
/// `Insert` family has no shifted spelling of the copy: with a selection it
/// copies, with none the key stays the child's, which is the same trade `^C`
/// makes and the reason a full-screen program that binds `Insert` keeps it.
/// **On macOS the Shift clause is gone and the answer is simply yes** (M1-7,
/// X-3 §4 ②). Everything above is about a chord that is *also* an interrupt: the
/// selection and the Shift are how one key serves two masters, and `Cmd+C` has
/// no second master to serve — `^C` is still an interrupt on that keyboard, on
/// the same press, with Control held instead. A `Cmd+C` with nothing selected
/// therefore copies nothing and sends nothing, which is what every application
/// on the platform does and what the clipboard door already answers for an empty
/// selection.
pub(crate) fn should_copy_selection(
    key: &Key,
    modifiers: ModifiersState,
    has_selection: bool,
) -> bool {
    should_copy_selection_on(key, modifiers, has_selection, bt_platform::host_platform())
}

/// The same on a named platform. See [`is_command_chord_on`].
pub(crate) fn should_copy_selection_on(
    key: &Key,
    modifiers: ModifiersState,
    has_selection: bool,
    platform: HostPlatform,
) -> bool {
    is_copy_shortcut_on(key, modifiers, platform)
        && (platform == HostPlatform::MacOs || modifiers.shift_key() || has_selection)
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

/// **Whether a key event is a keystroke at all**, or only a report of what was
/// already held down when this window took the keyboard (`docs/DESIGN.md`
/// §7.54d).
///
/// Two facts, and the second one is the whole of why this function exists.
///
/// A **release** is not a keystroke: what a terminal encodes is the press, and
/// the up half of every key in this window has always been dropped.
///
/// A **synthetic** event is not a keystroke either. winit reports one for every
/// key that is physically down at the moment a window gains the keyboard — its
/// answer to "what is the hand holding right now", posted as a press because
/// there is no other event shape to post it in. It is a *state report addressed
/// to the new window*, not a press the reader made at that window, and the two
/// are told apart by exactly one bit: `WindowEvent::KeyboardInput`'s
/// `is_synthetic`.
///
/// The difference is invisible until a window can arrive **under a hand that is
/// still on a key**, and this product has one: the summoned terminal comes up
/// while the summon chord is held (§7.54). The chord's own `WM_KEYDOWN` never
/// reaches any window — `RegisterHotKey` swallows it, which is documented and
/// was measured — but the *character key of that chord is still down* when the
/// window it summoned takes focus, so winit dutifully reports it as a press with
/// its text attached, and a terminal that read it typed the summon key into the
/// shell.
///
/// **A bit and not a clock.** The same sentence could be spelled as "ignore a
/// character for a moment after a summon", and that spelling is wrong twice: it
/// would swallow a real key from a fast hand, and it would still leak on a slow
/// machine. What is wrong with the event is not when it arrived; it is that it
/// was never a keystroke.
#[must_use]
pub(crate) fn is_a_keystroke(state: ElementState, is_synthetic: bool) -> bool {
    state == ElementState::Pressed && !is_synthetic
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
        // **The Windows key is not a text modifier** (§7.54d, the second half).
        // `Shift` and `AltGr` compose characters and `Ctrl` and `Alt` have
        // terminal encodings of their own; the Windows key has neither. It is
        // held to reach *another program's* verb, and no layout on this platform
        // puts a character behind it — so a chord wearing it produces no text,
        // rather than the text its key would have produced alone. Without this
        // an unclaimed `Win+j` types `j` into the shell, which is a keystroke
        // the reader aimed somewhere else entirely.
        // **And the `is_ascii()` half of this arm is gone** (M1-7, X-3 §4 ⑤).
        // It read `(text.is_ascii() || modifiers.alt_key())` and it swallowed
        // `ü ä ö ß` whole on a German layout — zero bytes each, measured on the
        // Mac and true on this platform for the same reason, while `⌥q` on the
        // same keyboard produced `ESC «` only because Alt happened to be held.
        // A character a layout produced is bytes for the child like any other:
        // that is the whole of what a terminal does with a key.
        //
        // What the guard was for was a *composed* character being typed twice —
        // once as the IME's commit and once as the key behind it — and the
        // composition is already kept out of this stream by the platform on both
        // sides. Windows never reaches here at all: an IME's `WM_CHAR` arrives
        // with no key event under it and winit drops it ("Received a CHAR
        // message but no `event_info` was available"), which is why the physical
        // key during a composition is `NamedKey::Process` and is answered at the
        // top of this function. macOS does not reach here either: X-3 measured
        // `你好` arriving as one `Ime::Commit` and no `KeyboardInput`, and `⌥e e`
        // as `Preedit("´")` then `Commit("é")` — three bytes, once. So what gates
        // this arm is the live composition, and the code point was never the
        // thing that knew about one.
        Key::Character(text)
            if text.chars().all(|character| !character.is_control())
                && !modifiers.control_key()
                && !modifiers.super_key() =>
        {
            Some(meta_prefix(text.as_bytes(), modifiers.alt_key()))
        }
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Backspace) => Some(vec![0x7f]),
        Key::Named(NamedKey::Tab) => Some(vec![b'\t']),
        Key::Named(NamedKey::Escape) => Some(vec![0x1b]),
        // winit reports the text-producing space key as Named rather than
        // Character. It is text, so it answers to the same rule one arm up: the
        // Windows key produces no character here either.
        Key::Named(NamedKey::Space) if !modifiers.control_key() && !modifiers.super_key() => {
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
    /// MUTATION: drop the `Insert` arm of [`is_copy_shortcut_on`] and the first
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

    /// One key event as winit hands it over: the two bits of the envelope
    /// (`state`, `is_synthetic`) and the two of the letter (the key, and what was
    /// held with it).
    type Event = (ElementState, bool, Key, ModifiersState);

    fn character(text: &str) -> Key {
        Key::Character(text.into())
    }

    /// **The whole path from a run of key events to the bytes the child reads**,
    /// which is the two lines `Runtime::keyboard_input` opens with and the
    /// encoder it ends at. Everything between them is the ladder of surfaces,
    /// and none of it is reachable by the events these tests are about — a
    /// terminal with nothing open over it is what is being modelled.
    fn typed_into_the_child(events: &[Event]) -> Vec<u8> {
        let mut written = Vec::new();
        for (state, is_synthetic, key, modifiers) in events {
            if !is_a_keystroke(*state, *is_synthetic) {
                continue;
            }
            if let Some(bytes) = keyboard_bytes(key, *modifiers, false) {
                written.extend_from_slice(&bytes);
            }
        }
        written
    }

    /// RED (§7.54d) — **the chord that summons a terminal types nothing into
    /// it.**
    ///
    /// The run is transcribed from a real one, measured 2026-09-05 with
    /// `BT_PTY_DUMP` and `SendInput` against a window summoned by `Win+'`: six
    /// events, in this order, with these flags. Read it downwards and the defect
    /// is legible without a keyboard —
    ///
    /// 1. `Super` goes down at the window that had the keyboard.
    /// 2. That window loses it: winit's synthetic release burst.
    /// 3. The summoned one takes it: winit's synthetic press burst.
    /// 4. **The character key of the chord, still physically down, reported to
    ///    the new window as a press with its text attached** — and with no
    ///    modifiers at all, because the modifier state that arrives with a
    ///    synthetic burst is the new window's, which has seen nothing yet. This
    ///    is the byte that reached the shell.
    /// 5. The real release of that key.
    /// 6. The real release of `Super`.
    ///
    /// The chord's own `WM_KEYDOWN` is **not in the list**, and that is not an
    /// omission: `RegisterHotKey` swallowed it, exactly as documented. Nothing
    /// about the hotkey path was ever wrong.
    ///
    /// MUTATION: drop `!is_synthetic` from [`is_a_keystroke`] and event 4 is
    /// encoded — `'` lands at the prompt of the window the reader has only just
    /// called up, which is the defect verbatim.
    #[test]
    fn a_chord_held_under_the_window_it_summons_types_nothing_into_it() {
        let run: [Event; 6] = [
            (
                ElementState::Pressed,
                false,
                Key::Named(NamedKey::Super),
                ModifiersState::SUPER,
            ),
            (
                ElementState::Released,
                true,
                Key::Named(NamedKey::Super),
                ModifiersState::empty(),
            ),
            (
                ElementState::Pressed,
                true,
                Key::Named(NamedKey::Super),
                ModifiersState::empty(),
            ),
            (
                ElementState::Pressed,
                true,
                character("'"),
                ModifiersState::empty(),
            ),
            (
                ElementState::Released,
                false,
                character("'"),
                ModifiersState::SUPER,
            ),
            (
                ElementState::Released,
                false,
                Key::Named(NamedKey::Super),
                ModifiersState::empty(),
            ),
        ];
        assert_eq!(
            typed_into_the_child(&run),
            Vec::<u8>::new(),
            "a summon put a character into the terminal it summoned"
        );
    }

    /// RED (§7.54d) — **and the key still works when it is pressed.**
    ///
    /// The companion the rule above needs: a gate written one notch too wide —
    /// dropping every press that arrives near a focus change, say, or every
    /// press of a key that a chord also names — would pass the test above by
    /// making the keyboard stop working. What separates the two runs is one bit
    /// of the envelope and nothing about the letter.
    ///
    /// MUTATION: return `false` from [`is_a_keystroke`] for anything but a real
    /// press *and* a real release, or gate it on the key rather than the
    /// envelope, and this goes red while the one above stays green.
    #[test]
    fn the_same_key_pressed_by_a_hand_still_reaches_the_child() {
        let run: [Event; 2] = [
            (
                ElementState::Pressed,
                false,
                character("'"),
                ModifiersState::empty(),
            ),
            (
                ElementState::Released,
                false,
                character("'"),
                ModifiersState::empty(),
            ),
        ];
        assert_eq!(typed_into_the_child(&run), b"'".to_vec());
    }

    /// RED (§7.54d) — **a chord wearing the Windows key produces no text.**
    ///
    /// The other half of the same sentence, at the other end of the path: the
    /// gate above throws away an event that was never a keystroke, and this
    /// throws away the text of a keystroke that was never text. `Win+j` is held
    /// to reach some other program's verb; a terminal that encoded the letter
    /// under it would type into the shell whatever the reader's hand was aiming
    /// past it.
    ///
    /// Space is asserted beside the letter because winit reports it as a
    /// `Named` key and it takes its own arm — a rule written once for
    /// `Character` would leave `Win+Space` spelling a space into the child.
    ///
    /// MUTATION: drop either `!modifiers.super_key()` and the matching line goes
    /// red; the modifier is otherwise invisible to this encoder, which has no
    /// xterm bit for it.
    #[test]
    fn the_windows_key_is_not_a_modifier_that_spells_anything() {
        let win = ModifiersState::SUPER;
        assert_eq!(keyboard_bytes(&character("j"), win, false), None);
        assert_eq!(
            keyboard_bytes(&Key::Named(NamedKey::Space), win, false),
            None
        );
        assert_eq!(
            keyboard_bytes(&character("j"), win.union(ModifiersState::SHIFT), false),
            None,
            "and it is not talked out of it by a second modifier"
        );
        // The same two keys with the Windows key up are the child's, unchanged.
        assert_eq!(
            keyboard_bytes(&character("j"), ModifiersState::empty(), false),
            Some(b"j".to_vec())
        );
        assert_eq!(
            keyboard_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty(), false),
            Some(b" ".to_vec())
        );
    }

    // ── The routing rule (M1-7, probe X-3 §4) ──────────────────────────────

    const MAC: HostPlatform = HostPlatform::MacOs;
    const WINDOWS: HostPlatform = HostPlatform::Windows;
    const CMD: ModifiersState = ModifiersState::SUPER;

    /// RED (M1-7, X-3 §4 ①) — **a Control chord is the child's on macOS**, byte
    /// for byte what it is here.
    ///
    /// The half of the rule that is about what this window must go on *not*
    /// doing. X-3 found `^C`, `^D` and `^Z` already correct on that machine —
    /// `0x03`, an EOF that ended `cat`, `zsh: suspended` — and the risk this
    /// ticket carries is that a routing change quietly takes one of them back.
    /// The encoder does not ask what platform it is on for these, and this is
    /// what says that is deliberate.
    ///
    /// MUTATION: make the control-alphabet arm ask `is_command_chord` instead of
    /// `control_key()`.
    #[test]
    fn a_control_chord_is_the_childs_on_macos() {
        for (letter, byte) in [("c", 0x03u8), ("d", 0x04), ("z", 0x1a), ("b", 0x02)] {
            assert_eq!(
                keyboard_bytes(&character(letter), ModifiersState::CONTROL, false),
                Some(vec![byte]),
                "Ctrl+{letter} is the child's control code on every platform"
            );
        }
        // And the modifier that is the *application's* there produces no byte at
        // all — a Command chord this table does not claim is not a keystroke the
        // shell should hear the letter of.
        for letter in ["c", "d", "z", "t", "w", "q"] {
            assert_eq!(
                keyboard_bytes(&character(letter), CMD, false),
                None,
                "Cmd+{letter} is the application's and sends nothing"
            );
        }
    }

    /// RED (M1-7, X-3 §4 ⑤) — **a letter a layout makes reaches the child.**
    ///
    /// `keyboard_bytes` carried `(text.is_ascii() || modifiers.alt_key())` and
    /// swallowed `ü ä ö ß` whole on a German layout — zero bytes each, measured
    /// on the Mac and true here for the same reason — while `⌥q` on the same
    /// keyboard produced `ESC «` only because Alt happened to be held. A
    /// character a layout produced is bytes for the child like any other.
    ///
    /// MUTATION: put the `is_ascii()` clause back and every one of these goes to
    /// `None`.
    #[test]
    fn a_layouts_non_ascii_letter_reaches_the_child() {
        for letter in ["ü", "ä", "ö", "ß", "é", "å", "中"] {
            assert_eq!(
                keyboard_bytes(&character(letter), ModifiersState::empty(), false),
                Some(letter.as_bytes().to_vec()),
                "{letter} is what the keyboard produced and the child hears it"
            );
        }
        // With Alt genuinely held it is still `ESC` and the same bytes, which is
        // the one case the old guard let through and the reason the fault hid.
        assert_eq!(
            keyboard_bytes(&character("«"), ModifiersState::ALT, false),
            Some(b"\x1b\xc2\xab".to_vec())
        );
    }

    /// RED (M1-7, §8 Q9) — **Option types text unless the setting says Alt.**
    ///
    /// X-3 measured `⌥a` arriving at the pty as `ESC å` (`1b c3 a5`): winit hands
    /// macOS applications the composed character **and** `alt_key()`, so Folio
    /// was applying both policies to one press. The character half is settled at
    /// the window (`OptionAsAlt`); this is the modifier half, and the two have to
    /// agree or one press means two things.
    ///
    /// MUTATION: make `effective_modifiers` the identity and the first assertion
    /// gets its `ESC` back.
    #[test]
    fn option_types_text_unless_the_setting_says_alt() {
        // Off — the shipped answer. winit reports the composed `å` with Alt
        // held; Alt is not held as far as this window is concerned, so the child
        // hears two bytes and no escape.
        let composed = effective_modifiers(ModifiersState::ALT, false, MAC);
        assert!(!composed.alt_key());
        assert_eq!(
            keyboard_bytes(&character("å"), composed, false),
            Some("å".as_bytes().to_vec()),
            "Option is text, so the child hears the character and nothing else"
        );

        // On — winit reports the raw letter instead and the Alt comes through,
        // which is the other policy, whole.
        let meta = effective_modifiers(ModifiersState::ALT, true, MAC);
        assert!(meta.alt_key());
        assert_eq!(
            keyboard_bytes(&character("a"), meta, false),
            Some(b"\x1ba".to_vec()),
        );

        // And off macOS the answer never moves: Alt is Alt on a keyboard with an
        // Alt key printed on it, whichever way the row is set.
        for setting in [false, true] {
            assert_eq!(
                effective_modifiers(ModifiersState::ALT, setting, WINDOWS),
                ModifiersState::ALT,
            );
        }
    }

    /// RED (M1-7, X-3 §4 ③) — **a chord is never typing.**
    ///
    /// The predicate every one-line field in this window asks before it inserts a
    /// character, and the one six of them were missing: they guarded `ctrl` and
    /// `alt` and never `super`, so `Cmd+C` typed a `c` into whatever held the
    /// caret on a Mac and `Win+C` did the same here. Both modifiers, on both
    /// platforms — the one that is not this platform's application modifier is
    /// the *terminal's*, and a terminal's chord is no more a character than an
    /// application's is.
    ///
    /// The six fields themselves are held by
    /// `a_command_chord_never_types_its_letter_into_a_field` in `main.rs`, which
    /// asks whether each of them consults this.
    #[test]
    fn a_chord_is_never_typing() {
        assert!(types_a_character(ModifiersState::empty()));
        assert!(types_a_character(ModifiersState::SHIFT));
        assert!(
            types_a_character(ModifiersState::ALT),
            "Alt composes text on both platforms and each field says what it does with it"
        );
        assert!(!types_a_character(CMD));
        assert!(!types_a_character(ModifiersState::CONTROL));
        assert!(!types_a_character(CMD.union(ModifiersState::SHIFT)));
    }

    /// RED (M1-7, X-3 §4 ①/②) — **which modifier is whose, on each platform.**
    ///
    /// The three predicates the whole ticket is built on, asserted as the pair of
    /// complements they are. A build that answered `control_key()` on both sides
    /// would compile, pass every Windows test in this file, and hand a Mac's `^C`
    /// to the command palette.
    #[test]
    fn command_is_the_applications_and_control_is_the_terminals() {
        assert!(is_command_chord_on(ModifiersState::CONTROL, WINDOWS));
        assert!(!is_command_chord_on(CMD, WINDOWS));
        assert!(is_terminal_chord_on(CMD, WINDOWS));

        assert!(is_command_chord_on(CMD, MAC));
        assert!(!is_command_chord_on(ModifiersState::CONTROL, MAC));
        assert!(is_terminal_chord_on(ModifiersState::CONTROL, MAC));
    }

    /// RED (M1-7, X-3 §4 ②) — **the clipboard pair speaks the platform's
    /// dialect, and drops a key no keyboard there has.**
    ///
    /// `Ctrl+Insert` and `Shift+Insert` are the older Windows pair; no Apple
    /// keyboard has an `Insert` key, so a spelling for it there would be a
    /// promise about a key nobody can press. And `Cmd+C` copies unconditionally
    /// where `Ctrl+C` copies only with Shift or a selection, because the Shift
    /// clause exists to share one key with an interrupt and `Cmd+C` shares
    /// nothing — `^C` is still the interrupt on that keyboard, on the same press,
    /// with Control held instead.
    #[test]
    fn the_clipboard_pair_is_the_platforms() {
        let c = character("c");
        let v = character("v");
        let insert = Key::Named(NamedKey::Insert);

        assert!(is_copy_shortcut_on(&c, ModifiersState::CONTROL, WINDOWS));
        assert!(is_copy_shortcut_on(
            &insert,
            ModifiersState::CONTROL,
            WINDOWS
        ));
        assert!(is_paste_shortcut_on(&v, ModifiersState::CONTROL, WINDOWS));
        assert!(is_paste_shortcut_on(
            &insert,
            ModifiersState::SHIFT,
            WINDOWS
        ));

        assert!(is_copy_shortcut_on(&c, CMD, MAC));
        assert!(is_paste_shortcut_on(&v, CMD, MAC));
        assert!(
            !is_copy_shortcut_on(&c, ModifiersState::CONTROL, MAC),
            "Ctrl+C on a Mac is the child's interrupt and nothing else"
        );
        assert!(
            !is_copy_shortcut_on(&insert, ModifiersState::CONTROL, MAC)
                && !is_paste_shortcut_on(&insert, ModifiersState::SHIFT, MAC),
            "no keyboard there has the key this pair is spelled on"
        );

        assert!(
            !should_copy_selection_on(&c, ModifiersState::CONTROL, false, WINDOWS),
            "with nothing selected `Ctrl+C` is an interrupt"
        );
        assert!(should_copy_selection_on(&c, CMD, false, MAC));
    }
}
