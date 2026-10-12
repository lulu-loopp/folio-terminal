//! **The crate root: diagnostics and trace.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::TITLE_FRAME;
use std::time::Duration;

#[test]
fn theme_changed_is_ignored_by_explicit_modes_and_resolved_by_system() {
    use bt_persist::ThemeModeV1::{Dark, Light, System};
    use winit::window::Theme::{Dark as OsDark, Light as OsLight};

    assert_eq!(resolved_theme_change(System, OsDark), Some(Theme::Dark));
    assert_eq!(resolved_theme_change(System, OsLight), Some(Theme::Light));
    assert_eq!(resolved_theme_change(Light, OsDark), None);
    assert_eq!(resolved_theme_change(Light, OsLight), None);
    assert_eq!(resolved_theme_change(Dark, OsDark), None);
    assert_eq!(resolved_theme_change(Dark, OsLight), None);
}

/// RED (49) — **A title that did not change is never written to the OS again.**
///
/// A shell that sets the title on every prompt (OSC 0/2) makes the drain say
/// "the chrome changed" on every turn, and until ticket 49 every one of those
/// turns called `Window::set_title` with the same string — a message to the
/// taskbar measured waiting up to 2.2 s. A hundred turns a whole frame apart,
/// each wanting the same title: one write.
///
/// MUTATION: write on every offer (drop the equality check in
/// `pace::LatestThrottle::offer`) — a hundred writes.
#[test]
fn a_title_that_did_not_change_is_never_written_to_the_os_again() {
    let start = Instant::now();
    let mut slot = TitleSlot::default();
    let mut writes = Vec::new();
    for turn in 0..100_u32 {
        slot.want("pwsh — ~/src".to_owned());
        if let Some(title) = slot.take_due(TITLE_FRAME, start + TITLE_FRAME * turn) {
            writes.push(title);
        }
    }
    assert_eq!(writes, vec!["pwsh — ~/src".to_owned()]);
    assert_eq!(
        slot.deadline(),
        None,
        "nothing is held, so nothing wakes the loop"
    );
}

#[test]
fn startup_trace_title_is_human_readable_without_console_output() {
    assert_eq!(startup_scale_title(1.5), "Folio M0-beta · 1.5x");
    assert_eq!(
        startup_trace_title(
            Duration::from_millis(682),
            Duration::from_millis(1089),
            1.25,
        ),
        "Folio M0-beta — bg 682ms · text 1089ms · 1.25x"
    );
}

#[test]
fn panic_log_uses_the_process_temp_directory_without_requiring_stderr() {
    assert_eq!(
        panic_log_path(),
        std::env::temp_dir().join("folio-panic.log"),
        "the file a user is asked to send is named after the product"
    );
}

/// Reading a periodic appointment does not move it; only spending it does.
/// This is the startup poll's instance of the deadline-fold rule.
#[test]
fn an_unchanged_periodic_owner_answers_the_same_absolute_instant() {
    let epoch = Instant::now();
    let mut appointment = epoch + STARTUP_PTY_POLL_INTERVAL;
    let first = appointment;
    let second = appointment;
    assert_eq!(first, second, "two reads did not rebase the startup poll");

    advance_periodic_deadline(
        &mut appointment,
        epoch + STARTUP_PTY_POLL_INTERVAL,
        STARTUP_PTY_POLL_INTERVAL,
    );
    assert_eq!(
        appointment,
        epoch + STARTUP_PTY_POLL_INTERVAL * 2,
        "the owner advances exactly when its appointment is spent",
    );
}

/// The named fold preserves both the absolute winner and its evidence label.
/// Supplying the same owner snapshots at two different query instants has no
/// place from which to manufacture a different answer.
#[test]
fn an_unchanged_deadline_fold_answers_the_same_named_instant() {
    let epoch = Instant::now();
    let names = ["later", "winner", "absent"];
    let entries = [Some(epoch + Duration::from_secs(2)), Some(epoch), None];
    let ask = |_now| earliest_named_deadline(names, entries);
    let first = ask(epoch);
    let second = ask(epoch + Duration::from_secs(1));
    assert_eq!(first, Some(("winner", epoch)));
    assert_eq!(second, first);
}

/// RED ((b)14-8; design note 2026-10-11 (b)7, defect 5) — **a write fault after start reaches a
/// card.**
///
/// Every `take_fault` of the four stores used to be asked at start only, so a write that stopped
/// landing mid-run was owed a card nobody raised. The answers' handler now asks each store after
/// taking every answer. Here the file has a second name — a dotfiles tree that links it in — so
/// the write is refused at once, and the handler's faults carry the sentence the start-up card
/// says, naming the file.
///
/// MUTATION: never call `take_fault` after start (drop the faults from `persist::hear_documents`)
/// — no sentence comes back.
#[test]
fn a_write_fault_after_start_reaches_a_card() {
    let root = bt_testpath::temp_path("bt-app-fault-after-start");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a private storage folder for this test");
    let pins_path = root.join(pins::PINS_FILE_NAME);
    std::fs::write(
        &pins_path,
        bt_persist::serialize_pins(&bt_persist::PinsV1::default()).unwrap(),
    )
    .unwrap();
    std::fs::hard_link(&pins_path, root.join("点文件 dotfiles-pins.json"))
        .expect("a second name for the file");

    let mut settings = persist::SettingsStore::at(root.join(persist::SETTINGS_FILE_NAME));
    let mut keybindings = persist::KeybindingsStore::at_on(
        root.join(persist::KEYBINDINGS_FILE_NAME),
        persist::StorageLane::of_its_own(),
    );
    let mut profiles = persist::ProfilesStore::at_on(
        root.join(persist::PROFILES_FILE_NAME),
        persist::StorageLane::of_its_own(),
    );
    let mut pins = pins::PinsStore::at(pins_path.clone());
    assert!(pins.toggle(bt_persist::PinKind::Folder, "项目/folio 笔记"));
    let _ = pins.settle();

    let heard = persist::hear_documents(&mut settings, &mut keybindings, &mut profiles, &mut pins);
    assert_eq!(
        heard.faults,
        vec![i18n::persisted_file_unwritable(pins::PINS_FILE_NAME)],
        "one card, naming the file"
    );
    assert!(
        persist::hear_documents(&mut settings, &mut keybindings, &mut profiles, &mut pins)
            .faults
            .is_empty(),
        "and said once"
    );
    let _ = std::fs::remove_dir_all(&root);
}
