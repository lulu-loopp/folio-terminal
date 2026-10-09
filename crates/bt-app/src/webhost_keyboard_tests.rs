//! `webhost`'s keyboard tests: which chords the window claims from a page, as WebView2's
//! Win32 accelerator keys. Declared from `webhost.rs` as `webhost::keyboard_tests`; the
//! Windows-only cases name their platform here, so the product file names none.

use super::*;
use crate::shortcuts::{BINDINGS, Chord, ChordKey, Focus, Shortcuts};
use winit::keyboard::{ModifiersState, NamedKey};

/// Bare `Alt`. Named here and not beside the table above because no code
/// path in this window reaches it — the point of the row below is precisely
/// that this key is the page's.
#[cfg(windows)]
const VK_MENU: u16 = 0x12;

/// Every chord the shipped table carries, spelled the way a person presses
/// it — the product-side twin of the transcription the W0′ probe fired at a
/// focused page and got 30/30 back from (`w0p-evidence.md` §2.1).
///
/// **If `BINDINGS` changes this list must change with it**, which is the
/// whole reason it is written out: the reconciliation is only worth anything
/// while somebody is forced to look at it.
const EXPECTED_CHORDS: &[(&str, &str)] = &[
    ("new-tab", "Ctrl+Shift+n"),
    ("new-window", "Ctrl+Shift+m"),
    // **One arrived on 2026-08-23** (multiwindow slice E2): the whole
    // application leaving. It is claimed back off a focused page like every
    // other window verb — a page that swallowed it would be a page a reader
    // cannot quit out of.
    ("quit", "Ctrl+Shift+q"),
    ("close-pane", "Ctrl+Shift+w"),
    ("next-tab", "Ctrl+Tab"),
    ("prev-tab", "Ctrl+Shift+Tab"),
    ("goto-tab-1", "Ctrl+Shift+1"),
    ("goto-tab-2", "Ctrl+Shift+2"),
    ("goto-tab-3", "Ctrl+Shift+3"),
    ("goto-tab-4", "Ctrl+Shift+4"),
    ("goto-tab-5", "Ctrl+Shift+5"),
    ("goto-tab-6", "Ctrl+Shift+6"),
    ("goto-tab-7", "Ctrl+Shift+7"),
    ("goto-tab-8", "Ctrl+Shift+8"),
    ("goto-tab-9", "Ctrl+Shift+9"),
    ("reopen-closed", "Ctrl+Shift+t"),
    ("jump-attention", "Ctrl+Shift+a"),
    // **`command-palette` is back on the list from 2026-09-02** (DESIGN.md
    // §7.55). It was off it for one release for the reason it was off the
    // table: an unclaimed chord is the page's to keep. The window claims it
    // again, so the page hands it back again — for the reason every window
    // row is on this list.
    ("command-palette", "Ctrl+Shift+p"),
    ("focus-mode", "Ctrl+Shift+z"),
    ("split-horizontal", "Alt+Shift+-"),
    ("split-vertical", "Alt+Shift+="),
    ("duplicate-pane-split", "Ctrl+Shift+d"),
    // **One more on 2026-08-25** (B7). A page must hand this one back for
    // the reason every window row is on this list: a chord the window owns
    // that a focused page keeps is a chord that stops working exactly where
    // a reader is most likely to want the pane bigger.
    //
    // It read `Ctrl+Shift+Enter` for half a day. A modified `Enter` never
    // reaches this application at all (measured 2026-08-19 and again
    // 2026-08-25), so the ruling moved the verb to a key that arrives, and
    // this list moved with it — a claim spelled for a chord the window can
    // never be reached by is a claim that takes nothing back from anybody.
    ("zoom-pane", "Ctrl+Shift+x"),
    ("text-larger", "Ctrl+="),
    ("text-smaller", "Ctrl+-"),
    ("text-actual-size", "Ctrl+0"),
    ("files-pane", "Ctrl+Shift+b"),
    ("git-page", "Ctrl+Shift+g"),
    ("open-settings", "Ctrl+,"),
    ("save-preview", "Ctrl+s"),
    // **Two more on 2026-09-10** (ticket T3), and they are the two rows on
    // this list a focused page does **not** hand back — the only such pair
    // this window ships, and `KEPT_BY_A_PAGE` is where that is counted. A
    // page *is* a preview seat, so a `Scope::Preview` row would be in force
    // over one; `Ctrl+Z` and `Ctrl+Y` are a page's own undo inside its own
    // fields (§2.2, and `the_page_keeps_every_key_the_window_does_not_claim`
    // below), so the two rows carry `Scope::PreviewDocument` instead.
    ("undo-preview", "Ctrl+z"),
    ("redo-preview", "Ctrl+y"),
    ("prev-command-mark", "Ctrl+Shift+ArrowUp"),
    ("next-command-mark", "Ctrl+Shift+ArrowDown"),
    ("open-search", "Ctrl+f"),
    ("next-match", "F3"),
    ("prev-match", "Shift+F3"),
    // **Three arrived on 2026-08-22** (§7.7, W2 slice ④): the capsule's own
    // Escape, and the two rows the user ruled in for a page's address field
    // and its developer tools.
    ("close-search", "Escape"),
    ("web-address", "Ctrl+l"),
    // **One more on 2026-08-24** (§7.7 ⑨). It is a `Scope::Window` row, so a
    // page hands it back in every focus state — which is the whole of what
    // the row is for: the address door has to answer over a page as well as
    // beside one, or the one surface where an address is most obviously
    // wanted would be the one place the chord went missing.
    ("window-address", "Ctrl+Shift+l"),
    ("web-devtools", "F12"),
];

fn spell(chord: &Chord) -> String {
    let mut out = String::new();
    if chord.modifiers.contains(ModifiersState::CONTROL) {
        out.push_str("Ctrl+");
    }
    if chord.modifiers.contains(ModifiersState::ALT) {
        out.push_str("Alt+");
    }
    if chord.modifiers.contains(ModifiersState::SHIFT) {
        out.push_str("Shift+");
    }
    match &chord.key {
        ChordKey::Character(text) => out.push_str(text),
        ChordKey::Named(named) => out.push_str(&format!("{named:?}")),
    }
    out
}

/// RED — the reconciliation. The table the window dispatches on and the list
/// the web host takes back from a focused page are the same rows.
#[test]
fn the_chord_table_the_web_seat_claims_is_the_table_the_window_ships() {
    // The same two filters `claimable_chords` applies, because the claim is
    // what this reconciles against: a row with no chord has none to give,
    // and a row whose chord Windows takes out of the input stream has none
    // to give back either (§7.54 ①).
    let spelled: Vec<(&str, String)> = BINDINGS
        .iter()
        .filter(|row| !row.action.is_claimed_from_windows())
        .filter_map(|row| row.chord.as_ref().map(|chord| (row.id, spell(chord))))
        .collect();
    let expected: Vec<(&str, String)> = EXPECTED_CHORDS
        .iter()
        .map(|(id, chord)| (*id, (*chord).to_owned()))
        .collect();
    assert_eq!(spelled, expected);
    assert_eq!(spelled.len(), EXPECTED_CHORDS.len());
}

/// RED (user ruling 2026-08-26) — **the claim follows the user's table and
/// not this build's.**
///
/// `claimable_chords` has read the effective table since it was written, and
/// `WebSeat::set_claims` recomputes on every turn — but nothing said so, and
/// the count above was a literal `36` that a shortcut page could not move.
/// Now that a chord can be rebound and swapped from a dialog, the derivation
/// is what the page is *for*: a page that kept handing the window back the
/// factory chords after somebody rebound one would be a shortcut table with
/// two answers, and the second one would only be wrong over a web seat.
///
/// MUTATION: pass `Shortcuts::defaults()` to `WebSeat::set_claims` instead
/// of the runtime's table — this goes red on both halves at once, and on the
/// real window `Ctrl+Shift+Y` would open a tab everywhere except over a page.
///
/// Windows only, like every claim test below that names a key by its Win32
/// number: see `every_shipped_chord_resolves_to_a_virtual_key_on_this_layout`.
#[cfg(windows)]
#[test]
fn the_claims_follow_a_rebound_chord_rather_than_the_one_this_build_ships() {
    let mut table = Shortcuts::defaults();
    let shipped = claimable_chords(&table, every_focus());
    assert!(claims_chord(&shipped, b'N' as u16, true, true, false));

    table.set("new-tab", crate::shortcuts::parse_chord("Ctrl+Shift+y"));
    let claims = claimable_chords(&table, every_focus());
    assert!(
        claims_chord(&claims, b'Y' as u16, true, true, false),
        "the page hands back the chord the user chose"
    );
    assert!(
        !claims_chord(&claims, b'N' as u16, true, true, false),
        "and keeps the one they gave up"
    );
    assert_eq!(
        claims.len(),
        shipped.len(),
        "a rebinding moves a claim; it does not add or drop one"
    );

    // A row cleared outright leaves the page one key richer, which is the
    // whole of what `\"chord\": null` is for.
    table.set("new-tab", None);
    let cleared = claimable_chords(&table, every_focus());
    assert_eq!(cleared.len(), shipped.len() - 1);
    assert!(!claims_chord(&cleared, b'Y' as u16, true, true, false));
}

/// How many of [`EXPECTED_CHORDS`] a focused page keeps for itself,
/// because their rows are out of force over one (ticket T3, 2026-09-10).
///
/// A literal rather than a filter over `BINDINGS`, and deliberately: a
/// count derived from the same scopes the claim is derived from would
/// agree with itself whatever anybody did to it. This is a number a
/// person has to change on purpose, which is what makes the next row
/// scoped away from a page a decision somebody took rather than one that
/// happened.
///
/// Five since ticket 37: the three text-size rows are [`Scope::Terminal`]'s, and a page
/// holding the keyboard is not a terminal holding it, so a page keeps `Ctrl+=`, `Ctrl+-`
/// and `Ctrl+0` for its own zoom.
#[cfg(windows)]
const KEPT_BY_A_PAGE: usize = 5;

/// RED — and every one of them reaches a virtual key, because
/// `AcceleratorKeyPressed` speaks Win32 and nothing else.
///
/// Windows only: a character's virtual key is the installed Windows layout's
/// answer (`VkKeyScanW`), and the claim list is WebView2's vocabulary. A
/// `WKWebView` has no accelerator callback — the window's own key handling
/// sees a Command chord before the page does, and the macOS host keeps the
/// list without reading it (`bt_platform::macos_webview`'s `Shared::chords`);
/// that order is what `bt-platform/tests/macos_menu_bar.rs` drives.
#[cfg(windows)]
#[test]
fn every_shipped_chord_resolves_to_a_virtual_key_on_this_layout() {
    let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
    assert_eq!(
        claims.len(),
        EXPECTED_CHORDS.len() - KEPT_BY_A_PAGE,
        "a chord this window owns that the web host cannot name in Win32 is \
         a chord that silently stops working while a page has the focus"
    );
    for claim in &claims {
        assert!(claim.chord.virtual_key != 0, "{:?}", claim.action);
    }
}

/// RED — W0′'s accidental finding, written down where the next person to add
/// a row will trip over it (`w0p-evidence.md` §2.4).
///
/// A bare printable key never enters `AcceleratorKeyPressed` at all: the
/// probe pressed `K` with the page focused, the page received it and the
/// callback did not fire once. So a bare letter in `BINDINGS` would be a
/// shortcut that works everywhere except over a web seat, silently. The
/// table has none today; this test is what says so tomorrow.
#[test]
fn no_shipped_chord_is_a_bare_printable_key() {
    for row in BINDINGS {
        let Some(chord) = row.chord.as_ref() else {
            continue;
        };
        let bare = chord.modifiers.is_empty();
        let printable = matches!(&chord.key, ChordKey::Character(_));
        assert!(
            !(bare && printable),
            "{}: a bare printable key never reaches AcceleratorKeyPressed, so \
             this row would never fire while a page has the focus",
            row.id
        );
    }
}

/// RED — the other half of the matrix (`w0p-evidence.md` §2.2): the keys the
/// page needs are the keys this window does not claim.
///
/// Windows only, for the reason above.
#[cfg(windows)]
#[test]
fn the_page_keeps_every_key_the_window_does_not_claim() {
    let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
    let ctrl = |vk: u16| claims_chord(&claims, vk, true, false, false);
    for letter in ['C', 'V', 'X', 'A', 'Z', 'Y', 'R', 'P'] {
        assert!(
            !ctrl(letter as u16),
            "Ctrl+{letter} belongs to the page (clipboard, undo, reload, print)"
        );
    }
    // F5 is still the page's: reload has a button of its own on the head,
    // and the key the engine already answers with the same verb is not one
    // this table has any reason to take.
    assert!(!claims_chord(&claims, VK_F5, false, false, false));
    // **F12 is the window's since 2026-08-22** (user ruling). It is a door
    // that did not otherwise exist from the keyboard — the developer-tools
    // tool is invisible until the pointer arrives — and the verb behind it
    // is this window's `web-devtools` row.
    assert!(claims_chord(&claims, VK_F12, false, false, false));
    // And `Ctrl+L`, which is the address field's second door.
    assert!(claims_chord(&claims, b'L' as u16, true, false, false));
    // Bare Alt walks in as a system key and walks straight out to the page.
    assert!(!claims_chord(&claims, VK_MENU, false, false, true));
    // The control row: this one is the window's, and the page must not see it.
    assert!(claims_chord(&claims, b'Z' as u16, true, true, false));
}

/// RED — `Alt+Left` / `Alt+Right` are the engine's back and forward.
///
/// Measured: the callback sees them, the host does not claim them, and the
/// page never receives them — the engine eats them itself and navigates.
/// Slice ① leaves that as it stands; taking them back is a product ruling
/// slice ④ makes, and it would be made *here*, by adding two rows to the
/// claim, which is why this test names the fact rather than the code.
#[test]
fn alt_left_and_alt_right_stay_with_the_engine() {
    let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
    assert!(!claims_chord(&claims, VK_LEFT, false, false, true));
    assert!(!claims_chord(&claims, VK_RIGHT, false, false, true));
    // **And the ruling that kept them there** (§7.7, W2 slice ④,
    // 2026-08-22). Not a measurement this time but a decision, and this is
    // the nail in it: no row of the shipped table may claim those two
    // chords under *any* focus the window can be in, so taking them back is
    // a change somebody makes to the ruling and never one that arrives as a
    // side effect of adding a row.
    for row in BINDINGS {
        let Some(chord) = row.chord.as_ref() else {
            continue;
        };
        let alt_only = chord.modifiers == ModifiersState::ALT;
        let arrow = matches!(
            &chord.key,
            ChordKey::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight)
        );
        assert!(
            !(alt_only && arrow),
            "{}: Alt+Left and Alt+Right are the engine's back and forward (DESIGN §7.7 W2 ④); claiming one is a ruling, not a row",
            row.id
        );
    }
}

/// PIN (§7.7 ②, W2 slice ④) — **`Ctrl+F` is claimed over a page, and that
/// is what keeps the engine's own find bar out of the seat.**
///
/// This build's bindings do not carry `AreBrowserAcceleratorKeysEnabled`, so
/// a key this table does not take is a key the engine keeps — and the key
/// the engine keeps here opens a second search box inside a window whose
/// whole search story is that there is one.
///
/// MUTATION: put `open-search` back on `Scope::TerminalPrimary` and this
/// goes red, which is the second host losing its capsule.
///
/// Windows only: the claim is spelled in Win32 virtual keys, as above.
#[cfg(windows)]
#[test]
fn the_page_gives_the_search_chord_back_to_the_window() {
    let on_a_page = Focus {
        preview: true,
        terminal_primary: false,
        terminal: false,
        search_open: false,
        web_page: true,
    };
    let claims = claimable_chords(&Shortcuts::defaults(), on_a_page);
    assert!(claims_chord(&claims, b'F' as u16, true, false, false));
    // And Escape is *not* claimed until there is a capsule to put away: a
    // page owns every key this table does not, and with nothing open there
    // is nothing for the window to do with it.
    assert!(!claims_chord(&claims, VK_ESCAPE, false, false, false));
    let searching = Focus {
        search_open: true,
        ..on_a_page
    };
    let claims = claimable_chords(&Shortcuts::defaults(), searching);
    assert!(claims_chord(&claims, VK_ESCAPE, false, false, false));
}

/// RED — a row out of scope is not claimed, because a key the window will
/// not act on must not be taken away from the page.
///
/// Windows only: the claim is spelled in Win32 virtual keys, as above.
#[cfg(windows)]
#[test]
fn a_row_out_of_scope_is_left_to_the_page() {
    let nothing_focused = Focus::default();
    let claims = claimable_chords(&Shortcuts::defaults(), nothing_focused);
    // `save-preview` is Ctrl+S and is scoped to a focused preview seat.
    assert!(!claims_chord(&claims, b'S' as u16, true, false, false));
    let on_a_preview = Focus {
        preview: true,
        ..Focus::default()
    };
    let claims = claimable_chords(&Shortcuts::defaults(), on_a_preview);
    assert!(claims_chord(&claims, b'S' as u16, true, false, false));
}

#[test]
fn the_named_keys_the_table_uses_all_have_a_virtual_key() {
    for named in [
        NamedKey::Tab,
        NamedKey::ArrowUp,
        NamedKey::ArrowDown,
        NamedKey::F3,
    ] {
        assert!(named_key_virtual_key(named).is_some(), "{named:?}");
    }
}

/// RED (M4-8) — **the two tables that name a key by a number agree about
/// every key either of them knows.**
///
/// M4-8 separated the summon's key code from a page's virtual key, because
/// they are answers in two currencies on two platforms — see
/// `quake::hotkey_for`'s note. On **this** platform they are the same
/// number, and that is a promise rather than a coincidence: the Windows arm
/// of `bt_platform::hotkey::summon_key_code` is `VkKeyScanW` and a named-key
/// table, which is what this function is. Two tables written down and one
/// checked against the other is this workspace's own shape for a number that
/// has to be stated twice.
///
/// **Both directions**, because either gap is a real defect: a named key
/// this table knows and the summon's does not is a chord a reader can bind
/// to a page and not to the summon, and the reverse is a summon that claims
/// a key no page will ever be offered.
///
/// MUTATION: change any one of the twenty-seven numbers in either table and
/// this fails naming the key.
#[test]
fn every_named_key_a_page_can_claim_is_a_key_a_summon_can_claim() {
    // Every `NamedKey` either table has an opinion about. Written out rather
    // than iterated, because `NamedKey` is `winit`'s and has hundreds of
    // variants — what is being asserted is that these twenty-seven agree,
    // not that the two tables are exhaustive over a library's enum.
    const EVERY: [NamedKey; 27] = [
        NamedKey::Tab,
        NamedKey::Escape,
        NamedKey::Enter,
        NamedKey::Space,
        NamedKey::Backspace,
        NamedKey::Delete,
        NamedKey::Insert,
        NamedKey::Home,
        NamedKey::End,
        NamedKey::PageUp,
        NamedKey::PageDown,
        NamedKey::ArrowLeft,
        NamedKey::ArrowUp,
        NamedKey::ArrowRight,
        NamedKey::ArrowDown,
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
    for named in EVERY {
        let page = named_key_virtual_key(named);
        let summon = crate::quake::summon_key(&ChordKey::Named(named));
        assert!(
            page.is_some() && summon.is_some(),
            "{named:?} is known to one of the two tables and not the other"
        );
        // The numbers themselves can only be compared where one currency is
        // in use, which is the platform whose numbers both tables hold —
        // asked of the host at run time, bt-app's one platform decision.
        if matches!(
            bt_platform::host_platform(),
            bt_platform::HostPlatform::Windows
        ) {
            assert_eq!(
                page,
                summon.and_then(bt_platform::hotkey::summon_key_code),
                "{named:?} is two different keys depending on which door asks"
            );
        }
    }
    // A key neither table has a number for is refused by both, and refused
    // rather than guessed.
    assert_eq!(named_key_virtual_key(NamedKey::PrintScreen), None);
    assert_eq!(
        crate::quake::summon_key(&ChordKey::Named(NamedKey::PrintScreen)),
        None
    );
}

fn every_focus() -> Focus {
    Focus {
        preview: true,
        terminal_primary: true,
        // **Not the terminal's own scope** (ticket 37): whatever else this focus stands
        // for, the keyboard is on the page, and `Scope::Terminal` is exactly the claim
        // a page must not take — the page keeps its own `Ctrl+=`.
        terminal: false,
        search_open: true,
        web_page: true,
    }
}
