//! **The crate root: launch and command line.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    host_path, host_uri_path, leaf_saying, method_body, saved_tab, strip_with_cli_tab,
};

/// PIN — mock-up 7426-7431: "Launch asks about exactly one thing, and it is
/// not the pinned tabs. **Pinning IS the answer**."
///
/// Red gate: `Runtime::create` used to rebuild *every* persisted tab
/// unconditionally, which is both halves of this wrong at once — it asked
/// nothing, and it restored what the user may well have meant to close.
#[test]
fn launch_opens_what_you_pinned_and_asks_only_about_the_rest() {
    let saved = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, true),
        saved_tab("pwsh", "C:\\c", None, false),
    ];
    let plan = plan_launch(&saved, 0, false);

    assert_eq!(plan.open, vec![saved[1].clone()], "the pinned one, alone");
    assert_eq!(
        plan.ask,
        vec![saved[0].clone(), saved[2].clone()],
        "the question is the tabs you did not pin, in their own order"
    );
    assert!(
        !plan.placeholder,
        "a pinned tab is already a window worth showing"
    );
    assert_eq!(
        plan.active_open, None,
        "the tab you were on was not pinned, so it is not one of these"
    );
}

/// The seat you were in comes back with you — but only if it was pinned.
#[test]
fn the_tab_you_were_on_keeps_its_seat_when_it_is_one_of_the_pinned() {
    let saved = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, true),
        saved_tab("pwsh", "C:\\c", None, true),
    ];
    // index 2 of the saved list is the second *pinned* tab.
    assert_eq!(plan_launch(&saved, 2, false).active_open, Some(1));
    assert_eq!(plan_launch(&saved, 1, false).active_open, Some(0));
}

/// The boundary ruled in this ticket: "Reopen your **other** tabs?" needs
/// other tabs. With one unpinned tab and nothing pinned there is no question
/// — and declining would have handed back a fresh shell in the wrong folder,
/// which is strictly worse than the tab it replaced.
#[test]
fn a_lone_unpinned_tab_is_restored_rather_than_asked_about() {
    let saved = [saved_tab("pwsh", "C:\\only", None, false)];
    let plan = plan_launch(&saved, 0, false);

    assert_eq!(plan.open, saved.to_vec(), "it simply comes back");
    assert!(plan.ask.is_empty(), "nothing to ask");
    assert!(!plan.placeholder, "it is a real tab, not scaffolding");
    assert_eq!(plan.active_open, Some(0));

    // Two unpinned tabs *are* a question, and then a stand-in shell carries
    // the window until it is answered.
    let two = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, false),
    ];
    let plan = plan_launch(&two, 0, false);
    assert!(plan.open.is_empty());
    assert_eq!(plan.ask.len(), 2);
    assert!(
        plan.placeholder,
        "nothing was pinned, so nothing is standing"
    );
}

#[test]
fn a_first_launch_with_nothing_saved_asks_nothing_and_stands_something_up() {
    let plan = plan_launch(&[], 0, false);
    assert!(plan.open.is_empty());
    assert!(plan.ask.is_empty(), "no prompt on a first run");
    assert!(plan.placeholder);
}

/// PIN (§7.2) — **a command line is not a placeholder**, so a Restore
/// accepted afterwards cannot sweep it away.
///
/// The placeholder exists for one situation: nothing was pinned, and the
/// window needed *something* to be a window with. A pane somebody named a
/// folder for is not that; it is the thing they asked for, and
/// `answer_restore` retires the placeholder without asking.
///
/// MUTATION: drop the `&& !cli_wants_pane` from `placeholder` and this
/// fails — and on the real machine, `folio --cwd D:\proj` followed by
/// "Restore" would close the pane in `D:\proj` while the shells it revived
/// came up.
#[test]
fn a_tab_the_command_line_asked_for_is_never_the_launch_placeholder() {
    let two = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, false),
    ];
    assert!(plan_launch(&two, 0, false).placeholder, "the red half");
    let plan = plan_launch(&two, 0, true);
    assert!(!plan.placeholder);
    assert!(plan.open.is_empty(), "nothing was pinned, so nothing opens");
    assert_eq!(plan.ask.len(), 2, "and the question is still asked");

    // The same on a first run with nothing saved at all: there is one tab and
    // it is the one that was asked for.
    let plan = plan_launch(&[], 0, true);
    assert!(!plan.placeholder);
    assert!(plan.ask.is_empty());
}

/// PIN (§7.2) — **a command line turns a lone saved tab back into a
/// question.**
///
/// The one-tab shortcut above holds because declining leaves the user with a
/// fresh shell in the wrong folder, which is strictly worse than the tab it
/// replaced. A launch that was told the folder has already opened one in the
/// right one, so that premise is gone and the question is a real question.
///
/// MUTATION: drop the `&& !cli_wants_pane` from the shortcut and this fails
/// — `folio --cwd D:\proj` would silently revive last night's tab beside the
/// one that was asked for, with no prompt and no way to say no.
#[test]
fn a_command_line_turns_a_lone_saved_tab_back_into_a_question() {
    let saved = [saved_tab("pwsh", "C:\\only", None, false)];
    let plan = plan_launch(&saved, 0, true);
    assert!(plan.open.is_empty(), "nothing comes back unasked");
    assert_eq!(plan.ask, saved.to_vec(), "it is the prompt's question");
    assert_eq!(plan.active_open, None);

    // A *pinned* tab is an answer already given, and a command line does not
    // reopen that question either: it opens alongside.
    let pinned = [saved_tab("pwsh", "C:\\only", None, true)];
    let plan = plan_launch(&pinned, 0, true);
    assert_eq!(plan.open, pinned.to_vec());
    assert!(plan.ask.is_empty());
}

/// The pure functions §7.2 names, put together over the one launch that
/// used to break: a session with an unpinned tab and two pinned ones, and a
/// `--cwd` on the command line.
///
/// Red gate: with [`cli_tab_slot`] answering `0`, the strip is
/// `cli, pinned, pinned` and the active tab is the one at slot 0 — the first
/// of those is what `debug_assert!` in `tab_trailers` fired on.
#[test]
fn a_launch_told_a_folder_opens_it_at_the_head_of_the_unpinned_run() {
    let saved = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, true),
        saved_tab("pwsh", "C:\\c", None, true),
    ];
    let plan = plan_launch(&saved, 1, true);
    assert_eq!(plan.open, vec![saved[1].clone(), saved[2].clone()]);
    assert_eq!(plan.ask, vec![saved[0].clone()], "the rest is still asked");

    let pins = plan.open.iter().map(|tab| tab.pinned).collect::<Vec<_>>();
    let slot = cli_tab_slot(&pins);
    assert_eq!(slot, 2, "both pinned tabs keep the head they were promised");
    assert!(
        seed::pins_are_normalized(&strip_with_cli_tab(&pins, false), |pinned| *pinned),
        "F57: the pinned run leads the strip"
    );
    assert_eq!(
        launch_active_tab(Some(slot), plan.active_open, plan.open.len() + 1),
        2,
        "and it is still the tab you are put in, rather than the pinned tab \
             you were last on"
    );
}

/// RED GATE (coordinator ruling 2026-08-29, DESIGN §7.46 ②) — **the theme
/// has one store, and it is `settings.json`.**
///
/// Until this gate there were two: `settings.json`'s `theme_mode`, which the
/// Settings page draws and nobody read, and `session.json`'s `theme`, which
/// nobody could see and which every boot actually obeyed. They disagreed by
/// construction — `SettingsV1::default()` is `System` and `SessionV1`'s
/// default was **Dark** — so a machine whose Windows says light opened a dark
/// window from a brand-new profile, and the first pane in it answered
/// `OSC 11` with the dark canvas. That is the shape §7.46 could not rule out
/// as the cause of the report it was written for: a Codex started on that
/// first dark canvas asks once, is told dark, and keeps it.
///
/// The three cases the ruling names, and the fourth that keeps the fix from
/// costing every existing user their choice.
#[test]
fn the_theme_a_window_opens_in_is_the_one_the_settings_file_names() {
    use bt_persist::SettingsV1;
    use bt_render::{FOLIO_DARK, FOLIO_LIGHT};

    /// What a pane is told when the window opened on `theme`.
    ///
    /// The chain this gate is really about — a boot mode, the canvas it
    /// resolves to, and the background the first `OSC 11` carries — spelled
    /// once so each case below reads as the one thing it varies.
    fn told(mode: ThemeModeV1, os: Option<OsTheme>) -> [u8; 3] {
        let scheme = match resolve_theme_mode(mode, os) {
            Theme::Light => FOLIO_LIGHT,
            Theme::Dark => FOLIO_DARK,
        };
        terminal_palette(scheme, scheme.background, scheme.foreground).background
    }

    // A brand-new profile carries no session theme at all. This is the fact
    // the old default contradicted, and asserting it here is what makes the
    // `None` arm below reachable on a real machine.
    assert_eq!(SessionV1::default().theme, None);

    // ① Fresh profile, Windows says light → the window opens light and the
    //    first pane is told the light canvas.
    let fresh = startup_theme_mode(&SettingsV1::default(), &SessionV1::default());
    assert_eq!(fresh, ThemeModeV1::System);
    assert_eq!(told(fresh, Some(OsTheme::Light)), FOLIO_LIGHT.background);

    // ② `theme_mode = Dark` outranks a Windows that says light. A chosen
    //    mode is a choice, not a preference to be overridden by the OS.
    let chosen_dark = SettingsV1 {
        theme_mode: ThemeModeV1::Dark,
        ..SettingsV1::default()
    };
    let dark = startup_theme_mode(&chosen_dark, &SessionV1::default());
    assert_eq!(dark, ThemeModeV1::Dark);
    assert_eq!(told(dark, Some(OsTheme::Light)), FOLIO_DARK.background);

    // ③ And a chosen Light outranks a Windows that says dark, which is the
    //    case the user in the report is actually in.
    let chosen_light = SettingsV1 {
        theme_mode: ThemeModeV1::Light,
        ..SettingsV1::default()
    };
    let light = startup_theme_mode(&chosen_light, &SessionV1::default());
    assert_eq!(told(light, Some(OsTheme::Dark)), FOLIO_LIGHT.background);

    // ④ **The carry-forward, and it fires exactly here.** Every profile
    //    written before this ruling holds the user's real choice in
    //    `session.json` and an untouched `System` in `settings.json`. Reading
    //    settings alone would silently reset all of them, so a session
    //    document that still carries the retired key is believed once — it is
    //    what that user has been looking at — and the boot that believes it
    //    writes it into settings and stops writing the key.
    let carried = startup_theme_mode(
        &SettingsV1::default(),
        &SessionV1 {
            theme: Some(SessionThemeV1::Light),
            ..SessionV1::default()
        },
    );
    assert_eq!(
        carried,
        ThemeModeV1::Light,
        "an old profile's choice lives in session.json and must survive the move"
    );
    assert_eq!(told(carried, Some(OsTheme::Dark)), FOLIO_LIGHT.background);
}

/// RED GATE (same ruling) — **the canvas is decided before the window
/// exists, so there is no first frame in the other one.**
///
/// The resolution used to read `window.theme()`, which cannot be asked until
/// there is a window; everything that happens between `create_window` and
/// `set_theme` therefore happens on whichever canvas the process was born
/// with. `Window::theme()` also answers `None` on a machine that will not
/// say, and `resolve_theme_mode` reads `None` as dark — a silent wrong answer
/// at exactly the moment the first pane is about to be asked what colour it
/// is standing on.
///
/// Mutation: move `set_theme` back below `create_window`, or resolve from the
/// window again.
#[test]
fn the_canvas_is_in_force_before_the_window_is_made() {
    let body = method_body("Runtime", "create");
    let themed = body
        .find("set_theme(resolved_theme)")
        .expect("`Runtime::create` puts a canvas in force");
    let made = body
        .find("create_window(")
        .expect("`Runtime::create` creates the window");
    let schemes = body
        .find("adopt_stored_schemes(")
        .expect("`Runtime::create` adopts the stored scheme pair");
    assert!(
        schemes < themed,
        "the pair is adopted before the theme picks one of it"
    );
    assert!(
        themed < made,
        "the canvas is settled before the window exists, or the first frame is              painted in the canvas the process was born with"
    );
    assert!(
        !body[..made].contains("window.theme()"),
        "the boot resolution cannot ask a window that does not exist yet"
    );
}

#[test]
fn startup_polls_pty_until_the_first_text_frame_is_presented() {
    assert_eq!(startup_poll_delay(false), Some(STARTUP_PTY_POLL_INTERVAL));
    assert_eq!(startup_poll_delay(true), None);
}

/// RED (T-RESTART-CWD, 2026-10-04) — **a pane whose shell never reported a
/// folder is started again where it was born** (`docs/M2-restart-shell-contract.md`
/// §1.1: 无上报则该 seat 的初始 cwd).
///
/// `Restart shell` read only the shell's OSC 7 report, so a pane opened by
/// `New terminal in folder…` (or split off into a folder) whose shell has no
/// integration restarted at its profile's default folder. The ladder is
/// [`LeafSession::place_for_a_new_shell`]: the report, else the folder the
/// spawn put the shell down in, except the shell's-own-home mark, which is
/// handed on as nothing so the next spawn asks for the home again.
///
/// MUTATIONS, each observed red: drop the spawn rung (answer `None` after the
/// report) — the first assertion; read the spawn rung before the report — the
/// second; drop the shell's-home exception — the third.
#[test]
fn a_pane_born_in_a_named_folder_starts_its_next_shells_there_whatever_the_profile_says() {
    // RED (coordinator's ruling 2026-10-05) — **a pane opened in a named folder hands that folder
    // on as named**, so Restart shell, Duplicate tab, Duplicate pane and the splits stand there
    // under Home and a fixed folder too; a pane not born that way hands its folder on as carried
    // and keeps the profile's rule. The leaf's half is `LeafSession::seed_place_for_a_new_shell`;
    // `profiles::place_for` weighs it (pinned by
    // `every_road_that_names_a_folder_opens_there_whatever_the_profile_says`).
    //
    // MUTATION, observed red: answer `seed_place_for_a_new_shell` with `Carried` whatever
    // `born_named` says — the named leaf's restart goes to the fixed folder.
    let born_in = PathBuf::from(r"D:\项目\clicked");
    let named = LeafSession {
        spawn_place: Some(born_in.clone()),
        born_named: true,
        ..leaf_saying("no report from this shell")
    };
    let carried = LeafSession {
        spawn_place: Some(born_in.clone()),
        ..leaf_saying("no report from this shell either")
    };
    let fixed = profiles::StartAt::Fixed(PathBuf::from(r"E:\固定"));
    let restarted_in = |leaf: &LeafSession| {
        struct Nowhere;
        impl bt_pty::ShellEnvironment for Nowhere {
            fn var_os(&self, _: &str) -> Option<std::ffi::OsString> {
                None
            }
            fn is_file(&self, _: &Path) -> bool {
                false
            }
        }
        profiles::place_for(
            &fixed,
            &profiles::StartingDir::AccountHome,
            profiles::PathNamespace::Windows,
            restart_seed(&leaf.profile, leaf.seed_place_for_a_new_shell()).cwd,
            &Nowhere,
        )
        .working_directory
    };
    assert_eq!(
        named.seed_place_for_a_new_shell(),
        Some(profiles::SeedPlace::Named(born_in.clone()))
    );
    assert_eq!(restarted_in(&named), Some(born_in.clone()));
    assert_eq!(
        carried.seed_place_for_a_new_shell(),
        Some(profiles::SeedPlace::Carried(born_in))
    );
    assert_eq!(
        restarted_in(&carried),
        Some(PathBuf::from(r"E:\固定")),
        "a pane not born in a named folder keeps its profile's fixed folder"
    );
    // The `+` and a picker row beside a pane born named still carry (the review's unpinned
    // clause). MUTATION, observed red: answer `place_for_a_new_tab_beside` with
    // `seed_place_for_a_new_shell` — the folder arrives named.
    let reported = LeafSession {
        spawn_place: Some(PathBuf::from(r"D:\项目\clicked")),
        born_named: true,
        ..leaf_saying(&format!(
            "\u{1b}]7;file://localhost{}\u{7}",
            host_uri_path(r"D:\Developer\elsewhere")
        ))
    };
    assert_eq!(
        reported.place_for_a_new_tab_beside(),
        Some(profiles::SeedPlace::Carried(host_path(
            r"D:\Developer\elsewhere"
        ))),
        "a new tab beside a pane born in a named folder carries its folder"
    );
}

#[test]
fn a_pane_that_never_reported_a_folder_is_started_again_where_it_was_born() {
    let born_in = PathBuf::from(r"D:\Projects\chosen-folder");

    let silent = LeafSession {
        spawn_place: Some(born_in.clone()),
        ..leaf_saying("no report from this shell")
    };
    assert_eq!(
        restart_seed(&silent.profile, silent.seed_place_for_a_new_shell()).cwd,
        Some(profiles::SeedPlace::Carried(born_in.clone())),
        "the folder the pane was born in, not the profile's default"
    );

    let reported = LeafSession {
        spawn_place: Some(born_in.clone()),
        ..leaf_saying(&format!(
            "\u{1b}]7;file://localhost{}\u{7}",
            host_uri_path(r"D:\Developer\folio-terminal")
        ))
    };
    assert_eq!(
        reported.place_for_a_new_shell(),
        Some(host_path(r"D:\Developer\folio-terminal")),
        "a report is the first rung and beats where the shell was born"
    );

    // A WSL pane put down at its shell's home hands its mark on; `profiles::place_for` reads a
    // place equal to the home mark as the shell's home (round 2), pinned in `profiles::tests`.
    let at_home = LeafSession {
        spawn_place: Some(PathBuf::from("~")),
        ..leaf_saying("no report from this shell either")
    };
    assert_eq!(at_home.place_for_a_new_shell(), Some(PathBuf::from("~")));
}
