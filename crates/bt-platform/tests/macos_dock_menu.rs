//! **A real `applicationDockMenu:`, answered by the delegate AppKit is holding,
//! inside a real `.app`** — the claims T-MAC-DOCKMENU makes that no Windows
//! runner can check (ticket T-MAC-DOCKMENU, `docs/DESIGN.md` §13.50).
//!
//! # Why this is a target of its own
//!
//! `tests/macos_app_delegate.rs`' two reasons, both of them this ticket's too.
//!
//! **The main thread.** Everything here is AppKit and AppKit is the main
//! thread's; libtest does not give a case that thread — a `#[test]` run with
//! `--test-threads=1` still executes on a thread libtest spawned, where
//! `MainThreadMarker::new()` is `None`. `harness = false` hands this file the
//! process's own `main`.
//!
//! **winit.** The method under test is added to `WinitApplicationDelegate`, a
//! private class **winit** registers when its event loop is built. A proof that
//! built no event loop would be a proof about a class that is not in the
//! process.
//!
//! And one of its own: an ssh session cannot reach the window server, so the
//! binary is copied into a throwaway ad-hoc-signed `.app` with an identifier of
//! its own and an isolated `HOME`, and started with `open`
//! (`docs/plans/port/t-mac-dockmenu/dock-menu-proof.sh`). Outside a bundle this
//! file prints one line and exits, so an ordinary `cargo test -p bt-platform`
//! costs nothing — the gate is the bundle rather than an environment variable,
//! because `open` passes none of the shell's environment through.
//!
//! # What it proves, in order
//!
//! ① **winit does not implement `applicationDockMenu:` itself** —
//!    `class_getInstanceMethod` on its own class, asked *before* anything is
//!    added, which is X-4's measurement made again for the fifth selector;
//! ② after `AppDelegate::install`, the object AppKit holds **answers** it —
//!    `respondsToSelector:` asked of `NSApp.delegate`, which is what AppKit
//!    itself consults before it sends the message — and that object is still
//!    winit's own, so all four of M3-1's are answered beside it;
//! ③ **sending the delegate `applicationDockMenu:` gives back a menu that is the
//!    plan**: one item per Dock row, in order, with the plan's titles, each
//!    enabled, each carrying no key equivalent, and the menu itself with
//!    automatic enabling off — the flag that would otherwise grey every row for
//!    a reader who is in another application;
//! ④ **the language switch reaches it with no door of its own**: a `refresh`
//!    with the Chinese titles, and the *next* answer carries them. Nothing is
//!    installed again and nothing of the bar is rebuilt, because the menu is
//!    built out of the plan on the spot every time AppKit asks;
//! ⑤ **each item's action produces the same choice the bar's row produces**,
//!    and says it came from the Dock: `⌘N` on the bar sends
//!    `(Bar, Verb("new-window"))`, and `-[NSApplication sendAction:to:from:]`
//!    with the Dock item's own target and action sends
//!    `(Dock, Verb("new-window"))`. Same verb, same inbox, one field apart;
//! ⑥ **a plan with no Dock rows answers nil**, which is AppKit's menu left
//!    exactly as it was before this ticket.
//!
//! **The right-click itself is NOT-CHECKABLE by an agent**, for
//! `tests/macos_app_delegate.rs`' reason: driving the Dock needs `System
//! Events`, and therefore Accessibility *and* Automation, behind a TCC prompt an
//! ssh session cannot reach. What is checked instead is the whole of what the
//! Dock does when it is right-clicked — it sends the delegate this message and
//! then sends the chosen item's action — asked of this process's own delegate
//! and this process's own items. What a human should look for is in §13.50 ⑥.

#[cfg(target_os = "macos")]
mod mac {
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};
    use std::time::Instant;

    use bt_platform::menu::{
        DockRow, MenuAction, MenuChoice, MenuChord, MenuEntry, MenuItem, MenuKey, MenuList,
        MenuPlan, MenuRole, MenuSurface,
    };
    use bt_platform::{
        AppDelegate, AppDelegateEvent, delegate_answers_the_dock_menu,
        delegate_answers_the_four_selectors, winit_delegate_already_answers_the_dock_menu,
    };
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, msg_send};
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSMenu};
    use objc2_foundation::{NSPoint, NSString};
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::WindowId;

    /// Everything the one sender has been handed, in order.
    static SENT: Mutex<Vec<(MenuSurface, MenuChoice)>> = Mutex::new(Vec::new());

    fn keep(surface: MenuSurface, choice: MenuChoice) {
        SENT.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((surface, choice));
    }

    fn taken() -> Vec<(MenuSurface, MenuChoice)> {
        std::mem::take(&mut *SENT.lock().unwrap_or_else(PoisonError::into_inner))
    }

    // ── where this process is, and where it writes ─────────────────────────

    /// The `.app` this binary is inside, or `None` when it is not inside one.
    fn enclosing_bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let bundle = exe.parent()?.parent()?.parent()?;
        (bundle.extension()? == "app").then(|| bundle.to_path_buf())
    }

    /// The report, appended to and flushed on every line.
    struct Report {
        file: std::fs::File,
        started: Instant,
        failures: usize,
    }

    impl Report {
        fn at(path: &Path) -> Self {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("the report is beside the bundle, which this process can write to");
            Self {
                file,
                started: Instant::now(),
                failures: 0,
            }
        }

        fn say(&mut self, line: &str) {
            let at = self.started.elapsed().as_millis();
            let _ = writeln!(self.file, "[{at:>7}ms] {line}");
            let _ = self.file.flush();
            println!("[{at:>7}ms] {line}");
        }

        fn pass(&mut self, claim: &str) {
            self.say(&format!("PASS {claim}"));
        }

        fn fail(&mut self, claim: &str, because: &str) {
            self.failures += 1;
            self.say(&format!("FAIL {claim}: {because}"));
        }

        fn claim(&mut self, claim: &str, held: bool, because: &str) {
            if held {
                self.pass(claim);
            } else {
                self.fail(claim, because);
            }
        }
    }

    // ── the plan this file installs ────────────────────────────────────────

    /// The two words a reader sees, in the two languages, as `bt_app::i18n`
    /// writes them.
    ///
    /// Copied rather than read: this crate cannot see `bt_app`, and the pin
    /// holding the real Dock rows to that table runs on a Windows workstation
    /// (`menubar::the_dock_names_a_verb_the_way_the_bar_names_it`). What is
    /// checked here is that whatever words the plan carries are the words the
    /// menu comes back with — including CJK, which is the half a byte-oriented
    /// bridge could get wrong.
    const ENGLISH: [&str; 2] = ["New window", "New tab"];
    const CHINESE: [&str; 2] = ["新建窗口", "新建标签"];

    fn command(key: &str) -> MenuChord {
        MenuChord {
            command: true,
            shift: false,
            option: false,
            control: false,
            key: MenuKey::Character(key.to_owned()),
        }
    }

    /// A bar with the two verbs the Dock also offers, so that ⑤ can put the two
    /// surfaces side by side, and the Dock rows in the language asked for.
    fn a_plan(titles: [&'static str; 2], dock: bool) -> MenuPlan {
        MenuPlan {
            menus: vec![MenuList {
                title: "File",
                role: MenuRole::Plain,
                entries: vec![
                    MenuEntry::Item(MenuItem {
                        title: titles[0],
                        action: MenuAction::Verb("new-window"),
                        chord: Some(command("n")),
                        enabled: true,
                    }),
                    MenuEntry::Item(MenuItem {
                        title: titles[1],
                        action: MenuAction::Verb("new-tab"),
                        chord: Some(command("t")),
                        enabled: true,
                    }),
                ],
            }],
            dock: if dock {
                vec![
                    DockRow {
                        title: titles[0],
                        choice: MenuChoice::Verb("new-window"),
                    },
                    DockRow {
                        title: titles[1],
                        choice: MenuChoice::Verb("new-tab"),
                    },
                ]
            } else {
                Vec::new()
            },
        }
    }

    /// A `⌘`-modified press of one character, as AppKit would deliver it.
    fn a_command_press(key: &str) -> Retained<NSEvent> {
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

    /// **Ask the delegate for the Dock menu, exactly as the Dock does.**
    ///
    /// `NSApp.delegate` is winit's object with Folio's method on it, and this is
    /// the message AppKit sends it when a reader presses and holds the tile. The
    /// answer is not owned by the caller — the method's name is not `alloc`,
    /// `new`, `copy` or `mutableCopy` — so it is retained here for as long as
    /// this case reads it, which is what `retain_autoreleased` is for.
    fn ask_the_delegate_for_the_dock_menu(mtm: MainThreadMarker) -> Option<Retained<NSMenu>> {
        let app = NSApplication::sharedApplication(mtm);
        let delegate = app.delegate()?;
        // SAFETY: the selector is `NSApplicationDelegate`'s own, takes the
        // application and answers an `NSMenu *` that is not owned by the caller.
        unsafe {
            let menu: *mut NSMenu = msg_send![&*delegate, applicationDockMenu: &*app];
            Retained::retain_autoreleased(menu)
        }
    }

    /// The titles of a menu's rows, top to bottom.
    fn titles_of(menu: &NSMenu) -> Vec<String> {
        (0..menu.numberOfItems())
            .filter_map(|at| menu.itemAtIndex(at))
            .map(|item| item.title().to_string())
            .collect()
    }

    // ── the exercise ───────────────────────────────────────────────────────

    struct Probe {
        _door: AppDelegate,
        report: Report,
        ran: bool,
    }

    impl Probe {
        /// Everything this file has to say, on the first turn there is an
        /// `NSApplication` under it.
        ///
        /// One turn and not a script: nothing here waits for another process,
        /// asks LaunchServices for anything or opens a window. The Dock menu is
        /// a question with an answer, and every claim is that answer read back.
        fn run_every_claim(&mut self, mtm: MainThreadMarker) {
            // ② the delegate AppKit holds answers the fifth selector, and still
            // answers M3-1's four.
            self.report.claim(
                "the delegate AppKit holds answers applicationDockMenu:",
                delegate_answers_the_dock_menu(),
                "NSApp.delegate does not respond to it, so AppKit will never send it",
            );
            self.report.claim(
                "and it still answers M3-1's four",
                delegate_answers_the_four_selectors(),
                "the fifth selector displaced one of the four",
            );

            // ③ the menu is the plan.
            let english = a_plan(ENGLISH, true);
            match bt_platform::menu::install(&english, Box::new(keep)) {
                Ok(()) => self.report.pass("the menu bar installed"),
                Err(reason) => {
                    self.report.fail("the menu bar installed", &reason);
                    return;
                }
            }
            let Some(menu) = ask_the_delegate_for_the_dock_menu(mtm) else {
                self.report.fail(
                    "applicationDockMenu: answered a menu",
                    "it answered nil for a plan with two Dock rows",
                );
                return;
            };
            let titles = titles_of(&menu);
            self.report
                .say(&format!("MEASURED the Dock menu carries {titles:?}"));
            self.report.claim(
                "the Dock menu is the plan's rows, in the plan's order",
                titles == ENGLISH.map(str::to_owned).to_vec(),
                &format!("it carries {titles:?}"),
            );
            self.report.claim(
                "the Dock menu decides its own rows rather than asking a responder chain",
                !menu.autoenablesItems(),
                "automatic enabling is on, which greys every row for a reader in another \
                 application",
            );
            let mut every_row_offers_itself = true;
            let mut no_row_claims_a_key = true;
            for at in 0..menu.numberOfItems() {
                let Some(item) = menu.itemAtIndex(at) else {
                    continue;
                };
                every_row_offers_itself &= item.isEnabled();
                no_row_claims_a_key &= item.keyEquivalent().to_string().is_empty();
            }
            self.report.claim(
                "every Dock row is in force",
                every_row_offers_itself,
                "a row is greyed, and the state this menu is read in is the one a bar row greys \
                 itself for",
            );
            self.report.claim(
                "no Dock row prints a key equivalent",
                no_row_claims_a_key,
                "a Dock row carries a key, which is a second claim on a chord keyDown: and the \
                 bar have already settled",
            );

            // ⑤ the two surfaces, side by side.
            let _ = taken();
            let bar = NSApplication::sharedApplication(mtm).mainMenu();
            let answered = bar.is_some_and(|bar| bar.performKeyEquivalent(&a_command_press("n")));
            let from_the_bar = taken();
            self.report
                .say(&format!("MEASURED ⌘N on the bar: {from_the_bar:?}"));
            self.report.claim(
                "the bar's own row sends one choice, from the bar",
                answered
                    && from_the_bar == vec![(MenuSurface::Bar, MenuChoice::Verb("new-window"))],
                "the bar did not answer ⌘N with exactly one Bar/new-window",
            );
            let sent = self.press_the_dock_row(mtm, 0);
            let from_the_dock = taken();
            self.report
                .say(&format!("MEASURED the first Dock row: {from_the_dock:?}"));
            self.report.claim(
                "the first Dock row sends the same verb, from the Dock",
                sent && from_the_dock == vec![(MenuSurface::Dock, MenuChoice::Verb("new-window"))],
                "the action did not arrive as exactly one Dock/new-window",
            );
            let sent = self.press_the_dock_row(mtm, 1);
            let second = taken();
            self.report
                .say(&format!("MEASURED the second Dock row: {second:?}"));
            self.report.claim(
                "the second Dock row is the new tab",
                sent && second == vec![(MenuSurface::Dock, MenuChoice::Verb("new-tab"))],
                "the action did not arrive as exactly one Dock/new-tab",
            );

            // ④ the language switch, with no door of its own.
            match bt_platform::menu::refresh(&a_plan(CHINESE, true)) {
                Ok(()) => self.report.pass("the bar took the second language"),
                Err(reason) => self
                    .report
                    .fail("the bar took the second language", &reason),
            }
            let after = ask_the_delegate_for_the_dock_menu(mtm)
                .map(|menu| titles_of(&menu))
                .unwrap_or_default();
            self.report
                .say(&format!("MEASURED after the language switch: {after:?}"));
            self.report.claim(
                "a refresh reaches the Dock tile with no door of its own",
                after == CHINESE.map(str::to_owned).to_vec(),
                "the Dock menu still carries the words it was installed with",
            );

            // ⑥ no rows, no menu.
            match bt_platform::menu::install(&a_plan(ENGLISH, false), Box::new(keep)) {
                Ok(()) => {}
                Err(reason) => self
                    .report
                    .fail("a plan with no Dock rows installed", &reason),
            }
            let bare = ask_the_delegate_for_the_dock_menu(mtm);
            self.report.claim(
                "a plan with no Dock rows answers nil, and AppKit's own menu is untouched",
                bare.is_none(),
                "a menu was built for a plan that asked for no rows",
            );
        }

        /// **Send one Dock item's action the way AppKit sends it.**
        ///
        /// `-[NSApplication sendAction:to:from:]` with the item's own target and
        /// action and the item as the sender, which is what `NSMenu`'s own
        /// dispatch does one layer down. The alternative — calling the target
        /// directly — would prove the method runs and not that the item is
        /// wired to it.
        fn press_the_dock_row(&mut self, mtm: MainThreadMarker, at: isize) -> bool {
            let Some(menu) = ask_the_delegate_for_the_dock_menu(mtm) else {
                self.report.fail(
                    "a Dock row could be pressed",
                    "there is no menu to press it on",
                );
                return false;
            };
            let Some(item) = menu.itemAtIndex(at) else {
                self.report
                    .fail("a Dock row could be pressed", "the menu has no row there");
                return false;
            };
            let (Some(action), Some(target)) = (item.action(), item.target()) else {
                self.report.fail(
                    "a Dock row could be pressed",
                    "the row carries no action, or no target to send it to",
                );
                return false;
            };
            let app = NSApplication::sharedApplication(mtm);
            // SAFETY: `sendAction:to:from:` is `NSApplication`'s own, and the
            // three arguments are the item's own action, its own target and the
            // item itself as sender — the triple AppKit assembles for a press.
            unsafe { msg_send![&*app, sendAction: action, to: &*target, from: &*item] }
        }
    }

    impl ApplicationHandler for Probe {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.ran {
                return;
            }
            self.ran = true;
            let Some(mtm) = MainThreadMarker::new() else {
                self.report.fail(
                    "this target owns the main thread",
                    "`resumed` is not on it, which cannot happen",
                );
                el.exit();
                return;
            };
            self.run_every_claim(mtm);
            self.report.say(&format!(
                "{} failed",
                if self.report.failures == 0 {
                    "0".to_owned()
                } else {
                    self.report.failures.to_string()
                }
            ));
            self.report.say("ALL_DONE");
            el.exit();
        }

        fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {}
    }

    pub fn run() {
        let Some(bundle) = enclosing_bundle() else {
            println!(
                "macos_dock_menu: skipped — this binary is not inside a .app, and a window \
                 server session is the whole exercise"
            );
            return;
        };
        let work = bundle
            .parent()
            .expect("a bundle is inside a directory")
            .to_path_buf();
        let mut report = Report::at(&work.join("t-mac-dockmenu-report.log"));
        report.say(&format!(
            "pid={} bundle={}",
            std::process::id(),
            bundle.display()
        ));

        // ① asked before anything is added, and asked again after the loop is
        // built, because the class does not exist until it is.
        report.say(&format!(
            "MEASURED class_getInstanceMethod(WinitApplicationDelegate, applicationDockMenu:) \
             before the event loop: {:?}",
            winit_delegate_already_answers_the_dock_menu()
        ));
        let event_loop = EventLoop::<()>::with_user_event()
            .build()
            .expect("an event loop");
        let already = winit_delegate_already_answers_the_dock_menu();
        report.say(&format!(
            "MEASURED the same reading after EventLoop::new: {already:?}"
        ));
        report.claim(
            "winit does not implement applicationDockMenu: itself",
            already == Some(false),
            "winit's own class already answers it, so adding Folio's would displace one of them",
        );

        // **After the loop and before `run_app`** — M3-1's order, and the same
        // reason: `EventLoop::new` is what registers the class the door adds its
        // selectors to.
        let proxy = event_loop.create_proxy();
        let door = match AppDelegate::install(move |_event: AppDelegateEvent| {
            let _ = proxy.send_event(());
        }) {
            Ok(door) => door,
            Err(reason) => {
                report.fail("the application delegate installs", &reason);
                report.say("ALL_DONE");
                return;
            }
        };
        door.ready();
        report.pass("the application delegate installed onto winit's own class");

        let mut probe = Probe {
            _door: door,
            report,
            ran: false,
        };
        // Nothing here waits for anything, so the loop turns once and leaves.
        // `Wait` rather than `Poll` all the same: a target that spun would be a
        // target that burns a core if a claim ever hangs.
        event_loop.set_control_flow(ControlFlow::Wait);
        match event_loop.run_app(&mut probe) {
            Ok(()) => probe.report.say("run_app returned Ok"),
            Err(error) => probe
                .report
                .fail("the event loop ran to the end", &format!("{error}")),
        }
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_dock_menu: nothing to run on this platform");
}
