//! **The glance card's foot, driven** (T-HEADLESS-RUNTIME-SEAM) on a headless window.

use crate::headless::{HeadlessFolio, WindowEffect};
use crate::test_support::{cross_tab, glance_fixture};
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
/// MUTATION: drop the `PeekFootPress::Reveal` arm's `self.reveal_in_explorer(&path)?` from
/// `Runtime::press_file_peek_foot` — the foot reveals nothing.
#[test]
fn a_ctrl_press_on_the_foot_reveals_the_file_through_its_surfaces_door() {
    let (folder, file) = glance_fixture("headless-reveal");
    let held = [ModifiersState::CONTROL, ModifiersState::SUPER]
        .into_iter()
        .find(|modifiers| crate::input::pointer_chord_held(*modifiers))
        .expect("one of the two is the hand-over chord here");
    for (host, terminal) in [
        (RowHost::Column(SeatId(1)), false),
        (RowHost::Git(SeatId(2)), false),
        (RowHost::Terminal(SeatId(2)), true),
    ] {
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
