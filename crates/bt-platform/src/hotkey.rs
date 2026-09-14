//! **The key a window answers to when it is not on the screen** — the quake
//! terminal's summon (`docs/DESIGN.md` §7.54).
//!
//! A fifth unsafe boundary in this crate, against a fifth thing. `windows_impl`
//! is Win32 for the sake of a window this process owns, [`crate::webview`] is
//! WebView2, [`crate::hang`] is Win32 turned on this process, and
//! [`crate::attention_pipe`] is a channel other processes speak into. This is
//! Win32 turned on **the keyboard while somebody else has it** — a chord that
//! has to arrive when no window of ours is focused, and a foreground that has to
//! go back to whoever held it.
//!
//! # Why the two halves are one module
//!
//! A summon that cannot hand the foreground back is half a summon. The window
//! comes up over whatever the reader was doing; when it goes away again the
//! keyboard belongs to that other window, and nothing in this process will be
//! told to give it back. So the registration and [`give_foreground_to`] are one
//! subject and are written in one place — and the two Win32 dances they perform,
//! `RegisterHotKey` on a thread with no window and `AttachThreadInput` around a
//! `SetForegroundWindow`, are the same dance seen from either end: both exist
//! because the foreground is a thing Windows will not simply hand to a process
//! that does not already have it.
//!
//! # The same two halves on a Mac (M4-8)
//!
//! Both sentences above are true there with every noun changed. The chord is
//! claimed with Carbon's `RegisterEventHotKey` against the **application** event
//! target — the one mechanism on that platform that hears a key while another
//! program has it *and* asks for no TCC grant; see the `macos_hotkey` module's
//! own header for why the other two were refused. And the foreground that has
//! to go back is an *application* rather than a window, which is why [`Foreground`] exists and
//! why [`hand_back_to`] is a different verb from [`give_foreground_to`] rather
//! than the same call twice.
//!
//! The two roads a press travels have nothing in common — a thread message into
//! winit's pump on one side, a Carbon handler on the other — and they meet at
//! [`summons_wake`], which is where this process says what a summon does.
//!
//! # What is pure and what is not
//!
//! [`registration_bits`] is the whole of the translation from a chord to the two
//! integers `RegisterHotKey` takes, and it is pure so a test can hold it on any
//! host — the rule [`crate::custom_frame_hit_test`] is written under, for its
//! reason: it is the part that can be wrong without a keyboard. Everything below
//! it needs a message queue and is gated on Windows.
//!
//! **[`carbon_key_code`] and [`carbon_registration_bits`] are the macOS twins of
//! that, and the macOS arm is the *more* testable of the two** — which is the
//! one surprise in this module. `RegisterEventHotKey` takes a key code that
//! names a position on the keyboard rather than a character, so the translation
//! has no `VkKeyScanW` in it and no installed layout behind it: it is a table,
//! it is pure, and every one of its answers is asserted on this workspace's
//! Windows host.

use crate::NativeWindow;

/// **A chord as the door that will claim it understands one**: four modifier
/// flags and the key code they are held with.
///
/// A key code and not a character, because the question "which key is that" is
/// answered before a chord gets here — [`summon_key_code`] is the one call that
/// answers it, and it answers in the currency of the platform that is about to
/// be asked. This type is what is left once that answer is in hand.
///
/// **The currency is the platform's and the field is one field** (M4-8). On
/// Windows [`summon_key_code`] answers with a Win32 virtual key, the number
/// `RegisterHotKey` takes; on macOS it answers with a `kVK_*` virtual key code,
/// the number `RegisterEventHotKey` takes. They are two different numbers for
/// the same press — `` ` `` is `0xc0` on one machine and `0x32` on the other —
/// and a struct with one field per platform would be a struct whose other half
/// is always a lie. What makes one field safe is that nothing above this module
/// ever *reads* it: `bt-app` fills it from [`summon_key_code`] and hands the
/// whole value straight back to [`register`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Windows key, which is Command on a Mac — one bit, because
    /// `winit`'s `ModifiersState::SUPER` is one bit and the two keyboards wear
    /// it in the same place. No row of this product's own table wears it on
    /// Windows, but a global summon is exactly the kind of key a person reaches
    /// for it on, and dropping the flag here would make that a decision this
    /// crate had taken on their behalf.
    pub win: bool,
    /// The key, in the currency named on this type.
    pub virtual_key: u16,
}

/// **The key half of a summon chord, in nobody's currency** (M4-8).
///
/// The table upstairs stores a chord as a person wrote it — a character they
/// typed, or a key that has a name rather than a character — and that is the
/// only description of a key that means the same thing on two platforms. The
/// numbers do not: a Win32 virtual key is one machine's answer and a `kVK_*`
/// code is the other's, and [`summon_key_code`] is the single door between
/// them. `bt-app` says which key; this crate says which number.
///
/// It is this crate's own enum rather than `winit::keyboard::Key` because
/// `winit` is not a dependency of this crate outside its tests, and because the
/// set of keys a *desktop-wide* claim may be made on is smaller than the set of
/// keys a window answers: a chord with no modifier on it is refused
/// ([`holds_a_summon_modifier`]), so nothing in here needs a name for a key that
/// only ever arrives bare.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SummonKey {
    /// The character the binding names, as the reader typed it into the
    /// recorder.
    Character(char),
    /// A key with a name rather than a character.
    Named(SummonNamedKey),
}

/// The keys a chord names rather than spells — `winit`'s `NamedKey`, narrowed to
/// the rows this product's own table can hold.
///
/// It is deliberately the same set `bt_app::webhost::named_key_virtual_key`
/// knows, and the two are held equal by a test in that crate rather than by a
/// shared table: the numbers *that* function answers with go to WebView2 and
/// the numbers this one answers with go to the platform's hotkey door, and a
/// single table would be one place deciding two questions that only happen to
/// have the same answer on Windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SummonNamedKey {
    Tab,
    Escape,
    Enter,
    Space,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

/// **The number this machine's hotkey door names a key by**, or `None` when it
/// has no number for it.
///
/// The one door between [`SummonKey`] and [`Hotkey::virtual_key`], and the
/// reason that field can be one field. Each arm is described where it is
/// written; what they share is the shape of the refusal. `None` is *not* "this
/// platform has no global hotkey" — it is **"this keyboard cannot press that"**,
/// which is a thing a reader can fix by recording a different key, and it is
/// what [`HotkeyFault::NoSuchKey`] is the sentence for.
#[must_use]
pub fn summon_key_code(key: SummonKey) -> Option<u16> {
    this_platforms_key_code(key)
}

/// [`summon_key_code`]'s three arms, as three definitions.
///
/// Three functions rather than three `cfg` blocks inside one, which is this
/// crate's shape everywhere else: a `cfg` that chooses between *definitions* is
/// read by the compiler before anything about types is decided, and a reader
/// looking for what this platform does finds one body rather than a body with
/// two thirds of it crossed out.
#[cfg(windows)]
fn this_platforms_key_code(key: SummonKey) -> Option<u16> {
    win32_key_code(key)
}

#[cfg(target_os = "macos")]
fn this_platforms_key_code(key: SummonKey) -> Option<u16> {
    carbon_key_code(key)
}

/// A platform with no door to claim a chord at has no number to name a key by
/// either, and answering one would be answering for a machine nobody asked.
/// `register`'s own third arm says the same thing in a sentence.
#[cfg(not(any(windows, target_os = "macos")))]
fn this_platforms_key_code(key: SummonKey) -> Option<u16> {
    let _ = key;
    None
}

/// The Win32 half of [`summon_key_code`].
///
/// **A character is asked of the layout and a named key is read off a table**,
/// and the difference is the whole of the Windows answer: `VkKeyScanW` reports
/// which key *this* installed layout types a character on — `` ` `` is
/// `VK_OEM_3` on a US keyboard and something else on a German one, and the
/// person pressing it is the one whose layout counts — while a named key already
/// *is* a virtual key wearing a name, so asking a layout about `F9` would be
/// asking a question that has no second answer.
#[cfg(windows)]
fn win32_key_code(key: SummonKey) -> Option<u16> {
    Some(match key {
        SummonKey::Character(character) => crate::virtual_key_for_character(character)?,
        SummonKey::Named(named) => match named {
            SummonNamedKey::Tab => 0x09,
            SummonNamedKey::Escape => 0x1b,
            SummonNamedKey::Enter => 0x0d,
            SummonNamedKey::Space => 0x20,
            SummonNamedKey::Backspace => 0x08,
            SummonNamedKey::Delete => 0x2e,
            SummonNamedKey::Insert => 0x2d,
            SummonNamedKey::Home => 0x24,
            SummonNamedKey::End => 0x23,
            SummonNamedKey::PageUp => 0x21,
            SummonNamedKey::PageDown => 0x22,
            SummonNamedKey::ArrowLeft => 0x25,
            SummonNamedKey::ArrowUp => 0x26,
            SummonNamedKey::ArrowRight => 0x27,
            SummonNamedKey::ArrowDown => 0x28,
            SummonNamedKey::F1 => 0x70,
            SummonNamedKey::F2 => 0x71,
            SummonNamedKey::F3 => 0x72,
            SummonNamedKey::F4 => 0x73,
            SummonNamedKey::F5 => 0x74,
            SummonNamedKey::F6 => 0x75,
            SummonNamedKey::F7 => 0x76,
            SummonNamedKey::F8 => 0x77,
            SummonNamedKey::F9 => 0x78,
            SummonNamedKey::F10 => 0x79,
            SummonNamedKey::F11 => 0x7a,
            SummonNamedKey::F12 => 0x7b,
        },
    })
}

/// **The macOS half of [`summon_key_code`], and it is a table on purpose**
/// (M4-8).
///
/// `RegisterEventHotKey` takes a **virtual key code**, which on this platform
/// names a *position on the keyboard* and not a character: `kVK_ANSI_Grave` is
/// the key to the left of `1`, whatever that key types. That is what makes the
/// whole translation a pure function with no system call in it, and it is why
/// the Windows arm above cannot be copied — there `VkKeyScanW` is the only
/// honest answer, and here there is no question to ask.
///
/// **Named keys are exactly right.** `Escape`, `F9`, `Home` and the arrows have
/// one position on every Mac keyboard ever sold, so the table below is the whole
/// truth for them.
///
/// **Characters are right for a keyboard laid out like a US one, and the limit
/// is written here rather than discovered.** The positions below are the ANSI
/// ones: `kVK_ANSI_A` is the key that types `a` on a US layout. A reader on a
/// French AZERTY who records `⌃A` presses the key labelled `A` — which is the
/// ANSI `Q` position — and this table would claim the ANSI `A` position, a key
/// their fingers are not on. Three things bound how far that reaches:
///
/// * the shipped default is `` ⌃` `` ([`crate::HostPlatform::MacOs`]'s column in
///   `bt_app::shortcuts::BINDINGS`), and the backtick key sits in the same
///   position on every Latin layout this product has a reader on;
/// * the digits, the punctuation and the whole of the top row are positional on
///   every layout, so only the letters can move;
/// * a chord with no modifier is refused ([`holds_a_summon_modifier`]), so the
///   worst case is a modified letter that summons nothing until it is recorded
///   again — never a key taken away from another program.
///
/// **The general answer, when a reader reports it, is `UCKeyTranslate`** against
/// the current `TISInputSource`: ask the installed layout which of the 128 key
/// codes produces this character, exactly as `VkKeyScanW` is asked on the other
/// side. It is not written here because it is a second unsafe surface, a run of
/// the whole key-code space per call, and an answer that changes while the
/// process is running — and because a table that is wrong for one layout and
/// testable on every host is a better trade than a syscall that is right for
/// every layout and provable on one machine.
///
/// **Compiled on every platform on purpose**, which is [`crate::HostPlatform`]'s
/// own argument at a third door: the answer is a table, so the machine writing
/// this ticket can ask what the machine running it will do. Every assertion
/// about the macOS translation in this workspace — this crate's and `bt-app`'s
/// both — is made on a Windows host.
#[must_use]
pub fn carbon_key_code(key: SummonKey) -> Option<u16> {
    Some(match key {
        SummonKey::Character(character) => match character.to_ascii_lowercase() {
            'a' => 0x00,
            's' => 0x01,
            'd' => 0x02,
            'f' => 0x03,
            'h' => 0x04,
            'g' => 0x05,
            'z' => 0x06,
            'x' => 0x07,
            'c' => 0x08,
            'v' => 0x09,
            'b' => 0x0b,
            'q' => 0x0c,
            'w' => 0x0d,
            'e' => 0x0e,
            'r' => 0x0f,
            'y' => 0x10,
            't' => 0x11,
            '1' => 0x12,
            '2' => 0x13,
            '3' => 0x14,
            '4' => 0x15,
            '6' => 0x16,
            '5' => 0x17,
            '=' => 0x18,
            '9' => 0x19,
            '7' => 0x1a,
            '-' => 0x1b,
            '8' => 0x1c,
            '0' => 0x1d,
            ']' => 0x1e,
            'o' => 0x1f,
            'u' => 0x20,
            '[' => 0x21,
            'i' => 0x22,
            'p' => 0x23,
            'l' => 0x25,
            'j' => 0x26,
            '\'' => 0x27,
            'k' => 0x28,
            ';' => 0x29,
            '\\' => 0x2a,
            ',' => 0x2b,
            '/' => 0x2c,
            'n' => 0x2d,
            'm' => 0x2e,
            '.' => 0x2f,
            '`' => 0x32,
            // Every other character — an accented letter, a CJK ideograph, a
            // symbol no ANSI key carries — has no position on this keyboard,
            // and saying so is what lets `register` answer `NoSuchKey` rather
            // than claim key code zero, which on this platform is the letter
            // `A` and not "no key at all".
            _ => return None,
        },
        SummonKey::Named(named) => match named {
            SummonNamedKey::Tab => 0x30,
            SummonNamedKey::Escape => 0x35,
            SummonNamedKey::Enter => 0x24,
            SummonNamedKey::Space => 0x31,
            // `kVK_Delete` is the key a Mac calls *delete* and every other
            // keyboard calls backspace, and `kVK_ForwardDelete` is the one
            // `Delete` names elsewhere. Reading the pair the other way round is
            // the single likeliest mistake in this table, which is why they are
            // adjacent here and pinned by a test.
            SummonNamedKey::Backspace => 0x33,
            SummonNamedKey::Delete => 0x75,
            // `kVK_Help`, which is the position the `Insert` key occupies on a
            // PC keyboard plugged into a Mac.
            SummonNamedKey::Insert => 0x72,
            SummonNamedKey::Home => 0x73,
            SummonNamedKey::End => 0x77,
            SummonNamedKey::PageUp => 0x74,
            SummonNamedKey::PageDown => 0x79,
            SummonNamedKey::ArrowLeft => 0x7b,
            SummonNamedKey::ArrowUp => 0x7e,
            SummonNamedKey::ArrowRight => 0x7c,
            SummonNamedKey::ArrowDown => 0x7d,
            // The function row is **not** in numeric order on this platform and
            // never has been; the codes below are the ones `Events.h` publishes.
            SummonNamedKey::F1 => 0x7a,
            SummonNamedKey::F2 => 0x78,
            SummonNamedKey::F3 => 0x63,
            SummonNamedKey::F4 => 0x76,
            SummonNamedKey::F5 => 0x60,
            SummonNamedKey::F6 => 0x61,
            SummonNamedKey::F7 => 0x62,
            SummonNamedKey::F8 => 0x64,
            SummonNamedKey::F9 => 0x65,
            SummonNamedKey::F10 => 0x6d,
            SummonNamedKey::F11 => 0x67,
            SummonNamedKey::F12 => 0x6f,
        },
    })
}

/// Carbon's modifier masks, written as the numbers they are.
///
/// Constants of this crate's own for `MOD_ALT`'s reason at a second door: the
/// mapping is the part with an opinion in it. Unlike the Win32 four there is no
/// crate in the tree that publishes these — Carbon has no binding in the `objc2`
/// family — so the pinning test on the other side has no counterpart here, and
/// the numbers are instead held by the proof that presses the key on a real Mac.
const CMD_KEY: u32 = 0x0100;
const SHIFT_KEY: u32 = 0x0200;
const OPTION_KEY: u32 = 0x0800;
const CONTROL_KEY: u32 = 0x1000;

/// **Carbon's four-character codes, out where a test can reach them**
/// (T-MAC-SUMMON-DIAG).
///
/// These six were written inside the `#[cfg(target_os = "macos")]` module, which
/// is the only part of this file's macOS arm a Windows host cannot see — and it
/// is where the one wrong number in the whole road spent a release. See
/// [`K_EVENT_PARAM_DIRECT_OBJECT`] for what it was.
///
/// [`carbon_key_code`]'s own note is the rule they now follow: **compiled on
/// every platform on purpose**, so the machine writing the ticket can ask what
/// the machine running it will do. `pub` for the same reason that function is —
/// the assertion is made from a test on a host that has no Carbon at all, and a
/// constant a `cfg` hides is a constant no such test can name.
///
/// `kEventClassKeyboard`. A hot key press is a keyboard event, and it is the
/// only class this module ever asks for.
pub const K_EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");

/// `kEventHotKeyPressed`. `kEventHotKeyReleased` is 6 and is deliberately not
/// asked for: this key's verb is a toggle spent on the press.
pub const K_EVENT_HOT_KEY_PRESSED: u32 = 5;

/// **`kEventParamDirectObject`, and it is four hyphens** — the defect
/// T-MAC-SUMMON-DIAG found, written here as the number rather than only as the
/// characters so that the pinning test is a second reading rather than an echo.
///
/// It shipped as `'obj '`, which is a real four-character code in another
/// namespace entirely — `typeObjectSpecifier`, an Apple event's way of naming a
/// thing rather than a Carbon event's way of naming its subject. Nothing
/// refused it: `GetEventParameter` answered `eventParameterNotFoundErr` for a
/// parameter the event does not carry, the handler below read that as
/// "not ours" and returned `noErr`, and the press was **swallowed in silence**.
/// The chord was claimed, so no other program saw it either; the window never
/// came down; nothing was written anywhere.
///
/// **Why the proof passed it.** `crates/bt-platform/tests/macos_hotkey.rs`
/// builds the event it sends, with `SetEventParameter` under the same constant.
/// Two halves of one file agreeing on a wrong name round-trip perfectly. The
/// system's own `kEventHotKeyPressed` does not agree with either of them — it
/// carries the `EventHotKeyID` under `'----'`, which is what
/// every Carbon hot key sample ever published reads it out of — so the one
/// station in that proof that used a real press was the one that failed, and it
/// failed for a second true reason (no Accessibility grant for `CGEventPost`)
/// that hid this one.
pub const K_EVENT_PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

/// `typeEventHotKeyID` — the type the parameter above is carried as.
pub const TYPE_EVENT_HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");

/// `eventHotKeyExistsErr` — somebody else has this chord.
pub const EVENT_HOT_KEY_EXISTS_ERR: i32 = -9878;

/// **This process's own four-character signature.**
///
/// `folo`, because the handler is offered *every* hot key event that reaches
/// this application's event target, including ones a framework registered
/// without telling anybody. The signature plus the id is what makes "is this
/// ours" a question with an answer.
pub const SUMMON_SIGNATURE: u32 = u32::from_be_bytes(*b"folo");

/// **The macOS translation, and the only part of that arm a test can hold
/// without a keyboard** — [`registration_bits`]'s twin, and written beside it
/// for that reason.
///
/// The two integers `RegisterEventHotKey` takes, in the order this crate states
/// them everywhere: modifiers, then the key.
///
/// **There is no zero clause here, and its absence is the finding** (M4-8). The
/// Windows arm refuses `virtual_key == 0` because Win32 has no key on that
/// number, so a zero is `VkKeyScanW` having failed. On this platform key code
/// `0x00` is `kVK_ANSI_A` — an ordinary letter somebody may well bind — and a
/// copied zero check would be a summon on `⌃A` that silently refused to
/// register. "This keyboard cannot press that" is said by [`carbon_key_code`]
/// answering `None`, upstream of here, which is the only place that can tell the
/// two apart.
///
/// **And no no-repeat bit**, because Carbon has none and needs none: a hot key
/// held down delivers one `kEventHotKeyPressed` and then nothing until it is
/// released, which is the behaviour `MOD_NOREPEAT` has to be asked for on the
/// other side.
#[must_use]
pub fn carbon_registration_bits(hotkey: Hotkey) -> Option<(u32, u32)> {
    if !holds_a_summon_modifier(hotkey) {
        return None;
    }
    let mut modifiers = 0;
    if hotkey.ctrl {
        modifiers |= CONTROL_KEY;
    }
    if hotkey.alt {
        modifiers |= OPTION_KEY;
    }
    if hotkey.shift {
        modifiers |= SHIFT_KEY;
    }
    if hotkey.win {
        modifiers |= CMD_KEY;
    }
    Some((modifiers, u32::from(hotkey.virtual_key)))
}

/// Win32's `MOD_*` values, written as the numbers they are.
///
/// Constants of this crate's own rather than the `windows` crate's, for
/// [`crate::CustomFrameHit`]'s reason: the mapping is the part with an opinion
/// in it, so it is expressed without Win32 constants and then pinned *against*
/// them by a test that only builds where they exist. A number written twice and
/// checked once is one decision; a number a test reads out of the same constant
/// the code did is no check at all.
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
/// **Held is not pressed again.**
///
/// Without it Windows repeats `WM_HOTKEY` for as long as the key is down, at the
/// keyboard's own repeat rate — and this key's verb is a *toggle*, so a summon
/// held for half a second would show and hide the window a dozen times and land
/// on whichever parity the finger happened to lift on. One press of this chord
/// means one thing, so exactly one message is asked for.
const MOD_NOREPEAT: u32 = 0x4000;

/// `WM_HOTKEY`, as its own number — see [`MOD_ALT`] for why this module writes
/// Win32's numbers down rather than reading them out of the constant its test
/// would also read.
const WM_HOTKEY: u32 = 0x0312;

/// **The translation, and the only part of this module a test can hold without a
/// keyboard**: a chord in, the two integers `RegisterHotKey` takes out.
///
/// `None` for a chord with no key at all. A virtual key of zero is what
/// [`crate::virtual_key_for_character`] answers with when the installed layout
/// cannot produce the character the chord names, and registering it would claim
/// whatever key Windows decides `0` means rather than the one nobody can press.
///
/// **And `None` for a chord no modifier holds down** ([`holds_a_summon_modifier`],
/// R2-14). This used to read "a chord with no modifier is allowed", on the
/// grounds that the product's opinion about bare keys belonged upstairs with the
/// recorder. It does — the *sentence* a person reads is
/// `bt_app::shortcuts::chord_verdict`'s — but the consequence of getting it wrong
/// is not a sentence: `RegisterHotKey` takes the key out of the input stream for
/// **the whole desktop**, so a bare `k` recorded once means no program on the
/// machine sees that letter again until Folio exits, and a persisted one means it
/// comes back at every launch. A refusal that lives only where a person can read
/// it is a refusal a hand-edited file walks past, so it is stated here as well,
/// at the one call that makes the claim.
#[must_use]
pub fn registration_bits(hotkey: Hotkey) -> Option<(u32, u32)> {
    if hotkey.virtual_key == 0 || !holds_a_summon_modifier(hotkey) {
        return None;
    }
    let mut modifiers = MOD_NOREPEAT;
    if hotkey.ctrl {
        modifiers |= MOD_CONTROL;
    }
    if hotkey.alt {
        modifiers |= MOD_ALT;
    }
    if hotkey.shift {
        modifiers |= MOD_SHIFT;
    }
    if hotkey.win {
        modifiers |= MOD_WIN;
    }
    Some((modifiers, u32::from(hotkey.virtual_key)))
}

/// **Whether a chord is one a desktop-wide claim may be made for** (R2-14).
///
/// At least one of Ctrl, Alt and the Windows key. **Shift alone is not enough**,
/// and that is the clause worth writing down: `Shift+A` is how a capital `A` is
/// typed, so claiming it would take a letter away from every program on the
/// machine exactly as surely as claiming the bare letter would — the difference
/// between them is a shape, not a consequence.
///
/// A function key alone is refused by the same rule, and deliberately: `F9` is a
/// key `less`, `gdb`, an IDE and a spreadsheet all answer to, and a terminal that
/// swallowed it desktop-wide would be taking it from all of them for one window
/// nobody is looking at.
#[must_use]
pub const fn holds_a_summon_modifier(hotkey: Hotkey) -> bool {
    hotkey.ctrl || hotkey.alt || hotkey.win
}

/// Why a chord could not be claimed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HotkeyFault {
    /// **Somebody else has this key** — `ERROR_HOTKEY_ALREADY_REGISTERED` on
    /// Windows, `eventHotKeyExistsErr` on macOS, which is the same sentence in
    /// another dialect and is why M4-8 added no variant here.
    ///
    /// The one refusal that is neither the reader's mistake nor this program's:
    /// another running program registered the chord first, and there is nothing
    /// to fix except to choose a different one. It is also what a *second* copy
    /// of this program is told, which is how "the summon belongs to the instance
    /// that started first" comes to be true — enforced by Windows, rather than by
    /// a lock of ours.
    AlreadyRegistered,
    /// The chord names a key this machine has no code for.
    NoSuchKey,
    /// **The chord holds none of Ctrl, Alt or the Windows key** — see
    /// [`holds_a_summon_modifier`], and R2-14 for what claiming one costs.
    NoModifier,
    /// Anything else Windows said, carried verbatim.
    Refused(String),
}

impl HotkeyFault {
    /// Whether this is the refusal a *second* instance of this program gets, and
    /// therefore the one that is expected rather than reported.
    #[must_use]
    pub const fn is_already_registered(&self) -> bool {
        matches!(self, Self::AlreadyRegistered)
    }
}

/// **Whether this message is the hotkey we asked for.**
///
/// Pure, and its own function for `is_system_preference_message`'s reason: it is
/// the part of a message hook that can be wrong, and a hook is not a place a
/// test can reach.
///
/// Three facts and not one. `WM_HOTKEY` is a **thread** message — Windows posts
/// it with no window at all when the registration named none — so a message
/// carrying an `hwnd` is one of winit's own windows talking and must be passed
/// through untouched. The `wparam` is the id handed to `RegisterHotKey`, and
/// this process may one day hold more than one.
#[must_use]
pub fn is_our_hotkey(message: u32, hwnd: isize, wparam: usize, id: i32) -> bool {
    message == WM_HOTKEY && hwnd == 0 && wparam == id as usize
}

/// **Whether a message is one this process should act on** (R2-5).
///
/// [`is_our_hotkey`] answers the *shape* of a message, and a shape is all a
/// message has: `WM_HOTKEY` with a null window and a `wparam` of one is four
/// integers, and `PostThreadMessage` is a call any process running as this user
/// can make. So the shape is not the whole question. The other half is whether
/// **this** process is currently holding a claim under that id — because a
/// summon nobody registered is a summon Windows was never going to send, and a
/// message claiming otherwise is a message from somewhere else.
///
/// It closes the two states the shape alone could not see: the shortcut cleared
/// or turned off in the table, and a `RegisterHotKey` Windows refused because a
/// second copy of Folio, or another program entirely, got the chord first. In
/// both of those the window used to come down for a message it had no claim
/// behind.
#[must_use]
pub fn summon_should_act(
    message: u32,
    hwnd: isize,
    wparam: usize,
    id: i32,
    registration_is_live: bool,
) -> bool {
    registration_is_live && is_our_hotkey(message, hwnd, wparam, id)
}

/// **The ids this process holds a live `RegisterHotKey` claim under.**
///
/// A list rather than a flag because [`is_our_hotkey`]'s own note says this
/// process may one day hold more than one, and a `bool` would be the place that
/// stopped being true. It is written at exactly two moments — a `register` that
/// Windows accepted, and the `Drop` that releases it — so "is the claim live"
/// has one answer and it is the same one Windows has.
///
/// Kept out of the `#[cfg(windows)]` module on purpose: it is bookkeeping with
/// no Win32 in it, and it is the half of [`summon_should_act`] a test can drive.
static LIVE_CLAIMS: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());

fn claims() -> std::sync::MutexGuard<'static, Vec<i32>> {
    LIVE_CLAIMS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether this process holds a claim under `id` right now.
#[must_use]
pub fn registration_is_live(id: i32) -> bool {
    claims().contains(&id)
}

/// Record a claim the platform accepted.
///
/// **Ungated since M4-8**, where the second platform that claims a chord
/// arrived: the ledger is bookkeeping with no Win32 and no AppKit in it, both
/// registrations write it at the same two moments, and the macOS event handler
/// reads it for exactly the reason the Windows hook does — see
/// [`summon_should_act`]. A platform with no door at all does not have it —
/// [`registration_is_live`] then answers `false` for every id, which is the
/// truth, and a writer with no caller would be a warning rather than a promise.
#[cfg(any(windows, target_os = "macos", test))]
fn note_claimed(id: i32) {
    let mut live = claims();
    if !live.contains(&id) {
        live.push(id);
    }
}

/// Record a claim that has been released, or that the platform refused.
#[cfg(any(windows, target_os = "macos", test))]
fn note_released(id: i32) {
    claims().retain(|held| *held != id);
}

/// **What a summon does, said once for the whole process** (M4-8).
///
/// The two platforms deliver a press down roads that have nothing in common —
/// Windows posts a thread message into winit's own `PeekMessageW` pump, macOS
/// dispatches a Carbon event to a handler on the application event target — and
/// the one thing they must agree on is what happens at the end of the road.
/// This is that thing, and it is a `static` rather than a parameter because only
/// one of the two roads has a caller to hand it to: the Windows hook is
/// installed by `bt-app` on a builder, and the macOS handler is installed by
/// [`register`] inside this crate, where no closure of `bt-app`'s is in reach.
///
/// **Set once, before the loop is built**, which is also before any chord can be
/// claimed. A press that arrives before it is set is a press with nowhere to go,
/// and is dropped — the same reading `bt-app`'s own proxy has for the same
/// window of time.
static SUMMON_WAKE: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

/// Say what a press of the claimed chord does.
///
/// **`wake` must do nothing but wake the loop.** On Windows it runs inside
/// winit's own message pump, before anything has been decided about the turn; on
/// macOS it runs inside the application's event dispatch, in the same position.
/// Which way the window goes is decided on the turn that reads the bit —
/// `bt_app::FolioApp::settle_quake`.
pub fn summons_wake(wake: impl Fn() + Send + Sync + 'static) {
    // A second call is the caller changing its mind about a thing that is only
    // ever said at startup; the first answer is kept rather than raced against
    // a press that may already be in flight.
    let _ = SUMMON_WAKE.set(Box::new(wake));
}

/// Ring it, if anybody has said what it does.
///
/// Not `pub`: the two hooks in this module are its only callers, and a door that
/// let anything else ring it would be a door that summons the window without a
/// key having been pressed. A platform with no delivery road does not have it,
/// for the reason `note_claimed` is gated the same way.
#[cfg(any(windows, target_os = "macos"))]
fn wake_the_summon() {
    if let Some(wake) = SUMMON_WAKE.get() {
        wake();
    }
}

/// **`BT_HOTKEY_TRACE` — one named file, one line per station on the road a
/// press travels** (T-MAC-SUMMON-DIAG, `docs/BT-ENVIRONMENT.md`).
///
/// This module is the one surface in the product that could fail with **nothing
/// said anywhere**. A claim the system refused is a sentence on the settings
/// page; a claim the system accepted and then delivered to a handler that threw
/// the press away is a key that does nothing, and until this ticket there was no
/// build, no log line and no switch that could tell the two apart — which is
/// exactly the state the owner reported from and exactly the state the defect in
/// [`K_EVENT_PARAM_DIRECT_OBJECT`] put them in.
///
/// Same five properties as `bt_app::trace`, which wrote them down first and
/// whose machinery this cannot borrow because that crate is the binary and this
/// one is underneath it: the value is a **file** and not a folder, it is
/// appended rather than truncated, every line is flushed, a closure at every
/// call site means an unset gate formats no field, and set-but-empty is off.
///
/// **It ends at one file although it is written from two crates.** The stations
/// are five and they are the five places a press can stop —
/// `reconcile wanted=…` and [`TRACE_WAKE`]/[`TRACE_ANSWERED`] are `bt-app`'s,
/// [`trace_register`] and [`trace_handler`] are this module's — and a reader
/// asking "how far did my press get" needs them interleaved in one order, not
/// spread over two logs that agree about nothing.
struct SummonTrace {
    file: std::sync::Mutex<std::fs::File>,
    /// [`Instant`](std::time::Instant) rather than a wall clock, for
    /// `bt_app::trace`'s reason: what a reader of this file needs is the
    /// *distance* between two stations of one press.
    started: std::time::Instant,
}

/// Named in the header so a file holding two runs, or two traces, stays
/// readable by whoever opens it.
const TRACE_HEADER: &str = "# BT_HOTKEY_TRACE_V1 elapsed_ms station field=value";

impl SummonTrace {
    fn opened() -> Option<Self> {
        use std::io::Write as _;

        // Set-but-empty is off, which is this product's rule for every one of
        // these: `BT_HOTKEY_TRACE=` is a shell saying "not this run", and a run
        // that answered it with a file named the empty string would fail in a
        // way that looks like the feature is broken.
        let path = std::env::var_os("BT_HOTKEY_TRACE").filter(|value| !value.is_empty())?;
        let path = std::path::PathBuf::from(path);
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut file) => {
                let _ = writeln!(file, "{TRACE_HEADER}");
                let _ = file.flush();
                Some(Self {
                    file: std::sync::Mutex::new(file),
                    started: std::time::Instant::now(),
                })
            }
            // Said out loud and then dropped: a trace that could not be opened
            // is a diagnostic that will not run, which the person who asked for
            // it has to be told — and is not a reason for the terminal to refuse
            // to start.
            Err(error) => {
                eprintln!(
                    "the hotkey trace names {} but it could not be opened: {error}",
                    path.display()
                );
                None
            }
        }
    }

    fn write(&self, message: &str) {
        use std::io::Write as _;

        let elapsed = self.started.elapsed().as_secs_f64() * 1000.0;
        // A poisoned lock means another thread panicked mid-line. The bytes are
        // still a file and this line is still worth having: a diagnostic must
        // not be the thing that turns one panic into two.
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(file, "{elapsed:9.3} {message}");
        let _ = file.flush();
    }
}

/// The process's trace, opened at the first station any road reaches.
static SUMMON_TRACE: std::sync::OnceLock<Option<SummonTrace>> = std::sync::OnceLock::new();

/// **Write one station**, formatting nothing at all when the variable is unset.
///
/// `pub` because three of the five stations are `bt-app`'s — see
/// [`SummonTrace`] for why they end at one file — and because the handler this
/// module installs is reached from a road no caller of that crate can see.
pub fn trace(message: impl FnOnce() -> String) {
    if let Some(trace) = SUMMON_TRACE.get_or_init(SummonTrace::opened) {
        trace.write(&message());
    }
}

/// The station `bt-app`'s wake closure writes: the press has left this crate.
///
/// A constant rather than a literal at the call site, for the reason the line
/// formatters below are functions: a station whose exact bytes a test pins is a
/// station a reader can grep for, and the test and the caller must be reading
/// one spelling.
pub const TRACE_WAKE: &str = "summons_wake called";

/// The station `bt-app` writes when the loop's own turn has taken the press —
/// the far end of the road, and the line whose absence says the proxy, not the
/// keyboard, is where the press stopped.
pub const TRACE_ANSWERED: &str = "QuakeSummoned handled";

/// `register keycode=0x32 modifiers=0x1000 -> Ok`, or the `OSStatus` that
/// refused it.
///
/// Pure, and compiled on every platform, for [`carbon_key_code`]'s reason: the
/// host that writes this ticket has no Carbon and must still be able to assert
/// what the host that runs it will write down.
#[must_use]
pub fn trace_register(key_code: u32, modifiers: u32, status: i32) -> String {
    let outcome = if status == 0 {
        "Ok".to_owned()
    } else {
        format!("Err({status})")
    };
    format!("register keycode={key_code:#x} modifiers={modifiers:#x} -> {outcome}")
}

/// `register -> Err(NoSuchKey)` — the refusals this product makes before Carbon
/// is asked anything, which carry no `OSStatus` because no call was made.
#[must_use]
pub fn trace_register_refused(fault: &HotkeyFault) -> String {
    let named = match fault {
        HotkeyFault::AlreadyRegistered => "AlreadyRegistered".to_owned(),
        HotkeyFault::NoSuchKey => "NoSuchKey".to_owned(),
        HotkeyFault::NoModifier => "NoModifier".to_owned(),
        HotkeyFault::Refused(why) => format!("Refused({why})"),
    };
    format!("register -> Err({named})")
}

/// `install handler -> Ok`, or the `OSStatus` that refused it.
///
/// Its own station because a handler that was never installed and a handler that
/// was never *called* are two different faults with one symptom, and the order
/// of the lines in the file is what tells them apart.
#[must_use]
pub fn trace_install(status: i32) -> String {
    if status == 0 {
        "install handler -> Ok".to_owned()
    } else {
        format!("install handler -> Err({status})")
    }
}

/// `handler fired id=1 signature=folo live=yes` — a hot key event reached this
/// process, said with both halves of the gate [`summon_should_act`] describes.
///
/// Every event is written, including the ones the gate refuses, because "a
/// framework in this address space has the chord" and "the press never arrived"
/// are the two readings a silent file would leave open.
#[must_use]
pub fn trace_handler(signature: u32, id: u32, live: bool) -> String {
    let signature = four_character_code(signature);
    let live = if live { "yes" } else { "no" };
    format!("handler fired id={id} signature={signature} live={live}")
}

/// `handler fired id=none GetEventParameter=Err(-9870)` — **the line this
/// ticket exists for.**
///
/// The handler ran, the event was a hot key press, and the parameter naming
/// *which* claim fired could not be read out of it. That is the whole of the
/// defect [`K_EVENT_PARAM_DIRECT_OBJECT`] was, and a build carrying this station
/// says it in one line instead of in a key that does nothing.
#[must_use]
pub fn trace_handler_lost(status: i32) -> String {
    format!("handler fired id=none GetEventParameter=Err({status})")
}

/// **A four-character code as its four characters**, which is the only form a
/// reader can compare against a header.
///
/// A byte outside printable ASCII is written as `.`: these are `OSType`s and a
/// framework may well hold one that is not text, and a trace line is not a place
/// to put a raw byte into somebody's terminal.
#[must_use]
pub fn four_character_code(value: u32) -> String {
    value
        .to_be_bytes()
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            }
        })
        .collect()
}

/// **Whoever had the keyboard before the summon came down** (M4-8).
///
/// A type of its own, and the reason is that the two platforms do not answer
/// the same question. Windows hands the keyboard to a **window**, and handing it
/// back is `SetForegroundWindow` on that window. macOS hands it to an
/// **application** — `NSWorkspace.frontmostApplication` — and handing it back is
/// `-[NSRunningApplication activate…]`; which of that application's windows then
/// has the keyboard is its own business and never was ours.
///
/// Until this ticket the answer was a [`NativeWindow`], which is the Windows
/// shape wearing a cross-platform name. Two things go wrong with reusing it: a
/// `NativeWindow` means *a window of this process* everywhere else in this
/// crate — [`give_foreground_to`] is called with one to raise a window of our
/// own — and a process id stuffed into that type would be a number the very next
/// caller would pass to AppKit as a view pointer.
///
/// Opaque on purpose: `bt-app` reads nothing out of it. It remembers one, and
/// gives it back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Foreground {
    /// The window handle on Windows, the process id on macOS. Private, and the
    /// reason this type exists.
    holder: std::num::NonZeroIsize,
}

impl Foreground {
    /// **A holder that names nobody**, for a test that needs two values it can
    /// tell apart — [`NativeWindow::stand_in`]'s twin, and every word of its
    /// note applies here. `bt-app`'s quake suite decides *which* holder the
    /// keyboard goes back to, and none of it touches the machine.
    #[doc(hidden)]
    #[must_use]
    pub const fn stand_in(tag: u16) -> Self {
        let holder = 0x0bad_0000_isize + (tag as isize) + 1;
        match std::num::NonZeroIsize::new(holder) {
            Some(holder) => Self { holder },
            // `0x0bad_0000 + tag + 1` is positive for every `u16`.
            None => unreachable!(),
        }
    }

    /// **Whether the thing that had the keyboard is this very window.**
    ///
    /// Asked by the summon, which must not record the window it is about to hide
    /// as the window it owes the keyboard to.
    ///
    /// On Windows it is handle equality. On macOS it is always `false`, and that
    /// is an answer rather than a stub: what is remembered there is an
    /// application, never a window, and the case this guard exists for —
    /// *we already had the keyboard* — is refused one step earlier, by
    /// [`foreground_holder`] answering `None` when the frontmost application is
    /// this one.
    #[cfg(windows)]
    #[must_use]
    pub fn is_window(self, window: NativeWindow) -> bool {
        NativeWindow::from_win32(self.holder) == window
    }

    /// Off Windows a holder is never a window — see the note above.
    #[cfg(not(windows))]
    #[must_use]
    pub fn is_window(self, window: NativeWindow) -> bool {
        let _ = (self, window);
        false
    }
}

#[cfg(windows)]
pub use windows_hotkey::{
    GlobalHotkey, allow_foreground_for, foreground_holder, give_foreground_to, hand_back_to,
    register, summon_message_hook,
};

#[cfg(windows)]
mod windows_hotkey {
    use std::ffi::c_void;
    use std::marker::PhantomData;
    use std::time::Instant;

    use windows::Win32::Foundation::ERROR_HOTKEY_ALREADY_REGISTERED;
    // `AttachThreadInput` is filed under `Threading` and not under
    // `KeyboardAndMouse` beside the three below it, which reads oddly until you
    // remember what it does: it joins two *threads'* input queues, and the
    // keyboard is only the thing that arrives on them.
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        IsHungAppWindow, IsWindow, MSG, SetForegroundWindow,
    };

    use super::{Foreground, Hotkey, HotkeyFault, registration_bits};
    use crate::NativeWindow;

    /// **A claim on a chord, held for as long as this value is alive.**
    ///
    /// The registration is a fact about a *thread*, not about a process or a
    /// window: `RegisterHotKey` with no window posts `WM_HOTKEY` to the queue of
    /// whichever thread called it, and `UnregisterHotKey` may only be called by
    /// that same thread. So this is deliberately neither `Send` nor `Sync` — a
    /// claim that travelled to another thread could not be released by the
    /// thread holding it, and the chord would stay taken for the life of the
    /// process.
    #[derive(Debug)]
    pub struct GlobalHotkey {
        id: i32,
        /// What makes the type thread-bound; it holds no pointer.
        _thread_bound: PhantomData<*const ()>,
    }

    impl GlobalHotkey {
        /// The id this claim was made under — the `wparam` its `WM_HOTKEY`
        /// carries, and what [`super::is_our_hotkey`] is asked about.
        #[must_use]
        pub const fn id(&self) -> i32 {
            self.id
        }
    }

    impl Drop for GlobalHotkey {
        fn drop(&mut self) {
            // SAFETY: `GlobalHotkey` is not `Send`, so this runs on the thread
            // that registered — `UnregisterHotKey`'s own requirement — and the id
            // was accepted by `RegisterHotKey` on that thread.
            //
            // Best-effort: a claim Windows has already dropped (a session change,
            // an unregister from a debugger) answers an error, and there is
            // nothing left to do about it while a value is being destroyed.
            let _ = unsafe { UnregisterHotKey(None, self.id) };
            // **And the ledger goes down with the claim** (R2-5). From here a
            // `WM_HOTKEY` carrying this id is a message nothing of ours asked
            // for, and `summon_should_act` says so.
            super::note_released(self.id);
        }
    }

    /// **Claim a chord for this thread**, or say why it could not be claimed.
    ///
    /// `None` for the window, which is what makes the message a thread message
    /// and what makes this survivable at all: the quake window is hidden for most
    /// of its life and *destroyed* when the reader closes it, and a registration
    /// hung off its `HWND` would die with it — leaving the key that summons the
    /// window in the hands of the window it was supposed to summon.
    pub fn register(id: i32, hotkey: Hotkey) -> Result<GlobalHotkey, HotkeyFault> {
        // **Refused before Windows is asked** (R2-14). The two are told apart
        // because their remedies are: a key this layout has no code for wants a
        // different key, and a chord with no modifier on it wants a modifier.
        if !super::holds_a_summon_modifier(hotkey) {
            return Err(HotkeyFault::NoModifier);
        }
        let Some((modifiers, virtual_key)) = registration_bits(hotkey) else {
            return Err(HotkeyFault::NoSuchKey);
        };
        // SAFETY: the two integers come from `registration_bits`, which refuses a
        // zero key; no window handle is passed, so nothing here can outlive an
        // `HWND`. The claim is released by `GlobalHotkey::drop` on this thread.
        match unsafe { RegisterHotKey(None, id, HOT_KEY_MODIFIERS(modifiers), virtual_key) } {
            Ok(()) => {
                // **The ledger goes up with the claim and not before** (R2-5):
                // it is what `summon_should_act` reads to tell a message Windows
                // sent from one another process posted.
                super::note_claimed(id);
                Ok(GlobalHotkey {
                    id,
                    _thread_bound: PhantomData,
                })
            }
            Err(error) if error.code() == ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() => {
                super::note_released(id);
                Err(HotkeyFault::AlreadyRegistered)
            }
            Err(error) => {
                super::note_released(id);
                Err(HotkeyFault::Refused(format!("RegisterHotKey: {error}")))
            }
        }
    }

    /// **The message hook winit's `with_msg_hook` takes**, already knowing how to
    /// recognise our hotkey and how little to do about it.
    ///
    /// The whole closure and not a predicate the caller writes the `unsafe`
    /// around, because `bt-app` is under the workspace's `unsafe_code = "deny"`
    /// and reading a raw `MSG` is precisely the sort of thing this crate exists
    /// to do on its behalf — the layout of a `MSG` is a Win32 fact, and Win32
    /// facts live on this side of the boundary.
    ///
    /// The pointer's validity is winit's contract: it documents the callback as
    /// receiving a live `*const MSG` for the length of the call, and this keeps
    /// nothing beyond it. The null check is not that contract being doubted; it
    /// is the one failure mode a `*const` can have that costs a comparison to
    /// rule out.
    ///
    /// **What the press does is [`super::summons_wake`]'s and no longer this
    /// function's parameter** (M4-8). It used to be a closure handed in here,
    /// which was right while one platform had a door; the second platform's door
    /// is installed inside this crate, where no closure of `bt-app`'s is in
    /// reach, so the statement moved to the one place both roads end at. What is
    /// left here is the half that really is Windows': which message is ours.
    ///
    /// **The wake must do nothing but wake the loop.** This runs inside winit's
    /// own `PeekMessageW` dispatch, before anything has been decided about the
    /// turn; it is `SystemSettingsWatch`'s discipline at a second door and for a
    /// stronger version of its reason.
    ///
    /// **Always `false`**, which is winit's word for "dispatch this normally".
    /// Two different reasons agree on it: a message that is not ours is winit's
    /// to dispatch, and a `WM_HOTKEY` that *is* ours carries no window, so there
    /// is no window procedure for a dispatch to reach and letting it through
    /// costs nothing.
    pub fn summon_message_hook(id: i32) -> impl FnMut(*const c_void) -> bool {
        move |message: *const c_void| {
            if message.is_null() {
                return false;
            }
            // SAFETY: winit documents this pointer as a live `*const MSG` for the
            // duration of the call. The three fields are read by value and the
            // reference does not outlive the statement.
            let message = unsafe { &*message.cast::<MSG>() };
            // **Shape and claim, not shape alone** (R2-5). See
            // [`super::summon_should_act`]: a `WM_HOTKEY` is four integers any
            // process of this user can post with `PostThreadMessage`, and the
            // only thing that separates one Windows sent from one somebody else
            // did is whether this process is holding a claim under that id.
            if super::summon_should_act(
                message.message,
                message.hwnd.0 as isize,
                message.wParam.0,
                id,
                super::registration_is_live(id),
            ) {
                super::wake_the_summon();
            }
            false
        }
    }

    /// Whoever has the keyboard right now, or `None` when no window does.
    ///
    /// Asked **before** the summoned window is shown and kept until it is
    /// dismissed: it is the whole of what "give it back" means, and there is no
    /// second chance to read it — by the time the quake window is going away, the
    /// foreground is the quake window.
    #[must_use]
    pub fn foreground_window() -> Option<NativeWindow> {
        // SAFETY: a read with no arguments and no lifetime; the handle is
        // immediately narrowed to an integer and never dereferenced.
        let hwnd = unsafe { GetForegroundWindow() };
        NativeWindow::from_hwnd(hwnd)
    }

    /// [`foreground_window`], in the currency `bt-app` remembers (M4-8).
    ///
    /// On this platform the thing that has the keyboard is a window, so the two
    /// readings are the same read; on macOS they are not, which is why the
    /// caller's door is the one that speaks [`Foreground`].
    #[must_use]
    pub fn foreground_holder() -> Option<Foreground> {
        foreground_window().map(|window| Foreground {
            holder: window.as_handle(),
        })
    }

    /// **Give the keyboard back to whoever had it before the summon.**
    ///
    /// The same call as [`give_foreground_to`] on this platform and deliberately
    /// a different name, because it is a different sentence: that one is *bring
    /// a window of ours to the front*, and this one is *let somebody else have
    /// the front back*. On macOS they are not even the same API.
    pub fn hand_back_to(holder: Foreground) -> bool {
        give_foreground_to(NativeWindow::from_win32(holder.holder))
    }

    /// **Hand this process's foreground rights to another process** (`docs/DESIGN.md` §7.59).
    ///
    /// The other half of [`give_foreground_to`], seen from the side that *has*
    /// the keyboard: that function is a window trying to come to the front, and
    /// Windows refuses it unless the process that owns the foreground has said
    /// otherwise first. This is that sentence. A second `folio.exe` started from
    /// Explorer, a shortcut or a pinned icon holds foreground rights because the
    /// user just started it, and it spends them here — on the Folio that is
    /// already running — before it exits.
    ///
    /// **Not `ASFW_ANY`, and the refusal is in the code and not only in this
    /// sentence** (review C-7, 2026-09-11). `ASFW_ANY` is the same call with
    /// `(DWORD)-1` and grants the right to whatever asks next — this program
    /// lifting the foreground lock for the whole machine. The paragraph above
    /// used to be the only thing standing between that value and this call, and
    /// the pid came off a wire, out of a field a peer filled in. It now comes
    /// from the kernel ([`crate::launch_pipe::hand_over`]) **and** `u32::MAX` and
    /// `0` are refused here, because a rule stated at one door is a rule until
    /// somebody opens a second one.
    ///
    /// The answer is read back and handed to the caller for [`give_foreground_to`]'s
    /// reason: it fails by answering `false` rather than by raising, and a caller
    /// that did not look would report a handover that never happened. Failure is
    /// never reported to a reader — there is nothing a person can do about a
    /// foreground lock, and the worst it costs is a window that opens behind
    /// another one.
    pub fn allow_foreground_for(process: u32) -> bool {
        // `u32::MAX` is `ASFW_ANY` and `0` is `ASFW_NONE` — the two values that
        // are not a process, and the two this call must never be asked with.
        if process == 0 || process == u32::MAX {
            return false;
        }
        // SAFETY: a call taking one integer; it names a process id and
        // dereferences nothing. A process id that has gone is a legal argument
        // and answers `false`.
        unsafe { AllowSetForegroundWindow(process) }.is_ok()
    }

    /// **Put this window back in front**, and say whether it actually got there.
    ///
    /// Windows refuses a bare `SetForegroundWindow` from a process that does not
    /// already own the foreground, and it refuses by answering `false` rather
    /// than by raising — so a caller that did not check would believe it had
    /// handed the keyboard back while the reader was still typing into a window
    /// that is no longer on the screen. Joining the foreground thread's input
    /// queue for the length of the call is the documented way round the lock, and
    /// the result is **read back** rather than assumed.
    ///
    /// The retry is the shape `scripts/release/smoke.ps1` and
    /// `scripts/dev/ui-probe.ps1` have used against real windows since August:
    /// the transition is not instantaneous, and a single attempt loses to a
    /// window still finishing an animation of its own.
    ///
    /// **Failure is silent to the reader and reported to the caller.** There is
    /// nothing a person can do about a foreground lock, and a card appearing over
    /// their editor to say the terminal could not give the keyboard back would be
    /// a worse interruption than the one it was reporting.
    pub fn give_foreground_to(window: NativeWindow) -> bool {
        let target = window.as_hwnd();
        // **The handle is revalidated before it is used** (R2-3). It was read at
        // the moment the summon came down, and between then and now the window it
        // named may have closed — an `HWND` is reused by Windows the moment a
        // window is destroyed, so a stale one does not fail, it names somebody
        // else's window.
        //
        // SAFETY: a read of a handle that is only ever compared and passed back
        // to Win32; `IsWindow` is defined on a handle that is no longer one.
        if !unsafe { IsWindow(Some(target)) }.as_bool() {
            return false;
        }
        let began = Instant::now();
        for attempt in 0..FOREGROUND_ATTEMPTS {
            if !super::another_round(attempt, began.elapsed()) {
                return false;
            }
            // SAFETY: a read with no arguments; the handle is only compared.
            if unsafe { GetForegroundWindow() } == target {
                return true;
            }
            // SAFETY: `GetForegroundWindow` may answer null, which
            // `GetWindowThreadProcessId` accepts and reports as thread 0 — the
            // "nobody has it" case the rule below declines to attach to. The
            // process-id out-parameter is deliberately `None`, which the API
            // documents as "do not report it".
            let foreground = unsafe { GetForegroundWindow() };
            let theirs = unsafe { GetWindowThreadProcessId(foreground, None) };
            // SAFETY: no arguments, no handle.
            let mine = unsafe { GetCurrentThreadId() };
            // **The one question that has to be asked before the queues are
            // joined** (R2-3). `AttachThreadInput` makes two threads share one
            // input queue, and a queue is only as responsive as the slower of the
            // two: attaching to an application that has stopped reading its
            // messages hands Folio that application's paralysis for as long as
            // the attachment lasts. `IsHungAppWindow` is Windows' own answer to
            // "has this window stopped answering", the same one the shell reads
            // before it draws the ghost frame.
            //
            // SAFETY: a read of a handle Win32 just gave us; null is a legal
            // argument and answers false.
            let hung = !foreground.is_invalid() && unsafe { IsHungAppWindow(foreground) }.as_bool();
            let step = super::handover_step(theirs, mine, hung);
            let attached = step == super::HandoverStep::AttachAndActivate
                // SAFETY: attaching two live thread input queues; detached below
                // on every path out of this iteration.
                && unsafe { AttachThreadInput(mine, theirs, true) }.as_bool();
            // SAFETY: `target` is the caller's live top-level window.
            let _ = unsafe { BringWindowToTop(target) };
            // SAFETY: same handle; the boolean answer is deliberately ignored in
            // favour of reading the foreground back below, which is the only
            // report that cannot be wrong.
            let _ = unsafe { SetForegroundWindow(target) };
            if attached {
                // SAFETY: undoing exactly the attachment made above, with the
                // same two thread ids.
                let _ = unsafe { AttachThreadInput(mine, theirs, false) };
            }
            // SAFETY: a read with no arguments.
            if unsafe { GetForegroundWindow() } == target {
                return true;
            }
        }
        false
    }

    /// How many times the handover is attempted before it is given up on.
    ///
    /// Five, which is `smoke.ps1`'s number, and no sleep between them: the
    /// scripts wait 400ms because they are photographing a window and a
    /// half-finished transition would be in the picture. This runs **on the event
    /// loop's own thread**, where four hundred milliseconds of sleep is four
    /// hundred milliseconds in which this program answers no keystroke, no
    /// present and no shell — a cure considerably worse than a foreground that
    /// went somewhere else.
    const FOREGROUND_ATTEMPTS: usize = 5;
}

/// **The second bound on the handover, and it is in time** (R2-3).
///
/// A count of attempts bounds the loop only if every attempt is quick, and the
/// calls inside one are not guaranteed to be: `SetForegroundWindow` and
/// `BringWindowToTop` both talk to whichever window station and desktop thread is
/// on the other end. Five attempts that each take a second is five seconds in
/// which this program answers no keystroke, no present and no shell — the exact
/// cost the "no sleep between them" note was protecting against, arriving by a
/// different door.
///
/// Pure, so that "the loop cannot run past its budget" is a claim a test makes
/// with numbers rather than a claim about a machine with a wedged window on it.
#[must_use]
pub fn another_round(attempt: usize, elapsed: std::time::Duration) -> bool {
    attempt == 0 || elapsed < FOREGROUND_BUDGET
}

/// The whole of what the handover may spend on somebody else's window.
///
/// A quarter of the frame budget of a 4 Hz redraw, and far more than the
/// transition takes on a machine that is answering at all: on this one the
/// handover completes on the first or second attempt inside a millisecond. What
/// it bounds is the machine that is not answering.
pub const FOREGROUND_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

/// **What one round of the handover may do**, from the facts about the two
/// threads — see [`handover_step`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoverStep {
    /// Ask for the foreground without joining anybody's input queue.
    ///
    /// Either there is nobody to join — no foreground window, or it is already
    /// this thread's — or the foreground belongs to an application that has
    /// stopped answering, and joining *that* is how one program's hang becomes
    /// two. The bare `SetForegroundWindow` will very likely be refused, which is
    /// a summon that did not take the keyboard; the alternative was a window that
    /// stopped drawing.
    ActivateAlone,
    /// Join the foreground thread's queue for the length of the call, then ask.
    ///
    /// The documented way past the foreground lock, and it is safe precisely when
    /// the other thread is reading its own messages.
    AttachAndActivate,
}

/// The rule [`give_foreground_to`] applies once per round, with the Win32 taken
/// out of it (R2-3).
#[must_use]
pub const fn handover_step(
    foreground_thread: u32,
    our_thread: u32,
    foreground_is_hung: bool,
) -> HandoverStep {
    if foreground_thread == 0 || foreground_thread == our_thread || foreground_is_hung {
        HandoverStep::ActivateAlone
    } else {
        HandoverStep::AttachAndActivate
    }
}

/// **Let the process we are handing a launch to come to the front.**
///
/// A no-op answering `false`, and one of §4.4's class-N items rather than
/// deferred work: `AllowSetForegroundWindow` exists because Windows has a
/// foreground *lock* to ask permission from, and macOS has none — the launch
/// handover simply activates the other application. The `false` says no
/// permission was granted, which is true, and the caller's own next step is the
/// activation that needs none.
#[cfg(not(windows))]
#[must_use]
pub fn allow_foreground_for(process: u32) -> bool {
    let _ = process;
    false
}

#[cfg(target_os = "macos")]
pub use macos_hotkey::{
    GlobalHotkey, foreground_holder, give_foreground_to, hand_back_to, register,
};

/// **The summon on a Mac: a Carbon hot key, and no permission asked for**
/// (M4-8, `docs/DESIGN.md` §13.51).
///
/// The eighth unsafe boundary in this crate and against an eighth thing: this is
/// **the keyboard while another application has it**, which on this platform is
/// a question with three possible answers and only one of them is free.
///
/// `CGEventTap` sees every key on the machine and needs *Input Monitoring*;
/// `NSEvent.addGlobalMonitorForEvents` needs *Accessibility* and cannot take the
/// key out of the stream, so the chord would also reach whatever the reader was
/// typing into. Both are TCC prompts, and this port's rule is that Folio asks
/// for no TCC grant it can do without. `RegisterEventHotKey` is the third:
/// system-wide, delivered whether or not this application is frontmost,
/// swallowed so nobody else sees it, and asking nobody for anything. It is
/// Carbon, which is thirty years old and has no binding in the `objc2` family —
/// so its five entry points are declared here, the way `macos_watch` declares
/// FSEvents' seven, and for the same reason: a framework with no crate is what
/// this crate is for.
#[cfg(target_os = "macos")]
mod macos_hotkey {
    use std::ffi::c_void;
    use std::marker::PhantomData;
    use std::ptr;
    use std::sync::OnceLock;

    use objc2_app_kit::{
        NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWorkspace,
    };

    use super::{
        EVENT_HOT_KEY_EXISTS_ERR, Foreground, Hotkey, HotkeyFault, K_EVENT_CLASS_KEYBOARD,
        K_EVENT_HOT_KEY_PRESSED, K_EVENT_PARAM_DIRECT_OBJECT, SUMMON_SIGNATURE,
        TYPE_EVENT_HOT_KEY_ID, carbon_registration_bits,
    };
    use crate::NativeWindow;

    // ── Carbon, declared by hand ───────────────────────────────────────────

    /// `EventTypeSpec` — the class and kind of event a handler is offered.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    /// `EventHotKeyID` — what the event carries to say *which* claim fired.
    ///
    /// Both fields are read back in the handler rather than only the id: the
    /// signature is what separates this process's claims from any other
    /// `RegisterEventHotKey` in the address space, and there is one in every
    /// framework that has ever wanted a shortcut.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    type EventRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;
    type EventTargetRef = *mut c_void;
    type EventHandlerRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type EventHandlerProc = unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> i32;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(
            target: EventTargetRef,
            handler: EventHandlerProc,
            number_of_types: usize,
            list: *const EventTypeSpec,
            user_data: *mut c_void,
            installed: *mut EventHandlerRef,
        ) -> i32;
        fn RegisterEventHotKey(
            key_code: u32,
            modifiers: u32,
            id: EventHotKeyID,
            target: EventTargetRef,
            options: u32,
            claimed: *mut EventHotKeyRef,
        ) -> i32;
        fn UnregisterEventHotKey(claimed: EventHotKeyRef) -> i32;
        fn GetEventParameter(
            event: EventRef,
            name: u32,
            parameter_type: u32,
            actual_type: *mut u32,
            buffer_size: usize,
            actual_size: *mut usize,
            data: *mut c_void,
        ) -> i32;
    }

    /// `noErr`, which is zero here as it is everywhere else in this framework.
    ///
    /// The one Carbon number still written inside this module, because it is the
    /// one no keyboard and no header could make wrong — see
    /// [`super::K_EVENT_PARAM_DIRECT_OBJECT`] for why the other six moved out.
    const NO_ERR: i32 = 0;

    /// **A claim on a chord, held for as long as this value is alive.**
    ///
    /// Neither `Send` nor `Sync`, for the Windows arm's reason wearing macOS
    /// clothes: `RegisterEventHotKey` against the *application* event target is
    /// a statement about the main run loop, and the handler that answers it runs
    /// on the main thread. A claim that travelled to another thread could be
    /// dropped there, and `UnregisterEventHotKey` from off the main thread is a
    /// Carbon call outside the loop that owns it.
    #[derive(Debug)]
    pub struct GlobalHotkey {
        id: i32,
        /// What Carbon gave us, and the only thing that can release the claim.
        claimed: EventHotKeyRef,
        /// What makes the type thread-bound; it holds no pointer of its own.
        _thread_bound: PhantomData<*const ()>,
    }

    impl GlobalHotkey {
        /// The id this claim was made under — the `EventHotKeyID.id` its event
        /// carries.
        #[must_use]
        pub const fn id(&self) -> i32 {
            self.id
        }
    }

    impl Drop for GlobalHotkey {
        fn drop(&mut self) {
            // SAFETY: `GlobalHotkey` is not `Send`, so this runs on the thread
            // that registered — the main thread — and `claimed` is the reference
            // `RegisterEventHotKey` answered with and nothing else has touched.
            //
            // Best-effort: a claim the window server has already dropped answers
            // an error, and there is nothing left to do about it while a value
            // is being destroyed.
            let _ = unsafe { UnregisterEventHotKey(self.claimed) };
            // **And the ledger goes down with the claim** (R2-5), exactly as on
            // the other side: from here a hot key event carrying this id is an
            // event nothing of ours asked for.
            super::note_released(self.id);
        }
    }

    /// **The handler, and it is the whole of the delivery road.**
    ///
    /// Carbon calls this on the main thread, from inside the application's own
    /// event dispatch — the same position in the turn winit's `PeekMessageW`
    /// hook occupies on Windows, which is why the two can end at one
    /// [`super::wake_the_summon`].
    ///
    /// **Two facts and not one**, the macOS spelling of
    /// [`super::summon_should_act`]: the event must carry *this process's*
    /// signature, and this process must be holding a live claim under the id it
    /// names. The second is not paranoia about a forged event — there is no
    /// `PostThreadMessage` here — it is the same two states the Windows arm
    /// closes: a shortcut cleared in the table, and a registration that was
    /// refused. A `GlobalHotkey` that has been dropped has also been
    /// unregistered, so in practice Carbon stops calling; the ledger is what
    /// makes that a fact this code knows rather than one it assumes.
    ///
    /// **`noErr`**, which is Carbon's word for "handled": the press is ours and
    /// stops here. Answering `eventNotHandledErr` would pass a chord this
    /// application claimed on to the rest of the responder chain.
    unsafe extern "C" fn summon_handler(
        _call: EventHandlerCallRef,
        event: EventRef,
        _user_data: *mut c_void,
    ) -> i32 {
        let mut named = EventHotKeyID {
            signature: 0,
            id: 0,
        };
        // SAFETY: `event` is live for the length of this call — Carbon's own
        // contract for a handler — and the out-parameter is a local of exactly
        // the size handed to the call. The two `null_mut`s are documented as
        // "do not report the actual type / size".
        let status = unsafe {
            GetEventParameter(
                event,
                K_EVENT_PARAM_DIRECT_OBJECT,
                TYPE_EVENT_HOT_KEY_ID,
                ptr::null_mut(),
                size_of::<EventHotKeyID>(),
                ptr::null_mut(),
                (&raw mut named).cast::<c_void>(),
            )
        };
        if status != NO_ERR {
            // **The line this ticket exists for** — see
            // [`super::trace_handler_lost`]. The handler ran and the event did
            // not carry the parameter naming which claim fired, which is a fault
            // in this file rather than anywhere near the keyboard.
            super::trace(|| super::trace_handler_lost(status));
            return NO_ERR;
        }
        let live = super::registration_is_live(named.id as i32);
        super::trace(|| super::trace_handler(named.signature, named.id, live));
        if named.signature == SUMMON_SIGNATURE && live {
            super::wake_the_summon();
        }
        NO_ERR
    }

    /// **The handler is installed once for the life of the process**, and the
    /// `OnceLock` is the whole of the reason.
    ///
    /// A chord moves — the recorder, a hand-edited `keybindings.json`, *Restore
    /// all defaults* — and `bt_app::FolioApp::settle_quake` reconciles the claim
    /// every turn, so `register` is called again every time it does. The claim
    /// is what moves; the handler is not. Installing one per registration would
    /// leave a handler behind at each move and wake the loop once per handler
    /// for one press.
    ///
    /// `RemoveEventHandler` is deliberately never called: this is installed on
    /// the **application** event target, which outlives every window and every
    /// claim, and the process's exit is the only moment it stops being wanted.
    fn install_the_handler() -> Result<(), HotkeyFault> {
        static INSTALLED: OnceLock<i32> = OnceLock::new();
        let status = *INSTALLED.get_or_init(|| {
            let wanted = EventTypeSpec {
                event_class: K_EVENT_CLASS_KEYBOARD,
                event_kind: K_EVENT_HOT_KEY_PRESSED,
            };
            let mut installed: EventHandlerRef = ptr::null_mut();
            // SAFETY: `GetApplicationEventTarget` takes nothing and answers a
            // target that lives as long as the application. The handler is a
            // `extern "C"` function of this module with Carbon's own signature;
            // the list is one live local read for the length of the call; no
            // user data is passed, so the handler dereferences none. The
            // reference is written into a local this function then drops on
            // purpose — see the note above on `RemoveEventHandler`.
            unsafe {
                InstallEventHandler(
                    GetApplicationEventTarget(),
                    summon_handler,
                    1,
                    &raw const wanted,
                    ptr::null_mut(),
                    &raw mut installed,
                )
            }
        });
        // **Inside the `OnceLock`'s answer and not inside its initialiser**, so
        // that a reader who set the variable after the first `register` still
        // sees the handler's state on the line above their press rather than
        // only in the run that installed it.
        super::trace(|| super::trace_install(status));
        if status == NO_ERR {
            Ok(())
        } else {
            Err(HotkeyFault::Refused(format!(
                "InstallEventHandler: OSStatus {status}"
            )))
        }
    }

    /// **Claim a chord for this application**, or say why it could not be
    /// claimed.
    ///
    /// The application event target and not a window's, which is the same
    /// decision the Windows arm makes by passing no `HWND`: the summoned window
    /// is hidden for most of its life and destroyed when the reader closes it,
    /// and a claim hung off it would die with it — leaving the key that summons
    /// the window in the hands of the window it was supposed to summon.
    ///
    /// **No options.** `kEventHotKeyExclusive` asks the system to refuse the
    /// chord to everybody else afterwards, which is a claim on other people's
    /// programs rather than on a key, and this product's own note on
    /// `RegisterHotKey` — "a default value is a starting point, not an
    /// occupation" — reads the same here.
    pub fn register(id: i32, hotkey: Hotkey) -> Result<GlobalHotkey, HotkeyFault> {
        // **The product's own refusal first, exactly as the Windows arm orders
        // them** (R2-14): a chord with no modifier on it is refused for a reason
        // that is true on every platform, and the two are told apart because
        // their remedies are.
        if !super::holds_a_summon_modifier(hotkey) {
            super::trace(|| super::trace_register_refused(&HotkeyFault::NoModifier));
            return Err(HotkeyFault::NoModifier);
        }
        let Some((modifiers, key_code)) = carbon_registration_bits(hotkey) else {
            super::trace(|| super::trace_register_refused(&HotkeyFault::NoSuchKey));
            return Err(HotkeyFault::NoSuchKey);
        };
        install_the_handler()?;
        let mut claimed: EventHotKeyRef = ptr::null_mut();
        // SAFETY: the two integers come from `carbon_registration_bits`; the id
        // is a plain struct passed by value; the target is the application's own
        // and outlives the claim; the out-parameter is a local. The claim is
        // released by `GlobalHotkey::drop` on this thread.
        let status = unsafe {
            RegisterEventHotKey(
                key_code,
                modifiers,
                EventHotKeyID {
                    signature: SUMMON_SIGNATURE,
                    id: id as u32,
                },
                GetApplicationEventTarget(),
                0,
                &raw mut claimed,
            )
        };
        // **The station the owner's report had no way to reach.** A claim Carbon
        // accepted and a claim Carbon refused look identical from a keyboard —
        // the key does nothing either way — and this is the one line that tells
        // them apart before the handler is ever reached.
        super::trace(|| super::trace_register(key_code, modifiers, status));
        match status {
            NO_ERR if !claimed.is_null() => {
                // **The ledger goes up with the claim and not before** (R2-5):
                // it is what the handler reads to tell a press of ours from a
                // press of somebody else's registration.
                super::note_claimed(id);
                Ok(GlobalHotkey {
                    id,
                    claimed,
                    _thread_bound: PhantomData,
                })
            }
            EVENT_HOT_KEY_EXISTS_ERR => {
                super::note_released(id);
                Err(HotkeyFault::AlreadyRegistered)
            }
            other => {
                super::note_released(id);
                // `noErr` with a null reference lands here too, and deliberately:
                // a claim with nothing to release is not a claim.
                Err(HotkeyFault::Refused(format!(
                    "RegisterEventHotKey: OSStatus {other}"
                )))
            }
        }
    }

    /// **Whoever has the keyboard right now**, or `None` when it is us.
    ///
    /// An *application* and not a window, which is the whole of [`Foreground`]'s
    /// reason: macOS gives the keyboard to a process, and which of its windows
    /// then holds it is that process's business.
    ///
    /// **`None` for ourselves, and that is where the Windows arm's "the window
    /// this one came down over is not this one" guard lands on this platform.**
    /// Folio being frontmost is exactly the state in which there is nothing to
    /// give back — the summon is about to take the keyboard from one of our own
    /// windows — and recording ourselves would mean a dismissal that activated
    /// the application it was dismissing.
    ///
    /// Asked **before** the summoned window is shown and kept until it is
    /// dismissed: there is no second chance to read it.
    #[must_use]
    pub fn foreground_holder() -> Option<Foreground> {
        let workspace = NSWorkspace::sharedWorkspace();
        let frontmost = workspace.frontmostApplication()?;
        let pid = frontmost.processIdentifier();
        // **Compared by pid and not by `isEqual:`**, which is the opposite of
        // `handoff`'s own note — and for the reason that note gives. There the
        // question was *is this the application I just launched*, where Apple's
        // advice applies because two `NSRunningApplication` objects may name one
        // process. Here the question is *is this process me*, and a pid is what
        // that question is about.
        if pid == NSRunningApplication::currentApplication().processIdentifier() {
            return None;
        }
        // A pid of zero is the kernel and a negative one is not a process; both
        // are answers `frontmostApplication` has no way to give, and the
        // `NonZeroIsize` is what carries that into the type.
        std::num::NonZeroIsize::new(pid as isize).map(|holder| Foreground { holder })
    }

    /// **Give the keyboard back to the application that had it.**
    ///
    /// `-[NSRunningApplication activateWithOptions:]` with no options, which is
    /// the platform's whole answer: there is no foreground *lock* here, no
    /// thread input queue to join, and therefore none of the Windows arm's
    /// retry — the request either names a process that is still running or it
    /// does not.
    ///
    /// **The pid is revalidated by the lookup itself** (R2-3's rule at this
    /// door). A pid is reused by the kernel exactly as an `HWND` is by Windows,
    /// and `runningApplicationWithProcessIdentifier:` answering `nil` is how a
    /// process that has gone says so. It cannot rule out a pid that has been
    /// reused by *another application* between the summon and the dismissal;
    /// what that costs is one activation of the wrong program, in a window of
    /// time bounded by how long a reader leaves the terminal on the screen, and
    /// the alternative — holding the `NSRunningApplication` itself — keeps an
    /// object alive across the same window and answers the same question no
    /// better.
    ///
    /// **Failure is silent to the reader and reported to the caller**, which is
    /// the Windows arm's own note and is true here for the same reason.
    pub fn hand_back_to(holder: Foreground) -> bool {
        let pid = holder.holder.get();
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        let Some(application) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        else {
            return false;
        };
        // The default option set, which is `handoff`'s own ruling at a second
        // door: main and key, never `NSApplicationActivateAllWindows` — the
        // reader is going back to the one window they were in, not to every
        // window that application has open.
        application.activateWithOptions(NSApplicationActivationOptions::empty())
    }

    /// **Put this window of ours in front**, and say whether it got there.
    ///
    /// Two statements and they are both needed, which is the one thing about
    /// this that is not obvious: `makeKeyAndOrderFront:` puts a window at the
    /// top of *this application's* windows, and `-[NSApplication activate]`
    /// makes this application the one with the keyboard. A summon that made only
    /// the first statement would raise the quake window behind the editor the
    /// reader was in.
    ///
    /// **Read back rather than assumed**, which is the Windows arm's rule and is
    /// what the answer means: `isKeyWindow` afterwards is the only report that
    /// cannot be wrong. An activation is not instantaneous on this platform
    /// either, so a `false` here is "not yet" as often as it is "not at all" —
    /// and the caller's own reading of `false` is one line in the log, never a
    /// card in front of the reader.
    pub fn give_foreground_to(window: NativeWindow) -> bool {
        let Ok((mtm, window)) = crate::macos_impl::window_for(window, "the summoned window") else {
            return false;
        };
        let application = NSApplication::sharedApplication(mtm);
        // `activate` is macOS 14's spelling of `activateIgnoringOtherApps:` and
        // this product's deployment target is 14.0 (`LSMinimumSystemVersion`).
        application.activate();
        window.makeKeyAndOrderFront(None);
        window.isKeyWindow()
    }
}

/// The summon on a platform with no door to claim a chord at.
///
/// Not deferred work and not a stub for a third port: `bt-platform` is built for
/// a Linux server in 0.5 (`docs/plans/port/macos-plan-2026-09-12.md` §4.6) and a
/// server has no desktop to take a key out of. The refusal is a sentence rather
/// than a variant of [`HotkeyFault`] for the reason the fault list itself gives:
/// its variants are things a reader can do something about.
#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Debug)]
pub struct GlobalHotkey {
    /// Never constructed: [`register`] refuses.
    _never: std::convert::Infallible,
}

#[cfg(not(any(windows, target_os = "macos")))]
impl GlobalHotkey {
    /// The id this claim was made under. Unreachable: there is no claim.
    #[must_use]
    pub const fn id(&self) -> i32 {
        match self._never {}
    }
}

/// Claim the chord. Refused: there is no desktop here.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn register(id: i32, hotkey: Hotkey) -> Result<GlobalHotkey, HotkeyFault> {
    let _ = id;
    // **The product's own refusal first, exactly as the two real arms order
    // them** (R2-14): a chord with no modifier on it is refused for a reason
    // that is true on every platform, and telling the reader "not on this
    // platform" about a chord that would be refused anyway sends them to fix
    // the wrong thing.
    if !holds_a_summon_modifier(hotkey) {
        return Err(HotkeyFault::NoModifier);
    }
    Err(HotkeyFault::Refused(
        "the global summon key is not on this platform".to_owned(),
    ))
}

/// Nobody has the keyboard on a host with no desktop.
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn foreground_holder() -> Option<Foreground> {
    None
}

/// The handover, on a host with no foreground to hand.
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn hand_back_to(_holder: Foreground) -> bool {
    false
}

/// The handover, on a host with no foreground to hand.
///
/// The `bool` is read by `bt-app`, which prints one line when the window it
/// summoned could not take the keyboard — so the refusal is visible in
/// `diagnostics.log` rather than silent.
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn give_foreground_to(_window: crate::NativeWindow) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        HandoverStep, Hotkey, another_round, handover_step, holds_a_summon_modifier, is_our_hotkey,
        registration_bits,
    };
    // **The ledger's two writers are Windows'**, because nothing claims a chord
    // anywhere else yet (M4-8), and so is `summon_should_act`, which reads a
    // `WM_HOTKEY` — the inventory classifies that predicate as a compile-time
    // absence for exactly that reason. `is_our_hotkey` stays above with the
    // rest: it is a pure predicate over four integers and its test is a claim
    // about arithmetic, which is true on every platform.
    #[cfg(windows)]
    use super::{note_claimed, note_released, registration_is_live, summon_should_act};

    const fn chord(ctrl: bool, alt: bool, shift: bool, win: bool, virtual_key: u16) -> Hotkey {
        Hotkey {
            ctrl,
            alt,
            shift,
            win,
            virtual_key,
        }
    }

    /// RED — **the four flags reach the four bits, and nothing else does.**
    ///
    /// MUTATION: swap the `MOD_ALT` and `MOD_CONTROL` literals and a chord bound
    /// with Ctrl registers as one held with Alt; the window then answers a chord
    /// nobody bound and never answers the one they did. Both halves of that are
    /// invisible from inside this process, which is why the numbers are asserted
    /// rather than a round trip.
    #[test]
    fn every_modifier_reaches_its_own_bit() {
        let one = registration_bits(chord(true, false, false, false, 0x70))
            .expect("a modified function key is a hotkey");
        assert_eq!(one.1, 0x70, "the virtual key is carried unchanged");
        // The no-repeat bit and nothing else, written out rather than read back
        // off a bare chord: since R2-14 a bare chord is not a hotkey at all, so
        // the baseline has to be stated instead of measured.
        let only_repeat = 0x4000;
        // Shift is asked about beside Ctrl rather than alone, and by the rule
        // R2-14 wrote: shift alone is not a chord this crate will claim, so the
        // only way to see its bit is to add it to one that is.
        for (held, expected) in [
            (chord(true, false, false, false, 0x70), only_repeat | 0x0002),
            (chord(false, true, false, false, 0x70), only_repeat | 0x0001),
            (
                chord(true, false, true, false, 0x70),
                only_repeat | 0x0002 | 0x0004,
            ),
            (chord(false, false, false, true, 0x70), only_repeat | 0x0008),
        ] {
            let (modifiers, _) = registration_bits(held).expect("a modified key is a hotkey");
            assert_eq!(
                modifiers, expected,
                "exactly the bits this chord names, and no others, for {held:?}"
            );
        }
    }

    /// RED — **a held key is one press.**
    ///
    /// MUTATION: drop `MOD_NOREPEAT` from the mask and a summon held for half a
    /// second toggles the window at the keyboard's repeat rate, landing on
    /// whichever parity the finger lifted on.
    #[test]
    fn a_hotkey_never_repeats_while_it_is_held() {
        let (modifiers, _) = registration_bits(chord(true, false, true, false, 0xc0))
            .expect("a modified punctuation key is a hotkey");
        assert_eq!(
            modifiers & 0x4000,
            0x4000,
            "MOD_NOREPEAT is on every registration this crate makes"
        );
    }

    /// RED — **a key this layout cannot produce is not registered at all.**
    ///
    /// MUTATION: pass the zero through and `RegisterHotKey` claims whatever
    /// virtual key 0 means — a chord the reader cannot press and cannot get rid
    /// of.
    #[test]
    fn a_chord_with_no_key_is_refused_before_windows_sees_it() {
        assert_eq!(registration_bits(chord(true, false, true, false, 0)), None);
    }

    /// RED — **the hook claims a thread message with our id, and nothing else.**
    ///
    /// MUTATION: drop the `hwnd == 0` clause and the hook starts eating
    /// `WM_HOTKEY` messages addressed to windows, which in this process means any
    /// a library registers against its own `HWND`. Drop the id clause and a
    /// second registration's key summons the first one's window.
    /// RED (R2-14) — **a chord no modifier holds down is never claimed
    /// desktop-wide.**
    ///
    /// `RegisterHotKey` takes a key out of the input stream for every program on
    /// the machine. A bare letter recorded here means nothing on this desktop
    /// sees that letter again until Folio exits, and a persisted one brings the
    /// state back at every launch. Shift alone does not answer: `Shift+A` is how
    /// a capital `A` is typed.
    ///
    /// MUTATION: let the bare chord through and the first four assertions pass a
    /// claim on `k`, `Shift+k` and `F9` to Windows.
    #[test]
    fn a_summon_with_no_modifier_is_never_claimed() {
        // `k`, `Shift+k`, `F9` and `Shift+F9`: a letter, a capital, a function
        // key and a shifted function key. None of them is a summon.
        for bare in [
            chord(false, false, false, false, 0x4b),
            chord(false, false, true, false, 0x4b),
            chord(false, false, false, false, 0x78),
            chord(false, false, true, false, 0x78),
        ] {
            assert!(
                !holds_a_summon_modifier(bare),
                "shift alone is not a modifier a desktop-wide claim may be made on: {bare:?}"
            );
            assert_eq!(
                registration_bits(bare),
                None,
                "a chord with no modifier must not reach RegisterHotKey: {bare:?}"
            );
        }
        for held in [
            chord(true, false, false, false, 0x4b),
            chord(false, true, false, false, 0x4b),
            chord(false, false, false, true, 0xc0),
            chord(true, false, true, false, 0x78),
        ] {
            assert!(holds_a_summon_modifier(held), "{held:?}");
            assert!(registration_bits(held).is_some(), "{held:?}");
        }
    }

    /// RED (R2-5) — **a hotkey message is acted on only while this process holds
    /// the claim.**
    ///
    /// `WM_HOTKEY` with a null window and a `wparam` of one is four integers, and
    /// `PostThreadMessage` is a call any process of this user can make. The shape
    /// alone was enough to bring the window down over whatever the reader was
    /// doing — including when the shortcut had been cleared, and when Windows had
    /// refused the registration to a second copy of Folio.
    ///
    /// MUTATION: drop the liveness clause and the two `!` assertions below fail,
    /// which is a window that answers a key nobody registered.
    #[cfg(windows)]
    #[test]
    fn a_summon_is_acted_on_only_while_this_process_holds_the_claim() {
        // An id of this test's own: the ledger is process-wide, and the product's
        // own id belongs to whatever else is running beside this.
        const ID: i32 = 0x5eed;
        note_released(ID);
        assert!(!registration_is_live(ID));
        assert!(
            !summon_should_act(0x0312, 0, ID as usize, ID, registration_is_live(ID)),
            "a forged message with nothing registered must be dropped"
        );
        note_claimed(ID);
        assert!(registration_is_live(ID));
        assert!(
            summon_should_act(0x0312, 0, ID as usize, ID, registration_is_live(ID)),
            "the key Windows sent while we hold the claim is ours"
        );
        assert!(
            !summon_should_act(0x0312, 0x1234, ID as usize, ID, true),
            "a WM_HOTKEY with a window is still somebody else's"
        );
        note_released(ID);
        assert!(
            !summon_should_act(0x0312, 0, ID as usize, ID, registration_is_live(ID)),
            "a claim that has been given up is a key this process no longer answers"
        );
    }

    /// RED (R2-3) — **the handover never joins a hung application's input
    /// queue, and never runs past its budget.**
    ///
    /// `AttachThreadInput` makes two threads share one input queue, so attaching
    /// to a window that has stopped reading its messages hands Folio that
    /// application's paralysis — five rounds of it, on the event loop's own
    /// thread, with nothing in the loop that could time it out.
    ///
    /// MUTATION: drop the `foreground_is_hung` clause and the first assertion
    /// asks to be attached to a wedged program; drop the elapsed clause in
    /// `another_round` and the loop runs its five rounds however long each takes.
    #[test]
    fn the_handover_declines_a_hung_foreground_and_stops_at_its_budget() {
        use std::time::Duration;
        assert_eq!(
            handover_step(4242, 99, true),
            HandoverStep::ActivateAlone,
            "a foreground that has stopped answering is not a queue to join"
        );
        assert_eq!(
            handover_step(4242, 99, false),
            HandoverStep::AttachAndActivate,
            "an application that is answering is the documented way past the lock"
        );
        assert_eq!(
            handover_step(0, 99, false),
            HandoverStep::ActivateAlone,
            "nobody has the foreground, so there is nobody to join"
        );
        assert_eq!(
            handover_step(99, 99, false),
            HandoverStep::ActivateAlone,
            "a thread does not attach to itself"
        );
        assert!(
            another_round(0, Duration::from_secs(30)),
            "the first attempt is always made — the budget bounds the retries"
        );
        assert!(another_round(1, Duration::from_millis(1)));
        assert!(
            !another_round(1, super::FOREGROUND_BUDGET),
            "a round that would start past the budget is not started"
        );
        assert!(!another_round(4, Duration::from_secs(30)));
    }

    /// RED (M4-8) — **the ledger is one claim's life, on every platform.**
    ///
    /// Ungated since the second platform that claims a chord arrived: both
    /// registrations write it at the same two moments and both delivery roads
    /// read it, so a test that only ran on one of them would be a test of half
    /// the callers.
    ///
    /// MUTATION: drop the `contains` guard in `note_claimed` and one id is
    /// recorded twice, so the `note_released` below leaves — nothing, because
    /// `retain` takes both. Change `retain`'s comparison to `==` and a release
    /// keeps the claim it was told to drop, which is a window that answers a key
    /// nobody holds.
    #[test]
    fn a_claim_is_live_from_the_moment_it_is_noted_until_it_is_released() {
        // An id of this test's own: the ledger is process-wide.
        const ID: i32 = 0x4d48;
        super::note_released(ID);
        assert!(!super::registration_is_live(ID));
        super::note_claimed(ID);
        super::note_claimed(ID);
        assert!(super::registration_is_live(ID));
        super::note_released(ID);
        assert!(
            !super::registration_is_live(ID),
            "one release gives up the claim, however many times it was noted"
        );
    }

    /// RED (M4-8) — **every Carbon modifier reaches its own bit, and nothing
    /// else does.**
    ///
    /// The macOS twin of `every_modifier_reaches_its_own_bit`, and it runs on
    /// this workspace's Windows host for the reason the module header gives:
    /// `RegisterEventHotKey` takes a key *position*, so the whole translation is
    /// a pure function.
    ///
    /// MUTATION: swap the `CONTROL_KEY` and `OPTION_KEY` literals and a summon
    /// bound with `⌃` registers as one held with `⌥` — the window then answers a
    /// chord nobody bound and never answers the one they did, and both halves of
    /// that are invisible from inside this process. Give the mask a
    /// `MOD_NOREPEAT`-shaped extra bit and `RegisterEventHotKey` answers
    /// `paramErr` for a chord that is perfectly good.
    #[test]
    fn every_carbon_modifier_reaches_its_own_bit() {
        // `` ⌃` ``, the shipped macOS default — see `docs/DESIGN.md` §13.51 ②.
        let grave = super::carbon_key_code(super::SummonKey::Character('`'))
            .expect("the backtick has a position on every keyboard this runs on");
        let (modifiers, key) =
            super::carbon_registration_bits(chord(true, false, false, false, grave))
                .expect("a modified backtick is a hotkey");
        assert_eq!(key, u32::from(grave), "the key code is carried unchanged");
        assert_eq!(
            modifiers, 0x1000,
            "controlKey and nothing beside it — no no-repeat bit, because Carbon has none"
        );
        for (held, expected) in [
            (chord(true, false, false, false, grave), 0x1000),
            (chord(false, true, false, false, grave), 0x0800),
            (chord(false, false, false, true, grave), 0x0100),
            (chord(true, false, true, false, grave), 0x1000 | 0x0200),
            (
                chord(true, true, true, true, grave),
                0x1000 | 0x0800 | 0x0200 | 0x0100,
            ),
        ] {
            let (modifiers, _) =
                super::carbon_registration_bits(held).expect("a modified key is a hotkey");
            assert_eq!(
                modifiers, expected,
                "exactly the bits this chord names, and no others, for {held:?}"
            );
        }
    }

    /// RED (M4-8) — **the two spellings of the same key are the same key.**
    ///
    /// `WIN` and `CMD` are one bit in `bt_app::shortcuts` — `ModifiersState::SUPER`
    /// under two names — and the whole point of the `win` field carrying it is
    /// that a chord recorded on a Mac's Command key and one written `Win` in the
    /// table reach the same claim.
    ///
    /// MUTATION: read `hotkey.win` into `CONTROL_KEY` instead and `⌘\`` is
    /// claimed as `` ⌃` ``, which on a Mac is a chord the system does not
    /// reserve and the reader never asked for.
    #[test]
    fn the_command_key_is_the_windows_key_wearing_its_own_name() {
        let key = super::carbon_key_code(super::SummonKey::Character('`')).expect("a real key");
        let as_win = super::carbon_registration_bits(chord(false, false, false, true, key));
        let as_cmd = super::carbon_registration_bits(Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            // The same bit `shortcuts::CMD` is: there is one modifier here and
            // two names for it upstairs.
            win: true,
            virtual_key: key,
        });
        assert_eq!(as_win, as_cmd);
        assert_eq!(as_win.expect("a modified key is a hotkey").0, 0x0100);
    }

    /// RED (M4-8) — **key code zero is a letter here, not a refusal.**
    ///
    /// The one place the two arms of this module disagree about arithmetic, and
    /// the disagreement is real: `registration_bits` refuses `virtual_key == 0`
    /// because Win32 has no key on that number, while `kVK_ANSI_A` **is** zero.
    ///
    /// MUTATION: copy the Windows zero clause into `carbon_registration_bits`
    /// and a reader who binds `⌃A` gets a summon that silently never registers.
    #[test]
    fn the_letter_a_is_key_code_zero_and_is_still_a_hotkey() {
        assert_eq!(
            super::carbon_key_code(super::SummonKey::Character('a')),
            Some(0),
            "kVK_ANSI_A is zero, which is why the Windows zero clause cannot be copied"
        );
        let bits = super::carbon_registration_bits(chord(true, false, false, false, 0))
            .expect("a modified A is a hotkey on this platform");
        assert_eq!(bits, (0x1000, 0));
        assert_eq!(
            registration_bits(chord(true, false, false, false, 0)),
            None,
            "and on the other side the very same zero is a layout that could not answer"
        );
    }

    /// RED (M4-8) — **a key this keyboard has no position for is refused, and
    /// refused rather than guessed.**
    ///
    /// The ticket's own case: a chord whose key has no code must come back as a
    /// `None` that becomes [`HotkeyFault::NoSuchKey`], never a panic and never a
    /// fallback onto some other key.
    ///
    /// MUTATION: make the character arm's fallthrough `_ => 0` instead of
    /// `return None` and every unknown character claims `⌃A`.
    #[test]
    fn a_character_with_no_position_on_this_keyboard_is_refused() {
        for absent in ['é', '中', '€', '±', '\u{0}'] {
            assert_eq!(
                super::carbon_key_code(super::SummonKey::Character(absent)),
                None,
                "{absent:?} has no ANSI position, so there is no key code to claim"
            );
        }
        // And a chord built on one is refused by the product's own rule before
        // any of this is asked — there is no key to put in the struct at all.
        assert!(
            super::summon_key_code(super::SummonKey::Character('中')).is_none(),
            "the one door between a key and a number says no for every platform"
        );
    }

    /// RED (M4-8) — **case is not a key.**
    ///
    /// The recorder stores what a person typed, and a person holding shift types
    /// a capital. A table that only knew lower case would refuse `⇧⌃N` and
    /// accept `⇧⌃n`, which are the same press.
    ///
    /// MUTATION: drop the `to_ascii_lowercase` and the first assertion fails.
    #[test]
    fn a_capital_is_the_same_key_as_its_own_lower_case() {
        assert_eq!(
            super::carbon_key_code(super::SummonKey::Character('N')),
            super::carbon_key_code(super::SummonKey::Character('n')),
        );
        assert_eq!(
            super::carbon_key_code(super::SummonKey::Character('n')),
            Some(0x2d),
        );
    }

    /// RED (M4-8) — **every key the table upstairs can hold has a position, and
    /// the four that are easy to swap are the right way round.**
    ///
    /// Every named key `bt_app::shortcuts::BINDINGS` can carry is asked for by
    /// name, so a variant added there and forgotten here fails this rather than
    /// failing on somebody's Mac. The four spelled out are the ones whose names
    /// mean different keys on the two keyboards.
    ///
    /// MUTATION: read `Backspace` and `Delete` the other way round — the
    /// plausible mistake, because a Mac calls its backspace key *delete* — and
    /// the second assertion fails. Put the function row in numeric order and the
    /// third fails: `F1` is `0x7a` on this platform, not `0x3a`.
    #[test]
    fn every_named_key_the_table_can_hold_has_a_position() {
        use super::SummonNamedKey as Named;
        const EVERY: [Named; 27] = [
            Named::Tab,
            Named::Escape,
            Named::Enter,
            Named::Space,
            Named::Backspace,
            Named::Delete,
            Named::Insert,
            Named::Home,
            Named::End,
            Named::PageUp,
            Named::PageDown,
            Named::ArrowLeft,
            Named::ArrowUp,
            Named::ArrowRight,
            Named::ArrowDown,
            Named::F1,
            Named::F2,
            Named::F3,
            Named::F4,
            Named::F5,
            Named::F6,
            Named::F7,
            Named::F8,
            Named::F9,
            Named::F10,
            Named::F11,
            Named::F12,
        ];
        let mut seen: Vec<u16> = Vec::new();
        for named in EVERY {
            let code = super::carbon_key_code(super::SummonKey::Named(named))
                .unwrap_or_else(|| panic!("{named:?} is a key this table must know"));
            assert!(
                !seen.contains(&code),
                "{named:?} shares key code {code:#04x} with a key already in the table"
            );
            seen.push(code);
        }
        assert_eq!(
            (
                super::carbon_key_code(super::SummonKey::Named(Named::Backspace)),
                super::carbon_key_code(super::SummonKey::Named(Named::Delete)),
            ),
            (Some(0x33), Some(0x75)),
            "kVK_Delete is the key a Mac calls delete and everyone else calls backspace"
        );
        assert_eq!(
            super::carbon_key_code(super::SummonKey::Named(Named::F1)),
            Some(0x7a),
            "the function row is not in numeric order on this platform"
        );
    }

    /// RED (M4-8) — **a chord no modifier holds down is never claimed on this
    /// platform either.**
    ///
    /// R2-14's rule, restated at the second door that makes a desktop-wide
    /// claim. `RegisterEventHotKey` takes a key away from every application on
    /// the Mac exactly as `RegisterHotKey` does on Windows.
    ///
    /// MUTATION: drop the `holds_a_summon_modifier` guard from
    /// `carbon_registration_bits` and a bare `k` recorded once means nothing on
    /// that desk types the letter again until Folio exits.
    #[test]
    fn a_carbon_summon_with_no_modifier_is_never_claimed() {
        let k = super::carbon_key_code(super::SummonKey::Character('k')).expect("a real key");
        for bare in [
            chord(false, false, false, false, k),
            chord(false, false, true, false, k),
        ] {
            assert_eq!(super::carbon_registration_bits(bare), None, "{bare:?}");
        }
        for held in [
            chord(true, false, false, false, k),
            chord(false, true, false, false, k),
            chord(false, false, false, true, k),
        ] {
            assert!(super::carbon_registration_bits(held).is_some(), "{held:?}");
        }
    }

    #[test]
    fn only_a_thread_wm_hotkey_carrying_our_id_is_ours() {
        assert!(is_our_hotkey(0x0312, 0, 1, 1));
        assert!(
            !is_our_hotkey(0x0312, 0x1234, 1, 1),
            "a WM_HOTKEY with a window is somebody else's"
        );
        assert!(
            !is_our_hotkey(0x0312, 0, 2, 1),
            "another id is another claim"
        );
        assert!(
            !is_our_hotkey(0x0100, 0, 1, 1),
            "WM_KEYDOWN is not WM_HOTKEY"
        );
    }

    /// RED (T-MAC-SUMMON-DIAG) — **Carbon's own numbers, asserted as numbers on
    /// a host that has no Carbon.**
    ///
    /// The one defect this ticket found was a four-character code that was a
    /// real code in another framework's namespace, written inside the
    /// `#[cfg(target_os = "macos")]` module where no test on this host could
    /// name it. So the literals moved out and are read here **twice** — once as
    /// the `u32` `CarbonEvents.h` publishes and once as the four characters that
    /// spell it — because a pin that only re-wrote `*b"----"` would be the same
    /// echo the old proof was.
    ///
    /// MUTATION: put `'obj '` back in `K_EVENT_PARAM_DIRECT_OBJECT` and this
    /// goes red on this host, in a run with no Mac in it, naming the constant —
    /// which is the whole of what was missing while the summon key did nothing.
    #[test]
    fn carbons_four_character_codes_are_the_ones_carbon_events_publishes() {
        assert_eq!(
            super::K_EVENT_CLASS_KEYBOARD,
            0x6B65_7962,
            "kEventClassKeyboard is 'keyb'"
        );
        assert_eq!(
            super::K_EVENT_HOT_KEY_PRESSED,
            5,
            "kEventHotKeyPressed is 5; 6 is the release this module never asks for"
        );
        assert_eq!(
            super::K_EVENT_PARAM_DIRECT_OBJECT,
            0x2D2D_2D2D,
            "kEventParamDirectObject is '----', four hyphens — and 0x6F626A20 \
             ('obj ') is typeObjectSpecifier, an Apple event's namespace"
        );
        assert_eq!(
            super::TYPE_EVENT_HOT_KEY_ID,
            0x686B_6964,
            "typeEventHotKeyID is 'hkid'"
        );
        assert_eq!(
            super::EVENT_HOT_KEY_EXISTS_ERR,
            -9878,
            "eventHotKeyExistsErr, measured on the Mac mini by M4-8 ③"
        );
        assert_eq!(
            super::SUMMON_SIGNATURE,
            0x666F_6C6F,
            "this application's own signature, 'folo'"
        );
    }

    /// RED (T-MAC-SUMMON-DIAG) — **Carbon's modifier masks**, which until this
    /// ticket were held by nothing but a keyboard on somebody's desk.
    ///
    /// MUTATION: swap `CONTROL_KEY` and `OPTION_KEY` — the two that are one
    /// nibble apart and the easiest pair in the file to transpose — and this
    /// names both. A build with them transposed claims `` ⌥` ``, which on
    /// several layouts is the dead key that begins a grave accent: the summon
    /// would not come and the accent would stop working desktop-wide.
    #[test]
    fn carbons_modifier_masks_are_the_ones_macos_publishes() {
        assert_eq!(super::CMD_KEY, 0x0100, "cmdKey");
        assert_eq!(super::SHIFT_KEY, 0x0200, "shiftKey");
        assert_eq!(super::OPTION_KEY, 0x0800, "optionKey");
        assert_eq!(super::CONTROL_KEY, 0x1000, "controlKey");
    }

    /// RED (T-MAC-SUMMON-DIAG) — **the shipped macOS summon, all the way to the
    /// two integers Carbon is handed**, read on a host with no Carbon at all.
    ///
    /// `` ⌃` `` is `controlKey` and `kVK_ANSI_Grave`, and this is the assertion
    /// the owner's report is about: the claim they made with a physical press
    /// was made with exactly these numbers.
    ///
    /// MUTATION: change the backtick's answer in `carbon_key_code` from 0x32 to
    /// any neighbouring code and this names it. 0x0A is `kVK_ISO_Section`, the key left
    /// of `1` on an **ISO** keyboard, and is deliberately *not* what this
    /// answers — the product registers the ANSI position, the probe registers
    /// both and says which one a physical press arrives on, and the general
    /// answer is `UCKeyTranslate` (`docs/DESIGN.md` §13.51 ⑤).
    #[test]
    fn the_shipped_macos_summon_reaches_carbon_as_control_and_grave() {
        let grave = super::carbon_key_code(super::SummonKey::Character('`')).expect("a real key");
        assert_eq!(grave, 0x32, "kVK_ANSI_Grave");
        assert_eq!(
            super::carbon_registration_bits(chord(true, false, false, false, grave)),
            Some((0x1000, 0x32)),
            "controlKey and kVK_ANSI_Grave, in the order RegisterEventHotKey takes them"
        );
    }

    /// RED (T-MAC-SUMMON-DIAG) — **the trace's five stations, spelled once.**
    ///
    /// The lines are the whole product of this ticket's observability half, and
    /// a reader of `BT_HOTKEY_TRACE` reads them rather than running them. Pinned
    /// here because they are pure — formatting, on any host — and because the
    /// ticket, the documentation and the file have to agree about the bytes.
    ///
    /// MUTATION: drop the `#x` from either integer in `trace_register` and the
    /// line says `register keycode=50 modifiers=4096`, which is the same fact
    /// written in the one base no Carbon header uses.
    #[test]
    fn every_station_of_the_trace_is_one_line_a_reader_can_grep() {
        assert_eq!(
            super::trace_register(0x32, 0x1000, 0),
            "register keycode=0x32 modifiers=0x1000 -> Ok"
        );
        assert_eq!(
            super::trace_register(0x32, 0x1000, -9878),
            "register keycode=0x32 modifiers=0x1000 -> Err(-9878)"
        );
        assert_eq!(
            super::trace_register_refused(&super::HotkeyFault::NoSuchKey),
            "register -> Err(NoSuchKey)"
        );
        assert_eq!(super::trace_install(0), "install handler -> Ok");
        assert_eq!(super::trace_install(-50), "install handler -> Err(-50)");
        assert_eq!(
            super::trace_handler(super::SUMMON_SIGNATURE, 1, true),
            "handler fired id=1 signature=folo live=yes"
        );
        assert_eq!(
            super::trace_handler(super::SUMMON_SIGNATURE, 1, false),
            "handler fired id=1 signature=folo live=no"
        );
        assert_eq!(
            super::trace_handler_lost(-9870),
            "handler fired id=none GetEventParameter=Err(-9870)",
            "eventParameterNotFoundErr — what the shipped 'obj ' produced"
        );
        assert_eq!(
            super::four_character_code(0x2D2D_2D2D),
            "----",
            "and this is how a reader checks the one number that was wrong"
        );
        assert_eq!(
            super::four_character_code(0x0001_0002),
            "....",
            "a signature that is not text is not put into somebody's terminal"
        );
    }
}

/// The numbers written down above, held against the ones Windows publishes.
#[cfg(all(test, windows))]
mod win32_constant_tests {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;

    /// RED — this module's own copies of six Win32 constants are the Win32 ones.
    ///
    /// MUTATION: change any one of the six literals in this file and this fails
    /// naming it. It is the price of writing them down, and writing them down is
    /// what lets the translation above be tested on a host with no `windows`
    /// crate at all.
    #[test]
    fn the_numbers_written_down_are_the_numbers_windows_publishes() {
        assert_eq!(super::MOD_ALT, MOD_ALT.0);
        assert_eq!(super::MOD_CONTROL, MOD_CONTROL.0);
        assert_eq!(super::MOD_SHIFT, MOD_SHIFT.0);
        assert_eq!(super::MOD_WIN, MOD_WIN.0);
        assert_eq!(super::MOD_NOREPEAT, MOD_NOREPEAT.0);
        assert_eq!(super::WM_HOTKEY, WM_HOTKEY);
    }
}
