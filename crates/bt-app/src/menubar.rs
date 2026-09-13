//! **What is on the application menu bar** — one table, read against the
//! shortcut table and the window that has the keyboard (ticket M3-2,
//! `docs/DESIGN.md` §13.26).
//!
//! # Why the bar is a table here and an `NSMenu` over there
//!
//! Everything a reader reads on that bar is this crate's: the titles are
//! [`crate::i18n`]'s two columns and every key equivalent is a row of
//! [`crate::shortcuts::BINDINGS`] in the macOS dialect. `bt-platform` owns the
//! `NSMenu` and nothing else — see [`bt_platform::menu`], whose header says why
//! it takes a plan rather than a builder. So this file is the bar, and it is
//! **one constant** for the reason `BINDINGS` is: a second place that decides
//! what `Cmd+T` does is a second answer to a question with one.
//!
//! # Three kinds of row, and who answers each
//!
//! * **[`Row::Verb`]** — a row of the shortcut table, named by the stable id
//!   that table calls it by. Its **title and its chord are the row's**, never
//!   written again here: one name per verb is the rule
//!   [`crate::shortcuts::Shortcuts::accelerator`] was written to serve, because
//!   the menu and the hint card introduce a reader to the same verb twice and
//!   two names would make it two verbs. Pressing it parks the row's `Action` and
//!   the loop runs it through `run_shortcut` — the very function the chord
//!   reaches.
//! * **[`Row::Own`]** — something this product answers that has no row in that
//!   table. Help is the only one.
//! * **[`Row::Standard`]** — one of AppKit's own selectors, sent with no target,
//!   so the responder chain answers. This process never hears about them, which
//!   is the point: a text field in a sheet keeps its own Copy and a window keeps
//!   its own Minimize.
//!
//! # Which rows print a key, and why it is not all of them
//!
//! **AppKit answers a menu key equivalent before `keyDown:`.** That is what this
//! ticket is *for* — §13.16 ⑤ records that a chord typed into a live composition
//! never reaches this application, because winit hands the key to
//! `interpretKeyEvents:` — and it is also a claim on the key from every other
//! surface on the screen, the terminal's child included.
//!
//! The first draft leaned on the enabled flag to make that claim conditional:
//! grey the row out of scope, and the press falls through. **It does not.**
//! Measured on the Mac, 2026-09-12, in `tests/macos_menu_bar.rs`: a greyed row
//! does not run its verb — and `performKeyEquivalent:` still answers `YES`, so
//! the press is **swallowed**. A scoped row with a key equivalent would
//! therefore take `Cmd+S` away from a shell and give it to nobody.
//!
//! So the rule is the one that does not depend on that: **a row prints its key
//! only when it is in force everywhere** ([`Scope::Window`]). Every other row is
//! on the bar, and clickable, and greyed when it cannot act — but its chord is
//! left to `keyDown:` and `Shortcuts::lookup`, which is where the scope has
//! always been decided and is the one place that can decide it. `Ctrl+S`'s
//! ruling is untouched: with the keyboard anywhere but a preview the key is
//! still the child's, and the bar is not what changed that.
//!
//! The enabled flag stays, and it is still `Scope::holds` asked of the window
//! with the keyboard: a greyed row is one a reader can see cannot act, and —
//! measured above — one that will not act if pressed.

use bt_platform::menu::{
    AppMenuAction, DockRow, MenuAction, MenuChoice, MenuChord, MenuEntry, MenuItem, MenuKey,
    MenuList, MenuNamedKey, MenuPlan, MenuRole, StandardMenuAction,
};
use winit::keyboard::{ModifiersState, NamedKey};

use crate::i18n::Text;
use crate::shortcuts::{Chord, ChordKey, Focus, Scope, Shortcuts};

/// The title a menu on the bar wears.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BarTitle {
    /// **The product's name, untranslated.** AppKit draws the first menu's title
    /// in bold out of the bundle anyway; this is what it is called before it
    /// gets there, and a name is the same word in both columns.
    AppName,
    Words(Text),
}

/// One row of one menu, before it has been read against the shortcut table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Row {
    /// A row of [`crate::shortcuts::BINDINGS`], by its stable id.
    Verb(&'static str),
    /// The same, wearing the **menu's** name instead of the table's.
    ///
    /// Three rows of the application menu name the application — `About Folio`,
    /// `Hide Folio`, `Quit Folio` — because that is what every application menu
    /// on this platform is, and the one row of the three that is a verb of the
    /// table cannot read `Quit` alone without being the one row a reader stops
    /// at. It is the *name* that differs and nothing else: the verb, the chord
    /// and the scope are still the row's.
    VerbNamed(&'static str, Text),
    /// A verb of this product's that has no row in that table.
    Own(Text, AppMenuAction),
    /// A row AppKit answers through the responder chain.
    Standard(Text, StandardMenuAction),
    Rule,
    /// The system's Services submenu; Folio's own entry in it is M4-9's.
    Services,
}

/// One menu on the bar.
struct Bar {
    title: BarTitle,
    role: MenuRole,
    rows: &'static [Row],
}

/// **The bar, left to right.**
///
/// The order and the grouping are the platform's rather than this product's: a
/// reader opens Folio's File menu having opened forty others, and a bar that
/// put Quit somewhere new would be the one bar they have to learn.
const BAR: &[Bar] = &[
    Bar {
        title: BarTitle::AppName,
        role: MenuRole::Application,
        rows: &[
            Row::Standard(Text::MenuAboutFolio, StandardMenuAction::AboutPanel),
            Row::Rule,
            // The one row of this menu that is a verb of the table, and it keeps
            // the table's own name — `Settings`, which is what the gear's
            // tooltip and the shortcuts page both say.
            Row::Verb("open-settings"),
            Row::Rule,
            Row::Services,
            Row::Rule,
            Row::Standard(Text::MenuHideFolio, StandardMenuAction::Hide),
            Row::Standard(Text::MenuHideOthers, StandardMenuAction::HideOthers),
            Row::Standard(Text::MenuShowAll, StandardMenuAction::ShowAll),
            Row::Rule,
            // **This product's quit row and not `terminate:`** (§13.13 ⑤, X-4's
            // rule). What AppKit's own Quit item does is end the process where
            // it stands, past the session document and past every window's place
            // in Recent; X-3 measured exactly that. The title is the menu's
            // because the platform's is `Quit Folio` and the table's row is
            // called `Quit`.
            Row::VerbNamed("quit", Text::MenuQuitFolio),
        ],
    },
    Bar {
        title: BarTitle::Words(Text::MenuBarFile),
        role: MenuRole::Plain,
        rows: &[
            Row::Verb("new-tab"),
            Row::Verb("new-window"),
            Row::Rule,
            Row::Verb("save-preview"),
            Row::Rule,
            // `Cmd+W` is this window's close-pane row, whose own chain ends in
            // the window leaving: the last pane of a tab closes the tab and the
            // last tab hands off to the window's shut flow. So the row below it
            // is the *other* verb — the whole window, now — and it takes no
            // chord, because the platform's `Cmd+W` is already spent one line up.
            Row::Verb("close-pane"),
            Row::Standard(Text::MenuCloseWindow, StandardMenuAction::CloseWindow),
        ],
    },
    Bar {
        title: BarTitle::Words(Text::MenuBarEdit),
        role: MenuRole::Plain,
        rows: &[
            // **The six that AppKit answers, and they carry no chord.** See
            // `NOT_ON_THE_BAR` for the whole of the reason; the short form is
            // that `Cmd+C` and `Cmd+V` are `input.rs` predicates rather than
            // rows of the table, and a key equivalent here would take them off
            // the terminal that M1-7 gave them to.
            Row::Standard(Text::MenuUndo, StandardMenuAction::Undo),
            Row::Standard(Text::MenuRedo, StandardMenuAction::Redo),
            Row::Rule,
            Row::Standard(Text::MenuCut, StandardMenuAction::Cut),
            Row::Standard(Text::MenuCopy, StandardMenuAction::Copy),
            Row::Standard(Text::MenuPaste, StandardMenuAction::Paste),
            Row::Standard(Text::MenuSelectAll, StandardMenuAction::SelectAll),
            Row::Rule,
            Row::Verb("open-search"),
            Row::Verb("next-match"),
            Row::Verb("prev-match"),
        ],
    },
    Bar {
        title: BarTitle::Words(Text::MenuBarView),
        role: MenuRole::Plain,
        rows: &[
            Row::Verb("focus-mode"),
            Row::Verb("zoom-pane"),
            Row::Rule,
            Row::Verb("files-pane"),
            Row::Verb("git-page"),
            Row::Rule,
            Row::Verb("split-vertical"),
            Row::Verb("split-horizontal"),
            Row::Verb("duplicate-pane-split"),
            Row::Rule,
            Row::Verb("command-palette"),
            Row::Verb("window-address"),
            Row::Verb("web-address"),
            Row::Verb("web-devtools"),
            Row::Rule,
            Row::Verb("prev-command-mark"),
            Row::Verb("next-command-mark"),
        ],
    },
    Bar {
        title: BarTitle::Words(Text::MenuBarWindow),
        role: MenuRole::Windows,
        rows: &[
            Row::Standard(Text::Minimize, StandardMenuAction::Minimize),
            Row::Standard(Text::MenuZoomWindow, StandardMenuAction::ZoomWindow),
            Row::Rule,
            Row::Verb("next-tab"),
            Row::Verb("prev-tab"),
            Row::Verb("reopen-closed"),
            Row::Verb("jump-attention"),
            Row::Rule,
            Row::Standard(
                Text::MenuBringAllToFront,
                StandardMenuAction::BringAllToFront,
            ),
            // AppKit appends the list of open windows below this, which is what
            // `MenuRole::Windows` asks it to do. A list this product kept itself
            // would be a second answer to which windows there are.
        ],
    },
    Bar {
        title: BarTitle::Words(Text::MenuBarHelp),
        role: MenuRole::Plain,
        rows: &[Row::Own(Text::MenuFolioHelp, AppMenuAction::Help)],
    },
];

/// **What the Dock tile's menu carries**, above the rows AppKit puts on every
/// application's (T-MAC-DOCKMENU, `docs/DESIGN.md` §13.50).
///
/// Two verbs, named by the same stable ids the bar names them by, so that the
/// Dock and the File menu cannot come to call one verb two things — a Dock row's
/// title is `BINDINGS`' own, read through the same `Shortcuts::row` the bar
/// reads it through, which is this file's whole rule applied one surface along.
///
/// **Why these two and nothing else.** A Dock menu is opened by a reader who is
/// *not in Folio*: they are in another application, or looking at an empty desk
/// with Folio sitting in the Dock after its last window closed (M3-1). So the
/// only rows that belong on it are the ones that answer "give me somewhere to
/// work" — and both of these do, from either state. Everything else on the bar
/// needs a window to act on, and a Dock row that greys itself is a row that
/// looks like an offer and is not one.
///
/// **What was considered and left off**, because the ticket's own example is
/// Terminal.app and it offers three rows rather than two:
///
/// * `New command…` and `New remote connection…` are Terminal's verbs and Folio
///   has neither. A row that opened something else under one of those names
///   would be a promise this product does not keep;
/// * `Settings` is on the application menu and reachable from any window. It
///   needs Folio to be frontmost to be any use, which is the one thing a reader
///   opening this menu has said they are not;
/// * the window list, *Options*, *Show All Windows*, *Hide* and *Quit* are
///   **AppKit's own** and appear under these two without being asked for. Folio
///   writing its own would be a second answer to a question the system has
///   already answered — the same ruling `MenuRole::Windows` stands on.
const DOCK: &[&str] = &["new-window", "new-tab"];

/// **The rows of the shortcut table that hold a macOS chord and are deliberately
/// not on the bar.**
///
/// Listed by hand, with the reason beside each, which is what
/// `every_mac_chord_is_on_the_bar_or_named_here` promises: a row that grows a
/// chord and reaches neither the bar nor this list is a verb the menu quietly
/// stopped offering.
#[cfg(test)]
const NOT_ON_THE_BAR: &[(&str, &str)] = &[
    // **The nine tab ordinals.** They are one folded family everywhere else a
    // reader meets them — one line on the shortcuts page, one line on the hint
    // card — and a menu has no fold: nine rows reading `Go to tab 1` … `Go to
    // tab 9` would stand directly above the Window menu's real list of open
    // windows, which is AppKit's and is the answer to the question those nine
    // rows look like they are answering.
    (
        "goto-tab-1",
        "one of nine ordinals; the Window menu's real list is AppKit's",
    ),
    ("goto-tab-2", "as goto-tab-1"),
    ("goto-tab-3", "as goto-tab-1"),
    ("goto-tab-4", "as goto-tab-1"),
    ("goto-tab-5", "as goto-tab-1"),
    ("goto-tab-6", "as goto-tab-1"),
    ("goto-tab-7", "as goto-tab-1"),
    ("goto-tab-8", "as goto-tab-1"),
    ("goto-tab-9", "as goto-tab-1"),
    // **Escape, and the row is not on the bar at all rather than on it without
    // a key.** `close-search` is scoped, so the rule in `verb_entry` would
    // already refuse it a key equivalent — and the whole of what the row would
    // then be is a menu line reading `Close search`, which is the one rung of
    // §7.1.5's Escape ladder a reader would meet as a *menu item*. The ladder is
    // this window's own: every rung of it (a menu, a float, a drag, a
    // composition being cancelled) is raised long before the shortcut table is
    // asked, and none of the others is on a menu either. The capsule's own rung
    // already answers this press.
    (
        "close-search",
        "its key is Escape, and §7.1.5's ladder answers that press before the table is asked",
    ),
    // **The preview buffer's undo and redo**, and the Edit menu is the wrong
    // place for them rather than the right place they have not reached yet.
    // The menu's Undo is whatever holds the keyboard — a field in a sheet, a
    // form on a page — and these two are a *document of this window's*, scoped
    // to `PreviewDocument`. A menu row carrying them would answer a field's
    // `Cmd+Z` with the document's undo, because a key equivalent is answered
    // before `keyDown:`. Edit's two rows are AppKit's own selectors instead
    // (`EDIT_ROWS_APPKIT_ANSWERS`), and these two go on answering their own
    // chord through `keyDown:` exactly as they do today.
    (
        "undo-preview",
        "the preview buffer's undo; the Edit menu's Undo is the first responder's",
    ),
    ("redo-preview", "as undo-preview"),
    // **The summon.** Its chord is registered with the system (M4-8, Carbon
    // `RegisterEventHotKey`) so that it fires while another app is frontmost,
    // which is the whole point of it; the menu bar is only shown while Folio
    // is frontmost, where the chord is already answered by the hotkey itself.
    // A row would advertise a verb that the bar can never be the one to fire.
    (
        "summon-quake",
        "a system-wide hotkey (M4-8); it fires when Folio is not frontmost, where there is no bar",
    ),
];

/// **The chords the Edit menu does not print, and why not yet.**
///
/// Cut, Copy, Paste and Select All are not rows of the shortcut table at all:
/// the clipboard pair lives in `input.rs` as two predicates
/// (`is_copy_shortcut`, `is_paste_shortcut`), which the port's own plan §4.7
/// names as the thing `BINDINGS` does not cover. So there is no mac column to
/// read a key equivalent out of — and writing `Cmd+C` here by hand would be the
/// one thing this ticket's pin forbids, *and* would take the chord off the
/// terminal: AppKit answers a key equivalent before `keyDown:`, so `Cmd+C` on a
/// selection would stop reaching `should_copy_selection` the day this shipped.
///
/// Undo and Redo are the same decision arrived at from the other side. The table
/// *does* have them — `undo-preview` and `redo-preview` — but they are the
/// **preview buffer's** undo, scoped to a document this window is showing, and
/// the Edit menu's Undo is whatever holds the keyboard: a text field in a sheet,
/// a page's own form. Binding the row to the table's verb would answer the
/// field's `Cmd+Z` with the document's undo. So all six are AppKit's, with no
/// chord, and the two table rows keep answering their own chord through
/// `keyDown:` exactly as they do today.
///
/// What that costs, written down rather than hidden: **Edit ▸ Copy with the
/// keyboard on a terminal does nothing**, because nothing in the responder chain
/// under winit's view implements `copy:`. The seat for Folio's own answer is a
/// responder at the end of that chain — the application delegate's — which is
/// the application delegate's object, and `docs/DESIGN.md` §13.26 ④ carries it
/// forward as this ticket's one undelivered half.
#[cfg(test)]
const EDIT_ROWS_APPKIT_ANSWERS: &[StandardMenuAction] = &[
    StandardMenuAction::Undo,
    StandardMenuAction::Redo,
    StandardMenuAction::Cut,
    StandardMenuAction::Copy,
    StandardMenuAction::Paste,
    StandardMenuAction::SelectAll,
];

/// The named key a menu can print for a chord that names one.
///
/// `None` for a key outside the small set `bt_platform::menu` can spell — see
/// [`MenuNamedKey`], whose own note says what that costs and does not: the chord
/// still reaches `keyDown:` and the table still answers it.
const fn named_key(key: NamedKey) -> Option<MenuNamedKey> {
    Some(match key {
        NamedKey::ArrowUp => MenuNamedKey::ArrowUp,
        NamedKey::ArrowDown => MenuNamedKey::ArrowDown,
        NamedKey::ArrowLeft => MenuNamedKey::ArrowLeft,
        NamedKey::ArrowRight => MenuNamedKey::ArrowRight,
        NamedKey::Enter => MenuNamedKey::Enter,
        NamedKey::Escape => MenuNamedKey::Escape,
        NamedKey::Tab => MenuNamedKey::Tab,
        NamedKey::Space => MenuNamedKey::Space,
        NamedKey::Backspace => MenuNamedKey::Backspace,
        NamedKey::Delete => MenuNamedKey::Delete,
        NamedKey::Home => MenuNamedKey::Home,
        NamedKey::End => MenuNamedKey::End,
        NamedKey::PageUp => MenuNamedKey::PageUp,
        NamedKey::PageDown => MenuNamedKey::PageDown,
        NamedKey::F1 => MenuNamedKey::Function(1),
        NamedKey::F2 => MenuNamedKey::Function(2),
        NamedKey::F3 => MenuNamedKey::Function(3),
        NamedKey::F4 => MenuNamedKey::Function(4),
        NamedKey::F5 => MenuNamedKey::Function(5),
        NamedKey::F6 => MenuNamedKey::Function(6),
        NamedKey::F7 => MenuNamedKey::Function(7),
        NamedKey::F8 => MenuNamedKey::Function(8),
        NamedKey::F9 => MenuNamedKey::Function(9),
        NamedKey::F10 => MenuNamedKey::Function(10),
        NamedKey::F11 => MenuNamedKey::Function(11),
        NamedKey::F12 => MenuNamedKey::Function(12),
        _ => return None,
    })
}

/// **One chord of the table, as the bar prints it.**
///
/// The modifiers cross one for one — winit's fourth modifier is `SUPER` on every
/// platform and the keyboard it is printed on decides what it is called
/// (`shortcuts::CMD`'s own note), and on this platform it is Command. The key is
/// the character the table wrote, unshifted, which is what AppKit wants beside a
/// mask that carries the Shift.
#[must_use]
pub(crate) fn menu_chord(chord: &Chord) -> Option<MenuChord> {
    let key = match &chord.key {
        ChordKey::Character(text) if !text.is_empty() => MenuKey::Character(text.to_string()),
        ChordKey::Character(_) => return None,
        ChordKey::Named(named) => MenuKey::Named(named_key(*named)?),
    };
    Some(MenuChord {
        command: chord.modifiers.contains(ModifiersState::SUPER),
        shift: chord.modifiers.contains(ModifiersState::SHIFT),
        option: chord.modifiers.contains(ModifiersState::ALT),
        control: chord.modifiers.contains(ModifiersState::CONTROL),
        key,
    })
}

/// **The bar as it stands right now**, for this table and this keyboard.
///
/// `focus` is the frontmost window's — `None` when there is no window at all,
/// which is a state this product only reaches on macOS (§8 Q10) and which every
/// verb row answers the same way: disabled, because there is nothing to do a
/// verb *to*, and the door that makes a window again is the application
/// delegate's reopen (M3-1). The rows AppKit answers stay enabled throughout;
/// the responder chain is what decides whether they do anything, and it decides
/// that whether this product has an opinion or not.
#[must_use]
pub(crate) fn plan(shortcuts: &Shortcuts, focus: Option<Focus>) -> MenuPlan {
    MenuPlan {
        menus: BAR
            .iter()
            .map(|bar| MenuList {
                title: match bar.title {
                    BarTitle::AppName => crate::APP_NAME,
                    BarTitle::Words(text) => text.text(),
                },
                role: bar.role,
                entries: bar
                    .rows
                    .iter()
                    .map(|row| entry(*row, shortcuts, focus))
                    .collect(),
            })
            .collect(),
        dock: dock_rows(shortcuts),
    }
}

/// **The Dock tile's rows** (T-MAC-DOCKMENU).
///
/// `focus` is not an argument and that is the ruling rather than an oversight:
/// every row of this menu is in force from every state this application can be
/// in, including the one with no window at all, so there is nothing for the
/// keyboard's whereabouts to decide. See [`DOCK`] for why only rows of that kind
/// are on it.
///
/// The titles are the shortcut table's, exactly as the bar's are, which is what
/// makes the Dock menu and the File menu say the same word for the same verb in
/// whichever language the reader has chosen.
fn dock_rows(shortcuts: &Shortcuts) -> Vec<DockRow> {
    DOCK.iter()
        .copied()
        .map(|id| DockRow {
            // A row this build does not know cannot happen —
            // `every_verb_on_the_bar_names_a_row` holds this list to `BINDINGS`
            // as well as the bar's — and the fallback is the id rather than a
            // panic for `verb_entry`'s reason: a menu is drawn on a frame.
            title: shortcuts.row(id).map_or(id, |row| row.title.text()),
            choice: MenuChoice::Verb(id),
        })
        .collect()
}

/// One row of the constant, read against the table and the keyboard.
fn entry(row: Row, shortcuts: &Shortcuts, focus: Option<Focus>) -> MenuEntry {
    match row {
        Row::Rule => MenuEntry::Separator,
        Row::Services => MenuEntry::Services {
            title: Text::MenuServices.text(),
        },
        Row::Standard(title, what) => MenuEntry::Item(MenuItem {
            title: title.text(),
            action: MenuAction::Standard(what),
            chord: None,
            enabled: true,
        }),
        Row::Own(title, what) => MenuEntry::Item(MenuItem {
            title: title.text(),
            action: MenuAction::Application(what),
            chord: None,
            // Help opens a page and needs no window to do it, so it is the one
            // row of this product's that answers with every window closed.
            enabled: true,
        }),
        Row::Verb(id) => verb_entry(id, None, shortcuts, focus),
        Row::VerbNamed(id, title) => verb_entry(id, Some(title), shortcuts, focus),
    }
}

/// One row of the shortcut table, as the bar draws it.
fn verb_entry(
    id: &'static str,
    name: Option<Text>,
    shortcuts: &Shortcuts,
    focus: Option<Focus>,
) -> MenuEntry {
    let binding = shortcuts.row(id);
    MenuEntry::Item(MenuItem {
        // A row this build does not know is a row with no title, and it cannot
        // happen: `every_verb_on_the_bar_names_a_row` holds the constant to
        // `BINDINGS`. The fallback is the id rather than a panic because a menu
        // is drawn on a frame and a frame may not be the place this product
        // finds out.
        title: name.map_or_else(|| binding.map_or(id, |row| row.title.text()), Text::text),
        action: MenuAction::Verb(id),
        // **Only a row in force everywhere prints its key.** See
        // `a_scoped_row_prints_no_key` for the measurement that decided it: a
        // greyed item does not run its verb, and it still *swallows* the press.
        chord: binding
            .filter(|row| row.scope == Scope::Window)
            .and_then(|row| row.chord.as_ref())
            .and_then(menu_chord),
        enabled: focus.is_some_and(|focus| binding.is_some_and(|row| row.scope.holds(focus))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;
    use crate::shortcuts::{Action, BINDINGS, Scope};
    use bt_platform::HostPlatform;

    fn mac_table() -> Shortcuts {
        Shortcuts::defaults_for(HostPlatform::MacOs)
    }

    fn every_focus() -> Focus {
        Focus {
            preview: true,
            terminal_primary: true,
            search_open: true,
            web_page: false,
        }
    }

    fn verbs_on_the_bar() -> Vec<&'static str> {
        BAR.iter()
            .flat_map(|bar| bar.rows.iter())
            .filter_map(|row| match row {
                Row::Verb(id) | Row::VerbNamed(id, _) => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// PIN — **every verb the bar names is a row of `BINDINGS`.**
    ///
    /// The one thing that would otherwise rot silently: a row renamed in the
    /// table leaves a menu item with the id printed on it and no verb behind it,
    /// and nothing on Windows would ever draw that item.
    ///
    /// MUTATION: change one id in `BAR` or in `DOCK` by a letter.
    #[test]
    fn every_verb_on_the_bar_names_a_row() {
        let table = mac_table();
        for id in verbs_on_the_bar() {
            assert!(
                table.row(id).is_some(),
                "the menu bar names {id}, which is not a row of BINDINGS"
            );
        }
        // The Dock tile's rows are held to the same table by the same sentence
        // (T-MAC-DOCKMENU): a row renamed in `BINDINGS` would otherwise leave
        // the Dock offering `new-window` as its own title, on a surface no
        // Windows workstation ever draws.
        for id in DOCK {
            assert!(
                table.row(id).is_some(),
                "the Dock menu names {id}, which is not a row of BINDINGS"
            );
        }
    }

    /// PIN (T-MAC-DOCKMENU) — **the Dock tile offers the two rows it was ruled
    /// to offer, in that order, and nothing else.**
    ///
    /// The list is short enough to write out, and writing it out is the point:
    /// the ticket refused `New command…`, `New remote connection…` and
    /// `Settings` by name, and a row added here without that ruling being
    /// revisited should have to go through this case.
    #[test]
    fn the_dock_offers_a_new_window_and_a_new_tab() {
        assert_eq!(DOCK.to_vec(), vec!["new-window", "new-tab"]);
        let table = mac_table();
        let rows = dock_rows(&table);
        assert_eq!(
            rows.iter().map(|row| row.choice).collect::<Vec<_>>(),
            vec![MenuChoice::Verb("new-window"), MenuChoice::Verb("new-tab")]
        );
    }

    /// PIN (T-MAC-DOCKMENU) — **the Dock says the same word for a verb as the
    /// bar does, in both languages.**
    ///
    /// Two surfaces naming one verb is exactly the shape this whole file exists
    /// to refuse; the Dock is the second one and it reads the same table row.
    /// Asked through `in_lang` rather than by installing a language, for
    /// `every_word_on_the_bar_is_in_the_string_table_in_both_languages`' reason:
    /// the language is a process-wide switch.
    ///
    /// MUTATION: give `dock_rows` a literal title and this goes red in whichever
    /// language the literal is not.
    #[test]
    fn the_dock_names_a_verb_the_way_the_bar_names_it() {
        let table = mac_table();
        for row in dock_rows(&table) {
            let MenuChoice::Verb(id) = row.choice else {
                unreachable!("every Dock row is a verb of the table")
            };
            let binding = table.row(id).expect("the row is in BINDINGS");
            assert_eq!(row.title, binding.title.text(), "{id} is titled twice");
            let on_the_bar = plan(&table, Some(every_focus()))
                .rows()
                .find(|item| item.action == MenuAction::Verb(id))
                .map(|item| item.title);
            assert_eq!(Some(row.title), on_the_bar, "{id} is named two things");
            for lang in Lang::ALL {
                assert!(
                    !binding.title.in_lang(lang).is_empty(),
                    "{id} reads as nothing in {lang:?}"
                );
            }
        }
    }

    /// PIN (T-MAC-DOCKMENU) — **the Dock's rows do not move with the keyboard.**
    ///
    /// The bar greys a verb row when there is no window, because there is
    /// nothing to do a verb *to*; the Dock menu is read in exactly that state
    /// and its two rows are the answer to it. So the plan carries the same two
    /// rows whatever `focus` says, and `DockRow` has no flag to say otherwise.
    #[test]
    fn the_dock_is_the_same_with_no_window_as_with_one() {
        let table = mac_table();
        let with = plan(&table, Some(every_focus()));
        let without = plan(&table, None);
        assert_eq!(with.dock, without.dock);
        assert_eq!(with.dock.len(), DOCK.len());
        assert_ne!(with, without, "the bar itself does move with the keyboard");
    }

    /// PIN — **no verb is on the bar twice.**
    ///
    /// Two rows for one verb would be two places a reader is told about the same
    /// key, which is the thing `Binding::title` living on the row rather than on
    /// the action already refuses one surface down.
    #[test]
    fn no_verb_is_offered_twice() {
        let mut seen: Vec<&str> = Vec::new();
        for id in verbs_on_the_bar() {
            assert!(!seen.contains(&id), "{id} is on the menu bar twice");
            seen.push(id);
        }
    }

    /// PIN — **every key equivalent on the bar is `BINDINGS`' macOS column,
    /// byte for byte.**
    ///
    /// The whole of the ticket's second requirement: the bar is *generated* from
    /// the table, so a chord printed beside a menu row cannot be a second
    /// opinion about which key that verb has.
    ///
    /// MUTATION: give any row in `entry` a chord of its own and this goes red.
    #[test]
    fn every_chord_on_the_bar_is_the_tables_mac_column() {
        let table = mac_table();
        let plan = plan(&table, Some(every_focus()));
        let mut chorded = 0;
        for (item, row) in plan.rows().zip(
            BAR.iter()
                .flat_map(|bar| bar.rows.iter())
                .filter(|row| !matches!(row, Row::Rule | Row::Services)),
        ) {
            match row {
                Row::Verb(id) | Row::VerbNamed(id, _) => {
                    let expected = table
                        .row(id)
                        .filter(|row| row.scope == Scope::Window)
                        .and_then(|row| row.chord.as_ref())
                        .and_then(menu_chord);
                    assert_eq!(
                        item.chord, expected,
                        "{id} prints a chord the table did not"
                    );
                    if item.chord.is_some() {
                        chorded += 1;
                    }
                }
                _ => assert_eq!(
                    item.chord, None,
                    "{:?} prints a chord, and only a row of the table may",
                    item.title
                ),
            }
        }
        assert!(chorded > 10, "the bar printed almost no chords: {chorded}");
    }

    /// PIN — **a scoped row prints no key, because a key equivalent cannot be
    /// scoped.**
    ///
    /// Measured on the Mac (`tests/macos_menu_bar.rs`, 2026-09-12): a **greyed**
    /// item does not run its verb, and `performKeyEquivalent:` still answers
    /// `YES` — so the press is swallowed rather than handed on to `keyDown:`.
    /// A `Cmd+S` printed beside a greyed `Save` would therefore be `Cmd+S`
    /// reaching neither the preview nor the shell.
    ///
    /// MUTATION: drop the `Scope::Window` filter in `verb_entry` and this goes
    /// red on the first scoped row of the bar.
    #[test]
    fn a_scoped_row_prints_no_key() {
        let table = mac_table();
        let plan = plan(&table, Some(every_focus()));
        let mut scoped = 0;
        for item in plan.rows() {
            let MenuAction::Verb(id) = item.action else {
                continue;
            };
            let row = table.row(id).expect("checked above");
            if row.scope == Scope::Window {
                assert_eq!(
                    item.chord,
                    row.chord.as_ref().and_then(menu_chord),
                    "{id} is in force everywhere and does not print its key"
                );
            } else {
                assert_eq!(
                    item.chord, None,
                    "{id} is scoped and prints a key AppKit would answer out of scope"
                );
                assert!(
                    row.chord.is_some(),
                    "{id} would be a pointless case if it had no chord at all"
                );
                scoped += 1;
            }
        }
        assert!(scoped >= 5, "the bar has almost no scoped rows: {scoped}");
    }

    /// PIN — **every macOS chord a reader can be shown is on the bar or named in
    /// the exclusion list.**
    ///
    /// The other half of the same requirement, read from the table's end: a verb
    /// that grows a chord and reaches neither the bar nor `NOT_ON_THE_BAR` is a
    /// verb the menu quietly stopped offering, which is exactly the drift
    /// `docs/shortcuts.md`'s own gate exists to stop one document over.
    ///
    /// MUTATION: delete a row from `BAR` without listing it.
    #[test]
    fn every_mac_chord_is_on_the_bar_or_named_here() {
        let table = mac_table();
        let on_the_bar = verbs_on_the_bar();
        for binding in BINDINGS {
            if !binding.surfaced || binding.chord_on(HostPlatform::MacOs).is_none() {
                continue;
            }
            let excluded = NOT_ON_THE_BAR.iter().any(|(id, _)| *id == binding.id);
            assert!(
                on_the_bar.contains(&binding.id) || excluded,
                "{} holds a macOS chord and is neither on the menu bar nor named in \
                 NOT_ON_THE_BAR",
                binding.id
            );
            assert!(
                !(on_the_bar.contains(&binding.id) && excluded),
                "{} is both on the bar and excluded from it",
                binding.id
            );
            assert!(
                table.row(binding.id).is_some(),
                "{} is in BINDINGS and not in the macOS table",
                binding.id
            );
        }
    }

    /// PIN — every exclusion names a real row, and one that still has a chord.
    ///
    /// A list that outlives the rows it names is a list nobody notices is wrong.
    #[test]
    fn every_exclusion_names_a_row_that_still_has_a_chord() {
        let table = mac_table();
        for (id, reason) in NOT_ON_THE_BAR {
            let row = table
                .row(id)
                .unwrap_or_else(|| panic!("NOT_ON_THE_BAR names {id}, which is not a row"));
            assert!(
                row.chord.is_some(),
                "{id} is excluded from the bar for having a chord, and it has none"
            );
            assert!(!reason.is_empty(), "{id} is excluded with no reason");
        }
    }

    /// PIN — **a menu row fires the verb the chord fires.**
    ///
    /// The mapping table the ticket asks for, and it is short because there is
    /// nothing to map: the bar carries the row's **id**, `Shortcuts::row` reads
    /// the row, and the row's `action` is the same `Action` `lookup` answers a
    /// press with. A second table from ids to actions is what this test refuses.
    ///
    /// The press is made in a focus the row's own scope holds, and that is the
    /// claim rather than a convenience: out of scope the row is not in the table
    /// for that press *and* the menu item is disabled, which are the same
    /// sentence said to the two doors.
    #[test]
    fn a_menu_row_and_its_chord_reach_the_same_action() {
        let table = mac_table();
        for id in verbs_on_the_bar() {
            let row = table.row(id).expect("checked by the test above");
            let chord = row.chord.clone().expect("every bar verb has a macOS chord");
            let focus = focus_holding(row.scope);
            let key = chord_logical_key(&chord);
            let reached = table.lookup(&key, &key, chord.modifiers, focus);
            assert_eq!(
                reached,
                Some(row.action),
                "{id}: the chord and the menu row answer differently"
            );
            let enabled = plan(&table, Some(focus))
                .rows()
                .find(|item| item.action == MenuAction::Verb(id))
                .map(|item| item.enabled);
            assert_eq!(
                enabled,
                Some(true),
                "{id} is greyed where its chord answers"
            );
        }
    }

    /// A keyboard focus the named scope is in force in.
    fn focus_holding(scope: Scope) -> Focus {
        let mut focus = Focus::default();
        match scope {
            Scope::Window => focus.terminal_primary = true,
            Scope::Preview | Scope::PreviewDocument => focus.preview = true,
            Scope::TerminalPrimary | Scope::SearchHost => focus.terminal_primary = true,
            Scope::SearchOpen => {
                focus.terminal_primary = true;
                focus.search_open = true;
            }
            Scope::WebPage => focus.web_page = true,
        }
        assert!(scope.holds(focus), "{scope:?} is not held by its own focus");
        focus
    }

    /// The key a chord's press would arrive as — the character or the name.
    fn chord_logical_key(chord: &Chord) -> winit::keyboard::Key {
        match &chord.key {
            ChordKey::Character(text) => winit::keyboard::Key::Character(text.as_ref().into()),
            ChordKey::Named(named) => winit::keyboard::Key::Named(*named),
        }
    }

    /// PIN — **the Quit row is this product's verb and nothing else.**
    ///
    /// §13.13 ⑤: winit's own menu answered `Cmd+Q` with `terminate:` and quit
    /// past the session document. The menu coming back must not bring that with
    /// it, and this is where that is stated in the crate that builds the menu.
    ///
    /// MUTATION: make Quit a `Row::Standard` and this goes red.
    #[test]
    fn quit_is_the_products_own_verb() {
        let table = mac_table();
        assert!(
            verbs_on_the_bar().contains(&"quit"),
            "the bar has no quit row at all"
        );
        assert_eq!(
            table.row("quit").map(|row| row.action),
            Some(Action::Quit),
            "the bar's Quit is not the table's Quit"
        );
    }

    /// PIN — **a row out of scope is disabled, and that is how the key gets back
    /// to the child.**
    ///
    /// `save-preview` is `Ctrl+S`'s ruling on this platform: claimed where there
    /// is something to save and the child's everywhere else. On the bar the same
    /// sentence is an enabled flag, because AppKit fires a key equivalent only
    /// for an enabled item.
    ///
    /// MUTATION: enable every verb row unconditionally and `Cmd+S` stops
    /// reaching a shell.
    #[test]
    fn a_row_out_of_scope_is_a_row_that_hands_its_key_back() {
        let table = mac_table();
        let on_a_shell = Focus {
            preview: false,
            terminal_primary: true,
            search_open: false,
            web_page: false,
        };
        let on_a_preview = Focus {
            preview: true,
            terminal_primary: false,
            search_open: false,
            web_page: false,
        };
        let enabled_of = |focus: Option<Focus>, wanted: &'static str| {
            plan(&table, focus)
                .rows()
                .find(|item| item.action == MenuAction::Verb(wanted))
                .map(|item| item.enabled)
        };
        assert_eq!(enabled_of(Some(on_a_shell), "save-preview"), Some(false));
        assert_eq!(enabled_of(Some(on_a_preview), "save-preview"), Some(true));
        // A window row is in force from every focus state, which is what
        // `Scope::Window` means.
        assert_eq!(enabled_of(Some(on_a_shell), "new-tab"), Some(true));
        // And with no window at all there is nothing to do a verb to.
        assert_eq!(enabled_of(None, "new-tab"), Some(false));
        assert_eq!(enabled_of(None, "quit"), Some(false));
    }

    /// PIN — **the six rows AppKit answers carry no chord, and they are the six
    /// named.**
    ///
    /// The clipboard pair is `input.rs`' predicates and not a row of the table,
    /// so a chord here would be a chord typed twice — *and* would take `Cmd+C`
    /// off the terminal, because a key equivalent is answered before `keyDown:`.
    /// See `EDIT_ROWS_APPKIT_ANSWERS`.
    #[test]
    fn the_edit_rows_appkit_answers_take_no_key() {
        let table = mac_table();
        let plan = plan(&table, Some(every_focus()));
        let mut found: Vec<StandardMenuAction> = Vec::new();
        for item in plan.rows() {
            if let MenuAction::Standard(what) = item.action
                && EDIT_ROWS_APPKIT_ANSWERS.contains(&what)
            {
                assert_eq!(item.chord, None, "{:?} prints a chord", item.title);
                assert!(item.enabled, "{:?} is disabled", item.title);
                found.push(what);
            }
        }
        assert_eq!(found.len(), EDIT_ROWS_APPKIT_ANSWERS.len());
    }

    /// Every `Text` entry the bar reads: the five menu names and every row the
    /// shortcut table does not title.
    fn words_of_the_bar() -> Vec<Text> {
        let mut out = vec![Text::MenuServices];
        for bar in BAR {
            if let BarTitle::Words(text) = bar.title {
                out.push(text);
            }
            for row in bar.rows {
                match row {
                    Row::Own(text, _) | Row::Standard(text, _) | Row::VerbNamed(_, text) => {
                        out.push(*text)
                    }
                    Row::Verb(_) | Row::Rule | Row::Services => {}
                }
            }
        }
        out
    }

    /// PIN — **the whole bar is titled out of the string table, in both
    /// languages, with nothing left as a literal.**
    ///
    /// The i18n suite already refuses an entry with one column filled in; this
    /// is the other end of the same promise — that the menu reads its titles out
    /// of that table at all. Asked through `in_lang` rather than by installing a
    /// language, because the language is a process-wide switch and a test that
    /// threw it would be throwing it under every other test in this binary.
    #[test]
    fn every_word_on_the_bar_is_in_the_string_table_in_both_languages() {
        for text in words_of_the_bar() {
            for lang in Lang::ALL {
                assert!(
                    !text.in_lang(lang).is_empty(),
                    "{text:?} reads as nothing in {lang:?}"
                );
            }
        }
        // And every verb row's title is the *row's*, which is the rule this
        // whole file is written to: one name per verb, never a second literal.
        let table = mac_table();
        for item in plan(&table, Some(every_focus())).rows() {
            assert!(!item.title.is_empty(), "a menu row has no title");
            if let MenuAction::Verb(id) = item.action
                && !BAR
                    .iter()
                    .flat_map(|bar| bar.rows.iter())
                    .any(|row| matches!(row, Row::VerbNamed(named, _) if *named == id))
            {
                assert_eq!(
                    Some(item.title),
                    table.row(id).map(|row| row.title.text()),
                    "{id} drew a title that is not its row's"
                );
            }
        }
    }

    /// PIN — the first menu is the application's, the Window menu is the one
    /// AppKit fills, and there is exactly one of each.
    ///
    /// `MenuRole` is what `bt-platform` reads to decide which menu to hand
    /// `NSApp.setWindowsMenu:`, and two of them would leave the window list on
    /// whichever came last.
    #[test]
    fn the_bar_has_one_application_menu_and_one_window_menu() {
        assert_eq!(BAR[0].role, MenuRole::Application);
        assert_eq!(
            BAR.iter()
                .filter(|bar| bar.role == MenuRole::Application)
                .count(),
            1
        );
        assert_eq!(
            BAR.iter()
                .filter(|bar| bar.role == MenuRole::Windows)
                .count(),
            1
        );
        assert_eq!(
            BAR.iter()
                .flat_map(|bar| bar.rows.iter())
                .filter(|row| matches!(row, Row::Services))
                .count(),
            1,
            "there is one Services submenu, and AppKit is handed exactly one"
        );
    }

    /// PIN — a chord whose key this bar cannot print leaves the row bare rather
    /// than printing the wrong key.
    #[test]
    fn a_key_with_no_menu_glyph_leaves_the_row_bare() {
        let printable = Chord {
            modifiers: ModifiersState::SUPER,
            key: ChordKey::Named(NamedKey::ArrowUp),
        };
        assert_eq!(
            menu_chord(&printable).map(|chord| chord.key),
            Some(MenuKey::Named(MenuNamedKey::ArrowUp))
        );
        let unprintable = Chord {
            modifiers: ModifiersState::SUPER,
            key: ChordKey::Named(NamedKey::BrowserBack),
        };
        assert_eq!(menu_chord(&unprintable), None);
    }

    /// PIN — the modifiers cross one for one, and Command is winit's fourth.
    #[test]
    fn the_modifiers_cross_one_for_one() {
        let chord = Chord {
            modifiers: ModifiersState::SUPER | ModifiersState::SHIFT,
            key: ChordKey::Character("t".into()),
        };
        let printed = menu_chord(&chord).expect("a letter is printable");
        assert!(printed.command);
        assert!(printed.shift);
        assert!(!printed.option);
        assert!(!printed.control);
    }
}
