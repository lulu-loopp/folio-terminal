//! **The glance card's foot, driven** (T-HEADLESS-RUNTIME-SEAM) on a headless window, **and the
//! peek cache's let-go and the card's opening and teardown at their `Runtime` sites**
//! (T-PEEK-COMPLETIONS-RUNTIME-PINS).

use crate::headless::{HeadlessFolio, WindowEffect};
use crate::test_support::{cross_tab, glance_fixture, tab_with_a_picture};
use crate::{FilePeek, PeekClock, RowHost};
use bt_layout::SeatId;
use std::time::Instant;
use winit::keyboard::ModifiersState;

/// A card standing up over `file`, raised from a row of `host`.
fn card_over(host: RowHost, file: &std::path::Path) -> FilePeek {
    FilePeek {
        host,
        source: crate::preview::PreviewSource::file(file.display().to_string()),
        key: file.display().to_string(),
        name: "plan.md".to_owned(),
        rect: [10.0, 10.0, 200.0, 30.0],
        clock: PeekClock::Shown(Instant::now()),
        frame: None,
        body: None,
        head: None,
        foot: None,
        foot_lit: false,
        column: None,
        closing_at: None,
        dwell: None,
    }
}

/// RED (ticket 12, user ruling 2026-09-20) — **`Ctrl` (`⌘`) and a press on the card's foot take
/// the card down and reveal the file through the door the card's surface uses for its own
/// hand-over**: a files row, a floating tree's row and a Git row through the file manager's
/// asking door, a reference a program printed through the verified one, off its pane's ledger.
///
/// Without the chord the same press hands the system nothing: the card's foot locates the file
/// in this window's files column instead (`peek_foot_press`'s `Here`).
///
/// MUTATIONS: drop the `PeekFootPress::Reveal` arm's `self.reveal_in_explorer(&path)?` from
/// `Runtime::press_file_peek_foot` — the foot reveals nothing; read every press as the chord
/// (`hand_over = true`) — a plain press hands the file to the system.
#[test]
fn a_ctrl_press_on_the_foot_reveals_the_file_through_its_surfaces_door() {
    let (folder, file) = glance_fixture("headless-reveal");
    let held = [ModifiersState::CONTROL, ModifiersState::SUPER]
        .into_iter()
        .find(|modifiers| crate::input::pointer_chord_held(*modifiers))
        .expect("one of the two is the hand-over chord here");
    for (host, terminal) in [
        (RowHost::Column(SeatId(1)), false),
        (RowHost::Float(7), false),
        (RowHost::Git(SeatId(2)), false),
        (RowHost::Terminal(SeatId(2)), true),
    ] {
        // Without the chord: nothing is handed to the system.
        let mut folio =
            HeadlessFolio::with_tabs("headless-reveal", vec![cross_tab(1, &["一", "二"])], 0);
        let mut runtime = folio.runtime();
        runtime.window.file_peek = Some(card_over(host, &file));
        runtime.window.modifiers_held = ModifiersState::empty();
        let _ = runtime
            .press_file_peek_foot()
            .expect("the plain press is answered");
        assert!(
            !folio
                .ledger
                .take()
                .iter()
                .any(|effect| matches!(effect, WindowEffect::HandedOff(_))),
            "{host:?}: a press without the chord hands the system nothing"
        );

        let mut folio =
            HeadlessFolio::with_tabs("headless-reveal", vec![cross_tab(1, &["一", "二"])], 0);
        let mut runtime = folio.runtime();
        runtime.window.file_peek = Some(card_over(host, &file));
        runtime.window.modifiers_held = held;
        let expected = if terminal {
            let facts = runtime.verified_target(SeatId(2), &file);
            bt_platform::Handoff::RevealVerified(file.clone(), facts)
        } else {
            bt_platform::Handoff::Reveal(file.clone())
        };
        assert!(
            runtime
                .press_file_peek_foot()
                .expect("the press is answered"),
            "{host:?}: the foot takes the press"
        );
        assert!(
            runtime.window.file_peek.is_none(),
            "{host:?}: the card comes down"
        );
        let handed: Vec<_> = folio
            .ledger
            .take()
            .into_iter()
            .filter_map(|effect| match effect {
                WindowEffect::HandedOff(handoff) => Some(handoff),
                _ => None,
            })
            .collect();
        assert_eq!(handed, [expected], "{host:?}: the file is revealed");
    }
    let _ = std::fs::remove_dir_all(&folder);
}

// ── the let-go's and the card's runtime sites (T-PEEK-COMPLETIONS-RUNTIME-PINS) ──────────────

/// **A window holding `tabs`, its computation lane the suite's scripted executor**, so a read the
/// window asks goes to a worker the test owns, and the window's cache starts empty of anything
/// the product's own lane was asked while the window took its layout.
fn on_a_scripted_lane(tag: &str, tabs: Vec<crate::TabState>) -> HeadlessFolio {
    let mut folio = HeadlessFolio::with_tabs(tag, tabs, 0);
    let lane = crate::computation_lane::fixtures::watched(crate::lane::Executor::scripted(
        std::sync::Arc::new(crate::lane::Gate::default()),
    ));
    folio.app.math_worker = std::sync::Arc::clone(&lane.lane);
    folio.window.peek_cache = crate::PeekCache::with_budget(crate::MAX_PEEK_CACHE_BYTES);
    folio
}

/// A picture file's place on this machine, named for this test; nothing is written there.
fn a_picture_named(tag: &str) -> std::path::PathBuf {
    bt_testpath::temp_path(tag).with_extension("png")
}

/// A picture file on this machine, named for this test and written: a card over a file the disk
/// no longer has says so and reads nothing.
fn a_picture_written(tag: &str) -> std::path::PathBuf {
    let file = crate::test_support::scratch_png_named(tag);
    std::fs::write(&file, crate::test_support::png_bytes(4, 2))
        .expect("a scratch file this test owns");
    file
}

/// The failure a decode's error answer files for a file.
fn a_failure() -> crate::PeekCacheEntry {
    crate::PeekCacheEntry::Failed(bt_term::InlineImageDecodeError::Io(
        "系统找不到指定的文件 not found".to_owned(),
    ))
}

/// What the window's cache holds for `file`: `"failed"`, `"pending"`, `"ready"` or `"nothing"`.
fn held_for(folio: &HeadlessFolio, file: &std::path::Path) -> &'static str {
    match folio
        .window
        .peek_cache
        .peek(&bt_term::normalized_local_image_path_key(file))
    {
        Some(crate::PeekCacheEntry::Failed(_)) => "failed",
        Some(crate::PeekCacheEntry::Pending { .. }) => "pending",
        Some(crate::PeekCacheEntry::Ready { .. }) => "ready",
        None => "nothing",
    }
}

/// The hover flyout's subject: `file`, raised over the shell at `seat`.
fn a_hover_over(file: &std::path::Path, seat: SeatId) -> crate::PeekCandidate {
    crate::PeekCandidate {
        subject: crate::PeekSubject::from_path(file.to_path_buf()),
        seat,
        pointer: winit::dpi::PhysicalPosition::new(40.0, 40.0),
    }
}

/// RED (T-PEEK-COMPLETIONS-RUNTIME-PINS; T-PEEK-CACHE-QUESTION-RECORDS) — **the let-go runs at
/// its three `Runtime` sites**: at the end of the walk of the surfaces
/// (`refresh_preview_for_layout`), when a new hover peek settles — before it files its own read —
/// and when a flyout that was up is dismissed. At each, a failure no surface names any more is
/// let go of, and one a surface still names stays.
///
/// MUTATION: remove any one of the three calls of `Runtime::let_go_unnamed_peek_records` (the
/// walk's, `show_or_request_peek`'s on a settled hover, `dismiss_peek`'s) — that site's unnamed
/// failure stays.
#[test]
fn the_let_go_runs_at_the_three_runtime_sites() {
    let tab = || cross_tab(1, &["一 one"]);
    let shell = tab().seats.terminals()[0];

    // The walk's end: the flyout still names one failed file, nothing names the other.
    let mut folio = on_a_scripted_lane("peek-let-go-walk", vec![tab()]);
    let unnamed = a_picture_named("没人看 unnamed");
    let shown = a_picture_named("还在看 shown");
    for file in [&unnamed, &shown] {
        folio
            .window
            .peek_cache
            .insert(bt_term::normalized_local_image_path_key(file), a_failure());
    }
    folio.window.peek_hover.active = Some(a_hover_over(&shown, shell));
    folio.runtime().refresh_preview_for_layout();
    assert_eq!(
        held_for(&folio, &unnamed),
        "nothing",
        "the walk lets go of a failure no surface names"
    );
    assert_eq!(
        held_for(&folio, &shown),
        "failed",
        "and keeps the one the flyout names"
    );

    // A new hover settling: the failure no surface names goes before the new peek files its read.
    let mut folio = on_a_scripted_lane("peek-let-go-hover", vec![tab()]);
    let left = a_picture_named("刚离开 left");
    let next = a_picture_named("下一张 next");
    folio
        .window
        .peek_cache
        .insert(bt_term::normalized_local_image_path_key(&left), a_failure());
    let now = Instant::now();
    folio.window.peek_hover.candidate = Some(a_hover_over(&next, shell));
    folio.window.peek_hover.show_at = Some(now);
    folio
        .runtime()
        .activate_peek_if_due(now)
        .expect("the settled hover is answered");
    assert_eq!(
        held_for(&folio, &left),
        "nothing",
        "a new peek lets go of the failure no surface names"
    );
    assert_eq!(held_for(&folio, &next), "pending", "and reads its own file");

    // A flyout that was up, dismissed: its failed file was named by it alone.
    let mut folio = on_a_scripted_lane("peek-let-go-dismiss", vec![tab()]);
    let flown = a_picture_named("浮着 flown");
    folio.window.peek_cache.insert(
        bt_term::normalized_local_image_path_key(&flown),
        a_failure(),
    );
    folio.window.peek_hover.active = Some(a_hover_over(&flown, shell));
    folio
        .runtime()
        .dismiss_peek()
        .expect("the dismiss is answered");
    assert_eq!(
        held_for(&folio, &flown),
        "nothing",
        "the flyout coming down lets go of the failure it alone named"
    );
}

/// A glance card over `file`, raised from a row of the shell at `shell`, its intent matured at
/// `due`.
fn a_card_settling(shell: SeatId, file: &std::path::Path, due: Instant) -> FilePeek {
    FilePeek {
        clock: PeekClock::Settling(due),
        name: file
            .file_name()
            .expect("the picture has a name")
            .to_string_lossy()
            .into_owned(),
        ..card_over(RowHost::Column(shell), file)
    }
}

/// RED (T-PEEK-COMPLETIONS-RUNTIME-PINS; T-PEEK-CACHE-QUESTION-RECORDS round 2) — **a card's
/// opening reads its file again and its teardown lets go through the `Runtime`**: a card that
/// matures (`mature_file_peek`) over a file whose read failed lets the failure go, and its body
/// asks for the file again; a card coming down (`hide_file_peek`) lets go of the failure it alone
/// named, and leaves the one a picture pane still shows.
///
/// MUTATIONS: drop `crate::peek_card_opened` from `mature_file_peek` — the failure stands and the
/// card asks nothing; drop `self.let_go_unnamed_peek_records()` from `hide_file_peek` — the
/// failure the card alone named stays.
#[test]
fn a_cards_open_rereads_and_its_teardown_lets_go_through_the_runtime() {
    let tab = || cross_tab(1, &["一 one"]);
    let shell = tab().seats.terminals()[0];
    let file = a_picture_written("卡片 card");
    let key = bt_term::normalized_local_image_path_key(&file);

    // The opening.
    let mut folio = on_a_scripted_lane("peek-card-open", vec![tab()]);
    folio.window.peek_cache.insert(key.clone(), a_failure());
    let now = Instant::now();
    folio.window.file_peek = Some(a_card_settling(shell, &file, now));
    let mut runtime = folio.runtime();
    assert!(runtime.mature_file_peek(now), "the card matures");
    assert_ne!(
        held_for(&folio, &file),
        "failed",
        "a card opening over the file lets its failure go"
    );
    let _ = folio.runtime().refresh_overlay();
    assert_eq!(
        held_for(&folio, &file),
        "pending",
        "and the card's body reads the file again"
    );

    // The teardown, with nothing else naming the file.
    let mut folio = on_a_scripted_lane("peek-card-down", vec![tab()]);
    folio.window.peek_cache.insert(key.clone(), a_failure());
    folio.window.file_peek = Some(card_over(RowHost::Terminal(shell), &file));
    assert!(
        folio.runtime().hide_file_peek(),
        "the card that was up comes down"
    );
    assert_eq!(
        held_for(&folio, &file),
        "nothing",
        "the failure the card alone named goes with it"
    );

    // The teardown, with a picture pane still showing the file.
    let (pane_tab, _) = tab_with_a_picture(1, &file.display().to_string());
    let mut folio = on_a_scripted_lane("peek-card-down-pane", vec![pane_tab]);
    folio.window.peek_cache.insert(key.clone(), a_failure());
    folio.window.file_peek = Some(card_over(RowHost::Terminal(shell), &file));
    assert!(
        folio.runtime().hide_file_peek(),
        "the card that was up comes down"
    );
    assert_eq!(
        held_for(&folio, &file),
        "failed",
        "the failure a pane still shows stays"
    );
}
