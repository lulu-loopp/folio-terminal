//! Web preview slice ①: the platform host and its input, and nothing else.
//!
//! Placeholder while the tests below are being made to fail on purpose.

#[cfg(test)]
mod machine_tests {
    use super::*;

    fn boot(preview: &mut WebMachine, url: &str) {
        assert_eq!(preview.request(url), WebEffect::Ignore);
        let generation = preview.generation();
        assert_eq!(
            preview.on_environment(generation, true),
            WebEffect::CreateController
        );
        assert_eq!(
            preview.on_controller(generation, true),
            WebEffect::InstallEvents
        );
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate(url.to_owned())
        );
        assert_eq!(preview.state(), WebState::Ready);
    }

    #[test]
    fn nothing_navigates_before_every_handler_is_on() {
        let mut preview = WebMachine::new();
        preview.request("https://example.com/");
        let generation = preview.generation();
        preview.on_environment(generation, true);
        assert_eq!(
            preview.on_controller(generation, true),
            WebEffect::InstallEvents
        );
        assert_eq!(preview.state(), WebState::ControllerPending);
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate("https://example.com/".into())
        );
    }

    #[test]
    fn a_late_environment_callback_from_a_closed_pane_is_dropped() {
        let mut preview = WebMachine::new();
        preview.request("https://example.com/");
        let stale = preview.generation();
        preview.close();
        assert_eq!(preview.on_environment(stale, true), WebEffect::Ignore);
    }

    #[test]
    fn a_late_controller_from_an_abandoned_generation_is_closed_not_adopted() {
        let mut preview = WebMachine::new();
        preview.request("https://example.com/");
        let stale = preview.generation();
        preview.on_environment(stale, true);
        assert_eq!(
            preview.on_browser_process_failed(),
            WebEffect::RebuildFromScratch
        );
        assert_ne!(preview.generation(), stale);
        assert_eq!(
            preview.on_controller(stale, true),
            WebEffect::CloseOrphanController
        );
        let current = preview.generation();
        assert_eq!(
            preview.on_environment(current, true),
            WebEffect::CreateController
        );
    }

    #[test]
    fn desired_url_is_last_write_wins_and_only_one_navigation_results() {
        let mut preview = WebMachine::new();
        preview.request("https://first.example/");
        let generation = preview.generation();
        preview.request("https://second.example/");
        preview.request("https://third.example/");
        preview.on_environment(generation, true);
        preview.on_controller(generation, true);
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate("https://third.example/".into())
        );
    }

    #[test]
    fn a_failed_page_does_not_overwrite_a_recoverable_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
        preview.on_navigation_completed(generation, "https://dead.example/", false);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
        preview.on_navigation_completed(generation, "about:blank", true);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
    }

    #[test]
    fn a_browser_crash_comes_back_to_the_last_good_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        preview.request("https://broken.example/");
        preview.on_navigation_completed(generation, "https://broken.example/", false);
        assert_eq!(
            preview.on_browser_process_failed(),
            WebEffect::RebuildFromScratch
        );
        let generation = preview.generation();
        preview.on_environment(generation, true);
        preview.on_controller(generation, true);
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate("https://good.example/".into())
        );
    }

    #[test]
    fn a_renderer_crash_only_reloads() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let before = preview.generation();
        assert_eq!(preview.on_render_process_failed(), WebEffect::Reload);
        assert_eq!(preview.generation(), before);
        assert_eq!(preview.state(), WebState::Ready);
    }

    #[test]
    fn closing_waits_for_the_browser_to_exit_before_the_udf_is_touched() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        assert_eq!(preview.close(), WebEffect::AwaitBrowserExitBeforeCleanup);
        assert_eq!(preview.state(), WebState::Closing);
        assert_eq!(
            preview.on_browser_process_exited(),
            WebEffect::ReleaseUserDataFolder
        );
    }

    /// W0′ revision ①. Eight measured shutdowns: six said `BrowserProcessExited`
    /// in 271–390 ms, one took 6 588 ms, and one never said anything at all
    /// (`w0p-evidence.md` §4.2). And on the graceful path `ProcessFailed` does
    /// **not** come — all eight read `process_failed_browser_exited: false` — so
    /// the second door is shut there and the deadline is the only backstop.
    #[test]
    fn a_browser_that_never_says_it_exited_is_cleaned_up_when_the_wait_runs_out() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        assert_eq!(preview.close(), WebEffect::AwaitBrowserExitBeforeCleanup);
        assert_eq!(
            preview.on_cleanup_deadline(),
            WebEffect::ReleaseUserDataFolder
        );
    }

    #[test]
    fn process_failed_is_the_second_door_to_cleanup() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        assert_eq!(
            preview.on_browser_process_failed(),
            WebEffect::ReleaseUserDataFolder
        );
    }

    #[test]
    fn the_user_data_folder_is_cleaned_exactly_once() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        assert_eq!(
            preview.on_browser_process_exited(),
            WebEffect::ReleaseUserDataFolder
        );
        assert_eq!(preview.on_browser_process_failed(), WebEffect::Ignore);
        assert_eq!(preview.on_cleanup_deadline(), WebEffect::Ignore);
        assert_eq!(preview.on_browser_process_exited(), WebEffect::Ignore);
    }

    /// W0′ revision ②. The same `ProcessFailed` means "rebuild" under a live
    /// pane and "the folder is yours" under a closing one, and only the state
    /// tells them apart. The plan's §4 as written rebuilt on both — which brings
    /// back a pane the person shut.
    #[test]
    fn a_dying_browser_does_not_resurrect_a_closed_pane() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        let closed_at = preview.generation();
        assert_ne!(
            preview.on_browser_process_failed(),
            WebEffect::RebuildFromScratch
        );
        assert_eq!(preview.state(), WebState::Closing);
        assert_eq!(preview.generation(), closed_at);
    }

    #[test]
    fn a_new_runtime_version_rebuilds_the_seat_at_the_last_good_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        assert_eq!(
            preview.on_new_browser_version_available(),
            WebEffect::RebuildForNewVersion
        );
        assert_ne!(preview.generation(), generation);
        let current = preview.generation();
        preview.on_environment(current, true);
        preview.on_controller(current, true);
        assert_eq!(
            preview.on_events_installed(current),
            WebEffect::Navigate("https://good.example/".into())
        );
    }

    #[test]
    fn a_controller_from_before_the_version_change_is_closed_not_adopted() {
        let mut preview = WebMachine::new();
        preview.request("https://good.example/");
        let stale = preview.generation();
        preview.on_environment(stale, true);
        assert_eq!(
            preview.on_new_browser_version_available(),
            WebEffect::RebuildForNewVersion
        );
        assert_eq!(
            preview.on_controller(stale, true),
            WebEffect::CloseOrphanController
        );
    }

    #[test]
    fn a_version_change_during_close_is_nobodys_business() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        assert_eq!(preview.on_new_browser_version_available(), WebEffect::Ignore);
        assert_eq!(preview.state(), WebState::Closing);
    }

    #[test]
    fn a_navigation_completed_from_a_stale_generation_cannot_write_the_recoverable_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let stale = preview.generation();
        preview.on_browser_process_failed();
        preview.on_navigation_completed(stale, "https://attacker.example/", true);
        assert_eq!(preview.recoverable_url(), None);
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;
    use crate::shortcuts::{BINDINGS, Chord, ChordKey, Focus, Shortcuts};
    use winit::keyboard::{ModifiersState, NamedKey};

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
        ("command-palette", "Ctrl+Shift+p"),
        ("focus-mode", "Ctrl+Shift+z"),
        ("split-horizontal", "Alt+Shift+-"),
        ("split-vertical", "Alt+Shift+="),
        ("duplicate-pane-split", "Ctrl+Shift+d"),
        ("files-pane", "Ctrl+Shift+b"),
        ("git-page", "Ctrl+Shift+g"),
        ("open-settings", "Ctrl+,"),
        ("save-preview", "Ctrl+s"),
        ("prev-command-mark", "Ctrl+Shift+ArrowUp"),
        ("next-command-mark", "Ctrl+Shift+ArrowDown"),
        ("open-search", "Ctrl+f"),
        ("next-match", "F3"),
        ("prev-match", "Shift+F3"),
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
    /// the web host takes back from a focused page are the same 30 rows.
    #[test]
    fn the_chord_table_the_web_seat_claims_is_the_table_the_window_ships() {
        let spelled: Vec<(&str, String)> = BINDINGS
            .iter()
            .filter_map(|row| row.chord.as_ref().map(|chord| (row.id, spell(chord))))
            .collect();
        let expected: Vec<(&str, String)> = EXPECTED_CHORDS
            .iter()
            .map(|(id, chord)| (*id, (*chord).to_owned()))
            .collect();
        assert_eq!(spelled, expected);
        assert_eq!(spelled.len(), 30);
    }

    /// RED — and every one of them reaches a virtual key, because
    /// `AcceleratorKeyPressed` speaks Win32 and nothing else.
    #[test]
    fn every_shipped_chord_resolves_to_a_virtual_key_on_this_layout() {
        let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
        assert_eq!(
            claims.len(),
            30,
            "a chord this window owns that the web host cannot name in Win32 is \
             a chord that silently stops working while a page has the focus"
        );
        for claim in &claims {
            assert!(claim.vk != 0);
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
        // F5 and F12 are the page's too, and stay so until somebody rules
        // otherwise — see `w0p-evidence.md` §8, slice ④.
        assert!(!claims_chord(&claims, VK_F5, false, false, false));
        assert!(!claims_chord(&claims, VK_F12, false, false, false));
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
    }

    /// RED — a row out of scope is not claimed, because a key the window will
    /// not act on must not be taken away from the page.
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

    fn every_focus() -> Focus {
        Focus {
            preview: true,
            terminal_primary: true,
            search_open: true,
        }
    }
}

#[cfg(test)]
mod navigation_stub_tests {
    use super::*;

    /// RED — slice ①'s navigation policy is the narrowest one that can exist:
    /// the URL the dev flag named, and the `about:blank` the host mints itself.
    #[test]
    fn only_the_hosts_own_two_targets_are_permitted() {
        let minted = "http://127.0.0.1:9134/";
        assert!(navigation_is_permitted(minted, Some(minted)));
        assert!(navigation_is_permitted(ABOUT_BLANK, Some(minted)));
        assert!(navigation_is_permitted("about:blank", Some(minted)));
    }

    /// RED — W0′ gate 9's `fail` in its slice-① shape.
    ///
    /// The engine really does raise `NavigationStarting` for `about:blank`
    /// (`navigation_starting_fired: true`, `uri: "about:blank"`), so a door that
    /// refuses `about:` outright cancels the product's own navigation. The door
    /// below lets through the blank page **the host mints** and nothing else
    /// that spells itself `about:`.
    #[test]
    fn an_about_url_that_is_not_the_hosts_own_blank_page_is_refused() {
        let minted = "http://127.0.0.1:9134/";
        for uri in [
            "about:config",
            "about:blank#evil",
            "about:srcdoc",
            "edge://settings",
        ] {
            assert!(!navigation_is_permitted(uri, Some(minted)), "{uri}");
        }
    }

    #[test]
    fn a_page_that_navigates_itself_somewhere_else_is_refused() {
        let minted = "http://127.0.0.1:9134/";
        for uri in [
            "https://example.com/",
            "http://127.0.0.1:9134/other",
            "javascript:alert(1)",
            "file:///C:/Windows/win.ini",
            "",
        ] {
            assert!(!navigation_is_permitted(uri, Some(minted)), "{uri}");
        }
    }

    #[test]
    fn with_nothing_minted_nothing_but_the_blank_page_goes_through() {
        assert!(navigation_is_permitted(ABOUT_BLANK, None));
        assert!(!navigation_is_permitted("http://127.0.0.1:9134/", None));
    }
}

#[cfg(test)]
mod folder_tests {
    use super::*;
    use std::path::Path;

    /// RED — `%LOCALAPPDATA%`, never `%APPDATA%`: the folder holds a cache, a
    /// cookie jar and a crash-dump directory, none of which may roam
    /// (`plan.md` §0).
    #[test]
    fn the_user_data_folder_is_the_products_own_under_local_appdata() {
        let folder = user_data_folder_in(Path::new(r"C:\Users\x\AppData\Local"));
        assert_eq!(
            folder,
            Path::new(r"C:\Users\x\AppData\Local\Folio\WebView2")
        );
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    /// RED — a seat with no rectangle has no page on it.
    ///
    /// Which is the same door three separate facts come through: the tab was
    /// switched away (the seat is not in the active tab's layout at all), the
    /// fit ladder dropped the seat, or focus mode parked it off stage. All three
    /// end as `device_rect: None`, and all three mean the same thing to a
    /// WebView — hide it, because a hidden WebView stops its timers and its
    /// requestAnimationFrame entirely (`w0p-evidence.md` §1 gate 8: 1 811 ms of
    /// CPU and 718 frames visible, 0 and 0 hidden).
    #[test]
    fn a_seat_with_no_rectangle_hides_the_page() {
        assert_eq!(web_presence(None, false), WebPresence::Hidden);
    }

    /// RED — and so does a modal, because the scrim is painted over the whole
    /// window by wgpu and the page is underneath wgpu. A hole punched through a
    /// scrim is a page the reader can see through a dimmed window.
    #[test]
    fn a_modal_hides_the_page_rather_than_being_shown_through() {
        let body = [10.0, 20.0, 210.0, 120.0];
        assert_eq!(web_presence(Some(body), true), WebPresence::Hidden);
    }

    /// RED — and the rectangle it is shown at is the seat's body in physical
    /// pixels, rounded outwards so no hairline of window shows at the seam.
    #[test]
    fn a_presented_seat_shows_the_page_at_its_body_rectangle() {
        let body = [10.4, 20.6, 210.2, 120.9];
        assert_eq!(
            web_presence(Some(body), false),
            WebPresence::Shown(WebBounds {
                x: 10,
                y: 20,
                width: 201,
                height: 101,
            })
        );
    }

    /// RED — a body that has collapsed to nothing is not a one-pixel page.
    #[test]
    fn a_body_with_no_area_hides_the_page() {
        assert_eq!(web_presence(Some([10.0, 20.0, 10.0, 120.0]), false), WebPresence::Hidden);
        assert_eq!(web_presence(Some([10.0, 20.0, 210.0, 20.0]), false), WebPresence::Hidden);
    }
}
