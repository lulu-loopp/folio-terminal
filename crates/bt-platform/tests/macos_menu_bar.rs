//! **A real `NSMenu` on a real `NSApplication`, answering a real key
//! equivalent** — the claims M3-2 makes that no Windows runner can check
//! (ticket M3-2, `docs/DESIGN.md` §13.26).
//!
//! # Why this is a target of its own rather than a `#[test]`
//!
//! `tests/macos_sheet.rs`'s reason, measured on the same toolchain and worth
//! restating because it is the whole of the file's shape: **AppKit is the main
//! thread's and libtest does not give a case the main thread.** A case run with
//! `--test-threads=1` still executes on a thread libtest spawned, where
//! `MainThreadMarker::new()` is `None` — and `bt_platform::menu::install` refuses
//! exactly there, by design. `harness = false` hands this file the process's own
//! `main`, which is the main thread, and every claim below follows from that.
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing, and the gate is the one the sheet probe already has: without
//! **`BT_MAC_GUI`** this binary prints one line and exits, and off macOS it has
//! no body at all. No new `BT_…` name is introduced — see `docs/BT-ENVIRONMENT.md`
//! §4 for why the names in `tests/` are outside that document's walk.
//!
//! **No window is opened and no key is posted.** A menu bar is the
//! application's rather than a window's, and the press is delivered by handing
//! `-[NSMenu performKeyEquivalent:]` an `NSEvent` this file makes — which is
//! what AppKit itself does one layer up, and which needs no Accessibility grant
//! and no window server interaction beyond the application existing.
//!
//! # What it proves, in order
//!
//! ① the bar AppKit is holding **is the plan** — five menus, in order, with
//!    the titles the plan asked for, the plan's own rows at the front of each,
//!    and `autoenablesItems` off on every one of them. *At the front* rather
//!    than *all of them*: AppKit appends rows of its own to a menu titled
//!    `Edit` and to the one it was handed as `windowsMenu`, and the case prints
//!    what it added rather than pretending it did not;
//! ② the Services submenu and the Window menu are the ones AppKit was **handed**
//!    (`NSApp.servicesMenu`, `NSApp.windowsMenu`), so the window list is the
//!    system's;
//! ③ every key equivalent on a row of **this product's** is the plan's chord —
//!    the character and the modifier mask, item by item, and a row the plan
//!    gave none prints none. A row AppKit *answers* may also be a row AppKit
//!    *keys* — it puts `Cmd+M` on `performMiniaturize:` in the menu it manages
//!    — and the case prints those rather than forbidding them;
//! ④ **`⌘T` produces exactly one `NewTab` on the channel**: `performKeyEquivalent:`
//!    answers `YES` and the sender is handed one `Verb("new-tab")` and then
//!    nothing;
//! ⑤ **what a greyed row does with its key**: `⌘S` on a greyed Save sends
//!    nothing — and is still *answered*, so the press is swallowed rather than
//!    handed to `keyDown:`. That measurement is why `bt_app::menubar` prints a
//!    key beside a row only when it is in force everywhere;
//! ⑥ **`⌘Q` is a choice on the channel and not `terminate:`** — the process is
//!    still running on the next line;
//! ⑦ a refresh re-greys a row **in place**: the same item objects, still
//!    answering, with no rebuild.

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::{Mutex, PoisonError};

    use bt_platform::menu::{
        AppMenuAction, MenuAction, MenuChoice, MenuChord, MenuEntry, MenuItem, MenuKey, MenuList,
        MenuPlan, MenuRole, StandardMenuAction,
    };
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSMenu};
    use objc2_foundation::{NSPoint, NSString};

    /// **Everything the sender this file installed has been handed**, in order.
    ///
    /// The sender is what `bt-app`'s `menu_wire` is: it keeps the choice and
    /// posts a wake. Here it only keeps, because there is no loop to wake — the
    /// claim being checked is what AppKit hands over and when.
    static SENT: Mutex<Vec<MenuChoice>> = Mutex::new(Vec::new());

    fn keep(choice: MenuChoice) {
        SENT.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(choice);
    }

    /// Everything sent since this was last called.
    fn taken() -> Vec<MenuChoice> {
        std::mem::take(&mut *SENT.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The owner's consent, and the main thread this file is written for.
    fn asked_for(name: &str) -> Option<MainThreadMarker> {
        if std::env::var_os("BT_MAC_GUI").is_none() {
            println!("{name}: skipped — set BT_MAC_GUI to touch AppKit");
            return None;
        }
        let mtm = MainThreadMarker::new().expect(
            "this target is `harness = false` precisely so that it owns the main thread, and it \
             is not on it",
        );
        static LAUNCHED: std::sync::Once = std::sync::Once::new();
        LAUNCHED.call_once(|| NSApplication::sharedApplication(mtm).finishLaunching());
        Some(mtm)
    }

    fn command(key: &str) -> MenuChord {
        MenuChord {
            command: true,
            shift: false,
            option: false,
            control: false,
            key: MenuKey::Character(key.to_owned()),
        }
    }

    fn verb(
        title: &'static str,
        id: &'static str,
        chord: Option<MenuChord>,
        enabled: bool,
    ) -> MenuEntry {
        MenuEntry::Item(MenuItem {
            title,
            action: MenuAction::Verb(id),
            chord,
            enabled,
        })
    }

    fn standard(title: &'static str, what: StandardMenuAction) -> MenuEntry {
        MenuEntry::Item(MenuItem {
            title,
            action: MenuAction::Standard(what),
            chord: None,
            enabled: true,
        })
    }

    /// **A plan with the same shape as the product's**, small enough to assert
    /// every row of.
    ///
    /// The real bar is `bt_app::menubar`, which this crate cannot see and does
    /// not need to: what is checked here is that a plan becomes the `NSMenu` it
    /// says it is, and the pin holding the product's plan to `BINDINGS` runs on
    /// a Windows workstation (`menubar::every_chord_on_the_bar_is_the_tables_mac_column`).
    fn a_plan(save_is_in_force: bool) -> MenuPlan {
        MenuPlan {
            menus: vec![
                MenuList {
                    title: "Folio",
                    role: MenuRole::Application,
                    entries: vec![
                        standard("About Folio", StandardMenuAction::AboutPanel),
                        MenuEntry::Separator,
                        verb("Settings", "open-settings", Some(command(",")), true),
                        MenuEntry::Services { title: "Services" },
                        MenuEntry::Separator,
                        verb("Quit Folio", "quit", Some(command("q")), true),
                    ],
                },
                MenuList {
                    title: "File",
                    role: MenuRole::Plain,
                    entries: vec![
                        verb("New tab", "new-tab", Some(command("t")), true),
                        verb("Save", "save-preview", Some(command("s")), save_is_in_force),
                        verb("Close pane", "close-pane", Some(command("w")), true),
                    ],
                },
                MenuList {
                    title: "Edit",
                    role: MenuRole::Plain,
                    entries: vec![standard("Copy", StandardMenuAction::Copy)],
                },
                MenuList {
                    title: "Window",
                    role: MenuRole::Windows,
                    entries: vec![standard("Minimize", StandardMenuAction::Minimize)],
                },
                MenuList {
                    title: "Help",
                    role: MenuRole::Plain,
                    entries: vec![MenuEntry::Item(MenuItem {
                        title: "Folio Help",
                        action: MenuAction::Application(AppMenuAction::Help),
                        chord: None,
                        enabled: true,
                    })],
                },
            ],
        }
    }

    /// A `⌘`-modified press of one character, as AppKit would deliver it.
    ///
    /// **The event object and not a posted key.** `performKeyEquivalent:` is
    /// what `NSWindow` calls on the main menu before it hands anything to
    /// `keyDown:`, and it takes the event as an argument — so a press can be put
    /// to the bar directly, with no `CGEvent`, no window and no Accessibility
    /// grant. `charactersIgnoringModifiers` is what AppKit compares against a
    /// key equivalent, and it is the same string here as `characters` because a
    /// Command chord produces the bare character anyway.
    fn a_command_press(key: &str) -> objc2::rc::Retained<NSEvent> {
        let characters = NSString::from_str(key);
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::Command,
            0.0,
            0,
            None,
            &characters,
            &characters,
            false,
            0,
        )
        .expect("AppKit builds a key event out of its own arguments")
    }

    /// The bar AppKit is holding.
    fn the_bar(mtm: MainThreadMarker) -> objc2::rc::Retained<NSMenu> {
        NSApplication::sharedApplication(mtm)
            .mainMenu()
            .expect("the application is holding a main menu")
    }

    /// The rows of one menu on the bar, by its index.
    fn submenu(bar: &NSMenu, index: isize) -> objc2::rc::Retained<NSMenu> {
        bar.itemAtIndex(index)
            .expect("the bar has a menu at this index")
            .submenu()
            .expect("every item on the bar carries a submenu")
    }

    fn run() {
        let name = "the_menu_bar_is_the_plan_and_answers_one_chord_once";
        let Some(mtm) = asked_for(name) else {
            return;
        };
        let plan = a_plan(false);
        bt_platform::menu::install(&plan, Box::new(keep))
            .expect("the bar installs on the main thread");
        assert!(bt_platform::menu::is_installed());
        assert_eq!(bt_platform::menu::installed_plan().as_ref(), Some(&plan));

        // ① the bar is the plan.
        let bar = the_bar(mtm);
        assert_eq!(
            bar.numberOfItems(),
            plan.menus.len() as isize,
            "the bar does not have the plan's menus"
        );
        for (index, list) in plan.menus.iter().enumerate() {
            let menu = submenu(&bar, index as isize);
            assert_eq!(
                menu.title().to_string(),
                list.title,
                "menu {index} is titled wrong"
            );
            // **At least, and the plan's rows first.** Measured on the Mac,
            // 2026-09-12: AppKit appends rows of its own to a menu titled
            // `Edit` (Writing Tools, AutoFill, Emoji & Symbols, Start
            // Dictation and the rules between them) and to the menu it was
            // handed as `windowsMenu`, where the list of open windows goes.
            // That is the system's half of those two menus and Folio does not
            // ask for it to stop; what the plan promises is its **own** rows,
            // in its own order, at the front. `refresh` is unaffected either
            // way: it walks the items this module retained rather than the
            // live menu, which is exactly why it can.
            let planned = list.entries.len() as isize;
            assert!(
                menu.numberOfItems() >= planned,
                "menu {index} lost rows the plan asked for: {} < {planned}",
                menu.numberOfItems()
            );
            if menu.numberOfItems() > planned {
                let added: Vec<String> = (planned..menu.numberOfItems())
                    .filter_map(|at| menu.itemAtIndex(at))
                    .map(|item| item.title().to_string())
                    .collect();
                println!("  AppKit added to {}: {added:?}", list.title);
            }
            assert!(
                !menu.autoenablesItems(),
                "menu {index} enables its own rows, and this product's answer to \
                 'is this row in force' is a scope no responder can be asked"
            );
        }

        // ② the two menus AppKit was handed.
        let app = NSApplication::sharedApplication(mtm);
        let services = app
            .servicesMenu()
            .expect("AppKit was handed a Services menu");
        assert_eq!(services.title().to_string(), "Services");
        let windows = app.windowsMenu().expect("AppKit was handed a Window menu");
        assert_eq!(windows.title().to_string(), "Window");

        // ③ every key equivalent is the plan's chord.
        for (index, list) in plan.menus.iter().enumerate() {
            let menu = submenu(&bar, index as isize);
            for (row, entry) in list.entries.iter().enumerate() {
                let MenuEntry::Item(wanted) = entry else {
                    continue;
                };
                let item = menu.itemAtIndex(row as isize).expect("the row is there");
                assert_eq!(item.title().to_string(), wanted.title);
                assert_eq!(
                    item.isEnabled(),
                    wanted.enabled,
                    "{} is mis-greyed",
                    wanted.title
                );
                let printed = item.keyEquivalent().to_string();
                match &wanted.chord {
                    // **A row AppKit answers may also be a row AppKit keys.**
                    // Measured 2026-09-12: the plan writes no key equivalent on
                    // `Minimize`, and the bar comes back with `Cmd+M` on it,
                    // because the menu handed over as `windowsMenu` is the
                    // system's to manage and `performMiniaturize:` is one of the
                    // selectors it manages. That is the right answer and the
                    // reason `BINDINGS` has no row for it; what this case holds
                    // is Folio's own claim, which is about Folio's own rows.
                    None if matches!(wanted.action, MenuAction::Standard(_)) => {
                        if !printed.is_empty() {
                            println!("  AppKit keyed {}: {printed:?}", wanted.title);
                        }
                    }
                    None => assert!(
                        printed.is_empty(),
                        "{} is a verb of this product's and prints a key the plan did not give it",
                        wanted.title
                    ),
                    Some(chord) => {
                        let MenuKey::Character(key) = &chord.key else {
                            unreachable!("this plan prints only characters")
                        };
                        assert_eq!(&printed, key, "{} prints the wrong key", wanted.title);
                        let mask = item.keyEquivalentModifierMask();
                        assert!(
                            mask.contains(NSEventModifierFlags::Command),
                            "{} is not held with Command",
                            wanted.title
                        );
                        assert_eq!(
                            mask.contains(NSEventModifierFlags::Shift),
                            chord.shift,
                            "{} disagrees about Shift",
                            wanted.title
                        );
                    }
                }
            }
        }

        // ④ one chord, one verb, once.
        let _ = taken();
        assert!(
            bar.performKeyEquivalent(&a_command_press("t")),
            "⌘T was not answered by the bar"
        );
        assert_eq!(
            taken(),
            vec![MenuChoice::Verb("new-tab")],
            "⌘T did not send exactly one NewTab"
        );
        assert!(taken().is_empty(), "the choice was handed over twice");

        // ⑤ **what a disabled row really does with its key equivalent**, and it
        // is not what the first draft of this ticket assumed.
        //
        // Measured here on 2026-09-12 and asserted from then on, because the
        // whole of `menubar`'s rule about which rows print a key rests on it: a
        // greyed row does **not** run its verb, and `performKeyEquivalent:`
        // still answers `YES` for it — so the press is *swallowed* rather than
        // handed on to the key window's `keyDown:`. `Cmd+D` is the control: no
        // row of this plan carries it, and that is what "this bar does not
        // claim that key" looks like.
        //
        // The day AppKit changes its mind, this goes red and
        // `docs/DESIGN.md` §13.26 ② can be revisited.
        let control = bar.performKeyEquivalent(&a_command_press("d"));
        let control_sent = taken();
        let answered = bar.performKeyEquivalent(&a_command_press("s"));
        let sent = taken();
        println!("  MEASURED unclaimed Cmd+D: answered={control} sent={control_sent:?}");
        println!("  MEASURED greyed Cmd+S: answered={answered} sent={sent:?}");
        assert!(!control, "a chord no row carries was answered by the bar");
        assert!(
            control_sent.is_empty(),
            "a chord no row carries sent a choice"
        );
        assert!(
            sent.is_empty(),
            "a greyed row ran its verb, which is the one thing a greyed row may never do"
        );
        assert!(
            answered,
            "a greyed row let Cmd+S through — which would be good news, and would mean \
             `menubar` may print a key beside a scoped row after all"
        );

        // ⑥ Quit is a choice like any other and this process is still here to
        // say so.
        assert!(
            bar.performKeyEquivalent(&a_command_press("q")),
            "⌘Q was not answered by the bar"
        );
        assert_eq!(
            taken(),
            vec![MenuChoice::Verb("quit")],
            "⌘Q did not send this product's own quit row"
        );
        println!("  the process is still running after ⌘Q, which is X-4's rule kept");

        // ⑦ a refresh re-greys a row in place: the same items, still answering.
        let second = a_plan(true);
        bt_platform::menu::refresh(&second).expect("the bar refreshes on the main thread");
        assert_eq!(bt_platform::menu::installed_plan().as_ref(), Some(&second));
        let file = submenu(&the_bar(mtm), 1);
        assert_eq!(file.numberOfItems(), 3, "the refresh rebuilt the menu");
        assert!(
            file.itemAtIndex(1).expect("Save is there").isEnabled(),
            "Save is still greyed after the refresh that put it in force"
        );
        assert!(
            the_bar(mtm).performKeyEquivalent(&a_command_press("s")),
            "Save did not answer ⌘S once it was in force"
        );
        assert_eq!(taken(), vec![MenuChoice::Verb("save-preview")]);
        // And the row that was never touched still answers through the same
        // target, which is what "in place" means.
        assert!(the_bar(mtm).performKeyEquivalent(&a_command_press("t")));
        assert_eq!(taken(), vec![MenuChoice::Verb("new-tab")]);

        println!("{name}: ok");
    }

    pub fn main() {
        run();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::main();
    #[cfg(not(target_os = "macos"))]
    println!("macos_menu_bar: nothing to run on this platform");
}
