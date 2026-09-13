//! **The application menu bar, over `NSMenu`** — the AppKit half of
//! [`crate::menu`] (ticket M3-2, `docs/DESIGN.md` §13.26).
//!
//! # Why this is a file of its own
//!
//! The same cut M2-3 made for the choosers, one shelf along. `macos_impl` is a
//! reading or a statement about a window that already exists; `macos_dialogs`
//! puts something in front of the reader and waits for them. This is neither: a
//! menu bar belongs to the **application** and outlives every window, it is
//! built once and then edited in place for the rest of the run, and the thing it
//! owns is a retained object graph rather than a gesture. It is also the one
//! file in this crate that installs an Objective-C **target** — an object AppKit
//! sends a message to whenever it likes — which is a lifetime of its own to
//! reason about.
//!
//! # The two halves, and the single rule between them
//!
//! [`install`] builds the whole bar from a [`MenuPlan`] and keeps every
//! `NSMenuItem` it made, in the plan's own row order. [`refresh`] walks that list
//! beside a new plan and sets three things per row — the title, the key
//! equivalent, the enabled flag. The rule joining them is
//! [`MenuPlan::same_shape_as`]: a plan that kept its rows is refreshed, and a
//! plan that moved one is installed again. Nothing here ever *guesses* which
//! item a row is; it is the same index in the same walk, and the walk is pinned
//! by a test in the portable module.
//!
//! # The target, the tag, and why there is only one of each
//!
//! Every Folio row on the bar shares **one** target object and **one** selector.
//! Which row was pressed is read off the sender's `tag`, which is that row's
//! index in the plan's walk, and the choice it stands for is looked up in the
//! same table `install` built. The alternative — an object per row, each holding
//! its own choice — is forty objects whose only difference is a field, and forty
//! chances for one of them to outlive the menu it was made for.
//!
//! # The Dock tile's menu, which is built and thrown away every time
//!
//! [`dock_menu`] is what `applicationDockMenu:` answers with (T-MAC-DOCKMENU,
//! `docs/DESIGN.md` §13.50). It is the second surface in this file and it is
//! arranged the other way round from the bar, because AppKit asks for it
//! differently: the bar is installed once and edited in place for the rest of
//! the run, and the Dock menu is **asked for on every right-click** and dropped
//! when the reader lets go. So nothing of it is retained here — no item list, no
//! shape to compare — and there is nothing to rebuild when the language changes,
//! only [`MenuPlan::dock`] to keep current, which [`refresh`] already does.
//!
//! What it does share is everything that makes a press safe: **the same target
//! object**, kept alive by the bar's own graph and therefore alive for as long
//! as any Dock menu it is named on, and **the same sender**, so a Dock row and a
//! bar row are parked in one inbox in the order they were pressed. The one thing
//! that differs is the selector — `folioDockChosen:` beside `folioMenuChosen:` —
//! because the two surfaces number their rows separately and the selector is the
//! honest place to say which table a `tag` is an index into.
//!
//! **Ownership.** `applicationDockMenu:` is not `alloc`, `new`, `copy` or
//! `mutableCopy`, so Cocoa's rule says the menu it answers with is **not owned
//! by the caller**: it is handed over autoreleased, through
//! [`Retained::autorelease_return`], and AppKit retains it for as long as the
//! menu is on the screen. Returning a `Retained` and forgetting it would leak
//! one menu per right-click; releasing it here instead would hand the Dock a
//! freed object, which is a crash in the Dock rather than in Folio.
//!
//! **The target is retained by this module and by the items**, and it is never
//! dropped while a menu that names it is on the screen: `install` replaces the
//! whole graph at once, items and target together, so there is no window in
//! which AppKit holds a pointer to an object this module has let go of.
//!
//! # What an action is allowed to do
//!
//! One statement: hand the choice to the sender. See [`crate::menu`]'s header
//! for why — X-4's rule, that nothing is executed inside an AppKit callback,
//! and the fact that a menu action runs with AppKit's own tracking still on the
//! stack. In particular **`Quit` is a parked choice like any other and this file
//! never sends `terminate:`**: the product's quit writes a session document, and
//! the one thing X-3 measured about AppKit's own Quit item is that it ends the
//! process past it.
//!
//! # Everything here is the main thread's
//!
//! AppKit's, as everywhere in this crate, and the gate is `macos_impl`'s own
//! [`window_thread`] rather than a second spelling of it. The installed graph
//! lives in a **thread-local**, which is not a shortcut around `Send`: a
//! `Retained<NSMenuItem>` may not leave this thread at all, and a `static` would
//! be a promise that it may.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{AnyThread, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::macos_impl::window_thread;
use crate::menu::{
    MenuAction, MenuChoice, MenuChord, MenuEntry, MenuKey, MenuList, MenuPlan, MenuRole,
    MenuSender, MenuSurface, StandardMenuAction,
};

// ── the target AppKit sends to ─────────────────────────────────────────────

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class declares no ivars and implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "FolioMenuTarget"]
    #[ivars = ()]
    struct MenuTarget;

    impl MenuTarget {
        /// **One press on one Folio row**, parked for the loop's next turn.
        ///
        /// A name of Folio's own, prefixed, for `folioPopFormulaMenu`'s reason:
        /// this selector is added to a class this program defines, and a plain
        /// `chosen:` would be a name the Objective-C runtime shares with
        /// everything else that ever thought of the word.
        #[unsafe(method(folioMenuChosen:))]
        fn chosen(&self, sender: Option<&NSMenuItem>) {
            let Some(sender) = sender else {
                // AppKit always sends the item; a nil sender is a message this
                // object did not put on any menu.
                return;
            };
            let tag = sender.tag();
            let Ok(tag) = usize::try_from(tag) else {
                return;
            };
            INSTALLED.with(|cell| {
                let Ok(borrowed) = cell.try_borrow() else {
                    return;
                };
                let Some(installed) = borrowed.as_ref() else {
                    return;
                };
                // A tag with no choice behind it is a standard row, which this
                // target is never set on, or a row of a bar that has been
                // replaced. Neither is a fault and neither is sent anywhere.
                let Some(Some(choice)) = installed.choices.get(tag).copied() else {
                    return;
                };
                (installed.send)(MenuSurface::Bar, choice);
            });
        }

        /// **One press on one row of the Dock tile's menu**, parked for the
        /// loop's next turn (T-MAC-DOCKMENU).
        ///
        /// A selector of its own beside `folioMenuChosen:` rather than a tag
        /// range inside it: the two surfaces number their rows separately — the
        /// bar's tag is an index into `choices`, this one an index into
        /// [`MenuPlan::dock`] — and which table a number is an index into is
        /// exactly the kind of thing a name should say rather than a comment.
        ///
        /// It is the same object, the same borrow and the same one statement:
        /// hand the choice to the sender the application installed, saying which
        /// surface it came from.
        #[unsafe(method(folioDockChosen:))]
        fn dock_chosen(&self, sender: Option<&NSMenuItem>) {
            let Some(sender) = sender else {
                return;
            };
            let Ok(tag) = usize::try_from(sender.tag()) else {
                return;
            };
            INSTALLED.with(|cell| {
                let Ok(borrowed) = cell.try_borrow() else {
                    return;
                };
                let Some(installed) = borrowed.as_ref() else {
                    return;
                };
                // A tag with no row behind it is a row of a plan that has been
                // replaced since the reader opened the menu — the language
                // switched under an open Dock menu. Not a fault, and not sent.
                let Some(row) = installed.plan.dock.get(tag) else {
                    return;
                };
                (installed.send)(MenuSurface::Dock, row.choice);
            });
        }
    }

    unsafe impl NSObjectProtocol for MenuTarget {}
);

impl MenuTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: `NSObject`'s designated initializer, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }
}

// ── what is on the screen right now ────────────────────────────────────────

/// The bar this module built, and everything it needs to edit it again.
struct Installed {
    /// The plan the bar on the screen was built from — what [`refresh`] compares
    /// against, so that an unchanged plan costs one comparison and no AppKit at
    /// all.
    plan: MenuPlan,
    /// Every Folio row and every standard row, in [`MenuPlan::rows`]' order.
    items: Vec<Retained<NSMenuItem>>,
    /// What each of those rows asks for, indexed by the tag it carries.
    /// `None` for a row AppKit answers by itself.
    choices: Vec<Option<MenuChoice>>,
    /// Kept alive for as long as the items that name it — **and for as long as
    /// any Dock menu built out of this plan**, which is the second reason it is
    /// held here rather than by the items. `-[NSMenuItem target]` does not own
    /// what it points at; the Dock menu is thrown away on every right-click and
    /// this object is not.
    target: Retained<MenuTarget>,
    send: MenuSender,
}

thread_local! {
    /// **The main thread's**, and that is the type saying so. A
    /// `Retained<NSMenuItem>` is not `Send`, and a `static` holding one would be
    /// a claim that it is.
    static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
}

// ── building ───────────────────────────────────────────────────────────────

/// The AppKit selector one of the standard rows sends.
///
/// `None` for [`StandardMenuAction::Redo`], whose selector is not one AppKit
/// declares on `NSResponder` — `redo:` belongs to `NSUndoManager`'s own
/// responder category and is reached by name like the rest. It is here as a
/// `Sel` all the same; the `None` arm does not exist.
fn standard_selector(action: StandardMenuAction) -> Sel {
    match action {
        StandardMenuAction::AboutPanel => sel!(orderFrontStandardAboutPanel:),
        StandardMenuAction::Hide => sel!(hide:),
        StandardMenuAction::HideOthers => sel!(hideOtherApplications:),
        StandardMenuAction::ShowAll => sel!(unhideAllApplications:),
        StandardMenuAction::Undo => sel!(undo:),
        StandardMenuAction::Redo => sel!(redo:),
        StandardMenuAction::Cut => sel!(cut:),
        StandardMenuAction::Copy => sel!(copy:),
        StandardMenuAction::Paste => sel!(paste:),
        StandardMenuAction::SelectAll => sel!(selectAll:),
        StandardMenuAction::CloseWindow => sel!(performClose:),
        StandardMenuAction::Minimize => sel!(performMiniaturize:),
        StandardMenuAction::ZoomWindow => sel!(performZoom:),
        StandardMenuAction::BringAllToFront => sel!(arrangeInFront:),
    }
}

/// The choice a row carries back, or `None` for a row AppKit answers itself.
const fn choice_of(action: MenuAction) -> Option<MenuChoice> {
    match action {
        MenuAction::Verb(id) => Some(MenuChoice::Verb(id)),
        MenuAction::Application(what) => Some(MenuChoice::Application(what)),
        MenuAction::Standard(_) => None,
    }
}

/// The modifier mask a chord is held with.
fn modifier_mask(chord: &MenuChord) -> NSEventModifierFlags {
    let mut mask = NSEventModifierFlags(0);
    if chord.command {
        mask |= NSEventModifierFlags::Command;
    }
    if chord.shift {
        mask |= NSEventModifierFlags::Shift;
    }
    if chord.option {
        mask |= NSEventModifierFlags::Option;
    }
    if chord.control {
        mask |= NSEventModifierFlags::Control;
    }
    mask
}

/// The character a chord's key is printed by, or `None` for a key this bar
/// cannot print — see [`crate::menu::MenuNamedKey`].
fn key_equivalent(chord: &MenuChord) -> Option<String> {
    match &chord.key {
        // **Lower-cased, with `Shift` left in the mask.** The two spellings of a
        // shifted key equivalent — an upper-case character with no Shift flag,
        // or a lower-case one with it — are the same chord to AppKit, and this
        // one is the same chord to a reader of `BINDINGS`: the table writes the
        // character printed on the key, and the modifiers beside it.
        MenuKey::Character(text) if !text.is_empty() => Some(text.to_lowercase()),
        MenuKey::Character(_) => None,
        MenuKey::Named(named) => named.key_equivalent().map(String::from),
    }
}

/// Write a row's three changeable properties onto its item.
fn dress(item: &NSMenuItem, title: &str, chord: Option<&MenuChord>, enabled: bool) {
    item.setTitle(&NSString::from_str(title));
    match chord.and_then(|chord| key_equivalent(chord).map(|key| (chord, key))) {
        Some((chord, key)) => {
            item.setKeyEquivalent(&NSString::from_str(&key));
            item.setKeyEquivalentModifierMask(modifier_mask(chord));
        }
        // The empty string is AppKit's own spelling of "this row has no key",
        // and it has to be written rather than left alone: a row whose chord the
        // reader has just taken away keeps the old one otherwise.
        None => {
            item.setKeyEquivalent(&NSString::from_str(""));
            item.setKeyEquivalentModifierMask(NSEventModifierFlags(0));
        }
    }
    item.setEnabled(enabled);
}

/// A fresh `NSMenu` with **automatic enabling off**.
///
/// Off on every menu in this bar, and it is the ticket's own rule rather than a
/// convenience: with it on, AppKit decides a row's enabled state by asking the
/// responder chain, and this product's answer to "is this row in force" is
/// `Scope::holds` on the window that has the keyboard — a question no responder
/// can be asked. See [`crate::menu`]'s header for what the flag is load-bearing
/// for: a **disabled row does not answer its key equivalent**, so the enabled
/// state is also what hands a chord back to the terminal.
fn menu_named(title: &str, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.setTitle(&NSString::from_str(title));
    menu.setAutoenablesItems(false);
    menu
}

/// Build the bar and hang it on the application.
fn build(plan: &MenuPlan, send: MenuSender, mtm: MainThreadMarker) -> Installed {
    let app = NSApplication::sharedApplication(mtm);
    let target = MenuTarget::new();
    let bar = menu_named("", mtm);
    let mut items: Vec<Retained<NSMenuItem>> = Vec::new();
    let mut choices: Vec<Option<MenuChoice>> = Vec::new();
    let mut services: Option<Retained<NSMenu>> = None;
    let mut windows: Option<Retained<NSMenu>> = None;

    for list in &plan.menus {
        let (holder, menu) = new_submenu(list, mtm);
        for entry in &list.entries {
            match entry {
                MenuEntry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                MenuEntry::Services { title } => {
                    let item = NSMenuItem::new(mtm);
                    item.setTitle(&NSString::from_str(title));
                    let submenu = menu_named(title, mtm);
                    // The system fills this one; `autoenablesItems` is left as
                    // AppKit set it above for the reason every other menu here
                    // turns it off — Folio decides nothing about these rows.
                    item.setSubmenu(Some(&submenu));
                    menu.addItem(&item);
                    services = Some(submenu);
                }
                MenuEntry::Item(row) => {
                    let item = NSMenuItem::new(mtm);
                    let tag = items.len();
                    item.setTag(tag as isize);
                    dress(&item, row.title, row.chord.as_ref(), row.enabled);
                    match row.action {
                        MenuAction::Standard(what) => {
                            // **No target, so the responder chain answers.**
                            // That is the whole of what a standard row is: a
                            // text field in a sheet keeps its own Copy, and a
                            // window keeps its own Minimize, without this
                            // process hearing about either.
                            // SAFETY: the selector is one of AppKit's own,
                            // named above, and a nil target is what sends it
                            // down the responder chain.
                            unsafe {
                                item.setAction(Some(standard_selector(what)));
                                item.setTarget(None);
                            }
                        }
                        MenuAction::Verb(_) | MenuAction::Application(_) => {
                            // SAFETY: the selector is the one `MenuTarget`
                            // defines above, and the target is that object,
                            // retained by this graph for as long as the item is.
                            unsafe {
                                item.setAction(Some(sel!(folioMenuChosen:)));
                                item.setTarget(Some(&*target as &AnyObject));
                            }
                        }
                    }
                    menu.addItem(&item);
                    items.push(item);
                    choices.push(choice_of(row.action));
                }
            }
        }
        if list.role == MenuRole::Windows {
            windows = Some(menu.clone());
        }
        bar.addItem(&holder);
    }

    app.setMainMenu(Some(&bar));
    // **Said after the bar is installed, and said even when there is nothing to
    // say.** AppKit keeps its own pointer to each of these; a bar rebuilt with
    // no Services row would otherwise leave the application pointing at a menu
    // that is no longer on it.
    app.setServicesMenu(services.as_deref());
    app.setWindowsMenu(windows.as_deref());

    Installed {
        plan: plan.clone(),
        items,
        choices,
        target,
        send,
    }
}

/// **The menu the Dock tile shows above AppKit's own rows**, built here and now
/// (T-MAC-DOCKMENU).
///
/// Called by `applicationDockMenu:` and by nothing else. It answers a raw
/// pointer rather than a `Retained` because that is what the Objective-C method
/// it stands behind returns and what Cocoa's rule makes it: **autoreleased, not
/// owned by AppKit**, which retains it while the menu is open and lets it go
/// with the pool. See this module's header.
///
/// `null` — AppKit's nil, and its own menu unchanged — in four cases, none of
/// them a fault: off the main thread, which cannot happen because AppKit asks;
/// while the graph is borrowed, which is a right-click during an install;
/// before the bar was installed at all, which is a right-click during launch;
/// and for a plan whose Dock rows are empty, which is an application that offers
/// nothing of its own.
pub(crate) fn dock_menu() -> *mut NSMenu {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::ptr::null_mut();
    };
    INSTALLED.with(|cell| {
        let Ok(borrowed) = cell.try_borrow() else {
            return std::ptr::null_mut();
        };
        let Some(installed) = borrowed.as_ref() else {
            return std::ptr::null_mut();
        };
        if installed.plan.dock.is_empty() {
            return std::ptr::null_mut();
        }
        // Titled with nothing: the Dock draws no title over this menu, and the
        // rows AppKit adds below are not in it. `menu_named` is still the door,
        // for the flag it turns off — with automatic enabling on, AppKit would
        // ask a responder chain that a reader in another application does not
        // have, and grey every row of it.
        let menu = menu_named("", mtm);
        for (tag, row) in installed.plan.dock.iter().enumerate() {
            let item = NSMenuItem::new(mtm);
            item.setTag(tag as isize);
            dress(&item, row.title, None, true);
            // SAFETY: the selector is the one `MenuTarget` defines above, and
            // the target is that object — retained by the installed graph this
            // very borrow is reading, which outlives the menu being built.
            unsafe {
                item.setAction(Some(sel!(folioDockChosen:)));
                item.setTarget(Some(&*installed.target as &AnyObject));
            }
            menu.addItem(&item);
        }
        Retained::autorelease_return(menu)
    })
}

/// The holder item on the bar, and the menu hanging off it.
///
/// A menu on the bar is two objects: an `NSMenuItem` with no action at all, and
/// the `NSMenu` that is its submenu. The item's own title is never drawn — the
/// submenu's is — which is why the title is written on both rather than argued
/// about.
fn new_submenu(list: &MenuList, mtm: MainThreadMarker) -> (Retained<NSMenuItem>, Retained<NSMenu>) {
    let holder = NSMenuItem::new(mtm);
    holder.setTitle(&NSString::from_str(list.title));
    let menu = menu_named(list.title, mtm);
    holder.setSubmenu(Some(&menu));
    (holder, menu)
}

// ── the two doors ──────────────────────────────────────────────────────────

/// See [`crate::menu::install`].
pub(crate) fn install(plan: &MenuPlan, send: MenuSender) -> Result<(), String> {
    let mtm = window_thread("installing the menu bar")?;
    let built = build(plan, send, mtm);
    INSTALLED.with(|cell| *cell.borrow_mut() = Some(built));
    Ok(())
}

/// See [`crate::menu::refresh`].
pub(crate) fn refresh(plan: &MenuPlan) -> Result<(), String> {
    let mtm = window_thread("refreshing the menu bar")?;
    // **A rebuild needs the sender back**, and the only place it exists is the
    // graph being replaced — so the decision and the extraction are one borrow,
    // and the rebuild itself happens with nothing borrowed. `build` reaches this
    // same cell through the target's action only when a reader presses a row,
    // which cannot happen inside this function.
    enum Next {
        Nothing,
        Rebuild(MenuSender),
    }
    let next = INSTALLED.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let Some(installed) = borrowed.as_mut() else {
            // Nothing is on the screen, so there is nothing to carry a plan
            // onto. `install` is the door for that and it has not been through.
            return Next::Nothing;
        };
        if installed.plan == *plan {
            return Next::Nothing;
        }
        if !installed.plan.same_shape_as(plan) {
            let old = borrowed.take().expect("checked just above");
            return Next::Rebuild(old.send);
        }
        for (item, row) in installed.items.iter().zip(plan.rows()) {
            dress(item, row.title, row.chord.as_ref(), row.enabled);
        }
        installed.plan = plan.clone();
        Next::Nothing
    });
    if let Next::Rebuild(send) = next {
        let built = build(plan, send, mtm);
        INSTALLED.with(|cell| *cell.borrow_mut() = Some(built));
    }
    Ok(())
}

/// See [`crate::menu::is_installed`].
pub(crate) fn is_installed() -> bool {
    INSTALLED.with(|cell| cell.borrow().is_some())
}

/// See [`crate::menu::installed_plan`].
pub(crate) fn installed_plan() -> Option<MenuPlan> {
    INSTALLED.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|installed| installed.plan.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{AppMenuAction, MenuNamedKey};

    /// PIN — a chord becomes a key equivalent AppKit can read: the character
    /// lower-cased, the modifiers in the mask.
    ///
    /// MUTATION: drop the `to_lowercase` and `Cmd+Shift+T` prints `T` with a
    /// Shift flag beside it, which AppKit draws as `⇧⇧T`.
    #[test]
    fn a_chord_is_the_character_and_the_mask() {
        let chord = MenuChord {
            command: true,
            shift: true,
            option: false,
            control: false,
            key: MenuKey::Character("T".to_owned()),
        };
        assert_eq!(key_equivalent(&chord).as_deref(), Some("t"));
        let mask = modifier_mask(&chord);
        assert!(mask.contains(NSEventModifierFlags::Command));
        assert!(mask.contains(NSEventModifierFlags::Shift));
        assert!(!mask.contains(NSEventModifierFlags::Option));
        assert!(!mask.contains(NSEventModifierFlags::Control));
    }

    /// PIN — a named key this bar cannot print gives the row no key equivalent
    /// rather than a wrong one.
    #[test]
    fn a_key_with_no_glyph_leaves_the_row_bare() {
        let bare = MenuChord {
            command: true,
            shift: false,
            option: false,
            control: false,
            key: MenuKey::Named(MenuNamedKey::Function(99)),
        };
        assert_eq!(key_equivalent(&bare), None);
        let empty = MenuChord {
            key: MenuKey::Character(String::new()),
            ..bare
        };
        assert_eq!(key_equivalent(&empty), None);
    }

    /// PIN — a standard row carries no choice, so nothing this process hears
    /// about can come from one.
    #[test]
    fn only_folios_own_rows_come_back_on_the_channel() {
        assert_eq!(
            choice_of(MenuAction::Verb("new-tab")),
            Some(MenuChoice::Verb("new-tab"))
        );
        assert_eq!(
            choice_of(MenuAction::Application(AppMenuAction::Help)),
            Some(MenuChoice::Application(AppMenuAction::Help))
        );
        assert_eq!(
            choice_of(MenuAction::Standard(StandardMenuAction::Copy)),
            None
        );
    }

    /// PIN (T-MAC-DOCKMENU) — **the Dock menu leaves this file autoreleased.**
    ///
    /// `applicationDockMenu:` is not a method whose name gives its caller
    /// ownership, so AppKit does not release what it is handed. A menu returned
    /// with its retain count still this module's is one leaked menu per
    /// right-click; a menu released on the way out is a freed object in the
    /// Dock's hands. The one spelling that is neither is
    /// `Retained::autorelease_return`, and it is asserted in the source text
    /// because the two wrong answers both compile.
    ///
    /// MUTATION: hand the menu over with its retain count still this module's —
    /// `into_raw` in place of the call below — and this goes red.
    ///
    /// **Both needles are spelled through `concat!`, and the mutation above is
    /// not spelled in full**, for `main.rs`' `launch_landing_tests`' reason and
    /// it is not a style: a needle written as one literal is in this file too,
    /// so the first assertion would pass on its own text however the menu is
    /// actually returned, and the second would fail on its own text however
    /// careful the code is. Both were measured doing exactly that on 2026-09-13,
    /// which is why they are written this way.
    #[test]
    fn the_dock_menu_is_handed_over_autoreleased() {
        let source = include_str!("macos_menu.rs");
        assert!(
            source.contains(concat!("Retained::", "autorelease_return(menu)")),
            "the Dock menu is not handed over autoreleased"
        );
        assert!(
            !source.contains(concat!("Retained::", "into_raw")),
            "a menu whose retain count is still this module's leaks once per right-click"
        );
    }

    /// PIN (X-4's rule) — **this file never sends `terminate:`.**
    ///
    /// The product's quit writes a session document and files every window in
    /// Recent; AppKit's own Quit item ends the process past all of it, which is
    /// what X-3 measured and what `with_default_menu(false)` refuses in `bt-app`.
    /// The menu coming back must not bring it with it.
    ///
    /// MUTATION: add a `terminate:` arm to `standard_selector` and this goes red.
    #[test]
    fn the_menu_bar_does_not_end_the_process_where_it_stands() {
        let source = include_str!("macos_menu.rs");
        assert!(
            !source.contains(concat!("sel!(", "terminate:)")),
            "the menu bar sends AppKit's own terminate:, which quits past the session document"
        );
    }
}
