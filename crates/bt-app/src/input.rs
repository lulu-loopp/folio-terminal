use bt_platform::HostPlatform;
use bt_pty::ConPtyKind;
use bt_term::{KeyboardProtocol, ModifyOtherKeys, SUPPORTED_KITTY_FLAGS};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{
    Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKey, PhysicalKey,
};

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

// ── The same rule, said about the pointer (T-MAC-CMDCLICK, §13.45) ──────────

/// **Whether the hand is holding the modifier that hands a pointer gesture
/// over** — `Ctrl` here, `⌘` on a Mac.
///
/// This is M1-7's sentence stated about the other input device, and it is the
/// same sentence rather than a second one: the rulings of 2026-08-20 spend a
/// modifier on a click (`点=留窗内,Ctrl+点=交出去`) and on a notch, and the
/// modifier they spend is *this application's* — the one that means "I am
/// talking to Folio" — which is why the answer is read off
/// [`is_command_chord_on`] instead of being matched a second time. Two matches
/// on `HostPlatform` for one dialect is how a dialect comes to be two.
///
/// **It has to be Command on a Mac, and not merely by symmetry.** Control-click
/// *is* the secondary click on that platform: AppKit gives a one-button mouse
/// its context menu that way, winit hands it on as a plain left press with
/// Control in the flags ([`pressed_button`] is the other half of this ticket),
/// and a build that also spent Control on "hand this link to the system" would
/// be answering one press with two verbs.
///
/// # It is asked of the hand
///
/// The state to hand this is `WindowRuntime::modifiers_held` and not
/// `modifiers` — §13.33 ①'s ruling, and for its reason: a gesture composes no
/// character, so a policy about what a key *types* has no business deciding
/// what a click *does*. The two states carry the same Control and the same
/// Command bit today, because [`effective_modifiers`] takes out `Alt` and
/// nothing else; the reading is the hand's all the same, so that the next key
/// a text policy speaks for does not silently take a gesture with it.
#[must_use]
pub(crate) fn pointer_chord_held(modifiers: ModifiersState) -> bool {
    pointer_chord_held_on(modifiers, bt_platform::host_platform())
}

/// The same on a named platform, so a test on either machine can ask about the
/// other. See [`is_command_chord_on`].
#[must_use]
pub(crate) fn pointer_chord_held_on(modifiers: ModifiersState, platform: HostPlatform) -> bool {
    is_command_chord_on(modifiers, platform)
}

/// **Whether a wheel notch wears the text-size gesture's modifier, and only it** (ticket 37):
/// `Ctrl` on Windows, `⌘` on a Mac — [`pointer_chord_held_on`] — with nothing else held.
///
/// Exact, because a blind "Ctrl is down" also takes `Ctrl+Alt` (the pair Windows reports AltGr
/// as) and `Ctrl+Shift`, and each of those already has a meaning on the wheel: `Alt` aims a card's
/// window, `Shift` is §7.1.5f's "this notch is the window's, not the program's". So Shift, Alt and
/// the platform's other modifier ([`is_terminal_chord_on`]: Super here, Control on a Mac) each
/// hand the notch back to the routes it had. Built on the two questions this file already answers
/// per platform, so there is no second reading of which key is which.
///
/// On a named platform, for [`is_command_chord_on`]'s reason: a test on either machine asks about
/// the other.
#[must_use]
pub(crate) fn text_size_wheel_held_on(modifiers: ModifiersState, platform: HostPlatform) -> bool {
    pointer_chord_held_on(modifiers, platform)
        && !modifiers.shift_key()
        && !modifiers.alt_key()
        && !is_terminal_chord_on(modifiers, platform)
}

/// **What button this press is**, once the platform's own secondary-click
/// convention has been applied to what winit reported.
///
/// On a Mac, Control+click is the secondary click — it is how a trackpad and a
/// one-button mouse raise a context menu, and every application on that desk
/// answers it. AppKit does not turn it into a right-button event on the way:
/// `mouseDown:` is delivered with `buttonNumber` 0 and the Control flag set, and
/// winit's `mouse_button` reads that number and nothing else
/// (`macos/view.rs:1090`), so a window that does not make the translation itself
/// sees a plain left press and the desk's oldest gesture does nothing.
///
/// It is made **once**, at the one door every button event in this process comes
/// through, for [`effective_modifiers`]'s reason: a second place that decided
/// what a press was would be a second answer. Downstream nothing is asked and
/// nothing changes — the pane's menu, the tab's menu, the page's menu and the
/// forwarding table all go on reading `MouseButton::Right`, which is the whole
/// point: the two ways a Mac makes a secondary press are one press here, and
/// this window's rule for it (`right_press_raises_terminal_menu`: the program's
/// while it is tracking the mouse, ours otherwise) is written down once.
///
/// Off macOS it is the identity function and says so: a Control+click on a
/// Windows mouse is a Control+click, and this product spends it on handing
/// references to the system.
#[must_use]
pub(crate) fn pressed_button(
    reported: MouseButton,
    modifiers: ModifiersState,
    platform: HostPlatform,
) -> MouseButton {
    if platform == HostPlatform::MacOs && reported == MouseButton::Left && modifiers.control_key() {
        MouseButton::Right
    } else {
        reported
    }
}

/// [`pressed_button`] **held for the length of the gesture**, which is what the
/// one door actually asks.
///
/// The press decides and the release is owed the press's answer. Control is a
/// key and a button is a button, so a hand can let one go before the other: lift
/// Control first and the platform reports a plain left release after a press
/// this window took as the secondary one. That release is not harmless. It
/// reaches `route_forwarded_mouse_button`, whose own comment already states the
/// rule this obeys — *the release is owed to the press that was forwarded, so it
/// is spelled the way that press was spelled* — and which spells the encoding
/// off its latch for exactly that reason; the button has to come from the same
/// place, or a mouse-tracking program is handed a right press and a left
/// release it can pair with nothing.
///
/// So one `bool` travels with the hand, and it is this window's smallest
/// possible statement of "a gesture belongs to the press that began it" — the
/// same sentence `MouseRoute` makes about where a drag goes and `DragLatch`
/// makes about whether it has travelled.
///
/// **Only the left button is ever latched**, because it is the only one the
/// platform rule can rewrite; a right, middle or back button passes through
/// with the latch untouched, so a middle-click tab close in the middle of
/// anything cannot disturb it. On Windows the latch is written `false` by every
/// press and read back `false`, which is the identity this function is there
/// on that platform.
#[must_use]
pub(crate) fn pressed_button_of_gesture(
    secondary: &mut bool,
    reported: MouseButton,
    state: ElementState,
    modifiers: ModifiersState,
    platform: HostPlatform,
) -> MouseButton {
    if reported != MouseButton::Left {
        return reported;
    }
    let secondary = match state {
        ElementState::Pressed => {
            *secondary = pressed_button(reported, modifiers, platform) == MouseButton::Right;
            *secondary
        }
        ElementState::Released => std::mem::take(secondary),
    };
    if secondary {
        MouseButton::Right
    } else {
        MouseButton::Left
    }
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
    let one = legacy_bytes(
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

/// **The key a program that types for you meant**, for an event that carries
/// text and names no key at all (T-REMOTE-INPUT-PACKET).
///
/// # What arrives
///
/// Something that types *for* you does not press keys. The phone keyboard in
/// OPPO's O+ Connect, a remote-desktop client, an accessibility tool, a
/// password manager, `SendKeys` in a script — all of them hand the OS a Unicode
/// code unit and ask it to deliver that. On Windows the vehicle is `SendInput`
/// with `KEYEVENTF_UNICODE`, and what the window gets is one triple per
/// character, measured on 2026-09-15 in a bare Win32 window:
///
/// ```text
/// WM_KEYDOWN wParam=0x00E7 lParam=0x00000001   (VK_PACKET, scancode 0)
/// WM_CHAR    wParam=0x6BD4                     ('比')
/// WM_KEYUP   wParam=0x00E7 lParam=0xC0000001
/// ```
///
/// winit delivers that press faithfully, and it delivers it **empty of a key**.
/// Its builder pairs the `WM_CHAR` with the keydown and puts the character in
/// `text` (`keyboard.rs` `WM_CHAR` arm: `event_info.text =
/// PartialText::System(event_info.utf16parts.clone())`), but the two key fields
/// have nothing to say: the scancode is zero, so `physical_key` is
/// `Unidentified`, and `VK_PACKET` sits in the layout's non-printable table
/// (`keyboard_layout.rs`: `VK_PACKET => Key::Unidentified(native_code)`), so
/// `logical_key` is `Unidentified` too.
///
/// # Why the text was lost
///
/// **Every rung of this window routes a keystroke by its key.** `keyboard_bytes`
/// matches on `Key` and falls to `None`; the search capsule, the rename editor,
/// the preview's quick edit and the files column all match on
/// `event.logical_key`. An `Unidentified` key is a key none of them has a verb
/// for, so the character fell off the bottom of the ladder and the pane stayed
/// empty — while the same phone in "PC keyboard" mode, which drives the local
/// IME and produces ordinary keys, worked.
///
/// # What this says
///
/// **A character that arrived with no key is the key that would have produced
/// it.** Said once, here, and applied by rewriting the event's `logical_key` at
/// the top of the ladder (`Runtime::keyboard_input`), so that every
/// rung below goes on reading one field and none of them learns a second way to
/// find out what was typed. That is also what keeps the modifier policy honest:
/// a rewritten event answers [`types_a_character`] and the `Ctrl`/`Super` guards
/// exactly as a typed one does.
///
/// **A control code is not a character; it is the key behind one.** The four an
/// injector can send have exactly one key each, and they are spelled out rather
/// than passed through, because `Key::Character("\r")` is a value no rung
/// answers — `keyboard_bytes`' character arm excludes control characters by
/// construction. Alone, or not at all: a control code inside a run of text is
/// neither a key press nor text.
///
/// **Asked of both key fields, not of `VK_PACKET`.** The virtual key is a
/// Windows fact and this is a statement about events, not about one platform's
/// injection API: any event that names no key and carries text means the text.
/// There is no key to lose by taking this road — an event winit could identify
/// is not one of these.
///
/// # What this cannot recover
///
/// A character outside the BMP is injected as two packets, one per UTF-16
/// surrogate, and winit finalises each on its own: `OsString::from_wide` of a
/// lone surrogate fails to become a `String`, so `text` is `None` on both halves
/// and the code units are gone before any event exists. Astral characters
/// (emoji) typed from a phone therefore still do not arrive; that loss is inside
/// the builder and cannot be seen from here.
#[must_use]
pub(crate) fn injected_logical_key(
    physical_key: PhysicalKey,
    logical_key: &Key,
    text: Option<&str>,
) -> Option<Key> {
    if !matches!(physical_key, PhysicalKey::Unidentified(_))
        || !matches!(logical_key, Key::Unidentified(_))
    {
        return None;
    }
    let text = text?;
    let mut rest = text.chars();
    let first = rest.next()?;
    if first.is_control() {
        if rest.next().is_some() {
            return None;
        }
        return match first {
            '\r' | '\n' => Some(Key::Named(NamedKey::Enter)),
            '\t' => Some(Key::Named(NamedKey::Tab)),
            '\u{8}' => Some(Key::Named(NamedKey::Backspace)),
            '\u{1b}' => Some(Key::Named(NamedKey::Escape)),
            _ => None,
        };
    }
    if rest.any(char::is_control) {
        return None;
    }
    Some(Key::Character(text.into()))
}

/// **The bytes one key press sends to the child** — the encoder, the last rung of
/// `Runtime::keyboard_input`, and the only rung the keyboard protocols change
/// (`docs/plans/design/keyboard-protocol-2026-09-29.md` §5).
///
/// `keyboard` is what the program in the pane asked for, read from the session at the
/// moment of the key as DECCKM is (§2.3). **A program that never asked gets exactly the
/// bytes it got before** — [`legacy_bytes`], unchanged, which
/// `key_encoding_legacy_{windows,macos}.tsv` hold byte for byte. When the kitty protocol's flag 1 is in
/// force the key is encoded by [`kitty_bytes`]; otherwise, when modifyOtherKeys is,
/// by [`modify_other_keys_bytes`], with every chord xterm leaves alone keeping its
/// legacy bytes. **When both are set, kitty wins**, as in kitty, Ghostty and WezTerm.
/// `key_encoding.tsv` is the table all of it answers to.
///
/// `key_without_modifiers` is winit's key with no modifier applied — kitty's
/// "unicode-key-code", the un-shifted key on whatever layout is in use — and
/// `location` says whether it is a keypad key.
///
/// **On Windows, with neither protocol asked for, the chords VT cannot express go as
/// win32-input-mode key records** while the transport says it parses them
/// ([`KeyboardProtocol::win32_input_mode`], which ConPTY sets): [`key_records`], built from what
/// `origin` says about the press (T-KEYBOARD-RECORDS, design note §7.3). A program that asked for
/// kitty or modifyOtherKeys gets its protocol instead (§4.3), and every chord outside the set
/// keeps its legacy bytes.
pub(crate) fn keyboard_bytes(
    key: &Key,
    key_without_modifiers: &Key,
    location: KeyLocation,
    modifiers: ModifiersState,
    application_cursor_mode: bool,
    keyboard: KeyboardProtocol,
    origin: KeyOrigin<'_>,
) -> Option<Vec<u8>> {
    if keyboard.kitty & SUPPORTED_KITTY_FLAGS != 0 {
        return kitty_bytes(
            key,
            key_without_modifiers,
            location,
            modifiers,
            application_cursor_mode,
            origin,
        );
    }
    let mode = keyboard.modify_other_keys;
    if mode == ModifyOtherKeys::Off {
        if keyboard.win32_input_mode
            && let Some(records) = key_records(key, key_without_modifiers, modifiers, origin)
        {
            return Some(records);
        }
        return legacy_bytes(key, modifiers, application_cursor_mode);
    }
    if withheld_from_every_protocol(key, modifiers) {
        return None;
    }
    modify_other_keys_bytes(key, key_without_modifiers, modifiers, mode, origin)
        .or_else(|| legacy_bytes(key, modifiers, application_cursor_mode))
}

/// Rule 1 of both protocols (§4.2): a composing key, a paste chord, or a chord that holds
/// Super sends nothing. Super never reaches the child — on Windows the key belongs to
/// another program's verb (§7.54d), on a Mac Command belongs to the application
/// (§13.13) — so no protocol ever reports bit 8.
fn withheld_from_every_protocol(key: &Key, modifiers: ModifiersState) -> bool {
    matches!(key, Key::Named(NamedKey::Process))
        || is_paste_shortcut(key, modifiers)
        || modifiers.super_key()
}

/// **The kitty keyboard protocol's disambiguate tier (flag 1)**, as kitty's own encoder
/// (`kitty/key_encoding.c`) writes it — the first of these rules that matches (§4.2):
///
/// 1. a composing key, a paste chord or Super held — nothing;
/// 2. printable text with no modifier but Shift — the text;
/// 3. a keypad key that produced no text — `CSI code;m u` with the protocol's keypad code
///    (owner ruling Q2, 2026-09-29), Numpad Enter included, so it is asked before rule 4;
/// 4. Enter, Tab or Backspace of the main block with no modifier — `\r`, `\t`, `\x7f`, so
///    `reset` can still be typed;
/// 5. arrows, Home and End — `CSI X`, or `CSI 1;m X` with modifiers, **ignoring DECCKM**
///    (kitty uses `SS3` only in legacy mode); Insert, Delete, PageUp and PageDown as ever;
/// 6. F1, F2 and F4 — `CSI P/Q/S` or `CSI 1;m P/Q/S`; F3 — `CSI 13~` or `CSI 13;m~`
///    (kitty dropped `CSI R`, which is the cursor position report); F5–F12 as ever;
/// 7. every other key that has a code — `CSI code;m u`, `;m` left out when `m = 1`:
///    Escape 27, Enter 13, Tab 9, Backspace 127, Space 32, and for a text key the
///    un-shifted character of `key_without_modifiers`.
///
/// A text key whose layout gives it no single un-shifted character has no code, and keeps
/// the bytes it has without the protocol. A text key that arrived with no character — on
/// Windows, Ctrl+Alt on a key the layout types nothing for ([`unidentified_text_key`]) — is
/// a text key here too, with the same code (T-KEYBOARD-CTRLALT).
fn kitty_bytes(
    key: &Key,
    key_without_modifiers: &Key,
    location: KeyLocation,
    modifiers: ModifiersState,
    application_cursor_mode: bool,
    origin: KeyOrigin<'_>,
) -> Option<Vec<u8>> {
    if withheld_from_every_protocol(key, modifiers) {
        return None;
    }
    let modifier = xterm_modifier(modifiers);
    let at_most_shift = !modifiers.control_key() && !modifiers.alt_key();
    match key {
        Key::Character(text) if at_most_shift && !text.chars().any(char::is_control) => {
            return Some(text.as_bytes().to_vec());
        }
        Key::Named(NamedKey::Space) if at_most_shift => return Some(b" ".to_vec()),
        _ => {}
    }
    // The keypad before the plain-key exception: Numpad Enter is `KP_ENTER`, not `\r`.
    if location == KeyLocation::Numpad
        && let Key::Named(named) = key
        && let Some(code) = keypad_code(*named)
    {
        return Some(csi_u(code, modifier));
    }
    match key {
        Key::Named(NamedKey::Enter) if modifier == 1 => return Some(vec![b'\r']),
        Key::Named(NamedKey::Tab) if modifier == 1 => return Some(vec![b'\t']),
        Key::Named(NamedKey::Backspace) if modifier == 1 => return Some(vec![0x7f]),
        _ => {}
    }
    let code = match key {
        Key::Named(NamedKey::ArrowUp) => return Some(kitty_cursor_key(b'A', modifier)),
        Key::Named(NamedKey::ArrowDown) => return Some(kitty_cursor_key(b'B', modifier)),
        Key::Named(NamedKey::ArrowRight) => return Some(kitty_cursor_key(b'C', modifier)),
        Key::Named(NamedKey::ArrowLeft) => return Some(kitty_cursor_key(b'D', modifier)),
        Key::Named(NamedKey::Home) => return Some(kitty_cursor_key(b'H', modifier)),
        Key::Named(NamedKey::End) => return Some(kitty_cursor_key(b'F', modifier)),
        Key::Named(NamedKey::Insert) => return Some(tilde_key(2, modifier)),
        Key::Named(NamedKey::Delete) => return Some(tilde_key(3, modifier)),
        Key::Named(NamedKey::PageUp) => return Some(tilde_key(5, modifier)),
        Key::Named(NamedKey::PageDown) => return Some(tilde_key(6, modifier)),
        Key::Named(NamedKey::Escape) => 27,
        Key::Named(NamedKey::Enter) => 13,
        Key::Named(NamedKey::Tab) => 9,
        Key::Named(NamedKey::Backspace) => 127,
        Key::Named(NamedKey::Space) => 32,
        Key::Named(named) => {
            return kitty_function_key(*named, modifiers, bt_platform::host_platform());
        }
        Key::Character(_) => match unshifted_code(key, key_without_modifiers) {
            Some(code) => code,
            None => return legacy_bytes(key, modifiers, application_cursor_mode),
        },
        Key::Unidentified(_) => {
            unidentified_text_key(key, key_without_modifiers, modifiers, origin)?;
            unshifted_code(key, key_without_modifiers)?
        }
        _ => return None,
    };
    Some(csi_u(code, modifier))
}

/// The protocol's code for a keypad key that produced no text (Num Lock off, or Enter):
/// `KP_ENTER` 57414, `KP_LEFT` 57417 … `KP_DELETE` 57426, `KP_BEGIN` 57427.
fn keypad_code(key: NamedKey) -> Option<u32> {
    Some(match key {
        NamedKey::Enter => 57414,
        NamedKey::ArrowLeft => 57417,
        NamedKey::ArrowRight => 57418,
        NamedKey::ArrowUp => 57419,
        NamedKey::ArrowDown => 57420,
        NamedKey::PageUp => 57421,
        NamedKey::PageDown => 57422,
        NamedKey::Home => 57423,
        NamedKey::End => 57424,
        NamedKey::Insert => 57425,
        NamedKey::Delete => 57426,
        NamedKey::Clear => 57427,
        _ => return None,
    })
}

/// A text key's code under flag 1: the one character of the key without modifiers — the
/// un-shifted key on the layout in use (`ф` is 1092) — or, where winit gives none, the
/// key's own one character lower-cased. `None` for a key that has neither.
fn unshifted_code(key: &Key, key_without_modifiers: &Key) -> Option<u32> {
    one_character(key_without_modifiers)
        .filter(|character| !character.is_control())
        .or_else(|| {
            one_character(key)
                .filter(|character| !character.is_control())
                .and_then(|character| {
                    let mut lower = character.to_lowercase();
                    let only = lower.next()?;
                    lower.next().is_none().then_some(only)
                })
        })
        .map(u32::from)
}

/// The key's one character: a `Character` of exactly one, or a dead key's own.
fn one_character(key: &Key) -> Option<char> {
    match key {
        Key::Character(text) => {
            let mut characters = text.chars();
            let character = characters.next()?;
            characters.next().is_none().then_some(character)
        }
        Key::Dead(Some(character)) => Some(*character),
        _ => None,
    }
}

fn csi_u(code: u32, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1b[{code}u").into_bytes()
    } else {
        format!("\x1b[{code};{modifier}u").into_bytes()
    }
}

/// Arrows, Home and End under flag 1: the `CSI` form whatever DECCKM says.
fn kitty_cursor_key(final_byte: u8, modifier: u8) -> Vec<u8> {
    cursor_key(final_byte, modifier, false)
}

/// F1–F12 under flag 1 — kitty's `encode_function_key`: F1, F2 and F4 in their `CSI`
/// forms, F3 as `CSI 13~` (kitty removed `CSI R`, the cursor position report's shape), and
/// F5–F12 as [`function_key`] spells them, whose Super and `Alt+F4` refusals hold here too.
fn kitty_function_key(
    key: NamedKey,
    modifiers: ModifiersState,
    platform: HostPlatform,
) -> Option<Vec<u8>> {
    if platform == HostPlatform::Windows && key == NamedKey::F4 && modifiers.alt_key() {
        return None;
    }
    let modifier = xterm_modifier(modifiers);
    match key {
        NamedKey::F1 => Some(kitty_cursor_key(b'P', modifier)),
        NamedKey::F2 => Some(kitty_cursor_key(b'Q', modifier)),
        NamedKey::F3 => Some(tilde_key(13, modifier)),
        NamedKey::F4 => Some(kitty_cursor_key(b'S', modifier)),
        _ => function_key(key, modifiers, platform),
    }
}

/// **xterm's modifyOtherKeys**, in xterm's default wire form (`formatOtherKeys=0`):
/// `CSI 27 ; m ; k ~`, or `None` for a chord the mode leaves to its legacy bytes (§4.3).
///
/// Which chords are encoded follows xterm's own reference table (`vte`'s
/// `doc/modifyOtherKeys-example.txt`, the output of xterm's `modify-keys.pl`), by the
/// class of the key without modifiers:
///
/// | class | mode 1 | mode 2 |
/// |---|---|---|
/// | Enter, Tab | every modified chord | every modified chord |
/// | Escape | chords with Alt | every modified chord |
/// | Backspace | none | every modified chord but Ctrl alone |
/// | a key with a legacy Ctrl code (letters, Space, `@ [ \ ] ^ _ ?`) | chords with Alt | every modified chord, Shift alone included |
/// | any other printable key | chords with Ctrl or Alt | chords with Ctrl or Alt |
///
/// `k` is the character with Shift applied and without Ctrl — xterm's keysym, so
/// `Ctrl+Shift+a` is 65. On Windows the logical key is exactly that; on a Mac a Ctrl chord
/// can arrive as its control character, and then `k` is the key without modifiers,
/// upper-cased when Shift is held. Backspace's code is 127, because Folio's Backspace sends
/// DEL (xterm's reference used 8 for its `^H`).
///
/// A text key that arrived with no character — on Windows, Ctrl+Alt on a key the layout types
/// nothing for ([`unidentified_text_key`]) — is classed by its key without modifiers like any
/// text key, and its `k` is that character, or with Shift held the character the installed
/// layout types on the key with Shift alone ([`KeyOrigin::shifted_character_of_virtual_key`]:
/// `{` for Ctrl+Shift+Alt+`[` on a US layout), since the press itself carries none
/// (T-KEYBOARD-CTRLALT).
fn modify_other_keys_bytes(
    key: &Key,
    key_without_modifiers: &Key,
    modifiers: ModifiersState,
    mode: ModifyOtherKeys,
    origin: KeyOrigin<'_>,
) -> Option<Vec<u8>> {
    let (shift, alt, control) = (
        modifiers.shift_key(),
        modifiers.alt_key(),
        modifiers.control_key(),
    );
    if !(shift || alt || control) {
        return None;
    }
    let every = mode == ModifyOtherKeys::Two;
    // A text key is classed by its key without modifiers (xterm's table); `keysym` is its code.
    let text_key_class = |base: char, keysym: char| {
        let has_a_control_code = control_byte(base.encode_utf8(&mut [0; 4])).is_some();
        let encoded = if has_a_control_code {
            alt || every
        } else {
            control || alt
        };
        (u32::from(keysym), encoded)
    };
    let (code, encoded) = match key {
        Key::Named(NamedKey::Enter) => (13, true),
        Key::Named(NamedKey::Tab) => (9, true),
        Key::Named(NamedKey::Escape) => (27, alt || every),
        Key::Named(NamedKey::Backspace) => (127, every && (shift || alt)),
        Key::Named(NamedKey::Space) => (32, alt || every),
        Key::Character(_) => {
            let base = one_character(key_without_modifiers)
                .or_else(|| one_character(key))
                .filter(|character| !character.is_control())?;
            let keysym = one_character(key)
                .filter(|character| !character.is_control())
                .or_else(|| {
                    if shift {
                        let mut upper = base.to_uppercase();
                        let only = upper.next()?;
                        upper.next().is_none().then_some(only)
                    } else {
                        Some(base)
                    }
                })?;
            text_key_class(base, keysym)
        }
        Key::Unidentified(_) => {
            let virtual_key = unidentified_text_key(key, key_without_modifiers, modifiers, origin)?;
            let base = one_character(key_without_modifiers)?;
            let keysym = if shift {
                (origin.shifted_character_of_virtual_key)(virtual_key)
                    .filter(|character| !character.is_control())?
            } else {
                base
            };
            text_key_class(base, keysym)
        }
        _ => return None,
    };
    encoded.then(|| format!("\x1b[27;{};{code}~", xterm_modifier(modifiers)).into_bytes())
}

/// **What the platform said about a key press beyond the key itself** — what a win32-input-mode
/// record is built from (T-KEYBOARD-RECORDS). Read from winit's event at the one call site.
#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyOrigin<'a> {
    /// The platform the key was pressed on. Key records are the Windows console's form, so
    /// [`key_records`] writes none anywhere else.
    pub(crate) platform: HostPlatform,
    /// Where the key is on the keyboard: the record's scan code ([`scan_code`]).
    pub(crate) physical_key: PhysicalKey,
    /// The text the press produced with every modifier applied — winit's
    /// `text_with_all_modifiers`, on Windows the `WM_CHAR` the system translated the press into
    /// (`\n` for Ctrl+Enter, DEL for Ctrl+Backspace, nothing for Ctrl+1). The record's
    /// character, which is what a real key event carries.
    pub(crate) text_with_all_modifiers: Option<&'a str>,
    /// The installed layout's virtual key for a scan code: `bt_platform::virtual_key_of_scan_code`
    /// in the product, a fixed layout in a test.
    pub(crate) virtual_key_of_scan_code: fn(u16) -> Option<u16>,
    /// Whether the installed layout makes a virtual key a dead key: `bt_platform::virtual_key_is_dead`
    /// in the product, a fixed layout in a test.
    pub(crate) virtual_key_is_dead: fn(u16) -> bool,
    /// The character the installed layout types on a virtual key with Shift alone:
    /// `bt_platform::shifted_character_of_virtual_key` in the product, a fixed layout in a test.
    /// modifyOtherKeys' `k` for Ctrl+Shift+Alt on a key that arrived with no character
    /// ([`modify_other_keys_bytes`], T-KEYBOARD-CTRLALT).
    pub(crate) shifted_character_of_virtual_key: fn(u16) -> Option<char>,
    /// Which pseudoconsole the pane runs on (`bt_pty::PtySession::conpty_kind`, fixed at spawn).
    /// Records are written only to the ConPTY Folio ships ([`key_records`]).
    pub(crate) conpty: ConPtyKind,
}

/// `dwControlKeyState` bits (`wincon.h`) a record carries: which modifiers were down, and whether
/// the key is one of the enhanced (`E0`-prefixed) keys. The left-hand modifier bits are used
/// because winit does not say which hand was used, as `SHIFT_ENTER_RECORDS` does.
const SHIFT_PRESSED: u16 = 0x0010;
const LEFT_CTRL_PRESSED: u16 = 0x0008;
const LEFT_ALT_PRESSED: u16 = 0x0002;
const ENHANCED_KEY: u16 = 0x0100;

/// **A chord VT cannot express, as the win32-input-mode down/up record pair ConPTY turns into
/// that exact key event** — or `None` for a chord outside the set, or on a platform that is not
/// Windows (T-KEYBOARD-RECORDS, `docs/plans/design/keyboard-protocol-2026-09-29.md` §7.3).
///
/// **The set** is the chords whose legacy bytes cannot tell them apart:
///
/// * Enter, Tab, Backspace or Space with Shift, Alt or Ctrl, and Escape with Shift — legacy
///   sends `\r`, `\t`, DEL, a space (or nothing) and ESC whatever is held, so PSReadLine, cmd,
///   .NET and Codex on Windows read Ctrl+Enter as Enter;
/// * Ctrl with a text key that has no C0 code ([`control_byte`] has none: digits, most
///   punctuation, non-ASCII letters), which legacy does not send at all;
/// * on Windows, Ctrl+Alt (with or without Shift) on a text key the layout types nothing for —
///   on a US layout, every one. winit keeps Ctrl while Alt is down, because Ctrl+Alt may be
///   AltGr (`WindowsModifiers::remove_only_ctrl`, winit 0.30.13
///   `platform_impl/windows/keyboard_layout.rs`, applied in `keyboard.rs`'s key-event builder),
///   and when `ToUnicodeEx` gives no text for that state it hands the key over as
///   `Key::Unidentified(NativeKey::Windows(vk))` (`keyboard_layout.rs`, `ToUnicodeResult::None`
///   keeps the preliminary unidentified key). No rung of the legacy encoder, and neither
///   protocol, has anything to send for such a key, so it is a chord VT cannot express here
///   whatever its C0 code would have been; its record carries the virtual key Windows reported.
///   A key whose key without modifiers is not text, or that has no scan code (a media key), is
///   not one; **nor is a dead key**, which winit reports as the character it would compose
///   (`keyboard.rs`: "We convert dead keys into their character"), so the installed layout is
///   asked only whether the virtual key is a dead key ([`KeyOrigin::virtual_key_is_dead`], the top
///   bit of `MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR)`; its low word is `0` for ordinary keys on some
///   layouts, so it is not read). The French layout's dead `^` under Ctrl+Alt is refused, the
///   Kazakh layout's `ж` key is its record.
///
/// **Only on the ConPTY Folio ships** ([`ConPtyKind::Shipped`]; coordinator's ruling,
/// 2026-09-29). A process falls back to the operating system's ConPTY on its own when the
/// packaged pair is missing or fails to load, and that ConPTY turns many records into other bytes
/// for a program that reads bytes (design note revision (e)); there, as everywhere records are not
/// written, a program that never asked receives exactly the bytes it received before.
///
/// A chord holding Super, a composing key and a paste chord are never in it
/// ([`withheld_from_every_protocol`]). A Ctrl chord that has a C0 code (`Ctrl+I`, `Ctrl+M`,
/// `Ctrl+[`, `Ctrl+Shift+A`) keeps it: ConPTY already turns the code into a key event, and the
/// set is only what VT cannot say. **Escape with Ctrl or Alt keeps its ESC too**: ConPTY
/// swallows that record and no reader gets anything (measured through the vendored ConPTY,
/// `bt-pty`'s `keyboard_protocol_through_conpty`), while the ESC reaches it as Escape. Windows
/// takes those chords for itself anyway (Ctrl+Esc, Alt+Esc, Ctrl+Shift+Esc).
///
/// **The record** is the form `SHIFT_ENTER_RECORDS` already writes, microsoft/terminal's
/// win32-input-mode (`doc/specs/#4999 - Improved keyboard handling in Conpty.md`):
/// `CSI Vk ; Sc ; Uc ; Kd ; Cs ; Rc _`, which ConPTY's `InputStateMachineEngine` parses on the
/// final `_` into one `KEY_EVENT_RECORD`, field for field (`wVirtualKeyCode`, `wVirtualScanCode`,
/// `UnicodeChar`, `bKeyDown`, `dwControlKeyState`, `wRepeatCount`). The pair is the key going down
/// (`Kd` 1) and coming up (`Kd` 0), one repeat each:
///
/// * `Vk` — Enter `VK_RETURN` 13, Tab `VK_TAB` 9, Backspace `VK_BACK` 8, Escape `VK_ESCAPE` 27,
///   Space `VK_SPACE` 32, the same on every layout; a text key's is the installed layout's for its
///   scan code, except that a keypad digit or decimal key that produced text is `VK_NUMPAD0`–`9` /
///   `VK_DECIMAL`, which Num Lock decides and a scan code does not ([`numpad_text_virtual_key`]);
/// * `Sc` — the key's set-1 scan code, from where it is on the keyboard ([`scan_code`]; `0` for a
///   named key whose position was not reported, which is what Windows reports for an injected
///   key). A text key whose position or virtual key is unknown has no record, and keeps its
///   legacy bytes;
/// * `Uc` — the one UTF-16 unit the press produced with every modifier ([`KeyOrigin`]), `0` for
///   a chord that types nothing (Ctrl+1, Ctrl+Shift+Enter). A press that produced more than one
///   unit is not one key event, and keeps its legacy bytes;
/// * `Cs` — `SHIFT_PRESSED`, `LEFT_CTRL_PRESSED`, `LEFT_ALT_PRESSED` for what is held, and
///   `ENHANCED_KEY` for an `E0` key (Numpad Enter, the keypad's `/`).
fn key_records(
    key: &Key,
    key_without_modifiers: &Key,
    modifiers: ModifiersState,
    origin: KeyOrigin<'_>,
) -> Option<Vec<u8>> {
    if origin.platform != HostPlatform::Windows
        || origin.conpty != ConPtyKind::Shipped
        || withheld_from_every_protocol(key, modifiers)
    {
        return None;
    }
    let (shift, alt, control) = (
        modifiers.shift_key(),
        modifiers.alt_key(),
        modifiers.control_key(),
    );
    let scan = scan_code(origin.physical_key);
    let virtual_key: u16 = match key {
        Key::Named(named) if shift || alt || control => match named {
            NamedKey::Enter => 0x0D,
            NamedKey::Tab => 0x09,
            NamedKey::Backspace => 0x08,
            NamedKey::Escape if !control && !alt => 0x1B,
            NamedKey::Space => 0x20,
            _ => return None,
        },
        Key::Character(text)
            if control && control_byte(text).is_none() && !text.chars().any(char::is_control) =>
        {
            match numpad_text_virtual_key(origin.physical_key) {
                Some(virtual_key) => virtual_key,
                None => (origin.virtual_key_of_scan_code)(scan?)?,
            }
        }
        // Ctrl+Alt on a text key the layout types nothing for: the record carries the virtual key
        // Windows reported.
        Key::Unidentified(_) => {
            unidentified_text_key(key, key_without_modifiers, modifiers, origin)?
        }
        _ => return None,
    };
    let character = match origin.text_with_all_modifiers {
        None => 0,
        Some(text) => {
            let mut units = text.encode_utf16();
            match (units.next(), units.next()) {
                (None, _) => 0,
                (Some(unit), None) => unit,
                (Some(_), Some(_)) => return None,
            }
        }
    };
    let scan = scan.unwrap_or(0);
    let mut state = 0;
    if shift {
        state |= SHIFT_PRESSED;
    }
    if control {
        state |= LEFT_CTRL_PRESSED;
    }
    if alt {
        state |= LEFT_ALT_PRESSED;
    }
    if scan & 0xFF00 == 0xE000 {
        state |= ENHANCED_KEY;
    }
    let scan = scan & 0x00FF;
    let record = |down: u8| format!("\x1b[{virtual_key};{scan};{character};{down};{state};1_");
    Some(format!("{}{}", record(1), record(0)).into_bytes())
}

/// **Whether a key that arrived with no character is an ordinary text key** — and if so, the
/// virtual key Windows reported for it. The one answer the three rungs that encode such a key
/// share: win32-input-mode records ([`key_records`]), kitty's flag 1 ([`kitty_bytes`]) and
/// modifyOtherKeys ([`modify_other_keys_bytes`]) (T-KEYBOARD-CTRLALT).
///
/// On Windows winit keeps Ctrl while Alt is down, because Ctrl+Alt may be AltGr
/// (`WindowsModifiers::remove_only_ctrl`, winit 0.30.13 `platform_impl/windows/keyboard_layout.rs`,
/// applied in `keyboard.rs`'s key-event builder), and when `ToUnicodeEx` types nothing for that
/// state it hands the key over as `Key::Unidentified(NativeKey::Windows(vk))` — on a US layout,
/// every Ctrl+Alt and Ctrl+Shift+Alt chord on a text key. Such a press is a text key when Ctrl is
/// held, its key without modifiers is text, it has a position ([`scan_code`]), and the installed
/// layout does not make the virtual key a dead key ([`KeyOrigin::virtual_key_is_dead`]; winit
/// reports a dead key's key without modifiers as the character it would compose). A media key, a
/// key with no scan code and a dead key are not. Under either protocol the key's code is then its
/// key without modifiers, as for any text key (design note §4.2, §4.3).
fn unidentified_text_key(
    key: &Key,
    key_without_modifiers: &Key,
    modifiers: ModifiersState,
    origin: KeyOrigin<'_>,
) -> Option<u16> {
    let Key::Unidentified(NativeKey::Windows(virtual_key)) = key else {
        return None;
    };
    (modifiers.control_key()
        && scan_code(origin.physical_key).is_some()
        && matches!(
            key_without_modifiers,
            Key::Character(text) if !text.chars().any(char::is_control)
        )
        && !(origin.virtual_key_is_dead)(*virtual_key))
    .then_some(*virtual_key)
}

/// **A key's set-1 scan code, from where it is on the keyboard** — an `E0`-prefixed key as
/// `0xE0nn` — or `None` for a key outside the ones a record can be written for (the keys that
/// type text, and the five named keys of [`key_records`]). The numbers are the hardware's, the
/// same ones winit's `physicalkey_to_scancode` answers on Windows, so the table answers the same
/// on every host; a position winit could not name arrives with its raw scan code.
fn scan_code(physical_key: PhysicalKey) -> Option<u16> {
    let code = match physical_key {
        PhysicalKey::Code(code) => code,
        PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Windows(scan)) if scan != 0 => {
            return Some(scan);
        }
        PhysicalKey::Unidentified(_) => return None,
    };
    Some(match code {
        KeyCode::Escape => 0x01,
        KeyCode::Digit1 => 0x02,
        KeyCode::Digit2 => 0x03,
        KeyCode::Digit3 => 0x04,
        KeyCode::Digit4 => 0x05,
        KeyCode::Digit5 => 0x06,
        KeyCode::Digit6 => 0x07,
        KeyCode::Digit7 => 0x08,
        KeyCode::Digit8 => 0x09,
        KeyCode::Digit9 => 0x0A,
        KeyCode::Digit0 => 0x0B,
        KeyCode::Minus => 0x0C,
        KeyCode::Equal => 0x0D,
        KeyCode::Backspace => 0x0E,
        KeyCode::Tab => 0x0F,
        KeyCode::KeyQ => 0x10,
        KeyCode::KeyW => 0x11,
        KeyCode::KeyE => 0x12,
        KeyCode::KeyR => 0x13,
        KeyCode::KeyT => 0x14,
        KeyCode::KeyY => 0x15,
        KeyCode::KeyU => 0x16,
        KeyCode::KeyI => 0x17,
        KeyCode::KeyO => 0x18,
        KeyCode::KeyP => 0x19,
        KeyCode::BracketLeft => 0x1A,
        KeyCode::BracketRight => 0x1B,
        KeyCode::Enter => 0x1C,
        KeyCode::KeyA => 0x1E,
        KeyCode::KeyS => 0x1F,
        KeyCode::KeyD => 0x20,
        KeyCode::KeyF => 0x21,
        KeyCode::KeyG => 0x22,
        KeyCode::KeyH => 0x23,
        KeyCode::KeyJ => 0x24,
        KeyCode::KeyK => 0x25,
        KeyCode::KeyL => 0x26,
        KeyCode::Semicolon => 0x27,
        KeyCode::Quote => 0x28,
        KeyCode::Backquote => 0x29,
        KeyCode::Backslash => 0x2B,
        KeyCode::KeyZ => 0x2C,
        KeyCode::KeyX => 0x2D,
        KeyCode::KeyC => 0x2E,
        KeyCode::KeyV => 0x2F,
        KeyCode::KeyB => 0x30,
        KeyCode::KeyN => 0x31,
        KeyCode::KeyM => 0x32,
        KeyCode::Comma => 0x33,
        KeyCode::Period => 0x34,
        KeyCode::Slash => 0x35,
        KeyCode::NumpadMultiply => 0x37,
        KeyCode::Space => 0x39,
        KeyCode::Numpad7 => 0x47,
        KeyCode::Numpad8 => 0x48,
        KeyCode::Numpad9 => 0x49,
        KeyCode::NumpadSubtract => 0x4A,
        KeyCode::Numpad4 => 0x4B,
        KeyCode::Numpad5 => 0x4C,
        KeyCode::Numpad6 => 0x4D,
        KeyCode::NumpadAdd => 0x4E,
        KeyCode::Numpad1 => 0x4F,
        KeyCode::Numpad2 => 0x50,
        KeyCode::Numpad3 => 0x51,
        KeyCode::Numpad0 => 0x52,
        KeyCode::NumpadDecimal => 0x53,
        KeyCode::IntlBackslash => 0x56,
        KeyCode::NumpadEqual => 0x59,
        KeyCode::IntlRo => 0x73,
        KeyCode::IntlYen => 0x7D,
        KeyCode::NumpadComma => 0x7E,
        KeyCode::NumpadEnter => 0xE01C,
        KeyCode::NumpadDivide => 0xE035,
        _ => return None,
    })
}

/// The virtual key of a keypad key that produced text. A digit key is `VK_NUMPAD0`–`9`, and the
/// decimal key `VK_DECIMAL`, only while Num Lock is on, which a scan code cannot say (the layout
/// maps the same scan codes to `VK_INSERT` … `VK_DELETE`); a key that produced text says it was
/// on. `None` for every other key, whose virtual key the layout answers for its scan code.
fn numpad_text_virtual_key(physical_key: PhysicalKey) -> Option<u16> {
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };
    Some(match code {
        KeyCode::Numpad0 => 0x60,
        KeyCode::Numpad1 => 0x61,
        KeyCode::Numpad2 => 0x62,
        KeyCode::Numpad3 => 0x63,
        KeyCode::Numpad4 => 0x64,
        KeyCode::Numpad5 => 0x65,
        KeyCode::Numpad6 => 0x66,
        KeyCode::Numpad7 => 0x67,
        KeyCode::Numpad8 => 0x68,
        KeyCode::Numpad9 => 0x69,
        KeyCode::NumpadDecimal => 0x6E,
        _ => return None,
    })
}

/// **What a key sends when no program asked for a keyboard protocol** — the encoder as it
/// was before T-KEYBOARD-PROTOCOL, byte for byte (`key_encoding_legacy_{windows,macos}.tsv`).
pub(crate) fn legacy_bytes(
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
        // **F1–F12** (T-FKEYS): before this arm a function key no chrome rung
        // claimed fell to `None` and the program heard nothing. Every other named
        // key that reaches here has already been answered above, so the table
        // decides; anything it does not list (F13 and up, media keys) stays
        // `None`.
        Key::Named(named) => function_key(*named, modifiers, bt_platform::host_platform()),
        _ => None,
    }
}

/// How a function key is spelled in xterm's legacy table (which is also kitty's
/// legacy table): an `SS3` final byte for F1–F4, a `CSI n ~` number for F5–F12.
#[derive(Clone, Copy)]
enum FunctionKeyForm {
    Ss3(u8),
    Tilde(u8),
}

/// The twelve function keys a terminal encodes. The gaps in the numbers (no 16,
/// no 22) are the VT220's, kept by every terminal since.
const FUNCTION_KEYS: [(NamedKey, FunctionKeyForm); 12] = [
    (NamedKey::F1, FunctionKeyForm::Ss3(b'P')),
    (NamedKey::F2, FunctionKeyForm::Ss3(b'Q')),
    (NamedKey::F3, FunctionKeyForm::Ss3(b'R')),
    (NamedKey::F4, FunctionKeyForm::Ss3(b'S')),
    (NamedKey::F5, FunctionKeyForm::Tilde(15)),
    (NamedKey::F6, FunctionKeyForm::Tilde(17)),
    (NamedKey::F7, FunctionKeyForm::Tilde(18)),
    (NamedKey::F8, FunctionKeyForm::Tilde(19)),
    (NamedKey::F9, FunctionKeyForm::Tilde(20)),
    (NamedKey::F10, FunctionKeyForm::Tilde(21)),
    (NamedKey::F11, FunctionKeyForm::Tilde(23)),
    (NamedKey::F12, FunctionKeyForm::Tilde(24)),
];

/// A function key's bytes, or `None` for a key that is not one of the twelve,
/// is held with Super (the Windows / Command key never reaches the child), or
/// is Windows' close chord, `Alt+F4` with or without Shift or Ctrl on top.
///
/// Without modifiers F1–F4 are `SS3 P/Q/R/S`; with them they change shape to
/// `CSI 1;m P/Q/R/S`, which is [`cursor_key`]'s application-mode spelling —
/// DECCKM itself does not apply to function keys, so the mode is not asked.
/// F5–F12 are [`tilde_key`]'s `CSI n ~` / `CSI n;m ~`.
/// The platform is a value so a test on either machine can ask about the other.
fn function_key(
    key: NamedKey,
    modifiers: ModifiersState,
    platform: HostPlatform,
) -> Option<Vec<u8>> {
    if modifiers.super_key() {
        return None;
    }
    // Alt+F4 is Windows' close (DefWindowProc closes on WM_SYSKEYDOWN F4; Windows Terminal
    // does not forward it): a close a dialog cancels must not have typed `CSI 1;3S` into the pane.
    if platform == HostPlatform::Windows && key == NamedKey::F4 && modifiers.alt_key() {
        return None;
    }
    let (_, form) = FUNCTION_KEYS.iter().find(|(named, _)| *named == key)?;
    let modifier = xterm_modifier(modifiers);
    Some(match *form {
        FunctionKeyForm::Ss3(final_byte) => cursor_key(final_byte, modifier, true),
        FunctionKeyForm::Tilde(number) => tilde_key(number, modifier),
    })
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

/// **How many lines a paste is, as the reader would count them** (0.4.4 ticket 02).
///
/// The separators [`sanitize_paste`] turns into Enters — `\r`, `\n` and `\r\n`, each one line
/// break — plus one, **not counting a single trailing separator**: `"a\nb"` is 2, `"a\n"` is 1,
/// `"a\n\n"` is 2 and `"a"` is 1. A command copied off a web page usually carries its own
/// newline, and that is still one command; the reader meant it to run. Only a second line is
/// what the multi-line paste card is about.
///
/// One pass over the text and no allocation, because it is asked of every paste that reaches a
/// shell without bracketed paste, single lines included.
pub(crate) fn pasted_line_count(text: &str) -> usize {
    let mut separators = 0usize;
    let mut trailing = false;
    let mut bytes = text.bytes().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'\r' => {
                if bytes.peek() == Some(&b'\n') {
                    bytes.next();
                }
                separators += 1;
                trailing = true;
            }
            b'\n' => {
                separators += 1;
                trailing = true;
            }
            _ => trailing = false,
        }
    }
    separators + 1 - usize::from(trailing)
}

/// **The card's `Join into one line`** (0.4.4 ticket 02): every run of line separators becomes
/// one space, and nothing else changes.
///
/// No terminating `\r`, so nothing runs: the reader still presses Enter once, after reading the
/// line. And no `;` or `&&` is invented — a separator swallowed by a trailing `#` comment would
/// silently change what the script does, and writing shell syntax the reader did not write is
/// the heuristic CONVENTIONS §一 forbids. A function of its own beside [`sanitize_paste`] rather
/// than a flag on it: `Run line by line` must stay byte-identical to today's paste.
pub(crate) fn join_lines(text: &str) -> String {
    let mut joined = String::with_capacity(text.len());
    let mut in_break = false;
    for character in text.chars() {
        if matches!(character, '\r' | '\n') {
            if !in_break {
                joined.push(' ');
                in_break = true;
            }
        } else {
            joined.push(character);
            in_break = false;
        }
    }
    joined
}

/// **The lines of a pasted block, as [`pasted_line_count`] counts them** — `\r\n`, `\r` and
/// `\n` each end one, and a single trailing separator ends the last rather than opening another.
fn pasted_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut start, mut at) = (0, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'\r' => {
                lines.push(&text[start..at]);
                at += if bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = at;
            }
            b'\n' => {
                lines.push(&text[start..at]);
                at += 1;
                start = at;
            }
            _ => at += 1,
        }
    }
    if start < text.len() || lines.is_empty() {
        lines.push(&text[start..]);
    }
    lines
}

/// **Does this line hand its command on to the next one with `mark`?** Trailing whitespace is
/// not part of the question, and a doubled mark is the mark written literally (`^^` in cmd, two
/// backticks in PowerShell, `\\` in a POSIX shell), so it is an odd run of marks that continues.
fn continues_with(line: &str, mark: char) -> bool {
    line.trim_end()
        .chars()
        .rev()
        .take_while(|character| *character == mark)
        .count()
        % 2
        == 1
}

/// **Is this block one command wrapped across lines?** (0.4.4 ticket 45) — every line but the
/// last ends with `mark`, the shell's own continuation mark.
///
/// A block of one line is not wrapped, and a blank line in the middle ends no command with a
/// mark, so a block with one is a block of commands.
pub(crate) fn continued_by(text: &str, mark: char) -> bool {
    let lines = pasted_lines(text);
    lines.len() > 1
        && lines[..lines.len() - 1]
            .iter()
            .all(|line| continues_with(line, mark))
}

/// **The card's `Join into one line` for a block [`continued_by`] `mark`** (0.4.4 ticket 45): the
/// marks it recognised come off — a `^` at a line's end is not part of the command once the
/// line has been joined — and the lines join with one space. A continuation line's indent is
/// the wrap's, not the command's, so it goes with the break. No terminating `\r`, for
/// [`join_lines`]'s reason.
pub(crate) fn join_continued_lines(text: &str, mark: char) -> String {
    let lines = pasted_lines(text);
    let last = lines.len() - 1;
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let line = if index == 0 { line } else { line.trim_start() };
            if index == last {
                return line;
            }
            let line = line.trim_end();
            line.strip_suffix(mark).unwrap_or(line).trim_end()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// **Shift+Enter, down and up, as two win32-input-mode key records** (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`:
/// `VK_RETURN` 13, scan code 28, character `\r`, key down then up, `SHIFT_PRESSED` 0x10, one
/// repeat) — PSReadLine's `AddLine`, which puts a line break into the edit buffer and runs
/// nothing (0.4.4 ticket 03, the spike's arm B).
///
/// ConPTY announces win32-input-mode (`?9001h`) at the head of every session and parses a record
/// written inside an otherwise plain-VT stream without Folio adopting the mode; the 2026-09-23
/// spike measured it on Folio's ConPTY and on the inbox one, and nothing leaked as text.
const SHIFT_ENTER_RECORDS: &[u8] = b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_";

/// **Would PSReadLine's own paste put on the input line exactly what [`sanitize_paste`] would
/// send?** (0.4.4 ticket 03) — which is the whole of whether a paste into a PowerShell prompt
/// may take the clipboard road (`bt_pty::PSREADLINE_PASTE_INPUT`).
///
/// On that road the shell reads the clipboard itself and Folio's cleanup never runs. PSReadLine's
/// `Paste` (the same body in 2.0.0, 2.4.5 and master: `KillYank.cs`) removes every `\r` and turns
/// every tab into four spaces, and inserts everything else as it is. So the two agree exactly
/// when the text holds **no control character but a tab, a `\n`, or a `\r` directly before a
/// `\n`**: a `\r\n` is one break on both roads. Anything else — a character `sanitize_paste`
/// drops (an escape, a bracketed-paste terminator, a bell) or a lone `\r`, which Folio sends as a
/// break and PSReadLine would delete, gluing two lines together — is text Folio has to change,
/// and it goes by [`input_line_bytes`] instead, which carries Folio's own bytes.
///
/// A tab is not in that list, on purpose: sent as a byte it is the Tab key and PSReadLine
/// *completes* something, while its own paste inserts four spaces — the clipboard road is the
/// better of the two for it, not a change Folio has to make.
///
/// One pass, no allocation.
pub(crate) fn psreadline_pastes_it_unchanged(text: &str) -> bool {
    let mut bytes = text.bytes().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'\t' | b'\n' => {}
            b'\r' if bytes.peek() == Some(&b'\n') => {}
            // `char::is_control` is Cc: C0, DEL and C1. The C1 controls are two bytes in UTF-8,
            // `0xC2 0x80..=0x9F`, and no other character's encoding contains that pair.
            0xC2 if bytes
                .peek()
                .is_some_and(|next| (0x80..=0x9F).contains(next)) =>
            {
                return false;
            }
            byte if byte < 0x20 || byte == 0x7F => return false,
            _ => {}
        }
    }
    true
}

/// **A block for a PowerShell prompt, carried as Folio's own bytes** (0.4.4 ticket 03, the
/// spike's C2): exactly what [`sanitize_paste`] sends, with every line break — each `\r` it
/// produced — written as [`SHIFT_ENTER_RECORDS`] instead of an Enter.
///
/// The road for a paste [`psreadline_pastes_it_unchanged`] says Folio has to change, so it still
/// lands whole on the input line and runs nothing until the reader's Enter. Everything else is
/// today's bytes, a tab included (it is the Tab key there, as it always was).
pub(crate) fn input_line_bytes(text: &str) -> Vec<u8> {
    let sanitized = sanitize_paste(text);
    let breaks = sanitized.iter().filter(|&&byte| byte == b'\r').count();
    let mut bytes = Vec::with_capacity(sanitized.len() + breaks * (SHIFT_ENTER_RECORDS.len() - 1));
    for byte in sanitized {
        if byte == b'\r' {
            bytes.extend_from_slice(SHIFT_ENTER_RECORDS);
        } else {
            bytes.push(byte);
        }
    }
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
    // The three spellings a key can have when the platform could not name it,
    // and one it could — only the tests need to build these by hand.
    use winit::keyboard::{KeyCode, NativeKey, NativeKeyCode};

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

    // ── Text that arrived with no key (T-REMOTE-INPUT-PACKET) ──────────────
    //
    // `winit::event::KeyEvent` cannot be built outside winit — its
    // `platform_specific` field is `pub(crate)`, so there is no synthetic event
    // to hand a rung. What these hold is the decision the rung is handed, which
    // is the whole of what this ticket added: the three fields in, the key out.

    /// The two key fields exactly as winit fills them for a `VK_PACKET` press:
    /// scancode zero, and `VK_PACKET` (0xE7) as the virtual key.
    fn injected(text: Option<&str>) -> Option<Key> {
        injected_logical_key(
            PhysicalKey::Unidentified(NativeKeyCode::Windows(0)),
            &Key::Unidentified(NativeKey::Windows(0xE7)),
            text,
        )
    }

    /// The ticket itself: `这` sent from a phone keyboard is the key that would
    /// have typed `这`, and so reaches the child as its own UTF-8.
    #[test]
    fn an_injected_character_is_the_key_that_would_have_typed_it() {
        let key = injected(Some("这")).expect("a character with no key is that character's key");
        assert_eq!(key, Key::Character("这".into()));
        assert_eq!(
            legacy_bytes(&key, ModifiersState::empty(), false),
            Some("这".as_bytes().to_vec()),
            "the encoder had no verb for the key winit reported, which is where the text was lost"
        );
    }

    /// Several characters delivered as one press are one insert, not none: an
    /// injector is free to hand over a whole word.
    #[test]
    fn injected_text_longer_than_a_character_is_still_that_text() {
        assert_eq!(injected(Some("你好")), Some(Key::Character("你好".into())));
        assert_eq!(
            legacy_bytes(
                &Key::Character("你好".into()),
                ModifiersState::empty(),
                false
            ),
            Some("你好".as_bytes().to_vec())
        );
    }

    /// **A key winit could name is never rewritten**, in either field. This is
    /// what keeps the rule from touching ordinary typing at all.
    #[test]
    fn a_key_that_names_itself_is_left_alone() {
        assert_eq!(
            injected_logical_key(
                PhysicalKey::Code(KeyCode::KeyA),
                &Key::Unidentified(NativeKey::Windows(0xE7)),
                Some("a"),
            ),
            None,
            "a press with a physical key is a press, whatever its logical key says"
        );
        assert_eq!(
            injected_logical_key(
                PhysicalKey::Unidentified(NativeKeyCode::Windows(0)),
                &Key::Character("a".into()),
                Some("a"),
            ),
            None,
            "and a press the ladder can already route must not be rewritten under it"
        );
        assert_eq!(
            injected_logical_key(
                PhysicalKey::Unidentified(NativeKeyCode::Windows(0)),
                &Key::Named(NamedKey::Enter),
                Some("\r"),
            ),
            None
        );
    }

    /// An event with no key **and** no text is nothing this rule has anything to
    /// say about — every dead key and modifier report on every platform.
    #[test]
    fn a_key_with_no_text_stays_unidentified() {
        assert_eq!(injected(None), None);
        assert_eq!(injected(Some("")), None);
    }

    /// **A control code is the key behind it, alone or not at all.**
    /// `Key::Character("\r")` is a value no rung in this window answers —
    /// `keyboard_bytes`' character arm excludes control characters — so an
    /// injected newline would be swallowed by the very arm it was routed to.
    #[test]
    fn an_injected_control_code_is_the_key_that_produces_it() {
        for (text, key) in [
            ("\r", NamedKey::Enter),
            ("\n", NamedKey::Enter),
            ("\t", NamedKey::Tab),
            ("\u{8}", NamedKey::Backspace),
            ("\u{1b}", NamedKey::Escape),
        ] {
            assert_eq!(
                injected(Some(text)),
                Some(Key::Named(key)),
                "injected {text:?} is {key:?}"
            );
            assert!(
                legacy_bytes(&Key::Named(key), ModifiersState::empty(), false).is_some(),
                "and {key:?} is a key the encoder answers"
            );
        }
        assert_eq!(
            injected(Some("\u{7}")),
            None,
            "a control code with no key behind it names none"
        );
    }

    /// A control code inside a run of text is neither a key press nor text, and
    /// guessing which half to keep would be this window inventing keystrokes.
    #[test]
    fn injected_text_with_a_control_code_in_it_is_refused() {
        for text in ["a\rb", "\r\n", "\rx", "x\n"] {
            assert_eq!(injected(Some(text)), None, "injected {text:?}");
        }
    }

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
                        legacy_bytes(&Key::Named(key), modifiers, application_mode),
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
                        legacy_bytes(&Key::Named(key), modifiers, application_mode),
                        Some(expected.into_bytes()),
                        "key={key:?} application_mode={application_mode} modifiers={modifiers:?}"
                    );
                }
            }
        }
    }

    /// RED (T-FKEYS) — **every function key reaches the program, in xterm's
    /// legacy form.**
    ///
    /// Before this ticket `keyboard_bytes` had no case for F1–F12, so a function
    /// key no chrome rung claimed fell to `None` and the program heard nothing:
    /// vim's `F1` help, htop's `F10` quit, mc's whole menu row, PSReadLine's
    /// `F2`/`F7`. The twelve expectations are written out as literals rather than
    /// rebuilt from the table, so a wrong number in the table cannot agree with
    /// itself here; and DECCKM is asked both ways because it does not apply to
    /// function keys — `SS3 P` is the bare form in either cursor mode.
    ///
    /// MUTATION: swap F11's `23` for `22` in `FUNCTION_KEYS` (or drop the
    /// `Key::Named(named) => function_key(..)` arm) and this goes red.
    #[test]
    fn every_function_key_reaches_the_program_in_its_legacy_form() {
        let expected: [(NamedKey, &[u8]); 12] = [
            (NamedKey::F1, b"\x1bOP"),
            (NamedKey::F2, b"\x1bOQ"),
            (NamedKey::F3, b"\x1bOR"),
            (NamedKey::F4, b"\x1bOS"),
            (NamedKey::F5, b"\x1b[15~"),
            (NamedKey::F6, b"\x1b[17~"),
            (NamedKey::F7, b"\x1b[18~"),
            (NamedKey::F8, b"\x1b[19~"),
            (NamedKey::F9, b"\x1b[20~"),
            (NamedKey::F10, b"\x1b[21~"),
            (NamedKey::F11, b"\x1b[23~"),
            (NamedKey::F12, b"\x1b[24~"),
        ];
        for application_mode in [false, true] {
            for (key, bytes) in expected {
                assert_eq!(
                    legacy_bytes(&Key::Named(key), ModifiersState::empty(), application_mode),
                    Some(bytes.to_vec()),
                    "key={key:?} application_mode={application_mode}"
                );
            }
        }
    }

    /// RED (T-FKEYS) — **Shift, Alt and Ctrl on a function key are encoded as in
    /// xterm: F1–F4 change shape to `CSI 1;m P/Q/R/S`, F5–F12 take `;m` before
    /// the `~`.**
    ///
    /// F1 and F5 stand for the two shapes. The first is the one a hand-written
    /// encoder gets wrong — `SS3` carries no parameters, so a modified F1 is not
    /// `ESC O 5 P` but a CSI sequence with a `1` in front of the modifier, the
    /// same spelling as a modified arrow key. The modifier values are xterm's
    /// (Shift 2, Alt 3, Ctrl 5, Ctrl+Shift 6), written out rather than taken from
    /// `xterm_modifier`.
    ///
    /// MUTATION: have `function_key` pass `1` instead of `xterm_modifier(..)` (drop
    /// the modifier) and every line goes red.
    #[test]
    fn a_modifier_on_a_function_key_is_encoded_as_in_xterm() {
        let cases: [(NamedKey, ModifiersState, &[u8]); 8] = [
            (NamedKey::F1, ModifiersState::SHIFT, b"\x1b[1;2P"),
            (NamedKey::F1, ModifiersState::ALT, b"\x1b[1;3P"),
            (NamedKey::F1, ModifiersState::CONTROL, b"\x1b[1;5P"),
            (
                NamedKey::F1,
                ModifiersState::CONTROL.union(ModifiersState::SHIFT),
                b"\x1b[1;6P",
            ),
            (NamedKey::F5, ModifiersState::SHIFT, b"\x1b[15;2~"),
            (NamedKey::F5, ModifiersState::ALT, b"\x1b[15;3~"),
            (NamedKey::F5, ModifiersState::CONTROL, b"\x1b[15;5~"),
            (
                NamedKey::F5,
                ModifiersState::CONTROL.union(ModifiersState::SHIFT),
                b"\x1b[15;6~",
            ),
        ];
        for application_mode in [false, true] {
            for (key, modifiers, bytes) in cases {
                assert_eq!(
                    legacy_bytes(&Key::Named(key), modifiers, application_mode),
                    Some(bytes.to_vec()),
                    "key={key:?} modifiers={modifiers:?} application_mode={application_mode}"
                );
            }
        }
    }

    /// RED (T-FKEYS) — **a function key held with Super reaches nothing, and a
    /// function key past F12 sends nothing.**
    ///
    /// Super is Folio's standing rule for every key (the Windows / Command key
    /// is held to reach another program's verb and never reaches the child); a
    /// table that encoded the other modifiers would otherwise spell `Win+F1` as a
    /// bare `SS3 P`, since xterm has no bit for Super. F13 and up have no legacy
    /// form this encoder writes, and the new arm must not invent one.
    ///
    /// MUTATION: drop the `modifiers.super_key()` return in `function_key` and
    /// the Super lines go red.
    #[test]
    fn a_function_key_with_super_or_past_f12_sends_nothing() {
        let win = ModifiersState::SUPER;
        assert_eq!(legacy_bytes(&Key::Named(NamedKey::F1), win, false), None);
        assert_eq!(
            legacy_bytes(
                &Key::Named(NamedKey::F5),
                win.union(ModifiersState::SHIFT),
                false
            ),
            None,
            "and it is not talked out of it by a second modifier"
        );
        assert_eq!(
            legacy_bytes(&Key::Named(NamedKey::F13), ModifiersState::empty(), false),
            None
        );
        assert_eq!(
            legacy_bytes(&Key::Named(NamedKey::F13), ModifiersState::CONTROL, true),
            None
        );
    }

    /// RED (T-FKEYS, coordinator's ruling 2026-09-29) — **on Windows, Alt+F4 is
    /// the system's close chord and never reaches the child; elsewhere it is
    /// `CSI 1;3S` like any other modified F4.**
    ///
    /// DefWindowProc closes the window on `WM_SYSKEYDOWN F4`, and the key event
    /// still arrives here on its way; if a dialog then cancels the close, the
    /// pane must not have been typed `CSI 1;3S`. Windows Terminal does not
    /// forward it either. Option+F4 has no system meaning on macOS, and none on
    /// the other Unixes this build names, so there it is the program's. Shift
    /// or Ctrl on top does not change Windows' answer. The platform is passed as
    /// a value (bt-app asks `bt_platform::host_platform()` rather than naming a
    /// platform in this file), so both halves run on every machine; the last
    /// assertion is the host's own road through `keyboard_bytes`.
    ///
    /// MUTATION: drop the `platform == HostPlatform::Windows` return in
    /// `function_key` and the Windows lines go red.
    #[test]
    fn alt_f4_is_the_windows_close_and_reaches_no_child_there() {
        let alt = ModifiersState::ALT;
        let chords = [
            alt,
            alt.union(ModifiersState::SHIFT),
            alt.union(ModifiersState::CONTROL),
            alt.union(ModifiersState::CONTROL)
                .union(ModifiersState::SHIFT),
        ];
        for modifiers in chords {
            assert_eq!(
                function_key(NamedKey::F4, modifiers, HostPlatform::Windows),
                None,
                "{modifiers:?}"
            );
        }
        assert_eq!(
            function_key(NamedKey::F4, ModifiersState::CONTROL, HostPlatform::Windows),
            Some(b"\x1b[1;5S".to_vec()),
            "only Alt makes it the close chord"
        );
        assert_eq!(
            function_key(NamedKey::F5, alt, HostPlatform::Windows),
            Some(b"\x1b[15;3~".to_vec()),
            "and only on F4"
        );
        for platform in [HostPlatform::MacOs, HostPlatform::OtherUnix] {
            assert_eq!(
                function_key(NamedKey::F4, alt, platform),
                Some(b"\x1b[1;3S".to_vec()),
                "{platform:?}"
            );
            assert_eq!(
                function_key(NamedKey::F4, alt.union(ModifiersState::CONTROL), platform),
                Some(b"\x1b[1;7S".to_vec()),
                "{platform:?}"
            );
        }
        let host = legacy_bytes(&Key::Named(NamedKey::F4), alt, false);
        if bt_platform::host_platform() == HostPlatform::Windows {
            assert_eq!(host, None);
        } else {
            assert_eq!(host, Some(b"\x1b[1;3S".to_vec()));
        }
    }

    #[test]
    fn tab_meta_and_legacy_controls_have_terminal_encodings() {
        assert_eq!(
            legacy_bytes(&Key::Named(NamedKey::Tab), ModifiersState::SHIFT, false),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            legacy_bytes(&Key::Character("x".into()), ModifiersState::ALT, false),
            Some(b"\x1bx".to_vec())
        );
        assert_eq!(
            legacy_bytes(&Key::Character("é".into()), ModifiersState::ALT, false),
            Some("\u{1b}é".as_bytes().to_vec())
        );
        assert_eq!(
            legacy_bytes(&Key::Named(NamedKey::Space), ModifiersState::ALT, false),
            Some(b"\x1b ".to_vec())
        );
        assert_eq!(
            legacy_bytes(&Key::Character("c".into()), ModifiersState::CONTROL, false),
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
                legacy_bytes(
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
            legacy_bytes(
                &Key::Character("\u{2}".into()),
                ModifiersState::CONTROL,
                false
            ),
            Some(vec![0x02])
        );
        // The punctuation with a code, and one without.
        assert_eq!(
            legacy_bytes(&Key::Character("[".into()), ModifiersState::CONTROL, false),
            Some(vec![0x1b])
        );
        assert_eq!(
            legacy_bytes(&Key::Character("_".into()), ModifiersState::CONTROL, false),
            Some(vec![0x1f])
        );
        assert_eq!(
            legacy_bytes(&Key::Character("1".into()), ModifiersState::CONTROL, false),
            None
        );
        // Alt on top prefixes ESC.
        assert_eq!(
            legacy_bytes(
                &Key::Character("b".into()),
                ModifiersState::CONTROL | ModifiersState::ALT,
                false
            ),
            Some(vec![0x1b, 0x02])
        );
        // Ctrl+V stays the paste door and is not encoded here.
        assert_eq!(
            legacy_bytes(&Key::Character("v".into()), ModifiersState::CONTROL, false),
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

    /// RED (0.4.4 ticket 03) — **the line-break record is Shift+Enter, down and up, and nothing
    /// else changes.**
    ///
    /// The bytes are the spike's arm B, byte for byte — the only encoding measured to land a
    /// break on PSReadLine's input line without running it — and the text between the breaks is
    /// `sanitize_paste`'s, so a paste that goes this way loses exactly what today's road loses.
    ///
    /// MUTATION: write `KeyDown 0` in the first record of `SHIFT_ENTER_RECORDS`, or push the `\r`
    /// as well as the records in `input_line_bytes`.
    #[test]
    fn the_line_break_record_is_shift_enter_and_nothing_else() {
        assert_eq!(
            input_line_bytes("a\r\nb\nc"),
            b"a\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_b\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_c"
        );
        // What `sanitize_paste` drops is dropped here too, and a tab stays the byte it always was.
        assert_eq!(
            input_line_bytes("x\x1b[201~\x07\ty\rz"),
            b"x\ty\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_z"
        );
        // Without a break it is today's bytes.
        assert_eq!(input_line_bytes("dir\tx"), paste_bytes("dir\tx", false));
    }

    /// RED (0.4.4 ticket 03) — **the clipboard road is taken only when PSReadLine's own paste
    /// would land what Folio's cleanup would have sent.**
    ///
    /// PSReadLine's `Paste` deletes every `\r` and inserts everything else as it is. So `\r\n` and
    /// `\n` agree with `sanitize_paste`, and a tab is its own case (four spaces there, the Tab key
    /// here — the clipboard road is the better one). A lone `\r` does not agree: Folio sends it
    /// as a break and PSReadLine would glue the two lines together. Neither does a control
    /// character, which `sanitize_paste` drops and PSReadLine would insert.
    ///
    /// MUTATION: accept any `\r` in `psreadline_pastes_it_unchanged` — `"a\rb"` goes green.
    #[test]
    fn only_text_folio_would_not_change_takes_the_clipboard_road() {
        for unchanged in [
            "a",
            "a\r\nb\r\nc",
            "a\nb\n",
            "if ($x) {\n\t'yes'\n}",
            "中文\r\n路径",
            "",
        ] {
            assert!(psreadline_pastes_it_unchanged(unchanged), "{unchanged:?}");
        }
        for changed in [
            "a\rb",
            "a\r",
            "a\x1b[201~\nb",
            "a\x07\nb",
            "a\u{85}\nb",
            "a\x7f\nb",
            "a\0\nb",
        ] {
            assert!(!psreadline_pastes_it_unchanged(changed), "{changed:?}");
        }
    }

    /// RED (0.4.4 ticket 02) — **a paste is as many lines as the reader sees, and the newline a
    /// copied command carries does not make it two.**
    ///
    /// The card is raised on `> 1`, so this count is the whole of "is this a multi-line paste".
    /// A count that included the trailing separator would ask about every command copied off a
    /// web page; one that collapsed a blank line would under-count a block the reader can see.
    ///
    /// MUTATION: drop the `- usize::from(trailing)` term in `pasted_line_count` — `"a\n"` reads
    /// as 2 and the second assertion goes red.
    #[test]
    fn pasted_line_count_ignores_one_trailing_separator() {
        assert_eq!(pasted_line_count("a\nb"), 2);
        assert_eq!(pasted_line_count("a\n"), 1);
        assert_eq!(pasted_line_count("a\n\n"), 2);
        assert_eq!(pasted_line_count("a"), 1);
        // The three spellings of a break are one break each, as `sanitize_paste` has them.
        assert_eq!(pasted_line_count("a\r\nb\rc\nd"), 4);
        assert_eq!(pasted_line_count("a\r\n"), 1);
        assert_eq!(pasted_line_count(""), 1);
        // And the count agrees with the Enters today's road would send.
        let text = "one\r\ntwo\nthree";
        let enters = paste_bytes(text, false)
            .iter()
            .filter(|byte| **byte == b'\r')
            .count();
        assert_eq!(pasted_line_count(text), enters + 1);
    }

    /// RED (0.4.4 ticket 02) — **`Join into one line` sends no carriage return, so nothing runs
    /// until the reader presses Enter.**
    ///
    /// The joined text is sent through the very door the paste always used (`paste_bytes`), so
    /// the claim is made of those bytes and not of the string: a join that left a `\r` anywhere
    /// would run the first half of the line as a command.
    ///
    /// MUTATION: make `join_lines` push `'\r'` instead of `' '` — every assertion goes red.
    #[test]
    fn joining_a_block_sends_no_carriage_return() {
        for text in ["one\r\ntwo\nthree\r", "a\n\n\nb", "\r\nlead", "tail\n"] {
            let bytes = paste_bytes(&join_lines(text), false);
            assert!(
                !bytes.contains(&b'\r') && !bytes.contains(&b'\n'),
                "{text:?} joined still carries a line break: {bytes:?}"
            );
        }
        assert_eq!(join_lines("one\r\ntwo\nthree"), "one two three");
    }

    /// RED (0.4.4 ticket 02) — **a join writes one space per break and never shell syntax.**
    ///
    /// `;` and `&&` are grammar, and which one is right depends on the shell and on whether the
    /// line before ends in a comment. Folio cannot know either, so it writes neither: every run of
    /// breaks is one space and every other character arrives as it was copied.
    ///
    /// MUTATION: join with `" && "` — the equality assertions go red.
    #[test]
    fn joining_never_invents_a_separator() {
        assert_eq!(
            join_lines("cd src\ncargo build # fast\n"),
            "cd src cargo build # fast "
        );
        assert_eq!(
            join_lines("a\r\n\r\nb"),
            "a b",
            "a run of breaks is one space"
        );
        let text = "echo 1\necho 2\r\necho 3";
        let joined = join_lines(text);
        assert!(!joined.contains(';') && !joined.contains('&'));
        assert_eq!(
            joined
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>(),
            text.chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>(),
            "every other character arrives as it was copied"
        );
    }

    /// RED (0.4.4 ticket 02) — **a dropped or copied file's path can never be a multi-line
    /// paste**, so the drop road's exemption from the card is a proof and not a belief.
    ///
    /// Runs the real producer: real paths, two of them carrying a line break, through
    /// `shell_literal::paths_text` for every grammar a pane can have. A path with a break in it is
    /// refused by `shell_literal::representable`, so the text that reaches `deliver_paste` from
    /// the `Files` arm is always one line.
    ///
    /// MUTATION: let `representable` accept a control character — the broken path is spelled and
    /// the count reads 2.
    #[test]
    fn a_path_insertion_can_never_be_multi_line() {
        use crate::shell_literal::{self, Encoder, Recipient, ShellGrammar};
        let dir = std::env::temp_dir();
        let paths = vec![
            dir.join("plain.txt"),
            dir.join("two\nlines.txt"),
            dir.join("carriage\rreturn.txt"),
            dir.join("with space.md"),
        ];
        for grammar in [
            ShellGrammar::PowerShell,
            ShellGrammar::Cmd,
            ShellGrammar::Posix,
            ShellGrammar::Fish,
            ShellGrammar::Nushell,
            ShellGrammar::Agent,
        ] {
            let recipient = Recipient {
                encoder: Encoder {
                    grammar,
                    named_cmd: grammar == ShellGrammar::Cmd,
                    delayed_expansion: false,
                    powershell_doubled_quotes: &[],
                },
                namespace: bt_transcript::paths::PrintedPathNamespace::Windows,
                spelling: None,
                wsl_distribution: None,
            };
            for leading_space in [false, true] {
                let insertion = shell_literal::paths_text(&paths, &recipient, leading_space);
                assert_eq!(
                    pasted_line_count(&insertion.text),
                    1,
                    "{grammar:?} spelled a path insertion over two lines: {:?}",
                    insertion.text
                );
                assert!(
                    !insertion.refused.is_empty(),
                    "the broken paths are refused"
                );
            }
        }
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
            legacy_bytes(&key, ModifiersState::CONTROL, false),
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
        assert_eq!(legacy_bytes(&paste, ctrl_shift, false), None);
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
            legacy_bytes(&insert, ModifiersState::CONTROL, false),
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
            if let Some(bytes) = legacy_bytes(key, *modifiers, false) {
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
        assert_eq!(legacy_bytes(&character("j"), win, false), None);
        assert_eq!(legacy_bytes(&Key::Named(NamedKey::Space), win, false), None);
        assert_eq!(
            legacy_bytes(&character("j"), win.union(ModifiersState::SHIFT), false),
            None,
            "and it is not talked out of it by a second modifier"
        );
        // The same two keys with the Windows key up are the child's, unchanged.
        assert_eq!(
            legacy_bytes(&character("j"), ModifiersState::empty(), false),
            Some(b"j".to_vec())
        );
        assert_eq!(
            legacy_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty(), false),
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
                legacy_bytes(&character(letter), ModifiersState::CONTROL, false),
                Some(vec![byte]),
                "Ctrl+{letter} is the child's control code on every platform"
            );
        }
        // And the modifier that is the *application's* there produces no byte at
        // all — a Command chord this table does not claim is not a keystroke the
        // shell should hear the letter of.
        for letter in ["c", "d", "z", "t", "w", "q"] {
            assert_eq!(
                legacy_bytes(&character(letter), CMD, false),
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
                legacy_bytes(&character(letter), ModifiersState::empty(), false),
                Some(letter.as_bytes().to_vec()),
                "{letter} is what the keyboard produced and the child hears it"
            );
        }
        // With Alt genuinely held it is still `ESC` and the same bytes, which is
        // the one case the old guard let through and the reason the fault hid.
        assert_eq!(
            legacy_bytes(&character("«"), ModifiersState::ALT, false),
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
            legacy_bytes(&character("å"), composed, false),
            Some("å".as_bytes().to_vec()),
            "Option is text, so the child hears the character and nothing else"
        );

        // On — winit reports the raw letter instead and the Alt comes through,
        // which is the other policy, whole.
        let meta = effective_modifiers(ModifiersState::ALT, true, MAC);
        assert!(meta.alt_key());
        assert_eq!(
            legacy_bytes(&character("a"), meta, false),
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

    /// RED (T-MAC-LIVE, §13.33 ①) — **what this door answers is a question
    /// about text, and it is not the only question anybody asks of a Mac's
    /// `Option` key.**
    ///
    /// Measured on the Mac (2026-09-12, an Option-flagged wheel notch posted at
    /// Folio's own window): winit reported `ModifiersState(ALT)` and this
    /// function handed the window `ModifiersState(0x0)`. That is the right
    /// answer for the encoder, for every chord and for every one-line field —
    /// and the wrong one for `⌥`+wheel, which composes no character and has no
    /// setting of its own to be switched off by. So the state this door is
    /// *given* stays live beside the state it returns: see
    /// `WindowRuntime::modifiers_held` and `column_notch`.
    ///
    /// MUTATION: hand `column_notch` the effective state and the second half of
    /// this goes red — which is what shipped, and what no keyboard test could
    /// have said, because on the keyboard the first half is correct.
    #[test]
    fn a_stripped_option_is_the_answer_for_text_and_not_for_a_wheel() {
        let reported = ModifiersState::ALT;
        assert!(
            !effective_modifiers(reported, false, MAC).alt_key(),
            "the keyboard half of the ruling is unchanged: nobody is holding Alt"
        );
        assert!(
            reported.alt_key(),
            "and the hand is still holding Option, which is the fact a gesture \
             is entitled to read"
        );
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

    /// RED (T-MAC-CMDCLICK, §13.45 ①) — **the pointer's hand-over modifier is
    /// `Ctrl` here and `⌘` on a Mac**, both platforms and both states.
    ///
    /// The whole of the ticket in four asserts. A build that answered
    /// `control_key()` on both sides compiles, passes every Windows test in
    /// this workspace, and spends the Mac's secondary click on handing a link
    /// to the system — two verbs on one press, and the one the reader gets is
    /// whichever arm this file happens to ask first.
    ///
    /// MUTATION: return `modifiers.control_key()` unconditionally.
    #[test]
    fn the_pointer_chord_is_control_here_and_command_on_a_mac() {
        assert!(pointer_chord_held_on(ModifiersState::CONTROL, WINDOWS));
        assert!(!pointer_chord_held_on(CMD, WINDOWS));

        assert!(pointer_chord_held_on(CMD, MAC));
        assert!(
            !pointer_chord_held_on(ModifiersState::CONTROL, MAC),
            "Control is the secondary click on that desk and cannot also hand a \
             reference over"
        );

        for platform in [WINDOWS, MAC] {
            assert!(
                !pointer_chord_held_on(ModifiersState::empty(), platform),
                "a bare click is never a hand-over on {platform:?}"
            );
            assert!(
                !pointer_chord_held_on(ModifiersState::SHIFT, platform),
                "Shift is the selection's on {platform:?}, not the system's"
            );
        }
    }

    /// PIN (T-MAC-CMDCLICK, §13.45 ①) — **one dialect, not two.**
    ///
    /// A pointer's hand-over modifier and a keyboard's application chord are
    /// the same key on each platform, which is why the pointer's door is read
    /// off the keyboard's rather than matching `HostPlatform` a second time.
    /// Two matches for one sentence is how a sentence comes to be two.
    #[test]
    fn the_pointer_and_the_keyboard_spell_the_applications_modifier_the_same_way() {
        for platform in [WINDOWS, MAC] {
            for modifiers in [
                ModifiersState::empty(),
                ModifiersState::CONTROL,
                CMD,
                ModifiersState::SHIFT,
                ModifiersState::ALT,
                CMD.union(ModifiersState::CONTROL),
            ] {
                assert_eq!(
                    pointer_chord_held_on(modifiers, platform),
                    is_command_chord_on(modifiers, platform),
                    "{modifiers:?} on {platform:?}"
                );
            }
        }
    }

    /// RED (T-MAC-CMDCLICK, §13.45 ②) — **Control+click is the secondary click
    /// on a Mac, and nowhere else.**
    ///
    /// winit reads the button off `NSEvent`'s `buttonNumber`, which is 0 for a
    /// control-click, so the translation is this window's to make. Measured on
    /// the machine: the trace's first station reports `button=Left` for a
    /// Control-flagged `CGEvent` press, and the pane's menu comes up.
    ///
    /// MUTATION: drop the `platform == MacOs` guard and a Windows Control+click
    /// stops handing a link to the system and raises the pane's menu instead.
    #[test]
    fn control_click_is_the_secondary_click_only_on_a_mac() {
        assert_eq!(
            pressed_button(MouseButton::Left, ModifiersState::CONTROL, MAC),
            MouseButton::Right,
            "a control-click on that desk is how a one-button mouse asks for a menu"
        );
        assert_eq!(
            pressed_button(MouseButton::Left, ModifiersState::CONTROL, WINDOWS),
            MouseButton::Left,
            "here Control+click hands a reference to the system and is not a menu"
        );
        assert_eq!(
            pressed_button(MouseButton::Left, ModifiersState::empty(), MAC),
            MouseButton::Left
        );
        assert_eq!(
            pressed_button(MouseButton::Left, CMD, MAC),
            MouseButton::Left,
            "the hand-over modifier is not the secondary click"
        );
        // The other buttons are the platform's own report and are never
        // rewritten — a right press wearing Control is already a right press,
        // and a middle click closes a tab on both desks.
        for button in [
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Back,
            MouseButton::Forward,
        ] {
            for platform in [WINDOWS, MAC] {
                assert_eq!(
                    pressed_button(button, ModifiersState::CONTROL, platform),
                    button,
                    "{button:?} on {platform:?}"
                );
            }
        }
    }

    /// RED (T-MAC-CMDCLICK, §13.45 ②) — **a release is spelled the way its own
    /// press was spelled**, even when the hand let Control go first.
    ///
    /// Control is a key and a button is a button, and nothing makes a reader
    /// lift them in order. Without the latch the platform reports a plain left
    /// release after a press this window took as the secondary one, and that
    /// release travels: `route_forwarded_mouse_button` spells its release off
    /// the argument, so a mouse-tracking program would be handed a right press
    /// and a left release it can pair with nothing.
    ///
    /// MUTATION: answer `pressed_button(reported, modifiers, platform)` on the
    /// release arm instead of the latch, and the third assert goes red.
    #[test]
    fn a_release_is_spelled_the_way_its_own_press_was() {
        let mut held = false;
        assert_eq!(
            pressed_button_of_gesture(
                &mut held,
                MouseButton::Left,
                ElementState::Pressed,
                ModifiersState::CONTROL,
                MAC
            ),
            MouseButton::Right
        );
        assert!(held, "the gesture is the secondary one until it comes up");
        assert_eq!(
            pressed_button_of_gesture(
                &mut held,
                MouseButton::Left,
                ElementState::Released,
                // Control is gone by now, and the press does not stop being
                // what it was.
                ModifiersState::empty(),
                MAC
            ),
            MouseButton::Right
        );
        assert!(!held, "and the latch is spent by the release that read it");

        // The ordinary gesture never sets it, and neither platform's other
        // buttons disturb it.
        assert_eq!(
            pressed_button_of_gesture(
                &mut held,
                MouseButton::Left,
                ElementState::Pressed,
                ModifiersState::CONTROL,
                WINDOWS
            ),
            MouseButton::Left
        );
        assert!(
            !held,
            "Windows never latches: the rule is the identity there"
        );
        for platform in [WINDOWS, MAC] {
            let mut standing = true;
            assert_eq!(
                pressed_button_of_gesture(
                    &mut standing,
                    MouseButton::Middle,
                    ElementState::Pressed,
                    ModifiersState::CONTROL,
                    platform
                ),
                MouseButton::Middle
            );
            assert!(
                standing,
                "a middle click in the middle of a gesture is not that gesture"
            );
        }
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

    // ── The keyboard protocols (T-KEYBOARD-PROTOCOL) ────────────────────────
    //
    // `docs/plans/design/keyboard-protocol-2026-09-29.md` §6.2. The table is data
    // (`key_encoding.tsv`); the specs' own cases are literals beside it; what a program
    // that never asked receives is held to what it received before
    // (`key_encoding_legacy_{windows,macos}.tsv`, captured from the pre-ticket encoder).

    const KEY_ENCODING: &str = include_str!("key_encoding.tsv");
    const KEY_ENCODING_LEGACY_WINDOWS: &str = include_str!("key_encoding_legacy_windows.tsv");
    const KEY_ENCODING_LEGACY_MACOS: &str = include_str!("key_encoding_legacy_macos.tsv");

    const KITTY: KeyboardProtocol = KeyboardProtocol {
        kitty: 1,
        modify_other_keys: ModifyOtherKeys::Off,
        win32_input_mode: false,
    };
    const MOK1: KeyboardProtocol = KeyboardProtocol {
        kitty: 0,
        modify_other_keys: ModifyOtherKeys::One,
        win32_input_mode: false,
    };
    const MOK2: KeyboardProtocol = KeyboardProtocol {
        kitty: 0,
        modify_other_keys: ModifyOtherKeys::Two,
        win32_input_mode: false,
    };
    const UNASKED: KeyboardProtocol = KeyboardProtocol {
        kitty: 0,
        modify_other_keys: ModifyOtherKeys::Off,
        win32_input_mode: false,
    };
    /// No program asked, and ConPTY has win32-input-mode set — every Windows pane's state from
    /// its first bytes (T-KEYBOARD-RECORDS).
    const RECORDS: KeyboardProtocol = KeyboardProtocol {
        kitty: 0,
        modify_other_keys: ModifyOtherKeys::Off,
        win32_input_mode: true,
    };
    const EVERY_MODE: [KeyboardProtocol; 4] = [UNASKED, KITTY, MOK1, MOK2];

    fn protocol(mode: &str) -> KeyboardProtocol {
        match mode {
            "legacy" => UNASKED,
            "kitty" => KITTY,
            "mok1" => MOK1,
            "mok2" => MOK2,
            "records" => RECORDS,
            other => panic!("no mode `{other}`"),
        }
    }

    fn no_virtual_key(_: u16) -> Option<u16> {
        None
    }

    /// A layout with no dead keys — the US layout among them (`MapVirtualKeyExW(vk,
    /// MAPVK_VK_TO_CHAR, 0x04090409)` sets the dead-key bit on none of its keys).
    fn no_dead_keys(_: u16) -> bool {
        false
    }

    /// A press that says nothing about itself beyond its key: for the tests whose mode writes no
    /// record, where the origin is never read.
    const NOWHERE: KeyOrigin<'static> = KeyOrigin {
        platform: HostPlatform::Windows,
        physical_key: PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
        text_with_all_modifiers: None,
        virtual_key_of_scan_code: no_virtual_key,
        virtual_key_is_dead: no_dead_keys,
        shifted_character_of_virtual_key: us_shifted_character,
        conpty: ConPtyKind::Shipped,
    };

    /// The US layout's virtual key for the scan codes of its digit row and punctuation, as
    /// `MapVirtualKeyExW(…, MAPVK_VSC_TO_VK_EX, 0x04090409)` answered on 2026-09-29.
    fn us_virtual_key(scan: u16) -> Option<u16> {
        Some(match scan {
            0x02..=0x0A => 0x31 + (scan - 0x02),
            0x0B => 0x30,
            0x0C => 0xBD,
            0x0D => 0xBB,
            0x1A => 0xDB,
            0x1B => 0xDD,
            0x27 => 0xBA,
            0x28 => 0xDE,
            0x29 => 0xC0,
            0x2B => 0xDC,
            0x33 => 0xBC,
            0x34 => 0xBE,
            0x35 => 0xBF,
            // The letters: `VK_A`…`VK_Z`, where the letter's key is.
            _ => {
                return ('a'..='z')
                    .find(|letter| scan_code(us_physical(&letter.to_string())) == Some(scan))
                    .map(|letter| u16::from(letter.to_ascii_uppercase() as u8));
            }
        })
    }

    /// What the US layout types on a virtual key with Shift alone — `ToUnicodeEx` with only
    /// `VK_SHIFT` down: the shifted character of the key whose virtual key it is ([`us_shifted`]),
    /// `{` for `VK_OEM_4`, `!` for `VK_1`, `E` for `VK_E`.
    fn us_shifted_character(virtual_key: u16) -> Option<char> {
        "`1234567890-=[]\\;',./abcdefghijklmnopqrstuvwxyz"
            .chars()
            .find(|character| us_virtual_key_of(&character.to_string()) == Some(virtual_key))
            .map(us_shifted)
    }

    /// The US layout's virtual key for the key the table names by its unshifted character.
    fn us_virtual_key_of(name: &str) -> Option<u16> {
        scan_code(us_physical(name)).and_then(us_virtual_key)
    }

    /// Where a US keyboard has the key the table names.
    fn us_physical(name: &str) -> PhysicalKey {
        let code = match name {
            "Enter" => KeyCode::Enter,
            "Tab" => KeyCode::Tab,
            "Backspace" => KeyCode::Backspace,
            "Escape" => KeyCode::Escape,
            "Space" => KeyCode::Space,
            "1" => KeyCode::Digit1,
            "2" => KeyCode::Digit2,
            "3" => KeyCode::Digit3,
            "4" => KeyCode::Digit4,
            "5" => KeyCode::Digit5,
            "6" => KeyCode::Digit6,
            "7" => KeyCode::Digit7,
            "8" => KeyCode::Digit8,
            "9" => KeyCode::Digit9,
            "0" => KeyCode::Digit0,
            "-" => KeyCode::Minus,
            "=" => KeyCode::Equal,
            "[" => KeyCode::BracketLeft,
            "]" => KeyCode::BracketRight,
            "\\" => KeyCode::Backslash,
            ";" => KeyCode::Semicolon,
            "'" => KeyCode::Quote,
            "`" => KeyCode::Backquote,
            "," => KeyCode::Comma,
            "." => KeyCode::Period,
            "/" => KeyCode::Slash,
            "a" => KeyCode::KeyA,
            "b" => KeyCode::KeyB,
            "c" => KeyCode::KeyC,
            "d" => KeyCode::KeyD,
            "e" => KeyCode::KeyE,
            "f" => KeyCode::KeyF,
            "g" => KeyCode::KeyG,
            "h" => KeyCode::KeyH,
            "i" => KeyCode::KeyI,
            "j" => KeyCode::KeyJ,
            "k" => KeyCode::KeyK,
            "l" => KeyCode::KeyL,
            "m" => KeyCode::KeyM,
            "n" => KeyCode::KeyN,
            "o" => KeyCode::KeyO,
            "p" => KeyCode::KeyP,
            "q" => KeyCode::KeyQ,
            "r" => KeyCode::KeyR,
            "s" => KeyCode::KeyS,
            "t" => KeyCode::KeyT,
            "u" => KeyCode::KeyU,
            "v" => KeyCode::KeyV,
            "w" => KeyCode::KeyW,
            "x" => KeyCode::KeyX,
            "y" => KeyCode::KeyY,
            "z" => KeyCode::KeyZ,

            _ => return PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
        };
        PhysicalKey::Code(code)
    }

    /// What the US layout types for a chord — the `WM_CHAR` Windows sends for it, which winit
    /// hands over as `text_with_all_modifiers` — as `ToUnicodeEx` against the US layout answered
    /// on 2026-09-29: Alt alone changes nothing; Ctrl with Shift or Alt types nothing on these
    /// keys; Ctrl types Enter's LF, Backspace's DEL, Escape and Space as themselves, and a key's
    /// C0 code where it has one.
    fn us_text(name: &str, modifiers: ModifiersState) -> Option<String> {
        let (shift, alt, control) = (
            modifiers.shift_key(),
            modifiers.alt_key(),
            modifiers.control_key(),
        );
        if let Some(named) = named(name) {
            let (plain, with_control) = match named {
                NamedKey::Enter => ('\r', Some('\n')),
                NamedKey::Tab => ('\t', None),
                NamedKey::Backspace => ('\u{8}', Some('\u{7f}')),
                NamedKey::Escape => ('\u{1b}', Some('\u{1b}')),
                NamedKey::Space => (' ', Some(' ')),
                _ => return None,
            };
            return match (control, shift || alt) {
                (false, _) => Some(plain.to_string()),
                (true, false) => with_control.map(String::from),
                (true, true) => None,
            };
        }
        let (logical, _) = windows_us(name, shift);
        let Key::Character(logical) = logical else {
            return None;
        };
        if !control {
            return Some(logical.to_string());
        }
        if alt {
            return None;
        }
        control_byte(&logical)
            .filter(|byte| *byte < 0x20)
            .map(|byte| char::from(byte).to_string())
    }

    /// A press as winit hands it over on Windows with a US layout, with what a record is built
    /// from.
    struct UsPress {
        physical_key: PhysicalKey,
        text: Option<String>,
    }

    impl UsPress {
        fn new(name: &str, modifiers: ModifiersState) -> Self {
            Self {
                physical_key: us_physical(name),
                text: us_text(name, modifiers),
            }
        }

        fn on(&self, platform: HostPlatform) -> KeyOrigin<'_> {
            KeyOrigin {
                platform,
                physical_key: self.physical_key,
                text_with_all_modifiers: self.text.as_deref(),
                virtual_key_of_scan_code: us_virtual_key,
                virtual_key_is_dead: no_dead_keys,
                shifted_character_of_virtual_key: us_shifted_character,
                conpty: if platform == HostPlatform::Windows {
                    ConPtyKind::Shipped
                } else {
                    ConPtyKind::NotConPty
                },
            }
        }

        /// The same press in a pane that runs on the Windows inbox ConPTY.
        fn on_the_inbox_conpty(&self) -> KeyOrigin<'_> {
            KeyOrigin {
                conpty: ConPtyKind::Inbox,
                ..self.on(HostPlatform::Windows)
            }
        }
    }

    /// `-`, or any of S, A, C in that order, as the table writes a chord.
    fn chord(text: &str) -> ModifiersState {
        let mut modifiers = ModifiersState::empty();
        for letter in text.chars() {
            modifiers |= match letter {
                '-' => ModifiersState::empty(),
                'S' => ModifiersState::SHIFT,
                'A' => ModifiersState::ALT,
                'C' => ModifiersState::CONTROL,
                other => panic!("no modifier `{other}`"),
            };
        }
        modifiers
    }

    /// What a US layout's key produces with Shift, as winit's logical key carries it on
    /// Windows (Shift applied, Ctrl not).
    fn us_shifted(character: char) -> char {
        const PAIRS: &str = "`~1!2@3#4$5%6^7&8*9(0)-_=+[{]}\\|;:'\",<.>/?";
        let pairs = PAIRS.chars().collect::<Vec<_>>();
        pairs
            .chunks(2)
            .find(|pair| pair[0] == character)
            .map_or_else(|| character.to_ascii_uppercase(), |pair| pair[1])
    }

    fn named(name: &str) -> Option<NamedKey> {
        Some(match name {
            "Enter" => NamedKey::Enter,
            "Tab" => NamedKey::Tab,
            "Backspace" => NamedKey::Backspace,
            "Escape" => NamedKey::Escape,
            "Space" => NamedKey::Space,
            "ArrowUp" => NamedKey::ArrowUp,
            "ArrowDown" => NamedKey::ArrowDown,
            "ArrowLeft" => NamedKey::ArrowLeft,
            "ArrowRight" => NamedKey::ArrowRight,
            "Home" => NamedKey::Home,
            "End" => NamedKey::End,
            "Insert" => NamedKey::Insert,
            "Delete" => NamedKey::Delete,
            "PageUp" => NamedKey::PageUp,
            "PageDown" => NamedKey::PageDown,
            "F1" => NamedKey::F1,
            "F2" => NamedKey::F2,
            "F3" => NamedKey::F3,
            "F4" => NamedKey::F4,
            "F5" => NamedKey::F5,
            "F6" => NamedKey::F6,
            "F7" => NamedKey::F7,
            "F8" => NamedKey::F8,
            "F9" => NamedKey::F9,
            "F10" => NamedKey::F10,
            "F11" => NamedKey::F11,
            "F12" => NamedKey::F12,
            _ => return None,
        })
    }

    /// **The chord as winit really hands it over on Windows with a US layout**: `(logical key,
    /// key without modifiers)`, Ctrl+Alt included.
    ///
    /// [`windows_us`] with one difference, which is winit's: on Windows it keeps Ctrl while Alt
    /// is down, because Ctrl+Alt may be AltGr (`WindowsModifiers::remove_only_ctrl`, winit 0.30.13
    /// `platform_impl/windows/keyboard_layout.rs` lines 168–175, applied to the logical key in
    /// `keyboard.rs` lines 516–539), and when `ToUnicodeEx` types nothing for that state it keeps
    /// the preliminary `Key::Unidentified(NativeKey::Windows(vk))` (`keyboard_layout.rs` lines
    /// 243–250 and 387–405). On the US layout `ToUnicodeEx` types nothing for Ctrl+Alt or
    /// Ctrl+Shift+Alt on any of its 47 text keys (measured 2026-09-29), so every such chord arrives
    /// with no character and the virtual key.
    fn windows_us_event(name: &str, modifiers: ModifiersState) -> (Key, Key) {
        let (logical, base) = windows_us(name, modifiers.shift_key());
        if modifiers.control_key() && modifiers.alt_key() && matches!(logical, Key::Character(_)) {
            let virtual_key = us_virtual_key_of(name)
                .unwrap_or_else(|| panic!("the US layout has a virtual key for {name:?}"));
            return (Key::Unidentified(NativeKey::Windows(virtual_key)), base);
        }
        (logical, base)
    }

    /// The chord with Shift applied to a text key and nothing else: `(logical key, key without
    /// modifiers)`. This is the model `key_encoding_legacy_{windows,macos}.tsv` were captured
    /// with — a statement about the encoder's inputs, which is how those captures are held — and
    /// what winit hands over for every chord but Ctrl+Alt on Windows ([`windows_us_event`]).
    fn windows_us(name: &str, shift: bool) -> (Key, Key) {
        if let Some(key) = named(name) {
            return (Key::Named(key), Key::Named(key));
        }
        let mut characters = name.chars();
        let base = characters.next().expect("a key name");
        assert!(characters.next().is_none(), "one character: {name:?}");
        let logical = if shift { us_shifted(base) } else { base };
        (
            Key::Character(logical.to_string().into()),
            Key::Character(base.to_string().into()),
        )
    }

    /// The table's escaped bytes: `CSI ` is ESC [; `\e`, `\r`, `\t`, `\xNN`; `—` is none.
    fn unescape(text: &str) -> Option<Vec<u8>> {
        if text == "—" {
            return None;
        }
        let text = text.replace("CSI ", "\\e[");
        let mut bytes = Vec::new();
        let mut rest = text.as_str();
        while let Some(character) = rest.chars().next() {
            if let Some(escaped) = rest.strip_prefix('\\') {
                let (byte, after) = match escaped.as_bytes().first() {
                    Some(b'e') => (0x1b, &escaped[1..]),
                    Some(b'r') => (b'\r', &escaped[1..]),
                    Some(b't') => (b'\t', &escaped[1..]),
                    Some(b'x') => (
                        u8::from_str_radix(&escaped[1..3], 16).expect("two hex digits"),
                        &escaped[3..],
                    ),
                    _ => panic!("no escape in {text:?}"),
                };
                bytes.push(byte);
                rest = after;
            } else {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                rest = &rest[character.len_utf8()..];
            }
        }
        Some(bytes)
    }

    struct EncodingRow<'a> {
        key: &'a str,
        location: &'a str,
        mods: &'a str,
        decckm: &'a str,
        mode: &'a str,
        bytes: &'a str,
        note: &'a str,
    }

    fn encoding_rows() -> Vec<EncodingRow<'static>> {
        KEY_ENCODING
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                let cells = line.split('\t').collect::<Vec<_>>();
                assert_eq!(cells.len(), 7, "seven columns: {line:?}");
                EncodingRow {
                    key: cells[0],
                    location: cells[1],
                    mods: cells[2],
                    decckm: cells[3],
                    mode: cells[4],
                    bytes: cells[5],
                    note: cells[6],
                }
            })
            .collect()
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **every row of `key_encoding.tsv` is what the encoder
    /// sends**, and every `(table)` row is a chord the shortcut table claims, so the encoder
    /// is never asked.
    ///
    /// The table is data: the design note's §4.4 was its first version, and from this ticket
    /// on the TSV is the source (`docs/key-encoding.md` is generated from it). Each chord is
    /// built as winit hands it over on Windows with a US layout.
    ///
    /// The `records` rows (T-KEYBOARD-RECORDS) are Windows' with win32-input-mode set, whatever
    /// host runs the test: the press is the US layout's, as `UsPress` builds it.
    ///
    /// MUTATION: make `kitty_bytes` send `CSI Z` for Shift+Tab as legacy does (the
    /// `Tab S kitty` row reads `CSI 9;2u`), or drop `Ctrl+Shift+M` from the shortcut table
    /// (its `(table)` row is no longer claimed), or leave Space out of `key_records`' set (the
    /// `Space C records` row reads `—`).
    #[test]
    fn every_row_of_the_key_encoding_table() {
        let windows = crate::shortcuts::Shortcuts::defaults_for(HostPlatform::Windows);
        let mut encoded = 0;
        let mut claimed = 0;
        for row in encoding_rows() {
            let modifiers = chord(row.mods);
            let (logical, base) = windows_us_event(row.key, modifiers);
            let where_ = format!(
                "{} {} {} DECCKM {} {}",
                row.key, row.location, row.mods, row.decckm, row.mode
            );
            if row.bytes == "(table)" {
                assert!(
                    windows
                        .lookup(
                            &logical,
                            &base,
                            modifiers,
                            crate::shortcuts::Focus::default()
                        )
                        .is_some(),
                    "{where_}: the table says the shortcut table claims this chord"
                );
                claimed += 1;
                continue;
            }
            let location = match row.location {
                "standard" => KeyLocation::Standard,
                "numpad" => KeyLocation::Numpad,
                other => panic!("no location `{other}`"),
            };
            let decckm = match row.decckm {
                "off" => false,
                "on" => true,
                other => panic!("no DECCKM `{other}`"),
            };
            assert!(
                matches!(row.note, "" | "system" | "n/a"),
                "{where_}: no note `{}`",
                row.note
            );
            let mut press = UsPress::new(row.key, modifiers);
            if location == KeyLocation::Numpad {
                press.physical_key = PhysicalKey::Code(match row.key {
                    "Enter" => KeyCode::NumpadEnter,
                    "ArrowUp" | "8" => KeyCode::Numpad8,
                    other => panic!("no keypad key `{other}`"),
                });
            }
            let sent = keyboard_bytes(
                &logical,
                &base,
                location,
                modifiers,
                decckm,
                protocol(row.mode),
                press.on(HostPlatform::Windows),
            );
            assert_eq!(
                sent.as_deref().map(String::from_utf8_lossy),
                unescape(row.bytes).as_deref().map(String::from_utf8_lossy),
                "{where_}: the table says `{}`",
                row.bytes
            );
            encoded += 1;
        }
        assert_eq!(
            (encoded, claimed),
            (432, 20),
            "every row was read — a table that lost rows would pass a filter over nothing"
        );
    }

    /// Render `key_encoding.tsv` as the document a reader opens.
    fn key_encoding_document() -> String {
        let mut order: Vec<(&str, &str, &str, &str)> = Vec::new();
        let mut cells: std::collections::HashMap<(&str, &str, &str, &str), [String; 5]> =
            std::collections::HashMap::new();
        for row in encoding_rows() {
            let id = (row.key, row.location, row.mods, row.decckm);
            if !cells.contains_key(&id) {
                order.push(id);
            }
            let column = ["legacy", "kitty", "mok1", "mok2", "records"]
                .iter()
                .position(|mode| *mode == row.mode)
                .expect("a known mode");
            let value = match row.bytes {
                "—" | "(table)" => row.bytes.to_owned(),
                bytes => format!("`{bytes}`"),
            };
            let value = if row.note.is_empty() {
                value
            } else {
                format!("{value} ({})", row.note)
            };
            cells.entry(id).or_default()[column] = value;
        }
        let mut document = String::from(
            "# What each key sends\n\
             \n\
             <!-- generated from crates/bt-app/src/key_encoding.tsv by \
             scripts/dev/generate-key-encoding-table.ps1; edit the table, not this page -->\n\
             \n\
             The bytes a key sends to the program in a pane, in each mode a program can ask for: \
             nothing asked (*legacy*), the kitty keyboard protocol's first tier (*kitty flag 1*), \
             or xterm's modifyOtherKeys (*1* or *2*). When a program asks for both, kitty wins. A \
             program that never asks receives the legacy column, which is what Folio sent before \
             either protocol existed here.\n\
             \n\
             On Windows, a program that never asks reads the *Windows records* column instead, \
             while ConPTY has win32-input-mode on (it turns it on for every session): the chords \
             whose legacy bytes cannot tell them apart — a modified Enter, Tab, Backspace or \
             Space, Shift+Escape, and Ctrl with a key that has no control code or with Ctrl+Alt — go as the key records \
             `CSI Vk;Sc;Uc;Kd;Cs;Rc _` (down, then up), which ConPTY turns into those exact key \
             events. Every other chord sends its legacy bytes. A program that asks for kitty or \
             modifyOtherKeys gets that protocol instead.\n\
             \n\
             `CSI` is `ESC [`. `—` sends nothing. *(table)* is a chord Folio's shortcut table \
             claims on Windows, so no program receives it. *(system)* is taken by Windows first; \
             *(n/a)* never arrives on Windows and is encoded for macOS. Chords are as a US layout \
             produces them. The design is `docs/plans/design/keyboard-protocol-2026-09-29.md`.\n\
             \n\
             | Key | Where | Mods | DECCKM | legacy | kitty flag 1 | modifyOtherKeys 1 | modifyOtherKeys 2 | Windows records |\n\
             |---|---|---|---|---|---|---|---|---|\n",
        );
        for id in order {
            let (key, location, mods, decckm) = id;
            let [legacy, kitty, one, two, records] = &cells[&id];
            let key = if key == "|" { "\\|" } else { key };
            document.push_str(&format!(
                "| {key} | {location} | {mods} | {decckm} | {legacy} | {kitty} | {one} | {two} | {records} |\n"
            ));
        }
        document
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **`docs/key-encoding.md` is `key_encoding.tsv`, rendered.**
    ///
    /// The rendering is left in `target/key-encoding.md`, which
    /// `scripts/dev/generate-key-encoding-table.ps1` copies into place — the pattern of
    /// `window_waits.tsv` and ARCHITECTURE §5.3: one list, so the page and the test cannot
    /// say different things.
    ///
    /// MUTATION: change any byte in `key_encoding.tsv` without regenerating the page.
    #[test]
    fn the_key_encoding_document_is_the_table() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let wanted = key_encoding_document();
        let generated = root.join("target").join("key-encoding.md");
        std::fs::create_dir_all(root.join("target")).expect("the workspace has a target directory");
        std::fs::write(&generated, wanted.as_bytes()).expect("target/ is writable");
        let held = std::fs::read_to_string(root.join("docs").join("key-encoding.md"))
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            held == wanted,
            "docs/key-encoding.md is not key_encoding.tsv — run \
             scripts/dev/generate-key-encoding-table.ps1"
        );
    }

    /// **Every chord the encoder writes a record pair for on Windows with a US layout**, as
    /// `key_records_windows_us.tsv` holds them: each key of the captured sweep under each of the
    /// seven modifier sets, and Numpad Enter under the same seven, built as winit hands the press
    /// over ([`windows_us_event`], [`UsPress`]), with win32-input-mode on and no protocol asked;
    /// a chord is a row when its bytes differ from the same press with the mode off, and a chord
    /// the shortcut table claims is left out, because the encoder is never asked for it.
    fn windows_us_record_rows() -> String {
        let windows = crate::shortcuts::Shortcuts::defaults_for(HostPlatform::Windows);
        let mut keys: Vec<&str> = Vec::new();
        for (key, ..) in legacy_sweep() {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        let mut rows = String::from(
            "# Every chord `input::keyboard_bytes` writes as a win32-input-mode record pair on Windows\n\
             # with a US layout, win32-input-mode on and no keyboard protocol asked (T-KEYBOARD-RECORDS).\n\
             # Generated by `input::tests::the_windows_us_records_file_is_what_the_encoder_writes`\n\
             # (scripts/dev/generate-key-encoding-table.ps1 copies it into place); read by\n\
             # crates/bt-pty/tests/keyboard_protocol_through_conpty.rs, which sends each row through a\n\
             # real ConPTY. Columns: key, location, modifiers, the bytes (`\\e` is ESC).\n",
        );
        let mut row = |key: &str, location: KeyLocation, mods: &str, physical: Option<KeyCode>| {
            let modifiers = chord(mods);
            let (logical, base) = windows_us_event(key, modifiers);
            if windows
                .lookup(
                    &logical,
                    &base,
                    modifiers,
                    crate::shortcuts::Focus::default(),
                )
                .is_some()
            {
                return;
            }
            let mut press = UsPress::new(key, modifiers);
            if let Some(code) = physical {
                press.physical_key = PhysicalKey::Code(code);
            }
            let sent = |protocol| {
                keyboard_bytes(
                    &logical,
                    &base,
                    location,
                    modifiers,
                    false,
                    protocol,
                    press.on(HostPlatform::Windows),
                )
            };
            let records = sent(RECORDS);
            if records == sent(UNASKED) {
                return;
            }
            let records = String::from_utf8(records.expect("a record pair")).expect("ASCII");
            let place = if location == KeyLocation::Numpad {
                "numpad"
            } else {
                "standard"
            };
            rows.push_str(&format!(
                "{key}\t{place}\t{mods}\t{}\n",
                records.replace('\x1b', "\\e")
            ));
        };
        const MODIFIER_SETS: [&str; 7] = ["S", "A", "SA", "C", "SC", "AC", "SAC"];
        for key in keys {
            for mods in MODIFIER_SETS {
                row(key, KeyLocation::Standard, mods, None);
            }
        }
        for mods in MODIFIER_SETS {
            row(
                "Enter",
                KeyLocation::Numpad,
                mods,
                Some(KeyCode::NumpadEnter),
            );
        }
        rows
    }

    /// RED (T-KEYBOARD-RECORDS, review round 2) — **`key_records_windows_us.tsv` is exactly the
    /// record pairs the encoder writes**, so the real-ConPTY test that sends every row sends the
    /// application's own bytes and not a second copy of them.
    ///
    /// The rendering is left in `target/key-records-windows-us.tsv`, which
    /// `scripts/dev/generate-key-encoding-table.ps1` copies into place, as for the page.
    ///
    /// MUTATION: change any record field in `key_records` (the file no longer matches), or drop
    /// the Ctrl+Alt arm (its 94 rows are gone).
    #[test]
    fn the_windows_us_records_file_is_what_the_encoder_writes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let wanted = windows_us_record_rows();
        let generated = root.join("target").join("key-records-windows-us.tsv");
        std::fs::create_dir_all(root.join("target")).expect("the workspace has a target directory");
        std::fs::write(&generated, wanted.as_bytes()).expect("target/ is writable");
        let held = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("key_records_windows_us.tsv"),
        )
        .unwrap_or_default()
        .replace("\r\n", "\n");
        assert!(
            held == wanted,
            "crates/bt-app/src/key_records_windows_us.tsv is not what the encoder writes — run \
             scripts/dev/generate-key-encoding-table.ps1"
        );
        let rows = wanted.lines().filter(|line| !line.starts_with('#')).count();
        assert_eq!(
            rows, 155,
            "158 chords of the sweep and 7 of Numpad Enter, less the 10 the shortcut table claims \
             (Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+comma, Ctrl+Shift+1, 3, 4, 5, 7, 8 and 9)"
        );
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **the cases the specifications themselves state**, as
    /// literals, independent of `key_encoding.tsv`.
    ///
    /// A wrong TSV would make the generated page and the exhaustive test agree with each
    /// other; these come from the kitty protocol's text and xterm's reference and are not read
    /// from the table.
    ///
    /// RED (T-KEYBOARD-RECORDS) for the records half: on Windows, with win32-input-mode set and
    /// no protocol asked, the chords VT cannot express are the key records of microsoft/terminal's
    /// win32-input-mode spec, the chords it can keep their bytes, and a program that asked wins.
    ///
    /// MUTATION: send `CSI 27u`'s legacy `\e` for a lone Esc under flag 1, or honour DECCKM
    /// for a plain arrow under flag 1 (`SS3 A`), or drop the `keyboard.win32_input_mode` rung
    /// from `keyboard_bytes` (Ctrl+Enter reads `\r`), or ask it before the kitty rung (the
    /// Ctrl+Enter a program asked for reads a record pair).
    #[test]
    fn the_protocols_normative_cases() {
        let none = ModifiersState::empty();
        let named = |key: NamedKey| Key::Named(key);
        let text = |text: &str| Key::Character(text.into());
        let flag1 = |key: &Key, base: &Key, modifiers: ModifiersState| {
            keyboard_bytes(
                key,
                base,
                KeyLocation::Standard,
                modifiers,
                false,
                KITTY,
                NOWHERE,
            )
        };
        let esc = named(NamedKey::Escape);
        let tab = named(NamedKey::Tab);
        let enter = named(NamedKey::Enter);
        let space = named(NamedKey::Space);
        assert_eq!(flag1(&esc, &esc, none), Some(b"\x1b[27u".to_vec()));
        assert_eq!(
            flag1(&text("i"), &text("i"), ModifiersState::CONTROL),
            Some(b"\x1b[105;5u".to_vec())
        );
        assert_eq!(flag1(&tab, &tab, none), Some(b"\t".to_vec()));
        assert_eq!(
            flag1(&text("m"), &text("m"), ModifiersState::CONTROL),
            Some(b"\x1b[109;5u".to_vec())
        );
        assert_eq!(flag1(&enter, &enter, none), Some(b"\r".to_vec()));
        assert_eq!(
            flag1(&tab, &tab, ModifiersState::SHIFT),
            Some(b"\x1b[9;2u".to_vec())
        );
        assert_eq!(
            flag1(&text("a"), &text("a"), ModifiersState::ALT),
            Some(b"\x1b[97;3u".to_vec())
        );
        assert_eq!(
            flag1(&space, &space, ModifiersState::CONTROL),
            Some(b"\x1b[32;5u".to_vec())
        );
        assert_eq!(
            keyboard_bytes(
                &enter,
                &enter,
                KeyLocation::Numpad,
                none,
                false,
                KITTY,
                NOWHERE
            ),
            Some(b"\x1b[57414u".to_vec())
        );
        assert_eq!(
            flag1(&enter, &enter, ModifiersState::CONTROL),
            Some(b"\x1b[13;5u".to_vec())
        );
        let up = named(NamedKey::ArrowUp);
        assert_eq!(
            keyboard_bytes(&up, &up, KeyLocation::Standard, none, true, KITTY, NOWHERE),
            Some(b"\x1b[A".to_vec())
        );
        let f1 = named(NamedKey::F1);
        let f3 = named(NamedKey::F3);
        assert_eq!(flag1(&f1, &f1, none), Some(b"\x1b[P".to_vec()));
        assert_eq!(flag1(&f3, &f3, none), Some(b"\x1b[13~".to_vec()));

        let mode = |key: &Key, base: &Key, modifiers: ModifiersState, protocol| {
            keyboard_bytes(
                key,
                base,
                KeyLocation::Standard,
                modifiers,
                false,
                protocol,
                NOWHERE,
            )
        };
        assert_eq!(
            mode(&enter, &enter, ModifiersState::CONTROL, MOK2),
            Some(b"\x1b[27;5;13~".to_vec())
        );
        assert_eq!(
            mode(&text("A"), &text("a"), ModifiersState::SHIFT, MOK2),
            Some(b"\x1b[27;2;65~".to_vec())
        );
        assert_eq!(
            mode(
                &text("A"),
                &text("a"),
                ModifiersState::SHIFT | ModifiersState::CONTROL,
                MOK2
            ),
            Some(b"\x1b[27;6;65~".to_vec())
        );
        assert_eq!(
            mode(&text("a"), &text("a"), ModifiersState::CONTROL, MOK1),
            Some(vec![0x01])
        );
        assert_eq!(
            mode(&text("a"), &text("a"), ModifiersState::ALT, MOK1),
            Some(b"\x1b[27;3;97~".to_vec())
        );

        // **win32-input-mode records** (T-KEYBOARD-RECORDS, design note §7.3): what Windows
        // Terminal sends for the same press, `CSI Vk;Sc;Uc;Kd;Cs;Rc _` down then up, with the
        // US layout's character for the chord (`WM_CHAR`: LF for Ctrl+Enter, nothing for Ctrl+1).
        let pressed = |key: &Key,
                       location: KeyLocation,
                       modifiers: ModifiersState,
                       physical: KeyCode,
                       typed: Option<&str>,
                       protocol: KeyboardProtocol| {
            keyboard_bytes(
                key,
                key,
                location,
                modifiers,
                false,
                protocol,
                KeyOrigin {
                    platform: HostPlatform::Windows,
                    physical_key: PhysicalKey::Code(physical),
                    text_with_all_modifiers: typed,
                    virtual_key_of_scan_code: us_virtual_key,
                    virtual_key_is_dead: no_dead_keys,
                    shifted_character_of_virtual_key: us_shifted_character,
                    conpty: ConPtyKind::Shipped,
                },
            )
        };
        let record = |key: &Key, modifiers: ModifiersState, physical: KeyCode, typed| {
            pressed(
                key,
                KeyLocation::Standard,
                modifiers,
                physical,
                typed,
                RECORDS,
            )
        };
        let ctrl = ModifiersState::CONTROL;
        assert_eq!(
            record(&enter, ctrl, KeyCode::Enter, Some("\n")),
            Some(b"\x1b[13;28;10;1;8;1_\x1b[13;28;10;0;8;1_".to_vec()),
            "Ctrl+Enter"
        );
        assert_eq!(
            record(&enter, ModifiersState::SHIFT, KeyCode::Enter, Some("\r")),
            Some(SHIFT_ENTER_RECORDS.to_vec()),
            "Shift+Enter, the paste road's own record pair"
        );
        assert_eq!(
            record(&enter, ModifiersState::ALT, KeyCode::Enter, Some("\r")),
            Some(b"\x1b[13;28;13;1;2;1_\x1b[13;28;13;0;2;1_".to_vec()),
            "Alt+Enter"
        );
        assert_eq!(
            record(&tab, ModifiersState::SHIFT, KeyCode::Tab, Some("\t")),
            Some(b"\x1b[9;15;9;1;16;1_\x1b[9;15;9;0;16;1_".to_vec()),
            "Shift+Tab"
        );
        assert_eq!(
            record(&space, ctrl, KeyCode::Space, Some(" ")),
            Some(b"\x1b[32;57;32;1;8;1_\x1b[32;57;32;0;8;1_".to_vec()),
            "Ctrl+Space"
        );
        assert_eq!(
            record(
                &named(NamedKey::Backspace),
                ctrl,
                KeyCode::Backspace,
                Some("\u{7f}")
            ),
            Some(b"\x1b[8;14;127;1;8;1_\x1b[8;14;127;0;8;1_".to_vec()),
            "Ctrl+Backspace"
        );
        assert_eq!(
            record(&esc, ModifiersState::SHIFT, KeyCode::Escape, Some("\u{1b}")),
            Some(b"\x1b[27;1;27;1;16;1_\x1b[27;1;27;0;16;1_".to_vec()),
            "Shift+Escape"
        );
        assert_eq!(
            record(&esc, ctrl, KeyCode::Escape, Some("\u{1b}")),
            Some(b"\x1b".to_vec()),
            "Ctrl+Escape keeps its ESC: ConPTY swallows that record"
        );
        assert_eq!(
            record(&text("1"), ctrl, KeyCode::Digit1, None),
            Some(b"\x1b[49;2;0;1;8;1_\x1b[49;2;0;0;8;1_".to_vec()),
            "Ctrl+1: a key with no C0 code, which types nothing"
        );
        assert_eq!(
            pressed(
                &enter,
                KeyLocation::Numpad,
                ctrl,
                KeyCode::NumpadEnter,
                Some("\n"),
                RECORDS
            ),
            Some(b"\x1b[13;28;10;1;264;1_\x1b[13;28;10;0;264;1_".to_vec()),
            "Ctrl + Numpad Enter, an enhanced key"
        );
        // Outside the set: a chord VT already expresses keeps its legacy bytes.
        assert_eq!(
            record(&text("i"), ctrl, KeyCode::KeyI, Some("\t")),
            Some(b"\t".to_vec()),
            "Ctrl+I has a C0 code, and ConPTY turns it into Tab"
        );
        assert_eq!(
            record(&enter, none, KeyCode::Enter, Some("\r")),
            Some(b"\r".to_vec()),
            "plain Enter"
        );
        assert_eq!(
            record(&text("a"), ModifiersState::ALT, KeyCode::KeyA, Some("a")),
            Some(b"\x1ba".to_vec()),
            "Alt+a: ConPTY turns ESC a into Alt+a"
        );
        // A program that asked wins (§4.3): the transport's mode does not outrank it.
        let asked = |protocol: KeyboardProtocol| KeyboardProtocol {
            win32_input_mode: true,
            ..protocol
        };
        assert_eq!(
            record_with(&enter, ctrl, asked(KITTY)),
            Some(b"\x1b[13;5u".to_vec())
        );
        assert_eq!(
            record_with(&enter, ctrl, asked(MOK2)),
            Some(b"\x1b[27;5;13~".to_vec())
        );
        fn record_with(
            key: &Key,
            modifiers: ModifiersState,
            protocol: KeyboardProtocol,
        ) -> Option<Vec<u8>> {
            keyboard_bytes(
                key,
                key,
                KeyLocation::Standard,
                modifiers,
                false,
                protocol,
                KeyOrigin {
                    platform: HostPlatform::Windows,
                    physical_key: PhysicalKey::Code(KeyCode::Enter),
                    text_with_all_modifiers: Some("\n"),
                    virtual_key_of_scan_code: us_virtual_key,
                    virtual_key_is_dead: no_dead_keys,
                    shifted_character_of_virtual_key: us_shifted_character,
                    conpty: ConPtyKind::Shipped,
                },
            )
        }
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **a program that never asked receives exactly the bytes it
    /// received before this ticket**, for every key of the sweep under all sixteen
    /// combinations of Shift, Alt, Ctrl and Super, with DECCKM off and on.
    ///
    /// `key_encoding_legacy_windows.tsv` and `key_encoding_legacy_macos.tsv` were captured, each on
    /// its platform, from the encoder as it stood on main before T-KEYBOARD-PROTOCOL. This is the promise the design note makes of PowerShell, cmd and
    /// Codex on Windows: byte-identical.
    ///
    /// MUTATION: let the kitty rules run when no protocol is in force (Esc sends `CSI 27u`).
    #[test]
    fn a_program_that_never_asked_gets_exactly_the_bytes_it_got_before() {
        let baseline = match bt_platform::host_platform() {
            HostPlatform::Windows => KEY_ENCODING_LEGACY_WINDOWS,
            HostPlatform::MacOs => KEY_ENCODING_LEGACY_MACOS,
            other => panic!("no pre-ticket capture was made on {other:?}"),
        };
        let mut compared = 0;
        for line in baseline
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let cells = line.split('\t').collect::<Vec<_>>();
            let [key, bits, decckm, hex] = cells[..] else {
                panic!("four columns: {line:?}");
            };
            let bits = bits.parse::<u8>().expect("modifier bits");
            let mut modifiers = ModifiersState::empty();
            for (bit, modifier) in [
                (1, ModifiersState::SHIFT),
                (2, ModifiersState::ALT),
                (4, ModifiersState::CONTROL),
                (8, ModifiersState::SUPER),
            ] {
                if bits & bit != 0 {
                    modifiers |= modifier;
                }
            }
            let (logical, base) = windows_us(key, modifiers.shift_key());
            let sent = keyboard_bytes(
                &logical,
                &base,
                KeyLocation::Standard,
                modifiers,
                decckm == "1",
                UNASKED,
                NOWHERE,
            );
            let sent = sent.map_or_else(
                || "-".to_owned(),
                |bytes| bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
            );
            assert_eq!(sent, hex, "{key} mods {bits} DECCKM {decckm}");
            compared += 1;
        }
        assert_eq!(compared, 2368, "the whole captured sweep");
    }

    /// The captured sweep of this host, as `(key, modifiers, DECCKM, hex or "-")`.
    fn legacy_sweep() -> Vec<(&'static str, ModifiersState, bool, &'static str)> {
        let baseline = match bt_platform::host_platform() {
            HostPlatform::Windows => KEY_ENCODING_LEGACY_WINDOWS,
            HostPlatform::MacOs => KEY_ENCODING_LEGACY_MACOS,
            other => panic!("no pre-ticket capture was made on {other:?}"),
        };
        baseline
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                let cells = line.split('\t').collect::<Vec<_>>();
                let [key, bits, decckm, hex] = cells[..] else {
                    panic!("four columns: {line:?}");
                };
                let bits = bits.parse::<u8>().expect("modifier bits");
                let mut modifiers = ModifiersState::empty();
                for (bit, modifier) in [
                    (1, ModifiersState::SHIFT),
                    (2, ModifiersState::ALT),
                    (4, ModifiersState::CONTROL),
                    (8, ModifiersState::SUPER),
                ] {
                    if bits & bit != 0 {
                        modifiers |= modifier;
                    }
                }
                (key, modifiers, decckm == "1", hex)
            })
            .collect()
    }

    fn hex_of(bytes: Option<Vec<u8>>) -> String {
        bytes.map_or_else(
            || "-".to_owned(),
            |bytes| bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        )
    }

    /// RED (T-KEYBOARD-RECORDS) — **the second run of the captured sweep, with win32-input-mode
    /// set: on Windows the only chords that change are the ones VT cannot express, each now its
    /// key record pair; on any other platform no row changes.**
    ///
    /// The sweep (`key_encoding_legacy_{windows,macos}.tsv`) was captured with no
    /// win32-input-mode, which is why `a_program_that_never_asked_gets_exactly_the_bytes_it_got_before`
    /// still passes unchanged: that run is a pane whose transport never asked for records. This
    /// run is a Windows pane's real state — ConPTY sets the mode at the head of every session —
    /// with every chord built as winit really hands it over ([`windows_us_event`]: Ctrl+Alt on a
    /// text key arrives with no character). For each chord it compares the mode on with the mode
    /// off on the same press, and ties the mode-off bytes to the capture wherever the press is the
    /// capture's own (every chord but Ctrl+Alt on a text key). What changes: a modified Enter,
    /// Tab, Backspace or Space, Shift+Escape, Ctrl or Ctrl+Shift with a text key whose character
    /// has no C0 code, and Ctrl+Alt (with or without Shift) on every text key — never with Super.
    /// That is 158 chords of the sweep (29 named, 18 Ctrl, 17 Ctrl+Shift, 47 Ctrl+Alt, 47
    /// Ctrl+Shift+Alt), each with DECCKM off and on. The set and the record's fields are written
    /// here from the win32-input-mode spec and the US layout's measured answers, not read from
    /// `key_records`. The same run with the press said to come from a Mac changes nothing:
    /// macOS is unchanged byte for byte. And the same Windows run in a pane on the inbox ConPTY
    /// changes nothing either: with win32-input-mode on and the inbox source, every chord is its
    /// mode-off bytes, which are the capture itself wherever the press is the capture's own
    /// (coordinator's ruling, 2026-09-29: records only on the ConPTY Folio ships).
    ///
    /// MUTATION: let `key_records` write for a Ctrl chord that has a C0 code (Ctrl+E reads a record
    /// pair), or drop its `Key::Unidentified` arm (Ctrl+Alt+1 reads nothing), or drop its
    /// `origin.platform != HostPlatform::Windows` refusal (the macOS run differs), or its
    /// `origin.conpty != ConPtyKind::Shipped` refusal (the inbox run differs).
    #[test]
    fn with_win32_input_mode_only_the_chords_vt_cannot_express_change() {
        let sweep = legacy_sweep();
        let mut changed = 0;
        for &(key, modifiers, decckm, hex) in &sweep {
            let where_ = format!("{key} {modifiers:?} DECCKM {decckm}");
            let press = UsPress::new(key, modifiers);
            let sent = |logical: &Key, base: &Key, protocol, platform| {
                hex_of(keyboard_bytes(
                    logical,
                    base,
                    KeyLocation::Standard,
                    modifiers,
                    decckm,
                    protocol,
                    press.on(platform),
                ))
            };

            // A Mac: the capture's own press, the mode on, nothing changes.
            let (character_logical, character_base) = windows_us(key, modifiers.shift_key());
            assert_eq!(
                sent(
                    &character_logical,
                    &character_base,
                    RECORDS,
                    HostPlatform::MacOs
                ),
                hex,
                "{where_}: a Mac is unchanged"
            );

            // Windows, as winit hands the chord over.
            let (logical, base) = windows_us_event(key, modifiers);
            let before = sent(&logical, &base, UNASKED, HostPlatform::Windows);
            if logical == character_logical {
                assert_eq!(before, hex, "{where_}: the mode off is the capture");
            }
            let after = sent(&logical, &base, RECORDS, HostPlatform::Windows);

            // The same Windows pane on the inbox ConPTY: no records, so exactly the mode-off
            // bytes — which are the capture wherever the press is the capture's own.
            let on_the_inbox = hex_of(keyboard_bytes(
                &logical,
                &base,
                KeyLocation::Standard,
                modifiers,
                decckm,
                RECORDS,
                press.on_the_inbox_conpty(),
            ));
            assert_eq!(
                on_the_inbox, before,
                "{where_}: the inbox ConPTY gets no record"
            );

            let (shift, alt, control) = (
                modifiers.shift_key(),
                modifiers.alt_key(),
                modifiers.control_key(),
            );
            let named_key = match key {
                "Enter" => Some((13, 0x1C)),
                "Tab" => Some((9, 0x0F)),
                "Backspace" => Some((8, 0x0E)),
                "Escape" => Some((27, 0x01)),
                "Space" => Some((32, 0x39)),
                _ => None,
            };
            let typed_character = match &character_logical {
                Key::Character(text) => text.chars().next(),
                _ => None,
            };
            let text_key_scan = || {
                scan_code(press.physical_key)
                    .unwrap_or_else(|| panic!("{where_}: the US keyboard has this key"))
            };
            let expected = if modifiers.super_key() {
                None
            } else if let Some((virtual_key, scan)) = named_key {
                // Escape with Ctrl or Alt keeps its ESC: ConPTY swallows that record.
                let escape_swallowed = key == "Escape" && (control || alt);
                ((shift || alt || control) && !escape_swallowed).then_some((virtual_key, scan))
            } else if typed_character.is_some() && control && alt {
                // winit hands Ctrl+Alt on a text key over with no character on the US layout.
                let scan = text_key_scan();
                Some((us_virtual_key(scan).expect("a US virtual key"), scan))
            } else if let Some(character) = typed_character
                && control
                && !character.is_ascii_alphabetic()
                && !"@[\\]^_?".contains(character)
            {
                let scan = text_key_scan();
                Some((us_virtual_key(scan).expect("a US virtual key"), scan))
            } else {
                None
            };
            match expected {
                None => assert_eq!(after, before, "{where_}: unchanged"),
                Some((virtual_key, scan)) => {
                    let character = press
                        .text
                        .as_deref()
                        .and_then(|text| text.encode_utf16().next())
                        .unwrap_or(0);
                    let state = 16 * u16::from(shift) + 8 * u16::from(control) + 2 * u16::from(alt);
                    let wanted = format!(
                        "\x1b[{virtual_key};{scan};{character};1;{state};1_\
                         \x1b[{virtual_key};{scan};{character};0;{state};1_"
                    );
                    assert_eq!(
                        after,
                        hex_of(Some(wanted.into_bytes())),
                        "{where_}: its record pair"
                    );
                    changed += 1;
                }
            }
        }
        assert_eq!(sweep.len(), 2368, "the whole captured sweep");
        assert_eq!(
            changed, 316,
            "158 chords of the sweep, each with DECCKM off and on"
        );
    }

    /// RED (T-KEYBOARD-RECORDS, review rounds 3 and 4) — **a dead key under Ctrl+Alt is not a
    /// record, though winit reports its key without modifiers as text; an ordinary key the map
    /// answers `0` for is.**
    ///
    /// winit turns a dead key into the character it would compose when it builds
    /// `key_without_modifiers` (winit 0.30.13 `platform_impl/windows/keyboard.rs`, "We convert dead
    /// keys into their character"). On the French layout the dead `^` is `VK_OEM_6` (221) at scan
    /// code 26, and Ctrl+Alt types nothing on it, so the press arrives as
    /// `Key::Unidentified(NativeKey::Windows(221))` with the key without modifiers `^` — the same
    /// shape as a text key. On the Kazakh layout `VK_OEM_1` (186) at scan code 39 types `ж` and
    /// Ctrl+Alt types nothing on it: the same shape again, and this one is a text key. The layout
    /// tells them apart only by the top bit of `MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR)`, which is set
    /// for the circumflex; for `ж` the whole answer is `0`. Both layouts are modelled here by that
    /// answer, read through the product's own reading of it (`bt_platform::vk_to_char_marks_a_dead_key`).
    ///
    /// MUTATION: drop the `virtual_key_is_dead` condition from `unidentified_text_key` (the dead
    /// `^` reads `ESC[221;26;0;1;10;1_…`), or refuse a key the map answers `0` for (the Kazakh `ж`
    /// reads nothing).
    #[test]
    fn a_dead_key_under_ctrl_alt_is_not_a_record() {
        /// `MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR)` on the French layout: the dead circumflex on
        /// `VK_OEM_6` carries the top bit; `VK_1` types `&`.
        fn french_is_dead(virtual_key: u16) -> bool {
            let answer = if virtual_key == 0xDD {
                0x8000_005E
            } else {
                0x26
            };
            bt_platform::vk_to_char_marks_a_dead_key(answer)
        }
        /// The same map on the Kazakh layout: `0` for `VK_OEM_1`, which types `ж`.
        fn kazakh_is_dead(virtual_key: u16) -> bool {
            let answer = if virtual_key == 0xBA { 0 } else { 0x31 };
            bt_platform::vk_to_char_marks_a_dead_key(answer)
        }
        let ctrl_alt = ModifiersState::CONTROL | ModifiersState::ALT;
        let sent = |virtual_key: u16, base: &str, physical, is_dead: fn(u16) -> bool| {
            keyboard_bytes(
                &Key::Unidentified(NativeKey::Windows(virtual_key)),
                &Key::Character(base.into()),
                KeyLocation::Standard,
                ctrl_alt,
                false,
                RECORDS,
                KeyOrigin {
                    platform: HostPlatform::Windows,
                    physical_key: PhysicalKey::Code(physical),
                    text_with_all_modifiers: None,
                    virtual_key_of_scan_code: no_virtual_key,
                    virtual_key_is_dead: is_dead,
                    shifted_character_of_virtual_key: us_shifted_character,
                    conpty: ConPtyKind::Shipped,
                },
            )
        };
        assert_eq!(
            sent(0xDD, "^", KeyCode::BracketLeft, french_is_dead),
            None,
            "the French dead circumflex"
        );
        assert_eq!(
            sent(0x31, "&", KeyCode::Digit1, french_is_dead),
            Some(b"\x1b[49;2;0;1;10;1_\x1b[49;2;0;0;10;1_".to_vec()),
            "an ordinary key of the French layout"
        );
        assert_eq!(
            sent(0xBA, "ж", KeyCode::Semicolon, kazakh_is_dead),
            Some(b"\x1b[186;39;0;1;10;1_\x1b[186;39;0;0;10;1_".to_vec()),
            "the Kazakh `ж`, which the map answers 0 for"
        );
    }

    /// RED (T-KEYBOARD-RECORDS, review round 3) — **a pane on the inbox ConPTY gets no records:
    /// every chord of the set is its legacy bytes there** (coordinator's ruling, 2026-09-29).
    ///
    /// The process falls back to the operating system's ConPTY when the packaged pair is missing
    /// or fails to load, and that ConPTY turns records into other bytes for a byte reader (it
    /// drops Ctrl+Alt+Enter altogether). There the promise holds that a program which never asked
    /// receives exactly what it received before. A pane with no ConPTY behind it writes none
    /// either. The pane's kind is fixed at spawn (`bt_pty::PtySession::conpty_kind`, pinned in
    /// `bt-pty`).
    ///
    /// MUTATION: drop `origin.conpty != ConPtyKind::Shipped` from `key_records` (the inbox
    /// Ctrl+Enter reads its record pair).
    #[test]
    fn a_pane_on_the_inbox_conpty_gets_no_records() {
        for (name, modifiers, legacy) in [
            ("Enter", ModifiersState::CONTROL, b"\r".to_vec()),
            ("Enter", ModifiersState::SHIFT, b"\r".to_vec()),
            (
                "Enter",
                ModifiersState::CONTROL | ModifiersState::ALT,
                b"\r".to_vec(),
            ),
            ("Tab", ModifiersState::SHIFT, b"\x1b[Z".to_vec()),
            ("Backspace", ModifiersState::CONTROL, b"\x7f".to_vec()),
            ("Space", ModifiersState::CONTROL, Vec::new()),
            ("1", ModifiersState::CONTROL, Vec::new()),
            (
                "1",
                ModifiersState::CONTROL | ModifiersState::ALT,
                Vec::new(),
            ),
        ] {
            let (logical, base) = windows_us_event(name, modifiers);
            let press = UsPress::new(name, modifiers);
            let sent = |origin| {
                keyboard_bytes(
                    &logical,
                    &base,
                    KeyLocation::Standard,
                    modifiers,
                    false,
                    RECORDS,
                    origin,
                )
                .unwrap_or_default()
            };
            assert_eq!(
                sent(press.on_the_inbox_conpty()),
                legacy,
                "{name} {modifiers:?} on the inbox ConPTY"
            );
            assert_eq!(
                sent(KeyOrigin {
                    conpty: ConPtyKind::NotConPty,
                    ..press.on(HostPlatform::Windows)
                }),
                legacy,
                "{name} {modifiers:?} with no ConPTY"
            );
            assert_ne!(
                sent(press.on(HostPlatform::Windows)),
                legacy,
                "{name} {modifiers:?} on the shipped ConPTY is its record pair"
            );
        }
    }

    /// RED (T-KEYBOARD-RECORDS, review round 2) — **Ctrl+Alt+1 on Windows, as winit really hands
    /// it over, is its record pair.**
    ///
    /// winit keeps Ctrl while Alt is down (Ctrl+Alt may be AltGr) and the US layout types nothing
    /// for Ctrl+Alt+1, so the press arrives as `Key::Unidentified(NativeKey::Windows(0x31))` with
    /// the key without modifiers `1` — not as `Key::Character("1")`, which the first round's model
    /// assumed. The record carries the virtual key Windows reported. A media key in the same
    /// shape (no text key under it) and a text key with no position are not records.
    ///
    /// MUTATION: drop `key_records`' `Key::Unidentified(_)` arm (the first three assertions read
    /// nothing).
    #[test]
    fn ctrl_alt_on_a_text_key_as_winit_hands_it_over_is_a_record() {
        let ctrl_alt = ModifiersState::CONTROL | ModifiersState::ALT;
        let one = Key::Character("1".into());
        let sent = |logical: &Key, base: &Key, modifiers, physical, protocol| {
            keyboard_bytes(
                logical,
                base,
                KeyLocation::Standard,
                modifiers,
                false,
                protocol,
                KeyOrigin {
                    platform: HostPlatform::Windows,
                    physical_key: physical,
                    text_with_all_modifiers: None,
                    virtual_key_of_scan_code: us_virtual_key,
                    virtual_key_is_dead: no_dead_keys,
                    shifted_character_of_virtual_key: us_shifted_character,
                    conpty: ConPtyKind::Shipped,
                },
            )
        };
        let digit_one = PhysicalKey::Code(KeyCode::Digit1);
        let unidentified_one = Key::Unidentified(NativeKey::Windows(0x31));
        assert_eq!(
            sent(&unidentified_one, &one, ctrl_alt, digit_one, RECORDS),
            Some(b"\x1b[49;2;0;1;10;1_\x1b[49;2;0;0;10;1_".to_vec()),
            "Ctrl+Alt+1"
        );
        assert_eq!(
            sent(
                &unidentified_one,
                &one,
                ctrl_alt | ModifiersState::SHIFT,
                digit_one,
                RECORDS
            ),
            Some(b"\x1b[49;2;0;1;26;1_\x1b[49;2;0;0;26;1_".to_vec()),
            "Ctrl+Shift+Alt+1"
        );
        let e = Key::Character("e".into());
        assert_eq!(
            sent(
                &Key::Unidentified(NativeKey::Windows(0x45)),
                &e,
                ctrl_alt,
                PhysicalKey::Code(KeyCode::KeyE),
                RECORDS
            ),
            Some(b"\x1b[69;18;0;1;10;1_\x1b[69;18;0;0;10;1_".to_vec()),
            "Ctrl+Alt+E: winit gives it no character, so no rung has its `ESC ^E` to send"
        );
        // Without win32-input-mode the same press sends nothing, as before.
        assert_eq!(
            sent(&unidentified_one, &one, ctrl_alt, digit_one, UNASKED),
            None
        );
        // A media key (its key without modifiers is not text) and a key with no position are not
        // records.
        let volume = Key::Named(NamedKey::AudioVolumeUp);
        assert_eq!(
            sent(
                &Key::Unidentified(NativeKey::Windows(0xAF)),
                &volume,
                ctrl_alt,
                PhysicalKey::Code(KeyCode::AudioVolumeUp),
                RECORDS
            ),
            None
        );
        assert_eq!(
            sent(
                &unidentified_one,
                &one,
                ctrl_alt,
                PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
                RECORDS
            ),
            None
        );
    }

    /// RED (T-KEYBOARD-CTRLALT) — **Ctrl+Alt and Ctrl+Shift+Alt on a letter or a digit reach a
    /// program that asked for the kitty protocol or modifyOtherKeys on Windows, as winit really
    /// hands the press over, with the bytes they have on a Mac.**
    ///
    /// winit keeps Ctrl while Alt is down on Windows (Ctrl+Alt may be AltGr), and the US layout
    /// types nothing for these chords, so each arrives as `Key::Unidentified(NativeKey::Windows(vk))`
    /// with its key without modifiers ([`windows_us_event`]); on a Mac the same chord arrives as
    /// its character ([`windows_us`]). Both must be encoded alike. The bytes are written here from
    /// the design note, not read from the encoder: the modifier value is `1 + Shift 1 + Alt 2 +
    /// Ctrl 4` (§4.1's table: 7 for Ctrl+Alt, 8 with Shift); flag 1's code is the un-shifted key
    /// (§4.2 rule 6: `e` 101, `1` 49); modifyOtherKeys' `k` is the character with Shift applied
    /// (§4.3: `E` 69, `!` 33, `{` 123), and both modes encode these chords — `e` and `[` have a
    /// legacy Ctrl code and Alt is held, `1` has none and Ctrl is held. A program that asked for
    /// neither still gets nothing on Windows (the legacy column), as before.
    ///
    /// MUTATION: drop `kitty_bytes`' `Key::Unidentified(_)` arm (the Windows flag-1 assertions read
    /// nothing), or drop `modify_other_keys_bytes`' `Key::Unidentified(_)` arm (the Windows
    /// modifyOtherKeys assertions read nothing), or take the Ctrl+Shift+Alt `k` from the key
    /// without modifiers instead of the layout's Shift character (Ctrl+Shift+Alt+1 reads 49).
    #[test]
    fn ctrl_alt_on_a_text_key_reaches_the_kitty_protocol_and_modify_other_keys_on_windows() {
        let ctrl_alt = ModifiersState::CONTROL | ModifiersState::ALT;
        let ctrl_shift_alt = ctrl_alt | ModifiersState::SHIFT;
        let cases: [(&str, ModifiersState, &str, &str); 6] = [
            ("e", ctrl_alt, "\x1b[101;7u", "\x1b[27;7;101~"),
            ("e", ctrl_shift_alt, "\x1b[101;8u", "\x1b[27;8;69~"),
            ("1", ctrl_alt, "\x1b[49;7u", "\x1b[27;7;49~"),
            ("1", ctrl_shift_alt, "\x1b[49;8u", "\x1b[27;8;33~"),
            ("[", ctrl_alt, "\x1b[91;7u", "\x1b[27;7;91~"),
            ("[", ctrl_shift_alt, "\x1b[91;8u", "\x1b[27;8;123~"),
        ];
        for (name, modifiers, kitty, modify_other_keys) in cases {
            let press = UsPress::new(name, modifiers);
            let (windows_logical, windows_base) = windows_us_event(name, modifiers);
            assert!(
                matches!(windows_logical, Key::Unidentified(NativeKey::Windows(_))),
                "{name} {modifiers:?}: winit hands it over with no character on Windows"
            );
            let (mac_logical, mac_base) = windows_us(name, modifiers.shift_key());
            for (logical, base, platform) in [
                (&windows_logical, &windows_base, HostPlatform::Windows),
                (&mac_logical, &mac_base, HostPlatform::MacOs),
            ] {
                let sent = |protocol| {
                    keyboard_bytes(
                        logical,
                        base,
                        KeyLocation::Standard,
                        modifiers,
                        false,
                        protocol,
                        press.on(platform),
                    )
                    .map(|bytes| String::from_utf8(bytes).expect("ASCII"))
                };
                let where_ = format!("{name} {modifiers:?} on {platform:?}");
                assert_eq!(sent(KITTY).as_deref(), Some(kitty), "{where_}: flag 1");
                assert_eq!(
                    sent(MOK1).as_deref(),
                    Some(modify_other_keys),
                    "{where_}: modifyOtherKeys 1"
                );
                assert_eq!(
                    sent(MOK2).as_deref(),
                    Some(modify_other_keys),
                    "{where_}: modifyOtherKeys 2"
                );
            }
            let unasked = keyboard_bytes(
                &windows_logical,
                &windows_base,
                KeyLocation::Standard,
                modifiers,
                false,
                UNASKED,
                press.on(HostPlatform::Windows),
            );
            assert_eq!(
                unasked, None,
                "{name} {modifiers:?}: nothing asked, nothing sent"
            );
        }
    }

    /// RED (T-KEYBOARD-CTRLALT) — **a dead key under Ctrl+Alt sends nothing under any protocol;
    /// an ordinary key of the same layout is encoded.**
    ///
    /// The French dead `^` (`VK_OEM_6`, scan code 26) under Ctrl+Alt arrives in the same shape as
    /// a text key — `Key::Unidentified`, with the key without modifiers `^` — because winit reports
    /// a dead key as the character it would compose. The one eligibility the three rungs share
    /// ([`unidentified_text_key`]) refuses it by the layout's dead-key bit, so neither protocol
    /// encodes it, nor does a record. The French `1` key (`&` unshifted) is encoded as `&` (38).
    ///
    /// MUTATION: drop the `virtual_key_is_dead` condition from `unidentified_text_key` (the dead
    /// `^` reads `CSI 94;7u` under flag 1).
    #[test]
    fn a_dead_key_under_ctrl_alt_sends_nothing_under_any_protocol() {
        fn french_is_dead(virtual_key: u16) -> bool {
            let answer = if virtual_key == 0xDD {
                0x8000_005E
            } else {
                0x26
            };
            bt_platform::vk_to_char_marks_a_dead_key(answer)
        }
        fn french_shifted(virtual_key: u16) -> Option<char> {
            (virtual_key == 0x31).then_some('1')
        }
        let ctrl_alt = ModifiersState::CONTROL | ModifiersState::ALT;
        let sent = |virtual_key: u16, base: &str, physical, modifiers, protocol| {
            keyboard_bytes(
                &Key::Unidentified(NativeKey::Windows(virtual_key)),
                &Key::Character(base.into()),
                KeyLocation::Standard,
                modifiers,
                false,
                protocol,
                KeyOrigin {
                    platform: HostPlatform::Windows,
                    physical_key: PhysicalKey::Code(physical),
                    text_with_all_modifiers: None,
                    virtual_key_of_scan_code: no_virtual_key,
                    virtual_key_is_dead: french_is_dead,
                    shifted_character_of_virtual_key: french_shifted,
                    conpty: ConPtyKind::Shipped,
                },
            )
        };
        for protocol in [UNASKED, KITTY, MOK1, MOK2, RECORDS] {
            for modifiers in [ctrl_alt, ctrl_alt | ModifiersState::SHIFT] {
                assert_eq!(
                    sent(0xDD, "^", KeyCode::BracketLeft, modifiers, protocol),
                    None,
                    "the French dead circumflex, {modifiers:?}, {protocol:?}"
                );
            }
        }
        assert_eq!(
            sent(0x31, "&", KeyCode::Digit1, ctrl_alt, KITTY),
            Some(b"\x1b[38;7u".to_vec())
        );
        assert_eq!(
            sent(0x31, "&", KeyCode::Digit1, ctrl_alt, MOK1),
            Some(b"\x1b[27;7;38~".to_vec())
        );
        assert_eq!(
            sent(
                0x31,
                "&",
                KeyCode::Digit1,
                ctrl_alt | ModifiersState::SHIFT,
                MOK2
            ),
            Some(b"\x1b[27;8;49~".to_vec()),
            "Shift on the French `&` key types `1`, and the layout says so"
        );
    }

    /// RED (T-KEYBOARD-RECORDS) — **a record is the press as Windows reported it, on any layout:
    /// the layout's virtual key for where the key is, and the character it typed.**
    ///
    /// On a Russian layout `Ctrl+ф` has no C0 code in the text winit hands over (`ф`), so VT
    /// cannot send it — but Windows types `^A` for it, because the key is `VK_A` there: the record
    /// carries `VK_A` from the layout and `Uc` 1 from `WM_CHAR`, which is what Windows Terminal
    /// sends. A keypad digit that typed text is `VK_NUMPAD1` whatever the layout maps its scan code
    /// to, because Num Lock was on. A text key whose position or virtual key is unknown, or a
    /// press that typed two UTF-16 units, is not one key event and keeps its legacy bytes; a named
    /// key whose position was not reported still has its virtual key, with scan code 0.
    ///
    /// MUTATION: take the virtual key from the US table instead of `virtual_key_of_scan_code`
    /// (the Russian record reads another key), or ask the layout for a keypad digit (it reads
    /// `VK_END`, 35).
    #[test]
    fn a_record_is_the_press_as_windows_reported_it_on_any_layout() {
        fn layout_answers_a_for_the_a_key_and_end_for_numpad_1(scan: u16) -> Option<u16> {
            match scan {
                0x1E => Some(0x41),
                0x4F => Some(0x23),
                _ => None,
            }
        }
        let origin = |physical: PhysicalKey, typed| KeyOrigin {
            platform: HostPlatform::Windows,
            physical_key: physical,
            text_with_all_modifiers: typed,
            virtual_key_of_scan_code: layout_answers_a_for_the_a_key_and_end_for_numpad_1,
            virtual_key_is_dead: no_dead_keys,
            shifted_character_of_virtual_key: us_shifted_character,
            conpty: ConPtyKind::Shipped,
        };
        let sent = |key: &Key, location, modifiers, origin| {
            keyboard_bytes(key, key, location, modifiers, false, RECORDS, origin)
        };
        let ctrl = ModifiersState::CONTROL;
        let ef = Key::Character("ф".into());
        assert_eq!(
            sent(
                &ef,
                KeyLocation::Standard,
                ctrl,
                origin(PhysicalKey::Code(KeyCode::KeyA), Some("\u{1}"))
            ),
            Some(b"\x1b[65;30;1;1;8;1_\x1b[65;30;1;0;8;1_".to_vec())
        );
        let one = Key::Character("1".into());
        assert_eq!(
            sent(
                &one,
                KeyLocation::Numpad,
                ctrl,
                origin(PhysicalKey::Code(KeyCode::Numpad1), None)
            ),
            Some(b"\x1b[97;79;0;1;8;1_\x1b[97;79;0;0;8;1_".to_vec())
        );
        let slash = Key::Character("/".into());
        assert_eq!(
            keyboard_bytes(
                &slash,
                &slash,
                KeyLocation::Numpad,
                ctrl,
                false,
                RECORDS,
                KeyOrigin {
                    virtual_key_of_scan_code: |scan| (scan == 0xE035).then_some(0x6F),
                    ..origin(PhysicalKey::Code(KeyCode::NumpadDivide), None)
                }
            ),
            Some(b"\x1b[111;53;0;1;264;1_\x1b[111;53;0;0;264;1_".to_vec()),
            "the keypad's `/` is an enhanced key"
        );
        // Unknown position, unknown virtual key, two units: legacy (nothing, for Ctrl+ф).
        assert_eq!(
            sent(
                &ef,
                KeyLocation::Standard,
                ctrl,
                origin(PhysicalKey::Unidentified(NativeKeyCode::Unidentified), None)
            ),
            None
        );
        assert_eq!(
            sent(
                &ef,
                KeyLocation::Standard,
                ctrl,
                origin(PhysicalKey::Code(KeyCode::KeyQ), None)
            ),
            None
        );
        assert_eq!(
            sent(
                &ef,
                KeyLocation::Standard,
                ctrl,
                origin(PhysicalKey::Code(KeyCode::KeyA), Some("\u{1F600}"))
            ),
            None
        );
        // An injected Enter (`VK_PACKET`, no position) with a modifier: `VK_RETURN`, scan code 0.
        let enter = Key::Named(NamedKey::Enter);
        assert_eq!(
            sent(
                &enter,
                KeyLocation::Standard,
                ModifiersState::SHIFT,
                origin(
                    PhysicalKey::Unidentified(NativeKeyCode::Windows(0)),
                    Some("\r")
                )
            ),
            Some(b"\x1b[13;0;13;1;16;1_\x1b[13;0;13;0;16;1_".to_vec())
        );
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **no letter is special**: every letter `a`–`z`, under Ctrl
    /// and under Alt+Ctrl, in all four modes.
    ///
    /// MUTATION: special-case `Ctrl+C` ahead of the protocols as the legacy path does (`c`
    /// under flag 1 reads `\x03`).
    #[test]
    fn every_letter_under_ctrl_and_alt_ctrl_in_every_mode() {
        let alt_ctrl = ModifiersState::ALT | ModifiersState::CONTROL;
        for letter in 'a'..='z' {
            let key = Key::Character(letter.to_string().into());
            let code = u32::from(letter);
            let control = (letter as u8) & 0x1f;
            for (modifiers, m) in [(ModifiersState::CONTROL, 5), (alt_ctrl, 7)] {
                let sent = |protocol| {
                    keyboard_bytes(
                        &key,
                        &key,
                        KeyLocation::Standard,
                        modifiers,
                        false,
                        protocol,
                        NOWHERE,
                    )
                };
                if is_paste_shortcut(&key, modifiers) {
                    // The paste chord of this platform (`Ctrl+V` off a Mac) never reaches
                    // the encoder's bytes in any mode: the paste door has it.
                    for protocol in EVERY_MODE {
                        assert_eq!(sent(protocol), None, "{letter} is the paste chord");
                    }
                    continue;
                }
                let legacy = if letter == 'c' || m == 5 {
                    vec![control]
                } else {
                    vec![0x1b, control]
                };
                assert_eq!(sent(UNASKED), Some(legacy), "{letter} legacy");
                assert_eq!(
                    sent(KITTY),
                    Some(format!("\x1b[{code};{m}u").into_bytes()),
                    "{letter} kitty"
                );
                let mok1 = if m == 5 {
                    vec![control]
                } else {
                    format!("\x1b[27;{m};{code}~").into_bytes()
                };
                assert_eq!(sent(MOK1), Some(mok1), "{letter} mok1");
                assert_eq!(
                    sent(MOK2),
                    Some(format!("\x1b[27;{m};{code}~").into_bytes()),
                    "{letter} mok2"
                );
            }
        }
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **a text key's code is the layout's un-shifted key**: `é`
    /// with Ctrl is `CSI 233;5u`, and Russian `ф` is 1092 — what kitty itself sends without
    /// flag 4.
    ///
    /// MUTATION: take the code from the ASCII key under the character (`Ctrl+ф` reads 97).
    #[test]
    fn a_text_keys_code_is_the_layouts_unshifted_key() {
        let e_acute = Key::Character("é".into());
        assert_eq!(
            keyboard_bytes(
                &e_acute,
                &e_acute,
                KeyLocation::Standard,
                ModifiersState::CONTROL,
                false,
                KITTY,
                NOWHERE,
            ),
            Some(b"\x1b[233;5u".to_vec())
        );
        let ef = Key::Character("ф".into());
        let capital_ef = Key::Character("Ф".into());
        assert_eq!(
            keyboard_bytes(
                &capital_ef,
                &ef,
                KeyLocation::Standard,
                ModifiersState::SHIFT | ModifiersState::ALT,
                false,
                KITTY,
                NOWHERE,
            ),
            Some(b"\x1b[1092;4u".to_vec())
        );
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **AltGr text is text in every mode, and Super sends nothing
    /// in any protocol.**
    ///
    /// On Windows winit removes Ctrl and Alt while AltGr is held, so `@` on a German layout
    /// arrives with no modifier and is text in all four modes. Super — the Windows key, or
    /// Command on a Mac — is never encoded: a chord holding it sends nothing under either
    /// protocol, and a letter with it sends nothing without one, as before. (Legacy keeps what
    /// it sent before for the named keys: `Super+Enter` is `\r` there, which
    /// `a_program_that_never_asked_gets_exactly_the_bytes_it_got_before` pins.)
    ///
    /// MUTATION: drop `modifiers.super_key()` from `withheld_from_every_protocol` (Super+Enter
    /// reads `CSI 13;9u` under flag 1).
    #[test]
    fn altgr_text_is_text_and_super_sends_nothing() {
        let at = Key::Character("@".into());
        let q = Key::Character("q".into());
        for protocol in EVERY_MODE {
            assert_eq!(
                keyboard_bytes(
                    &at,
                    &q,
                    KeyLocation::Standard,
                    ModifiersState::empty(),
                    false,
                    protocol,
                    NOWHERE,
                ),
                Some(b"@".to_vec()),
                "{protocol:?}"
            );
            let j = Key::Character("j".into());
            assert_eq!(
                keyboard_bytes(
                    &j,
                    &j,
                    KeyLocation::Standard,
                    ModifiersState::SUPER,
                    false,
                    protocol,
                    NOWHERE,
                ),
                None,
                "{protocol:?}"
            );
        }
        let enter = Key::Named(NamedKey::Enter);
        for protocol in [KITTY, MOK1, MOK2] {
            for modifiers in [
                ModifiersState::SUPER,
                ModifiersState::SUPER | ModifiersState::CONTROL,
                ModifiersState::SUPER | ModifiersState::SHIFT,
            ] {
                assert_eq!(
                    keyboard_bytes(
                        &enter,
                        &enter,
                        KeyLocation::Standard,
                        modifiers,
                        false,
                        protocol,
                        NOWHERE,
                    ),
                    None,
                    "{protocol:?} {modifiers:?}"
                );
            }
        }
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **on a Mac, Option types text unless the setting makes it
    /// Alt**, under the protocol as without it.
    ///
    /// With *Option key sends Alt* off, `effective_modifiers` takes Alt away and `⌥a` is the
    /// text `å`; with it on, Alt is Alt and flag 1 sends `CSI 97;3u`.
    ///
    /// MUTATION: encode the reported modifiers instead of the effective ones at the call site
    /// (`⌥a` would read `CSI 97;3u` with the setting off).
    #[test]
    fn macos_option_types_text_unless_the_setting_makes_it_alt() {
        let a = Key::Character("a".into());
        let a_ring = Key::Character("å".into());
        let off = effective_modifiers(ModifiersState::ALT, false, HostPlatform::MacOs);
        assert_eq!(
            keyboard_bytes(
                &a_ring,
                &a,
                KeyLocation::Standard,
                off,
                false,
                KITTY,
                NOWHERE
            ),
            Some("å".as_bytes().to_vec())
        );
        let on = effective_modifiers(ModifiersState::ALT, true, HostPlatform::MacOs);
        assert_eq!(
            keyboard_bytes(&a, &a, KeyLocation::Standard, on, false, KITTY, NOWHERE),
            Some(b"\x1b[97;3u".to_vec())
        );
        // A Mac may hand a Ctrl chord over as the control character itself; the code is the
        // key without modifiers, in both protocols.
        let control_e = Key::Character("\u{5}".into());
        let e = Key::Character("e".into());
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(
            keyboard_bytes(
                &control_e,
                &e,
                KeyLocation::Standard,
                ctrl_shift,
                false,
                KITTY,
                NOWHERE,
            ),
            Some(b"\x1b[101;6u".to_vec())
        );
        assert_eq!(
            keyboard_bytes(
                &control_e,
                &e,
                KeyLocation::Standard,
                ctrl_shift,
                false,
                MOK2,
                NOWHERE,
            ),
            Some(b"\x1b[27;6;69~".to_vec())
        );
    }

    /// Encode one key the way `Runtime::keyboard_input` does: the protocol read from the
    /// terminal at the moment of the key.
    fn encoded_by(
        terminal: &bt_term::TerminalAdapter,
        key: &Key,
        modifiers: ModifiersState,
    ) -> Vec<u8> {
        keyboard_bytes(
            key,
            key,
            KeyLocation::Standard,
            modifiers,
            terminal.application_cursor_mode(),
            terminal.modes().keyboard,
            NOWHERE,
        )
        .unwrap_or_default()
    }

    /// RED (T-KEYBOARD-PROTOCOL) — **the golden replays: what Codex on Unix and Claude Code
    /// write, and what they then receive.**
    ///
    /// Codex (crossterm): `CSI ? u`, then `CSI > 7 u` (disambiguate, event types, alternate
    /// keys — Folio honours the first). Plain Enter stays `\r`, Ctrl+Enter is `CSI 13;5u`,
    /// Shift+Enter `CSI 13;2u`, Esc `CSI 27u`, `a` is `a`, and no release event is sent for
    /// anything. Its exit (`CSI < 1 u`, `CSI < u`, `CSI > 4 ; 0 m`) puts Ctrl+Enter back to
    /// `\r`. Claude Code asks for `CSI > 5 u` and `CSI > 4 ; 2 m`: kitty wins, so Ctrl+Enter
    /// is `CSI 13;5u` and not `CSI 27;5;13~`.
    ///
    /// MUTATION: prefer modifyOtherKeys over kitty in `keyboard_bytes` (the Claude Code replay
    /// reads `CSI 27;5;13~`).
    #[test]
    fn the_golden_replays_of_codex_on_unix_and_claude_code() {
        let enter = Key::Named(NamedKey::Enter);
        let esc = Key::Named(NamedKey::Escape);
        let a = Key::Character("a".into());
        let one = std::num::NonZeroU32::new(24).expect("nonzero");
        let mut codex = bt_term::TerminalAdapter::new(one, one);
        codex.feed(b"\x1b[?u\x1b[>7u");
        assert_eq!(codex.take_pty_writes(), vec![b"\x1b[?0u".to_vec()]);
        assert_eq!(encoded_by(&codex, &enter, ModifiersState::empty()), b"\r");
        assert_eq!(
            encoded_by(&codex, &enter, ModifiersState::CONTROL),
            b"\x1b[13;5u"
        );
        assert_eq!(
            encoded_by(&codex, &enter, ModifiersState::SHIFT),
            b"\x1b[13;2u"
        );
        assert_eq!(
            encoded_by(&codex, &esc, ModifiersState::empty()),
            b"\x1b[27u"
        );
        assert_eq!(encoded_by(&codex, &a, ModifiersState::empty()), b"a");
        codex.feed(b"\x1b[<1u\x1b[<u\x1b[>4;0m");
        assert_eq!(encoded_by(&codex, &enter, ModifiersState::CONTROL), b"\r");

        let mut claude = bt_term::TerminalAdapter::new(one, one);
        claude.feed(b"\x1b[?u\x1b[>5u\x1b[>4;2m");
        assert_eq!(
            encoded_by(&claude, &enter, ModifiersState::CONTROL),
            b"\x1b[13;5u",
            "kitty wins over modifyOtherKeys"
        );
        assert_eq!(
            encoded_by(&claude, &enter, ModifiersState::SHIFT),
            b"\x1b[13;2u"
        );
    }

    /// Encode one key the way `Runtime::keyboard_input` does on Windows, pressed on a US layout.
    fn encoded_on_windows(
        terminal: &bt_term::TerminalAdapter,
        name: &str,
        modifiers: ModifiersState,
    ) -> Vec<u8> {
        let (logical, base) = windows_us_event(name, modifiers);
        let press = UsPress::new(name, modifiers);
        keyboard_bytes(
            &logical,
            &base,
            KeyLocation::Standard,
            modifiers,
            terminal.application_cursor_mode(),
            terminal.modes().keyboard,
            press.on(HostPlatform::Windows),
        )
        .unwrap_or_default()
    }

    /// RED (T-KEYBOARD-RECORDS) — **the golden replay of a PowerShell pane: a session that never
    /// asks, with ConPTY's `?9001h` seen, gets Ctrl+Enter as its key record pair; a program that
    /// then asks for kitty gets `CSI 13;5u`; once it pops, the record pair again.**
    ///
    /// The bytes are ConPTY's session head (`\e[1t\e[c\e[?1004h\e[?9001h`, as a real ConPTY writes
    /// it — `bt-term`'s `a_teardown_that_conpty_re_enables_does_not_type_at_the_pane` and the
    /// adapter's doc record it) and then what a WSL or Node program writes. PSReadLine itself never
    /// asks. Without `?9001h` — a transport that does not parse records — Ctrl+Enter stays `\r`.
    ///
    /// MUTATION: read `win32_input_mode` as true in `keyboard_bytes` whatever the terminal says
    /// (the first pane, which never saw `?9001h`, reads a record pair).
    #[test]
    fn the_golden_replay_of_a_powershell_pane() {
        let one = std::num::NonZeroU32::new(24).expect("nonzero");
        let mut bare = bt_term::TerminalAdapter::new(one, one);
        bare.feed(b"\x1b[1t\x1b[c\x1b[?1004h");
        assert_eq!(
            encoded_on_windows(&bare, "Enter", ModifiersState::CONTROL),
            b"\r",
            "no `?9001h`, no records"
        );

        let mut pane = bt_term::TerminalAdapter::new(one, one);
        pane.feed(b"\x1b[1t\x1b[c\x1b[?1004h\x1b[?9001h");
        let ctrl_enter = b"\x1b[13;28;10;1;8;1_\x1b[13;28;10;0;8;1_".as_slice();
        assert_eq!(
            encoded_on_windows(&pane, "Enter", ModifiersState::CONTROL),
            ctrl_enter
        );
        assert_eq!(
            encoded_on_windows(&pane, "Enter", ModifiersState::SHIFT),
            SHIFT_ENTER_RECORDS
        );
        assert_eq!(
            encoded_on_windows(&pane, "Enter", ModifiersState::empty()),
            b"\r"
        );
        assert_eq!(
            encoded_on_windows(&pane, "e", ModifiersState::empty()),
            b"e"
        );
        pane.feed(b"\x1b[>1u");
        assert_eq!(
            encoded_on_windows(&pane, "Enter", ModifiersState::CONTROL),
            b"\x1b[13;5u",
            "a program that asked wins"
        );
        pane.feed(b"\x1b[<u");
        assert_eq!(
            encoded_on_windows(&pane, "Enter", ModifiersState::CONTROL),
            ctrl_enter,
            "and after its pop, the records again"
        );
        pane.feed(b"\x1b[>4;2m");
        assert_eq!(
            encoded_on_windows(&pane, "Enter", ModifiersState::CONTROL),
            b"\x1b[27;5;13~",
            "modifyOtherKeys wins too"
        );
        pane.feed(b"\x1b[>4m");
        assert_eq!(
            encoded_on_windows(&pane, "Enter", ModifiersState::CONTROL),
            ctrl_enter
        );
    }
}
