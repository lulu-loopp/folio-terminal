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

/// RED (T-HIGH-CONTRAST, owner rulings 2026-10-28, row 90 ⑥ and ⑦) — **while a Windows
/// contrast theme is on, its Background decides the canvas whatever the Theme row says, and
/// the window's ground holds no picture and lets nothing through; off, both are what the row
/// and the file say again.**
///
/// The two decisions a theme makes outside the palette, each a pure function of the row's
/// own answer and the reading. Night sky is dark and Desert light, so each overrides the row
/// that names the other canvas.
///
/// MUTATION (observed red): return `resolved` from `theme_under_contrast` whatever the reading
/// — a Light window under Night sky draws Night sky's black under a light canvas's readers; or
/// return `ground` from `ground_under_contrast` — a background picture stays under a contrast
/// theme's text.
#[test]
fn a_contrast_theme_decides_the_canvas_and_clears_the_ground_while_it_is_on() {
    use crate::runtime::{ground_under_contrast, theme_under_contrast};
    let night_sky = bt_render::SystemContrast {
        window: [0x00, 0x00, 0x00],
        window_text: [0xff, 0xff, 0xff],
        highlight: [0xd6, 0xb4, 0xfd],
        highlight_text: [0x2b, 0x2b, 0x2b],
        gray_text: [0xa6, 0xa6, 0xa6],
    };
    let desert = bt_render::SystemContrast {
        window: [0xff, 0xfa, 0xef],
        window_text: [0x3d, 0x3d, 0x3d],
        highlight: [0x90, 0x39, 0x09],
        highlight_text: [0xff, 0xf5, 0xe3],
        gray_text: [0x67, 0x67, 0x67],
    };
    assert_eq!(
        theme_under_contrast(Theme::Light, Some(night_sky)),
        Theme::Dark
    );
    assert_eq!(
        theme_under_contrast(Theme::Dark, Some(desert)),
        Theme::Light
    );
    assert_eq!(theme_under_contrast(Theme::Light, None), Theme::Light);
    assert_eq!(theme_under_contrast(Theme::Dark, None), Theme::Dark);

    let picture = std::sync::Arc::new(bt_render::BackgroundImage {
        key: "wallpaper".to_owned(),
        rgba: std::sync::Arc::from(vec![0_u8; 4]),
        width_px: 1,
        height_px: 1,
    });
    let stored = bt_render::WindowGround {
        image: Some(picture),
        fit: bt_render::BackgroundFit::Tile,
        image_opacity: 0.4,
        alpha: 0.6,
    };
    let under = ground_under_contrast(stored.clone(), true);
    assert_eq!(
        under.image, None,
        "no picture under a contrast theme's text"
    );
    assert_eq!(under.alpha, 1.0, "and nothing showing through it");
    assert_eq!(
        under.fit, stored.fit,
        "the rest is the file's, for when it goes off"
    );
    assert_eq!(under.image_opacity, stored.image_opacity);
    assert_eq!(ground_under_contrast(stored.clone(), false), stored);
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

/// The disk a stop keeps the owed documents on, as a test scripts it: the copy lands in
/// the folder (the machine's own `preview::keep_in_recovered`) or is refused, and every
/// line offered to `diagnostics.log` is recorded — and, refused, lost.
struct ScriptedStopDisk {
    refuse: bool,
    offered: Vec<String>,
    kept_lines: Vec<String>,
}

impl ScriptedStopDisk {
    fn new(refuse: bool) -> Self {
        Self {
            refuse,
            offered: Vec::new(),
            kept_lines: Vec::new(),
        }
    }
}

impl StopDisk for ScriptedStopDisk {
    fn keep(
        &mut self,
        folder: &Path,
        name: &str,
        bytes: &[u8],
        at: SystemTime,
    ) -> Result<PathBuf, String> {
        if self.refuse {
            return Err("拒绝 the volume refused the copy".to_owned());
        }
        preview::keep_in_recovered(folder, name, bytes, at)
    }

    fn note(&mut self, line: &str) {
        self.offered.push(line.to_owned());
        if !self.refuse {
            self.kept_lines.push(line.to_owned());
        }
    }
}

/// The native channel, as a test hears it: every text it was asked to say. No box opens.
#[derive(Default)]
struct HeardNatively(Vec<String>);

impl NativeSay for HeardNatively {
    fn say(&mut self, text: &str) {
        self.0.push(text.to_owned());
    }
}

/// The system's box, as a test is the platform: it answers whether a box would be seen as it
/// was told to, and keeps every box raised and every `stderr` line handed over. No box opens.
struct ToldBoxes {
    seen: bool,
    raised: Vec<String>,
    lines: Vec<String>,
}

impl ToldBoxes {
    fn answering(seen: bool) -> Self {
        Self {
            seen,
            raised: Vec::new(),
            lines: Vec::new(),
        }
    }
}

impl SystemBoxes for ToldBoxes {
    type Seen = ();

    fn would_be_seen(&mut self) -> Option<()> {
        self.seen.then_some(())
    }

    fn raise(&mut self, (): (), text: &str) {
        self.raised.push(text.to_owned());
    }

    fn line(&mut self, text: String) {
        self.lines.push(text);
    }
}

/// RED (T-STOP-NOTICE-BOX-QUERY-MACOS, 0.4.9 plan row 66) — **a stop's lost-change notice that
/// no box would show is one queued `stderr` line, and no box is called**: the two lines the
/// box's synchronous fallback wrote (the name, the notice), handed to the run's `stderr` road
/// as one entry.
///
/// MUTATION: the unseen arm drops the line (`None => {}`) — no line is handed over. (Raising
/// the box on `None` cannot be written: there is no `Seen` value to raise it with.)
#[test]
fn a_stop_notice_no_box_would_show_is_a_queued_stderr_line() {
    let notice = i18n::stopped_changes_lost(&[persist::SETTINGS_FILE_NAME]);
    let mut boxes = ToldBoxes::answering(false);
    say_in_a_box_or_on_stderr(&mut boxes, &notice);
    assert_eq!(boxes.raised, Vec::<String>::new(), "no box is called");
    assert_eq!(boxes.lines, [format!("{APP_NAME}\n{notice}")]);
}

/// RED (T-STOP-NOTICE-BOX-QUERY-MACOS) — **a stop's notice a box would show is raised in the box,
/// and nothing goes to `stderr`.**
///
/// MUTATION: hand the notice to the `stderr` road whatever the answer — no box is raised.
#[test]
fn a_stop_notice_a_box_would_show_is_raised_and_not_queued() {
    let notice = i18n::stopped_changes_lost(&[persist::SETTINGS_FILE_NAME]);
    let mut boxes = ToldBoxes::answering(true);
    say_in_a_box_or_on_stderr(&mut boxes, &notice);
    assert_eq!(boxes.raised, [notice]);
    assert_eq!(boxes.lines, Vec::<String>::new(), "nothing is queued");
}

/// RED (T-FRAME-SHAPE-STOP-BOX-QUERY, 0.4.9 plan row 79) — **a frame-shape stop's alert that no
/// box would show is one queued `stderr` line, and no box is called**: the name and the alert,
/// handed to the run's `stderr` road as one entry, where the panic hook's box fell back to two
/// lines written synchronously on the window thread.
///
/// MUTATION: the owed arm of `say_the_stop_alert` drops the alert (`if let … {}`) — no line.
/// (`FolioApp::fail` handing the old `announce_panic` back its closure is the static walk's
/// RED: `every_request_door_returns_at_once` against the committed `REQUEST-DOOR-SITES.tsv`.)
#[test]
fn a_frame_shape_stop_no_box_would_show_queues_its_alert_on_stderr() {
    let alert = panic_alert_text(&std::env::temp_dir().join(PANIC_LOG_FILENAME));
    let mut boxes = ToldBoxes::answering(false);
    say_the_stop_alert(Some(PanicAlert::Owed(alert.clone())), &mut boxes);
    assert_eq!(boxes.raised, Vec::<String>::new(), "no box is called");
    assert_eq!(boxes.lines, [format!("{APP_NAME}\n{alert}")]);
}

/// RED (T-FRAME-SHAPE-STOP-BOX-QUERY) — **a frame-shape stop's alert a box would show is raised
/// in the box, and nothing goes to `stderr`.**
///
/// MUTATION: the owed arm hands the alert to `boxes.line` whatever the answer — no box.
#[test]
fn a_frame_shape_stop_a_box_would_show_raises_its_alert() {
    let alert = panic_alert_text(&std::env::temp_dir().join(PANIC_LOG_FILENAME));
    let mut boxes = ToldBoxes::answering(true);
    say_the_stop_alert(Some(PanicAlert::Owed(alert.clone())), &mut boxes);
    assert_eq!(boxes.raised, [alert]);
    assert_eq!(boxes.lines, Vec::<String>::new(), "nothing is queued");
}

/// RED (T-FRAME-SHAPE-STOP-BOX-QUERY) — **the one alert stays one**: a stop whose alert was
/// already written to a watching console, or whose claim another caller (a panic) already
/// made, says nothing more — neither a box nor a line.
///
/// MUTATION: `say_the_stop_alert` says something for every claim (`Some(_)` / `None` →
/// `say_in_a_box_or_on_stderr(boxes, "")`) — a box or a line appears.
#[test]
fn a_frame_shape_stop_whose_alert_was_said_says_nothing_more() {
    let claims: [fn() -> Option<PanicAlert>; 2] = [|| Some(PanicAlert::Spoken), || None];
    for claim in claims {
        for seen in [false, true] {
            let mut boxes = ToldBoxes::answering(seen);
            say_the_stop_alert(claim(), &mut boxes);
            assert_eq!(boxes.raised, Vec::<String>::new(), "no box (seen: {seen})");
            assert_eq!(boxes.lines, Vec::<String>::new(), "no line (seen: {seen})");
        }
    }
}

/// A folder for one test, and a settings store in it owing a change of the terminal face
/// whose write is on a worker that never answers — the stop comes before it does.
fn a_stop_with_an_owed_settings_change(
    case: &str,
) -> (PathBuf, persist::SettingsStore, persist::UnservedJobs) {
    let root = bt_testpath::temp_path(&format!("bt-app-forced-stop-{case}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a private storage folder for this test");
    let (lane, jobs) = persist::StorageLane::unserved();
    let mut settings = persist::SettingsStore::at_on(root.join(persist::SETTINGS_FILE_NAME), lane);
    assert!(settings.store(bt_persist::SettingsV1 {
        terminal_font_family: "停止 Stopped Mono".to_owned(),
        ..bt_persist::SettingsV1::default()
    }));
    (root, settings, jobs)
}

/// RED (T-QUIT-DOCUMENTS, revision (c)2) — **a forced stop keeps an owed `settings.json`
/// change in `recovered`**, beside the preview edits the same stop keeps: the copy holds
/// the change, landed create-new under the folder the next start announces, and
/// `diagnostics.log` is offered where it is.
///
/// MUTATION: skip the stores in the forced stop's recovery step (in
/// `keep_owed_documents_over`, keep no document) — nothing is in the folder.
#[test]
fn a_forced_stop_keeps_an_owed_settings_change_in_recovered() {
    let (root, mut settings, _jobs) = a_stop_with_an_owed_settings_change("kept");
    let folder = root.join(preview::RECOVERED_FOLDER);
    let owed = document_request::QuitDocument::owed_bytes(&settings)
        .expect("the change is owed")
        .expect("and serializes");
    let mut disk = ScriptedStopDisk::new(false);
    let mut heard = HeardNatively::default();
    let kept = keep_owed_documents_over(
        &mut [&mut settings],
        &folder,
        SystemTime::UNIX_EPOCH,
        &mut disk,
        &mut heard,
    );
    let [DocumentKept::Kept { file, copy }] = kept.as_slice() else {
        panic!("one store, kept: {kept:?}");
    };
    assert_eq!(*file, persist::SETTINGS_FILE_NAME);
    assert_eq!(
        copy.parent(),
        Some(folder.as_path()),
        "in the recovered folder"
    );
    assert_eq!(std::fs::read(copy).expect("the copy is on disk"), owed);
    assert!(
        disk.kept_lines.iter().any(
            |line| line.contains("settings.json") && line.contains(&copy.display().to_string())
        ),
        "diagnostics.log names the file and the copy: {:?}",
        disk.kept_lines
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, revision (d)3) — **a copy the disk refused is `Lost`, and said
/// through the native channel**: the disk refuses the copy and the line in
/// `diagnostics.log` alike, and the system's own box is asked once, naming the file.
///
/// MUTATION: report `Lost` to `diagnostics.log` only (the preview's behaviour today: drop
/// the `say` in `keep_owed_documents_over`) — the loss is said nowhere that survives.
#[test]
fn a_refused_recovery_copy_is_lost_and_said_natively() {
    let (root, mut settings, _jobs) = a_stop_with_an_owed_settings_change("lost");
    let mut disk = ScriptedStopDisk::new(true);
    let mut heard = HeardNatively::default();
    let kept = keep_owed_documents_over(
        &mut [&mut settings],
        &root.join(preview::RECOVERED_FOLDER),
        SystemTime::UNIX_EPOCH,
        &mut disk,
        &mut heard,
    );
    assert!(
        matches!(kept.as_slice(), [DocumentKept::Lost { file, .. }] if *file == persist::SETTINGS_FILE_NAME),
        "the store answers Lost: {kept:?}"
    );
    assert_eq!(
        disk.offered.len(),
        1,
        "diagnostics.log was still offered the line"
    );
    assert!(disk.kept_lines.is_empty(), "and refused it");
    assert_eq!(
        heard.0,
        vec![i18n::stopped_changes_lost(&[persist::SETTINGS_FILE_NAME])],
        "asked once, naming settings.json"
    );
    assert_eq!(
        i18n::stopped_changes_lost_in(i18n::Lang::English, &[persist::SETTINGS_FILE_NAME]),
        "Folio stopped. A change to settings.json could not be kept."
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, revision (d)3) — **several lost stores are one native message**,
/// naming every file.
///
/// MUTATION: one box per store (in `keep_owed_documents_over`, `say` each `Lost` as it is
/// answered) — two boxes.
#[test]
fn several_lost_stores_are_one_native_message() {
    let (root, mut settings, _jobs) = a_stop_with_an_owed_settings_change("several");
    let (lane, _pins_jobs) = persist::StorageLane::unserved();
    let mut pins = pins::PinsStore::at_on(root.join(pins::PINS_FILE_NAME), lane);
    assert!(pins.toggle(bt_persist::PinKind::Folder, "项目/folio 笔记"));
    let mut disk = ScriptedStopDisk::new(true);
    let mut heard = HeardNatively::default();
    let kept = keep_owed_documents_over(
        &mut [&mut settings, &mut pins],
        &root.join(preview::RECOVERED_FOLDER),
        SystemTime::UNIX_EPOCH,
        &mut disk,
        &mut heard,
    );
    assert_eq!(kept.len(), 2, "both lost: {kept:?}");
    assert_eq!(
        heard.0,
        vec![i18n::stopped_changes_lost(&[
            persist::SETTINGS_FILE_NAME,
            pins::PINS_FILE_NAME
        ])],
        "one message for both"
    );
    assert_eq!(
        i18n::stopped_changes_lost_in(
            i18n::Lang::English,
            &[persist::SETTINGS_FILE_NAME, pins::PINS_FILE_NAME]
        ),
        "Folio stopped. Changes to settings.json and pins.json could not be kept."
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, revision (d)3) — **a copy that landed raises no native message**:
/// the next start says where it is, as it says the preview's.
///
/// MUTATION: say every outcome natively (in `keep_owed_documents_over`, `say` whenever any
/// store was kept or lost) — a box for a change that is safe.
#[test]
fn a_landed_recovery_copy_raises_no_native_message() {
    let (root, mut settings, _jobs) = a_stop_with_an_owed_settings_change("landed");
    let mut disk = ScriptedStopDisk::new(false);
    let mut heard = HeardNatively::default();
    let kept = keep_owed_documents_over(
        &mut [&mut settings],
        &root.join(preview::RECOVERED_FOLDER),
        SystemTime::UNIX_EPOCH,
        &mut disk,
        &mut heard,
    );
    assert!(
        matches!(kept.as_slice(), [DocumentKept::Kept { .. }]),
        "{kept:?}"
    );
    assert!(heard.0.is_empty(), "no box: {:?}", heard.0);
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS round 4, item 3) — **a forced stop during the documents phase
/// disposes of the held trial release.** `take_released` is one-shot: once a quit holds the
/// release, the stop is its only owner. The document writers are moot (the stores' step kept
/// their changes in `recovered`), the toast identity is window-thread work the process is not
/// staying for, and the session's half arms this run's sentinel, so the next start is told
/// truthfully whether the stop's session write landed. Nothing stays held.
///
/// MUTATION: finish without disposing (`dispose_of_a_held_release_on_a_stop` does nothing) —
/// no sentinel is armed.
#[test]
fn a_forced_stop_during_the_documents_phase_disposes_of_the_held_release() {
    use crate::update_trial::Writer;
    let root = bt_testpath::temp_path("bt-app-forced-stop-held-release");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a private storage folder for this test");
    let mut session =
        persist::SessionStore::at(root.join("session.json"), root.join("session.lock"));
    let mut quit = quit::Quit::begin(Vec::new());
    assert_eq!(
        quit.documents(
            &[(persist::SETTINGS_FILE_NAME, quit::Standing::Waiting)],
            Instant::now()
        ),
        quit::QuitStep::Documents
    );
    assert!(
        quit.trial_release(vec![
            Writer::Session,
            Writer::Settings,
            Writer::ToastIdentity
        ])
        .is_empty(),
        "held while the phase holds the worker"
    );
    assert!(!session.sentinel_is_armed());
    dispose_of_a_held_release_on_a_stop(Some(&mut quit), &mut session);
    assert!(
        session.sentinel_is_armed(),
        "the session's half arms the sentinel"
    );
    assert!(!quit.release_on_a_stop(), "and nothing is held any more");
    let _ = std::fs::remove_dir_all(&root);
}
