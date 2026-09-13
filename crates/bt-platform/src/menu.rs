//! **The application menu bar: what is on it, and the one door that installs
//! it** (ticket M3-2, `docs/DESIGN.md` §13.26).
//!
//! # Why the plan is a value and not a builder
//!
//! Everything on a macOS menu bar that a reader can read comes from the
//! application: the titles are `bt_app::i18n`'s table, and every key equivalent
//! is a row of `bt_app::shortcuts::BINDINGS` read in the macOS dialect. Neither
//! of those is reachable from this crate, and neither should be — a menu built
//! here would be a second place that decides what `Cmd+T` does, which is the one
//! thing the shortcut table exists to prevent.
//!
//! So the application hands this module a **[`MenuPlan`]**: a plain data
//! structure, with no Objective-C in it, that says what the bar is. This module
//! turns it into an `NSMenu` on macOS and into nothing at all anywhere else. The
//! plan is comparable ([`PartialEq`]), which is what lets [`refresh`] be called
//! on every turn of the event loop and touch AppKit only when something has
//! actually moved — a language switch, a rebound chord, the keyboard landing
//! somewhere a scoped row is not in force.
//!
//! # Why a choice is parked rather than run
//!
//! X-4's rule, and it is the same rule the rest of this crate's deferred
//! gestures obey: **nothing is executed inside an AppKit callback**. A menu
//! action runs on the main thread with AppKit's own menu tracking still on the
//! stack, and `bt-app` answering a verb there would be `bt-app` running a verb
//! inside a nested event loop — E55 on the other platform, and the reason
//! `MathContextMenu` schedules instead of popping.
//!
//! What an action does here is one statement: hand the [`MenuChoice`] to the
//! [`MenuSender`] the application installed.
//!
//! **And that sender is M3-1's channel rather than one of this module's own.**
//! `bt-app` wraps the choice in an [`AppDelegateEvent`](crate::AppDelegateEvent)
//! with [`AppDelegateOrigin::Menu`](crate::AppDelegateOrigin::Menu) and hands it
//! to [`AppDelegate::sender`](crate::AppDelegate::sender), so a menu press is
//! parked in the same inbox as a Finder reopen, buffered by the same rule,
//! released in the same order, and drained by the same statement on the loop's
//! own turn. What the two have in common is everything that gave that channel
//! its shape: the gesture arrives at **AppKit** rather than at any window of
//! ours, on the main thread, inside a callback with a framework frame
//! underneath it. Two channels would be two answers to one question about one
//! stack — and, less abstractly, a reopen and the `New window` row a reader
//! pressed a millisecond later arriving in two inboxes with no order between
//! them.
//!
//! # What the enabled flag is for, and what it is not
//!
//! **AppKit answers a menu key equivalent before `keyDown:`.** That is the whole
//! reason this ticket exists — `docs/DESIGN.md` §13.16 ⑤ records that a chord
//! typed into a live composition never reaches the application, because winit
//! hands the key to `interpretKeyEvents:`, and a menu is the only thing on this
//! platform that answers first. It is also a loaded gun: a key equivalent is
//! claimed from *everything*, including the terminal's child and every text
//! field on the screen.
//!
//! **A disabled row is not the safety catch it looks like.** Measured in
//! `tests/macos_menu_bar.rs` on 2026-09-12: a greyed item does not run its
//! action — and `-[NSMenu performKeyEquivalent:]` still answers `YES` for it, so
//! the press is swallowed rather than passed on to the key window. What the flag
//! is, then, is exactly what it says: a row a reader can see cannot act, and one
//! that will not act if pressed. What it is **not** is a way to give a
//! conditional verb an unconditional key.
//!
//! So the application prints a key equivalent only beside a row that is in force
//! from every focus state, and leaves a scoped row's chord to `keyDown:` — see
//! `bt_app::menubar`, which is where that rule lives, because the scope is its
//! table's and not this module's.
//!
//! # The floor under the responder chain (T-MAC-EDIT-CLIPBOARD)
//!
//! A [`MenuAction::Standard`] row is sent with **no target**, so AppKit walks
//! the responder chain for something that answers the selector. That is right
//! for every row on the Edit menu and it has one consequence nobody wrote down
//! until it was reported: over a **terminal pane** the walk finds nothing.
//! Folio's grid is drawn by this process on a winit view, it is not an
//! `NSTextView`, and no responder under it implements `copy:` — so `Edit ▸ Copy`
//! on a selection put nothing on the pasteboard and `Edit ▸ Paste` typed
//! nothing.
//!
//! The answer is not to take the rows off the chain — a page in a web pane and
//! a field in a sheet must go on winning, and they win by being *earlier* in the
//! walk. The answer is to put a **floor** at the end of it: AppKit's last
//! resort for a nil-target action is the application delegate, and
//! `macos_app` now adds `copy:` and `paste:` there. Reaching that floor
//! means precisely one thing — no text responder wanted this — and it is
//! reported as [`AppMenuAction::CopySelection`] / [`AppMenuAction::PasteIntoFocus`]
//! on **this module's own sender**, so a press that came off the Edit menu is
//! parked in the inbox every other menu press is parked in, in the order it was
//! pressed. What the application does with it is `bt_app::menubar`'s
//! `clipboard_seat`, which is the only place that knows what has the keyboard.
//!
//! **Neither row grows a key equivalent out of this**, and that is the load
//! bearing half: `Cmd+C` and `Cmd+V` go on reaching `keyDown:` and
//! `input::should_copy_selection`, because a key equivalent is answered before
//! `keyDown:` and would take the chord off the terminal M1-7 gave it to.
//!
//! # The second surface: the Dock tile's own menu (T-MAC-DOCKMENU)
//!
//! Right-clicking an application's Dock tile opens a menu AppKit builds — the
//! window list, *Options*, *Show All Windows*, *Hide*, *Quit* — and an
//! application may put rows of its own **above** those by answering
//! `applicationDockMenu:`. Terminal.app does; Folio did not, which is the whole
//! of the ticket.
//!
//! **It is the same plan, the same target and the same sender**, and that is a
//! decision rather than a saving. The rows a Dock menu can carry are rows this
//! application already has — a new window, a new tab — their titles are the same
//! `bt_app::i18n` entries the bar reads, and a press on one arrives at AppKit on
//! the same stack a bar press does. Two plans would be two places that decide
//! what `New window` is called; two senders would be two inboxes with no order
//! between them. So [`MenuPlan`] carries [`MenuPlan::dock`] beside the bar's
//! menus, [`install`] and [`refresh`] carry it without a door of their own, and
//! what tells the application which of the two a press came from is the
//! [`MenuSurface`] the sender is handed beside the choice.
//!
//! **Nothing of the Dock menu is kept alive between right-clicks.** AppKit asks
//! for it every time the reader opens it, so the `NSMenu` is built from the plan
//! on the spot and handed over autoreleased. That is also the whole answer to
//! "rebuild it when the language changes": there is no built menu to rebuild,
//! only a plan, and [`refresh`] already carries a new language onto it on the
//! turn the reader switches.

/// **What a menu item asks for**, in the application's own vocabulary.
///
/// Three kinds, and the difference between them is who answers:
///
/// * [`Self::Verb`] — a row of the application's shortcut table, named by the
///   stable id that table calls it by (`"new-tab"`, `"quit"`). This crate never
///   interprets the string; it carries it back on [`MenuChoice::Verb`] and the
///   application looks it up in the one table it already has.
/// * [`Self::Application`] — something the application answers that has no row
///   in that table at all.
/// * [`Self::Standard`] — one of AppKit's own selectors, with no target, so the
///   responder chain decides. Folio does not hear these *from the row*, which
///   is the point: a text field in a sheet keeps its own Copy, and the window
///   keeps its own Minimize.
///
///   **Two of them have a floor under the chain** (T-MAC-EDIT-CLIPBOARD):
///   `copy:` and `paste:` are answered by the application delegate when nobody
///   above it would, and arrive as
///   [`AppMenuAction::CopySelection`] / [`AppMenuAction::PasteIntoFocus`]. That
///   does not change what the row is — it is still the chain that decides, and
///   a real text responder still wins — it only stops the chain ending in
///   nothing over a surface AppKit has never heard of.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    Verb(&'static str),
    Application(AppMenuAction),
    Standard(StandardMenuAction),
}

/// The application's own verbs that are not rows of the shortcut table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppMenuAction {
    /// Open the page a reader is sent to for help — through the process door,
    /// which is the only way anything leaves this window.
    Help,
    /// **Edit ▸ Copy, after the responder chain declined it**
    /// (T-MAC-EDIT-CLIPBOARD, `docs/DESIGN.md` §13.26 ⑨).
    ///
    /// Not a row of any menu, and that is the whole of how it works. The Edit
    /// menu's Copy stays [`MenuAction::Standard`] — `copy:` with no target — so
    /// that a real text responder answers it first: the page in a web pane, a
    /// field in a sheet, an open panel's search box. This variant is what
    /// arrives when **nothing in that chain answered**, because the last
    /// responder AppKit tries is the application delegate and this crate now
    /// puts one there. It therefore means exactly one thing: *the reader chose
    /// Copy and no text responder wanted it*, which is the state a terminal
    /// pane and Folio's own preview editor are always in.
    ///
    /// It is on this enum rather than in the shortcut table because the
    /// clipboard pair genuinely has no row there — `Cmd+C` is `input.rs`'
    /// `should_copy_selection` and never `Shortcuts::lookup` — which is what
    /// [`Self::Help`]'s own sentence says about the one other verb here.
    CopySelection,
    /// **Edit ▸ Paste, after the responder chain declined it.** See
    /// [`Self::CopySelection`]; the two are one decision and arrive by one
    /// route.
    PasteIntoFocus,
}

impl AppMenuAction {
    /// **The two verbs that reach the application through AppKit's responder
    /// chain rather than off a row of a menu** (T-MAC-EDIT-CLIPBOARD).
    ///
    /// Written out so that both halves of the crossing can be held to one list:
    /// the platform adds a delegate method per entry, and `bt_app::menubar`
    /// refuses to put any of them on the bar as a row of its own.
    pub const THROUGH_THE_RESPONDER_CHAIN: &[Self] = &[Self::CopySelection, Self::PasteIntoFocus];
}

/// The AppKit selectors this bar sends with **no target**, so that whatever
/// holds the keyboard answers them.
///
/// Spelled as an enum rather than as a selector string for the reason every
/// other crossing in this crate is: a `sel!` in the application would be the
/// application speaking Objective-C, and a typo in a selector name is a runtime
/// silence rather than a compile error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StandardMenuAction {
    /// `orderFrontStandardAboutPanel:` — the panel AppKit builds out of the
    /// bundle's own `Info.plist`, which is where this product's name and version
    /// already are.
    AboutPanel,
    Hide,
    HideOthers,
    ShowAll,
    Undo,
    Redo,
    Cut,
    /// `copy:` — and the one selector here whose chain **ends somewhere**: the
    /// application delegate answers it when no text responder did, which is
    /// [`AppMenuAction::CopySelection`] (T-MAC-EDIT-CLIPBOARD).
    Copy,
    /// `paste:`, with [`Self::Copy`]'s floor under it —
    /// [`AppMenuAction::PasteIntoFocus`].
    Paste,
    SelectAll,
    /// `performClose:` — which reaches the window delegate and therefore
    /// winit's `WindowEvent::CloseRequested`, so the window's own close flow
    /// runs. Not `close`, which would take the window down where it stands.
    CloseWindow,
    Minimize,
    /// `performZoom:` — the green button's verb, which is the window's and not
    /// a pane's.
    ZoomWindow,
    BringAllToFront,
}

/// A named key a chord can be pressed with, in the small set a menu can print.
///
/// **A key this list does not name is a key the menu prints nothing for**, and
/// that is an answer rather than a gap: the chord still reaches the application
/// through `keyDown:` and the shortcut table still answers it, exactly as it does
/// today on a build with no menu at all. What is lost is the annotation on the
/// row, and what is refused is this crate guessing at a glyph for a key AppKit
/// has no constant for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuNamedKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Enter,
    Escape,
    Tab,
    Space,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    /// A function key, `1..=12`.
    Function(u8),
}

impl MenuNamedKey {
    /// The character AppKit wants in a key equivalent for this key.
    ///
    /// The function-key block is Unicode's private use area, which is where
    /// AppKit puts every key that has no character of its own
    /// (`NSUpArrowFunctionKey` and its neighbours, `NSEvent.h`). The four that
    /// are ordinary characters — Enter, Tab, Space, Escape — are themselves.
    #[must_use]
    pub const fn key_equivalent(self) -> Option<char> {
        let code: u32 = match self {
            Self::ArrowUp => 0xF700,
            Self::ArrowDown => 0xF701,
            Self::ArrowLeft => 0xF702,
            Self::ArrowRight => 0xF703,
            Self::Enter => 0x000D,
            Self::Escape => 0x001B,
            Self::Tab => 0x0009,
            Self::Space => 0x0020,
            Self::Backspace => 0x0008,
            Self::Delete => 0xF728,
            Self::Home => 0xF729,
            Self::End => 0xF72B,
            Self::PageUp => 0xF72C,
            Self::PageDown => 0xF72D,
            Self::Function(ordinal) => {
                if ordinal == 0 || ordinal > 12 {
                    return None;
                }
                0xF704 + (ordinal as u32) - 1
            }
        };
        char::from_u32(code)
    }
}

/// The key half of a chord as a menu prints it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuKey {
    /// The character printed on the key, unshifted — `"t"`, `"]"`, `","`.
    Character(String),
    Named(MenuNamedKey),
}

/// One chord, as AppKit's two halves: the key, and the modifiers held with it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuChord {
    pub command: bool,
    pub shift: bool,
    pub option: bool,
    pub control: bool,
    pub key: MenuKey,
}

/// One row of one menu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuItem {
    pub title: &'static str,
    pub action: MenuAction,
    /// The chord this row prints and answers, or `None` for a row reached only
    /// by the pointer.
    pub chord: Option<MenuChord>,
    /// **Whether this row answers at all** — see the module header. A disabled
    /// row is greyed *and* hands its key equivalent back to the key window.
    pub enabled: bool,
}

/// One entry of one menu: a row, a rule between rows, or the system's own
/// Services submenu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
    /// **The Services submenu, handed to AppKit to fill** (`NSApp.servicesMenu`).
    ///
    /// Empty here on purpose: what goes in it is decided by the system out of
    /// every application's `NSServices`, and Folio's own entry is M4-9's.
    Services {
        title: &'static str,
    },
}

/// What one menu on the bar is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuList {
    pub title: &'static str,
    pub role: MenuRole,
    pub entries: Vec<MenuEntry>,
}

/// Which of the three menus AppKit itself has an opinion about this is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuRole {
    /// The first menu, whose title AppKit replaces with the process's own name
    /// in bold. Everything in it is still this plan's.
    Application,
    /// A menu of the application's own, start to finish.
    Plain,
    /// Handed to AppKit as `NSApp.windowsMenu`, so that the list of open windows
    /// is the system's and not a second one this product maintains.
    Windows,
}

/// **One row of the Dock tile's own menu** (T-MAC-DOCKMENU).
///
/// Smaller than [`MenuItem`] by three fields, and each absence is a ruling:
///
/// * **no [`MenuAction`]** — a Dock row is one of this application's own verbs
///   and never one of AppKit's selectors. The responder chain a `Standard` row
///   is answered by is the key window's, and a Dock menu is opened by a reader
///   who is usually in *another application*; there is no first responder for
///   `Copy` to mean anything to;
/// * **no chord** — the menu is reached with the pointer and nothing else, and
///   a key equivalent printed here would be a second place claiming a key that
///   `keyDown:` and the bar have already settled between them;
/// * **no enabled flag** — every row of it is in force whenever it is drawn.
///   The two verbs it carries make a window or a tab, and the state a bar row
///   greys itself for — no window open — is precisely the state a reader opens
///   this menu *in*. See `docs/DESIGN.md` §13.50 ②.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DockRow {
    pub title: &'static str,
    pub choice: MenuChoice,
}

/// **The whole bar**, in the order it is read left to right, and the rows the
/// Dock tile's menu carries above AppKit's own.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MenuPlan {
    pub menus: Vec<MenuList>,
    /// **What Folio puts on its Dock menu** (T-MAC-DOCKMENU), in the order it
    /// is read top to bottom, above the rows AppKit adds for every application.
    ///
    /// Empty is an answer and not a gap: an application that offers nothing of
    /// its own answers `applicationDockMenu:` with nil, and the reader gets
    /// AppKit's menu exactly as they did before this ticket.
    pub dock: Vec<DockRow>,
}

impl MenuPlan {
    /// Every row of every menu, in the order [`install`] numbers them.
    ///
    /// The order is the contract between [`install`] and [`refresh`]: a row is
    /// found again by its position in this walk, so a plan that changes its
    /// *shape* — a row added, a separator moved — is a plan that has to be
    /// installed again rather than refreshed. [`refresh`] checks exactly that.
    pub fn rows(&self) -> impl Iterator<Item = &MenuItem> {
        self.menus
            .iter()
            .flat_map(|menu| menu.entries.iter())
            .filter_map(|entry| match entry {
                MenuEntry::Item(item) => Some(item),
                MenuEntry::Separator | MenuEntry::Services { .. } => None,
            })
    }

    /// Whether two plans name the same rows in the same places — which is what
    /// makes one refreshable into the other.
    ///
    /// **[`Self::dock`] is not read here**, and that is not an omission. The
    /// question this answers is whether the `NSMenuItem`s the platform is
    /// holding can be carried onto the new plan, and the Dock menu has none: it
    /// is built out of the plan on the right-click that asks for it and freed
    /// when that menu closes. So a plan that changed only its Dock rows — the
    /// reader switching language with the bar's shape untouched — is refreshed
    /// like any other, and the next right-click reads the new words.
    #[must_use]
    pub fn same_shape_as(&self, other: &Self) -> bool {
        self.menus.len() == other.menus.len()
            && self.menus.iter().zip(&other.menus).all(|(mine, theirs)| {
                mine.role == theirs.role
                    && mine.entries.len() == theirs.entries.len()
                    && mine
                        .entries
                        .iter()
                        .zip(&theirs.entries)
                        .all(|(mine, theirs)| match (mine, theirs) {
                            (MenuEntry::Item(mine), MenuEntry::Item(theirs)) => {
                                mine.action == theirs.action
                            }
                            (MenuEntry::Separator, MenuEntry::Separator) => true,
                            (MenuEntry::Services { .. }, MenuEntry::Services { .. }) => true,
                            _ => false,
                        })
            })
    }
}

/// **What a press on a menu row asks the application to do**, parked for the
/// next turn of the event loop.
///
/// [`MenuAction::Standard`] has no variant here, and that is the whole of what
/// "standard" means: those rows are answered by the responder chain, and a row
/// this process hears about is a row it answered itself.
///
/// **A `Copy` the chain declined is therefore not a standard row arriving
/// late** (T-MAC-EDIT-CLIPBOARD): it arrives as
/// [`Self::Application`], because by the time the application delegate is asked
/// the question is no longer "who answers this selector" but "what does Folio
/// do about it", and that is one of this product's own verbs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuChoice {
    Verb(&'static str),
    Application(AppMenuAction),
}

/// **Which of this application's two menus a press came from**
/// (T-MAC-DOCKMENU).
///
/// The choice is the same on both and the channel is the same on both; what
/// differs is what the application may assume around the press, and it is a
/// real difference rather than bookkeeping:
///
/// * [`Self::Bar`] — the reader is *in* Folio. The bar drew itself against the
///   window that has the keyboard, and greyed every verb row when there is
///   none, so a choice arriving from it names that window;
/// * [`Self::Dock`] — the reader is usually in **another application**, there
///   may be no key window at all, and there may be no window at all: Folio
///   stays in the Dock after its last window closes (M3-1). So a Dock choice
///   names the window the reader was last in, and makes one when there is none.
///
/// It is an argument to the sender rather than a second sender, for the reason
/// [`AppDelegateOrigin::Menu`](crate::AppDelegateOrigin::Menu) gives about the
/// channel: two inboxes for one kind of press would be two orders for two
/// presses a millisecond apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuSurface {
    /// The application menu bar (M3-2).
    Bar,
    /// The Dock tile's own menu (T-MAC-DOCKMENU).
    Dock,
}

/// **Where a pressed row goes**, called on AppKit's own stack.
///
/// It is handed the surface and the choice and nothing else, and it may do
/// nothing but keep them and post one wake — see the module header. The one
/// implementation wraps them in an
/// [`AppDelegateEvent`](crate::AppDelegateEvent) and hands it to
/// [`AppDelegate::sender`](crate::AppDelegate::sender). A sender that turned the
/// event loop would be turning it from inside AppKit's menu tracking, with
/// winit's handler borrow still live.
pub type MenuSender = Box<dyn Fn(MenuSurface, MenuChoice) + Send + Sync + 'static>;

// ── the door ───────────────────────────────────────────────────────────────

/// **Put this plan on the screen as the application's menu bar**, once.
///
/// Called from the moment there is an application to hang a bar off — after the
/// event loop has brought `NSApp` up and before the first window is on the
/// screen is early enough, and `bt-app` calls it from `resumed` for that reason.
///
/// A second call replaces the bar wholesale, which is what a plan whose *shape*
/// has changed needs; [`refresh`] is the cheap door for a plan whose shape is
/// the same.
///
/// # Errors
///
/// Off the main thread, where AppKit may not be touched at all. Everything else
/// about a menu is this process's own memory and cannot fail.
pub fn install(plan: &MenuPlan, send: MenuSender) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::macos_menu::install(plan, send)
    }
    #[cfg(not(target_os = "macos"))]
    {
        // **A no-op and not a refusal** (`portable_impl`'s class N). A platform
        // whose windows carry their own chrome has no application menu bar for
        // this to fail to install, and a launch that reported a fault here would
        // be reporting one about a thing that is not missing.
        let _ = (plan, send);
        Ok(())
    }
}

/// **Carry a new plan onto the bar that is already there** — new titles after a
/// language switch, new key equivalents after a rebind, new enabled flags after
/// the keyboard moved.
///
/// Cheap by construction: a plan equal to the one on the screen touches nothing,
/// so this is called from the event loop's settle chain on every turn. A plan
/// whose shape has changed is installed again rather than refreshed — see
/// [`MenuPlan::same_shape_as`].
///
/// # Errors
///
/// Off the main thread, and only there.
pub fn refresh(plan: &MenuPlan) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::macos_menu::refresh(plan)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = plan;
        Ok(())
    }
}

/// **Whether there is a bar on the screen for [`refresh`] to carry a plan onto.**
///
/// Asked by the application *before* it builds a plan, and that is the whole of
/// what it is for: deriving the bar costs a walk of the shortcut table and a
/// handful of small allocations, and the event loop's settle chain runs on every
/// turn. On a platform with no menu bar this is a constant `false` and the walk
/// never happens; before [`install`] has been through it is `false` for the same
/// reason, because there is nothing yet to carry a plan onto.
#[must_use]
pub fn is_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::macos_menu::is_installed()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// **The plan behind the bar that is on the screen**, or `None` where none has
/// been installed.
///
/// For the `.app` test that walks `NSApp.mainMenu`: it reads the real object out
/// of AppKit and this says which row of which menu each item is meant to be, so
/// a case can name what it is looking at instead of re-deriving the walk it is
/// checking. Nothing in the product reads it.
#[doc(hidden)]
#[must_use]
pub fn installed_plan() -> Option<MenuPlan> {
    #[cfg(target_os = "macos")]
    {
        crate::macos_menu::installed_plan()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(action: MenuAction) -> MenuEntry {
        MenuEntry::Item(MenuItem {
            title: "row",
            action,
            chord: None,
            enabled: true,
        })
    }

    fn plan(entries: Vec<MenuEntry>) -> MenuPlan {
        MenuPlan {
            menus: vec![MenuList {
                title: "menu",
                role: MenuRole::Plain,
                entries,
            }],
            dock: Vec::new(),
        }
    }

    /// PIN — every named key this menu can print has a character, and every one
    /// it cannot is honest about it.
    ///
    /// The function-key block is the arithmetic worth pinning: `F1` is
    /// `NSF1FunctionKey` and `F12` is eleven above it, and an off-by-one there
    /// would put `F12` on a key AppKit calls something else.
    #[test]
    fn a_named_key_prints_the_character_appkit_names_it_by() {
        assert_eq!(
            MenuNamedKey::ArrowUp.key_equivalent(),
            char::from_u32(0xF700)
        );
        assert_eq!(
            MenuNamedKey::Function(1).key_equivalent(),
            char::from_u32(0xF704)
        );
        assert_eq!(
            MenuNamedKey::Function(12).key_equivalent(),
            char::from_u32(0xF70F)
        );
        assert_eq!(MenuNamedKey::Escape.key_equivalent(), Some('\u{1b}'));
        assert_eq!(MenuNamedKey::Function(0).key_equivalent(), None);
        assert_eq!(MenuNamedKey::Function(13).key_equivalent(), None);
    }

    /// PIN (T-MAC-EDIT-CLIPBOARD) — **the two verbs that come up the responder
    /// chain are the two clipboard ones, and Help is not among them.**
    ///
    /// The list the platform adds a delegate method per entry for. Help is the
    /// counter-example that gives it its meaning: it is this application's verb
    /// just as much, and it reaches the application off a **row** of the bar
    /// with a target of Folio's own — so a Help that drifted onto this list
    /// would be a row asking AppKit's responder chain about a web page.
    ///
    /// MUTATION: add `Help` to the constant and this goes red.
    #[test]
    fn only_the_clipboard_verbs_come_up_the_responder_chain() {
        assert_eq!(
            AppMenuAction::THROUGH_THE_RESPONDER_CHAIN.to_vec(),
            vec![AppMenuAction::CopySelection, AppMenuAction::PasteIntoFocus]
        );
        assert!(
            !AppMenuAction::THROUGH_THE_RESPONDER_CHAIN.contains(&AppMenuAction::Help),
            "Help is a row of the bar with a target of its own, not a selector the chain declined"
        );
    }

    /// PIN — `rows` walks items only, in reading order, and skips the two
    /// entries that are not rows.
    ///
    /// It is the contract `refresh` finds an installed item again by, so a walk
    /// that counted a separator would put every title after the first rule on
    /// the wrong row.
    #[test]
    fn the_row_walk_skips_what_is_not_a_row() {
        let plan = plan(vec![
            row(MenuAction::Verb("one")),
            MenuEntry::Separator,
            MenuEntry::Services { title: "Services" },
            row(MenuAction::Verb("two")),
        ]);
        let actions: Vec<MenuAction> = plan.rows().map(|item| item.action).collect();
        assert_eq!(
            actions,
            vec![MenuAction::Verb("one"), MenuAction::Verb("two")]
        );
    }

    /// PIN — a plan that moved a title is refreshable; a plan that moved a row
    /// is not.
    ///
    /// MUTATION: compare titles in `same_shape_as` and the language switch
    /// starts rebuilding the whole bar instead of renaming it.
    #[test]
    fn only_a_plan_that_kept_its_rows_is_refreshable() {
        let one = plan(vec![row(MenuAction::Verb("one")), MenuEntry::Separator]);
        let mut renamed = one.clone();
        if let MenuEntry::Item(item) = &mut renamed.menus[0].entries[0] {
            item.title = "another name";
            item.enabled = false;
        }
        assert!(one.same_shape_as(&renamed));
        assert_ne!(one, renamed, "a renamed row is still a change");

        let reordered = plan(vec![MenuEntry::Separator, row(MenuAction::Verb("one"))]);
        assert!(!one.same_shape_as(&reordered));

        let other_verb = plan(vec![row(MenuAction::Verb("two")), MenuEntry::Separator]);
        assert!(!one.same_shape_as(&other_verb));

        let longer = plan(vec![
            row(MenuAction::Verb("one")),
            MenuEntry::Separator,
            row(MenuAction::Verb("two")),
        ]);
        assert!(!one.same_shape_as(&longer));
    }

    /// PIN (T-MAC-DOCKMENU) — **a plan that changed only its Dock rows is
    /// refreshable**, and it is still a change.
    ///
    /// The two halves are the whole of how a language switch reaches the Dock
    /// tile with no door of its own: `refresh` notices the plan is not equal, so
    /// it does not return early, and `same_shape_as` says the bar's own items
    /// may be carried over, so the new plan — Dock rows and all — is stored
    /// without rebuilding the bar. The next right-click reads it.
    ///
    /// MUTATION: compare `dock` in `same_shape_as` and every language switch
    /// starts rebuilding the whole menu bar; drop `dock` from the derived
    /// `PartialEq` and the switch never reaches the Dock at all.
    #[test]
    fn a_plan_that_changed_only_its_dock_rows_is_a_change_the_bar_can_carry() {
        let english = MenuPlan {
            dock: vec![DockRow {
                title: "New window",
                choice: MenuChoice::Verb("new-window"),
            }],
            ..plan(vec![row(MenuAction::Verb("one"))])
        };
        let chinese = MenuPlan {
            dock: vec![DockRow {
                title: "新建窗口",
                choice: MenuChoice::Verb("new-window"),
            }],
            ..english.clone()
        };
        assert_ne!(english, chinese, "a retitled Dock row is a change");
        assert!(
            english.same_shape_as(&chinese),
            "a Dock row's title is not a shape the bar's items are found by"
        );
        // And a row that names a different verb is still only a Dock change:
        // there is no item to find again, so there is nothing to rebuild.
        let other = MenuPlan {
            dock: vec![DockRow {
                title: "New tab",
                choice: MenuChoice::Verb("new-tab"),
            }],
            ..english.clone()
        };
        assert!(english.same_shape_as(&other));
        assert_eq!(
            english.rows().count(),
            1,
            "the row walk is the bar's and the Dock is not on it"
        );
    }

    /// PIN — off macOS the inbox is empty and the two doors answer `Ok`.
    ///
    /// The claim is `portable_impl`'s class N: there is no menu bar to fail to
    /// install, so a launch on a platform without one is not carrying a fault.
    /// A Dock tile is the same sentence one surface along: the rows travel in
    /// the plan, and a platform with no Dock simply never asks for them.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_platform_with_no_menu_bar_is_not_a_platform_with_a_broken_one() {
        let plan = MenuPlan {
            dock: vec![DockRow {
                title: "New window",
                choice: MenuChoice::Verb("new-window"),
            }],
            ..plan(vec![row(MenuAction::Verb("one"))])
        };
        assert_eq!(install(&plan, Box::new(|_, _| {})), Ok(()));
        assert_eq!(refresh(&plan), Ok(()));
        assert!(!is_installed());
        assert_eq!(installed_plan(), None);
    }
}
